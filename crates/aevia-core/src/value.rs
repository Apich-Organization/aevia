//! Runtime values and their printed form.
//!
//! Types are erased (§9.2.3), but values keep the witnesses the runtime needs:
//! a scalar carries its dimension and an optional display unit, a `Point` is a
//! scalar with an affine display, a tensor carries its shape, and a variant
//! carries its constructor tag. Everything else — closures, records, IO
//! actions — is ordinary data.

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

mod numeric;
mod views;

pub(crate) use numeric::compensated_sum;
pub use numeric::kahan_sum;

use crate::sym::Term;
use aevia_dim::Dim;
use aevia_dim::DimEnv;
use aevia_dim::UnitDef;
use aevia_syntax::ast::BinOp;
use aevia_syntax::ast::Expr;
use aevia_syntax::ast::Pat;
use aevia_types::adt::AdtId;
use aevia_types::adt::{
    self,
};
use indexmap::IndexMap;
use num_rational::BigRational;
use num_traits::One;
use num_traits::Signed;
use num_traits::Zero;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::fmt::Write as _;
use std::rc::Rc;

/// The representation of a numeric value.
#[derive(Clone, Debug)]
pub enum NumKind {
    /// An arbitrary-precision integer (inline when it fits in `i64`).
    Int(crate::int::Int),
    /// An exact rational (shared: rarely on a hot path, and inline it
    /// would make every `Value` four times larger).
    Exact(Rc<BigRational>),
    /// An IEEE-754 double.
    Float(f64),
    /// A symbolic expression.
    Sym(Rc<Term>),
    /// A complex number.
    Complex(f64, f64),
}

impl NumKind {
    /// The runtime type name, for diagnostics.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            | Self::Int(_) => "Int",
            | Self::Exact(_) | Self::Sym(_) => "Real",
            | Self::Float(_) => "Float",
            | Self::Complex(..) => "Complex",
        }
    }
}

/// A scalar: a representation, a dimension, and an optional display unit.
#[derive(Clone, Debug)]
pub struct NumVal {
    /// The representation and value.
    pub kind: NumKind,
    /// The physical dimension.
    pub dim: Dim,
    /// The unit the value should be printed in, when it came from one.
    pub display: Option<Rc<UnitDef>>,
}

impl NumVal {
    /// An exact integer.
    #[must_use]
    pub fn int(value: impl Into<crate::int::Int>) -> Self {
        Self {
            kind: NumKind::Int(value.into()),
            dim: Dim::one(),
            display: None,
        }
    }

    /// An exact rational.
    #[must_use]
    pub fn exact(
        value: BigRational,
        dim: Dim,
    ) -> Self {
        Self {
            kind: NumKind::Exact(Rc::new(value)),
            dim,
            display: None,
        }
    }

    /// A float.
    #[must_use]
    pub const fn float(
        value: f64,
        dim: Dim,
    ) -> Self {
        Self {
            kind: NumKind::Float(value),
            dim,
            display: None,
        }
    }

    /// A symbolic value.
    #[must_use]
    pub fn symbolic(
        term: Term,
        dim: Dim,
    ) -> Self {
        Self {
            kind: NumKind::Sym(Rc::new(term)),
            dim,
            display: None,
        }
    }

    /// Attaches a display unit.
    #[must_use]
    pub fn with_display(
        mut self,
        unit: Rc<UnitDef>,
    ) -> Self {
        self.display = Some(unit);
        self
    }

    /// The value as `f64`, when it has a numeric representation.
    #[must_use]
    pub fn to_f64(&self) -> Option<f64> {
        match &self.kind {
            | NumKind::Int(value) => Some(value.to_f64()),
            | NumKind::Exact(value) => Some(aevia_dim::rat::to_f64(value)),
            | NumKind::Float(value) => Some(*value),
            | NumKind::Complex(re, im) => {
                if im.abs() < f64::EPSILON {
                    Some(*re)
                } else {
                    None
                }
            },
            | NumKind::Sym(term) => term.eval(&HashMap::new()).ok(),
        }
    }

    /// The value as an exact rational, when it has one.
    #[must_use]
    pub fn to_rational(&self) -> Option<BigRational> {
        match &self.kind {
            | NumKind::Int(value) => Some(value.to_rational()),
            | NumKind::Exact(value) => Some(value.as_ref().clone()),
            | NumKind::Float(value) => BigRational::from_float(*value),
            | _ => None,
        }
    }

    /// True when the value is exactly zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        match &self.kind {
            | NumKind::Int(value) => value.is_zero(),
            | NumKind::Exact(value) => value.is_zero(),
            | NumKind::Float(value) => *value == 0.0,
            | _ => false,
        }
    }

    /// True when the value contains free symbols.
    #[must_use]
    pub const fn is_symbolic(&self) -> bool {
        matches!(self.kind, NumKind::Sym(_))
    }
}

/// The element representation retained by an exact numeric tensor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExactRep {
    /// Integer elements.
    Int,
    /// Exact real elements.
    Real,
}

/// A tensor's backing storage. Typed exact buffers retain the element
/// representation needed to dispatch and reconstruct scalar values.
#[derive(Clone, Debug)]
pub enum Storage {
    /// Contiguous, row-major `f64` elements.
    ///
    /// # When a tensor is dense
    ///
    /// A tensor is `Dense` **iff every element is `NumKind::Float`**. A
    /// `Float` is an `f64` by definition, so the buffer is lossless: `show`,
    /// `repr`, `==` and every later operation see exactly the value the boxed
    /// form held.
    ///
    /// An `Int` or `Exact` element never enters this class, even when its
    /// value happens to be representable in `f64`:
    ///
    /// * the storage class carries the phase, and `repr` prints `3` for an
    ///   `Int` but `3.0` for a `Float`, so an `f64` buffer alone could not
    ///   reproduce the value it was built from;
    /// * an `Int` above 2^53 has no `f64` value at all, so "representable"
    ///   would silently round for large exact input;
    /// * `1/3` is not representable in any case.
    ///
    /// Exact tensors therefore stay in [`Storage::Exact`] — `[1/3]` is never
    /// dense — and a tensor that mixes phases, or holds structured elements,
    /// is [`Storage::Boxed`].
    ///
    /// # Invariant
    ///
    /// `data.len() == TensorData::element_count(shape)` for the shape on the
    /// enclosing [`TensorData`], and the data is in row-major order. The shape
    /// is not duplicated here: one copy cannot disagree with itself.
    ///
    /// The buffer is behind an `Rc` so that a slice can be a [`View`] of it
    /// rather than a copy: [`TensorData::buffer`] exposes it, and `Rc::ptr_eq`
    /// on two of them is what "shares storage" means.
    Dense(Rc<Vec<f64>>),
    /// Exact elements whose original representation is unknown.
    Exact(Vec<BigRational>),
    /// Exact elements with a proven representation. Integer buffers contain
    /// only rational numbers with denominator one.
    TypedExact(Vec<BigRational>, ExactRep),
    /// Mixed or structured elements.
    Boxed(Vec<Value>),
    /// `Bool` elements.
    Bool(Vec<bool>),
    /// A strided window over an `f64` buffer.
    ///
    /// A slice, a transpose or a reversed axis of an `f64` tensor is this
    /// variant: it owns no elements, only a share of its parent's buffer, so
    /// `xs[1:3]` allocates nothing, `transpose(m)` moves no data and
    /// `xs[::-1]` is the same buffer walked backwards. The buffer is `f64`, so
    /// the phase rule of [`Storage::Dense`] applies unchanged — a view is a
    /// `Float` tensor whatever it was sliced from.
    View(View),
    /// A logical contiguous range sharing a tensor of any element representation.
    /// Nested suffixes are flattened, so repeated pattern tails are O(1).
    SharedSlice(SharedSlice),
}

/// A rank-one logical window over immutable shared tensor storage.
#[derive(Clone, Debug)]
pub struct SharedSlice {
    pub(crate) source: Rc<TensorData>,
    pub(crate) offset: usize,
    pub(crate) len: usize,
}
impl SharedSlice {
    /// The original tensor whose storage this window retains.
    #[must_use]
    pub const fn source(&self) -> &Rc<TensorData> {
        &self.source
    }

    /// The logical start within the original tensor.
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }
}

/// A strided window over a shared `f64` buffer (§6.6.4, §9.5).
///
/// The element at logical row-major index `(i₀, …, iₙ₋₁)` is
/// `data[offset + Σ iₖ·sₖ]`, where the signed step `sₖ` is
/// `±strides[k]` — negative exactly when `reversed[k]` is set.
///
/// # Invariants
///
/// * `strides.len()` and `reversed.len()` equal the rank of the enclosing
///   shape;
/// * `len` equals the product of that shape;
/// * every reachable position is inside `data`, in *both* directions.
///
/// # Signed strides
///
/// A stride is stored as a magnitude plus a per-axis direction, not as a
/// signed number, because the sign is orthogonal to every question the rest of
/// the module asks of a stride: [`TensorData::dense`] compares the magnitudes
/// against [`row_major_strides`] to decide whether a window is contiguous, and
/// [`TensorData::strides`] publishes them as `&[usize]`. Coordinate movement
/// uses checked offsets in the stored direction.
///
/// A reversed axis — `xs[::-1]`, a flipped convolution kernel, an axis a fold
/// ran backwards — moves no element: the offset starts at the far end of the
/// axis and the direction flips. The result is never contiguous (its elements
/// run backwards through the buffer), so it materialises at the kernel
/// boundary like any other strided view.
#[derive(Clone, Debug)]
pub struct View {
    /// The buffer this view shares with its parent and its siblings.
    pub data: Rc<Vec<f64>>,
    /// The position of the first logical element.
    pub offset: usize,
    /// One stride per dimension, in elements. A stride is a *magnitude*: the
    /// direction along the axis is [`View::reversed`].
    pub strides: Vec<usize>,
    /// One direction per dimension. `reversed[axis]` is true when increasing
    /// the logical index moves *backwards* through the buffer, so coordinate
    /// `c` on that axis is at `offset - c·strides[axis]`.
    pub reversed: Vec<bool>,
    /// The number of logical elements.
    pub len: usize,
}

impl Storage {
    pub(crate) fn backing(&self) -> &Self {
        let mut current = self;
        while let Self::SharedSlice(view) = current {
            current = &view.source.storage;
        }
        current
    }

    /// Number of stored elements.
    ///
    /// For a view this is the number of *logical* elements, which is what
    /// every caller means by "how many elements has this tensor": the buffer
    /// is usually larger than the window.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            | Self::Dense(items) => items.len(),
            | Self::Exact(items) | Self::TypedExact(items, _) => items.len(),
            | Self::Boxed(items) => items.len(),
            | Self::Bool(items) => items.len(),
            | Self::View(view) => view.len,
            | Self::SharedSlice(view) => view.len,
        }
    }

    /// True when there are no elements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True when the elements are a contiguous `f64` buffer.
    #[must_use]
    pub const fn is_dense(&self) -> bool {
        matches!(self, Self::Dense(_))
    }

    /// True when the elements are a strided window.
    #[must_use]
    pub const fn is_view(&self) -> bool {
        matches!(self, Self::View(_) | Self::SharedSlice(_))
    }
}

/// A tensor: a shape, an element dimension, and storage.
///
/// The storage may be a contiguous buffer ([`Storage::Dense`]), a strided
/// window into one ([`Storage::View`]), or one of the boxed classes. Nothing
/// outside this module may treat the class as semantics: the values, the
/// rendering, the shape and the dimension of a tensor are the same whichever
/// class holds it.
#[derive(Clone, Debug)]
pub struct TensorData {
    /// Row-major shape.
    pub shape: Vec<usize>,
    /// The elements.
    pub storage: Storage,
    /// The element dimension.
    pub dim: Dim,
    /// A display unit applied to every element.
    pub display: Option<Rc<UnitDef>>,
    /// Immutable element-schema provenance, set only at a verified creation.
    pub(crate) verified_element: Option<Rc<aevia_types::Type>>,
}

/// The row-major strides of a shape: `strides[last] = 1`, and each earlier
/// stride is the product of the extents after it.
#[must_use]
pub(crate) fn row_major_strides(shape: &[usize]) -> Option<Vec<usize>> {
    let mut strides = vec![1; shape.len()];
    let mut stride = 1usize;
    for (slot, extent) in strides
        .iter_mut()
        .rev()
        .skip(1)
        .zip(shape.iter().skip(1).rev())
    {
        stride = stride.checked_mul(*extent)?;
        *slot = stride;
    }
    Some(strides)
}

