//! The live kernel session and the calls the runtime makes on it.
//!
//! There is one session per thread, created on first use. rssn's `Session` is
//! a `RefCell` and its `Term` borrows it, so sharing one across threads is not
//! possible; a thread-local keeps the model honest and makes `par_map` do the
//! right thing (each worker gets its own session) without the runtime having
//! to think about it.

use crate::error::KernelError;
use crate::error::from_compute_error;
use crate::error::from_op_error;
use crate::value::Num;
use crate::value::Sym;
use crate::value::Val;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::ToPrimitive;
use rssn::api::Config;
use rssn::api::Session;
use rssn::kernel::call::CallCtx;
use rssn::kernel::call::OpError;
use rssn::kernel::manifest::KernelManifest;
use rssn::kernel::result::Solutions;
use rssn::kernel::signature::OpKey;
use rssn::kernel::signature::Signature;
use rssn::kernel::value::SessionId;
use rssn::kernel::value::TermRef;
use rssn::kernel::value::Value;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// A scoped host function using the neutral bridge value model.
/// Implementations may carry only thread-safe handles; interpreter state stays
/// in its owning thread and may not be borrowed across this boundary.
pub trait BridgeCallback: Send + Sync {
    /// A concrete, immutable signature captured at registration.
    fn signature(&self) -> crate::Sig;
    /// Invoke with explicit runtime dimension witnesses.
    fn call(
        &self,
        args: &[Val],
        witnesses: &[crate::witness::DimensionWitness],
    ) -> Result<(Val, crate::witness::DimensionWitness), KernelError>;
}

struct CallbackAdapter {
    callback: Arc<dyn BridgeCallback>,
    signature: crate::Sig,
    session: SessionId,
}

/// Public callback implementations supply their own schema. Validate it
/// before conversion to RSSN TypeExpr: malformed or unsupported metadata must
/// never be silently weakened into a rank-only or opaque signature.
fn supported_callback_schema(ty: &crate::SigType) -> bool {
    use crate::SigType as S;
    match ty {
        | S::Int | S::Real | S::Float | S::Complex | S::Bool | S::Str => true,
        | S::Dim(inner, dim) => {
            dim.is_concrete() && matches!(inner.as_ref(), S::Int | S::Real | S::Float | S::Complex)
        },
        | S::Tensor {
            rank: Some(rank),
            shape,
            elem,
        } => {
            u64::try_from(*rank).is_ok()
                && shape.as_ref().is_none_or(|shape| {
                    shape.len() == *rank
                        && shape.iter().all(|extent| u64::try_from(*extent).is_ok())
                })
                && supported_callback_schema(elem)
        },
        | S::Record(fields) => {
            fields.iter().enumerate().all(|(index, (name, ty))| {
                !fields[..index].iter().any(|(other, _)| other == name)
                    && supported_callback_schema(ty)
            })
        },
        | _ => false,
    }
}
impl rssn::kernel::plugin::HostCallback for CallbackAdapter {
    fn signature(&self) -> Signature {
        Signature::new(
            self.signature
                .params
                .iter()
                .map(crate::registry::to_type_expr)
                .collect(),
            crate::registry::to_type_expr(&self.signature.ret),
        )
    }

    fn call(
        &self,
        args: &[Value],
    ) -> Result<Value, OpError> {
        let map_error = |error: KernelError| {
            OpError::HostDiagnostic {
                code: error.code,
                message: error.message,
            }
        };
        let values = args
            .iter()
            .map(|v| Val::from_rssn(v, self.session))
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_error)?;
        let witnesses: Vec<_> = args
            .iter()
            .map(|v| crate::witness::extract(v, true))
            .collect();
        let (value, witness) = self.callback.call(&values, &witnesses).map_err(map_error)?;
        validate_callback_result(&value, &witness, &self.signature.ret).map_err(map_error)?;
        let mut value = value.to_rssn(self.session).map_err(map_error)?;
        crate::witness::attach(&mut value, &witness, &self.signature.ret).map_err(map_error)?;
        apply_callback_schema(&mut value, &self.signature.ret);
        Ok(value)
    }
}

// All payloads were independently checked before this point. Restore nested
// tensor metadata from the concrete callback schema after attaching witnesses;
// deriving it from a pre-attachment first element would erase dimensions.
fn apply_callback_schema(
    value: &mut Value,
    sig: &crate::SigType,
) {
    match (value, sig) {
        | (Value::Tensor(tensor), crate::SigType::Tensor { elem, .. }) => {
            for value in &mut tensor.data {
                apply_callback_schema(value, elem);
            }
            tensor.elem = crate::registry::to_type_expr(elem);
        },
        | (Value::Record(record), crate::SigType::Record(fields)) => {
            for (name, value) in &mut record.fields {
                if let Some((_, sig)) = fields.iter().find(|(n, _)| n == name) {
                    apply_callback_schema(value, sig);
                }
            }
        },
        | _ => {},
    }
}

// RSSN's ordinary Float payload is also a valid dimensionless value. The
// bridge may claim that only when the host supplied an independent witness;
// an omitted witness must never silently acquire dimension 1.
// A schema on an empty container is evidence only if each representation,
// rank, named field and numeric dimension is recursively established.
fn callback_schema_proves(
    expected: &crate::SigType,
    actual: &crate::SigType,
    witness: &crate::witness::DimensionWitness,
) -> bool {
    use crate::SigType as S;
    use crate::witness::DimensionWitness as W;
    match (expected, actual, witness) {
        | (S::Dim(inner, dim), repr, W::Scalar(actual_dim)) => {
            dim.is_concrete()
                && dim == actual_dim
                && callback_schema_proves(inner, repr, &W::Scalar(aevia_dim::Dim::one()))
        },
        | (S::Int, S::Int, W::Scalar(dim))
        | (S::Real, S::Int | S::Real, W::Scalar(dim))
        | (S::Float, S::Float, W::Scalar(dim))
        | (S::Complex, S::Complex, W::Scalar(dim)) => dim.is_one(),
        | (S::Bool, S::Bool, _) | (S::Str, S::Str, _) => true,
        | (
            S::Tensor {
                rank: Some(expected_rank),
                shape: expected_shape,
                elem,
            },
            S::Tensor {
                rank: Some(actual_rank),
                shape: actual_shape,
                elem: actual_elem,
            },
            W::Tensor {
                element: Some(element),
                element_repr: Some(repr),
                ..
            },
        ) => {
            expected_rank == actual_rank
                && actual_shape
                    .as_ref()
                    .is_none_or(|shape| shape.len() == *actual_rank)
                && expected_shape
                    .as_ref()
                    .is_none_or(|shape| actual_shape.as_ref() == Some(shape))
                && actual_elem.as_ref() == repr
                && callback_schema_proves(elem, actual_elem, element)
        },
        | (S::Record(expected), S::Record(actual), W::Record(witnesses)) => {
            expected.len() == actual.len()
                && expected.len() == witnesses.len()
                && expected
                    .iter()
                    .map(|(name, _)| name)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    == expected.len()
                && expected.iter().all(|(name, field)| {
                    let mut fields = actual.iter().filter(|(n, _)| n == name);
                    let mut proof = witnesses.iter().filter(|(n, _)| n == name);
                    match (fields.next(), fields.next(), proof.next(), proof.next()) {
                        | (Some((_, repr)), None, Some((_, witness)), None) => {
                            callback_schema_proves(field, repr, witness)
                        },
                        | _ => false,
                    }
                })
        },
        | _ => false,
    }
}

