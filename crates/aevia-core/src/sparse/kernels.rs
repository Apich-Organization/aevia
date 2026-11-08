//! The algebra of sparse tensors, generic over the element class.
//!
//! Every kernel here is written once against [`Ring`] and instantiated for
//! `f64` ([`FloatRing`]) and for boxed [`Value`]s ([`ValueRing`], which routes
//! each scalar operation through the language's own arithmetic so exact
//! values stay exact).

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
pub(crate) trait Ring<T: Clone> {
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

pub(crate) struct FloatRing;

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

pub(crate) struct ValueRing<'a> {
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

/// Drops every entry equal to zero.
pub(crate) fn clean<T: Clone, Rg: Ring<T>>(
    entries: Entries<T>,
    ring: &Rg,
) -> Entries<T> {
    if entries.vals.iter().all(|v| !ring.is_zero(v)) {
        return entries;
    }
    let mut out = Entries::empty(entries.rank);
    for (i, value) in entries.vals.iter().enumerate() {
        if !ring.is_zero(value) {
            out.push(entries.coord(i), value.clone());
        }
    }
    out
}

/// Sorts unordered `(coordinate, value)` triplets, sums duplicates and drops
/// zeros.
pub(crate) fn assemble<T: Clone, Rg: Ring<T>>(
    rank: usize,
    coords: &[usize],
    vals: Vec<T>,
    ring: &mut Rg,
) -> R<Entries<T>> {
    let n = vals.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| coords[a * rank..(a + 1) * rank].cmp(&coords[b * rank..(b + 1) * rank]));
    let mut out: Entries<T> = Entries::empty(rank);
    for &k in &order {
        let coord = &coords[k * rank..(k + 1) * rank];
        let duplicate = out.len() > 0 && out.coord(out.len() - 1) == coord;
        if duplicate {
            let last = out.vals.len() - 1;
            out.vals[last] = ring.add(&out.vals[last], &vals[k])?;
        } else {
            out.push(coord, vals[k].clone());
        }
    }
    Ok(clean(out, ring))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Merge {
    Add,
    Sub,
    Mul,
}

/// `a + b`, `a - b` or the Hadamard product, over the union (resp.
/// intersection) of the patterns.
pub(crate) fn merge<T: Clone, Rg: Ring<T>>(
    a: &Entries<T>,
    b: &Entries<T>,
    mode: Merge,
    ring: &mut Rg,
) -> R<Entries<T>> {
    let mut out = Entries::empty(a.rank);
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        let order = if i >= a.len() {
            Ordering::Greater
        } else if j >= b.len() {
            Ordering::Less
        } else {
            a.coord(i).cmp(b.coord(j))
        };
        match order {
            | Ordering::Equal => {
                let value = match mode {
                    | Merge::Add => ring.add(&a.vals[i], &b.vals[j])?,
                    | Merge::Sub => ring.sub(&a.vals[i], &b.vals[j])?,
                    | Merge::Mul => ring.mul(&a.vals[i], &b.vals[j])?,
                };
                out.push(a.coord(i), value);
                i += 1;
                j += 1;
            },
            | Ordering::Less => {
                if mode != Merge::Mul {
                    out.push(a.coord(i), a.vals[i].clone());
                }
                i += 1;
            },
            | Ordering::Greater => {
                if mode != Merge::Mul {
                    let value = if mode == Merge::Sub {
                        let zero = ring.zero_like(&b.vals[j]);
                        ring.sub(&zero, &b.vals[j])?
                    } else {
                        b.vals[j].clone()
                    };
                    out.push(b.coord(j), value);
                }
                j += 1;
            },
        }
    }
    Ok(clean(out, ring))
}

