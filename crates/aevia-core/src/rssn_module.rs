//! The `Rssn` module: the kernel's operators as ordinary Aevia functions.
//!
//! The member list is not written here. `Rssn.sin` resolves through the
//! Kernel Manifest at check time ([`aevia_types`] maps the signature to a
//! type) and at run time this module executes the overload the checker
//! pinned. Adding an operator to rssn makes `Rssn.<name>` work without a
//! change in this file.
//!
//! # The fixed members of §8.19
//!
//! Besides the operators, §8.19 declares a handful of *fixed* entry points:
//! `call`/`dynamic`, `typed`, `call_ho`, `call_key`, `register`/
//! `register_plugin`, `resolve_overload`, `overloads`, and the data members
//! (`Version`, `ManifestHash`, `Capabilities`, `Units`, `Constants`,
//! `ValueKinds`). `call_ho` executes interpreter-owned callbacks in a
//! thread-affine scope; the host-only `call` helper has no interpreter and
//! cannot run non-empty callback lists. `register` and `register_plugin`
//! take a host `Plugin`, which source cannot hold (`E0302`); source plugins
//! are published by `@op`/`@rule` declarations and `load_plugin` (§13.5.2).
//!
//! # Two operator paths, one decision
//!
//! [`call_operator`] takes an explicit [`OpKey`] and never resolves anything:
//! that is the path the design requires, and the one the checker pins. [`call`]'s
//! operator arm is unreachable from a checked call site — the evaluator uses
//! [`call_operator`] — so an operator that reaches it is `E1005` rather than a
//! re-resolution.

use crate::eval::EvalError;
use crate::kernel as bridge;
use crate::value::NativeFn;
use crate::value::Value;
use crate::value::show;
use aevia_dim::Dim;
use aevia_dim::DimUnifier;
use aevia_rssn::OpEntry;
use aevia_rssn::OpKey;
use aevia_rssn::SigType;
use aevia_rssn::Val;
use aevia_rssn::registry;
use aevia_syntax::Span;
use std::rc::Rc;

