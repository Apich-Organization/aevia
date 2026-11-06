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

use aevia_dim::Dim;
use num_traits::One;
use num_traits::Signed;
use std::collections::BTreeMap;
use std::fmt;

/// Identifier of a nominal algebraic data type.
pub type AdtId = u32;

/// The representation of a scalar.
///
/// `Rep::Var` is a representation variable: `sqrt` and `+` are polymorphic in
/// it, and it is resolved by unification or by the defaulting rules of §3.10.2.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
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
    pub fn default_for_literal(kind: LiteralKind) -> Self {
        match kind {
            | LiteralKind::Int => Self::Int,
            | LiteralKind::Decimal => Self::Float,
            | LiteralKind::QuantityInt => Self::Real,
            | LiteralKind::QuantityDecimal => Self::Float,
        }
    }

    /// `div_rep` of §3.4.2: division promotes integers to exact rationals.
    #[must_use]
    pub fn div_rep(self) -> Self {
        match self {
            | Self::Int => Self::Real,
            | other => other,
        }
    }

    /// The least upper bound of two *concrete* representations, per §3.4.1.
    ///
    /// `None` when either side is a variable, or when the two are not
    /// comparable — the caller records a bound instead of guessing.
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
            | (Int, Real) | (Real, Int) => Some(Real),
            | (Int, Float) | (Float, Int) => Some(Float),
            | (Int, Complex) | (Complex, Int) => Some(Complex),
            // §3.4.1: `Real` wins over `Float` — exactness is preserved.
            | (Real, Float) | (Float, Real) => Some(Real),
            | (Real, Complex) | (Complex, Real) => Some(Complex),
            | (Float, Complex) | (Complex, Float) => Some(Complex),
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
    pub fn upper_candidates(lower: Self) -> &'static [Rep] {
        use Rep::Complex;
        use Rep::Float;
        use Rep::Int;
        use Rep::Real;
        match lower {
            | Int => &[Int, Float, Real, Complex],
            | Float => &[Float, Complex],
            | Real => &[Real, Complex],
            | Complex => &[Complex],
            | Rep::Var(_) => &[Int, Float, Real, Complex],
        }
    }

    /// The surface name.
    #[must_use]
    pub fn name(self) -> &'static str {
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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    pub fn is_quantity(self) -> bool {
        matches!(self, Self::QuantityInt | Self::QuantityDecimal)
    }

    /// Whether the literal may unify with a given representation.
    #[must_use]
    pub fn accepts_rep(
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
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Nat {
    /// A concrete extent.
    Lit(usize),
    /// A type-level variable.
    Var(u32),
    /// `a + b`.
    Add(Box<Nat>, Box<Nat>),
    /// `a * b`. Only a constant factor makes the equation linear; a product of
    /// two variables is rejected (`E0304`).
    Mul(Box<Nat>, Box<Nat>),
}

impl Nat {
    /// A literal.
    #[must_use]
    pub fn lit(value: usize) -> Self {
        Self::Lit(value)
    }

    /// A variable.
    #[must_use]
    pub fn var(id: u32) -> Self {
        Self::Var(id)
    }

    /// `self + other`.
    #[must_use]
    pub fn add(
        self,
        other: Self,
    ) -> Self {
        Self::Add(Box::new(self), Box::new(other))
    }

    /// `self * other`.
    #[must_use]
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

    /// Substitutes variables through `map`, folding any constant arithmetic.
    #[must_use]
    pub fn substitute(
        &self,
        map: &std::collections::HashMap<u32, Nat>,
    ) -> Nat {
        match self {
            | Self::Lit(value) => Self::Lit(*value),
            | Self::Var(id) => map.get(id).cloned().unwrap_or_else(|| self.clone()),
            | Self::Add(left, right) => {
                let left = left.substitute(map);
                let right = right.substitute(map);
                match (left.as_const(), right.as_const()) {
                    | (Some(left), Some(right)) => Nat::Lit(left.saturating_add(right)),
                    | _ => Nat::Add(Box::new(left), Box::new(right)),
                }
            },
            | Self::Mul(left, right) => {
                let left = left.substitute(map);
                let right = right.substitute(map);
                match (left.as_const(), right.as_const()) {
                    | (Some(left), Some(right)) => Nat::Lit(left.saturating_mul(right)),
                    | _ => Nat::Mul(Box::new(left), Box::new(right)),
                }
            },
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
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
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
    pub fn rank_only(rank: u32) -> Self {
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
    ) -> Shape {
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
    pub fn unknown_of_rank(rank: u32) -> Self {
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
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
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
        tail: Box<Row>,
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
        let mut row = Row::Empty;
        for (label, ty) in map.into_iter().rev() {
            row = Row::Extend {
                label,
                ty: Box::new(ty),
                tail: Box::new(row),
            };
        }
        row
    }

    /// The fields of a row, in order, ignoring the tail.
    #[must_use]
    pub fn fields(&self) -> Vec<(String, Type)> {
        let mut out = Vec::new();
        let mut current = self;
        loop {
            match current {
                | Row::Empty | Row::Var(_) => return out,
                | Row::Extend { label, ty, tail } => {
                    out.push((label.clone(), (**ty).clone()));
                    current = tail;
                },
            }
        }
    }

    /// The tail variable, when the row is open.
    #[must_use]
    pub fn tail_var(&self) -> Option<u32> {
        let mut current = self;
        loop {
            match current {
                | Row::Empty => return None,
                | Row::Var(id) => return Some(*id),
                | Row::Extend { tail, .. } => current = tail,
            }
        }
    }

    /// The type of a field, if present.
    #[must_use]
    pub fn field(
        &self,
        name: &str,
    ) -> Option<&Type> {
        let mut current = self;
        loop {
            match current {
                | Row::Empty | Row::Var(_) => return None,
                | Row::Extend { label, ty, tail } => {
                    if label == name {
                        return Some(ty);
                    }
                    current = tail;
                },
            }
        }
    }

    /// The first label that appears twice, if any (§3.6.1: `E0208`).
    #[must_use]
    pub fn duplicate_label(&self) -> Option<String> {
        let mut seen: Vec<&str> = Vec::new();
        let mut current = self;
        loop {
            match current {
                | Row::Empty | Row::Var(_) => return None,
                | Row::Extend { label, tail, .. } => {
                    if seen.contains(&label.as_str()) {
                        return Some(label.clone());
                    }
                    seen.push(label);
                    current = tail;
                },
            }
        }
    }

    /// Extends a row with a field, keeping an open tail (§3.6.2).
    ///
    /// A label already present is *replaced* — that is record update, not a
    /// duplicate.
    #[must_use]
    pub fn update(
        self,
        label: &str,
        ty: Type,
    ) -> Self {
        match self {
            | Row::Extend {
                label: existing,
                ty: existing_ty,
                tail,
            } => {
                let tail = tail.update(label, ty.clone());
                let ty = if existing == label {
                    ty
                } else {
                    *existing_ty
                };
                Row::Extend {
                    label: existing,
                    ty: Box::new(ty),
                    tail: Box::new(tail),
                }
            },
            | Row::Var(id) => Row::Var(id),
            | Row::Empty => {
                Row::Extend {
                    label: label.to_string(),
                    ty: Box::new(ty),
                    tail: Box::new(Row::Empty),
                }
            },
        }
    }
}

impl fmt::Display for Row {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        let fields = self.fields();
        for (index, (label, ty)) in fields.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{label}: {ty}")?;
        }
        match self.tail_var() {
            | Some(id) => {
                if !fields.is_empty() {
                    f.write_str(", ")?;
                }
                write!(f, "...r{id}")
            },
            | None => Ok(()),
        }
    }
}

/// A rank-polymorphic tensor constructor, preserved across value aliases.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
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
    pub fn name(self) -> &'static str {
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
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TensorIntrinsicType {
    /// The constructor's operation.
    pub kind: TensorIntrinsic,
    /// An explicit, dimensionless numeric element representation, if selected.
    pub element: Option<Rep>,
}

/// A type.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
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
        elem: Box<Type>,
    },
    /// `Point[d]`: an affine point such as an absolute temperature.
    Point(Dim),
    /// `Unit[d]`: a unit value.
    UnitOf(Dim),
    /// A variadic tensor constructor; its result rank depends on its call.
    TensorIntrinsic(TensorIntrinsicType),
    /// A type reflection constructor, optionally specialized to a concrete type.
    TypeRepIntrinsic(Option<Box<Type>>),
    /// A checked opaque-value conversion, optionally specialized to its result type.
    FromValueIntrinsic(Option<Box<Type>>),
    /// A checked conversion of a concrete function into an opaque callback.
    ToCallbackIntrinsic,
    /// `(τ1, ..., τn) -> τ`.
    Fn(Vec<Type>, Box<Type>),
    /// A record with a row.
    Record(Row),
    /// A nominal variant type with arguments.
    Adt(AdtId, Vec<Type>),
    /// `IO τ`.
    Io(Box<Type>),
    /// `Dyn<ρ>`.
    Dyn(Rep),
    /// A type variable.
    Var(u32),
    /// An ADT or alias parameter, substituted on use.
    Param(u32),
    /// The type of an expression that already failed to check.
    Error,
}

