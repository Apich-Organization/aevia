//! The algebra of sparse tensors, generic over the element class.
//!
//! Every kernel here is written once against [`Ring`] and instantiated for
//! `f64` ([`FloatRing`]) and for boxed [`Value`]s ([`ValueRing`], which routes
//! each scalar operation through the language's own arithmetic so exact
//! values stay exact).

#![cfg_attr(
    not(test),
    deny(
        missing_docs,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::nursery,
        clippy::panic,
        clippy::pedantic,
        clippy::single_call_fn,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]

use super::Entries;
use super::R;
use super::Store;
use super::err;
use super::kind_rank;
use super::num_is_zero;
use super::zero_num;
use crate::eval::EvalError;
use crate::eval::Interp;
use crate::value::Value;
use aevia_syntax::Span;
use aevia_syntax::ast::BinOp;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::collections::HashMap;

/// The scalar arithmetic a kernel needs, for one element class.
pub(super) trait Ring<T: Clone> {
    fn add(
        &mut self,
        a: &T,
        b: &T,
    ) -> R<T>;
    fn sub(
        &mut self,
        a: &T,
        b: &T,
    ) -> R<T>;
    fn mul(
        &mut self,
        a: &T,
        b: &T,
    ) -> R<T>;
    fn div(
        &mut self,
        a: &T,
        b: &T,
    ) -> R<T>;
    fn is_zero(
        &self,
        a: &T,
    ) -> bool;
    /// The zero of `a`'s representation and dimension.
    fn zero_like(
        &self,
        a: &T,
    ) -> T;
}

pub(super) struct FloatRing;

impl Ring<f64> for FloatRing {
    fn add(
        &mut self,
        a: &f64,
        b: &f64,
    ) -> R<f64> {
        Ok(a + b)
    }

    fn sub(
        &mut self,
        a: &f64,
        b: &f64,
    ) -> R<f64> {
        Ok(a - b)
    }

    fn mul(
        &mut self,
        a: &f64,
        b: &f64,
    ) -> R<f64> {
        Ok(a * b)
    }

    fn div(
        &mut self,
        a: &f64,
        b: &f64,
    ) -> R<f64> {
        Ok(a / b)
    }

    fn is_zero(
        &self,
        a: &f64,
    ) -> bool {
        *a == 0.0
    }

    fn zero_like(
        &self,
        _: &f64,
    ) -> f64 {
        0.0
    }
}

pub(super) struct ValueRing<'a> {
    pub interp: &'a mut Interp,
    pub span: Span,
}

impl ValueRing<'_> {
    fn apply(
        &mut self,
        op: BinOp,
        a: &Value,
        b: &Value,
    ) -> R<Value> {
        self.interp.binary(op, a.clone(), b.clone(), self.span)
    }
}

impl Ring<Value> for ValueRing<'_> {
    fn add(
        &mut self,
        a: &Value,
        b: &Value,
    ) -> R<Value> {
        self.apply(BinOp::Add, a, b)
    }

    fn sub(
        &mut self,
        a: &Value,
        b: &Value,
    ) -> R<Value> {
        self.apply(BinOp::Sub, a, b)
    }

    fn mul(
        &mut self,
        a: &Value,
        b: &Value,
    ) -> R<Value> {
        self.apply(BinOp::Mul, a, b)
    }

    fn div(
        &mut self,
        a: &Value,
        b: &Value,
    ) -> R<Value> {
        self.apply(BinOp::Div, a, b)
    }

    fn is_zero(
        &self,
        a: &Value,
    ) -> bool {
        matches!(a, Value::Num(num) if num_is_zero(num))
    }

    fn zero_like(
        &self,
        a: &Value,
    ) -> Value {
        match a {
            | Value::Num(num) => Value::Num(zero_num(kind_rank(&num.kind), &num.dim)),
            | other => other.clone(),
        }
    }
}

fn invalid_layout() -> EvalError {
    EvalError::new(
        "E0802",
        "sparse coordinates or storage do not match the shape",
    )
}

fn shape_overflow() -> EvalError {
    EvalError::new(
        "E0802",
        "sparse operation output exceeds the representable size",
    )
}

fn reserved<T>(count: usize) -> R<Vec<T>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| shape_overflow())?;
    Ok(values)
}

fn filled<T: Clone>(
    count: usize,
    value: T,
) -> R<Vec<T>> {
    let mut values = reserved(count)?;
    values.resize(count, value);
    Ok(values)
}

fn coordinates(
    coords: &[usize],
    rank: usize,
    index: usize,
) -> Option<&[usize]> {
    let start = index.checked_mul(rank)?;
    coords.get(start..start.checked_add(rank)?)
}

fn matches_rank<T>(entries: &Entries<T>) -> bool {
    entries.rank.checked_mul(entries.vals.len()) == Some(entries.coords.len())
}

fn stored_entries<T>(entries: &Entries<T>) -> R<impl Iterator<Item = (&[usize], &T)>> {
    if !matches_rank(entries) {
        return Err(invalid_layout());
    }
    let iter = entries.vals.iter().enumerate().filter_map(|(i, value)| {
        coordinates(&entries.coords, entries.rank, i).map(|coord| (coord, value))
    });
    Ok(iter)
}