/// Calls `Rssn.<name>`.
///
/// # Errors
///
/// `E1003` when the member is neither a fixed entry point nor a registered
/// operator, plus whatever the kernel reports.
pub fn call(
    name: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, EvalError> {
    match name {
        | "Version" => Ok(Value::string(registry().version.clone())),
        | "ManifestHash" => Ok(Value::string(registry().manifest_hash.clone())),
        | "Capabilities" => Ok(string_tensor(registry().capabilities.names())),
        | "Units" => {
            Ok(string_tensor(
                registry()
                    .units
                    .iter()
                    .map(|unit| unit.name.clone())
                    .collect(),
            ))
        },
        | "Constants" => {
            Ok(string_tensor(
                registry()
                    .constants
                    .iter()
                    .map(|constant| constant.symbol.clone())
                    .collect(),
            ))
        },
        | "ValueKinds" => {
            Ok(string_tensor(
                registry()
                    .value_kinds
                    .iter()
                    .map(|(kind, _, _)| kind.clone())
                    .collect(),
            ))
        },
        | "call" | "dynamic" => dynamic_call(&args, span),
        | "typed" => typed_call(&args, span),
        | "call_ho" => call_ho(&args, span),
        | "call_ho_key" => {
            Err(EvalError::new(
                "E1001",
                "core.callback requires an interpreter-owned callback scope",
            )
            .at(span))
        },
        | "register" | "register_plugin" => register(&args, span),
        | "call_key" => call_key(&args, span),
        | "invoke" => invoke(&args, span, None),
        | "resolve_overload" => resolve(&args, span),
        | "overloads" => overloads(&args, span),
        // An operator call reaches the evaluator through
        // `Interp::rssn_calls`, which carries the key the checker pinned
        //. Reaching this arm means the call did not come from a
        // checked call site, and there is nothing to resolve *with*.
        | _ => {
            Err(EvalError::new(
                "E1005",
                format!("`Rssn.{name}` has no pinned overload here"),
            )
            .at(span)
            .with_note("an operator is not a first-class value"))
        },
    }
}

#[cfg(test)]
mod empty_composite_schema_tests {
    use super::*;
    use aevia_rssn::witness::DimensionWitness as W;
    use aevia_types::Row;
    use aevia_types::Shape;
    use aevia_types::Type;

    fn wrapped(
        repr: Option<SigType>,
        element: Option<W>,
    ) -> Value {
        Value::KernelValue(Rc::new(crate::value::KernelValue {
            payload: Val::Tensor {
                shape: vec![0],
                data: vec![],
            },
            witness: W::Tensor {
                dimension: None,
                element: element.map(Box::new),
                element_repr: repr,
                elements: vec![],
            },
            host_witness: None,
        }))
    }

    fn record_schema() -> Type {
        Type::Record(Row::closed([
            ("value".to_string(), Type::float(Dim::base("Time"))),
            ("ready".to_string(), Type::Bool),
        ]))
    }

    #[test]
    fn empty_records_require_the_full_structural_and_dimensional_schema() {
        let record = record_schema();
        let expected = Type::tensor(Shape::Rank(1), record.clone());
        let (repr, witness) = empty_schema(&record).unwrap();
        assert!(
            from_kernel_value(
                &wrapped(Some(repr.clone()), Some(witness.clone())),
                &expected
            )
            .is_ok()
        );
        assert!(from_kernel_value(&wrapped(None, Some(witness.clone())), &expected).is_err());
        assert!(from_kernel_value(&wrapped(Some(repr.clone()), None), &expected).is_err());
        let W::Record(mut fields) = witness.clone() else {
            unreachable!()
        };
        fields
            .iter_mut()
            .find(|(name, _)| name == "value")
            .unwrap()
            .1 = W::Scalar(Dim::base("Length"));
        assert!(
            from_kernel_value(
                &wrapped(Some(repr.clone()), Some(W::Record(fields))),
                &expected
            )
            .is_err()
        );
        let SigType::Record(mut fields) = repr else {
            unreachable!()
        };
        fields
            .iter_mut()
            .find(|(name, _)| name == "value")
            .unwrap()
            .1 = SigType::Int;
        assert!(
            from_kernel_value(
                &wrapped(Some(SigType::Record(fields)), Some(witness)),
                &expected
            )
            .is_err()
        );
        let nominal = Type::tensor(Shape::Rank(1), Type::option(Type::int()));
        assert!(
            from_kernel_value(
                &wrapped(Some(SigType::Opaque("Option".into())), Some(W::Unknown)),
                &nominal
            )
            .is_err()
        );
    }

    #[test]
    fn empty_nested_tensors_require_authenticated_rank_and_extents() {
        let inner = Type::tensor(Shape::Rank(1), Type::float(Dim::one()));
        let expected = Type::tensor(Shape::Rank(1), inner.clone());
        let (repr, witness) = empty_schema(&inner).unwrap();
        let value = wrapped(Some(repr.clone()), Some(witness.clone()));
        assert!(from_kernel_value(&value, &expected).is_ok());
        let wrong_rank = SigType::Tensor {
            rank: Some(2),
            shape: None,
            elem: Box::new(SigType::Float),
        };
        assert!(from_kernel_value(&wrapped(Some(wrong_rank), Some(witness)), &expected).is_err());
        let refined_inner = Type::tensor(Shape::refined([Some(3)]), Type::float(Dim::one()));
        let (refined_repr, refined_witness) = empty_schema(&refined_inner).unwrap();
        let refined = Type::tensor(Shape::Rank(1), refined_inner);
        assert!(from_kernel_value(&value, &refined).is_err());
        assert!(
            from_kernel_value(
                &wrapped(Some(refined_repr.clone()), Some(refined_witness.clone())),
                &refined
            )
            .is_ok()
        );
        let SigType::Tensor { rank, elem, .. } = refined_repr else {
            unreachable!()
        };
        let wrong_extent = SigType::Tensor {
            rank,
            shape: Some(vec![4]),
            elem,
        };
        assert!(
            from_kernel_value(
                &wrapped(Some(wrong_extent), Some(refined_witness)),
                &refined
            )
            .is_err()
        );
        let malformed = Value::KernelValue(Rc::new(crate::value::KernelValue {
            payload: Val::Tensor {
                shape: vec![1],
                data: vec![],
            },
            witness: W::Tensor {
                dimension: None,
                element: None,
                element_repr: Some(repr),
                elements: vec![],
            },
            host_witness: None,
        }));
        assert!(from_kernel_value(&malformed, &expected).is_err());
    }

    #[test]
    fn authenticated_host_schema_survives_an_empty_kernel_roundtrip() {
        let element = record_schema();
        let tensor = crate::value::TensorData::checked(
            vec![0],
            crate::value::Storage::Boxed(vec![]),
            Dim::one(),
            None,
            element.clone(),
        )
        .unwrap();
        let value = Value::Tensor(Rc::new(tensor));
        let witness = host_dimension_witness(&value);
        let opaque = Value::KernelValue(Rc::new(crate::value::KernelValue {
            payload: crate::kernel::to_val(&value).unwrap(),
            witness,
            host_witness: None,
        }));
        let expected = Type::tensor(Shape::Rank(1), element);
        let decoded = from_kernel_value(&opaque, &expected).unwrap();
        let second = Value::KernelValue(Rc::new(crate::value::KernelValue {
            payload: crate::kernel::to_val(&decoded).unwrap(),
            witness: host_dimension_witness(&decoded),
            host_witness: None,
        }));
        assert!(from_kernel_value(&second, &expected).is_ok());
    }
}

/// True when `name` is one of the fixed entry points rather than an operator.
#[must_use]
pub fn is_entry_point(name: &str) -> bool {
    matches!(
        name,
        "Version"
            | "ManifestHash"
            | "Capabilities"
            | "Units"
            | "Constants"
            | "ValueKinds"
            | "call"
            | "dynamic"
            | "typed"
            | "call_ho"
            | "call_ho_key"
            | "register"
            | "register_plugin"
            | "call_key"
            | "invoke"
            | "resolve_overload"
            | "overloads"
    )
}

/// Calls exactly the overload `key` names, without resolving anything.
///
/// This is the dispatch entry point of pinned calls: the key was chosen
/// statically, so the runtime must execute it and never look for a better
/// match.
///
/// # Errors
///
/// `E1003` when the key names no registered operator, `E0315` on a wrong
/// argument count, plus whatever the kernel reports.
pub fn call_operator(
    key: &OpKey,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, EvalError> {
    let Some(entry) = registry().op(&key.name) else {
        return Err(
            EvalError::new("E1003", format!("rssn has no operator `{}`", key.name)).at(span),
        );
    };
    call_operator_checked(key, entry, args, span, None)
}

/// Executes a pinned call with the resolved call-site return type, when the
/// interpreter has static evidence. The unifier is local to this call.
pub(crate) fn call_operator_with_evidence(
    key: &OpKey,
    args: Vec<Value>,
    span: Span,
    expected: Option<&aevia_types::Type>,
) -> Result<Value, EvalError> {
    let Some(entry) = registry().op(&key.name) else {
        return Err(
            EvalError::new("E1003", format!("rssn has no operator `{}`", key.name)).at(span),
        );
    };
    call_operator_checked(key, entry, args, span, expected)
}
fn call_operator_checked(
    key: &OpKey,
    entry: &OpEntry,
    args: Vec<Value>,
    span: Span,
    expected: Option<&aevia_types::Type>,
) -> Result<Value, EvalError> {
    let converted = arguments(entry, &key.name, &args, span)?;
    let flat = flatten_arguments(entry, &args);
    execute_checked(key, entry, &flat, converted, span, expected)
}
fn flatten_arguments(
    entry: &OpEntry,
    args: &[Value],
) -> Vec<Value> {
    let mut flat = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        if entry.is_variadic()
            && index + 1 == args.len()
            && let Some(items) = list_items(arg)
        {
            flat.extend(items);
            continue;
        }
        flat.push(arg.clone());
    }
    flat
}

/// Converts the arguments of a call, honouring the variadic ABI.
///
/// A variadic operator is typed `[α] -> R` and generated as
/// `concat([fixed…], rest)`, so the list the call site passes is *one*
/// argument statically and N positional arguments at the kernel. Passing the
/// list through as a single argument is what made `Rssn.and([true, false])`
/// answer a term where the checker promised a `Bool`.
fn arguments(
    entry: &OpEntry,
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Vec<Val>, EvalError> {
    if !entry.accepts(args.len()) {
        return Err(EvalError::new(
            "E0315",
            format!(
                "`{name}` takes {} argument(s), found {}",
                entry.arity.0,
                args.len()
            ),
        )
        .at(span));
    }
    let variadic = entry.is_variadic();
    let last = args.len().checked_sub(1);
    let mut converted = Vec::with_capacity(args.len());
    for (index, arg) in args.iter().enumerate() {
        // Only the trailing argument spreads, and only when it is a list: a
        // rank-1 tensor is the static `[α]`, anything else is a single value.
        if variadic
            && Some(index) == last
            && let Some(items) = list_items(arg)
        {
            for item in &items {
                converted.push(bridge::to_val(item)?);
            }
            continue;
        }
        converted.push(bridge::to_val(arg)?);
    }
    Ok(converted)
}

/// The elements of a rank-1 tensor, when the value is one.
fn list_items(value: &Value) -> Option<Vec<Value>> {
    let Value::Tensor(tensor) = value else {
        return None;
    };
    if tensor.shape.len() != 1 {
        return None;
    }
    Some(
        (0..tensor.storage.len())
            .filter_map(|index| tensor.get_flat(index))
            .collect(),
    )
}

/// Executes a resolved overload on converted arguments.
fn execute_checked(
    key: &OpKey,
    entry: &OpEntry,
    args: &[Value],
    converted: Vec<Val>,
    span: Span,
    expected: Option<&aevia_types::Type>,
) -> Result<Value, EvalError> {
    let signature = entry
        .key_index(key)
        .and_then(|index| entry.signatures.get(index));
    let dimensions = if entry.source.is_exact() {
        signature
            .map(|sig| bind_dimensions(sig, args, expected, span))
            .transpose()?
    } else {
        None
    };
    let (value, returned_witness) = if entry.is_value_native() && entry.source.is_exact() {
        let inputs: Vec<_> = args.iter().map(host_dimension_witness).collect();
        let (value, witness) = aevia_rssn::kernel()
            .call_key_with_witnesses(key, &converted, &inputs)
            .map_err(|failure| bridge::error(&failure).at(span))?;
        (value, Some(witness))
    } else {
        let value = aevia_rssn::kernel()
            .call_key(key, &converted)
            .map_err(|failure| bridge::error(&failure).at(span))?;
        (value, None)
    };
    if let (Some(sig), Some(dimensions)) = (signature, dimensions) {
        if let Some(witness) = &returned_witness {
            verify_result_witness(&sig.ret, witness, &dimensions, span)?;
        }
        return result_with_dimensions(&value, &sig.ret, &dimensions, span);
    }
    let dim = result_dim(entry, key);
    Ok(bridge::from_val(&value, dim))
}

/// The source value's dimension is evidence, including for an empty tensor.
pub(crate) fn host_dimension_witness(value: &Value) -> aevia_rssn::witness::DimensionWitness {
    use aevia_rssn::witness::DimensionWitness as W;
    match value {
        | Value::KernelValue(value) => value.witness.clone(),
        | Value::Num(_) | Value::Dyn(_) => W::Scalar(crate::value::value_dim(value)),
        | Value::Tensor(tensor) => {
            let nested = tensor
                .get_flat(0)
                .is_some_and(|item| matches!(item, Value::Tensor(_) | Value::Record(_)));
            let elements = if nested {
                (0..tensor.storage.len())
                    .filter_map(|i| tensor.get_flat(i))
                    .map(|v| host_dimension_witness(&v))
                    .collect()
            } else {
                Vec::new()
            };
            let mut element_repr = match tensor.storage.backing() {
                | crate::value::Storage::Dense(_) | crate::value::Storage::View(_) => {
                    Some(SigType::Float)
                },
                | crate::value::Storage::TypedExact(_, crate::value::ExactRep::Int) => {
                    Some(SigType::Int)
                },
                | crate::value::Storage::TypedExact(_, crate::value::ExactRep::Real) => {
                    Some(SigType::Real)
                },
                | crate::value::Storage::Bool(_) => Some(SigType::Bool),
                | _ => None,
            };
            // An empty tensor's element schema cannot be read from its data.
            // Private tensor provenance can establish it, but only after the
            // current public shape and storage have been revalidated.
            let mut element = None;
            if tensor.storage.is_empty()
                && let Some(schema) = tensor.verified_element.as_deref()
            {
                let rank = u32::try_from(tensor.shape.len()).ok();
                let valid = rank.is_some_and(|rank| {
                    crate::host_witness::tensor_witnesses(
                        tensor,
                        &aevia_types::Shape::Rank(rank),
                        schema,
                        false,
                    )
                    .is_some()
                });
                if valid && let Some((repr, witness)) = empty_schema(schema) {
                    element_repr = Some(repr);
                    element = Some(Box::new(witness));
                }
            }
            W::Tensor {
                dimension: Some(tensor.dim.clone()),
                element,
                element_repr,
                elements,
            }
        },
        | Value::Record(record) => {
            W::Record(
                record
                    .fields
                    .iter()
                    .map(|(name, value)| (name.clone(), host_dimension_witness(value)))
                    .collect(),
            )
        },
        | _ => W::Unknown,
    }
}

/// The structural schema that can be carried independently of an empty
/// tensor's payload. Fully concrete inner extents are retained; partially
/// known shapes and nominal identities are never inferred.
fn empty_schema(
    ty: &aevia_types::Type
) -> Option<(SigType, aevia_rssn::witness::DimensionWitness)> {
    use aevia_rssn::witness::DimensionWitness as W;
    use aevia_types::Rep;
    use aevia_types::Shape;
    use aevia_types::Type;
    match ty {
        | Type::Scalar(rep, dim) if dim.is_concrete() => {
            let repr = match rep {
                | Rep::Int => SigType::Int,
                | Rep::Real => SigType::Real,
                | Rep::Float => SigType::Float,
                | Rep::Complex => SigType::Complex,
                | Rep::Var(_) => return None,
            };
            Some((repr, W::Scalar(dim.clone())))
        },
        | Type::Bool => Some((SigType::Bool, W::Unknown)),
        | Type::Str => Some((SigType::Str, W::Unknown)),
        | Type::Tensor { shape, elem } => {
            let extents = match shape {
                | Shape::Rank(_) => None,
                | Shape::Refined(extents) if extents.iter().all(Option::is_none) => None,
                | Shape::Refined(extents) => {
                    Some(
                        extents
                            .iter()
                            .map(|extent| extent.as_ref().and_then(|extent| extent.as_const()))
                            .collect::<Option<Vec<_>>>()?,
                    )
                },
            };
            let (repr, witness) = empty_schema(elem)?;
            let dimension = if let W::Scalar(dim) = &witness {
                Some(dim.clone())
            } else {
                None
            };
            Some((
                SigType::Tensor {
                    rank: Some(shape.rank() as usize),
                    shape: extents,
                    elem: Box::new(repr.clone()),
                },
                W::Tensor {
                    dimension,
                    element: Some(Box::new(witness)),
                    element_repr: Some(repr),
                    elements: vec![],
                },
            ))
        },
        | Type::Record(row) if row.tail_var().is_none() => {
            let mut repr = Vec::new();
            let mut fields = Vec::new();
            for (name, ty) in row.fields() {
                let (field_repr, witness) = empty_schema(&ty)?;
                repr.push((name.clone(), field_repr));
                fields.push((name, witness));
            }
            Some((SigType::Record(repr), W::Record(fields)))
        },
        | _ => None,
    }
}

/// Compare independently supplied structural representation and dimensional
/// evidence; a record must have every field exactly once in both channels.
fn empty_schema_matches(
    expected: &aevia_types::Type,
    actual_repr: &SigType,
    actual_witness: &aevia_rssn::witness::DimensionWitness,
) -> bool {
    use aevia_rssn::witness::DimensionWitness as W;
    use aevia_types::Rep;
    use aevia_types::Row;
    use aevia_types::Shape;
    use aevia_types::Type;
    match (expected, actual_repr, actual_witness) {
        | (Type::Scalar(rep, dim), repr, W::Scalar(actual)) => {
            dim.is_concrete()
                && dim == actual
                && matches!(
                    (rep, repr),
                    (Rep::Int, SigType::Int)
                        | (Rep::Real, SigType::Real)
                        | (Rep::Float, SigType::Float)
                        | (Rep::Complex, SigType::Complex)
                )
        },
        | (Type::Bool, SigType::Bool, W::Unknown) | (Type::Str, SigType::Str, W::Unknown) => true,
        | (
            Type::Tensor { shape, elem },
            SigType::Tensor {
                rank: Some(rank),
                shape: actual_shape,
                elem: actual_elem,
            },
            W::Tensor {
                dimension,
                element: Some(actual_element),
                element_repr: Some(element_repr),
                elements,
            },
        ) => {
            let extents_match = match shape {
                | Shape::Rank(_) => true,
                | Shape::Refined(extents) if extents.iter().all(Option::is_none) => true,
                | Shape::Refined(extents) => {
                    actual_shape.as_ref().is_some_and(|actual| {
                        actual.len() == extents.len()
                            && extents.iter().zip(actual).all(|(extent, actual)| {
                                extent.as_ref().and_then(|extent| extent.as_const())
                                    == Some(*actual)
                            })
                    })
                },
            };
            extents_match
                && actual_shape
                    .as_ref()
                    .is_none_or(|shape| shape.len() == *rank)
                && *rank == shape.rank() as usize
                && elements.is_empty()
                && actual_elem.as_ref() == element_repr
                && match elem.as_ref() {
                    | Type::Scalar(_, dim) => dimension.as_ref() == Some(dim),
                    | _ => true,
                }
                && empty_schema_matches(elem, actual_elem, actual_element)
        },
        | (Type::Record(row), SigType::Record(actual_fields), W::Record(actual_witnesses)) => {
            let mut current = row;
            let mut count = 0;
            loop {
                match current {
                    | Row::Empty => {
                        break count == actual_fields.len() && count == actual_witnesses.len();
                    },
                    | Row::Var(_) => break false,
                    | Row::Extend { label, ty, tail } => {
                        let mut remaining = tail.as_ref();
                        let mut duplicate = false;
                        while let Row::Extend {
                            label: other, tail, ..
                        } = remaining
                        {
                            if other == label {
                                duplicate = true;
                                break;
                            }
                            remaining = tail;
                        }
                        if duplicate {
                            break false;
                        }
                        if actual_fields.iter().filter(|(key, _)| key == label).count() != 1
                            || actual_witnesses
                                .iter()
                                .filter(|(key, _)| key == label)
                                .count()
                                != 1
                        {
                            break false;
                        }
                        let repr = &actual_fields
                            .iter()
                            .find(|(key, _)| key == label)
                            .unwrap()
                            .1;
                        let witness = &actual_witnesses
                            .iter()
                            .find(|(key, _)| key == label)
                            .unwrap()
                            .1;
                        if !empty_schema_matches(ty, repr, witness) {
                            break false;
                        }
                        count += 1;
                        current = tail;
                    },
                }
            }
        },
        | _ => false,
    }
}
/// Compare explicit kernel witnesses to the independently instantiated result
/// type. Missing or contradictory physical evidence is never overwritten.
fn verify_result_witness(
    sig: &SigType,
    witness: &aevia_rssn::witness::DimensionWitness,
    dims: &DimUnifier,
    span: Span,
) -> Result<(), EvalError> {
    use aevia_rssn::witness::DimensionWitness as W;
    fn require(
        expected: Dim,
        got: Option<&Dim>,
        span: Span,
    ) -> Result<(), EvalError> {
        if !expected.is_concrete() {
            return Err(EvalError::new("E1001", "kernel result dimension is unknown").at(span));
        }
        match got {
            | Some(actual) if *actual == expected => Ok(()),
            | Some(_) => {
                Err(
                    EvalError::new("E0412", "kernel result dimension contradicts its signature")
                        .at(span),
                )
            },
            | None => {
                Err(EvalError::new("E1001", "kernel omitted the result dimension witness").at(span))
            },
        }
    }
    match (sig, witness) {
        | (SigType::Dim(_, d), W::Scalar(got)) => require(dims.resolve(d), Some(got), span),
        | (SigType::Dim(_, d), _) => require(dims.resolve(d), None, span),
        | (SigType::Int | SigType::Real | SigType::Float | SigType::Complex, W::Scalar(got)) => {
            require(Dim::one(), Some(got), span)
        },
        | (SigType::Int | SigType::Real | SigType::Float | SigType::Complex, _) => {
            require(Dim::one(), None, span)
        },
        | (
            SigType::Tensor { elem, .. } | SigType::Sparse { elem, .. },
            W::Tensor {
                dimension,
                element,
                elements,
                ..
            },
        ) => {
            match &**elem {
                | SigType::Dim(_, d) => {
                    let expected = dims.resolve(d);
                    require(expected.clone(), dimension.as_ref(), span)?;
                    for item in elements {
                        match item {
                            | W::Scalar(actual) => require(expected.clone(), Some(actual), span)?,
                            | _ => {
                                return Err(EvalError::new(
                                    "E1001",
                                    "kernel tensor element has an unknown dimension witness",
                                )
                                .at(span));
                            },
                        }
                    }
                },
                | SigType::Int | SigType::Real | SigType::Float | SigType::Complex => {
                    require(Dim::one(), dimension.as_ref(), span)?;
                    for item in elements {
                        match item {
                            | W::Scalar(actual) => require(Dim::one(), Some(actual), span)?,
                            | _ => {
                                return Err(EvalError::new(
                                    "E1001",
                                    "kernel tensor element has an unknown dimension witness",
                                )
                                .at(span));
                            },
                        }
                    }
                },
                | _ => {
                    if elements.is_empty() {
                        if let Some(element) = element {
                            verify_result_witness(elem, element, dims, span)?;
                        } else {
                            return Err(EvalError::new(
                                "E1001",
                                "kernel omitted empty tensor element witness",
                            )
                            .at(span));
                        }
                    } else {
                        for item in elements {
                            verify_result_witness(elem, item, dims, span)?;
                        }
                    }
                },
            }
            Ok(())
        },
        | (SigType::Record(fields), W::Record(actual)) => {
            for (name, field) in fields {
                let Some((_, value)) = actual.iter().find(|(field, _)| field == name) else {
                    return Err(EvalError::new(
                        "E1004",
                        format!("kernel omitted result field `{name}`"),
                    )
                    .at(span));
                };
                verify_result_witness(field, value, dims, span)?;
            }
            Ok(())
        },
        | (SigType::Tensor { .. } | SigType::Sparse { .. } | SigType::Record(_), _) => {
            Err(EvalError::new("E1001", "kernel omitted structured result witnesses").at(span))
        },
        | _ => Ok(()),
    }
}

/// Bind the manifest's dimension variables to the actual argument values.
fn bind_dimensions(
    sig: &aevia_rssn::Sig,
    args: &[Value],
    expected: Option<&aevia_types::Type>,
    span: Span,
) -> Result<DimUnifier, EvalError> {
    let mut dims = DimUnifier::new();
    fn bind(
        sig: &SigType,
        value: &Value,
        dims: &mut DimUnifier,
        span: Span,
    ) -> Result<(), EvalError> {
        let mismatch = |_: aevia_dim::DimMismatch| {
            EvalError::new(
                "E0412",
                "kernel argument dimensions do not satisfy its signature",
            )
            .at(span)
        };
        if let Value::KernelValue(boxed) = value {
            fn bind_witness(
                sig: &SigType,
                witness: &aevia_rssn::witness::DimensionWitness,
                dims: &mut DimUnifier,
                span: Span,
            ) -> Result<(), EvalError> {
                use aevia_rssn::witness::DimensionWitness as W;
                let mismatch = |_: aevia_dim::DimMismatch| {
                    EvalError::new(
                        "E0412",
                        "kernel argument dimensions do not satisfy its signature",
                    )
                    .at(span)
                };
                match (sig, witness) {
                    | (SigType::Dim(_, d), W::Scalar(actual)) => {
                        dims.unify(d, actual).map_err(mismatch)
                    },
                    | (
                        SigType::Int | SigType::Real | SigType::Float | SigType::Complex,
                        W::Scalar(actual),
                    ) => dims.unify(&Dim::one(), actual).map_err(mismatch),
                    | (
                        SigType::Tensor { elem, .. } | SigType::Sparse { elem, .. },
                        W::Tensor {
                            dimension: Some(d), ..
                        },
                    ) => bind_witness(elem, &W::Scalar(d.clone()), dims, span),
                    | (SigType::Record(fields), W::Record(values)) => {
                        for (name, ty) in fields {
                            if let Some((_, w)) = values.iter().find(|(n, _)| n == name) {
                                bind_witness(ty, w, dims, span)?;
                            }
                        }
                        Ok(())
                    },
                    | (
                        SigType::Dim(_, _)
                        | SigType::Int
                        | SigType::Real
                        | SigType::Float
                        | SigType::Complex,
                        _,
                    ) => {
                        Err(
                            EvalError::new("E1001", "kernel argument has no dimension witness")
                                .at(span),
                        )
                    },
                    | _ => Ok(()),
                }
            }
            return bind_witness(sig, &boxed.witness, dims, span);
        }
        match (sig, value) {
            | (SigType::Dim(_, dim), Value::Num(_) | Value::Dyn(_)) => {
                dims.unify(dim, &crate::value::value_dim(value))
                    .map_err(mismatch)
            },
            | (
                SigType::Real | SigType::Float | SigType::Int | SigType::Complex,
                Value::Num(_) | Value::Dyn(_),
            ) => {
                dims.unify(&Dim::one(), &crate::value::value_dim(value))
                    .map_err(mismatch)
            },
            | (SigType::Unit(dim) | SigType::Point(dim), Value::UnitOf(unit)) => {
                dims.unify(dim, &unit.dim).map_err(mismatch)
            },
            | (SigType::Point(dim), Value::Num(_)) => {
                dims.unify(dim, &crate::value::value_dim(value))
                    .map_err(mismatch)
            },
            | (
                SigType::Tensor { elem, .. } | SigType::Sparse { elem, .. },
                Value::Tensor(tensor),
            ) => {
                match &**elem {
                    | SigType::Dim(_, dim) => dims.unify(dim, &tensor.dim).map_err(mismatch),
                    | SigType::Real | SigType::Float | SigType::Int | SigType::Complex => {
                        dims.unify(&Dim::one(), &tensor.dim).map_err(mismatch)
                    },
                    | _ => {
                        for i in 0..tensor.storage.len() {
                            if let Some(item) = tensor.get_flat(i) {
                                bind(elem, &item, dims, span)?;
                            }
                        }
                        Ok(())
                    },
                }
            },
            | (SigType::Record(fields), Value::Record(record)) => {
                for (name, field) in fields {
                    if let Some(value) = record.fields.get(name) {
                        bind(field, value, dims, span)?;
                    }
                }
                Ok(())
            },
            | _ => Ok(()),
        }
    }
    fn evidence(
        sig: &SigType,
        ty: &aevia_types::Type,
        dims: &mut DimUnifier,
        span: Span,
    ) -> Result<(), EvalError> {
        use aevia_types::Type;
        let mismatch = |_: aevia_dim::DimMismatch| {
            EvalError::new(
                "E0412",
                "kernel return dimensions disagree with the checked call",
            )
            .at(span)
        };
        match (sig, ty) {
            | (SigType::Dim(_, dim), Type::Scalar(_, got))
            | (SigType::Point(dim), Type::Point(got))
            | (SigType::Unit(dim), Type::UnitOf(got))
                if got.is_concrete() =>
            {
                // A result annotation is a check, never a source of evidence:
                // binding an otherwise unknown result dimension from it would
                // turn `from_value<T>` into a type assertion.
                let actual = dims.resolve(dim);
                if actual.is_concrete() {
                    dims.unify(&actual, got).map_err(mismatch)
                } else {
                    Ok(())
                }
            },
            | (
                SigType::Tensor { elem, .. } | SigType::Sparse { elem, .. },
                Type::Tensor { elem: got, .. },
            ) => evidence(elem, got, dims, span),
            | (SigType::Record(fields), Type::Record(row)) => {
                for (name, field) in fields {
                    if let Some(got) = row.field(name) {
                        evidence(field, got, dims, span)?;
                    }
                }
                Ok(())
            },
            | _ => Ok(()),
        }
    }
    for (i, value) in args.iter().enumerate() {
        let param = sig
            .params
            .get(i)
            .or_else(|| sig.variadic.as_ref().map(|(_, _, ty)| ty));
        if let Some(param) = param {
            bind(param, value, &mut dims, span)?;
        }
    }
    if let Some(expected) = expected {
        evidence(&sig.ret, expected, &mut dims, span)?;
    }
    Ok(dims)
}

/// Restore each result component's own instantiated dimension instead of
/// assigning one dimension to an entire heterogeneous record.
fn result_with_dimensions(
    value: &Val,
    sig: &SigType,
    dims: &DimUnifier,
    span: Span,
) -> Result<Value, EvalError> {
    let resolved = |dim: &Dim| {
        let dim = dims.resolve(dim);
        if dim.is_concrete() {
            Ok(dim)
        } else {
            Err(EvalError::new(
                "E1001",
                "kernel result dimension is not determined by its arguments",
            )
            .at(span)
            .with_note("missing capability: dim.result_inference"))
        }
    };
    match (value, sig) {
        | (Val::Record(values), SigType::Record(fields)) => {
            let mut output = indexmap::IndexMap::new();
            for (name, value) in values {
                let Some((_, field)) = fields.iter().find(|(field, _)| field == name) else {
                    return Err(EvalError::new(
                        "E1004",
                        format!("kernel returned undeclared record field `{name}`"),
                    )
                    .at(span));
                };
                output.insert(
                    name.clone(),
                    result_with_dimensions(value, field, dims, span)?,
                );
            }
            if output.len() != fields.len() {
                return Err(
                    EvalError::new("E1004", "kernel omitted a declared record field").at(span),
                );
            }
            Ok(Value::Record(Rc::new(crate::value::RecordValue {
                fields: output,
            })))
        },
        | (
            Val::Tensor { shape, data },
            SigType::Tensor { elem, .. } | SigType::Sparse { elem, .. },
        ) => {
            let dim = match &**elem {
                | SigType::Dim(_, d) | SigType::Point(d) => resolved(d)?,
                | _ => Dim::one(),
            };
            let mut output = Vec::with_capacity(data.len());
            for item in data {
                output.push(result_with_dimensions(item, elem, dims, span)?);
            }
            Ok(Value::Tensor(Rc::new(crate::value::TensorData {
                verified_element: None,
                shape: shape.clone(),
                storage: crate::value::Storage::Boxed(output),
                dim,
                display: None,
            })))
        },
        | (_, SigType::Dim(_, dim) | SigType::Point(dim)) => {
            Ok(bridge::from_val(value, resolved(dim)?))
        },
        | _ => Ok(bridge::from_val(value, Dim::one())),
    }
}

/// The dimension to attach to a result.
///
/// The dimension comes from the overload the key names, not from the first
/// one: when the two differ, reading `primary()` gives the result a dimension
/// the executed overload never promised.
fn result_dim(
    entry: &OpEntry,
    key: &OpKey,
) -> Dim {
    signature_dim(entry, entry.key_index(key))
}

/// The dimension a signature states, `1` when it states none.
fn signature_dim(
    entry: &OpEntry,
    index: Option<usize>,
) -> Dim {
    let signature = index
        .and_then(|index| entry.signatures.get(index))
        .or_else(|| entry.primary());
    match signature.map(|sig| &sig.ret) {
        | Some(SigType::Dim(_, dim) | SigType::Point(dim)) => dim.clone(),
        | _ => Dim::one(),
    }
}

/// The `OpKey` a `Rssn.<name>` call with these argument types denotes.
///
/// The checker calls this at a call site: the returned key
/// is pinned into the Core IR and handed to [`call_operator`], so the overload
/// that was typed is the overload that runs. Resolution is `kernel::overload`'s
/// lattice, not a second implementation of it.
///
/// # Errors
///
/// `E1002` when several overloads tie, `E1003` when none accepts the
/// arguments.
pub fn resolve_key(
    name: &str,
    arg_types: &[SigType],
) -> Result<OpKey, aevia_rssn::KernelError> {
    aevia_rssn::resolve_key(name, arg_types)
}

/// `Rssn.typed` runs a dynamic call and validates the opaque result against a
/// concrete, non-forgeable TypeRep. The returned payload remains a kernel
/// Value; `from_value` is the explicit conversion back to an Aevia value.
fn typed_call(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    if args.len() != 3 {
        return Err(EvalError::new("E0315", "`Rssn.typed` takes three arguments").at(span));
    }
    let Value::TypeRep(expected) = &args[2] else {
        return Err(EvalError::new(
            "E0302",
            format!(
                "`Rssn.typed` expects a TypeRep, found {}",
                args[2].type_name()
            ),
        )
        .at(span));
    };
    let result = dynamic_call(args, span)?;
    if let Value::Variant(variant) = &result
        && variant.adt == aevia_types::adt::RESULT_ID
        && variant.index == 0
    {
        let Some(payload) = variant.fields.first() else {
            return Ok(err(value_error(
                "TypeMismatch",
                "kernel returned an empty Ok",
            )));
        };
        if let Err(reason) = validate_kernel_value(payload, expected) {
            return Ok(err(value_error("TypeMismatch", reason)));
        }
    }
    Ok(result)
}

/// `Rssn.dynamic(name, values)`.
///
/// The result is always a `Result`: §8.19 makes the dynamic entry point the one
/// place where a kernel failure is an ordinary value rather than a diagnostic,
/// so a caller can match on it. A *capability* gap is not such a failure: a
/// build without `core.dynamic_call` cannot run the entry point at all, and
/// says so as `E1001`.
fn dynamic_call(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let name = args
        .first()
        .map(show)
        .ok_or_else(|| EvalError::new("E0315", "`Rssn.dynamic` needs a name").at(span))?;
    registry()
        .capabilities
        .require("core.dynamic_call")
        .map_err(|failure| bridge::error(&failure).at(span))?;
    let values = dynamic_arguments(args.get(1), span)?;
    forward(&name, &values, span, false)
}

/// `Rssn.call_ho(name, values, callbacks)` without interpreter context.
/// Empty callback lists follow the ordinary dynamic path. Calls with callbacks
/// require the interpreter-aware member dispatch.
fn call_ho(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let name = args
        .first()
        .map(show)
        .ok_or_else(|| EvalError::new("E0315", "`Rssn.call_ho` needs a name").at(span))?;
    if args.len() != 3 {
        return Err(EvalError::new(
            "E0315",
            format!("`Rssn.call_ho` takes 3 arguments, found {}", args.len()),
        )
        .at(span));
    }
    let callbacks = dynamic_arguments(args.get(2), span)?;
    if !callbacks.is_empty() {
        return Err(EvalError::new(
            "E1001",
            "core.callback requires an interpreter-owned callback scope",
        )
        .at(span));
    }
    registry()
        .capabilities
        .require("core.dynamic_call")
        .map_err(|failure| bridge::error(&failure).at(span))?;
    let values = dynamic_arguments(args.get(1), span)?;
    forward(&name, &values, span, true)
}

/// Interpreter-aware callback path. The host-only `call` entry point keeps its
/// capability error because it has no interpreter to scope.
fn call_ho_with_interp(
    interp: &mut crate::eval::Interp,
    args: &[Value],
    span: Span,
    pinned: bool,
) -> Result<Value, EvalError> {
    let expected = if pinned { 4 } else { 3 };
    if args.len() != expected {
        return Err(
            EvalError::new("E0315", format!("callback call takes {expected} arguments")).at(span),
        );
    }
    let callbacks = dynamic_arguments(args.get(expected - 1), span)?;
    if callbacks.is_empty() && !pinned {
        return call_ho(args, span);
    }
    let callbacks: Vec<_> = callbacks
        .into_iter()
        .map(|value| {
            match value {
                | Value::Callback(callback) => Ok(callback),
                | other => {
                    Err(EvalError::new(
                        "E0302",
                        format!("expected Callback, found {}", other.type_name()),
                    )
                    .at(span))
                },
            }
        })
        .collect::<Result<_, _>>()?;
    let name = show(&args[0]);
    let key = if pinned {
        let hash = show(&args[1]);
        let Some(key) = registry()
            .op(&name)
            .and_then(|entry| pinned_key(entry, &hash))
        else {
            return Ok(err(value_error(
                "E1005",
                format!("no overload of `{name}` has signature {hash}"),
            )));
        };
        Some(key)
    } else {
        None
    };
    let values = dynamic_arguments(args.get(if pinned { 2 } else { 1 }), span)?;
    let mut converted = Vec::with_capacity(values.len());
    for value in &values {
        match bridge::to_val(value) {
            | Ok(value) => converted.push(value),
            | Err(error) => return Ok(err(value_error(error.code, error.message))),
        }
    }
    let witnesses: Vec<_> = values.iter().map(host_dimension_witness).collect();
    let result = crate::callback::with_scoped(interp, callbacks, |handles| {
        if let Some(key) = &key {
            aevia_rssn::kernel().call_ho_scoped_key(key, &converted, &witnesses, handles)
        } else {
            aevia_rssn::kernel().call_ho_scoped(&name, &converted, &witnesses, handles)
        }
    })
    .and_then(|result| result);
    Ok(match result {
        | Ok((payload, witness)) => {
            ok(Value::KernelValue(Rc::new(crate::value::KernelValue {
                payload,
                witness,
                host_witness: None,
            })))
        },
        | Err(error) => err(value_error(error.code, error.message)),
    })
}

/// `Rssn.register(plugin)` and its alias `Rssn.register_plugin(plugin)`.
///
/// §8.19 types the argument as `Plugin`, the handle §13.5.1 registers with
/// `Session::register(Box<dyn Plugin>)`. A kernel `Plugin` is a Rust host
/// object; Aevia source never holds one. Source plugins are published by
/// their declarations instead (§13.5.2): `@op`/`@rule` functions when their
/// module loads, and `load_plugin(path)` for a file at run time. A value that
/// reaches `Rssn.register` is therefore a type error at run time (`E0302`)
/// with that guidance, not a missing capability.
fn register(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::new(
            "E0315",
            format!("`Rssn.register` takes 1 argument, found {}", args.len()),
        )
        .at(span));
    }
    Err(EvalError::new(
        "E0302",
        "`Rssn.register` takes a host `Plugin`, which Aevia source cannot hold",
    )
    .at(span)
    .with_note(
        "publish source operators with `@op(\"name\")`/`@rule(\"name\")` (registered when \
         the module loads) or load a plugin file with `load_plugin(path)` (§13.5.2)",
    ))
}