/// The physical element count, allowing zero extents and rejecting overflow.
pub(crate) fn checked_element_count(shape: &[usize]) -> Option<usize> {
    shape
        .iter()
        .try_fold(1usize, |count, extent| count.checked_mul(*extent))
        .or_else(|| shape.contains(&0).then_some(0))
}

impl TensorData {
    /// Build an unverified tensor from host data. No element schema is inferred
    /// for empty boxed storage; use the checked constructor for explicit proof.
    #[must_use]
    pub const fn new(
        shape: Vec<usize>,
        storage: Storage,
        dim: Dim,
        display: Option<Rc<UnitDef>>,
    ) -> Self {
        Self {
            shape,
            storage,
            dim,
            display,
            verified_element: None,
        }
    }

    /// Construct a host tensor with an explicitly declared element schema.
    /// Shape, dimensions, storage and every present element are checked;
    /// an empty boxed tensor obtains its element schema only from this explicit
    /// declaration, never from a later variable annotation.
    ///
    /// # Errors
    /// Returns an error if the shape, contents or element schema disagree.
    pub fn checked(
        shape: Vec<usize>,
        storage: Storage,
        dim: Dim,
        display: Option<Rc<UnitDef>>,
        element: aevia_types::Type,
    ) -> Result<Self, String> {
        let mut tensor = Self::new(shape, storage, dim, display);
        let rank = u32::try_from(tensor.shape.len()).map_err(|_| "tensor rank is too large")?;
        crate::host_witness::tensor_witnesses(
            &tensor,
            &aevia_types::Shape::Rank(rank),
            &element,
            true,
        )
        .ok_or_else(|| "tensor contents do not prove the declared element schema".to_string())?;
        tensor.verified_element = Some(Rc::new(element));
        Ok(tensor)
    }

    /// The number of elements implied by the shape, saturated at `usize::MAX`.
    /// Use [`Self::checked_element_count`] to reject overflow.
    #[must_use]
    pub fn element_count(shape: &[usize]) -> usize {
        checked_element_count(shape).unwrap_or(usize::MAX)
    }

    /// The element count, or `None` if a nonempty shape overflows `usize`.
    #[must_use]
    pub fn checked_element_count(&self) -> Option<usize> {
        checked_element_count(&self.shape)
    }

    /// The rank.
    #[must_use]
    pub const fn rank(&self) -> usize {
        self.shape.len()
    }

    /// The element at a logical row-major index.
    ///
    /// A view is walked through its strides, so index `k` is the `k`-th
    /// element in *this tensor's* row-major order — which is what `show`,
    /// `elements` and `==` all mean by "the k-th element".
    #[must_use]
    pub fn get_flat(
        &self,
        index: usize,
    ) -> Option<Value> {
        match &self.storage {
            | Storage::Dense(items) => items.get(index).map(|value| self.float_value(*value)),
            | Storage::SharedSlice(view) => {
                if index >= view.len {
                    return None;
                }
                let mut value = view.source.get_flat(view.offset.checked_add(index)?)?;
                if let Value::Num(number) = &mut value {
                    number.dim = self.dim.clone();
                    number.display.clone_from(&self.display);
                }
                Some(value)
            },
            | Storage::View(view) => {
                view.element_at(&self.shape, index)
                    .map(|value| self.float_value(value))
            },
            | Storage::Exact(items) | Storage::TypedExact(items, ExactRep::Real) => {
                items.get(index).map(|value| {
                    let mut num = NumVal::exact(value.clone(), self.dim.clone());
                    num.display.clone_from(&self.display);
                    Value::Num(num)
                })
            },
            | Storage::TypedExact(items, ExactRep::Int) => {
                let value = items.get(index)?;
                if !value.is_integer() {
                    return None;
                }
                Some(Value::Num(NumVal {
                    kind: NumKind::Int(value.to_integer().into()),
                    dim: self.dim.clone(),
                    display: self.display.clone(),
                }))
            },
            | Storage::Boxed(items) => items.get(index).cloned(),
            | Storage::Bool(items) => items.get(index).map(|value| Value::Bool(*value)),
        }
    }

    /// A `Float` element carrying this tensor's dimension and display unit.
    fn float_value(
        &self,
        value: f64,
    ) -> Value {
        let mut num = NumVal::float(value, self.dim.clone());
        num.display.clone_from(&self.display);
        Value::Num(num)
    }

    /// The `f64` buffer, when the tensor's elements are contiguous in one.
    ///
    /// A view whose strides are the row-major strides of its own shape is
    /// contiguous — `m[1..3, 0..3]` of a 3×3 matrix is — and answers with its
    /// window; a view that is not (`m[0..3, 1..2]`, `transpose(m)`, or any
    /// reversed axis) answers `None` and is materialised by
    /// [`TensorData::dense_flat`] instead.
    ///
    /// Every caller that uses this must keep the boxed path's answer
    /// bit-identical: the buffer is an optimisation, not a second semantics.
    #[must_use]
    pub fn dense(&self) -> Option<&[f64]> {
        match &self.storage {
            | Storage::Dense(items) => {
                (self.checked_element_count()? == items.len()).then_some(items.as_slice())
            },
            | Storage::View(view) => view.contiguous_slice(&self.shape),
            | Storage::SharedSlice(view) => {
                if self.checked_element_count()? != view.len {
                    return None;
                }
                view.source
                    .dense()?
                    .get(view.offset..view.offset.checked_add(view.len)?)
            },
            | _ => None,
        }
    }

    /// The `f64` elements in row-major order, borrowing when possible.
    ///
    /// Kernels that require a contiguous slice use this boundary. Dense
    /// storage and forward row-major views are borrowed. Strided and
    /// reversed views are materialised in logical order.
    ///
    /// Returns `None` when storage is neither `f64` nor a view of it, when the
    /// storage disagrees with the shape, or when materialisation fails.
    #[must_use]
    pub fn dense_flat(&self) -> Option<Cow<'_, [f64]>> {
        match &self.storage {
            | Storage::Dense(_) => self.dense().map(Cow::Borrowed),
            | Storage::SharedSlice(view) => {
                if self.checked_element_count()? != view.len {
                    return None;
                }
                let end = view.offset.checked_add(view.len)?;
                match view.source.dense_flat()? {
                    | Cow::Borrowed(data) => Some(Cow::Borrowed(data.get(view.offset..end)?)),
                    | Cow::Owned(data) => Some(Cow::Owned(data.get(view.offset..end)?.to_vec())),
                }
            },
            | Storage::View(view) => view.dense_flat(&self.shape),
            | _ => None,
        }
    }

    /// The buffer this tensor's elements live in, when it has one.
    ///
    /// Two tensors share storage exactly when these are `Rc::ptr_eq`: that is
    /// the property a slice must have, and the one a copying slice lacks.
    #[must_use]
    pub const fn buffer(&self) -> Option<&Rc<Vec<f64>>> {
        match &self.storage {
            | Storage::Dense(data) => Some(data),
            | Storage::View(view) => Some(&view.data),
            | _ => None,
        }
    }

    /// True when the tensor is a contiguous `f64` buffer.
    #[must_use]
    pub const fn is_dense(&self) -> bool {
        self.storage.is_dense()
    }

    /// True when the elements are a strided window.
    #[must_use]
    pub const fn is_view(&self) -> bool {
        self.storage.is_view()
    }

    /// True when the elements are contiguous in one `f64` buffer, dense or
    /// not. This is the predicate the `f64` kernels can rely on.
    #[must_use]
    pub fn is_contiguous(&self) -> bool {
        self.dense().is_some()
    }

    /// The offset of a view, or `0` for a contiguous class.
    #[must_use]
    pub const fn offset(&self) -> usize {
        match &self.storage {
            | Storage::View(view) => view.offset,
            | _ => 0,
        }
    }

    /// One stride per dimension, when the tensor is a view.
    ///
    /// These are magnitudes: the direction along each axis is
    /// [`TensorData::reversed`].
    #[must_use]
    pub fn strides(&self) -> Option<&[usize]> {
        match &self.storage {
            | Storage::View(view) => Some(&view.strides),
            | _ => None,
        }
    }

    /// One direction per dimension, when the tensor is a view.
    ///
    /// `true` means the axis runs against the buffer, so increasing the
    /// logical index moves towards lower positions.
    #[must_use]
    pub fn reversed(&self) -> Option<&[bool]> {
        match &self.storage {
            | Storage::View(view) => Some(&view.reversed),
            | _ => None,
        }
    }

    /// The element at a multi-dimensional index, with negative indices counted
    /// from the end (§6.6.4).
    #[must_use]
    pub fn get(
        &self,
        indices: &[isize],
    ) -> Option<Value> {
        let flat = self.flat_index(indices)?;
        self.get_flat(flat)
    }

    pub(crate) fn flat_index(
        &self,
        indices: &[isize],
    ) -> Option<usize> {
        if indices.len() != self.shape.len() {
            return None;
        }
        let mut flat = 0usize;
        for (index, extent) in indices.iter().zip(self.shape.iter()) {
            let normalized = if *index < 0 {
                extent.checked_add_signed(*index)?
            } else {
                usize::try_from(*index).ok()?
            };
            if normalized >= *extent {
                return None;
            }
            flat = flat.checked_mul(*extent)?.checked_add(normalized)?;
        }
        Some(flat)
    }

    /// A zero-copy suffix of a rank-one tensor, preserving its representation,
    /// dimension, display unit and authenticated element schema. Repeated tails
    /// point directly to the original storage instead of forming a view chain.
    #[must_use]
    pub fn tail_view(
        self: &Rc<Self>,
        start: usize,
    ) -> Option<Rc<Self>> {
        let [extent] = self.shape.as_slice() else {
            return None;
        };
        if *extent != self.storage.len() {
            return None;
        }
        let len = extent.checked_sub(start)?;
        if start == 0 {
            return Some(self.clone());
        }
        let (source, offset) = match &self.storage {
            | Storage::SharedSlice(view) => (view.source.clone(), view.offset.checked_add(start)?),
            | _ => (self.clone(), start),
        };
        Some(Rc::new(Self {
            shape: vec![len],
            storage: Storage::SharedSlice(SharedSlice { source, offset, len }),
            dim: self.dim.clone(),
            display: self.display.clone(),
            verified_element: self.verified_element.clone(),
        }))
    }

    /// The elements in row-major order.
    #[must_use]
    pub fn elements(&self) -> Vec<Value> {
        self.iter_elements().collect()
    }

    /// Reads each available element in row-major order as it is needed.
    pub(crate) fn iter_elements(&self) -> impl Iterator<Item = Value> + '_ {
        (0..self.storage.len()).filter_map(|index| self.get_flat(index))
    }

    /// A contiguous copy of this tensor's elements, when they are `f64`.
    ///
    /// This is the explicit materialisation step: a kernel that cannot walk
    /// strides calls it, and gets a tensor whose buffer is exactly its
    /// elements in row-major order.
    #[must_use]
    pub fn to_dense(&self) -> Option<Self> {
        let data = self.dense_flat()?.into_owned();
        Some(Self {
            verified_element: self.verified_element.clone(),
            shape: self.shape.clone(),
            storage: Storage::Dense(Rc::new(data)),
            dim: self.dim.clone(),
            display: self.display.clone(),
        })
    }

    /// The storage a reshape can reuse, when the elements of `shape` are the
    /// elements this tensor holds.
    ///
    /// §6.6.4 and §9.5: `reshape` is a **view when the elements are
    /// contiguous, a copy otherwise**. A dense buffer is already contiguous
    /// and is shared as-is; a view whose strides are the row-major strides of
    /// its own shape (`m[1..3, 0..3]`) names a contiguous window of its parent
    /// buffer, so the new shape gets fresh row-major strides over that same
    /// window and the buffer is shared (`Rc::ptr_eq` holds). A strided or
    /// reversed view has no single contiguous buffer to point into, so it is
    /// materialised in row-major order.
    ///
    /// Returns `None` when the element count is inconsistent or overflows,
    /// or a view cannot be read.
    #[must_use]
    pub fn reshape_storage(
        &self,
        shape: &[usize],
    ) -> Option<Storage> {
        let len = checked_element_count(shape)?;
        if len != self.storage.len() || self.checked_element_count() != Some(len) {
            return None;
        }
        match &self.storage {
            | Storage::SharedSlice(_) => {
                if len == 0 {
                    return Some(match self.storage.backing() {
                        | Storage::Dense(_) | Storage::View(_) => {
                            Storage::Dense(Rc::new(Vec::new()))
                        },
                        | Storage::Exact(_) => Storage::Exact(Vec::new()),
                        | Storage::TypedExact(_, rep) => Storage::TypedExact(Vec::new(), *rep),
                        | Storage::Bool(_) => Storage::Bool(Vec::new()),
                        | Storage::Boxed(_) => Storage::Boxed(Vec::new()),
                        | Storage::SharedSlice(_) => return None,
                    });
                }
                if self.dense().is_some() {
                    let View { data, offset, .. } = self.f64_window()?;
                    return Some(Storage::View(View {
                        data,
                        offset,
                        strides: row_major_strides(shape)?,
                        reversed: vec![false; shape.len()],
                        len,
                    }));
                }
                if let Some(data) = self.dense_flat() {
                    return Some(Storage::Dense(Rc::new(data.into_owned())));
                }
                Some(self.storage.clone())
            },
            | Storage::View(view) => {
                // `len == 0` cannot be a view (empty tensors stay boxed), but
                // guarding it keeps the offset arithmetic honest.
                if len == 0 {
                    return Some(Storage::Dense(Rc::new(Vec::new())));
                }
                if self.dense().is_some() {
                    return Some(Storage::View(View {
                        data: view.data.clone(),
                        offset: view.offset,
                        strides: row_major_strides(shape)?,
                        reversed: vec![false; shape.len()],
                        len,
                    }));
                }
                Some(Storage::Dense(Rc::new(self.dense_flat()?.into_owned())))
            },
            | other => Some(other.clone()),
        }
    }
}

