//! Sparse tensors (§8.4.1, §13.9.5): the runtime value behind
//! `Sparse<F, shape, τ>`.
//!
//! # Representation
//!
//! A [`SparseData`] is a shape, an element dimension, a storage format and the
//! stored entries:
//!
//! * `Csr` / `Csc` are rank 2 and compressed (`ptr`, `idx`);
//! * `Coo` is rank N and holds the flat, lexicographically sorted coordinates;
//! * `Bcsr` (block CSR, also spelled `Block`) is rank 2 and holds dense
//!   `br x bc` blocks in CSR layout over the *blocks*: `ptr` counts the stored
//!   blocks of each block row, `idx` their block columns (strictly
//!   increasing), and the values hold `br * bc` slots per stored block,
//!   row-major inside the block. The block shape is [`SparseData::block`]
//!   (`(1, 1)` for every other format).
//!
//! Every format stores each logical entry at most once, in sorted order, and
//! never stores an element equal to the implicit zero ([`SparseData::validate`]
//! checks this; it runs under `debug_assert!` after every construction).
//! `Bcsr` relaxes this in one place only: the slots *inside* a stored block
//! may be zero (padding), but every stored block holds at least one nonzero.
//! [`SparseData::nnz`] counts the logical nonzeros, never the padding
//! ([`SparseData::stored_len`] counts the slots), and the sorted
//! [`Entries`] form skips the padding, so every operation that goes through
//! it is format-blind.
//!
//! # Exactness
//!
//! The values are either an `f64` buffer or boxed [`Value`]s, exactly like the
//! dense [`crate::value::Storage`]: the `f64` class holds a sparse tensor *iff*
//! every stored element is a `Float` (and the implicit zero is a `Float`
//! zero). An `Int`, an exact `Real`, a `Complex` or a symbolic element keeps
//! its boxed representation, and every operation on boxed elements goes
//! through the language's own scalar arithmetic (`Interp::binary`), so
//! nothing exact is ever rounded to `f64`.
//!
//! # Implicit zero
//!
//! The zero of the element representation and dimension ([`SparseData::zero`]):
//! `0` for `Int`, exact `0` for `Real`, `0.0` for `Float`, `0+0i` for
//! `Complex`. It is what `A[i, j]` answers for an unstored position and what
//! `to_dense` fills in.
//!
//! Nothing here densifies except the functions whose result is dense by
//! definition (`to_dense`, `sparse ± dense`, `sparse @ dense`, `sum(A, axis)`).

#![cfg_attr(
    not(test),
    deny(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]

mod api;
mod kernels;

pub(crate) use api::SliceAxis;
pub(crate) use api::binary;
pub(crate) use api::call;
pub(crate) use api::call_typed;
pub(crate) use api::equal_data;
pub(crate) use api::index;
pub(crate) use api::negate;
pub(crate) use api::render;
pub(crate) use api::slice;

use crate::eval::EvalError;
use crate::eval::build_tensor;
use crate::value::NumKind;
use crate::value::NumVal;
use crate::value::RecordValue;
use crate::value::Value;
use aevia_dim::Dim;
use aevia_dim::UnitDef;
use aevia_syntax::Span;
use indexmap::IndexMap;
use num_rational::BigRational;
use num_traits::ToPrimitive;
use num_traits::Zero;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::rc::Rc;

pub(crate) type R<T> = Result<T, EvalError>;

/// The largest dense result a sparse operation may allocate.
pub(crate) const DENSE_LIMIT: usize = 100_000_000;

// ---------------------------------------------------------------------------
// Data
// ---------------------------------------------------------------------------

/// A storage format (§3, `Format`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SparseFormat {
    /// Compressed sparse row (rank 2).
    Csr,
    /// Compressed sparse column (rank 2).
    Csc,
    /// Coordinate list (rank N).
    Coo,
    /// Block compressed sparse row (rank 2); the type language also spells it
    /// `Block`.
    Bcsr,
}

impl SparseFormat {
    /// The spelling used in types and by `format_of`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            | Self::Csr => "Csr",
            | Self::Csc => "Csc",
            | Self::Coo => "Coo",
            | Self::Bcsr => "Bcsr",
        }
    }

    /// Reads a format name.
    ///
    /// `Block` is a synonym of `Bcsr`.
    ///
    /// # Errors
    /// `E0310` for a name that is not a format.
    pub fn parse(
        name: &str,
        span: Span,
    ) -> R<Self> {
        match name {
            | "Csr" | "CSR" => Ok(Self::Csr),
            | "Csc" | "CSC" => Ok(Self::Csc),
            | "Coo" | "COO" => Ok(Self::Coo),
            | "Block" | "Bcsr" | "BCSR" | "BlockCsr" => Ok(Self::Bcsr),
            | other => {
                Err(EvalError::new(
                    "E0310",
                    format!("`{other}` is not a sparse format (expected Csr, Csc, Coo or Bcsr)"),
                )
                .at(span))
            },
        }
    }
}

