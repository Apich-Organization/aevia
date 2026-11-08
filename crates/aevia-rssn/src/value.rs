//! The value model shared across the bridge.
//!
//! Aevia's runtime has its own `Value` (dimensions, display units, closures)
//! and rssn has its own (`rssn::kernel::Value`). Neither type is allowed to
//! leak across the boundary, so the bridge speaks this small neutral language
//! instead. Conversion happens exactly twice per call — in and out — which
//! keeps the dependency direction one-way: `aevia-core` knows this module,
//! this module knows `rssn`, and `rssn` knows nothing about either.

use crate::error::Failure;
use crate::error::KernelError;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;
use num_traits::ToPrimitive;
use rssn::graph::Number;
use rssn::kernel::result::Solutions;
use rssn::kernel::value::ComplexValue;
use rssn::kernel::value::OpaqueValue;
use rssn::kernel::value::QuantityValue;
use rssn::kernel::value::RecordValue;
use rssn::kernel::value::SessionId;
use rssn::kernel::value::SparseFormat;
use rssn::kernel::value::SparseValue;
use rssn::kernel::value::TensorValue;
use rssn::kernel::value::TermRef;
use rssn::kernel::value::UnitValue;
use rssn::kernel::value::Value;
use rssn::kernel::value::VariantValue;
use std::fmt;

/// A scalar the kernel understands.
#[derive(Clone, Debug, PartialEq)]
pub enum Num {
    /// An arbitrary-precision integer.
    Int(BigInt),
    /// An exact rational.
    Rat(BigRational),
    /// An IEEE-754 double.
    Float(f64),
}

impl Num {
    /// The value as a double, saturating at infinity.
    ///
    /// Saturation is a *rendering* convenience, not a fact about the number: an
    /// integer too large for `f64` and an infinite value both come out `∞`.
    /// A caller that has to tell them apart uses [`Num::try_to_f64`].
    #[must_use]
    pub fn to_f64(&self) -> f64 {
        match self.try_to_f64() {
            | Some(value) => value,
            | None if self.is_negative() => f64::NEG_INFINITY,
            | None => f64::INFINITY,
        }
    }

    /// The value as a double, or `None` when it does not fit one.
    ///
    /// `10^400` has no `f64`, and answering `∞` for it (as this bridge used to)
    /// makes an exact huge integer indistinguishable from an infinite value at
    /// every call site that reads a number as a double.
    #[must_use]
    pub fn try_to_f64(&self) -> Option<f64> {
        match self {
            | Self::Int(value) => value.to_f64().filter(|value| value.is_finite()),
            | Self::Rat(value) => value.to_f64().filter(|value| value.is_finite()),
            | Self::Float(value) => Some(*value),
        }
    }

    /// Whether the value is below zero.
    #[must_use]
    fn is_negative(&self) -> bool {
        match self {
            | Self::Int(value) => num_traits::Signed::is_negative(value),
            | Self::Rat(value) => num_traits::Signed::is_negative(value),
            | Self::Float(value) => value.is_sign_negative(),
        }
    }

    /// The value as an exact rational, when it is exact.
    #[must_use]
    pub fn to_rational(&self) -> Option<BigRational> {
        match self {
            | Self::Int(value) => Some(BigRational::from_integer(value.clone())),
            | Self::Rat(value) => Some(value.clone()),
            | Self::Float(_) => None,
        }
    }

    /// Builds from an rssn number.
    #[must_use]
    pub fn from_number(number: &Number) -> Self {
        match number {
            | Number::Int(value) => Self::Int(value.clone()),
            | Number::Rat(value) => Self::Rat(value.clone()),
            | Number::Float(value) => Self::Float(*value),
        }
    }