/// Drops every entry equal to zero.
fn clean<T: Clone, Rg: Ring<T>>(
    entries: Entries<T>,
    ring: &Rg,
) -> R<Entries<T>> {
    if !matches_rank(&entries) {
        return Err(invalid_layout());
    }
    if entries.vals.iter().all(|v| !ring.is_zero(v)) {
        return Ok(entries);
    }
    let mut out = Entries::empty(entries.rank);
    for (coord, value) in stored_entries(&entries)? {
        if !ring.is_zero(value) {
            out.push(coord, value.clone());
        }
    }
    Ok(out)
}

/// Sorts unordered coordinate/value triplets, sums duplicates and drops zeros.
pub(super) fn assemble<T: Clone, Rg: Ring<T>>(
    rank: usize,
    coords: &[usize],
    vals: Vec<T>,
    ring: &mut Rg,
) -> R<Entries<T>> {
    if rank.checked_mul(vals.len()) != Some(coords.len()) {
        return Err(invalid_layout());
    }
    let mut entries = vals
        .into_iter()
        .enumerate()
        .map(|(i, value)| {
            let coord = coordinates(coords, rank, i).ok_or_else(invalid_layout)?;
            Ok((coord, value))
        })
        .collect::<R<Vec<_>>>()?;
    // Equal coordinates must retain their original input order for arithmetic.
    entries.sort_by_key(|(coord, _)| *coord);
    let mut out = Entries::empty(rank);
    let mut pending: Option<(&[usize], T)> = None;
    for (coord, value) in entries {
        if let Some((last_coord, last_value)) = pending.as_mut()
            && *last_coord == coord
        {
            *last_value = ring.add(last_value, &value)?;
        } else if let Some((last_coord, last_value)) = pending.replace((coord, value))
            && !ring.is_zero(&last_value)
        {
            out.push(last_coord, last_value);
        }
    }
    if let Some((coord, value)) = pending
        && !ring.is_zero(&value)
    {
        out.push(coord, value);
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Merge {
    Add,
    Sub,
    Mul,
}

/// `a + b`, `a - b` or the Hadamard product, over the union (resp.
/// intersection) of the patterns.
pub(super) fn merge<T: Clone, Rg: Ring<T>>(
    a: &Entries<T>,
    b: &Entries<T>,
    mode: Merge,
    ring: &mut Rg,
) -> R<Entries<T>> {
    if a.rank != b.rank {
        return Err(invalid_layout());
    }
    let mut out = Entries::empty(a.rank);
    let mut left = stored_entries(a)?.peekable();
    let mut right = stored_entries(b)?.peekable();
    loop {
        let order = match (left.peek(), right.peek()) {
            | (Some((a, _)), Some((b, _))) => a.cmp(b),
            | (Some(_), None) => Ordering::Less,
            | (None, Some(_)) => Ordering::Greater,
            | (None, None) => break,
        };
        match order {
            | Ordering::Equal => {
                let (coord, a) = left.next().ok_or_else(invalid_layout)?;
                let (_, b) = right.next().ok_or_else(invalid_layout)?;
                let value = match mode {
                    | Merge::Add => ring.add(a, b)?,
                    | Merge::Sub => ring.sub(a, b)?,
                    | Merge::Mul => ring.mul(a, b)?,
                };
                out.push(coord, value);
            },
            | Ordering::Less => {
                let (coord, value) = left.next().ok_or_else(invalid_layout)?;
                if mode != Merge::Mul {
                    out.push(coord, value.clone());
                }
            },
            | Ordering::Greater => {
                let (coord, value) = right.next().ok_or_else(invalid_layout)?;
                if mode != Merge::Mul {
                    let value = if mode == Merge::Sub {
                        let zero = ring.zero_like(value);
                        ring.sub(&zero, value)?
                    } else {
                        value.clone()
                    };
                    out.push(coord, value);
                }
            },
        }
    }
    clean(out, ring)
}

/// Applies `f` to every stored value; the result is cleaned.
pub(super) fn map_values<T: Clone, Rg: Ring<T>>(
    a: &Entries<T>,
    ring: &mut Rg,
    mut f: impl FnMut(&mut Rg, &T) -> R<T>,
) -> R<Entries<T>> {
    let mut vals = Vec::with_capacity(a.len());
    for value in &a.vals {
        vals.push(f(ring, value)?);
    }
    clean(
        Entries {
            rank: a.rank,
            coords: a.coords.clone(),
            vals,
        },
        ring,
    )
}

/// The flat row-major offset of an in-bounds coordinate in `shape`.
pub(super) fn flat_index(
    coord: &[usize],
    shape: &[usize],
) -> Option<usize> {
    if coord.len() != shape.len() {
        return None;
    }
    coord
        .iter()
        .zip(shape)
        .try_fold(0usize, |flat, (&coordinate, &extent)| {
            if coordinate >= extent {
                return None;
            }
            flat.checked_mul(extent)?.checked_add(coordinate)
        })
}

/// The coordinates of a flat row-major offset.
pub(super) fn unflatten(
    mut flat: usize,
    shape: &[usize],
    out: &mut [usize],
) {
    for (&extent, coordinate) in shape.iter().rev().zip(out.iter_mut().rev()) {
        // An empty axis contributes zero and leaves the quotient unchanged.
        if let Some(extent) = std::num::NonZeroUsize::new(extent) {
            *coordinate = flat % extent;
            flat /= extent;
        } else {
            *coordinate = 0;
        }
    }
}

/// `sparse ∘ dense` over the stored entries only.
pub(super) fn hadamard_dense<T: Clone, Rg: Ring<T>>(
    a: &Entries<T>,
    shape: &[usize],
    dense: &[T],
    ring: &mut Rg,
) -> R<Entries<T>> {
    let mut out = Entries::empty(a.rank);
    for (coord, sparse) in stored_entries(a)? {
        let rhs = flat_index(coord, shape)
            .and_then(|offset| dense.get(offset))
            .ok_or_else(invalid_layout)?;
        let value = ring.mul(sparse, rhs)?;
        out.push(coord, value);
    }
    clean(out, ring)
}

pub(super) fn transpose<T: Clone>(
    a: &Entries<T>,
    cols: usize,
) -> R<Entries<T>> {
    if a.rank != 2 {
        return Err(invalid_layout());
    }
    let mut transposed = reserved(a.len())?;
    for (coord, value) in stored_entries(a)? {
        let [row, col] = coord else {
            return Err(invalid_layout());
        };
        if *col >= cols {
            return Err(invalid_layout());
        }
        transposed.push(([*col, *row], value));
    }
    transposed.sort_unstable_by_key(|(coord, _)| *coord);
    let mut out = Entries::empty(2);
    out.coords = reserved(a.coords.len())?;
    out.vals = reserved(a.vals.len())?;
    for (coord, value) in transposed {
        out.push(&coord, value.clone());
    }
    Ok(out)
}

/// CSR arrays of a rank-2 entry list.
pub(super) struct Csr<'a, T: Clone> {
    pub rows: usize,
    pub cols: usize,
    pub ptr: Cow<'a, [usize]>,
    pub idx: Cow<'a, [usize]>,
    pub vals: Cow<'a, [T]>,
}

impl<T: Clone> Csr<'_, T> {
    fn row(
        &self,
        row: usize,
    ) -> R<impl Iterator<Item = (usize, &T)>> {
        let [start, end] = self
            .ptr
            .get(row..)
            .and_then(<[usize]>::first_chunk::<2>)
            .copied()
            .ok_or_else(invalid_layout)?;
        let idx = self.idx.get(start..end).ok_or_else(invalid_layout)?;
        let vals = self.vals.get(start..end).ok_or_else(invalid_layout)?;
        Ok(idx.iter().copied().zip(vals))
    }
}

