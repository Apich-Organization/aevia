//! Types, rows, schemes and their display form.
//!
//! The internal form follows §3.2: every numeric scalar is `Scalar<ρ, δ>`, and
//! the surface spellings `Int`, `Real[d]`, `Float[d]` and `Complex[d]` are
//! abbreviations that the parser produces and the printer reproduces.
//!
//! Records use rows, so `{ x: Real[L] }` and `{ x: Real[L], y: Real[T] }` can be
//! unified when a function is written against an open row
//! (`{ x: Real[L], ...r }`). Variants are nominal: two structurally identical
//! declarations are still different types, which is what makes `Shape` and
//! `Rect` in different modules distinct.

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

use aevia_dim::Dim;
use num_traits::One;
use num_traits::Signed;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::hash_map::Entry;
use std::fmt;

/// Identifier of a nominal algebraic data type.
pub type AdtId = u32;

/// The representation of a scalar.
///
/// `Rep::Var` is a representation variable: `sqrt` and `+` are polymorphic in
/// it, and it is resolved by unification or by the defaulting rules of §3.10.2.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
    bincode_next::Fingerprint,
)]
pub enum Rep {
    /// Arbitrary-precision integer; always dimensionless.
    Int,
    /// Exact rational or symbolic real.
    Real,
    /// IEEE-754 binary64.
    Float,
    /// Complex scalar.
    Complex,
    /// A representation variable.
    Var(u32),
}

impl Rep {
    /// The default representation for a variable, per §3.10.2.
    #[must_use]
    pub const fn default_for_literal(kind: LiteralKind) -> Self {
        match kind {
            | LiteralKind::Int => Self::Int,
            | LiteralKind::Decimal | LiteralKind::QuantityDecimal => Self::Float,
            | LiteralKind::QuantityInt => Self::Real,
        }
    }

    /// `div_rep` of §3.4.2: division promotes integers to exact rationals.
    #[must_use]
    pub const fn div_rep(self) -> Self {
        match self {
            | Self::Int => Self::Real,
            | other => other,
        }
    }

    /// The least upper bound of two *concrete* representations, per §3.4.1.
    ///
    /// Equal operands retain their representation. A variable paired with a
    /// different representation needs a constraint before it can join.
    #[must_use]
    pub fn join_concrete(
        self,
        other: Self,
    ) -> Option<Self> {
        use Rep::Complex;
        use Rep::Float;
        use Rep::Int;
        use Rep::Real;
        if self == other {
            return Some(self);
        }
        match (self, other) {
            // §3.4.1: `Real` wins over `Float` to preserve exactness.
            | (Int | Float, Real) | (Real, Int | Float) => Some(Real),
            | (Int, Float) | (Float, Int) => Some(Float),
            | (Int | Real | Float, Complex) | (Complex, Int | Real | Float) => Some(Complex),
            | _ => None,
        }
    }

    /// Whether `self` is at least `lower` in the lattice of §3.4.1.
    ///
    /// `Int < Float < Complex` and `Int < Real < Complex`; `Float` and `Real`
    /// are incomparable.
    #[must_use]
    pub fn at_least(
        self,
        lower: Self,
    ) -> bool {
        use Rep::Complex;
        use Rep::Float;
        use Rep::Int;
        use Rep::Real;
        if self == lower {
            return true;
        }
        match (self, lower) {
            | (_, Int) => matches!(self, Real | Float | Complex),
            | (Complex, Real | Float) => true,
            | _ => false,
        }
    }

    /// The concrete representations a variable may still take, given its
    /// lower bounds, in increasing order.
    #[must_use]
    pub const fn upper_candidates(lower: Self) -> &'static [Self] {
        use Rep::Complex;
        use Rep::Float;
        use Rep::Int;
        use Rep::Real;
        match lower {
            | Int | Self::Var(_) => &[Int, Float, Real, Complex],
            | Float => &[Float, Complex],
            | Real => &[Real, Complex],
            | Complex => &[Complex],
        }
    }

    /// The surface name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            | Self::Int => "Int",
            | Self::Real => "Real",
            | Self::Float => "Float",
            | Self::Complex => "Complex",
            | Self::Var(_) => "?",
        }
    }
}

impl fmt::Display for Rep {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            | Self::Var(id) => write!(f, "ρ{id}"),
            | other => f.write_str(other.name()),
        }
    }
}

/// What kind of literal introduced a type variable.
///
/// Literals are representation-polymorphic (§3.10.1): `1` can become `Int`,
/// `Real`, `Float` or `Complex` depending on context, but never a dimensioned
/// scalar — for that you write `1 m`.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
    bincode_next::Fingerprint,
)]
pub enum LiteralKind {
    /// An integer literal: default `Int`, dimensionless only.
    Int,
    /// A decimal literal: default `Float[1]`, dimensionless only.
    Decimal,
    /// `3 m`: default `Real[L]`, may become `Float[L]`.
    QuantityInt,
    /// `3.0 m`: default `Float[L]`, may become `Real[L]`.
    QuantityDecimal,
}

impl LiteralKind {
    /// Whether the literal carries a dimension from its unit.
    #[must_use]
    pub const fn is_quantity(self) -> bool {
        matches!(self, Self::QuantityInt | Self::QuantityDecimal)
    }

    /// Whether the literal may unify with a given representation.
    #[must_use]
    pub const fn accepts_rep(
        self,
        rep: Rep,
    ) -> bool {
        match self {
            | Self::Int => matches!(rep, Rep::Int | Rep::Real | Rep::Float | Rep::Complex),
            | Self::Decimal => matches!(rep, Rep::Float | Rep::Real),
            | Self::QuantityInt => matches!(rep, Rep::Real | Rep::Float | Rep::Complex),
            | Self::QuantityDecimal => matches!(rep, Rep::Float | Rep::Real),
        }
    }
}

/// A type-level natural number (§3.5.1, §13.9.1).
///
/// The language offers literals, `+`, `*` by a literal, and linear equations;
/// there is no general recursion or proof terms. A variable is solved by
/// [`crate::infer::Infer::unify_nat`] when a linear equation isolates it
/// (`n + 1 ~ 3` ⇒ `n = 2`, `2 * n ~ 6` ⇒ `n = 3`).
#[derive(
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
)]
pub enum Nat {
    /// A concrete extent.
    Lit(usize),
    /// A type-level variable.
    Var(u32),
    /// `a + b`.
    Add(Box<Self>, Box<Self>),
    /// `a * b`. Only a constant factor makes the equation linear; a product of
    /// two variables is rejected (`E0304`).
    Mul(Box<Self>, Box<Self>),
}