    /// Converts to an rssn number.
    ///
    /// An integral rational is collapsed to `Number::Int`: the kernel's `Number`
    /// invariant is that `Rat` never has denominator one (`Number::rat`
    /// enforces it), and a non-normal form misses the kernel's structural
    /// rewrite patterns — `cos(0/1)` would not fold to `1` while `cos(0)` does.
    /// The kernel also normalises at its ABI boundary; doing it here as well
    /// keeps the bridge from handing out a form the invariant forbids.
    #[must_use]
    pub fn to_number(&self) -> Number {
        match self {
            | Self::Int(value) => Number::Int(value.clone()),
            | Self::Rat(value) if value.is_integer() => Number::Int(value.to_integer()),
            | Self::Rat(value) => Number::Rat(value.clone()),
            | Self::Float(value) => Number::Float(*value),
        }
    }
}

/// An owned handle to a term of the bridge's session.
///
/// The session id makes a stale handle detectable: after a reset the node
/// numbers start again, and a handle that survived the reset must be
/// rejected rather than silently naming a different expression.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sym {
    /// The session that owns the node.
    pub session: u64,
    /// The node id within that session.
    pub node: u32,
}

impl Sym {
    /// The handle for `term`.
    #[must_use]
    pub const fn new(
        session: u64,
        node: u32,
    ) -> Self {
        Self { session, node }
    }
}

/// A unit crossing the bridge.
///
/// The name alone is not enough: a unit *is* a dimension plus a scale to
/// coherent SI, and dropping either turns `Units.m` into a dimensionless
/// symbol. Both directions of the bridge carry the definition, so a unit that
/// enters the kernel is the unit the checker typed.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitVal {
    /// The canonical spelling (`m`).
    pub name: String,
    /// How it is printed (`°C`).
    pub display: String,
    /// The dimension it measures.
    pub dim: aevia_dim::Dim,
    /// Exact scale to the coherent SI unit of `dim`.
    pub scale: aevia_dim::Scale,
    /// Additive offset for affine units.
    pub offset: Option<aevia_dim::Scale>,
    /// Whether the unit is an affine point scale.
    pub affine: bool,
    /// Whether SI prefixes may be attached.
    pub prefixable: bool,
    /// Coarse grouping.
    pub category: String,
}

impl UnitVal {
    /// A linear unit.
    #[must_use]
    pub fn linear(
        name: &str,
        dim: aevia_dim::Dim,
        scale: aevia_dim::Scale,
    ) -> Self {
        Self {
            name: name.to_string(),
            display: name.to_string(),
            dim,
            scale,
            offset: None,
            affine: false,
            prefixable: false,
            category: String::new(),
        }
    }

    /// The definition of a unit the compiler knows.
    #[must_use]
    pub fn from_def(unit: &aevia_dim::UnitDef) -> Self {
        Self {
            name: unit.name.clone(),
            display: unit.display.clone(),
            dim: unit.dim.clone(),
            scale: unit.scale.clone(),
            offset: unit.offset.clone(),
            affine: unit.affine,
            prefixable: unit.prefixable,
            category: unit.category.clone(),
        }
    }

    /// The unit a manifest value carries.
    #[must_use]
    pub fn from_rssn(unit: &UnitValue) -> Self {
        Self {
            name: unit.name.clone(),
            display: unit.display.clone(),
            dim: crate::registry::convert_dim(&unit.dimension),
            scale: crate::registry::convert_scale(&unit.scale),
            offset: unit.offset.as_ref().map(crate::registry::convert_scale),
            affine: unit.affine,
            prefixable: unit.prefixable,
            category: unit.category.clone(),
        }
    }

    /// The manifest form of this unit.
    ///
    /// `aliases` and `since` are manifest bookkeeping, not part of a value: a
    /// unit that travels as a value has already been resolved.
    #[must_use]
    pub fn to_rssn(&self) -> UnitValue {
        UnitValue {
            name: self.name.clone(),
            display: self.display.clone(),
            dimension: crate::registry::to_dim_expr(&self.dim),
            scale: crate::registry::to_scale_value(&self.scale),
            offset: self.offset.as_ref().map(crate::registry::to_scale_value),
            affine: self.affine,
            aliases: Vec::new(),
            prefixable: self.prefixable,
            category: self.category.clone(),
            since: String::new(),
        }
    }