/// Where the stored entries live.
#[derive(Clone, Debug)]
pub(crate) enum Pattern {
    /// `nnz * rank` coordinates, sorted lexicographically (`Coo`).
    Coords(Vec<usize>),
    /// `ptr` has one more entry than the compressed axis; `idx` holds the
    /// other coordinate (`Csr`: columns, `Csc`: rows).
    Compressed { ptr: Vec<usize>, idx: Vec<usize> },
    /// Block CSR: `ptr` over block rows, `idx` the block columns; the values
    /// hold `br * bc` slots per block (`Bcsr`).
    Blocked { ptr: Vec<usize>, idx: Vec<usize> },
}

/// The stored element values.
#[derive(Clone, Debug)]
pub(crate) enum SpValues {
    /// Every stored element is a `Float`.
    F64(Vec<f64>),
    /// Any other element, one `Value::Num` each.
    Boxed(Vec<Value>),
}

/// A sparse tensor.
#[derive(Clone, Debug)]
pub struct SparseData {
    /// The storage format.
    pub format: SparseFormat,
    /// The logical shape.
    pub shape: Vec<usize>,
    /// The element dimension (the same witness `TensorData::dim` is).
    pub dim: Dim,
    /// A display unit applied to every element.
    pub display: Option<Rc<UnitDef>>,
    /// The implicit zero: the zero of the element representation, with `dim`.
    pub zero: NumVal,
    /// The block shape `(br, bc)` of a `Bcsr` tensor; `(1, 1)` otherwise.
    pub block: (usize, usize),
    pub(crate) pattern: Pattern,
    pub(crate) values: SpValues,
}

/// The sorted working form every format converts to and from.
#[derive(Clone, Debug)]
pub(crate) struct Entries<T> {
    pub rank: usize,
    pub coords: Vec<usize>,
    pub vals: Vec<T>,
}

impl<T> Entries<T> {
    pub const fn empty(rank: usize) -> Self {
        Self {
            rank,
            coords: Vec::new(),
            vals: Vec::new(),
        }
    }

    pub const fn len(&self) -> usize {
        self.vals.len()
    }

    pub fn coord(
        &self,
        index: usize,
    ) -> &[usize] {
        self.coord_checked(index).unwrap_or(&[])
    }

    fn coord_checked(
        &self,
        index: usize,
    ) -> Option<&[usize]> {
        let begin = index.checked_mul(self.rank)?;
        self.coords.get(begin..begin.checked_add(self.rank)?)
    }

    pub fn push(
        &mut self,
        coord: &[usize],
        value: T,
    ) {
        self.coords.extend_from_slice(coord);
        self.vals.push(value);
    }
}

const SIZE_ERROR: &str = "sparse storage exceeds the representable size";

fn reserve<T>(count: usize) -> Result<Vec<T>, String> {
    let mut items = Vec::new();
    items.try_reserve_exact(count).map_err(|_| SIZE_ERROR)?;
    Ok(items)
}

fn filled<T: Clone>(
    count: usize,
    value: T,
) -> Result<Vec<T>, String> {
    let mut items = reserve(count)?;
    items.resize(count, value);
    Ok(items)
}

/// The distinct blocks that hold a stored coordinate. Workspace follows the
/// stored block columns in one block row instead of the logical matrix width.
#[allow(
    clippy::single_call_fn,
    reason = "one block-geometry candidate has its own row-local occupancy scan"
)]
fn count_blocks(
    coords: &[[usize; 2]],
    br: usize,
    bc: usize,
) -> Option<usize> {
    let mut active_row = None;
    let mut columns = HashSet::new();
    let mut count = 0usize;
    for &[row, column] in coords {
        let block_row = row.checked_div(br)?;
        if active_row != Some(block_row) {
            active_row = Some(block_row);
            columns.clear();
        }
        if columns.insert(column.checked_div(bc)?) {
            count = count.checked_add(1)?;
        }
    }
    Some(count)
}

/// Chooses the largest block area whose padding uses at most three slots for
/// every two logical nonzeros. Equal areas prefer the smaller slot count,
/// then the earlier candidate.
#[allow(
    clippy::single_call_fn,
    reason = "automatic block-size selection is independent of constructing the chosen layout"
)]
fn auto_block(
    shape: &[usize],
    coords: &[usize],
) -> (usize, usize) {
    const CANDIDATES: [(usize, usize); 15] = [
        (4, 4),
        (3, 3),
        (2, 2),
        (4, 3),
        (3, 4),
        (4, 2),
        (2, 4),
        (3, 2),
        (2, 3),
        (4, 1),
        (1, 4),
        (3, 1),
        (1, 3),
        (2, 1),
        (1, 2),
    ];
    let [rows, cols] = shape else {
        return (1, 1);
    };
    let (coords, _) = coords.as_chunks::<2>();
    if coords.is_empty() {
        return (1, 1);
    }
    let mut best = (1, 1);
    let mut best_key = (1usize, u128::MAX);
    for (br, bc) in CANDIDATES {
        if !rows.is_multiple_of(br) || !cols.is_multiple_of(bc) {
            continue;
        }
        let area = br.saturating_mul(bc);
        if area < best_key.0 {
            continue;
        }
        let Some(count) = count_blocks(coords, br, bc) else {
            continue;
        };
        let slots = (count as u128).saturating_mul(area as u128);
        if slots.saturating_mul(2) <= (coords.len() as u128).saturating_mul(3)
            && (area > best_key.0 || slots < best_key.1)
        {
            best = (br, bc);
            best_key = (area, slots);
        }
    }
    best
}