// Input signatures may contain dimension variables. A witness proves the
// actual concrete dimension; RSSN's pinned signature check then enforces all
// declared relationships between inputs and callback signatures.
fn callback_input_dimension(
    sig: &crate::SigType,
    witness: &crate::witness::DimensionWitness,
) -> bool {
    use crate::SigType as S;
    use crate::witness::DimensionWitness as W;
    match (sig, witness) {
        | (S::Dim(_, expected), W::Scalar(actual)) => {
            actual.is_concrete() && (!expected.is_concrete() || expected == actual)
        },
        | (S::Int | S::Real | S::Float | S::Complex, W::Scalar(actual)) => actual.is_one(),
        | _ => false,
    }
}
fn callback_input_schema_proves(
    expected: &crate::SigType,
    actual: &crate::SigType,
    witness: &crate::witness::DimensionWitness,
) -> bool {
    use crate::SigType as S;
    use crate::witness::DimensionWitness as W;
    match (expected, actual) {
        | (S::Dim(inner, _), actual) => {
            callback_input_dimension(expected, witness)
                && callback_input_schema_proves(inner, actual, &W::Scalar(aevia_dim::Dim::one()))
        },
        | (S::Int, S::Int)
        | (S::Real, S::Int | S::Real)
        | (S::Float, S::Float)
        | (S::Complex, S::Complex) => callback_input_dimension(expected, witness),
        | (S::Bool, S::Bool) | (S::Str, S::Str) => true,
        | (
            S::Tensor {
                rank: Some(er),
                shape: es,
                elem: ee,
            },
            S::Tensor {
                rank: Some(ar),
                shape: actual_shape,
                elem: ae,
            },
        ) => {
            let W::Tensor {
                element: Some(element),
                element_repr: Some(repr),
                ..
            } = witness
            else {
                return false;
            };
            er == ar
                && actual_shape.as_ref().is_none_or(|shape| shape.len() == *ar)
                && es
                    .as_ref()
                    .is_none_or(|shape| actual_shape.as_ref() == Some(shape))
                && ae.as_ref() == repr
                && callback_input_schema_proves(ee, ae, element)
        },
        | (S::Record(expected), S::Record(actual)) => {
            let W::Record(witnesses) = witness else {
                return false;
            };
            expected.len() == actual.len()
                && expected.len() == witnesses.len()
                && expected.iter().enumerate().all(|(index, (name, field))| {
                    if expected[..index].iter().any(|(other, _)| other == name) {
                        return false;
                    }
                    let mut fields = actual.iter().filter(|(n, _)| n == name);
                    let mut proofs = witnesses.iter().filter(|(n, _)| n == name);
                    match (fields.next(), fields.next(), proofs.next(), proofs.next()) {
                        | (Some((_, repr)), None, Some((_, proof)), None) => {
                            callback_input_schema_proves(field, repr, proof)
                        },
                        | _ => false,
                    }
                })
        },
        | _ => false,
    }
}
fn validate_callback_input(
    value: &Val,
    witness: &crate::witness::DimensionWitness,
    sig: &crate::SigType,
) -> Result<(), KernelError> {
    use crate::SigType as S;
    use crate::witness::DimensionWitness as W;
    let missing = || {
        KernelError::new(
            "E1001",
            "callback input lacks matching type or dimension evidence",
        )
    };
    let scalar_representation = |value: &Val, sig: &S| {
        let sig = if let S::Dim(inner, _) = sig {
            inner.as_ref()
        } else {
            sig
        };
        matches!(
            (value, sig),
            (Val::Num(Num::Int(_)), S::Int | S::Real)
                | (Val::Num(Num::Rat(_)), S::Real)
                | (Val::Num(Num::Float(_)), S::Float)
                | (Val::Complex { .. }, S::Complex)
        )
    };
    match (value, sig) {
        | (Val::Bool(_), S::Bool) | (Val::Str(_), S::Str) => Ok(()),
        | (
            Val::Num(_) | Val::Complex { .. },
            S::Int | S::Real | S::Float | S::Complex | S::Dim(_, _),
        ) if scalar_representation(value, sig) && callback_input_dimension(sig, witness) => Ok(()),
        | (Val::Quantity { value, unit }, S::Dim(inner, _))
            if matches!(witness, W::Scalar(dim) if dim == &unit.dim)
                && callback_input_dimension(sig, witness) =>
        {
            validate_callback_input(value, &W::Scalar(aevia_dim::Dim::one()), inner)
        },
        | (
            Val::Tensor { shape, data },
            S::Tensor {
                rank: Some(rank),
                shape: expected_shape,
                elem,
            },
        ) => {
            let W::Tensor {
                dimension,
                element,
                element_repr,
                elements,
            } = witness
            else {
                return Err(missing());
            };
            if shape.len() != *rank
                || expected_shape
                    .as_ref()
                    .is_some_and(|expected| expected != shape)
                || shape
                    .iter()
                    .try_fold(1usize, |n, extent| n.checked_mul(*extent))
                    != Some(data.len())
            {
                return Err(missing());
            }
            if data.is_empty() {
                let actual = element_repr.as_ref().ok_or_else(missing)?;
                let fallback = dimension
                    .as_ref()
                    .map_or(W::Unknown, |dim| W::Scalar(dim.clone()));
                let proof = element.as_deref().unwrap_or(&fallback);
                if callback_input_schema_proves(elem, actual, proof) {
                    return Ok(());
                }
                return Err(missing());
            }
            if matches!(
                elem.as_ref(),
                S::Int | S::Real | S::Float | S::Complex | S::Dim(_, _)
            ) {
                let dim = dimension.as_ref().ok_or_else(missing)?;
                let proof = W::Scalar(dim.clone());
                if !callback_input_dimension(elem, &proof) {
                    return Err(missing());
                }
                if !elements.is_empty()
                    && (elements.len() != data.len() || elements.iter().any(|item| item != &proof))
                {
                    return Err(missing());
                }
                for item in data {
                    validate_callback_input(item, &proof, elem)?;
                }
            } else {
                if !elements.is_empty() && elements.len() != data.len() {
                    return Err(missing());
                }
                for (index, item) in data.iter().enumerate() {
                    let proof = elements
                        .get(index)
                        .or(element.as_deref())
                        .unwrap_or(&W::Unknown);
                    validate_callback_input(item, proof, elem)?;
                }
            }
            Ok(())
        },
        | (Val::Record(fields), S::Record(schema)) => {
            let W::Record(witnesses) = witness else {
                return Err(missing());
            };
            if fields.len() != schema.len() || witnesses.len() != schema.len() {
                return Err(missing());
            }
            for (index, (name, field)) in schema.iter().enumerate() {
                if schema[..index].iter().any(|(other, _)| other == name) {
                    return Err(missing());
                }
                let mut values = fields.iter().filter(|(n, _)| n == name);
                let mut proofs = witnesses.iter().filter(|(n, _)| n == name);
                match (values.next(), values.next(), proofs.next(), proofs.next()) {
                    | (Some((_, value)), None, Some((_, proof)), None) => {
                        validate_callback_input(value, proof, field)?
                    },
                    | _ => return Err(missing()),
                }
            }
            Ok(())
        },
        | _ => Err(missing()),
    }
}

fn validate_callback_result(
    value: &Val,
    witness: &crate::witness::DimensionWitness,
    sig: &crate::SigType,
) -> Result<(), KernelError> {
    use crate::Num;
    use crate::SigType as S;
    use crate::witness::DimensionWitness as W;
    fn scalar(
        value: &Val,
        sig: &S,
    ) -> bool {
        matches!(
            (value, sig),
            (Val::Num(Num::Int(_)), S::Int | S::Real)
                | (Val::Num(Num::Rat(_)), S::Real)
                | (Val::Num(Num::Float(_)), S::Float)
                | (Val::Complex { .. }, S::Complex)
        )
    }
    fn is_numeric(sig: &S) -> bool {
        matches!(sig, S::Int | S::Real | S::Float | S::Complex | S::Dim(_, _))
    }
    let mismatch = || {
        KernelError::new(
            "E1003",
            "callback result does not match its concrete type and dimension witness",
        )
    };
    match (value, sig) {
        | (_, S::Dim(inner, dim))
            if dim.is_concrete()
                && scalar(value, inner)
                && matches!(witness, W::Scalar(actual) if actual == dim) =>
        {
            Ok(())
        },
        | (_, S::Int | S::Real | S::Float | S::Complex)
            if scalar(value, sig) && matches!(witness, W::Scalar(actual) if actual.is_one()) =>
        {
            Ok(())
        },
        | (Val::Bool(_), S::Bool) | (Val::Str(_), S::Str) => Ok(()),
        | (
            Val::Tensor { shape, data },
            S::Tensor {
                rank: Some(rank),
                shape: expected_shape,
                elem,
            },
        ) => {
            if shape.len() != *rank
                || expected_shape
                    .as_ref()
                    .is_some_and(|expected| expected != shape)
                || shape
                    .iter()
                    .try_fold(1usize, |n, extent| n.checked_mul(*extent))
                    != Some(data.len())
            {
                return Err(mismatch());
            }
            let W::Tensor {
                dimension,
                element,
                element_repr,
                elements,
            } = witness
            else {
                return Err(mismatch());
            };
            if data.is_empty() {
                let actual = element_repr.as_ref().ok_or_else(mismatch)?;
                let fallback = dimension
                    .as_ref()
                    .map_or(W::Unknown, |dim| W::Scalar(dim.clone()));
                let element = element.as_deref().unwrap_or(&fallback);
                if !callback_schema_proves(elem, actual, element) {
                    return Err(mismatch());
                }
                return Ok(());
            }
            if is_numeric(elem) {
                let dim = match elem.as_ref() {
                    | S::Dim(_, dim) if dim.is_concrete() => dim.clone(),
                    | S::Dim(_, _) => return Err(mismatch()),
                    | _ => aevia_dim::Dim::one(),
                };
                if dimension.as_ref() != Some(&dim)
                    || elements
                        .iter()
                        .any(|w| !matches!(w, W::Scalar(actual) if *actual == dim))
                {
                    return Err(mismatch());
                }
                let scalar = W::Scalar(dim);
                for item in data {
                    validate_callback_result(item, &scalar, elem)?;
                }
            } else {
                if !elements.is_empty() && elements.len() != data.len() {
                    return Err(mismatch());
                }
                for (i, item) in data.iter().enumerate() {
                    let w = elements
                        .get(i)
                        .or(element.as_deref())
                        .unwrap_or(&W::Unknown);
                    validate_callback_result(item, w, elem)?;
                }
            }
            Ok(())
        },
        | (Val::Record(fields), S::Record(schema)) => {
            let W::Record(witnesses) = witness else {
                return Err(mismatch());
            };
            if fields.len() != schema.len()
                || witnesses.len() != schema.len()
                || schema
                    .iter()
                    .map(|(name, _)| name)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != schema.len()
            {
                return Err(mismatch());
            }
            for (name, field) in schema {
                if fields.iter().filter(|(n, _)| n == name).count() != 1
                    || witnesses.iter().filter(|(n, _)| n == name).count() != 1
                {
                    return Err(mismatch());
                }
                let (_, value) = fields
                    .iter()
                    .find(|(n, _)| n == name)
                    .ok_or_else(mismatch)?;
                let (_, witness) = witnesses
                    .iter()
                    .find(|(n, _)| n == name)
                    .ok_or_else(mismatch)?;
                validate_callback_result(value, witness, field)?;
            }
            Ok(())
        },
        | (
            _,
            S::Tensor { .. }
            | S::Record(_)
            | S::Dim(_, _)
            | S::Int
            | S::Real
            | S::Float
            | S::Complex
            | S::Bool
            | S::Str,
        ) => Err(mismatch()),
        | _ => Err(KernelError::capability("core.callback.result_schema")),
    }
}

