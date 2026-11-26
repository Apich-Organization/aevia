//! Dense tensor constructors and products with checked output shapes.
#![cfg_attr(
    not(test),
    deny(
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

use super::checked_element_count;
use super::err;
use super::expect_exact_int;
use super::ok;
use super::shape_error;
use super::zero_like;
use crate::eval::EvalError;
use crate::eval::Interp;
use crate::eval::build_tensor;
use crate::eval::empty_tensor_like;
use crate::int::Int;
use crate::value::ExactRep;
use crate::value::NumKind;
use crate::value::NumVal;
use crate::value::Storage;
use crate::value::TensorData;
use crate::value::Value;
use crate::value::value_dim;
use aevia_dim::Dim;
use aevia_dim::UnitDef;
use aevia_syntax::Span;
use num_rational::BigRational;
use num_traits::ToPrimitive;
use num_traits::Zero;
use std::borrow::Cow;
use std::rc::Rc;

pub(super) fn shape_from_args(
    args: &[Value],
    start: usize,
    name: &str,
    span: Span,
) -> Result<Vec<usize>, EvalError> {
    (start..args.len())
        .map(|index| {
            expect_exact_int(args, index, name, span)?
                .to_usize()
                .ok_or_else(|| {
                    EvalError::new(
                        "E0802",
                        "tensor extents must be representable nonnegative Ints",
                    )
                    .at(span)
                })
        })
        .collect()
}

pub(super) fn reshape(
    tensor: &TensorData,
    shape: Vec<usize>,
) -> Value {
    let Some(storage) = tensor.reshape_storage(&shape) else {
        return err(shape_error(format!(
            "cannot reshape a tensor with shape {:?} into {shape:?}",
            tensor.shape,
        )));
    };
    let result = Value::Tensor(Rc::new(TensorData::new(
        shape,
        storage,
        tensor.dim.clone(),
        tensor.display.clone(),
    )));
    ok(crate::host_witness::preserve_tensor_schema(result, tensor))
}

#[expect(
    clippy::single_call_fn,
    reason = "keeps shape validation out of the dispatcher"
)]
pub(super) fn reshape_dyn(
    tensor: &TensorData,
    dimensions: &TensorData,
) -> Value {
    if dimensions.shape.as_slice() != [tensor.rank()] || dimensions.storage.len() != tensor.rank() {
        return err(shape_error(format!(
            "a rank-{} tensor needs {} shape extents",
            tensor.rank(),
            tensor.rank(),
        )));
    }
    let Some(shape) = (0..tensor.rank())
        .map(|index| dimension_integer(dimensions, index)?.to_usize())
        .collect()
    else {
        return err(shape_error(
            "every shape extent must be a representable nonnegative Int".into(),
        ));
    };
    reshape(tensor, shape)
}

pub(super) fn dimension_integer(
    tensor: &TensorData,
    index: usize,
) -> Option<Int> {
    match tensor.get_flat(index)? {
        | Value::Num(NumVal {
            kind: NumKind::Int(value),
            dim,
            ..
        }) if dim.is_one() => Some(value),
        | _ => None,
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "keeps index validation and copying out of the dispatcher"
)]
pub(super) fn set(
    tensor: &TensorData,
    indices: &TensorData,
    value: Value,
    span: Span,
) -> Result<Value, EvalError> {
    if indices.shape.as_slice() != [tensor.rank()] || indices.storage.len() != tensor.rank() {
        return Err(EvalError::new(
            "E0802",
            format!("`set` needs a vector of {} indices", tensor.rank()),
        )
        .at(span));
    }
    let coordinates = (0..tensor.rank())
        .map(|index| dimension_integer(indices, index)?.to_isize())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| EvalError::new("E0802", "invalid tensor coordinate for `set`").at(span))?;
    let flat = tensor
        .flat_index(&coordinates)
        .ok_or_else(|| EvalError::new("E0802", "`set` index out of range").at(span))?;
    let count = checked_element_count(&tensor.shape, span)?;
    let unreadable = || EvalError::new("E0802", "this tensor's elements cannot be read").at(span);
    if count != tensor.storage.len() {
        return Err(unreadable());
    }
    if let Value::Num(NumVal {
        kind: NumKind::Float(scalar),
        dim,
        ..
    }) = &value
        && *dim == tensor.dim
        && let Some(dense) = tensor.dense_flat()
    {
        let mut values = dense.into_owned();
        let target = values.get_mut(flat).ok_or_else(unreadable)?;
        *target = *scalar;
        let result = dense_output(
            values,
            tensor.shape.clone(),
            tensor.dim.clone(),
            tensor.display.clone(),
        );
        return Ok(crate::host_witness::preserve_tensor_schema(result, tensor));
    }
    let mut elements = output_buffer(count, span)?;
    for index in 0..count {
        elements.push(tensor.get_flat(index).ok_or_else(unreadable)?);
    }
    let target = elements.get_mut(flat).ok_or_else(unreadable)?;
    *target = value;
    let result = build_tensor(elements, tensor.shape.clone(), tensor.dim.clone());
    Ok(crate::host_witness::preserve_tensor_schema(result, tensor))
}