/// The dynamic call behind `Rssn.dynamic` and a callback-free `Rssn.call_ho`.
///
/// The two differ only in which kernel entry point runs: with no callbacks,
/// `Session::call_ho` *is* `Session::call_value`. The conversion and the
/// `Result`-as-value channel are shared so the two cannot drift.
fn forward(
    name: &str,
    values: &[Value],
    _span: Span,
    _higher_order: bool,
) -> Result<Value, EvalError> {
    let mut converted = Vec::with_capacity(values.len());
    for value in values {
        match bridge::to_val(value) {
            | Ok(value) => converted.push(value),
            | Err(error) => return Ok(err(value_error(error.code, error.message))),
        }
    }
    let kernel = aevia_rssn::kernel();
    let witnesses: Vec<_> = values.iter().map(host_dimension_witness).collect();
    let result = kernel.call_with_witnesses(name, &converted, &witnesses);
    match result {
        | Ok((payload, witness)) => {
            Ok(ok(Value::KernelValue(Rc::new(crate::value::KernelValue {
                payload,
                witness,
                host_witness: None,
            }))))
        },
        | Err(failure) => Ok(err(value_error(failure.code, failure.message))),
    }
}

/// `Rssn.call_key(name, hash, values)`.
fn call_key(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let name = args
        .first()
        .map(show)
        .ok_or_else(|| EvalError::new("E0315", "`Rssn.call_key` needs a name").at(span))?;
    let hash = args.get(1).map(show).ok_or_else(|| {
        EvalError::new("E0315", "`Rssn.call_key` needs a signature hash").at(span)
    })?;
    let values = dynamic_arguments(args.get(2), span)?;
    let mut converted = Vec::with_capacity(values.len());
    for value in &values {
        converted.push(bridge::to_val(value)?);
    }
    let entry = registry().op(&name);
    // §10.0: `call_key` executes the key it is given. A key that no longer
    // names a live overload means the module was generated against a different
    // manifest, which is `E1005` — not a reason to pick another overload.
    let Some(key) = entry.and_then(|entry| pinned_key(entry, &hash)) else {
        return Ok(err(value_error(
            "E1005",
            format!("no overload of `{name}` has signature {hash}; regenerate the Rssn module"),
        )));
    };
    let Some(entry) = entry else {
        return Ok(err(value_error(
            "E1003",
            format!("rssn has no operator `{name}`"),
        )));
    };
    let kernel = aevia_rssn::kernel();
    let result = if entry.is_value_native() && entry.source.is_exact() {
        let witnesses: Vec<_> = values.iter().map(host_dimension_witness).collect();
        kernel.call_key_with_witnesses(&key, &converted, &witnesses)
    } else {
        use aevia_rssn::witness::DimensionWitness as W;
        let safe = values.iter().all(|v| {
            match host_dimension_witness(v) {
                | W::Scalar(d) => d.is_one(),
                | W::Tensor {
                    dimension: Some(d), ..
                } => d.is_one(),
                | W::Unknown => true,
                | _ => false,
            }
        });
        if !safe {
            return Ok(err(value_error(
                "E1001",
                "kernel overload cannot preserve argument dimensions",
            )));
        }
        kernel
            .call_key(&key, &converted)
            .map(|value| (value, W::Unknown))
    };
    match result {
        | Ok((payload, witness)) => {
            Ok(ok(Value::KernelValue(Rc::new(crate::value::KernelValue {
                payload,
                witness,
                host_witness: None,
            }))))
        },
        | Err(error) => Ok(err(value_error(error.code, error.message))),
    }
}