/// Applies `f` to every stored value; the result is cleaned.
pub(crate) fn map_values<T: Clone, Rg: Ring<T>>(
    a: &Entries<T>,
    ring: &mut Rg,
    mut f: impl FnMut(&mut Rg, &T) -> R<T>,
) -> R<Entries<T>> {
    let mut vals = Vec::with_capacity(a.len());
    for value in &a.vals {
        vals.push(f(ring, value)?);
    }
    Ok(clean(
        Entries {
            rank: a.rank,
            coords: a.coords.clone(),
            vals,
        },
        ring,
    ))
}

/// The flat row-major offset of `coord` in `shape`.
pub(crate) fn flat_index(
    coord: &[usize],
    shape: &[usize],
) -> usize {
    let mut flat = 0usize;
    for (c, e) in coord.iter().zip(shape) {
        flat = flat * e + c;
    }
    flat
}

/// The coordinates of a flat row-major offset.
pub(crate) fn unflatten(
    mut flat: usize,
    shape: &[usize],
    out: &mut [usize],
) {
    for axis in (0..shape.len()).rev() {
        let extent = shape[axis].max(1);
        out[axis] = flat % extent;
        flat /= extent;
    }
}

/// `sparse ∘ dense` over the stored entries only.
pub(crate) fn hadamard_dense<T: Clone, Rg: Ring<T>>(
    a: &Entries<T>,
    shape: &[usize],
    dense: &[T],
    ring: &mut Rg,
) -> R<Entries<T>> {
    let mut out = Entries::empty(a.rank);
    for i in 0..a.len() {
        let value = ring.mul(&a.vals[i], &dense[flat_index(a.coord(i), shape)])?;
        out.push(a.coord(i), value);
    }
    Ok(clean(out, ring))
}

/// The transpose of a rank-2 entry list (a stable counting sort by column).
pub(crate) fn transpose<T: Clone>(
    a: &Entries<T>,
    cols: usize,
) -> Entries<T> {
    let nnz = a.len();
    let mut start = vec![0usize; cols + 1];
    for i in 0..nnz {
        start[a.coord(i)[1] + 1] += 1;
    }
    for c in 0..cols {
        start[c + 1] += start[c];
    }
    let mut coords = vec![0usize; nnz * 2];
    let mut slots: Vec<Option<T>> = vec![None; nnz];
    for i in 0..nnz {
        let c = a.coord(i)[1];
        let pos = start[c];
        start[c] += 1;
        coords[pos * 2] = c;
        coords[pos * 2 + 1] = a.coord(i)[0];
        slots[pos] = Some(a.vals[i].clone());
    }
    Entries {
        rank: 2,
        coords,
        vals: slots.into_iter().flatten().collect(),
    }
}

/// CSR arrays of a rank-2 entry list.
pub(crate) struct Csr<'a, T: Clone> {
    pub rows: usize,
    pub cols: usize,
    pub ptr: Cow<'a, [usize]>,
    pub idx: Cow<'a, [usize]>,
    pub vals: Cow<'a, [T]>,
}

pub(crate) fn csr_of<T: Clone>(
    e: Entries<T>,
    rows: usize,
    cols: usize,
) -> Csr<'static, T> {
    let mut ptr = vec![0usize; rows + 1];
    let mut idx = Vec::with_capacity(e.len());
    for i in 0..e.len() {
        ptr[e.coord(i)[0] + 1] += 1;
        idx.push(e.coord(i)[1]);
    }
    for r in 0..rows {
        ptr[r + 1] += ptr[r];
    }
    Csr {
        rows,
        cols,
        ptr: Cow::Owned(ptr),
        idx: Cow::Owned(idx),
        vals: Cow::Owned(e.vals),
    }
}

pub(crate) fn entries_of_csr<T: Clone>(csr: &Csr<'_, T>) -> Entries<T> {
    let mut coords = Vec::with_capacity(csr.idx.len() * 2);
    for r in 0..csr.rows {
        for k in csr.ptr[r]..csr.ptr[r + 1] {
            coords.push(r);
            coords.push(csr.idx[k]);
        }
    }
    Entries {
        rank: 2,
        coords,
        vals: csr.vals.to_vec(),
    }
}