    /// The compiler's unit definition.
    #[must_use]
    pub fn to_def(&self) -> aevia_dim::UnitDef {
        aevia_dim::UnitDef {
            name: self.name.clone(),
            aliases: Vec::new(),
            dim: self.dim.clone(),
            scale: self.scale.clone(),
            offset: self.offset.clone(),
            prefixable: self.prefixable,
            affine: self.affine,
            display: self.display.clone(),
            category: self.category.clone(),
        }
    }
}

/// The storage format of a sparse value crossing the bridge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SparseKind {
    /// Compressed sparse row.
    Csr,
    /// Compressed sparse column.
    Csc,
    /// Coordinate list.
    Coo,
}

/// A value crossing the bridge.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    /// A number.
    Num(Num),
    /// A truth value.
    Bool(bool),
    /// Text.
    Str(String),
    /// A free symbol of the expression graph.
    Symbol(String),
    /// An owned handle to a symbolic term.
    Term(Sym),
    /// A complex number.
    Complex {
        /// Real part.
        re: f64,
        /// Imaginary part.
        im: f64,
    },
    /// A dense tensor in row-major order.
    Tensor {
        /// Extents, outermost first.
        shape: Vec<usize>,
        /// Elements.
        data: Vec<Self>,
    },
    /// A sparse tensor: only the stored entries cross the bridge.
    Sparse {
        /// The storage format.
        format: SparseKind,
        /// Extents, outermost first.
        shape: Vec<usize>,
        /// Flat row-major indices of the stored entries, ascending.
        indices: Vec<u64>,
        /// The stored entries, in the order of `indices`.
        data: Vec<Self>,
    },
    /// A record.
    Record(Vec<(String, Self)>),
    /// A tagged union.
    Variant {
        /// The tag.
        tag: String,
        /// The payload.
        payload: Vec<Self>,
    },
    /// A unit, not a quantity.
    Unit(UnitVal),
    /// A value with a unit.
    Quantity {
        /// The numeric part.
        value: Box<Self>,
        /// The unit it is measured in.
        unit: UnitVal,
    },
    /// Host data the kernel does not interpret.
    Opaque {
        /// What the bytes mean.
        kind: String,
        /// The payload.
        bytes: Vec<u8>,
    },
}

impl Val {
    /// A rank-1 tensor from a list.
    #[must_use]
    pub fn list(data: Vec<Self>) -> Self {
        let len = data.len();
        Self::Tensor {
            shape: vec![len],
            data,
        }
    }

    /// The elements of a rank-1 tensor, when it is one.
    #[must_use]
    pub fn as_list(&self) -> Option<&[Self]> {
        match self {
            | Self::Tensor { shape, data } if shape.len() == 1 => Some(data),
            | _ => None,
        }
    }

