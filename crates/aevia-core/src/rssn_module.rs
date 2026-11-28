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
use aevia_rssn::kernel::NativeArgument;
use aevia_rssn::registry;
use aevia_rssn::witness::DimensionWitness;
use aevia_syntax::Span;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

const RSSN_SCRATCH_POOL_CAP: usize = 8;
const RSSN_SCRATCH_ELEMS_CAP: usize = 128;

std::thread_local! {
    static WITNESS_SCRATCH_POOL: RefCell<Vec<Vec<DimensionWitness>>> = const { RefCell::new(Vec::new()) };
    static VAL_SCRATCH_POOL: RefCell<Vec<Vec<Val>>> = const { RefCell::new(Vec::new()) };
}

struct WitnessScratchGuard(Vec<DimensionWitness>);

impl WitnessScratchGuard {
    fn with_capacity(min_capacity: usize) -> Self {
        let mut buf = WITNESS_SCRATCH_POOL
            .try_with(|pool| pool.try_borrow_mut().ok().and_then(|mut guard| guard.pop()))
            .ok()
            .flatten()
            .unwrap_or_default();
        buf.clear();
        buf.reserve(min_capacity);
        Self(buf)
    }
}

impl Drop for WitnessScratchGuard {
    fn drop(&mut self) {
        let mut buf = std::mem::take(&mut self.0);
        buf.clear();
        if buf.capacity() > 0 && buf.capacity() <= RSSN_SCRATCH_ELEMS_CAP {
            let _ = WITNESS_SCRATCH_POOL.try_with(|pool| {
                if let Ok(mut guard) = pool.try_borrow_mut()
                    && guard.len() < RSSN_SCRATCH_POOL_CAP
                {
                    guard.push(buf);
                }
            });
        }
    }
}

struct ValScratchGuard(Vec<Val>);

impl ValScratchGuard {
    fn with_capacity(min_capacity: usize) -> Self {
        let mut buf = VAL_SCRATCH_POOL
            .try_with(|pool| pool.try_borrow_mut().ok().and_then(|mut guard| guard.pop()))
            .ok()
            .flatten()
            .unwrap_or_default();
        buf.clear();
        buf.reserve(min_capacity);
        Self(buf)
    }
}

impl Drop for ValScratchGuard {
    fn drop(&mut self) {
        let mut buf = std::mem::take(&mut self.0);
        buf.clear();
        if buf.capacity() > 0 && buf.capacity() <= RSSN_SCRATCH_ELEMS_CAP {
            let _ = VAL_SCRATCH_POOL.try_with(|pool| {
                if let Ok(mut guard) = pool.try_borrow_mut()
                    && guard.len() < RSSN_SCRATCH_POOL_CAP
                {
                    guard.push(buf);
                }
            });
        }
    }
}

fn fill_host_witnesses(
    values: &[Value],
    out: &mut Vec<DimensionWitness>,
) {
    out.clear();
    out.reserve(values.len());
    out.extend(values.iter().map(host_dimension_witness));
}

fn fill_neutral_vals(
    values: &[Value],
    out: &mut Vec<Val>,
) -> Result<(), EvalError> {
    out.clear();
    out.reserve(values.len());
    for value in values {
        out.push(bridge::to_val(value)?);
    }
    Ok(())
}

/// Calls `Rssn.<name>`.
///
/// # Errors
///
/// `E1003` when the member is neither a fixed entry point nor a registered
/// operator, plus whatever the kernel reports.
#[allow(
    clippy::needless_pass_by_value,
    reason = "retains the public host call interface with owned arguments"
)]
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

    #[test]
    fn record_validation_rejects_repeated_expected_fields() {
        let row = |tail| {
            Row::Extend {
                label: "first".into(),
                ty: Box::new(Type::Bool),
                tail: Box::new(tail),
            }
        };
        let expected = Type::Record(row(row(Row::Empty)));
        let payload = Val::Record(vec![
            ("first".into(), Val::Bool(true)),
            ("other".into(), Val::Bool(false)),
        ]);
        let witness = W::Record(vec![
            ("first".into(), W::Unknown),
            ("other".into(), W::Unknown),
        ]);
        assert!(validate_payload(&payload, &witness, None, &expected).is_err());
    }

    #[test]
    fn opaque_list_projection_checks_tensor_storage_and_expected_length() {
        let expected = Type::tensor(Shape::Rank(1), aevia_types::builtins::dynamic_value_type());
        let tensor = |shape, data| {
            Value::KernelValue(Rc::new(crate::value::KernelValue {
                payload: Val::Tensor { shape, data },
                witness: W::Unknown,
                host_witness: None,
            }))
        };
        assert!(from_kernel_value(&tensor(vec![2], vec![Val::Bool(true)]), &expected).is_err());
        assert!(from_kernel_value(&tensor(vec![1, usize::MAX, 2], vec![]), &expected).is_err());
        let valid = tensor(vec![2, 0], vec![]);
        let Value::Tensor(projected) = from_kernel_value(&valid, &expected).unwrap() else {
            panic!("expected an opaque list");
        };
        assert_eq!(projected.storage.len(), 2);
        let wrong_length = Type::tensor(
            Shape::refined([Some(1)]),
            aevia_types::builtins::dynamic_value_type(),
        );
        assert!(from_kernel_value(&valid, &wrong_length).is_err());
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
    call_operator_with_evidence(key, args, span, None)
}

/// Executes a pinned call with its checked result type from the interpreter.
pub(crate) fn call_operator_with_evidence(
    key: &OpKey,
    mut args: Vec<Value>,
    span: Span,
    expected: Option<&aevia_types::Type>,
) -> Result<Value, EvalError> {
    let Some(entry) = registry().op(&key.name) else {
        return Err(
            EvalError::new("E1003", format!("rssn has no operator `{}`", key.name)).at(span),
        );
    };
    if !entry.accepts(args.len()) {
        return Err(EvalError::new(
            "E0315",
            format!(
                "`{}` takes {} argument(s), found {}",
                key.name,
                entry.arity.0,
                args.len()
            ),
        )
        .at(span));
    }
    // Checked variadic callers pack the final argument as a rank-one list.
    // Flatten once so the bridge and dimension witnesses see the same inputs.
    if entry.is_variadic()
        && matches!(args.last(), Some(Value::Tensor(tensor)) if tensor.rank() == 1)
        && let Some(last) = args.pop()
    {
        match dynamic_arguments(Some(&last), span)? {
            | Cow::Borrowed(items) => args.extend(items.iter().cloned()),
            | Cow::Owned(items) => args.extend(items),
        }
    }
    let converted = prepare_checked_arguments(&args, Some(entry))?;
    execute_checked(key, entry, &args, &converted, span, expected)
}

enum PreparedArguments<'a> {
    Native(Vec<NativeArgument<'a>>),
    Neutral(ValScratchGuard),
}

fn prepare_native_arguments(args: &[Value]) -> Result<Vec<NativeArgument<'_>>, EvalError> {
    let mut converted = Vec::with_capacity(args.len());
    for arg in args {
        if let Value::Tensor(tensor) = arg {
            if let Some(data) = tensor.dense_flat()
                && let Some(borrowed) = NativeArgument::dense_float_cow(&tensor.shape, data)
            {
                converted.push(borrowed);
                continue;
            }
            if let crate::value::Storage::Bool(items) = &tensor.storage
                && tensor.checked_element_count() == Some(items.len())
                && let Some(borrowed) = NativeArgument::dense_bool(&tensor.shape, items)
            {
                converted.push(borrowed);
                continue;
            }
        }
        converted.push(NativeArgument::from(bridge::to_val(arg)?));
    }
    Ok(converted)
}

fn prepare_checked_arguments<'a>(
    args: &'a [Value],
    entry: Option<&OpEntry>,
) -> Result<PreparedArguments<'a>, EvalError> {
    if entry.is_some_and(|entry| entry.is_value_native() && entry.source.is_exact()) {
        return prepare_native_arguments(args).map(PreparedArguments::Native);
    }
    let mut neutral = ValScratchGuard::with_capacity(args.len());
    fill_neutral_vals(args, &mut neutral.0)?;
    Ok(PreparedArguments::Neutral(neutral))
}

/// Executes a resolved overload on converted arguments.
fn execute_checked(
    key: &OpKey,
    entry: &OpEntry,
    args: &[Value],
    converted: &PreparedArguments<'_>,
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
    let kernel = aevia_rssn::kernel();
    let (value, returned_witness) = match converted {
        | PreparedArguments::Native(converted) => {
            let mut inputs = WitnessScratchGuard::with_capacity(args.len());
            fill_host_witnesses(args, &mut inputs.0);
            kernel
                .call_key_with_native_arguments(key, converted, &inputs.0)
                .map(|(value, witness)| (value, Some(witness)))
        },
        | PreparedArguments::Neutral(converted) => {
            kernel
                .call_key(key, &converted.0)
                .map(|value| (value, None))
        },
    }
    .map_err(|failure| bridge::error(&failure).at(span))?;
    if let (Some(sig), Some(dimensions)) = (signature, dimensions) {
        if let Some(witness) = &returned_witness {
            verify_result_witness(&sig.ret, witness, &dimensions, span)?;
        }
        return result_with_dimensions(&value, &sig.ret, &dimensions, span);
    }
    let signature = signature.or_else(|| entry.primary());
    let dim = match signature.map(|sig| &sig.ret) {
        | Some(SigType::Dim(_, dim) | SigType::Point(dim)) => dim.clone(),
        | _ => Dim::one(),
    };
    Ok(bridge::from_val(&value, dim))
}

