//! Checked offset arithmetic and shared storage for dense tensor views.
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

use super::Storage;
use super::TensorData;
use super::View;
use super::checked_element_count;
use super::row_major_strides;
use std::borrow::Cow;
use std::rc::Rc;

fn advance(
    offset: usize,
    coordinate: usize,
    stride: usize,
    reversed: bool,
) -> Option<usize> {
    let distance = coordinate.checked_mul(stride)?;
    if reversed {
        offset.checked_sub(distance)
    } else {
        offset.checked_add(distance)
    }
}

impl View {
    fn matches_shape(
        &self,
        shape: &[usize],
    ) -> bool {
        checked_element_count(shape) == Some(self.len)
            && self.strides.len() == shape.len()
            && self.reversed.len() == shape.len()
    }

    /// Check the extrema of all reachable positions before deriving or
    /// materialising a view. Each axis contributes to just one endpoint.
    fn validate(
        &self,
        shape: &[usize],
    ) -> Option<()> {
        if !self.matches_shape(shape) {
            return None;
        }
        if self.len == 0 {
            return (self.offset <= self.data.len()).then_some(());
        }

        let mut lowest = self.offset;
        let mut highest = self.offset;
        for ((&extent, &stride), &reversed) in shape.iter().zip(&self.strides).zip(&self.reversed) {
            let last = extent.checked_sub(1)?;
            if reversed {
                lowest = advance(lowest, last, stride, true)?;
            } else {
                highest = advance(highest, last, stride, false)?;
            }
        }
        (highest < self.data.len()).then_some(())
    }

    fn position(
        &self,
        shape: &[usize],
        mut index: usize,
    ) -> Option<usize> {
        let mut position = self.offset;
        let axes = shape.iter().zip(&self.strides).zip(&self.reversed);
        for ((&extent, &stride), &reversed) in axes.rev() {
            let coordinate = index.checked_rem(extent)?;
            position = advance(position, coordinate, stride, reversed)?;
            index = index.checked_div(extent)?;
        }
        Some(position)
    }

    /// Read a row-major element from a shape-compatible view.
    pub(super) fn element_at(
        &self,
        shape: &[usize],
        index: usize,
    ) -> Option<f64> {
        if index >= self.len || !self.matches_shape(shape) {
            return None;
        }
        self.data.get(self.position(shape, index)?).copied()
    }

    /// Borrow the exact physical window of a forward row-major layout.
    pub(super) fn contiguous_slice(
        &self,
        shape: &[usize],
    ) -> Option<&[f64]> {
        if !self.matches_shape(shape) || self.reversed.iter().any(|reversed| *reversed) {
            return None;
        }
        if self.len > 0 {
            let mut expected = 1usize;
            for (&stride, &extent) in self.strides.iter().zip(shape).rev() {
                if stride != expected {
                    return None;
                }
                expected = expected.checked_mul(extent)?;
            }
        }
        self.data
            .get(self.offset..self.offset.checked_add(self.len)?)
    }

    /// Borrow when contiguous; otherwise walk the validated layout in order.
    pub(super) fn dense_flat(
        &self,
        shape: &[usize],
    ) -> Option<Cow<'_, [f64]>> {
        if let Some(window) = self.contiguous_slice(shape) {
            return Some(Cow::Borrowed(window));
        }
        self.map_flat(shape, std::convert::identity).map(Cow::Owned)
    }

    /// Visit the checked view in logical row-major order without a buffer.
    pub(super) fn flat_values<'a>(
        &'a self,
        shape: &'a [usize],
    ) -> Option<impl ExactSizeIterator<Item = Option<f64>> + 'a> {
        self.validate(shape)?;
        Some((0..self.len).map(move |index| {
            self.position(shape, index)
                .and_then(|position| self.data.get(position).copied())
        }))
    }

    /// Materialise the validated view in row-major order directly into the
    /// caller's element representation.
    pub(crate) fn map_flat<T>(
        &self,
        shape: &[usize],
        convert: impl Fn(f64) -> T,
    ) -> Option<Vec<T>> {
        let values = self.flat_values(shape)?;
        let mut elements = Vec::new();
        elements.try_reserve_exact(values.len()).ok()?;
        for value in values {
            elements.push(convert(value?));
        }
        Some(elements)
    }
}