impl<C: bincode_next::config::Config> bincode_next::Fingerprint<C> for Nat {
    const SCHEMA_HASH: u64 = bincode_next::rapidhash::v3::rapidhash_v3_seeded(
        b"Nat:v1:Lit(usize)|Var(u32)|Add|Mul",
        &bincode_next::rapidhash::v3::RapidSecrets::seed_cpp(C::CONFIG_HASH),
    );
}

impl Nat {
    /// A literal.
    #[must_use]
    pub const fn lit(value: usize) -> Self {
        Self::Lit(value)
    }

    /// A variable.
    #[must_use]
    pub const fn var(id: u32) -> Self {
        Self::Var(id)
    }

    /// `self + other`.
    #[must_use]
    #[allow(clippy::should_implement_trait)] // exponent arithmetic on `Copy` values, not `std::ops::Add`
    pub fn add(
        self,
        other: Self,
    ) -> Self {
        Self::Add(Box::new(self), Box::new(other))
    }

    /// `self * other`.
    #[must_use]
    #[allow(clippy::should_implement_trait)] // exponent arithmetic on `Copy` values, not `std::ops::Mul`
    pub fn mul(
        self,
        other: Self,
    ) -> Self {
        Self::Mul(Box::new(self), Box::new(other))
    }

    /// The value when the term folds to a literal.
    #[must_use]
    pub fn as_const(&self) -> Option<usize> {
        match self {
            | Self::Lit(value) => Some(*value),
            | Self::Var(_) => None,
            | Self::Add(left, right) => Some(left.as_const()?.checked_add(right.as_const()?)?),
            | Self::Mul(left, right) => Some(left.as_const()?.checked_mul(right.as_const()?)?),
        }
    }

    pub(crate) fn fold_add(
        self,
        other: Self,
    ) -> Self {
        let folded = self
            .as_const()
            .zip(other.as_const())
            .and_then(|(left, right)| left.checked_add(right));
        folded.map_or_else(|| self.add(other), Self::Lit)
    }

    pub(crate) fn fold_mul(
        self,
        other: Self,
    ) -> Self {
        let folded = self
            .as_const()
            .zip(other.as_const())
            .and_then(|(left, right)| left.checked_mul(right));
        folded.map_or_else(|| self.mul(other), Self::Lit)
    }

    /// Whether the term is linear: every product has at least one literal
    /// factor, so it expands to `Σ cᵢ·vᵢ + c`.
    #[must_use]
    pub fn is_linear(&self) -> bool {
        match self {
            | Self::Lit(_) | Self::Var(_) => true,
            | Self::Add(left, right) => left.is_linear() && right.is_linear(),
            | Self::Mul(left, right) => {
                left.as_const().is_some() && right.is_linear()
                    || right.as_const().is_some() && left.is_linear()
            },
        }
    }

    /// The variables this term mentions.
    #[must_use]
    pub fn vars(&self) -> Vec<u32> {
        let mut out = Vec::new();
        self.collect_vars(&mut out);
        out
    }

    fn collect_vars(
        &self,
        out: &mut Vec<u32>,
    ) {
        match self {
            | Self::Lit(_) => {},
            | Self::Var(id) => {
                if !out.contains(id) {
                    out.push(*id);
                }
            },
            | Self::Add(left, right) | Self::Mul(left, right) => {
                left.collect_vars(out);
                right.collect_vars(out);
            },
        }
    }

    /// Substitutes through `map`, folding constants that fit a machine extent.
    #[must_use]
    pub fn substitute(
        &self,
        map: &std::collections::HashMap<u32, Self>,
    ) -> Self {
        match self {
            | Self::Lit(value) => Self::Lit(*value),
            | Self::Var(id) => map.get(id).cloned().unwrap_or_else(|| self.clone()),
            | Self::Add(left, right) => left.substitute(map).fold_add(right.substitute(map)),
            | Self::Mul(left, right) => left.substitute(map).fold_mul(right.substitute(map)),
        }
    }
}

impl From<usize> for Nat {
    fn from(value: usize) -> Self {
        Self::Lit(value)
    }
}

impl From<u32> for Nat {
    fn from(value: u32) -> Self {
        Self::Lit(usize::try_from(value).unwrap_or(usize::MAX))
    }
}

impl fmt::Display for Nat {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            | Self::Lit(value) => write!(f, "{value}"),
            | Self::Var(id) => write!(f, "n{id}"),
            | Self::Add(left, right) => write!(f, "({left} + {right})"),
            | Self::Mul(left, right) => write!(f, "({left} * {right})"),
        }
    }
}

/// A tensor shape annotation.
///
/// `Rank(n)` is `Tensor<n, τ>`: the rank is known (a concrete `u32` — indexing
/// requires it, §3.5.4), the extents are not. `Refined` is
/// `Tensor<[n1, ..., nk], τ>` with `None` for `?`; each extent is a type-level
/// [`Nat`], so `reshape<[n + 1, 2]>` is expressible.
#[derive(
    Clone,
    PartialEq,
    Eq,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
    bincode_next::Fingerprint,
)]
pub enum Shape {
    /// Only the rank is known.
    Rank(u32),
    /// A refined shape; `None` entries are unknown extents.
    Refined(Vec<Option<Nat>>),
}

impl Shape {
    /// The rank, when it is known.
    #[must_use]
    pub fn rank(&self) -> u32 {
        match self {
            | Self::Rank(rank) => *rank,
            | Self::Refined(items) => u32::try_from(items.len()).unwrap_or(u32::MAX),
        }
    }

    /// A rank-only shape, for code that constructs one from a `u32`.
    #[must_use]
    pub const fn rank_only(rank: u32) -> Self {
        Self::Rank(rank)
    }

    /// A refined shape from literal extents (`None` stays unknown).
    #[must_use]
    pub fn refined(items: impl IntoIterator<Item = Option<usize>>) -> Self {
        Self::Refined(items.into_iter().map(|item| item.map(Nat::lit)).collect())
    }

    /// The extents when every one folds to a literal.
    #[must_use]
    pub fn extents(&self) -> Option<Vec<usize>> {
        match self {
            | Self::Rank(_) => None,
            | Self::Refined(items) => {
                items
                    .iter()
                    .map(|item| item.as_ref().and_then(Nat::as_const))
                    .collect()
            },
        }
    }

    /// Whether the shape mentions a type-level variable.
    #[must_use]
    pub fn has_variables(&self) -> bool {
        match self {
            | Self::Rank(_) => false,
            | Self::Refined(items) => items.iter().flatten().any(|nat| !nat.vars().is_empty()),
        }
    }