thread_local! {
    /// The session of the current thread.
    static KERNEL: RefCell<Option<Rc<Kernel>>> = const { RefCell::new(None) };
}

/// The kernel session of the current thread, created on first use.
///
/// The handle is reference counted so that a caller can hold it across a
/// callback without keeping the thread-local borrow alive.
#[must_use]
pub fn kernel() -> Rc<Kernel> {
    KERNEL.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Rc::new(Kernel::new()));
        }
        slot.as_ref()
            .map(Rc::clone)
            .unwrap_or_else(|| Rc::new(Kernel::new()))
    })
}

/// Discards the current thread's session.
///
/// Every handle taken from the old session becomes invalid, which is exactly
/// what `:reset` in the REPL wants.
pub fn reset() {
    KERNEL.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

/// An expression to materialise in the kernel.
///
/// The runtime converts its own symbolic tree into this and asks for one
/// build, instead of calling `add`/`mul` itself at every node: the traversal
/// stays in Aevia and the kernel sees one visit per node.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// A literal number.
    Num(Num),
    /// A free symbol.
    Sym(String),
    /// An operator applied to arguments.
    App(String, Vec<Self>),
    /// A term that already lives in the session.
    Term(Sym),
}

/// How to run a request.
#[derive(Clone, Debug, Default)]
pub struct Opts {
    /// Ask for a numeric answer within this tolerance.
    pub numeric: Option<f64>,
    /// Values for free symbols.
    pub bindings: Vec<(String, f64)>,
}

impl Opts {
    /// A symbolic request.
    #[must_use]
    pub fn symbolic() -> Self {
        Self::default()
    }

    /// A numeric request within `tolerance`.
    #[must_use]
    pub fn numeric(tolerance: f64) -> Self {
        Self {
            numeric: Some(tolerance),
            bindings: Vec::new(),
        }
    }

    /// Binds a symbol.
    #[must_use]
    pub fn bind(
        mut self,
        name: &str,
        value: f64,
    ) -> Self {
        self.bindings.push((name.to_string(), value));
        self
    }

    fn config(&self) -> Config {
        let mut config = Config::new();
        if let Some(tolerance) = self.numeric {
            config = config.numeric(tolerance);
        }
        for (name, value) in &self.bindings {
            config = config.bind(name, *value);
        }
        config
    }
}

/// The result of a whole-request call.
#[derive(Clone, Debug)]
pub struct Computed {
    /// The answer.
    pub val: Val,
    /// The numeric value, when the kernel produced one.
    pub numeric: Option<f64>,
    /// Whether the request was fully reduced.
    pub reduced: bool,
}

/// The dimension signature of an operator defined from Aevia source
/// (§13.5.2): its parameters' and result's dimensions as signature
/// dimensions, with the source function's dimension variables as
/// `DimExpr::Var`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceOpSignature {
    /// The operator name (`demo/area`).
    pub name: String,
    /// One dimension per parameter.
    pub params: Vec<rssn::kernel::signature::DimExpr>,
    /// The result's dimension.
    pub result: rssn::kernel::signature::DimExpr,
}

/// A kernel session.
pub struct Kernel {
    session: Session,
    id: SessionId,
}

impl Kernel {
    /// Creates a session with every standard rule set.
    #[must_use]
    pub fn new() -> Self {
        let session = Session::new();
        let id = session.id();
        Self { session, id }
    }

    /// Registers a plugin compiled from Aevia source (§13.5.2): kernel
    /// definitions (`name(a, b) := body`) for `@op` functions and declarative
    /// rewrites (`name: lhs => rhs`) for `@rule` functions.
    ///
    /// The operators then exist in this thread's session like any built-in
    /// one: the kernel can call, differentiate, simplify and evaluate them.
    ///
    /// # Errors
    ///
    /// The kernel's refusal: a duplicate plugin name, or a definition or
    /// rewrite that does not parse.
    pub fn register_source_plugin(
        &self,
        name: &str,
        version: &str,
        definitions: Vec<String>,
        rewrites: Vec<String>,
    ) -> Result<u32, String> {
        self.register_source_plugin_with(name, version, definitions, rewrites, Vec::new())
    }

    /// [`Kernel::register_source_plugin`] with the dimension signatures of
    /// the defined operators.
    ///
    /// Each [`SourceOpSignature`] becomes the operator's dimension rule
    /// (`rssn::graph::signature_rule`): the kernel's dimension inference then
    /// knows that `demo/area(r)` has dimension `dim(r)^2`, and a dynamic call
    /// returns a dimension witness instead of `Unknown`.
    ///
    /// # Errors
    ///
    /// As [`Kernel::register_source_plugin`], plus a signature that names an
    /// operator the definitions did not create or that the kernel cannot
    /// state (a symbolic exponent).
    pub fn register_source_plugin_with(
        &self,
        name: &str,
        version: &str,
        definitions: Vec<String>,
        rewrites: Vec<String>,
        signatures: Vec<SourceOpSignature>,
    ) -> Result<u32, String> {
        use rssn::kernel::plugin::PluginError;
        let plugin = rssn::kernel::plugin::SimplePlugin::new(name, version, move |registry| {
            let texts: Vec<&str> = definitions.iter().map(String::as_str).collect();
            if !texts.is_empty() {
                registry
                    .register_definitions(&texts)
                    .map_err(|error| PluginError::Registration(error.to_string()))?;
            }
            for signature in &signatures {
                let op = registry
                    .graph()
                    .ops()
                    .lookup(&signature.name)
                    .ok_or_else(|| {
                        PluginError::Registration(format!(
                            "no operator `{}` to attach a dimension signature to",
                            signature.name
                        ))
                    })?;
                registry
                    .register_dim_signature(op, &signature.params, &signature.result)
                    .map_err(|error| PluginError::Registration(error.to_string()))?;
            }
            let rules: Vec<&str> = rewrites.iter().map(String::as_str).collect();
            if !rules.is_empty() {
                registry
                    .register_rewrites(rssn::graph::Tier::Normalize, &rules)
                    .map_err(|error| PluginError::Registration(error.to_string()))?;
            }
            Ok(())
        })
        .with_description("compiled from Aevia source (@op/@rule)");
        self.session
            .register_shared(Box::new(plugin))
            .map(|id| id.0)
            .map_err(|error| error.to_string())
    }

    /// Builds the operator application `name(args…)` as a term without
    /// reducing it: the node a source operator call denotes on symbolic
    /// arguments (§13.5.2). Unlike [`Kernel::call`] it never resolves an
    /// overload or runs the operator.
    ///
    /// # Errors
    ///
    /// An argument that has no term form, or a session failure.
    pub fn apply_node(
        &self,
        name: &str,
        args: &[Val],
    ) -> Result<Val, KernelError> {
        let mut terms = Vec::with_capacity(args.len());
        for arg in args {
            terms.push(self.as_term(name, arg)?);
        }
        let term = self
            .session
            .call(name, &terms)
            .map_err(|error| from_compute_error(&error).at_operator(name))?;
        Ok(Val::Term(Sym::new(self.id.0, term.id())))
    }

    /// The operator name and the children of a term: the structural view
    /// a host needs to take an answer such as `y(x) = C1*exp(x)` apart.
    ///
    /// # Errors
    ///
    /// A stale or foreign handle.
    pub fn structure(
        &self,
        sym: Sym,
    ) -> Result<(String, Vec<Sym>), KernelError> {
        let term = self.term(sym)?;
        let children = term
            .children()
            .into_iter()
            .map(|child| Sym::new(self.id.0, child.id()))
            .collect();
        Ok((term.op_name().to_string(), children))
    }

    /// The session's identity, for rejecting stale handles.
    #[must_use]
    pub const fn id(&self) -> SessionId {
        self.id
    }

    /// The manifest this session was built with.
    #[must_use]
    pub fn manifest(&self) -> Arc<KernelManifest> {
        self.session.manifest()
    }

    /// Calls an operator by name.
    ///
    /// # Deprecated
    ///
    /// This is the **pre-pinning** path: it hands the kernel a name and lets
    /// the kernel choose an overload, which is exactly the freedom pinning
    /// removes. It exists only because the checker still types `Rssn.<op>` as
    /// `manifest.overloads[0]`; it is scheduled for deletion, and no new
    /// caller may use it. New code resolves once with
    /// [`crate::registry::Registry::resolve`] and executes the key it got with
    /// [`Kernel::call_key`].
    ///
    /// It is kept compiling (rather than `#[deprecated]`) so that the runtime
    /// keeps working until the checker lands the pinned key; see
    /// `.workbuddy-ai/audit/bridge-fixed.md`.
    ///
    /// # Errors
    ///
    /// Maps every kernel failure onto a diagnostic.
    pub fn call(
        &self,
        name: &str,
        args: &[Val],
    ) -> Result<Val, KernelError> {
        self.call_opts(name, args, &Opts::symbolic())
    }

