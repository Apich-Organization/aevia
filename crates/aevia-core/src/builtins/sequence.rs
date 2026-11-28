//! Checked traversal and reordering of tensor sequences.
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
use crate::eval::build_tensor;
use crate::eval::empty_tensor_like;
use crate::host_witness::preserve_tensor_schema;
use crate::host_witness::tensor_witnesses;
use crate::value::NumVal;
use crate::value::Storage;
use crate::value::TensorData;
use crate::value::Value;
use crate::value::checked_element_count;
use aevia_syntax::Span;
use std::cmp::Ordering;
use std::ops::Range;
use std::rc::Rc;

fn shape_error(span: Span) -> EvalError {
    EvalError::new(
        "E0802",
        "tensor shape or backing storage is too large or inconsistent",
    )
    .at(span)
}

fn checked_len(
    tensor: &TensorData,
    span: Span,
) -> Result<usize, EvalError> {
    checked_element_count(&tensor.shape)
        .filter(|count| *count == tensor.storage.len())
        .ok_or_else(|| shape_error(span))
}

/// Reserve space for a collection of tensor sequence items.
pub(super) fn reserve_items<T>(
    size: usize,
    span: Span,
) -> Result<Vec<T>, EvalError> {
    let mut items = Vec::new();
    items
        .try_reserve_exact(size)
        .map_err(|_| shape_error(span))?;
    Ok(items)
}

/// A row geometry validated against the tensor's logical element count.
pub(super) struct FirstAxis<'a> {
    tensor: &'a TensorData,
    /// How many rows the tensor contains.
    pub(super) rows: usize,
    tail: &'a [usize],
    width: usize,
}

impl<'a> FirstAxis<'a> {
    /// The first axis of a tensor with rank two or higher.
    pub(super) fn new(
        tensor: &'a TensorData,
        span: Span,
    ) -> Result<Self, EvalError> {
        checked_len(tensor, span)?;
        let Some((&rows, tail)) = tensor
            .shape
            .split_first()
            .filter(|(_, tail)| !tail.is_empty())
        else {
            return Err(shape_error(span));
        };
        // A zero leading axis admits no row accesses, irrespective of the
        // shape a hypothetical row would have.
        let width = if rows == 0 {
            0
        } else {
            checked_element_count(tail).ok_or_else(|| shape_error(span))?
        };
        Ok(Self {
            tensor,
            rows,
            tail,
            width,
        })
    }

    fn range(
        &self,
        row: usize,
        span: Span,
    ) -> Result<Range<usize>, EvalError> {
        if row >= self.rows {
            return Err(shape_error(span));
        }
        let start = row
            .checked_mul(self.width)
            .ok_or_else(|| shape_error(span))?;
        let end = start
            .checked_add(self.width)
            .ok_or_else(|| shape_error(span))?;
        Ok(start..end)
    }

    /// Select one row, using a shared view when backed by floats.
    pub(super) fn row(
        &self,
        index: usize,
        span: Span,
    ) -> Result<Value, EvalError> {
        let range = self.range(index, span)?;
        if range.is_empty() {
            return Ok(empty_tensor_like(self.tensor, self.tail.to_vec()));
        }
        if let Some(row) = self.tensor.index_view(&[index]) {
            return Ok(Value::Tensor(Rc::new(row)));
        }
        let mut values = reserve_items(range.len(), span)?;
        for offset in range {
            values.push(tensor_item(self.tensor, offset, span)?);
        }
        let mut output = selection_storage(self.tensor, values, self.tail.to_vec());
        if let (Some(element), Value::Tensor(row)) = (&self.tensor.verified_element, &mut output)
            && let Ok(rank) = u32::try_from(row.rank())
            && tensor_witnesses(row, &aevia_types::Shape::Rank(rank), element, false).is_some()
        {
            // Validate only this row: validating the whole source for each
            // callback would make walking a boxed sequence quadratic.
            Rc::make_mut(row).verified_element = Some(Rc::clone(element));
        }
        Ok(output)
    }

