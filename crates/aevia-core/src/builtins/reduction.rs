//! Ordered axis reductions over dense and exact tensor storage.
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
use super::exact_number;
use super::exact_values;
use super::float_tensor;
use super::numeric_error;
use super::record_of;
use super::sequence::reserve_items;
use super::tensor_entries;
use super::zero_of;
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
use crate::value::compensated_sum;
use aevia_dim::Dim;
use aevia_std::stats;
use aevia_syntax::Span;
use num_rational::BigRational;
use num_traits::ToPrimitive;
use std::borrow::Cow;
use std::rc::Rc;

fn shape_error(span: Span) -> EvalError {
    EvalError::new("E0802", "tensor shape and storage disagree").at(span)
}

#[expect(clippy::single_call_fn, reason = "the axis reduction dispatches here")]
pub(super) fn sum_axis(
    interp: &mut Interp,
    tensor: &Rc<TensorData>,
    axis: usize,
    span: Span,
) -> Result<Value, EvalError> {
    let Some((_, [extent, tail @ ..])) = tensor.shape.split_at_checked(axis) else {
        return Err(EvalError::new("E0802", "axis out of range").at(span));
    };
    if tensor.checked_element_count() != Some(tensor.storage.len()) {
        return Err(shape_error(span));
    }
    let mut shape = tensor.shape.clone();
    *shape.get_mut(axis).ok_or_else(|| shape_error(span))? = 1;
    let count = checked_element_count(&shape, span)?;
    if count == 0 {
        return Ok(empty_tensor_like(tensor, shape));
    }
    if *extent == 0 {
        let mut values = reserve_items(count, span)?;
        values.resize(count, Value::Num(zero_of(tensor)));
        return Ok(build_tensor(values, shape, tensor.dim.clone()));
    }
    let stride = checked_element_count(tail, span)?;
    let block_len = stride
        .checked_mul(*extent)
        .ok_or_else(|| shape_error(span))?;
    if stride == 0 || block_len == 0 {
        return Err(shape_error(span));
    }
    if *extent == 1 {
        return Ok(Value::Tensor(tensor.clone()));
    }
    if let Some(values) = tensor.dense_flat() {
        let mode = interp.execution_mode();
        if aevia_rssn::should_split_for_mode(values.len(), 65_536, mode) && count >= 64 {
            let mut sums = reserve_items(count, span)?;
            sums.resize(count, 0.0);
            let source = values.as_ref();
            aevia_rssn::par_for_chunks_mut_in_mode(&mut sums, 64, 64, mode, |base, chunk| {
                for (local, slot) in chunk.iter_mut().enumerate() {
                    let idx = base.saturating_add(local);
                    let block_idx = idx.checked_div(stride).unwrap_or(0);
                    let offset = idx.checked_rem(stride).unwrap_or(0);
                    let block_start = block_idx.saturating_mul(block_len);
                    let block_end = block_start.saturating_add(block_len);
                    if let Some(sequence) = source
                        .get(block_start..block_end)
                        .and_then(|block| block.get(offset..))
                    {
                        *slot = compensated_sum(sequence.iter().step_by(stride).copied());
                    }
                }
            });
            interp.check_cancelled()?;
            return Ok(float_tensor(shape, sums, tensor.dim.clone()));
        }
        let mut sums = reserve_items(count, span)?;
        for block in values.chunks_exact(block_len) {
            interp.check_cancelled()?;
            for offset in 0..stride {
                let sequence = block.get(offset..).ok_or_else(|| shape_error(span))?;
                sums.push(compensated_sum(sequence.iter().step_by(stride).copied()));
            }
        }
        return Ok(float_tensor(shape, sums, tensor.dim.clone()));
    }
    let mut elements = reserve_items(tensor.storage.len(), span)?;
    for index in 0..tensor.storage.len() {
        elements.push(tensor.get_flat(index).ok_or_else(|| shape_error(span))?);
    }
    fold_elements(
        interp, tensor, shape, count, stride, block_len, &elements, span,
    )
}

