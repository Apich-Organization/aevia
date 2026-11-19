//! Runtime adapters for exact and floating-point dense factorizations.
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

use super::exact_number;
use super::float_tensor;
use super::lin_result;
use super::numeric_error;
use super::sequence::reserve_items;
use crate::eval::EvalError;
use crate::eval::Interp;
use crate::eval::build_tensor;
use crate::value::NumKind;
use crate::value::NumVal;
use crate::value::TensorData;
use crate::value::Value;
use aevia_std::NumericError;
use aevia_std::linalg;
use aevia_syntax::Span;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;
use num_traits::Zero;
use std::borrow::Cow;

fn square_order(tensor: &TensorData) -> Result<usize, NumericError> {
    let [rows, cols] = tensor.shape.as_slice() else {
        return Err(NumericError::NotSquare);
    };
    if rows != cols {
        return Err(NumericError::NotSquare);
    }
    if rows.checked_mul(*cols) != Some(tensor.storage.len()) {
        return Err(NumericError::ShapeMismatch(
            "matrix shape disagrees with backing storage".into(),
        ));
    }
    Ok(*rows)
}

fn exact_entry(
    tensor: &TensorData,
    flat: usize,
) -> Option<BigRational> {
    let Value::Num(number) = tensor.get_flat(flat)? else {
        return None;
    };
    match number.kind {
        | NumKind::Int(value) => Some(value.to_rational()),
        | NumKind::Exact(value) => Some((*value).clone()),
        | _ => None,
    }
}

fn exact_rows(
    tensor: &TensorData,
    order: usize,
    width: usize,
    span: Span,
) -> Result<Option<Vec<Vec<BigRational>>>, EvalError> {
    if tensor.storage.is_dense() {
        return Ok(None);
    }
    let count = tensor.storage.len();
    // The caller has verified that count equals order squared.
    let mut rows = reserve_items(order, span)?;
    let mut indices = 0..count;
    for _ in 0..order {
        let mut row = reserve_items(width, span)?;
        for flat in indices.by_ref().take(order) {
            let Some(entry) = exact_entry(tensor, flat) else {
                return Ok(None);
            };
            row.push(entry);
        }
        rows.push(row);
    }
    Ok(Some(rows))
}

fn real_entries<'a>(
    tensor: &'a TensorData,
    operation: &str,
    span: Span,
) -> Result<Cow<'a, [f64]>, EvalError> {
    if let Some(dense) = tensor.dense_flat() {
        return Ok(dense);
    }
    let count = tensor.storage.len();
    let mut values = reserve_items(count, span)?;
    for index in 0..count {
        let element = tensor
            .get_flat(index)
            .ok_or_else(|| EvalError::new("E0802", "invalid matrix backing storage").at(span))?;
        let value = element.as_f64().ok_or_else(|| {
            numeric_error(
                operation,
                NumericError::Domain("entries must be real numbers".into()),
                span,
            )
        })?;
        values.push(value);
    }
    Ok(Cow::Owned(values))
}

/// Forms an upper triangular or reduced matrix and returns its determinant.
/// A missing pivot denotes a singular system.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "rational arithmetic is arbitrary precision and each pivot divisor is nonzero"
)]
fn eliminate(
    interp: &Interp,
    rows: &mut [Vec<BigRational>],
    reduced: bool,
) -> Result<Option<BigRational>, EvalError> {
    let mut determinant = BigRational::one();
    for column in 0..rows.len() {
        interp.check_cancelled()?;
        let Some((offset, _)) = rows
            .iter()
            .skip(column)
            .enumerate()
            .find(|(_, row)| row.get(column).is_some_and(|entry| !entry.is_zero()))
        else {
            return Ok(None);
        };
        let Some(pivot_index) = column
            .checked_add(offset)
            .filter(|&index| index < rows.len())
        else {
            return Ok(None);
        };
        if column != pivot_index {
            rows.swap(column, pivot_index);
            if !reduced {
                determinant = -determinant;
            }
        }
        let Some((previous, remaining)) = rows.split_at_mut_checked(column) else {
            return Ok(None);
        };
        let Some((pivot, following)) = remaining.split_first_mut() else {
            return Ok(None);
        };
        let Some(lead) = pivot.get(column).cloned() else {
            return Ok(None);
        };
        if reduced {
            for value in pivot.iter_mut().skip(column) {
                *value /= &lead;
            }
        } else {
            determinant *= &lead;
        }
        let scale = if reduced {
            BigRational::one()
        } else {
            lead
        };
        for row in previous.iter_mut().filter(|_| reduced).chain(following) {
            let Some(entry) = row.get(column).filter(|entry| !entry.is_zero()) else {
                continue;
            };
            let factor = entry / &scale;
            for (value, coefficient) in row.iter_mut().zip(pivot.iter()).skip(column) {
                *value -= &factor * coefficient;
            }
        }
    }
    Ok(Some(determinant))
}

