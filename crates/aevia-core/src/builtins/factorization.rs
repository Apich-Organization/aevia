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
use rayon::iter::IndexedParallelIterator;
use rayon::iter::IntoParallelRefMutIterator;
use rayon::iter::ParallelIterator;
use std::borrow::Cow;
use std::cell::OnceCell;

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

#[allow(
    clippy::single_call_fn,
    reason = "production checks eligibility during elimination while tests check it independently"
)]
fn use_ambient_workers(interp: &Interp) -> bool {
    interp.on_worker
        && rayon::current_thread_index().is_some()
        && !matches!(
            aevia_rssn::execution::default_execution_mode(),
            aevia_rssn::execution::ExecutionMode::Serial
        )
        && (2..=interp.jobs).contains(&aevia_rssn::execution::cpu_threads())
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
    const MIN_PARALLEL_ORDER: usize = 48;
    const MIN_ROW_BATCH: usize = 4;
    const MIN_ACTIVE_ROWS: usize = 12;
    let work_items = rows.len().div_ceil(MIN_ROW_BATCH);
    let ambient = rows.len() >= MIN_PARALLEL_ORDER && use_ambient_workers(interp);
    let parallel = rows.len() >= MIN_PARALLEL_ORDER
        && (ambient
            || !matches!(
                interp.execution_mode(),
                aevia_rssn::execution::ExecutionMode::Serial
            ));
    let workers = OnceCell::new();
    let mut determinant = BigRational::one();
    for column in 0..rows.len() {
        interp.check_cancelled()?;
        let Some((pivot_index, _)) = rows
            .iter()
            .enumerate()
            .skip(column)
            .find(|(_, row)| row.get(column).is_some_and(|entry| !entry.is_zero()))
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
        let previous = if reduced { previous } else { &mut [] };
        let update_row = |row: &mut Vec<BigRational>| {
            let Some(entry) = row.get(column).filter(|entry| !entry.is_zero()) else {
                return;
            };
            let factor = entry / &scale;
            for (value, coefficient) in row.iter_mut().zip(pivot.iter()).skip(column) {
                *value -= &factor * coefficient;
            }
        };
        if parallel
            && pivot.len().saturating_sub(column) >= MIN_ROW_BATCH
            && previous
                .iter()
                .chain(following.iter())
                .filter(|row| row.get(column).is_some_and(|entry| !entry.is_zero()))
                .take(MIN_ACTIVE_ROWS)
                .count()
                == MIN_ACTIVE_ROWS
        {
            let pool = if ambient {
                None
            } else {
                workers
                    .get_or_init(|| crate::parallel::pool(interp.jobs, work_items))
                    .as_ref()
            };
            if ambient || pool.is_some() {
                // Only the pivot row is shared, and all destination rows are disjoint.
                let mut update = || {
                    previous
                        .par_iter_mut()
                        .chain(following.par_iter_mut())
                        .with_min_len(MIN_ROW_BATCH)
                        .for_each(update_row);
                };
                if let Some(pool) = pool {
                    pool.install(update);
                } else {
                    update();
                }
                continue;
            }
        }
        previous.iter_mut().chain(following).for_each(update_row);
    }
    Ok(Some(determinant))
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps determinant handling out of the dispatcher and available to direct tests"
)]
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

#[allow(
    clippy::single_call_fn,
    reason = "keeps inverse handling out of the dispatcher and available to direct tests"
)]
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