fn compressed_layout(
    rows: usize,
    cols: usize,
    ptr: &[usize],
    idx: &[usize],
    nvalues: usize,
) -> R<()> {
    if rows.checked_add(1) != Some(ptr.len())
        || ptr.first() != Some(&0)
        || ptr.last() != Some(&idx.len())
        || nvalues != idx.len()
        || ptr.array_windows::<2>().any(|[start, end]| start > end)
        || idx.iter().any(|&col| col >= cols)
    {
        return Err(invalid_layout());
    }
    Ok(())
}

#[allow(
    clippy::single_call_fn,
    reason = "converting entries to owned CSR is independent of borrowing existing storage"
)]
fn csr_of<T: Clone>(
    e: Entries<T>,
    rows: usize,
    cols: usize,
) -> R<Csr<'static, T>> {
    if e.rank != 2 || !matches_rank(&e) {
        return Err(invalid_layout());
    }
    let mut ptr = filled(rows.checked_add(1).ok_or_else(shape_overflow)?, 0usize)?;
    let mut idx = reserved(e.len())?;
    for [row, col] in e.coords.as_chunks::<2>().0 {
        if *col >= cols {
            return Err(invalid_layout());
        }
        let count = row
            .checked_add(1)
            .and_then(|index| ptr.get_mut(index))
            .ok_or_else(invalid_layout)?;
        *count = count.saturating_add(1);
        idx.push(*col);
    }
    let mut cumulative = 0usize;
    for count in &mut ptr {
        cumulative = cumulative.saturating_add(*count);
        *count = cumulative;
    }
    Ok(Csr {
        rows,
        cols,
        ptr: Cow::Owned(ptr),
        idx: Cow::Owned(idx),
        vals: Cow::Owned(e.vals),
    })
}

pub(super) fn entries_of_csr<T: Clone>(csr: &Csr<'_, T>) -> R<Entries<T>> {
    let mut coords = reserved(csr.idx.len().checked_mul(2).ok_or_else(shape_overflow)?)?;
    for row in 0..csr.rows {
        for (column, _) in csr.row(row)? {
            coords.extend([row, column]);
        }
    }
    Ok(Entries {
        rank: 2,
        coords,
        vals: csr.vals.to_vec(),
    })
}