    /// Converts to an rssn value.
    ///
    /// # Errors
    ///
    /// Rejects a term handle from another session, and host-only payloads the
    /// kernel has no representation for.
    pub fn to_rssn(
        &self,
        session: SessionId,
    ) -> Result<Value, KernelError> {
        Ok(match self {
            | Self::Num(Num::Int(value)) => Value::Int(value.clone()),
            // `Num::to_number` collapses an integral rational to `Int`; an
            // exact integer is `Value::Int` in the kernel's model, and a
            // non-normal `Rat` would miss the structural rewrites.
            | Self::Num(number @ Num::Rat(_)) => {
                match number.to_number() {
                    | Number::Int(value) => Value::Int(value),
                    | other => Value::Real(other),
                }
            },
            | Self::Num(Num::Float(value)) => Value::Float(*value),
            | Self::Bool(value) => Value::Bool(*value),
            | Self::Str(value) => Value::String(value.clone()),
            | Self::Symbol(name) => Value::Symbol(name.clone()),
            | Self::Term(sym) => {
                if sym.session != session.0 {
                    return Err(KernelError::stale_term(*sym, session.0));
                }
                // The node is not checked here: a `Val` does not know which
                // `Kernel` will receive it, and the thread-local one is the
                // wrong session whenever a caller built its own. The session
                // that owns the handle checks it (`Kernel::require_live`), and
                // the kernel rejects a node it does not have.
                Value::Term(TermRef {
                    session,
                    node: sym.node,
                })
            },
            | Self::Complex { re, im } => Value::Complex(ComplexValue::Float { re: *re, im: *im }),
            | Self::Tensor { shape, data } => {
                let mut converted = Vec::with_capacity(data.len());
                for item in data {
                    converted.push(item.to_rssn(session)?);
                }
                // The kernel carries the element type now, so it has to be
                // supplied here rather than guessed later from an empty
                // `data`. Aevia's own `Tensor` does not record it, so the
                // first element is the best available evidence — and `Real`
                // when there is no element at all.
                let elem = converted
                    .first()
                    .map_or(rssn::kernel::signature::TypeExpr::Real, Value::type_expr);
                Value::Tensor(TensorValue {
                    shape: shape.clone(),
                    elem,
                    data: converted,
                })
            },
            | Self::Sparse {
                format,
                shape,
                indices,
                data,
            } => {
                let mut converted = Vec::with_capacity(data.len());
                for item in data {
                    converted.push(item.to_rssn(session)?);
                }
                Value::Sparse(SparseValue {
                    shape: shape.clone(),
                    format: match format {
                        | SparseKind::Csr => SparseFormat::Csr,
                        | SparseKind::Csc => SparseFormat::Csc,
                        | SparseKind::Coo => SparseFormat::Coo,
                    },
                    indices: indices.clone(),
                    data: converted,
                })
            },
            | Self::Record(fields) => {
                let mut converted = Vec::with_capacity(fields.len());
                for (name, value) in fields {
                    converted.push((name.clone(), value.to_rssn(session)?));
                }
                Value::Record(RecordValue { fields: converted })
            },
            | Self::Variant { tag, payload } => {
                let mut converted = Vec::with_capacity(payload.len());
                for item in payload {
                    converted.push(item.to_rssn(session)?);
                }
                Value::Variant(VariantValue {
                    tag: tag.clone(),
                    payload: converted,
                })
            },
            | Self::Unit(unit) => Value::Unit(unit.to_rssn()),
            | Self::Quantity { value, unit } => {
                Value::Quantity(QuantityValue {
                    value: Box::new(value.to_rssn(session)?),
                    unit: unit.to_rssn(),
                })
            },
            | Self::Opaque { kind, bytes } => {
                Value::Opaque(OpaqueValue {
                    kind: kind.clone(),
                    bytes: bytes.clone(),
                })
            },
        })
    }