#[expect(clippy::single_call_fn, reason = "the determinant dispatches here")]
pub(super) fn determinant(
    interp: &Interp,
    tensor: &TensorData,
    span: Span,
) -> Result<Value, EvalError> {
    let order = square_order(tensor).map_err(|error| numeric_error("det", error, span))?;
    if order == 0 {
        return Ok(Value::Num(NumVal::int(1)));
    }
    let dim = tensor
        .dim
        .pow(&BigRational::from_integer(BigInt::from(order)));
    if let Some(mut rows) = exact_rows(tensor, order, order, span)? {
        let value = eliminate(interp, &mut rows, false)?.unwrap_or_else(BigRational::zero);
        return Ok(exact_number(value, dim));
    }
    let values = real_entries(tensor, "det", span)?;
    let value =
        linalg::det(order, order, &values).map_err(|error| numeric_error("det", error, span))?;
    Ok(Value::Num(NumVal::float(value, dim)))
}

#[expect(clippy::single_call_fn, reason = "the inverse dispatches here")]
pub(super) fn inverse(
    interp: &Interp,
    tensor: &TensorData,
    span: Span,
) -> Result<Value, EvalError> {
    let order = match square_order(tensor) {
        | Ok(order) => order,
        | Err(error) => return lin_result(Err(error), span),
    };
    let dim = tensor.dim.inverse();
    let shape = vec![order, order];
    let width = order
        .checked_mul(2)
        .ok_or_else(|| EvalError::new("E0802", "inverse workspace shape is too large").at(span))?;
    if let Some(mut rows) = exact_rows(tensor, order, width, span)? {
        for (index, row) in rows.iter_mut().enumerate() {
            row.extend(
                (0..order)
                    .map(|column| BigRational::from_integer(u8::from(index == column).into())),
            );
        }
        if eliminate(interp, &mut rows, true)?.is_none() {
            return lin_result(Err(NumericError::Singular), span);
        }
        let mut values = reserve_items(tensor.storage.len(), span)?;
        for row in rows {
            values.extend(
                row.into_iter()
                    .skip(order)
                    .map(|value| exact_number(value, dim.clone())),
            );
        }
        return lin_result(Ok(build_tensor(values, shape, dim)), span);
    }
    let values = real_entries(tensor, "inv", span)?;
    lin_result(
        linalg::inv(order, order, &values).map(|data| float_tensor(shape, data, dim)),
        span,
    )
}

#[expect(
    clippy::single_call_fn,
    reason = "the square system solver dispatches here"
)]
pub(super) fn solve(
    interp: &Interp,
    matrix: &TensorData,
    rhs: &TensorData,
    span: Span,
) -> Result<Value, EvalError> {
    let order = match square_order(matrix) {
        | Ok(order) => order,
        | Err(error) => return lin_result(Err(error), span),
    };
    if rhs.shape.as_slice() != [order] || rhs.storage.len() != order {
        return lin_result(
            Err(NumericError::ShapeMismatch(
                "system right-hand side has the wrong shape".into(),
            )),
            span,
        );
    }
    let dim = rhs.dim.div(&matrix.dim);
    let width = order.checked_add(1).ok_or_else(|| {
        EvalError::new("E0802", "linear solve workspace shape is too large").at(span)
    })?;
    if let Some(mut rows) = exact_rows(matrix, order, width, span)? {
        let mut exact_rhs = reserve_items(order, span)?;
        for index in 0..order {
            let Some(value) = exact_entry(rhs, index) else {
                break;
            };
            exact_rhs.push(value);
        }
        if exact_rhs.len() == order {
            for (row, value) in rows.iter_mut().zip(exact_rhs) {
                row.push(value);
            }
            if eliminate(interp, &mut rows, true)?.is_none() {
                return lin_result(Err(NumericError::Singular), span);
            }
            let mut values = reserve_items(order, span)?;
            values.extend(
                rows.into_iter()
                    .filter_map(|mut row| row.pop())
                    .map(|value| exact_number(value, dim.clone())),
            );
            return lin_result(Ok(build_tensor(values, vec![order], dim)), span);
        }
    }
    let values = real_entries(matrix, "linsolve", span)?;
    let rhs = real_entries(rhs, "linsolve", span)?;
    lin_result(
        linalg::solve(order, order, &values, &rhs).map(|data| float_tensor(vec![order], data, dim)),
        span,
    )
}
