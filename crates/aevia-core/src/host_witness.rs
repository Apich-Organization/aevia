//! Provenance for Aevia-origin values crossing the opaque kernel boundary.
//!
//! This never infers a nominal identity from a kernel tag or from a variable
//! annotation. Only checked constructors and checked tensor creation can mint
//! the corresponding private evidence.

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
use crate::value::HostWitness;
use crate::value::HostWitnessNode;
use crate::value::NumKind;
use crate::value::Storage;
use crate::value::TensorData;
use crate::value::Value;
use aevia_types::Rep;
use aevia_types::Row;
use aevia_types::Type;
use aevia_types::adt;
use std::collections::HashMap;

/// Whether a schema has no unresolved parameters or intrinsic markers.
pub fn concrete(ty: &Type) -> bool {
    !matches!(
        ty,
        Type::Error
            | Type::TensorIntrinsic(_)
            | Type::TypeRepIntrinsic(_)
            | Type::FromValueIntrinsic(_)
            | Type::WriteNpyIntrinsic
            | Type::ReadNpyIntrinsic(_)
            | Type::ToCallbackIntrinsic
    ) && !aevia_types::class::has_variables(ty)
}

/// Instantiate the immutable registered constructor's field templates.
#[allow(
    clippy::single_call_fn,
    reason = "takes a registry snapshot and substitutes checked constructor field types"
)]
pub fn constructor_fields(
    adt_id: adt::AdtId,
    index: usize,
    args: &[Type],
) -> Option<Vec<Type>> {
    let registered = adt::get(adt_id)?;
    let ctor = registered.ctors.get(index)?;
    let kinds = adt::param_kinds_of(adt_id);
    if kinds.len() != args.len() {
        return None;
    }
    let mut types = Vec::with_capacity(kinds.len());
    let mut dims = HashMap::new();
    for (kind, arg) in kinds.iter().zip(args) {
        match (kind, arg) {
            | (adt::ParamKind::Type, ty) if concrete(ty) => types.push(Some(ty.clone())),
            | (adt::ParamKind::Dim(id), Type::UnitOf(dim)) if dim.is_concrete() => {
                types.push(None);
                dims.insert(*id, dim.clone());
            },
            | _ => return None,
        }
    }
    let fields: Vec<_> = ctor
        .fields
        .iter()
        .map(|field| field.substitute_kinded(&types, &dims, &HashMap::new(), &HashMap::new()))
        .collect();
    fields.iter().all(concrete).then_some(fields)
}

/// Check a value against concrete source evidence and retain recursive origin.
/// `new_constructor` is true only for the result of an authenticated source
/// constructor call; it never applies to arbitrary values read from a scope.
pub fn capture(
    value: &Value,
    schema: &Type,
    new_constructor: bool,
) -> Option<HostWitness> {
    if !concrete(schema) {
        return None;
    }
    let node = match (value, schema) {
        | (Value::KernelValue(v), ty)
            if aevia_types::adt::is_native_ad_var_type(ty)
                && crate::value::is_native_ad_var_payload(&v.payload) =>
        {
            HostWitnessNode::Scalar
        },
        | (Value::Num(number), Type::Scalar(rep, dim)) => {
            if &number.dim != dim || !scalar_rep_matches(&number.kind, *rep) {
                return None;
            }
            HostWitnessNode::Scalar
        },
        | (Value::Bool(_), Type::Bool) | (Value::Str(_), Type::Str) | (Value::Unit, Type::Unit) => {
            HostWitnessNode::Scalar
        },
        | (Value::Record(record), Type::Record(row)) => {
            if row.tail_var().is_some() {
                return None;
            }
            let fields = row.fields();
            if fields.len() != record.fields.len() {
                return None;
            }
            let mut witnesses = Vec::with_capacity(fields.len());
            for (name, field_ty) in fields {
                let field = record.fields.get(&name)?;
                witnesses.push((name, capture(field, &field_ty, false)?));
            }
            HostWitnessNode::Record(witnesses)
        },
        | (Value::Tensor(tensor), Type::Tensor { shape, elem }) => {
            HostWitnessNode::Tensor(tensor_witnesses(tensor, shape, elem, false)?)
        },
        | (Value::Variant(variant), Type::Adt(expected_id, args)) => {
            if !variant.constructor_origin || !adt::same_type(variant.adt, *expected_id) {
                return None;
            }
            if !new_constructor {
                let Type::Adt(proven_id, proven_args) = variant.verified_type.as_ref()? else {
                    return None;
                };
                if !adt::same_type(*proven_id, *expected_id)
                    || proven_args.len() != args.len()
                    || !proven_args
                        .iter()
                        .zip(args)
                        .all(|(a, b)| strict_schema_equal(a, b))
                {
                    return None;
                }
            }
            let registered = adt::get(variant.adt)?;
            let ctor = registered.ctors.get(variant.index)?;
            if ctor.name != variant.name {
                return None;
            }
            let fields = constructor_fields(variant.adt, variant.index, args)?;
            if fields.len() != variant.fields.len() {
                return None;
            }
            let witnesses = variant
                .fields
                .iter()
                .zip(&fields)
                .map(|(value, ty)| capture(value, ty, false))
                .collect::<Option<Vec<_>>>()?;
            HostWitnessNode::Variant {
                adt: variant.adt,
                index: variant.index,
                tag: variant.name.clone(),
                fields: witnesses,
            }
        },
        | _ => return None,
    };
    Some(HostWitness {
        schema: schema.clone(),
        node,
    })
}