    /// Stack rows selected from this axis. All selectors originate from the
    /// validated sequence or from a range bounded by `rows`.
    pub(super) fn gather(
        &self,
        rows: impl ExactSizeIterator<Item = usize>,
        span: Span,
    ) -> Result<Value, EvalError> {
        let count = rows.len();
        let len = count
            .checked_mul(self.width)
            .ok_or_else(|| shape_error(span))?;
        let mut shape = Vec::with_capacity(self.tensor.rank());
        shape.push(count);
        shape.extend_from_slice(self.tail);
        if len == 0 {
            return Ok(empty_tensor_like(self.tensor, shape));
        }
        if let Some(data) = self.tensor.dense() {
            let mut gathered = reserve_items(len, span)?;
            for index in rows {
                let values = data
                    .get(self.range(index, span)?)
                    .ok_or_else(|| shape_error(span))?;
                gathered.extend_from_slice(values);
            }
            let output = Value::Tensor(Rc::new(TensorData::new(
                shape,
                Storage::Dense(Rc::new(gathered)),
                self.tensor.dim.clone(),
                self.tensor.display.clone(),
            )));
            return Ok(preserve_tensor_schema(output, self.tensor));
        }
        let mut gathered = reserve_items(len, span)?;
        for index in rows {
            for offset in self.range(index, span)? {
                gathered.push(tensor_item(self.tensor, offset, span)?);
            }
        }
        Ok(rebuild_selection(self.tensor, gathered, shape))
    }
}

fn tensor_item(
    tensor: &TensorData,
    index: usize,
    span: Span,
) -> Result<Value, EvalError> {
    tensor.get_flat(index).ok_or_else(|| shape_error(span))
}

/// The scalars in a vector or rows of a higher-rank tensor.
pub(super) fn sequence_items(
    tensor: &TensorData,
    span: Span,
) -> Result<Vec<Value>, EvalError> {
    if tensor.rank() >= 2 {
        let axis = FirstAxis::new(tensor, span)?;
        let mut values = reserve_items(axis.rows, span)?;
        for row in 0..axis.rows {
            values.push(axis.row(row, span)?);
        }
        return Ok(values);
    }
    let count = checked_len(tensor, span)?;
    let mut values = reserve_items(count, span)?;
    for index in 0..count {
        values.push(tensor_item(tensor, index, span)?);
    }
    Ok(values)
}

fn selection_storage(
    tensor: &TensorData,
    elements: Vec<Value>,
    shape: Vec<usize>,
) -> Value {
    // Boxed elements can carry different display units or numeric phases.
    // A selection only reorders them, so it must not homogenize the storage.
    if matches!(tensor.storage.backing(), Storage::Boxed(_)) {
        return Value::Tensor(Rc::new(TensorData::new(
            shape,
            Storage::Boxed(elements),
            tensor.dim.clone(),
            tensor.display.clone(),
        )));
    }
    build_tensor(elements, shape, tensor.dim.clone())
}

/// Rebuild an unchanged selection while retaining its element metadata.
pub(super) fn rebuild_selection(
    tensor: &TensorData,
    elements: Vec<Value>,
    shape: Vec<usize>,
) -> Value {
    if elements.is_empty() {
        return empty_tensor_like(tensor, shape);
    }
    preserve_tensor_schema(selection_storage(tensor, elements, shape), tensor)
}

/// Stack the selected rows, or gather selected scalars into a vector.
pub(super) fn gather_sequence(
    tensor: &TensorData,
    items: &[Value],
    order: &[usize],
    span: Span,
) -> Result<Value, EvalError> {
    if tensor.rank() >= 2 {
        return FirstAxis::new(tensor, span)?.gather(order.iter().copied(), span);
    }
    let mut elements = reserve_items(order.len(), span)?;
    for &index in order {
        elements.push(items.get(index).ok_or_else(|| shape_error(span))?.clone());
    }
    Ok(rebuild_selection(tensor, elements, vec![order.len()]))
}

/// Sort the elements along one dimension while retaining the tensor's shape.
#[expect(
    clippy::single_call_fn,
    reason = "keeps lane traversal out of the builtin dispatcher"
)]
pub(super) fn sort_axis(
    tensor: &TensorData,
    axis: usize,
    compare: impl Fn(&Value, &Value) -> Ordering,
    span: Span,
) -> Result<Value, EvalError> {
    let count = checked_len(tensor, span)?;
    if count == 0 {
        return Ok(empty_tensor_like(tensor, tensor.shape.clone()));
    }
    let Some((extent, tail)) = tensor.shape.get(axis..).and_then(<[usize]>::split_first) else {
        return Err(shape_error(span));
    };
    let inner = checked_element_count(tail).ok_or_else(|| shape_error(span))?;
    let block_width = extent.checked_mul(inner).ok_or_else(|| shape_error(span))?;
    let mut elements = reserve_items(count, span)?;
    for index in 0..count {
        elements.push(tensor_item(tensor, index, span)?);
    }
    if inner == 1 {
        for block in elements.chunks_exact_mut(block_width) {
            block.sort_by(&compare);
        }
    } else {
        let mut lane = reserve_items(*extent, span)?;
        for block in elements.chunks_exact_mut(block_width) {
            for offset in 0..inner {
                lane.extend(block.iter().skip(offset).step_by(inner).cloned());
                lane.sort_by(&compare);
                for cell in block.iter_mut().skip(offset).step_by(inner).rev() {
                    *cell = lane.pop().ok_or_else(|| shape_error(span))?;
                }
            }
        }
    }
    Ok(rebuild_selection(tensor, elements, tensor.shape.clone()))
}

