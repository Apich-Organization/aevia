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
use super::View;
use aevia_dim::Dim;
use aevia_syntax::ast::BinOp;
use num_rational::BigRational;
use std::alloc::Layout;
use std::rc::Rc;

/// Buffers at least this long are split across threads. Each element is
/// computed independently, so the split never changes a result.
const DENSE_PAR_MIN: usize = 1 << 17;
/// Chunk size for dense broadcast loops.
const BROADCAST_CHUNK: usize = 4_096;

macro_rules! compensated_add {
    ($total:ident, $compensation:ident, $value:ident) => {{
        let next = $total + $value;
        $compensation += if $total.abs() >= $value.abs() {
            ($total - next) + $value
        } else {
            ($value - next) + $total
        };
        $total = next;
    }};
}

/// `f` over the pairs of two equally long buffers, in parallel when long.
fn zip_map(
    left: &[f64],
    right: &[f64],
    mode: aevia_rssn::execution::ExecutionMode,
    f: impl Fn(f64, f64) -> f64 + Sync + Send,
) -> Vec<f64> {
    use rayon::prelude::*;
    if !aevia_rssn::should_split_for_mode(left.len(), DENSE_PAR_MIN, mode) {
        return left.iter().zip(right).map(|(&a, &b)| f(a, b)).collect();
    }
    aevia_rssn::run_in_mode(mode, || {
        left.par_iter()
            .zip(right.par_iter())
            .map(|(&a, &b)| f(a, b))
            .collect()
    })
}

/// Computes broadcast shape and per-axis strides (0 for broadcast axes) for
/// two equal-rank dense tensors.
#[expect(
    clippy::single_call_fn,
    reason = "broadcast geometry is isolated from the arithmetic kernel"
)]
fn broadcast_plan(
    left_shape: &[usize],
    right_shape: &[usize],
) -> Option<(Vec<usize>, Vec<usize>, Vec<usize>, usize)> {
    if left_shape.len() != right_shape.len() {
        return None;
    }
    let rank = left_shape.len();
    let mut out_shape = Vec::with_capacity(rank);
    let mut count = 1usize;
    for (&a, &b) in left_shape.iter().zip(right_shape) {
        let dim = if a == b {
            a
        } else if a == 1 {
            b
        } else if b == 1 {
            a
        } else {
            return None;
        };
        count = count.checked_mul(dim)?;
        out_shape.push(dim);
    }
    if count == 0 {
        return None;
    }
    let mut left_strides = vec![0usize; rank];
    let mut right_strides = vec![0usize; rank];
    let mut l_stride = 1usize;
    let mut r_stride = 1usize;
    for axis in (0..rank).rev() {
        let l_ext = *left_shape.get(axis)?;
        let r_ext = *right_shape.get(axis)?;
        if l_ext > 1
            && let Some(slot) = left_strides.get_mut(axis)
        {
            *slot = l_stride;
        }
        if r_ext > 1
            && let Some(slot) = right_strides.get_mut(axis)
        {
            *slot = r_stride;
        }
        l_stride = l_stride.checked_mul(l_ext)?;
        r_stride = r_stride.checked_mul(r_ext)?;
    }
    Some((out_shape, left_strides, right_strides, count))
}