impl Type {
    /// `Int`, that is `Scalar<Int, 1>`.
    #[must_use]
    pub fn int() -> Self {
        Self::Scalar(Rep::Int, Dim::one())
    }

    /// `Real[d]`.
    #[must_use]
    pub fn real(dim: Dim) -> Self {
        Self::Scalar(Rep::Real, dim)
    }

    /// `Float[d]`.
    #[must_use]
    pub fn float(dim: Dim) -> Self {
        Self::Scalar(Rep::Float, dim)
    }

    /// `Complex[d]`.
    #[must_use]
    pub fn complex(dim: Dim) -> Self {
        Self::Scalar(Rep::Complex, dim)
    }

    /// A scalar with a representation and dimension.
    #[must_use]
    pub fn scalar(
        rep: Rep,
        dim: Dim,
    ) -> Self {
        Self::Scalar(rep, dim)
    }

    /// `Bool`.
    #[must_use]
    pub fn bool() -> Self {
        Self::Bool
    }

    /// `String`.
    #[must_use]
    pub fn string() -> Self {
        Self::Str
    }

    /// `()`.
    #[must_use]
    pub fn unit() -> Self {
        Self::Unit
    }

    /// `Tensor<shape, elem>`.
    #[must_use]
    pub fn tensor(
        shape: Shape,
        elem: Type,
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
        elem: Type,
    ) -> Self {
        Self::tensor(Shape::Rank(rank), elem)
    }

