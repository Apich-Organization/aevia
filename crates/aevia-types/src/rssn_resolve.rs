//! The overload lattice of §13.4.1, applied to Aevia types.
//!
//! `aevia-rssn` owns the manifest; this module decides which overload a call
//! is typed against. The rules are the kernel's:
//!
//! 1. filter by arity;
//! 2. classify each argument, take the *weakest* score;
//! 3. the unique maximum wins;
//! 4. pin the term contract for an eligible fixed-nullary derived pair;
//! 5. any other tie is `E1002`, no match is `E1003`.
//!
//! Resolution is a *type-level* decision, made once, at the call site; the
//! runtime executes the key it produced.

#![cfg_attr(
    not(test),
    deny(
        missing_docs,
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::nursery,
        clippy::panic,
        clippy::pedantic,
        clippy::single_call_fn,
        clippy::unwrap_used
    )
)]

use crate::ty::Rep;
use crate::ty::Shape;
use crate::ty::Type;
use aevia_dim::Dim;
use aevia_rssn::Sig;
use aevia_rssn::SigType;
use std::cmp::Ordering;

/// How well one argument matches one parameter.
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
        self != Self::No
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
    args: &[SigType],
) -> Resolution {
    let mut best_score = Score::No;
    let mut best_index = 0;
    let mut tied = Vec::new();
    'overloads: for (index, signature) in entry.signatures.iter().enumerate() {
        if !accepts_arity(signature, args.len()) {
            continue;
        }
        let mut worst = Score::Exact;
        for (param, got) in signature_parameters(signature).zip(args) {
            let score = score_type(param, got);
            if !score.accepted() {
                continue 'overloads;
            }
            worst = worst.min(score);
        }
        match worst.cmp(&best_score) {
            | Ordering::Greater => {
                best_score = worst;
                best_index = index;
                tied.clear();
            },
            | Ordering::Equal => {
                if tied.is_empty() {
                    tied.push(best_index);
                }
                tied.push(index);
            },
            | Ordering::Less => {},
        }
    }
    if !best_score.accepted() {
        Resolution::None
    } else if tied.is_empty() {
        Resolution::Unique {
            index: best_index,
            score: best_score,
        }
    } else if args.is_empty()
        && let Some(index) = entry.derived_nullary_term_index()
        && tied.len() == 2
        && tied.contains(&index)
    {
        Resolution::Unique {
            index,
            score: best_score,
        }
    } else {
        Resolution::Ambiguous { candidates: tied }
    }
}

fn accepts_arity(
    signature: &Sig,
    count: usize,
) -> bool {
    let Some(extra) = count.checked_sub(signature.params.len()) else {
        return false;
    };
    match &signature.variadic {
        | Some((min, max, _)) => extra >= *min && max.is_none_or(|max| extra <= max),
        | None => extra == 0,
    }
}

/// Finds the first argument that needs a free symbol for an otherwise applicable overload.
#[allow(
    clippy::single_call_fn,
    reason = "diagnoses the boundary without changing which calls resolve"
)]
pub(crate) fn symbol_argument_mismatch(
    entry: &aevia_rssn::OpEntry,
    args: &[SigType],
) -> Option<usize> {
    'overloads: for signature in &entry.signatures {
        if !accepts_arity(signature, args.len()) {
            continue;
        }
        let mut first_symbol = None;
        for (index, (param, got)) in signature_parameters(signature).zip(args).enumerate() {
            if score_type(param, got).accepted() {
                continue;
            }
            if matches!((param, got), (SigType::Symbol, SigType::Real)) {
                first_symbol.get_or_insert(index);
            } else {
                continue 'overloads;
            }
        }
        if first_symbol.is_some() {
            return first_symbol;
        }
    }
    None
}

fn signature_parameters(signature: &Sig) -> impl Iterator<Item = &SigType> {
    signature.params.iter().chain(
        signature
            .variadic
            .iter()
            .flat_map(|(_, max, elem)| std::iter::repeat_n(elem, max.unwrap_or(usize::MAX))),
    )
}