/// Computes a broadcasted dense `f64` binary op across chunks, in parallel
/// when `count` is large and `mode` allows parallel execution.
#[expect(
    clippy::too_many_arguments,
    reason = "broadcast kernel takes prevalidated shapes, strides, slices, and mode"
)]
fn broadcast_map(
    left: &[f64],
    right: &[f64],
    out_shape: &[usize],
    left_strides: &[usize],
    right_strides: &[usize],
    count: usize,
    mode: aevia_rssn::execution::ExecutionMode,
    f: impl Fn(f64, f64) -> f64 + Sync + Send,
) -> Vec<f64> {
    let mut out = vec![0.0; count];
    aevia_rssn::par_for_chunks_mut_in_mode(
        &mut out,
        BROADCAST_CHUNK,
        DENSE_PAR_MIN,
        mode,
        |base, chunk| {
            for (local, slot) in chunk.iter_mut().enumerate() {
                let mut rem = base.saturating_add(local);
                let mut l_idx = 0usize;
                let mut r_idx = 0usize;
                for axis in (0..out_shape.len()).rev() {
                    let extent = out_shape.get(axis).copied().unwrap_or(1);
                    let coord = rem.checked_rem(extent).unwrap_or(0);
                    rem = rem.checked_div(extent).unwrap_or(0);
                    let ls = left_strides.get(axis).copied().unwrap_or(0);
                    let rs = right_strides.get(axis).copied().unwrap_or(0);
                    l_idx = l_idx.saturating_add(coord.saturating_mul(ls));
                    r_idx = r_idx.saturating_add(coord.saturating_mul(rs));
                }
                let a = left.get(l_idx).copied().unwrap_or(0.0);
                let b = right.get(r_idx).copied().unwrap_or(0.0);
                *slot = f(a, b);
            }
        },
    );
    out
}

/// `f` over a buffer, in parallel when long and `mode` permits splitting.
fn map_each(
    data: &[f64],
    mode: aevia_rssn::execution::ExecutionMode,
    f: impl Fn(f64) -> f64 + Sync + Send,
) -> Vec<f64> {
    aevia_rssn::par_map_f64_slice(data, DENSE_PAR_MIN, mode, f)
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
        compensated_add!(total, compensation, value);
    }
    compensated_total(total, compensation)
}

fn compensated_total(
    total: f64,
    compensation: f64,
) -> f64 {
    if total.is_finite() {
        total + compensation
    } else {
        // The correction for a non-finite addition can itself be NaN.
        total
    }
}