/// `Rssn.invoke(name, hash, values)`: the call behind a generated
/// declaration.
///
/// It differs from [`call_key`] only in the error channel: a generated `fn`
/// must return the declared type, so a kernel failure raises instead of
/// returning `Err`.
fn invoke(
    args: &[Value],
    span: Span,
    expected: Option<&aevia_types::Type>,
) -> Result<Value, EvalError> {
    let name = args
        .first()
        .map(show)
        .ok_or_else(|| EvalError::new("E0315", "`Rssn.invoke` needs a name").at(span))?;
    let hash = args
        .get(1)
        .map(show)
        .ok_or_else(|| EvalError::new("E0315", "`Rssn.invoke` needs a signature hash").at(span))?;
    let values = dynamic_arguments(args.get(2), span)?;
    let mut converted = Vec::with_capacity(values.len());
    for value in &values {
        converted.push(bridge::to_val(value)?);
    }
    let entry = registry().op(&name);
    let Some(key) = entry.and_then(|entry| pinned_key(entry, &hash)) else {
        return Err(EvalError::new(
            "E1005",
            format!("no overload of `{name}` has signature {hash}"),
        )
        .at(span)
        .with_note("regenerate the Rssn module against the current manifest"));
    };
    let Some(entry) = entry else {
        return Err(EvalError::new("E1003", format!("rssn has no operator `{name}`")).at(span));
    };
    execute_checked(&key, entry, &values, converted, span, expected)
}