    /// Calls an operator by name with options.
    ///
    /// # Deprecated
    ///
    /// The pre-pinning path, as [`Kernel::call`]: it re-resolves the overload
    /// from the argument values. Deleted when the checker pins `OpKey`s.
    ///
    /// # Errors
    ///
    /// Maps every kernel failure onto a diagnostic.
    pub fn call_opts(
        &self,
        name: &str,
        args: &[Val],
        opts: &Opts,
    ) -> Result<Val, KernelError> {
        // The name-based ABI is the manifest's `dynamic_call` feature; a build
        // without it can only run pinned keys, and says so with a named
        // capability instead of an obscure failure inside the kernel.
        let registry = crate::registry::registry();
        registry.capabilities.require("core.dynamic_call")?;
        if let Some(entry) = registry.op(name) {
            registry.capabilities.require_op(entry)?;
        }
        let converted = self.convert(name, args)?;
        let mut ctx = CallCtx::new();
        for (name, value) in &opts.bindings {
            ctx = ctx.bind(name, *value);
        }
        if let Some(tolerance) = opts.numeric {
            ctx = ctx.numeric(tolerance);
        }
        let value = self
            .session
            .call_value(name, &converted, &ctx)
            .map_err(|error| from_op_error(&error).at_operator(name))?;
        Val::from_rssn(&value, self.id)
    }

    /// Calls the dynamic ABI while preserving explicit input and output witnesses.
    pub fn call_with_witnesses(
        &self,
        name: &str,
        args: &[Val],
        witnesses: &[crate::witness::DimensionWitness],
    ) -> Result<(Val, crate::witness::DimensionWitness), KernelError> {
        if args.len() != witnesses.len() {
            return Err(KernelError::new(
                "E0899",
                "kernel argument witness count differs from argument count",
            ));
        }
        let registry = crate::registry::registry();
        registry.capabilities.require("core.dynamic_call")?;
        let Some(entry) = registry.op(name) else {
            // The process registry is the startup manifest. An operator a
            // plugin registered in *this session* afterwards (§13.5.2) is
            // still callable by name; it carries no dimension witness.
            if self.session.manifest().ops.iter().any(|op| op.name == name) {
                let value = self.call(name, args)?;
                return Ok((value, crate::witness::DimensionWitness::Unknown));
            }
            return Err(KernelError::new(
                "E1003",
                format!("rssn has no operator `{name}`"),
            ));
        };
        registry.capabilities.require_op(entry)?;
        let mut converted = self.convert(name, args)?;
        if entry.source.is_exact() && entry.signatures.len() == 1 {
            let signature = &entry.signatures[0];
            for (index, (value, witness)) in converted.iter_mut().zip(witnesses).enumerate() {
                if let Some(param) = signature
                    .params
                    .get(index)
                    .or_else(|| signature.variadic.as_ref().map(|(_, _, ty)| ty))
                {
                    fn known(
                        sig: &crate::SigType,
                        witness: &crate::witness::DimensionWitness,
                    ) -> bool {
                        use crate::witness::DimensionWitness as W;
                        match (sig, witness) {
                            | (
                                crate::SigType::Dim(_, _)
                                | crate::SigType::Int
                                | crate::SigType::Real
                                | crate::SigType::Float
                                | crate::SigType::Complex,
                                W::Scalar(d),
                            ) => d.is_concrete(),
                            | (
                                crate::SigType::Dim(_, _)
                                | crate::SigType::Int
                                | crate::SigType::Real
                                | crate::SigType::Float
                                | crate::SigType::Complex,
                                _,
                            ) => false,
                            | (
                                crate::SigType::Tensor { elem, .. }
                                | crate::SigType::Sparse { elem, .. },
                                W::Tensor {
                                    dimension: Some(d), ..
                                },
                            ) if matches!(
                                elem.as_ref(),
                                crate::SigType::Dim(_, _)
                                    | crate::SigType::Int
                                    | crate::SigType::Real
                                    | crate::SigType::Float
                                    | crate::SigType::Complex
                            ) =>
                            {
                                d.is_concrete()
                            },
                            | (
                                crate::SigType::Tensor { elem, .. }
                                | crate::SigType::Sparse { elem, .. },
                                W::Tensor {
                                    element, elements, ..
                                },
                            ) => {
                                if elements.is_empty() {
                                    element.as_deref().is_some_and(|w| known(elem, w))
                                } else {
                                    elements.iter().all(|w| known(elem, w))
                                }
                            },
                            | (crate::SigType::Record(fields), W::Record(values)) => {
                                fields.iter().all(|(name, ty)| {
                                    values
                                        .iter()
                                        .find(|(n, _)| n == name)
                                        .is_some_and(|(_, w)| known(ty, w))
                                })
                            },
                            | (
                                crate::SigType::Tensor { .. }
                                | crate::SigType::Sparse { .. }
                                | crate::SigType::Record(_),
                                _,
                            ) => false,
                            | _ => true,
                        }
                    }
                    if !known(param, witness) {
                        return Err(KernelError::capability("dim.dynamic_input_witness"));
                    }
                    crate::witness::attach(value, witness, param)?;
                }
            }
        } else {
            fn dimensionless(w: &crate::witness::DimensionWitness) -> bool {
                use crate::witness::DimensionWitness as W;
                match w {
                    | W::Unknown => true,
                    | W::Scalar(d) => d.is_one(),
                    | W::Tensor {
                        dimension,
                        element,
                        elements,
                        ..
                    } => {
                        dimension.as_ref().is_none_or(aevia_dim::Dim::is_one)
                            && element.as_ref().is_none_or(|w| dimensionless(w))
                            && elements.iter().all(dimensionless)
                    },
                    | W::Record(fields) => fields.iter().all(|(_, w)| dimensionless(w)),
                }
            }
            if !witnesses.iter().all(dimensionless) {
                return Err(KernelError::capability("dim.dynamic_input_witness"));
            }
        }
        let raw = self
            .session
            .call_value(name, &converted, &CallCtx::new())
            .map_err(|error| from_op_error(&error).at_operator(name))?;
        self.refuse_declined(name, &raw)?;
        let trusted = (entry.source.is_exact() && entry.is_value_native())
            || result_is_dimensionless(entry, witnesses);
        let mut witness = crate::witness::extract(&raw, trusted);
        let mut value = Val::from_rssn(&raw, self.id)?;
        // A graph-backed operator answers `[[1, 1, 2], [2]]` as a `list` term.
        // For a result already known to be dimensionless, read the term back
        // into a tensor (nested when the rows differ in length) so that
        // `from_value<Tensor<..>>` can decode it. Anything else stays a term.
        if trusted
            && let Val::Term(sym) = &value
            && let Some((lifted, lifted_witness)) = self.lift_list(*sym)
        {
            value = lifted;
            witness = lifted_witness;
        }
        Ok((value, witness))
    }

    /// Reads a `list(...)` term of numbers (or of such lists) as a tensor.
    ///
    /// Rows of equal shape merge into one higher-rank tensor; unequal rows
    /// stay a tensor of tensors. Returns `None` for anything that is not a
    /// well-formed, non-empty numeric list, in which case the caller keeps
    /// the term. The witness claims dimension 1: the caller must only use this
    /// where the operator's rule already made the result dimensionless.
    fn lift_list(
        &self,
        sym: Sym,
    ) -> Option<(Val, crate::witness::DimensionWitness)> {
        use crate::witness::DimensionWitness as W;
        let (op, children) = self.structure(sym).ok()?;
        if op != "list" || children.is_empty() {
            return None;
        }
        let one = aevia_dim::Dim::one();
        let mut items: Vec<(Val, W)> = Vec::with_capacity(children.len());
        for child in children {
            let item = match self.structure(child) {
                | Ok((child_op, _)) if child_op == "list" => self.lift_list(child)?,
                | _ => {
                    match self.value_of(child).ok()? {
                        | value @ Val::Num(_) => (value, W::Scalar(one.clone())),
                        | _ => return None,
                    }
                },
            };
            items.push(item);
        }
        let repr = |value: &Val| {
            match value {
                | Val::Num(Num::Int(_)) => Some(crate::SigType::Int),
                | Val::Num(Num::Rat(_)) => Some(crate::SigType::Real),
                | Val::Num(Num::Float(_)) => Some(crate::SigType::Float),
                | _ => None,
            }
        };
        if items.iter().all(|(value, _)| matches!(value, Val::Num(_))) {
            // Mixed Int/Rat leaves are Real; Float never mixes with exact.
            let mut element_repr = crate::SigType::Int;
            for (value, _) in &items {
                match repr(value)? {
                    | crate::SigType::Int => {},
                    | crate::SigType::Real if element_repr == crate::SigType::Int => {
                        element_repr = crate::SigType::Real;
                    },
                    | crate::SigType::Real => {},
                    | crate::SigType::Float if element_repr == crate::SigType::Int => {
                        element_repr = crate::SigType::Float;
                    },
                    | crate::SigType::Float => {},
                    | _ => return None,
                }
            }
            let data: Vec<Val> = items.into_iter().map(|(value, _)| value).collect();
            return Some((
                Val::Tensor {
                    shape: vec![data.len()],
                    data,
                },
                W::Tensor {
                    dimension: Some(one.clone()),
                    element: Some(Box::new(W::Scalar(one))),
                    element_repr: Some(element_repr),
                    elements: Vec::new(),
                },
            ));
        }
        let n = items.len();
        let first_shape = match &items[0].0 {
            | Val::Tensor { shape, .. } => Some(shape.clone()),
            | _ => None,
        };
        let uniform = first_shape.as_ref().is_some_and(|first| {
            items.iter().all(|(value, witness)| {
                matches!(value, Val::Tensor { shape, .. } if shape == first)
                    && *witness == items[0].1
            })
        });
        if uniform {
            let mut shape = vec![n];
            shape.extend(first_shape?);
            let witness = items[0].1.clone();
            let mut data = Vec::new();
            for (value, _) in items {
                if let Val::Tensor { data: inner, .. } = value {
                    data.extend(inner);
                }
            }
            return Some((Val::Tensor { shape, data }, witness));
        }
        // Parts of unlike kind (tensors and a number, as in the `list(core,
        // factors, error)` of a decomposition) stay a rank-1 value with one
        // witness per part: no Aevia tensor type holds them, so
        // `from_value<Tensor<1, Value>>` hands each part out as a handle and
        // `from_value<T>` decodes it.
        let witnesses: Vec<W> = items.iter().map(|(_, w)| w.clone()).collect();
        let data: Vec<Val> = items.into_iter().map(|(value, _)| value).collect();
        Some((
            Val::Tensor { shape: vec![n], data },
            W::Tensor {
                dimension: None,
                element: None,
                element_repr: None,
                elements: witnesses,
            },
        ))
    }