    /// The extent at `index`, when it is present and folds to a literal.
    #[must_use]
    pub fn extent(
        &self,
        index: usize,
    ) -> Option<usize> {
        match self {
            | Self::Rank(_) => None,
            | Self::Refined(items) => items.get(index)?.as_ref().and_then(Nat::as_const),
        }
    }

    /// Substitutes type-level variables (parallel to [`Nat::substitute`]).
    #[must_use]
    pub fn substitute_nats(
        &self,
        map: &std::collections::HashMap<u32, Nat>,
    ) -> Self {
        match self {
            | Self::Rank(rank) => Self::Rank(*rank),
            | Self::Refined(items) => {
                Self::Refined(
                    items
                        .iter()
                        .map(|item| item.as_ref().map(|nat| nat.substitute(map)))
                        .collect(),
                )
            },
        }
    }

    /// A refined shape with all extents unknown, for a given rank.
    #[must_use]
    pub const fn unknown_of_rank(rank: u32) -> Self {
        Self::Rank(rank)
    }
}

impl fmt::Display for Shape {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            | Self::Rank(rank) => write!(f, "{rank}"),
            | Self::Refined(items) => {
                f.write_str("[")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    match item {
                        | Some(extent) => write!(f, "{extent}")?,
                        | None => f.write_str("?")?,
                    }
                }
                f.write_str("]")
            },
        }
    }
}

/// A record row: labels in order, terminated by an empty row or a row variable.
#[derive(
    Clone,
    PartialEq,
    Eq,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
)]
pub enum Row {
    /// The closed empty row.
    Empty,
    /// A field followed by the rest.
    Extend {
        /// The field name.
        label: String,
        /// The field type.
        ty: Box<Type>,
        /// The remaining fields.
        tail: Box<Self>,
    },
    /// An unknown tail, for open records.
    Var(u32),
}

impl Row {
    /// Builds a closed row from fields, replacing an existing label.
    #[must_use]
    pub fn closed(fields: impl IntoIterator<Item = (String, Type)>) -> Self {
        let mut map: BTreeMap<String, Type> = BTreeMap::new();
        for (label, ty) in fields {
            map.insert(label, ty);
        }
        let mut row = Self::Empty;
        for (label, ty) in map.into_iter().rev() {
            row = Self::Extend {
                label,
                ty: Box::new(ty),
                tail: Box::new(row),
            };
        }
        row
    }

    /// Borrows the fields of a row in order.
    pub(crate) fn iter_fields(&self) -> impl Iterator<Item = (&str, &Type)> {
        let mut next = self;
        std::iter::from_fn(move || {
            let Self::Extend { label, ty, tail } = next else {
                return None;
            };
            next = tail;
            Some((label.as_str(), ty.as_ref()))
        })
    }

    /// The fields of a row, in order, ignoring the tail.
    #[must_use]
    pub fn fields(&self) -> Vec<(String, Type)> {
        self.iter_fields()
            .map(|(label, ty)| (label.to_string(), ty.clone()))
            .collect()
    }

    /// The tail variable, when the row is open.
    #[must_use]
    pub fn tail_var(&self) -> Option<u32> {
        let mut current = self;
        loop {
            match current {
                | Self::Empty => return None,
                | Self::Var(id) => return Some(*id),
                | Self::Extend { tail, .. } => current = tail,
            }
        }
    }

    /// The type of a field, if present.
    #[must_use]
    pub fn field(
        &self,
        name: &str,
    ) -> Option<&Type> {
        self.iter_fields()
            .find_map(|(label, ty)| (label == name).then_some(ty))
    }

    /// The first label that appears twice, if any (§3.6.1: `E0208`).
    #[must_use]
    pub fn duplicate_label(&self) -> Option<String> {
        let mut seen = HashSet::new();
        self.iter_fields()
            .find_map(|(label, _)| (!seen.insert(label)).then(|| label.to_string()))
    }

    /// Replaces an existing field or inserts it before the row tail (§3.6.2).
    #[must_use]
    pub fn update(
        mut self,
        label: &str,
        ty: Type,
    ) -> Self {
        let mut current = &mut self;
        loop {
            match current {
                | Self::Extend {
                    label: existing,
                    ty: stored,
                    tail,
                } => {
                    if existing == label {
                        **stored = ty;
                        break;
                    }
                    current = tail;
                },
                | Self::Empty | Self::Var(_) => {
                    current.insert_before_tail(label, ty);
                    break;
                },
            }
        }
        self
    }

    /// Updates fields in input order using one traversal of the original row.
    /// The callback receives the open tail only when the label is new.
    #[allow(
        clippy::single_call_fn,
        reason = "keeps the mutable row representation and its indexing private to the type layer"
    )]
    pub(crate) fn update_many<'a, T>(
        &'a mut self,
        fields: impl ExactSizeIterator<Item = (&'a str, T)>,
        mut field_type: impl FnMut(&str, T, Option<u32>) -> Type,
    ) {
        let mut slots = HashMap::with_capacity(fields.len());
        let mut end = self;
        while let Self::Extend { label, ty, tail } = end {
            slots.entry(label.as_str()).or_insert_with(|| ty.as_mut());
            end = tail.as_mut();
        }
        let tail_var = match end {
            | Self::Var(id) => Some(*id),
            | _ => None,
        };
        for (label, value) in fields {
            match slots.entry(label) {
                | Entry::Occupied(mut slot) => **slot.get_mut() = field_type(label, value, None),
                | Entry::Vacant(slot) => {
                    end.insert_before_tail(label, field_type(label, value, tail_var));
                    if let Self::Extend { ty, tail, .. } = end {
                        slot.insert(ty.as_mut());
                        end = tail.as_mut();
                    }
                },
            }
        }
    }

    fn insert_before_tail(
        &mut self,
        label: &str,
        ty: Type,
    ) {
        let prior_tail = std::mem::replace(self, Self::Empty);
        *self = Self::Extend {
            label: label.to_string(),
            ty: Box::new(ty),
            tail: Box::new(prior_tail),
        };
    }
}

impl fmt::Display for Row {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        let mut separator = "";
        for (label, ty) in self.iter_fields() {
            write!(f, "{separator}{label}: {ty}")?;
            separator = ", ";
        }
        if let Some(id) = self.tail_var() {
            write!(f, "{separator}...r{id}")?;
        }
        Ok(())
    }
}

/// A rank-polymorphic tensor constructor, preserved across value aliases.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
    bincode_next::Fingerprint,
)]
pub enum TensorIntrinsic {
    /// Construct a tensor of zeros.
    Zeros,
    /// Construct a tensor of ones.
    Ones,
    /// Construct a tensor filled with one scalar.
    Fill,
}