/// `A · B` (Gustavson's row-wise algorithm); the result is sorted and cleaned.
pub(super) fn spgemm<T: Clone, Rg: Ring<T>>(
    a: &Csr<'_, T>,
    b: &Csr<'_, T>,
    ring: &mut Rg,
) -> R<Csr<'static, T>> {
    if a.cols != b.rows {
        return Err(invalid_layout());
    }
    // Keep the direct indexed workspace when its size follows the stored
    // matrix. Wide logical axes use a map sized by the products they visit.
    let mut dense = (b.cols <= b.idx.len().saturating_mul(4).max(256))
        .then(|| filled(b.cols, None).ok())
        .flatten();
    let mut touched = Vec::new();
    let mut sparse = HashMap::new();
    let mut row = Vec::new();
    let mut ptr = reserved(a.rows.checked_add(1).ok_or_else(shape_overflow)?)?;
    ptr.push(0);
    let mut idx = Vec::new();
    let mut vals = Vec::new();
    for i in 0..a.rows {
        for (j, left) in a.row(i)? {
            for (column, right) in b.row(j)? {
                let product = ring.mul(left, right)?;
                if let Some(dense) = dense.as_mut() {
                    let slot = dense.get_mut(column).ok_or_else(invalid_layout)?;
                    if slot.is_none() {
                        touched.push(column);
                    }
                    accumulate(slot, product, ring)?;
                } else {
                    match sparse.entry(column) {
                        | std::collections::hash_map::Entry::Vacant(slot) => {
                            slot.insert(product);
                        },
                        | std::collections::hash_map::Entry::Occupied(mut slot) => {
                            *slot.get_mut() = ring.add(slot.get(), &product)?;
                        },
                    }
                }
            }
        }
        let mut emit = |column, value| {
            if !ring.is_zero(&value) {
                idx.push(column);
                vals.push(value);
            }
        };
        if let Some(dense) = dense.as_mut() {
            touched.sort_unstable();
            for &column in &touched {
                if let Some(value) = dense.get_mut(column).and_then(Option::take) {
                    emit(column, value);
                }
            }
            touched.clear();
        } else {
            row.extend(sparse.drain());
            row.sort_unstable_by_key(|(column, _)| std::cmp::Reverse(*column));
            while let Some((column, value)) = row.pop() {
                emit(column, value);
            }
        }
        ptr.push(idx.len());
    }
    Ok(Csr {
        rows: a.rows,
        cols: b.cols,
        ptr: Cow::Owned(ptr),
        idx: Cow::Owned(idx),
        vals: Cow::Owned(vals),
    })
}

fn accumulate<T: Clone, Rg: Ring<T>>(
    slot: &mut Option<T>,
    value: T,
    ring: &mut Rg,
) -> R<()> {
    match slot {
        | None => *slot = Some(value),
        | Some(sum) => *sum = ring.add(sum, &value)?,
    }
    Ok(())
}

fn dense_row<T>(
    data: &[T],
    row: usize,
    width: usize,
) -> Option<&[T]> {
    let start = row.checked_mul(width)?;
    data.get(start..start.checked_add(width)?)
}

/// `A · X` for a dense `X` of `n` columns (`n = 1` is the matrix-vector
/// product); the result is `rows × n`, row-major, `None` where nothing was
/// accumulated.
#[allow(
    clippy::single_call_fn,
    reason = "the generic sparse-dense kernel works with every scalar ring"
)]
pub(super) fn spmm<T: Clone, Rg: Ring<T>>(
    a: &Csr<'_, T>,
    x: &[T],
    n: usize,
    ring: &mut Rg,
) -> R<Vec<Option<T>>> {
    if a.cols.checked_mul(n) != Some(x.len()) {
        return Err(invalid_layout());
    }
    let mut out = filled(a.rows.checked_mul(n).ok_or_else(shape_overflow)?, None)?;
    if out.is_empty() {
        return Ok(out);
    }
    for (i, output) in out.chunks_exact_mut(n).enumerate() {
        for (j, left) in a.row(i)? {
            let right = dense_row(x, j, n).ok_or_else(invalid_layout)?;
            for (slot, item) in output.iter_mut().zip(right) {
                let product = ring.mul(left, item)?;
                accumulate(slot, product, ring)?;
            }
        }
    }
    Ok(out)
}

/// Products below this many multiply-adds run on the calling thread.
const PARALLEL_WORK: usize = 1 << 17;

/// Rows per parallel work item. A constant, so the partition never depends on
/// the number of threads (and, because every output row is accumulated by one
/// item in column order, neither does any bit of the result).
const ROW_BLOCK: usize = 256;

/// `A · X` for an `f64` CSR `A` and a dense `X` of `n` columns, row-parallel
/// for large products.
///
/// Bit-identical to [`spmm`] with [`FloatRing`] (the first product of an
/// entry is stored, later ones are added in column order) and to itself under
/// any thread count: each output row is computed by exactly one work item.
#[allow(
    clippy::single_call_fn,
    reason = "the float-specific kernel owns deterministic work partitioning"
)]
pub(super) fn spmm_f64(
    a: &Csr<'_, f64>,
    x: &[f64],
    n: usize,
) -> R<Vec<Option<f64>>> {
    use rayon::prelude::*;
    if a.cols.checked_mul(n) != Some(x.len()) {
        return Err(invalid_layout());
    }
    let mut out = filled(a.rows.checked_mul(n).ok_or_else(shape_overflow)?, None)?;
    if out.is_empty() {
        return Ok(out);
    }
    let block_rows = a.rows.min(ROW_BLOCK);
    let block_size = block_rows.checked_mul(n).ok_or_else(shape_overflow)?;
    let block = |(index, chunk): (usize, &mut [Option<f64>])| -> Option<()> {
        let first = index.checked_mul(block_rows)?;
        for (i, row_out) in (first..).zip(chunk.chunks_exact_mut(n)) {
            for (j, left) in a.row(i).ok()? {
                let right = dense_row(x, j, n)?;
                for (slot, item) in row_out.iter_mut().zip(right) {
                    let product = left * item;
                    *slot = Some(match *slot {
                        | None => product,
                        | Some(sum) => sum + product,
                    });
                }
            }
        }
        Some(())
    };
    if a.vals.len().saturating_mul(n) >= PARALLEL_WORK {
        out.par_chunks_mut(block_size)
            .enumerate()
            .try_for_each(block)
            .ok_or_else(invalid_layout)?;
    } else {
        out.chunks_mut(block_size)
            .enumerate()
            .try_for_each(block)
            .ok_or_else(invalid_layout)?;
    }
    Ok(out)
}