/// Whether a freshly built source constructor result may carry `schema` as
/// its witness without walking its payload.
///
/// The checker proved the application, so the fields already have the field
/// types `schema` implies; the only facts left to establish are the ones the
/// runtime itself contributes: the schema is concrete, and the value is the
/// checked constructor of that type. A deep [`capture`] here made building a
/// list quadratic.
pub fn checked_constructor_result(
    value: &Value,
    schema: &Type,
) -> bool {
    let (Value::Variant(variant), Type::Adt(expected, _)) = (value, schema) else {
        return false;
    };
    concrete(schema) && variant.constructor_origin && adt::same_type(variant.adt, *expected)
}

/// Verify tensor contents before minting element-schema evidence. The caller
/// may declare an empty boxed schema only at authenticated construction.
pub fn tensor_witnesses(
    tensor: &TensorData,
    shape: &aevia_types::Shape,
    elem: &Type,
    declared_empty: bool,
) -> Option<Vec<HostWitness>> {
    if !concrete(elem)
        || tensor.shape.len() != shape.rank() as usize
        || tensor.checked_element_count() != Some(tensor.storage.len())
    {
        return None;
    }
    for (axis, actual) in tensor.shape.iter().enumerate() {
        if shape
            .extent(axis)
            .is_some_and(|expected| expected != *actual)
        {
            return None;
        }
    }
    if let Type::Scalar(_, dim) = elem
        && &tensor.dim != dim
    {
        return None;
    }
    if tensor.storage.is_empty()
        && !empty_tensor_has_schema(tensor, elem)
        && !(declared_empty && matches!(tensor.storage, Storage::Boxed(_)))
    {
        return None;
    }
    // Homogeneous primitive elements need no per-element origin objects. For
    // contiguous primitive buffers their storage and dimension are sufficient.
    match (&tensor.storage, elem) {
        | (_, Type::Scalar(Rep::Float, _)) if tensor.dense().is_some() => return Some(Vec::new()),
        | (Storage::Bool(_), Type::Bool)
        | (
            Storage::Exact(_) | Storage::TypedExact(_, crate::value::ExactRep::Real),
            Type::Scalar(Rep::Real, _),
        ) => return Some(Vec::new()),
        | (
            Storage::TypedExact(values, crate::value::ExactRep::Int),
            Type::Scalar(Rep::Int | Rep::Real, _),
        ) => {
            return values
                .iter()
                .all(num_rational::Ratio::is_integer)
                .then(Vec::new);
        },
        | _ => {},
    }
    if is_leaf_schema(elem) {
        for index in 0..tensor.storage.len() {
            if !leaf_matches(&tensor.get_flat(index)?, elem) {
                return None;
            }
        }
        return Some(Vec::new());
    }
    let mut items = Vec::with_capacity(tensor.storage.len());
    for i in 0..tensor.storage.len() {
        items.push(capture(&tensor.get_flat(i)?, elem, false)?);
    }
    Some(items)
}