/// Join two tensors along their first axis.
#[allow(
    clippy::single_call_fn,
    reason = "keeps concatenation checks out of the dispatcher and available to direct tests"
)]
pub(super) fn concat(
    left: &TensorData,
    right: &TensorData,
    span: Span,
) -> Result<Value, EvalError> {
    let left_count = checked_len(left, span)?;
    let right_count = checked_len(right, span)?;
    let count = left_count
        .checked_add(right_count)
        .ok_or_else(|| shape_error(span))?;
    let shape = match (left.shape.split_first(), right.shape.split_first()) {
        | (Some((&a, left_tail)), Some((&b, right_tail))) if left_tail == right_tail => {
            let mut shape = left.shape.clone();
            let first = shape.first_mut().ok_or_else(|| shape_error(span))?;
            *first = a.checked_add(b).ok_or_else(|| shape_error(span))?;
            shape
        },
        | _ if left.rank() <= 1 && right.rank() <= 1 => vec![count],
        | _ => {
            return Err(EvalError::new(
                "E0802",
                format!("`concat` stacks along the first axis: shapes {:?} and {:?} disagree on the rest", left.shape, right.shape),
            ).at(span));
        },
    };
    if count == 0 {
        return Ok(empty_tensor_like(left, shape));
    }
    if let (Some(left_data), Some(right_data)) = (left.dense(), right.dense()) {
        let mut values = reserve_items(count, span)?;
        values.extend_from_slice(left_data);
        values.extend_from_slice(right_data);
        // Numeric compaction keeps the first display unit from a nonempty input.
        let display = left
            .display
            .as_ref()
            .filter(|_| !left_data.is_empty())
            .or_else(|| right.display.as_ref().filter(|_| !right_data.is_empty()))
            .cloned();
        let output = Value::Tensor(Rc::new(TensorData::new(
            shape,
            Storage::Dense(Rc::new(values)),
            left.dim.clone(),
            display,
        )));
        return Ok(preserve_tensor_schema(output, left));
    }
    let mut elements = reserve_items(count, span)?;
    for index in 0..left_count {
        elements.push(tensor_item(left, index, span)?);
    }
    for index in 0..right_count {
        elements.push(tensor_item(right, index, span)?);
    }
    Ok(rebuild_selection(left, elements, shape))
}

/// A zero-based index as an Aevia arbitrary-precision integer.
pub(super) fn index_value(index: usize) -> Value {
    Value::Num(NumVal::int(index))
}

/// The order of `items` under a total comparator, keeping ties stable.
pub(super) fn sorted_indices<T>(
    items: &[T],
    compare: impl Fn(&T, &T) -> Ordering,
    span: Span,
) -> Result<Vec<usize>, EvalError> {
    let mut order = reserve_items(items.len(), span)?;
    order.extend(0..items.len());
    order.sort_by(|&left, &right| {
        match (items.get(left), items.get(right)) {
            | (Some(a), Some(b)) => compare(a, b),
            // Every index originates from this slice.
            | _ => Ordering::Equal,
        }
    });
    Ok(order)
}