/// `A · X` for a block-CSR `A` (`ptr`/`idx` over blocks, `vals` row-major
/// `br x bc` slots per block) and a dense `X` of `n` columns.
///
/// The padding zeros inside a block are skipped, and every output entry
/// accumulates its products in column order, so the result is bit-identical
/// to the CSR kernel [`spmm`] with the `f64` ring.
#[allow(
    clippy::single_call_fn,
    reason = "the block kernel owns its block addressing and work partitioning"
)]
pub(super) fn bsr_spmm_f64(
    ptr: &[usize],
    idx: &[usize],
    vals: &[f64],
    block: (usize, usize),
    rows: usize,
    x: &[f64],
    n: usize,
) -> R<Vec<f64>> {
    use rayon::prelude::*;
    let (br, bc) = block;
    if br == 0 || bc == 0 || !rows.is_multiple_of(br) {
        return Err(invalid_layout());
    }
    let bs = br.checked_mul(bc).ok_or_else(shape_overflow)?;
    if idx.len().checked_mul(bs) != Some(vals.len()) {
        return Err(invalid_layout());
    }
    let block_rows = rows.checked_div(br).ok_or_else(invalid_layout)?;
    if n == 0 {
        compressed_layout(block_rows, usize::MAX, ptr, idx, idx.len())?;
        return if x.is_empty() {
            Ok(Vec::new())
        } else {
            Err(invalid_layout())
        };
    }
    if !x.len().is_multiple_of(n) {
        return Err(invalid_layout());
    }
    let x_rows = x.len().checked_div(n).ok_or_else(invalid_layout)?;
    if !x_rows.is_multiple_of(bc) {
        return Err(invalid_layout());
    }
    let block_cols = x_rows.checked_div(bc).ok_or_else(invalid_layout)?;
    compressed_layout(block_rows, block_cols, ptr, idx, idx.len())?;

    let mut out = filled(rows.checked_mul(n).ok_or_else(shape_overflow)?, 0.0)?;
    if out.is_empty() {
        return Ok(out);
    }
    // One work item per block row. It owns all `br` output rows.
    let block_size = br.checked_mul(n).ok_or_else(shape_overflow)?;
    let block_row = |(ib, chunk): (usize, &mut [f64])| -> Option<()> {
        let [start, end] = ptr
            .get(ib..)
            .and_then(<[usize]>::first_chunk::<2>)
            .copied()?;
        let (start_value, end_value) = (start.checked_mul(bs)?, end.checked_mul(bs)?);
        let columns = idx.get(start..end)?;
        let blocks = vals.get(start_value..end_value)?;
        for (r, row) in chunk.chunks_exact_mut(n).enumerate() {
            let mut first_product = true;
            for (&column, block_vals) in columns.iter().zip(blocks.chunks_exact(bs)) {
                let stored_row = dense_row(block_vals, r, bc)?;
                let first = column.checked_mul(bc)?;
                for (c, &value) in (first..).zip(stored_row) {
                    if value != 0.0 {
                        let xr = dense_row(x, c, n)?;
                        if first_product {
                            // Like CSR, store the first product directly so
                            // a negative-zero product keeps its sign.
                            for (out, item) in row.iter_mut().zip(xr) {
                                *out = value * item;
                            }
                            first_product = false;
                        } else {
                            for (out, item) in row.iter_mut().zip(xr) {
                                *out += value * item;
                            }
                        }
                    }
                }
            }
        }
        Some(())
    };
    if vals.len().saturating_mul(n) >= PARALLEL_WORK {
        out.par_chunks_mut(block_size)
            .enumerate()
            .try_for_each(block_row)
            .ok_or_else(invalid_layout)?;
    } else {
        out.chunks_mut(block_size)
            .enumerate()
            .try_for_each(block_row)
            .ok_or_else(invalid_layout)?;
    }
    Ok(out)
}

/// `X · B` for a dense `X` of `p` rows (`p = 1` is vector-matrix).
pub(super) fn dense_spmm<T: Clone, Rg: Ring<T>>(
    data: &[T],
    rows: usize,
    right: &Csr<'_, T>,
    ring: &mut Rg,
) -> R<Vec<Option<T>>> {
    let inner = right.rows;
    let columns = right.cols;
    if rows.checked_mul(inner) != Some(data.len()) {
        return Err(invalid_layout());
    }
    let mut out = filled(rows.checked_mul(columns).ok_or_else(shape_overflow)?, None)?;
    if out.is_empty() || inner == 0 {
        return Ok(out);
    }
    for (source, output) in data.chunks_exact(inner).zip(out.chunks_exact_mut(columns)) {
        for (inner_index, source_value) in source.iter().enumerate() {
            if ring.is_zero(source_value) {
                continue;
            }
            for (column, sparse_value) in right.row(inner_index)? {
                let product = ring.mul(source_value, sparse_value)?;
                let slot = output.get_mut(column).ok_or_else(invalid_layout)?;
                accumulate(slot, product, ring)?;
            }
        }
    }
    Ok(out)
}

