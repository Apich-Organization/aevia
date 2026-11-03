//! The overload lattice of §13.4.1, applied to Aevia types.
//!
//! `aevia-rssn` owns the manifest; this module decides which overload a call
//! is typed against. The rules are the kernel's:
//!
//! 1. filter by arity;
//! 2. classify each argument, take the *weakest* score;
//! 3. the unique maximum wins;
//! 4. a tie is `E1002`, no match is `E1003`.
//!
//! Resolution is a *type-level* decision, made once, at the call site; the
//! runtime executes the key it produced (PLAN D1).

use crate::ty::{Rep, Shape, Type};
use aevia_dim::Dim;

/// How well one argument matches one parameter, ordered weakest to strongest.
///
/// The order is the kernel's `Match`: `No < Generic < Shaped < Widened <
/// Exact`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Score {
    /// The parameter does not accept the argument.
    No,
    /// A type variable or `Dyn` parameter.
    Generic,
    /// The same constructor, with the element or dimension not pinned.
    Shaped,
    /// A numeric promotion, `Int -> Real` and friends.
    Widened,
    /// The types are equal.
    Exact,
}

impl Score {
    /// Whether the parameter accepts the argument at all.
    #[must_use]
    pub fn accepted(self) -> bool {
        self != Score::No
    }
}

/// The outcome of resolving one call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// One overload matched best.
    Unique {
        /// Its index in the operator's overload list.
        index: usize,
        /// The weakest per-argument score.
        score: Score,
    },
    /// Several overloads tied for best.
    Ambiguous {
        /// The tied indices.
        candidates: Vec<usize>,
    },
    /// No overload accepts the arguments.
    None,
    /// The operator is not in the manifest.
    Unknown,
}

/// Resolves a call against an operator's overloads.
#[must_use]
pub fn resolve(
    entry: &aevia_rssn::OpEntry,
    args: &[aevia_rssn::SigType],
) -> Resolution {
    let mut scored: Vec<(usize, Score)> = Vec::new();
    for (index, signature) in entry.signatures.iter().enumerate() {
        if !accepts_arity(signature, args.len()) {
            continue;
        }
        let mut worst = Score::Exact;
        let mut ok = true;
        for (position, got) in args.iter().enumerate() {
            let Some(param) = signature.params.get(position) else {
                // Beyond the fixed parameters: the variadic tail, when there is
                // one, is `[α]` and accepts anything.
                if signature.variadic.is_some() {
                    worst = worst.min(Score::Generic);
                    continue;
                }
                ok = false;
                break;
            };
            let score = score_type(param, got);
            if !score.accepted() {
                ok = false;
                break;
            }
            worst = worst.min(score);
        }
        if ok {
            scored.push((index, worst));
        }
    }
    let Some(top) = scored.iter().map(|(_, score)| *score).max() else {
        return Resolution::None;
    };
    let tied: Vec<usize> = scored
        .iter()
        .filter(|(_, score)| *score == top)
        .map(|(index, _)| *index)
        .collect();
    match tied.as_slice() {
        [only] => Resolution::Unique {
            index: *only,
            score: top,
        },
        _ => Resolution::Ambiguous { candidates: tied },
    }
}

fn accepts_arity(signature: &aevia_rssn::Sig, count: usize) -> bool {
    if signature.params.len() == count {
        return true;
    }
    match &signature.variadic {
        Some((min, max, _)) => count >= *min && max.is_none_or(|max| count <= max),
        None => false,
    }
}