/// A stable, fallible merge sort of `0..count` using one reusable buffer.
#[allow(
    clippy::single_call_fn,
    reason = "the Aevia comparator can fail and requires a fallible stable sort"
)]
pub(super) fn merge_sort_indices(
    count: usize,
    after: &mut dyn FnMut(usize, usize) -> Result<bool, EvalError>,
    span: Span,
) -> Result<Vec<usize>, EvalError> {
    fn sort(
        order: &mut [usize],
        scratch: &mut [usize],
        after: &mut dyn FnMut(usize, usize) -> Result<bool, EvalError>,
    ) -> Result<(), EvalError> {
        if order.len() <= 1 {
            return Ok(());
        }
        let mid = order.len() / 2;
        let (left, right) = order.split_at_mut(mid);
        let (left_scratch, right_scratch) = scratch.split_at_mut(mid);
        sort(left, left_scratch, after)?;
        sort(right, right_scratch, after)?;
        let mut left = left.iter().copied().peekable();
        let mut right = right.iter().copied().peekable();
        for dest in scratch.iter_mut() {
            *dest = match (left.peek(), right.peek()) {
                | (Some(&a), Some(&b)) if after(a, b)? => right.next(),
                | (Some(_), _) => left.next(),
                | (None, _) => right.next(),
            }
            .ok_or_else(|| EvalError::new("E0802", "invalid internal sort range"))?;
        }
        order.copy_from_slice(scratch);
        Ok(())
    }

    let mut order = reserve_items(count, span)?;
    order.extend(0..count);
    let mut scratch = reserve_items(count, span)?;
    scratch.resize(count, 0);
    sort(&mut order, &mut scratch, after)?;
    Ok(order)
}