/// `A ⊗ B` as a CSR matrix.
pub(super) fn kron<T: Clone, Rg: Ring<T>>(
    a: &Csr<'_, T>,
    b: &Csr<'_, T>,
    ring: &mut Rg,
) -> R<Csr<'static, T>> {
    let rows = a.rows.checked_mul(b.rows).ok_or_else(shape_overflow)?;
    let cols = a.cols.checked_mul(b.cols).ok_or_else(shape_overflow)?;
    let mut ptr = reserved(rows.checked_add(1).ok_or_else(shape_overflow)?)?;
    ptr.push(0);
    let mut idx = Vec::new();
    let mut vals = Vec::new();
    if rows != 0 {
        for i in 0..a.rows {
            for p in 0..b.rows {
                for (a_column, left) in a.row(i)? {
                    let first = a_column.checked_mul(b.cols).ok_or_else(shape_overflow)?;
                    for (b_column, right) in b.row(p)? {
                        let value = ring.mul(left, right)?;
                        if !ring.is_zero(&value) {
                            let column = first.checked_add(b_column).ok_or_else(shape_overflow)?;
                            idx.push(column);
                            vals.push(value);
                        }
                    }
                }
                ptr.push(idx.len());
            }
        }
    }
    Ok(Csr {
        rows,
        cols,
        ptr: Cow::Owned(ptr),
        idx: Cow::Owned(idx),
        vals: Cow::Owned(vals),
    })
}

/// The sum of every stored entry (`None` for an empty list).
pub(super) fn sum_all<T: Clone, Rg: Ring<T>>(
    a: &Entries<T>,
    ring: &mut Rg,
) -> R<Option<T>> {
    let mut total: Option<T> = None;
    for value in &a.vals {
        accumulate(&mut total, value.clone(), ring)?;
    }
    Ok(total)
}

/// The sum over `axis`, keeping the axis with extent 1, as a dense row-major
/// buffer of `shape` with `shape[axis] = 1`.
pub(super) fn sum_axis<T: Clone, Rg: Ring<T>>(
    a: &Entries<T>,
    shape: &[usize],
    axis: usize,
    ring: &mut Rg,
) -> R<Vec<Option<T>>> {
    let mut reduced = shape.to_vec();
    *reduced.get_mut(axis).ok_or_else(invalid_layout)? = 1;
    let count = crate::value::checked_element_count(&reduced).ok_or_else(shape_overflow)?;
    let mut out = filled(count, None)?;
    for (coord, value) in stored_entries(a)? {
        let mut flat = 0usize;
        if coord.len() != shape.len() {
            return Err(invalid_layout());
        }
        for (i, (&coordinate, &extent)) in coord.iter().zip(shape).enumerate() {
            if coordinate >= extent {
                return Err(invalid_layout());
            }
            if i != axis {
                flat = flat
                    .checked_mul(extent)
                    .and_then(|base| base.checked_add(coordinate))
                    .ok_or_else(shape_overflow)?;
            }
        }
        let slot = out.get_mut(flat).ok_or_else(invalid_layout)?;
        accumulate(slot, value.clone(), ring)?;
    }
    Ok(out)
}

fn select_axes(
    coord: &[usize],
    axes: &[usize],
) -> R<Vec<usize>> {
    axes.iter()
        .map(|&axis| coord.get(axis).copied().ok_or_else(invalid_layout))
        .collect()
}

/// The pairwise contraction of two sparse tensors (a sparse `tensordot`).
///
/// The result's axes are the free axes of `a`, in order, then the free axes
/// of `b`, in order. Entries are accumulated in a sorted map, so the result
/// is sorted and duplicate-free; zeros are dropped.
pub(super) fn contract<T: Clone, Rg: Ring<T>>(
    a: &Entries<T>,
    b: &Entries<T>,
    axes_a: &[usize],
    axes_b: &[usize],
    ring: &mut Rg,
) -> R<Entries<T>> {
    if axes_a.len() != axes_b.len()
        || axes_a.iter().any(|&axis| axis >= a.rank)
        || axes_b.iter().any(|&axis| axis >= b.rank)
    {
        return Err(invalid_layout());
    }
    let free_a: Vec<usize> = (0..a.rank).filter(|x| !axes_a.contains(x)).collect();
    let free_b: Vec<usize> = (0..b.rank).filter(|x| !axes_b.contains(x)).collect();
    let mut groups: HashMap<Vec<usize>, Vec<(Vec<usize>, &T)>> = HashMap::new();
    for (coord, value) in stored_entries(b)? {
        let key = select_axes(coord, axes_b)?;
        let free = select_axes(coord, &free_b)?;
        groups.entry(key).or_default().push((free, value));
    }
    let mut result: BTreeMap<Vec<usize>, T> = BTreeMap::new();
    for (coord, left) in stored_entries(a)? {
        let key = select_axes(coord, axes_a)?;
        let Some(matches) = groups.get(&key) else {
            continue;
        };
        let free = select_axes(coord, &free_a)?;
        for (free_other, value) in matches {
            let product = ring.mul(left, value)?;
            let mut target = free.clone();
            target.extend_from_slice(free_other);
            match result.get_mut(&target) {
                | Some(sum) => *sum = ring.add(sum, &product)?,
                | None => {
                    result.insert(target, product);
                },
            }
        }
    }
    let mut out = Entries::empty(
        free_a
            .len()
            .checked_add(free_b.len())
            .ok_or_else(shape_overflow)?,
    );
    for (coord, value) in result {
        out.push(&coord, value);
    }
    clean(out, ring)
}