    /// Converts from an rssn value.
    ///
    /// # Errors
    ///
    /// Rejects the values that only exist on the kernel's side of the
    /// boundary (callbacks) and values the bridge cannot represent.
    pub fn from_rssn(
        value: &Value,
        session: SessionId,
    ) -> Result<Self, KernelError> {
        Ok(match value {
            | Value::Int(value) => Self::Num(Num::Int(value.clone())),
            | Value::Real(number) => Self::Num(Num::from_number(number)),
            | Value::Float(value) => Self::Num(Num::Float(*value)),
            | Value::Bool(value) => Self::Bool(*value),
            | Value::String(value) => Self::Str(value.clone()),
            | Value::Symbol(name) => Self::Symbol(name.clone()),
            | Value::Term(term) => {
                if term.session != session {
                    return Err(KernelError::stale_term(
                        Sym::new(term.session.0, term.node),
                        session.0,
                    ));
                }
                Self::Term(Sym::new(term.session.0, term.node))
            },
            | Value::Complex(complex) => {
                // Lossy in both directions: `Val::Complex` holds two `f64`s,
                // so an exact complex (`ComplexValue::Exact`) loses its
                // rationals here, and a non-real complex cannot re-enter the
                // kernel at all. See `.workbuddy-ai/audit/bridge-fixed.md`;
                // the fix needs a wider `Val::Complex`, which the runtime's
                // `NumKind::Complex` has to widen with it.
                let (re, im) = complex.to_f64();
                Self::Complex { re, im }
            },
            | Value::Tensor(tensor) => {
                let mut data = Vec::with_capacity(tensor.data.len());
                for item in &tensor.data {
                    data.push(Self::from_rssn(item, session)?);
                }
                Self::Tensor {
                    shape: tensor.shape.clone(),
                    data,
                }
            },
            | Value::Sparse(sparse) => {
                // The stored entries cross as they are: densifying would
                // allocate the zeros the representation exists to avoid.
                let extent = sparse
                    .shape
                    .iter()
                    .try_fold(1u64, |acc, extent| {
                        acc.checked_mul(u64::try_from(*extent).ok()?)
                    })
                    .ok_or_else(|| {
                        KernelError::of(
                            "E0809",
                            crate::error::Failure::Kernel,
                            format!("a sparse tensor of shape {:?} overflows", sparse.shape),
                        )
                    })?;
                if sparse.indices.len() != sparse.data.len() {
                    return Err(KernelError::of(
                        "E0809",
                        crate::error::Failure::Kernel,
                        format!(
                            "a sparse tensor stores {} indices for {} values",
                            sparse.indices.len(),
                            sparse.data.len()
                        ),
                    ));
                }
                if let Some(index) = sparse.indices.iter().find(|index| **index >= extent) {
                    // Dropping the entry would silently produce a matrix that
                    // is not the one the kernel computed.
                    return Err(KernelError::of(
                        "E0809",
                        crate::error::Failure::Kernel,
                        format!(
                            "a sparse entry is stored at {index}, outside a tensor of {extent} elements"
                        ),
                    ));
                }
                let mut data = Vec::with_capacity(sparse.data.len());
                for item in &sparse.data {
                    data.push(Self::from_rssn(item, session)?);
                }
                Self::Sparse {
                    format: match sparse.format {
                        | SparseFormat::Csr => SparseKind::Csr,
                        | SparseFormat::Csc => SparseKind::Csc,
                        | SparseFormat::Coo => SparseKind::Coo,
                    },
                    shape: sparse.shape.clone(),
                    indices: sparse.indices.clone(),
                    data,
                }
            },
            | Value::Record(record) => {
                let mut fields = Vec::with_capacity(record.fields.len());
                for (name, item) in &record.fields {
                    fields.push((name.clone(), Self::from_rssn(item, session)?));
                }
                Self::Record(fields)
            },
            | Value::Variant(variant) => {
                let mut payload = Vec::with_capacity(variant.payload.len());
                for item in &variant.payload {
                    payload.push(Self::from_rssn(item, session)?);
                }
                Self::Variant {
                    tag: variant.tag.clone(),
                    payload,
                }
            },
            | Value::Unit(unit) => Self::Unit(UnitVal::from_rssn(unit)),
            | Value::Quantity(quantity) => {
                Self::Quantity {
                    value: Box::new(Self::from_rssn(&quantity.value, session)?),
                    unit: UnitVal::from_rssn(&quantity.unit),
                }
            },
            // The payload is what Aevia can hold; `Dyn`'s representation
            // (`repr`) and its dimension (`dim`) have no mirror in `Val`, and
            // the dimension is re-attached by the caller from the static type.
            // Recorded as a requirement in `.workbuddy-ai/audit/bridge-fixed.md`.
            | Value::Dyn(dynamic) => Self::from_rssn(&dynamic.value, session)?,
            | Value::Opaque(opaque) => {
                Self::Opaque {
                    kind: opaque.kind.clone(),
                    bytes: opaque.bytes.clone(),
                }
            },
            | Value::Function(_) => {
                return Err(KernelError::new(
                    "E0899",
                    "a host callback cannot cross back into Aevia",
                ));
            },
        })
    }