impl TensorIntrinsic {
    /// The name used by the runtime builtin dispatcher.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            | Self::Zeros => "zeros",
            | Self::Ones => "ones",
            | Self::Fill => "fill",
        }
    }

    /// Look up an ambient constructor.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            | "zeros" => Some(Self::Zeros),
            | "ones" => Some(Self::Ones),
            | "fill" => Some(Self::Fill),
            | _ => None,
        }
    }
}

/// The callable identity and optional explicitly selected element representation.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
    bincode_next::Fingerprint,
)]
pub struct TensorIntrinsicType {
    /// The constructor's operation.
    pub kind: TensorIntrinsic,
    /// An explicit, dimensionless numeric element representation, if selected.
    pub element: Option<Rep>,
}

/// A type.
#[derive(Clone, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum Type {
    /// `Scalar<ρ, δ>`: every numeric scalar.
    Scalar(Rep, Dim),
    /// `Bool`.
    Bool,
    /// `Prop`: a symbolic proposition.
    Prop,
    /// `String`.
    Str,
    /// `()`.
    Unit,
    /// `Tensor<shape, τ>`.
    Tensor {
        /// The rank or refined shape.
        shape: Shape,
        /// The element type.
        elem: Box<Self>,
    },
    /// `Point[d]`: an affine point such as an absolute temperature.
    Point(Dim),
    /// `Unit[d]`: a unit value.
    UnitOf(Dim),
    /// A variadic tensor constructor; its result rank depends on its call.
    TensorIntrinsic(TensorIntrinsicType),
    /// A type reflection constructor, optionally specialized to a concrete type.
    TypeRepIntrinsic(Option<Box<Self>>),
    /// A checked opaque-value conversion, optionally specialized to its result type.
    FromValueIntrinsic(Option<Box<Self>>),
    /// An NPY reader specialized to a statically known tensor rank.
    ReadNpyIntrinsic(Option<u32>),
    /// An NPY writer accepting dimensionless Float tensors of any rank.
    WriteNpyIntrinsic,
    /// A checked conversion of a concrete function into an opaque callback.
    ToCallbackIntrinsic,
    /// `(τ1, ..., τn) -> τ`.
    Fn(Vec<Self>, Box<Self>),
    /// A record with a row.
    Record(Row),
    /// A nominal variant type with arguments.
    Adt(#[serde(with = "crate::persist::adt_ref")] AdtId, Vec<Self>),
    /// `IO τ`.
    Io(Box<Self>),
    /// `Dyn<ρ>`.
    Dyn(Rep),
    /// A type variable.
    Var(u32),
    /// An ADT or alias parameter, substituted on use.
    Param(u32),
    /// The type of an expression that already failed to check.
    Error,
}

pub(crate) enum TypeVariableRef<'a> {
    Type(u32),
    Representation(Rep),
    Dimension(&'a Dim),
    Extents(&'a [Option<Nat>]),
    Row(u32),
}

impl<C: bincode_next::config::Config> bincode_next::Fingerprint<C> for Row {
    const SCHEMA_HASH: u64 = bincode_next::rapidhash::v3::rapidhash_v3_seeded(
        b"Row:v1:Empty|Extend(String,Type,Row)|Var(u32)",
        &bincode_next::rapidhash::v3::RapidSecrets::seed_cpp(C::CONFIG_HASH),
    );
}

impl<C: bincode_next::config::Config> bincode_next::Fingerprint<C> for Type {
    const SCHEMA_HASH: u64 = {
        let h = crate::persist::mix_hash(
            C::CONFIG_HASH,
            b"Type:v1:Dim",
            <Dim as bincode_next::Fingerprint<C>>::SCHEMA_HASH,
        );
        let h = crate::persist::mix_hash(
            h,
            b"Type:v1:Rep",
            <Rep as bincode_next::Fingerprint<C>>::SCHEMA_HASH,
        );
        let h = crate::persist::mix_hash(
            h,
            b"Type:v1:Shape",
            <Shape as bincode_next::Fingerprint<C>>::SCHEMA_HASH,
        );
        crate::persist::mix_hash(
            h,
            b"Type:v1:Row",
            <Row as bincode_next::Fingerprint<C>>::SCHEMA_HASH,
        )
    };
}

impl bincode_next::Encode for Type {
    fn encode<E: bincode_next::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode_next::error::EncodeError> {
        match self {
            | Self::Scalar(rep, dim) => {
                0u8.encode(encoder)?;
                rep.encode(encoder)?;
                dim.encode(encoder)
            },
            | Self::Bool => 1u8.encode(encoder),
            | Self::Prop => 2u8.encode(encoder),
            | Self::Str => 3u8.encode(encoder),
            | Self::Unit => 4u8.encode(encoder),
            | Self::Tensor { shape, elem } => {
                5u8.encode(encoder)?;
                shape.encode(encoder)?;
                elem.encode(encoder)
            },
            | Self::Point(dim) => {
                6u8.encode(encoder)?;
                dim.encode(encoder)
            },
            | Self::UnitOf(dim) => {
                7u8.encode(encoder)?;
                dim.encode(encoder)
            },
            | Self::TensorIntrinsic(t) => {
                8u8.encode(encoder)?;
                t.encode(encoder)
            },
            | Self::TypeRepIntrinsic(t) => {
                9u8.encode(encoder)?;
                t.encode(encoder)
            },
            | Self::FromValueIntrinsic(t) => {
                10u8.encode(encoder)?;
                t.encode(encoder)
            },
            | Self::ReadNpyIntrinsic(t) => {
                11u8.encode(encoder)?;
                t.encode(encoder)
            },
            | Self::WriteNpyIntrinsic => 12u8.encode(encoder),
            | Self::ToCallbackIntrinsic => 13u8.encode(encoder),
            | Self::Fn(params, ret) => {
                14u8.encode(encoder)?;
                params.encode(encoder)?;
                ret.encode(encoder)
            },
            | Self::Record(row) => {
                15u8.encode(encoder)?;
                row.encode(encoder)
            },
            | Self::Adt(id, args) => {
                16u8.encode(encoder)?;
                crate::persist::encode_adt_id(*id)
                    .map_err(bincode_next::error::EncodeError::OtherString)?
                    .encode(encoder)?;
                args.encode(encoder)
            },
            | Self::Io(inner) => {
                17u8.encode(encoder)?;
                inner.encode(encoder)
            },
            | Self::Dyn(rep) => {
                18u8.encode(encoder)?;
                rep.encode(encoder)
            },
            | Self::Var(v) => {
                19u8.encode(encoder)?;
                v.encode(encoder)
            },
            | Self::Param(p) => {
                20u8.encode(encoder)?;
                p.encode(encoder)
            },
            | Self::Error => 21u8.encode(encoder),
        }
    }
}

impl<Context> bincode_next::Decode<Context> for Type {
    fn decode<D: bincode_next::de::Decoder<Context = Context>>(
        decoder: &mut D
    ) -> Result<Self, bincode_next::error::DecodeError> {
        match u8::decode(decoder)? {
            | 0 => Ok(Self::Scalar(Rep::decode(decoder)?, Dim::decode(decoder)?)),
            | 1 => Ok(Self::Bool),
            | 2 => Ok(Self::Prop),
            | 3 => Ok(Self::Str),
            | 4 => Ok(Self::Unit),
            | 5 => {
                Ok(Self::Tensor {
                    shape: Shape::decode(decoder)?,
                    elem: Box::<Self>::decode(decoder)?,
                })
            },
            | 6 => Ok(Self::Point(Dim::decode(decoder)?)),
            | 7 => Ok(Self::UnitOf(Dim::decode(decoder)?)),
            | 8 => Ok(Self::TensorIntrinsic(TensorIntrinsicType::decode(decoder)?)),
            | 9 => {
                Ok(Self::TypeRepIntrinsic(Option::<Box<Self>>::decode(
                    decoder,
                )?))
            },
            | 10 => {
                Ok(Self::FromValueIntrinsic(Option::<Box<Self>>::decode(
                    decoder,
                )?))
            },
            | 11 => Ok(Self::ReadNpyIntrinsic(Option::<u32>::decode(decoder)?)),
            | 12 => Ok(Self::WriteNpyIntrinsic),
            | 13 => Ok(Self::ToCallbackIntrinsic),
            | 14 => {
                Ok(Self::Fn(
                    Vec::<Self>::decode(decoder)?,
                    Box::<Self>::decode(decoder)?,
                ))
            },
            | 15 => Ok(Self::Record(Row::decode(decoder)?)),
            | 16 => {
                let index = u32::decode(decoder)?;
                let id = crate::persist::decode_adt_id(index)
                    .map_err(bincode_next::error::DecodeError::OtherString)?;
                let args = Vec::<Self>::decode(decoder)?;
                Ok(Self::Adt(id, args))
            },
            | 17 => Ok(Self::Io(Box::<Self>::decode(decoder)?)),
            | 18 => Ok(Self::Dyn(Rep::decode(decoder)?)),
            | 19 => Ok(Self::Var(u32::decode(decoder)?)),
            | 20 => Ok(Self::Param(u32::decode(decoder)?)),
            | 21 => Ok(Self::Error),
            | tag => {
                Err(bincode_next::error::DecodeError::UnexpectedVariant {
                    type_name: "Type",
                    allowed: &bincode_next::error::AllowedEnumVariants::Range { min: 0, max: 21 },
                    found: u32::from(tag),
                })
            },
        }
    }
}

impl<'de, Context> bincode_next::BorrowDecode<'de, Context> for Type {
    fn borrow_decode<D: bincode_next::de::BorrowDecoder<'de, Context = Context>>(
        decoder: &mut D
    ) -> Result<Self, bincode_next::error::DecodeError> {
        bincode_next::Decode::decode(decoder)
    }
}