fn invalid_tensor(span: Span) -> EvalError {
    EvalError::new("E0802", "tensor shape and storage disagree").at(span)
}

fn validated_len(
    tensor: &TensorData,
    span: Span,
) -> Result<usize, EvalError> {
    tensor
        .checked_element_count()
        .filter(|count| *count == tensor.storage.len())
        .ok_or_else(|| invalid_tensor(span))
}

#[expect(
    clippy::single_call_fn,
    reason = "tensor transposition dispatches here"
)]
pub(super) fn transpose(
    tensor: &TensorData,
    span: Span,
) -> Result<Value, EvalError> {
    if tensor.rank() < 2 {
        return Err(EvalError::new(
            "E0802",
            format!(
                "transpose expects a tensor of rank 2 or more, got rank {} with shape {:?}",
                tensor.rank(),
                tensor.shape
            ),
        )
        .at(span));
    }
    let count = validated_len(tensor, span)?;
    if count == 0 {
        return Ok(empty_tensor_like(
            tensor,
            tensor.shape.iter().rev().copied().collect(),
        ));
    }
    if let Some(view) = tensor.transposed_view() {
        return Ok(Value::Tensor(Rc::new(view)));
    }
    let mut elements = output_buffer(count, span)?;
    for mut position in 0..count {
        // The innermost output axis is the outermost source axis. Decode each
        // output position in reversed radices and directly form its source index.
        let source = tensor
            .shape
            .iter()
            .try_fold(0usize, |offset, &extent| {
                let coordinate = position.checked_rem(extent)?;
                position = position.checked_div(extent)?;
                offset.checked_mul(extent)?.checked_add(coordinate)
            })
            .ok_or_else(|| invalid_tensor(span))?;
        elements.push(
            tensor
                .get_flat(source)
                .ok_or_else(|| invalid_tensor(span))?,
        );
    }
    let shape = tensor.shape.iter().rev().copied().collect();
    let output = build_tensor(elements, shape, tensor.dim.clone());
    Ok(crate::host_witness::preserve_tensor_schema(output, tensor))
}

#[expect(clippy::single_call_fn, reason = "matrix trace dispatches here")]
pub(super) fn trace(
    interp: &mut Interp,
    tensor: &TensorData,
    span: Span,
) -> Result<Value, EvalError> {
    let [rows, columns] = tensor.shape.as_slice() else {
        return Err(EvalError::new(
            "E0802",
            format!(
                "trace expects a rank-2 tensor, got rank {} with shape {:?}",
                tensor.rank(),
                tensor.shape
            ),
        )
        .at(span));
    };
    if validated_len(tensor, span)? == 0 {
        return Ok(zero_like(tensor));
    }
    let diagonal_len = *rows.min(columns);
    let step = columns.saturating_add(1);
    if let Some(dense) = tensor.dense_flat() {
        let mut sum = 0.0_f64;
        for index in 0..diagonal_len {
            let offset = index
                .checked_mul(step)
                .ok_or_else(|| invalid_tensor(span))?;
            let &entry = dense.get(offset).ok_or_else(|| invalid_tensor(span))?;
            sum += entry;
        }
        return Ok(Value::Num(NumVal {
            kind: NumKind::Float(sum),
            dim: tensor.dim.clone(),
            display: tensor.display.clone(),
        }));
    }
    let get = |index| tensor.get_flat(index).ok_or_else(|| invalid_tensor(span));
    (1..diagonal_len).try_fold(get(0)?, |sum, index| {
        let offset = index
            .checked_mul(step)
            .ok_or_else(|| invalid_tensor(span))?;
        interp.apply_binary("+", sum, get(offset)?, span)
    })
}