/// A record value; fields keep their insertion order.
#[derive(Clone, Debug)]
pub struct RecordValue {
    /// The fields.
    pub fields: IndexMap<String, Value>,
}

/// A constructed variant.
#[derive(Clone, Debug)]
pub struct VariantValue {
    /// The type it belongs to.
    pub adt: AdtId,
    /// The constructor index.
    pub index: usize,
    /// The constructor name.
    pub name: String,
    /// The payload.
    pub fields: Vec<Value>,
    /// Concrete type arguments supplied by a checked expression, if known.
    pub type_args: Option<Vec<aevia_types::Type>>,
    /// A concrete type established at an authenticated source constructor.
    /// Unlike `type_args`, this is never inferred from a variable's annotation.
    pub(crate) verified_type: Option<aevia_types::Type>,
    /// The value originated at an interpreter-resolved source constructor.
    pub(crate) constructor_origin: bool,
}

impl Drop for VariantValue {
    /// Unwinds a long constructor chain iteratively.
    ///
    /// Dropping `Cons(1, Cons(2, …))` recursively would exhaust the Rust stack
    /// inside `Drop`, where no `E0805` can be raised and the process aborts
    /// (§9.3.4). The payload is moved onto an explicit worklist instead.
    fn drop(&mut self) {
        if self.fields.is_empty() {
            return;
        }
        let mut pending = std::mem::take(&mut self.fields);
        while let Some(value) = pending.pop() {
            if let Value::Variant(variant) = value
                && let Ok(mut inner) = Rc::try_unwrap(variant)
            {
                pending.append(&mut inner.fields);
            }
        }
    }
}

/// A value whose dimension is known only at run time (§3.11, §4.9.2).
///
/// `Dyn<ρ>` is `(Scalar<ρ,1>, DimValue, display unit)`: the payload is the
/// coherent value with its dimension stripped, and the dimension travels
/// beside it. It is not a `Scalar` and does not convert implicitly — `check`
/// is the only way back (§4.9.4).
#[derive(Clone, Debug)]
pub struct DynValue {
    /// The payload, dimensionless by construction.
    pub value: NumVal,
    /// The dimension the value carries.
    pub dim: Dim,
    /// The unit the value was written with, when it had one.
    pub display: Option<Rc<UnitDef>>,
}

impl DynValue {
    /// Wraps a scalar, moving its dimension out of the payload.
    #[must_use]
    pub fn new(num: &NumVal) -> Self {
        let mut payload = num.clone();
        payload.dim = Dim::one();
        payload.display = None;
        Self {
            value: payload,
            dim: num.dim.clone(),
            display: num.display.clone(),
        }
    }

    /// The dimensionless payload, as a value.
    #[must_use]
    pub fn payload(&self) -> NumVal {
        self.value.clone()
    }

    /// The payload with the carried dimension restored.
    #[must_use]
    pub fn as_scalar(&self) -> NumVal {
        let mut scalar = self.value.clone();
        scalar.dim = self.dim.clone();
        scalar.display.clone_from(&self.display);
        scalar
    }
}

/// A reserved source name either awaits initialization or intentionally hides
/// its first nonrecursive initializer's own binding in favor of the Prelude.
pub(crate) enum SourceLookupFailure {
    Uninitialized,
    Prelude,
}

thread_local! {
    static ENVIRONMENT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Records that a binding table name resolution reads (a scope's variables or
/// source revisions, the interpreter's globals or method classes) changed.
///
/// Global references are resolved through an inline cache
/// (`Interp::lookup_global_at`) whose entries are only valid for the epoch
/// they were filled in; every mutation of such a table calls this first.
/// Scopes are `Rc` and never cross threads, so a thread-local counter covers
/// every table a thread's interpreter can read.
pub fn bump_environment() {
    ENVIRONMENT.with(|epoch| epoch.set(epoch.get().wrapping_add(1)));
}

/// The current environment epoch; see [`bump_environment`].
pub(crate) fn environment_epoch() -> u64 {
    ENVIRONMENT.with(std::cell::Cell::get)
}

/// A lexical environment: a persistent chain of scopes.
///
/// The bindings are behind a `RefCell` so that a recursive function can be
/// inserted into the very scope it captures (§9.3.1).
#[derive(Debug, Default)]
pub struct Scope {
    /// The enclosing scope.
    pub parent: Option<Rc<Self>>,
    /// Bindings introduced here.
    pub vars: RefCell<crate::fxhash::FxHashMap<String, Value>>,
    /// Top-level source revisions, including reserved forward declarations.
    /// `None` spans hold bindings inherited from an earlier incremental batch.
    source_revisions:
        RefCell<crate::fxhash::FxHashMap<String, Vec<(Option<aevia_syntax::Span>, Option<Value>)>>>,
    /// Ranges whose first nonrecursive initializer hides its own reservation.
    source_initializers: RefCell<crate::fxhash::FxHashMap<String, Vec<aevia_syntax::Span>>>,
    /// Exact checked revision used by this lexical environment.
    pub(crate) context: Option<Rc<crate::evidence::ModuleContext>>,
    /// Imported module environments retain their own implementation revision.
    /// Module imports are acyclic, so these edges create no ownership cycle.
    pub(crate) imports: Vec<Rc<Self>>,
    pub(crate) types: Option<Rc<crate::generics::CallBindings>>,
}

// A flat do program can introduce many lexical scopes without nesting calls.
// Release an exclusively owned parent spine iteratively; shared parents and
// arbitrary value/closure ownership graphs retain normal Rc semantics.
impl Drop for Scope {
    fn drop(&mut self) {
        let mut parent = self.parent.take();
        while let Some(current) = parent {
            match Rc::try_unwrap(current) {
                | Ok(mut scope) => {
                    parent = scope.parent.take();
                    // Its parent has been detached, so this drop cannot recurse
                    // down the same lexical spine.
                    drop(scope);
                },
                | Err(_) => break,
            }
        }
    }
}

impl Scope {
    /// An empty root scope.
    #[must_use]
    pub fn root() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// A child scope.
    #[must_use]
    pub fn child(parent: &Rc<Self>) -> Rc<Self> {
        Rc::new(Self {
            parent: Some(parent.clone()),
            vars: RefCell::default(),
            source_revisions: RefCell::default(),
            source_initializers: RefCell::default(),
            context: parent.context.clone(),
            imports: Vec::new(),
            types: parent.types.clone(),
        })
    }

    pub(crate) fn module(
        context: Rc<crate::evidence::ModuleContext>,
        imports: Vec<Rc<Self>>,
    ) -> Rc<Self> {
        Rc::new(Self {
            parent: None,
            vars: RefCell::default(),
            source_revisions: RefCell::default(),
            source_initializers: RefCell::default(),
            context: Some(context),
            imports,
            types: None,
        })
    }

    pub(crate) fn child_with_types(
        parent: &Rc<Self>,
        types: Option<Rc<crate::generics::CallBindings>>,
    ) -> Rc<Self> {
        Rc::new(Self {
            parent: Some(parent.clone()),
            vars: RefCell::default(),
            source_revisions: RefCell::default(),
            source_initializers: RefCell::default(),
            context: parent.context.clone(),
            imports: Vec::new(),
            types,
        })
    }

    /// Reserve every declaration before initialization so a deferred forward
    /// reference cannot accidentally resolve to another revision of the name.
    pub(crate) fn reserve_source(
        &self,
        name: &str,
        visible: aevia_syntax::Span,
    ) {
        self.update_source_revision(name, visible, None);
    }

    pub(crate) fn reserve_initializer(
        &self,
        name: &str,
        initializer: aevia_syntax::Span,
    ) {
        crate::value::bump_environment();
        let mut initializers = self.source_initializers.borrow_mut();
        let ranges = initializers.entry(name.to_string()).or_default();
        if !ranges.contains(&initializer) {
            ranges.push(initializer);
        }
    }

    /// Install a source revision, keeping the public table ordered by source
    /// position even when deferred initializers complete out of order.
    pub(crate) fn bind_source(
        &self,
        name: &str,
        value: Value,
        visible: aevia_syntax::Span,
    ) {
        self.update_source_revision(name, visible, Some(value));
    }

    fn update_source_revision(
        &self,
        name: &str,
        visible: aevia_syntax::Span,
        value: Option<Value>,
    ) {
        bump_environment();
        let mut revisions = self.source_revisions.borrow_mut();
        let entries = revisions.entry(name.to_string()).or_insert_with(|| {
            self.vars
                .borrow()
                .get(name)
                .cloned()
                .map_or_else(Vec::new, |value| vec![(None, Some(value))])
        });
        let publishing = value.is_some();
        if let Some((_, target)) = entries.iter_mut().find(|(site, _)| *site == Some(visible)) {
            if publishing {
                *target = value;
            }
        } else {
            entries.push((Some(visible), value));
        }
        if publishing
            && let Some((_, Some(latest))) = entries
                .iter()
                .filter(|(_, value)| value.is_some())
                .max_by_key(|(site, _)| site.map(|span| (span.file, span.start)))
        {
            self.vars
                .borrow_mut()
                .insert(name.to_string(), latest.clone());
        }
    }

    /// Replace the declaration visible at `reference`, retaining all other
    /// revisions. Source plugins use their attribute site so a later function
    /// with the same name cannot steal the registration or its wrapper.
    pub(crate) fn replace_source_at(
        &self,
        name: &str,
        reference: aevia_syntax::Span,
        replacement: Value,
    ) -> Option<Value> {
        crate::value::bump_environment();
        let mut revisions = self.source_revisions.borrow_mut();
        if let Some(entries) = revisions.get_mut(name) {
            let preceding = entries
                .iter()
                .enumerate()
                .filter(|(_, (site, _))| {
                    site.is_some_and(|span| {
                        span.file == reference.file && span.start <= reference.start
                    })
                })
                .max_by_key(|(_, (site, _))| site.map(|span| span.start))
                .map(|(index, _)| index);
            let baseline = entries.iter().position(|(site, _)| site.is_none());
            let first = entries
                .iter()
                .enumerate()
                .filter(|(_, (site, _))| site.is_some_and(|span| span.file == reference.file))
                .min_by_key(|(_, (site, _))| site.map(|span| span.start))
                .map(|(index, _)| index);
            if let Some((_, target)) = preceding
                .or(baseline)
                .or(first)
                .and_then(|index| entries.get_mut(index))
            {
                let original = target.replace(replacement);
                if let Some((_, Some(latest))) = entries
                    .iter()
                    .filter(|(_, value)| value.is_some())
                    .max_by_key(|(site, _)| site.map(|span| (span.file, span.start)))
                {
                    self.vars
                        .borrow_mut()
                        .insert(name.to_string(), latest.clone());
                }
                return original;
            }
        }
        self.vars.borrow_mut().insert(name.to_string(), replacement)
    }

    /// Resolve top-level references at their lexical source site. Local
    /// scopes retain ordinary lexical shadowing. An unavailable selected
    /// revision asks module initialization to defer, rather than falling back
    /// to a differently typed revision from the public globals table.
    pub(crate) fn lookup_at(
        self: &Rc<Self>,
        name: &str,
        reference: aevia_syntax::Span,
    ) -> Result<Option<Value>, SourceLookupFailure> {
        let mut scope: &Self = self;
        loop {
            {
                let revisions = scope.source_revisions.borrow();
                if let Some(entries) = revisions.get(name) {
                    let same_file = |site: &Option<aevia_syntax::Span>| {
                        site.is_some_and(|span| span.file == reference.file)
                    };
                    let preceding = entries
                        .iter()
                        .filter(|(site, _)| {
                            same_file(site)
                                && site.is_some_and(|span| span.start <= reference.start)
                        })
                        .max_by_key(|(site, _)| site.map(|span| span.start));
                    let baseline = entries.iter().find(|(site, _)| site.is_none());
                    let first = entries
                        .iter()
                        .filter(|(site, _)| same_file(site))
                        .min_by_key(|(site, _)| site.map(|span| span.start));
                    // The checker hides the first nonrecursive definition
                    // while checking its initializer. Its own references may
                    // therefore denote a Prelude function/constant, while a
                    // forward reference outside it still selects this source
                    // reservation. Repeated definitions see the preceding one.
                    if preceding.is_none()
                        && baseline.is_none()
                        && let Some((Some(first_site), _)) = first
                        && self_initializes(scope, name, *first_site, reference)
                    {
                        return Err(SourceLookupFailure::Prelude);
                    }
                    if let Some((_, value)) = preceding.or(baseline).or(first) {
                        return value
                            .clone()
                            .map(Some)
                            .ok_or(SourceLookupFailure::Uninitialized);
                    }
                }
            }
            if let Some(value) = scope.vars.borrow().get(name) {
                return Ok(Some(value.clone()));
            }
            let Some(parent) = scope.parent.as_deref() else {
                return Ok(None);
            };
            scope = parent;
        }
    }

    /// Looks a name up through the chain.
    #[must_use]
    pub fn lookup(
        self: &Rc<Self>,
        name: &str,
    ) -> Option<Value> {
        // Walk by reference: cloning the `Rc` at every level was a refcount
        // round trip per scope for every global read.
        let mut scope: &Self = self;
        loop {
            if let Some(value) = scope.vars.borrow().get(name) {
                return Some(value.clone());
            }
            scope = scope.parent.as_deref()?;
        }
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps the three distinct source-span predicates together"
)]
fn self_initializes(
    scope: &Scope,
    name: &str,
    visible: aevia_syntax::Span,
    reference: aevia_syntax::Span,
) -> bool {
    scope
        .source_initializers
        .borrow()
        .get(name)
        .is_some_and(|initializers| {
            initializers.iter().any(|initializer| {
                let in_reference_file = initializer.file == reference.file;
                let surrounds_reference =
                    initializer.start <= reference.start && reference.end <= initializer.end;
                // The visibility site starts where this initializer ends.
                // This ties the reference to this definition, not another one
                // of the same name whose initializer happens to contain it.
                let opens_definition = initializer.end == visible.start;
                in_reference_file && surrounds_reference && opens_definition
            })
        })
}

/// What a closure runs.
///
/// Both forms are `Value::Closure`, so everything that consumes a function —
/// `map`, `diff`, `run_io` — keeps working unchanged; only `apply` needs to
/// know which interpreter to call.
#[derive(Clone, Debug)]
pub enum ClosureBody {
    /// A body interpreted from the typed AST.
    Ast(Rc<Expr>),
    /// A body lowered to the Core IR.
    Ir(Rc<aevia_ir::Function>),
}

/// The environment a closure captured.
#[derive(Clone, Debug)]
pub enum ClosureEnv {
    /// A persistent chain of scopes (the AST path).
    Ast(Rc<Scope>),
    /// A flat vector of captured values, indexed by capture position (the IR
    /// path), plus the module scope top-level names resolve through.
    Ir(Rc<IrEnv>),
}

/// The environment of an IR closure.
///
/// The vector is behind a `RefCell` for one reason only: a block-local
/// recursive binding has to place the closure inside its own environment after
/// the closure exists. Every other environment is written once, at construction.
#[derive(Debug)]
pub struct IrEnv {
    /// The captured values, in capture order.
    pub slots: RefCell<Vec<Value>>,
    /// The module's scope, so a top-level name resolves exactly as it would
    /// through the AST path's lexical chain.
    pub scope: Option<Rc<Scope>>,
}

/// A user function together with its captured environment.
#[derive(Clone, Debug)]
pub struct Closure {
    /// The parameters, as patterns. Empty for an IR closure, whose parameters
    /// live on the lowered [`aevia_ir::Function`].
    pub(crate) params: Vec<Pat>,
    /// The body.
    pub(crate) body: ClosureBody,
    /// The captured environment.
    pub(crate) env: ClosureEnv,
    /// The name, for diagnostics.
    ///
    /// `Rc<str>`: the stack frame clones the name on every call, and cloning a
    /// `String` made every call allocate.
    pub(crate) name: Option<Rc<str>>,
    pub(crate) proof: Option<Rc<crate::generics::FunctionProof>>,
    pub(crate) captured_bindings: Option<Rc<crate::generics::CallBindings>>,
    /// Proof specialization copies preserve the original callable allocation.
    /// Weak pointer identity remains valid after its strong value is dropped.
    pub(crate) canonical_origin: std::rc::Weak<Self>,
}

impl Closure {
    /// An AST-path closure.
    #[must_use]
    pub fn ast(
        params: Vec<Pat>,
        body: Expr,
        env: Rc<Scope>,
        name: Option<String>,
    ) -> Self {
        let captured_bindings = env.types.clone();
        Self {
            params,
            body: ClosureBody::Ast(Rc::new(body)),
            env: ClosureEnv::Ast(env),
            name: name.map(Rc::from),
            proof: None,
            captured_bindings,
            canonical_origin: std::rc::Weak::new(),
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "part of the callable identity API, shared with the JIT feature"
    )]
    pub(crate) fn origin(closure: &Rc<Self>) -> std::rc::Weak<Self> {
        if std::rc::Weak::ptr_eq(&closure.canonical_origin, &std::rc::Weak::new()) {
            Rc::downgrade(closure)
        } else {
            closure.canonical_origin.clone()
        }
    }

    #[cfg(feature = "jit")]
    #[allow(
        clippy::single_call_fn,
        reason = "canonical closure identity belongs with closure storage"
    )]
    pub(crate) fn same_origin(
        left: &Rc<Self>,
        right: &Rc<Self>,
    ) -> bool {
        std::rc::Weak::ptr_eq(&Self::origin(left), &Self::origin(right))
    }

    /// How many parameters the closure takes.
    #[must_use]
    pub fn arity(&self) -> usize {
        match &self.body {
            | ClosureBody::Ast(_) => self.params.len(),
            | ClosureBody::Ir(function) => function.params.len(),
        }
    }
}