/// Whether primitive tensor data can be validated without origin objects.
pub const fn is_leaf_schema(ty: &Type) -> bool {
    matches!(ty, Type::Scalar(..) | Type::Bool | Type::Str | Type::Unit)
}

#[allow(
    clippy::single_call_fn,
    reason = "validates boxed or strided primitive elements without minting witness objects"
)]
fn leaf_matches(
    value: &Value,
    ty: &Type,
) -> bool {
    match (value, ty) {
        | (Value::Num(number), Type::Scalar(rep, dim)) => {
            &number.dim == dim && scalar_rep_matches(&number.kind, *rep)
        },
        | (Value::Bool(_), Type::Bool) | (Value::Str(_), Type::Str) | (Value::Unit, Type::Unit) => {
            true
        },
        | _ => false,
    }
}

const fn scalar_rep_matches(
    kind: &NumKind,
    rep: Rep,
) -> bool {
    matches!(
        (kind, rep),
        (NumKind::Int(_), Rep::Int | Rep::Real)
            | (NumKind::Exact(_) | NumKind::Sym(_), Rep::Real)
            | (NumKind::Float(_), Rep::Float)
            | (NumKind::Complex(..), Rep::Complex)
    )
}

#[allow(
    clippy::single_call_fn,
    reason = "empty storage must carry enough schema evidence in its own right"
)]
fn empty_tensor_has_schema(
    tensor: &TensorData,
    elem: &Type,
) -> bool {
    if tensor
        .verified_element
        .as_deref()
        .is_some_and(|schema| strict_schema_equal(schema, elem))
    {
        return true;
    }
    match (tensor.storage.backing(), elem) {
        | (Storage::Dense(_) | Storage::View(_), Type::Scalar(Rep::Float, dim))
        | (Storage::TypedExact(_, crate::value::ExactRep::Int), Type::Scalar(Rep::Int, dim))
        | (
            Storage::TypedExact(_, crate::value::ExactRep::Real) | Storage::Exact(_),
            Type::Scalar(Rep::Real, dim),
        ) => &tensor.dim == dim,
        | (Storage::Bool(_), Type::Bool) => true,
        | _ => false,
    }
}

/// Compare exact schema identity, including nested nominal arguments. No
/// numeric promotion or rank widening is permitted inside nominal type args.
pub fn strict_schema_equal(
    a: &Type,
    b: &Type,
) -> bool {
    match (a, b) {
        | (Type::Adt(a, xs), Type::Adt(b, ys)) => {
            adt::same_type(*a, *b)
                && xs.len() == ys.len()
                && xs.iter().zip(ys).all(|(a, b)| strict_schema_equal(a, b))
        },
        | (Type::Tensor { shape: a, elem: x }, Type::Tensor { shape: b, elem: y }) => {
            a == b && strict_schema_equal(x, y)
        },
        | (Type::Record(a), Type::Record(b)) => closed_record_matches(a, b, strict_schema_equal),
        | _ => a == b,
    }
}

/// Whether a verified source schema can satisfy an expected conversion schema.
/// Runtime tensor extents are checked separately against the payload.
pub fn schema_compatible(
    source: &Type,
    expected: &Type,
) -> bool {
    match (source, expected) {
        | (Type::Scalar(Rep::Int, a), Type::Scalar(Rep::Real, b)) => a == b,
        | (Type::Tensor { shape: a, elem: x }, Type::Tensor { shape: b, elem: y }) => {
            a.rank() == b.rank() && schema_compatible(x, y)
        },
        | (Type::Adt(a, xs), Type::Adt(b, ys)) => {
            adt::same_type(*a, *b)
                && xs.len() == ys.len()
                && xs.iter().zip(ys).all(|(a, b)| strict_schema_equal(a, b))
        },
        | (Type::Record(a), Type::Record(b)) => closed_record_matches(a, b, schema_compatible),
        | _ => source == expected,
    }
}