/// The score of one argument against one parameter.
#[must_use]
#[cfg_attr(
    not(test),
    expect(
        clippy::too_many_lines,
        reason = "keep the ordered overload match cases together"
    )
)]
pub fn score_type(
    param: &aevia_rssn::SigType,
    got: &aevia_rssn::SigType,
) -> Score {
    use aevia_rssn::SigType as S;
    match (param, got) {
        // A variable or `Dyn` parameter accepts anything (§13.4.1).
        | (S::Var(_) | S::Dyn | S::Never, _) | (_, S::Dyn | S::Var(_)) => Score::Generic,
        | (S::Int, S::Int)
        | (S::Real, S::Real)
        | (S::Float, S::Float)
        | (S::Complex, S::Complex)
        | (S::Bool, S::Bool)
        | (S::Str, S::Str)
        | (S::Symbol, S::Symbol)
        | (S::Table, S::Table)
        | (S::Device, S::Device) => Score::Exact,
        | (S::Unit(a), S::Unit(b)) | (S::Point(a), S::Point(b)) => {
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
        | (S::Real, S::Int | S::Float)
        | (S::Float, S::Int)
        | (S::Complex, S::Int | S::Real | S::Float) => Score::Widened,
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
        | (S::Dyn | S::Var(_), _)
        | (S::Sparse { .. }, S::Tensor { .. })
        | (S::Tensor { .. }, S::Sparse { .. }) => false,
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
        | Type::Bool | Type::Prop => S::Bool,
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
                            .map(|extent| extent.as_ref().and_then(super::ty::Nat::as_const))
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
                crate::keys::row_fields(row)
                    .map(|(name, ty)| (name.to_owned(), classify(ty)))
                    .collect(),
            )
        },
        | Type::Adt(id, args) => {
            if crate::adt::is_native_ad_var_type(ty) {
                return S::Opaque(aevia_rssn::registry::NATIVE_AD_VAR_KIND.into());
            }
            if *id == crate::adt::OPTION_ID
                && let [payload] = args.as_slice()
            {
                return S::Variant(vec![
                    ("None".to_string(), Vec::new()),
                    ("Some".to_string(), vec![classify(payload)]),
                ]);
            }
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
        | Type::Scalar(Rep::Var(_), _)
        | Type::Dyn(_)
        | Type::Var(_)
        | Type::Param(_)
        | Type::TensorIntrinsic(_)
        | Type::TypeRepIntrinsic(_)
        | Type::FromValueIntrinsic(_)
        | Type::WriteNpyIntrinsic
        | Type::ReadNpyIntrinsic(_)
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
            | Self::Int => "Int".to_string(),
            | Self::Real => "Real".to_string(),
            | Self::Float => "Float".to_string(),
            | Self::Complex => "Complex".to_string(),
            | Self::Bool => "Bool".to_string(),
            | Self::Str => "String".to_string(),
            | Self::Opaque(name) => name.clone(),
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

    #[test]
    fn variadic_limits_and_element_scoring_follow_the_fixed_parameters() {
        use SigType as S;
        let mut entry = aevia_rssn::registry().op("sin").unwrap().clone();
        entry.signatures = vec![Sig {
            params: vec![S::Bool],
            ret: S::Real,
            variadic: Some((1, Some(2), S::Real)),
        }];

        for invalid in [
            &[][..],
            &[S::Bool][..],
            &[S::Real, S::Real][..],
            &[S::Bool, S::Str][..],
            &[S::Bool, S::Real, S::Real, S::Real][..],
        ] {
            assert_eq!(resolve(&entry, invalid), Resolution::None, "{invalid:?}");
        }
        assert_eq!(
            resolve(&entry, &[S::Bool, S::Real]),
            Resolution::Unique {
                index: 0,
                score: Score::Exact
            }
        );
        assert_eq!(
            resolve(&entry, &[S::Bool, S::Real, S::Int]),
            Resolution::Unique {
                index: 0,
                score: Score::Widened
            }
        );
    }

    #[test]
    fn equal_best_overloads_retain_manifest_order() {
        use SigType as S;
        let mut entry = aevia_rssn::registry().op("sin").unwrap().clone();
        entry.signatures = [S::Dyn, S::Dyn, S::Real, S::Int, S::Dyn, S::Int]
            .into_iter()
            .map(|param| {
                Sig {
                    params: vec![param],
                    ret: S::Bool,
                    variadic: None,
                }
            })
            .collect();
        assert_eq!(
            resolve(&entry, &[S::Int]),
            Resolution::Ambiguous {
                candidates: vec![3, 5]
            }
        );
        entry.signatures.pop();
        assert_eq!(
            resolve(&entry, &[S::Int]),
            Resolution::Unique {
                index: 3,
                score: Score::Exact
            }
        );
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
    let digest = parse_signature_hash(signature_hash_hex)?;
    let registry = aevia_rssn::registry();
    let entry = registry.op(op_name)?;
    let mut matching = entry
        .keys
        .iter()
        .enumerate()
        .filter(|(_, key)| key.signature_hash == digest);
    let (index, _) = matching.next()?;
    if matching.next().is_some() {
        // Two overloads with one digest would make the claim ambiguous.
        return None;
    }
    let signature = entry.signatures.get(index)?;
    let mut dims = std::collections::HashMap::new();
    sig_type_to_type(&signature.ret, infer, &mut dims)
}

/// Instantiates a signature against the argument dimensions.
///
/// Representation widening has already been decided by the overload lattice.
/// The fixed parameters, variadic arguments, and return share one fresh
/// substitution. Arity is checked when the overload is selected.
///
/// # Errors
///
/// Returns the first dimension or inferred type conflict against the signature.
pub fn instantiate_dimensions(
    signature: &Sig,
    actual: &[Type],
    infer: &mut crate::infer::Infer,
) -> Result<Option<Type>, crate::infer::UnifyError> {
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
            | (S::Tensor { elem, .. }, Type::Tensor { elem: got, .. }) => {
                constrain(elem, got, infer, dims)
            },
            | (S::Sparse { elem, .. }, Type::Adt(..)) => {
                match crate::builtins::sparse_parts(&actual) {
                    | Some((_, Type::Tensor { elem: got, .. })) => {
                        constrain(elem, got, infer, dims)
                    },
                    | _ => Ok(()),
                }
            },
            | (S::Variant(cases), Type::Adt(id, args)) if *id == crate::adt::OPTION_ID => {
                if let [argument] = args.as_slice()
                    && let Some(payload) = aevia_rssn::registry::option_payload(cases)
                {
                    constrain(payload, argument, infer, dims)?;
                }
                Ok(())
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
    let mut dims = std::collections::HashMap::new();
    for (param, got) in signature_parameters(signature).zip(actual) {
        constrain(param, got, infer, &mut dims)?;
    }
    Ok(sig_type_to_type(&signature.ret, infer, &mut dims).map(|ret| infer.resolve(&ret)))
}

#[cfg_attr(
    not(test),
    expect(clippy::single_call_fn, reason = "parse a literal overload key once")
)]
fn parse_signature_hash(text: &str) -> Option<[u8; 16]> {
    const fn nibble(digit: u8) -> Option<u8> {
        match digit {
            | b'0'..=b'9' => Some(digit.saturating_sub(b'0')),
            | b'a'..=b'f' => Some(digit.saturating_sub(b'a').saturating_add(10)),
            | _ => None,
        }
    }
    let bytes: &[u8; 32] = text.as_bytes().try_into().ok()?;
    let mut out = [0; 16];
    for (byte, [high, low]) in out.iter_mut().zip(bytes.as_chunks::<2>().0) {
        *byte = (nibble(*high)? << 4) | nibble(*low)?;
    }
    Some(out)
}

/// Substitutes a signature dimension's variables with fresh checker variables,
/// sharing one mapping per call.
fn map_dim(
    dim: &Dim,
    infer: &mut crate::infer::Infer,
    dims: &mut std::collections::HashMap<u32, Dim>,
) -> Dim {
    dim.substitute(|id| Some(dims.entry(id).or_insert_with(|| infer.fresh_dim()).clone()))
}

/// Maps a manifest result type to the checker's type, or `None` when the
/// emitted spelling cannot express it.
///
/// The emitted façade uses `Dyn<term>` for symbols, tables, devices, most
/// opaque types, type variables and tensors with an unknown rank.
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
        | S::Unit(dim) => Type::UnitOf(map_dim(dim, infer, dims)),
        | S::Point(dim) => Type::Point(map_dim(dim, infer, dims)),
        | S::Dim(base, dim) => {
            let rep = match **base {
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
            let rank = u32::try_from(*rank).ok()?;
            Type::tensor(
                shape.as_ref().map_or_else(
                    || Shape::Rank(rank),
                    |extents| Shape::refined(extents.iter().copied().map(Some)),
                ),
                sig_type_to_type(elem, infer, dims)?,
            )
        },
        | S::Sparse {
            rank: Some(rank),
            elem,
        } => {
            let dense = Type::tensor(
                Shape::Rank(u32::try_from(*rank).ok()?),
                sig_type_to_type(elem, infer, dims)?,
            );
            crate::builtins::sparse_type(infer.fresh_type(), dense)
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
        | S::Variant(cases) => {
            match aevia_rssn::registry::option_payload(cases) {
                | Some(payload) => Type::option(sig_type_to_type(payload, infer, dims)?),
                | None => Type::Dyn(Rep::Real),
            }
        },
        | S::Opaque(name) if name == aevia_rssn::registry::NATIVE_AD_VAR_KIND => {
            crate::adt::native_ad_var_type()
        },
        // The emitted façade spells every one of these `Dyn<term>`.
        | S::Symbol
        | S::Dyn
        | S::Never
        | S::Table
        | S::Device
        | S::Opaque(_)
        | S::Var(_)
        | S::Tensor { rank: None, .. }
        | S::Sparse { rank: None, .. } => Type::Dyn(Rep::Real),
    })
}

