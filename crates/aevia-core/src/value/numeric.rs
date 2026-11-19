//! Dense IEEE arithmetic and reductions over validated tensor storage.
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

use super::NumKind;
use super::NumVal;
use super::Storage;
use super::TensorData;
use super::Value;
use aevia_dim::Dim;
use aevia_syntax::ast::BinOp;
use num_rational::BigRational;
use std::alloc::Layout;
use std::rc::Rc;

/// Buffers at least this long are split across threads. Each element is
/// computed independently, so the split never changes a result.
const DENSE_PAR_MIN: usize = 1 << 17;

/// `f` over the pairs of two equally long buffers, in parallel when long.
fn zip_map(
    left: &[f64],
    right: &[f64],
    f: impl Fn(f64, f64) -> f64 + Sync + Send,
) -> Vec<f64> {
    use rayon::prelude::*;
    if aevia_rssn::execution::should_split(left.len(), DENSE_PAR_MIN) {
        left.par_iter()
            .zip(right.par_iter())
            .map(|(&a, &b)| f(a, b))
            .collect()
    } else {
        left.iter().zip(right).map(|(&a, &b)| f(a, b)).collect()
    }
}

/// `f` over a buffer, in parallel when long.
fn map_each(
    data: &[f64],
    f: impl Fn(f64) -> f64 + Sync + Send,
) -> Vec<f64> {
    use rayon::prelude::*;
    if aevia_rssn::execution::should_split(data.len(), DENSE_PAR_MIN) {
        data.par_iter().map(|&x| f(x)).collect()
    } else {
        data.iter().map(|&x| f(x)).collect()
    }
}

/// Sum an `f64` slice with Neumaier compensation (§6.6.6, §9.5).
///
/// A second accumulator retains the rounding error of each finite addition.
/// A non-finite running sum keeps its IEEE result, because its rounding error
/// cannot be recovered. A singleton retains its original bits.
#[must_use]
pub fn kahan_sum(data: &[f64]) -> f64 {
    compensated_sum(data.iter().copied())
}

/// Sum an ordered floating-point sequence with Neumaier compensation.
pub fn compensated_sum(mut data: impl ExactSizeIterator<Item = f64>) -> f64 {
    if data.len() == 1 {
        return data.next().unwrap_or(0.0);
    }

    let mut total = 0.0f64;
    let mut compensation = 0.0f64;
    for value in data {
        let next = total + value;
        compensation += if total.abs() >= value.abs() {
            (total - next) + value
        } else {
            (value - next) + total
        };
        total = next;
    }
    if total.is_finite() {
        total + compensation
    } else {
        // The correction for a non-finite addition can itself be NaN.
        total
    }
}

impl TensorData {
    fn dense_result(
        shape: Vec<usize>,
        data: Vec<f64>,
        dim: Dim,
    ) -> Value {
        Value::Tensor(Rc::new(Self {
            verified_element: None,
            shape,
            storage: Storage::Dense(Rc::new(data)),
            dim,
            display: None,
        }))
    }

    /// Elementwise `op` over two `f64` tensors of the same shape.
    ///
    /// Returns `None` — leaving the caller on the boxed path — unless both
    /// operands are `f64` tensors, their shapes are equal (no broadcasting),
    /// and the buffer is non-empty. A strided operand is materialised first
    /// ([`TensorData::dense_flat`]), so a non-contiguous view takes the same
    /// loop over the same values in the same order. Every result is produced
    /// with the same IEEE operation, in the same order, as the boxed loop, so
    /// the two paths are bit-identical.
    ///
    /// The display unit is dropped, exactly as the boxed path drops it: the
    /// elementwise result is a fresh `NumVal::float`, and a `NumVal` built by
    /// arithmetic carries no display unit.
    #[must_use]
    pub fn dense_binary(
        &self,
        op: BinOp,
        right: &Self,
    ) -> Option<Value> {
        if self.shape != right.shape {
            return None;
        }
        let (left_data, right_data) = (self.dense_flat()?, right.dense_flat()?);
        if left_data.is_empty() {
            return None;
        }
        // Choose the operation outside the loop so each optimized kernel
        // computes a single IEEE operation per element.
        let (data, dim) = match op {
            | BinOp::Add => {
                (
                    zip_map(&left_data, &right_data, |a, b| a + b),
                    self.dim.clone(),
                )
            },
            | BinOp::Sub => {
                (
                    zip_map(&left_data, &right_data, |a, b| a - b),
                    self.dim.clone(),
                )
            },
            | BinOp::Mul => {
                (
                    zip_map(&left_data, &right_data, |a, b| a * b),
                    self.dim.mul(&right.dim),
                )
            },
            | BinOp::Div => {
                (
                    zip_map(&left_data, &right_data, |a, b| a / b),
                    self.dim.div(&right.dim),
                )
            },
            | BinOp::Pow => {
                let (&first, rest) = right_data.split_first()?;
                if rest.iter().any(|value| !value.is_finite()) {
                    return None;
                }
                // The first exponent determines the resulting element dimension.
                let exponent = BigRational::from_float(first)?;
                (
                    zip_map(&left_data, &right_data, f64::powf),
                    self.dim.pow(&exponent),
                )
            },
            | _ => return None,
        };
        Some(Self::dense_result(self.shape.clone(), data, dim))
    }