/// Compare closed row labels and field types independently of declaration order.
fn closed_record_matches(
    mut left: &Row,
    mut right: &Row,
    field_matches: fn(&Type, &Type) -> bool,
) -> bool {
    let mut fields = HashMap::<&str, &Type>::new();
    while let Row::Extend { label, ty, tail } = right {
        if fields.insert(label, ty).is_some() {
            return false;
        }
        right = tail;
    }
    while let Row::Extend { label, ty, tail } = left {
        if !fields
            .remove(label.as_str())
            .is_some_and(|expected| field_matches(ty, expected))
        {
            return false;
        }
        left = tail;
    }
    matches!((left, right), (Row::Empty, Row::Empty)) && fields.is_empty()
}

/// Preserve an element schema only across an operation that preserves every
/// element. Both the source and the produced tensor are revalidated, because
/// host code may have changed public tensor fields after construction.
pub fn preserve_tensor_schema(
    mut output: Value,
    source: &TensorData,
) -> Value {
    let Some(element) = source.verified_element.as_ref() else {
        return output;
    };
    let Ok(source_rank) = u32::try_from(source.shape.len()) else {
        return output;
    };
    if tensor_witnesses(
        source,
        &aevia_types::Shape::Rank(source_rank),
        element,
        false,
    )
    .is_none()
    {
        return output;
    }
    if let Value::Tensor(tensor) = &mut output {
        let Ok(rank) = u32::try_from(tensor.shape.len()) else {
            return output;
        };
        if tensor_witnesses(tensor, &aevia_types::Shape::Rank(rank), element, true).is_some() {
            std::rc::Rc::make_mut(tensor).verified_element = Some(element.clone());
        }
    }
    output
}

/// Recover only schema facts already carried by a runtime value itself. This
/// is safe for plain host/HOF apply because it does not consult a call span.
pub fn runtime_schema(value: &Value) -> Option<Type> {
    match value {
        | Value::KernelValue(v) if crate::value::is_native_ad_var_payload(&v.payload) => {
            Some(aevia_types::adt::native_ad_var_type())
        },
        | Value::Num(number) => {
            let rep = match &number.kind {
                | NumKind::Int(_) => Rep::Int,
                | NumKind::Exact(_) | NumKind::Sym(_) => Rep::Real,
                | NumKind::Float(_) => Rep::Float,
                | NumKind::Complex(..) => Rep::Complex,
            };
            Some(Type::Scalar(rep, number.dim.clone()))
        },
        | Value::Bool(_) => Some(Type::Bool),
        | Value::Str(_) => Some(Type::Str),
        | Value::Unit => Some(Type::Unit),
        | Value::Variant(variant) => variant.verified_type.clone(),
        | Value::Tensor(tensor) => {
            let elem = tensor.verified_element.as_ref()?;
            Some(Type::tensor(
                aevia_types::Shape::Rank(u32::try_from(tensor.shape.len()).ok()?),
                (**elem).clone(),
            ))
        },
        | Value::Record(record) => {
            let fields = record
                .fields
                .iter()
                .map(|(name, value)| Some((name.clone(), runtime_schema(value)?)))
                .collect::<Option<Vec<_>>>()?;
            Some(Type::Record(aevia_types::Row::closed(fields)))
        },
        | _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_schema_compares_labels_without_accepting_duplicates() {
        let row = |names: &[&str]| {
            Type::Record(names.iter().rev().fold(Row::Empty, |tail, name| {
                Row::Extend {
                    label: (*name).to_owned(),
                    ty: Box::new(Type::int()),
                    tail: Box::new(tail),
                }
            }))
        };
        let one = row(&["left", "right"]);
        let reordered = row(&["right", "left"]);
        let duplicate = row(&["left", "left"]);
        assert!(strict_schema_equal(&one, &reordered));
        assert!(schema_compatible(&one, &reordered));
        assert!(!strict_schema_equal(&duplicate, &one));
        assert!(!schema_compatible(&duplicate, &duplicate));
    }
}