/// The source value's dimension is evidence, including for an empty tensor.
pub(crate) fn host_dimension_witness(value: &Value) -> aevia_rssn::witness::DimensionWitness {
    use crate::value::Storage;
    use aevia_rssn::witness::DimensionWitness as W;
    match value {
        | Value::KernelValue(value) => value.witness.clone(),
        | Value::Num(_) | Value::Dyn(_) => W::Scalar(crate::value::value_dim(value)),
        | Value::Tensor(tensor) => {
            // A primitive buffer cannot contain nested witnesses. Walking it
            // would only materialize scalar Values, including copied rationals.
            let (mut element_repr, may_nest) = match tensor.storage.backing() {
                | Storage::Dense(_) | Storage::View(_) => (Some(SigType::Float), false),
                | Storage::TypedExact(_, crate::value::ExactRep::Int) => {
                    (Some(SigType::Int), false)
                },
                | Storage::TypedExact(_, crate::value::ExactRep::Real) => {
                    (Some(SigType::Real), false)
                },
                | Storage::Bool(_) => (Some(SigType::Bool), false),
                | Storage::Exact(_) => (None, false),
                | _ => (None, true),
            };
            let is_nested = |item: &Value| {
                matches!(
                    item,
                    Value::Tensor(_)
                        | Value::Record(_)
                        | Value::KernelValue(_)
                        | Value::Dyn(_)
                        | Value::Variant(_)
                )
            };
            let elements = match &tensor.storage {
                | Storage::Boxed(items) => {
                    if items.iter().any(is_nested) {
                        items.iter().map(host_dimension_witness).collect()
                    } else {
                        Vec::new()
                    }
                },
                | _ if may_nest
                    && (0..tensor.storage.len())
                        .any(|i| tensor.get_flat(i).as_ref().is_some_and(is_nested)) =>
                {
                    // Logical windows can change the metadata on a scalar.
                    // Read only their own elements when building the witness.
                    (0..tensor.storage.len())
                        .filter_map(|i| tensor.get_flat(i))
                        .map(|item| host_dimension_witness(&item))
                        .collect()
                },
                | _ => Vec::new(),
            };
            // Boxed storage does not establish a uniform representation.
            // Reuse the verified element schema after checking shape and storage.
            let mut element = None;
            if (element_repr.is_none() || tensor.storage.is_empty())
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
        | Value::Variant(value) if value.adt == aevia_types::adt::OPTION_ID => {
            W::Variant {
                tag: value.name.clone(),
                fields: value.fields.iter().map(host_dimension_witness).collect(),
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

#[cfg(test)]
mod tensor_witness_storage_tests {
    use super::*;
    use crate::value::ExactRep;
    use crate::value::RecordValue;
    use crate::value::Storage;
    use crate::value::TensorData;
    use aevia_rssn::witness::DimensionWitness as W;
    use aevia_types::Type;
    use num_rational::BigRational;

    fn uniform(
        dim: &Dim,
        repr: Option<SigType>,
    ) -> W {
        W::Tensor {
            dimension: Some(dim.clone()),
            element: None,
            element_repr: repr,
            elements: Vec::new(),
        }
    }

    #[test]
    fn primitive_tensors_and_slices_keep_storage_representation() {
        let length = Dim::base("Length");
        let dense = TensorData::new(
            vec![2, 2],
            Storage::Dense(Rc::new(vec![0.0, 1.0, 2.0, 3.0])),
            length.clone(),
            None,
        );
        let reversed = dense.transposed_view().unwrap().reversed_view(1).unwrap();
        for tensor in [dense, reversed] {
            assert_eq!(
                host_dimension_witness(&Value::Tensor(Rc::new(tensor))),
                uniform(&length, Some(SigType::Float))
            );
        }

        let ratio = BigRational::new(2.into(), 3.into());
        let integer = BigRational::from_integer(17.into());
        for (storage, repr) in [
            (
                Storage::TypedExact(vec![integer; 2], ExactRep::Int),
                Some(SigType::Int),
            ),
            (
                Storage::TypedExact(vec![ratio.clone(); 2], ExactRep::Real),
                Some(SigType::Real),
            ),
            (Storage::Exact(vec![ratio; 2]), None),
            (Storage::Bool(vec![false, true]), Some(SigType::Bool)),
        ] {
            let dim = if repr == Some(SigType::Bool) {
                Dim::one()
            } else {
                length.clone()
            };
            let tensor = Rc::new(TensorData::new(vec![2], storage, dim.clone(), None));
            let tail = tensor.tail_view(1).unwrap();
            for view in [tensor, tail] {
                assert_eq!(
                    host_dimension_witness(&Value::Tensor(view)),
                    uniform(&dim, repr.clone())
                );
            }
        }
    }

    #[test]
    fn boxed_slice_collects_the_surviving_nested_witnesses_in_order() {
        let one = Dim::one();
        let length = Dim::base("Length");
        let record = Value::Record(Rc::new(RecordValue {
            fields: [(
                "point".to_owned(),
                Value::Num(crate::value::NumVal::float(4.0, length.clone())),
            )]
            .into(),
        }));
        let flags = Value::Tensor(Rc::new(TensorData::new(
            vec![1],
            Storage::Bool(vec![true]),
            one.clone(),
            None,
        )));
        let boxed = Rc::new(TensorData::new(
            vec![3],
            Storage::Boxed(vec![Value::Unit, record, flags]),
            one.clone(),
            None,
        ));
        let W::Tensor { elements, .. } = host_dimension_witness(&Value::Tensor(boxed.clone()))
        else {
            panic!("expected the recursive witness of the direct tensor");
        };
        assert_eq!(elements.len(), 3);
        assert_eq!(elements.first(), Some(&W::Unknown));
        let tail = Value::Tensor(boxed.tail_view(1).unwrap());
        assert_eq!(
            host_dimension_witness(&tail),
            W::Tensor {
                dimension: Some(one.clone()),
                element: None,
                element_repr: None,
                elements: vec![
                    W::Record(vec![("point".to_owned(), W::Scalar(length))]),
                    uniform(&one, Some(SigType::Bool)),
                ],
            },
        );
    }

    #[test]
    fn boxed_slice_ignores_nested_elements_before_its_window() {
        let time = Dim::base("Time");
        let nested = Value::Record(Rc::new(RecordValue {
            fields: Default::default(),
        }));
        let tensor = Rc::new(TensorData::new(
            vec![3],
            Storage::Boxed(vec![nested, Value::float(1.0), Value::float(2.0)]),
            time.clone(),
            None,
        ));
        let W::Tensor { elements, .. } = host_dimension_witness(&Value::Tensor(tensor.clone()))
        else {
            panic!("expected a boxed tensor witness");
        };
        assert_eq!(elements.len(), 3);
        let tail = Value::Tensor(tensor.tail_view(1).unwrap());
        assert_eq!(host_dimension_witness(&tail), uniform(&time, None));
    }

    #[test]
    fn checked_empty_tensor_schema_still_requires_matching_extent() {
        let length = Dim::base("Length");
        let empty = TensorData::checked(
            vec![2, 0],
            Storage::Dense(Rc::new(Vec::new())),
            length.clone(),
            None,
            Type::float(length.clone()),
        )
        .unwrap();
        assert_eq!(
            host_dimension_witness(&Value::Tensor(Rc::new(empty.clone()))),
            W::Tensor {
                dimension: Some(length.clone()),
                element: Some(Box::new(W::Scalar(length.clone()))),
                element_repr: Some(SigType::Float),
                elements: Vec::new(),
            },
        );
        let mut malformed = empty;
        malformed.shape = vec![2, 1];
        let malformed = Value::Tensor(Rc::new(malformed));
        assert_eq!(
            host_dimension_witness(&malformed),
            uniform(&length, Some(SigType::Float))
        );
        assert_eq!(bridge::to_val(&malformed).unwrap_err().code, "E0809");
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
                            .map(|extent| extent.as_ref().and_then(aevia_types::Nat::as_const))
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
        | (Type::Adt(id, args), SigType::Variant(cases), W::OptionSchema(proof))
            if *id == aevia_types::adt::OPTION_ID =>
        {
            match (args.as_slice(), aevia_rssn::registry::option_payload(cases)) {
                | ([expected], Some(payload)) => empty_schema_matches(expected, payload, proof),
                | _ => false,
            }
        },
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
                                extent.as_ref().and_then(aevia_types::Nat::as_const)
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
        | (Type::Record(row), SigType::Record(actual), W::Record(witnesses)) => {
            let Some(mut actual) = unique_fields(actual) else {
                return false;
            };
            let Some(mut witnesses) = unique_fields(witnesses) else {
                return false;
            };
            let mut remaining = row;
            while let Row::Extend { label, ty, tail } = remaining {
                let label = label.as_str();
                let Some((repr, witness)) = actual.remove(label).zip(witnesses.remove(label))
                else {
                    return false;
                };
                if !empty_schema_matches(ty, repr, witness) {
                    return false;
                }
                remaining = tail;
            }
            matches!(remaining, Row::Empty) && actual.is_empty() && witnesses.is_empty()
        },
        | _ => false,
    }
}
/// Index fields exactly once, rejecting duplicate names.
fn unique_fields<T>(fields: &[(String, T)]) -> Option<HashMap<&str, &T>> {
    let map: HashMap<_, _> = fields
        .iter()
        .map(|(name, field)| (name.as_str(), field))
        .collect();
    (map.len() == fields.len()).then_some(map)
}

fn witness_numeric_dim(
    sig: &SigType,
    dims: &DimUnifier,
) -> Option<Dim> {
    match sig {
        | SigType::Dim(_, dim) => Some(dims.resolve(dim)),
        | SigType::Int | SigType::Real | SigType::Float | SigType::Complex => Some(Dim::one()),
        | _ => None,
    }
}

fn require_result_dim(
    expected: &Dim,
    got: Option<&Dim>,
    span: Span,
) -> Result<(), EvalError> {
    if !expected.is_concrete() {
        return Err(EvalError::new("E1001", "kernel result dimension is unknown").at(span));
    }
    match got {
        | Some(actual) if actual == expected => Ok(()),
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

/// Compare explicit kernel witnesses to the independently instantiated result
/// type. Missing or contradictory physical evidence is never overwritten.
fn verify_result_witness(
    sig: &SigType,
    witness: &aevia_rssn::witness::DimensionWitness,
    dims: &DimUnifier,
    span: Span,
) -> Result<(), EvalError> {
    use aevia_rssn::witness::DimensionWitness as W;
    if let Some(expected) = witness_numeric_dim(sig, dims) {
        let actual = match witness {
            | W::Scalar(actual) => Some(actual),
            | _ => None,
        };
        return require_result_dim(&expected, actual, span);
    }
    match (sig, witness) {
        | (
            SigType::Tensor { elem, .. } | SigType::Sparse { elem, .. },
            W::Tensor {
                dimension,
                element,
                elements,
                ..
            },
        ) => {
            if let Some(expected) = witness_numeric_dim(elem, dims) {
                require_result_dim(&expected, dimension.as_ref(), span)?;
                for item in elements {
                    let W::Scalar(actual) = item else {
                        return Err(EvalError::new(
                            "E1001",
                            "kernel tensor element has an unknown dimension witness",
                        )
                        .at(span));
                    };
                    require_result_dim(&expected, Some(actual), span)?;
                }
            } else if elements.is_empty() {
                let Some(element) = element else {
                    return Err(EvalError::new(
                        "E1001",
                        "kernel omitted empty tensor element witness",
                    )
                    .at(span));
                };
                verify_result_witness(elem, element, dims, span)?;
            } else {
                for item in elements {
                    verify_result_witness(elem, item, dims, span)?;
                }
            }
            Ok(())
        },
        | (SigType::Variant(cases), W::Variant { tag, fields }) => {
            if aevia_rssn::registry::option_payload(cases).is_none() {
                return Ok(());
            }
            let Some((_, schema)) = cases.iter().find(|(name, _)| name == tag) else {
                return Err(
                    EvalError::new("E1004", "kernel returned an invalid Option tag").at(span),
                );
            };
            if schema.len() != fields.len() {
                return Err(
                    EvalError::new("E1004", "kernel returned an invalid Option payload").at(span),
                );
            }
            for (field, proof) in schema.iter().zip(fields) {
                verify_result_witness(field, proof, dims, span)?;
            }
            Ok(())
        },
        | (SigType::Variant(cases), W::OptionSchema(proof)) => {
            let Some(payload) = aevia_rssn::registry::option_payload(cases) else {
                return Err(
                    EvalError::new("E1004", "kernel returned an invalid Option schema").at(span),
                );
            };
            verify_result_witness(payload, proof, dims, span)
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
#[allow(
    clippy::single_call_fn,
    reason = "binds validated input evidence before checking the result context"
)]
fn bind_dimensions(
    sig: &aevia_rssn::Sig,
    args: &[Value],
    expected: Option<&aevia_types::Type>,
    span: Span,
) -> Result<DimUnifier, EvalError> {
    let mut dims = DimUnifier::new();
    for (i, value) in args.iter().enumerate() {
        let param = sig
            .params
            .get(i)
            .or_else(|| sig.variadic.as_ref().map(|(_, _, ty)| ty));
        if let Some(param) = param {
            bind_argument_dimension(param, value, &mut dims, span)?;
        }
    }
    if let Some(expected) = expected {
        check_result_evidence(&sig.ret, expected, &mut dims, span)?;
    }
    Ok(dims)
}

fn bind_argument_dimension(
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
        return bind_witness(sig, &boxed.witness, dims, span);
    }
    match (sig, value) {
        | (SigType::Dim(_, dim), Value::Num(_) | Value::Dyn(_))
        | (SigType::Point(dim), Value::Num(_)) => {
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
        | (SigType::Tensor { elem, .. } | SigType::Sparse { elem, .. }, Value::Tensor(tensor)) => {
            match &**elem {
                | SigType::Dim(_, dim) => dims.unify(dim, &tensor.dim).map_err(mismatch),
                | SigType::Real | SigType::Float | SigType::Int | SigType::Complex => {
                    dims.unify(&Dim::one(), &tensor.dim).map_err(mismatch)
                },
                | _ => {
                    for i in 0..tensor.storage.len() {
                        if let Some(item) = tensor.get_flat(i) {
                            bind_argument_dimension(elem, &item, dims, span)?;
                        }
                    }
                    Ok(())
                },
            }
        },
        | (SigType::Variant(cases), Value::Variant(value))
            if value.adt == aevia_types::adt::OPTION_ID
                && aevia_rssn::registry::option_payload(cases).is_some() =>
        {
            let Some((_, fields)) = cases.iter().find(|(name, _)| name == &value.name) else {
                return Err(EvalError::new("E1003", "invalid Option tag").at(span));
            };
            if fields.len() != value.fields.len() {
                return Err(EvalError::new("E1003", "invalid Option payload").at(span));
            }
            for (field, actual) in fields.iter().zip(&value.fields) {
                bind_argument_dimension(field, actual, dims, span)?;
            }
            Ok(())
        },
        | (SigType::Record(fields), Value::Record(record)) => {
            for (name, field) in fields {
                if let Some(value) = record.fields.get(name) {
                    bind_argument_dimension(field, value, dims, span)?;
                }
            }
            Ok(())
        },
        | _ => Ok(()),
    }
}

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
        | (SigType::Dim(_, d), W::Scalar(actual)) => dims.unify(d, actual).map_err(mismatch),
        | (SigType::Int | SigType::Real | SigType::Float | SigType::Complex, W::Scalar(actual)) => {
            dims.unify(&Dim::one(), actual).map_err(mismatch)
        },
        | (
            SigType::Tensor { elem, .. } | SigType::Sparse { elem, .. },
            W::Tensor {
                dimension: Some(d), ..
            },
        ) => bind_witness(elem, &W::Scalar(d.clone()), dims, span),
        | (SigType::Variant(cases), W::Variant { tag, fields }) => {
            if aevia_rssn::registry::option_payload(cases).is_some()
                && let Some((_, schema)) = cases.iter().find(|(name, _)| name == tag)
                && schema.len() == fields.len()
            {
                for (sig, proof) in schema.iter().zip(fields) {
                    bind_witness(sig, proof, dims, span)?;
                }
            }
            Ok(())
        },
        | (SigType::Variant(cases), W::OptionSchema(proof)) => {
            if let Some(payload) = aevia_rssn::registry::option_payload(cases) {
                bind_witness(payload, proof, dims, span)?;
            }
            Ok(())
        },
        | (SigType::Record(fields), W::Record(values)) => {
            for (name, ty) in fields {
                if let Some((_, w)) = values.iter().find(|(n, _)| n == name) {
                    bind_witness(ty, w, dims, span)?;
                }
            }
            Ok(())
        },
        | (
            SigType::Dim(_, _) | SigType::Int | SigType::Real | SigType::Float | SigType::Complex,
            _,
        ) => Err(EvalError::new("E1001", "kernel argument has no dimension witness").at(span)),
        | _ => Ok(()),
    }
}

fn check_result_evidence(
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
        ) => check_result_evidence(elem, got, dims, span),
        | (SigType::Variant(cases), Type::Adt(id, args))
            if *id == aevia_types::adt::OPTION_ID && matches!(args.as_slice(), [_]) =>
        {
            if let (Some(payload), [argument]) =
                (aevia_rssn::registry::option_payload(cases), args.as_slice())
            {
                check_result_evidence(payload, argument, dims, span)?;
            }
            Ok(())
        },
        | (SigType::Record(fields), Type::Record(row)) => {
            for (name, field) in fields {
                if let Some(got) = row.field(name) {
                    check_result_evidence(field, got, dims, span)?;
                }
            }
            Ok(())
        },
        | _ => Ok(()),
    }
}

fn resolve_result_dimension(
    dim: &Dim,
    dims: &DimUnifier,
    span: Span,
) -> Result<Dim, EvalError> {
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
}

/// Restore each result component's own instantiated dimension instead of
/// assigning one dimension to an entire heterogeneous record.
fn result_with_dimensions(
    value: &Val,
    sig: &SigType,
    dims: &DimUnifier,
    span: Span,
) -> Result<Value, EvalError> {
    match (value, sig) {
        | (payload, SigType::Opaque(kind)) if kind == aevia_rssn::registry::NATIVE_AD_VAR_KIND => {
            if !crate::value::is_native_ad_var_payload(payload) {
                return Err(EvalError::new(
                    "E1004",
                    "kernel returned an invalid native AD variable handle",
                )
                .at(span));
            }
            Ok(Value::KernelValue(Rc::new(crate::value::KernelValue {
                payload: payload.clone(),
                witness: aevia_rssn::witness::DimensionWitness::Unknown,
                host_witness: None,
            })))
        },
        | (Val::Variant { tag, payload }, SigType::Variant(cases))
            if aevia_rssn::registry::option_payload(cases).is_some() =>
        {
            let Some((_, fields)) = cases.iter().find(|(name, _)| name == tag) else {
                return Err(
                    EvalError::new("E1004", "kernel returned an invalid Option tag").at(span),
                );
            };
            if fields.len() != payload.len() {
                return Err(
                    EvalError::new("E1004", "kernel returned an invalid Option payload").at(span),
                );
            }
            let output = payload
                .iter()
                .zip(fields)
                .map(|(value, field)| result_with_dimensions(value, field, dims, span))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(crate::value::make_variant(
                aevia_types::adt::OPTION_ID,
                usize::from(tag == "Some"),
                tag,
                output,
            ))
        },
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
        ) => result_tensor_with_dimensions(shape, data, elem, sig, dims, span),
        | (_, SigType::Dim(_, dim) | SigType::Point(dim)) => {
            Ok(bridge::from_val(
                value,
                resolve_result_dimension(dim, dims, span)?,
            ))
        },
        | _ => Ok(bridge::from_val(value, Dim::one())),
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "preserves uniform Float storage and recursive fallback in one tensor result boundary"
)]
fn result_tensor_with_dimensions(
    shape: &[usize],
    data: &[Val],
    elem: &SigType,
    sig: &SigType,
    dims: &DimUnifier,
    span: Span,
) -> Result<Value, EvalError> {
    let dim = match elem {
        | SigType::Dim(_, d) | SigType::Point(d) => resolve_result_dimension(d, dims, span)?,
        | _ => Dim::one(),
    };
    let float_elem = matches!(elem, SigType::Float)
        || matches!(elem, SigType::Dim(inner, _) if matches!(inner.as_ref(), SigType::Float));
    let storage = if float_elem {
        let matching_shape = matches!(
            sig,
            SigType::Tensor { rank, shape: declared, .. }
                if rank.is_none_or(|rank| rank == shape.len())
                    && declared.as_deref().is_none_or(|declared| declared == shape)
        );
        let packed =
            if matching_shape && crate::value::checked_element_count(shape) == Some(data.len()) {
                packed_float_storage(data)
            } else {
                None
            };
        packed.unwrap_or_else(|| {
            // Both scalar signatures decode each leaf with this same resolved
            // dimension. Keep the ordinary bridge conversion for mixed and
            // unit-bearing leaves without resolving the dimension again.
            crate::value::Storage::Boxed(
                data.iter()
                    .map(|item| bridge::from_val(item, dim.clone()))
                    .collect(),
            )
        })
    } else {
        let mut output = Vec::with_capacity(data.len());
        for item in data {
            output.push(result_with_dimensions(item, elem, dims, span)?);
        }
        crate::value::Storage::Boxed(output)
    };
    Ok(Value::Tensor(Rc::new(crate::value::TensorData::new(
        shape.to_vec(),
        storage,
        dim,
        None,
    ))))
}

// An explicit Float on every wire element is required. Exact, structured and
// mixed payloads retain their full values through the recursive fallback.
fn packed_float_storage(data: &[Val]) -> Option<crate::value::Storage> {
    if !matches!(data.first(), Some(Val::Num(aevia_rssn::Num::Float(_)))) {
        return None;
    }
    let mut values = Vec::with_capacity(data.len());
    for item in data {
        let Val::Num(aevia_rssn::Num::Float(value)) = item else {
            return None;
        };
        values.push(*value);
    }
    Some(crate::value::Storage::Dense(Rc::new(values)))
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
/// concrete, non-forgeable `TypeRep`. The returned payload remains a kernel
/// Value; `from_value` is the explicit conversion back to an Aevia value.
#[allow(
    clippy::single_call_fn,
    reason = "implements the checked contract of the fixed Rssn.typed entry point"
)]
fn typed_call(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let [_, _, type_rep] = args else {
        return Err(EvalError::new("E0315", "`Rssn.typed` takes three arguments").at(span));
    };
    let Value::TypeRep(expected) = type_rep else {
        return Err(EvalError::new(
            "E0302",
            format!(
                "`Rssn.typed` expects a TypeRep, found {}",
                type_rep.type_name()
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
    dynamic_call_from(args, span, None)
}

/// [`dynamic_call`] on behalf of the Aevia function `caller`.
///
/// A generated wrapper forwards its arguments positionally, so a kernel finding
/// about "argument 3" can only be put in the wrapper's own terms (`axes_a`) by
/// someone who knows which wrapper is calling: the innermost frame.
fn dynamic_call_from(
    args: &[Value],
    span: Span,
    caller: Option<&str>,
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
    Ok(forward(&name, &values, caller))
}

/// How a failure reads to the program that receives it.
///
/// A kernel finding is put in the terms of the calling wrapper (its name and
/// the name of the offending parameter) when `caller` is a curated wrapper of
/// the operator that reported it, and in the operator's own terms otherwise.
fn failure_text(
    failure: &aevia_rssn::KernelError,
    caller: Option<&str>,
) -> String {
    let Some(diag) = &failure.diag else {
        return failure.message.clone();
    };
    let wrapper = caller.and_then(|function| {
        aevia_rssn::domains::wrapper_params(function, &diag.op).map(|params| (function, params))
    });
    match wrapper {
        | Some((function, params)) => failure.describe(Some(function), Some(&params)),
        | None => failure.describe(None, None),
    }
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
    Ok(forward(&name, &values, None))
}

/// Interpreter-aware callback path. The host-only `call` entry point keeps its
/// capability error because it has no interpreter to scope.
#[allow(
    clippy::single_call_fn,
    reason = "contains ownership and lifetime handling for interpreter callbacks"
)]
fn call_ho_with_interp(
    interp: &mut crate::eval::Interp,
    args: &[Value],
    span: Span,
    pinned: bool,
) -> Result<Value, EvalError> {
    let (name, hash, values, callbacks) = match (pinned, args) {
        | (false, [name, values, callbacks]) => (name, None, values, callbacks),
        | (true, [name, hash, values, callbacks]) => (name, Some(hash), values, callbacks),
        | _ => {
            let expected = if pinned { 4 } else { 3 };
            return Err(EvalError::new(
                "E0315",
                format!("callback call takes {expected} arguments"),
            )
            .at(span));
        },
    };
    let callbacks = dynamic_arguments(Some(callbacks), span)?;
    if callbacks.is_empty() && hash.is_none() {
        return call_ho(args, span);
    }
    let callbacks: Vec<_> = callbacks
        .iter()
        .map(|value| {
            match value {
                | Value::Callback(callback) => Ok(callback.clone()),
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
    let name = show(name);
    let key = if let Some(hash) = hash {
        let hash = show(hash);
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
    let values = dynamic_arguments(Some(values), span)?;
    let converted = match prepare_native_arguments(&values) {
        | Ok(converted) => converted,
        | Err(error) => return Ok(err(value_error(error.code, error.message))),
    };
    let mut witnesses = WitnessScratchGuard::with_capacity(values.len());
    fill_host_witnesses(&values, &mut witnesses.0);
    let result = crate::callback::with_scoped(interp, callbacks, |handles| {
        key.as_ref().map_or_else(
            || aevia_rssn::kernel().call_ho_scoped_native(&name, &converted, &witnesses.0, handles),
            |key| {
                aevia_rssn::kernel().call_ho_scoped_key_native_diagnosed(
                    key,
                    &converted,
                    &witnesses.0,
                    handles,
                )
            },
        )
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
#[allow(
    clippy::single_call_fn,
    reason = "reports the host-only Plugin boundary for both registration spellings"
)]
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
    caller: Option<&str>,
) -> Value {
    let converted = match prepare_native_arguments(values) {
        | Ok(converted) => converted,
        | Err(error) => return err(value_error(error.code, error.message)),
    };
    let mut witnesses = WitnessScratchGuard::with_capacity(values.len());
    fill_host_witnesses(values, &mut witnesses.0);
    match aevia_rssn::kernel().call_with_native_arguments(name, &converted, &witnesses.0) {
        | Ok((payload, witness)) => {
            ok(Value::KernelValue(Rc::new(crate::value::KernelValue {
                payload,
                witness,
                host_witness: None,
            })))
        },
        | Err(failure) => err(value_error(failure.code, failure_text(&failure, caller))),
    }
}

/// `Rssn.call_key(name, hash, values)`.
#[allow(
    clippy::single_call_fn,
    reason = "keeps the pinned-key operation and its Result error channel together"
)]
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
        let converted = prepare_native_arguments(&values)?;
        let mut witnesses = WitnessScratchGuard::with_capacity(values.len());
        fill_host_witnesses(&values, &mut witnesses.0);
        kernel.call_key_with_native_arguments_diagnosed(&key, &converted, &witnesses.0)
    } else {
        use aevia_rssn::witness::DimensionWitness as W;
        let mut converted = ValScratchGuard::with_capacity(values.len());
        fill_neutral_vals(&values, &mut converted.0)?;
        let safe = values.iter().all(|v| {
            match host_dimension_witness(v) {
                | W::Scalar(d)
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
            .call_key_diagnosed(&key, &converted.0)
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
        | Err(error) => Ok(err(value_error(error.code, failure_text(&error, None)))),
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
    let entry = registry().op(&name);
    let converted = prepare_checked_arguments(&values, entry)?;
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
    execute_checked(&key, entry, &values, &converted, span, expected)
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

#[allow(
    clippy::single_call_fn,
    reason = "implements the fixed overload-resolution operation and its textual key"
)]
fn resolve(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let name = args
        .first()
        .map(show)
        .ok_or_else(|| EvalError::new("E0315", "`Rssn.resolve_overload` needs a name").at(span))?;
    let values = dynamic_arguments(args.get(1), span)?;
    let mut converted = ValScratchGuard::with_capacity(values.len());
    fill_neutral_vals(&values, &mut converted.0)?;
    let kernel = aevia_rssn::kernel();
    match kernel.resolve(&name, &converted.0) {
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

#[allow(
    clippy::single_call_fn,
    reason = "builds the fixed overload-inspection result"
)]
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

/// The argument list of a dynamic call, borrowing boxed rank-1 lists directly.
fn dynamic_arguments(
    value: Option<&Value>,
    span: Span,
) -> Result<Cow<'_, [Value]>, EvalError> {
    match value {
        | Some(Value::Tensor(tensor)) => {
            let [count] = tensor.shape.as_slice() else {
                return Err(EvalError::new("E0302", "expected a rank-one list of values").at(span));
            };
            let malformed =
                || EvalError::new("E0802", "argument list does not match its storage").at(span);
            if *count != tensor.storage.len() {
                return Err(malformed());
            }
            if let crate::value::Storage::Boxed(items) = &tensor.storage {
                return Ok(Cow::Borrowed(items.as_slice()));
            }
            (0..*count)
                .map(|index| tensor.get_flat(index))
                .collect::<Option<Vec<_>>>()
                .map(Cow::Owned)
                .ok_or_else(malformed)
        },
        | Some(Value::Unit) | None => Ok(Cow::Borrowed(&[])),
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
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
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
        let _backend = interp.rssn_source_access();
        if owned == "invoke" {
            let expected = interp.resolved_expr_type(span);
            invoke(&args, span, expected.as_ref())
        } else if owned == "call_ho" || owned == "call_ho_key" {
            call_ho_with_interp(interp, &args, span, owned == "call_ho_key")
        } else if owned == "dynamic" || owned == "call" {
            let caller = interp.frames.last().map(|frame| frame.function.clone());
            dynamic_call_from(&args, span, caller.as_deref())
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
    fn static_float_tensor_storage_needs_matching_shape_and_explicit_float_leaves() {
        let dim = Dim::nominal_base("books", "Credit").with_display_name("CustomerCredit");
        let signature = SigType::Tensor {
            rank: Some(1),
            shape: Some(vec![2]),
            elem: Box::new(SigType::Dim(Box::new(SigType::Float), dim.clone())),
        };
        let dims = DimUnifier::new();
        let first = Val::Num(aevia_rssn::Num::Float(-0.0));
        let nan = f64::from_bits(0xfff8_0000_0000_0019);
        let second = Val::Num(aevia_rssn::Num::Float(nan));
        let decode = |signature: &SigType, shape, data| {
            let value = Val::Tensor { shape, data };
            let Value::Tensor(tensor) =
                result_with_dimensions(&value, signature, &dims, Span::default()).unwrap()
            else {
                panic!("expected a tensor")
            };
            tensor
        };
        let tensor = decode(&signature, vec![2], vec![first.clone(), second.clone()]);
        assert_eq!(tensor.shape, [2]);
        assert_eq!(
            tensor.dim.to_display_si_string(),
            dim.to_display_si_string()
        );
        assert!(tensor.display.is_none());
        assert!(tensor.verified_element.is_none());
        let bits = tensor
            .dense()
            .unwrap()
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>();
        assert_eq!(bits, [(-0.0_f64).to_bits(), nan.to_bits()]);

        let malformed = decode(&signature, vec![2], vec![first.clone()]);
        assert!(matches!(malformed.storage, crate::value::Storage::Boxed(_)));
        let wrong_rank = decode(&signature, vec![1, 2], vec![first.clone(), second]);
        assert!(matches!(
            wrong_rank.storage,
            crate::value::Storage::Boxed(_)
        ));
        let mixed = decode(
            &signature,
            vec![2],
            vec![first, Val::Num(aevia_rssn::Num::Int(17.into()))],
        );
        let crate::value::Storage::Boxed(values) = &mixed.storage else {
            panic!("expected mixed storage");
        };
        let integer = values[1].as_num().unwrap();
        assert!(matches!(integer.kind, crate::value::NumKind::Int(_)));
        assert_eq!(integer.to_f64(), Some(17.0));
        assert_eq!(integer.dim, dim);
        let explicit = Val::Quantity {
            value: Box::new(Val::Num(aevia_rssn::Num::Float(1.25))),
            unit: aevia_rssn::UnitVal::linear("credit", dim.clone(), aevia_dim::Scale::one()),
        };
        let with_unit = decode(
            &signature,
            vec![2],
            vec![Val::Num(aevia_rssn::Num::Float(-0.0)), explicit],
        );
        assert!(matches!(with_unit.storage, crate::value::Storage::Boxed(_)));
        let number = with_unit.get_flat(1).unwrap();
        assert_eq!(
            number.as_num().unwrap().display.as_ref().unwrap().name,
            "credit"
        );
        let mut empty_signature = signature.clone();
        if let SigType::Tensor { shape, .. } = &mut empty_signature {
            *shape = Some(vec![0]);
        }
        let empty = decode(&empty_signature, vec![0], Vec::new());
        assert!(
            matches!(&empty.storage, crate::value::Storage::Boxed(values) if values.is_empty())
        );
        assert!(empty.verified_element.is_none());
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
#[allow(
    clippy::single_call_fn,
    reason = "keep opaque Value validation available without decoding its payload"
)]
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

/// `from_value<Tensor<1, Value>>`: the list accessor of a kernel value.
///
/// A kernel list of unlike parts (the `list(core, factors, error)` of a tensor
/// decomposition, whose parts are tensors of different ranks and a number) has
/// no Aevia tensor type. Asking for `Tensor<1, Value>` hands out the parts of
/// the first axis as handles, each carrying its own dimension witness, and
/// `from_value<T>` on a part decodes it as usual. A tensor of rank 2 or more
/// yields its rows; a scalar is not a list and is an `Err`.
///
/// Returns `None` when `expected` is not `Tensor<1, Value>` (the ordinary
/// path), and `Some` for the accessor, with the same all-or-nothing contract:
/// the payload is only split, never reinterpreted.
#[allow(
    clippy::single_call_fn,
    reason = "checks the shape and allocation of an opaque tensor decomposition"
)]
fn kernel_pieces(
    value: &Value,
    expected: &aevia_types::Type,
) -> Option<Result<Value, String>> {
    use aevia_rssn::witness::DimensionWitness as W;
    let aevia_types::Type::Tensor {
        shape: expected_shape,
        elem,
    } = expected
    else {
        return None;
    };
    if expected_shape.rank() != 1 || **elem != aevia_types::builtins::dynamic_value_type() {
        return None;
    }
    let Value::KernelValue(kernel) = value else {
        return Some(Err("expected an opaque kernel Value".into()));
    };
    let Val::Tensor { shape, data } = &kernel.payload else {
        return Some(Err(format!(
            "kernel value does not prove expected type {expected}: it is not a list"
        )));
    };
    let &[count, ref inner @ ..] = shape.as_slice() else {
        return Some(Err("kernel value is a scalar, not a list".into()));
    };
    if expected_shape
        .extent(0)
        .is_some_and(|extent| extent != count)
    {
        return Some(Err(format!(
            "kernel list length {count} does not match {expected}"
        )));
    }
    let Some(stride) = crate::value::checked_element_count(inner) else {
        return Some(Err("kernel tensor shape exceeds the supported range".into()));
    };
    if count.checked_mul(stride) != Some(data.len()) {
        return Some(Err("kernel tensor shape does not match its storage".into()));
    }
    if count > crate::sparse::DENSE_LIMIT {
        return Some(Err("kernel list exceeds the dense element limit".into()));
    }
    let (dimension, element, parts) = match &kernel.witness {
        | W::Tensor {
            dimension,
            element,
            elements,
            ..
        } => (dimension.clone(), element.as_deref(), elements.as_slice()),
        | _ => (None, None, [].as_slice()),
    };
    if !parts.is_empty() && parts.len() != count {
        return Some(Err("kernel value has a malformed element witness".into()));
    }
    let mut items = Vec::new();
    if items.try_reserve_exact(count).is_err() {
        return Some(Err("could not allocate the kernel list".into()));
    }
    let mut offset = 0usize;
    for i in 0..count {
        let Some(next) = offset.checked_add(stride) else {
            return Some(Err("kernel tensor shape exceeds the supported range".into()));
        };
        let Some(row) = data.get(offset..next) else {
            return Some(Err("kernel tensor shape does not match its storage".into()));
        };
        offset = next;
        let (payload, witness) = if inner.is_empty() {
            let witness = parts
                .get(i)
                .cloned()
                .or_else(|| element.cloned())
                .or_else(|| dimension.clone().map(W::Scalar))
                .unwrap_or(W::Unknown);
            let [item] = row else {
                return Some(Err("kernel tensor shape does not match its storage".into()));
            };
            (item.clone(), witness)
        } else {
            // A slice of a uniform tensor has the witness of the whole.
            (
                Val::Tensor {
                    shape: inner.to_vec(),
                    data: row.to_vec(),
                },
                parts
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| kernel.witness.clone()),
            )
        };
        items.push(Value::KernelValue(Rc::new(crate::value::KernelValue {
            payload,
            witness,
            host_witness: None,
        })));
    }
    Some(Ok(Value::Tensor(Rc::new(crate::value::TensorData {
        verified_element: Some(Rc::new((**elem).clone())),
        shape: vec![count],
        storage: crate::value::Storage::Boxed(items),
        dim: Dim::one(),
        display: None,
    }))))
}

/// Converts only after the shared borrowed validator has established the contract.
pub(crate) fn from_kernel_value(
    value: &Value,
    expected: &aevia_types::Type,
) -> Result<Value, String> {
    if let Some(pieces) = kernel_pieces(value, expected) {
        return pieces;
    }
    let Value::KernelValue(value) = value else {
        return Err("expected an opaque kernel Value".into());
    };
    decode_checked_payload(
        &value.payload,
        &value.witness,
        value.host_witness.as_deref(),
        expected,
    )
}

/// Checks a borrowed kernel packet before converting it to a runtime value.
pub(crate) fn decode_checked_payload(
    payload: &Val,
    witness: &aevia_rssn::witness::DimensionWitness,
    host: Option<&crate::value::HostWitness>,
    expected: &aevia_types::Type,
) -> Result<Value, String> {
    validate_payload(payload, witness, host, expected)?;
    decode_payload(payload, witness, host, expected)
}

// Structural erasure may forget a field/element type without asserting a new
// type. Nominal identities and nominal type arguments remain unchanged.
fn erased_schema_compatible(
    source: &aevia_types::Type,
    expected: &aevia_types::Type,
) -> bool {
    use aevia_types::Type;
    if *expected == aevia_types::builtins::dynamic_value_type()
        || crate::host_witness::schema_compatible(source, expected)
    {
        return true;
    }
    match (source, expected) {
        | (Type::Tensor { shape: a, elem: x }, Type::Tensor { shape: b, elem: y }) => {
            a.rank() == b.rank() && erased_schema_compatible(x, y)
        },
        | (Type::Record(a), Type::Record(b)) if a.tail_var().is_none() && b.tail_var().is_none() =>
        {
            let fields = a.fields();
            fields.len() == b.fields().len()
                && fields.iter().all(|(label, ty)| {
                    b.field(label)
                        .is_some_and(|expected| erased_schema_compatible(ty, expected))
                })
        },
        | _ => false,
    }
}

/// Checks a kernel packet against the expected runtime schema and evidence.
pub(crate) fn validate_payload(
    payload: &Val,
    witness: &aevia_rssn::witness::DimensionWitness,
    host: Option<&crate::value::HostWitness>,
    expected: &aevia_types::Type,
) -> Result<(), String> {
    use crate::value::HostWitnessNode as H;
    use aevia_rssn::witness::DimensionWitness as W;
    use aevia_types::Rep;
    use aevia_types::Type;
    // Erased Value requests assert no representation, dimension or nominal
    // identity. Keep its existing evidence for a later checked conversion.
    if *expected == aevia_types::builtins::dynamic_value_type() {
        return Ok(());
    }
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
    if host.is_some_and(|h| !erased_schema_compatible(&h.schema, expected)) {
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
        | (payload, ty)
            if aevia_types::adt::is_native_ad_var_type(ty)
                && crate::value::is_native_ad_var_payload(payload) =>
        {
            Ok(())
        },
        | (Val::Quantity { value, unit }, Type::Scalar(rep, dim)) => {
            if !unit.dim.is_concrete() || &unit.dim != dim || !proves_dim(dim) {
                return Err(mismatch());
            }
            quantity_scalar(value, unit, *rep).map(|_| ())
        },
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
        | (Val::Variant { tag, payload }, Type::Adt(id, args))
            if *id == aevia_types::adt::OPTION_ID
                && host.is_none()
                && let [argument] = args.as_slice() =>
        {
            let W::Variant { tag: proven, fields } = witness else {
                return Err(mismatch());
            };
            if proven != tag {
                return Err(mismatch());
            }
            match (tag.as_str(), payload.as_slice(), fields.as_slice()) {
                | ("None", [], []) => Ok(()),
                | ("Some", [item], [proof]) => validate_payload(item, proof, None, argument),
                | _ => Err(mismatch()),
            }
        },
        | (Val::Variant { tag, payload }, Type::Adt(expected_id, _)) => {
            validate_nominal_payload(tag, payload, *expected_id, witness, host, mismatch)
        },
        | (
            Val::Tensor { shape, data },
            Type::Tensor {
                shape: expected_shape,
                elem,
            },
        ) => validate_tensor_payload(shape, data, expected_shape, elem, witness, host, mismatch),
        | (Val::Record(fields), Type::Record(row)) => {
            validate_record_payload(fields, row, witness, host, mismatch)
        },
        | _ => Err(mismatch()),
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "checks nominal constructor identity independently from native Option encoding"
)]
fn validate_nominal_payload(
    tag: &str,
    payload: &[Val],
    expected_id: aevia_types::adt::AdtId,
    witness: &aevia_rssn::witness::DimensionWitness,
    host: Option<&crate::value::HostWitness>,
    mismatch: impl Fn() -> String + Copy,
) -> Result<(), String> {
    use crate::value::HostWitnessNode as H;
    use aevia_rssn::witness::DimensionWitness as W;
    let Some(crate::value::HostWitness {
        node:
            H::Variant {
                adt,
                tag: proven_tag,
                fields,
                ..
            },
        ..
    }) = host
    else {
        return Err(mismatch());
    };
    if !aevia_types::adt::same_type(*adt, expected_id)
        || tag != proven_tag
        || payload.len() != fields.len()
    {
        return Err(mismatch());
    }
    // Nominal identity still comes exclusively from the private host
    // proof. Independently retained kernel evidence follows each field.
    let proven = match witness {
        | W::Variant {
            tag: actual,
            fields: proofs,
        } => {
            if actual != tag || proofs.len() != payload.len() {
                return Err(mismatch());
            }
            Some(proofs)
        },
        | _ => None,
    };
    for (i, (item, field)) in payload.iter().zip(fields).enumerate() {
        let proof = proven
            .and_then(|proofs| proofs.get(i))
            .unwrap_or(&W::Unknown);
        validate_payload(item, proof, Some(field), &field.schema)?;
    }
    Ok(())
}

#[allow(
    clippy::single_call_fn,
    reason = "tensor validation relates physical dimensions, extents, and independently proven elements"
)]
fn validate_tensor_payload(
    shape: &[usize],
    data: &[Val],
    expected_shape: &aevia_types::Shape,
    elem: &aevia_types::Type,
    witness: &aevia_rssn::witness::DimensionWitness,
    host: Option<&crate::value::HostWitness>,
    mismatch: impl Fn() -> String + Copy,
) -> Result<(), String> {
    use crate::value::HostWitnessNode as H;
    use aevia_rssn::witness::DimensionWitness as W;
    use aevia_types::Rep;
    use aevia_types::Shape;
    use aevia_types::Type;
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
        | Some(H::Tensor(items)) if items.is_empty() && crate::host_witness::is_leaf_schema(elem) => {
            None
        },
        | Some(_) => return Err(mismatch()),
        | None => None,
    };
    if data.is_empty() {
        let valid_repr = matches!(
            (elem, element_repr),
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
        let proven = valid_repr
            || host_items.is_some()
            || element_repr
                .as_ref()
                .zip(element.as_deref())
                .is_some_and(|(repr, witness)| empty_schema_matches(elem, repr, witness));
        let dimension_matches = match elem {
            | Type::Scalar(_, dim) => dim.is_concrete() && dimension.as_ref() == Some(dim),
            | _ => true,
        };
        return (proven && dimension_matches)
            .then_some(())
            .ok_or_else(mismatch);
    }
    let scalar = if let Type::Scalar(_, dim) = elem {
        if !dim.is_concrete()
            || dimension.as_ref() != Some(dim)
            || elements
                .iter()
                .any(|w| !matches!(w, W::Scalar(actual) if actual == dim))
        {
            return Err(mismatch());
        }
        Some(W::Scalar(dim.clone()))
    } else {
        if !elements.is_empty() && elements.len() != data.len() {
            return Err(mismatch());
        }
        None
    };
    for (i, item) in data.iter().enumerate() {
        let proof = scalar
            .as_ref()
            .or_else(|| elements.get(i))
            .or(element.as_deref())
            .unwrap_or(&W::Unknown);
        validate_payload(item, proof, host_items.and_then(|items| items.get(i)), elem)?;
    }
    Ok(())
}

#[allow(
    clippy::single_call_fn,
    reason = "requires exact shape agreement across three named-field evidence channels"
)]
fn validate_record_payload(
    fields: &[(String, Val)],
    row: &aevia_types::Row,
    witness: &aevia_rssn::witness::DimensionWitness,
    host: Option<&crate::value::HostWitness>,
    mismatch: impl Fn() -> String + Copy,
) -> Result<(), String> {
    use crate::value::HostWitnessNode as H;
    use aevia_rssn::witness::DimensionWitness as W;
    use aevia_types::Row;

    let W::Record(witnesses) = witness else {
        return Err(mismatch());
    };
    let mut host_fields = match host.map(|h| &h.node) {
        | Some(H::Record(fields)) => Some(unique_fields(fields).ok_or_else(mismatch)?),
        | Some(_) => return Err(mismatch()),
        | None => None,
    };
    let mut payload = unique_fields(fields).ok_or_else(mismatch)?;
    let mut witnesses = unique_fields(witnesses).ok_or_else(mismatch)?;
    let mut remaining = row;
    while let Row::Extend { label, ty, tail } = remaining {
        let label = label.as_str();
        let (item, proof) = payload
            .remove(label)
            .zip(witnesses.remove(label))
            .ok_or_else(mismatch)?;
        let host = host_fields
            .as_mut()
            .map(|fields| fields.remove(label).ok_or_else(mismatch))
            .transpose()?;
        validate_payload(item, proof, host, ty)?;
        remaining = tail;
    }
    if !matches!(remaining, Row::Empty)
        || !payload.is_empty()
        || !witnesses.is_empty()
        || host_fields.is_some_and(|fields| !fields.is_empty())
    {
        return Err(mismatch());
    }
    Ok(())
}

/// Convert a linear quantity in its actual unit, preserving the requested
/// numeric phase. Affine points cannot be relabelled as ordinary scalars.
fn quantity_scalar(
    payload: &Val,
    unit: &aevia_rssn::UnitVal,
    rep: aevia_types::Rep,
) -> Result<crate::value::NumVal, String> {
    use aevia_types::Rep;
    use num_traits::Zero;
    if unit.affine
        || unit.offset.is_some()
        || !unit.dim.is_concrete()
        || unit.scale.rat.denom() <= &num_bigint::BigInt::from(0)
        || unit.scale.rat <= num_rational::BigRational::zero()
    {
        return Err("kernel quantity has an invalid linear unit scale or offset".into());
    }
    let mismatch =
        || "kernel quantity does not preserve the expected scalar representation".to_string();
    let exact = match payload {
        | Val::Num(aevia_rssn::Num::Int(n)) => {
            Some(num_rational::BigRational::from_integer(n.clone()))
        },
        | Val::Num(aevia_rssn::Num::Rat(n)) if rep == Rep::Real => Some(n.clone()),
        | _ => None,
    };
    let mut number = match (payload, rep) {
        | (Val::Num(aevia_rssn::Num::Int(_)), Rep::Int) => {
            let exact = scale_exact_quantity(exact.ok_or_else(mismatch)?, &unit.scale.rat);
            if unit.scale.pi != 0 || !exact.is_integer() {
                return Err(mismatch());
            }
            let mut value = crate::value::NumVal::int(exact.to_integer());
            value.dim = unit.dim.clone();
            value
        },
        | (Val::Num(aevia_rssn::Num::Int(_) | aevia_rssn::Num::Rat(_)), Rep::Real) => {
            let exact = scale_exact_quantity(exact.ok_or_else(mismatch)?, &unit.scale.rat);
            if unit.scale.pi == 0 {
                crate::value::NumVal::exact(exact, unit.dim.clone())
            } else {
                crate::value::NumVal::symbolic(
                    crate::sym::Term::mul(
                        crate::sym::Term::Num(exact),
                        crate::sym::Term::pow(
                            crate::sym::Term::Sym("pi".into()),
                            num_rational::BigRational::from_integer(unit.scale.pi.into()),
                        ),
                    ),
                    unit.dim.clone(),
                )
            }
        },
        | (Val::Symbol(_) | Val::Term(_), Rep::Real) => {
            let term = bridge::term_of(payload).ok_or_else(mismatch)?;
            let scale = crate::sym::Term::Num(unit.scale.rat.clone());
            let scale = if unit.scale.pi == 0 {
                scale
            } else {
                crate::sym::Term::mul(
                    scale,
                    crate::sym::Term::pow(
                        crate::sym::Term::Sym("pi".into()),
                        num_rational::BigRational::from_integer(unit.scale.pi.into()),
                    ),
                )
            };
            crate::value::NumVal::symbolic(crate::sym::Term::mul(term, scale), unit.dim.clone())
        },
        | (Val::Num(aevia_rssn::Num::Float(value)), Rep::Float) => {
            let scale = unit.scale.to_f64();
            if !scale.is_finite() || scale <= 0.0 {
                return Err("kernel quantity unit scale is not a finite positive Float".into());
            }
            crate::value::NumVal::float(value * scale, unit.dim.clone())
        },
        | (Val::Complex { re, im }, Rep::Complex) => {
            let scale = unit.scale.to_f64();
            if !scale.is_finite() || scale <= 0.0 {
                return Err("kernel quantity unit scale is not a finite positive Float".into());
            }
            crate::value::NumVal {
                kind: crate::value::NumKind::Complex(re * scale, im * scale),
                dim: unit.dim.clone(),
                display: None,
            }
        },
        | _ => return Err(mismatch()),
    };
    number.display = Some(Rc::new(unit.to_def()));
    Ok(number)
}

#[allow(
    clippy::arithmetic_side_effects,
    reason = "exact rational multiplication is arbitrary precision and the unit scale denominator was validated"
)]
fn scale_exact_quantity(
    value: num_rational::BigRational,
    scale: &num_rational::BigRational,
) -> num_rational::BigRational {
    value * scale
}