/// The key of the overload of `entry` whose signature hash is `hash`.
fn pinned_key(
    entry: &OpEntry,
    hash: &str,
) -> Option<OpKey> {
    entry
        .keys
        .iter()
        .find(|key| hex(&key.signature_hash) == hash)
        .cloned()
}

fn resolve(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let name = args
        .first()
        .map(show)
        .ok_or_else(|| EvalError::new("E0315", "`Rssn.resolve_overload` needs a name").at(span))?;
    let values = dynamic_arguments(args.get(1), span)?;
    let mut converted = Vec::with_capacity(values.len());
    for value in &values {
        converted.push(bridge::to_val(value)?);
    }
    let kernel = aevia_rssn::kernel();
    match kernel.resolve(&name, &converted) {
        | Ok(key) => {
            Ok(ok(Value::string(format!(
                "{}.{}@{}",
                key.rule_set,
                key.name,
                hex(&key.signature_hash)
            ))))
        },
        | Err(failure) => Ok(err(Value::string(failure.message))),
    }
}

fn overloads(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let name = args
        .first()
        .map(show)
        .ok_or_else(|| EvalError::new("E0315", "`Rssn.overloads` needs a name").at(span))?;
    let Some(entry) = registry().op(&name) else {
        return Ok(Value::Tensor(Rc::new(crate::value::TensorData {
            verified_element: None,
            shape: vec![0],
            storage: crate::value::Storage::Boxed(Vec::new()),
            dim: Dim::one(),
            display: None,
        })));
    };
    let items: Vec<Value> = entry
        .signatures
        .iter()
        .map(|signature| Value::string(format!("{} :: {}", name, signature.render())))
        .collect();
    let len = items.len();
    Ok(Value::Tensor(Rc::new(crate::value::TensorData {
        verified_element: None,
        shape: vec![len],
        storage: crate::value::Storage::Boxed(items),
        dim: Dim::one(),
        display: None,
    })))
}

