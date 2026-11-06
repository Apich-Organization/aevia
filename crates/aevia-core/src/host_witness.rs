//! Provenance for Aevia-origin values crossing the opaque kernel boundary.
//!
//! This never infers a nominal identity from a kernel tag or from a variable
//! annotation. Only checked constructors and checked tensor creation can mint
//! the corresponding private evidence.
use crate::value::HostWitness;
use crate::value::HostWitnessNode;
use crate::value::NumKind;
use crate::value::Storage;
use crate::value::TensorData;
use crate::value::Value;
use aevia_types::Rep;
use aevia_types::Type;
use aevia_types::adt;
use std::collections::HashMap;

fn concrete(ty: &Type) -> bool {
    !matches!(
        ty,
        Type::Error
            | Type::TensorIntrinsic(_)
            | Type::TypeRepIntrinsic(_)
            | Type::FromValueIntrinsic(_)
            | Type::ToCallbackIntrinsic
    ) && !aevia_types::class::has_variables(ty)
}

/// Instantiate the immutable registered constructor's field templates.
pub(crate) fn constructor_fields(
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
pub(crate) fn capture(
    value: &Value,
    schema: &Type,
    new_constructor: bool,
) -> Option<HostWitness> {
    if !concrete(schema) {
        return None;
    }
    let node = match (value, schema) {
        | (Value::Num(number), Type::Scalar(rep, dim)) => {
            let matches = matches!(
                (&number.kind, rep),
                (NumKind::Int(_), Rep::Int | Rep::Real)
                    | (NumKind::Exact(_), Rep::Real)
                    | (NumKind::Float(_), Rep::Float)
                    | (NumKind::Complex(..), Rep::Complex)
                    | (NumKind::Sym(_), Rep::Real)
            );
            if !matches || &number.dim != dim {
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

/// Verify tensor contents before minting element-schema evidence. The caller
/// may declare an empty boxed schema only at authenticated construction.
pub(crate) fn tensor_witnesses(
    tensor: &TensorData,
    shape: &aevia_types::Shape,
    elem: &Type,
    declared_empty: bool,
) -> Option<Vec<HostWitness>> {
    if !concrete(elem)
        || tensor.shape.len() != shape.rank() as usize
        || tensor
            .shape
            .iter()
            .try_fold(1usize, |a, b| a.checked_mul(*b))
            != Some(tensor.storage.len())
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
    if let Type::Scalar(_, dim) = elem {
        if &tensor.dim != dim {
            return None;
        }
    }
    if tensor.storage.is_empty() && !empty_tensor_has_schema(tensor, elem) {
        if !(declared_empty && matches!(tensor.storage, Storage::Boxed(_))) {
            return None;
        }
    }
    // Homogeneous primitive elements need no per-element origin objects: the
    // payload and the independently stored dimension witness validate them.
    if is_leaf_schema(elem) {
        for i in 0..tensor.storage.len() {
            if !leaf_matches(&tensor.get_flat(i)?, elem) {
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

pub(crate) fn is_leaf_schema(ty: &Type) -> bool {
    matches!(ty, Type::Scalar(..) | Type::Bool | Type::Str | Type::Unit)
}

fn leaf_matches(
    value: &Value,
    ty: &Type,
) -> bool {
    match (value, ty) {
        | (Value::Num(number), Type::Scalar(rep, dim)) => {
            &number.dim == dim
                && matches!(
                    (&number.kind, rep),
                    (NumKind::Int(_), Rep::Int | Rep::Real)
                        | (NumKind::Exact(_), Rep::Real)
                        | (NumKind::Float(_), Rep::Float)
                        | (NumKind::Complex(..), Rep::Complex)
                        | (NumKind::Sym(_), Rep::Real)
                )
        },
        | (Value::Bool(_), Type::Bool) | (Value::Str(_), Type::Str) | (Value::Unit, Type::Unit) => {
            true
        },
        | _ => false,
    }
}

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
    match (&tensor.storage, elem) {
        | (Storage::Dense(_) | Storage::View(_), Type::Scalar(Rep::Float, dim)) => {
            &tensor.dim == dim
        },
        | (Storage::TypedExact(_, crate::value::ExactRep::Int), Type::Scalar(Rep::Int, dim)) => {
            &tensor.dim == dim
        },
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
pub(crate) fn strict_schema_equal(
    a: &Type,
    b: &Type,
) -> bool {
    if a == b {
        return true;
    }
    match (a, b) {
        | (Type::Adt(a, xs), Type::Adt(b, ys)) => {
            adt::same_type(*a, *b)
                && xs.len() == ys.len()
                && xs.iter().zip(ys).all(|(a, b)| strict_schema_equal(a, b))
        },
        | (Type::Tensor { shape: a, elem: x }, Type::Tensor { shape: b, elem: y }) => {
            a == b && strict_schema_equal(x, y)
        },
        | (Type::Record(a), Type::Record(b)) => {
            let mut count_a = 0usize;
            let mut cursor = a;
            loop {
                match cursor {
                    | aevia_types::Row::Empty => break,
                    | aevia_types::Row::Var(_) => return false,
                    | aevia_types::Row::Extend { label, ty, tail } => {
                        if !b
                            .field(label)
                            .is_some_and(|other| strict_schema_equal(ty, other))
                        {
                            return false;
                        }
                        count_a += 1;
                        cursor = tail;
                    },
                }
            }
            let mut count_b = 0usize;
            let mut cursor = b;
            loop {
                match cursor {
                    | aevia_types::Row::Empty => break,
                    | aevia_types::Row::Var(_) => return false,
                    | aevia_types::Row::Extend { tail, .. } => {
                        count_b += 1;
                        cursor = tail;
                    },
                }
            }
            count_a == count_b
        },
        | _ => false,
    }
}

/// Whether a verified source schema can satisfy an expected conversion schema.
/// Runtime tensor extents are checked separately against the payload.
pub(crate) fn schema_compatible(
    source: &Type,
    expected: &Type,
) -> bool {
    use aevia_types::Row;
    if source == expected {
        return true;
    }
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
        | (Type::Record(a), Type::Record(b)) => {
            fn count(row: &Row) -> Option<usize> {
                match row {
                    | Row::Empty => Some(0),
                    | Row::Var(_) => None,
                    | Row::Extend { tail, .. } => Some(1 + count(tail)?),
                }
            }
            if count(a).is_none() || count(a) != count(b) {
                return false;
            }
            let mut cursor = a;
            loop {
                match cursor {
                    | Row::Empty => return true,
                    | Row::Var(_) => return false,
                    | Row::Extend { label, ty, tail } => {
                        if !b
                            .field(label)
                            .is_some_and(|expected| schema_compatible(ty, expected))
                        {
                            return false;
                        }
                        cursor = tail;
                    },
                }
            }
        },
        | _ => false,
    }
}

/// Preserve an element schema only across an operation that preserves every
/// element. Both the source and the produced tensor are revalidated, because
/// host code may have changed public tensor fields after construction.
pub(crate) fn preserve_tensor_schema(
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
pub(crate) fn runtime_schema(value: &Value) -> Option<Type> {
    match value {
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