/// A function implemented in Rust, used by higher-order builtins such as
/// `diff`, which returns a closure that is not written in Aevia source.
pub struct NativeFn(
    pub  Box<
        dyn Fn(
            &mut crate::eval::Interp,
            Vec<Value>,
            aevia_syntax::Span,
        ) -> Result<Value, crate::eval::EvalError>,
    >,
);

/// An evaluator-local callback with an authenticated, immutable function schema.
#[derive(Debug)]
pub struct CallbackValue {
    pub(crate) function: Value,
    pub(crate) schema: aevia_types::Type,
}

impl fmt::Debug for NativeFn {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        f.write_str("<native fn>")
    }
}

/// An IO action. Building one has no effect; only the runtime executes it.
///
/// §6.8.1: an `IO a` is a description. Everything observable — printing, the
/// RNG, the clock, an assertion — happens in [`crate::eval::Interp::run_io`],
/// never while the description is being built.
#[derive(Debug)]
pub enum IoAction {
    /// A flat deferred source do program, replayed with fresh local bindings.
    Do(Rc<crate::io_do::DoBlock>),
    /// Produces a value without doing anything.
    Pure(Value),
    /// Writes text to stdout or stderr.
    Print {
        /// The text.
        text: String,
        /// Whether to append a newline.
        newline: bool,
        /// Whether to write to stderr.
        stderr: bool,
    },
    /// Reads a file as UTF-8.
    ReadFile(String),
    /// Writes a file.
    WriteFile {
        /// The path.
        path: String,
        /// The contents.
        contents: String,
    },
    /// Reads an NPY file into a Float tensor with the requested rank (§8.11).
    ///
    /// NPY is binary, so it does not fit [`Self::ReadFile`]'s UTF-8 text; the
    /// decode happens when the action runs, and a malformed file becomes the
    /// typed `Err` the spec asks for.
    ReadNpy {
        /// File to decode.
        path: String,
        /// Statically selected rank; checked against the decoded file.
        rank: u32,
    },
    /// Writes pre-encoded NPY bytes (§8.11).
    WriteNpy {
        /// The path.
        path: String,
        /// The encoded file.
        bytes: Vec<u8>,
    },
    /// Reads one line from stdin.
    ReadLine,
    /// Prints a prompt without a newline, flushes stdout, and reads one line from stdin.
    Input(String),
    /// Reads and validates an SVG file, returning its XML source.
    ReadSvg(String),
    /// Writes an SVG string to a file.
    WriteSvg {
        /// The path.
        path: String,
        /// The SVG contents.
        contents: String,
    },
    /// Reads and decodes an animated GIF file.
    ReadGif(String),
    /// Encodes and writes an animated GIF file.
    WriteGif {
        /// The path.
        path: String,
        /// The animation frames and metadata.
        animation: rssn::io::GifAnimation,
    },
    /// Loads a source plugin (§13.5.2) and publishes its registrations.
    LoadPlugin(String),
    /// Starts `thunk()` as a concurrent task (§9.7.3); produces a `Task`.
    Spawn(Value),
    /// Waits for a task and produces its result.
    Await(Rc<crate::parallel::TaskHandle>),
    /// Runs an action and passes its result to a function.
    Bind {
        /// The action.
        action: Rc<Self>,
        /// The continuation.
        func: Value,
    },
    /// Runs actions in order, keeping the last result.
    Seq(Vec<Rc<Self>>),
    /// Renders a plot specification to `plot.html`.
    Plot(String),
    /// Writes a plot to a named file.
    Save {
        /// The path.
        path: String,
        /// The HTML.
        html: String,
    },
    /// Calls a zero-argument closure and runs the action it produces.
    ///
    /// This is what makes `do let x = e; …` run `e` when the block runs rather
    /// than when it is built: a `let` in a `do` block is part of the
    /// description, not of its construction (§6.8.1).
    Defer(Rc<Closure>),
    /// Fails when executed (`assert`, `assert_eq`).
    Assert {
        /// The failure message.
        message: String,
    },
    /// Advances the runtime RNG and yields a `Float` (§6.10).
    Random,
    /// Reads the wall clock and yields a `Float[T]`.
    TimeNow,
    /// Runs an action with the session's capabilities narrowed to `allowed`
    /// (§9.14, `with_cap`). The previous set is restored when it finishes.
    WithCaps {
        /// The capability bits that stay granted.
        allowed: u8,
        /// The sandboxed action.
        action: Rc<Self>,
    },
}

/// The runtime capabilities an `IO` action can need (§9.14, §13.9.6).
pub mod caps {
    /// Reading and writing files (including plots and data files).
    pub const FS: u8 = 1;
    /// Network access.
    pub const NET: u8 = 2;
    /// The process environment.
    pub const ENV: u8 = 4;
    /// The session random-number generator.
    pub const RAND: u8 = 8;
    /// Accelerator devices.
    pub const GPU: u8 = 16;
    /// Every capability: what a program has unless it is sandboxed.
    pub const ALL: u8 = FS | NET | ENV | RAND | GPU;

    /// The names, in the order they are documented.
    pub const NAMES: &[(&str, u8)] = &[
        ("fs", FS),
        ("net", NET),
        ("env", ENV),
        ("rand", RAND),
        ("gpu", GPU),
    ];