    /// Calls exactly the overload `key` names.
    ///
    /// This is the execution entry point of pinned calls: the key was chosen
    /// statically (or once, by [`Kernel::resolve`]) and is executed as it
    /// stands. Nothing here looks for a better match, and no error path falls
    /// back to [`Kernel::call`].
    ///
    /// # Errors
    ///
    /// `E1003` when the key does not name a live overload, plus whatever the
    /// kernel reports; every error names the operator and keeps the kernel's
    /// own `OpError` in [`KernelError::cause`].
    pub fn call_key(
        &self,
        key: &OpKey,
        args: &[Val],
    ) -> Result<Val, KernelError> {
        let registry = crate::registry::registry();
        if let Some(entry) = registry.op(&key.name) {
            registry.capabilities.require_op(entry)?;
        }
        let converted = self.convert(&key.name, args)?;
        let value = self
            .session
            .call_key(key, &converted, &CallCtx::new())
            .map_err(|error| from_op_error(&error).at_operator(&key.name))?;
        self.refuse_declined(&key.name, &value)?;
        Val::from_rssn(&value, self.id)
    }

    /// Turns a call the operator declined into the kernel's reason.
    ///
    /// An operator whose evaluator rejects its arguments leaves the call as an
    /// unreduced term headed by its own name. That is the right answer for a
    /// rewrite system and the wrong one for a caller who has been promised a
    /// number, so a result that is exactly the call is explained with
    /// `Session::explain` and refused with the finding (`rssn.diag.<kind>`,
    /// the argument at fault, the message). A symbolic argument is not a
    /// refusal: the call stays as the answer. Any other term, a heavy operator
    /// that recorded no reason included, is returned as before.
    fn refuse_declined(
        &self,
        name: &str,
        raw: &Value,
    ) -> Result<(), KernelError> {
        let Value::Term(handle) = raw else {
            return Ok(());
        };
        let term = self.term(Sym::new(self.id.0, handle.node))?;
        if &*term.op_name() != name {
            return Ok(());
        }
        match self.session.explain(term) {
            | Some(diag) if diag.kind != rssn::kernel::DiagKind::Symbolic => {
                Err(KernelError::from_diag(diag.in_op(name)))
            },
            | _ => Ok(()),
        }
    }

    /// Calls a value-native overload while carrying explicit argument and result
    /// dimension witnesses across the kernel ABI.
    ///
    /// # Errors
    /// Reports an unknown key, malformed witness list, conversion failure, or
    /// the kernel's error without replacing it with another overload.
    pub fn call_key_with_witnesses(
        &self,
        key: &OpKey,
        args: &[Val],
        witnesses: &[crate::witness::DimensionWitness],
    ) -> Result<(Val, crate::witness::DimensionWitness), KernelError> {
        if args.len() != witnesses.len() {
            return Err(KernelError::new(
                "E0899",
                "kernel argument witness count differs from argument count",
            ));
        }
        let entry = crate::registry::registry().op(&key.name).ok_or_else(|| {
            KernelError::new("E1003", format!("rssn has no operator `{}`", key.name))
        })?;
        crate::registry::registry().capabilities.require_op(entry)?;
        let index = entry
            .key_index(key)
            .ok_or_else(|| KernelError::new("E1005", "kernel overload key is not registered"))?;
        let signature = entry
            .signatures
            .get(index)
            .ok_or_else(|| KernelError::new("E0899", "kernel signature is missing"))?;
        let mut converted = self.convert(&key.name, args)?;
        for (index, (value, witness)) in converted.iter_mut().zip(witnesses).enumerate() {
            if let Some(param) = signature
                .params
                .get(index)
                .or_else(|| signature.variadic.as_ref().map(|(_, _, ty)| ty))
            {
                crate::witness::attach(value, witness, param)?;
            }
        }
        let raw = self
            .session
            .call_key(key, &converted, &CallCtx::new())
            .map_err(|error| from_op_error(&error).at_operator(&key.name))?;
        self.refuse_declined(&key.name, &raw)?;
        let witness = crate::witness::extract(&raw, entry.source.is_exact());
        Ok((Val::from_rssn(&raw, self.id)?, witness))
    }

    /// Calls a higher-order operator with **no host callbacks**.
    ///
    /// This convenience method supplies an empty callback list and therefore
    /// follows the ordinary dynamic path. For non-empty callbacks use
    /// [`Kernel::call_ho_scoped`], which registers immutable signatures and
    /// passes thread-safe handles for the duration of the call.
    ///
    /// # Errors
    ///
    /// `E1001("core.dynamic_call")` when the build has no name-based ABI, plus
    /// whatever the kernel reports.
    pub fn call_ho(
        &self,
        name: &str,
        args: &[Val],
    ) -> Result<Val, KernelError> {
        let registry = crate::registry::registry();
        registry.capabilities.require("core.dynamic_call")?;
        if let Some(entry) = registry.op(name) {
            registry.capabilities.require_op(entry)?;
        }
        let converted = self.convert(name, args)?;
        let value = self
            .session
            .call_ho(name, &converted, &[], &CallCtx::new())
            .map_err(|error| from_op_error(&error).at_operator(name))?;
        Val::from_rssn(&value, self.id)
    }

    /// Calls a higher-order operator with explicitly scoped callbacks and
    /// physical input/output witnesses. Callback registrations live only for
    /// this call and each bank entry snapshots its concrete signature.
    pub fn call_ho_scoped(
        &self,
        name: &str,
        args: &[Val],
        witnesses: &[crate::witness::DimensionWitness],
        callbacks: &[Arc<dyn BridgeCallback>],
    ) -> Result<(Val, crate::witness::DimensionWitness), KernelError> {
        self.call_ho_scoped_inner(name, None, args, witnesses, callbacks)
    }

    /// Calls exactly the manifest-pinned higher-order overload with scoped callbacks.
    ///
    /// # Errors
    /// Returns a stale-key, witness, callback, or kernel execution error.
    pub fn call_ho_scoped_key(
        &self,
        key: &OpKey,
        args: &[Val],
        witnesses: &[crate::witness::DimensionWitness],
        callbacks: &[Arc<dyn BridgeCallback>],
    ) -> Result<(Val, crate::witness::DimensionWitness), KernelError> {
        self.call_ho_scoped_inner(&key.name, Some(key), args, witnesses, callbacks)
    }

    fn call_ho_scoped_inner(
        &self,
        name: &str,
        key: Option<&OpKey>,
        args: &[Val],
        witnesses: &[crate::witness::DimensionWitness],
        callbacks: &[Arc<dyn BridgeCallback>],
    ) -> Result<(Val, crate::witness::DimensionWitness), KernelError> {
        if args.len() != witnesses.len() {
            return Err(KernelError::new(
                "E0899",
                "kernel argument witness count differs from argument count",
            ));
        }
        let registry = crate::registry::registry();
        registry.capabilities.require("core.dynamic_call")?;
        let entry = registry
            .op(name)
            .ok_or_else(|| KernelError::new("E1003", format!("rssn has no operator `{name}`")))?;
        if !entry.higher_order || !entry.source.is_exact() || entry.signatures.len() != 1 {
            return Err(KernelError::capability("core.callback.signature"));
        }
        if let Some(key) = key
            && entry.key_index(key) != Some(0)
        {
            return Err(KernelError::new(
                "E1005",
                "kernel callback overload key is not registered",
            ));
        }
        let signature = &entry.signatures[0];
        for (index, (value, witness)) in args.iter().zip(witnesses).enumerate() {
            let param = signature
                .params
                .get(index)
                .ok_or_else(|| KernelError::new("E1003", "too many callback operator arguments"))?;
            validate_callback_input(value, witness, param)?;
        }
        let mut converted = self.convert(name, args)?;
        for (index, (value, witness)) in converted.iter_mut().zip(witnesses).enumerate() {
            let param = signature
                .params
                .get(index)
                .ok_or_else(|| KernelError::new("E1003", "too many callback operator arguments"))?;
            if matches!(
                param,
                crate::SigType::Dim(_, _)
                    | crate::SigType::Int
                    | crate::SigType::Real
                    | crate::SigType::Float
                    | crate::SigType::Complex
            ) && !matches!(witness, crate::witness::DimensionWitness::Scalar(dim) if dim.is_concrete())
            {
                return Err(KernelError::capability("dim.callback_input_witness"));
            }
            crate::witness::attach(value, witness, param)?;
        }
        let mut ctx = CallCtx::new();
        let mut handles = Vec::with_capacity(callbacks.len());
        for callback in callbacks {
            let sig = callback.signature();
            if sig.variadic.is_some() {
                return Err(KernelError::capability("core.callback.variadic"));
            }
            if !sig.params.iter().all(supported_callback_schema)
                || !supported_callback_schema(&sig.ret)
            {
                return Err(KernelError::capability("core.callback.signature"));
            }
            let adapter = CallbackAdapter {
                callback: Arc::clone(callback),
                signature: sig,
                session: self.id,
            };
            handles.push(
                ctx.add_callback(Arc::new(adapter))
                    .map_err(|e| from_op_error(&e))?,
            );
        }
        let raw = if let Some(key) = key {
            rssn::kernel::call::Kernel::call_ho(&self.session, key, &converted, &handles, &ctx)
        } else {
            self.session.call_ho(name, &converted, &handles, &ctx)
        }
        .map_err(|e| from_op_error(&e).at_operator(name))?;
        let witness = crate::witness::extract(&raw, true);
        Ok((Val::from_rssn(&raw, self.id)?, witness))
    }