    /// A function type.
    #[must_use]
    pub fn func(
        args: Vec<Type>,
        ret: Type,
    ) -> Self {
        Self::Fn(args, Box::new(ret))
    }

    /// `IO τ`.
    #[must_use]
    pub fn io(inner: Type) -> Self {
        Self::Io(Box::new(inner))
    }

    /// A record from fields.
    #[must_use]
    pub fn record(fields: impl IntoIterator<Item = (String, Type)>) -> Self {
        Self::Record(Row::closed(fields))
    }

    /// `Option<τ>` as an ADT reference.
    #[must_use]
    pub fn option(inner: Type) -> Self {
        Self::Adt(crate::adt::OPTION_ID, vec![inner])
    }

    /// `Result<τ, ε>` as an ADT reference.
    #[must_use]
    pub fn result(
        ok: Type,
        err: Type,
    ) -> Self {
        Self::Adt(crate::adt::RESULT_ID, vec![ok, err])
    }

    /// The scalar representation and dimension, when this is a scalar.
    #[must_use]
    pub fn as_scalar(&self) -> Option<(Rep, &Dim)> {
        match self {
            | Self::Scalar(rep, dim) => Some((*rep, dim)),
            | _ => None,
        }
    }

    /// True when the type is `()`, which `do` blocks discard silently.
    #[must_use]
    pub fn is_unit(&self) -> bool {
        matches!(self, Self::Unit)
    }