impl Type {
    /// Visits variable-bearing parts in freshening order without cloning the type.
    pub(crate) fn visit_variables(
        &self,
        visit: &mut impl FnMut(TypeVariableRef<'_>),
    ) {
        match self {
            | Self::Var(id) => visit(TypeVariableRef::Type(*id)),
            | Self::Scalar(rep, dim) => {
                visit(TypeVariableRef::Representation(*rep));
                visit(TypeVariableRef::Dimension(dim));
            },
            | Self::Dyn(rep) => visit(TypeVariableRef::Representation(*rep)),
            | Self::Point(dim) | Self::UnitOf(dim) => visit(TypeVariableRef::Dimension(dim)),
            | Self::Tensor { shape, elem } => {
                if let Shape::Refined(items) = shape {
                    visit(TypeVariableRef::Extents(items));
                }
                elem.visit_variables(visit);
            },
            | Self::Fn(args, ret) => {
                for arg in args {
                    arg.visit_variables(visit);
                }
                ret.visit_variables(visit);
            },
            | Self::Record(row) => {
                let mut current = row;
                while let Row::Extend { ty, tail, .. } = current {
                    ty.visit_variables(visit);
                    current = tail;
                }
                if let Row::Var(id) = current {
                    visit(TypeVariableRef::Row(*id));
                }
            },
            | Self::Adt(_, args) => {
                for arg in args {
                    arg.visit_variables(visit);
                }
            },
            | Self::Io(value)
            | Self::TypeRepIntrinsic(Some(value))
            | Self::FromValueIntrinsic(Some(value)) => value.visit_variables(visit),
            | Self::TensorIntrinsic(intrinsic) => {
                if let Some(rep) = intrinsic.element {
                    visit(TypeVariableRef::Representation(rep));
                }
            },
            | Self::TypeRepIntrinsic(None)
            | Self::FromValueIntrinsic(None)
            | Self::Bool
            | Self::Prop
            | Self::Str
            | Self::Unit
            | Self::Param(_)
            | Self::Error
            | Self::WriteNpyIntrinsic
            | Self::ReadNpyIntrinsic(_)
            | Self::ToCallbackIntrinsic => {},
        }
    }

    /// Visits type and representation variables that connect numeric constraints.
    pub(crate) fn visit_numeric_variables(
        &self,
        mut visit: impl FnMut(u32),
    ) {
        self.visit_variables(&mut |variable| {
            if let TypeVariableRef::Type(id) | TypeVariableRef::Representation(Rep::Var(id)) =
                variable
            {
                visit(id);
            }
        });
    }

    /// `Int`, that is `Scalar<Int, 1>`.
    #[must_use]
    pub fn int() -> Self {
        Self::Scalar(Rep::Int, Dim::one())
    }

    /// `Real[d]`.
    #[must_use]
    pub const fn real(dim: Dim) -> Self {
        Self::Scalar(Rep::Real, dim)
    }

    /// `Float[d]`.
    #[must_use]
    pub const fn float(dim: Dim) -> Self {
        Self::Scalar(Rep::Float, dim)
    }

    /// `Complex[d]`.
    #[must_use]
    pub const fn complex(dim: Dim) -> Self {
        Self::Scalar(Rep::Complex, dim)
    }

    /// A scalar with a representation and dimension.
    #[must_use]
    pub const fn scalar(
        rep: Rep,
        dim: Dim,
    ) -> Self {
        Self::Scalar(rep, dim)
    }

    /// `Bool`.
    #[must_use]
    pub const fn bool() -> Self {
        Self::Bool
    }

    /// `String`.
    #[must_use]
    pub const fn string() -> Self {
        Self::Str
    }

    /// `()`.
    #[must_use]
    pub const fn unit() -> Self {
        Self::Unit
    }

    /// `Tensor<shape, elem>`.
    #[must_use]
    pub fn tensor(
        shape: Shape,
        elem: Self,
    ) -> Self {
        Self::Tensor {
            shape,
            elem: Box::new(elem),
        }
    }

    /// A rank-only tensor.
    #[must_use]
    pub fn tensor_rank(
        rank: u32,
        elem: Self,
    ) -> Self {
        Self::tensor(Shape::Rank(rank), elem)
    }

    /// A function type.
    #[must_use]
    pub fn func(
        args: Vec<Self>,
        ret: Self,
    ) -> Self {
        Self::Fn(args, Box::new(ret))
    }

    /// `IO τ`.
    #[must_use]
    pub fn io(inner: Self) -> Self {
        Self::Io(Box::new(inner))
    }

    /// A record from fields.
    #[must_use]
    pub fn record(fields: impl IntoIterator<Item = (String, Self)>) -> Self {
        Self::Record(Row::closed(fields))
    }

    /// `Option<τ>` as an ADT reference.
    #[must_use]
    pub fn option(inner: Self) -> Self {
        Self::Adt(crate::adt::OPTION_ID, vec![inner])
    }

    /// `Result<τ, ε>` as an ADT reference.
    #[must_use]
    pub fn result(
        ok: Self,
        err: Self,
    ) -> Self {
        Self::Adt(crate::adt::RESULT_ID, vec![ok, err])
    }

    /// The scalar representation and dimension, when this is a scalar.
    #[must_use]
    pub const fn as_scalar(&self) -> Option<(Rep, &Dim)> {
        match self {
            | Self::Scalar(rep, dim) => Some((*rep, dim)),
            | _ => None,
        }
    }

    /// True when the type is `()`, which `do` blocks discard silently.
    #[must_use]
    pub const fn is_unit(&self) -> bool {
        matches!(self, Self::Unit)
    }

    /// True when the type already failed to check.
    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self, Self::Error)
    }

    /// Substitutes ADT/alias parameters (type-kinded positions only).
    #[must_use]
    pub fn substitute_params(
        &self,
        args: &[Self],
    ) -> Self {
        match self {
            | Self::Param(index) => {
                args.get(usize::try_from(*index).unwrap_or(usize::MAX))
                    .cloned()
                    .unwrap_or(Self::Error)
            },
            | Self::Scalar(rep, dim) => Self::Scalar(*rep, dim.clone()),
            | Self::Tensor { shape, elem } => {
                Self::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(elem.substitute_params(args)),
                }
            },
            | Self::TypeRepIntrinsic(ty) => {
                Self::TypeRepIntrinsic(ty.as_ref().map(|ty| Box::new(ty.substitute_params(args))))
            },
            | Self::FromValueIntrinsic(ty) => {
                Self::FromValueIntrinsic(ty.as_ref().map(|ty| Box::new(ty.substitute_params(args))))
            },
            | Self::Fn(params, ret) => {
                Self::Fn(
                    params.iter().map(|p| p.substitute_params(args)).collect(),
                    Box::new(ret.substitute_params(args)),
                )
            },
            | Self::Record(row) => Self::Record(row.substitute_params(args)),
            | Self::Adt(id, values) => {
                Self::Adt(
                    *id,
                    values.iter().map(|v| v.substitute_params(args)).collect(),
                )
            },
            | Self::Io(inner) => Self::Io(Box::new(inner.substitute_params(args))),
            | other => other.clone(),
        }
    }

    /// Substitutes the parameters of a generic *declaration* (§2: type
    /// parameters may be type-, dimension-, natural- or row-kinded).
    ///
    /// `types` maps `Param(i)` by position (only type-kinded positions are
    /// used), `dims` maps the dimension variables of dimension-kinded
    /// parameters, `nats` the shape-extent variables of natural-kinded ones and
    /// `rows` the row variables of row-kinded ones.
    #[must_use]
    pub fn substitute_kinded(
        &self,
        types: &[Option<Self>],
        dims: &std::collections::HashMap<u32, Dim>,
        nats: &std::collections::HashMap<u32, Nat>,
        rows: &std::collections::HashMap<u32, Row>,
    ) -> Self {
        match self {
            | Self::Param(index) => {
                types
                    .get(usize::try_from(*index).unwrap_or(usize::MAX))
                    .cloned()
                    .flatten()
                    .unwrap_or(Self::Error)
            },
            | Self::Scalar(rep, dim) => {
                Self::Scalar(*rep, dim.substitute(|id| dims.get(&id).cloned()))
            },
            | Self::Tensor { shape, elem } => {
                Self::Tensor {
                    shape: shape.substitute_nats(nats),
                    elem: Box::new(elem.substitute_kinded(types, dims, nats, rows)),
                }
            },
            | Self::Fn(params, ret) => {
                Self::Fn(
                    params
                        .iter()
                        .map(|param| param.substitute_kinded(types, dims, nats, rows))
                        .collect(),
                    Box::new(ret.substitute_kinded(types, dims, nats, rows)),
                )
            },
            | Self::Record(row) => Self::Record(row.substitute_kinded(types, dims, nats, rows)),
            | Self::Adt(id, values) => {
                Self::Adt(
                    *id,
                    values
                        .iter()
                        .map(|value| value.substitute_kinded(types, dims, nats, rows))
                        .collect(),
                )
            },
            | Self::Io(inner) => {
                Self::Io(Box::new(inner.substitute_kinded(types, dims, nats, rows)))
            },
            | Self::Point(dim) => Self::Point(dim.substitute(|id| dims.get(&id).cloned())),
            | Self::UnitOf(dim) => Self::UnitOf(dim.substitute(|id| dims.get(&id).cloned())),
            | other => other.clone(),
        }
    }
}