#[expect(
    clippy::single_call_fn,
    clippy::too_many_arguments,
    reason = "folds the boxed representation with the same checked axis geometry"
)]
fn fold_elements(
    interp: &mut Interp,
    tensor: &TensorData,
    shape: Vec<usize>,
    count: usize,
    stride: usize,
    block_len: usize,
    elements: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let all_float = elements.iter().all(|element| {
        matches!(
            element,
            Value::Num(NumVal {
                kind: NumKind::Float(_),
                ..
            })
        )
    });
    let mut sums = reserve_items(count, span)?;
    for block in elements.chunks_exact(block_len) {
        interp.check_cancelled()?;
        for offset in 0..stride {
            let mut sequence = block
                .get(offset..)
                .ok_or_else(|| shape_error(span))?
                .iter()
                .step_by(stride);
            if all_float {
                let total =
                    compensated_sum(sequence.map(|value| value.as_f64().unwrap_or(f64::NAN)));
                sums.push(Value::Num(NumVal::float(total, tensor.dim.clone())));
            } else {
                let first = sequence.next().ok_or_else(|| shape_error(span))?.clone();
                let total = sequence.try_fold(first, |sum, item| {
                    interp.apply_binary("+", sum, item.clone(), span)
                })?;
                sums.push(total);
            }
        }
    }
    Ok(build_tensor(sums, shape, tensor.dim.clone()))
}

#[expect(
    clippy::single_call_fn,
    reason = "the descriptive reduction dispatches here"
)]
pub(super) fn mean(
    tensor: &TensorData,
    span: Span,
) -> Result<Value, EvalError> {
    let dim = tensor.dim.clone();
    if let Some(values) = tensor.dense_flat() {
        if values.is_empty() {
            // The Float mean of an empty tensor is IEEE NaN (§6.6.6).
            return Ok(Value::Num(NumVal::float(f64::NAN, dim)));
        }
        let average = stats::mean(&values).map_err(|error| numeric_error("mean", error, span))?;
        return Ok(Value::Num(NumVal::float(average, dim)));
    }
    if let Some(values) = exact_values(tensor) {
        let average =
            stats::mean_exact(&values).map_err(|error| numeric_error("mean", error, span))?;
        return Ok(exact_number(average, dim));
    }
    let values = tensor_entries(tensor).map_err(|error| error.at(span))?;
    let average = stats::mean(&values).map_err(|error| numeric_error("mean", error, span))?;
    Ok(Value::Num(NumVal::float(average, dim)))
}

fn sort_exact_sample(values: &mut Cow<'_, [BigRational]>) {
    if !values.is_sorted() {
        values.to_mut().sort_unstable();
    }
}

fn has_float_storage(tensor: &TensorData) -> bool {
    matches!(
        tensor.storage.backing(),
        Storage::Dense(_) | Storage::View(_)
    )
}

#[expect(clippy::single_call_fn, reason = "the scalar quantile dispatches here")]
pub(super) fn quantile(
    tensor: &TensorData,
    fraction: &NumVal,
    span: Span,
) -> Result<Value, EvalError> {
    let failed = |error| numeric_error("quantile", error, span);
    if !has_float_storage(tensor)
        && let Some(mut sample) = exact_values(tensor)
        && let Some(probability) = fraction.to_rational()
    {
        sort_exact_sample(&mut sample);
        let value = stats::quantile_exact(&sample, &probability).map_err(failed)?;
        return Ok(exact_number(value, tensor.dim.clone()));
    }
    let probability = fraction
        .to_f64()
        .ok_or_else(|| EvalError::new("E0806", "`quantile` needs a concrete fraction").at(span))?;
    let sample = tensor_entries(tensor).map_err(|error| error.at(span))?;
    let value = stats::quantile(&sample, probability).map_err(failed)?;
    Ok(Value::Num(NumVal::float(value, tensor.dim.clone())))
}