    /// True when the type already failed to check.
    #[must_use]
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error)
    }

    /// Substitutes ADT/alias parameters (type-kinded positions only).
    #[must_use]
    pub fn substitute_params(
        &self,
        args: &[Type],
    ) -> Type {
        match self {
            | Self::Param(index) => {
                args.get(usize::try_from(*index).unwrap_or(usize::MAX))
                    .cloned()
                    .unwrap_or(Type::Error)
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
        types: &[Option<Type>],
        dims: &std::collections::HashMap<u32, Dim>,
        nats: &std::collections::HashMap<u32, Nat>,
        rows: &std::collections::HashMap<u32, Row>,
    ) -> Type {
        match self {
            | Self::Param(index) => {
                types
                    .get(usize::try_from(*index).unwrap_or(usize::MAX))
                    .cloned()
                    .flatten()
                    .unwrap_or(Type::Error)
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
    ) -> Row {
        match self {
            | Self::Empty => Self::Empty,
            | Self::Var(id) => Self::Var(*id),
            | Self::Extend { label, ty, tail } => {
                Self::Extend {
                    label: label.clone(),
                    ty: Box::new(ty.substitute_params(args)),
                    tail: Box::new(tail.substitute_params(args)),
                }
            },
        }
    }

    /// Substitutes all four kinds of declaration parameters (§2).
    #[must_use]
    pub fn substitute_kinded(
        &self,
        types: &[Option<Type>],
        dims: &std::collections::HashMap<u32, Dim>,
        nats: &std::collections::HashMap<u32, Nat>,
        rows: &std::collections::HashMap<u32, Row>,
    ) -> Row {
        match self {
            | Self::Empty => Self::Empty,
            | Self::Var(id) => rows.get(id).cloned().unwrap_or_else(|| self.clone()),
            | Self::Extend { label, ty, tail } => {
                Self::Extend {
                    label: label.clone(),
                    ty: Box::new(ty.substitute_kinded(types, dims, nats, rows)),
                    tail: Box::new(tail.substitute_kinded(types, dims, nats, rows)),
                }
            },
        }
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
            | Self::ToCallbackIntrinsic => f.write_str("to_callback converter"),
            | Self::Fn(params, ret) => {
                if params.len() == 1 {
                    write!(f, "{} -> {ret}", params[0])
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
/// statically (PLAN D3), so a scheme never carries a runtime dictionary.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
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
#[derive(Clone, Debug)]
pub struct Scheme {
    /// Quantified type variables.
    pub vars: Vec<u32>,
    /// Quantified dimension variables.
    pub dims: Vec<u32>,
    /// Quantified row variables.
    pub rows: Vec<u32>,
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
    pub fn mono(ty: Type) -> Self {
        Self {
            vars: Vec::new(),
            dims: Vec::new(),
            rows: Vec::new(),
            nats: Vec::new(),
            constraints: Vec::new(),
            numeric: Vec::new(),
            materializations: Vec::new(),
            ty,
        }
    }

    /// A scheme quantified over type variables.
    #[must_use]
    pub fn poly(
        vars: Vec<u32>,
        ty: Type,
    ) -> Self {
        Self {
            vars,
            dims: Vec::new(),
            rows: Vec::new(),
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
    pub fn is_closed(&self) -> bool {
        self.vars.is_empty()
            && self.dims.is_empty()
            && self.rows.is_empty()
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
    fn division_promotes_integers() {
        assert_eq!(Rep::Int.div_rep(), Rep::Real);
        assert_eq!(Rep::Float.div_rep(), Rep::Float);
    }
}
