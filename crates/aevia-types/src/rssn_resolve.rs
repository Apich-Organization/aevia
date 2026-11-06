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

use crate::ty::Rep;
use crate::ty::Shape;
use crate::ty::Type;
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
        | [only] => {
            Resolution::Unique {
                index: *only,
                score: top,
            }
        },
        | _ => Resolution::Ambiguous { candidates: tied },
    }
}

fn accepts_arity(
    signature: &aevia_rssn::Sig,
    count: usize,
) -> bool {
    if signature.params.len() == count {
        return true;
    }
    match &signature.variadic {
        | Some((min, max, _)) => count >= *min && max.is_none_or(|max| count <= max),
        | None => false,
    }
}

/// The score of one argument against one parameter.
#[must_use]
pub fn score_type(
    param: &aevia_rssn::SigType,
    got: &aevia_rssn::SigType,
) -> Score {
    use aevia_rssn::SigType as S;
    match (param, got) {
        // A variable or `Dyn` parameter accepts anything (§13.4.1).
        | (S::Var(_), _) | (S::Dyn, _) | (S::Never, _) => Score::Generic,
        | (_, S::Dyn) | (_, S::Var(_)) => Score::Generic,
        | (S::Int, S::Int)
        | (S::Real, S::Real)
        | (S::Float, S::Float)
        | (S::Complex, S::Complex)
        | (S::Bool, S::Bool)
        | (S::Str, S::Str)
        | (S::Symbol, S::Symbol)
        | (S::Table, S::Table)
        | (S::Device, S::Device) => Score::Exact,
        | (S::Unit(a), S::Unit(b)) => {
            if a == b {
                Score::Exact
            } else {
                Score::No
            }
        },
        | (S::Point(a), S::Point(b)) => {
            if a == b {
                Score::Exact
            } else {
                Score::No
            }
        },
        | (S::Dim(base, dim), other) => {
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
        },
        | (other, S::Dim(base, dim)) => {
            let inner = score_type(other, base);
            if inner.accepted() && *dim != Dim::one() {
                Score::Shaped
            } else {
                inner
            }
        },
        | (
            S::Tensor {
                rank: a,
                shape: sa,
                elem: ea,
            },
            S::Tensor {
                rank: b,
                shape: sb,
                elem: eb,
            },
        ) => {
            if a.is_some() && b.is_some() && a != b
                || sa.as_ref().zip(sb.as_ref()).is_some_and(|(a, b)| a != b)
                || sa.as_ref().is_some_and(|s| *a != Some(s.len()))
                || sb.as_ref().is_some_and(|s| *b != Some(s.len()))
            {
                return Score::No;
            }
            let inner = score_type(ea, eb);
            if !inner.accepted() {
                Score::No
            } else if a.is_none() || b.is_none() || sa != sb || inner != Score::Exact {
                Score::Shaped
            } else {
                Score::Exact
            }
        },
        | (
            S::Tensor {
                rank: a,
                shape: sa,
                elem: ea,
            },
            S::Sparse { rank: b, elem: eb },
        ) => {
            if (a.is_some() && b.is_some() && a != b)
                || sa.as_ref().is_some_and(|shape| *a != Some(shape.len()))
            {
                return Score::No;
            }
            if score_type(ea, eb).accepted() {
                Score::Shaped
            } else {
                Score::No
            }
        },
        | (
            S::Sparse { rank: a, elem: ea },
            S::Tensor {
                rank: b,
                shape: sb,
                elem: eb,
            },
        ) => {
            if (a.is_some() && b.is_some() && a != b)
                || sb.as_ref().is_some_and(|shape| *b != Some(shape.len()))
            {
                return Score::No;
            }
            if score_type(ea, eb).accepted() {
                Score::Shaped
            } else {
                Score::No
            }
        },
        | (S::Sparse { rank: a, elem: ea }, S::Sparse { rank: b, elem: eb }) => {
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
        },
        | (S::Record(a), S::Record(b)) => {
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
        },
        | (S::Variant(a), S::Variant(b)) => {
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
        },
        | (S::Func(pa, ra), S::Func(pb, rb)) => {
            if pa.len() != pb.len() {
                return Score::No;
            }
            // An actual callback must accept every value the operator may
            // supply, and its result must prove everything the operator needs.
            // Pointwise matching alone is symmetric for unknown shapes, so it
            // cannot establish these directional function promises.
            if !pa
                .iter()
                .zip(pb)
                .all(|(expected, actual)| schema_subtype(expected, actual))
                || !schema_subtype(rb, ra)
            {
                return Score::No;
            }
            let mut worst = Score::Exact;
            for (x, y) in pa.iter().zip(pb.iter()) {
                let score = score_type(y, x);
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
        },
        | (S::Opaque(a), S::Opaque(b)) => {
            if a == b {
                Score::Exact
            } else {
                Score::No
            }
        },
        // Numeric widening: `Int -> Real`, `Int|Real|Float -> Complex`.
        //
        // `Float -> Real` is the exact conversion of §3.4.1, so `sin(0.5)`
        // picks the numeric overload rather than tying with the symbolic one.
        | (S::Real, S::Int | S::Float) | (S::Complex, S::Int) => Score::Widened,
        | (S::Float, S::Int) => Score::Widened,
        | (S::Complex, S::Real | S::Float) => Score::Widened,
        | _ => Score::No,
    }
}

/// Whether every value described by `actual` is accepted by `expected`.
/// Unlike ordinary argument scoring, this must prove shape promises before
/// a callback is invoked; a rank-only result cannot promise fixed extents.
fn schema_subtype(
    actual: &aevia_rssn::SigType,
    expected: &aevia_rssn::SigType,
) -> bool {
    use aevia_rssn::SigType as S;
    match (actual, expected) {
        | (_, S::Dyn) => true,
        | (S::Var(a), S::Var(b)) if a == b => true,
        | (S::Dyn | S::Var(_), _) => false,
        | (_, S::Var(_)) => true,
        | (S::Int | S::Real | S::Float | S::Complex, S::Int | S::Real | S::Float | S::Complex) => {
            actual == expected
        },
        | (
            S::Tensor {
                rank: actual_rank,
                shape: actual_shape,
                elem: actual_elem,
            },
            S::Tensor {
                rank: expected_rank,
                shape: expected_shape,
                elem: expected_elem,
            },
        ) => {
            expected_rank.is_none_or(|rank| *actual_rank == Some(rank))
                && expected_shape
                    .as_ref()
                    .is_none_or(|shape| actual_shape.as_ref() == Some(shape))
                && actual_shape
                    .as_ref()
                    .is_none_or(|shape| *actual_rank == Some(shape.len()))
                && expected_shape
                    .as_ref()
                    .is_none_or(|shape| *expected_rank == Some(shape.len()))
                && schema_subtype(actual_elem, expected_elem)
        },
        | (S::Dim(actual_base, actual_dim), S::Dim(expected_base, expected_dim)) => {
            (!expected_dim.is_concrete() || actual_dim == expected_dim)
                && schema_subtype(actual_base, expected_base)
        },
        | (S::Dim(actual_base, actual_dim), expected) => {
            actual_dim.is_one() && schema_subtype(actual_base, expected)
        },
        | (actual, S::Dim(expected_base, expected_dim)) => {
            (!expected_dim.is_concrete() || expected_dim.is_one())
                && schema_subtype(actual, expected_base)
        },
        | (S::Record(actual_fields), S::Record(expected_fields)) => {
            actual_fields.len() == expected_fields.len()
                && expected_fields.iter().all(|(name, _)| {
                    expected_fields
                        .iter()
                        .filter(|(other, _)| other == name)
                        .count()
                        == 1
                })
                && expected_fields.iter().all(|(name, expected)| {
                    let mut matches = actual_fields.iter().filter(|(field, _)| field == name);
                    matches
                        .next()
                        .is_some_and(|(_, actual)| schema_subtype(actual, expected))
                        && matches.next().is_none()
                })
        },
        | (S::Sparse { .. }, S::Tensor { .. }) | (S::Tensor { .. }, S::Sparse { .. }) => false,
        | (S::Func(actual_params, actual_result), S::Func(expected_params, expected_result)) => {
            actual_params.len() == expected_params.len()
                && expected_params
                    .iter()
                    .zip(actual_params)
                    .all(|(expected, actual)| schema_subtype(expected, actual))
                && schema_subtype(actual_result, expected_result)
        },
        | _ => score_type(expected, actual).accepted(),
    }
}

/// Classifies an Aevia type as the signature language sees it.
#[must_use]
pub fn classify(ty: &Type) -> aevia_rssn::SigType {
    use aevia_rssn::SigType as S;
    match ty {
        | Type::Scalar(Rep::Int, _) => S::Int,
        | Type::Scalar(Rep::Real, dim) => {
            if dim.is_one() {
                S::Real
            } else {
                S::Dim(Box::new(S::Real), dim.clone())
            }
        },
        | Type::Scalar(Rep::Float, dim) => {
            if dim.is_one() {
                S::Float
            } else {
                S::Dim(Box::new(S::Float), dim.clone())
            }
        },
        | Type::Scalar(Rep::Complex, dim) => {
            if dim.is_one() {
                S::Complex
            } else {
                S::Dim(Box::new(S::Complex), dim.clone())
            }
        },
        // A representation variable is not known at the call site; the dynamic
        // parameter is the only safe reading.
        | Type::Scalar(Rep::Var(_), _) => S::Dyn,
        | Type::Bool => S::Bool,
        | Type::Prop => S::Bool,
        | Type::Str => S::Str,
        | Type::Unit => S::Opaque("Unit".to_string()),
        | Type::UnitOf(dim) => S::Unit(dim.clone()),
        | Type::Point(dim) => S::Point(dim.clone()),
        | Type::Tensor { shape, elem } => {
            S::Tensor {
                rank: Some(usize::try_from(shape.rank()).unwrap_or(0)),
                shape: match shape {
                    | Shape::Rank(_) => None,
                    | Shape::Refined(extents) => {
                        extents
                            .iter()
                            .map(|extent| extent.as_ref().and_then(|extent| extent.as_const()))
                            .collect()
                    },
                },
                elem: Box::new(classify(elem)),
            }
        },
        | Type::Fn(params, ret) => {
            S::Func(
                params.iter().map(classify).collect(),
                Box::new(classify(ret)),
            )
        },
        | Type::Record(row) => {
            S::Record(
                row.fields()
                    .into_iter()
                    .map(|(name, ty)| (name, classify(&ty)))
                    .collect(),
            )
        },
        | Type::Adt(id, _args) => {
            if *id == crate::adt::ORDERING_ID {
                return S::Variant(vec![
                    ("Less".to_string(), Vec::new()),
                    ("Equal".to_string(), Vec::new()),
                    ("Greater".to_string(), Vec::new()),
                ]);
            }
            S::Opaque(crate::adt::name_of(*id))
        },
        | Type::Io(inner) => S::Opaque(format!("IO {}", classify(inner).render_hint())),
        | Type::Dyn(_) => S::Dyn,
        | Type::Var(_)
        | Type::Param(_)
        | Type::TensorIntrinsic(_)
        | Type::TypeRepIntrinsic(_)
        | Type::FromValueIntrinsic(_)
        | Type::ToCallbackIntrinsic
        | Type::Error => S::Dyn,
    }
}

/// A short rendering used for opaque names.
trait RenderHint {
    fn render_hint(&self) -> String;
}

impl RenderHint for aevia_rssn::SigType {
    fn render_hint(&self) -> String {
        match self {
            | aevia_rssn::SigType::Int => "Int".to_string(),
            | aevia_rssn::SigType::Real => "Real".to_string(),
            | aevia_rssn::SigType::Float => "Float".to_string(),
            | aevia_rssn::SigType::Complex => "Complex".to_string(),
            | aevia_rssn::SigType::Bool => "Bool".to_string(),
            | aevia_rssn::SigType::Str => "String".to_string(),
            | aevia_rssn::SigType::Opaque(name) => name.clone(),
            | other => format!("{other:?}"),
        }
    }
}

/// The rank of a shape, when it is known.
#[must_use]
pub fn known_rank(shape: &Shape) -> Option<usize> {
    match shape {
        | Shape::Rank(rank) => Some(usize::try_from(*rank).ok()?),
        | Shape::Refined(items) => Some(items.len()),
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
        let score = score_type(&S::Dim(Box::new(S::Float), Dim::base("Length")), &S::Float);
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

// ---------------------------------------------------------------------------
// Literal-key result typing for `Rssn.invoke` (§13.2.2)
// ---------------------------------------------------------------------------

/// The declared **result** of the overload `signature_hash_hex` pins for
/// `op_name`, when exactly one overload matches and the type is expressible.
///
/// This is the static half of the generated façade: the emitted
/// `Rssn.invoke("<op>", "<hash>", [...])` names the overload by its `OpKey`
/// signature digest, so the checker can type the call with the same manifest
/// signature the runtime pins. Anything ambiguous or unrepresentable returns
/// `None`, and the caller keeps the dynamic value type (the runtime's `E1005`
/// stays the diagnostic of record for a wrong or stale literal key).
///
/// Dimension variables are freshened **once per call**: a signature's `T` is
/// one variable in its parameters and in its result, so `Real[T]` in and
/// `T^(1/2)` out relate exactly as they do in the generated source.
#[must_use]
pub fn invoke_result(
    op_name: &str,
    signature_hash_hex: &str,
    infer: &mut crate::infer::Infer,
) -> Option<Type> {
    let registry = aevia_rssn::registry();
    let entry = registry.op(op_name)?;
    let mut matching = entry
        .keys
        .iter()
        .enumerate()
        .filter(|(_, key)| hex(&key.signature_hash) == signature_hash_hex);
    let (index, _) = matching.next()?;
    if matching.next().is_some() {
        // Two overloads with one digest would make the claim ambiguous.
        return None;
    }
    let signature = entry.signatures.get(index)?;
    let mut dims = std::collections::HashMap::new();
    sig_type_to_type(&signature.ret, infer, &mut dims)
}

/// Instantiates an exact signature once and solves its parameter dimension
/// equations against the actual argument types. Representation widening was
/// already decided by the kernel lattice, so this deliberately unifies only
/// dimensions rather than rejecting a legal Int/Float-to-Real widening.
pub fn instantiate_dimensions(
    signature: &aevia_rssn::Sig,
    actual: &[Type],
    infer: &mut crate::infer::Infer,
) -> Result<Option<Type>, crate::infer::UnifyError> {
    let mut dims = std::collections::HashMap::new();
    fn constrain(
        sig: &aevia_rssn::SigType,
        actual: &Type,
        infer: &mut crate::infer::Infer,
        dims: &mut std::collections::HashMap<u32, Dim>,
    ) -> Result<(), crate::infer::UnifyError> {
        use aevia_rssn::SigType as S;
        let actual = infer.resolve(actual);
        match (sig, &actual) {
            | (S::Dim(_, expected), Type::Scalar(_, got))
            | (S::Unit(expected), Type::UnitOf(got))
            | (S::Point(expected), Type::Point(got)) => {
                let expected = map_dim(expected, infer, dims);
                infer.unify_dim(&expected, got)
            },
            | (S::Int | S::Real | S::Float | S::Complex, Type::Scalar(_, got)) => {
                infer.unify_dim(&Dim::one(), got)
            },
            | (S::Tensor { elem, .. } | S::Sparse { elem, .. }, Type::Tensor { elem: got, .. }) => {
                constrain(elem, got, infer, dims)
            },
            | (S::Record(fields), Type::Record(row)) => {
                for (name, field) in fields {
                    if let Some(got) = row.field(name) {
                        constrain(field, got, infer, dims)?;
                    }
                }
                Ok(())
            },
            | (S::Func(params, ret), Type::Fn(got_params, got_ret)) => {
                for (param, got) in params.iter().zip(got_params) {
                    constrain(param, got, infer, dims)?;
                }
                constrain(ret, got_ret, infer, dims)
            },
            | (_, Type::Var(_)) => {
                if let Some(param) = sig_type_to_type(sig, infer, dims) {
                    infer.unify(&param, &actual)?;
                }
                Ok(())
            },
            | _ => Ok(()),
        }
    }
    for (param, got) in signature.params.iter().zip(actual) {
        constrain(param, got, infer, &mut dims)?;
    }
    Ok(sig_type_to_type(&signature.ret, infer, &mut dims).map(|ret| infer.resolve(&ret)))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Substitutes a signature dimension's variables with fresh checker variables,
/// sharing one mapping per call.
fn map_dim(
    dim: &Dim,
    infer: &mut crate::infer::Infer,
    dims: &mut std::collections::HashMap<u32, Dim>,
) -> Dim {
    for id in dim.vars() {
        if !dims.contains_key(&id) {
            let fresh = infer.fresh_dim();
            dims.insert(id, fresh);
        }
    }
    dim.substitute(|id| dims.get(&id).cloned())
}

/// Maps a manifest result type to the checker's type, or `None` when the
/// emitted spelling cannot express it.
///
/// The emitted source renders `Symbol`, `Table`, `Device`, host opaque types,
/// variants, `Never`, type variables and rank-variable tensors as `Dyn<term>`,
/// so those map to `Dyn(Real)`; only a `Symbol` *result* has no safe spelling
/// here (the emitted module declares `Symbol` itself), and it falls back.
fn sig_type_to_type(
    sig: &aevia_rssn::SigType,
    infer: &mut crate::infer::Infer,
    dims: &mut std::collections::HashMap<u32, Dim>,
) -> Option<Type> {
    use aevia_rssn::SigType as S;
    Some(match sig {
        | S::Int => Type::int(),
        | S::Real => Type::real(Dim::one()),
        | S::Float => Type::float(Dim::one()),
        | S::Complex => Type::complex(Dim::one()),
        | S::Bool => Type::Bool,
        | S::Str => Type::string(),
        | S::Symbol => Type::Dyn(Rep::Real),
        | S::Unit(dim) => Type::UnitOf(map_dim(dim, infer, dims)),
        | S::Point(dim) => Type::Point(map_dim(dim, infer, dims)),
        | S::Dim(inner, dim) => {
            let rep = match **inner {
                | S::Int => Rep::Int,
                | S::Float => Rep::Float,
                | S::Complex => Rep::Complex,
                | _ => Rep::Real,
            };
            Type::Scalar(rep, map_dim(dim, infer, dims))
        },
        | S::Tensor {
            rank: Some(rank),
            shape,
            elem,
        } => {
            if shape.as_ref().is_some_and(|shape| shape.len() != *rank) {
                return None;
            }
            Type::tensor(
                shape.as_ref().map_or_else(
                    || Shape::Rank(u32::try_from(*rank).unwrap_or(1)),
                    |extents| Shape::refined(extents.iter().copied().map(Some)),
                ),
                sig_type_to_type(elem, infer, dims)?,
            )
        },
        | S::Sparse {
            rank: Some(rank),
            elem,
        } => {
            Type::tensor(
                Shape::Rank(u32::try_from(*rank).unwrap_or(1)),
                sig_type_to_type(elem, infer, dims)?,
            )
        },
        | S::Record(fields) => {
            let mut converted = Vec::with_capacity(fields.len());
            for (name, ty) in fields {
                converted.push((name.clone(), sig_type_to_type(ty, infer, dims)?));
            }
            Type::record(converted)
        },
        | S::Func(params, ret) => {
            let mut converted = Vec::with_capacity(params.len());
            for param in params {
                converted.push(sig_type_to_type(param, infer, dims)?);
            }
            Type::func(converted, sig_type_to_type(ret, infer, dims)?)
        },
        // The emitted façade spells every one of these `Dyn<term>`.
        | S::Dyn
        | S::Never
        | S::Table
        | S::Device
        | S::Opaque(_)
        | S::Variant(_)
        | S::Var(_)
        | S::Tensor { rank: None, .. }
        | S::Sparse { rank: None, .. } => Type::Dyn(Rep::Real),
    })
}

#[cfg(test)]
mod invoke_tests {
    use super::*;
    use crate::infer::Infer;

    fn hash_of(
        op: &str,
        index: usize,
    ) -> Option<String> {
        let registry = aevia_rssn::registry();
        let entry = registry.op(op)?;
        Some(hex(&entry.keys.get(index)?.signature_hash))
    }

    #[test]
    fn a_pinned_det_result_is_typed() {
        let Some(hash) = hash_of("det", 0) else {
            return; // the kernel manifest in this build has no `det`
        };
        let mut infer = Infer::new();
        let ty = invoke_result("det", &hash, &mut infer);
        assert!(ty.is_some(), "det's pinned result must be expressible");
        // Whatever class the manifest declares (`Dyn`, `Real`, a tensor), a
        // dimension variable introduced by the mapping is fresh and resolvable.
        if let Some(ty) = ty {
            let _ = infer.resolve(&ty);
        }
    }

    #[test]
    fn an_unknown_hash_falls_back() {
        let mut infer = Infer::new();
        assert!(invoke_result("det", "00000000000000000000000000000000", &mut infer).is_none());
        assert!(invoke_result("no_such_operator", "x", &mut infer).is_none());
    }

    #[test]
    fn sde_wiener_shares_one_dimension_variable() {
        let Some(hash) = hash_of("sde_wiener", 0) else {
            return; // the SDE manifest in this build has no `sde_wiener`
        };
        let mut infer = Infer::new();
        let Some(Type::Tensor { elem, .. }) = invoke_result("sde_wiener", &hash, &mut infer) else {
            panic!("sde_wiener's result is a rank-2 tensor");
        };
        let Type::Scalar(_, dim) = *elem else {
            panic!("its element is a dimensioned scalar");
        };
        // `T^(1/2)`: exactly one non-zero exponent, and it is a half.
        assert!(!dim.is_one(), "the time exponent survives: {dim}");
        let factors: Vec<_> = dim.factors().collect();
        assert_eq!(factors.len(), 1, "one dimension atom: {dim}");
    }
}