#[expect(
    clippy::single_call_fn,
    reason = "both triangular masks share this storage path"
)]
pub(super) fn triangle(
    tensor: &TensorData,
    name: &str,
    span: Span,
) -> Result<Value, EvalError> {
    let [_, columns] = tensor.shape.as_slice() else {
        return Err(EvalError::new("E0802", format!("`{name}` expects a rank-2 tensor")).at(span));
    };
    let count = validated_len(tensor, span)?;
    if count == 0 {
        return Ok(empty_tensor_like(tensor, tensor.shape.clone()));
    }
    let invalid = || invalid_tensor(span);
    let lower = name == "tril";
    let kept = |row: usize| {
        if lower {
            0..row.saturating_add(1).min(*columns)
        } else {
            row.min(*columns)..*columns
        }
    };
    let output = match tensor.dense_flat() {
        | Some(Cow::Borrowed(source)) => {
            let mut output = output_buffer(count, span)?;
            for (row, items) in source.chunks_exact(*columns).enumerate() {
                let kept = kept(row);
                let start = output.len();
                output.resize(start.checked_add(kept.start).ok_or_else(invalid)?, 0.0);
                output.extend_from_slice(items.get(kept).ok_or_else(invalid)?);
                output.resize(start.checked_add(*columns).ok_or_else(invalid)?, 0.0);
            }
            output
        },
        | Some(Cow::Owned(mut output)) => {
            // A non-contiguous view already has a private row-major buffer.
            for (row, items) in output.chunks_exact_mut(*columns).enumerate() {
                let kept = kept(row);
                items.get_mut(..kept.start).ok_or_else(invalid)?.fill(0.0);
                items.get_mut(kept.end..).ok_or_else(invalid)?.fill(0.0);
            }
            output
        },
        // A malformed Float view must fail even when the mask would hide its missing elements.
        | None if matches!(tensor.storage, Storage::View(_)) => return Err(invalid()),
        | None => {
            let mut output = output_buffer(count, span)?;
            let zero = zero_like(tensor);
            for (row, start) in (0..count).step_by(*columns).enumerate() {
                let kept = kept(row);
                let begin = start.checked_add(kept.start).ok_or_else(invalid)?;
                let end = start.checked_add(kept.end).ok_or_else(invalid)?;
                output.resize(begin, zero.clone());
                for index in begin..end {
                    output.push(tensor.get_flat(index).ok_or_else(invalid)?);
                }
                output.resize(
                    start.checked_add(*columns).ok_or_else(invalid)?,
                    zero.clone(),
                );
            }
            let output = build_tensor(output, tensor.shape.clone(), tensor.dim.clone());
            return Ok(crate::host_witness::preserve_tensor_schema(output, tensor));
        },
    };
    Ok(dense_output(
        output,
        tensor.shape.clone(),
        tensor.dim.clone(),
        tensor.display.clone(),
    ))
}

pub(super) fn repeat_float(
    shape: Vec<usize>,
    one: bool,
    span: Span,
) -> Result<Value, EvalError> {
    let count = checked_element_count(&shape, span)?;
    let mut values = output_buffer(count, span)?;
    values.resize(count, f64::from(u8::from(one)));
    Ok(dense_output(values, shape, Dim::one(), None))
}

pub(super) fn repeat_exact(
    shape: Vec<usize>,
    one: bool,
    representation: ExactRep,
    span: Span,
) -> Result<Value, EvalError> {
    let count = checked_element_count(&shape, span)?;
    let mut values = output_buffer(count, span)?;
    values.resize(count, BigRational::from_integer(u8::from(one).into()));
    Ok(Value::Tensor(Rc::new(TensorData::new(
        shape,
        Storage::TypedExact(values, representation),
        Dim::one(),
        None,
    ))))
}

#[expect(
    clippy::single_call_fn,
    reason = "keeps allocation out of the dispatcher"
)]
pub(super) fn fill(
    shape: Vec<usize>,
    value: Value,
    span: Span,
) -> Result<Value, EvalError> {
    let count = checked_element_count(&shape, span)?;
    if let Value::Num(NumVal {
        kind: NumKind::Float(scalar),
        dim,
        display,
    }) = value
    {
        let mut values = output_buffer(count, span)?;
        values.resize(count, scalar);
        return Ok(dense_output(values, shape, dim, display));
    }
    let mut values = output_buffer(count, span)?;
    let dim = value_dim(&value);
    values.resize(count, value);
    Ok(build_tensor(values, shape, dim))
}