    /// The solver's structured answer for `equation = 0`, as §8.6's record.
    ///
    /// `operator` is `solve` or `solve_complex`. `solve` goes through the
    /// kernel's own [`Session::solve_structured`], so the completeness claim
    /// is the solver's: a list answer is the exhaustive form (`complete`),
    /// a bare value is one root found on the way and claims nothing about the
    /// others. `solve_complex` has no structured entry point in the kernel,
    /// so the same classification is applied to its answer here.
    ///
    /// The answer is a [`Val::Record`] of the five fields, not a bare list of
    /// roots: multiplicities, conditions and completeness cannot be recovered
    /// from a list, and dropping them is what made `s.complete` a constant.
    ///
    /// # Errors
    ///
    /// `E1003` when the equation or the unknown has no term form, plus
    /// whatever the solver reports.
    pub fn solve_structured(
        &self,
        operator: &str,
        equation: &Val,
        variable: &Val,
    ) -> Result<Val, KernelError> {
        let name = match variable {
            | Val::Symbol(name) => Some(name.clone()),
            | _ => None,
        };
        let solutions = if operator == "solve" {
            let equation = self.term(self.term_of(equation)?)?;
            let variable = self.term(self.term_of(variable)?)?;
            let mut solutions = self
                .session
                .solve_structured(equation, variable)
                .map_err(|error| from_op_error(&error).at_operator(operator))?;
            // The kernel names the unknown from the term it was handed. The
            // caller's spelling is the one §8.6 means by `variable`, and it is
            // the only one the runtime can clean of internal suffixes.
            solutions.variable = name;
            solutions
        } else {
            let converted = self.convert(operator, &[equation.clone(), variable.clone()])?;
            let value = self
                .session
                .call_value(operator, &converted, &CallCtx::new())
                .map_err(|error| from_op_error(&error).at_operator(operator))?;
            // The kernel's classification, verbatim: `Session::solve_structured`
            // reads a `Tensor`/`Sparse` answer as the exhaustive form and any
            // other value as a single root that is not a claim of totality.
            let (roots, complete) = match value {
                | Value::Tensor(tensor) => (tensor.data, true),
                | Value::Sparse(sparse) => (sparse.data, true),
                | other => (vec![other], false),
            };
            Solutions {
                variable: name,
                roots,
                multiplicities: Vec::new(),
                conditions: Vec::new(),
                complete,
            }
        };
        Val::from_solutions(&solutions, self.id)
    }

    /// Every typed overload of `name`.
    #[must_use]
    pub fn overloads(
        &self,
        name: &str,
    ) -> Vec<(OpKey, Signature)> {
        self.session.overloads(name)
    }

    /// The overload `args` belong to.
    ///
    /// This is `kernel::overload`'s lattice over the converted argument
    /// values: the one place the runtime chooses an overload, and the only
    /// thing the unkeyed path may call. A tie is `E1002` and is never resolved
    /// by a second algorithm.
    ///
    /// # Errors
    ///
    /// `E1002` when several overloads tie, `E1003` when none matches.
    pub fn resolve(
        &self,
        name: &str,
        args: &[Val],
    ) -> Result<OpKey, KernelError> {
        let converted = self.convert(name, args)?;
        self.session
            .resolve_overload(name, &converted)
            .map_err(|error| from_op_error(&error).at_operator(name))
    }

    /// Builds a request and then runs the rewriter on it.
    ///
    /// The heavy operators (`integral`, `limit`, `nsolve`, ...) are not
    /// identities: `call_value` only constructs their node, and the answer
    /// appears when the engine reduces it. This is the path §10.6.1 asks for.
    ///
    /// # Errors
    ///
    /// Maps every kernel failure onto a diagnostic.
    pub fn compute_call(
        &self,
        name: &str,
        args: &[Val],
        opts: &Opts,
    ) -> Result<Computed, KernelError> {
        let built = self.call_opts(name, args, &Opts::symbolic())?;
        let Val::Term(sym) = built else {
            // The operator answered outright; there is nothing to reduce.
            return Ok(Computed {
                val: built,
                numeric: None,
                reduced: true,
            });
        };
        self.compute(sym, opts)
    }

    /// Runs a whole request: rewrite to a closed form, or evaluate numerically.
    ///
    /// # Errors
    ///
    /// Maps every kernel failure onto a diagnostic.
    pub fn compute(
        &self,
        sym: Sym,
        opts: &Opts,
    ) -> Result<Computed, KernelError> {
        let term = self.term(sym)?;
        let config = opts.config();
        let answer = self
            .session
            .compute(term, &config)
            .map_err(|error| from_compute_error(&error).at_operator("compute"))?;
        let value = self
            .session
            .value_of(answer.term)
            .map_err(|error| from_op_error(&error).at_operator("compute"))?;
        Ok(Computed {
            val: Val::from_rssn(&value, self.id)?,
            numeric: answer.value,
            reduced: answer.reduced,
        })
    }

    /// Materialises an expression.
    ///
    /// # Errors
    ///
    /// Propagates the kernel's failure for any node it cannot build.
    pub fn build(
        &self,
        expr: &Expr,
    ) -> Result<Val, KernelError> {
        match expr {
            | Expr::Num(number) => Ok(Val::Num(number.clone())),
            | Expr::Sym(name) => Ok(Val::Symbol(name.clone())),
            | Expr::Term(sym) => {
                // Check liveness now: a stale handle must fail here rather
                // than when the enclosing call finally runs.
                self.term(*sym)?;
                Ok(Val::Term(*sym))
            },
            | Expr::App(name, args) => {
                // Build the node, do not call the operator. `call_value`
                // resolves an overload and checks argument types, which is
                // wrong here: Aevia's checker already decided the types, and a
                // symbolic argument is a term, not a `Real`. `Session::call`
                // is the node constructor, which is what §10.6.1 asks for.
                if let Some(entry) = crate::registry::registry().op(name)
                    && !entry.accepts(args.len())
                {
                    return Err(KernelError::new(
                        "E0315",
                        format!(
                            "`{name}` takes {} argument(s), found {}",
                            entry.arity.0,
                            args.len()
                        ),
                    )
                    .at_operator(name));
                }
                let mut terms = Vec::with_capacity(args.len());
                for arg in args {
                    let value = self.build(arg)?;
                    terms.push(self.as_term(name, &value)?);
                }
                let term = self
                    .session
                    .call(name, &terms)
                    .map_err(|error| from_compute_error(&error).at_operator(name))?;
                Ok(Val::Term(Sym::new(self.id.0, term.id())))
            },
        }
    }