    /// Parses a comma-separated capability list (`"fs,rand"`; `""` is none).
    ///
    /// # Errors
    ///
    /// The first name that is not a capability.
    pub fn parse(list: &str) -> Result<u8, String> {
        let mut bits = 0;
        for name in list
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            let Some((_, bit)) = NAMES.iter().find(|(known, _)| *known == name) else {
                return Err(name.to_string());
            };
            bits |= bit;
        }
        Ok(bits)
    }

    /// The name of one capability bit.
    #[must_use]
    pub fn name(bit: u8) -> &'static str {
        NAMES
            .iter()
            .find(|(_, known)| *known == bit)
            .map_or("?", |(name, _)| name)
    }
}

/// A symbolic proposition, produced by comparing `Real` values.
#[derive(Clone, Debug)]
pub struct PropValue {
    /// The proposition text, e.g. `x > 0`.
    pub text: String,
    /// The structured form, when the proposition compares two values.
    ///
    /// `decide` needs the operands, not a rendering of them: parsing the text
    /// back would lose exactness and would not survive a change of printing.
    pub comparison: Option<Comparison>,
}

/// A comparison a proposition was built from.
#[derive(Clone, Debug)]
pub struct Comparison {
    /// The relation.
    pub op: BinOp,
    /// The left operand.
    pub left: Value,
    /// The right operand.
    pub right: Value,
}

/// A kernel value retains the payload and independently established dimension evidence.
#[derive(Clone, Debug)]
pub struct KernelValue {
    /// The retained kernel value.
    pub payload: aevia_rssn::Val,
    /// Dimension information established independently of the payload.
    pub witness: aevia_rssn::witness::DimensionWitness,
    /// Aevia-side evidence exists only for a verified host-origin value.
    pub(crate) host_witness: Option<Rc<HostWitness>>,
}

/// Provenance issued only when the interpreter resolves a source constructor.
/// Its fields are private so host code cannot create an arbitrary proof.
#[derive(Clone, Debug)]
pub struct SourceCtorToken {
    pub(crate) adt: AdtId,
    pub(crate) index: usize,
    pub(crate) name: String,
    pub(crate) selected_result: Option<aevia_types::Type>,
}

/// A checked, host-origin schema with recursively authenticated variants.
/// This evidence is never attached to an arbitrary kernel result.
#[derive(Clone, Debug)]
pub(crate) struct HostWitness {
    pub(crate) schema: aevia_types::Type,
    pub(crate) node: HostWitnessNode,
}

#[derive(Clone, Debug)]
pub(crate) enum HostWitnessNode {
    Scalar,
    Variant {
        adt: AdtId,
        index: usize,
        tag: String,
        fields: Vec<HostWitness>,
    },
    Record(Vec<(String, HostWitness)>),
    Tensor(Vec<HostWitness>),
}

/// A value-level kernel error, retaining its original code and message.
#[derive(Clone, Debug)]
pub struct KernelErrorValue {
    /// The kernel's original error code.
    pub code: String,
    /// A human-readable description from the kernel.
    pub message: String,
}

/// A constructor, possibly partially applied (see [`Value::Ctor`]).
#[derive(Clone, Debug)]
pub struct CtorValue {
    /// The type.
    pub adt: AdtId,
    /// The constructor index.
    pub index: usize,
    /// The name.
    pub name: String,
    /// How many fields it takes.
    pub arity: usize,
    /// Fields collected so far.
    pub args: Vec<Value>,
    /// Provenance for constructors resolved from checked source.
    pub origin: Option<SourceCtorToken>,
}

impl Value {
    /// A constructor value.
    #[must_use]
    pub fn ctor(ctor: CtorValue) -> Self {
        Self::Ctor(Rc::new(ctor))
    }
}

/// A runtime value.
#[derive(Clone, Debug)]
pub enum Value {
    /// `()`.
    Unit,
    /// `true` / `false`.
    Bool(bool),
    /// A scalar.
    Num(NumVal),
    /// A string.
    Str(Rc<str>),
    /// A tensor.
    Tensor(Rc<TensorData>),
    /// A sparse tensor (§8.4.1): `Sparse<F, shape, τ>`.
    Sparse(Rc<crate::sparse::SparseData>),
    /// A record.
    Record(Rc<RecordValue>),
    /// A variant.
    Variant(Rc<VariantValue>),
    /// A function.
    Closure(Rc<Closure>),
    /// A Rust-implemented function.
    Native(Rc<NativeFn>),
    /// A constructor, possibly partially applied.
    ///
    /// Shared: builtins are constructor values too (`arity == usize::MAX`),
    /// so every reference to `map` or `mod` clones one.
    Ctor(Rc<CtorValue>),
    /// A unit value such as `m`.
    UnitOf(Rc<UnitDef>),
    /// An IO action.
    Io(Rc<IoAction>),
    /// A symbolic proposition.
    Prop(Rc<PropValue>),
    /// A value with a run-time dimension (§3.11).
    Dyn(Rc<DynValue>),
    /// A kernel ABI value and its separately tracked dimensional evidence.
    KernelValue(Rc<KernelValue>),
    /// A structured error returned through the kernel ABI.
    KernelError(Rc<KernelErrorValue>),
    /// Opaque resolved type evidence produced by `type_rep<T>()`.
    TypeRep(Rc<aevia_types::Type>),
    /// A checked evaluator-local callback for the kernel callback ABI.
    Callback(Rc<CallbackValue>),
    /// A concurrent task started by `spawn` (§9.7.3).
    Task(Rc<crate::parallel::TaskHandle>),
    /// A poisoned value produced after an error.
    Error,
}

impl Value {
    /// A float scalar.
    #[must_use]
    pub fn float(value: f64) -> Self {
        Self::Num(NumVal::float(value, Dim::one()))
    }

    /// An integer scalar.
    #[must_use]
    pub fn int(value: i64) -> Self {
        Self::Num(NumVal::int(value))
    }

    /// A string.
    #[must_use]
    pub fn string(value: impl Into<String>) -> Self {
        Self::Str(Rc::from(value.into()))
    }

    /// The scalar, when this is one.
    #[must_use]
    pub const fn as_num(&self) -> Option<&NumVal> {
        match self {
            | Self::Num(num) => Some(num),
            | _ => None,
        }
    }

    /// The `f64` value, when this is a numeric scalar.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        self.as_num().and_then(NumVal::to_f64)
    }

    /// True when the value is an IO action.
    #[must_use]
    pub const fn is_io(&self) -> bool {
        matches!(self, Self::Io(_))
    }

    /// The runtime type name, for diagnostics.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            | Self::Unit => "()",
            | Self::Bool(_) => "Bool",
            | Self::Num(num) => num.kind.type_name(),
            | Self::Str(_) => "String",
            | Self::Tensor(_) => "Tensor",
            | Self::Sparse(_) => "Sparse",
            | Self::Record(_) => "record",
            | Self::Variant(_) => "variant",
            | Self::Closure(_) | Self::Native(_) => "function",
            | Self::Ctor { .. } => "constructor",
            | Self::UnitOf(_) => "Unit",
            | Self::Io(_) => "IO",
            | Self::Prop(_) => "Prop",
            | Self::Dyn(_) => "Dyn",
            | Self::KernelValue(_) => "Value",
            | Self::KernelError(_) => "OpError",
            | Self::TypeRep(_) => "TypeRep",
            | Self::Callback(_) => "Callback",
            | Self::Task(_) => "Task",
            | Self::Error => "<error>",
        }
    }
}

/// Formats a float as the shortest text that round-trips to the same `f64`.
///
/// §6.12.3 asks for the shortest round-trip representation, so nothing is ever
/// truncated: `0.1 + 0.2` prints `0.30000000000000004`, because that is the
/// value the program computed. A shorter text would make `show` disagree with
/// `==`, which is how the exactness guarantee became unverifiable.
#[must_use]
pub fn format_float(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        // IEEE infinity is `inf` (§6.3.4, §8.1): `oo` names the *exact*
        // infinity of the `Real` phase, and printing a `Float` that way made
        // the two phases indistinguishable.
        return if value.is_sign_negative() {
            "-inf".to_string()
        } else {
            "inf".to_string()
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        };
    }
    let magnitude = value.abs();
    if magnitude >= 1e16 || magnitude < 1e-4 {
        // Rust's `{:e}` is also shortest-round-trip, and avoids 300 digits of
        // decimal expansion for `1e300`.
        return format!("{value:e}");
    }
    // `Display` for `f64` is the shortest text that parses back to `value`.
    // An integral `Float` keeps its decimal point (§6.12.3): `10.0` and the
    // exact `10` are different values in different phases, and `show` must
    // not make them look the same.
    let text = format!("{value}");
    if text.contains('.') {
        text
    } else {
        format!("{text}.0")
    }
}

/// Formats a float so that it reads back as a `Float` and not as an `Int`.
///
/// `repr` has to keep the phase visible (§6.12.3): `3` is an `Int`, `3/1` an
/// `Exact` and `3.0` a `Float`, and a text that renders all three the same way
/// cannot be parsed back to the value it came from.
#[must_use]
pub fn format_float_repr(value: f64) -> String {
    if value.is_infinite() {
        // There is no `inf` constant; this is the shortest source text that
        // evaluates to the same IEEE infinity in the `Float` phase.
        return if value.is_sign_negative() {
            "(-1.0 / 0.0)".to_string()
        } else {
            "(1.0 / 0.0)".to_string()
        };
    }
    let text = format_float(value);
    if text == "nan" || text.contains('.') || text.contains('e') {
        text
    } else {
        format!("{text}.0")
    }
}

/// The SI spelling of a dimension, used when no display unit is attached.
///
/// The spelling itself lives in `aevia-dim` so diagnostics and value display
/// agree: a Brownian increment's `s^(1/2)` must not render as `s^1` (§8.9.1).
#[must_use]
pub fn si_unit_text(dim: &Dim) -> String {
    dim.to_display_si_string()
}

/// Renders a scalar with its display unit or dimension.
///
/// `show` is human-readable (§6.12.3): a terminating exact rational reads as a
/// decimal, a non-terminating one as `n/d`, and a float as its shortest
/// round-trip text. Exactness is never hidden by rounding — that is what made
/// `0.1 + 0.2` print `0.3` while `0.1 + 0.2 == 0.3` was `false`.
#[must_use]
pub fn format_scalar(num: &NumVal) -> String {
    format_scalar_in(num, false)
}

/// Renders a scalar so that the text names the same value and the same phase.
///
/// An `Exact` is a fraction, a `Float` keeps a decimal point, and a symbolic
/// term keeps its internal symbol identity (`x#2`), which is what makes the
/// text parse back to the value (§6.12.3).
#[must_use]
pub fn format_scalar_repr(num: &NumVal) -> String {
    format_scalar_in(num, true)
}

fn format_scalar_in(
    num: &NumVal,
    repr: bool,
) -> String {
    let unit = num.display.as_deref();
    let (text, unit) = match &num.kind {
        | NumKind::Int(value) if unit.is_some() => display_exact(&value.to_rational(), unit, repr),
        | NumKind::Int(value) => (value.to_string(), None),
        | NumKind::Exact(value) => display_exact(value, unit, repr),
        | NumKind::Float(value) => {
            let shown = unit.map_or(*value, |unit| float_in_display_unit(*value, unit));
            (
                if repr {
                    format_float_repr(shown)
                } else {
                    format_float(shown)
                },
                unit,
            )
        },
        | NumKind::Complex(re, im) => {
            let sign = if *im < 0.0 { "-" } else { "+" };
            (
                format!("{} {sign} {}*i", format_float(*re), format_float(im.abs())),
                unit,
            )
        },
        | NumKind::Sym(term) => {
            (
                if repr {
                    term.to_string_repr()
                } else {
                    term.to_string()
                },
                unit,
            )
        },
    };
    let suffix = unit.map_or_else(|| si_unit_text(&num.dim), |unit| unit.display.clone());
    if suffix.is_empty() {
        text
    } else {
        format!("{text} {suffix}")
    }
}