/// Direct solution of `A x = b` by Gaussian elimination on sparse rows, for
/// element classes `aevia-std`'s `f64` LU cannot serve (exact rationals,
/// `Complex`, mixed phases).
///
/// Pivoting picks, among the rows with a nonzero in the current column, the
/// one with the fewest stored entries (Markowitz-style, to limit fill).
/// Returns `Ok(None)` when the matrix is singular.
#[allow(
    clippy::single_call_fn,
    reason = "pivoting and substitution form a scalar-generic solver"
)]
pub(super) fn solve_elimination<T: Clone, Rg: Ring<T>>(
    a: &Csr<'_, T>,
    b: &[T],
    ring: &mut Rg,
) -> R<Option<Vec<T>>> {
    let n = a.rows;
    if a.cols != n || b.len() != n {
        return Err(invalid_layout());
    }
    let mut rows = reserved(n)?;
    for r in 0..n {
        let row: BTreeMap<_, _> = a
            .row(r)?
            .map(|(column, value)| (column, value.clone()))
            .collect();
        rows.push(row);
    }
    let mut rhs = b.to_vec();
    for col in 0..n {
        let pivot = rows
            .iter()
            .enumerate()
            .skip(col)
            .filter(|(_, row)| row.get(&col).is_some_and(|v| !ring.is_zero(v)))
            .min_by_key(|(_, row)| row.len())
            .map(|(position, _)| position);
        let Some(pivot) = pivot else {
            return Ok(None);
        };
        rows.swap(col, pivot);
        rhs.swap(col, pivot);
        let (row, trailing) = rows
            .get_mut(col..)
            .and_then(<[_]>::split_first_mut)
            .ok_or_else(invalid_layout)?;
        let (pivot_rhs, trailing_rhs) = rhs
            .get_mut(col..)
            .and_then(<[_]>::split_first_mut)
            .ok_or_else(invalid_layout)?;
        let pivot_value = row.get(&col).ok_or_else(invalid_layout)?;
        for (other, other_rhs) in trailing.iter_mut().zip(trailing_rhs) {
            let Some(entry) = other.remove(&col) else {
                continue;
            };
            if ring.is_zero(&entry) {
                continue;
            }
            let factor = ring.div(&entry, pivot_value)?;
            for (c, v) in row.iter().filter(|(c, _)| **c > col) {
                let scaled = ring.mul(&factor, v)?;
                let updated = match other.get(c) {
                    | Some(existing) => ring.sub(existing, &scaled)?,
                    | None => {
                        let zero = ring.zero_like(&scaled);
                        ring.sub(&zero, &scaled)?
                    },
                };
                if ring.is_zero(&updated) {
                    other.remove(c);
                } else {
                    other.insert(*c, updated);
                }
            }
            let scaled = ring.mul(&factor, pivot_rhs)?;
            *other_rhs = ring.sub(other_rhs, &scaled)?;
        }
    }
    let mut x = filled(n, None)?;
    for (r, (row, rhs)) in rows.iter().zip(&rhs).enumerate().rev() {
        let mut value = rhs.clone();
        for (c, v) in row.iter().filter(|(c, _)| **c > r) {
            let Some(xc) = x.get(*c).ok_or_else(invalid_layout)? else {
                continue;
            };
            let product = ring.mul(v, xc)?;
            value = ring.sub(&value, &product)?;
        }
        let pivot = row.get(&r).ok_or_else(invalid_layout)?;
        *x.get_mut(r).ok_or_else(invalid_layout)? = Some(ring.div(&value, pivot)?);
    }
    Ok(Some(x.into_iter().flatten().collect()))
}

/// A zero error for kernels that need a stored value class they do not have.
pub(super) fn class_error(span: Span) -> EvalError {
    err(
        "E0809",
        "internal error: the sparse storage class does not match the kernel",
        span,
    )
}