fn block_geometry(
    rows: usize,
    cols: usize,
    (br, bc): (usize, usize),
) -> Result<(usize, usize, usize), String> {
    if br == 0 || bc == 0 || !rows.is_multiple_of(br) || !cols.is_multiple_of(bc) {
        return Err(format!(
            "block {br}x{bc} does not divide the shape [{rows}, {cols}]"
        ));
    }
    let block_rows = rows.checked_div(br).ok_or(SIZE_ERROR)?;
    let block_cols = cols.checked_div(bc).ok_or(SIZE_ERROR)?;
    let area = br.checked_mul(bc).ok_or(SIZE_ERROR)?;
    Ok((block_rows, block_cols, area))
}

/// The block-CSR pattern and values of sorted, zero-free entries.
fn blocked_parts<T: Store>(
    rows: usize,
    cols: usize,
    block: (usize, usize),
    entries: Entries<T>,
) -> Result<(Pattern, Vec<T>), String> {
    let (br, bc) = block;
    let (block_rows, _, area) = block_geometry(rows, cols, block)?;
    let mut ptr = reserve(block_rows.checked_add(1).ok_or(SIZE_ERROR)?)?;
    ptr.push(0);
    let mut idx = Vec::new();
    let mut vals = Vec::new();
    let mut stored = entries
        .coords
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .zip(entries.vals)
        .peekable();
    for block_row in 0..block_rows {
        let mut columns = BTreeMap::<usize, Vec<T>>::new();
        while let Some(([row, _], _)) = stored.peek()
            && row.checked_div(br) == Some(block_row)
        {
            let ([row, col], value) = stored.next().ok_or("missing stored block entry")?;
            let column = col.checked_div(bc).ok_or(SIZE_ERROR)?;
            let values = match columns.entry(column) {
                | std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(filled(area, T::pad_zero())?)
                },
                | std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            };
            let offset = row
                .checked_rem(br)
                .and_then(|r| r.checked_mul(bc))
                .zip(col.checked_rem(bc))
                .and_then(|(r, c)| r.checked_add(c))
                .ok_or(SIZE_ERROR)?;
            *values.get_mut(offset).ok_or("invalid block entry offset")? = value;
        }
        idx.try_reserve(columns.len()).map_err(|_| SIZE_ERROR)?;
        let slots = columns.len().checked_mul(area).ok_or(SIZE_ERROR)?;
        vals.try_reserve(slots).map_err(|_| SIZE_ERROR)?;
        for (column, block_values) in columns {
            idx.push(column);
            vals.extend(block_values);
        }
        ptr.push(idx.len());
    }
    Ok((Pattern::Blocked { ptr, idx }, vals))
}

fn compressed_parts<T>(
    lines: usize,
    nnz: usize,
    entries: impl Iterator<Item = ([usize; 2], T)>,
) -> Result<(Pattern, Vec<T>), String> {
    let mut ptr = filled(lines.checked_add(1).ok_or(SIZE_ERROR)?, 0usize)?;
    let mut idx = reserve(nnz)?;
    let mut values = reserve(nnz)?;
    for ([outer, inner], value) in entries {
        let count = outer
            .checked_add(1)
            .and_then(|index| ptr.get_mut(index))
            .ok_or("compressed entry is out of range")?;
        *count = count.saturating_add(1);
        idx.push(inner);
        values.push(value);
    }
    let mut cumulative = 0usize;
    for count in &mut ptr {
        cumulative = cumulative.saturating_add(*count);
        *count = cumulative;
    }
    Ok((Pattern::Compressed { ptr, idx }, values))
}

fn validate_coordinates(
    shape: &[usize],
    coords: &[usize],
    nnz: usize,
) -> Result<(), String> {
    let rank = shape.len();
    if rank.checked_mul(nnz) != Some(coords.len()) || (rank == 0 && nnz > 1) {
        return Err("coordinate list does not match the values".into());
    }
    if rank == 0 {
        return Ok(());
    }
    let mut previous = None;
    for coord in coords.chunks_exact(rank) {
        if coord.iter().zip(shape).any(|(c, e)| c >= e) {
            return Err("coordinate out of range".into());
        }
        if previous.is_some_and(|earlier| earlier >= coord) {
            return Err("coordinates are not strictly sorted".into());
        }
        previous = Some(coord);
    }
    Ok(())
}

fn validate_compressed(
    ptr: &[usize],
    idx: &[usize],
    outer: usize,
    inner: usize,
) -> Result<(), String> {
    if outer.checked_add(1) != Some(ptr.len())
        || ptr.first() != Some(&0)
        || ptr.last() != Some(&idx.len())
    {
        return Err("compressed pointers do not match the values".into());
    }
    for &[start, end] in ptr.array_windows::<2>() {
        let row = idx
            .get(start..end)
            .ok_or("invalid compressed pointer range")?;
        if row.iter().any(|&index| index >= inner) {
            return Err("index out of range".into());
        }
        if row
            .array_windows::<2>()
            .any(|[current, next]| current >= next)
        {
            return Err("indices are not strictly increasing".into());
        }
    }
    Ok(())
}