    /// The term a value denotes, for use as an argument.
    fn as_term(
        &self,
        operator: &str,
        value: &Val,
    ) -> Result<rssn::api::Term<'_>, KernelError> {
        Ok(match value {
            | Val::Num(Num::Int(number)) => {
                match number.to_i64() {
                    | Some(value) => self.session.int(value),
                    | None => self.parse_term(&number.to_string())?,
                }
            },
            | Val::Num(Num::Rat(number)) => {
                match (number.numer().to_i64(), number.denom().to_i64()) {
                    | (Some(numer), Some(denom)) => {
                        match self.session.rational(numer, denom) {
                            | Some(term) => term,
                            | None => self.parse_term(&format!("({numer})/({denom})"))?,
                        }
                    },
                    | _ => {
                        self.parse_term(&format!("({})/({})", number.numer(), number.denom()))?
                    },
                }
            },
            | Val::Num(Num::Float(number)) => self.session.float(*number),
            | Val::Symbol(name) => self.session.sym(name),
            | Val::Term(sym) => self.term(*sym)?,
            // A unit and a quantity are values of the kernel's term graph
            // (`Value::Unit`, `Value::Quantity`), so the session is the only
            // thing that can say what term they denote. Refusing them here
            // made `Rssn.quantity(3.0, Units.m)` — which type-checks — fail at
            // the first node that used it.
            | other => {
                let value = self.one(operator, other)?;
                let term = self
                    .session
                    .term_of(&value)
                    .map_err(|error| from_op_error(&error).at_operator("build"))?;
                let node = term.id();
                self.session
                    .term_ref(TermRef {
                        session: self.id,
                        node,
                    })
                    .map_err(|error| from_op_error(&error).at_operator("build"))?
            },
        })
    }

    /// Parses a term from text, for numbers that do not fit an `i64`.
    fn parse_term(
        &self,
        text: &str,
    ) -> Result<rssn::api::Term<'_>, KernelError> {
        self.session
            .parse(text)
            .map_err(|error| from_compute_error(&error))
    }

    /// A free symbol.
    #[must_use]
    pub fn symbol(
        &self,
        name: &str,
    ) -> Val {
        Val::Symbol(name.to_string())
    }

    /// The source form of a term.
    ///
    /// Lossy on purpose: this is the `show` path, where a stale handle must
    /// still render *something*. A caller that needs to know whether the
    /// handle is live uses [`Self::try_text`] or [`Self::require_live`].
    #[must_use]
    pub fn text(
        &self,
        sym: Sym,
    ) -> String {
        match self.term(sym) {
            | Ok(term) => term.to_string(),
            | Err(_) => "<stale term>".to_string(),
        }
    }

    /// The source form of a term, or why it cannot be printed.
    ///
    /// # Errors
    ///
    /// `E0899` when the handle does not name a live node of this session.
    pub fn try_text(
        &self,
        sym: Sym,
    ) -> Result<String, KernelError> {
        Ok(self.term(sym)?.to_string())
    }

    /// Whether `sym` still names a node of this session.
    ///
    /// The runtime calls this before it trusts a symbolic value it did not
    /// build itself; a failed check is a diagnostic, never a default value.
    ///
    /// # Errors
    ///
    /// `E0899` when the session or the node is gone.
    pub fn require_live(
        &self,
        sym: Sym,
    ) -> Result<(), KernelError> {
        self.term(sym).map(|_| ())
    }

    /// Differentiates `sym` with respect to `variable`.
    ///
    /// This exists so that a failed differentiation has somewhere to go.
    /// `Term::Foreign::diff` used to call the operator and answer `0` on
    /// failure, which is a wrong answer of exactly the kind a CAS must never
    /// produce; the fallible form makes the failure a diagnostic the runtime
    /// can raise as `E0899`/`E0807`.
    ///
    /// # Errors
    ///
    /// `E0899` when the handle is not live, plus whatever the kernel reports.
    pub fn diff(
        &self,
        sym: Sym,
        variable: &str,
    ) -> Result<Val, KernelError> {
        self.require_live(sym)
            .map_err(|_| KernelError::foreign(sym, "diff"))?;
        self.call("diff", &[Val::Term(sym), Val::Symbol(variable.to_string())])
    }

    /// The free symbols of a term, or why they cannot be read.
    ///
    /// # Errors
    ///
    /// `E0899` when the handle is not live.
    pub fn try_free_symbols(
        &self,
        sym: Sym,
    ) -> Result<Vec<String>, KernelError> {
        Ok(self.term(sym)?.free_symbols())
    }

    /// The free symbols of a term.
    ///
    /// Lossy on purpose and only for the show path: a stale handle answers the
    /// empty list, which reads as "no free symbols" — that is, as a closed
    /// form. A caller that draws a conclusion from the emptiness (evaluating a
    /// term, deciding it is constant) must use [`Self::try_free_symbols`].
    #[must_use]
    pub fn free_symbols(
        &self,
        sym: Sym,
    ) -> Vec<String> {
        self.try_free_symbols(sym).unwrap_or_default()
    }

    /// Evaluates a term numerically.
    ///
    /// `None` means "no numeric value": a stale handle, a free symbol without
    /// a binding, or a result the evaluator cannot reach. The failure is in
    /// the `Option`, never in a substituted value.
    #[must_use]
    pub fn eval(
        &self,
        sym: Sym,
        bindings: &[(String, f64)],
    ) -> Option<f64> {
        let term = self.term(sym).ok()?;
        let pairs: Vec<(&str, f64)> = bindings
            .iter()
            .map(|(name, value)| (name.as_str(), *value))
            .collect();
        term.eval(&pairs)
    }

    /// Replaces `from` by `to` inside `sym`.
    ///
    /// # Errors
    ///
    /// Propagates the kernel's failure.
    pub fn substitute(
        &self,
        sym: Sym,
        from: Sym,
        to: &Val,
    ) -> Result<Val, KernelError> {
        let target = self.term(sym)?;
        let source = self.term(from)?;
        let value = self.one("substitute", to)?;
        let replacement = self
            .session
            .term_of(&value)
            .map_err(|error| from_op_error(&error).at_operator("substitute"))?;
        let result = target.substitute(&[(source, replacement)]);
        let value = self
            .session
            .value_of(result)
            .map_err(|error| from_op_error(&error).at_operator("substitute"))?;
        Val::from_rssn(&value, self.id)
    }

    /// The term a value denotes.
    ///
    /// # Errors
    ///
    /// `E1003` when the value has no term form.
    pub fn term_of(
        &self,
        val: &Val,
    ) -> Result<Sym, KernelError> {
        let value = self.one("term_of", val)?;
        let term = self
            .session
            .term_of(&value)
            .map_err(|error| from_op_error(&error).at_operator("term_of"))?;
        Ok(Sym::new(self.id.0, term.id()))
    }

    /// The value of a term.
    ///
    /// # Errors
    ///
    /// Propagates the kernel's failure.
    pub fn value_of(
        &self,
        sym: Sym,
    ) -> Result<Val, KernelError> {
        let term = self.term(sym)?;
        let value = self
            .session
            .value_of(term)
            .map_err(|error| from_op_error(&error).at_operator("value_of"))?;
        Val::from_rssn(&value, self.id)
    }

    /// The term a handle names.
    fn term(
        &self,
        sym: Sym,
    ) -> Result<rssn::api::Term<'_>, KernelError> {
        if sym.session != self.id.0 {
            return Err(KernelError::stale_term(sym, self.id.0));
        }
        self.session
            .term_ref(TermRef {
                session: self.id,
                node: sym.node,
            })
            // `term_ref` says `Internal` for a node id the graph does not
            // have. That is a dead handle, not an invariant violation, and the
            // caller has to be able to tell it from a kernel bug.
            .map_err(|error| {
                match error {
                    | OpError::Internal(_) => KernelError::foreign(sym, "term"),
                    | other => from_op_error(&other),
                }
            })
    }

    fn convert(
        &self,
        operator: &str,
        args: &[Val],
    ) -> Result<Vec<Value>, KernelError> {
        let mut converted = Vec::with_capacity(args.len());
        for arg in args {
            converted.push(self.one(operator, arg)?);
        }
        Ok(converted)
    }

    /// Converts one argument, validating a term handle against this session.
    ///
    /// The check lives here and not in [`Val::to_rssn`] because only the
    /// session can answer it: a `Val` carries no kernel, and the thread-local
    /// one is a different session whenever a caller built its own.
    fn one(
        &self,
        operator: &str,
        arg: &Val,
    ) -> Result<Value, KernelError> {
        if let Val::Term(sym) = arg {
            // A handle from another session is the call's failure, so the
            // diagnostic names the call: `E0899` without an operator is not
            // actionable in a program that calls a thousand of them.
            self.require_live(*sym)
                .map_err(|error| error.at_operator(operator))?;
        }
        arg.to_rssn(self.id)
            .map_err(|error| error.at_operator(operator))
    }
}

impl Default for Kernel {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether a handle names a live node of the current thread's session.
///
/// `Session::term_ref` checks the session and the node id, so this is a
/// convenience for callers that hold only a `Sym`. A caller with its own
/// [`Kernel`] must use [`Kernel::require_live`] instead: this function
/// consults the thread-local session, which is a different one.
#[must_use]
pub fn node_exists(sym: Sym) -> bool {
    kernel().require_live(sym).is_ok()
}

/// Builds a rational from two integers.
#[must_use]
pub fn rational(
    numer: i64,
    denom: i64,
) -> Num {
    Num::Rat(BigRational::new(BigInt::from(numer), BigInt::from(denom)))
}

/// Whether the kernel's *own dimension rule* makes a bare numeric result
/// dimensionless.
///
/// A `Derived` signature never states a dimension, so before this rule a
/// derived operator's number could not "prove" `Int` or `Float` at
/// `from_value`, however obviously dimensionless it was (`invmod`, `binomial`,
/// `totient`). The manifest does state the rule, though:
///
/// * `Dimensionless` says the result is a pure number whatever the arguments;
/// * `List`, `NonMetric`, `Compare` and `Angle` yield pure numbers only when
///   every argument is itself *known* to be dimensionless.
///
/// Any other rule (`Same`, `Sum`, `Product`, `Custom`, ...) depends on the
/// arguments' dimensions and is not trusted here, so a dimensioned result is
/// still never mislabelled.
fn result_is_dimensionless(
    entry: &crate::registry::OpEntry,
    witnesses: &[crate::witness::DimensionWitness],
) -> bool {
    use crate::witness::DimensionWitness as W;
    use rssn::kernel::signature::DimRuleInfo as R;
    fn known_one(w: &W) -> bool {
        match w {
            | W::Unknown => false,
            | W::Scalar(d) => d.is_one(),
            | W::Tensor {
                dimension,
                element,
                elements,
                ..
            } => {
                dimension.as_ref().is_none_or(aevia_dim::Dim::is_one)
                    && element.as_ref().is_none_or(|w| known_one(w))
                    && elements.iter().all(known_one)
            },
            | W::Record(fields) => fields.iter().all(|(_, w)| known_one(w)),
        }
    }
    // A curated signature (`domains.overlay`) is the overlay's statement that
    // the operator is a pure function of dimensionless arguments, whatever its
    // (often `Custom`) kernel dimension rule says. Inputs were already checked
    // dimensionless by the caller.
    if crate::domains::overlay().sigs.contains_key(&entry.name) {
        return true;
    }
    // A witnessed overload was observed on bare numbers and bare lists and
    // answered with one: with every argument known to be dimensionless, an
    // operator whose value is a function of its arguments cannot produce a
    // dimension. This is what lets the witnessed tier of the domain modules
    // decode `Int` and `Tensor<_, Int>` results; a dimensioned argument still
    // gets no such evidence.
    if (!entry.witnessed.is_empty() || !entry.refinements.is_empty())
        && !witnesses.is_empty()
        && witnesses.iter().all(known_one)
    {
        return true;
    }
    // The same operators, when the kernel itself says the value is not a
    // physical scalar (a count, a flag, a structure): an opaque handle among
    // the arguments carries no dimension witness, and needs none.
    if (!entry.witnessed.is_empty() || !entry.refinements.is_empty())
        && matches!(&entry.dim_rule, Some(R::NonMetric | R::Container))
    {
        return true;
    }
    match &entry.dim_rule {
        | Some(R::Dimensionless) => true,
        | Some(R::List | R::NonMetric | R::Compare | R::Angle) => witnesses.iter().all(known_one),
        | _ => false,
    }
}

#[cfg(test)]
mod recursive_callback_witness_tests {
    use super::*;
    use crate::SigType as S;
    use crate::witness::DimensionWitness as W;
    use rssn::kernel::plugin::HostCallback;
    struct Mock {
        value: Val,
        witness: W,
        result: S,
    }
    impl BridgeCallback for Mock {
        fn signature(&self) -> crate::Sig {
            crate::Sig {
                params: vec![],
                ret: self.result.clone(),
                variadic: None,
            }
        }