#[expect(
    clippy::single_call_fn,
    reason = "keeps interpolation out of the dispatcher"
)]
pub(super) fn linspace(
    start: &NumVal,
    end: &NumVal,
    count: i64,
    span: Span,
) -> Result<Value, EvalError> {
    let count = usize::try_from(count.max(0))
        .map_err(|_| EvalError::new("E0802", "tensor shape is too large").at(span))?;
    let shape = vec![count];
    let mut values = output_buffer(checked_element_count(&shape, span)?, span)?;
    if start.dim != end.dim {
        return Err(
            EvalError::new("E0302", "`linspace` endpoints need the same dimension").at(span),
        );
    }
    let numeric_endpoint =
        || EvalError::new("E0302", "`linspace` endpoints need real numeric values").at(span);
    let left = start.to_f64().ok_or_else(numeric_endpoint)?;
    let right = end.to_f64().ok_or_else(numeric_endpoint)?;
    if count != 0 {
        values.push(left);
    }
    if let Some(segments) = count.checked_sub(1).filter(|segments| *segments != 0) {
        let segments = u32::try_from(segments)
            .map_err(|_| EvalError::new("E0802", "tensor shape is too large").at(span))?;
        let points = (1..segments).map(|index| f64::from(index) / f64::from(segments));
        if left.is_sign_negative() == right.is_sign_negative() {
            let delta = right - left;
            values.extend(points.map(|fraction| delta.mul_add(fraction, left)));
        } else {
            // Weighted endpoints avoid overflow in the interval between opposite extremes.
            values.extend(points.map(|fraction| left.mul_add(1.0 - fraction, right * fraction)));
        }
        values.push(right);
    }
    Ok(dense_output(values, shape, start.dim.clone(), None))
}

#[expect(
    clippy::single_call_fn,
    reason = "keeps matrix construction out of the dispatcher"
)]
pub(super) fn identity(
    extent: i64,
    span: Span,
) -> Result<Value, EvalError> {
    let extent = usize::try_from(extent.max(0))
        .map_err(|_| EvalError::new("E0802", "tensor shape is too large").at(span))?;
    let shape = vec![extent, extent];
    let count = checked_element_count(&shape, span)?;
    let mut values = output_buffer(count, span)?;
    values.resize(count, 0.0);
    for (index, row) in values.chunks_exact_mut(extent.max(1)).enumerate() {
        if let Some(value) = row.get_mut(index) {
            *value = 1.0;
        }
    }
    Ok(dense_output(values, shape, Dim::one(), None))
}