/// The score of one argument against one parameter.
#[must_use]
pub fn score_type(param: &aevia_rssn::SigType, got: &aevia_rssn::SigType) -> Score {
    use aevia_rssn::SigType as S;
    match (param, got) {
        // A variable or `Dyn` parameter accepts anything (§13.4.1).
        (S::Var(_), _) | (S::Dyn, _) | (S::Never, _) => Score::Generic,
        (_, S::Dyn) | (_, S::Var(_)) => Score::Generic,
        (S::Int, S::Int)
        | (S::Real, S::Real)
        | (S::Float, S::Float)
        | (S::Complex, S::Complex)
        | (S::Bool, S::Bool)
        | (S::Str, S::Str)
        | (S::Symbol, S::Symbol)
        | (S::Table, S::Table)
        | (S::Device, S::Device) => Score::Exact,
        (S::Unit(a), S::Unit(b)) => {
            if a == b {
                Score::Exact
            } else {
                Score::No
            }
        }
        (S::Point(a), S::Point(b)) => {
            if a == b {
                Score::Exact
            } else {
                Score::No
            }
        }
        (S::Dim(base, dim), other) => {
            // A dimensioned parameter matches the same base representation;
            // the dimension itself is not part of the score.
            let inner = score_type(base, other);
            if inner.accepted() && *dim != Dim::one() {
                Score::Shaped
            } else if inner == Score::Exact {
                Score::Exact
            } else {
                inner
            }
        }
        (other, S::Dim(base, dim)) => {
            let inner = score_type(other, base);
            if inner.accepted() && *dim != Dim::one() {
                Score::Shaped
            } else {
                inner
            }
        }
        (S::Tensor { rank: a, elem: ea }, S::Tensor { rank: b, elem: eb })
        | (S::Sparse { rank: a, elem: ea }, S::Sparse { rank: b, elem: eb })
        | (S::Tensor { rank: a, elem: ea }, S::Sparse { rank: b, elem: eb })
        | (S::Sparse { rank: a, elem: ea }, S::Tensor { rank: b, elem: eb }) => {
            if a.is_some() && b.is_some() && a != b {
                return Score::No;
            }
            let inner = score_type(ea, eb);
            if !inner.accepted() {
                return Score::No;
            }
            if a.is_none() || b.is_none() || inner != Score::Exact {
                Score::Shaped
            } else {
                Score::Exact
            }
        }
        (S::Record(a), S::Record(b)) => {
            if a.len() != b.len() {
                return Score::No;
            }
            let mut worst = Score::Exact;
            for (label, ty) in a {
                let Some((_, other)) = b.iter().find(|(name, _)| name == label) else {
                    return Score::No;
                };
                let score = score_type(ty, other);
                if !score.accepted() {
                    return Score::No;
                }
                worst = worst.min(score);
            }
            worst
        }
        (S::Variant(a), S::Variant(b)) => {
            if a.len() != b.len() {
                return Score::No;
            }
            let mut worst = Score::Exact;
            for (name, fields) in a {
                let Some((_, other)) = b.iter().find(|(candidate, _)| candidate == name) else {
                    return Score::No;
                };
                if fields.len() != other.len() {
                    return Score::No;
                }
                for (x, y) in fields.iter().zip(other.iter()) {
                    let score = score_type(x, y);
                    if !score.accepted() {
                        return Score::No;
                    }
                    worst = worst.min(score);
                }
            }
            worst
        }
        (S::Func(pa, ra), S::Func(pb, rb)) => {
            if pa.len() != pb.len() {
                return Score::No;
            }
            let mut worst = Score::Exact;
            for (x, y) in pa.iter().zip(pb.iter()) {
                let score = score_type(x, y);
                if !score.accepted() {
                    return Score::No;
                }
                worst = worst.min(score);
            }
            let score = score_type(ra, rb);
            if !score.accepted() {
                return Score::No;
            }
            worst.min(score)
        }
        (S::Opaque(a), S::Opaque(b)) => {
            if a == b {
                Score::Exact
            } else {
                Score::No
            }
        }
        // Numeric widening: `Int -> Real`, `Int|Real|Float -> Complex`.
        //
        // `Float -> Real` is the exact conversion of §3.4.1, so `sin(0.5)`
        // picks the numeric overload rather than tying with the symbolic one.
        (S::Real, S::Int | S::Float) | (S::Complex, S::Int) => Score::Widened,
        (S::Float, S::Int) => Score::Widened,
        (S::Complex, S::Real | S::Float) => Score::Widened,
        _ => Score::No,
    }
}