#[allow(
    clippy::single_call_fn,
    reason = "keeps square-system handling out of the dispatcher and available to direct tests"
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CancellationToken;
    use crate::value::Storage;
    use aevia_dim::Dim;
    use aevia_types::CheckedModule;

    fn rational(
        numerator: i64,
        denominator: i64,
    ) -> BigRational {
        BigRational::new(numerator.into(), denominator.into())
    }

    fn assert_number(
        value: &Value,
        expected: BigRational,
        dimension: &Dim,
    ) {
        let Value::Num(number) = value else {
            panic!("expected a numeric value, got {value:?}");
        };
        assert_eq!(number.to_rational(), Some(expected));
        assert_eq!(&number.dim, dimension);
    }

    fn solution(result: Value) -> Vec<Value> {
        let Value::Variant(result) = result else {
            panic!("expected a Result, got {result:?}");
        };
        assert_eq!(result.index, 0);
        let [Value::Tensor(tensor)] = result.fields.as_slice() else {
            panic!("expected a tensor payload, got {result:?}");
        };
        tensor.elements()
    }

    #[test]
    fn wide_exact_factorizations_keep_pivot_sign_values_and_dimensions() {
        let size = 48;
        let span = Span::default();
        let length = Dim::base("Length");
        let time = Dim::base("Time");
        let determinant_dim = length.pow(&rational(i64::try_from(size).unwrap(), 1));
        let inverse_dim = length.inverse();
        let solution_dim = time.div(&length);

        // I + U/3 with U[:,0] = 0 and all other entries of U equal to 1
        // has determinant 50/3 and inverse I - U/50. Swapping the first two
        // rows also puts zero in the first pivot position.
        let swapped = |index| {
            match index {
                | 0 => 1,
                | 1 => 0,
                | _ => index,
            }
        };
        let mut values = Vec::new();
        for row in 0..size {
            for column in 0..size {
                let numerator = 3 * i64::from(swapped(row) == column) + i64::from(column != 0);
                values.push(exact_number(rational(numerator, 3), length.clone()));
            }
        }
        let matrix = TensorData::new(vec![size, size], Storage::Boxed(values), length, None);
        let rhs = TensorData::new(
            vec![size],
            Storage::Boxed(vec![exact_number(rational(1, 1), time.clone()); size]),
            time,
            None,
        );

        let mut interp = Interp::new(&CheckedModule::default());
        for jobs in [1, 2] {
            interp.set_jobs(jobs);
            let determinant = determinant(&interp, &matrix, span).unwrap();
            assert_number(&determinant, rational(-50, 3), &determinant_dim);

            let entries = solution(inverse(&interp, &matrix, span).unwrap());
            assert_eq!(entries.len(), size * size);
            for (flat, entry) in entries.iter().enumerate() {
                let column = swapped(flat % size);
                let numerator = 50 * i64::from(flat / size == column) - i64::from(column != 0);
                assert_number(entry, rational(numerator, 50), &inverse_dim);
            }

            let entries = solution(solve(&interp, &matrix, &rhs, span).unwrap());
            assert_eq!(entries.len(), size);
            for entry in &entries {
                assert_number(entry, rational(3, 50), &solution_dim);
            }
        }
    }

    #[test]
    fn wide_elimination_keeps_no_pivot_and_interruption_distinct() {
        let mut interp = Interp::new(&CheckedModule::default());
        interp.set_jobs(2);
        let token = CancellationToken::new();
        interp.set_cancellation_token(token.clone());
        let mut rows = vec![vec![rational(1, 3); 48]; 48];
        assert_eq!(eliminate(&interp, &mut rows.clone(), false).unwrap(), None);

        token.cancel();
        let error = eliminate(&interp, &mut rows, false).unwrap_err();
        assert_eq!(error.code, crate::cancellation::INTERRUPTED);
        assert!(rows.iter().flatten().all(|value| *value == rational(1, 3)));
    }
    #[test]
    fn wide_exact_determinant_uses_only_eligible_ambient_workers() {
        let case = |jobs, expected_ambient| {
            let mut interp = Interp::new(&CheckedModule::default());
            interp.set_jobs(jobs);
            interp.on_worker = true;
            assert_eq!(use_ambient_workers(&interp), expected_ambient);

            let size: usize = 48;
            let length = Dim::base("Length");
            // The rank-one perturbation I + eeᵀ/3 activates the dense pivot path.
            let entries = (0..size * size)
                .map(|entry| {
                    let diagonal = entry / size == entry % size;
                    exact_number(rational(if diagonal { 4 } else { 1 }, 3), length.clone())
                })
                .collect();
            let matrix = TensorData::new(
                vec![size, size],
                Storage::Boxed(entries),
                length.clone(),
                None,
            );
            let result = determinant(&interp, &matrix, Span::default()).unwrap();
            let dimension = length.pow(&rational(i64::try_from(size).unwrap(), 1));
            assert_number(&result, rational(17, 1), &dimension);
        };

        // A worker marker alone does not authorize dispatch on the global pool.
        assert!(rayon::current_thread_index().is_none());
        case(4, false);

        let one = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap();
        one.install(|| case(1, false));

        let two = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        two.install(|| {
            case(2, true);
            case(1, false); // Preserve a deliberately narrower host policy.
            rayon::join(|| case(2, true), || case(2, true));
        });
    }
}