impl Row {
    /// Substitutes ADT/alias parameters inside the row.
    #[must_use]
    pub fn substitute_params(
        &self,
        args: &[Type],
    ) -> Self {
        self.map_types(|ty| ty.substitute_params(args), Self::Var)
    }

    /// Substitutes all four kinds of declaration parameters (§2).
    #[must_use]
    pub fn substitute_kinded(
        &self,
        types: &[Option<Type>],
        dims: &std::collections::HashMap<u32, Dim>,
        nats: &std::collections::HashMap<u32, Nat>,
        rows: &std::collections::HashMap<u32, Self>,
    ) -> Self {
        self.map_types(
            |ty| ty.substitute_kinded(types, dims, nats, rows),
            |id| rows.get(&id).cloned().unwrap_or(Self::Var(id)),
        )
    }

    fn map_types(
        &self,
        mut map_field: impl FnMut(&Type) -> Type,
        map_tail: impl FnOnce(u32) -> Self,
    ) -> Self {
        let fields: Vec<_> = self
            .iter_fields()
            .map(|(label, ty)| (label, map_field(ty)))
            .collect();
        let tail = self.tail_var().map_or(Self::Empty, map_tail);
        fields.into_iter().rev().fold(tail, |tail, (label, ty)| {
            Self::Extend {
                label: label.to_string(),
                ty: Box::new(ty),
                tail: Box::new(tail),
            }
        })
    }
}