impl TensorData {
    /// Describe this tensor's physical layout over its retained f64 buffer.
    pub(super) fn f64_window(&self) -> Option<View> {
        let window = match &self.storage {
            | Storage::Dense(data) => {
                View {
                    data: Rc::clone(data),
                    offset: 0,
                    strides: row_major_strides(&self.shape)?,
                    reversed: vec![false; self.rank()],
                    len: data.len(),
                }
            },
            | Storage::SharedSlice(slice) => {
                let mut window = slice.source.f64_window()?;
                let [stride] = window.strides.as_slice() else {
                    return None;
                };
                let [reversed] = window.reversed.as_slice() else {
                    return None;
                };
                let end = slice.offset.checked_add(slice.len)?;
                if end > window.len {
                    return None;
                }
                window.offset = advance(window.offset, slice.offset, *stride, *reversed)?;
                window.len = slice.len;
                if self.rank() != 1 {
                    // A reshaped suffix can use physical strides only if its
                    // logical elements occupy one contiguous range.
                    self.dense()?;
                    window.strides = row_major_strides(&self.shape)?;
                    window.reversed = vec![false; self.rank()];
                }
                window
            },
            | Storage::View(view) => view.clone(),
            | _ => return None,
        };
        window.validate(&self.shape)?;
        Some(window)
    }

    fn with_view(
        &self,
        shape: Vec<usize>,
        mut view: View,
    ) -> Option<Self> {
        view.len = checked_element_count(&shape)?;
        if view.len == 0 {
            return None;
        }
        view.validate(&shape)?;
        Some(Self {
            verified_element: self.verified_element.clone(),
            shape,
            storage: Storage::View(view),
            dim: self.dim.clone(),
            display: self.display.clone(),
        })
    }

    /// Share this tensor's buffer across one half-open range per axis.
    ///
    /// Each range follows the existing direction of its axis. Returns
    /// `None` for an empty slice, an invalid range or unavailable f64 storage.
    #[must_use]
    pub fn slice_view(
        &self,
        ranges: &[(usize, usize)],
    ) -> Option<Self> {
        if ranges.len() != self.rank() {
            return None;
        }
        let mut view = self.f64_window()?;
        let axes = self.shape.iter().zip(&view.strides).zip(&view.reversed);
        let mut shape = Vec::with_capacity(ranges.len());
        for (&(begin, end), ((&extent, &stride), &reversed)) in ranges.iter().zip(axes) {
            if end > extent {
                return None;
            }
            shape.push(end.checked_sub(begin)?);
            view.offset = advance(view.offset, begin, stride, reversed)?;
        }
        self.with_view(shape, view)
    }

    /// Share this tensor's buffer across one `(first, count, step)` walk per
    /// axis. Negative steps traverse the source axis backwards.
    ///
    /// Returns `None` for an empty slice, a walk outside the source extent,
    /// a zero step or unavailable f64 storage.
    #[must_use]
    pub fn slice_step_view(
        &self,
        walks: &[(usize, usize, isize)],
    ) -> Option<Self> {
        if walks.len() != self.rank() {
            return None;
        }
        let mut view = self.f64_window()?;
        let axes = self
            .shape
            .iter()
            .zip(&mut view.strides)
            .zip(&mut view.reversed);
        let mut shape = Vec::with_capacity(walks.len());
        for (&(first, count, step), ((&extent, stride), reversed)) in walks.iter().zip(axes) {
            let magnitude = step.unsigned_abs();
            let backwards = step < 0;
            let last = advance(first, count.checked_sub(1)?, magnitude, backwards)?;
            if magnitude == 0 || first >= extent || last >= extent {
                return None;
            }
            view.offset = advance(view.offset, first, *stride, *reversed)?;
            // With one coordinate the stride is immaterial, so retain the
            // existing magnitude if composition would overflow.
            if let Some(composed) = stride.checked_mul(magnitude) {
                *stride = composed;
            } else if count > 1 {
                return None;
            }
            *reversed ^= backwards;
            shape.push(count);
        }
        self.with_view(shape, view)
    }

    /// Pin leading axes at the supplied coordinates and share the remaining
    /// window of the buffer. Returns `None` for an empty result, an invalid
    /// coordinate or unavailable f64 storage.
    #[must_use]
    pub fn index_view(
        &self,
        fixed: &[usize],
    ) -> Option<Self> {
        let shape = self.shape.get(fixed.len()..)?;
        let mut view = self.f64_window()?;
        let axes = self.shape.iter().zip(&view.strides).zip(&view.reversed);
        for (&coordinate, ((&extent, &stride), &reversed)) in fixed.iter().zip(axes) {
            if coordinate >= extent {
                return None;
            }
            view.offset = advance(view.offset, coordinate, stride, reversed)?;
        }
        view.strides = view.strides.get(fixed.len()..)?.to_vec();
        view.reversed = view.reversed.get(fixed.len()..)?.to_vec();
        self.with_view(shape.to_vec(), view)
    }