/// A coherent `Float` expressed in a display unit. Choose the shorter of IEEE
/// conversion and exact rational conversion rounded once. The two paths
/// preserve both a literal's spelling (25 °C) and a computed point (300 K).
#[allow(
    clippy::single_call_fn,
    reason = "isolates the floating-point display rounding policy"
)]
fn float_in_display_unit(
    value: f64,
    unit: &UnitDef,
) -> f64 {
    let offset = unit.offset.as_ref().map_or(0.0, aevia_dim::Scale::to_f64);
    let scale = unit.scale.to_f64();
    let ieee = if scale == 0.0 {
        value
    } else {
        (value - offset) / scale
    };
    if let Some(rounded) = BigRational::from_float(value)
        .and_then(|exact| rational_in_display_unit(&exact, unit))
        .map(|exact| aevia_dim::rat::to_f64(&exact))
        && format_float(rounded).len() < format_float(ieee).len()
    {
        return rounded;
    }
    ieee
}

fn rational_in_display_unit(
    value: &BigRational,
    unit: &UnitDef,
) -> Option<BigRational> {
    use num_traits::CheckedDiv;
    use num_traits::CheckedSub;

    let scale = &unit.scale;
    if scale.pi != 0 || scale.rat.denom().is_zero() || value.denom().is_zero() {
        return None;
    }
    match &unit.offset {
        | None => value.checked_div(&scale.rat),
        | Some(offset) if offset.pi == 0 && !offset.rat.denom().is_zero() => {
            value.checked_sub(&offset.rat)?.checked_div(&scale.rat)
        },
        | Some(_) => None,
    }
}

/// Converts an exact coefficient when the display unit can represent it
/// rationally. Otherwise keep its coherent value and coherent unit together.
fn display_exact<'a>(
    value: &BigRational,
    unit: Option<&'a UnitDef>,
    repr: bool,
) -> (String, Option<&'a UnitDef>) {
    let shown = unit.and_then(|unit| rational_in_display_unit(value, unit));
    let shown_unit = unit.filter(|_| shown.is_some());
    let coefficient = shown.as_ref().unwrap_or(value);
    let text = if repr {
        aevia_dim::rat::format_rational(coefficient)
    } else {
        aevia_dim::format_rational_decimal(coefficient)
    };
    (text, shown_unit)
}

/// Renders a value for `show` and string interpolation.
///
/// Human-readable (§6.12.3): strings are bare, constructors are `Name(payload)`
/// and floats are their shortest round-trip text. It is *not* round-trippable —
/// [`repr`] is.
#[must_use]
pub fn show(value: &Value) -> String {
    render(value, false)
}

/// Renders a value so that the text parses back to the same value (§6.12.3).
///
/// The difference from [`show`] is phase and structure: `3.0` is a `Float`,
/// `3/2` an `Exact`, `"3"` a `String`, and a symbolic term keeps the internal
/// identity of its symbols.
#[must_use]
pub fn repr(value: &Value) -> String {
    render(value, true)
}

/// Limits recursive rendering of values and tensor axes. This also covers
/// host-created boxed tensors whose elements are themselves tensors.
const MAX_RENDER_DEPTH: usize = 256;

/// A virtual tensor can name many more positions than its physical buffer
/// contains. Bound those generated positions separately from stored data.
const MAX_VIRTUAL_RENDER_ITEMS: usize = 65_536;

#[allow(
    clippy::single_call_fn,
    reason = "checks the runtime's consecutive tuple-label convention"
)]
fn is_tuple_labels<'a>(mut labels: impl Iterator<Item = &'a str>) -> bool {
    matches!((labels.next(), labels.next()), (Some("0"), Some("1")))
        && (2usize..)
            .zip(labels)
            .all(|(index, label)| label == index.to_string())
}

fn render_sequence<'a>(
    items: impl IntoIterator<Item = &'a Value>,
    repr: bool,
    depth: usize,
    start: &str,
    end: &str,
) -> String {
    let mut out = start.to_owned();
    for (index, item) in items.into_iter().enumerate() {
        if index != 0 {
            out.push_str(", ");
        }
        out.push_str(&render_at(item, repr, depth));
    }
    out.push_str(end);
    out
}

fn render(
    value: &Value,
    repr: bool,
) -> String {
    render_at(value, repr, 0)
}

fn render_at(
    value: &Value,
    repr: bool,
    depth: usize,
) -> String {
    if depth > MAX_RENDER_DEPTH {
        return "...".to_string();
    }
    let next_depth = depth.saturating_add(1);
    match value {
        | Value::Unit => "()".to_string(),
        | Value::Bool(flag) => flag.to_string(),
        | Value::Num(num) => format_scalar_in(num, repr),
        // A string on its own shows as its text; inside a container it is
        // quoted, so `Err("not found")`, `["a", "b"]` and `{ name = "x" }`
        // say where each string starts and ends (`["a, b"]` is one element).
        | Value::Str(text) => {
            if repr || depth > 0 {
                quote(text)
            } else {
                text.to_string()
            }
        },
        | Value::Tensor(tensor) => show_tensor_in(tensor, repr, depth),
        | Value::Sparse(sparse) => crate::sparse::render(sparse, repr),
        | Value::Record(record) => {
            // Tuples are records labelled `0, 1, …` (deviation 5); they are
            // written `(a, b)`, so they print that way too.
            if is_tuple_labels(record.fields.keys().map(String::as_str)) {
                return render_sequence(record.fields.values(), repr, next_depth, "(", ")");
            }
            let mut out = String::from("{ ");
            for (index, (name, value)) in record.fields.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                let _ = write!(out, "{name} = {}", render_at(value, repr, next_depth));
            }
            out.push_str(" }");
            out
        },
        | Value::Variant(variant) if variant.adt == aevia_types::adt::set_id() => {
            // §8.1.9: a set shows as `{a, b, c}` in its canonical order.
            let items = match variant.fields.first() {
                | Some(Value::Tensor(tensor)) if renderable_tensor_shape(tensor) => {
                    tensor.elements()
                },
                | Some(Value::Tensor(_)) => return "...".to_string(),
                | _ => Vec::new(),
            };
            let (start, end) = if repr {
                ("set_of([", "])")
            } else {
                ("{", "}")
            };
            render_sequence(&items, repr, next_depth, start, end)
        },
        | Value::Variant(variant) if variant.fields.is_empty() => variant.name.clone(),
        | Value::Variant(variant) => {
            render_sequence(
                &variant.fields,
                repr,
                next_depth,
                &format!("{}(", variant.name),
                ")",
            )
        },
        | Value::Closure(_) | Value::Native(_) => "<fn>".to_string(),
        | Value::Ctor(ctor) => {
            let CtorValue { name, args, .. } = &**ctor;
            if args.is_empty() {
                name.clone()
            } else {
                render_sequence(args, repr, next_depth, &format!("{name}("), ")")
            }
        },
        | Value::UnitOf(unit) => unit.display.clone(),
        | Value::Io(_) => "<io>".to_string(),
        | Value::Prop(prop) => prop.text.clone(),
        // §4.9.4: a `Dyn` prints as the scalar it carries, in the unit it was
        // written with, so `show(dyn(3 m))` names the same quantity.
        | Value::Dyn(dynamic) => format_scalar_in(&dynamic.as_scalar(), repr),
        // A kernel value is not reconstructible from text, so `repr` keeps
        // the opaque marker; `show` renders what it carries (a number, a term
        // in the kernel's syntax) and its dimension when the kernel or the
        // host established one, so `Rssn.dynamic(..)` results are readable.
        | Value::KernelValue(_) if repr => "<Value>".to_string(),
        | Value::KernelValue(kernel) => {
            let text = match &kernel.payload {
                | aevia_rssn::Val::Term(handle) => {
                    aevia_rssn::kernel()
                        .try_text(*handle)
                        .unwrap_or_else(|_| kernel.payload.text())
                },
                | other => other.text(),
            };
            match &kernel.witness {
                | aevia_rssn::witness::DimensionWitness::Scalar(dim) if !dim.is_one() => {
                    format!("{text} [{dim}]")
                },
                | _ => text,
            }
        },
        | Value::KernelError(error) => format!("{}: {}", error.code, error.message),
        | Value::TypeRep(_) => "<TypeRep>".to_string(),
        | Value::Callback(_) => "<Callback>".to_string(),
        | Value::Task(_) => "<Task>".to_string(),
        | Value::Error => "<error>".to_string(),
    }
}

/// Quotes a string as Aevia source does.
#[allow(
    clippy::single_call_fn,
    reason = "keeps Aevia source escaping in one formatting routine"
)]
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len().saturating_add(2));
    out.push('"');
    for character in text.chars() {
        match character {
            | '"' => out.push_str("\\\""),
            | '\\' => out.push_str("\\\\"),
            | '\n' => out.push_str("\\n"),
            | '\r' => out.push_str("\\r"),
            | '\t' => out.push_str("\\t"),
            | other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Check backing length and bound output for shapes with synthetic positions:
/// empty grids with large rows, and host views which repeat a small buffer.
fn renderable_tensor_shape(tensor: &TensorData) -> bool {
    let Some(count) = tensor.checked_element_count() else {
        return false;
    };
    if count != tensor.storage.len() {
        return false;
    }
    if count != 0 {
        return match tensor.storage.backing() {
            | Storage::View(view) if count > view.data.len() => count <= MAX_VIRTUAL_RENDER_ITEMS,
            | _ => true,
        };
    }
    tensor
        .shape
        .iter()
        .take_while(|extent| **extent != 0)
        .try_fold(1usize, |prefix, extent| {
            prefix
                .checked_mul(*extent)
                .filter(|count| *count <= MAX_VIRTUAL_RENDER_ITEMS)
        })
        .is_some()
}

#[allow(
    clippy::single_call_fn,
    reason = "contains the shape validation and depth-aware tensor renderer"
)]
fn show_tensor_in(
    tensor: &TensorData,
    repr: bool,
    depth: usize,
) -> String {
    fn write_axes(
        tensor: &TensorData,
        axes: &[usize],
        index: &mut usize,
        out: &mut String,
        repr: bool,
        depth: usize,
    ) {
        let Some((&extent, tail)) = axes.split_first() else {
            return;
        };
        let next_depth = depth.saturating_add(1);
        out.push('[');
        for cell in 0..extent {
            if cell != 0 {
                out.push_str(", ");
            }
            if tail.is_empty() {
                if let Some(value) = tensor.get_flat(*index) {
                    out.push_str(&render_at(&value, repr, next_depth));
                } else {
                    out.push('?');
                }
                *index = index.saturating_add(1);
            } else {
                write_axes(tensor, tail, index, out, repr, next_depth);
            }
        }
        out.push(']');
    }

    if !renderable_tensor_shape(tensor) || depth.saturating_add(tensor.rank()) > MAX_RENDER_DEPTH {
        return "...".to_string();
    }
    if tensor.shape.is_empty() {
        return match tensor.get_flat(0) {
            | Some(Value::Str(text)) if !repr => text.to_string(),
            | Some(value) => render_at(&value, repr, depth.saturating_add(1)),
            | None => "[]".to_string(),
        };
    }
    let mut result = String::new();
    write_axes(tensor, &tensor.shape, &mut 0, &mut result, repr, depth);
    result
}

/// Which mathematical typesetting a value is rendered in.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MathSyntax {
    /// LaTeX, for `to_latex` (§6.12.3).
    Latex,
    /// Typst, for `to_typst` (§6.12.3).
    Typst,
}

/// Renders a value in LaTeX or Typst (§6.12.3).
///
/// `to_latex` and `to_typst` are not aliases of `show`: they emit the target
/// syntax, and a value that has no mathematical reading (a function, an IO
/// action) is an error rather than a silently different string.
///
/// # Errors
///
/// `E1001` naming the capability when the value has no typeset form.
pub fn to_math(
    value: &Value,
    syntax: MathSyntax,
) -> Result<String, String> {
    math_at(value, syntax, 0)
}