        fn call(
            &self,
            _: &[Val],
            _: &[W],
        ) -> Result<(Val, W), KernelError> {
            Ok((self.value.clone(), self.witness.clone()))
        }
    }
    #[test]
    fn callback_input_evidence_is_recursive_and_allows_nonnumeric_unknowns() {
        assert!(validate_callback_input(&Val::Bool(true), &W::Unknown, &S::Bool).is_ok());
        assert!(validate_callback_input(&Val::Str("ok".into()), &W::Unknown, &S::Str).is_ok());
        let length = aevia_dim::Dim::base("Length");
        let schema = S::Record(vec![
            ("flag".into(), S::Bool),
            (
                "nested".into(),
                S::Record(vec![(
                    "x".into(),
                    S::Dim(Box::new(S::Float), length.clone()),
                )]),
            ),
        ]);
        let value = Val::Record(vec![
            ("flag".into(), Val::Bool(true)),
            (
                "nested".into(),
                Val::Record(vec![("x".into(), Val::Num(Num::Float(1.0)))]),
            ),
        ]);
        let missing = W::Record(vec![
            ("flag".into(), W::Unknown),
            ("nested".into(), W::Record(vec![("x".into(), W::Unknown)])),
        ]);
        assert!(validate_callback_input(&value, &missing, &schema).is_err());
        let proven = W::Record(vec![
            ("flag".into(), W::Unknown),
            (
                "nested".into(),
                W::Record(vec![("x".into(), W::Scalar(length))]),
            ),
        ]);
        assert!(validate_callback_input(&value, &proven, &schema).is_ok());
        let duplicate = W::Record(vec![
            ("flag".into(), W::Unknown),
            ("flag".into(), W::Unknown),
        ]);
        assert!(validate_callback_input(&value, &duplicate, &schema).is_err());
    }
    #[test]
    fn callback_empty_tensor_needs_actual_recursive_schema() {
        let element = S::Record(vec![("x".into(), S::Float), ("ok".into(), S::Bool)]);
        let sig = S::Tensor {
            rank: Some(1),
            shape: Some(vec![0]),
            elem: Box::new(element.clone()),
        };
        let value = Val::Tensor {
            shape: vec![0],
            data: vec![],
        };
        let unknown = W::Tensor {
            dimension: None,
            element: None,
            element_repr: None,
            elements: vec![],
        };
        assert!(validate_callback_input(&value, &unknown, &sig).is_err());
        let proven = W::Tensor {
            dimension: None,
            element: Some(Box::new(W::Record(vec![
                ("x".into(), W::Scalar(aevia_dim::Dim::one())),
                ("ok".into(), W::Unknown),
            ]))),
            element_repr: Some(element),
            elements: vec![],
        };
        assert!(validate_callback_input(&value, &proven, &sig).is_ok());
        let wrong = W::Tensor {
            dimension: None,
            element: Some(Box::new(W::Record(vec![
                ("x".into(), W::Unknown),
                ("ok".into(), W::Unknown),
            ]))),
            element_repr: Some(S::Record(vec![
                ("x".into(), S::Float),
                ("ok".into(), S::Bool),
            ])),
            elements: vec![],
        };
        assert!(validate_callback_input(&value, &wrong, &sig).is_err());
    }
    #[test]
    fn nested_record_and_tensor_results_preserve_independent_witnesses() {
        let length = aevia_dim::Dim::base("Length");
        let time = aevia_dim::Dim::base("Time");
        let dim_float = |dim| S::Dim(Box::new(S::Float), dim);
        let result = S::Record(vec![
            ("length".into(), dim_float(length.clone())),
            (
                "samples".into(),
                S::Tensor {
                    rank: Some(1),
                    shape: None,
                    elem: Box::new(dim_float(time.clone())),
                },
            ),
            ("ok".into(), S::Bool),
        ]);
        let value = Val::Record(vec![
            ("length".into(), Val::Num(Num::Float(2.0))),
            (
                "samples".into(),
                Val::Tensor {
                    shape: vec![2],
                    data: vec![Val::Num(Num::Float(1.0)), Val::Num(Num::Float(3.0))],
                },
            ),
            ("ok".into(), Val::Bool(true)),
        ]);
        let tensor_witness = |dim| {
            W::Tensor {
                dimension: Some(dim),
                element: None,
                element_repr: Some(S::Float),
                elements: vec![],
            }
        };
        let witness = W::Record(vec![
            ("length".into(), W::Scalar(length)),
            ("samples".into(), tensor_witness(time.clone())),
            ("ok".into(), W::Unknown),
        ]);
        let mock = Arc::new(Mock {
            value: value.clone(),
            witness: witness.clone(),
            result: result.clone(),
        });
        let adapter = CallbackAdapter {
            callback: mock,
            signature: crate::Sig {
                params: vec![],
                ret: result.clone(),
                variadic: None,
            },
            session: SessionId(0),
        };
        let raw = adapter.call(&[]).expect("validated record");
        let Value::Record(record) = raw else {
            panic!("record")
        };
        let Value::Tensor(samples) = &record
            .fields
            .iter()
            .find(|(name, _)| name == "samples")
            .unwrap()
            .1
        else {
            panic!("tensor")
        };
        assert_eq!(
            samples.elem,
            crate::registry::to_type_expr(&dim_float(time.clone()))
        );
        let bad = W::Record(vec![
            ("length".into(), W::Unknown),
            ("samples".into(), tensor_witness(time)),
            ("ok".into(), W::Unknown),
        ]);
        assert!(validate_callback_result(&value, &bad, &result).is_err());
    }
    #[test]
    fn empty_structured_tensors_need_explicit_schema_evidence() {
        let result = S::Tensor {
            rank: Some(1),
            shape: None,
            elem: Box::new(S::Record(vec![("x".into(), S::Float)])),
        };
        let value = Val::Tensor {
            shape: vec![0],
            data: vec![],
        };
        let witness = W::Tensor {
            dimension: None,
            element: None,
            element_repr: None,
            elements: vec![],
        };
        assert!(validate_callback_result(&value, &witness, &result).is_err());
        let proof = W::Tensor {
            dimension: None,
            element: Some(Box::new(W::Record(vec![(
                "x".into(),
                W::Scalar(aevia_dim::Dim::one()),
            )]))),
            element_repr: Some(S::Record(vec![("x".into(), S::Float)])),
            elements: vec![],
        };
        assert!(validate_callback_result(&value, &proof, &result).is_ok());
        let wrong = W::Tensor {
            dimension: None,
            element: Some(Box::new(W::Record(vec![("x".into(), W::Unknown)]))),
            element_repr: Some(S::Record(vec![("x".into(), S::Float)])),
            elements: vec![],
        };
        assert!(validate_callback_result(&value, &wrong, &result).is_err());
    }

    #[test]
    fn refined_callback_extents_are_checked_for_values_and_empty_nested_schemas() {
        let inner = S::Tensor {
            rank: Some(1),
            shape: Some(vec![2]),
            elem: Box::new(S::Float),
        };
        let witness = W::Tensor {
            dimension: Some(aevia_dim::Dim::one()),
            element: Some(Box::new(W::Scalar(aevia_dim::Dim::one()))),
            element_repr: Some(S::Float),
            elements: vec![],
        };
        assert!(callback_schema_proves(&inner, &inner, &witness));
        let wrong = S::Tensor {
            rank: Some(1),
            shape: Some(vec![3]),
            elem: Box::new(S::Float),
        };
        assert!(!callback_schema_proves(&inner, &wrong, &witness));
        let rank_only = S::Tensor {
            rank: Some(1),
            shape: None,
            elem: Box::new(S::Float),
        };
        assert!(!callback_schema_proves(&inner, &rank_only, &witness));
        let payload = Val::Tensor {
            shape: vec![3],
            data: vec![Val::Num(Num::Float(0.0)); 3],
        };
        assert!(validate_callback_result(&payload, &witness, &inner).is_err());
    }
}