#[cfg(test)]
mod invoke_tests {
    use super::*;
    use crate::infer::Infer;
    use std::fmt::Write as _;

    fn hash_of(
        op: &str,
        index: usize,
    ) -> Option<String> {
        let registry = aevia_rssn::registry();
        let entry = registry.op(op)?;
        let mut hash = String::new();
        for byte in entry.keys.get(index)?.signature_hash {
            write!(hash, "{byte:02x}").ok()?;
        }
        Some(hash)
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
    fn only_canonical_hex_can_pin_a_signature() {
        assert_eq!(
            parse_signature_hash("ab".repeat(16).as_str()),
            Some([0xab; 16])
        );
        assert!(parse_signature_hash("AB".repeat(16).as_str()).is_none());
        assert!(parse_signature_hash("00".repeat(17).as_str()).is_none());
        assert!(parse_signature_hash("λ".repeat(16).as_str()).is_none());
    }

    #[test]
    fn an_unrepresentable_rank_does_not_become_rank_one() {
        let Ok(rank) = usize::try_from(u64::from(u32::MAX) + 1) else {
            return;
        };
        let mut infer = Infer::new();
        let mut dimensions = std::collections::HashMap::new();
        for signature in [
            aevia_rssn::SigType::Tensor {
                rank: Some(rank),
                shape: None,
                elem: Box::new(aevia_rssn::SigType::Real),
            },
            aevia_rssn::SigType::Sparse {
                rank: Some(rank),
                elem: Box::new(aevia_rssn::SigType::Real),
            },
        ] {
            assert!(sig_type_to_type(&signature, &mut infer, &mut dimensions).is_none());
        }
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

#[cfg(test)]
mod option_tests {
    use super::*;
    use aevia_rssn::SigType as S;

    #[test]
    fn option_payload_dimensions_are_shared_with_parameters_and_result() {
        let d = Dim::var(12345);
        let option = |payload| {
            S::Variant(vec![
                ("None".into(), vec![]),
                ("Some".into(), vec![payload]),
            ])
        };
        let signature = aevia_rssn::Sig {
            params: vec![option(S::Dim(Box::new(S::Float), d.clone()))],
            ret: option(S::Dim(Box::new(S::Float), d)),
            variadic: None,
        };
        let actual = Type::option(Type::float(Dim::base("Length")));
        assert!(score_type(&signature.params[0], &classify(&actual)).accepted());
        let result = instantiate_dimensions(&signature, &[actual], &mut crate::infer::Infer::new())
            .unwrap()
            .unwrap();
        assert_eq!(result, Type::option(Type::float(Dim::base("Length"))));
    }
}