#[expect(
    clippy::single_call_fn,
    reason = "keeps matrix construction out of the dispatcher"
)]
pub(super) fn diagonal(
    tensor: &TensorData,
    span: Span,
) -> Result<Value, EvalError> {
    let invalid = || EvalError::new("E0802", "invalid tensor storage for `diag`").at(span);
    if tensor.checked_element_count() != Some(tensor.storage.len()) {
        return Err(invalid());
    }
    match tensor.shape.as_slice() {
        | [extent] => {
            let shape = vec![*extent, *extent];
            let count = checked_element_count(&shape, span)?;
            if count == 0 {
                return Ok(empty_tensor_like(tensor, shape));
            }
            if let Some(source) = tensor.dense_flat() {
                let mut values = output_buffer(count, span)?;
                values.resize(count, 0.0);
                for (index, (row, value)) in values
                    .chunks_exact_mut(*extent)
                    .zip(source.iter())
                    .enumerate()
                {
                    *row.get_mut(index).ok_or_else(invalid)? = *value;
                }
                return Ok(dense_output(
                    values,
                    shape,
                    tensor.dim.clone(),
                    tensor.display.clone(),
                ));
            }
            let mut zero = tensor.get_flat(0).ok_or_else(invalid)?;
            match &mut zero {
                | Value::Num(num) => {
                    num.kind = match num.kind {
                        | NumKind::Int(_) => NumKind::Int(Int::ZERO),
                        | NumKind::Exact(_) | NumKind::Sym(_) => {
                            NumKind::Exact(Rc::new(BigRational::zero()))
                        },
                        | NumKind::Float(_) => NumKind::Float(0.0),
                        | NumKind::Complex(..) => NumKind::Complex(0.0, 0.0),
                    };
                },
                | Value::Bool(value) => *value = false,
                | _ => {
                    return Err(EvalError::new(
                        "E0302",
                        "`diag` needs a numeric or Boolean vector to fill off-diagonal entries",
                    )
                    .at(span));
                },
            }
            let mut values = output_buffer(count, span)?;
            values.resize(count, zero);
            for (index, row) in values.chunks_exact_mut(*extent).enumerate() {
                *row.get_mut(index).ok_or_else(invalid)? =
                    tensor.get_flat(index).ok_or_else(invalid)?;
            }
            Ok(build_tensor(values, shape, tensor.dim.clone()))
        },
        | [rows, columns] => {
            let size = *rows.min(columns);
            let shape = vec![size];
            if size == 0 {
                return Ok(empty_tensor_like(tensor, shape));
            }
            let stride = columns.checked_add(1).ok_or_else(invalid)?;
            if let Some(source) = tensor.dense() {
                let mut values = output_buffer(size, span)?;
                values.extend(source.iter().step_by(stride).take(size).copied());
                return Ok(dense_output(
                    values,
                    shape,
                    tensor.dim.clone(),
                    tensor.display.clone(),
                ));
            }
            let mut elements = output_buffer(size, span)?;
            for index in 0..size {
                let offset = index.checked_mul(stride).ok_or_else(invalid)?;
                elements.push(tensor.get_flat(offset).ok_or_else(invalid)?);
            }
            let result = build_tensor(elements, shape, tensor.dim.clone());
            Ok(crate::host_witness::preserve_tensor_schema(result, tensor))
        },
        | _ => Err(EvalError::new("E0302", "`diag` expects a vector or matrix").at(span)),
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "keeps numeric paths together outside the dispatcher"
)]
pub(super) fn outer(
    interp: &mut Interp,
    left: &TensorData,
    right: &TensorData,
    span: Span,
) -> Result<Value, EvalError> {
    let ([rows], [columns]) = (left.shape.as_slice(), right.shape.as_slice()) else {
        return Err(EvalError::new("E0302", "`outer` expects two vectors").at(span));
    };
    let invalid = || EvalError::new("E0802", "invalid vector storage for `outer`").at(span);
    if left.storage.len() != *rows || right.storage.len() != *columns {
        return Err(invalid());
    }
    let shape = vec![*rows, *columns];
    let count = checked_element_count(&shape, span)?;
    let dim = left.dim.mul(&right.dim);
    if count == 0 {
        let source = if *rows == 0 { left } else { right };
        let mut output = empty_tensor_like(source, shape);
        if let Value::Tensor(tensor) = &mut output {
            let data = Rc::make_mut(tensor);
            data.dim = dim;
            data.display = None;
            // The source witness may describe a different dimension.
            data.verified_element = None;
        }
        return Ok(output);
    }
    let cancellation = |interp: &Interp| interp.check_cancelled().map_err(|error| error.at(span));
    if let Some(left_values) = left.dense_flat()
        && let Some(right_values) = right.dense_flat()
    {
        let display = crate::eval::product_display(
            &left.dim,
            left.display.as_ref(),
            &right.dim,
            right.display.as_ref(),
        );
        let mut values = output_buffer(count, span)?;
        for &factor in left_values.iter() {
            for chunk in right_values.chunks(4096) {
                cancellation(interp)?;
                values.extend(chunk.iter().map(|&value| factor * value));
            }
        }
        return Ok(dense_output(values, shape, dim, display));
    }
    let mut elements = output_buffer(count, span)?;
    let rhs = right.elements();
    if rhs.len() != *columns {
        return Err(invalid());
    }
    for row in 0..*rows {
        let factor = left.get_flat(row).ok_or_else(invalid)?;
        for chunk in rhs.chunks(4096) {
            cancellation(interp)?;
            for value in chunk {
                elements.push(interp.apply_binary("*", factor.clone(), value.clone(), span)?);
            }
        }
    }
    Ok(build_tensor(elements, shape, dim))
}

fn dense_output(
    values: Vec<f64>,
    shape: Vec<usize>,
    dim: Dim,
    display: Option<Rc<UnitDef>>,
) -> Value {
    Value::Tensor(Rc::new(TensorData::new(
        shape,
        Storage::Dense(Rc::new(values)),
        dim,
        display,
    )))
}

fn output_buffer<T>(
    count: usize,
    span: Span,
) -> Result<Vec<T>, EvalError> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| {
        EvalError::new("E0802", "unable to allocate tensor output storage").at(span)
    })?;
    Ok(values)
}