    /// Elementwise `op` between an `f64` tensor and a `Float` scalar.
    ///
    /// `flipped` means the scalar was on the left (`2.0 - xs`). This fast
    /// path handles a `Float` scalar. Integer scalars use the generic path,
    /// whose elementwise arithmetic applies the same Int/Float promotion.
    ///
    /// `scalar ^ tensor` is excluded: its dimension depends on every
    /// exponent, so no single result dimension exists.
    #[must_use]
    pub fn dense_scalar(
        &self,
        op: BinOp,
        scalar: &NumVal,
        flipped: bool,
    ) -> Option<Value> {
        let NumKind::Float(number) = &scalar.kind else {
            return None;
        };
        let number = *number;
        if flipped && op == BinOp::Pow {
            return None;
        }
        let data = self.dense_flat()?;
        if data.is_empty() {
            return None;
        }
        let dim = match op {
            | BinOp::Add | BinOp::Sub => {
                if flipped {
                    scalar.dim.clone()
                } else {
                    self.dim.clone()
                }
            },
            | BinOp::Mul => self.dim.mul(&scalar.dim),
            | BinOp::Div => {
                if flipped {
                    scalar.dim.div(&self.dim)
                } else {
                    self.dim.div(&scalar.dim)
                }
            },
            | BinOp::Pow => self.dim.pow(&BigRational::from_float(number)?),
            | _ => return None,
        };
        let out = match (op, flipped) {
            | (BinOp::Add, false) => map_each(&data, |x| x + number),
            | (BinOp::Add, true) => map_each(&data, |x| number + x),
            | (BinOp::Sub, false) => map_each(&data, |x| x - number),
            | (BinOp::Sub, true) => map_each(&data, |x| number - x),
            | (BinOp::Mul, false) => map_each(&data, |x| x * number),
            | (BinOp::Mul, true) => map_each(&data, |x| number * x),
            | (BinOp::Div, false) => map_each(&data, |x| x / number),
            | (BinOp::Div, true) => map_each(&data, |x| number / x),
            | (BinOp::Pow, false) => map_each(&data, |x| x.powf(number)),
            | _ => return None,
        };
        Some(Self::dense_result(self.shape.clone(), out, dim))
    }

    /// The `+` reduction over an `f64` tensor, using the compensated
    /// summation §6.6.6 and §9.5 ask for.
    ///
    /// A strided view is materialised first, so the fold order — and
    /// therefore the bits — is the one the boxed reduction produces.
    ///
    /// A one-element tensor keeps its display unit, because the boxed
    /// reduction returns the element it started from unchanged.
    #[must_use]
    pub fn dense_sum(&self) -> Option<NumVal> {
        let data = self.dense_flat()?;
        if data.is_empty() {
            return None;
        }
        let total = kahan_sum(&data);
        let mut num = NumVal::float(total, self.dim.clone());
        if data.len() == 1 {
            num.display.clone_from(&self.display);
        }
        Some(num)
    }