fn math_at(
    value: &Value,
    syntax: MathSyntax,
    depth: usize,
) -> Result<String, String> {
    if depth > MAX_RENDER_DEPTH {
        return Err("the mathematical value is nested too deeply".into());
    }
    let next_depth = depth.saturating_add(1);
    let latex = syntax == MathSyntax::Latex;
    match value {
        | Value::Num(num) => math_scalar(num, syntax),
        | Value::Str(text) if latex => Ok(format!("\\text{{{}}}", escape_latex_text(text))),
        | Value::Str(text) => Ok(quote_typst_text(text)),
        | Value::Bool(flag) => {
            Ok(match (latex, *flag) {
                | (true, true) => "\\mathrm{true}".to_string(),
                | (true, false) => "\\mathrm{false}".to_string(),
                | (false, true) => "true".to_string(),
                | (false, false) => "false".to_string(),
            })
        },
        | Value::Unit => Ok("()".to_string()),
        | Value::Tensor(tensor) => math_tensor(tensor, syntax, next_depth),
        | Value::Record(record) => math_record(record, syntax, next_depth),
        | Value::Variant(variant) if variant.fields.is_empty() => {
            Ok(if latex {
                format!("\\mathrm{{{}}}", escape_latex_text(&variant.name))
            } else {
                quote_typst_text(&variant.name)
            })
        },
        | Value::Variant(_) => Err("variant payloads have no mathematical form".to_string()),
        | Value::Closure(_) | Value::Native(_) => {
            Err("a function has no mathematical form".to_string())
        },
        | Value::Ctor { .. } => Err("a constructor has no mathematical form".to_string()),
        | Value::UnitOf(unit) => {
            Ok(if latex {
                format!("\\mathrm{{{}}}", escape_latex_text(&unit.display))
            } else {
                format!("upright({})", quote_typst_text(&unit.display))
            })
        },
        | Value::Sparse(_) => {
            Err("a sparse tensor has no mathematical form; use `to_dense`".to_string())
        },
        | Value::Io(_) => Err("an IO action has no mathematical form".to_string()),
        | Value::Prop(prop) => math_relation(prop, syntax, next_depth),
        | Value::Dyn(dynamic) => math_scalar(&dynamic.as_scalar(), syntax),
        | Value::KernelValue(payload) => {
            let kernel = aevia_rssn::kernel();
            (if latex {
                kernel.to_latex(&payload.payload)
            } else {
                kernel.to_typst(&payload.payload)
            })
            .map_err(|error| error.message)
        },
        | Value::KernelError(_) | Value::TypeRep(_) | Value::Callback(_) => {
            Err("an opaque kernel value has no mathematical form".to_string())
        },
        | Value::Task(_) => Err("a task has no mathematical form".to_string()),
        | Value::Error => Err("an error value has no mathematical form".to_string()),
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "record rows use target-specific formatting"
)]
fn math_record(
    record: &RecordValue,
    syntax: MathSyntax,
    depth: usize,
) -> Result<String, String> {
    let latex = syntax == MathSyntax::Latex;
    let mut out = if latex {
        String::from("\\begin{cases}")
    } else {
        String::from("cases(")
    };
    for (index, (name, value)) in record.fields.iter().enumerate() {
        if index > 0 {
            out.push_str(if latex { " \\\\ " } else { ", " });
        }
        if latex {
            let _ = write!(
                out,
                "\\mathrm{{{}}} &= {}",
                escape_latex_text(name),
                math_at(value, syntax, depth)?
            );
        } else {
            let _ = write!(
                out,
                "{} &= {}",
                quote_typst_text(name),
                math_at(value, syntax, depth)?
            );
        }
    }
    out.push_str(if latex { "\\end{cases}" } else { ")" });
    Ok(out)
}

#[allow(
    clippy::single_call_fn,
    reason = "relation spellings stay together for both typesetting targets"
)]
fn math_relation(
    prop: &PropValue,
    syntax: MathSyntax,
    depth: usize,
) -> Result<String, String> {
    let latex = syntax == MathSyntax::Latex;
    if let Some(comparison) = &prop.comparison {
        let relation = match (syntax, comparison.op) {
            | (_, BinOp::Eq) => "=",
            | (MathSyntax::Latex, BinOp::Ne) => "\\ne",
            | (MathSyntax::Latex, BinOp::Le) => "\\le",
            | (MathSyntax::Latex, BinOp::Ge) => "\\ge",
            | (_, BinOp::Lt) => "<",
            | (_, BinOp::Gt) => ">",
            | (MathSyntax::Typst, BinOp::Ne) => "!=",
            | (MathSyntax::Typst, BinOp::Le) => "<=",
            | (MathSyntax::Typst, BinOp::Ge) => ">=",
            | _ => return Err("this proposition has no mathematical relation".into()),
        };
        Ok(format!(
            "{} {relation} {}",
            math_at(&comparison.left, syntax, depth)?,
            math_at(&comparison.right, syntax, depth)?
        ))
    } else if latex {
        Ok(format!("\\text{{{}}}", escape_latex_text(&prop.text)))
    } else {
        Ok(quote_typst_text(&prop.text))
    }
}
fn math_scalar(
    num: &NumVal,
    syntax: MathSyntax,
) -> Result<String, String> {
    let body = match &num.kind {
        | NumKind::Sym(term) => {
            crate::sym::term_to_math_checked(term, syntax == MathSyntax::Latex)?
        },
        | NumKind::Int(value) => value.to_string(),
        | NumKind::Exact(value) => {
            if value.denom().is_one() {
                value.numer().to_string()
            } else if syntax == MathSyntax::Latex {
                format!("\\frac{{{}}}{{{}}}", value.numer(), value.denom())
            } else {
                format!("frac({}, {})", value.numer(), value.denom())
            }
        },
        | NumKind::Float(value) => math_float(*value, syntax),
        | NumKind::Complex(re, im) => {
            let sign = if *im < 0.0 { "-" } else { "+" };
            format!(
                "{} {sign} {} i",
                math_float(*re, syntax),
                math_float(im.abs(), syntax)
            )
        },
    };
    let suffix = si_unit_text(&num.dim);
    Ok(if suffix.is_empty() {
        body
    } else if syntax == MathSyntax::Latex {
        format!("{body}\\ \\mathrm{{{}}}", escape_latex_text(&suffix))
    } else {
        format!("{body} thin upright({})", quote_typst_text(&suffix))
    })
}

/// Numeric mathematical output uses the target's scientific notation and
/// non-finite constants rather than source-language exponent identifiers.
fn math_float(
    value: f64,
    syntax: MathSyntax,
) -> String {
    let latex = syntax == MathSyntax::Latex;
    if value.is_nan() {
        return if latex {
            r"\mathrm{NaN}".into()
        } else {
            "upright(\"NaN\")".into()
        };
    }
    if value.is_infinite() {
        let sign = if value.is_sign_negative() {
            "-"
        } else {
            ""
        };
        return format!("{sign}{}", if latex { r"\infty" } else { "oo" });
    }
    let text = format_float(value);
    if let Some((mantissa, exponent)) = text.split_once('e') {
        if latex {
            format!(r"{mantissa} \times 10^{{{exponent}}}")
        } else {
            format!("{mantissa} times 10^({exponent})")
        }
    } else {
        text
    }
}

/// Escapes literal text used inside LaTeX text/roman/operator labels.
pub(crate) fn escape_latex_text(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            | '\\' => out.push_str(r"\textbackslash{}"),
            | '{' => out.push_str(r"\{"),
            | '}' => out.push_str(r"\}"),
            | '$' => out.push_str(r"\$"),
            | '&' => out.push_str(r"\&"),
            | '#' => out.push_str(r"\#"),
            | '%' => out.push_str(r"\%"),
            | '_' => out.push_str(r"\_"),
            | '^' => out.push_str(r"\textasciicircum{}"),
            | '~' => out.push_str(r"\textasciitilde{}"),
            | other => out.push(other),
        }
    }
    out
}

/// A quoted Typst math text literal. It never enters Typst markup mode.
pub(crate) fn quote_typst_text(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            | '\\' => out.push_str(r"\\"),
            | '"' => out.push_str("\\\""),
            | '\n' => out.push_str(r"\n"),
            | '\r' => out.push_str(r"\r"),
            | '\t' => out.push_str(r"\t"),
            | other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[allow(
    clippy::single_call_fn,
    reason = "centralizes checked matrix traversal for both typesetting targets"
)]
fn math_tensor(
    tensor: &TensorData,
    syntax: MathSyntax,
    depth: usize,
) -> Result<String, String> {
    if !renderable_tensor_shape(tensor) {
        return Err("tensor shape or backing storage is too large or inconsistent".into());
    }
    let (rows, cols) = match tensor.shape.as_slice() {
        | [] => {
            let cell = tensor
                .get_flat(0)
                .ok_or_else(|| "scalar tensor has no element".to_string())?;
            return math_at(&cell, syntax, depth);
        },
        | &[cols] => (1, cols),
        | &[rows, cols] => (rows, cols),
        | _ => return Err("only rank 1 and 2 tensors have a matrix form".to_string()),
    };
    let (open, close, row_separator, cell_separator) = if syntax == MathSyntax::Latex {
        (r"\begin{pmatrix}", r"\end{pmatrix}", r" \\ ", " & ")
    } else {
        ("mat(", ")", "; ", ", ")
    };
    let mut out = open.to_owned();
    let mut index = 0usize;
    for row in 0..rows {
        if row != 0 {
            out.push_str(row_separator);
        }
        for column in 0..cols {
            if column != 0 {
                out.push_str(cell_separator);
            }
            let cell = tensor
                .get_flat(index)
                .ok_or_else(|| "tensor index out of range".to_string())?;
            out.push_str(&math_at(&cell, syntax, depth)?);
            index = index
                .checked_add(1)
                .ok_or_else(|| "tensor index overflow".to_string())?;
        }
    }
    out.push_str(close);
    Ok(out)
}

/// Why two values are not structurally equal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotEqual {
    /// The values have different runtime types.
    TypeMismatch(&'static str, &'static str),
    /// One of them has no `Eq` instance (§3.8.4).
    NoEq(&'static str),
    /// A poisoned value from an earlier error.
    Poisoned,
}

impl fmt::Display for NotEqual {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            | Self::TypeMismatch(left, right) => write!(f, "cannot compare {left} with {right}"),
            | Self::NoEq(name) => write!(f, "{name} has no `Eq` instance"),
            | Self::Poisoned => f.write_str("a poisoned value reached an equality test"),
        }
    }
}

/// Structural equality (§6.12.1).
///
/// Payloads are compared recursively — never their renderings — because a
/// rendering is a lossy projection: `Some(1) == Some(2)` and
/// `{f = fn a => a} == {f = fn b => b}` were both `true` when the comparison
/// looked at tags and text. Functions have no `Eq`, so comparing them is an
/// error rather than `false`.
///
/// # Errors
///
/// [`NotEqual`] when the values are not comparable.
pub fn equal(
    left: &Value,
    right: &Value,
) -> Result<bool, NotEqual> {
    match (left, right) {
        | (Value::Unit, Value::Unit) => Ok(true),
        | (Value::Bool(a), Value::Bool(b)) => Ok(a == b),
        | (Value::Str(a), Value::Str(b)) => Ok(a == b),
        | (Value::Num(a), Value::Num(b)) => Ok(numbers_equal(a, b)),
        | (Value::Tensor(a), Value::Tensor(b)) => tensors_equal(a, b),
        | (Value::Sparse(a), Value::Sparse(b)) => Ok(crate::sparse::equal_data(a, b)),
        | (Value::Record(a), Value::Record(b)) => {
            if a.fields.len() != b.fields.len() {
                return Ok(false);
            }
            for (name, value) in &a.fields {
                let Some(other) = b.fields.get(name) else {
                    return Ok(false);
                };
                if !equal(value, other)? {
                    return Ok(false);
                }
            }
            Ok(true)
        },
        | (Value::Variant(a), Value::Variant(b)) => {
            if a.adt != b.adt || a.index != b.index || a.fields.len() != b.fields.len() {
                return Ok(false);
            }
            for (x, y) in a.fields.iter().zip(b.fields.iter()) {
                if !equal(x, y)? {
                    return Ok(false);
                }
            }
            Ok(true)
        },
        | (Value::UnitOf(a), Value::UnitOf(b)) => Ok(a.name == b.name),
        | (Value::Prop(a), Value::Prop(b)) => Ok(a.text == b.text),
        // §4.9.4: two `Dyn`s are equal when they carry the same dimension and
        // the same payload.
        | (Value::Dyn(a), Value::Dyn(b)) => {
            Ok(a.dim == b.dim && equal(&Value::Num(a.payload()), &Value::Num(b.payload()))?)
        },
        | (Value::Error, _) | (_, Value::Error) => Err(NotEqual::Poisoned),
        | (Value::Closure(_) | Value::Native(_), _) | (_, Value::Closure(_) | Value::Native(_)) => {
            Err(NotEqual::NoEq("a function"))
        },
        | (Value::Ctor { .. }, _) | (_, Value::Ctor { .. }) => Err(NotEqual::NoEq("a constructor")),
        | (Value::Io(_), _) | (_, Value::Io(_)) => Err(NotEqual::NoEq("an IO action")),
        | (a, b) => Err(NotEqual::TypeMismatch(a.type_name(), b.type_name())),
    }
}