    /// Reverse the order of axes of an f64 tensor using a shared buffer view.
    /// Returns `None` below rank two, for an empty source or for unavailable f64 storage.
    #[must_use]
    pub fn transposed_view(&self) -> Option<Self> {
        if self.rank() < 2 {
            return None;
        }
        let mut view = self.f64_window()?;
        view.strides.reverse();
        view.reversed.reverse();
        self.with_view(self.shape.iter().rev().copied().collect(), view)
    }

    /// Share the same f64 buffer with one axis running in the opposite
    /// direction. Returns `None` for an empty tensor, an invalid axis or
    /// unavailable f64 storage.
    #[must_use]
    pub fn reversed_view(
        &self,
        axis: usize,
    ) -> Option<Self> {
        let extent = *self.shape.get(axis)?;
        let last = extent.checked_sub(1)?;
        let mut view = self.f64_window()?;
        let stride = *view.strides.get(axis)?;
        let reversed = view.reversed.get_mut(axis)?;
        view.offset = advance(view.offset, last, stride, *reversed)?;
        // A singleton retains its original direction and contiguity.
        if extent > 1 {
            *reversed = !*reversed;
        }
        self.with_view(self.shape.clone(), view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aevia_dim::Dim;
    use aevia_syntax::ast::BinOp;

    fn malformed_view(
        shape: Vec<usize>,
        strides: Vec<usize>,
        reversed: Vec<bool>,
        len: usize,
    ) -> TensorData {
        TensorData::new(
            shape,
            Storage::View(View {
                data: Rc::new(vec![7.0]),
                offset: 0,
                strides,
                reversed,
                len,
            }),
            Dim::one(),
            None,
        )
    }

    #[test]
    fn malformed_axis_metadata_cannot_create_views() {
        let tensor = malformed_view(vec![1, 1], vec![1], vec![false], 1);
        assert!(tensor.get_flat(0).is_none());
        assert!(tensor.dense_flat().is_none());
        assert!(tensor.slice_view(&[(0, 1), (0, 1)]).is_none());
        assert!(tensor.slice_step_view(&[(0, 1, 1), (0, 1, 1)]).is_none());
        assert!(tensor.index_view(&[0]).is_none());
        assert!(tensor.transposed_view().is_none());
        assert!(tensor.reversed_view(1).is_none());
    }

    #[test]
    fn invalid_lengths_and_reversed_endpoints_are_rejected_before_materialising() {
        let false_length = malformed_view(vec![1], vec![1], vec![false], usize::MAX);
        assert!(false_length.get_flat(0).is_none());
        assert!(false_length.dense_flat().is_none());
        let zero_extent = malformed_view(vec![0], vec![1], vec![false], 1);
        assert!(zero_extent.get_flat(0).is_none());
        assert!(zero_extent.dense_flat().is_none());
        let before_buffer = malformed_view(vec![2], vec![1], vec![true], 2);
        assert!(before_buffer.dense_flat().is_none());
        assert!(before_buffer.slice_view(&[(0, 1)]).is_none());
    }

    #[test]
    fn malformed_dense_shape_never_reaches_float_kernels() {
        let malformed = TensorData::new(
            vec![2, 2],
            Storage::Dense(Rc::new(vec![7.0])),
            Dim::one(),
            None,
        );
        assert!(malformed.dense().is_none());
        assert!(malformed.dense_binary(BinOp::Pow, &malformed).is_none());

        let valid = TensorData::new(
            vec![2, 2],
            Storage::Dense(Rc::new(vec![1.0; 4])),
            Dim::one(),
            None,
        );
        assert!(malformed.dense_matmul(&valid).is_none());
        assert!(valid.dense_matmul(&malformed).is_none());

        let oversized = TensorData::new(
            vec![usize::MAX, 3],
            Storage::Dense(Rc::new(vec![7.0])),
            Dim::one(),
            None,
        );
        assert!(oversized.get(&[isize::MAX, 2]).is_none());
        assert!(oversized.dense_flat().is_none());
    }

    #[test]
    fn singleton_slice_accepts_the_largest_backward_step() {
        let buffer = Rc::new(vec![42.0]);
        let tensor = TensorData::new(
            vec![1],
            Storage::Dense(Rc::clone(&buffer)),
            Dim::one(),
            None,
        );
        let view = tensor
            .slice_step_view(&[(0, 1, isize::MIN)])
            .expect("one position requires no movement");
        assert!(Rc::ptr_eq(view.buffer().expect("f64 view"), &buffer));
        assert_eq!(view.dense_flat().as_deref(), Some([42.0].as_slice()));
    }
}