/// The argument list of a dynamic call.
fn dynamic_arguments(
    value: Option<&Value>,
    span: Span,
) -> Result<Vec<Value>, EvalError> {
    match value {
        | Some(Value::Tensor(tensor)) => {
            Ok((0..tensor.storage.len())
                .filter_map(|index| tensor.get_flat(index))
                .collect())
        },
        | Some(Value::Unit) | None => Ok(Vec::new()),
        | Some(other) => {
            Err(EvalError::new(
                "E0302",
                format!("expected a list of values, found {}", other.type_name()),
            )
            .at(span))
        },
    }
}

fn string_tensor(items: Vec<String>) -> Value {
    let len = items.len();
    Value::Tensor(Rc::new(crate::value::TensorData {
        verified_element: None,
        shape: vec![len],
        storage: crate::value::Storage::Boxed(items.into_iter().map(Value::string).collect()),
        dim: Dim::one(),
        display: None,
    }))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn ok(value: Value) -> Value {
    crate::value::make_variant(aevia_types::adt::RESULT_ID, 0, "Ok", vec![value])
}

fn err(value: Value) -> Value {
    crate::value::make_variant(aevia_types::adt::RESULT_ID, 1, "Err", vec![value])
}

/// The members that are data rather than operators.
const DATA_MEMBERS: [&str; 6] = [
    "Version",
    "ManifestHash",
    "Capabilities",
    "Units",
    "Constants",
    "ValueKinds",
];

/// Whether `name` is data rather than a callable.
#[must_use]
pub fn is_data_member(name: &str) -> bool {
    DATA_MEMBERS.contains(&name)
}

/// A `Rssn.<name>` value.
///
/// Data members evaluate now; everything else becomes a callable, so
/// `Rssn.sin` can be passed to `map` without being applied.
#[must_use]
pub fn member(name: &str) -> Value {
    if is_data_member(name) {
        return call(name, Vec::new(), Span::default()).unwrap_or(Value::Error);
    }
    let owned = name.to_string();
    Value::Native(Rc::new(NativeFn(Box::new(move |interp, args, span| {
        if owned == "invoke" {
            let expected = interp.resolved_expr_type(span);
            invoke(&args, span, expected.as_ref())
        } else if owned == "call_ho" || owned == "call_ho_key" {
            call_ho_with_interp(interp, &args, span, owned == "call_ho_key")
        } else {
            call(&owned, args, span)
        }
    }))))
}

#[cfg(test)]
mod dimension_contract_tests {
    use self::aevia_core_test_values::number;
    use super::*;

    // Keep the values used in this test distinct: the metadata says only the
    // singular-value field carries the input dimension.
    mod aevia_core_test_values {
        use super::*;
        pub fn number(
            value: f64,
            dim: Dim,
        ) -> Value {
            Value::Num(crate::value::NumVal::float(value, dim))
        }
    }

    #[test]
    fn record_fields_restore_their_own_instantiated_dimensions() {
        let d = Dim::var(2_000_001);
        let dimensioned = SigType::Dim(Box::new(SigType::Float), d.clone());
        let sig = aevia_rssn::Sig {
            params: vec![dimensioned.clone()],
            ret: SigType::Record(vec![
                ("u".into(), SigType::Float),
                ("s".into(), dimensioned),
            ]),
            variadic: None,
        };
        let actual = number(3.0, Dim::base("Length"));
        let dims = bind_dimensions(&sig, &[actual], None, Span::default()).unwrap();
        let raw = Val::Record(vec![
            ("u".into(), Val::Num(aevia_rssn::Num::Float(1.0))),
            ("s".into(), Val::Num(aevia_rssn::Num::Float(3.0))),
        ]);
        let Value::Record(result) =
            result_with_dimensions(&raw, &sig.ret, &dims, Span::default()).unwrap()
        else {
            panic!("record")
        };
        assert_eq!(crate::value::value_dim(&result.fields["u"]), Dim::one());
        assert_eq!(
            crate::value::value_dim(&result.fields["s"]),
            Dim::base("Length")
        );
    }
    #[test]
    fn explicit_kernel_witness_cannot_be_overwritten_by_the_expected_dimension() {
        use aevia_rssn::witness::DimensionWitness as W;
        let d = Dim::var(2_000_002);
        let sig = aevia_rssn::Sig {
            params: vec![SigType::Dim(Box::new(SigType::Float), d.clone())],
            ret: SigType::Dim(Box::new(SigType::Float), d),
            variadic: None,
        };
        let dims = bind_dimensions(
            &sig,
            &[number(1.0, Dim::base("Length"))],
            None,
            Span::default(),
        )
        .unwrap();
        let wrong = verify_result_witness(
            &sig.ret,
            &W::Scalar(Dim::base("Time")),
            &dims,
            Span::default(),
        )
        .unwrap_err();
        assert_eq!(wrong.code, "E0412");
        let unknown =
            verify_result_witness(&sig.ret, &W::Unknown, &dims, Span::default()).unwrap_err();
        assert_eq!(unknown.code, "E1001");
    }
}

/// Builds an opaque ABI error without losing its original code.
pub(crate) fn value_error(
    code: &str,
    message: impl Into<String>,
) -> Value {
    Value::KernelError(Rc::new(crate::value::KernelErrorValue {
        code: code.to_string(),
        message: message.into(),
    }))
}

/// Validates opaque evidence without materializing an Aevia copy of the payload.
pub(crate) fn validate_kernel_value(
    value: &Value,
    expected: &aevia_types::Type,
) -> Result<(), String> {
    let Value::KernelValue(value) = value else {
        return Err("expected an opaque kernel Value".into());
    };
    validate_payload(
        &value.payload,
        &value.witness,
        value.host_witness.as_deref(),
        expected,
    )
}

/// Converts only after the shared borrowed validator has established the contract.
pub(crate) fn from_kernel_value(
    value: &Value,
    expected: &aevia_types::Type,
) -> Result<Value, String> {
    validate_kernel_value(value, expected)?;
    let Value::KernelValue(value) = value else {
        unreachable!()
    };
    decode_payload(&value.payload, value.host_witness.as_deref(), expected)
}

fn validate_payload(
    payload: &Val,
    witness: &aevia_rssn::witness::DimensionWitness,
    host: Option<&crate::value::HostWitness>,
    expected: &aevia_types::Type,
) -> Result<(), String> {
    use crate::value::HostWitnessNode as H;
    use aevia_rssn::witness::DimensionWitness as W;
    use aevia_types::Rep;
    use aevia_types::Row;
    use aevia_types::Shape;
    use aevia_types::Type;
    let mismatch = || {
        // A graph-backed operator that cannot reduce its arguments (`invmod(6, 15)`:
        // no inverse) answers with the unreduced call. Say that, and show it,
        // instead of a bare "does not prove": the term *is* the diagnosis.
        if let Val::Term(sym) = payload
            && !matches!(expected, aevia_types::Type::Scalar(Rep::Real, _))
            && let Ok(text) = aevia_rssn::kernel().try_text(*sym)
        {
            return format!(
                "kernel value does not prove expected type {expected}: the operator did not reduce and returned the term `{text}` (it is undefined for these arguments)"
            );
        }
        format!("kernel value does not prove expected type {expected}")
    };
    if host.is_some_and(|h| !crate::host_witness::schema_compatible(&h.schema, expected)) {
        return Err(mismatch());
    }
    let host_scalar = host.is_some_and(|h| matches!(h.node, H::Scalar));
    let proves_dim = |dim: &Dim| {
        dim.is_concrete()
            && match witness {
                | W::Scalar(actual) => actual == dim,
                | W::Unknown => host_scalar,
                | _ => false,
            }
    };
    match (payload, expected) {
        | (Val::Num(n), Type::Scalar(rep, dim)) => {
            let representation = matches!(
                (n, rep),
                (aevia_rssn::Num::Int(_), Rep::Int | Rep::Real)
                    | (aevia_rssn::Num::Rat(_), Rep::Real)
                    | (aevia_rssn::Num::Float(_), Rep::Float)
            );
            if representation && proves_dim(dim) {
                Ok(())
            } else {
                Err(mismatch())
            }
        },
        | (Val::Symbol(_) | Val::Term(_), Type::Scalar(Rep::Real, dim))
        | (Val::Complex { .. }, Type::Scalar(Rep::Complex, dim))
            if proves_dim(dim) =>
        {
            Ok(())
        },
        | (Val::Bool(_), Type::Bool) | (Val::Str(_), Type::Str) => Ok(()),
        | (Val::Variant { tag, payload }, Type::Unit) if tag == "Unit" && payload.is_empty() => {
            Ok(())
        },
        | (Val::Variant { tag, payload }, Type::Adt(expected_id, _)) => {
            let Some(crate::value::HostWitness {
                node:
                    H::Variant {
                        adt,
                        index,
                        tag: proven_tag,
                        fields,
                    },
                ..
            }) = host
            else {
                return Err(mismatch());
            };
            if !aevia_types::adt::same_type(*adt, *expected_id)
                || tag != proven_tag
                || payload.len() != fields.len()
            {
                return Err(mismatch());
            }
            // The private host witness was minted only after this exact source
            // constructor's immutable schema, index and arity were checked.
            let _ = index;
            for (item, field) in payload.iter().zip(fields) {
                validate_payload(item, &W::Unknown, Some(field), &field.schema)?;
            }
            Ok(())
        },
        | (
            Val::Tensor { shape, data },
            Type::Tensor {
                shape: expected_shape,
                elem,
            },
        ) => {
            if shape.len() != expected_shape.rank() as usize
                || shape
                    .iter()
                    .try_fold(1usize, |n, extent| n.checked_mul(*extent))
                    != Some(data.len())
            {
                return Err(mismatch());
            }
            if let Shape::Refined(extents) = expected_shape {
                for (actual, extent) in shape.iter().zip(extents) {
                    if let Some(extent) = extent
                        && extent.as_const() != Some(*actual)
                    {
                        return Err(mismatch());
                    }
                }
            }
            let W::Tensor {
                dimension,
                element,
                element_repr,
                elements,
            } = witness
            else {
                return Err(mismatch());
            };
            let host_items = match host.map(|h| &h.node) {
                | Some(H::Tensor(items)) if items.len() == data.len() => Some(items),
                | Some(H::Tensor(items))
                    if items.is_empty() && crate::host_witness::is_leaf_schema(elem) =>
                {
                    None
                },
                | Some(_) => return Err(mismatch()),
                | None => None,
            };
            if data.is_empty() {
                let valid_repr = matches!(
                    (elem.as_ref(), element_repr),
                    (Type::Scalar(Rep::Int, _), Some(SigType::Int))
                        | (
                            Type::Scalar(Rep::Real, _),
                            Some(SigType::Int | SigType::Real)
                        )
                        | (Type::Scalar(Rep::Float, _), Some(SigType::Float))
                        | (Type::Scalar(Rep::Complex, _), Some(SigType::Complex))
                        | (Type::Bool, Some(SigType::Bool))
                        | (Type::Str, Some(SigType::Str))
                );
                if !valid_repr && host_items.is_none() {
                    let proven = element_repr
                        .as_ref()
                        .zip(element.as_deref())
                        .is_some_and(|(repr, witness)| empty_schema_matches(elem, repr, witness));
                    if !proven {
                        return Err(mismatch());
                    }
                }
                if let Type::Scalar(_, dim) = elem.as_ref()
                    && (!dim.is_concrete() || dimension.as_ref() != Some(dim))
                {
                    return Err(mismatch());
                }
                return Ok(());
            }
            if let Type::Scalar(_, dim) = elem.as_ref() {
                if !dim.is_concrete()
                    || dimension.as_ref() != Some(dim)
                    || elements
                        .iter()
                        .any(|w| !matches!(w, W::Scalar(actual) if actual == dim))
                {
                    return Err(mismatch());
                }
                let scalar = W::Scalar(dim.clone());
                for (i, item) in data.iter().enumerate() {
                    validate_payload(
                        item,
                        &scalar,
                        host_items.and_then(|items| items.get(i)),
                        elem,
                    )?;
                }
            } else {
                if !elements.is_empty() && elements.len() != data.len() {
                    return Err(mismatch());
                }
                for (i, item) in data.iter().enumerate() {
                    let item_witness = elements
                        .get(i)
                        .or(element.as_deref())
                        .unwrap_or(&W::Unknown);
                    validate_payload(
                        item,
                        item_witness,
                        host_items.and_then(|items| items.get(i)),
                        elem,
                    )?;
                }
            }
            Ok(())
        },
        | (Val::Record(fields), Type::Record(row)) => {
            let W::Record(witnesses) = witness else {
                return Err(mismatch());
            };
            let host_fields = match host.map(|h| &h.node) {
                | Some(H::Record(fields)) => Some(fields),
                | Some(_) => return Err(mismatch()),
                | None => None,
            };
            let mut row = row;
            let mut count = 0;
            loop {
                match row {
                    | Row::Empty => break,
                    | Row::Var(_) => return Err(mismatch()),
                    | Row::Extend { label, ty, tail } => {
                        if fields.iter().filter(|(n, _)| n == label).count() != 1
                            || witnesses.iter().filter(|(n, _)| n == label).count() != 1
                        {
                            return Err(mismatch());
                        }
                        let (_, item) = fields
                            .iter()
                            .find(|(n, _)| n == label)
                            .ok_or_else(mismatch)?;
                        let (_, w) = witnesses
                            .iter()
                            .find(|(n, _)| n == label)
                            .ok_or_else(mismatch)?;
                        let h = match host_fields {
                            | Some(fs) => {
                                Some(&fs.iter().find(|(n, _)| n == label).ok_or_else(mismatch)?.1)
                            },
                            | None => None,
                        };
                        validate_payload(item, w, h, ty)?;
                        count += 1;
                        row = tail;
                    },
                }
            }
            if count != fields.len()
                || count != witnesses.len()
                || host_fields.is_some_and(|fs| fs.len() != count)
            {
                return Err(mismatch());
            }
            Ok(())
        },
        | _ => Err(mismatch()),
    }
}

fn decode_payload(
    payload: &Val,
    host: Option<&crate::value::HostWitness>,
    expected: &aevia_types::Type,
) -> Result<Value, String> {
    use crate::value::HostWitnessNode as H;
    use aevia_types::Rep;
    use aevia_types::Row;
    use aevia_types::Type;
    match (payload, expected) {
        | (Val::Num(aevia_rssn::Num::Int(n)), Type::Scalar(Rep::Real, dim)) => {
            Ok(Value::Num(crate::value::NumVal::exact(
                num_rational::BigRational::from_integer(n.clone()),
                dim.clone(),
            )))
        },
        | (Val::Num(aevia_rssn::Num::Rat(n)), Type::Scalar(Rep::Real, dim)) => {
            Ok(Value::Num(crate::value::NumVal::exact(
                n.clone(),
                dim.clone(),
            )))
        },
        | (_, Type::Scalar(_, dim)) => Ok(bridge::from_val(payload, dim.clone())),
        | (Val::Tensor { shape, data }, Type::Tensor { elem, .. }) => {
            let dim = match elem.as_ref() {
                | Type::Scalar(_, dim) => dim.clone(),
                | _ => Dim::one(),
            };
            let host_items = match host.map(|h| &h.node) {
                | Some(H::Tensor(items)) => Some(items),
                | _ => None,
            };
            let storage = if data.is_empty() {
                match elem.as_ref() {
                    | Type::Scalar(Rep::Float, _) => {
                        crate::value::Storage::Dense(Rc::new(Vec::new()))
                    },
                    | Type::Scalar(Rep::Int, _) => {
                        crate::value::Storage::TypedExact(Vec::new(), crate::value::ExactRep::Int)
                    },
                    | Type::Scalar(Rep::Real, _) => {
                        crate::value::Storage::TypedExact(Vec::new(), crate::value::ExactRep::Real)
                    },
                    | Type::Bool => crate::value::Storage::Bool(Vec::new()),
                    | _ => crate::value::Storage::Boxed(Vec::new()),
                }
            } else {
                crate::value::Storage::Boxed(
                    data.iter()
                        .enumerate()
                        .map(|(i, item)| {
                            decode_payload(item, host_items.and_then(|items| items.get(i)), elem)
                        })
                        .collect::<Result<_, _>>()?,
                )
            };
            Ok(Value::Tensor(Rc::new(crate::value::TensorData {
                // decode_payload is reached only after the shared validator
                // has authenticated both payload and the element schema.
                verified_element: Some(Rc::new((**elem).clone())),
                shape: shape.clone(),
                storage,
                dim,
                display: None,
            })))
        },
        | (Val::Record(fields), Type::Record(row)) => {
            let host_fields = match host.map(|h| &h.node) {
                | Some(H::Record(fields)) => Some(fields),
                | _ => None,
            };
            let mut row = row;
            let mut result = indexmap::IndexMap::new();
            while let Row::Extend { label, ty, tail } = row {
                let (_, item) = fields
                    .iter()
                    .find(|(n, _)| n == label)
                    .ok_or_else(|| "validated field disappeared".to_string())?;
                let h =
                    host_fields.and_then(|fs| fs.iter().find(|(n, _)| n == label).map(|(_, h)| h));
                result.insert(label.clone(), decode_payload(item, h, ty)?);
                row = tail;
            }
            Ok(Value::Record(Rc::new(crate::value::RecordValue {
                fields: result,
            })))
        },
        | (Val::Variant { tag, payload }, Type::Adt(id, args)) => {
            let Some(crate::value::HostWitness {
                node: H::Variant { index, fields, .. },
                ..
            }) = host
            else {
                return Err("missing nominal witness".into());
            };
            let values = payload
                .iter()
                .zip(fields)
                .map(|(item, field)| decode_payload(item, Some(field), &field.schema))
                .collect::<Result<Vec<_>, _>>()?;
            let mut result = crate::value::make_variant(*id, *index, tag, values);
            if let Value::Variant(variant) = &mut result {
                let variant = Rc::make_mut(variant);
                variant.constructor_origin = true;
                variant.verified_type = Some(expected.clone());
                variant.type_args = Some(args.clone());
            }
            Ok(result)
        },
        | (Val::Variant { .. }, Type::Unit) => Ok(Value::Unit),
        | _ => Ok(bridge::from_val(payload, Dim::one())),
    }
}

#[cfg(test)]
mod opaque_boundary_tests {
    use super::*;
    use aevia_rssn::witness::DimensionWitness as W;
    use aevia_types::Rep;
    use aevia_types::Shape;
    use aevia_types::Type;
    fn wrapped(
        payload: Val,
        witness: W,
    ) -> Value {
        Value::KernelValue(Rc::new(crate::value::KernelValue {
            payload,
            witness,
            host_witness: None,
        }))
    }
    #[test]
    fn conversion_requires_dimension_and_representation_evidence() {
        let payload = Val::Num(aevia_rssn::Num::Float(2.0));
        let length = Dim::base("Length");
        assert!(
            from_kernel_value(
                &wrapped(payload.clone(), W::Unknown),
                &Type::float(length.clone())
            )
            .is_err()
        );
        let value = wrapped(payload, W::Scalar(length.clone()));
        assert!(from_kernel_value(&value, &Type::float(Dim::one())).is_err());
        assert!(from_kernel_value(&value, &Type::Scalar(Rep::Real, length.clone())).is_err());
        let value = from_kernel_value(&value, &Type::float(length.clone())).unwrap();
        assert_eq!(crate::value::value_dim(&value), length);
    }
    #[test]
    fn malformed_shapes_and_duplicate_fields_never_validate() {
        let witness = W::Tensor {
            dimension: Some(Dim::one()),
            element: None,
            element_repr: Some(SigType::Float),
            elements: vec![],
        };
        let malformed = wrapped(
            Val::Tensor {
                shape: vec![2],
                data: vec![Val::Num(aevia_rssn::Num::Float(1.0))],
            },
            witness.clone(),
        );
        assert!(
            validate_kernel_value(
                &malformed,
                &Type::tensor(Shape::Rank(1), Type::float(Dim::one()))
            )
            .is_err()
        );
        let overflow = wrapped(
            Val::Tensor {
                shape: vec![usize::MAX, 2],
                data: vec![],
            },
            witness,
        );
        assert!(
            validate_kernel_value(
                &overflow,
                &Type::tensor(Shape::Rank(2), Type::float(Dim::one()))
            )
            .is_err()
        );
        let duplicate = wrapped(
            Val::Record(vec![
                ("x".into(), Val::Bool(true)),
                ("x".into(), Val::Bool(false)),
            ]),
            W::Record(vec![("x".into(), W::Unknown)]),
        );
        assert!(
            validate_kernel_value(
                &duplicate,
                &Type::Record(aevia_types::Row::closed([("x".into(), Type::Bool)]))
            )
            .is_err()
        );
    }
    #[test]
    fn real_conversion_promotes_integer_payload_to_exact_representation() {
        let value = wrapped(
            Val::Num(aevia_rssn::Num::Int(7.into())),
            W::Scalar(Dim::one()),
        );
        let result = from_kernel_value(&value, &Type::real(Dim::one())).unwrap();
        assert!(matches!(
            result,
            Value::Num(crate::value::NumVal {
                kind: crate::value::NumKind::Exact(_),
                ..
            })
        ));
    }
    #[test]
    fn empty_tensor_requires_actual_element_schema() {
        let payload = Val::Tensor {
            shape: vec![0],
            data: vec![],
        };
        let witness = |element_repr| {
            W::Tensor {
                dimension: Some(Dim::one()),
                element: None,
                element_repr,
                elements: vec![],
            }
        };
        let expected = Type::tensor(Shape::Rank(1), Type::float(Dim::one()));
        assert!(from_kernel_value(&wrapped(payload.clone(), witness(None)), &expected).is_err());
        assert!(
            from_kernel_value(
                &wrapped(payload.clone(), witness(Some(SigType::Int))),
                &expected
            )
            .is_err()
        );
        assert!(
            from_kernel_value(&wrapped(payload, witness(Some(SigType::Float))), &expected).is_ok()
        );
    }
}
