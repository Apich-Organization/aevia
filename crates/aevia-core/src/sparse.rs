//! Sparse tensors (§8.4.1, §13.9.5): the runtime value behind
//! `Sparse<F, shape, τ>`.
//!
//! # Representation
//!
//! A [`SparseData`] is a shape, an element dimension, a storage format and the
//! stored entries:
//!
//! * `Csr` / `Csc` are rank 2 and compressed (`ptr`, `idx`);
//! * `Coo` is rank N and holds the flat, lexicographically sorted coordinates.
//!
//! Every format stores each logical entry at most once, in sorted order, and
//! never stores an element equal to the implicit zero ([`SparseData::validate`]
//! checks this; it runs under `debug_assert!` after every construction).
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

mod api;
mod kernels;

pub(crate) use api::binary;
pub(crate) use api::call;
pub(crate) use api::call_typed;
pub(crate) use api::equal_data;
pub(crate) use api::index;
pub(crate) use api::negate;
pub(crate) use api::render;

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
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::ToPrimitive;
use num_traits::Zero;
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
}

impl SparseFormat {
    /// The spelling used in types and by `format_of`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            | Self::Csr => "Csr",
            | Self::Csc => "Csc",
            | Self::Coo => "Coo",
        }
    }

    /// Reads a format name.
    ///
    /// `Block` and `Bcsr` are formats of the type language that this runtime
    /// cannot build: they are `E1001`, not an unknown name.
    ///
    /// # Errors
    /// `E1001` for `Block`/`Bcsr`, `E0310` for anything else.
    pub fn parse(
        name: &str,
        span: Span,
    ) -> R<Self> {
        match name {
            | "Csr" | "CSR" => Ok(Self::Csr),
            | "Csc" | "CSC" => Ok(Self::Csc),
            | "Coo" | "COO" => Ok(Self::Coo),
            | "Block" | "Bcsr" | "BCSR" => {
                Err(EvalError::new(
                    "E1001",
                    format!(
                        "the sparse format `{name}` is accepted in types but this runtime cannot \
                         store it (capability `storage.sparse.block`)"
                    ),
                )
                .at(span))
            },
            | other => {
                Err(EvalError::new(
                    "E0310",
                    format!("`{other}` is not a sparse format (expected Csr, Csc or Coo)"),
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
    pub fn empty(rank: usize) -> Self {
        Self {
            rank,
            coords: Vec::new(),
            vals: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.vals.len()
    }

    pub fn coord(
        &self,
        index: usize,
    ) -> &[usize] {
        &self.coords[index * self.rank..(index + 1) * self.rank]
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

/// An element class: `f64` or a boxed [`Value`].
pub(crate) trait Store: Clone + 'static {
    fn wrap(values: Vec<Self>) -> SpValues;
    fn view(values: &SpValues) -> Option<&[Self]>;
}

impl Store for f64 {
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

pub(crate) fn kind_rank(kind: &NumKind) -> u8 {
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
        | 0 => NumKind::Int(BigInt::zero()),
        | 1 => NumKind::Exact(BigRational::zero()),
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
    Value::int(i64::try_from(n).unwrap_or(i64::MAX))
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
    let mut count: usize = 1;
    for extent in shape {
        count = count
            .checked_mul(*extent)
            .filter(|c| *c <= DENSE_LIMIT)
            .ok_or_else(|| {
                err(
                    "E0802",
                    format!("a dense tensor of shape {shape:?} is too large to build"),
                    span,
                )
            })?;
    }
    Ok(count)
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
                | NumKind::Int(n) => n.to_i64(),
                | NumKind::Float(f) if f.fract() == 0.0 => Some(*f as i64),
                | NumKind::Exact(q) if q.is_integer() => q.to_integer().to_i64(),
                | _ => None,
            };
            exact.and_then(|n| usize::try_from(n).ok()).ok_or_else(|| {
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
    tensor
        .elements()
        .iter()
        .map(|item| index_of(item, what, span))
        .collect()
}

// ---------------------------------------------------------------------------
// Construction and conversion
// ---------------------------------------------------------------------------

impl SparseData {
    /// The number of stored entries.
    #[must_use]
    pub fn nnz(&self) -> usize {
        match &self.values {
            | SpValues::F64(items) => items.len(),
            | SpValues::Boxed(items) => items.len(),
        }
    }

    /// The rank.
    #[must_use]
    pub fn rank(&self) -> usize {
        self.shape.len()
    }

    /// True when the values are an `f64` buffer.
    #[must_use]
    pub fn is_f64(&self) -> bool {
        matches!(self.values, SpValues::F64(_))
    }

    /// The stored entries as `(coordinates, value)` in sorted order.
    #[must_use]
    pub fn stored(&self) -> Vec<(Vec<usize>, Value)> {
        let entries = self.entries_with(self.boxed_values());
        (0..entries.len())
            .map(|i| (entries.coord(i).to_vec(), entries.vals[i].clone()))
            .collect()
    }

    /// For `Csc`: `order[k]` is the storage position of the k-th entry in
    /// row-major sorted order.
    fn csc_order(
        &self,
        ptr: &[usize],
        idx: &[usize],
    ) -> Vec<usize> {
        let rows = self.shape[0];
        let mut counts = vec![0usize; rows + 1];
        for r in idx {
            counts[*r + 1] += 1;
        }
        for r in 0..rows {
            counts[r + 1] += counts[r];
        }
        let mut order = vec![0usize; idx.len()];
        let mut next = counts;
        for c in 0..self.shape[1] {
            for k in ptr[c]..ptr[c + 1] {
                order[next[idx[k]]] = k;
                next[idx[k]] += 1;
            }
        }
        order
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
        let rank = self.rank();
        match &self.pattern {
            | Pattern::Coords(coords) => {
                Entries {
                    rank,
                    coords: coords.clone(),
                    vals: values,
                }
            },
            | Pattern::Compressed { ptr, idx } => {
                let mut coords = Vec::with_capacity(idx.len() * 2);
                match self.format {
                    | SparseFormat::Csc => {
                        let order = self.csc_order(ptr, idx);
                        let mut col_of = vec![0usize; idx.len()];
                        for c in 0..self.shape[1] {
                            for k in ptr[c]..ptr[c + 1] {
                                col_of[k] = c;
                            }
                        }
                        let mut vals = Vec::with_capacity(idx.len());
                        for &k in &order {
                            coords.push(idx[k]);
                            coords.push(col_of[k]);
                            vals.push(values[k].clone());
                        }
                        Entries { rank, coords, vals }
                    },
                    | _ => {
                        for r in 0..self.shape[0] {
                            for k in ptr[r]..ptr[r + 1] {
                                coords.push(r);
                                coords.push(idx[k]);
                            }
                        }
                        Entries {
                            rank,
                            coords,
                            vals: values,
                        }
                    },
                }
            },
        }
    }

    /// The typed entries, when the storage class is `T`'s.
    pub(crate) fn entries<T: Store>(&self) -> Option<Entries<T>> {
        let values = T::view(&self.values)?.to_vec();
        Some(self.entries_with(values))
    }

    /// Builds a sparse tensor of `format` from sorted, duplicate-free,
    /// zero-free entries.
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
    ) -> Self {
        let (pattern, values) = match format {
            | SparseFormat::Coo => (Pattern::Coords(entries.coords), entries.vals),
            | SparseFormat::Csr => {
                let rows = shape[0];
                let nnz = entries.vals.len();
                let mut ptr = vec![0usize; rows + 1];
                let mut idx = Vec::with_capacity(nnz);
                for i in 0..nnz {
                    ptr[entries.coords[i * 2] + 1] += 1;
                    idx.push(entries.coords[i * 2 + 1]);
                }
                for r in 0..rows {
                    ptr[r + 1] += ptr[r];
                }
                (Pattern::Compressed { ptr, idx }, entries.vals)
            },
            | SparseFormat::Csc => {
                let cols = shape[1];
                let nnz = entries.vals.len();
                let mut ptr = vec![0usize; cols + 1];
                for i in 0..nnz {
                    ptr[entries.coords[i * 2 + 1] + 1] += 1;
                }
                for c in 0..cols {
                    ptr[c + 1] += ptr[c];
                }
                let mut next = ptr.clone();
                let mut idx = vec![0usize; nnz];
                let mut slots: Vec<Option<T>> = vec![None; nnz];
                for (i, value) in entries.vals.into_iter().enumerate() {
                    let c = entries.coords[i * 2 + 1];
                    idx[next[c]] = entries.coords[i * 2];
                    slots[next[c]] = Some(value);
                    next[c] += 1;
                }
                let vals = slots.into_iter().flatten().collect();
                (Pattern::Compressed { ptr, idx }, vals)
            },
        };
        Self::assemble(
            format,
            shape,
            dim,
            display,
            zero_rank,
            pattern,
            T::wrap(values),
        )
    }

    fn assemble(
        format: SparseFormat,
        shape: Vec<usize>,
        dim: Dim,
        display: Option<Rc<UnitDef>>,
        zero_rank: u8,
        pattern: Pattern,
        values: SpValues,
    ) -> Self {
        let mut rank = zero_rank;
        let mut values = values;
        if let SpValues::Boxed(items) = &values {
            for item in items {
                if let Value::Num(num) = item {
                    rank = rank.max(kind_rank(&num.kind));
                }
            }
            // The `f64` class: every stored element, and the zero, a `Float`.
            let all_float = rank == 2
                && items
                    .iter()
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
            pattern,
            values,
        };
        debug_assert!(data.validate().is_ok(), "{:?}", data.validate());
        data
    }

    /// Checks the representation invariants.
    ///
    /// # Errors
    /// A description of the first violated invariant.
    pub fn validate(&self) -> Result<(), String> {
        let rank = self.rank();
        match self.format {
            | SparseFormat::Csr | SparseFormat::Csc if rank != 2 => {
                return Err(format!(
                    "{} must have rank 2, found {rank}",
                    self.format.name()
                ));
            },
            | _ => {},
        }
        let nnz = self.nnz();
        match &self.pattern {
            | Pattern::Coords(coords) => {
                if self.format != SparseFormat::Coo || coords.len() != nnz * rank {
                    return Err("coordinate list does not match the values".into());
                }
            },
            | Pattern::Compressed { ptr, idx } => {
                let axis = if self.format == SparseFormat::Csc {
                    self.shape[1]
                } else {
                    self.shape[0]
                };
                let other = if self.format == SparseFormat::Csc {
                    self.shape[0]
                } else {
                    self.shape[1]
                };
                if ptr.len() != axis + 1 || ptr[0] != 0 || ptr[axis] != nnz || idx.len() != nnz {
                    return Err("compressed pointers do not match the values".into());
                }
                for a in 0..axis {
                    if ptr[a] > ptr[a + 1] {
                        return Err("pointers are not monotone".into());
                    }
                    for k in ptr[a]..ptr[a + 1] {
                        if idx[k] >= other {
                            return Err("index out of range".into());
                        }
                        if k > ptr[a] && idx[k - 1] >= idx[k] {
                            return Err("indices are not strictly increasing".into());
                        }
                    }
                }
            },
        }
        if let Pattern::Coords(coords) = &self.pattern {
            for i in 0..nnz {
                let coord = &coords[i * rank..(i + 1) * rank];
                if coord.iter().zip(&self.shape).any(|(c, e)| c >= e) {
                    return Err("coordinate out of range".into());
                }
                if i > 0 && coords[(i - 1) * rank..i * rank] >= *coord {
                    return Err("coordinates are not strictly sorted".into());
                }
            }
        }
        let explicit_zero = match &self.values {
            | SpValues::F64(items) => items.iter().any(|v| *v == 0.0),
            | SpValues::Boxed(items) => {
                items
                    .iter()
                    .any(|v| matches!(v, Value::Num(n) if num_is_zero(n)))
            },
        };
        if explicit_zero {
            return Err("an explicit zero is stored".into());
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
    /// A description when a flat index does not fit in `u64`.
    pub fn to_flat_parts(&self) -> Result<(Vec<u64>, Vec<Value>), String> {
        let mut flat = Vec::with_capacity(self.nnz());
        let mut values = Vec::with_capacity(self.nnz());
        for (coord, value) in self.stored() {
            let mut index: u64 = 0;
            for (c, extent) in coord.iter().zip(&self.shape) {
                index = index
                    .checked_mul(*extent as u64)
                    .and_then(|i| i.checked_add(*c as u64))
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
        let total = shape
            .iter()
            .try_fold(1u64, |acc, e| acc.checked_mul(*e as u64))
            .ok_or("the shape overflows")?;
        let rank = shape.len();
        let mut entries: Entries<Value> = Entries::empty(rank);
        let mut coord = vec![0usize; rank];
        let mut zero_rank_seen = 0u8;
        for (position, (index, value)) in flat.iter().zip(values).enumerate() {
            if *index >= total || (position > 0 && flat[position - 1] >= *index) {
                return Err(format!("index {index} is out of range or out of order"));
            }
            let Value::Num(num) = &value else {
                return Err("a sparse element must be a number".to_string());
            };
            zero_rank_seen = zero_rank_seen.max(kind_rank(&num.kind));
            if num_is_zero(num) {
                continue;
            }
            kernels::unflatten(
                usize::try_from(*index).map_err(|_| "index too large")?,
                &shape,
                &mut coord,
            );
            entries.push(&coord, value);
        }
        Ok(Self::from_entries(
            format,
            shape,
            dim,
            None,
            zero_rank_seen,
            entries,
        ))
    }
}
