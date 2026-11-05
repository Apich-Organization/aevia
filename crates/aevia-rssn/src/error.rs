//! Kernel failures, mapped onto the diagnostic codes of appendix B.
//!
//! Every rssn error variant has exactly one Aevia code here. Keeping the
//! mapping in one place is what lets the checker, the REPL and the language
//! server report the same thing for the same failure.

use crate::value::Sym;
use rssn::kernel::call::OpError;

/// What kind of failure this is.
///
/// The code says what to print; the kind says what the caller may *do*. A
/// runtime that has to choose between "the kernel could not answer" and
/// "nothing matched" cannot read that off a message, and the difference is
/// exactly the one that turns a failed `diff` into a plausible `0`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Nothing is registered under that name.
    Unknown,
    /// Several overloads match equally well.
    Ambiguous,
    /// The arguments do not fit any overload.
    Type,
    /// A symbolic handle that outlived its session, or belongs to another.
    Stale,
    /// The build lacks a capability, not the program a type.
    Capability,
    /// The kernel itself failed: not the program, not a stale handle.
    ///
    /// This is the one kind the runtime must surface as `E0899` rather than
    /// retrying, defaulting or re-resolving: the value that would come back
    /// from a second attempt is not the value the static type promised.
    Kernel,
    /// A failure of the computation itself.
    Other,
}

/// A failure that crossed the bridge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KernelError {
    /// The diagnostic code (`E0401`, `E1003`, ...).
    pub code: &'static str,
    /// What went wrong.
    pub message: String,
    /// What to do about it.
    pub help: Option<String>,
    /// How the caller should treat it.
    pub kind: Failure,
    /// The operator whose call failed, when the failure came from a call.
    ///
    /// A diagnostic that cannot say *which* `Rssn.<op>` failed is not
    /// actionable in a program that calls a thousand of them.
    pub operator: Option<String>,
    /// The kernel's own failure, kept whole.
    ///
    /// `code` is the diagnostic the user sees; this is the machine-readable
    /// reason, and it is what makes "ambiguous" distinguishable from "the
    /// kernel gave up" without matching on message text. Keeping the variant
    /// instead of a rendering is deliberate: `OpError` is `Clone + Eq`, and a
    /// caller that wants to retry a `BudgetExceeded` must not have to parse
    /// a sentence to find out.
    ///
    /// Boxed because `OpError` is the largest thing a diagnostic can carry and
    /// a `KernelError` is returned by value from every call.
    pub cause: Option<Box<OpError>>,
}

impl KernelError {
    /// A failure with a code and a message.
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            help: None,
            kind: Failure::Other,
            operator: None,
            cause: None,
        }
    }

    /// A failure with a code, a message and a kind.
    #[must_use]
    pub fn of(code: &'static str, kind: Failure, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            help: None,
            kind,
            operator: None,
            cause: None,
        }
    }

    /// Adds the operator whose call failed.
    #[must_use]
    pub fn at_operator(mut self, operator: &str) -> Self {
        self.operator = Some(operator.to_string());
        self
    }

    /// Records the kernel's own failure.
    ///
    /// An `Internal`, `Host` or `Codec` failure is the kernel's fault, not the
    /// program's, so it becomes `E0899` with [`Failure::Kernel`] whatever code
    /// the message was given: the runtime has to stop, not retry.
    #[must_use]
    pub fn caused_by(mut self, error: &OpError) -> Self {
        if matches!(
            error,
            OpError::Internal(_) | OpError::Host(_) | OpError::Codec(_)
        ) {
            self.code = "E0899";
            self.kind = Failure::Kernel;
        }
        self.cause = Some(Box::new(error.clone()));
        self
    }

    /// The name of the `OpError` variant, for callers that must not name an
    /// rssn type.
    #[must_use]
    pub fn variant(&self) -> Option<&'static str> {
        self.cause.as_deref().map(|error| match error {
            OpError::UnknownOp(_) => "UnknownOp",
            OpError::Arity { .. } => "Arity",
            OpError::Type { .. } => "Type",
            OpError::Ambiguous { .. } => "Ambiguous",
            OpError::Dimension(_) => "Dimension",
            OpError::NotNumeric(_) => "NotNumeric",
            OpError::NotReduced(_) => "NotReduced",
            OpError::Domain(_) => "Domain",
            OpError::BudgetExceeded => "BudgetExceeded",
            OpError::Unsupported { .. } => "Unsupported",
            OpError::Plugin(_) => "Plugin",
            OpError::ForeignTerm { .. } => "ForeignTerm",
            OpError::Codec(_) => "Codec",
            OpError::Host(_) => "Host",
            OpError::Internal(_) => "Internal",
        })
    }

    /// Adds a suggestion.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// A handle that outlived its session.
    #[must_use]
    pub fn stale_term(sym: Sym, session: u64) -> Self {
        Self::of(
            "E0899",
            Failure::Stale,
            format!(
                "a symbolic value from session {} was used in session {session}",
                sym.session
            ),
        )
        .with_help("recompute the expression after resetting the kernel")
    }

    /// A handle the kernel no longer has a node for.
    ///
    /// Distinct from [`Self::stale_term`]: the session is the live one, but the
    /// node is gone (a `reset`, a `fork` that was never merged, a handle from a
    /// session that has since been replaced).
    #[must_use]
    pub fn foreign(sym: Sym, operation: &str) -> Self {
        Self::of(
            "E0899",
            Failure::Stale,
            format!(
                "`{operation}` needs the symbolic value at node {} of session {}, which the kernel no longer holds",
                sym.node, sym.session
            ),
        )
        .with_help("recompute the expression; a symbolic value does not survive `:reset`")
        .at_operator(operation)
    }

    /// A capability the build does not provide.
    ///
    /// The message names the capability rather than the feature that would
    /// provide it: a capability can be missing because the kernel was built
    /// without a feature *or* because the host bridge was not written yet, and
    /// only the named capability tells a user which.
    #[must_use]
    pub fn capability(name: &str) -> Self {
        Self::of(
            "E1001",
            Failure::Capability,
            format!("this build does not provide `{name}`"),
        )
        .with_help("this is a gap in the build, not in the program")
    }
}