    /// The `*` reduction over an `f64` tensor, folded left to right.
    #[must_use]
    pub fn dense_prod(&self) -> Option<NumVal> {
        let data = self.dense_flat()?;
        let (&first, rest) = data.split_first()?;
        let mut total = first;
        for value in rest {
            total *= value;
        }
        let mut num = NumVal::float(total, self.dim.clone());
        if rest.is_empty() {
            num.display.clone_from(&self.display);
        }
        Some(num)
    }

    /// Multiply dense vectors and matrices using the rank pairs in §3.5.3a.
    ///
    /// Matrix products use the blocked kernel in [`aevia_std::linalg::matmul`],
    /// which can round differently from the sequential vector products.
    /// Returns `None` for incompatible shapes, an oversized result or
    /// unavailable float storage. The generic path handles zero extents.
    #[must_use]
    pub fn dense_matmul(
        &self,
        right: &Self,
    ) -> Option<Value> {
        let (rows, inner, right_inner, cols, shape) =
            match (self.shape.as_slice(), right.shape.as_slice()) {
                | (&[rows, inner], &[right_inner, cols]) => {
                    (rows, inner, right_inner, cols, vec![rows, cols])
                },
                | (&[rows, inner], &[right_inner]) => (rows, inner, right_inner, 1, vec![rows]),
                | (&[inner], &[right_inner, cols]) => (1, inner, right_inner, cols, vec![cols]),
                | (&[inner], &[right_inner]) => (1, inner, right_inner, 1, Vec::new()),
                | _ => return None,
            };
        if inner != right_inner || inner == 0 || rows == 0 || cols == 0 {
            return None;
        }
        let count = rows.checked_mul(cols)?;
        Layout::array::<f64>(count).ok()?;
        let (left_data, right_data) = (self.dense_flat()?, right.dense_flat()?);
        let legacy =
            || aevia_std::linalg::legacy::matmul(rows, inner, cols, &left_data, &right_data);
        let data = if shape.len() == 2 {
            aevia_std::linalg::matmul(rows, inner, cols, &left_data, &right_data)
                .unwrap_or_else(|_| legacy())
        } else {
            legacy()
        };
        let dim = self.dim.mul(&right.dim);
        if data.len() != count {
            return None;
        }
        if shape.is_empty() {
            return Some(Value::Num(NumVal::float(*data.first()?, dim)));
        }
        Some(Self::dense_result(shape, data, dim))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_power_rejects_a_non_finite_exponent_after_the_first() {
        let tensor = |values| {
            Value::Tensor(Rc::new(TensorData::new(
                vec![2],
                Storage::Dense(Rc::new(values)),
                Dim::one(),
                None,
            )))
        };
        let base = tensor(vec![3.0, 4.0]);
        let exponent = tensor(vec![2.0, f64::INFINITY]);
        let checked = aevia_types::CheckedModule::default();
        let mut interp = crate::Interp::new(&checked);
        let failure = interp
            .apply_binary("^", base, exponent, aevia_syntax::Span::default())
            .expect_err("every exponent must be finite");
        assert_eq!(failure.code, "E0402");
    }

    #[test]
    fn compensated_sum_preserves_non_finite_ieee_results() {
        assert_eq!(kahan_sum(&[f64::INFINITY]), f64::INFINITY);
        assert_eq!(kahan_sum(&[1.0, f64::INFINITY, 2.0]), f64::INFINITY);
        assert_eq!(kahan_sum(&[f64::NEG_INFINITY, 1.0]), f64::NEG_INFINITY);
        assert_eq!(kahan_sum(&[f64::MAX, f64::MAX]), f64::INFINITY);
        assert!(kahan_sum(&[f64::INFINITY, f64::NEG_INFINITY]).is_nan());
        assert!(kahan_sum(&[1.0, f64::NAN]).is_nan());
    }

    #[test]
    fn sum_preserves_singleton_bits_and_finite_compensation() {
        let payload = f64::from_bits(0x7ff8_0000_0000_0042);
        assert_eq!(kahan_sum(&[payload]).to_bits(), payload.to_bits());
        assert_eq!(kahan_sum(&[-0.0]).to_bits(), (-0.0f64).to_bits());
        assert_eq!(kahan_sum(&[]).to_bits(), 0.0f64.to_bits());
        assert_eq!(kahan_sum(&[1e16, 1.0, -1e16]), 1.0);
    }
}