impl fmt::Display for Type {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            | Self::Scalar(Rep::Int, dim) if dim.is_one() => f.write_str("Int"),
            | Self::Scalar(Rep::Real, dim) => {
                f.write_str("Real")?;
                write_dim(f, dim)
            },
            | Self::Scalar(Rep::Float, dim) => {
                f.write_str("Float")?;
                write_dim(f, dim)
            },
            | Self::Scalar(Rep::Complex, dim) => {
                f.write_str("Complex")?;
                write_dim(f, dim)
            },
            | Self::Scalar(rep, dim) => write!(f, "Scalar<{rep}, {dim}>"),
            | Self::Bool => f.write_str("Bool"),
            | Self::Prop => f.write_str("Prop"),
            | Self::Str => f.write_str("String"),
            | Self::Unit => f.write_str("()"),
            | Self::Tensor { shape, elem } => write!(f, "Tensor<{shape}, {elem}>"),
            | Self::Point(dim) => write!(f, "Point[{dim}]"),
            | Self::UnitOf(dim) => write!(f, "Unit[{dim}]"),
            | Self::TensorIntrinsic(intrinsic) => {
                if intrinsic.kind == TensorIntrinsic::Fill {
                    f.write_str("variadic fill constructor")
                } else if let Some(rep) = intrinsic.element {
                    write!(f, "variadic tensor constructor<{rep}>")
                } else {
                    f.write_str("variadic tensor constructor")
                }
            },
            | Self::TypeRepIntrinsic(Some(ty)) => write!(f, "type_rep<{ty}>"),
            | Self::TypeRepIntrinsic(None) => f.write_str("type_rep constructor"),
            | Self::FromValueIntrinsic(Some(ty)) => write!(f, "from_value<{ty}>"),
            | Self::FromValueIntrinsic(None) => f.write_str("from_value converter"),
            | Self::ReadNpyIntrinsic(Some(rank)) => write!(f, "read_npy<{rank}>"),
            | Self::ReadNpyIntrinsic(None) => f.write_str("read_npy reader"),
            | Self::WriteNpyIntrinsic => f.write_str("write_npy writer"),
            | Self::ToCallbackIntrinsic => f.write_str("to_callback converter"),
            | Self::Fn(params, ret) => {
                if let [param] = params.as_slice() {
                    write!(f, "{param} -> {ret}")
                } else {
                    f.write_str("(")?;
                    for (index, param) in params.iter().enumerate() {
                        if index > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{param}")?;
                    }
                    write!(f, ") -> {ret}")
                }
            },
            | Self::Record(row) => write!(f, "{{ {row} }}"),
            | Self::Adt(id, args) => {
                let name = crate::adt::name_of(*id);
                if name == "Sparse"
                    && let [format, Self::Tensor { shape, elem }] = args.as_slice()
                {
                    return write!(f, "Sparse<{format}, {shape}, {elem}>");
                }
                if args.is_empty() {
                    f.write_str(&name)
                } else {
                    write!(f, "{name}<")?;
                    for (index, arg) in args.iter().enumerate() {
                        if index > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{arg}")?;
                    }
                    f.write_str(">")
                }
            },
            | Self::Io(inner) => write!(f, "IO {inner}"),
            | Self::Dyn(rep) => write!(f, "Dyn<{rep}>"),
            | Self::Var(id) => write!(f, "t{id}"),
            | Self::Param(id) => write!(f, "p{id}"),
            | Self::Error => f.write_str("<error>"),
        }
    }
}

fn write_dim(
    f: &mut fmt::Formatter<'_>,
    dim: &Dim,
) -> fmt::Result {
    if dim.is_one() {
        return Ok(());
    }
    write!(f, "[{}]", render_dim(dim))
}

/// Renders a dimension with the single-letter SI aliases used by the surface
/// syntax (`L/T^2`), keeping user base names as declared.
///
/// Dimension variables print as `d<id>`, so a polymorphic scheme shows its
/// parameter instead of the dimensionless `1`.
#[cfg_attr(
    not(test),
    expect(clippy::single_call_fn, reason = "render surface dimension notation")
)]
fn render_dim(dim: &Dim) -> String {
    let mut numerator = Vec::new();
    let mut denominator = Vec::new();
    for (atom, exponent) in dim.factors() {
        let symbol = match atom {
            | aevia_dim::Atom::Base(name) => {
                match &**name {
                    | "Length" => "L",
                    | "Mass" => "M",
                    | "Time" => "T",
                    | "Current" => "I",
                    | "Temperature" => "Θ",
                    | "Amount" => "Amt",
                    | "Luminosity" => "Lum",
                    | other => other,
                }
            },
            | aevia_dim::Atom::Nominal(base) => base.display_name(),
            | aevia_dim::Atom::Var(id) => {
                let symbol = format!("d{id}");
                let magnitude = exponent.abs();
                let text = if magnitude.is_one() {
                    symbol
                } else if magnitude.denom().is_one() {
                    format!("{symbol}^{}", magnitude.numer())
                } else {
                    // A rational exponent must keep its denominator: a
                    // polymorphic `Complex[d^(1/2)]` used to render as `d0^1`.
                    format!("{symbol}^({})", aevia_dim::format_rational(&magnitude))
                };
                if exponent.is_negative() {
                    denominator.push(text);
                } else {
                    numerator.push(text);
                }
                continue;
            },
        };
        let target = if exponent.is_negative() {
            &mut denominator
        } else {
            &mut numerator
        };
        let magnitude = exponent.abs();
        if magnitude.is_one() {
            target.push(symbol.to_string());
        } else if magnitude.denom().is_one() {
            target.push(format!("{symbol}^{}", magnitude.numer()));
        } else {
            target.push(format!(
                "{symbol}^({})",
                aevia_dim::format_rational(&magnitude)
            ));
        }
    }
    let numerator = if numerator.is_empty() {
        "1".to_string()
    } else {
        numerator.join("*")
    };
    if denominator.is_empty() {
        numerator
    } else {
        format!("{numerator}/{}", denominator.join("*"))
    }
}