/// Borrow the typed CSR arrays of a `Csr`-format tensor, or build them.
pub(super) fn csr_parts<T: Store>(
    data: &super::SparseData,
    span: Span,
) -> R<Csr<'_, T>> {
    use super::Pattern;
    use super::SparseFormat;
    let [rows, cols] = data.shape.as_slice() else {
        return Err(err(
            "E0802",
            format!(
                "expected a rank-2 sparse matrix, found rank {}",
                data.rank()
            ),
            span,
        ));
    };
    let [rows, cols] = [*rows, *cols];
    let stored = T::view(&data.values).ok_or_else(|| class_error(span))?;
    let malformed = || invalid_layout().at(span);
    match (data.format, &data.pattern) {
        | (SparseFormat::Csr, Pattern::Compressed { ptr, idx }) => {
            compressed_layout(rows, cols, ptr, idx, stored.len())
                .map_err(|error| error.at(span))?;
            return Ok(Csr {
                rows,
                cols,
                ptr: Cow::Borrowed(ptr),
                idx: Cow::Borrowed(idx),
                vals: Cow::Borrowed(stored),
            });
        },
        | (SparseFormat::Csc, Pattern::Compressed { ptr, idx }) => {
            compressed_layout(cols, rows, ptr, idx, stored.len())
                .map_err(|error| error.at(span))?;
        },
        | (SparseFormat::Coo, Pattern::Coords(coords)) => {
            if stored.len().checked_mul(2) != Some(coords.len())
                || coords
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .any(|[row, col]| *row >= rows || *col >= cols)
            {
                return Err(malformed());
            }
        },
        | (SparseFormat::Bcsr, Pattern::Blocked { ptr, idx }) => {
            let (br, bc) = data.block;
            if br == 0
                || bc == 0
                || !rows.is_multiple_of(br)
                || !cols.is_multiple_of(bc)
                || idx
                    .len()
                    .checked_mul(br)
                    .and_then(|count| count.checked_mul(bc))
                    != Some(stored.len())
            {
                return Err(malformed());
            }
            let block_rows = rows.checked_div(br).ok_or_else(malformed)?;
            let block_cols = cols.checked_div(bc).ok_or_else(malformed)?;
            compressed_layout(block_rows, block_cols, ptr, idx, idx.len())
                .map_err(|error| error.at(span))?;
        },
        | _ => return Err(malformed()),
    }
    let entries = data.entries::<T>().ok_or_else(|| class_error(span))?;
    csr_of(entries, rows, cols).map_err(|error| error.at(span))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_offsets_reject_wrong_rank_bounds_and_overflow() {
        assert_eq!(flat_index(&[1, 2], &[2, 3]), Some(5));
        assert_eq!(flat_index(&[1], &[2, 3]), None);
        assert_eq!(flat_index(&[0], &[0]), None);
        assert_eq!(flat_index(&[0, 3], &[2, 3]), None);
        assert_eq!(flat_index(&[usize::MAX - 1, 1], &[usize::MAX, 2]), None);
    }

    #[test]
    fn sparse_workspace_follows_stored_nonzeros_on_wide_axes() -> R<()> {
        let last = usize::MAX - 1;
        let left = csr_of(assemble(2, &[0, 0], vec![2.0], &mut FloatRing)?, 1, 1)?;
        let right_entries = assemble(2, &[0, last], vec![3.0], &mut FloatRing)?;
        let transposed = transpose(&right_entries, usize::MAX)?;
        assert_eq!(transposed.coords, [last, 0]);
        assert_eq!(transposed.vals, [3.0]);
        let right = csr_of(right_entries, 1, usize::MAX)?;
        let product = spgemm(&left, &right, &mut FloatRing)?;
        assert_eq!(product.cols, usize::MAX);
        let entries = entries_of_csr(&product)?;
        assert_eq!(entries.coords, [0, last]);
        assert_eq!(entries.vals, [6.0]);

        let right = csr_of(assemble(2, &[0, 1], vec![4.0], &mut FloatRing)?, 1, 2)?;
        let entries = entries_of_csr(&spgemm(&left, &right, &mut FloatRing)?)?;
        assert_eq!(entries.coords, [0, 1]);
        assert_eq!(entries.vals, [8.0]);
        Ok(())
    }

    #[test]
    fn sparse_products_report_malformed_layouts_before_dereferencing() -> R<()> {
        let matrix = csr_of(assemble(2, &[0, 0], vec![2.0], &mut FloatRing)?, 1, 1)?;
        let serial = spmm(&matrix, &[3.0, -0.0], 2, &mut FloatRing)?;
        let specialized = spmm_f64(&matrix, &[3.0, -0.0], 2)?;
        assert_eq!(serial, specialized);
        assert_eq!(
            specialized.last().copied().flatten().map(f64::to_bits),
            Some((-0.0f64).to_bits()),
        );
        let blocked = bsr_spmm_f64(&[0, 1], &[0], &[2.0], (1, 1), 1, &[3.0, -0.0], 2)?;
        assert!(
            blocked
                .into_iter()
                .zip(specialized)
                .all(|(bcsr, csr)| Some(bcsr.to_bits()) == csr.map(f64::to_bits))
        );
        assert_eq!(
            spmm_f64(&matrix, &[], 1).err().map(|error| error.code),
            Some("E0802")
        );
        assert_eq!(
            bsr_spmm_f64(&[0, 2], &[0], &[1.0], (1, 1), 1, &[1.0], 1)
                .err()
                .map(|error| error.code),
            Some("E0802"),
        );
        assert_eq!(
            bsr_spmm_f64(
                &[0, 1],
                &[0],
                &[2.0, 0.0, 0.0, 3.0],
                (2, 2),
                2,
                &[4.0, 5.0],
                1
            )?,
            [8.0, 15.0],
        );
        Ok(())
    }

    #[test]
    fn mutable_host_shape_and_block_metadata_are_validated() -> R<()> {
        let make = |format| -> R<super::super::SparseData> {
            let entries = assemble(2, &[0, 0], vec![1.0], &mut FloatRing)?;
            super::super::SparseData::from_entries(
                format,
                vec![1, 1],
                aevia_dim::Dim::one(),
                None,
                2,
                entries,
            )
            .map_err(|message| EvalError::new("E0802", message))
        };
        let mut csr = make(super::super::SparseFormat::Csr)?;
        csr.shape = vec![usize::MAX, 1];
        let mut blocked = make(super::super::SparseFormat::Bcsr)?;
        blocked.block = (0, 1);
        assert_eq!(
            csr_parts::<f64>(&csr, Span::default())
                .err()
                .map(|error| error.code),
            Some("E0802")
        );
        assert_eq!(
            csr_parts::<f64>(&blocked, Span::default())
                .err()
                .map(|error| error.code),
            Some("E0802")
        );
        Ok(())
    }
}
