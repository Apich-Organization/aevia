//! The live kernel session and the calls the runtime makes on it.
//!
//! There is one session per thread, created on first use. rssn's `Session` is
//! a `RefCell` and its `Term` borrows it, so sharing one across threads is not
//! possible; a thread-local keeps the model honest and makes `par_map` do the
//! right thing (each worker gets its own session) without the runtime having
//! to think about it.

use crate::error::{from_compute_error, from_op_error, KernelError};
use crate::value::{Num, Sym, Val};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use num_rational::BigRational;
use rssn::api::{Config, Session};
use rssn::kernel::call::{CallCtx, OpError};
use rssn::kernel::manifest::KernelManifest;
use rssn::kernel::result::Solutions;
use rssn::kernel::signature::{OpKey, Signature};
use rssn::kernel::value::{SessionId, TermRef, Value};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

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
        slot.as_ref().map(Rc::clone).unwrap_or_else(|| Rc::new(Kernel::new()))
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
    pub fn bind(mut self, name: &str, value: f64) -> Self {
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
    /// the kernel choose an overload, which is exactly the freedom PLAN D1
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
    pub fn call(&self, name: &str, args: &[Val]) -> Result<Val, KernelError> {
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
    pub fn call_opts(&self, name: &str, args: &[Val], opts: &Opts) -> Result<Val, KernelError> {
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

    /// Calls exactly the overload `key` names.
    ///
    /// This is the execution entry point of PLAN D1: the key was chosen
    /// statically (or once, by [`Kernel::resolve`]) and is executed as it
    /// stands. Nothing here looks for a better match, and no error path falls
    /// back to [`Kernel::call`].
    ///
    /// # Errors
    ///
    /// `E1003` when the key does not name a live overload, plus whatever the
    /// kernel reports; every error names the operator and keeps the kernel's
    /// own `OpError` in [`KernelError::cause`].
    pub fn call_key(&self, key: &OpKey, args: &[Val]) -> Result<Val, KernelError> {
        let registry = crate::registry::registry();
        if let Some(entry) = registry.op(&key.name) {
            registry.capabilities.require_op(entry)?;
        }
        let converted = self.convert(&key.name, args)?;
        let value = self
            .session
            .call_key(key, &converted, &CallCtx::new())
            .map_err(|error| from_op_error(&error).at_operator(&key.name))?;
        Val::from_rssn(&value, self.id)
    }

    /// Calls a higher-order operator with **no host callbacks**.
    ///
    /// §8.19's `Rssn.call_ho` names a `Tensor<1, Callback>`. This build cannot
    /// produce such a value: the kernel's
    /// [`HostCallback`](rssn::kernel::plugin::HostCallback) is `Send + Sync`,
    /// while Aevia's evaluator is a single-threaded `Rc` graph, so an Aevia
    /// closure cannot cross as one. `core.callback` is therefore off, and the
    /// runtime reports `E1001` naming it when a non-empty callback list
    /// reaches the boundary.
    ///
    /// With an empty list the kernel's `call_ho` *is* the ordinary dynamic
    /// call, so this method is the honest half that can be supported: it
    /// forwards to `Session::call_ho(name, args, &[], ctx)`. A caller can
    /// compare it with [`Kernel::call`] and must get the same answer; the test
    /// `the_callback_free_call_ho_is_the_dynamic_call` does exactly that.
    ///
    /// # Errors
    ///
    /// `E1001("core.dynamic_call")` when the build has no name-based ABI, plus
    /// whatever the kernel reports.
    pub fn call_ho(&self, name: &str, args: &[Val]) -> Result<Val, KernelError> {
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
            Val::Symbol(name) => Some(name.clone()),
            _ => None,
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
                Value::Tensor(tensor) => (tensor.data, true),
                Value::Sparse(sparse) => (sparse.data, true),
                other => (vec![other], false),
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
    pub fn overloads(&self, name: &str) -> Vec<(OpKey, Signature)> {
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
    pub fn resolve(&self, name: &str, args: &[Val]) -> Result<OpKey, KernelError> {
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
    pub fn compute(&self, sym: Sym, opts: &Opts) -> Result<Computed, KernelError> {
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
    pub fn build(&self, expr: &Expr) -> Result<Val, KernelError> {
        match expr {
            Expr::Num(number) => Ok(Val::Num(number.clone())),
            Expr::Sym(name) => Ok(Val::Symbol(name.clone())),
            Expr::Term(sym) => {
                // Check liveness now: a stale handle must fail here rather
                // than when the enclosing call finally runs.
                self.term(*sym)?;
                Ok(Val::Term(*sym))
            }
            Expr::App(name, args) => {
                // Build the node, do not call the operator. `call_value`
                // resolves an overload and checks argument types, which is
                // wrong here: Aevia's checker already decided the types, and a
                // symbolic argument is a term, not a `Real`. `Session::call`
                // is the node constructor, which is what §10.6.1 asks for.
                if let Some(entry) = crate::registry::registry().op(name) {
                    if !entry.accepts(args.len()) {
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
            }
        }
    }

    /// The term a value denotes, for use as an argument.
    fn as_term(&self, operator: &str, value: &Val) -> Result<rssn::api::Term<'_>, KernelError> {
        Ok(match value {
            Val::Num(Num::Int(number)) => match number.to_i64() {
                Some(value) => self.session.int(value),
                None => self.parse_term(&number.to_string())?,
            },
            Val::Num(Num::Rat(number)) => match (number.numer().to_i64(), number.denom().to_i64()) {
                (Some(numer), Some(denom)) => match self.session.rational(numer, denom) {
                    Some(term) => term,
                    None => self.parse_term(&format!("({numer})/({denom})"))?,
                },
                _ => self.parse_term(&format!("({})/({})", number.numer(), number.denom()))?,
            },
            Val::Num(Num::Float(number)) => self.session.float(*number),
            Val::Symbol(name) => self.session.sym(name),
            Val::Term(sym) => self.term(*sym)?,
            // A unit and a quantity are values of the kernel's term graph
            // (`Value::Unit`, `Value::Quantity`), so the session is the only
            // thing that can say what term they denote. Refusing them here
            // made `Rssn.quantity(3.0, Units.m)` — which type-checks — fail at
            // the first node that used it.
            other => {
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
            }
        })
    }

    /// Parses a term from text, for numbers that do not fit an `i64`.
    fn parse_term(&self, text: &str) -> Result<rssn::api::Term<'_>, KernelError> {
        self.session
            .parse(text)
            .map_err(|error| from_compute_error(&error))
    }

    /// A free symbol.
    #[must_use]
    pub fn symbol(&self, name: &str) -> Val {
        Val::Symbol(name.to_string())
    }

    /// The source form of a term.
    ///
    /// Lossy on purpose: this is the `show` path, where a stale handle must
    /// still render *something*. A caller that needs to know whether the
    /// handle is live uses [`Self::try_text`] or [`Self::require_live`].
    #[must_use]
    pub fn text(&self, sym: Sym) -> String {
        match self.term(sym) {
            Ok(term) => term.to_string(),
            Err(_) => "<stale term>".to_string(),
        }
    }

    /// The source form of a term, or why it cannot be printed.
    ///
    /// # Errors
    ///
    /// `E0899` when the handle does not name a live node of this session.
    pub fn try_text(&self, sym: Sym) -> Result<String, KernelError> {
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
    pub fn require_live(&self, sym: Sym) -> Result<(), KernelError> {
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
    pub fn diff(&self, sym: Sym, variable: &str) -> Result<Val, KernelError> {
        self.require_live(sym)
            .map_err(|_| KernelError::foreign(sym, "diff"))?;
        self.call("diff", &[Val::Term(sym), Val::Symbol(variable.to_string())])
    }

    /// The free symbols of a term, or why they cannot be read.
    ///
    /// # Errors
    ///
    /// `E0899` when the handle is not live.
    pub fn try_free_symbols(&self, sym: Sym) -> Result<Vec<String>, KernelError> {
        Ok(self.term(sym)?.free_symbols())
    }

    /// The free symbols of a term.
    ///
    /// Lossy on purpose and only for the show path: a stale handle answers the
    /// empty list, which reads as "no free symbols" — that is, as a closed
    /// form. A caller that draws a conclusion from the emptiness (evaluating a
    /// term, deciding it is constant) must use [`Self::try_free_symbols`].
    #[must_use]
    pub fn free_symbols(&self, sym: Sym) -> Vec<String> {
        self.try_free_symbols(sym).unwrap_or_default()
    }

    /// Evaluates a term numerically.
    ///
    /// `None` means "no numeric value": a stale handle, a free symbol without
    /// a binding, or a result the evaluator cannot reach. The failure is in
    /// the `Option`, never in a substituted value.
    #[must_use]
    pub fn eval(&self, sym: Sym, bindings: &[(String, f64)]) -> Option<f64> {
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
    pub fn substitute(&self, sym: Sym, from: Sym, to: &Val) -> Result<Val, KernelError> {
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
    pub fn term_of(&self, val: &Val) -> Result<Sym, KernelError> {
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
    pub fn value_of(&self, sym: Sym) -> Result<Val, KernelError> {
        let term = self.term(sym)?;
        let value = self
            .session
            .value_of(term)
            .map_err(|error| from_op_error(&error).at_operator("value_of"))?;
        Val::from_rssn(&value, self.id)
    }

    /// The term a handle names.
    fn term(&self, sym: Sym) -> Result<rssn::api::Term<'_>, KernelError> {
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
            .map_err(|error| match error {
                OpError::Internal(_) => KernelError::foreign(sym, "term"),
                other => from_op_error(&other),
            })
    }

    fn convert(&self, operator: &str, args: &[Val]) -> Result<Vec<Value>, KernelError> {
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
    fn one(&self, operator: &str, arg: &Val) -> Result<Value, KernelError> {
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
pub fn rational(numer: i64, denom: i64) -> Num {
    Num::Rat(BigRational::new(BigInt::from(numer), BigInt::from(denom)))
}