/// Compare scalar items or rows lexicographically, with configurable NaN placement.
#[allow(
    clippy::single_call_fn,
    reason = "implements the float-specific stable ordering used by sort_float"
)]
pub(super) fn float_sequence_order(
    left: &Value,
    right: &Value,
    nan_last: bool,
) -> Ordering {
    fn floats(value: &Value) -> impl Iterator<Item = f64> + '_ {
        let (len, dense) = match value {
            | Value::Tensor(tensor) => (tensor.storage.len(), tensor.dense()),
            | _ => (1, None),
        };
        (0..len).map(move |index| {
            let number = match value {
                | Value::Tensor(tensor) => {
                    dense
                        .and_then(|items| items.get(index).copied())
                        .or_else(|| tensor.get_flat(index)?.as_f64())
                },
                | other => other.as_f64(),
            };
            number.unwrap_or(f64::NAN)
        })
    }
    let nan_order = if nan_last {
        Ordering::Greater
    } else {
        Ordering::Less
    };
    for (left, right) in floats(left).zip(floats(right)) {
        let ordering = match (left.is_nan(), right.is_nan()) {
            | (true, true) => Ordering::Equal,
            | (true, false) => nan_order,
            | (false, true) => nan_order.reverse(),
            | (false, false) => left.partial_cmp(&right).unwrap_or(Ordering::Equal),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

/// Reverse an axis, retaining a view of a float buffer when possible.
pub(super) fn flip_axis(
    tensor: &TensorData,
    axis: usize,
    span: Span,
) -> Result<Value, EvalError> {
    let len = checked_len(tensor, span)?;
    let Some((&extent, tail)) = tensor.shape.get(axis..).and_then(<[usize]>::split_first) else {
        return Err(shape_error(span));
    };
    if len == 0 {
        return Ok(empty_tensor_like(tensor, tensor.shape.clone()));
    }
    if let Some(view) = tensor.reversed_view(axis) {
        return Ok(Value::Tensor(Rc::new(view)));
    }
    let inner = checked_element_count(tail).ok_or_else(|| shape_error(span))?;
    let stride = extent.checked_mul(inner).ok_or_else(|| shape_error(span))?;
    let mut output = reserve_items(len, span)?;
    for block in (0..len).step_by(stride) {
        for coordinate in (0..extent).rev() {
            let start = coordinate
                .checked_mul(inner)
                .and_then(|index| block.checked_add(index))
                .ok_or_else(|| shape_error(span))?;
            let end = start.checked_add(inner).ok_or_else(|| shape_error(span))?;
            for source in start..end {
                output.push(tensor_item(tensor, source, span)?);
            }
        }
    }
    Ok(rebuild_selection(tensor, output, tensor.shape.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aevia_dim::Dim;

    fn dense(
        shape: Vec<usize>,
        values: Vec<f64>,
    ) -> TensorData {
        TensorData::new(shape, Storage::Dense(Rc::new(values)), Dim::one(), None)
    }

    fn tensor(value: &Value) -> &TensorData {
        let Value::Tensor(tensor) = value else {
            panic!("expected a tensor");
        };
        tensor
    }

    #[test]
    fn synthetic_empty_rows_keep_their_shape_without_materialising_indices() {
        let span = Span::default();
        let input = dense(vec![usize::MAX, 0, 2], Vec::new());
        let axis = FirstAxis::new(&input, span).expect("valid empty matrix");
        let selected = axis.gather(1..usize::MAX, span).expect("select empty rows");
        assert_eq!(
            tensor(&selected).shape,
            [usize::MAX.saturating_sub(1), 0, 2]
        );
        let row = axis
            .row(usize::MAX.saturating_sub(1), span)
            .expect("last row");
        assert_eq!(tensor(&row).shape, [0, 2]);
        let flipped = flip_axis(&input, 0, span).expect("flip empty tensor");
        assert_eq!(tensor(&flipped).shape, input.shape);
        assert!(tensor(&flipped).storage.is_empty());
        assert_eq!(
            sequence_items(&input, span)
                .expect_err("capacity overflow")
                .code,
            "E0802"
        );
    }

    #[test]
    fn empty_rows_keep_their_storage_display_and_authenticated_schema() {
        let span = Span::default();
        let dim = Dim::base("Length");
        let display = Rc::new(aevia_dim::UnitDef::linear(
            "m",
            dim.clone(),
            aevia_dim::Scale::one(),
        ));
        let floats = TensorData::new(
            vec![2, 0],
            Storage::Dense(Rc::new(Vec::new())),
            dim,
            Some(Rc::clone(&display)),
        );
        let axis = FirstAxis::new(&floats, span).expect("empty float rows");
        let row = axis.row(0, span).expect("first row");
        assert!(tensor(&row).dense_flat().is_some());
        assert!(Rc::ptr_eq(
            tensor(&row).display.as_ref().expect("display unit"),
            &display
        ));

        let boxed = TensorData::checked(
            vec![2, 0],
            Storage::Boxed(Vec::new()),
            Dim::one(),
            None,
            aevia_types::Type::Str,
        )
        .expect("authenticated boxed tensor");
        let axis = FirstAxis::new(&boxed, span).expect("empty boxed rows");
        let row = axis.row(0, span).expect("first row");
        assert_eq!(
            tensor(&row).verified_element.as_deref(),
            Some(&aevia_types::Type::Str)
        );
        let anonymous = TensorData::new(vec![2, 0], Storage::Boxed(Vec::new()), Dim::one(), None);
        let row = FirstAxis::new(&anonymous, span)
            .unwrap()
            .row(0, span)
            .unwrap();
        assert!(tensor(&row).verified_element.is_none());
    }

    #[test]
    fn inconsistent_host_buffers_report_shape_errors() {
        let span = Span::default();
        let input = dense(vec![2, 2], vec![1.0]);
        assert_eq!(
            sequence_items(&input, span)
                .expect_err("missing cells")
                .code,
            "E0802"
        );
        assert_eq!(
            flip_axis(&input, 0, span).expect_err("missing cells").code,
            "E0802"
        );
        let input = dense(vec![usize::MAX, 2], Vec::new());
        assert_eq!(
            sequence_items(&input, span)
                .expect_err("invalid extent product")
                .code,
            "E0802"
        );
    }

    #[test]
    fn dense_row_selection_retains_storage_and_float_sorting_respects_nan() {
        let span = Span::default();
        let input = dense(
            vec![4, 2],
            vec![f64::NAN, 5.0, 0.0, 3.0, 0.0, 1.0, f64::NAN, 1.0],
        );
        let rows = sequence_items(&input, span).expect("float rows");
        let first = tensor(rows.first().expect("first row"));
        assert!(Rc::ptr_eq(input.buffer().unwrap(), first.buffer().unwrap()));
        for (nan_last, expected) in [(true, [2, 1, 3, 0]), (false, [3, 0, 2, 1])] {
            let order = sorted_indices(&rows, |a, b| float_sequence_order(a, b, nan_last), span)
                .expect("float order");
            assert_eq!(order, expected);
        }
    }

    #[test]
    fn concat_dense_window_keeps_bits_dimensions_schema_and_display_order() {
        let span = Span::default();
        let dim = Dim::base("Length");
        let metre = Rc::new(aevia_dim::UnitDef::linear(
            "m",
            dim.clone(),
            aevia_dim::Scale::one(),
        ));
        let centimetre = Rc::new(aevia_dim::UnitDef::linear(
            "cm",
            dim.clone(),
            aevia_dim::Scale::parse("1/100").unwrap(),
        ));
        let element = aevia_types::Type::Scalar(aevia_types::Rep::Float, dim.clone());
        let nan = f64::from_bits(0x7ff8_0000_0000_002a);
        let source = TensorData::checked(
            vec![2, 2],
            Storage::Dense(Rc::new(vec![99.0, 77.0, -0.0, nan])),
            dim.clone(),
            Some(Rc::clone(&metre)),
            element.clone(),
        )
        .unwrap();
        let left = source.index_view(&[1]).unwrap();
        let right = TensorData::new(
            vec![2],
            Storage::Dense(Rc::new(vec![0.0, f64::INFINITY])),
            dim.clone(),
            Some(Rc::clone(&centimetre)),
        );
        let joined = concat(&left, &right, span).unwrap();
        let joined = tensor(&joined);
        assert_eq!(joined.shape, [4]);
        assert_eq!(joined.dim, dim);
        assert_eq!(joined.verified_element.as_deref(), Some(&element));
        assert!(Rc::ptr_eq(joined.display.as_ref().unwrap(), &metre));
        let bits: Vec<_> = joined
            .dense()
            .unwrap()
            .iter()
            .map(|value| value.to_bits())
            .collect();
        assert_eq!(bits, [-0.0, nan, 0.0, f64::INFINITY].map(f64::to_bits));

        let mut undisplayed = left;
        undisplayed.display = None;
        let mut empty = dense(vec![0], Vec::new());
        empty.dim = dim;
        empty.display = Some(metre);
        for first in [&undisplayed, &empty] {
            let joined = concat(first, &right, span).unwrap();
            assert!(Rc::ptr_eq(
                tensor(&joined).display.as_ref().unwrap(),
                &centimetre
            ));
        }
        let joined = concat(&undisplayed, &empty, span).unwrap();
        assert!(tensor(&joined).display.is_none());
    }

    #[test]
    fn concat_keeps_exact_promotion_boxed_identity_strides_and_shape_errors() {
        let span = Span::default();
        let exact = TensorData::new(
            vec![1],
            Storage::TypedExact(
                vec![num_rational::BigRational::new(3.into(), 2.into())],
                crate::value::ExactRep::Real,
            ),
            Dim::one(),
            None,
        );
        let floats = dense(vec![1], vec![8.0]);
        let joined = concat(&exact, &floats, span).unwrap();
        assert_eq!(tensor(&joined).dense().unwrap(), [1.5, 8.0]);

        let boxed = TensorData::new(
            vec![1],
            Storage::Boxed(vec![Value::int(3)]),
            Dim::one(),
            None,
        );
        let joined = concat(&boxed, &floats, span).unwrap();
        let Storage::Boxed(elements) = &tensor(&joined).storage else {
            panic!("boxed numeric elements must keep their representation");
        };
        assert_eq!(crate::value::repr(&elements[0]), "3");
        assert_eq!(elements[1].as_f64(), Some(8.0));

        let source = dense(vec![2], vec![1.0, 2.0]);
        let reversed = flip_axis(&source, 0, span).unwrap();
        let joined = concat(tensor(&reversed), &floats, span).unwrap();
        assert_eq!(tensor(&joined).dense().unwrap(), [2.0, 1.0, 8.0]);

        let malformed = dense(vec![2], vec![1.0]);
        let incompatible = dense(vec![1, 1], vec![1.0]);
        for input in [&malformed, &incompatible] {
            assert_eq!(concat(input, &floats, span).unwrap_err().code, "E0802");
        }
    }

    #[test]
    fn boxed_axis_reversal_preserves_rank_and_logical_order() {
        let span = Span::default();
        let input = TensorData::new(
            vec![2, 2, 2],
            Storage::Boxed((0..8).map(Value::int).collect()),
            Dim::one(),
            None,
        );
        let reversed = flip_axis(&input, 1, span).expect("reverse middle axis");
        let reversed = tensor(&reversed);
        assert_eq!(reversed.shape, [2, 2, 2]);
        assert_eq!(
            reversed
                .elements()
                .iter()
                .map(Value::as_f64)
                .collect::<Option<Vec<_>>>(),
            Some(vec![2.0, 3.0, 0.0, 1.0, 6.0, 7.0, 4.0, 5.0]),
        );
    }

    #[test]
    fn merge_sort_keeps_ties_stable_and_propagates_comparison_failure() {
        let span = Span::default();
        let keys = [2, 1, 1, 0, 2];
        let order = merge_sort_indices(keys.len(), &mut |a, b| Ok(keys[a] > keys[b]), span)
            .expect("sort keys");
        assert_eq!(order, [3, 1, 2, 0, 4]);
        let error = merge_sort_indices(
            2,
            &mut |_, _| Err(EvalError::new("E0302", "comparator failed")),
            span,
        )
        .expect_err("comparison failure");
        assert_eq!(error.code, "E0302");
    }
}