/// An element class: `f64` or a boxed [`Value`].
pub(crate) trait Store: Clone + 'static {
    /// The filler of a padding slot inside a stored block of a `Bcsr` tensor.
    fn pad_zero() -> Self;
    fn wrap(values: Vec<Self>) -> SpValues;
    fn view(values: &SpValues) -> Option<&[Self]>;
}

impl Store for f64 {
    fn pad_zero() -> Self {
        0.0
    }

    fn wrap(values: Vec<Self>) -> SpValues {
        SpValues::F64(values)
    }

    fn view(values: &SpValues) -> Option<&[Self]> {
        match values {
            | SpValues::F64(items) => Some(items),
            | SpValues::Boxed(_) => None,
        }
    }
}

impl Store for Value {
    fn pad_zero() -> Self {
        // Replaced by the zero of the final element class in `assemble`.
        Self::Num(zero_num(0, &Dim::one()))
    }

    fn wrap(values: Vec<Self>) -> SpValues {
        SpValues::Boxed(values)
    }

    fn view(values: &SpValues) -> Option<&[Self]> {
        match values {
            | SpValues::Boxed(items) => Some(items),
            | SpValues::F64(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Scalar helpers
// ---------------------------------------------------------------------------

pub(crate) const fn kind_rank(kind: &NumKind) -> u8 {
    match kind {
        | NumKind::Int(_) => 0,
        | NumKind::Exact(_) | NumKind::Sym(_) => 1,
        | NumKind::Float(_) => 2,
        | NumKind::Complex(..) => 3,
    }
}

pub(crate) fn zero_num(
    rank: u8,
    dim: &Dim,
) -> NumVal {
    let kind = match rank {
        | 0 => NumKind::Int(crate::int::Int::ZERO),
        | 1 => NumKind::Exact(Rc::new(BigRational::zero())),
        | 2 => NumKind::Float(0.0),
        | _ => NumKind::Complex(0.0, 0.0),
    };
    NumVal {
        kind,
        dim: dim.clone(),
        display: None,
    }
}

pub(crate) fn num_is_zero(num: &NumVal) -> bool {
    match &num.kind {
        | NumKind::Complex(re, im) => *re == 0.0 && *im == 0.0,
        | _ => num.is_zero(),
    }
}

pub(crate) fn err(
    code: &'static str,
    message: impl Into<String>,
    span: Span,
) -> EvalError {
    EvalError::new(code, message).at(span)
}

pub(crate) fn int_value(n: usize) -> Value {
    Value::Num(NumVal::int(n))
}

pub(crate) fn int_tensor(items: &[usize]) -> Value {
    build_tensor(
        items.iter().map(|n| int_value(*n)).collect(),
        vec![items.len()],
        Dim::one(),
    )
}

pub(crate) fn tuple_of(items: Vec<Value>) -> Value {
    let mut fields = IndexMap::new();
    for (index, item) in items.into_iter().enumerate() {
        fields.insert(index.to_string(), item);
    }
    Value::Record(Rc::new(RecordValue { fields }))
}

pub(crate) fn record_of(fields: Vec<(&str, Value)>) -> Value {
    let mut map = IndexMap::new();
    for (name, value) in fields {
        map.insert(name.to_string(), value);
    }
    Value::Record(Rc::new(RecordValue { fields: map }))
}

pub(crate) fn element_count(
    shape: &[usize],
    span: Span,
) -> R<usize> {
    crate::value::checked_element_count(shape)
        .filter(|count| *count <= DENSE_LIMIT)
        .ok_or_else(|| {
            err(
                "E0802",
                format!("a dense tensor of shape {shape:?} is too large to build"),
                span,
            )
        })
}

/// A non-negative `Int` (or integral `Float`) as `usize`.
pub(crate) fn index_of(
    value: &Value,
    what: &str,
    span: Span,
) -> R<usize> {
    match value {
        | Value::Num(num) => {
            let exact = match &num.kind {
                | NumKind::Int(n) => n.to_usize(),
                | NumKind::Float(f) if f.fract() == 0.0 => f.to_usize(),
                | NumKind::Exact(q) if q.is_integer() => q.to_integer().to_usize(),
                | _ => None,
            };
            exact.ok_or_else(|| {
                err(
                    "E0802",
                    format!(
                        "`{what}` holds {}, which is not a non-negative Int",
                        crate::value::show(value)
                    ),
                    span,
                )
            })
        },
        | other => {
            Err(err(
                "E0302",
                format!("`{what}` expects Int entries, found {}", other.type_name()),
                span,
            ))
        },
    }
}

/// A tensor of non-negative `Int`s as `usize`s.
pub(crate) fn index_list(
    value: &Value,
    what: &str,
    span: Span,
) -> R<Vec<usize>> {
    let Value::Tensor(tensor) = value else {
        return Err(err(
            "E0302",
            format!(
                "`{what}` expects a tensor of Int, found {}",
                value.type_name()
            ),
            span,
        ));
    };
    if tensor.checked_element_count() != Some(tensor.storage.len()) {
        return Err(err(
            "E0802",
            format!("`{what}` does not match its shape"),
            span,
        ));
    }
    let items = tensor.elements();
    if items.len() != tensor.storage.len() {
        return Err(err(
            "E0802",
            format!("`{what}` has unreadable elements"),
            span,
        ));
    }
    items
        .iter()
        .map(|item| index_of(item, what, span))
        .collect()
}

// ---------------------------------------------------------------------------
// Construction and conversion
// ---------------------------------------------------------------------------

impl SparseData {
    /// The number of logically nonzero entries (the padding inside the
    /// blocks of a `Bcsr` tensor is not counted).
    #[must_use]
    pub fn nnz(&self) -> usize {
        match &self.pattern {
            | Pattern::Blocked { .. } => self.nonzero_mask().iter().filter(|k| **k).count(),
            | _ => self.stored_len(),
        }
    }

    /// The number of stored value slots: `nnz` for every format but `Bcsr`,
    /// where it includes the padding zeros of the stored blocks.
    #[must_use]
    pub const fn stored_len(&self) -> usize {
        match &self.values {
            | SpValues::F64(items) => items.len(),
            | SpValues::Boxed(items) => items.len(),
        }
    }

    /// For each stored slot, whether it holds a logical nonzero.
    pub(crate) fn nonzero_mask(&self) -> Vec<bool> {
        match &self.values {
            | SpValues::F64(items) => items.iter().map(|v| *v != 0.0).collect(),
            | SpValues::Boxed(items) => {
                items
                    .iter()
                    .map(|v| !matches!(v, Value::Num(n) if num_is_zero(n)))
                    .collect()
            },
        }
    }

    /// The rank.
    #[must_use]
    pub const fn rank(&self) -> usize {
        self.shape.len()
    }

    /// True when the values are an `f64` buffer.
    #[must_use]
    pub const fn is_f64(&self) -> bool {
        matches!(self.values, SpValues::F64(_))
    }

    /// The stored entries as `(coordinates, value)` in sorted order.
    /// Returns an empty list if mutated host metadata invalidates the layout.
    #[must_use]
    pub fn stored(&self) -> Vec<(Vec<usize>, Value)> {
        self.checked_stored().unwrap_or_default()
    }

    fn checked_stored(&self) -> Result<Vec<(Vec<usize>, Value)>, String> {
        let entries = self.try_entries_with(self.boxed_values())?;
        let mut stored = reserve(entries.len())?;
        if entries.rank == 0 {
            stored.extend(entries.vals.into_iter().map(|value| (Vec::new(), value)));
        } else {
            stored.extend(
                entries
                    .coords
                    .chunks_exact(entries.rank)
                    .map(<[usize]>::to_vec)
                    .zip(entries.vals),
            );
        }
        Ok(stored)
    }

    /// The values in the order of the storage (`Csc`: column-major).
    pub(crate) fn boxed_values(&self) -> Vec<Value> {
        match &self.values {
            | SpValues::Boxed(items) => items.clone(),
            | SpValues::F64(items) => {
                items
                    .iter()
                    .map(|v| Value::Num(NumVal::float(*v, self.dim.clone())))
                    .collect()
            },
        }
    }

    /// Entries in sorted order, with `values` (given in storage order)
    /// permuted to match.
    pub(crate) fn entries_with<T: Clone>(
        &self,
        values: Vec<T>,
    ) -> Entries<T> {
        self.try_entries_with(values)
            .unwrap_or_else(|_| Entries::empty(self.rank()))
    }

    fn try_entries_with<T: Clone>(
        &self,
        values: Vec<T>,
    ) -> Result<Entries<T>, String> {
        self.validate_layout()?;
        if values.len() != self.stored_len() {
            return Err("sparse values do not match the stored layout".into());
        }
        let rank = self.rank();
        match &self.pattern {
            | Pattern::Coords(coords) => {
                Ok(Entries {
                    rank,
                    coords: coords.clone(),
                    vals: values,
                })
            },
            | Pattern::Compressed { ptr, idx } => {
                let coordinate_count = idx.len().checked_mul(2).ok_or(SIZE_ERROR)?;
                let mut coords = reserve(coordinate_count)?;
                if self.format == SparseFormat::Csc {
                    let mut pairs = reserve(idx.len())?;
                    let mut values = values.into_iter();
                    for (column, &[start, end]) in ptr.array_windows::<2>().enumerate() {
                        let rows = idx.get(start..end).ok_or("invalid compressed row range")?;
                        pairs.extend(
                            rows.iter()
                                .map(|&row| [row, column])
                                .zip(values.by_ref().take(rows.len())),
                        );
                    }
                    pairs.sort_unstable_by_key(|(coord, _)| *coord);
                    let mut vals = reserve(pairs.len())?;
                    for (coord, value) in pairs {
                        coords.extend(coord);
                        vals.push(value);
                    }
                    Ok(Entries { rank, coords, vals })
                } else {
                    for (row, &[start, end]) in ptr.array_windows::<2>().enumerate() {
                        let columns = idx
                            .get(start..end)
                            .ok_or("invalid compressed column range")?;
                        coords.extend(columns.iter().flat_map(|&column| [row, column]));
                    }
                    Ok(Entries {
                        rank,
                        coords,
                        vals: values,
                    })
                }
            },
            | Pattern::Blocked { ptr, idx } => self.decode_blocks(ptr, idx, &values),
        }
    }

    fn decode_blocks<T: Clone>(
        &self,
        ptr: &[usize],
        idx: &[usize],
        values: &[T],
    ) -> Result<Entries<T>, String> {
        let (br, bc) = self.block;
        let area = br.checked_mul(bc).ok_or(SIZE_ERROR)?;
        let live = self.nonzero_mask();
        let mut entries = Entries::empty(2);
        for (block_row, &[start, end]) in ptr.array_windows::<2>().enumerate() {
            if start == end {
                continue;
            }
            let columns = idx.get(start..end).ok_or("invalid block column range")?;
            let first_row = block_row.checked_mul(br).ok_or(SIZE_ERROR)?;
            for local_row in 0..br {
                let row = first_row.checked_add(local_row).ok_or(SIZE_ERROR)?;
                for (block, &column) in (start..end).zip(columns) {
                    let first_col = column.checked_mul(bc).ok_or(SIZE_ERROR)?;
                    let offset = block
                        .checked_mul(area)
                        .zip(local_row.checked_mul(bc))
                        .and_then(|(block, row)| block.checked_add(row))
                        .ok_or(SIZE_ERROR)?;
                    let end = offset.checked_add(bc).ok_or(SIZE_ERROR)?;
                    let row_values = values.get(offset..end).ok_or("invalid block value range")?;
                    let row_live = live.get(offset..end).ok_or("invalid block mask range")?;
                    for (local_col, (value, &nonzero)) in
                        row_values.iter().zip(row_live).enumerate()
                    {
                        if nonzero {
                            let col = first_col.checked_add(local_col).ok_or(SIZE_ERROR)?;
                            entries.push(&[row, col], value.clone());
                        }
                    }
                }
            }
        }
        Ok(entries)
    }

    /// The typed entries, when the storage class is `T`'s and its layout is coherent.
    pub(crate) fn entries<T: Store>(&self) -> Option<Entries<T>> {
        let values = T::view(&self.values)?.to_vec();
        self.try_entries_with(values).ok()
    }

    /// Builds a sparse tensor from sorted, duplicate-free, zero-free entries.
    ///
    /// `zero_rank` is the representation class of the implicit zero for boxed
    /// storage (see [`kind_rank`]); the stored values raise it.
    pub(crate) fn from_entries<T: Store>(
        format: SparseFormat,
        shape: Vec<usize>,
        dim: Dim,
        display: Option<Rc<UnitDef>>,
        zero_rank: u8,
        entries: Entries<T>,
    ) -> Result<Self, String> {
        if entries.rank != shape.len() {
            return Err("entry rank does not match the sparse shape".into());
        }
        validate_coordinates(&shape, &entries.coords, entries.len())?;
        let mut block = (1, 1);
        let (pattern, values) = match (format, shape.as_slice()) {
            | (SparseFormat::Coo, _) => (Pattern::Coords(entries.coords), entries.vals),
            | (SparseFormat::Bcsr, [rows, cols]) => {
                block = auto_block(&shape, &entries.coords);
                blocked_parts(*rows, *cols, block, entries)?
            },
            | (SparseFormat::Csr, [rows, _]) => {
                let nnz = entries.len();
                compressed_parts(
                    *rows,
                    nnz,
                    entries
                        .coords
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .copied()
                        .zip(entries.vals),
                )?
            },
            | (SparseFormat::Csc, [_, cols]) => {
                let nnz = entries.len();
                let mut columns = reserve(nnz)?;
                columns.extend(
                    entries
                        .coords
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|&[row, col]| [col, row])
                        .zip(entries.vals),
                );
                columns.sort_unstable_by_key(|(coord, _)| *coord);
                compressed_parts(*cols, nnz, columns.into_iter())?
            },
            | _ => {
                return Err(format!(
                    "{} must have rank 2, found {}",
                    format.name(),
                    shape.len()
                ));
            },
        };
        Ok(Self::assemble(
            format,
            shape,
            dim,
            display,
            zero_rank,
            block,
            pattern,
            T::wrap(values),
        ))
    }

    /// Builds a `Bcsr` tensor with an explicit block shape that tiles the matrix.
    pub(crate) fn from_entries_blocked<T: Store>(
        shape: Vec<usize>,
        dim: Dim,
        display: Option<Rc<UnitDef>>,
        zero_rank: u8,
        block: (usize, usize),
        entries: Entries<T>,
    ) -> Result<Self, String> {
        let [rows, cols] = shape.as_slice() else {
            return Err("Bcsr requires a rank-2 shape".into());
        };
        if entries.rank != 2 {
            return Err("entry rank does not match the sparse shape".into());
        }
        validate_coordinates(&shape, &entries.coords, entries.len())?;
        let (pattern, values) = blocked_parts(*rows, *cols, block, entries)?;
        Ok(Self::assemble(
            SparseFormat::Bcsr,
            shape,
            dim,
            display,
            zero_rank,
            block,
            pattern,
            T::wrap(values),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn assemble(
        format: SparseFormat,
        shape: Vec<usize>,
        dim: Dim,
        display: Option<Rc<UnitDef>>,
        zero_rank: u8,
        block: (usize, usize),
        pattern: Pattern,
        values: SpValues,
    ) -> Self {
        let mut rank = zero_rank;
        let mut values = values;
        if let SpValues::Boxed(items) = &mut values {
            // Padding zeros (only inside `Bcsr` blocks) do not raise the class.
            let live = |v: &Value| !matches!(v, Value::Num(n) if num_is_zero(n));
            for item in items.iter().filter(|v| live(v)) {
                if let Value::Num(num) = item {
                    rank = rank.max(kind_rank(&num.kind));
                }
            }
            // The `f64` class: every stored element, and the zero, a `Float`.
            let all_float = rank == 2
                && items
                    .iter()
                    .filter(|v| live(v))
                    .all(|v| matches!(v, Value::Num(n) if matches!(n.kind, NumKind::Float(_))));
            if all_float {
                let floats = items
                    .iter()
                    .map(|v| {
                        match v {
                            | Value::Num(NumVal {
                                kind: NumKind::Float(f),
                                ..
                            }) => *f,
                            | _ => 0.0,
                        }
                    })
                    .collect();
                values = SpValues::F64(floats);
            } else if matches!(pattern, Pattern::Blocked { .. }) {
                // Padding takes the zero of the final element class.
                let zero = Value::Num(zero_num(rank, &dim));
                for item in items.iter_mut() {
                    if !live(item) {
                        *item = zero.clone();
                    }
                }
            }
        } else {
            rank = 2;
        }
        let zero = zero_num(rank, &dim);
        let data = Self {
            format,
            shape,
            dim,
            display,
            zero,
            block,
            pattern,
            values,
        };
        debug_assert!(data.validate().is_ok(), "{:?}", data.validate());
        data
    }

    fn validate_layout(&self) -> Result<(), String> {
        let nnz = self.stored_len();
        if self.format != SparseFormat::Bcsr && self.block != (1, 1) {
            return Err("the block shape does not match the format".into());
        }
        match (self.format, self.shape.as_slice(), &self.pattern) {
            | (SparseFormat::Coo, shape, Pattern::Coords(coords)) => {
                validate_coordinates(shape, coords, nnz)
            },
            | (SparseFormat::Csr, &[rows, cols], Pattern::Compressed { ptr, idx })
            | (SparseFormat::Csc, &[cols, rows], Pattern::Compressed { ptr, idx }) => {
                if idx.len() != nnz {
                    return Err("compressed indices do not match the values".into());
                }
                validate_compressed(ptr, idx, rows, cols)
            },
            | (SparseFormat::Bcsr, &[rows, cols], Pattern::Blocked { ptr, idx }) => {
                let (block_rows, block_cols, area) = block_geometry(rows, cols, self.block)?;
                if idx.len().checked_mul(area) != Some(nnz) {
                    return Err("block indices do not match the values".into());
                }
                validate_compressed(ptr, idx, block_rows, block_cols)
            },
            | _ => Err("sparse shape or pattern does not match the format".into()),
        }
    }

    /// Checks the representation invariants.
    ///
    /// # Errors
    /// A description of the first violated invariant.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_layout()?;
        if matches!(self.pattern, Pattern::Blocked { .. }) {
            let (br, bc) = self.block;
            let area = br.checked_mul(bc).ok_or(SIZE_ERROR)?;
            if self
                .nonzero_mask()
                .chunks_exact(area)
                .any(|block| !block.iter().any(|live| *live))
            {
                return Err("an all-zero block is stored".into());
            }
        } else {
            let explicit_zero = match &self.values {
                | SpValues::F64(items) => items.contains(&0.0),
                | SpValues::Boxed(items) => {
                    items
                        .iter()
                        .any(|v| matches!(v, Value::Num(n) if num_is_zero(n)))
                },
            };
            if explicit_zero {
                return Err("an explicit zero is stored".into());
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Kernel bridge
// ---------------------------------------------------------------------------

impl SparseData {
    /// The stored entries as flat row-major indices (ascending) and values:
    /// the layout the kernel's sparse value uses. Nothing is densified.
    ///
    /// # Errors
    /// A description of invalid host metadata or an offset outside `u64`.
    pub fn to_flat_parts(&self) -> Result<(Vec<u64>, Vec<Value>), String> {
        let stored = self.checked_stored()?;
        let mut flat = reserve(stored.len())?;
        let mut values = reserve(stored.len())?;
        for (coord, value) in stored {
            let mut index: u64 = 0;
            for (c, extent) in coord.iter().zip(&self.shape) {
                let extent = u64::try_from(*extent).map_err(|_| SIZE_ERROR)?;
                let coordinate = u64::try_from(*c).map_err(|_| SIZE_ERROR)?;
                index = index
                    .checked_mul(extent)
                    .and_then(|i| i.checked_add(coordinate))
                    .ok_or_else(|| {
                        format!(
                            "the flat index of a {:?} sparse tensor overflows",
                            self.shape
                        )
                    })?;
            }
            flat.push(index);
            values.push(value);
        }
        Ok((flat, values))
    }

    /// A sparse tensor from the kernel's layout: ascending flat row-major
    /// indices and one value per index. Explicit zeros are dropped.
    ///
    /// # Errors
    /// A description of the first violated precondition.
    pub fn from_flat_parts(
        format: SparseFormat,
        shape: Vec<usize>,
        flat: &[u64],
        values: Vec<Value>,
        dim: Dim,
    ) -> Result<Self, String> {
        if flat.len() != values.len() {
            return Err(format!(
                "{} indices for {} values",
                flat.len(),
                values.len()
            ));
        }
        if shape.is_empty() || (format != SparseFormat::Coo && shape.len() != 2) {
            return Err(format!(
                "a {} tensor cannot have shape {shape:?}",
                format.name()
            ));
        }
        let rank = shape.len();
        let mut entries: Entries<Value> = Entries::empty(rank);
        let mut coord = filled(rank, 0usize)?;
        let mut zero_rank_seen = 0u8;
        let mut previous = None;
        for (&index, value) in flat.iter().zip(values) {
            if previous.is_some_and(|earlier| earlier >= index) {
                return Err(format!("index {index} is out of range or out of order"));
            }
            // Decode the individual extents so the whole logical product
            // does not have to fit in a host-sized integer.
            let remaining = shape.iter().zip(&mut coord).rev().try_fold(
                u128::from(index),
                |remaining, (&extent, coordinate)| {
                    let extent = extent as u128;
                    *coordinate = usize::try_from(remaining.checked_rem(extent)?).ok()?;
                    remaining.checked_div(extent)
                },
            );
            if remaining != Some(0) {
                return Err(format!("index {index} is out of range or out of order"));
            }
            previous = Some(index);
            let Value::Num(num) = &value else {
                return Err("a sparse element must be a number".to_string());
            };
            zero_rank_seen = zero_rank_seen.max(kind_rank(&num.kind));
            if num_is_zero(num) {
                continue;
            }
            entries.push(&coord, value);
        }
        Self::from_entries(format, shape, dim, None, zero_rank_seen, entries)
    }
}


#[cfg(test)]
mod storage_boundary_tests {
    use super::*;

    #[test]
    fn paired_wide_sparse_shapes_need_only_stored_coordinates() -> Result<(), String> {
        let extent = u64::try_from(usize::MAX).map_err(|_| SIZE_ERROR)?;
        let last = extent.saturating_sub(1);
        for (format, shape, index, expected_coord) in [
            (
                SparseFormat::Coo,
                vec![usize::MAX, 2],
                extent,
                vec![usize::MAX / 2, 1],
            ),
            (
                SparseFormat::Csc,
                vec![usize::MAX, 1],
                last,
                vec![usize::MAX - 1, 0],
            ),
            (
                SparseFormat::Bcsr,
                vec![1, usize::MAX],
                last,
                vec![0, usize::MAX - 1],
            ),
        ] {
            let value = SparseData::from_flat_parts(
                format,
                shape,
                &[index],
                vec![Value::float(4.0)],
                Dim::one(),
            )?;
            value.validate()?;
            let stored = value.stored();
            let [(coords, number)] = stored.as_slice() else {
                return Err("expected exactly one sparse entry".to_string());
            };
            assert_eq!(*coords, expected_coord);
            assert_eq!(number.as_f64(), Some(4.0));
            assert_eq!(value.to_flat_parts()?.0, [index]);
        }
        Ok(())
    }

    #[test]
    fn compressed_capacity_and_invalid_public_metadata_report_errors() -> Result<(), String> {
        let impossible = SparseData::from_flat_parts(
            SparseFormat::Csr,
            vec![usize::MAX, 1],
            &[],
            Vec::new(),
            Dim::one(),
        );
        assert!(impossible.is_err());
        assert!(
            SparseData::from_flat_parts(
                SparseFormat::Coo,
                vec![0, usize::MAX],
                &[0],
                vec![Value::int(1)],
                Dim::one()
            )
            .is_err()
        );

        let mut invalid = SparseData::from_flat_parts(
            SparseFormat::Csr,
            vec![2, 2],
            &[1],
            vec![Value::int(1)],
            Dim::one(),
        )?;
        invalid.shape = vec![usize::MAX, 1];
        assert!(invalid.validate().is_err());
        assert!(invalid.stored().is_empty());
        assert!(invalid.to_flat_parts().is_err());
        invalid.format = SparseFormat::Coo;
        invalid.shape = vec![1];
        assert!(invalid.validate().is_err());

        let mut blocked = SparseData::from_flat_parts(
            SparseFormat::Bcsr,
            vec![2, 2],
            &[1],
            vec![Value::int(1)],
            Dim::one(),
        )?;
        blocked.block = (usize::MAX, usize::MAX);
        assert!(blocked.validate().is_err());
        assert!(blocked.to_flat_parts().is_err());
        Ok(())
    }
}