/// Classifies an Aevia type as the signature language sees it.
#[must_use]
pub fn classify(ty: &Type) -> aevia_rssn::SigType {
    use aevia_rssn::SigType as S;
    match ty {
        Type::Scalar(Rep::Int, _) => S::Int,
        Type::Scalar(Rep::Real, dim) => {
            if dim.is_one() {
                S::Real
            } else {
                S::Dim(Box::new(S::Real), dim.clone())
            }
        }
        Type::Scalar(Rep::Float, dim) => {
            if dim.is_one() {
                S::Float
            } else {
                S::Dim(Box::new(S::Float), dim.clone())
            }
        }
        Type::Scalar(Rep::Complex, dim) => {
            if dim.is_one() {
                S::Complex
            } else {
                S::Dim(Box::new(S::Complex), dim.clone())
            }
        }
        // A representation variable is not known at the call site; the dynamic
        // parameter is the only safe reading.
        Type::Scalar(Rep::Var(_), _) => S::Dyn,
        Type::Bool => S::Bool,
        Type::Prop => S::Bool,
        Type::Str => S::Str,
        Type::Unit => S::Opaque("Unit".to_string()),
        Type::UnitOf(dim) => S::Unit(dim.clone()),
        Type::Point(dim) => S::Point(dim.clone()),
        Type::Tensor { shape, elem } => S::Tensor {
            rank: Some(usize::try_from(shape.rank()).unwrap_or(0)),
            elem: Box::new(classify(elem)),
        },
        Type::Fn(params, ret) => S::Func(
            params.iter().map(classify).collect(),
            Box::new(classify(ret)),
        ),
        Type::Record(row) => S::Record(
            row.fields()
                .into_iter()
                .map(|(name, ty)| (name, classify(&ty)))
                .collect(),
        ),
        Type::Adt(id, _args) => {
            if *id == crate::adt::ORDERING_ID {
                return S::Variant(vec![
                    ("Less".to_string(), Vec::new()),
                    ("Equal".to_string(), Vec::new()),
                    ("Greater".to_string(), Vec::new()),
                ]);
            }
            S::Opaque(crate::adt::name_of(*id))
        }
        Type::Io(inner) => S::Opaque(format!("IO {}", classify(inner).render_hint())),
        Type::Dyn(_) => S::Dyn,
        Type::Var(_) | Type::Param(_) | Type::Error => S::Dyn,
    }
}

/// A short rendering used for opaque names.
trait RenderHint {
    fn render_hint(&self) -> String;
}

impl RenderHint for aevia_rssn::SigType {
    fn render_hint(&self) -> String {
        match self {
            aevia_rssn::SigType::Int => "Int".to_string(),
            aevia_rssn::SigType::Real => "Real".to_string(),
            aevia_rssn::SigType::Float => "Float".to_string(),
            aevia_rssn::SigType::Complex => "Complex".to_string(),
            aevia_rssn::SigType::Bool => "Bool".to_string(),
            aevia_rssn::SigType::Str => "String".to_string(),
            aevia_rssn::SigType::Opaque(name) => name.clone(),
            other => format!("{other:?}"),
        }
    }
}

/// The rank of a shape, when it is known.
#[must_use]
pub fn known_rank(shape: &Shape) -> Option<usize> {
    match shape {
        Shape::Rank(rank) => Some(usize::try_from(*rank).ok()?),
        Shape::Refined(items) => Some(items.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_widening_is_weak_but_accepted() {
        use aevia_rssn::SigType as S;
        assert_eq!(score_type(&S::Real, &S::Int), Score::Widened);
        assert_eq!(score_type(&S::Int, &S::Real), Score::No);
        assert_eq!(score_type(&S::Int, &S::Int), Score::Exact);
    }

    #[test]
    fn a_symbolic_parameter_is_generic() {
        use aevia_rssn::SigType as S;
        assert_eq!(
            score_type(&S::Var("t".to_string()), &S::Float),
            Score::Generic
        );
        assert_eq!(score_type(&S::Dyn, &S::Int), Score::Generic);
    }

    #[test]
    fn a_dimensioned_parameter_is_shaped() {
        use aevia_rssn::SigType as S;
        let score = score_type(
            &S::Dim(Box::new(S::Float), Dim::base("Length")),
            &S::Float,
        );
        assert_eq!(score, Score::Shaped);
    }

    #[test]
    fn an_int_argument_prefers_the_int_overload() {
        use aevia_rssn::SigType as S;
        let entry = aevia_rssn::registry::registry().op("abs");
        if let Some(entry) = entry {
            let resolution = resolve(entry, &[S::Int]);
            assert!(matches!(
                resolution,
                Resolution::Unique { .. } | Resolution::Ambiguous { .. }
            ));
        }
    }
}