    /// A solver's [`Solutions`] as the record §8.6 names.
    ///
    /// The kernel already defines the mapping — [`Solutions::to_value`] writes
    /// `variable`, `roots`, `multiplicities`, `conditions` and `complete` in
    /// that order — so the bridge does not re-derive any of them. Flattening
    /// to a bare root list, which is what the bridge used to do, threw away
    /// three of the five fields.
    ///
    /// # Errors
    ///
    /// Rejects a root or condition expression that belongs to another session.
    pub fn from_solutions(
        solutions: &Solutions,
        session: SessionId,
    ) -> Result<Self, KernelError> {
        Self::from_rssn(&solutions.to_value(), session)
    }

    /// The [`Solutions`] this value denotes, when it is that record.
    ///
    /// The inverse of [`Val::from_solutions`]: a `Solutions` that goes to the
    /// kernel and comes back is unchanged, including the difference between
    /// "no multiplicities reported" (empty) and a reported `1`.
    ///
    /// # Errors
    ///
    /// `E0809` when the value is not a record, a field is missing or
    /// mistyped, or the multiplicities do not line up with the roots.
    pub fn to_solutions(
        &self,
        session: SessionId,
    ) -> Result<Solutions, KernelError> {
        let value = self.to_rssn(session)?;
        Solutions::from_value(&value).map_err(|error| {
            KernelError::of(
                "E0809",
                Failure::Kernel,
                format!("this value is not a `Solutions` record: {error}"),
            )
        })
    }

    /// A short rendering, for diagnostics and `show`.
    #[must_use]
    pub fn text(&self) -> String {
        match self {
            | Self::Num(Num::Int(value)) => value.to_string(),
            | Self::Num(Num::Rat(value)) => {
                if value.denom().is_one() {
                    value.numer().to_string()
                } else {
                    value.to_string()
                }
            },
            | Self::Num(Num::Float(value)) => crate::format_float(*value),
            | Self::Bool(value) => value.to_string(),
            | Self::Str(value) => value.clone(),
            | Self::Symbol(name) => name.clone(),
            | Self::Term(sym) => format!("<term {}:{}>", sym.session, sym.node),
            | Self::Complex { re, im } => format!("{re}{im:+}i"),
            | Self::Tensor { shape, data } => {
                let _ = shape;
                let items: Vec<String> = data.iter().map(Self::text).collect();
                format!("[{}]", items.join(", "))
            },
            | Self::Sparse {
                shape, indices, data, ..
            } => {
                let items: Vec<String> = indices
                    .iter()
                    .zip(data)
                    .map(|(index, value)| format!("{index} = {}", value.text()))
                    .collect();
                format!("sparse({shape:?}, {{ {} }})", items.join(", "))
            },
            | Self::Record(fields) => {
                let items: Vec<String> = fields
                    .iter()
                    .map(|(name, value)| format!("{name} = {}", value.text()))
                    .collect();
                format!("{{ {} }}", items.join(", "))
            },
            | Self::Variant { tag, payload } => {
                if payload.is_empty() {
                    tag.clone()
                } else {
                    let items: Vec<String> = payload.iter().map(Self::text).collect();
                    format!("{tag}({})", items.join(", "))
                }
            },
            | Self::Unit(unit) => unit.display.clone(),
            | Self::Quantity { value, unit } => format!("{} {}", value.text(), unit.display),
            | Self::Opaque { kind, .. } => format!("<{kind}>"),
        }
    }
}

impl fmt::Display for Val {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        f.write_str(&self.text())
    }
}