/// A class constraint on a scheme, `C τ1 ... τn` (§3.2.5, §3.8).
///
/// Constraints live only in the type language: an instance set is resolved
/// statically, so a scheme never carries a runtime dictionary.
#[derive(
    Clone,
    PartialEq,
    Eq,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
    bincode_next::Fingerprint,
)]
pub struct Constraint {
    /// The class name.
    pub class: String,
    /// The constrained types, in declaration order.
    pub args: Vec<Type>,
}

impl Constraint {
    /// A one-parameter constraint such as `Show a`.
    #[must_use]
    pub fn new(
        class: impl Into<String>,
        args: Vec<Type>,
    ) -> Self {
        Self {
            class: class.into(),
            args,
        }
    }
}

impl fmt::Display for Constraint {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        f.write_str(&self.class)?;
        for arg in &self.args {
            write!(f, " {arg}")?;
        }
        Ok(())
    }
}

/// A polymorphic type scheme.
#[derive(
    Clone,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
    bincode_next::Fingerprint,
)]
pub struct Scheme {
    /// Quantified type variables.
    pub vars: Vec<u32>,
    /// Quantified dimension variables.
    pub dims: Vec<u32>,
    /// Quantified row variables.
    pub rows: Vec<u32>,
    /// Quantified rows and the field names that must be absent from each one.
    #[serde(default)]
    pub row_lacks: Vec<(u32, Vec<String>)>,
    /// Quantified type-level natural-number variables (§3.5.1).
    ///
    /// A shape extent `[n]` on a declaration takes a fresh Nat variable; a
    /// function that is polymorphic in it quantifies it here, and instantiation
    /// maps it to a fresh variable at the call site.
    pub nats: Vec<u32>,
    /// Class constraints, quantified with the variables they mention.
    pub constraints: Vec<Constraint>,
    /// Numeric literal domains and representation transfers.
    pub numeric: Vec<crate::representation::NumericConstraint>,
    /// Types materialized while applying the function, not in a deferred value.
    pub materializations: Vec<Type>,
    /// The body.
    pub ty: Type,
}

impl Scheme {
    /// A monomorphic scheme.
    #[must_use]
    pub const fn mono(ty: Type) -> Self {
        Self {
            vars: Vec::new(),
            dims: Vec::new(),
            rows: Vec::new(),
            row_lacks: Vec::new(),
            nats: Vec::new(),
            constraints: Vec::new(),
            numeric: Vec::new(),
            materializations: Vec::new(),
            ty,
        }
    }

    /// A scheme quantified over type variables.
    #[must_use]
    pub const fn poly(
        vars: Vec<u32>,
        ty: Type,
    ) -> Self {
        Self {
            vars,
            dims: Vec::new(),
            rows: Vec::new(),
            row_lacks: Vec::new(),
            nats: Vec::new(),
            constraints: Vec::new(),
            numeric: Vec::new(),
            materializations: Vec::new(),
            ty,
        }
    }

    /// Attaches class constraints.
    #[must_use]
    pub fn constrained(
        mut self,
        constraints: Vec<Constraint>,
    ) -> Self {
        self.constraints = constraints;
        self
    }

    /// Whether the scheme is closed (no quantified variables and no
    /// constraints), so instantiating it is a no-op.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.vars.is_empty()
            && self.dims.is_empty()
            && self.rows.is_empty()
            && self.row_lacks.is_empty()
            && self.nats.is_empty()
            && self.constraints.is_empty()
            && self.numeric.is_empty()
    }
}

impl fmt::Display for Scheme {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        if !self.constraints.is_empty() {
            let rendered: Vec<String> = self.constraints.iter().map(ToString::to_string).collect();
            write!(f, "{} => ", rendered.join(", "))?;
        }
        write!(f, "{}", self.ty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_range_nat_arithmetic_does_not_become_a_machine_limit_literal() {
        let map = std::collections::HashMap::from([(0, Nat::lit(usize::MAX))]);
        for expression in [Nat::var(0).add(Nat::lit(1)), Nat::var(0).mul(Nat::lit(2))] {
            let substituted = expression.substitute(&map);
            assert_eq!(substituted.as_const(), None);
            assert_eq!(crate::Infer::new().resolve_nat(&substituted), substituted);
        }
        assert_eq!(Nat::lit(6).fold_mul(Nat::lit(7)), Nat::lit(42));
    }

    #[test]
    fn surface_spellings_round_trip() {
        let length = Dim::base("Length");
        assert_eq!(Type::int().to_string(), "Int");
        assert_eq!(Type::real(length.clone()).to_string(), "Real[L]");
        assert_eq!(Type::float(Dim::one()).to_string(), "Float");
        assert_eq!(
            Type::tensor_rank(2, Type::float(Dim::one())).to_string(),
            "Tensor<2, Float>"
        );
    }

    #[test]
    fn dimension_variable_exponents_keep_their_denominator() {
        // §8.15: `complex_sqrt : Complex[d] -> Complex[d^(1/2)]`; the display
        // used to drop the `/2` and read as `d0^1`.
        let half = num_rational::BigRational::new(1.into(), 2.into());
        let ty = Type::complex(Dim::var(0).pow(&half));
        assert_eq!(ty.to_string(), "Complex[d0^(1/2)]");
    }

    #[test]
    fn rows_expose_fields_and_tail() {
        let row = Row::Extend {
            label: "x".to_string(),
            ty: Box::new(Type::int()),
            tail: Box::new(Row::Var(3)),
        };
        assert_eq!(row.fields().len(), 1);
        assert_eq!(row.tail_var(), Some(3));
        assert!(row.field("x").is_some());
        assert!(row.field("y").is_none());
    }

    #[test]
    fn record_updates_replace_in_place_and_keep_new_open_fields() {
        let closed = Row::closed([
            ("left".to_string(), Type::int()),
            ("right".to_string(), Type::int()),
        ]);
        let updated = closed.update("left", Type::Str);
        assert_eq!(updated.field("left"), Some(&Type::Str));
        assert_eq!(updated.field("right"), Some(&Type::int()));
        assert_eq!(updated.iter_fields().count(), 2);
        assert_eq!(updated.duplicate_label(), None);

        let open = Row::Var(4).update("added", Type::Bool);
        assert_eq!(open.field("added"), Some(&Type::Bool));
        assert_eq!(open.tail_var(), Some(4));
    }

    #[test]
    fn division_promotes_integers() {
        assert_eq!(Rep::Int.div_rep(), Rep::Real);
        assert_eq!(Rep::Float.div_rep(), Rep::Float);
    }
}