fn decode_payload(
    payload: &Val,
    witness: &aevia_rssn::witness::DimensionWitness,
    host: Option<&crate::value::HostWitness>,
    expected: &aevia_types::Type,
) -> Result<Value, String> {
    use crate::value::HostWitnessNode as H;
    use aevia_rssn::witness::DimensionWitness as W;
    use aevia_types::Rep;
    use aevia_types::Type;
    if *expected == aevia_types::builtins::dynamic_value_type() {
        return Ok(Value::KernelValue(Rc::new(crate::value::KernelValue {
            payload: payload.clone(),
            witness: witness.clone(),
            host_witness: host.cloned().map(Rc::new),
        })));
    }
    match (payload, expected) {
        | (payload, ty)
            if aevia_types::adt::is_native_ad_var_type(ty)
                && crate::value::is_native_ad_var_payload(payload) =>
        {
            Ok(Value::KernelValue(Rc::new(crate::value::KernelValue {
                payload: payload.clone(),
                witness: aevia_rssn::witness::DimensionWitness::Unknown,
                host_witness: None,
            })))
        },
        | (Val::Quantity { value, unit }, Type::Scalar(rep, _)) => {
            quantity_scalar(value, unit, *rep).map(Value::Num)
        },
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
            decode_tensor_payload(shape, data, elem, witness, host)
        },
        | (Val::Record(fields), Type::Record(row)) => {
            decode_record_payload(fields, row, witness, host)
        },
        | (Val::Variant { tag, payload }, Type::Adt(id, args))
            if *id == aevia_types::adt::OPTION_ID
                && host.is_none()
                && let [argument] = args.as_slice() =>
        {
            let W::Variant { fields: proofs, .. } = witness else {
                return Err("validated Option witness disappeared".into());
            };
            let (index, fields) = match (tag.as_str(), payload.as_slice(), proofs.as_slice()) {
                | ("None", [], []) => (0, Vec::new()),
                | ("Some", [item], [proof]) => {
                    (1, vec![decode_payload(item, proof, None, argument)?])
                },
                | _ => return Err("invalid native Option payload".into()),
            };
            Ok(decoded_variant(*id, index, tag, fields, expected, args))
        },
        | (Val::Variant { tag, payload }, Type::Adt(id, args)) => {
            let Some(crate::value::HostWitness {
                node: H::Variant { index, fields, .. },
                ..
            }) = host
            else {
                return Err("missing nominal witness".into());
            };
            let proofs = match witness {
                | W::Variant { fields, .. } => Some(fields),
                | _ => None,
            };
            let values = payload
                .iter()
                .zip(fields)
                .enumerate()
                .map(|(i, (item, field))| {
                    let proof = proofs
                        .and_then(|fields| fields.get(i))
                        .unwrap_or(&W::Unknown);
                    decode_payload(item, proof, Some(field), &field.schema)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(decoded_variant(*id, *index, tag, values, expected, args))
        },
        | (Val::Variant { .. }, Type::Unit) => Ok(Value::Unit),
        | _ => Ok(bridge::from_val(payload, Dim::one())),
    }
}

fn decoded_variant(
    id: aevia_types::adt::AdtId,
    index: usize,
    tag: &str,
    fields: Vec<Value>,
    expected: &aevia_types::Type,
    args: &[aevia_types::Type],
) -> Value {
    let mut result = crate::value::make_variant(id, index, tag, fields);
    if let Value::Variant(value) = &mut result {
        let value = Rc::make_mut(value);
        value.constructor_origin = true;
        value.verified_type = Some(expected.clone());
        value.type_args = Some(args.to_vec());
    }
    result
}

#[allow(
    clippy::single_call_fn,
    reason = "preserves typed empty storage and decodes witnessed elements of a tensor"
)]
fn decode_tensor_payload(
    shape: &[usize],
    data: &[Val],
    elem: &aevia_types::Type,
    witness: &aevia_rssn::witness::DimensionWitness,
    host: Option<&crate::value::HostWitness>,
) -> Result<Value, String> {
    use crate::value::HostWitnessNode as H;
    use aevia_rssn::witness::DimensionWitness as W;
    use aevia_types::Rep;
    use aevia_types::Type;
    let dim = match elem {
        | Type::Scalar(_, dim) => dim.clone(),
        | _ => Dim::one(),
    };
    let host_items = match host.map(|h| &h.node) {
        | Some(H::Tensor(items)) => Some(items),
        | _ => None,
    };
    let W::Tensor {
        dimension,
        element,
        elements,
        ..
    } = witness
    else {
        return Err("validated tensor witness disappeared".into());
    };
    let uniform = dimension.as_ref().map(|d| W::Scalar(d.clone()));
    let packed = if matches!(elem, Type::Scalar(Rep::Float, _)) {
        packed_float_storage(data)
    } else {
        None
    };
    let storage = if let Some(storage) = packed {
        storage
    } else if data.is_empty() {
        match elem {
            | Type::Scalar(Rep::Float, _) => crate::value::Storage::Dense(Rc::new(Vec::new())),
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
                    let proof = elements
                        .get(i)
                        .or(element.as_deref())
                        .or(uniform.as_ref())
                        .unwrap_or(&W::Unknown);
                    decode_payload(item, proof, host_items.and_then(|items| items.get(i)), elem)
                })
                .collect::<Result<_, _>>()?,
        )
    };
    Ok(Value::Tensor(Rc::new(crate::value::TensorData {
        // decode_payload is reached only after the shared validator
        // has authenticated both payload and the element schema.
        verified_element: Some(Rc::new(elem.clone())),
        shape: shape.to_vec(),
        storage,
        dim,
        display: None,
    })))
}