/// `A · B` (Gustavson's row-wise algorithm); the result is sorted and cleaned.
pub(crate) fn spgemm<T: Clone, Rg: Ring<T>>(
    a: &Csr<'_, T>,
    b: &Csr<'_, T>,
    ring: &mut Rg,
) -> R<Csr<'static, T>> {
    let mut acc: Vec<Option<T>> = vec![None; b.cols];
    let mut touched: Vec<usize> = Vec::new();
    let mut ptr = Vec::with_capacity(a.rows + 1);
    ptr.push(0);
    let mut idx = Vec::new();
    let mut vals = Vec::new();
    for i in 0..a.rows {
        for ka in a.ptr[i]..a.ptr[i + 1] {
            let j = a.idx[ka];
            for kb in b.ptr[j]..b.ptr[j + 1] {
                let c = b.idx[kb];
                let product = ring.mul(&a.vals[ka], &b.vals[kb])?;
                match &mut acc[c] {
                    | slot @ None => {
                        *slot = Some(product);
                        touched.push(c);
                    },
                    | Some(sum) => *sum = ring.add(sum, &product)?,
                }
            }
        }
        touched.sort_unstable();
        for c in touched.drain(..) {
            if let Some(value) = acc[c].take() {
                if !ring.is_zero(&value) {
                    idx.push(c);
                    vals.push(value);
                }
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

/// `A · X` for a dense `X` of `n` columns (`n = 1` is the matrix-vector
/// product); the result is `rows × n`, row-major, `None` where nothing was
/// accumulated.
pub(crate) fn spmm<T: Clone, Rg: Ring<T>>(
    a: &Csr<'_, T>,
    x: &[T],
    n: usize,
    ring: &mut Rg,
) -> R<Vec<Option<T>>> {
    let mut out: Vec<Option<T>> = vec![None; a.rows * n];
    for i in 0..a.rows {
        for k in a.ptr[i]..a.ptr[i + 1] {
            let j = a.idx[k];
            for c in 0..n {
                let product = ring.mul(&a.vals[k], &x[j * n + c])?;
                accumulate(&mut out[i * n + c], product, ring)?;
            }
        }
    }
    Ok(out)
}

/// `X · B` for a dense `X` of `p` rows (`p = 1` is vector-matrix).
pub(crate) fn dense_spmm<T: Clone, Rg: Ring<T>>(
    x: &[T],
    p: usize,
    b: &Csr<'_, T>,
    ring: &mut Rg,
) -> R<Vec<Option<T>>> {
    let k = b.rows;
    let n = b.cols;
    let mut out: Vec<Option<T>> = vec![None; p * n];
    for r in 0..p {
        for kk in 0..k {
            let xv = &x[r * k + kk];
            if ring.is_zero(xv) {
                continue;
            }
            for kb in b.ptr[kk]..b.ptr[kk + 1] {
                let product = ring.mul(xv, &b.vals[kb])?;
                accumulate(&mut out[r * n + b.idx[kb]], product, ring)?;
            }
        }
    }
    Ok(out)
}

/// `A ⊗ B` as a CSR matrix.
pub(crate) fn kron<T: Clone, Rg: Ring<T>>(
    a: &Csr<'_, T>,
    b: &Csr<'_, T>,
    ring: &mut Rg,
) -> R<Csr<'static, T>> {
    let rows = a.rows * b.rows;
    let mut ptr = Vec::with_capacity(rows + 1);
    ptr.push(0);
    let mut idx = Vec::new();
    let mut vals = Vec::new();
    for i in 0..a.rows {
        for p in 0..b.rows {
            for ka in a.ptr[i]..a.ptr[i + 1] {
                for kb in b.ptr[p]..b.ptr[p + 1] {
                    let value = ring.mul(&a.vals[ka], &b.vals[kb])?;
                    if !ring.is_zero(&value) {
                        idx.push(a.idx[ka] * b.cols + b.idx[kb]);
                        vals.push(value);
                    }
                }
            }
            ptr.push(idx.len());
        }
    }
    Ok(Csr {
        rows,
        cols: a.cols * b.cols,
        ptr: Cow::Owned(ptr),
        idx: Cow::Owned(idx),
        vals: Cow::Owned(vals),
    })
}

/// The sum of every stored entry (`None` for an empty list).
pub(crate) fn sum_all<T: Clone, Rg: Ring<T>>(
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
pub(crate) fn sum_axis<T: Clone, Rg: Ring<T>>(
    a: &Entries<T>,
    shape: &[usize],
    axis: usize,
    ring: &mut Rg,
) -> R<Vec<Option<T>>> {
    let mut reduced = shape.to_vec();
    reduced[axis] = 1;
    let count: usize = reduced.iter().product();
    let mut out: Vec<Option<T>> = vec![None; count];
    let mut coord = vec![0usize; shape.len()];
    for i in 0..a.len() {
        coord.copy_from_slice(a.coord(i));
        coord[axis] = 0;
        accumulate(
            &mut out[flat_index(&coord, &reduced)],
            a.vals[i].clone(),
            ring,
        )?;
    }
    Ok(out)
}

/// The pairwise contraction of two sparse tensors (a sparse `tensordot`).
///
/// The result's axes are the free axes of `a`, in order, then the free axes
/// of `b`, in order. Entries are accumulated in a sorted map, so the result
/// is sorted and duplicate-free; zeros are dropped.
pub(crate) fn contract<T: Clone, Rg: Ring<T>>(
    a: &Entries<T>,
    b: &Entries<T>,
    axes_a: &[usize],
    axes_b: &[usize],
    ring: &mut Rg,
) -> R<Entries<T>> {
    let free_a: Vec<usize> = (0..a.rank).filter(|x| !axes_a.contains(x)).collect();
    let free_b: Vec<usize> = (0..b.rank).filter(|x| !axes_b.contains(x)).collect();
    let mut groups: HashMap<Vec<usize>, Vec<(Vec<usize>, T)>> = HashMap::new();
    for j in 0..b.len() {
        let coord = b.coord(j);
        let key: Vec<usize> = axes_b.iter().map(|x| coord[*x]).collect();
        let free: Vec<usize> = free_b.iter().map(|x| coord[*x]).collect();
        groups
            .entry(key)
            .or_default()
            .push((free, b.vals[j].clone()));
    }
    let mut result: BTreeMap<Vec<usize>, T> = BTreeMap::new();
    for i in 0..a.len() {
        let coord = a.coord(i);
        let key: Vec<usize> = axes_a.iter().map(|x| coord[*x]).collect();
        let Some(matches) = groups.get(&key) else {
            continue;
        };
        let free: Vec<usize> = free_a.iter().map(|x| coord[*x]).collect();
        for (free_other, value) in matches {
            let product = ring.mul(&a.vals[i], value)?;
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
    let mut out = Entries::empty(free_a.len() + free_b.len());
    for (coord, value) in result {
        out.push(&coord, value);
    }
    Ok(clean(out, ring))
}

/// Direct solution of `A x = b` by Gaussian elimination on sparse rows, for
/// element classes `aevia-std`'s `f64` LU cannot serve (exact rationals,
/// `Complex`, mixed phases).
///
/// Pivoting picks, among the rows with a nonzero in the current column, the
/// one with the fewest stored entries (Markowitz-style, to limit fill).
/// Returns `Ok(None)` when the matrix is singular.
pub(crate) fn solve_elimination<T: Clone, Rg: Ring<T>>(
    a: &Csr<'_, T>,
    b: &[T],
    ring: &mut Rg,
) -> R<Option<Vec<T>>> {
    let n = a.rows;
    let mut rows: Vec<BTreeMap<usize, T>> = (0..n)
        .map(|r| {
            (a.ptr[r]..a.ptr[r + 1])
                .map(|k| (a.idx[k], a.vals[k].clone()))
                .collect()
        })
        .collect();
    let mut rhs: Vec<T> = b.to_vec();
    for col in 0..n {
        let mut pivot: Option<usize> = None;
        for r in col..n {
            if rows[r].get(&col).is_some_and(|v| !ring.is_zero(v))
                && pivot.is_none_or(|p| rows[r].len() < rows[p].len())
            {
                pivot = Some(r);
            }
        }
        let Some(p) = pivot else {
            return Ok(None);
        };
        rows.swap(col, p);
        rhs.swap(col, p);
        let pivot_row: Vec<(usize, T)> = rows[col]
            .iter()
            .filter(|(c, _)| **c > col)
            .map(|(c, v)| (*c, v.clone()))
            .collect();
        let pivot_value = rows[col][&col].clone();
        let pivot_rhs = rhs[col].clone();
        for r in col + 1..n {
            let Some(entry) = rows[r].get(&col).cloned() else {
                continue;
            };
            if ring.is_zero(&entry) {
                rows[r].remove(&col);
                continue;
            }
            let factor = ring.div(&entry, &pivot_value)?;
            rows[r].remove(&col);
            for (c, v) in &pivot_row {
                let scaled = ring.mul(&factor, v)?;
                let updated = match rows[r].get(c) {
                    | Some(existing) => ring.sub(existing, &scaled)?,
                    | None => {
                        let zero = ring.zero_like(&scaled);
                        ring.sub(&zero, &scaled)?
                    },
                };
                if ring.is_zero(&updated) {
                    rows[r].remove(c);
                } else {
                    rows[r].insert(*c, updated);
                }
            }
            let scaled = ring.mul(&factor, &pivot_rhs)?;
            rhs[r] = ring.sub(&rhs[r], &scaled)?;
        }
    }
    let mut x: Vec<Option<T>> = vec![None; n];
    for r in (0..n).rev() {
        let mut value = rhs[r].clone();
        for (c, v) in rows[r].iter().filter(|(c, _)| **c > r) {
            let Some(xc) = &x[*c] else {
                continue;
            };
            let product = ring.mul(v, xc)?;
            value = ring.sub(&value, &product)?;
        }
        x[r] = Some(ring.div(&value, &rows[r][&r])?);
    }
    Ok(Some(x.into_iter().flatten().collect()))
}

/// A zero error for kernels that need a stored value class they do not have.
pub(crate) fn class_error(span: Span) -> EvalError {
    err(
        "E0809",
        "internal error: the sparse storage class does not match the kernel",
        span,
    )
}

/// Borrow the typed CSR arrays of a `Csr`-format tensor, or build them.
pub(crate) fn csr_parts<'a, T: Store>(
    data: &'a super::SparseData,
    span: Span,
) -> R<Csr<'a, T>> {
    if data.rank() != 2 {
        return Err(err(
            "E0802",
            format!(
                "expected a rank-2 sparse matrix, found rank {}",
                data.rank()
            ),
            span,
        ));
    }
    if let (super::SparseFormat::Csr, super::Pattern::Compressed { ptr, idx }) =
        (data.format, &data.pattern)
    {
        let vals = T::view(&data.values).ok_or_else(|| class_error(span))?;
        return Ok(Csr {
            rows: data.shape[0],
            cols: data.shape[1],
            ptr: Cow::Borrowed(ptr),
            idx: Cow::Borrowed(idx),
            vals: Cow::Borrowed(vals),
        });
    }
    let entries = data.entries::<T>().ok_or_else(|| class_error(span))?;
    Ok(csr_of(entries, data.shape[0], data.shape[1]))
}