impl std::fmt::Display for KernelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// Maps an operator failure onto a diagnostic.
///
/// Every variant keeps its identity: `Ambiguous` stays `E1002` and is never
/// rewritten as "the operator failed", because that is the one signal the
/// static and the dynamic choice disagree about. The variant itself is kept in
/// [`KernelError::cause`], so the runtime can act on it without reading the
/// message.
#[must_use]
pub fn from_op_error(error: &OpError) -> KernelError {
    classify(error).caused_by(error)
}

fn classify(error: &OpError) -> KernelError {
    match error {
        OpError::UnknownOp(key) => KernelError::of(
            "E1003",
            Failure::Unknown,
            format!("rssn has no operator `{}`", key.qualified()),
        )
        .with_help("check the name against `Rssn.operators`, or use `Rssn.dynamic`"),
        OpError::Arity { expected, got } => KernelError::of(
            "E1003",
            Failure::Type,
            format!("operator takes {expected:?} arguments, got {got}"),
        ),
        OpError::Type {
            expected,
            got,
            arg,
        } => KernelError::of(
            "E0302",
            Failure::Type,
            format!("argument {arg} is {got:?}, expected {expected:?}"),
        ),
        OpError::Ambiguous { name, count } => KernelError::of(
            "E1002",
            Failure::Ambiguous,
            format!("{count} overloads of `{name}` match equally well"),
        )
        .with_help("annotate the arguments so one overload is strictly more specific"),
        OpError::Dimension(dimension) => KernelError::of(
            "E0401",
            Failure::Type,
            format!("dimension error: {dimension}"),
        ),
        OpError::NotNumeric(what) => KernelError::of(
            "E0803",
            Failure::Other,
            format!("`{what}` has no numeric value"),
        ),
        OpError::NotReduced(what) => KernelError::of(
            "E0807",
            Failure::Other,
            format!("`{what}` did not reduce to a closed form"),
        )
        .with_help("try `nsolve` for a numeric root, or `approx` for a value"),
        OpError::Domain(what) => {
            KernelError::of("E0806", Failure::Other, format!("out of domain: {what}"))
        }
        OpError::BudgetExceeded => KernelError::of(
            "E0808",
            Failure::Other,
            "the rewrite budget ran out before a closed form was found",
        )
        .with_help("raise the budget with `Rssn.budget(n)`"),
        // A backend that cannot execute an operator is a capability gap, not a
        // wrong program: the code names it so a user can tell the two apart.
        OpError::Unsupported { backend, feature } => KernelError::of(
            "E1001",
            Failure::Capability,
            format!("the `{backend}` backend does not support `{feature}`"),
        )
        .with_help("this is a gap in the build, not in the program"),
        OpError::Plugin(error) => KernelError::of(
            "E1006",
            Failure::Capability,
            format!("plugin failure: {error:?}"),
        ),
        OpError::ForeignTerm { expected, got } => KernelError::of(
            "E0899",
            Failure::Stale,
            format!(
                "a term handle from session {} reached session {}",
                expected.0, got.0
            ),
        ),
        OpError::Codec(error) => KernelError::of(
            "E0899",
            Failure::Other,
            format!("codec failure: {error:?}"),
        ),
        OpError::Host(message) => KernelError::of(
            "E0899",
            Failure::Other,
            format!("host callback: {message}"),
        ),
        OpError::Internal(message) => {
            KernelError::of("E0899", Failure::Kernel, format!("rssn: {message}"))
        }
    }
}

/// Maps a whole-request failure onto a diagnostic.
#[must_use]
pub fn from_compute_error(error: &rssn::api::ComputeError) -> KernelError {
    use rssn::api::ComputeError;
    match error {
        ComputeError::Dimension(dimension) => KernelError::of(
            "E0401",
            Failure::Type,
            format!("dimension error: {dimension}"),
        ),
        ComputeError::NotNumeric(what) => KernelError::of(
            "E0803",
            Failure::Other,
            format!("`{what}` has no numeric value"),
        ),
        ComputeError::Inconsistent(what) => KernelError::of(
            "E0809",
            Failure::Other,
            format!("the kernel produced an inconsistent result: {what}"),
        ),
        ComputeError::Rule(message) => KernelError::of(
            "E0899",
            Failure::Other,
            format!("rule failure: {message}"),
        ),
        ComputeError::UnknownOperator(name) => KernelError::of(
            "E1003",
            Failure::Unknown,
            format!("rssn has no operator `{name}`"),
        ),
        other => KernelError::of("E0899", Failure::Other, format!("rssn: {other}")),
    }
}