#[allow(
    clippy::single_call_fn,
    reason = "reconstructs a validated record in the expected field order"
)]
fn decode_record_payload(
    fields: &[(String, Val)],
    row: &aevia_types::Row,
    witness: &aevia_rssn::witness::DimensionWitness,
    host: Option<&crate::value::HostWitness>,
) -> Result<Value, String> {
    use crate::value::HostWitnessNode as H;
    use aevia_rssn::witness::DimensionWitness as W;
    use aevia_types::Row;
    let host_fields = match host.map(|h| &h.node) {
        | Some(H::Record(fields)) => Some(fields),
        | _ => None,
    };
    let W::Record(proofs) = witness else {
        return Err("validated record witness disappeared".into());
    };
    let mut row = row;
    let mut result = indexmap::IndexMap::new();
    while let Row::Extend { label, ty, tail } = row {
        let (_, item) = fields
            .iter()
            .find(|(n, _)| n == label)
            .ok_or_else(|| "validated field disappeared".to_string())?;
        let h = host_fields.and_then(|fs| fs.iter().find(|(n, _)| n == label).map(|(_, h)| h));
        let proof = &proofs
            .iter()
            .find(|(name, _)| name == label)
            .ok_or_else(|| "validated field witness disappeared".to_string())?
            .1;
        result.insert(label.clone(), decode_payload(item, proof, h, ty)?);
        row = tail;
    }
    Ok(Value::Record(Rc::new(crate::value::RecordValue {
        fields: result,
    })))
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

#[cfg(test)]
mod native_option_tests {
    use super::*;
    use aevia_rssn::witness::DimensionWitness as W;
    use aevia_types::Type;

    fn wrapped(
        tag: &str,
        payload: Vec<Val>,
        witness: W,
    ) -> Value {
        Value::KernelValue(Rc::new(crate::value::KernelValue {
            payload: Val::Variant {
                tag: tag.into(),
                payload,
            },
            witness,
            host_witness: None,
        }))
    }

    #[test]
    fn native_option_requires_exact_tags_arity_and_recursive_units() {
        let expected = Type::option(Type::float(Dim::base("Length")));
        let scalar = Val::Num(aevia_rssn::Num::Float(2.0));
        let some = |fields| {
            W::Variant {
                tag: "Some".into(),
                fields,
            }
        };
        let valid = wrapped(
            "Some",
            vec![scalar.clone()],
            some(vec![W::Scalar(Dim::base("Length"))]),
        );
        let Value::Variant(decoded) = from_kernel_value(&valid, &expected).unwrap() else {
            panic!("Option");
        };
        assert_eq!(decoded.adt, aevia_types::adt::OPTION_ID);
        assert_eq!(
            crate::value::value_dim(&decoded.fields[0]),
            Dim::base("Length")
        );
        let none = wrapped(
            "None",
            vec![],
            W::Variant {
                tag: "None".into(),
                fields: vec![],
            },
        );
        assert!(from_kernel_value(&none, &expected).is_ok());
        for invalid in [
            wrapped(
                "Some",
                vec![scalar.clone()],
                some(vec![W::Scalar(Dim::base("Time"))]),
            ),
            wrapped("Some", vec![scalar.clone()], some(vec![W::Unknown])),
            wrapped("Some", vec![], some(vec![])),
            wrapped(
                "Some",
                vec![scalar.clone(), scalar.clone()],
                some(vec![W::Scalar(Dim::base("Length")); 2]),
            ),
            wrapped(
                "None",
                vec![scalar],
                W::Variant {
                    tag: "None".into(),
                    fields: vec![W::Scalar(Dim::base("Length"))],
                },
            ),
            wrapped(
                "Other",
                vec![],
                W::Variant {
                    tag: "Other".into(),
                    fields: vec![],
                },
            ),
            wrapped("None", vec![], W::Unknown),
            wrapped(
                "None",
                vec![],
                W::Variant {
                    tag: "Some".into(),
                    fields: vec![],
                },
            ),
        ] {
            assert!(from_kernel_value(&invalid, &expected).is_err());
        }
    }
}

#[cfg(test)]
mod nested_erased_value_tests {
    use super::*;
    use aevia_rssn::witness::DimensionWitness as W;
    use aevia_types::Row;
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
    fn some(proof: W) -> Value {
        wrapped(
            Val::Variant {
                tag: "Some".into(),
                payload: vec![Val::Num(aevia_rssn::Num::Float(2954.0))],
            },
            W::Variant {
                tag: "Some".into(),
                fields: vec![proof],
            },
        )
    }
    #[test]
    fn option_value_preserves_actual_units_without_promoting_unknown_evidence() {
        let value_type = aevia_types::builtins::dynamic_value_type();
        let expected = Type::option(value_type);
        for (proof, known) in [(W::Scalar(Dim::base("Length")), true), (W::Unknown, false)] {
            let Value::Variant(decoded) =
                from_kernel_value(&some(proof.clone()), &expected).unwrap()
            else {
                panic!("Option")
            };
            let Value::KernelValue(inner) = &decoded.fields[0] else {
                panic!("erased Value must remain a kernel handle")
            };
            assert_eq!(inner.witness, proof);
            assert_eq!(
                from_kernel_value(&decoded.fields[0], &Type::float(Dim::base("Length"))).is_ok(),
                known
            );
            assert!(
                from_kernel_value(&decoded.fields[0], &Type::float(Dim::base("Time"))).is_err()
            );
            assert!(from_kernel_value(&decoded.fields[0], &Type::float(Dim::one())).is_err());
        }
    }
    #[test]
    fn record_and_tensor_erasure_retain_each_actual_leaf_and_private_host_proof() {
        let d = Dim::base("Length");
        let value_type = aevia_types::builtins::dynamic_value_type();
        let raw = wrapped(
            Val::Record(vec![(
                "items".into(),
                Val::Tensor {
                    shape: vec![1],
                    data: vec![Val::Num(aevia_rssn::Num::Float(2.0))],
                },
            )]),
            W::Record(vec![(
                "items".into(),
                W::Tensor {
                    dimension: Some(d.clone()),
                    element: None,
                    element_repr: Some(SigType::Float),
                    elements: vec![],
                },
            )]),
        );
        let expected = Type::Record(Row::closed([(
            "items".into(),
            Type::tensor(Shape::Rank(1), value_type.clone()),
        )]));
        let Value::Record(record) = from_kernel_value(&raw, &expected).unwrap() else {
            panic!("record")
        };
        let Value::Tensor(items) = &record.fields["items"] else {
            panic!("tensor")
        };
        let item = items.get_flat(0).unwrap();
        assert!(from_kernel_value(&item, &Type::float(d.clone())).is_ok());
        assert!(from_kernel_value(&item, &Type::float(Dim::base("Time"))).is_err());
        let source_type = Type::Record(Row::closed([("value".into(), Type::float(d.clone()))]));
        let source = Value::Record(Rc::new(crate::value::RecordValue {
            fields: indexmap::IndexMap::from_iter([(
                "value".into(),
                Value::Num(crate::value::NumVal::float(2.0, d.clone())),
            )]),
        }));
        let host = crate::host_witness::capture(&source, &source_type, false).unwrap();
        let value = Value::KernelValue(Rc::new(crate::value::KernelValue {
            payload: Val::Record(vec![(
                "value".into(),
                Val::Num(aevia_rssn::Num::Float(2.0)),
            )]),
            witness: W::Record(vec![("value".into(), W::Scalar(d.clone()))]),
            host_witness: Some(Rc::new(host)),
        }));
        let expected = Type::Record(Row::closed([("value".into(), value_type)]));
        let Value::Record(decoded) = from_kernel_value(&value, &expected).unwrap() else {
            panic!("record")
        };
        let leaf = &decoded.fields["value"];
        let Value::KernelValue(proof) = leaf else {
            panic!("Value")
        };
        assert!(proof.host_witness.is_some());
        assert!(from_kernel_value(leaf, &Type::float(d)).is_ok());
        assert!(from_kernel_value(leaf, &Type::float(Dim::base("Time"))).is_err());
    }
    #[test]
    fn nested_quantities_use_actual_linear_unit_scales_and_reject_wrong_units_or_phase() {
        let length = Dim::base("Length");
        let unit = aevia_rssn::UnitVal::linear(
            "cm",
            length.clone(),
            aevia_dim::Scale::rational(num_rational::BigRational::new(1.into(), 100.into())),
        );
        let quantity = |unit: aevia_rssn::UnitVal, value: Val| {
            wrapped(
                Val::Record(vec![(
                    "reference".into(),
                    Val::Quantity {
                        value: Box::new(value),
                        unit,
                    },
                )]),
                W::Record(vec![("reference".into(), W::Scalar(length.clone()))]),
            )
        };
        let expected = Type::Record(Row::closed([(
            "reference".into(),
            Type::float(length.clone()),
        )]));
        let raw = quantity(unit.clone(), Val::Num(aevia_rssn::Num::Float(200.0)));
        let Value::Record(record) = from_kernel_value(&raw, &expected).unwrap() else {
            panic!("record")
        };
        let Value::Num(number) = &record.fields["reference"] else {
            panic!("number")
        };
        assert_eq!(number.dim, length);
        assert_eq!(number.to_f64(), Some(2.0));
        assert!(
            from_kernel_value(
                &raw,
                &Type::Record(Row::closed([(
                    "reference".into(),
                    Type::float(Dim::base("Time"))
                )]))
            )
            .is_err()
        );
        for invalid in [
            aevia_rssn::UnitVal {
                dim: Dim::base("Time"),
                ..unit.clone()
            },
            aevia_rssn::UnitVal {
                affine: true,
                ..unit.clone()
            },
            aevia_rssn::UnitVal {
                offset: Some(aevia_dim::Scale::one()),
                ..unit.clone()
            },
            aevia_rssn::UnitVal {
                scale: aevia_dim::Scale::rational(num_rational::BigRational::from_integer(
                    0.into(),
                )),
                ..unit.clone()
            },
        ] {
            assert!(
                from_kernel_value(
                    &quantity(invalid, Val::Num(aevia_rssn::Num::Float(200.0))),
                    &expected
                )
                .is_err()
            );
        }
        assert!(
            from_kernel_value(
                &quantity(
                    unit,
                    Val::Num(aevia_rssn::Num::Rat(
                        num_rational::BigRational::from_integer(200.into())
                    ))
                ),
                &expected
            )
            .is_err()
        );
    }

    #[test]
    fn dynamic_arguments_and_native_marshaling_borrow_dense_and_boxed_buffers() {
        use crate::value::Storage;
        use crate::value::TensorData;
        use std::borrow::Cow;

        let span = Span::default();
        let unit_args = dynamic_arguments(Some(&Value::Unit), span).unwrap();
        assert!(matches!(unit_args, Cow::Borrowed([])));

        let boxed_list = Value::Tensor(Rc::new(TensorData::new(
            vec![2],
            Storage::Boxed(vec![Value::int(1), Value::int(2)]),
            Dim::one(),
            None,
        )));
        let borrowed = dynamic_arguments(Some(&boxed_list), span).unwrap();
        assert!(matches!(borrowed, Cow::Borrowed([_, _])));

        let dense_list = Value::Tensor(Rc::new(TensorData::new(
            vec![2],
            Storage::Dense(Rc::new(vec![1.0, 2.0])),
            Dim::one(),
            None,
        )));
        let owned = dynamic_arguments(Some(&dense_list), span).unwrap();
        assert!(matches!(owned, Cow::Owned(_)));

        // Small dense float, larger dense float (>= 256), contiguous slice, and strided view
        // all execute cleanly through both pinned `execute_checked` and dynamic `forward`.
        let entry = registry().op("num_l1_norm").unwrap();
        let key = entry.key(0).unwrap();
        for len in [4usize, 300usize] {
            let values: Vec<f64> = (1..=len).map(|i| -(i as f64)).collect();
            let expected_sum: f64 = values.iter().map(|v| v.abs()).sum();
            let dense = Rc::new(TensorData::new(
                vec![len],
                Storage::Dense(Rc::new(values)),
                Dim::one(),
                None,
            ));
            let slice = dense.tail_view(1).unwrap();
            let reversed = Rc::new(dense.reversed_view(0).unwrap());

            for (tensor, expect_val) in [
                (dense, expected_sum),
                (slice, expected_sum - 1.0),
                (reversed, expected_sum),
            ] {
                let args = [Value::Tensor(tensor)];
                let converted = prepare_checked_arguments(&args, Some(entry)).unwrap();
                assert!(matches!(converted, PreparedArguments::Native(_)));
                let checked_val =
                    execute_checked(key, entry, &args, &converted, span, None).unwrap();
                assert_eq!(checked_val.as_f64(), Some(expect_val));

                let dyn_res = forward("num_l1_norm", &args, None);
                let Value::Variant(variant) = dyn_res else {
                    panic!("expected Result variant from forward");
                };
                assert_eq!(variant.name, "Ok");
            }
        }

        // Malformed shape still fails with `E0809` via fallback `bridge::to_val`.
        let malformed = Value::Tensor(Rc::new(TensorData::new(
            vec![5],
            Storage::Dense(Rc::new(vec![1.0, 2.0])),
            Dim::one(),
            None,
        )));
        let Err(err) = prepare_checked_arguments(&[malformed], Some(entry)) else {
            panic!("expected malformed tensor arguments to be rejected");
        };
        assert_eq!(err.code, "E0809");
    }
}
