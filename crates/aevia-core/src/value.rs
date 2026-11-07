//! Runtime values and their printed form.
//!
//! Types are erased (§9.2.3), but values keep the witnesses the runtime needs:
//! a scalar carries its dimension and an optional display unit, a `Point` is a
//! scalar with an affine display, a tensor carries its shape, and a variant
//! carries its constructor tag. Everything else — closures, records, IO
//! actions — is ordinary data.

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
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;
use num_traits::Signed;
use num_traits::ToPrimitive;
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
    /// An arbitrary-precision integer.
    Int(BigInt),
    /// An exact rational.
    Exact(BigRational),
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
    pub fn type_name(&self) -> &'static str {
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
    pub fn int(value: BigInt) -> Self {
        Self {
            kind: NumKind::Int(value),
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
            kind: NumKind::Exact(value),
            dim,
            display: None,
        }
    }

    /// A float.
    #[must_use]
    pub fn float(
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
            | NumKind::Int(value) => value.to_f64(),
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
            | NumKind::Int(value) => Some(BigRational::from_integer(value.clone())),
            | NumKind::Exact(value) => Some(value.clone()),
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
    pub fn is_symbolic(&self) -> bool {
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
    pub fn source(&self) -> &Rc<TensorData> {
        &self.source
    }

    /// The logical start within the original tensor.
    #[must_use]
    pub fn offset(&self) -> usize {
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
/// [`TensorData::strides`] publishes them as `&[usize]`. Splitting the sign out
/// keeps that comparison about layout and makes the direction impossible to
/// drop: a signed multiply that silently went negative would be caught by
/// `checked_add` at the *wrong* address rather than by the direction flag.
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

impl View {
    /// The signed step along `axis`, in elements.
    ///
    /// `None` when the magnitude does not fit an `isize`, which no buffer the
    /// runtime can allocate reaches: the window is rejected rather than
    /// wrapped.
    fn step(
        &self,
        axis: usize,
    ) -> Option<isize> {
        signed_stride(self.strides[axis], self.reversed[axis])
    }
}

/// A stride magnitude and its direction as one signed step.
///
/// The only place a direction becomes a sign. `usize → isize` is checked, so a
/// magnitude that cannot be negated rejects the window instead of wrapping to
/// `isize::MIN`.
fn signed_stride(
    stride: usize,
    reversed: bool,
) -> Option<isize> {
    let magnitude = isize::try_from(stride).ok()?;
    Some(if reversed {
        -magnitude
    } else {
        magnitude
    })
}

/// True when every position a window can reach lies inside a buffer of
/// `data_len` elements.
///
/// The reachable positions are `offset + Σ cₖ·sₖ` for `0 ≤ cₖ < extentₖ`, so
/// the two extremes of that sum are the two ends of every axis at once: a
/// positive step puts its largest coordinate on the maximum and zero on the
/// minimum, a negative step does the opposite. The bound check this replaces
/// assumed the furthest element was "the last coordinate of every axis", which
/// is only true while every stride is non-negative — a reversed axis puts the
/// furthest element at coordinate `0`, so that check would accept a window
/// whose low end lies before the buffer and reject one whose high end is
/// merely the first element.
///
/// The sum is taken in `i128` because the minimum is legitimately negative
/// before the final comparison: a coordinate before the start of the buffer is
/// exactly the case that has to be rejected, and a `usize` cannot hold it.
fn window_fits(
    shape: &[usize],
    strides: &[usize],
    reversed: &[bool],
    offset: usize,
    data_len: usize,
) -> bool {
    if shape.len() != strides.len() || shape.len() != reversed.len() {
        return false;
    }
    let mut lowest = offset as i128;
    let mut highest = lowest;
    for ((extent, stride), reversed) in shape.iter().zip(strides.iter()).zip(reversed.iter()) {
        let Some(step) = signed_stride(*stride, *reversed) else {
            return false;
        };
        let last = (*extent).max(1) as i128 - 1;
        let Some(span) = (step as i128).checked_mul(last) else {
            return false;
        };
        if span < 0 {
            let Some(low) = lowest.checked_add(span) else {
                return false;
            };
            lowest = low;
        } else {
            let Some(high) = highest.checked_add(span) else {
                return false;
            };
            highest = high;
        }
    }
    lowest >= 0 && highest < data_len as i128
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
    pub fn is_dense(&self) -> bool {
        matches!(self, Self::Dense(_))
    }

    /// True when the elements are a strided window.
    #[must_use]
    pub fn is_view(&self) -> bool {
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
pub(crate) fn row_major_strides(shape: &[usize]) -> Vec<usize> {
    let mut strides = vec![1usize; shape.len()];
    for axis in (0..shape.len().saturating_sub(1)).rev() {
        strides[axis] = strides[axis + 1] * shape[axis + 1];
    }
    strides
}

/// The compensated (Kahan–Babuška/Neumaier) sum of an `f64` sequence
/// (§6.6.6, §9.5).
///
/// A plain left-to-right fold loses the small terms as soon as one term is
/// much larger than the running total: ten tenths sum to `0.9999999999999999`
/// and `1e16 + 1 - 1e16` folds to `0`, while the exact answers are `1` and `1`.
/// The compensation accumulates the rounding error of each step in a second
/// accumulator, which is what makes the reduction "pairwise/Kahan" in the sense
/// the standard requires: the error no longer grows with the sequence length,
/// and a result that is exactly representable comes out exactly.
#[must_use]
pub fn kahan_sum(data: &[f64]) -> f64 {
    let mut total = 0.0f64;
    let mut compensation = 0.0f64;
    for value in data {
        let next = total + value;
        // Add back the part of the addition that was rounded away. The branch
        // is Neumaier's correction: it also recovers the small term when the
        // large term comes last, which the original Kahan step does not.
        if total.abs() >= value.abs() {
            compensation += (total - next) + value;
        } else {
            compensation += (value - next) + total;
        }
        total = next;
    }
    total + compensation
}

impl TensorData {
    /// Build an unverified tensor from host data. No element schema is inferred
    /// for empty boxed storage; use the checked constructor for explicit proof.
    #[must_use]
    pub fn new(
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

    /// The number of elements implied by the shape.
    #[must_use]
    pub fn element_count(shape: &[usize]) -> usize {
        shape.iter().product()
    }

    /// The rank.
    #[must_use]
    pub fn rank(&self) -> usize {
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
                    number.display = self.display.clone();
                }
                Some(value)
            },
            | Storage::View(view) => {
                let position = self.view_position(view, index)?;
                view.data
                    .get(position)
                    .map(|value| self.float_value(*value))
            },
            | Storage::Exact(items) | Storage::TypedExact(items, ExactRep::Real) => {
                items.get(index).map(|value| {
                    let mut num = NumVal::exact(value.clone(), self.dim.clone());
                    num.display = self.display.clone();
                    Value::Num(num)
                })
            },
            | Storage::TypedExact(items, ExactRep::Int) => {
                let value = items.get(index)?;
                if !value.is_integer() {
                    return None;
                }
                Some(Value::Num(NumVal {
                    kind: NumKind::Int(value.to_integer()),
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
        num.display = self.display.clone();
        Value::Num(num)
    }

    /// The buffer position of a logical index, for a view.
    ///
    /// The step along a reversed axis is negative, so the walk is signed:
    /// `checked_add_signed` rejects a position that would fall below the start
    /// of the buffer instead of wrapping it to a huge `usize` that happens to
    /// land inside the buffer.
    fn view_position(
        &self,
        view: &View,
        index: usize,
    ) -> Option<usize> {
        if index >= view.len
            || view.strides.len() != self.shape.len()
            || view.reversed.len() != self.shape.len()
        {
            return None;
        }
        let mut rest = index;
        let mut position = view.offset;
        for axis in (0..view.strides.len()).rev() {
            let extent = self.shape[axis].max(1);
            let coordinate = isize::try_from(rest % extent).ok()?;
            position = position.checked_add_signed(view.step(axis)?.checked_mul(coordinate)?)?;
            rest /= extent;
        }
        Some(position)
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
            | Storage::Dense(items) => Some(items.as_slice()),
            | Storage::View(view) => self.contiguous_window(view),
            | Storage::SharedSlice(view) => {
                view.source
                    .dense()?
                    .get(view.offset..view.offset.checked_add(view.len)?)
            },
            | _ => None,
        }
    }

    /// The `f64` elements in row-major order, borrowing when possible.
    ///
    /// This is the boundary every `f64` kernel uses. A contiguous tensor —
    /// dense, or a view with row-major strides — is borrowed, so the fast path
    /// costs nothing extra. A strided view is walked once into an owned
    /// buffer: materialising at the kernel boundary is what keeps a kernel
    /// that cannot walk strides from computing a wrong value. A *reversed*
    /// view takes the same owned path even when its strides are otherwise
    /// row-major, because its elements are not adjacent in the buffer.
    ///
    /// Returns `None` for a class that is not `f64` at all.
    #[must_use]
    pub fn dense_flat(&self) -> Option<Cow<'_, [f64]>> {
        match &self.storage {
            | Storage::Dense(items) => Some(Cow::Borrowed(items.as_slice())),
            | Storage::SharedSlice(view) => {
                let end = view.offset.checked_add(view.len)?;
                match view.source.dense_flat()? {
                    | Cow::Borrowed(data) => Some(Cow::Borrowed(data.get(view.offset..end)?)),
                    | Cow::Owned(data) => Some(Cow::Owned(data.get(view.offset..end)?.to_vec())),
                }
            },
            | Storage::View(view) => {
                match self.contiguous_window(view) {
                    | Some(window) => Some(Cow::Borrowed(window)),
                    | None => {
                        let mut out = Vec::with_capacity(view.len);
                        for index in 0..view.len {
                            out.push(*view.data.get(self.view_position(view, index)?)?);
                        }
                        Some(Cow::Owned(out))
                    },
                }
            },
            | _ => None,
        }
    }

    /// The contiguous window a view names, when its strides are row-major.
    ///
    /// A reversed axis is never contiguous, even when its magnitude is 1: the
    /// window's elements run backwards through the buffer, so no single slice
    /// of it holds them in logical order.
    fn contiguous_window<'a>(
        &self,
        view: &'a View,
    ) -> Option<&'a [f64]> {
        if view.reversed.iter().any(|reversed| *reversed) {
            return None;
        }
        if view.strides != row_major_strides(&self.shape) {
            return None;
        }
        view.data
            .get(view.offset..view.offset.checked_add(view.len)?)
    }

    /// The buffer this tensor's elements live in, when it has one.
    ///
    /// Two tensors share storage exactly when these are `Rc::ptr_eq`: that is
    /// the property a slice must have, and the one a copying slice lacks.
    #[must_use]
    pub fn buffer(&self) -> Option<&Rc<Vec<f64>>> {
        match &self.storage {
            | Storage::Dense(data) => Some(data),
            | Storage::View(view) => Some(&view.data),
            | _ => None,
        }
    }

    /// True when the tensor is a contiguous `f64` buffer.
    #[must_use]
    pub fn is_dense(&self) -> bool {
        self.storage.is_dense()
    }

    /// True when the elements are a strided window.
    #[must_use]
    pub fn is_view(&self) -> bool {
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
    pub fn offset(&self) -> usize {
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

    /// A dense tensor with the same shape, dimension and display.
    fn dense_with(
        &self,
        data: Vec<f64>,
        dim: Dim,
        display: Option<Rc<UnitDef>>,
    ) -> Value {
        Value::Tensor(Rc::new(Self {
            verified_element: None,
            shape: self.shape.clone(),
            storage: Storage::Dense(Rc::new(data)),
            dim,
            display,
        }))
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

    fn flat_index(
        &self,
        indices: &[isize],
    ) -> Option<usize> {
        if indices.len() != self.shape.len() {
            return None;
        }
        let mut flat = 0usize;
        for (index, extent) in indices.iter().zip(self.shape.iter()) {
            let extent = isize::try_from(*extent).ok()?;
            let normalized = if *index < 0 {
                extent + index
            } else {
                *index
            };
            if normalized < 0 || normalized >= extent {
                return None;
            }
            flat = flat * usize::try_from(extent).ok()? + usize::try_from(normalized).ok()?;
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
        if self.rank() != 1 || self.shape[0] != self.storage.len() || start > self.shape[0] {
            return None;
        }
        if start == 0 {
            return Some(self.clone());
        }
        let len = self.shape[0] - start;
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
        (0..self.storage.len())
            .filter_map(|index| self.get_flat(index))
            .collect()
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
        let (left_data, right_data) = (self.dense_flat()?, right.dense_flat()?);
        if self.shape != right.shape || left_data.is_empty() {
            return None;
        }
        let dim = match op {
            | BinOp::Add | BinOp::Sub => self.dim.clone(),
            | BinOp::Mul => self.dim.mul(&right.dim),
            | BinOp::Div => self.dim.div(&right.dim),
            | BinOp::Pow => {
                // The boxed path takes the tensor's dimension from the first
                // element it builds, so the first exponent decides it here too.
                let exponent = BigRational::from_float(right_data[0])?;
                self.dim.pow(&exponent)
            },
            | _ => return None,
        };
        let mut data = Vec::with_capacity(left_data.len());
        for (a, b) in left_data.iter().zip(right_data.iter()) {
            data.push(match op {
                | BinOp::Add => a + b,
                | BinOp::Sub => a - b,
                | BinOp::Mul => a * b,
                | BinOp::Div => a / b,
                | BinOp::Pow => a.powf(*b),
                | _ => return None,
            });
        }
        Some(self.dense_with(data, dim, None))
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
        let data = self.dense_flat()?;
        let NumKind::Float(number) = &scalar.kind else {
            return None;
        };
        let number = *number;
        if data.is_empty() || (flipped && op == BinOp::Pow) {
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
        let mut out = Vec::with_capacity(data.len());
        for element in data.iter() {
            let (a, b) = if flipped {
                (number, *element)
            } else {
                (*element, number)
            };
            out.push(match op {
                | BinOp::Add => a + b,
                | BinOp::Sub => a - b,
                | BinOp::Mul => a * b,
                | BinOp::Div => a / b,
                | BinOp::Pow => a.powf(b),
                | _ => return None,
            });
        }
        Some(self.dense_with(out, dim, None))
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
            num.display = self.display.clone();
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
            num.display = self.display.clone();
        }
        Some(num)
    }

    /// The `f64` `@` for the rank pairs §3.5.3a defines, or `None`.
    ///
    /// `matrix @ matrix` runs on `aevia_std::linalg::matmul`, a blocked kernel,
    /// which reassociates the inner sum: it agrees with the sequential loop to
    /// rounding, not bit for bit. Every other rank pair is a vector kernel and
    /// is still the loop — it is memory-bound, was not measured, and its call
    /// overhead would dominate at the sizes the language uses it at. A zero
    /// inner extent is left to the boxed path, which answers a dimensionless
    /// zero.
    #[must_use]
    pub fn dense_matmul(
        &self,
        right: &Self,
    ) -> Option<Value> {
        let (a, b) = (self.dense_flat()?, right.dense_flat()?);
        match (self.rank(), right.rank()) {
            | (2, 2) => {
                let (m, k) = (self.shape[0], self.shape[1]);
                let (k2, n) = (right.shape[0], right.shape[1]);
                if k != k2 || k == 0 || m == 0 || n == 0 {
                    return None;
                }
                let data = aevia_std::linalg::matmul(m, k, n, &a, &b)
                    .unwrap_or_else(|_| matmul_f64_loop(m, k, n, &a, &b));
                // The result is `m × n`, which is not `self.shape` unless
                // `k == n`: `dense_with` would keep the left operand's shape.
                Some(Value::Tensor(Rc::new(Self {
                    verified_element: None,
                    shape: vec![m, n],
                    storage: Storage::Dense(Rc::new(data)),
                    dim: self.dim.mul(&right.dim),
                    display: None,
                })))
            },
            | (2, 1) => {
                let (m, k) = (self.shape[0], self.shape[1]);
                if k != right.shape[0] || k == 0 || m == 0 {
                    return None;
                }
                let mut data = vec![0.0; m];
                for row in 0..m {
                    let mut total = a[row * k] * b[0];
                    for inner in 1..k {
                        total += a[row * k + inner] * b[inner];
                    }
                    data[row] = total;
                }
                Some(Value::Tensor(Rc::new(Self {
                    verified_element: None,
                    shape: vec![m],
                    storage: Storage::Dense(Rc::new(data)),
                    dim: self.dim.mul(&right.dim),
                    display: None,
                })))
            },
            | (1, 2) => {
                let k = self.shape[0];
                let (k2, n) = (right.shape[0], right.shape[1]);
                if k != k2 || k == 0 || n == 0 {
                    return None;
                }
                let mut data = vec![0.0; n];
                for column in 0..n {
                    let mut total = a[0] * b[column];
                    for inner in 1..k {
                        total += a[inner] * b[inner * n + column];
                    }
                    data[column] = total;
                }
                Some(Value::Tensor(Rc::new(Self {
                    verified_element: None,
                    shape: vec![n],
                    storage: Storage::Dense(Rc::new(data)),
                    dim: self.dim.mul(&right.dim),
                    display: None,
                })))
            },
            | (1, 1) => {
                let k = self.shape[0];
                if k != right.shape[0] || k == 0 {
                    return None;
                }
                let mut total = a[0] * b[0];
                for index in 1..k {
                    total += a[index] * b[index];
                }
                Some(Value::Num(NumVal::float(total, self.dim.mul(&right.dim))))
            },
            | _ => None,
        }
    }

    /// The `f64` buffer a slice of this tensor would walk, with the offset,
    /// strides and directions that describe its current layout.
    ///
    /// A dense tensor answers with its buffer and the row-major strides of its
    /// shape, every axis forward; a view answers with the buffer, offset,
    /// strides and directions it already has, which is what makes a slice of a
    /// view a view of the *same* buffer — including a slice of a reversed one.
    fn f64_window(&self) -> Option<(Rc<Vec<f64>>, usize, Vec<usize>, Vec<bool>)> {
        match &self.storage {
            | Storage::Dense(data) => {
                Some((
                    data.clone(),
                    0,
                    row_major_strides(&self.shape),
                    vec![false; self.rank()],
                ))
            },
            | Storage::SharedSlice(view) => {
                let (data, offset, strides, reversed) = view.source.f64_window()?;
                if strides.len() != 1 || reversed.len() != 1 {
                    return None;
                }
                let step = signed_stride(*strides.first()?, *reversed.first()?)?;
                let shift = step.checked_mul(isize::try_from(view.offset).ok()?)?;
                let start = offset.checked_add_signed(shift)?;
                if self.rank() != 1 {
                    // A host may reuse logical suffix storage under a new
                    // shape. Only a contiguous range has a strided f64 layout
                    // in that shape; other layouts use the logical fallback.
                    self.dense()?;
                    return Some((
                        data,
                        start,
                        row_major_strides(&self.shape),
                        vec![false; self.rank()],
                    ));
                }
                Some((data, start, strides, reversed))
            },
            | Storage::View(view) => {
                Some((
                    view.data.clone(),
                    view.offset,
                    view.strides.clone(),
                    view.reversed.clone(),
                ))
            },
            | _ => None,
        }
    }

    /// The sub-tensor named by one `(start, end)` range per axis, as a view.
    ///
    /// Slicing moves the offset and keeps the parent's strides and directions,
    /// so nothing is copied and `xs[1:3]` shares `xs`'s buffer. A range along a
    /// reversed axis keeps that axis reversed — `xs[::-1][0:2]` is the last two
    /// elements, in the order the reversal gave them — because a slice selects
    /// a sub-sequence of the sequence it was given, not of the buffer.
    ///
    /// The result is a view even when its window happens to be contiguous: the
    /// class is not observable, and a contiguous view already takes the `f64`
    /// path through [`TensorData::dense_flat`].
    ///
    /// Returns `None` when the elements are not in an `f64` buffer, when the
    /// ranges do not match the rank, when a range is out of bounds, or when
    /// the result is empty (an empty tensor stays [`Storage::Boxed`], as
    /// everywhere else).
    #[must_use]
    pub fn slice_view(
        &self,
        ranges: &[(usize, usize)],
    ) -> Option<Self> {
        let (data, offset, strides, reversed) = self.f64_window()?;
        if ranges.len() != self.rank() {
            return None;
        }
        let mut shape = Vec::with_capacity(ranges.len());
        let mut start = offset;
        for (axis, (begin, end)) in ranges.iter().enumerate() {
            let extent = self.shape[axis];
            if *begin > *end || *end > extent {
                return None;
            }
            shape.push(*end - *begin);
            // The offset moves by whole logical steps, so a reversed axis moves
            // it backwards; `checked_add_signed` refuses a start before the
            // buffer rather than wrapping it.
            let step = signed_stride(strides[axis], reversed[axis])?;
            let begin = isize::try_from(*begin).ok()?;
            start = start.checked_add_signed(step.checked_mul(begin)?)?;
        }
        let len: usize = shape.iter().product();
        // Both ends of the window must be inside the buffer: with a reversed
        // axis the furthest element is at coordinate 0, not at the last one.
        if len == 0 || !window_fits(&shape, &strides, &reversed, start, data.len()) {
            return None;
        }
        Some(Self {
            verified_element: self.verified_element.clone(),
            shape,
            storage: Storage::View(View {
                data,
                offset: start,
                strides,
                reversed,
                len,
            }),
            dim: self.dim.clone(),
            display: self.display.clone(),
        })
    }

    /// The sub-tensor named by one `(first, count, step)` walk per axis, as a
    /// view (§6.6.4).
    ///
    /// The stride of an axis is multiplied by the step and a negative step
    /// flips the axis's direction, so a stepped slice moves no data: `xs[::2]`
    /// reads every second element of the same buffer and `xs[::-1]` is that
    /// buffer walked backwards from its end. That is what makes a negative
    /// step a view rather than a copy, and why `Rc::ptr_eq` holds between it
    /// and its source.
    ///
    /// Returns `None` for a class that is not in an `f64` buffer, for a walk
    /// that leaves the buffer, and for an empty result (an empty tensor stays
    /// [`Storage::Boxed`], as everywhere else).
    #[must_use]
    pub fn slice_step_view(
        &self,
        walks: &[(usize, usize, isize)],
    ) -> Option<Self> {
        let (data, offset, strides, reversed) = self.f64_window()?;
        if walks.len() != self.rank() {
            return None;
        }
        let mut shape = Vec::with_capacity(walks.len());
        let mut new_strides = Vec::with_capacity(walks.len());
        let mut new_reversed = Vec::with_capacity(walks.len());
        let mut start = offset;
        for (axis, (first, count, step)) in walks.iter().enumerate() {
            if *count > 0 && *first >= self.shape[axis] {
                return None;
            }
            let magnitude = usize::try_from(step.unsigned_abs()).ok()?;
            if magnitude == 0 {
                return None;
            }
            // The first element of the walk, in the parent's window.
            let base = signed_stride(strides[axis], reversed[axis])?;
            start = start.checked_add_signed(base.checked_mul(isize::try_from(*first).ok()?)?)?;
            shape.push(*count);
            new_strides.push(strides[axis].checked_mul(magnitude)?);
            // The walk runs backwards exactly when the parent's direction and
            // the step's sign disagree.
            new_reversed.push(reversed[axis] != (*step < 0));
        }
        let len: usize = shape.iter().product();
        if len == 0 || !window_fits(&shape, &new_strides, &new_reversed, start, data.len()) {
            return None;
        }
        Some(Self {
            verified_element: self.verified_element.clone(),
            shape,
            storage: Storage::View(View {
                data,
                offset: start,
                strides: new_strides,
                reversed: new_reversed,
                len,
            }),
            dim: self.dim.clone(),
            display: self.display.clone(),
        })
    }

    /// The sub-tensor at `fixed` coordinates, keeping the trailing axes, as a
    /// view.
    ///
    /// This is `m[0]` on a matrix: the leading axes are pinned and the rest
    /// are the window, so the result shares the parent's buffer instead of
    /// copying a row out of it. Pinning a reversed axis moves the offset
    /// backwards, and the trailing axes keep the direction they had.
    ///
    /// Returns `None` when the elements are not in an `f64` buffer, when
    /// `fixed` names more axes than the tensor has, when a coordinate is out
    /// of range, or when the result is empty.
    #[must_use]
    pub fn index_view(
        &self,
        fixed: &[usize],
    ) -> Option<Self> {
        let (data, offset, strides, reversed) = self.f64_window()?;
        if fixed.len() > self.rank() {
            return None;
        }
        let mut start = offset;
        for (axis, coordinate) in fixed.iter().enumerate() {
            if *coordinate >= self.shape[axis] {
                return None;
            }
            let step = signed_stride(strides[axis], reversed[axis])?;
            let coordinate = isize::try_from(*coordinate).ok()?;
            start = start.checked_add_signed(step.checked_mul(coordinate)?)?;
        }
        let shape = self.shape[fixed.len()..].to_vec();
        let len: usize = shape.iter().product();
        if len == 0
            || !window_fits(
                &shape,
                &strides[fixed.len()..],
                &reversed[fixed.len()..],
                start,
                data.len(),
            )
        {
            return None;
        }
        Some(Self {
            verified_element: self.verified_element.clone(),
            shape,
            storage: Storage::View(View {
                data,
                offset: start,
                strides: strides[fixed.len()..].to_vec(),
                reversed: reversed[fixed.len()..].to_vec(),
                len,
            }),
            dim: self.dim.clone(),
            display: self.display.clone(),
        })
    }

    /// The rank-2 transpose as a view: the strides are permuted, not the data.
    ///
    /// `mᵀ[i, j] == m[j, i]` holds by construction — the strides and the
    /// directions swap with the extents — so a transpose moves no element. It
    /// is not contiguous unless the matrix is 1×n or n×1, which is exactly the
    /// case where [`TensorData::dense_flat`] can still borrow. A transpose of
    /// a reversed matrix keeps the reversal on the axis it moved to.
    ///
    /// Returns `None` when the elements are not in an `f64` buffer, when the
    /// tensor is not rank 2, or when it is empty.
    #[must_use]
    pub fn transposed_view(&self) -> Option<Self> {
        let (data, offset, strides, reversed) = self.f64_window()?;
        if self.rank() != 2 {
            return None;
        }
        let shape = vec![self.shape[1], self.shape[0]];
        let strides = vec![strides[1], strides[0]];
        let reversed = vec![reversed[1], reversed[0]];
        let len: usize = shape.iter().product();
        if len == 0 || !window_fits(&shape, &strides, &reversed, offset, data.len()) {
            return None;
        }
        Some(Self {
            verified_element: self.verified_element.clone(),
            shape,
            storage: Storage::View(View {
                data,
                offset,
                strides,
                reversed,
                len,
            }),
            dim: self.dim.clone(),
            display: self.display.clone(),
        })
    }

    /// The same window with one axis running the other way, as a view.
    ///
    /// This is `xs[::-1]` on a vector, a flipped convolution kernel, or an axis
    /// a fold walked backwards: no element moves and the buffer is still the
    /// parent's, so `Rc::ptr_eq` holds against the source. The offset moves to
    /// the far end of the axis and the axis' direction flips, which is why two
    /// reversals of one axis are the identity and give the original offset
    /// back.
    ///
    /// The result is never contiguous, even for a vector of stride 1: its
    /// elements are adjacent in the buffer but in the opposite order, so
    /// [`TensorData::dense_flat`] materialises it before a kernel sees it.
    ///
    /// Returns `None` when the elements are not in an `f64` buffer, when
    /// `axis` is not a dimension, or when the tensor is empty.
    #[must_use]
    pub fn reversed_view(
        &self,
        axis: usize,
    ) -> Option<Self> {
        let (data, offset, strides, mut reversed) = self.f64_window()?;
        let extent = *self.shape.get(axis)?;
        let len: usize = self.shape.iter().product();
        if len == 0 || extent == 0 {
            return None;
        }
        let step = signed_stride(strides[axis], reversed[axis])?;
        let span = step.checked_mul(isize::try_from(extent - 1).ok()?)?;
        let start = offset.checked_add_signed(span)?;
        // An axis of extent 1 has no direction to flip: every coordinate on it
        // names the same element, and flipping would make a contiguous view
        // look strided for nothing.
        if extent > 1 {
            reversed[axis] = !reversed[axis];
        }
        if !window_fits(&self.shape, &strides, &reversed, start, data.len()) {
            return None;
        }
        Some(Self {
            verified_element: self.verified_element.clone(),
            shape: self.shape.clone(),
            storage: Storage::View(View {
                data,
                offset: start,
                strides,
                reversed,
                len,
            }),
            dim: self.dim.clone(),
            display: self.display.clone(),
        })
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
    /// Returns `None` for a view over a buffer that is not `f64`, which cannot
    /// be constructed today.
    #[must_use]
    pub fn reshape_storage(
        &self,
        shape: &[usize],
    ) -> Option<Storage> {
        let len = Self::element_count(shape);
        match &self.storage {
            | Storage::SharedSlice(_) => {
                if len != self.storage.len() {
                    return None;
                }
                if len == 0 {
                    return Some(match self.storage.backing() {
                        | Storage::Dense(_) | Storage::View(_) => {
                            Storage::Dense(Rc::new(Vec::new()))
                        },
                        | Storage::Exact(_) => Storage::Exact(Vec::new()),
                        | Storage::TypedExact(_, rep) => Storage::TypedExact(Vec::new(), *rep),
                        | Storage::Bool(_) => Storage::Bool(Vec::new()),
                        | Storage::Boxed(_) => Storage::Boxed(Vec::new()),
                        | Storage::SharedSlice(_) => unreachable!("unwrapped storage"),
                    });
                }
                if self.dense().is_some() {
                    let (data, offset, _, _) = self.f64_window()?;
                    return Some(Storage::View(View {
                        data,
                        offset,
                        strides: row_major_strides(shape),
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
                        strides: row_major_strides(shape),
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

/// The sequential `rows × inner` by `inner × cols` product.
///
/// This is the loop `@` ran before the dispatcher existed. It is kept as the
/// answer for a call `aevia_std::linalg::matmul` declines (a buffer whose
/// length disagrees with its shape), so a shape this crate cannot express
/// still produces a value rather than an error.
fn matmul_f64_loop(
    rows: usize,
    inner: usize,
    cols: usize,
    a: &[f64],
    b: &[f64],
) -> Vec<f64> {
    let mut data = vec![0.0; rows * cols];
    for row in 0..rows {
        for column in 0..cols {
            let mut total = a[row * inner] * b[column];
            for step in 1..inner {
                total += a[row * inner + step] * b[step * cols + column];
            }
            data[row * cols + column] = total;
        }
    }
    data
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
            if let Value::Variant(variant) = value {
                if let Ok(mut inner) = Rc::try_unwrap(variant) {
                    pending.append(&mut inner.fields);
                }
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
        scalar.display = self.display.clone();
        scalar
    }
}

/// A lexical environment: a persistent chain of scopes.
///
/// The bindings are behind a `RefCell` so that a recursive function can be
/// inserted into the very scope it captures (§9.3.1).
#[derive(Debug, Default)]
pub struct Scope {
    /// The enclosing scope.
    pub parent: Option<Rc<Scope>>,
    /// Bindings introduced here.
    pub vars: RefCell<crate::fxhash::FxHashMap<String, Value>>,
    /// Exact checked revision used by this lexical environment.
    pub(crate) context: Option<Rc<crate::evidence::ModuleContext>>,
    /// Imported module environments retain their own implementation revision.
    /// Module imports are acyclic, so these edges create no ownership cycle.
    pub(crate) imports: Vec<Rc<Scope>>,
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
            context: parent.context.clone(),
            imports: Vec::new(),
            types: parent.types.clone(),
        })
    }

    pub(crate) fn module(
        context: Rc<crate::evidence::ModuleContext>,
        imports: Vec<Rc<Scope>>,
    ) -> Rc<Self> {
        Rc::new(Self {
            parent: None,
            vars: RefCell::default(),
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
            context: parent.context.clone(),
            imports: Vec::new(),
            types,
        })
    }

    /// Looks a name up through the chain.
    #[must_use]
    pub fn lookup(
        self: &Rc<Self>,
        name: &str,
    ) -> Option<Value> {
        // Walk by reference: cloning the `Rc` at every level was a refcount
        // round trip per scope for every global read.
        let mut scope: &Scope = self;
        loop {
            if let Some(value) = scope.vars.borrow().get(name) {
                return Some(value.clone());
            }
            scope = scope.parent.as_deref()?;
        }
    }
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
    pub(crate) canonical_origin: std::rc::Weak<Closure>,
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

    pub(crate) fn origin(closure: &Rc<Self>) -> std::rc::Weak<Self> {
        if std::rc::Weak::ptr_eq(&closure.canonical_origin, &std::rc::Weak::new()) {
            Rc::downgrade(closure)
        } else {
            closure.canonical_origin.clone()
        }
    }

    #[cfg(feature = "jit")]
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
    /// Runs an action and passes its result to a function.
    Bind {
        /// The action.
        action: Rc<IoAction>,
        /// The continuation.
        func: Value,
    },
    /// Runs actions in order, keeping the last result.
    Seq(Vec<Rc<IoAction>>),
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
        action: Rc<IoAction>,
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
    pub payload: aevia_rssn::Val,
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
    pub code: String,
    pub message: String,
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
    /// A record.
    Record(Rc<RecordValue>),
    /// A variant.
    Variant(Rc<VariantValue>),
    /// A function.
    Closure(Rc<Closure>),
    /// A Rust-implemented function.
    Native(Rc<NativeFn>),
    /// A constructor, possibly partially applied.
    Ctor {
        /// The type.
        adt: AdtId,
        /// The constructor index.
        index: usize,
        /// The name.
        name: String,
        /// How many fields it takes.
        arity: usize,
        /// Fields collected so far.
        args: Vec<Value>,
        /// Provenance for constructors resolved from checked source.
        origin: Option<SourceCtorToken>,
    },
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
        Self::Num(NumVal::int(BigInt::from(value)))
    }

    /// A string.
    #[must_use]
    pub fn string(value: impl Into<String>) -> Self {
        Self::Str(Rc::from(value.into()))
    }

    /// The scalar, when this is one.
    #[must_use]
    pub fn as_num(&self) -> Option<&NumVal> {
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
    pub fn is_io(&self) -> bool {
        matches!(self, Self::Io(_))
    }

    /// The runtime type name, for diagnostics.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            | Self::Unit => "()",
            | Self::Bool(_) => "Bool",
            | Self::Num(num) => num.kind.type_name(),
            | Self::Str(_) => "String",
            | Self::Tensor(_) => "Tensor",
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
    // A symbolic term prints itself; the display unit only adds a suffix.
    if let NumKind::Sym(term) = &num.kind {
        let text = if repr {
            term.to_string_repr()
        } else {
            term.to_string()
        };
        let suffix = num
            .display
            .as_ref()
            .map(|unit| unit.display.clone())
            .unwrap_or_else(|| si_unit_text(&num.dim));
        return if suffix.is_empty() {
            text
        } else {
            format!("{text} {suffix}")
        };
    }
    let (text, unit) = match (&num.kind, &num.display) {
        | (kind, Some(unit)) => {
            let text = match kind {
                | NumKind::Int(value) => {
                    display_exact(&BigRational::from_integer(value.clone()), unit, repr)
                },
                | NumKind::Exact(value) => display_exact(value, unit, repr),
                | NumKind::Float(value) => {
                    let shown = float_in_display_unit(*value, unit);
                    if repr {
                        format_float_repr(shown)
                    } else {
                        format_float(shown)
                    }
                },
                | NumKind::Complex(re, im) => {
                    let sign = if *im < 0.0 { "-" } else { "+" };
                    format!("{} {sign} {}*i", format_float(*re), format_float(im.abs()))
                },
                | NumKind::Sym(_) => unreachable!("handled above"),
            };
            (text, Some(unit.display.clone()))
        },
        | (NumKind::Int(value), None) => (value.to_string(), None),
        | (NumKind::Exact(value), None) => {
            (
                if repr {
                    // `repr` of an exact value is a rational: an integral one is
                    // its integer (`Int ⊂ Real`, so the text is the same), and the
                    // rest keep their denominator.
                    aevia_dim::rat::format_rational(value)
                } else {
                    aevia_dim::format_rational_decimal(value)
                },
                None,
            )
        },
        | (NumKind::Float(value), None) => {
            (
                if repr {
                    format_float_repr(*value)
                } else {
                    format_float(*value)
                },
                None,
            )
        },
        | (NumKind::Complex(re, im), None) => {
            let sign = if *im < 0.0 { "-" } else { "+" };
            (
                format!("{} {sign} {}*i", format_float(*re), format_float(im.abs())),
                None,
            )
        },
        | (NumKind::Sym(_), None) => unreachable!("handled above"),
    };
    let suffix = match unit {
        | Some(unit) => unit,
        | None => si_unit_text(&num.dim),
    };
    if suffix.is_empty() {
        text
    } else {
        format!("{text} {suffix}")
    }
}

/// A coherent `Float` expressed in a display unit.
///
/// Two renderings are within an ulp of the true `(x - offset) / scale`: the
/// IEEE evaluation (whose rounding of the offset cancels when `x` came from a
/// literal in the same unit: `25.0 °C` stored as `298.15` reads back `25`),
/// and the conversion on the exact rationals rounded once (which is right
/// when it does not cancel: `point(300.0, K)` reads `26.85 °C`, not
/// `26.850000000000023 °C`). The shorter text is shown. The value itself is
/// unchanged; only its rendering is.
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
    let exact_scale = (unit.scale.pi == 0).then(|| unit.scale.rat.clone());
    let exact_offset = match &unit.offset {
        | None => Some(BigRational::zero()),
        | Some(offset) if offset.pi == 0 => Some(offset.rat.clone()),
        | Some(_) => None,
    };
    if let (Some(scale), Some(offset), Some(exact)) =
        (exact_scale, exact_offset, BigRational::from_float(value))
    {
        if !scale.is_zero() {
            let rounded = aevia_dim::rat::to_f64(&((exact - offset) / scale));
            if format_float(rounded).len() < format_float(ieee).len() {
                return rounded;
            }
        }
    }
    ieee
}

/// Converts a coherent exact value into a display unit without leaving the
/// rationals, so `3.2 km` stays `3.2 km` instead of `3.2000000000000006 km`.
fn display_exact(
    value: &BigRational,
    unit: &UnitDef,
    repr: bool,
) -> String {
    let offset = unit
        .offset
        .as_ref()
        .map_or_else(BigRational::zero, |scale| {
            scale.rat.clone()
                * if scale.pi == 0 {
                    BigRational::one()
                } else {
                    // An affine offset never carries a factor of pi in the SI
                    // table; a unit that did would not be a temperature.
                    BigRational::one()
                }
        });
    if unit.scale.rat.is_zero() {
        return if repr {
            aevia_dim::rat::format_rational(value)
        } else {
            aevia_dim::format_rational_decimal(value)
        };
    }
    let shown = (value - offset) / unit.scale.rat.clone();
    if repr {
        aevia_dim::rat::format_rational(&shown)
    } else {
        aevia_dim::format_rational_decimal(&shown)
    }
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

/// Renders a value that sits inside a container (a tensor element).
fn render_element(
    value: &Value,
    repr: bool,
) -> String {
    render_at(value, repr, 1)
}

/// The deepest nesting `show` will walk.
///
/// A value nested deeper than this would recurse once per level in a path that
/// cannot report an error (`show` returns a `String`), and an overflow inside
/// a diagnostic is an uncatchable abort (§9.3.4). The marker is the honest
/// answer for a value no program prints by hand.
const MAX_RENDER_DEPTH: usize = 1_024;

/// Whether record labels are exactly `0, 1, …, n-1` in order with `n ≥ 2`:
/// the labels a tuple expression produces.
fn is_tuple_labels<'a>(labels: impl Iterator<Item = &'a str>) -> bool {
    let mut count = 0usize;
    for (index, label) in labels.enumerate() {
        if label != index.to_string() {
            return false;
        }
        count += 1;
    }
    count >= 2
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
    match value {
        | Value::Unit => "()".to_string(),
        | Value::Bool(true) => "true".to_string(),
        | Value::Bool(false) => "false".to_string(),
        | Value::Num(num) => {
            if repr {
                format_scalar_repr(num)
            } else {
                format_scalar(num)
            }
        },
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
        | Value::Tensor(tensor) => show_tensor_in(tensor, repr),
        | Value::Record(record) => {
            // Tuples are records labelled `0, 1, …` (deviation 5); they are
            // written `(a, b)`, so they print that way too.
            if is_tuple_labels(record.fields.keys().map(String::as_str)) {
                let mut out = String::from("(");
                for (index, value) in record.fields.values().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&render_at(value, repr, depth + 1));
                }
                out.push(')');
                return out;
            }
            let mut out = String::from("{ ");
            for (index, (name, value)) in record.fields.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                let _ = write!(out, "{name} = {}", render_at(value, repr, depth + 1));
            }
            out.push_str(" }");
            out
        },
        | Value::Variant(variant) if variant.adt == aevia_types::adt::set_id() => {
            // §8.1.9: a set shows as `{a, b, c}` in its canonical order.
            let items = match variant.fields.first() {
                | Some(Value::Tensor(tensor)) => tensor.elements(),
                | _ => Vec::new(),
            };
            let rendered: Vec<String> = items
                .iter()
                .map(|item| render_at(item, repr, depth + 1))
                .collect();
            if repr {
                format!("set_of([{}])", rendered.join(", "))
            } else {
                format!("{{{}}}", rendered.join(", "))
            }
        },
        | Value::Variant(variant) => {
            if variant.fields.is_empty() {
                variant.name.clone()
            } else {
                let mut out = format!("{}(", variant.name);
                for (index, field) in variant.fields.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&render_at(field, repr, depth + 1));
                }
                out.push(')');
                out
            }
        },
        | Value::Closure(_) | Value::Native(_) => "<fn>".to_string(),
        | Value::Ctor { name, args, .. } => {
            if args.is_empty() {
                name.clone()
            } else {
                let rendered: Vec<String> = args
                    .iter()
                    .map(|arg| render_at(arg, repr, depth + 1))
                    .collect();
                format!("{name}({})", rendered.join(", "))
            }
        },
        | Value::UnitOf(unit) => unit.display.clone(),
        | Value::Io(_) => "<io>".to_string(),
        | Value::Prop(prop) => prop.text.clone(),
        // §4.9.4: a `Dyn` prints as the scalar it carries, in the unit it was
        // written with, so `show(dyn(3 m))` names the same quantity.
        | Value::Dyn(dynamic) => {
            let scalar = dynamic.as_scalar();
            if repr {
                format_scalar_repr(&scalar)
            } else {
                format_scalar(&scalar)
            }
        },
        | Value::KernelValue(_) => "<Value>".to_string(),
        | Value::KernelError(error) => format!("{}: {}", error.code, error.message),
        | Value::TypeRep(_) => "<TypeRep>".to_string(),
        | Value::Callback(_) => "<Callback>".to_string(),
        | Value::Error => "<error>".to_string(),
    }
}

/// Quotes a string as Aevia source does.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
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

fn show_tensor_in(
    tensor: &TensorData,
    repr: bool,
) -> String {
    fn render_at(
        tensor: &TensorData,
        offset: &mut usize,
        depth: usize,
        repr: bool,
    ) -> String {
        let extent = tensor.shape.get(depth).copied().unwrap_or(0);
        if depth + 1 == tensor.shape.len() {
            let mut parts = Vec::with_capacity(extent);
            for _ in 0..extent {
                parts.push(
                    tensor
                        .get_flat(*offset)
                        .map_or_else(|| "?".to_string(), |value| render_element(&value, repr)),
                );
                *offset += 1;
            }
            format!("[{}]", parts.join(", "))
        } else {
            let mut parts = Vec::with_capacity(extent);
            for _ in 0..extent {
                parts.push(render_at(tensor, offset, depth + 1, repr));
            }
            format!("[{}]", parts.join(", "))
        }
    }
    if tensor.shape.is_empty() {
        return tensor
            .get_flat(0)
            .map_or_else(|| "[]".to_string(), |value| render(&value, repr));
    }
    // A deep rank would recurse once per axis; the shape of a value the
    // runtime can build is bounded by the literal nesting it came from.
    let mut offset = 0;
    render_at(tensor, &mut offset, 0, repr)
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
    let latex = syntax == MathSyntax::Latex;
    match value {
        | Value::Num(num) => Ok(math_scalar(num, syntax)),
        | Value::Str(text) if latex => Ok(format!("\\text{{{text}}}")),
        | Value::Str(text) => Ok(format!("\"{text}\"")),
        | Value::Bool(flag) => {
            Ok(match (latex, *flag) {
                | (true, true) => "\\mathrm{true}".to_string(),
                | (true, false) => "\\mathrm{false}".to_string(),
                | (false, true) => "true".to_string(),
                | (false, false) => "false".to_string(),
            })
        },
        | Value::Unit => Ok("()".to_string()),
        | Value::Tensor(tensor) => math_tensor(tensor, syntax),
        | Value::Record(record) => {
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
                    let _ = write!(out, "\\mathrm{{{name}}} &= {}", to_math(value, syntax)?);
                } else {
                    let _ = write!(out, "{name} &= {}", to_math(value, syntax)?);
                }
            }
            out.push_str(if latex { "\\end{cases}" } else { ")" });
            Ok(out)
        },
        | Value::Variant(variant) if variant.fields.is_empty() => {
            Ok(if latex {
                format!("\\mathrm{{{}}}", variant.name)
            } else {
                variant.name.clone()
            })
        },
        | Value::Variant(_) => Err("variant payloads have no mathematical form".to_string()),
        | Value::Closure(_) | Value::Native(_) => {
            Err("a function has no mathematical form".to_string())
        },
        | Value::Ctor { .. } => Err("a constructor has no mathematical form".to_string()),
        | Value::UnitOf(unit) => {
            Ok(if latex {
                format!("\\mathrm{{{}}}", unit.display)
            } else {
                unit.display.clone()
            })
        },
        | Value::Io(_) => Err("an IO action has no mathematical form".to_string()),
        | Value::Prop(prop) => Ok(prop.text.clone()),
        | Value::Dyn(dynamic) => Ok(math_scalar(&dynamic.as_scalar(), syntax)),
        | Value::KernelValue(_)
        | Value::KernelError(_)
        | Value::TypeRep(_)
        | Value::Callback(_) => Err("an opaque kernel value has no mathematical form".to_string()),
        | Value::Error => Err("an error value has no mathematical form".to_string()),
    }
}

fn math_scalar(
    num: &NumVal,
    syntax: MathSyntax,
) -> String {
    let body = match &num.kind {
        | NumKind::Sym(term) => crate::sym::term_to_math(term, syntax == MathSyntax::Latex),
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
        | NumKind::Float(value) => format_float(*value),
        | NumKind::Complex(re, im) => {
            let sign = if *im < 0.0 { "-" } else { "+" };
            format!("{}{} {} i", format_float(*re), sign, format_float(im.abs()))
        },
    };
    let suffix = si_unit_text(&num.dim);
    if suffix.is_empty() {
        body
    } else {
        format!("{body}\\ \\mathrm{{{suffix}}}")
    }
}

fn math_tensor(
    tensor: &TensorData,
    syntax: MathSyntax,
) -> Result<String, String> {
    if tensor.rank() > 2 {
        return Err("only rank 1 and 2 tensors have a matrix form".to_string());
    }
    let open = if syntax == MathSyntax::Latex {
        "\\begin{pmatrix}"
    } else {
        "mat("
    };
    let close = if syntax == MathSyntax::Latex {
        "\\end{pmatrix}"
    } else {
        ")"
    };
    let mut rows = Vec::new();
    if tensor.rank() == 1 {
        for index in 0..tensor.shape[0] {
            let cell = tensor
                .get(&[index as isize])
                .ok_or_else(|| "tensor index out of range".to_string())?;
            rows.push(to_math(&cell, syntax)?);
        }
        return Ok(format!(
            "{open}{}{close}",
            rows.join(if syntax == MathSyntax::Latex {
                " & "
            } else {
                ", "
            })
        ));
    }
    for row in 0..tensor.shape[0] {
        let mut cells = Vec::with_capacity(tensor.shape[1]);
        for column in 0..tensor.shape[1] {
            let cell = tensor
                .get(&[row as isize, column as isize])
                .ok_or_else(|| "tensor index out of range".to_string())?;
            cells.push(to_math(&cell, syntax)?);
        }
        rows.push(cells.join(if syntax == MathSyntax::Latex {
            " & "
        } else {
            ", "
        }));
    }
    Ok(format!(
        "{open}{}{close}",
        rows.join(if syntax == MathSyntax::Latex {
            " \\\\ "
        } else {
            "; "
        })
    ))
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
fn numbers_equal(
    left: &NumVal,
    right: &NumVal,
) -> bool {
    match (&left.kind, &right.kind) {
        | (NumKind::Sym(_), _) | (_, NumKind::Sym(_)) => false,
        | (NumKind::Complex(ar, ai), NumKind::Complex(br, bi)) => ar == br && ai == bi,
        | (NumKind::Complex(..), _) | (_, NumKind::Complex(..)) => false,
        | (NumKind::Float(a), NumKind::Float(b)) => a == b,
        | _ => {
            match (left.to_rational(), right.to_rational()) {
                | (Some(a), Some(b)) => a == b,
                | _ => false,
            }
        },
    }
}

fn tensors_equal(
    left: &TensorData,
    right: &TensorData,
) -> Result<bool, NotEqual> {
    if left.shape != right.shape {
        return Ok(false);
    }
    for index in 0..left.storage.len() {
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
pub(crate) fn mark_source_ctor(value: &mut Value) {
    match value {
        | Value::Ctor {
            adt,
            index,
            name,
            arity,
            args,
            origin,
        } => {
            let valid = args.is_empty()
                && aevia_types::adt::get(*adt)
                    .and_then(|entry| entry.ctors.get(*index).cloned())
                    .is_some_and(|ctor| ctor.name == *name && ctor.fields.len() == *arity);
            if valid {
                *origin = Some(SourceCtorToken {
                    adt: *adt,
                    index: *index,
                    name: name.clone(),
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
pub(crate) fn mark_ctor_result(
    value: &mut Value,
    origin: &SourceCtorToken,
) {
    if let Value::Variant(variant) = value {
        if variant.adt == origin.adt && variant.index == origin.index {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(show_tensor_in(&tensor, false), "[[1.0, 2.0], [3.0, 4.0]]");
    }

    fn tensor(
        shape: Vec<usize>,
        values: Vec<f64>,
    ) -> Value {
        Value::Tensor(Rc::new(TensorData {
            verified_element: None,
            shape,
            storage: Storage::Dense(Rc::new(values)),
            dim: Dim::one(),
            display: None,
        }))
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