impl TensorData {
    fn strided_float_view(&self) -> Option<&View> {
        match &self.storage {
            | Storage::View(view) if view.contiguous_slice(&self.shape).is_none() => Some(view),
            | _ => None,
        }
    }

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
        self.dense_binary_in_mode(op, right, aevia_rssn::execution::default_execution_mode())
    }

    /// Elementwise `op` between two dense `f64` tensors (equal shape or
    /// broadcast-compatible same rank) under an explicit
    /// [`aevia_rssn::execution::ExecutionMode`].
    #[must_use]
    pub fn dense_binary_in_mode(
        &self,
        op: BinOp,
        right: &Self,
        mode: aevia_rssn::execution::ExecutionMode,
    ) -> Option<Value> {
        let (left_data, right_data) = (self.dense_flat()?, right.dense_flat()?);
        if left_data.is_empty() || right_data.is_empty() {
            return None;
        }
        if self.shape == right.shape {
            let (data, dim) = match op {
                | BinOp::Add => {
                    (
                        aevia_rssn::simd_binary_f64_in_mode(
                            aevia_rssn::SimdBinaryOp::Add,
                            &left_data,
                            &right_data,
                            mode,
                        ),
                        self.dim.clone(),
                    )
                },
                | BinOp::Sub => {
                    (
                        aevia_rssn::simd_binary_f64_in_mode(
                            aevia_rssn::SimdBinaryOp::Sub,
                            &left_data,
                            &right_data,
                            mode,
                        ),
                        self.dim.clone(),
                    )
                },
                | BinOp::Mul => {
                    (
                        aevia_rssn::simd_binary_f64_in_mode(
                            aevia_rssn::SimdBinaryOp::Mul,
                            &left_data,
                            &right_data,
                            mode,
                        ),
                        self.dim.mul(&right.dim),
                    )
                },
                | BinOp::Div => {
                    (
                        zip_map(&left_data, &right_data, mode, |a, b| a / b),
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
                        zip_map(&left_data, &right_data, mode, f64::powf),
                        self.dim.pow(&exponent),
                    )
                },
                | _ => return None,
            };
            return Some(Self::dense_result(self.shape.clone(), data, dim));
        }

        let (out_shape, left_strides, right_strides, count) =
            broadcast_plan(&self.shape, &right.shape)?;
        Layout::array::<f64>(count).ok()?;
        let (data, dim) = match op {
            | BinOp::Add => {
                (
                    broadcast_map(
                        &left_data,
                        &right_data,
                        &out_shape,
                        &left_strides,
                        &right_strides,
                        count,
                        mode,
                        |a, b| a + b,
                    ),
                    self.dim.clone(),
                )
            },
            | BinOp::Sub => {
                (
                    broadcast_map(
                        &left_data,
                        &right_data,
                        &out_shape,
                        &left_strides,
                        &right_strides,
                        count,
                        mode,
                        |a, b| a - b,
                    ),
                    self.dim.clone(),
                )
            },
            | BinOp::Mul => {
                (
                    broadcast_map(
                        &left_data,
                        &right_data,
                        &out_shape,
                        &left_strides,
                        &right_strides,
                        count,
                        mode,
                        |a, b| a * b,
                    ),
                    self.dim.mul(&right.dim),
                )
            },
            | BinOp::Div => {
                (
                    broadcast_map(
                        &left_data,
                        &right_data,
                        &out_shape,
                        &left_strides,
                        &right_strides,
                        count,
                        mode,
                        |a, b| a / b,
                    ),
                    self.dim.div(&right.dim),
                )
            },
            | BinOp::Pow => {
                let (&first, rest) = right_data.split_first()?;
                if rest.iter().any(|value| !value.is_finite()) {
                    return None;
                }
                let exponent = BigRational::from_float(first)?;
                (
                    broadcast_map(
                        &left_data,
                        &right_data,
                        &out_shape,
                        &left_strides,
                        &right_strides,
                        count,
                        mode,
                        f64::powf,
                    ),
                    self.dim.pow(&exponent),
                )
            },
            | _ => return None,
        };
        Some(Self::dense_result(out_shape, data, dim))
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
        self.dense_scalar_in_mode(
            op,
            scalar,
            flipped,
            aevia_rssn::execution::default_execution_mode(),
        )
    }

    /// Elementwise `op` between an `f64` tensor and a `Float` or `Int` scalar
    /// under an explicit [`aevia_rssn::execution::ExecutionMode`].
    #[must_use]
    pub fn dense_scalar_in_mode(
        &self,
        op: BinOp,
        scalar: &NumVal,
        flipped: bool,
        mode: aevia_rssn::execution::ExecutionMode,
    ) -> Option<Value> {
        let number = match &scalar.kind {
            | NumKind::Float(value) => *value,
            | NumKind::Int(_) => scalar.to_f64()?,
            | _ => return None,
        };
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
            | BinOp::Pow => self.dim.pow(&scalar.to_rational()?),
            | _ => return None,
        };
        let out = match (op, flipped) {
            | (BinOp::Add, false) => map_each(&data, mode, |x| x + number),
            | (BinOp::Add, true) => map_each(&data, mode, |x| number + x),
            | (BinOp::Sub, false) => map_each(&data, mode, |x| x - number),
            | (BinOp::Sub, true) => map_each(&data, mode, |x| number - x),
            | (BinOp::Mul, false | true) => aevia_rssn::simd_scale_f64_in_mode(&data, number, mode),
            | (BinOp::Div, false) => map_each(&data, mode, |x| x / number),
            | (BinOp::Div, true) => map_each(&data, mode, |x| number / x),
            | (BinOp::Pow, false) => map_each(&data, mode, |x| x.powf(number)),
            | _ => return None,
        };
        Some(Self::dense_result(self.shape.clone(), out, dim))
    }

    /// The `+` reduction over an `f64` tensor, using the compensated
    /// summation §6.6.6 and §9.5 ask for.
    ///
    /// Strided views are traversed in logical order, preserving the bits
    /// produced by the boxed reduction.
    ///
    /// A one-element tensor keeps its display unit, because the boxed
    /// reduction returns the element it started from unchanged.
    #[must_use]
    pub fn dense_sum(&self) -> Option<NumVal> {
        self.dense_sum_in_mode(aevia_rssn::execution::default_execution_mode())
    }

    /// The `+` reduction over an `f64` tensor under an explicit
    /// [`aevia_rssn::execution::ExecutionMode`], using deterministic parallel
    /// Neumaier summation for large contiguous buffers.
    #[must_use]
    pub fn dense_sum_in_mode(
        &self,
        mode: aevia_rssn::execution::ExecutionMode,
    ) -> Option<NumVal> {
        let (total, len) = if let Some(view) = self.strided_float_view() {
            let mut values = view.flat_values(&self.shape)?;
            let len = values.len();
            let total = if len == 1 {
                values.next()??
            } else {
                let mut total = 0.0f64;
                let mut compensation = 0.0f64;
                for value in values {
                    let value = value?;
                    compensated_add!(total, compensation, value);
                }
                compensated_total(total, compensation)
            };
            (total, len)
        } else {
            let data = self.dense_flat()?;
            let total = if data.len() > 1
                && aevia_rssn::should_split_for_mode(data.len(), DENSE_PAR_MIN, mode)
            {
                let par_total = aevia_rssn::par_sum_kahan_f64_in_mode(&data, mode);
                if par_total.is_finite() {
                    par_total
                } else {
                    kahan_sum(&data)
                }
            } else {
                kahan_sum(&data)
            };
            (total, data.len())
        };
        if len == 0 {
            return None;
        }
        let mut num = NumVal::float(total, self.dim.clone());
        if len == 1 {
            num.display.clone_from(&self.display);
        }
        Some(num)
    }

    /// The `*` reduction over an `f64` tensor, folded left to right.
    #[must_use]
    pub fn dense_prod(&self) -> Option<NumVal> {
        let (total, singleton) = if let Some(view) = self.strided_float_view() {
            let mut values = view.flat_values(&self.shape)?;
            let mut total = values.next()??;
            let singleton = values.len() == 0;
            for value in values {
                total *= value?;
            }
            (total, singleton)
        } else {
            let data = self.dense_flat()?;
            let (&first, rest) = data.split_first()?;
            let mut total = first;
            for value in rest {
                total *= value;
            }
            (total, rest.is_empty())
        };
        let mut num = NumVal::float(total, self.dim.clone());
        if singleton {
            num.display.clone_from(&self.display);
        }
        Some(num)
    }

    /// Multiply dense vectors and matrices using the rank pairs in §3.5.3a.
    ///
    /// Matrix products use the blocked kernel in [`aevia_std::linalg::matmul`],
    /// and 1D dot products use RSSN's SIMD + parallel dot kernel.
    /// Returns `None` for incompatible shapes, an oversized result or
    /// unavailable float storage. The generic path handles zero extents.
    #[must_use]
    pub fn dense_matmul(
        &self,
        right: &Self,
    ) -> Option<Value> {
        self.dense_matmul_in_mode(right, aevia_rssn::execution::default_execution_mode())
    }

    /// Multiply dense vectors and matrices under an explicit
    /// [`aevia_rssn::execution::ExecutionMode`].
    #[must_use]
    pub fn dense_matmul_in_mode(
        &self,
        right: &Self,
        mode: aevia_rssn::execution::ExecutionMode,
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
        let dim = self.dim.mul(&right.dim);
        if shape.is_empty() {
            let dot = aevia_rssn::par_dot_f64_in_mode(&left_data, &right_data, mode);
            return Some(Value::Num(NumVal::float(dot, dim)));
        }
        let legacy =
            || aevia_std::linalg::legacy::matmul(rows, inner, cols, &left_data, &right_data);
        let data = if shape.len() == 2 || rows.saturating_mul(inner).saturating_mul(cols) >= 4_096 {
            aevia_std::linalg::matmul(rows, inner, cols, &left_data, &right_data)
                .unwrap_or_else(|_| legacy())
        } else {
            legacy()
        };
        if data.len() != count {
            return None;
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

    #[test]
    fn dense_broadcast_int_scalar_sum_and_dot_agree_across_modes() {
        use aevia_rssn::execution::ExecutionMode;
        let a = TensorData::new(
            vec![2, 3],
            Storage::Dense(Rc::new(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0])),
            Dim::one(),
            None,
        );
        let row = TensorData::new(
            vec![1, 3],
            Storage::Dense(Rc::new(vec![10.0, 20.0, 30.0])),
            Dim::one(),
            None,
        );
        let s = a
            .dense_binary_in_mode(BinOp::Add, &row, ExecutionMode::Serial)
            .expect("broadcast add");
        let p = a
            .dense_binary_in_mode(BinOp::Add, &row, ExecutionMode::Parallel(2))
            .expect("parallel broadcast add");
        assert_eq!(crate::value::show(&s), crate::value::show(&p));
        let Value::Tensor(t) = s else {
            panic!("expected tensor");
        };
        assert_eq!(t.shape, vec![2, 3]);
        assert_eq!(t.dense().unwrap(), &[11.0, 22.0, 33.0, 14.0, 25.0, 36.0]);

        let two = NumVal::int(2);
        let scaled = a
            .dense_scalar_in_mode(BinOp::Mul, &two, false, ExecutionMode::Serial)
            .expect("int scalar mul");
        let Value::Tensor(st) = scaled else {
            panic!("expected tensor");
        };
        assert_eq!(st.dense().unwrap(), &[2.0, 4.0, 6.0, 8.0, 10.0, 12.0]);

        let v1 = TensorData::new(
            vec![4],
            Storage::Dense(Rc::new(vec![1.0, 2.0, 3.0, 4.0])),
            Dim::one(),
            None,
        );
        let v2 = TensorData::new(
            vec![4],
            Storage::Dense(Rc::new(vec![5.0, 6.0, 7.0, 8.0])),
            Dim::one(),
            None,
        );
        let dot = v1
            .dense_matmul_in_mode(&v2, ExecutionMode::Parallel(2))
            .expect("dot");
        assert_eq!(crate::value::show(&dot), "70.0");

        let large = TensorData::new(
            vec![140_000],
            Storage::Dense(Rc::new(vec![0.25; 140_000])),
            Dim::one(),
            None,
        );
        let sum_s = large.dense_sum_in_mode(ExecutionMode::Serial).unwrap();
        let sum_p = large.dense_sum_in_mode(ExecutionMode::Parallel(2)).unwrap();
        assert_eq!(sum_s.to_f64().unwrap(), 35_000.0);
        assert_eq!(
            sum_s.to_f64().unwrap().to_bits(),
            sum_p.to_f64().unwrap().to_bits()
        );
    }

    #[test]
    fn dense_binary_and_scalar_modes_produce_identical_dense_results() {
        use aevia_rssn::execution::ExecutionMode;
        let len = DENSE_PAR_MIN + 17;
        let left_vals: Vec<f64> = (0..len).map(|i| (i as f64) * 0.5 + 1.0).collect();
        let right_vals: Vec<f64> = (0..len).map(|i| ((i % 7) as f64) + 0.25).collect();
        let left = TensorData::new(
            vec![len],
            Storage::Dense(Rc::new(left_vals)),
            Dim::one(),
            None,
        );
        let right = TensorData::new(
            vec![len],
            Storage::Dense(Rc::new(right_vals)),
            Dim::one(),
            None,
        );

        let serial_bin = left
            .dense_binary_in_mode(BinOp::Add, &right, ExecutionMode::Serial)
            .unwrap();
        let parallel_bin = left
            .dense_binary_in_mode(BinOp::Add, &right, ExecutionMode::Parallel(2))
            .unwrap();
        let (Value::Tensor(sb), Value::Tensor(pb)) = (&serial_bin, &parallel_bin) else {
            panic!("expected dense binary tensor results");
        };
        assert_eq!(sb.dense(), pb.dense());

        let scalar = NumVal::float(1.5, Dim::one());
        let serial_sc = left
            .dense_scalar_in_mode(BinOp::Mul, &scalar, false, ExecutionMode::Serial)
            .unwrap();
        let parallel_sc = left
            .dense_scalar_in_mode(BinOp::Mul, &scalar, false, ExecutionMode::Parallel(2))
            .unwrap();
        let (Value::Tensor(ss), Value::Tensor(ps)) = (&serial_sc, &parallel_sc) else {
            panic!("expected dense scalar tensor results");
        };
        assert_eq!(ss.dense(), ps.dense());
    }
}