#[expect(clippy::single_call_fn, reason = "the batch quantile dispatches here")]
pub(super) fn quantiles(
    tensor: &TensorData,
    probabilities: &TensorData,
    span: Span,
) -> Result<Value, EvalError> {
    if tensor.shape.len() != 1 || probabilities.shape.len() != 1 {
        return Err(EvalError::new("E0802", "`quantiles` needs two vectors").at(span));
    }
    if !probabilities.dim.is_one() {
        return Err(
            EvalError::new("E0401", "`quantiles` needs dimensionless probabilities").at(span),
        );
    }
    let failed = |error| numeric_error("quantiles", error, span);
    let storage = if !has_float_storage(tensor)
        && !has_float_storage(probabilities)
        && let Some(queries) = exact_values(probabilities)
        && let Some(mut sample) = exact_values(tensor)
    {
        if !queries.is_empty() {
            sort_exact_sample(&mut sample);
        }
        let results = stats::quantiles_exact(&sample, &queries).map_err(failed)?;
        Storage::TypedExact(results, ExactRep::Real)
    } else {
        let sample = tensor_entries(tensor).map_err(|error| error.at(span))?;
        let queries = tensor_entries(probabilities).map_err(|error| error.at(span))?;
        let results = stats::quantiles(&sample, &queries).map_err(failed)?;
        Storage::Dense(Rc::new(results))
    };
    Ok(Value::Tensor(Rc::new(TensorData::new(
        probabilities.shape.clone(),
        storage,
        tensor.dim.clone(),
        tensor.display.clone(),
    ))))
}

#[expect(
    clippy::single_call_fn,
    reason = "variance and deviation share the same reduction path"
)]
pub(super) fn variance_or_std(
    tensor: &TensorData,
    name: &str,
    population: bool,
    span: Span,
) -> Result<Value, EvalError> {
    let variance = name == "var";
    let dim = if variance {
        tensor.dim.pow(&BigRational::from_integer(2.into()))
    } else {
        tensor.dim.clone()
    };
    let failed = |error| numeric_error(name, error, span);
    if let Some(values) = exact_values(tensor) {
        let value = stats::var_exact(&values, population).map_err(failed)?;
        if variance {
            return Ok(exact_number(value, dim));
        }
        return Ok(match aevia_std::linalg::exact_sqrt(&value) {
            | Some(root) => exact_number(root, dim),
            | None => Value::Num(NumVal::float(aevia_dim::rat::to_f64(&value).sqrt(), dim)),
        });
    }
    let values = tensor_entries(tensor).map_err(|error| error.at(span))?;
    let value = if variance {
        stats::var(&values, population)
    } else {
        stats::std_dev(&values, population)
    }
    .map_err(failed)?;
    Ok(Value::Num(NumVal::float(value, dim)))
}

#[expect(
    clippy::single_call_fn,
    reason = "keeps validated histogram allocation out of the dispatcher"
)]
pub(super) fn histogram(
    tensor: &TensorData,
    bins: &Int,
    span: Span,
) -> Result<Value, EvalError> {
    let bins = bins.to_usize().filter(|bins| *bins > 0).ok_or_else(|| {
        EvalError::new("E0802", "`histogram` needs a positive bin count").at(span)
    })?;
    let edge_count = bins
        .checked_add(1)
        .ok_or_else(|| EvalError::new("E0802", "`histogram` has too many bins").at(span))?;
    checked_element_count(&[edge_count], span)?;
    let sample = tensor_entries(tensor).map_err(|error| error.at(span))?;
    let (counts, edges) =
        stats::histogram(&sample, bins).map_err(|error| numeric_error("histogram", error, span))?;
    let counts = counts.into_iter().map(Value::int).collect();
    Ok(record_of(vec![
        ("counts", build_tensor(counts, vec![bins], Dim::one())),
        (
            "edges",
            Value::Tensor(Rc::new(TensorData::new(
                vec![edge_count],
                Storage::Dense(Rc::new(edges)),
                tensor.dim.clone(),
                tensor.display.clone(),
            ))),
        ),
    ]))
}