/// Numeric equality: exact values compare as rationals, floats by IEEE.
///
/// A symbolic operand is not decidable here; [`crate::eval::Interp::compare`]
/// turns that case into a `Prop` before reaching this function.
#[allow(
    clippy::float_cmp,
    clippy::single_call_fn,
    reason = "structural equality uses IEEE equality and exact rational comparison"
)]
fn numbers_equal(
    left: &NumVal,
    right: &NumVal,
) -> bool {
    match (&left.kind, &right.kind) {
        | (NumKind::Complex(ar, ai), NumKind::Complex(br, bi)) => ar == br && ai == bi,
        | (NumKind::Sym(_) | NumKind::Complex(..), _)
        | (_, NumKind::Sym(_) | NumKind::Complex(..)) => false,
        | (NumKind::Float(a), NumKind::Float(b)) => a == b,
        | (NumKind::Int(a), NumKind::Int(b)) => a == b,
        // An integer of magnitude at most 2^53 is exactly an `f64`, so the
        // float comparison is the exact one (and NaN/infinity compare
        // unequal, as their missing rational does below).
        | (NumKind::Float(float), NumKind::Int(integer @ crate::int::Int::Small(small)))
        | (NumKind::Int(integer @ crate::int::Int::Small(small)), NumKind::Float(float))
            if small.unsigned_abs() <= 1 << 53 =>
        {
            *float == integer.to_f64()
        },
        | _ => {
            match (left.to_rational(), right.to_rational()) {
                | (Some(a), Some(b)) => a == b,
                | _ => false,
            }
        },
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "validates the tensor-specific shape and backing-storage contract"
)]
fn tensors_equal(
    left: &TensorData,
    right: &TensorData,
) -> Result<bool, NotEqual> {
    let Some(len) = left.checked_element_count() else {
        return Ok(false);
    };
    if left.shape != right.shape || left.storage.len() != len || right.storage.len() != len {
        return Ok(false);
    }
    if let (Some(a), Some(b)) = (left.dense(), right.dense()) {
        return Ok(a == b);
    }
    for index in 0..len {
        let (Some(a), Some(b)) = (left.get_flat(index), right.get_flat(index)) else {
            return Ok(false);
        };
        if !equal(&a, &b)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Builds a variant value.
#[must_use]
pub fn make_variant(
    adt: AdtId,
    index: usize,
    name: &str,
    fields: Vec<Value>,
) -> Value {
    Value::Variant(Rc::new(VariantValue {
        adt,
        index,
        name: name.to_string(),
        fields,
        type_args: None,
        verified_type: None,
        constructor_origin: false,
    }))
}

/// Mark only a constructor freshly resolved from a checked source interface.
/// Public host-created variants and `make_variant` never receive this proof.
#[allow(
    clippy::single_call_fn,
    reason = "validates provenance at the source resolver's trust boundary"
)]
pub(crate) fn mark_source_ctor(value: &mut Value) {
    match value {
        | Value::Ctor(ctor) => {
            let valid = ctor.args.is_empty()
                && aevia_types::adt::get(ctor.adt)
                    .and_then(|entry| entry.ctors.get(ctor.index).cloned())
                    .is_some_and(|c| c.name == ctor.name && c.fields.len() == ctor.arity);
            if valid {
                let ctor = Rc::make_mut(ctor);
                ctor.origin = Some(SourceCtorToken {
                    adt: ctor.adt,
                    index: ctor.index,
                    name: ctor.name.clone(),
                    selected_result: None,
                });
            }
        },
        | Value::Variant(variant) if variant.fields.is_empty() => {
            let valid = aevia_types::adt::get(variant.adt)
                .and_then(|entry| entry.ctors.get(variant.index).cloned())
                .is_some_and(|ctor| ctor.name == variant.name && ctor.fields.is_empty());
            if valid {
                Rc::make_mut(variant).constructor_origin = true;
            }
        },
        | _ => {},
    }
}

/// Retain constructor provenance after applying a source-resolved constructor.
#[allow(
    clippy::single_call_fn,
    reason = "validates provenance at the constructor application boundary"
)]
pub(crate) fn mark_ctor_result(
    value: &mut Value,
    origin: &SourceCtorToken,
) {
    if let Value::Variant(variant) = value
        && variant.adt == origin.adt
        && variant.index == origin.index
    {
        let valid = aevia_types::adt::get(variant.adt)
            .and_then(|entry| entry.ctors.get(variant.index).cloned())
            .is_some_and(|ctor| {
                ctor.name == variant.name && ctor.fields.len() == variant.fields.len()
            });
        if valid {
            Rc::make_mut(variant).constructor_origin = true;
        }
    }
}

/// The constructor name for an id/index pair.
#[must_use]
pub fn ctor_name(
    adt: AdtId,
    index: usize,
) -> Option<String> {
    adt::get(adt).and_then(|registered| registered.ctors.get(index).map(|ctor| ctor.name.clone()))
}

/// The dimension of a value, when it has one.
#[must_use]
pub fn value_dim(value: &Value) -> Dim {
    match value {
        | Value::Num(num) => num.dim.clone(),
        | Value::Tensor(tensor) => tensor.dim.clone(),
        | Value::Sparse(sparse) => sparse.dim.clone(),
        | Value::Dyn(dynamic) => dynamic.dim.clone(),
        | _ => Dim::one(),
    }
}

/// A helper used by tests and the REPL to print a value with a dimension
/// environment.
#[must_use]
pub fn show_with(
    value: &Value,
    _dims: &DimEnv,
) -> String {
    show(value)
}

/// Whether a payload is a well-formed native AD handle envelope.
/// RSSN separately authenticates its session token and retained object.
pub(crate) fn is_native_ad_var_payload(payload: &aevia_rssn::Val) -> bool {
    matches!(payload, aevia_rssn::Val::Opaque {kind,bytes} if kind == aevia_rssn::registry::NATIVE_AD_VAR_KIND && bytes.len() == 16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigInt;

    #[test]
    fn rendering_bounds_virtual_and_inconsistent_host_shapes() {
        let huge_empty = tensor(vec![usize::MAX, 0], Vec::new());
        assert_eq!(show(&huge_empty), "...");
        assert!(to_math(&huge_empty, MathSyntax::Latex).is_err());
        let huge_product = tensor(vec![usize::MAX, 2], Vec::new());
        assert_eq!(show(&huge_product), "...");
        assert!(to_math(&huge_product, MathSyntax::Typst).is_err());
        assert_eq!(TensorData::element_count(&[usize::MAX, 2]), usize::MAX);
        assert_eq!(TensorData::element_count(&[usize::MAX, 2, 0]), 0);
        let ordinary_empty = tensor(vec![2, 0], Vec::new());
        assert_eq!(show(&ordinary_empty), "[[], []]");
        assert_eq!(
            to_math(&ordinary_empty, MathSyntax::Latex).as_deref(),
            Ok(r"\begin{pmatrix} \\ \end{pmatrix}"),
        );
    }

    #[test]
    fn boxed_tensor_elements_share_the_render_depth_budget() {
        let mut value = Value::int(1);
        for _ in 0..=MAX_RENDER_DEPTH {
            value = Value::Tensor(Rc::new(TensorData::new(
                Vec::new(),
                Storage::Boxed(vec![value]),
                Dim::one(),
                None,
            )));
        }
        assert_eq!(show(&value), "...");
        assert!(to_math(&value, MathSyntax::Latex).is_err());
    }

    #[test]
    fn unrepresentable_exact_display_keeps_the_coherent_unit() {
        let dim = Dim::base("Length");
        let unit = UnitDef::linear(
            "pi_length",
            dim.clone(),
            aevia_dim::Scale {
                rat: BigRational::from_integer(1.into()),
                pi: 1,
            },
        );
        let val =
            NumVal::exact(BigRational::from_integer(3.into()), dim).with_display(Rc::new(unit));
        assert_eq!(format_scalar(&val), "3 m");
        assert_eq!(format_scalar_repr(&val), "3 m");
    }

    #[test]
    fn equality_rejects_malformed_tensor_storage_in_both_orders() {
        let short = tensor(vec![2], vec![1.0]);
        let complete = tensor(vec![2], vec![1.0, 2.0]);
        assert_eq!(equal(&short, &complete), Ok(false));
        assert_eq!(equal(&complete, &short), Ok(false));
    }

    #[test]
    fn floats_print_compactly() {
        assert_eq!(format_float(2.0), "2.0");
        assert_eq!(format_float(-0.0), "-0.0");
        assert_eq!(format_float(1e300), "1e300");
        assert_eq!(format_float(0.5), "0.5");
        assert_eq!(format_float(1e-9), "1e-9");
        assert_eq!(format_float(f64::INFINITY), "inf");
    }

    #[test]
    fn dimensioned_numbers_print_their_si_unit() {
        let num = NumVal::float(
            9.80665,
            Dim::base("Length")
                .div(&Dim::base("Time").pow(&BigRational::from_integer(BigInt::from(2)))),
        );
        assert_eq!(format_scalar(&num), "9.80665 m/s^2");
    }

    #[test]
    fn tensors_print_nested() {
        let tensor = TensorData {
            verified_element: None,
            shape: vec![2, 2],
            storage: Storage::Dense(Rc::new(vec![1.0, 2.0, 3.0, 4.0])),
            dim: Dim::one(),
            display: None,
        };
        assert_eq!(
            show_tensor_in(&tensor, false, 0),
            "[[1.0, 2.0], [3.0, 4.0]]"
        );
    }

    fn tensor(
        shape: Vec<usize>,
        values: Vec<f64>,
    ) -> Value {
        Value::Tensor(Rc::new(TensorData::new(
            shape,
            Storage::Dense(Rc::new(values)),
            Dim::one(),
            None,
        )))
    }

    #[test]
    fn tensor_equality_compares_shape_and_elements() {
        assert_eq!(
            equal(
                &tensor(vec![2], vec![1.0, 2.0]),
                &tensor(vec![2], vec![1.0, 2.0])
            ),
            Ok(true)
        );
        assert_eq!(
            equal(
                &tensor(vec![2], vec![1.0, 2.0]),
                &tensor(vec![2], vec![1.0, 3.0])
            ),
            Ok(false)
        );
        // The same elements in a different shape are different values.
        assert_eq!(
            equal(
                &tensor(vec![2, 2], vec![1.0, 2.0, 3.0, 4.0]),
                &tensor(vec![4], vec![1.0, 2.0, 3.0, 4.0])
            ),
            Ok(false)
        );
    }

    #[test]
    fn variant_equality_compares_payloads() {
        let one = make_variant(adt::OPTION_ID, 1, "Some", vec![Value::int(1)]);
        let two = make_variant(adt::OPTION_ID, 1, "Some", vec![Value::int(2)]);
        let also_one = make_variant(adt::OPTION_ID, 1, "Some", vec![Value::int(1)]);
        assert_eq!(equal(&one, &two), Ok(false));
        assert_eq!(equal(&one, &also_one), Ok(true));
        let none = make_variant(adt::OPTION_ID, 0, "None", Vec::new());
        assert_eq!(equal(&one, &none), Ok(false));
    }

    #[test]
    fn functions_have_no_equality() {
        let function = Value::Native(Rc::new(NativeFn(Box::new(|_interp, _args, _span| {
            Ok(Value::Unit)
        }))));
        assert_eq!(
            equal(&function, &function),
            Err(NotEqual::NoEq("a function"))
        );
    }

    #[test]
    fn repr_keeps_the_phase_and_show_does_not() {
        let exact = Value::Num(NumVal::exact(
            BigRational::new(BigInt::from(1), BigInt::from(2)),
            Dim::one(),
        ));
        assert_eq!(show(&exact), "0.5");
        assert_eq!(repr(&exact), "1/2");
        // A float keeps a decimal point so that it reads back as a `Float`.
        assert_eq!(show(&Value::float(0.5)), "0.5");
        assert_eq!(repr(&Value::float(0.5)), "0.5");
        assert_eq!(repr(&Value::float(3.0)), "3.0");
        assert_eq!(repr(&Value::int(3)), "3");
        assert_eq!(repr(&Value::string("a\nb")), "\"a\\nb\"");
    }
}
