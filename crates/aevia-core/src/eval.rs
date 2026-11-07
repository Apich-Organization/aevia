//! The interpreter: values in, values out.
//!
//! Evaluation is strict and left-to-right (§6.1). The AST evaluator is the
//! semantic reference for the Core IR path. Closures retain lexical evidence;
//! `do` builds IO actions and pattern matching is first-match. Recursive AST
//! paths enter small dispatch functions before the residual expression and
//! constructor dispatchers, so the language's call-depth guard is reached
//! before the CLI worker exhausts its native stack.

use crate::value::Closure;
use crate::value::ClosureBody;
use crate::value::ClosureEnv;
use crate::value::DynValue;
use crate::value::IoAction;
use crate::value::NativeFn;
use crate::value::NumKind;
use crate::value::NumVal;
use crate::value::PropValue;
use crate::value::RecordValue;
use crate::value::Scope;
use crate::value::Storage;
use crate::value::TensorData;
use crate::value::Value;
use crate::value::ctor_name;
use crate::value::format_scalar;
use crate::value::make_variant;
use crate::value::{
    self,
};
use aevia_dim::Dim;
use aevia_dim::DimEnv;
use aevia_dim::UnitDef;
use aevia_dim::UnitTable;
use aevia_syntax::Diagnostic;
use aevia_syntax::Span;
use aevia_syntax::ast::*;
use aevia_types::adt;
use aevia_types::check::BuiltinDispatch;
use aevia_types::check::CheckedModule;
use aevia_types::check::DictTarget;
use aevia_types::check::DictionaryUse;
use aevia_types::check::Export;
use aevia_types::check::InstanceExport;
use aevia_types::check::ModuleInterface;
use aevia_types::class::ClassTable;
use aevia_types::class::Resolution;
use aevia_types::keys::ClassKey;
use aevia_types::keys::InstanceKey;
use aevia_types::keys::RepKind;
use aevia_types::keys::StorageClass;
use aevia_types::keys::Witness;
use aevia_types::keys::{
    self,
};
use aevia_types::ty::Rep;
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;
use num_traits::Signed;
use num_traits::ToPrimitive;
use num_traits::Zero;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// One entry of a call stack (§9.3.4).
#[derive(Clone, Debug)]
pub struct Frame {
    /// The function that was called.
    ///
    /// `Rc<str>` rather than `String`: a frame is pushed on every call, and a
    /// per-call `String` clone was one allocation per call (round-2 performance
    /// work). The name is interned once, when the function is lowered.
    pub function: Rc<str>,
    /// Where it was called.
    pub span: Span,
}

/// A slice index whose expression has already been evaluated.
///
/// `None` means the expression produced no number. The two indexing branches
/// treat that differently on purpose — `E0802` when every index is scalar, `0`
/// inside a slice — so the resolved form keeps the distinction instead of
/// inventing a value.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SliceValue {
    /// `a[i]`.
    Index(Option<f64>),
    /// `a[i..j]`, `a[i..]`, `a[..j]`, `a[..]`, `a[i:j:s]`.
    Range {
        /// The start.
        start: Option<f64>,
        /// The end.
        end: Option<f64>,
        /// Whether the range is inclusive.
        inclusive: bool,
        /// The step, when written. `None` means one; `NaN` means the
        /// expression was not a number.
        step: Option<f64>,
    },
}

/// One `instance` method body, as the runtime sees it.
///
/// The body is bound under `InstanceKey::mangle() :: method`, the name the
/// checker also derives from the symbolic key, so two instances with primitive
/// heads (`Pretty@Int` and `Pretty@String`) cannot overwrite one global
/// (§3.8.1, round-2 decision 1a).
#[derive(Clone, Debug)]
pub(crate) struct InstanceEntry {
    /// The symbolic identity the checker proved. `None` only for an entry the
    /// runtime built without checker data (the legacy fallback).
    pub(crate) key: Option<aevia_types::keys::InstanceKey>,
    /// The checked head, including field types and repeated variables.
    pub(crate) checked_head: Option<aevia_types::Type>,
    /// The head as a predicate over a runtime value.
    pub(crate) shape: HeadShape,
    /// The mangled global the body is bound to.
    pub(crate) global: String,
}

/// The runtime shape an instance head accepts.
///
/// Heads are disjoint (§3.8.3), so at most one entry of a class matches any
/// value; the shape is only ever used to *select* that one, never to widen a
/// head.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HeadShape {
    /// A nominal type: the value is a variant of that type.
    Nominal(String),
    /// `Tensor<…>`: any tensor.
    Tensor,
    /// A record with exactly these labels, in any order.
    Record(Vec<String>),
    /// A scalar representation, `Int` / `Real` / `Float` / `Complex`.
    Scalar(&'static str),
    /// `Bool`.
    Bool,
    /// `String`.
    Str,
    /// `()`.
    Unit,
    /// `Prop`.
    Prop,
    /// A head the runtime cannot recognise: `dyn`-typed, a type variable, a
    /// function type, or an alias the checker expanded.
    Unknown,
}

impl HeadShape {
    /// Whether a value of this shape is an instance of the head.
    fn accepts(
        &self,
        value: &Value,
    ) -> bool {
        match (self, value) {
            | (Self::Nominal(name), Value::Variant(variant)) => adt::name_of(variant.adt) == *name,
            | (Self::Tensor, Value::Tensor(_)) => true,
            | (Self::Record(labels), Value::Record(record)) => {
                record.fields.len() == labels.len()
                    && labels.iter().all(|label| record.fields.contains_key(label))
            },
            | (Self::Scalar(rep), Value::Num(num)) => {
                match (rep, &num.kind) {
                    | (&"Int", NumKind::Int(_)) => true,
                    | (&"Real", NumKind::Exact(_) | NumKind::Sym(_)) => true,
                    | (&"Float", NumKind::Float(_)) => true,
                    | (&"Complex", NumKind::Complex(..)) => true,
                    | _ => false,
                }
            },
            | (Self::Bool, Value::Bool(_))
            | (Self::Str, Value::Str(_))
            | (Self::Unit, Value::Unit)
            | (Self::Prop, Value::Prop(_)) => true,
            | _ => false,
        }
    }
}

/// What a class-method call site runs (§3.8.1).
pub(crate) enum Dispatch {
    /// The builtin of the method name: a prelude instance (§3.8.4).
    Builtin,
    /// A user instance's body.
    Instance(Value),
}

/// The shape of an `instance` head written in the source.
///
/// A nominal type is spelled the same way in both worlds, so its name is a
/// usable predicate. A record alias is not: `Vec2` converts to a record type,
/// and only the checker knows which labels it has, so an alias head stays
/// [`HeadShape::Unknown`] and is found by the recorded head or the
/// single-instance rule.
pub(crate) fn head_shape(
    ty: &Ty,
    aliases: &HashMap<String, aevia_types::ty::Type>,
) -> HeadShape {
    match &ty.kind {
        | TyKind::Named { path, .. } => {
            path.last().cloned().map_or(HeadShape::Unknown, |name| {
                if name.chars().next().is_some_and(char::is_uppercase) {
                    // An alias is transparent: `type Vec2 = { x, y }` names a
                    // record, and the value at run time is a record, not a variant.
                    aliases
                        .get(&name)
                        .map_or(HeadShape::Nominal(name), |aliased| type_head_shape(aliased))
                } else {
                    HeadShape::Unknown
                }
            })
        },
        | TyKind::Tensor { .. } | TyKind::Matrix { .. } | TyKind::Sparse { .. } => {
            HeadShape::Tensor
        },
        | TyKind::Record { fields, .. } => {
            let mut labels: Vec<String> = fields.iter().map(|(name, _)| name.clone()).collect();
            labels.sort();
            HeadShape::Record(labels)
        },
        | TyKind::Int => HeadShape::Scalar("Int"),
        | TyKind::Real(_) => HeadShape::Scalar("Real"),
        | TyKind::Float(_) => HeadShape::Scalar("Float"),
        | TyKind::Complex(_) => HeadShape::Scalar("Complex"),
        | TyKind::Bool => HeadShape::Bool,
        | TyKind::Str => HeadShape::Str,
        | TyKind::Unit => HeadShape::Unit,
        | TyKind::Prop => HeadShape::Prop,
        | _ => HeadShape::Unknown,
    }
}

/// The runtime shape of a *checked* type, used to see through an alias.
fn type_head_shape(ty: &aevia_types::ty::Type) -> HeadShape {
    use aevia_types::ty::Type;
    match ty {
        | Type::Adt(id, _) => HeadShape::Nominal(adt::name_of(*id)),
        | Type::Record(row) => {
            let mut labels = Vec::new();
            let mut current = row;
            while let aevia_types::ty::Row::Extend { label, tail, .. } = current {
                labels.push(label.clone());
                current = tail;
            }
            labels.sort();
            HeadShape::Record(labels)
        },
        | Type::Tensor { .. } => HeadShape::Tensor,
        | Type::Scalar(rep, _) => HeadShape::Scalar(rep.name()),
        | Type::Bool => HeadShape::Bool,
        | Type::Str => HeadShape::Str,
        | Type::Unit => HeadShape::Unit,
        | _ => HeadShape::Unknown,
    }
}

/// A runtime error, carrying the diagnostic code of appendix B.
#[derive(Clone, Debug)]
pub struct EvalError {
    /// The stable code, for example `E0802`.
    pub code: &'static str,
    /// The message.
    pub message: String,
    /// Where it happened, when known.
    pub span: Option<Span>,
    /// Extra context.
    pub notes: Vec<String>,
    /// The call stack, outermost first, when the failure happened in a call.
    pub stack: Vec<Frame>,
    /// For `?`: the `Err`/`None` value the enclosing function must return.
    /// Internal — it never reaches a user diagnostic.
    pub payload: Option<Value>,
    /// Private source control-flow destination. A regular error payload never
    /// becomes a successful function return.
    pub(crate) try_target: Option<aevia_types::check::TryBoundary>,
}

impl EvalError {
    /// Builds an error.
    #[must_use]
    pub fn new(
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            span: None,
            notes: Vec::new(),
            stack: Vec::new(),
            payload: None,
            try_target: None,
        }
    }

    /// Attaches a span.
    #[must_use]
    pub fn at(
        mut self,
        span: Span,
    ) -> Self {
        self.span = Some(span);
        self
    }

    /// Attaches a call stack, when the error does not have one yet.
    #[must_use]
    pub fn with_stack(
        mut self,
        stack: Vec<Frame>,
    ) -> Self {
        if self.stack.is_empty() {
            self.stack = stack;
        }
        self
    }

    /// The call stack rendered as notes, outermost call first.
    #[must_use]
    pub fn trace(&self) -> Vec<String> {
        // A deep recursion is thousands of identical frames; print a run of
        // the same function once with its extent instead of one line each.
        let mut notes = Vec::new();
        let mut depth = 0;
        while depth < self.stack.len() {
            let name = self.stack[depth].function.as_ref();
            let run = self.stack[depth..]
                .iter()
                .take_while(|frame| frame.function.as_ref() == name)
                .count();
            if run > 3 {
                notes.push(format!("{} in `{}`", depth + 1, name));
                notes.push(format!(
                    "{}..{} in `{}` ({} more recursive frames)",
                    depth + 2,
                    depth + run,
                    name,
                    run - 1
                ));
            } else {
                for offset in 0..run {
                    notes.push(format!("{} in `{}`", depth + offset + 1, name));
                }
            }
            depth += run;
        }
        notes
    }

    /// Converts to a compiler diagnostic.
    #[must_use]
    pub fn diagnostic(&self) -> Diagnostic {
        let span = self.span.unwrap_or_default();
        let mut diagnostic = Diagnostic::error(self.code, self.message.clone(), span);
        for note in &self.notes {
            diagnostic = diagnostic.with_note(note.clone());
        }
        for note in self.trace() {
            diagnostic = diagnostic.with_note(note);
        }
        diagnostic
    }
}

/// Where the interpreter's `print` output goes.
#[derive(Clone)]
pub enum Output {
    /// Write straight to the process streams.
    Console,
    /// Collect into a buffer (tests, notebooks).
    Buffer(Rc<RefCell<String>>),
}

/// Which evaluator runs a program.
///
/// The AST path stays selectable: it is the semantic reference, and a feature
/// that cannot be made to agree on both paths stays on it (§9.2.2, PLAN D2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Walk the typed AST.
    Ast,
    /// Walk the Core IR.
    Ir,
}

/// `E0802` naming the first coordinate outside the shape.
///
/// "index out of range" alone sent the reader looking for which of several
/// indices, and by how much; the static check already says both.
fn index_out_of_range(
    coordinates: &[isize],
    shape: &[usize],
) -> EvalError {
    for (axis, (coordinate, extent)) in coordinates.iter().zip(shape).enumerate() {
        let extent = *extent as isize;
        if *coordinate >= extent || *coordinate < -extent {
            let axis_text = if shape.len() == 1 {
                String::new()
            } else {
                format!(" on axis {axis}")
            };
            return EvalError::new(
                "E0802",
                format!(
                    "index {coordinate} is out of bounds{axis_text} for extent {extent} (valid: {} to {})",
                    -extent,
                    extent - 1
                ),
            );
        }
    }
    EvalError::new("E0802", "index out of range")
}

thread_local! {
    /// `(base address, usable bytes)` of the current thread's stack, when the
    /// host declared one with [`install_stack_budget`].
    static STACK_BUDGET: std::cell::Cell<Option<(usize, usize)>> =
        const { std::cell::Cell::new(None) };
}

/// Declares that the calling thread has `total` bytes of stack.
///
/// Call it near the top of a thread created with that stack size. From then
/// on non-tail recursion in that thread is bounded by the stack actually
/// left rather than only by [`Interp::max_depth`]: an interpreter call's
/// native frame differs by an order of magnitude between debug and release
/// builds, so a fixed call count is either far too small or unsafe. A margin
/// (1/8 of the stack, at least 8 MiB, at most 64 MiB) is kept for builtins
/// and kernel calls made from the deepest frame.
pub fn install_stack_budget(total: usize) {
    let marker = 0u8;
    let base = std::ptr::addr_of!(marker) as usize;
    let margin = (total / 8).clamp(8 << 20, 64 << 20);
    STACK_BUDGET.with(|cell| cell.set(Some((base, total.saturating_sub(margin)))));
}

/// The declared budget, when the current stack position is past it.
#[inline]
fn stack_budget_exhausted() -> Option<usize> {
    let (base, usable) = STACK_BUDGET.with(std::cell::Cell::get)?;
    let marker = 0u8;
    let here = std::ptr::addr_of!(marker) as usize;
    (base.abs_diff(here) > usable).then_some(usable)
}

/// The interpreter state for one program.
pub struct Interp {
    pub(crate) tensor_inputs: crate::generics::TensorInputCache,
    pub(crate) call_proofs: crate::generics::CallProofCache,
    /// Literal values per (site, bindings, module); see
    /// [`Interp::memoized_literal`].
    pub(crate) literal_values: std::cell::RefCell<
        crate::fxhash::FxHashMap<
            (Span, usize, usize),
            (
                Value,
                Option<Rc<crate::generics::CallBindings>>,
                Rc<crate::evidence::ModuleContext>,
            ),
        >,
    >,
    /// Constructor type witnesses per (site, bindings, module); the entry
    /// pins the bindings and module whose addresses its key names.
    pub(crate) constructor_witnesses: std::cell::RefCell<
        crate::fxhash::FxHashMap<
            (Span, usize, usize),
            (
                Option<aevia_types::Type>,
                Option<Rc<crate::generics::CallBindings>>,
                Rc<crate::evidence::ModuleContext>,
            ),
        >,
    >,
    /// Mangled instance globals by call site; see `cached_instance_global`.
    pub(crate) instance_globals:
        RefCell<crate::evidence::SpanMap<Vec<(aevia_types::InstanceKey, String, Rc<str>)>>>,
    /// Spare frame and argument buffers; see `eval_ir::run_closure`.
    pub(crate) buffer_pool: RefCell<Vec<Vec<Value>>>,
    pub(crate) session: Rc<()>,
    pub(crate) context: Rc<crate::evidence::ModuleContext>,
    pub(crate) active_scope: Option<Rc<Scope>>,
    pub(crate) module_scopes: HashMap<keys::ModulePath, Rc<Scope>>,
    pub(crate) bindings: Option<Rc<crate::generics::CallBindings>>,
    /// Top-level bindings.
    pub globals: HashMap<String, Value>,
    /// The unit registry.
    pub units: UnitTable,
    /// The dimension environment.
    pub dims: DimEnv,
    /// The module's export table.
    pub exports: IndexMap<String, Export>,
    /// Constructor metadata, for pattern matching and `show`.
    pub adts: HashMap<adt::AdtId, adt::Adt>,
    /// Where output goes.
    pub output: Output,
    /// How many times each symbol name has been created.
    pub symbol_counts: HashMap<String, u32>,
    /// Deterministic RNG state.
    pub seed: u64,
    /// The runtime capabilities `IO` actions may use (§9.14), as
    /// [`crate::value::caps`] bits. Every capability is granted unless the
    /// host (`aevia run --caps`) or a `with_cap` block narrows the set.
    pub capabilities: u8,
    /// Maximum call depth before `E0805`.
    pub max_depth: usize,
    /// Maximum consecutive tail calls in one function activation before
    /// `E0805`.
    ///
    /// A tail call does not consume stack (§9.3.2), so the depth guard cannot
    /// catch a tail-recursive loop that never terminates. This budget does: it
    /// is the same "a legal program terminates, a runaway one is `E0805`"
    /// guarantee, expressed in the quantity a trampoline actually consumes.
    ///
    /// It is a runaway guard, **not** a semantic limit: a program that makes
    /// more tail calls than this is rejected even though it would terminate, so
    /// the default is set far above anything a real program does rather than
    /// just above the test suite's longest loop (a 1.5 M default rejected a
    /// legal 1.6 M iteration count). A caller that wants a tighter bound — the
    /// CLI's `--budget` — sets it explicitly.
    pub max_tail_calls: usize,
    /// Maximum scheduler steps in one IO execution (including deferred source
    /// steps). Exhaustion is E0805 before the next effect runs.
    pub max_io_steps: usize,
    /// Maximum pending IO continuations. Flat and tail IO programs use constant
    /// space; recursive non-tail effect chains are stopped before heap exhaustion.
    pub max_io_depth: usize,
    /// Maximum expression nesting before `E0805`.
    ///
    /// This is a safety net for the recursive walk, not the language's limit:
    /// a flat chain of `+` is a legal program (§9.3.2), and the parser's own
    /// nesting limit is far below this.
    pub max_nodes: usize,
    /// Which evaluator runs: the typed AST, or the Core IR.
    pub mode: Mode,
    /// Calls in progress, the quantity `max_depth` bounds.
    pub(crate) depth: usize,
    /// Expression nodes in progress, the quantity `max_nodes` bounds.
    pub(crate) nodes: usize,
    /// The call stack, outermost first.
    pub(crate) frames: Vec<Frame>,
    /// The interned name a nameless closure reports in a stack frame.
    pub(crate) lambda_name: Rc<str>,
    /// Whether the kernel constants (`pi`, `oo`, `e`, `nan`, `i`) have been
    /// seeded.
    ///
    /// Seeding asks the kernel for symbolic values and dominates an empty
    /// program's cold start, so it is deferred until the program actually names
    /// one of them (§9.13 startup budget). A program that never mentions `pi`
    /// never pays for it; one that does pays on its first lookup, not knowing
    /// the difference.
    pub(crate) constants_seeded: bool,
    /// The program arguments `args()` reports (§9.12), set by the host.
    pub program_args: Vec<String>,
    /// The overload the checker pinned for each `Rssn.<op>` call site, keyed
    /// by the call's span (PLAN D1).
    ///
    /// The runtime executes this key and never resolves again: a second
    /// resolution by argument values could pick a different overload than the
    /// one the program was typed against.
    pub rssn_calls: HashMap<Span, aevia_rssn::OpKey>,
    /// The instance the checker proved for each class-method call site, keyed
    /// by the call's span (§3.8.1, PLAN D3).
    ///
    /// A call the checker resolved this way runs the instance's body; the
    /// global of the same name is only the fallback for a prelude instance.
    pub dictionaries: HashMap<Span, Vec<DictionaryUse>>,
    /// The checker's `Rssn.<op>` records, kept whole so the lowering can carry
    /// the pinned overload without the runtime re-resolving it.
    pub rssn_call_sites: Vec<aevia_types::check::RssnCall>,
    /// The checker's class-method records, kept whole for the same reason.
    pub dictionary_sites: Vec<DictionaryUse>,
    /// The concrete dimension the checker materialised for each `dyn`/`check`
    /// call site, keyed by span (§4.9.4).
    ///
    /// `dyn`/`check` are type-directed: the static `d` never appears in the
    /// argument list, so the checker records it here and `check` compares it
    /// with the value's carried dimension at that span. Without the lookup a
    /// `check` can never fail and `E0412` is unreachable.
    pub dyn_calls: HashMap<Span, Dim>,
    /// Host registrations (`@op`/`@rule`) the checker accepted (§13.5.2).
    ///
    /// Publishing them to the kernel is a bridge step this build does not
    /// implement; the runtime reports the named `core.plugin.source` gap
    /// instead of silently dropping them.
    pub registrations: Vec<aevia_types::check::HostRegistration>,
    /// The representation the checker instantiated for every numeric literal,
    /// keyed by the literal's span (§3.10.1, §6.3.2).
    ///
    /// §3.10.1 lets a literal take the representation of its context; without
    /// this table the runtime guessed (`1` stayed an `Int` where the checker
    /// had a `Float`), which is the measured static/runtime disagreement. The
    /// IR lowering reads the same table, so both evaluators agree.
    pub literal_reps: HashMap<Span, Rep>,
    /// A literal or unit value uses the defining source file's unit namespace,
    /// even when called after another package installed a same-named unit.
    pub(crate) module_units: HashMap<aevia_syntax::FileId, UnitTable>,
    /// Resolved source-expression types, including imported modules.
    pub(crate) expr_types: HashMap<Span, aevia_types::Type>,
    /// The non-derived `instance` exports of the module being loaded, in
    /// declaration order. The source `instance` declarations are matched to
    /// them so a body can be bound under the same `InstanceKey::mangle()` name
    /// the checker derived (round-2 decision 1a).
    pub instance_exports: Vec<InstanceExport>,
    /// All checked local instance bodies, including private classes, by site.
    pub(crate) instance_sites: HashMap<Span, InstanceExport>,
    /// Nominal identities of methods used as first-class values.
    pub(crate) method_refs: HashMap<Span, aevia_types::check::ClassMethodRef>,
    /// How many `instance` declarations of the current module have been bound.
    pub(crate) next_instance_export: usize,
    /// `ClassKey` by source name, for user classes declared in a loaded module
    /// (a class's defining module is part of its identity, §3.8.1).
    pub(crate) class_keys: HashMap<String, ClassKey>,
    /// The module currently being loaded, for constructor identity (§3.6.2).
    ///
    /// User types are nominal per module (D6), so `A.Circle` and `B.Circle` are
    /// different constructors; name-only lookup would pick whichever module was
    /// checked first.
    pub current_module: keys::ModulePath,
    /// Constructors pinned by this exact check, including private declarations.
    pub(crate) current_constructors: indexmap::IndexMap<String, (adt::AdtId, usize)>,
    /// Instance bodies, by `Class::method`, in declaration order.
    pub(crate) instance_methods: HashMap<String, Vec<InstanceEntry>>,
    /// The class that declares each instance method, so a method used as a
    /// value can still be dispatched on its argument (§3.8.1).
    pub(crate) method_classes: HashMap<String, String>,
    /// The prelude's instance table, so a call the checker resolved to a
    /// *builtin* instance is not mistaken for a user one (§3.8.4).
    prelude_classes: ClassTable,
    /// Transparent type aliases, by name.
    ///
    /// A record alias has no nominal tag at run time, so an instance head
    /// spelled `Vec2` and the checker's record `{ x: Float, y: Float }` are the
    /// same head written two ways; without the definition the instance is
    /// unreachable.
    pub(crate) aliases: HashMap<String, aevia_types::ty::Type>,
}

/// The `(value, uncertainty)` pair of a §8.14 measurement record.
///
/// The runtime recognises the library type structurally: `Measurement<d>` is a
/// record with exactly those two `Real[d]` fields. A record that merely has a
/// field of that name is not a measurement (the checker's type is the real
/// gate; the runtime only needs a safe, total recogniser).
/// A plain dimensionless scalar, borrowed: what the fast arithmetic path needs
/// to know about an operand, without cloning it into a `NumVal` first.
#[derive(Clone, Copy)]
pub(crate) enum Scalar<'a> {
    Int(&'a BigInt),
    Float(f64),
}

impl<'a> Scalar<'a> {
    /// The view of a number, when it is a dimensionless `Int` or `Float`.
    pub(crate) fn of(number: &'a NumVal) -> Option<Self> {
        // A display unit (`50 %`) takes the general path, which applies the
        // §5.4.2 display rules.
        if !number.dim.is_one() || number.display.is_some() {
            return None;
        }
        match &number.kind {
            | NumKind::Int(value) => Some(Self::Int(value)),
            | NumKind::Float(value) => Some(Self::Float(*value)),
            | _ => None,
        }
    }

    fn to_f64(self) -> f64 {
        match self {
            | Self::Int(value) => num_traits::ToPrimitive::to_f64(value).unwrap_or(f64::NAN),
            | Self::Float(value) => value,
        }
    }
}

/// The result of `op` on two plain scalars, when it is one of the cases whose
/// answer is unambiguous: `Int` `+ - *`, `Float` (or `Float` with `Int`)
/// `+ - * /`, and the six comparisons on two `Int`s or two `Float`s. Mirrors
/// `scalar_arithmetic` / `compare` exactly for those, and answers `None` for
/// everything else.
pub(crate) fn plain_scalar_binary(
    op: BinOp,
    left: Scalar<'_>,
    right: Scalar<'_>,
) -> Option<Value> {
    match (left, right) {
        | (Scalar::Int(a), Scalar::Int(b)) => {
            Some(match op {
                | BinOp::Add => Value::Num(NumVal::int(a + b)),
                | BinOp::Sub => Value::Num(NumVal::int(a - b)),
                | BinOp::Mul => Value::Num(NumVal::int(a * b)),
                | BinOp::Eq => Value::Bool(a == b),
                | BinOp::Ne => Value::Bool(a != b),
                | BinOp::Lt => Value::Bool(a < b),
                | BinOp::Le => Value::Bool(a <= b),
                | BinOp::Gt => Value::Bool(a > b),
                | BinOp::Ge => Value::Bool(a >= b),
                | _ => return None,
            })
        },
        | (Scalar::Float(a), Scalar::Float(b)) => {
            Some(match op {
                | BinOp::Add => Value::Num(NumVal::float(a + b, Dim::one())),
                | BinOp::Sub => Value::Num(NumVal::float(a - b, Dim::one())),
                | BinOp::Mul => Value::Num(NumVal::float(a * b, Dim::one())),
                | BinOp::Div => Value::Num(NumVal::float(a / b, Dim::one())),
                | BinOp::Eq => Value::Bool(a == b),
                | BinOp::Ne => Value::Bool(a != b),
                | BinOp::Lt => Value::Bool(a < b),
                | BinOp::Le => Value::Bool(a <= b),
                | BinOp::Gt => Value::Bool(a > b),
                | BinOp::Ge => Value::Bool(a >= b),
                | _ => return None,
            })
        },
        // `Int` promotes below `Float` on the arithmetic path; comparisons of
        // mixed operands keep the general path's rules.
        | (left, right) => {
            let (a, b) = (left.to_f64(), right.to_f64());
            Some(Value::Num(NumVal::float(
                match op {
                    | BinOp::Add => a + b,
                    | BinOp::Sub => a - b,
                    | BinOp::Mul => a * b,
                    | BinOp::Div => a / b,
                    | _ => return None,
                },
                Dim::one(),
            )))
        },
    }
}

fn measurement_parts(value: &Value) -> Option<(NumVal, NumVal)> {
    let Value::Record(record) = value else {
        return None;
    };
    if record.fields.len() != 2 {
        return None;
    }
    match (record.fields.get("value"), record.fields.get("uncertainty")) {
        | (Some(Value::Num(value)), Some(Value::Num(uncertainty)))
            if value.dim == uncertainty.dim =>
        {
            Some((value.clone(), uncertainty.clone()))
        },
        | _ => None,
    }
}

/// A measurement record, fields in spec order (§8.14).
fn measurement_record(
    value: NumVal,
    uncertainty: NumVal,
) -> Value {
    let mut fields = IndexMap::new();
    fields.insert("value".to_string(), Value::Num(value));
    fields.insert("uncertainty".to_string(), Value::Num(uncertainty));
    Value::Record(Rc::new(RecordValue { fields }))
}

impl Interp {
    /// Creates an interpreter from a checked module.
    #[must_use]
    pub fn new(checked: &CheckedModule) -> Self {
        let session = Rc::new(());
        let context = crate::evidence::ModuleContext::new(checked, &session);
        Self {
            session,
            context,
            active_scope: None,
            module_scopes: HashMap::new(),
            bindings: None,
            globals: HashMap::new(),
            units: checked.units.clone(),
            dims: checked.dims.clone(),
            exports: checked.exports.clone(),
            adts: HashMap::new(),
            output: Output::Console,
            symbol_counts: HashMap::new(),
            seed: 0x2545_f491_4f6c_dd1d,
            capabilities: crate::value::caps::ALL,
            max_depth: 2048,
            max_tail_calls: 100_000_000,
            tensor_inputs: crate::generics::TensorInputCache::default(),
            call_proofs: crate::generics::CallProofCache::default(),
            constructor_witnesses: std::cell::RefCell::default(),
            literal_values: std::cell::RefCell::default(),
            instance_globals: RefCell::default(),
            buffer_pool: RefCell::default(),
            max_io_steps: 100_000_000,
            max_io_depth: 2048,
            max_nodes: 16_384,
            mode: if std::env::var("AEVIA_EVAL").as_deref() == Ok("ast") {
                Mode::Ast
            } else {
                Mode::Ir
            },
            depth: 0,
            nodes: 0,
            frames: Vec::new(),
            lambda_name: Rc::from("<lambda>"),
            constants_seeded: false,
            program_args: Vec::new(),
            rssn_calls: checked
                .rssn_calls
                .iter()
                .map(|call| (call.span, call.key.clone()))
                .collect(),
            dictionaries: dictionary_table(&checked.dictionaries),
            rssn_call_sites: checked.rssn_calls.clone(),
            dictionary_sites: checked.dictionaries.clone(),
            dyn_calls: checked
                .dyn_calls
                .iter()
                .map(|call| (call.span, call.dim.clone()))
                .collect(),
            registrations: checked.registrations.clone(),
            literal_reps: checked.literal_reps.clone(),
            module_units: [(checked.anonymous_file, checked.units.clone())]
                .into_iter()
                .collect(),
            expr_types: checked.expr_types.clone(),
            instance_exports: checked
                .instances
                .iter()
                .filter(|export| !export.derived)
                .cloned()
                .collect(),
            instance_sites: checked.instance_sites.clone(),
            method_refs: checked.method_refs.clone(),
            next_instance_export: 0,
            class_keys: checked
                .local_classes
                .keys()
                .map(|key| (key.name.clone(), key.clone()))
                .collect(),
            current_module: checked.module_path(),
            current_constructors: checked.constructors.clone(),
            instance_methods: HashMap::new(),
            method_classes: HashMap::new(),
            prelude_classes: ClassTable::prelude(),
            aliases: checked
                .aliases
                .iter()
                .map(|(name, ty)| (name.clone(), ty.clone()))
                .collect(),
        }
    }

    /// Sets the arguments `args()` returns (§9.12).
    ///
    /// The host calls this before `run_main`; the interpreter holds them as
    /// plain strings and `args()` answers a rank-1 `Tensor<1, String>`, so a
    /// program can index them like any other vector. Building the tensor is
    /// deferred to the call, so setting them cannot fail.
    pub fn set_args(
        &mut self,
        arguments: impl IntoIterator<Item = String>,
    ) {
        self.program_args = arguments.into_iter().collect();
    }

    /// Binds the prelude constants that are values rather than functions.
    ///
    /// These are kernel constants, not literals: `pi` has no exact
    /// representation, so it must stay symbolic or `2 * pi` stops being exact.
    /// A constant the kernel cannot build is left unbound rather than
    /// approximated, because a float `pi` would be a wrong answer that looks
    /// right.
    pub fn seed_constants(&mut self) {
        self.constants_seeded = true;
        let one = aevia_dim::Dim::one();
        for name in ["pi", "oo"] {
            if let Ok(value) = crate::kernel::call(name, &[], one.clone()) {
                self.globals.insert(name.to_string(), value);
            }
        }
        // `e` is not an operator in every kernel build; `exp(1)` is the same
        // value and is exact in the symbolic phase.
        if !self.globals.contains_key("e") {
            let one_value = Value::Num(NumVal::int(num_bigint::BigInt::from(1)));
            if let Ok(value) = crate::kernel::call("exp", &[one_value], one.clone()) {
                self.globals.insert("e".to_string(), value);
            }
        }
        self.globals.insert(
            "nan".to_string(),
            Value::Num(NumVal::float(f64::NAN, one.clone())),
        );
        self.globals.insert(
            "i".to_string(),
            Value::Num(NumVal {
                kind: NumKind::Complex(0.0, 1.0),
                dim: one,
                display: None,
            }),
        );
    }

    /// Adds another module's resolved class-method call sites.
    ///
    /// A call in an imported module has a span in *that* file, so without its
    /// records the evaluator would fall back to the global of the method name.
    pub fn add_dictionaries(
        &mut self,
        uses: &[DictionaryUse],
    ) {
        for use_site in uses {
            self.dictionaries
                .entry(use_site.span)
                .or_default()
                .push(use_site.clone());
            self.dictionary_sites.push(use_site.clone());
        }
    }

    /// Adds another module's pinned `Rssn.<op>` call sites.
    ///
    /// The lowering needs the records themselves, not just the resolved keys,
    /// so a call in an imported module carries its overload into the IR.
    pub fn add_rssn_calls(
        &mut self,
        calls: &[aevia_types::check::RssnCall],
    ) {
        for call in calls {
            self.rssn_calls.insert(call.span, call.key.clone());
            self.rssn_call_sites.push(call.clone());
        }
    }

    /// Switches the interpreter to the module about to be loaded.
    ///
    /// The checker's non-derived `InstanceExport`s are what name an instance
    /// body's global (`InstanceKey::mangle()`), so each module's load starts
    /// with its own list; a module's instances are matched to its `instance`
    /// declarations in order. The entry module is already set by
    /// [`Interp::new`], so a single-file program needs no call.
    pub fn prepare_module(
        &mut self,
        checked: &CheckedModule,
    ) {
        self.context = crate::evidence::ModuleContext::new(checked, &self.session);
        self.active_scope = None;
        self.bindings = None;
        // Span includes FileId: equal byte offsets in separate modules must
        // remain separate evidence. Reinstalling one checked module replaces
        // its records, so the entry's initial metadata is not appended twice.
        let file = checked.anonymous_file;
        self.expr_types.retain(|span, _| span.file != file);
        self.expr_types.extend(checked.expr_types.clone());
        self.literal_reps.retain(|span, _| span.file != file);
        self.literal_reps.extend(checked.literal_reps.clone());
        self.dyn_calls.retain(|span, _| span.file != file);
        self.dyn_calls.extend(
            checked
                .dyn_calls
                .iter()
                .map(|call| (call.span, call.dim.clone())),
        );
        self.rssn_calls.retain(|span, _| span.file != file);
        self.rssn_call_sites.retain(|call| call.span.file != file);
        self.add_rssn_calls(&checked.rssn_calls);
        self.dictionaries.retain(|span, _| span.file != file);
        self.dictionary_sites.retain(|call| call.span.file != file);
        self.add_dictionaries(&checked.dictionaries);
        self.method_refs.retain(|span, _| span.file != file);
        self.method_refs.extend(checked.method_refs.clone());
        self.instance_sites = checked.instance_sites.clone();
        self.class_keys.extend(
            checked
                .local_classes
                .keys()
                .map(|key| (key.name.clone(), key.clone())),
        );
        self.module_units.insert(file, checked.units.clone());
        self.registrations = checked.registrations.clone();
        self.current_module = checked.module_path();
        self.current_constructors = checked.constructors.clone();
        self.instance_exports = checked
            .instances
            .iter()
            .filter(|export| !export.derived)
            .cloned()
            .collect();
        self.next_instance_export = 0;
        self.add_interface(&checked.interface());
    }

    /// Makes an imported module's classes, instances and alias definitions
    /// reachable (§7.4): an importer can use a class it did not declare, and an
    /// alias expands to its definition instead of looking opaque.
    pub fn add_interface(
        &mut self,
        interface: &ModuleInterface,
    ) {
        for key in interface.classes.keys() {
            self.class_keys
                .entry(key.name.clone())
                .or_insert_with(|| key.clone());
        }
        for (name, alias) in &interface.aliases {
            self.aliases
                .entry(name.clone())
                .or_insert_with(|| alias.body.clone());
        }
    }

    /// The next `instance` declaration's export, if the checker supplied one.
    pub(crate) fn take_instance_export(&mut self) -> Option<InstanceExport> {
        let export = self
            .instance_exports
            .get(self.next_instance_export)
            .cloned();
        if export.is_some() {
            self.next_instance_export += 1;
        }
        export
    }

    /// A checked instance body is bound by its declaration site, never by the
    /// traversal order of a class registry. Default unchecked interpreters keep
    /// the legacy ordered API for compatibility.
    pub(crate) fn instance_export_at(
        &mut self,
        span: Span,
    ) -> Option<InstanceExport> {
        if self.instance_sites.is_empty() {
            self.take_instance_export()
        } else {
            self.instance_sites.get(&span).cloned()
        }
    }

    /// The global name of one instance method body.
    ///
    /// `InstanceKey::mangle()` is the single spelling the checker and the
    /// runtime both use (round-2 decision 1a); `Pretty@Int` and
    /// `Pretty@String` therefore get different globals.
    #[must_use]
    pub fn instance_global(
        key: &InstanceKey,
        method: &str,
    ) -> String {
        format!("{}::{method}", key.mangle())
    }

    /// Binds one `instance` method body under its mangled global (§3.8.1).
    ///
    /// `export` is the checker's symbolic record for this instance: it names
    /// the global and carries the head type whose witness dispatch compares.
    /// Without one (an interpreter built from a default checked module) the
    /// legacy source-spelling mangling is used, so the runtime API stays
    /// callable without a checker.
    pub fn register_instance(
        &mut self,
        class: &str,
        head: &Ty,
        export: Option<&InstanceExport>,
        method: &aevia_syntax::ast::InstanceMethod,
        env: &Rc<Scope>,
    ) {
        let spelled = instance_head_name(head);
        let key = export.map(|export| export.key.clone());
        let name = match &key {
            | Some(key) => Self::instance_global(key, &method.name),
            | None => {
                format!(
                    "{class}@{}::{}",
                    spelled.as_deref().unwrap_or("?"),
                    method.name
                )
            },
        };
        let closure = self.checked_ast_closure(
            method
                .params
                .iter()
                .map(|param| param.pattern.clone())
                .collect(),
            method.body.clone(),
            env.clone(),
            Some(name.clone()),
            method.span,
        );
        let entry = InstanceEntry {
            key: key.clone(),
            checked_head: export.map(|export| export.head.clone()),
            shape: head_shape(head, &self.aliases),
            global: name.clone(),
        };
        self.context
            .instances
            .borrow_mut()
            .push(crate::evidence::InstanceTemplate {
                entry,
                method: method.name.clone(),
                scope: Rc::downgrade(env),
                cached: Rc::new(RefCell::new(match &closure {
                    | Value::Closure(closure) => Rc::downgrade(closure),
                    | _ => unreachable!(),
                })),
                code: Rc::new(crate::evidence::MethodCode::Ast {
                    site: method.span,
                    params: method
                        .params
                        .iter()
                        .map(|param| param.pattern.clone())
                        .collect(),
                    body: method.body.clone(),
                    name: name.clone(),
                }),
            });
        self.globals.insert(name.clone(), closure);
        self.method_classes
            .insert(method.name.clone(), class.to_string());
        self.instance_methods
            .entry(format!("{class}::{}", method.name))
            .or_default()
            .push(InstanceEntry {
                key,
                checked_head: export.map(|export| export.head.clone()),
                shape: head_shape(head, &self.aliases),
                global: name,
            });
    }

    /// The dictionary the checker recorded for a class-method call at `span`.
    pub(crate) fn dictionary_use(
        &self,
        span: Span,
        method: &str,
    ) -> Option<DictionaryUse> {
        self.context
            .evidence
            .dictionaries
            .get(&span)?
            .iter()
            .find(|use_site| use_site.method == method)
            .cloned()
    }

    /// Whether the recorded head is one the prelude already provides.
    ///
    /// §3.8.4: `Eq`/`Ord`/`TotalOrd`/`Show`/`Hash` have prelude instances, and
    /// coherence (`add_instance`) forbids a user instance whose head overlaps
    /// one, so a prelude match is necessarily the instance the checker proved.
    fn is_builtin_instance(
        &self,
        use_site: &DictionaryUse,
    ) -> bool {
        matches!(
            self.prelude_classes
                .resolve(&use_site.class, &use_site.head),
            Resolution::Found { .. }
        )
    }

    /// What a class-method call at one site must run.
    ///
    /// `None` means the checker proved a user instance the runtime cannot
    /// find; the caller reports that instead of running the global.
    pub(crate) fn dispatch_method(
        &self,
        use_site: &DictionaryUse,
        method: &str,
        args: &[Value],
    ) -> Option<Dispatch> {
        // The checker pinned the target (round-2 decision 1c); this is the
        // authority. A concrete instance runs by its symbolic key (so two
        // primitive heads never share a global), a quantified site dispatches
        // among the proved candidates by value witness, and §3.8.4's
        // structural `Show` falls through to the builtin printer.
        match &use_site.target {
            | DictTarget::Instance(key) => {
                // A concrete instance is one named template. Collecting every
                // visible template (a scope-graph walk that clones each one)
                // and formatting the mangled name for each call were most of
                // an instance method call's cost; the name is formatted once
                // per site and the template is looked up in place.
                let global = self.cached_instance_global(use_site.span, key, method);
                if let Some(Some(body)) = self.find_instance_template(&global) {
                    return Some(Dispatch::Instance(body));
                }
                if self.active_scope.is_none() {
                    if let Some(body) = self.globals.get(&*global).cloned() {
                        return Some(Dispatch::Instance(body));
                    }
                }
                // The checked key is authoritative. A missing user body is
                // an error; selecting another instance would be unsound.
                return if self.is_builtin_instance(use_site) {
                    Some(Dispatch::Builtin)
                } else {
                    None
                };
            },
            | DictTarget::Builtin(BuiltinDispatch::ShowStructural | BuiltinDispatch::Derived) => {
                return Some(Dispatch::Builtin);
            },
            | DictTarget::Witness { candidates, derived } => {
                let templates = self.instance_templates();
                if let Some(first) = args.first() {
                    let mut matched = templates.iter().filter(|template| {
                        let entry = &template.entry;
                        template.method == method
                            && entry
                                .key
                                .as_ref()
                                .is_some_and(|key| candidates.contains(key))
                            && entry.checked_head.as_ref().is_some_and(|head| {
                                runtime_head_matches(head, first, &mut HashMap::new())
                            })
                    });
                    if let Some(entry) = matched.next() {
                        if matched.next().is_none() {
                            if let Some(body) = entry.instantiate() {
                                return Some(Dispatch::Instance(body));
                            }
                        }
                    }
                }
                // A checked candidate must be proved by the runtime value;
                // never fall back to labels or a sole unrelated instance.
                if let Some(first) = args.first() {
                    let builtin = self
                        .prelude_classes
                        .instances_of(&use_site.class)
                        .into_iter()
                        .any(|instance| {
                            candidates.contains(&instance.key)
                                && runtime_head_matches(&instance.head, first, &mut HashMap::new())
                        });
                    let derived_match = derived
                        .iter()
                        .any(|(_, head)| runtime_head_matches(head, first, &mut HashMap::new()));
                    if builtin
                        || derived_match
                        || (use_site.class == "Show" && runtime_structural_show(first))
                    {
                        return Some(Dispatch::Builtin);
                    }
                }
                return None;
            },
        }
    }

    /// Runs a class method that reached the evaluator as a *value*.
    ///
    /// A first-class method carries no call-site record, so the instance is
    /// chosen from the argument alone; when no user instance accepts it the
    /// builtin of the same name is the prelude instance (§3.8.4).
    ///
    /// # Errors
    ///
    /// `E0310` when a user class has instances but none accepts the argument,
    /// plus whatever the instance or the builtin reports.
    pub fn call_class_method(
        &mut self,
        class: &str,
        method: &str,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, EvalError> {
        let selected = self
            .instance_methods
            .get(&format!("{class}::{method}"))
            .and_then(|entries| {
                let first = args.first()?;
                let mut matched = entries.iter().filter(|entry| {
                    match &entry.checked_head {
                        | Some(head) => runtime_head_matches(head, first, &mut HashMap::new()),
                        | None => entry.shape.accepts(first),
                    }
                });
                let entry = matched.next()?;
                if matched.next().is_some() {
                    return None;
                }
                self.globals.get(&entry.global).cloned()
            });
        if let Some(body) = selected {
            return self.apply(body, args, span);
        }
        if self.prelude_classes.has_class(class) {
            return self.call_named(method, args, span);
        }
        Err(EvalError::new(
            "E0310",
            format!("no `{class}` instance accepts this argument"),
        )
        .at(span))
    }

    /// A method value retains the class identity selected by the checker.
    pub(crate) fn class_method_value(
        class: ClassKey,
        method: String,
        scope: Rc<Scope>,
    ) -> Value {
        Value::Native(Rc::new(NativeFn(Box::new(move |interp, args, span| {
            let previous = interp.enter_scope(&scope)?;
            let result = interp.call_class_method_key(&class, &method, args, span);
            interp.restore_context(previous);
            result
        }))))
    }

    fn call_class_method_key(
        &mut self,
        class: &ClassKey,
        method: &str,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, EvalError> {
        let templates = self.instance_templates();
        let selected = (|| {
            let first = args.first()?;
            let mut matched = templates.iter().filter(|template| {
                let entry = &template.entry;
                template.method == method
                    && entry.key.as_ref().is_some_and(|key| &key.class == class)
                    && entry
                        .checked_head
                        .as_ref()
                        .is_some_and(|head| runtime_head_matches(head, first, &mut HashMap::new()))
            });
            let entry = matched.next()?;
            if matched.next().is_some() {
                return None;
            }
            entry.instantiate()
        })();
        if let Some(body) = selected {
            return self.apply(body, args, span);
        }
        if class.module.is_prelude() && self.prelude_classes.has_class(&class.name) {
            return self.call_named(method, args, span);
        }
        Err(EvalError::new(
            "E0310",
            format!("no `{class}` instance accepts this argument"),
        )
        .at(span))
    }

    /// Redirects output to a buffer.
    #[must_use]
    pub fn with_buffer(mut self) -> Self {
        self.output = Output::Buffer(Rc::new(RefCell::new(String::new())));
        self
    }

    /// Selects which evaluator runs the program.
    ///
    /// The AST path is the semantic reference; the IR path is the default once
    /// the two agree on the whole suite (§9.2.2, PLAN D2).
    #[must_use]
    pub fn with_mode(
        mut self,
        mode: Mode,
    ) -> Self {
        self.mode = mode;
        self
    }

    /// The captured output, when buffering.
    #[must_use]
    pub fn captured(&self) -> String {
        match &self.output {
            | Output::Buffer(buffer) => buffer.borrow().clone(),
            | Output::Console => String::new(),
        }
    }

    fn write_out(
        &mut self,
        text: &str,
        newline: bool,
        stderr: bool,
    ) {
        match &self.output {
            | Output::Console => {
                if stderr {
                    eprint!("{text}");
                    if newline {
                        eprintln!();
                    }
                } else {
                    print!("{text}");
                    if newline {
                        println!();
                    }
                }
            },
            | Output::Buffer(buffer) => {
                let mut buffer = buffer.borrow_mut();
                buffer.push_str(text);
                if newline {
                    buffer.push('\n');
                }
            },
        }
    }

    /// The root scope.
    #[must_use]
    pub fn root(&self) -> Rc<Scope> {
        let mut imports = self.module_scopes.iter().collect::<Vec<_>>();
        imports.sort_by(|(a, _), (b, _)| a.cmp(b));
        let scope = Scope::module(
            self.context.clone(),
            imports
                .into_iter()
                .map(|(_, scope)| scope.clone())
                .collect(),
        );
        // REPL closures capture the bindings already checked at their lexical
        // definition, including implementation globals. Later revisions may
        // replace the public inspection table without changing old closures.
        scope.vars.borrow_mut().extend(self.globals.clone());
        scope
    }

    // ----- evaluation -----------------------------------------------------

    /// Evaluates an expression in a scope.
    ///
    /// # Errors
    ///
    /// Returns [`EvalError`] for runtime failures (division by zero, shape
    /// mismatches, unmatched patterns, and so on).
    pub fn eval(
        &mut self,
        expr: &Expr,
        env: &Rc<Scope>,
    ) -> Result<Value, EvalError> {
        self.eval_expression(expr, env, true)
    }

    fn eval_callee(
        &mut self,
        expr: &Expr,
        env: &Rc<Scope>,
    ) -> Result<Value, EvalError> {
        self.eval_expression(expr, env, false)
    }

    fn eval_expression(
        &mut self,
        expr: &Expr,
        env: &Rc<Scope>,
        escaping: bool,
    ) -> Result<Value, EvalError> {
        let previous = self.enter_scope(env)?;
        self.nodes += 1;
        if self.nodes > self.max_nodes {
            self.nodes -= 1;
            self.restore_context(previous);
            return Err(EvalError::new("E0805", "expression is nested too deeply")
                .at(expr.span)
                .with_stack(self.frames.clone()));
        }
        // These paths may remain live across a source-language call. Enter
        // their small frames directly rather than retaining eval_inner's
        // stack slots for every unrelated expression variant at each depth.
        let result = match &expr.kind {
            | ExprKind::Unary { op, operand } => self.eval_unary_expression(expr, op, operand, env),
            | ExprKind::Binary { op, left, right } => {
                self.eval_binary_expression(expr, op, left, right, env)
            },
            | ExprKind::Call { callee, args } => self.eval_call_expression(expr, callee, args, env),
            | ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.eval_if_expression(cond, then_branch, else_branch, env),
            | ExprKind::Match { scrutinee, arms } => {
                self.eval_match_expression(expr, scrutinee, arms, env)
            },
            | ExprKind::LetIn {
                pattern, value, body, ..
            } => self.eval_let_expression(pattern, value, body, env),
            | ExprKind::Block { stmts, tail } => {
                self.eval_block_expression(stmts, tail.as_deref(), env)
            },
            | ExprKind::Annot { expr: inner, .. } => self.eval(inner, env),
            | _ => self.eval_inner(expr, env),
        }
        .map(|value| self.attach_checked_type(value, expr.span))
        .map(|value| {
            if escaping {
                self.bind_source_function(value, expr.span)
            } else {
                value
            }
        });
        self.nodes -= 1;
        self.restore_context(previous);
        result
    }

    /// Preserve only fully concrete checked type information.
    pub(crate) fn attach_checked_type(
        &self,
        mut value: Value,
        span: Span,
    ) -> Value {
        // Only actual checked constructor references may specialize a value.
        // Ordinary variable reads must not acquire proof from an annotation.
        let Some(source_constructor) = self.context.evidence.constructor_refs.get(&span).copied()
        else {
            return value;
        };
        let Some(ty) = self.resolved_expr_type(span) else {
            return value;
        };
        let source_constructor = Some(source_constructor);
        if let (
            aevia_types::Type::Fn(_, result),
            Value::Ctor {
                adt,
                index,
                name,
                origin: Some(token),
                ..
            },
        ) = (&ty, &mut value)
        {
            if let aevia_types::Type::Adt(result_id, _) = result.as_ref() {
                let authentic = source_constructor.is_some_and(|(declared, declared_index)| {
                    declared_index == *index && adt::same_type(declared, *adt)
                });
                if authentic
                    && token.adt == *adt
                    && token.index == *index
                    && token.name == *name
                    && adt::same_type(*result_id, *adt)
                    && !aevia_types::class::has_variables(result)
                    && token.selected_result.is_none()
                {
                    token.selected_result = Some((**result).clone());
                }
            }
        }
        if aevia_types::class::has_variables(&ty) {
            return value;
        }
        if let (aevia_types::Type::Adt(id, args), Value::Variant(variant)) = (&ty, &value) {
            let authentic = source_constructor.is_some_and(|(declared, index)| {
                index == variant.index && adt::same_type(declared, variant.adt)
            });
            if authentic
                && variant.constructor_origin
                && variant.verified_type.is_none()
                && variant.type_args.is_none()
                && adt::same_type(*id, variant.adt)
                && crate::host_witness::checked_constructor_result(&value, &ty)
            {
                if let Value::Variant(variant) = &mut value {
                    let variant = Rc::make_mut(variant);
                    variant.verified_type = Some(ty.clone());
                    variant.type_args = Some(args.clone());
                }
            }
        }
        value
    }

    #[inline(never)]
    fn eval_unary_expression(
        &mut self,
        expr: &Expr,
        op: &UnOp,
        operand: &Expr,
        env: &Rc<Scope>,
    ) -> Result<Value, EvalError> {
        let value = self.eval(operand, env)?;
        self.unary(*op, value, expr.span)
    }

    #[inline(never)]
    fn eval_binary_expression(
        &mut self,
        expr: &Expr,
        op: &BinOp,
        left: &Expr,
        right: &Expr,
        env: &Rc<Scope>,
    ) -> Result<Value, EvalError> {
        if matches!(op, BinOp::And | BinOp::Or) {
            let left_value = self.eval(left, env)?;
            let short_circuit = match (op, &left_value) {
                | (BinOp::And, Value::Bool(false)) => Some(false),
                | (BinOp::Or, Value::Bool(true)) => Some(true),
                | _ => None,
            };
            if let Some(value) = short_circuit {
                return Ok(Value::Bool(value));
            }
            let right_value = self.eval(right, env)?;
            return self.binary(*op, left_value, right_value, expr.span);
        }
        let left_value = self.eval(left, env)?;
        let right_value = self.eval(right, env)?;
        self.binary(*op, left_value, right_value, expr.span)
    }

    #[inline(never)]
    fn eval_call_expression(
        &mut self,
        expr: &Expr,
        callee: &Expr,
        args: &[Expr],
        env: &Rc<Scope>,
    ) -> Result<Value, EvalError> {
        if args
            .iter()
            .any(|arg| matches!(arg.kind, ExprKind::Placeholder))
        {
            // `f(_, x)` is a lambda over the placeholders (§2.3.6).
            let mut params = Vec::new();
            let mut call_args = Vec::new();
            for (index, arg) in args.iter().enumerate() {
                if matches!(arg.kind, ExprKind::Placeholder) {
                    let name = aevia_syntax::ast::placeholder_param_name(index);
                    params.push(Pat::new(PatKind::Var(name.clone()), arg.span));
                    call_args.push(Expr::new(ExprKind::Var(name), arg.span));
                } else {
                    call_args.push(arg.clone());
                }
            }
            let body = Expr::new(
                ExprKind::Call {
                    callee: Box::new(callee.clone()),
                    args: call_args,
                },
                expr.span,
            );
            return Ok(self.checked_ast_closure(params, body, env.clone(), None, expr.span));
        }
        // Type arguments belong to the actual value, not its spelling:
        // aliases and qualified imports retain builtin identity, while a
        // user function with the same name still shadows the builtin.
        if let ExprKind::TypeApp {
            callee: inner,
            args: type_args,
        } = &callee.kind
        {
            let function = self.eval_callee(inner, env)?;
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(self.eval(arg, env)?);
            }
            let function = self.type_apply_value(function, type_args, callee.span);
            let arg_spans: Vec<_> = args.iter().map(|arg| arg.span).collect();
            return self.apply_source_call(function, values, expr.span, callee.span, &arg_spans);
        }
        // `Rssn.<op>(…)`: execute the overload the checker pinned at
        // this call site (PLAN D1). There is no fallback — resolving
        // again here would let the executed overload differ from the
        // typed one, which is the unsoundness the pin exists to
        // remove. An unknown key is `E1005`, never a re-resolution.
        if let Some(op) = rssn_operator_name(callee) {
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(self.eval(arg, env)?);
            }
            return self.call_rssn(&op, expr.span, values);
        }
        // §3.8.1: a call the checker resolved to a class instance
        // runs that instance's body. Without this the body is
        // unreachable — it lives under a mangled global that no
        // program can name — and the call would run the global of the
        // method name instead.
        if let ExprKind::Var(name) = &callee.kind {
            if let Some(use_site) = self.dictionary_use(expr.span, name) {
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.eval(arg, env)?);
                }
                return self.run_dictionary(&use_site, name, values, expr.span);
            }
        }
        let function = self.eval_callee(callee, env)?;
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval(arg, env)?);
        }
        let arg_spans: Vec<_> = args.iter().map(|arg| arg.span).collect();
        self.apply_source_call(function, values, expr.span, callee.span, &arg_spans)
    }

    #[inline(never)]
    fn eval_if_expression(
        &mut self,
        cond: &Expr,
        then_branch: &Expr,
        else_branch: &Expr,
        env: &Rc<Scope>,
    ) -> Result<Value, EvalError> {
        match self.eval(cond, env)? {
            | Value::Bool(true) => self.eval(then_branch, env),
            | Value::Bool(false) => self.eval(else_branch, env),
            | other => {
                Err(EvalError::new(
                    "E0302",
                    format!("`if` expects a Bool condition, found {}", other.type_name()),
                )
                .at(cond.span))
            },
        }
    }

    #[inline(never)]
    fn eval_match_expression(
        &mut self,
        expr: &Expr,
        scrutinee: &Expr,
        arms: &[MatchArm],
        env: &Rc<Scope>,
    ) -> Result<Value, EvalError> {
        let subject = self.eval(scrutinee, env)?;
        for arm in arms {
            let scope = Scope::child(env);
            if self.match_pattern(&arm.pattern, &subject, &scope)? {
                if let Some(guard) = &arm.guard {
                    match self.eval(guard, &scope)? {
                        | Value::Bool(true) => {},
                        | Value::Bool(false) => continue,
                        | other => {
                            return Err(EvalError::new(
                                "E0308",
                                format!("a match guard must be Bool, found {}", other.type_name()),
                            )
                            .at(guard.span));
                        },
                    }
                }
                return self.eval(&arm.body, &scope);
            }
        }
        Err(EvalError::new(
            "E0309",
            format!("no match arm accepted {}", value::show(&subject)),
        )
        .at(expr.span))
    }

    #[inline(never)]
    fn eval_let_expression(
        &mut self,
        pattern: &Pat,
        value: &Expr,
        body: &Expr,
        env: &Rc<Scope>,
    ) -> Result<Value, EvalError> {
        let bound = self.eval(value, env)?;
        let scope = Scope::child(env);
        self.bind_pattern(pattern, &bound, &scope)?;
        self.eval(body, &scope)
    }

    #[inline(never)]
    fn eval_block_expression(
        &mut self,
        stmts: &[BlockStmt],
        tail: Option<&Expr>,
        env: &Rc<Scope>,
    ) -> Result<Value, EvalError> {
        // One scope per *binding*, not one per block. A block-wide
        // scope is mutated by every `let`, so a closure created before
        // a later `let x = …` in the same block would read the later
        // value — a cell, not a value, and not what the lexical scope
        // says. Extending the chain keeps every captured scope frozen.
        let mut scope = Scope::child(env);
        for stmt in stmts {
            match stmt {
                | BlockStmt::Let { pattern, value, .. } => {
                    let bound = self.eval(value, &scope)?;
                    let next = Scope::child(&scope);
                    self.bind_pattern(pattern, &bound, &next)?;
                    scope = next;
                },
                | BlockStmt::Fn(decl) => {
                    if let DeclKind::Fn { name, args, body, .. } = &decl.kind {
                        let next = Scope::child(&scope);
                        let closure = self.checked_ast_closure(
                            args.iter().map(|arg| arg.pattern.clone()).collect(),
                            body.clone(),
                            next.clone(),
                            Some(name.clone()),
                            decl.span,
                        );
                        next.vars.borrow_mut().insert(name.clone(), closure);
                        scope = next;
                    }
                },
            }
        }
        match tail {
            | Some(tail) => self.eval(tail, &scope),
            | None => Ok(Value::Unit),
        }
    }

    fn eval_inner(
        &mut self,
        expr: &Expr,
        env: &Rc<Scope>,
    ) -> Result<Value, EvalError> {
        match &expr.kind {
            // A parser recovery node: the syntax error was already reported.
            | ExprKind::Error => Ok(Value::Unit),
            | ExprKind::UnitLit => Ok(Value::Unit),
            | ExprKind::Bool(value) => Ok(Value::Bool(*value)),
            // §3.10.1 / §6.3.2: the checker's instantiated representation is
            // the runtime's, so a literal in a `Float` context is an f64 and one
            // in a `Real` context is exact.
            | ExprKind::Int(value) => Ok(Value::Num(self.integer_literal(value, expr.span)?)),
            | ExprKind::Float(text) => Ok(Value::Num(self.float_literal(text, expr.span)?)),
            | ExprKind::Str(pieces) => {
                let mut out = String::new();
                for piece in pieces {
                    match piece {
                        | StrPiece::Text(text) => out.push_str(text),
                        | StrPiece::Expr(inner) => {
                            let value = self.eval(inner, env)?;
                            out.push_str(&value::show(&value));
                        },
                    }
                }
                Ok(Value::string(out))
            },
            | ExprKind::Quantity { value, unit } => {
                let as_float = self.literal_rep(expr.span) == Some(Rep::Float);
                self.quantity(value, unit, expr.span, as_float)
            },
            | ExprKind::Var(name) => self.lookup(name, env, expr.span),
            | ExprKind::QName(path) => {
                if path.len() == 2 && path[0] == "Rssn" {
                    return Ok(crate::rssn_module::member(&path[1]));
                }
                self.lookup(&path.join("."), env, expr.span)
            },
            | ExprKind::Unary { op, operand } => self.eval_unary_expression(expr, op, operand, env),
            | ExprKind::Binary { op, left, right } => {
                self.eval_binary_expression(expr, op, left, right, env)
            },
            | ExprKind::Call { callee, args } => self.eval_call_expression(expr, callee, args, env),
            | ExprKind::TypeApp { callee, args } => {
                let function = self.eval_callee(callee, env)?;
                Ok(self.type_apply_value(function, args, expr.span))
            },
            | ExprKind::Lambda { params, body, .. } => {
                Ok(self.checked_ast_closure(
                    params.iter().map(|param| param.pattern.clone()).collect(),
                    (**body).clone(),
                    env.clone(),
                    None,
                    expr.span,
                ))
            },
            | ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.eval_if_expression(cond, then_branch, else_branch, env),
            | ExprKind::Match { scrutinee, arms } => {
                self.eval_match_expression(expr, scrutinee, arms, env)
            },
            | ExprKind::LetIn {
                pattern, value, body, ..
            } => self.eval_let_expression(pattern, value, body, env),
            | ExprKind::Block { stmts, tail } => {
                self.eval_block_expression(stmts, tail.as_deref(), env)
            },
            | ExprKind::Do { stmts } => self.eval_do(stmts, env, expr.span),
            | ExprKind::Record { base, fields } => {
                let mut collected: IndexMap<String, Value> = IndexMap::new();
                if let Some(base) = base {
                    match self.eval(base, env)? {
                        | Value::Record(record) => {
                            for (name, value) in &record.fields {
                                collected.insert(name.clone(), value.clone());
                            }
                        },
                        | other => {
                            return Err(EvalError::new(
                                "E0302",
                                format!(
                                    "record update expects a record, found {}",
                                    other.type_name()
                                ),
                            )
                            .at(base.span));
                        },
                    }
                }
                for (name, value) in fields {
                    let evaluated = self.eval(value, env)?;
                    collected.insert(name.clone(), evaluated);
                }
                Ok(Value::Record(Rc::new(RecordValue { fields: collected })))
            },
            | ExprKind::Field { base, name } => {
                if is_rssn_root(base) {
                    // `Rssn.sin` is a kernel operator, not a record field.
                    return Ok(crate::rssn_module::member(name));
                }
                if let Some(qualified) = qualified_name(base, name) {
                    if self.globals.contains_key(&qualified) {
                        return self.lookup(&qualified, env, expr.span);
                    }
                }
                match self.eval(base, env)? {
                    | Value::Record(record) => {
                        record.fields.get(name).cloned().ok_or_else(|| {
                            EvalError::new("E0306", format!("no field `{name}`")).at(expr.span)
                        })
                    },
                    | other => {
                        Err(EvalError::new(
                            "E0302",
                            format!("field access expects a record, found {}", other.type_name()),
                        )
                        .at(expr.span))
                    },
                }
            },
            | ExprKind::Tuple(items) => {
                let mut fields = IndexMap::new();
                for (index, item) in items.iter().enumerate() {
                    fields.insert(index.to_string(), self.eval(item, env)?);
                }
                Ok(Value::Record(Rc::new(RecordValue { fields })))
            },
            | ExprKind::Tensor(items) => self.tensor_literal(items, env, expr.span),
            | ExprKind::Index { base, indices } => {
                let tensor = self.eval(base, env)?;
                self.index_value(tensor, indices, env, expr.span)
            },
            | ExprKind::Annot { expr: inner, .. } => self.eval(inner, env),
            | ExprKind::Placeholder => {
                Err(EvalError::new("E0202", "`_` cannot be evaluated outside a call").at(expr.span))
            },
            | ExprKind::Try(inner) => {
                let value = self.eval(inner, env)?;
                self.try_value(value, expr.span)
            },
            | ExprKind::Factorial { expr: inner, double } => {
                let value = self.eval(inner, env)?;
                let n = match &value {
                    | Value::Num(num) => {
                        match &num.kind {
                            | NumKind::Int(value) => {
                                value.to_i64().ok_or_else(|| {
                                    EvalError::new("E0801", "factorial is too large")
                                })?
                            },
                            | _ => {
                                return Err(EvalError::new("E0302", "factorial expects an Int")
                                    .at(expr.span));
                            },
                        }
                    },
                    | _ => {
                        return Err(
                            EvalError::new("E0302", "factorial expects an Int").at(expr.span)
                        );
                    },
                };
                Ok(Value::Num(NumVal::int(factorial(n, *double))))
            },
        }
    }

    fn lookup(
        &mut self,
        name: &str,
        env: &Rc<Scope>,
        span: Span,
    ) -> Result<Value, EvalError> {
        if let Some(value) = env.lookup(name) {
            return Ok(value);
        }
        self.lookup_global(name, Some(env), span)
    }

    /// Resolves a name that is not a slot of the current frame.
    ///
    /// This is the tail of [`Interp::lookup`]: the module scope, the globals,
    /// the unit table, constructors, class methods and the builtin names, in
    /// the order the AST path uses. The IR passes the module scope explicitly
    /// because its lexical chain is a slot vector rather than a `Scope`.
    pub(crate) fn lookup_global(
        &mut self,
        name: &str,
        scope: Option<&Rc<Scope>>,
        span: Span,
    ) -> Result<Value, EvalError> {
        // The kernel constants are seeded on first use: building `pi`/`e` asks
        // the kernel for symbolic values, which dominated an empty program's
        // cold start (§9.13). A name the program never mentions never pays.
        if !self.constants_seeded && matches!(name, "pi" | "oo" | "e" | "nan" | "i") {
            self.seed_constants();
        }
        if let Some(scope) = scope {
            if let Some(value) = scope.lookup(name) {
                return Ok(value);
            }
        }
        if let Some(reference) = self
            .context
            .evidence
            .method_refs
            .get(&span)
            .filter(|reference| reference.method == name)
        {
            return Ok(Self::class_method_value(
                reference.class.clone(),
                reference.method.clone(),
                scope
                    .cloned()
                    .or_else(|| self.active_scope.clone())
                    .unwrap_or_else(|| self.root()),
            ));
        }
        if let Some(value) = self.globals.get(name) {
            return Ok(value.clone());
        }
        // §3.8.1: a class method is a value, so `map(pretty, xs)` and
        // `let f = pretty` are legal. It carries no call-site record, so the
        // instance is chosen from the argument when it is applied.
        if let Some(class) = self.method_classes.get(name).cloned() {
            let method = name.to_string();
            return Ok(Value::Native(Rc::new(NativeFn(Box::new(
                move |interp, args, span| interp.call_class_method(&class, &method, args, span),
            )))));
        }
        // Builtin functions are resolved *before* units: several unit names are
        // also function names — `dyn` is the CGS force unit and `at` the
        // technical atmosphere, while `dyn`/`at`/`check` are language
        // primitives the checker types as calls. Resolving the unit first made
        // `dyn(3 m)` "Unit is not a function" at run time while it type-checked
        // as the primitive. A unit in *call* position is never what the program
        // meant; as a value it is still reachable through the unit table and
        // the `N unit` literal syntax.
        if aevia_types::builtins::is_builtin(name) || aevia_types::builtins::is_special(name) {
            return Ok(Value::Ctor {
                adt: u32::MAX,
                index: usize::MAX,
                name: name.to_string(),
                arity: usize::MAX,
                args: Vec::new(),
                origin: Some(value::SourceCtorToken {
                    adt: u32::MAX,
                    index: usize::MAX,
                    name: name.to_string(),
                    selected_result: None,
                }),
            });
        }
        let source_units = &self.context.evidence.units;
        if let Some(unit) = source_units.lookup(name) {
            return Ok(Value::UnitOf(Rc::new(unit)));
        }
        if let Some((adt, index, _)) = adt::find_prelude_ctor(name) {
            if let Some(value) = source_ctor_value_at(adt, index) {
                return Ok(value);
            }
        }
        Err(EvalError::new("E0301", format!("unbound name `{name}`")).at(span))
    }

    /// The representation the checker instantiated for the literal at `span`.
    fn literal_rep(
        &self,
        span: Span,
    ) -> Option<Rep> {
        let rep = self.context.evidence.literal_reps.get(&span).copied()?;
        Some(
            self.bindings
                .as_ref()
                .filter(|bindings| {
                    bindings
                        .context
                        .upgrade()
                        .is_some_and(|context| Rc::ptr_eq(&context, &self.context))
                })
                .map_or(rep, |bindings| bindings.values.rep(rep)),
        )
    }

    fn checked_literal_rep(
        &self,
        kind: aevia_types::LiteralKind,
        span: Span,
    ) -> Result<Option<Rep>, EvalError> {
        // A concrete creation-site reading is independent of the activation's
        // generic parameters, including body-only literal variables defaulted
        // by the checker. Never reinterpret it through another expression.
        if let Some(rep) = self
            .context
            .evidence
            .literal_reps
            .get(&span)
            .copied()
            .filter(|rep| !matches!(rep, Rep::Var(_)))
        {
            if kind.accepts_rep(rep) {
                return Ok(Some(rep));
            }
            return Err(EvalError::new(
                "E0302",
                "the selected representation cannot represent this numeric literal",
            )
            .at(span));
        }
        if let Some(ty) = self.resolved_expr_type(span) {
            if aevia_types::class::has_variables(&ty) {
                return Err(EvalError::new(
                    "E0304",
                    "a numeric literal requires concrete runtime type arguments",
                )
                .at(span));
            }
            match ty {
                | aevia_types::Type::Scalar(rep, dim)
                    if kind.accepts_rep(rep) && (kind.is_quantity() || dim.is_one()) =>
                {
                    return Ok(Some(rep));
                },
                | aevia_types::Type::Point(_) if kind.is_quantity() => return Ok(Some(Rep::Float)),
                | _ => {
                    return Err(EvalError::new(
                        "E0302",
                        format!("the selected type {ty} cannot represent this numeric literal"),
                    )
                    .at(span));
                },
            }
        }
        let rep = self.literal_rep(span);
        if matches!(rep, Some(Rep::Var(_)))
            || self
                .expr_type(span)
                .is_some_and(aevia_types::class::has_variables)
        {
            return Err(EvalError::new(
                "E0304",
                "a numeric literal requires verified concrete runtime type arguments",
            )
            .at(span));
        }
        if rep.is_some_and(|rep| !kind.accepts_rep(rep)) {
            return Err(EvalError::new(
                "E0302",
                "the selected representation cannot represent this numeric literal",
            )
            .at(span));
        }
        Ok(rep)
    }

    /// An integer literal in the representation its context chose (§3.10.1).
    pub(crate) fn integer_literal(
        &self,
        value: &BigInt,
        span: Span,
    ) -> Result<NumVal, EvalError> {
        Ok(
            match self.checked_literal_rep(aevia_types::LiteralKind::Int, span)? {
                | Some(Rep::Float) => {
                    NumVal::float(
                        num_traits::ToPrimitive::to_f64(value).unwrap_or(f64::NAN),
                        Dim::one(),
                    )
                },
                | Some(Rep::Real) => {
                    NumVal::exact(BigRational::from_integer(value.clone()), Dim::one())
                },
                | Some(Rep::Complex) => {
                    NumVal {
                        kind: NumKind::Complex(
                            num_traits::ToPrimitive::to_f64(value).unwrap_or(f64::NAN),
                            0.0,
                        ),
                        dim: Dim::one(),
                        display: None,
                    }
                },
                | _ => NumVal::int(value.clone()),
            },
        )
    }

    /// A decimal literal in the representation its context chose.
    ///
    /// A `Real` context keeps the exact rational the decimal text denotes
    /// (§6.3.2); a `Float` context uses the nearest `f64`.
    pub(crate) fn float_literal(
        &self,
        text: &str,
        span: Span,
    ) -> Result<NumVal, EvalError> {
        let cleaned = text.replace('_', "");
        let as_float = || cleaned.parse::<f64>().unwrap_or(f64::NAN);
        Ok(
            match self.checked_literal_rep(aevia_types::LiteralKind::Decimal, span)? {
                | Some(Rep::Real) => {
                    aevia_dim::parse_decimal(&cleaned).map_or_else(
                        || NumVal::float(as_float(), Dim::one()),
                        |value| NumVal::exact(value, Dim::one()),
                    )
                },
                | Some(Rep::Complex) => {
                    NumVal {
                        kind: NumKind::Complex(as_float(), 0.0),
                        dim: Dim::one(),
                        display: None,
                    }
                },
                | _ => NumVal::float(as_float(), Dim::one()),
            },
        )
    }

    pub(crate) fn quantity(
        &mut self,
        value: &NumberLit,
        unit: &aevia_dim::UnitExpr,
        span: Span,
        as_float: bool,
    ) -> Result<Value, EvalError> {
        let kind = if value.is_decimal() {
            aevia_types::LiteralKind::QuantityDecimal
        } else {
            aevia_types::LiteralKind::QuantityInt
        };
        let representation = self.checked_literal_rep(kind, span)?;
        let as_float = representation.map_or(as_float, |rep| rep == Rep::Float);
        let resolved = self
            .context
            .evidence
            .units
            .resolve(unit)
            .map_err(|error| EvalError::new("E0501", error.to_string()).at(span))?;
        let display = UnitDef {
            name: resolved.name.clone(),
            aliases: Vec::new(),
            dim: resolved.dim.clone(),
            scale: resolved.scale.clone(),
            offset: resolved.offset.clone(),
            prefixable: false,
            affine: resolved.affine,
            display: resolved.name.clone(),
            category: "literal".to_string(),
        };
        let display = Rc::new(display);
        if resolved.affine {
            let coefficient = value.as_f64();
            let coherent = resolved.to_coherent(coefficient);
            let mut num = NumVal::float(coherent, resolved.dim);
            num.display = Some(display);
            return Ok(Value::Num(num));
        }
        if representation == Some(Rep::Complex) {
            let coherent = value.as_f64() * resolved.scale.to_f64();
            return Ok(Value::Num(NumVal {
                kind: NumKind::Complex(coherent, 0.0),
                dim: resolved.dim,
                display: Some(display),
            }));
        }
        // §3.10.1: the literal's context chose `Float`, so its value is an f64
        // — including a unit literal such as `2.0 m` in a `Float[L]` slot.
        if as_float {
            let coefficient = value.as_f64();
            let coherent = coefficient * resolved.scale.to_f64();
            let mut num = NumVal::float(coherent, resolved.dim);
            num.display = Some(display);
            return Ok(Value::Num(num));
        }
        // A scale with a factor of pi (`deg`) stays symbolic.
        if resolved.scale.pi != 0 {
            let coefficient = value
                .as_rational()
                .ok_or_else(|| EvalError::new("E0106", "invalid numeric literal").at(span))?;
            let mut term = crate::sym::Term::Num(coefficient * resolved.scale.rat.clone());
            if resolved.scale.pi != 0 {
                let pi = crate::sym::Term::Sym("pi".to_string());
                term = crate::sym::Term::mul(
                    term,
                    crate::sym::Term::pow(
                        pi,
                        BigRational::from_integer(BigInt::from(resolved.scale.pi)),
                    ),
                );
            }
            let mut num = NumVal::symbolic(term, resolved.dim);
            num.display = Some(display);
            return Ok(Value::Num(num));
        }
        let num = match value {
            | NumberLit::Int(coefficient) => {
                let scaled =
                    BigRational::from_integer(coefficient.clone()) * resolved.scale.rat.clone();
                NumVal::exact(scaled, resolved.dim)
            },
            | NumberLit::Decimal(text) => {
                let coefficient = aevia_dim::parse_decimal(text)
                    .ok_or_else(|| EvalError::new("E0106", "invalid decimal literal").at(span))?;
                let scaled = coefficient * resolved.scale.rat.clone();
                NumVal::exact(scaled, resolved.dim)
            },
        };
        Ok(Value::Num(num.with_display(display)))
    }

    pub(crate) fn unary(
        &mut self,
        op: UnOp,
        value: Value,
        span: Span,
    ) -> Result<Value, EvalError> {
        match op {
            | UnOp::Pos => Ok(value),
            | UnOp::Neg => {
                // §8.14: negating a measurement negates its value; the
                // uncertainty is a magnitude and stays positive.
                if let Some((value, uncertainty)) = measurement_parts(&value) {
                    let negated = self.unary(UnOp::Neg, Value::Num(value), span)?;
                    let Value::Num(negated) = negated else {
                        return Err(EvalError::new(
                            "E0809",
                            "the negation of a measurement value is not a scalar",
                        )
                        .at(span));
                    };
                    return Ok(measurement_record(negated, uncertainty));
                }
                match value {
                    | Value::Num(mut num) => {
                        num.kind = match num.kind {
                            | NumKind::Int(value) => NumKind::Int(-value),
                            | NumKind::Exact(value) => NumKind::Exact(-value),
                            | NumKind::Float(value) => NumKind::Float(-value),
                            | NumKind::Sym(term) => {
                                NumKind::Sym(Rc::new(crate::sym::Term::neg((*term).clone())))
                            },
                            | NumKind::Complex(re, im) => NumKind::Complex(-re, -im),
                        };
                        Ok(Value::Num(num))
                    },
                    | Value::Tensor(tensor) => {
                        let elements = tensor
                            .elements()
                            .into_iter()
                            .map(|element| self.unary(UnOp::Neg, element, span))
                            .collect::<Result<Vec<_>, _>>()?;
                        Ok(build_tensor(
                            elements,
                            tensor.shape.clone(),
                            tensor.dim.clone(),
                        ))
                    },
                    // §4.9.3: unary `-` preserves the dimension of a `Dyn` and
                    // negates its payload (the dimension lives beside the value).
                    | Value::Dyn(dynamic) => {
                        let payload = self.unary(UnOp::Neg, Value::Num(dynamic.payload()), span)?;
                        let Value::Num(mut payload) = payload else {
                            return Err(EvalError::new(
                                "E0809",
                                "the negation of a Dyn payload is not a scalar",
                            )
                            .at(span));
                        };
                        payload.dim = Dim::one();
                        Ok(Value::Dyn(Rc::new(DynValue {
                            value: payload,
                            dim: dynamic.dim.clone(),
                            display: dynamic.display.clone(),
                        })))
                    },
                    | other => {
                        Err(EvalError::new(
                            "E0302",
                            format!("`-` is not defined for {}", other.type_name()),
                        )
                        .at(span))
                    },
                }
            },
            | UnOp::Not => {
                match value {
                    | Value::Bool(value) => Ok(Value::Bool(!value)),
                    | Value::Prop(prop) => {
                        Ok(Value::Prop(Rc::new(PropValue {
                            text: format!("!({})", prop.text),
                            comparison: None,
                        })))
                    },
                    | other => {
                        Err(EvalError::new(
                            "E0302",
                            format!("`!` expects Bool, found {}", other.type_name()),
                        )
                        .at(span))
                    },
                }
            },
            | UnOp::Sqrt => self.call_named("sqrt", vec![value], span),
        }
    }

    pub(crate) fn binary(
        &mut self,
        op: BinOp,
        left: Value,
        right: Value,
        span: Span,
    ) -> Result<Value, EvalError> {
        // Plain `Int`/`Float` scalars are by far the commonest operands. The
        // general path below turns every `Int` into a `BigRational` (two
        // allocations and a gcd) just to add one to it; this answers the same
        // question directly. It declines anything it is not certain about
        // (dimensions, display units, other representations, division, powers)
        // and leaves those to the general path.
        if let (Value::Num(a), Value::Num(b)) = (&left, &right) {
            if let (Some(a), Some(b)) = (Scalar::of(a), Scalar::of(b)) {
                if let Some(result) = plain_scalar_binary(op, a, b) {
                    return Ok(result);
                }
            }
        }
        // §8.14: measurement arithmetic is the library type's own operator set,
        // with independent-error propagation. It is handled before the numeric
        // cases because both operands are records, not scalars.
        if let Some(result) = self.measurement_binary(op, &left, &right, span)? {
            return Ok(result);
        }
        match op {
            | BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow => {
                self.arithmetic(op, left, right, span)
            },
            | BinOp::MatMul => self.matmul(left, right, span),
            | BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                self.compare(op, left, right, span)
            },
            | BinOp::And | BinOp::Or => {
                match (&left, &right) {
                    | (Value::Bool(a), Value::Bool(b)) => {
                        Ok(Value::Bool(if op == BinOp::And {
                            *a && *b
                        } else {
                            *a || *b
                        }))
                    },
                    | (Value::Prop(a), Value::Prop(b)) => {
                        Ok(Value::Prop(Rc::new(PropValue {
                            text: format!(
                                "({}) {} ({})",
                                a.text,
                                if op == BinOp::And {
                                    "&&"
                                } else {
                                    "||"
                                },
                                b.text
                            ),
                            comparison: None,
                        })))
                    },
                    | _ => Err(EvalError::new("E0302", "`&&`/`||` expect Bool operands").at(span)),
                }
            },
            | BinOp::Cons => {
                let mut elements = vec![left];
                match right {
                    | Value::Tensor(tensor) => elements.extend(tensor.elements()),
                    | other => elements.push(other),
                }
                let len = elements.len();
                let dim = value::value_dim(&elements[0]);
                Ok(build_tensor(elements, vec![len], dim))
            },
            | BinOp::Pipe | BinOp::PipeBack => {
                Err(EvalError::new("E0809", "pipes must be desugared").at(span))
            },
        }
    }

    /// §8.14 measurement arithmetic: independent-error propagation.
    ///
    /// Returns `None` when neither operand is a measurement, so the ordinary
    /// numeric cases stay in charge. `+`/`-` need one dimension (`E0401`);
    /// products and quotients combine their dimensions through the ordinary
    /// scalar operators, and the uncertainty follows the first-order
    /// quadrature rule the spec states ("平方和开根").
    fn measurement_binary(
        &mut self,
        op: BinOp,
        left: &Value,
        right: &Value,
        span: Span,
    ) -> Result<Option<Value>, EvalError> {
        let left_measurement = measurement_parts(left);
        let right_measurement = measurement_parts(right);
        match (left_measurement, right_measurement) {
            | (Some((left_value, left_uncertainty)), Some((right_value, right_uncertainty))) => {
                match op {
                    | BinOp::Add | BinOp::Sub => {
                        if left_value.dim != right_value.dim {
                            return Err(EvalError::new(
                                "E0401",
                                "measurement `+`/`-` need the same dimension",
                            )
                            .at(span));
                        }
                        let value =
                            self.binary(op, Value::Num(left_value), Value::Num(right_value), span)?;
                        let uncertainty = self.quadrature(
                            Value::Num(left_uncertainty),
                            Value::Num(right_uncertainty),
                            span,
                        )?;
                        self.finish_measurement(value, uncertainty, span)
                    },
                    | BinOp::Mul => {
                        let product = self.binary(
                            BinOp::Mul,
                            Value::Num(left_value.clone()),
                            Value::Num(right_value.clone()),
                            span,
                        )?;
                        let first = self.binary(
                            BinOp::Mul,
                            Value::Num(left_uncertainty),
                            Value::Num(right_value),
                            span,
                        )?;
                        let second = self.binary(
                            BinOp::Mul,
                            Value::Num(right_uncertainty),
                            Value::Num(left_value),
                            span,
                        )?;
                        let uncertainty = self.quadrature(first, second, span)?;
                        self.finish_measurement(product, uncertainty, span)
                    },
                    | BinOp::Div => {
                        let quotient = self.binary(
                            BinOp::Div,
                            Value::Num(left_value.clone()),
                            Value::Num(right_value.clone()),
                            span,
                        )?;
                        let first = self.binary(
                            BinOp::Div,
                            Value::Num(left_uncertainty),
                            Value::Num(right_value.clone()),
                            span,
                        )?;
                        let square = self.binary(
                            BinOp::Mul,
                            Value::Num(right_value.clone()),
                            Value::Num(right_value),
                            span,
                        )?;
                        let numerator = self.binary(
                            BinOp::Mul,
                            Value::Num(right_uncertainty),
                            Value::Num(left_value),
                            span,
                        )?;
                        let second = self.binary(BinOp::Div, numerator, square, span)?;
                        let uncertainty = self.quadrature(first, second, span)?;
                        self.finish_measurement(quotient, uncertainty, span)
                    },
                    | _ => Ok(None),
                }
            },
            // A scalar scales a measurement; `measurement + scalar` is not a
            // declared operator, so it stays an `E0302` from the normal path.
            | (Some((measurement_value, measurement_uncertainty)), None) => {
                let Value::Num(scalar) = right.clone() else {
                    return Ok(None);
                };
                match op {
                    | BinOp::Mul | BinOp::Div => {
                        let value = self.binary(
                            op,
                            Value::Num(measurement_value),
                            Value::Num(scalar.clone()),
                            span,
                        )?;
                        let magnitude = self.call_named("abs", vec![Value::Num(scalar)], span)?;
                        let uncertainty =
                            self.binary(op, Value::Num(measurement_uncertainty), magnitude, span)?;
                        self.finish_measurement(value, uncertainty, span)
                    },
                    | _ => Ok(None),
                }
            },
            | (None, Some((measurement_value, measurement_uncertainty))) => {
                let Value::Num(scalar) = left.clone() else {
                    return Ok(None);
                };
                if op != BinOp::Mul {
                    return Ok(None);
                }
                let value = self.binary(
                    BinOp::Mul,
                    Value::Num(scalar.clone()),
                    Value::Num(measurement_value),
                    span,
                )?;
                let magnitude = self.call_named("abs", vec![Value::Num(scalar)], span)?;
                let uncertainty = self.binary(
                    BinOp::Mul,
                    Value::Num(measurement_uncertainty),
                    magnitude,
                    span,
                )?;
                self.finish_measurement(value, uncertainty, span)
            },
            | (None, None) => Ok(None),
        }
    }

    /// `sqrt(a² + b²)`, the independent-error combination (§8.14).
    fn quadrature(
        &mut self,
        first: Value,
        second: Value,
        span: Span,
    ) -> Result<Value, EvalError> {
        let first_squared = self.binary(BinOp::Mul, first.clone(), first, span)?;
        let second_squared = self.binary(BinOp::Mul, second.clone(), second, span)?;
        let sum = self.binary(BinOp::Add, first_squared, second_squared, span)?;
        self.call_named("sqrt", vec![sum], span)
    }

    fn finish_measurement(
        &mut self,
        value: Value,
        uncertainty: Value,
        span: Span,
    ) -> Result<Option<Value>, EvalError> {
        let (Value::Num(value), Value::Num(uncertainty)) = (value, uncertainty) else {
            return Err(
                EvalError::new("E0809", "measurement arithmetic produced a non-scalar").at(span),
            );
        };
        Ok(Some(measurement_record(value, uncertainty)))
    }

    fn arithmetic(
        &mut self,
        op: BinOp,
        left: Value,
        right: Value,
        span: Span,
    ) -> Result<Value, EvalError> {
        match (&left, &right) {
            | (Value::Tensor(a), Value::Tensor(b)) => {
                self.tensor_arithmetic(op, a.as_ref(), b.as_ref(), span)
            },
            | (Value::Tensor(tensor), Value::Num(num))
            | (Value::Num(num), Value::Tensor(tensor)) => {
                let flipped = matches!(left, Value::Num(_));
                // §9.13: a dense buffer of floats and a float scalar stay in
                // the buffer. Anything that would promote to `Real` falls
                // through to the boxed loop.
                if let Some(value) = tensor.dense_scalar(op, num, flipped) {
                    return Ok(value);
                }
                let tensor = tensor.clone();
                let num = num.clone();
                let mut elements = Vec::with_capacity(tensor.storage.len());
                for element in tensor.elements() {
                    let Value::Num(element) = element else {
                        return Err(EvalError::new(
                            "E0302",
                            "tensor elements must be scalars here",
                        )
                        .at(span));
                    };
                    let result = if flipped {
                        self.scalar_arithmetic(op, num.clone(), element, span)?
                    } else {
                        self.scalar_arithmetic(op, element, num.clone(), span)?
                    };
                    elements.push(Value::Num(result));
                }
                let dim = elements
                    .first()
                    .map_or_else(|| tensor.dim.clone(), value::value_dim);
                Ok(build_tensor(elements, tensor.shape.clone(), dim))
            },
            | (Value::Num(a), Value::Num(b)) => {
                let mut result = self.scalar_arithmetic(op, a.clone(), b.clone(), span)?;
                if result.display.is_none() {
                    result.display = propagated_display(op, a, b);
                }
                Ok(Value::Num(result))
            },
            | (Value::UnitOf(a), Value::UnitOf(b)) => {
                match op {
                    | BinOp::Mul => Ok(Value::UnitOf(Rc::new(combine_units(a, b, true)))),
                    | BinOp::Div => Ok(Value::UnitOf(Rc::new(combine_units(a, b, false)))),
                    | _ => Err(EvalError::new("E0302", "units support `*` and `/`").at(span)),
                }
            },
            // A unit value is its scale times its dimension (§5.2): `3 * km`
            // is 3000 coherent metres displayed in km, `0.02 / m` is 0.02 per
            // metre, `km / 2` is 500 m. Only an affine unit has no product.
            | (Value::UnitOf(unit), Value::Num(num)) => {
                match op {
                    | BinOp::Mul => self.scale_by_unit(num, unit, true, span),
                    | BinOp::Div => {
                        let scale = unit_scale_value(unit, matches!(num.kind, NumKind::Float(_)));
                        let mut result =
                            self.scalar_arithmetic(BinOp::Div, scale, num.clone(), span)?;
                        result.dim = unit.dim.div(&num.dim);
                        result.display = None;
                        Ok(Value::Num(result))
                    },
                    | _ => Err(EvalError::new("E0302", "invalid unit arithmetic").at(span)),
                }
            },
            | (Value::Num(num), Value::UnitOf(unit)) if matches!(op, BinOp::Mul | BinOp::Div) => {
                self.scale_by_unit(num, unit, op == BinOp::Mul, span)
            },
            // §3.5.3 / §4.5.2: a unit scales a tensor elementwise. The
            // coefficient of every element is unchanged — only the element
            // dimension and the display metadata move — so the buffer is
            // shared. `Float` stays `Float`, `Real` stays `Real`.
            | (Value::Tensor(tensor), Value::UnitOf(unit)) => {
                self.tensor_with_unit(op, tensor, unit, false, span)
            },
            | (Value::UnitOf(unit), Value::Tensor(tensor)) => {
                self.tensor_with_unit(op, tensor, unit, true, span)
            },
            // §4.9.3: the `Dyn` escape layer's arithmetic. Dimensions are
            // checked/combined at run time because they are data there.
            | (Value::Dyn(left), Value::Dyn(right)) => self.dyn_arithmetic(op, left, right, span),
            | (Value::Dyn(dynamic), Value::Num(exponent)) if op == BinOp::Pow => {
                self.dyn_pow(dynamic, exponent, span)
            },
            | _ => {
                Err(EvalError::new(
                    "E0302",
                    format!(
                        "`{}` is not defined for {} and {}",
                        op.as_str(),
                        left.type_name(),
                        right.type_name()
                    ),
                )
                .at(span))
            },
        }
    }

    /// `base ^ exponent` where the exponent is a symbolic term.
    ///
    /// The local term language only has rational exponents, so the power is
    /// built as a kernel term; differentiation, substitution, display and
    /// numeric evaluation of it then go through the kernel (`Term::Foreign`).
    fn symbolic_power(
        &mut self,
        base: &NumVal,
        exponent: &NumVal,
        span: Span,
    ) -> Result<NumVal, EvalError> {
        if !base.dim.is_one() || !exponent.dim.is_one() {
            return Err(EvalError::new(
                "E0404",
                "a symbolic exponent needs a dimensionless base and exponent",
            )
            .at(span)
            .with_note("divide by a unit first, or use `pow_dyn` for a checked dynamic power"));
        }
        let a = self.num_to_term(base);
        let b = self.num_to_term(exponent);
        // §6.3.4: an infinite exponent (`0 ^ oo`, `oo ^ oo`) is not a power the
        // exact phase defines; it needs a finite rational exponent.
        if crate::sym::contains_infinity(&b) {
            return Err(EvalError::new("E0402", "exponent must be finite").at(span));
        }
        // `1^x = 1` and `e^x = exp(x)`: the two identities the local layer can
        // decide without the kernel, and the second keeps `diff` local.
        if a == crate::sym::Term::Num(BigRational::one()) {
            return Ok(NumVal::exact(BigRational::one(), Dim::one()));
        }
        if a == crate::sym::Term::Sym("e".to_string()) {
            return Ok(NumVal::symbolic(
                crate::sym::Term::fun(crate::sym::Fun::Exp, vec![b]),
                Dim::one(),
            ));
        }
        let request = aevia_rssn::Expr::App(
            "pow".to_string(),
            vec![crate::kernel::to_expr(&a), crate::kernel::to_expr(&b)],
        );
        let built = aevia_rssn::kernel()
            .build(&request)
            .map_err(|failure| crate::kernel::error(&failure).at(span))?;
        let term = crate::kernel::term_of(&built).ok_or_else(|| {
            EvalError::new("E1003", "the kernel built a power with no term form").at(span)
        })?;
        Ok(NumVal::symbolic(term, Dim::one()))
    }

    pub(crate) fn scalar_arithmetic(
        &mut self,
        op: BinOp,
        left: NumVal,
        right: NumVal,
        span: Span,
    ) -> Result<NumVal, EvalError> {
        // A symbolic exponent (`2^x`, `x^x`, `(1 + 1/x)^x`) has no rational
        // value, so the result's dimension cannot be computed from it: it is
        // only defined for a dimensionless base and exponent (§4.6), and the
        // term is built by the kernel, which owns general powers.
        if op == BinOp::Pow && right.is_symbolic() && right.to_rational().is_none() {
            return self.symbolic_power(&left, &right, span);
        }
        let dim = match op {
            | BinOp::Add | BinOp::Sub => left.dim.clone(),
            | BinOp::Mul => left.dim.mul(&right.dim),
            | BinOp::Div => left.dim.div(&right.dim),
            | BinOp::Pow => {
                let exponent = right
                    .to_rational()
                    .ok_or_else(|| EvalError::new("E0402", "exponent must be finite").at(span))?;
                left.dim.pow(&exponent)
            },
            | _ => left.dim.clone(),
        };
        // Symbolic if either side is symbolic.
        if left.is_symbolic() || right.is_symbolic() {
            let a = self.num_to_term(&left);
            let b = self.num_to_term(&right);
            // The exact/Real path is *partial* on the extended reals: an
            // undefined form such as `oo * 0`, `oo / oo` or `oo ^ 0` must not
            // collapse to a finite answer. Such a collapse is recognised by the
            // result being a plain number while an infinity occurred; the
            // expression is then kept, readable, as a symbolic term (§6.3.4).
            let has_infinity =
                crate::sym::contains_infinity(&a) || crate::sym::contains_infinity(&b);
            let power = if op == BinOp::Pow {
                Some(
                    right
                        .to_rational()
                        .ok_or_else(|| EvalError::new("E0402", "invalid exponent").at(span))?,
                )
            } else {
                None
            };
            let term = match op {
                | BinOp::Add => crate::sym::Term::add(a.clone(), b.clone()),
                | BinOp::Sub => crate::sym::Term::sub(a.clone(), b.clone()),
                | BinOp::Mul => crate::sym::Term::mul(a.clone(), b.clone()),
                | BinOp::Div => {
                    let inverse = crate::sym::Term::pow(
                        b.clone(),
                        BigRational::from_integer(BigInt::from(-1)),
                    );
                    crate::sym::Term::mul(a.clone(), inverse)
                },
                | BinOp::Pow => {
                    let exponent = power.clone().expect("computed above");
                    crate::sym::Term::pow(a.clone(), exponent)
                },
                | _ => return Err(EvalError::new("E0809", "unexpected operator").at(span)),
            };
            if let crate::sym::Term::Num(value) = &term {
                if has_infinity {
                    let unevaluated = match op {
                        | BinOp::Add => crate::sym::Term::Add(vec![a, b]),
                        | BinOp::Sub => crate::sym::Term::Add(vec![a, crate::sym::Term::neg(b)]),
                        | BinOp::Mul => crate::sym::Term::Mul(vec![a, b]),
                        | BinOp::Div => {
                            crate::sym::Term::Mul(vec![
                                a,
                                crate::sym::Term::pow(
                                    b,
                                    BigRational::from_integer(BigInt::from(-1)),
                                ),
                            ])
                        },
                        | BinOp::Pow => {
                            crate::sym::Term::Pow(Box::new(a), power.expect("computed above"))
                        },
                        | _ => crate::sym::Term::Num(value.clone()),
                    };
                    return Ok(NumVal::symbolic(unevaluated, dim));
                }
                // §6.3.2: a symbolic operation that collapses to a rational is
                // that rational. `x - x` is exactly `0`, not a symbolic zero that
                // cannot be compared.
                return Ok(NumVal::exact(value.clone(), dim));
            }
            return Ok(NumVal::symbolic(term, dim));
        }
        // Complex arithmetic.
        if matches!(left.kind, NumKind::Complex(..)) || matches!(right.kind, NumKind::Complex(..)) {
            let (ar, ai) = to_complex(&left);
            let (br, bi) = to_complex(&right);
            let (re, im) = match op {
                | BinOp::Add => (ar + br, ai + bi),
                | BinOp::Sub => (ar - br, ai - bi),
                | BinOp::Mul => (ar * br - ai * bi, ar * bi + ai * br),
                | BinOp::Div => {
                    let denominator = br * br + bi * bi;
                    (
                        (ar * br + ai * bi) / denominator,
                        (ai * br - ar * bi) / denominator,
                    )
                },
                | BinOp::Pow => {
                    let value = (ar, ai);
                    let exponent = br;
                    let magnitude = value.0.hypot(value.1).powf(exponent);
                    let angle = value.1.atan2(value.0) * exponent;
                    (magnitude * angle.cos(), magnitude * angle.sin())
                },
                | _ => return Err(EvalError::new("E0809", "unexpected operator").at(span)),
            };
            return Ok(NumVal {
                kind: NumKind::Complex(re, im),
                dim,
                display: None,
            });
        }
        // §3.4.1 / §6.3.2: Int is below Float in the representation
        // lattice. A materialized integer therefore promotes on this path.
        // Real (Exact or Sym) remains distinct: mixing it with Float takes
        // the exact Real path below. Literal-only `1.0 + 2` still reads both
        // literals as Real under its default source arithmetic rule.
        let float_operation = matches!(
            (&left.kind, &right.kind),
            (NumKind::Float(_), NumKind::Float(_) | NumKind::Int(_))
                | (NumKind::Int(_), NumKind::Float(_))
        );
        if float_operation {
            let a = left.to_f64().unwrap_or(f64::NAN);
            let b = right.to_f64().unwrap_or(f64::NAN);
            let value = match op {
                | BinOp::Add => a + b,
                | BinOp::Sub => a - b,
                | BinOp::Mul => a * b,
                | BinOp::Div => a / b,
                | BinOp::Pow => a.powf(b),
                | _ => f64::NAN,
            };
            return Ok(NumVal::float(value, dim));
        }
        // The Real path: every operand is exact (a `Float` is the exact
        // rational it denotes), so an operation that is closed on the
        // rationals answers exactly, and one that is not answers symbolically.
        let both_int =
            matches!(left.kind, NumKind::Int(_)) && matches!(right.kind, NumKind::Int(_));
        let (Some(a), Some(b)) = (left.to_rational(), right.to_rational()) else {
            return Err(EvalError::new(
                "E0302",
                format!(
                    "`{}` is not defined for {} and {}",
                    op.as_str(),
                    Value::Num(left.clone()).type_name(),
                    Value::Num(right.clone()).type_name()
                ),
            )
            .at(span));
        };
        let value = match op {
            | BinOp::Add => Some(&a + &b),
            | BinOp::Sub => Some(&a - &b),
            | BinOp::Mul => Some(&a * &b),
            | BinOp::Div => {
                if b.is_zero() {
                    return Err(EvalError::new("E0801", "division by zero").at(span));
                }
                Some(&a / &b)
            },
            | BinOp::Pow => {
                if a.is_zero() && b < BigRational::zero() {
                    return Err(EvalError::new("E0801", "division by zero").at(span));
                }
                if b.denom().is_one() {
                    b.numer()
                        .to_i32()
                        .map(|power| aevia_dim::rat::pow_int(&a, power))
                } else {
                    None
                }
            },
            | _ => None,
        };
        if let Some(value) = value {
            // Non-dividing integer arithmetic stays Int. Division and
            // negative powers promise Real even when this particular exact
            // result happens to have denominator one.
            if both_int
                && op != BinOp::Div
                && !(op == BinOp::Pow && b.is_negative())
                && value.denom().is_one()
            {
                return Ok(NumVal {
                    kind: NumKind::Int(value.numer().clone()),
                    dim,
                    display: None,
                });
            }
            return Ok(NumVal::exact(value, dim));
        }
        // `2^(1/2)` is not a rational; §6.3.1 makes it the symbolic `sqrt(2)`
        // rather than a float that only looks like the answer.
        if op == BinOp::Pow {
            let term = crate::sym::Term::pow(crate::sym::Term::Num(a), b);
            return Ok(NumVal::symbolic(term, dim));
        }
        Err(EvalError::new("E0809", "unexpected operator").at(span))
    }

    /// The element dimension and display unit of a tensor scaled by a unit.
    ///
    /// §3.5.3 / §4.5.2: `A * u` multiplies every element's dimension by the
    /// unit's, `A / u` divides it. The element *representation* and every
    /// coefficient are unchanged — this is metadata, not an arithmetic pass —
    /// so the storage (and therefore the buffer) is shared.
    ///
    /// §5.4.2's display rule follows the scalar `x * u` behaviour in this
    /// runtime: the unit becomes the display of the result, so `(A * km)` shows
    /// its elements in kilometres and `to`/`value_in` convert from there. A
    /// division has no single unit that displays `d/u`, so its display is
    /// dropped and the result prints in SI.
    fn tensor_with_unit(
        &mut self,
        op: BinOp,
        tensor: &Rc<TensorData>,
        unit: &Rc<UnitDef>,
        unit_on_left: bool,
        span: Span,
    ) -> Result<Value, EvalError> {
        let dim = match op {
            | BinOp::Mul => tensor.dim.mul(&unit.dim),
            | BinOp::Div if !unit_on_left => tensor.dim.div(&unit.dim),
            | _ => {
                return Err(EvalError::new(
                    "E0302",
                    format!(
                        "`{}` is not defined between a tensor and a unit",
                        op.as_str()
                    ),
                )
                .at(span));
            },
        };
        let display = if op == BinOp::Mul {
            Some(unit.clone())
        } else {
            None
        };
        // A unit of scale one (`m`, `ohm`, `A`) leaves every coefficient as
        // it is, so the buffer is shared. Any other scale multiplies (or
        // divides) each element, in the element's own phase.
        if unit.affine || unit.scale.is_one() {
            return Ok(Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape: tensor.shape.clone(),
                storage: tensor.storage.clone(),
                dim,
                display,
            })));
        }
        let float = tensor.storage.is_dense()
            || tensor.elements().first().is_some_and(|element| {
                matches!(
                    element,
                    Value::Num(NumVal {
                        kind: NumKind::Float(_),
                        ..
                    })
                )
            });
        let scale = Value::Num(unit_scale_value(unit, float));
        let scaled = self.binary(op, Value::Tensor(tensor.clone()), scale, span)?;
        let Value::Tensor(scaled) = scaled else {
            return Err(
                EvalError::new("E0809", "scaling a tensor by a unit gave a non-tensor").at(span),
            );
        };
        let mut data = (*scaled).clone();
        data.dim = dim;
        data.display = display;
        data.verified_element = None;
        Ok(Value::Tensor(Rc::new(data)))
    }

    /// `num * unit` (`multiply`) or `num / unit`: the coefficient scales by the
    /// unit's exact scale and the dimension combines with the unit's.
    fn scale_by_unit(
        &mut self,
        num: &NumVal,
        unit: &Rc<UnitDef>,
        multiply: bool,
        span: Span,
    ) -> Result<Value, EvalError> {
        let mut result = if unit.affine || unit.scale.is_one() {
            num.clone()
        } else {
            let scale = unit_scale_value(unit, matches!(num.kind, NumKind::Float(_)));
            let op = if multiply {
                BinOp::Mul
            } else {
                BinOp::Div
            };
            self.scalar_arithmetic(op, num.clone(), scale, span)?
        };
        if multiply {
            result.dim = num.dim.mul(&unit.dim);
            result.display = Some(unit.clone());
        } else {
            result.dim = num.dim.div(&unit.dim);
            result.display = None;
        }
        Ok(Value::Num(result))
    }

    /// `Dyn ⊕ Dyn` for `+`, `-`, `*` and `/` (§4.9.3).
    ///
    /// `+`/`-` require equal run-time dimensions; a mismatch is `E0412`,
    /// the fatal `Dyn` dimension check (the recoverable variants are
    /// `try_add`/`try_mul`, which this build does not have yet — the error is
    /// therefore reported, never silently accepted). `*` adds the dimensions
    /// and `/` subtracts them; the payloads are the dimensionless parts, so the
    /// ordinary scalar arithmetic gives the coefficient.
    fn dyn_arithmetic(
        &mut self,
        op: BinOp,
        left: &Rc<DynValue>,
        right: &Rc<DynValue>,
        span: Span,
    ) -> Result<Value, EvalError> {
        let dim = match op {
            | BinOp::Add | BinOp::Sub => {
                if left.dim != right.dim {
                    return Err(EvalError::new(
                        "E0412",
                        format!(
                            "Dyn dimension check failed: `{}` and `{}`",
                            left.dim.to_plain_string(),
                            right.dim.to_plain_string()
                        ),
                    )
                    .at(span)
                    .with_note(
                        "`+` and `-` on Dyn require equal runtime dimensions (§4.9.3)".to_string(),
                    ));
                }
                left.dim.clone()
            },
            | BinOp::Mul => left.dim.mul(&right.dim),
            | BinOp::Div => left.dim.div(&right.dim),
            | _ => {
                return Err(EvalError::new(
                    "E0302",
                    format!("`{}` is not defined on Dyn", op.as_str()),
                )
                .at(span));
            },
        };
        let payload = self.scalar_arithmetic(op, left.payload(), right.payload(), span)?;
        Ok(Value::Dyn(Rc::new(DynValue {
            value: payload,
            dim,
            display: None,
        })))
    }

    /// `Dyn ^ Int` (§4.9.3): the dimension exponent is multiplied by the
    /// integer, the payload is raised to it.
    fn dyn_pow(
        &mut self,
        dynamic: &Rc<DynValue>,
        exponent: &NumVal,
        span: Span,
    ) -> Result<Value, EvalError> {
        let exponent = exponent.to_rational().ok_or_else(|| {
            EvalError::new("E0302", "`Dyn ^ n` needs an Int exponent (§4.9.3)").at(span)
        })?;
        if !exponent.denom().is_one() {
            return Err(EvalError::new(
                "E0302",
                "`Dyn ^ n` needs an Int exponent (§4.9.3); a fractional exponent \
                 needs `pow_dyn` with an explicit dimension",
            )
            .at(span));
        }
        let integer = NumVal::int(exponent.numer().clone());
        let payload = self.scalar_arithmetic(BinOp::Pow, dynamic.payload(), integer, span)?;
        Ok(Value::Dyn(Rc::new(DynValue {
            value: payload,
            dim: dynamic.dim.pow(&exponent),
            display: None,
        })))
    }

    fn num_to_term(
        &self,
        num: &NumVal,
    ) -> crate::sym::Term {
        match &num.kind {
            | NumKind::Int(value) => {
                crate::sym::Term::Num(BigRational::from_integer(value.clone()))
            },
            | NumKind::Exact(value) => crate::sym::Term::Num(value.clone()),
            | NumKind::Float(value) => {
                crate::sym::Term::Num(
                    BigRational::from_float(*value).unwrap_or_else(BigRational::zero),
                )
            },
            | NumKind::Sym(term) => (**term).clone(),
            | NumKind::Complex(re, _) => {
                crate::sym::Term::Num(
                    BigRational::from_float(*re).unwrap_or_else(BigRational::zero),
                )
            },
        }
    }

    fn tensor_arithmetic(
        &mut self,
        op: BinOp,
        left: &TensorData,
        right: &TensorData,
        span: Span,
    ) -> Result<Value, EvalError> {
        // §9.13: two dense operands of the same shape are one `f64` loop.
        // Broadcasting, mixed storage and empty buffers stay on the boxed
        // path, which is the reference for the result.
        if let Some(value) = left.dense_binary(op, right) {
            return Ok(value);
        }
        if left.shape.len() != right.shape.len() {
            return Err(EvalError::new(
                "E0802",
                format!(
                    "rank mismatch in `{}`: {:?} and {:?}",
                    op.as_str(),
                    left.shape,
                    right.shape
                ),
            )
            .at(span));
        }
        let shape = broadcast_shape(&left.shape, &right.shape).ok_or_else(|| {
            EvalError::new(
                "E0802",
                format!(
                    "shape mismatch in `{}`: {:?} and {:?}",
                    op.as_str(),
                    left.shape,
                    right.shape
                ),
            )
            .at(span)
        })?;
        let count = TensorData::element_count(&shape);
        if count == 0 && matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) {
            if let (Storage::TypedExact(_, a), Storage::TypedExact(_, b)) =
                (left.storage.backing(), right.storage.backing())
            {
                let rep = if *a == value::ExactRep::Int && *b == value::ExactRep::Int {
                    value::ExactRep::Int
                } else {
                    value::ExactRep::Real
                };
                let dim = if op == BinOp::Mul {
                    left.dim.mul(&right.dim)
                } else {
                    left.dim.clone()
                };
                return Ok(Value::Tensor(Rc::new(TensorData {
                    verified_element: None,
                    shape,
                    storage: Storage::TypedExact(Vec::new(), rep),
                    dim,
                    display: None,
                })));
            }
        }
        let mut elements = Vec::with_capacity(count);
        for index in 0..count {
            let coordinates = unravel(index, &shape);
            let a = element_at_broadcast(left, &coordinates)
                .ok_or_else(|| EvalError::new("E0802", "shape mismatch").at(span))?;
            let b = element_at_broadcast(right, &coordinates)
                .ok_or_else(|| EvalError::new("E0802", "shape mismatch").at(span))?;
            elements.push(self.arithmetic(op, a, b, span)?);
        }
        let dim = elements
            .first()
            .map_or_else(|| left.dim.clone(), value::value_dim);
        Ok(build_tensor(elements, shape, dim))
    }

    fn matmul(
        &mut self,
        left: Value,
        right: Value,
        span: Span,
    ) -> Result<Value, EvalError> {
        let (Value::Tensor(a), Value::Tensor(b)) = (&left, &right) else {
            return Err(EvalError::new("E0302", "`@` expects two tensors").at(span));
        };
        if let Some(value) = a.dense_matmul(b) {
            return Ok(value);
        }
        match (a.rank(), b.rank()) {
            | (2, 2) => {
                let (m, k) = (a.shape[0], a.shape[1]);
                let (k2, n) = (b.shape[0], b.shape[1]);
                if k != k2 {
                    return Err(EvalError::new(
                        "E0802",
                        format!(
                            "shape mismatch in `matmul`: left shape {:?}, right shape {:?}",
                            a.shape, b.shape
                        ),
                    )
                    .at(span)
                    .with_note("matmul requires left columns = right rows"));
                }
                let mut elements = Vec::with_capacity(m * n);
                for row in 0..m {
                    for column in 0..n {
                        let mut sum: Option<Value> = None;
                        for inner in 0..k {
                            let x = a
                                .get(&[row as isize, inner as isize])
                                .ok_or_else(|| EvalError::new("E0802", "index out of range"))?;
                            let y = b
                                .get(&[inner as isize, column as isize])
                                .ok_or_else(|| EvalError::new("E0802", "index out of range"))?;
                            let product = self.arithmetic(BinOp::Mul, x, y, span)?;
                            sum = Some(match sum {
                                | None => product,
                                | Some(acc) => self.arithmetic(BinOp::Add, acc, product, span)?,
                            });
                        }
                        elements.push(sum.unwrap_or_else(|| Value::float(0.0)));
                    }
                }
                let dim = elements
                    .first()
                    .map_or_else(|| a.dim.mul(&b.dim), value::value_dim);
                Ok(build_tensor(elements, vec![m, n], dim))
            },
            | (2, 1) => {
                let (m, k) = (a.shape[0], a.shape[1]);
                if k != b.shape[0] {
                    return Err(EvalError::new(
                        "E0802",
                        format!(
                            "shape mismatch in `matvec`: left shape {:?}, right shape {:?}",
                            a.shape, b.shape
                        ),
                    )
                    .at(span));
                }
                let mut elements = Vec::with_capacity(m);
                for row in 0..m {
                    let mut sum: Option<Value> = None;
                    for inner in 0..k {
                        let x = a
                            .get(&[row as isize, inner as isize])
                            .unwrap_or(Value::Unit);
                        let y = b.get(&[inner as isize]).unwrap_or(Value::Unit);
                        let product = self.arithmetic(BinOp::Mul, x, y, span)?;
                        sum = Some(match sum {
                            | None => product,
                            | Some(acc) => self.arithmetic(BinOp::Add, acc, product, span)?,
                        });
                    }
                    elements.push(sum.unwrap_or_else(|| Value::float(0.0)));
                }
                let dim = elements
                    .first()
                    .map_or_else(|| a.dim.mul(&b.dim), value::value_dim);
                Ok(build_tensor(elements, vec![m], dim))
            },
            | (1, 2) => {
                let k = a.shape[0];
                let (k2, n) = (b.shape[0], b.shape[1]);
                if k != k2 {
                    return Err(EvalError::new("E0802", "shape mismatch in `vecmat`").at(span));
                }
                let mut elements = Vec::with_capacity(n);
                for column in 0..n {
                    let mut sum: Option<Value> = None;
                    for inner in 0..k {
                        let x = a.get(&[inner as isize]).unwrap_or(Value::Unit);
                        let y = b
                            .get(&[inner as isize, column as isize])
                            .unwrap_or(Value::Unit);
                        let product = self.arithmetic(BinOp::Mul, x, y, span)?;
                        sum = Some(match sum {
                            | None => product,
                            | Some(acc) => self.arithmetic(BinOp::Add, acc, product, span)?,
                        });
                    }
                    elements.push(sum.unwrap_or_else(|| Value::float(0.0)));
                }
                let dim = elements
                    .first()
                    .map_or_else(|| a.dim.mul(&b.dim), value::value_dim);
                Ok(build_tensor(elements, vec![n], dim))
            },
            | (1, 1) => {
                if a.shape[0] != b.shape[0] {
                    return Err(EvalError::new("E0802", "shape mismatch in `dot`").at(span));
                }
                let mut sum: Option<Value> = None;
                for index in 0..a.shape[0] {
                    let x = a.get(&[index as isize]).unwrap_or(Value::Unit);
                    let y = b.get(&[index as isize]).unwrap_or(Value::Unit);
                    let product = self.arithmetic(BinOp::Mul, x, y, span)?;
                    sum = Some(match sum {
                        | None => product,
                        | Some(acc) => self.arithmetic(BinOp::Add, acc, product, span)?,
                    });
                }
                Ok(sum.unwrap_or_else(|| Value::float(0.0)))
            },
            | _ => {
                Err(EvalError::new(
                    "E0802",
                    format!("`@` is not defined for ranks {} and {}", a.rank(), b.rank()),
                )
                .at(span))
            },
        }
    }

    fn compare(
        &mut self,
        op: BinOp,
        left: Value,
        right: Value,
        span: Span,
    ) -> Result<Value, EvalError> {
        match (&left, &right) {
            | (Value::Num(a), Value::Num(b)) => {
                if a.is_symbolic() || b.is_symbolic() {
                    let text = format!(
                        "{} {} {}",
                        value::show(&left),
                        op.as_str(),
                        value::show(&right)
                    );
                    return Ok(Value::Prop(Rc::new(PropValue {
                        text,
                        comparison: Some(crate::value::Comparison {
                            op,
                            left: left.clone(),
                            right: right.clone(),
                        }),
                    })));
                }
                if matches!(op, BinOp::Eq | BinOp::Ne) {
                    // §6.12.1: a concrete `Int`/`Float` compares numerically,
                    // and two exact values compare as rationals.
                    let equal = value::equal(&left, &right).unwrap_or(false);
                    return Ok(Value::Bool(if op == BinOp::Eq {
                        equal
                    } else {
                        !equal
                    }));
                }
                let both_exact = matches!(a.kind, NumKind::Int(_) | NumKind::Exact(_))
                    && matches!(b.kind, NumKind::Int(_) | NumKind::Exact(_));
                let result = if both_exact {
                    let a = a.to_rational().unwrap_or_else(BigRational::zero);
                    let b = b.to_rational().unwrap_or_else(BigRational::zero);
                    match op {
                        | BinOp::Lt => a < b,
                        | BinOp::Le => a <= b,
                        | BinOp::Gt => a > b,
                        | BinOp::Ge => a >= b,
                        | _ => false,
                    }
                } else {
                    let (a, b) = match (a.to_f64(), b.to_f64()) {
                        | (Some(a), Some(b)) => (a, b),
                        | _ => return Ok(Value::Bool(false)),
                    };
                    match op {
                        | BinOp::Lt => a < b,
                        | BinOp::Le => a <= b,
                        | BinOp::Gt => a > b,
                        | BinOp::Ge => a >= b,
                        | _ => false,
                    }
                };
                Ok(Value::Bool(result))
            },
            | (Value::Bool(a), Value::Bool(b)) => {
                Ok(Value::Bool(match op {
                    | BinOp::Eq => a == b,
                    | BinOp::Ne => a != b,
                    | _ => false,
                }))
            },
            | (Value::Str(a), Value::Str(b)) => {
                Ok(Value::Bool(match op {
                    | BinOp::Eq => a == b,
                    | BinOp::Ne => a != b,
                    | BinOp::Lt => a < b,
                    | BinOp::Le => a <= b,
                    | BinOp::Gt => a > b,
                    | BinOp::Ge => a >= b,
                    | _ => false,
                }))
            },
            | (Value::Unit, Value::Unit) => {
                Ok(Value::Bool(matches!(op, BinOp::Eq | BinOp::Le | BinOp::Ge)))
            },
            | _ if matches!(op, BinOp::Eq | BinOp::Ne) => {
                // Structural equality for variants, records and tensors
                // (§6.12.1): payloads and elements are compared, never the
                // text they print as.
                match value::equal(&left, &right) {
                    | Ok(equal) => {
                        Ok(Value::Bool(if op == BinOp::Eq {
                            equal
                        } else {
                            !equal
                        }))
                    },
                    | Err(reason) => Err(EvalError::new("E0302", reason.to_string()).at(span)),
                }
            },
            | _ => {
                Err(EvalError::new(
                    "E0302",
                    format!(
                        "cannot compare {} with {}",
                        left.type_name(),
                        right.type_name()
                    ),
                )
                .at(span))
            },
        }
    }

    // ----- calls ----------------------------------------------------------

    /// Preserve an explicitly selected builtin representation in a value.
    pub(crate) fn type_apply_value(
        &self,
        function: Value,
        types: &[aevia_syntax::ast::Ty],
        span: Span,
    ) -> Value {
        if let Value::Closure(closure) = &function {
            if let Some(proof) = &closure.proof {
                if !proof.definition.binders.is_empty() {
                    let selected =
                        self.checked_callable_selection(proof, span, true)
                            .and_then(|selected| {
                                if let Some(expected @ aevia_types::Type::Fn(..)) =
                                    self.resolved_expr_type(span)
                                {
                                    selected.specialize(&expected)
                                } else {
                                    Ok(selected)
                                }
                            });
                    return match selected {
                        | Ok(proof) => {
                            Value::Closure(crate::generics::specialize_closure(closure, proof))
                        },
                        | Err(error) => {
                            Value::Native(Rc::new(NativeFn(Box::new(move |_, _, _| {
                                Err(error.clone())
                            }))))
                        },
                    };
                }
            }
        }
        if let Some(expected) = self.resolved_expr_type(span) {
            let result = match &expected {
                | aevia_types::Type::Fn(_, result) => result.as_ref(),
                | ty => ty,
            };
            if let aevia_types::Type::Adt(expected_id, _) = result {
                if !aevia_types::class::has_variables(result) {
                    match &function {
                        | Value::Ctor {
                            adt,
                            index,
                            name,
                            origin: Some(origin),
                            ..
                        } if origin.adt == *adt
                            && origin.index == *index
                            && origin.name == *name
                            && adt::same_type(*adt, *expected_id) =>
                        {
                            if origin.selected_result.as_ref().is_none_or(|old| {
                                crate::host_witness::strict_schema_equal(old, result)
                            }) {
                                let mut selected = function.clone();
                                if let Value::Ctor {
                                    origin: Some(origin), ..
                                } = &mut selected
                                {
                                    origin.selected_result = Some(result.clone());
                                }
                                return selected;
                            }
                        },
                        | Value::Variant(variant)
                            if variant.constructor_origin
                                && adt::same_type(variant.adt, *expected_id) =>
                        {
                            if variant.verified_type.as_ref().is_some_and(|old| {
                                crate::host_witness::strict_schema_equal(old, result)
                            }) {
                                return function;
                            }
                            if variant.verified_type.is_none()
                                && variant.type_args.is_none()
                                && crate::host_witness::checked_constructor_result(
                                    &function, result,
                                )
                            {
                                let mut selected = function.clone();
                                if let Value::Variant(variant) = &mut selected {
                                    let variant = Rc::make_mut(variant);
                                    variant.verified_type = Some(result.clone());
                                    if let aevia_types::Type::Adt(_, args) = result {
                                        variant.type_args = Some(args.clone());
                                    }
                                }
                                return selected;
                            }
                        },
                        | _ => {},
                    }
                }
            }
        }
        if let Value::Ctor { adt, index, name, .. } = &function {
            if *adt == u32::MAX
                && *index == usize::MAX
                && name == "read_npy"
                && matches!(&function, Value::Ctor { origin: Some(origin), .. } if origin.adt == *adt && origin.index == *index && origin.name == *name)
            {
                let rank = match self.resolved_expr_type(span) {
                    | Some(aevia_types::Type::ReadNpyIntrinsic(Some(rank))) => Some(rank),
                    | _ => None,
                };
                return Value::Native(Rc::new(NativeFn(Box::new(move |_, args, span| {
                    let rank = rank.ok_or_else(|| {
                        EvalError::new("E0304", "missing concrete rank evidence for `read_npy`")
                            .at(span)
                    })?;
                    crate::builtins::read_npy_ranked(&args, rank, span)
                }))));
            }
            if *adt == u32::MAX && *index == usize::MAX && name == "from_value" {
                let evidence = types
                    .first()
                    .and_then(|argument| self.reified_type(argument));
                return Value::Native(Rc::new(NativeFn(Box::new(move |_interp, args, span| {
                    if args.len() != 1 {
                        return Err(EvalError::new(
                            "E0315",
                            "`from_value` takes one value argument",
                        )
                        .at(span));
                    }
                    let witness = evidence.as_ref().ok_or_else(|| {
                        EvalError::new("E0304", "missing concrete type evidence for `from_value`")
                            .at(span)
                    })?;
                    Ok(
                        match crate::rssn_module::from_kernel_value(&args[0], witness) {
                            | Ok(value) => {
                                make_variant(aevia_types::adt::RESULT_ID, 0, "Ok", vec![value])
                            },
                            | Err(reason) => {
                                make_variant(
                                    aevia_types::adt::RESULT_ID,
                                    1,
                                    "Err",
                                    vec![crate::rssn_module::value_error("TypeMismatch", reason)],
                                )
                            },
                        },
                    )
                }))));
            }
            if *adt == u32::MAX && *index == usize::MAX && name == "type_rep" {
                let evidence = types
                    .first()
                    .and_then(|argument| self.reified_type(argument));
                return Value::Native(Rc::new(NativeFn(Box::new(move |_interp, args, span| {
                    if !args.is_empty() {
                        return Err(
                            EvalError::new("E0315", "`type_rep` takes no value arguments").at(span),
                        );
                    }
                    let witness = evidence.clone().ok_or_else(|| {
                        EvalError::new("E0304", "missing concrete type evidence for `type_rep`")
                            .at(span)
                    })?;
                    Ok(Value::TypeRep(Rc::new(witness)))
                }))));
            }
            if *adt == u32::MAX
                && *index == usize::MAX
                && crate::builtins::takes_type_argument(name)
            {
                let name = name.clone();
                let types = self.canonical_tensor_types(&name, types, span);
                return Value::Native(Rc::new(NativeFn(Box::new(move |interp, args, span| {
                    crate::builtins::call_typed(interp, &name, &types, args, span)
                }))));
            }
        }
        function
    }

    /// Apply at an actual checked source call. Unlike `apply`, this path may
    /// use lexical call/argument evidence; host higher-order callbacks never
    /// enter it merely by reusing an outer span.
    pub(crate) fn apply_source_call(
        &mut self,
        function: Value,
        mut args: Vec<Value>,
        span: Span,
        callee_span: Span,
        argument_spans: &[Span],
    ) -> Result<Value, EvalError> {
        let instance = self.prepare_function_call(&function, &mut args, Some(callee_span))?;
        if instance.is_some() {
            return self.apply_instantiated(function, args, span, instance);
        }
        let source_ctor = match &function {
            | Value::Ctor {
                adt,
                index,
                name,
                origin: Some(origin),
                ..
            } if *adt != u32::MAX
                && *adt == origin.adt
                && *index == origin.index
                && *name == origin.name =>
            {
                Some(origin.selected_result.clone())
            },
            | _ => None,
        };
        let builtin = match &function {
            | Value::Ctor {
                adt,
                index,
                name,
                arity,
                args,
                origin: Some(origin),
            } if *adt == u32::MAX
                && *index == usize::MAX
                && *arity == usize::MAX
                && args.is_empty()
                && origin.adt == *adt
                && origin.index == *index
                && origin.name == *name =>
            {
                Some(name.clone())
            },
            | _ => None,
        };
        let to_value = builtin.as_deref() == Some("to_value");
        let result_type = source_ctor.and_then(|selected| {
            self.resolved_expr_type(span).filter(|ty| {
                selected
                    .as_ref()
                    .is_none_or(|selected| crate::host_witness::strict_schema_equal(selected, ty))
            })
        });
        let input = if to_value && args.len() == 1 && argument_spans.len() == 1 {
            self.resolved_expr_type(argument_spans[0])
                .map(|ty| (args[0].clone(), ty))
        } else {
            None
        };
        let builtin_result = builtin
            .as_deref()
            .and_then(|name| builtin_result_schema(name, &args));
        if builtin.as_deref() == Some("to_callback") {
            if args.len() != 1 || argument_spans.len() != 1 {
                return Err(
                    EvalError::new("E0315", "`to_callback` requires one function").at(span),
                );
            }
            let expected = self.resolved_expr_type(argument_spans[0]).ok_or_else(|| {
                EvalError::new(
                    "E0304",
                    "`to_callback` requires a concrete function signature",
                )
                .at(span)
            })?;
            let function = self.reify_callable(&args[0], &expected)?;
            let schema = self
                .concrete_callable_type(&function)
                .filter(|schema| crate::host_witness::strict_schema_equal(schema, &expected))
                .ok_or_else(|| {
                    EvalError::new("E0302", "callback signature does not match its callable")
                        .at(span)
                })?;
            return Ok(Value::Callback(Rc::new(value::CallbackValue {
                function,
                schema,
            })));
        }
        if builtin.as_deref() == Some("from_value") {
            let expected = self.resolved_expr_type(span).and_then(|ty| {
                match ty {
                    | aevia_types::Type::Adt(id, args) if id == adt::RESULT_ID => {
                        args.first().cloned()
                    },
                    | _ => None,
                }
            });
            let converted = expected
                .ok_or_else(|| "missing concrete result type evidence".to_string())
                .and_then(|ty| {
                    args.first()
                        .ok_or_else(|| "missing kernel value".to_string())
                        .and_then(|value| crate::rssn_module::from_kernel_value(value, &ty))
                });
            return Ok(match converted {
                | Ok(value) => make_variant(adt::RESULT_ID, 0, "Ok", vec![value]),
                | Err(error) => {
                    make_variant(
                        adt::RESULT_ID,
                        1,
                        "Err",
                        vec![crate::rssn_module::value_error("TypeMismatch", error)],
                    )
                },
            });
        }
        let mut result = self.apply(function, args, span)?;
        if let Some(ty) = builtin_result.filter(|ty| {
            self.resolved_expr_type(span)
                .is_some_and(|expected| crate::host_witness::strict_schema_equal(ty, &expected))
        }) {
            if let Value::Variant(variant) = &mut result {
                Rc::make_mut(variant).constructor_origin = true;
            }
            if crate::host_witness::capture(&result, &ty, true).is_some() {
                if let Value::Variant(variant) = &mut result {
                    let variant = Rc::make_mut(variant);
                    variant.verified_type = Some(ty.clone());
                    if let aevia_types::Type::Adt(_, args) = ty {
                        variant.type_args = Some(args);
                    }
                }
            } else if let Value::Variant(variant) = &mut result {
                Rc::make_mut(variant).constructor_origin = false;
            }
        }
        if let Some(ty) = result_type {
            // The checker proved this constructor application, so a closed
            // result type is the variant's witness as it stands. Walking the
            // value to re-prove it made every `Cons(x, acc)` linear in `acc`
            // (and building a list quadratic). Only a type that still has
            // variables is checked against the payload.
            if crate::host_witness::checked_constructor_result(&result, &ty) {
                if let Value::Variant(variant) = &mut result {
                    let variant = Rc::make_mut(variant);
                    variant.verified_type = Some(ty.clone());
                    if let aevia_types::Type::Adt(_, args) = ty {
                        variant.type_args = Some(args);
                    }
                }
            }
        }
        if let Some((value, ty)) = input {
            if let Some(witness) = crate::host_witness::capture(&value, &ty, false) {
                if let Value::KernelValue(kernel) = &mut result {
                    Rc::make_mut(kernel).host_witness = Some(Rc::new(witness));
                }
            }
        }
        Ok(result)
    }

    /// A literal's value at `span` under the active bindings.
    ///
    /// A numeric or unit literal is a constant once its representation is
    /// chosen, and the representation is a function of the site and the
    /// active bindings only. Units are compile-time scales (§0.4): resolving
    /// `m/s^2` against the unit table, building its display unit and scaling
    /// the coefficient on every evaluation made a unit literal in a loop the
    /// most expensive thing the loop did.
    pub(crate) fn memoized_literal(
        &mut self,
        span: Span,
        compute: impl FnOnce(&mut Self) -> Result<Value, EvalError>,
    ) -> Result<Value, EvalError> {
        let key = (
            span,
            self.bindings
                .as_ref()
                .map_or(0, |bindings| Rc::as_ptr(bindings) as usize),
            Rc::as_ptr(&self.context) as usize,
        );
        if let Some((value, _, _)) = self.literal_values.borrow().get(&key) {
            return Ok(value.clone());
        }
        let value = compute(self)?;
        let mut memo = self.literal_values.borrow_mut();
        if memo.len() >= 8192 {
            memo.clear();
        }
        memo.insert(
            key,
            (value.clone(), self.bindings.clone(), self.context.clone()),
        );
        Ok(value)
    }

    /// Builds the variant of a checked, saturated constructor application.
    ///
    /// The checker proved the application, so the payload is not re-validated;
    /// the only runtime contribution is the variant's type witness, which is
    /// the checker's concrete type at this site under the active bindings and
    /// is memoized per (site, bindings, module).
    pub(crate) fn construct_checked(
        &self,
        adt: adt::AdtId,
        index: usize,
        name: &str,
        fields: Vec<Value>,
        span: Span,
    ) -> Value {
        let witness = self.constructor_witness(adt, span);
        let type_args = match &witness {
            | Some(aevia_types::Type::Adt(_, args)) => Some(args.clone()),
            | _ => None,
        };
        Value::Variant(Rc::new(value::VariantValue {
            adt,
            index,
            name: name.to_string(),
            fields,
            type_args,
            verified_type: witness,
            constructor_origin: true,
        }))
    }

    fn constructor_witness(
        &self,
        adt: adt::AdtId,
        span: Span,
    ) -> Option<aevia_types::Type> {
        let key = (
            span,
            self.bindings
                .as_ref()
                .map_or(0, |bindings| Rc::as_ptr(bindings) as usize),
            Rc::as_ptr(&self.context) as usize,
        );
        if let Some((witness, _, _)) = self.constructor_witnesses.borrow().get(&key) {
            return witness.clone();
        }
        let witness = self.resolved_expr_type(span).filter(|ty| {
            matches!(ty, aevia_types::Type::Adt(id, _) if adt::same_type(*id, adt))
                && crate::host_witness::concrete(ty)
        });
        let mut memo = self.constructor_witnesses.borrow_mut();
        if memo.len() >= 4096 {
            memo.clear();
        }
        memo.insert(
            key,
            (witness.clone(), self.bindings.clone(), self.context.clone()),
        );
        witness
    }

    /// Retrieve the concrete type recorded by the checker for an explicit
    /// type argument in this source module. Missing evidence is never guessed.
    pub(crate) fn reified_type(
        &self,
        argument: &Ty,
    ) -> Option<aevia_types::Type> {
        self.resolved_expr_type(argument.span)
    }

    /// Use the checker's resolved element representation, including through
    /// transparent type aliases, rather than reinterpreting its source spelling.
    fn canonical_tensor_types(
        &self,
        name: &str,
        types: &[Ty],
        span: Span,
    ) -> Vec<Ty> {
        if !matches!(name, "zeros" | "ones") || types.len() != 1 {
            return types.to_vec();
        }
        let rep = match self.resolved_expr_type(span) {
            | Some(aevia_types::Type::Tensor { elem, .. }) => {
                match elem.as_ref() {
                    | aevia_types::Type::Scalar(rep, _) => Some(*rep),
                    | _ => None,
                }
            },
            | Some(aevia_types::Type::TensorIntrinsic(intrinsic)) => intrinsic.element,
            | _ => None,
        };
        let kind = match rep {
            | Some(Rep::Int) => Some(TyKind::Int),
            | Some(Rep::Real) => Some(TyKind::Real(DimAst::One)),
            | Some(Rep::Float) => Some(TyKind::Float(DimAst::One)),
            | _ => None,
        };
        kind.map_or_else(|| types.to_vec(), |kind| vec![Ty::new(kind, types[0].span)])
    }

    /// Applies a function value to arguments.
    ///
    /// # Errors
    ///
    /// Reports arity mismatches (`E0315`) and builtin failures.
    pub fn apply(
        &mut self,
        function: Value,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, EvalError> {
        self.apply_instantiated(function, args, span, None)
    }

    pub(crate) fn apply_instantiated(
        &mut self,
        function: Value,
        args: Vec<Value>,
        span: Span,
        instance: Option<Rc<crate::generics::FunctionProof>>,
    ) -> Result<Value, EvalError> {
        // §9.3.2: the depth limit counts *calls*, not AST nodes. Counting nodes
        // rejected a legal flat `a + a + …` chain of 2100 terms while letting
        // an unbounded call chain through, which is the opposite of the rule.
        self.depth += 1;
        if self.depth > self.max_depth {
            self.depth -= 1;
            return Err(EvalError::new("E0805", "stack overflow")
                .at(span)
                .with_note(format!(
                    "the call depth limit is {} nested calls; tail calls do not count",
                    self.max_depth
                ))
                .with_stack(self.frames.clone()));
        }
        if let Some(budget) = stack_budget_exhausted() {
            self.depth -= 1;
            return Err(EvalError::new("E0805", "stack overflow")
                .at(span)
                .with_note(format!(
                    "the interpreter's {} MiB stack is exhausted after {} nested calls; tail calls do not count, and `--max-stack <MiB>` raises the budget",
                    budget / (1024 * 1024),
                    self.depth
                ))
                .with_stack(self.frames.clone()));
        }
        let result = self.apply_inner(function, args, span, instance);
        self.depth -= 1;
        match result {
            | Ok(value) => Ok(value),
            | Err(error) => Err(error.with_stack(self.frames.clone())),
        }
    }

    fn apply_inner(
        &mut self,
        function: Value,
        mut args: Vec<Value>,
        span: Span,
        instance: Option<Rc<crate::generics::FunctionProof>>,
    ) -> Result<Value, EvalError> {
        let instance = match instance {
            | Some(instance) => Some(instance),
            | None => self.prepare_function_call(&function, &mut args, None)?,
        };
        let prior_bindings = self.bindings.clone();
        let scope = match &function {
            | Value::Closure(closure) => {
                match &closure.env {
                    | ClosureEnv::Ast(scope) => Some(scope.clone()),
                    | ClosureEnv::Ir(env) => env.scope.clone(),
                }
            },
            | _ => None,
        };
        let previous = if let Some(scope) = scope {
            self.enter_scope(&scope)?
        } else {
            None
        };
        if let Some(instance) = &instance {
            self.bindings = Some(instance.bindings());
        } else if let Value::Closure(closure) = &function {
            self.bindings = closure
                .captured_bindings
                .clone()
                .or_else(|| self.bindings.clone());
        }
        let result = match function {
            | Value::Closure(closure) => self.apply_user_closure(closure, args, span, instance),
            | Value::Native(function) => (function.0)(self, args, span),
            | other => self.apply_in_context(other, args, span, instance),
        };
        self.restore_context(previous);
        self.bindings = prior_bindings;
        result
    }

    #[inline(never)]
    fn apply_user_closure(
        &mut self,
        closure: Rc<Closure>,
        args: Vec<Value>,
        span: Span,
        instance: Option<Rc<crate::generics::FunctionProof>>,
    ) -> Result<Value, EvalError> {
        // §9.2.2: a closure lowered to the Core IR runs on the IR
        // interpreter, which trampolines its tail calls. Everything
        // else about applying a function — the depth guard, the stack
        // frame, the `?` payload — is shared.
        if matches!(closure.body, ClosureBody::Ir(_)) {
            return crate::eval_ir::run_closure(self, &closure, args, span, instance);
        }
        let ClosureBody::Ast(body) = &closure.body else {
            unreachable!("the IR branch returned above")
        };
        let ClosureEnv::Ast(captured) = &closure.env else {
            unreachable!("an AST body has an AST environment")
        };
        if closure.params.len() != args.len() {
            return Err(EvalError::new(
                "E0315",
                format!(
                    "this function takes {} argument(s) but {} were given",
                    closure.params.len(),
                    args.len()
                ),
            )
            .at(span));
        }
        let scope = Scope::child_with_types(captured, self.bindings.clone());
        for (param, arg) in closure.params.iter().zip(args.iter()) {
            self.bind_pattern(param, arg, &scope)?;
        }
        self.frames.push(Frame {
            function: closure
                .name
                .clone()
                .unwrap_or_else(|| self.lambda_name.clone()),
            span,
        });
        // §9.3.4: the trace must name the function that failed, so the
        // error is given the call stack while its own frame is still
        // on it — popping first dropped the innermost frame.
        let outcome = self
            .eval(body, &scope)
            .map_err(|error| error.with_stack(self.frames.clone()));
        self.frames.pop();
        crate::try_control::finish_function(
            outcome,
            closure.proof.as_ref().map(|proof| proof.definition.site),
        )
    }

    fn apply_in_context(
        &mut self,
        function: Value,
        args: Vec<Value>,
        span: Span,
        instance: Option<Rc<crate::generics::FunctionProof>>,
    ) -> Result<Value, EvalError> {
        match function {
            | Value::Native(function) => (function.0)(self, args, span),
            | Value::Closure(closure) => self.apply_user_closure(closure, args, span, instance),
            | Value::Ctor {
                adt,
                index,
                name,
                arity,
                args: collected,
                origin,
            } => {
                if arity == usize::MAX {
                    // A builtin referenced as a value: merge any arguments it
                    // already collected with the new ones.
                    let mut all = collected;
                    all.extend(args);
                    return self.call_named(&name, all, span);
                }
                let mut all = collected;
                all.extend(args);
                if all.len() < arity {
                    return Ok(Value::Ctor {
                        adt,
                        index,
                        name,
                        arity,
                        args: all,
                        origin,
                    });
                }
                if all.len() > arity {
                    return Err(EvalError::new(
                        "E0307",
                        format!("constructor `{name}` takes {arity} argument(s)"),
                    )
                    .at(span));
                }
                let mut result = make_variant(adt, index, &name, all);
                if let Some(origin) = &origin {
                    value::mark_ctor_result(&mut result, origin);
                }
                Ok(result)
            },
            | other => {
                Err(
                    EvalError::new("E0305", format!("{} is not a function", other.type_name()))
                        .at(span),
                )
            },
        }
    }

    /// Applies an infix operator by its source spelling.
    ///
    /// Used by builtins (`sum`, `dot`, ...) that fold with the same promotion
    /// rules as the surface operator.
    ///
    /// # Errors
    ///
    /// Propagates the operator's runtime errors.
    pub fn apply_binary(
        &mut self,
        op: &str,
        left: Value,
        right: Value,
        span: Span,
    ) -> Result<Value, EvalError> {
        let op = match op {
            | "+" => BinOp::Add,
            | "-" => BinOp::Sub,
            | "*" => BinOp::Mul,
            | "/" => BinOp::Div,
            | "@" => BinOp::MatMul,
            | "^" => BinOp::Pow,
            | "==" => BinOp::Eq,
            | "!=" => BinOp::Ne,
            | "<" => BinOp::Lt,
            | "<=" => BinOp::Le,
            | ">" => BinOp::Gt,
            | ">=" => BinOp::Ge,
            | "&&" => BinOp::And,
            | "||" => BinOp::Or,
            | "::" => BinOp::Cons,
            | other => {
                return Err(
                    EvalError::new("E0809", format!("unknown operator `{other}`")).at(span),
                );
            },
        };
        self.binary(op, left, right, span)
    }

    /// Binds a pattern into a scope; the public entry point used by `load`.
    ///
    /// # Errors
    ///
    /// Reports a mismatch when the pattern does not accept the value.
    pub fn bind_pattern_entry(
        &mut self,
        pattern: &Pat,
        value: &Value,
        scope: &Rc<Scope>,
    ) -> Result<(), EvalError> {
        self.bind_pattern(pattern, value, scope)
    }

    /// Calls a builtin by name.
    ///
    /// # Errors
    ///
    /// Reports unknown builtins and their runtime failures.
    pub fn call_named(
        &mut self,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, EvalError> {
        crate::builtins::call(self, name, args, span)
    }

    /// Executes a `Rssn.<op>` call with the overload the checker pinned.
    ///
    /// There is no fallback (PLAN D1): resolving again here would let the
    /// executed overload differ from the one the program was typed against, so
    /// an unknown key is `E1005`.
    pub(crate) fn call_rssn(
        &mut self,
        op: &str,
        span: Span,
        values: Vec<Value>,
    ) -> Result<Value, EvalError> {
        let Some(key) = self.context.evidence.rssn_calls.get(&span).cloned() else {
            return Err(EvalError::new(
                "E1005",
                format!("no pinned overload for `Rssn.{op}` at this call site"),
            )
            .at(span)
            .with_note("the checked module did not record this call; re-check the module"));
        };
        let expected = self.resolved_expr_type(span);
        crate::rssn_module::call_operator_with_evidence(&key, values, span, expected.as_ref())
    }

    /// Runs a class-method call site through the instance the checker proved.
    pub(crate) fn run_dictionary(
        &mut self,
        use_site: &DictionaryUse,
        method: &str,
        values: Vec<Value>,
        span: Span,
    ) -> Result<Value, EvalError> {
        match self.resolve_dictionary(use_site, method, &values, span)? {
            | Dispatch::Instance(body) => self.apply(body, values, span),
            | Dispatch::Builtin => self.call_named(method, values, span),
        }
    }

    /// What a class-method call must run, without running it.
    ///
    /// The IR path needs the target rather than its result: a dictionary call
    /// in tail position is a tail call like any other, and applying it here
    /// would spend Rust stack on every step of a recursive instance method.
    pub(crate) fn resolve_dictionary(
        &mut self,
        use_site: &DictionaryUse,
        method: &str,
        values: &[Value],
        span: Span,
    ) -> Result<Dispatch, EvalError> {
        match self.dispatch_method(use_site, method, values) {
            | Some(dispatch) => Ok(dispatch),
            // The checker proved a user instance the runtime cannot find.
            // Running the global of the method name here would be a silently
            // different program.
            | None => {
                Err(EvalError::new(
                    "E0310",
                    format!(
                        "no `{}` instance for {} is available at run time",
                        use_site.class, use_site.head
                    ),
                )
                .at(span)
                .with_note(format!(
                    "the checker proved `{} {}` for `{method}`",
                    use_site.class, use_site.head
                )))
            },
        }
    }

    // ----- patterns -------------------------------------------------------

    fn match_pattern(
        &mut self,
        pattern: &Pat,
        value: &Value,
        scope: &Rc<Scope>,
    ) -> Result<bool, EvalError> {
        let Some(bindings) =
            crate::pattern::ast(pattern, value, &self.context.evidence.constructor_refs)
        else {
            return Ok(false);
        };
        let mut variables = scope.vars.borrow_mut();
        for binding in bindings {
            if let crate::pattern::Binding::Name(name, value) = binding {
                variables.insert(name, value);
            }
        }
        Ok(true)
    }

    fn bind_pattern(
        &mut self,
        pattern: &Pat,
        value: &Value,
        scope: &Rc<Scope>,
    ) -> Result<(), EvalError> {
        if self.match_pattern(pattern, value, scope)? {
            Ok(())
        } else {
            Err(EvalError::new(
                "E0308",
                format!("pattern does not match value {}", value::show(value)),
            )
            .at(pattern.span))
        }
    }

    // ----- tensors --------------------------------------------------------

    fn tensor_literal(
        &mut self,
        items: &[Expr],
        env: &Rc<Scope>,
        span: Span,
    ) -> Result<Value, EvalError> {
        let mut values = Vec::with_capacity(items.len());
        for item in items {
            values.push(self.eval(item, env)?);
        }
        self.tensor_from_values(values, span)
    }

    /// Builds a tensor literal from values that have already been evaluated.
    ///
    /// Shared with the IR path, which evaluates the elements into atoms and
    /// then calls this, so the two paths cannot disagree about nesting, ragged
    /// rows or the storage class.
    pub(crate) fn tensor_from_values(
        &mut self,
        values: Vec<Value>,
        span: Span,
    ) -> Result<Value, EvalError> {
        let mut value = self.tensor_from_values_unverified(values, span)?;
        // This function is called only for an actual AST/IR tensor literal,
        // never for a variable read or a host higher-order apply.
        if let Some(aevia_types::Type::Tensor { shape, elem }) = self.resolved_expr_type(span) {
            if let Value::Tensor(tensor) = &mut value {
                // Empty literal storage has no elements from which to learn a
                // representation. Its own checked literal type may supply it;
                // unlike a variable read, this is a genuine creation site.
                if tensor.storage.is_empty()
                    && matches!(tensor.storage, Storage::Boxed(_))
                    && tensor.shape.len() == shape.rank() as usize
                    && tensor
                        .shape
                        .iter()
                        .enumerate()
                        .all(|(i, n)| shape.extent(i).is_none_or(|e| e == *n))
                {
                    let choice = match elem.as_ref() {
                        | aevia_types::Type::Scalar(Rep::Int, dim) => {
                            Some((
                                Storage::TypedExact(Vec::new(), value::ExactRep::Int),
                                dim.clone(),
                            ))
                        },
                        | aevia_types::Type::Scalar(Rep::Real, dim) => {
                            Some((
                                Storage::TypedExact(Vec::new(), value::ExactRep::Real),
                                dim.clone(),
                            ))
                        },
                        | aevia_types::Type::Scalar(Rep::Float, dim) => {
                            Some((Storage::Dense(Rc::new(Vec::new())), dim.clone()))
                        },
                        | aevia_types::Type::Bool => Some((Storage::Bool(Vec::new()), Dim::one())),
                        | _ => None,
                    };
                    if let Some((storage, dim)) = choice {
                        let tensor = Rc::make_mut(tensor);
                        tensor.storage = storage;
                        tensor.dim = dim;
                    }
                }
                let checked_shape = tensor.shape.len() == shape.rank() as usize
                    && tensor.shape.iter().enumerate().all(|(axis, extent)| {
                        shape
                            .extent(axis)
                            .is_none_or(|expected| expected == *extent)
                    })
                    && tensor
                        .shape
                        .iter()
                        .try_fold(1usize, |count, extent| count.checked_mul(*extent))
                        == Some(tensor.storage.len());
                // Opaque callable values have their own private proof. They
                // cannot pass through the numerical host witness validator.
                let checked_elements = !aevia_types::class::has_variables(&elem)
                    && checked_shape
                    && (0..tensor.storage.len()).all(|index| {
                        tensor
                            .get_flat(index)
                            .is_some_and(|value| self.runtime_input_matches(&value, &elem))
                    });
                if checked_elements
                    || crate::host_witness::tensor_witnesses(tensor, &shape, &elem, true).is_some()
                {
                    Rc::make_mut(tensor).verified_element = Some(Rc::new(*elem));
                }
            }
        }
        Ok(value)
    }

    fn tensor_from_values_unverified(
        &mut self,
        values: Vec<Value>,
        span: Span,
    ) -> Result<Value, EvalError> {
        if values.is_empty() {
            return Ok(Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape: vec![0],
                storage: Storage::Boxed(Vec::new()),
                dim: Dim::one(),
                display: None,
            })));
        }
        // `Value` is an opaque ABI element. A checked list of values may
        // contain tensors, including differently shaped ones, without becoming
        // a stacked numeric tensor. Preserve argument boundaries for dynamic
        // kernel calls in both evaluators.
        if self.resolved_expr_type(span).is_some_and(|ty| {
            matches!(ty,
                aevia_types::Type::Tensor { shape, elem }
                    if shape.rank() == 1 && *elem == aevia_types::builtins::dynamic_value_type()
            )
        }) {
            return Ok(Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape: vec![values.len()],
                storage: Storage::Boxed(values),
                dim: Dim::one(),
                display: None,
            })));
        }
        // Nested literals build higher-rank tensors.
        if values.iter().all(|value| matches!(value, Value::Tensor(_))) {
            let mut shape = vec![values.len()];
            let mut flat = Vec::new();
            let mut dim = Dim::one();
            for value in &values {
                if let Value::Tensor(tensor) = value {
                    if shape.len() == 1 {
                        shape.extend(tensor.shape.iter().copied());
                        dim = tensor.dim.clone();
                    } else if tensor.shape != shape[1..] {
                        return Err(EvalError::new(
                            "E0203",
                            "ragged tensor literal: every row must have the same length",
                        )
                        .at(span));
                    }
                    flat.extend(tensor.elements());
                }
            }
            return Ok(build_tensor(flat, shape, dim));
        }
        let dim = value::value_dim(&values[0]);
        let len = values.len();
        Ok(build_tensor(values, vec![len], dim))
    }

    fn index_value(
        &mut self,
        base: Value,
        indices: &[SliceAst],
        env: &Rc<Scope>,
        span: Span,
    ) -> Result<Value, EvalError> {
        // The base is checked before any index is evaluated, so a bad base is
        // reported without running the indices.
        if !matches!(base, Value::Tensor(_) | Value::Str(_)) {
            return Err(EvalError::new("E0302", "indexing expects a tensor or a string").at(span));
        }
        let mut resolved = Vec::with_capacity(indices.len());
        for index in indices {
            match index {
                | SliceAst::Index(expr) => {
                    let value = self.eval(expr, env)?;
                    resolved.push(SliceValue::Index(value.as_f64()));
                },
                | SliceAst::Range {
                    start,
                    end,
                    inclusive,
                    step,
                } => {
                    let start = match start {
                        | Some(expr) => Some(self.eval(expr, env)?.as_f64().unwrap_or(0.0)),
                        | None => None,
                    };
                    let end = match end {
                        | Some(expr) => Some(self.eval(expr, env)?.as_f64().unwrap_or(0.0)),
                        | None => None,
                    };
                    let step = match step {
                        | Some(expr) => Some(self.eval(expr, env)?.as_f64().unwrap_or(f64::NAN)),
                        | None => None,
                    };
                    resolved.push(SliceValue::Range {
                        start,
                        end,
                        inclusive: *inclusive,
                        step,
                    });
                },
            }
        }
        self.index_resolved(base, &resolved, span)
    }

    /// Applies resolved indices to a tensor.
    ///
    /// Shared with the IR path. An index that is not a number is `E0802` when
    /// every index is scalar and `0` inside a slice, which is what the AST path
    /// has always done; keeping the resolved form explicit makes that visible
    /// instead of implicit in which branch ran.
    pub(crate) fn index_resolved(
        &mut self,
        base: Value,
        indices: &[SliceValue],
        span: Span,
    ) -> Result<Value, EvalError> {
        if let Value::Str(text) = &base {
            return index_string(text, indices, span);
        }
        let Value::Tensor(tensor) = base else {
            return Err(EvalError::new("E0302", "indexing expects a tensor").at(span));
        };
        // All-scalar indexing selects an element or a sub-tensor.
        if indices
            .iter()
            .all(|index| matches!(index, SliceValue::Index(_)))
        {
            let mut coordinates = Vec::with_capacity(indices.len());
            for index in indices {
                if let SliceValue::Index(value) = index {
                    let coordinate =
                        value.ok_or_else(|| EvalError::new("E0802", "index must be an Int"))?;
                    coordinates.push(coordinate as isize);
                }
            }
            if coordinates.len() == tensor.rank() {
                return tensor
                    .get(&coordinates)
                    .ok_or_else(|| index_out_of_range(&coordinates, &tensor.shape).at(span));
            }
            if coordinates.len() < tensor.rank() {
                let fixed = coordinates.len();
                let remaining = &tensor.shape[fixed..];
                // §9.5: pinning the leading axes is a slice of the trailing
                // ones, so an `f64` tensor answers with a view over the same
                // buffer instead of copying the row out of it.
                let mut pinned = Vec::with_capacity(fixed);
                for coordinate in &coordinates {
                    let extent = tensor.shape[pinned.len()] as isize;
                    let normalized = if *coordinate < 0 {
                        extent + *coordinate
                    } else {
                        *coordinate
                    };
                    if normalized < 0 || normalized >= extent {
                        return Err(index_out_of_range(&coordinates, &tensor.shape).at(span));
                    }
                    pinned.push(normalized as usize);
                }
                if let Some(view) = tensor.index_view(&pinned) {
                    return Ok(Value::Tensor(Rc::new(view)));
                }
                let mut elements = Vec::new();
                let count = TensorData::element_count(remaining);
                for index in 0..count {
                    let tail = unravel(index, remaining);
                    let mut full = coordinates.clone();
                    full.extend(tail.iter().map(|value| *value as isize));
                    if let Some(element) = tensor.get(&full) {
                        elements.push(element);
                    }
                }
                if count == 0 {
                    return Ok(empty_tensor_like(&tensor, remaining.to_vec()));
                }
                return Ok(crate::host_witness::preserve_tensor_schema(
                    build_tensor(elements, remaining.to_vec(), tensor.dim.clone()),
                    &tensor,
                ));
            }
        }
        // §6.6.4: an integer index removes its axis, a range keeps it, so
        // `m[:, 1]` is the column as a rank-1 tensor (the static type says so)
        // and not a `[n, 1]` matrix.
        let dropped: Vec<bool> = indices
            .iter()
            .map(|index| matches!(index, SliceValue::Index(_)))
            .collect();
        let sliced = self.slice_axes(&tensor, indices, span)?;
        Ok(drop_indexed_axes(sliced, &dropped))
    }

    /// Mixed index/range indexing with every axis kept (an integer index is a
    /// one-element range here; [`Self::index_resolved`] removes those axes).
    fn slice_axes(
        &mut self,
        tensor: &Rc<TensorData>,
        indices: &[SliceValue],
        span: Span,
    ) -> Result<Value, EvalError> {
        // Slicing: one range per axis, producing a new tensor.
        //
        // A written step turns the axis into a *walk* — it may skip elements
        // or run backwards — which is collected into `walks` beside the
        // windows. The two are kept apart on purpose: a slice without a step
        // keeps the exact behaviour it had, including the `E0802` on an
        // inverted range, while a walk that passes its end immediately is an
        // empty slice (§6.6.4).
        let mut ranges = Vec::with_capacity(indices.len());
        let mut walks: Vec<(usize, usize, isize)> = Vec::with_capacity(indices.len());
        let mut stepped = false;
        for (axis, index) in indices.iter().enumerate() {
            let extent = tensor.shape.get(axis).copied().unwrap_or(0);
            match index {
                | SliceValue::Index(value) => {
                    let coordinate = value.unwrap_or(0.0) as isize;
                    let normalized = normalize_index(coordinate, extent).ok_or_else(|| {
                        EvalError::new(
                            "E0802",
                            format!(
 "index {coordinate} is out of bounds for axis {axis} of extent {extent}"
                            ),
                        )
                        .at(span)
                    })?;
                    ranges.push((normalized, normalized + 1));
                    walks.push((normalized, 1, 1));
                },
                | SliceValue::Range {
                    start,
                    end,
                    inclusive,
                    step,
                } => {
                    if let Some(step) = step {
                        stepped = true;
                        walks.push(walk_range(*start, *end, *step, extent, span)?);
                        continue;
                    }
                    let start_value = start.map_or(0, |value| value as isize);
                    let end_value = end.map_or(extent as isize, |value| value as isize);
                    let start = normalize_bound(start_value, extent, false);
                    let end = normalize_bound(end_value, extent, *inclusive);
                    if start > end {
                        return Err(EvalError::new("E0802", "empty slice").at(span));
                    }
                    ranges.push((start, end));
                    walks.push((start, end - start, 1));
                },
            }
        }
        if stepped {
            while walks.len() < tensor.rank() {
                let extent = tensor.shape[walks.len()];
                walks.push((0, extent, 1));
            }
            let new_shape: Vec<usize> = walks.iter().map(|(_, count, _)| *count).collect();
            // A stepped slice of an `f64` tensor is still a view: the stride
            // is multiplied by the step and a negative step flips the axis, so
            // `xs[::-1]` allocates nothing and shares the buffer `reverse(xs)`
            // returns.
            if let Some(sliced) = tensor.slice_step_view(&walks) {
                return Ok(Value::Tensor(Rc::new(sliced)));
            }
            let count = TensorData::element_count(&new_shape);
            let mut elements = Vec::with_capacity(count);
            for index in 0..count {
                let coordinates = unravel(index, &new_shape);
                let full: Vec<isize> = coordinates
                    .iter()
                    .zip(walks.iter())
                    .map(|(coordinate, (start, _, step))| {
                        (*start as isize) + (*coordinate as isize) * *step
                    })
                    .collect();
                if let Some(element) = tensor.get(&full) {
                    elements.push(element);
                }
            }
            if count == 0 {
                return Ok(empty_tensor_like(&tensor, new_shape));
            }
            return Ok(crate::host_witness::preserve_tensor_schema(
                build_tensor(elements, new_shape, tensor.dim.clone()),
                &tensor,
            ));
        }
        while ranges.len() < tensor.rank() {
            let extent = tensor.shape[ranges.len()];
            ranges.push((0, extent));
        }
        let new_shape: Vec<usize> = ranges.iter().map(|(start, end)| end - start).collect();
        // §9.5/§6.6.4: a slice of an `f64` tensor is a *view* — offset and
        // strides into the buffer it already has — so `xs[1:3]` allocates
        // nothing and a slice of a view is a view of the same buffer. The
        // values are the ones `get` returns for the same coordinates.
        if let Some(sliced) = tensor.slice_view(&ranges) {
            return Ok(Value::Tensor(Rc::new(sliced)));
        }
        let count = TensorData::element_count(&new_shape);
        let mut elements = Vec::with_capacity(count);
        for index in 0..count {
            let coordinates = unravel(index, &new_shape);
            let full: Vec<isize> = coordinates
                .iter()
                .zip(ranges.iter())
                .map(|(coordinate, (start, _))| (start + coordinate) as isize)
                .collect();
            if let Some(element) = tensor.get(&full) {
                elements.push(element);
            }
        }
        if count == 0 {
            return Ok(empty_tensor_like(&tensor, new_shape));
        }
        Ok(crate::host_witness::preserve_tensor_schema(
            build_tensor(elements, new_shape, tensor.dim.clone()),
            &tensor,
        ))
    }

    // ----- IO -------------------------------------------------------------

    /// Captures one immutable deferred program. A replay-local cursor evaluates
    /// every statement in source order when the action executes.
    fn eval_do(
        &mut self,
        stmts: &[DoStmt],
        env: &Rc<Scope>,
        span: Span,
    ) -> Result<Value, EvalError> {
        Ok(Value::Io(Rc::new(IoAction::Do(
            crate::io_do::DoBlock::ast(self, stmts, env, span),
        ))))
    }

    /// Executes an IO action.
    ///
    /// Iterative: the pending actions and their continuations live on an
    /// explicit stack, so a `do` block of any length runs in constant Rust
    /// stack (§9.3.2).
    ///
    /// # Errors
    ///
    /// Reports IO failures (`E0810`), failed assertions (`E0899`), invalid IO
    /// values (`E0701`) and execution-step/continuation limits (`E0805`).
    pub fn run_io(
        &mut self,
        action: &Rc<IoAction>,
    ) -> Result<Value, EvalError> {
        // A failure inside a `with_cap` block must not leave the session
        // sandboxed (the REPL runs the next line in the same interpreter).
        let saved_capabilities = self.capabilities;
        let result = self.run_io_steps(action);
        self.capabilities = saved_capabilities;
        result
    }

    /// The step loop of [`Self::run_io`].
    fn run_io_steps(
        &mut self,
        action: &Rc<IoAction>,
    ) -> Result<Value, EvalError> {
        enum Work {
            Run(Rc<IoAction>, Span),
            Continue(Value, Span),
            Do(crate::io_do::DoCursor),
            Sequence(Rc<IoAction>, usize, Span),
            /// Puts back the capability set a `with_cap` block narrowed.
            RestoreCaps(u8, Span),
        }
        impl Work {
            fn span(&self) -> Span {
                match self {
                    | Self::Do(cursor) => cursor.span(),
                    | Self::Run(action, span) => {
                        match &**action {
                            | IoAction::Do(block) => block.source_span(),
                            | _ => *span,
                        }
                    },
                    | Self::Continue(_, span)
                    | Self::Sequence(_, _, span)
                    | Self::RestoreCaps(_, span) => *span,
                }
            }

            fn continuation(&self) -> bool {
                !matches!(self, Self::Run(..))
            }
        }
        fn push(
            pending: &mut Vec<Work>,
            continuations: &mut usize,
            work: Work,
            limit: usize,
        ) -> Result<(), EvalError> {
            if work.continuation() {
                if *continuations >= limit {
                    return Err(
                        EvalError::new("E0805", "IO continuation depth exceeded").at(work.span())
                    );
                }
                *continuations += 1;
            }
            pending.push(work);
            Ok(())
        }
        let mut pending = vec![Work::Run(action.clone(), Span::default())];
        let mut continuations = 0usize;
        let mut steps = 0usize;
        let mut last = Value::Unit;
        while let Some(work) = pending.pop() {
            if work.continuation() {
                continuations -= 1;
            }
            if steps >= self.max_io_steps {
                return Err(
                    EvalError::new("E0805", "IO execution step budget exhausted")
                        .at(work.span())
                        .with_stack(self.frames.clone()),
                );
            }
            steps += 1;
            let span = work.span();
            let action = match work {
                | Work::Continue(function, _) => {
                    let next = self.apply(function, vec![last.clone()], span)?;
                    let Value::Io(next) = next else {
                        return Err(EvalError::new(
                            "E0701",
                            "continuation did not produce an IO action",
                        )
                        .at(span));
                    };
                    push(
                        &mut pending,
                        &mut continuations,
                        Work::Run(next, span),
                        self.max_io_depth,
                    )?;
                    continue;
                },
                | Work::Do(mut cursor) => {
                    match cursor.step(self, &last)? {
                        | crate::io_do::Step::Continue => {
                            last = Value::Unit;
                            push(
                                &mut pending,
                                &mut continuations,
                                Work::Do(cursor),
                                self.max_io_depth,
                            )?;
                        },
                        | crate::io_do::Step::Complete(value) => last = value,
                        | crate::io_do::Step::Action { action, resume, span } => {
                            if resume {
                                push(
                                    &mut pending,
                                    &mut continuations,
                                    Work::Do(cursor),
                                    self.max_io_depth,
                                )?;
                            }
                            push(
                                &mut pending,
                                &mut continuations,
                                Work::Run(action, span),
                                self.max_io_depth,
                            )?;
                        },
                    }
                    continue;
                },
                | Work::Sequence(sequence, index, span) => {
                    let IoAction::Seq(actions) = &*sequence else {
                        unreachable!()
                    };
                    let next = actions[index].clone();
                    if index + 1 < actions.len() {
                        push(
                            &mut pending,
                            &mut continuations,
                            Work::Sequence(sequence.clone(), index + 1, span),
                            self.max_io_depth,
                        )?;
                    }
                    push(
                        &mut pending,
                        &mut continuations,
                        Work::Run(next, span),
                        self.max_io_depth,
                    )?;
                    continue;
                },
                | Work::RestoreCaps(previous, _) => {
                    self.capabilities = previous;
                    continue;
                },
                | Work::Run(action, _) => action,
            };
            // §9.14: an action that needs a capability the session (or the
            // enclosing `with_cap`) does not grant fails as `E0902`. Data
            // actions answer with their typed `Err`; the others stop.
            let needed = match &*action {
                | IoAction::ReadFile(_)
                | IoAction::WriteFile { .. }
                | IoAction::ReadNpy { .. }
                | IoAction::WriteNpy { .. }
                | IoAction::Plot(_)
                | IoAction::Save { .. } => crate::value::caps::FS,
                | IoAction::Random => crate::value::caps::RAND,
                | _ => 0,
            };
            if needed != 0 && self.capabilities & needed == 0 {
                let message = format!(
                    "E0902: permission denied: the `{}` capability is not granted here",
                    crate::value::caps::name(needed)
                );
                match &*action {
                    | IoAction::ReadFile(_)
                    | IoAction::WriteFile { .. }
                    | IoAction::ReadNpy { .. }
                    | IoAction::WriteNpy { .. } => {
                        last = make_variant(adt::RESULT_ID, 1, "Err", vec![Value::string(message)]);
                        continue;
                    },
                    | _ => {
                        return Err(EvalError::new(
                            "E0902",
                            format!(
                                "permission denied: the `{}` capability is not granted here",
                                crate::value::caps::name(needed)
                            ),
                        )
                        .at(span)
                        .with_note("grant it with `with_cap`, or run without `--caps`"));
                    },
                }
            }
            match &*action {
                | IoAction::WithCaps {
                    allowed,
                    action: inner,
                } => {
                    push(
                        &mut pending,
                        &mut continuations,
                        Work::RestoreCaps(self.capabilities, span),
                        self.max_io_depth,
                    )?;
                    self.capabilities &= *allowed;
                    push(
                        &mut pending,
                        &mut continuations,
                        Work::Run(inner.clone(), span),
                        self.max_io_depth,
                    )?;
                },
                | IoAction::Do(block) => {
                    push(
                        &mut pending,
                        &mut continuations,
                        Work::Do(block.cursor()),
                        self.max_io_depth,
                    )?;
                },
                | IoAction::Pure(value) => last = value.clone(),
                | IoAction::Print {
                    text,
                    newline,
                    stderr,
                } => {
                    self.write_out(text, *newline, *stderr);
                    last = Value::Unit;
                },
                | IoAction::ReadFile(path) => {
                    last = match std::fs::read_to_string(path) {
                        | Ok(contents) => {
                            make_variant(adt::RESULT_ID, 0, "Ok", vec![Value::string(contents)])
                        },
                        | Err(error) => {
                            make_variant(
                                adt::RESULT_ID,
                                1,
                                "Err",
                                vec![Value::string(format!("{}: {error}", io_error_code(&error)))],
                            )
                        },
                    };
                },
                | IoAction::WriteFile { path, contents } => {
                    last = match std::fs::write(path, contents) {
                        | Ok(()) => make_variant(adt::RESULT_ID, 0, "Ok", vec![Value::Unit]),
                        | Err(error) => {
                            make_variant(
                                adt::RESULT_ID,
                                1,
                                "Err",
                                vec![Value::string(format!("{}: {error}", io_error_code(&error)))],
                            )
                        },
                    };
                },
                | IoAction::ReadNpy { path, rank } => {
                    let extension = std::path::Path::new(path)
                        .extension()
                        .and_then(std::ffi::OsStr::to_str)
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    let decoded = if extension != "npy" {
                        Err(format!(
                            "E0904: `read_npy` reads .npy files, not `{}`",
                            if extension.is_empty() {
                                "a file with no extension"
                            } else {
                                &extension
                            }
                        ))
                    } else {
                        std::fs::read(path)
                            .map_err(|error| format!("{}: {error}", io_error_code(&error)))
                            .and_then(|bytes| crate::builtins::npy_decode(&bytes))
                            .and_then(|tensor| {
                                if tensor.shape.len() == *rank as usize {
                                    Ok(tensor)
                                } else {
                                    Err(format!(
                                        "E0903: NPY file has rank {}, expected {rank}",
                                        tensor.shape.len()
                                    ))
                                }
                            })
                    };
                    last = match decoded {
                        | Ok(tensor) => {
                            make_variant(
                                adt::RESULT_ID,
                                0,
                                "Ok",
                                vec![Value::Tensor(Rc::new(tensor))],
                            )
                        },
                        | Err(message) => {
                            make_variant(adt::RESULT_ID, 1, "Err", vec![Value::string(message)])
                        },
                    };
                },
                | IoAction::WriteNpy { path, bytes } => {
                    last = match std::fs::write(path, bytes) {
                        | Ok(()) => make_variant(adt::RESULT_ID, 0, "Ok", vec![Value::Unit]),
                        | Err(error) => {
                            make_variant(
                                adt::RESULT_ID,
                                1,
                                "Err",
                                vec![Value::string(format!("{}: {error}", io_error_code(&error)))],
                            )
                        },
                    };
                },
                | IoAction::ReadLine => {
                    let mut line = String::new();
                    std::io::stdin()
                        .read_line(&mut line)
                        .map_err(|error| EvalError::new("E0810", error.to_string()))?;
                    last = Value::string(line.trim_end_matches(['\n', '\r']));
                },
                | IoAction::Bind { action, func } => {
                    push(
                        &mut pending,
                        &mut continuations,
                        Work::Continue(func.clone(), span),
                        self.max_io_depth,
                    )?;
                    push(
                        &mut pending,
                        &mut continuations,
                        Work::Run(action.clone(), span),
                        self.max_io_depth,
                    )?;
                },
                | IoAction::Seq(actions) => {
                    if actions.is_empty() {
                        last = Value::Unit;
                    } else {
                        push(
                            &mut pending,
                            &mut continuations,
                            Work::Sequence(action.clone(), 0, span),
                            self.max_io_depth,
                        )?;
                    }
                },
                | IoAction::Plot(html) => {
                    std::fs::write("plot.html", html)
                        .map_err(|error| io_error("E0905", "plot rendering failed", &error))?;
                    last = Value::Unit;
                },
                | IoAction::Save { path, html } => {
                    std::fs::write(path, html)
                        .map_err(|error| io_error("E0905", "plot rendering failed", &error))?;
                    last = Value::Unit;
                },
                | IoAction::Defer(thunk) => {
                    let produced = self.apply(Value::Closure(thunk.clone()), Vec::new(), span)?;
                    match produced {
                        | Value::Io(next) => {
                            push(
                                &mut pending,
                                &mut continuations,
                                Work::Run(next, span),
                                self.max_io_depth,
                            )?
                        },
                        | other => {
                            return Err(EvalError::new(
                                "E0702",
                                format!(
                                    "`?` in a `do` block tried to return {} from a function that \
                                     does not return a `Result`",
                                    other.type_name()
                                ),
                            ));
                        },
                    }
                },
                | IoAction::Assert { message } => {
                    return Err(
                        EvalError::new("E0899", message.clone()).with_stack(self.frames.clone())
                    );
                },
                | IoAction::Random => {
                    last = Value::Num(NumVal::float(self.next_random(), Dim::one()));
                },
                | IoAction::TimeNow => {
                    last = Value::Num(NumVal::float(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0.0, |elapsed| elapsed.as_secs_f64()),
                        Dim::base("Time"),
                    ));
                },
            }
        }
        Ok(last)
    }

    /// The next value of the session RNG, in `[0, 1)`.
    ///
    /// Advanced only when a `random` action runs (§6.10): a `let` that is never
    /// executed must not perturb the sequence.
    pub fn next_random(&mut self) -> f64 {
        next_unit(&mut self.seed)
    }
}

/// The appendix-B code for an IO failure (§9.12).
fn io_error_code(error: &std::io::Error) -> &'static str {
    match error.kind() {
        | std::io::ErrorKind::NotFound => "E0901",
        | std::io::ErrorKind::PermissionDenied => "E0902",
        | _ => "E0901",
    }
}

/// A fatal IO error with the taxonomy code and the OS message.
fn io_error(
    code: &'static str,
    context: &str,
    error: &std::io::Error,
) -> EvalError {
    EvalError::new(code, format!("{context}: {error}"))
}

/// The next value of the deterministic LCG in `[0, 1)`.
///
/// Shared by the session RNG and the seeded `sample_n`, so a seeded sample is
/// reproducible across processes: the same seed always produces the same
/// sequence, and a seeded call does not perturb the session's sequence.
#[must_use]
pub fn next_unit(state: &mut u64) -> f64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1);
    (*state >> 11) as f64 / (1u64 << 53) as f64
}

impl EvalError {
    /// Adds a note.
    #[must_use]
    pub fn with_note(
        mut self,
        note: impl Into<String>,
    ) -> Self {
        self.notes.push(note.into());
        self
    }
}


fn to_complex(num: &NumVal) -> (f64, f64) {
    match &num.kind {
        | NumKind::Complex(re, im) => (*re, *im),
        | _ => (num.to_f64().unwrap_or(f64::NAN), 0.0),
    }
}

/// The head of an `instance` in the vocabulary the checked type prints in.
///
/// A nominal type is spelled the same way in both worlds — `type Shape = …`
/// converts to `Type::Adt`, whose rendering is the declared name — so the text
/// is usable as an exact key. An alias, a scalar or a structural head is not:
/// `Vec2` converts to a record type, and only the checker knows that. Those
/// heads are recorded as `None` and found by the single-instance rule in
/// [`Interp::dictionary_body`].
pub(crate) fn instance_head_name(ty: &Ty) -> Option<String> {
    match &ty.kind {
        | TyKind::Named { path, args } if args.is_empty() => path.last().cloned(),
        | _ => None,
    }
}

/// The class-method call sites of one module, by span.
fn dictionary_table(uses: &[DictionaryUse]) -> HashMap<Span, Vec<DictionaryUse>> {
    let mut table: HashMap<Span, Vec<DictionaryUse>> = HashMap::new();
    for use_site in uses {
        table
            .entry(use_site.span)
            .or_default()
            .push(use_site.clone());
    }
    table
}

/// The name a type argument is known by at run time.
///
/// Only the scalar representations matter: `zeros<Int>` and `zeros<Float>`
/// differ in storage, and nothing else about the type survives erasure.
#[must_use]
pub fn type_argument_name(ty: &Ty) -> String {
    match &ty.kind {
        | TyKind::Int => "Int".to_string(),
        | TyKind::Real(_) => "Real".to_string(),
        | TyKind::Float(_) => "Float".to_string(),
        | TyKind::Complex(_) => "Complex".to_string(),
        | TyKind::Bool => "Bool".to_string(),
        | TyKind::Prop => "Prop".to_string(),
        | TyKind::Str => "String".to_string(),
        | TyKind::Unit => "()".to_string(),
        | TyKind::Var(name) => name.clone(),
        | TyKind::Tensor { .. } | TyKind::Matrix { .. } | TyKind::Sparse { .. } => {
            "Tensor".to_string()
        },
        | _ => "?".to_string(),
    }
}

fn combine_units(
    left: &UnitDef,
    right: &UnitDef,
    multiply: bool,
) -> UnitDef {
    let dim = if multiply {
        left.dim.mul(&right.dim)
    } else {
        left.dim.div(&right.dim)
    };
    let scale = if multiply {
        left.scale.mul(&right.scale)
    } else {
        left.scale.div(&right.scale)
    };
    let name = if multiply {
        format!("{}*{}", left.display, right.display)
    } else {
        format!("{}/{}", left.display, right.display)
    };
    UnitDef {
        name: name.clone(),
        aliases: Vec::new(),
        dim,
        scale,
        offset: None,
        prefixable: false,
        affine: false,
        display: name,
        category: "composed".to_string(),
    }
}

fn factorial(
    n: i64,
    double: bool,
) -> BigInt {
    let mut result = BigInt::one();
    if n <= 0 {
        return result;
    }
    let step = if double { 2 } else { 1 };
    let mut current = n;
    while current > 0 {
        result *= BigInt::from(current);
        current -= step;
    }
    result
}

fn normalize_index(
    index: isize,
    extent: usize,
) -> Option<usize> {
    let extent = isize::try_from(extent).ok()?;
    let normalized = if index < 0 {
        extent + index
    } else {
        index
    };
    if normalized < 0 || normalized >= extent {
        None
    } else {
        Some(usize::try_from(normalized).ok()?)
    }
}

fn normalize_bound(
    value: isize,
    extent: usize,
    inclusive: bool,
) -> usize {
    let extent_i = isize::try_from(extent).unwrap_or(isize::MAX);
    let normalized = if value < 0 {
        extent_i + value
    } else {
        value
    };
    let clamped = normalized.clamp(0, extent_i);
    let clamped = usize::try_from(clamped).unwrap_or(0);
    if inclusive {
        (clamped + 1).min(extent)
    } else {
        clamped
    }
}

/// One axis of a stepped slice, as `(first index, element count, step)`.
///
/// The count is the exact number of indices the walk visits (§6.6.4):
/// `ceil((stop - first) / step)` forwards, and the mirror image backwards. A
/// walk whose first index is already at or past the stop visits nothing — that
/// is an empty slice, not one element — so `xs[0:0:1]` and `xs[5:2:1]` are
/// empty.
///
/// A missing bound follows the direction of the walk: `xs[::2]` starts at 0
/// and runs to the end, while `xs[::-1]` starts at the last index and runs to
/// just before the first, which is what makes it `reverse(xs)`.
///
/// A step of zero would never advance and a fractional one names a position
/// that does not exist. The checker rejects both when the value is a literal;
/// this is the same rule for a computed one, and it is an error rather than a
/// division by zero or a loop that never ends.
fn walk_range(
    start: Option<f64>,
    end: Option<f64>,
    step: f64,
    extent: usize,
    span: Span,
) -> Result<(usize, usize, isize), EvalError> {
    if !step.is_finite() || step.fract() != 0.0 || step == 0.0 {
        return Err(EvalError::new("E0802", "a slice step must be a non-zero Int").at(span));
    }
    let step = step as isize;
    let extent = isize::try_from(extent).unwrap_or(isize::MAX);
    if extent <= 0 {
        return Ok((0, 0, step));
    }
    let forward = step > 0;
    // A negative bound counts from the end, exactly as an index does. The
    // clamp keeps a start past the end past the end, so `xs[7:8:1]` is empty
    // rather than the last element.
    let from_end = |value: isize| {
        if value < 0 {
            extent + value
        } else {
            value
        }
    };
    let first = match start {
        | Some(value) => {
            let high = if forward {
                extent
            } else {
                extent - 1
            };
            from_end(value as isize).clamp(0, high)
        },
        | None if forward => 0,
        | None => extent - 1,
    };
    let stop = match end {
        | Some(value) => {
            let high = if forward {
                extent
            } else {
                extent - 1
            };
            from_end(value as isize).clamp(-1, high)
        },
        | None if forward => extent,
        | None => -1,
    };
    // `i128` because the step may be any `Int` the source wrote: a step near
    // `isize::MAX` must count one element, not overflow on `span + step`.
    let (span, divisor) = if forward {
        (stop as i128 - first as i128, step as i128)
    } else {
        (first as i128 - stop as i128, -(step as i128))
    };
    let count = if span <= 0 {
        0
    } else {
        // `ceil(span / divisor)`, in integers, with both terms positive.
        ((span + divisor - 1) / divisor) as usize
    };
    Ok((
        if count == 0 {
            0
        } else {
            first as usize
        },
        count,
        step,
    ))
}

fn broadcast_shape(
    left: &[usize],
    right: &[usize],
) -> Option<Vec<usize>> {
    if left.len() != right.len() {
        return None;
    }
    let mut shape = Vec::with_capacity(left.len());
    for (a, b) in left.iter().zip(right.iter()) {
        if a == b {
            shape.push(*a);
        } else if *a == 1 {
            shape.push(*b);
        } else if *b == 1 {
            shape.push(*a);
        } else {
            return None;
        }
    }
    Some(shape)
}

fn unravel(
    mut index: usize,
    shape: &[usize],
) -> Vec<usize> {
    let mut coordinates = vec![0; shape.len()];
    for axis in (0..shape.len()).rev() {
        let extent = shape[axis].max(1);
        coordinates[axis] = index % extent;
        index /= extent;
    }
    coordinates
}

/// The flat key a qualified field access names, when the base is a path or an
/// import alias.
#[must_use]
pub fn qualified_name(
    base: &Expr,
    field: &str,
) -> Option<String> {
    match &base.kind {
        | ExprKind::QName(path) => Some(format!("{}.{field}", path.join("."))),
        | ExprKind::Var(name) if name.chars().next().is_some_and(char::is_uppercase) => {
            Some(format!("{name}.{field}"))
        },
        | _ => None,
    }
}

/// Whether an expression names the `Rssn` module itself.
#[must_use]
pub fn is_rssn_root(expr: &Expr) -> bool {
    match &expr.kind {
        | ExprKind::Var(name) => name == "Rssn",
        | ExprKind::QName(path) => path.len() == 1 && path[0] == "Rssn",
        | _ => false,
    }
}

/// The operator a `Rssn.<op>` callee names, when it names one.
///
/// The fixed entry points (`Rssn.dynamic`, `Rssn.invoke`, …) are not
/// operators and carry no pinned key; everything else is an operator call the
/// checker resolved.
#[must_use]
pub fn rssn_operator_name(expr: &Expr) -> Option<String> {
    let name = match &expr.kind {
        | ExprKind::Field { base, name } if is_rssn_root(base) => name.clone(),
        | ExprKind::QName(path) if path.len() == 2 && path[0] == "Rssn" => path[1].clone(),
        | _ => return None,
    };
    if crate::rssn_module::is_entry_point(&name) || name == "overloads" {
        return None;
    }
    Some(name)
}

fn element_at_broadcast(
    tensor: &TensorData,
    coordinates: &[usize],
) -> Option<Value> {
    let mut adjusted = Vec::with_capacity(coordinates.len());
    for (coordinate, extent) in coordinates.iter().zip(tensor.shape.iter()) {
        adjusted.push(if *extent == 1 {
            0
        } else {
            *coordinate
        } as isize);
    }
    tensor.get(&adjusted)
}

/// Preserve a tensor's element representation when a slice has no elements.
fn empty_tensor_like(
    tensor: &TensorData,
    shape: Vec<usize>,
) -> Value {
    let storage = match tensor.storage.backing() {
        | Storage::Dense(_) | Storage::View(_) => Storage::Dense(Rc::new(Vec::new())),
        | Storage::Exact(_) => Storage::Exact(Vec::new()),
        | Storage::TypedExact(_, rep) => Storage::TypedExact(Vec::new(), *rep),
        | Storage::Bool(_) => Storage::Bool(Vec::new()),
        | Storage::Boxed(_) => Storage::Boxed(Vec::new()),
        | Storage::SharedSlice(_) => unreachable!("unwrapped storage"),
    };
    let result = Value::Tensor(Rc::new(TensorData {
        verified_element: None,
        shape,
        storage,
        dim: tensor.dim.clone(),
        display: tensor.display.clone(),
    }));
    crate::host_witness::preserve_tensor_schema(result, tensor)
}

/// Builds a tensor from elements, choosing the narrowest storage.
#[must_use]
pub fn build_tensor(
    elements: Vec<Value>,
    shape: Vec<usize>,
    dim: Dim,
) -> Value {
    if elements.is_empty() {
        return Value::Tensor(Rc::new(TensorData {
            verified_element: None,
            shape,
            storage: Storage::Boxed(Vec::new()),
            dim,
            display: None,
        }));
    }
    if elements.iter().all(|element| {
        matches!(
            element,
            Value::Num(NumVal {
                kind: NumKind::Float(_),
                ..
            })
        )
    }) {
        let mut values = Vec::with_capacity(elements.len());
        let mut display = None;
        for element in &elements {
            if let Value::Num(num) = element {
                values.push(num.to_f64().unwrap_or(f64::NAN));
                if display.is_none() {
                    display = num.display.clone();
                }
            }
        }
        return Value::Tensor(Rc::new(TensorData {
            verified_element: None,
            shape,
            storage: Storage::Dense(Rc::new(values)),
            dim,
            display,
        }));
    }
    // §6.6.1: mixing numeric phases widens the storage — `[1, 2.0]` is an
    // `F64` tensor, not a boxed one — so the elementwise class is the widest
    // one that holds every element. This runs after the all-`Float` case so a
    // pure `Float` literal keeps its fast path, and it only applies when every
    // element is numeric: a tensor of records or variants stays `Boxed`.
    if elements.iter().any(|element| {
        matches!(
            element,
            Value::Num(NumVal {
                kind: NumKind::Float(_),
                ..
            })
        )
    }) && elements.iter().all(|element| {
        matches!(
            element,
            Value::Num(NumVal {
                kind: NumKind::Int(_) | NumKind::Exact(_) | NumKind::Float(_),
                ..
            })
        )
    }) {
        let mut values = Vec::with_capacity(elements.len());
        let mut converted = true;
        let mut display = None;
        for element in &elements {
            if let Value::Num(num) = element {
                match num.to_f64() {
                    | Some(value) => values.push(value),
                    | None => {
                        converted = false;
                        break;
                    },
                }
                if display.is_none() {
                    display = num.display.clone();
                }
            }
        }
        if converted {
            return Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape,
                storage: Storage::Dense(Rc::new(values)),
                dim,
                display,
            }));
        }
    }
    if elements.iter().all(|element| {
        matches!(
            element,
            Value::Num(NumVal {
                kind: NumKind::Int(_) | NumKind::Exact(_),
                ..
            })
        )
    }) {
        let mut values = Vec::with_capacity(elements.len());
        let mut display = None;
        for element in &elements {
            if let Value::Num(num) = element {
                values.push(num.to_rational().unwrap_or_else(BigRational::zero));
                if display.is_none() {
                    display = num.display.clone();
                }
            }
        }
        let rep = if elements.iter().all(|element| {
            matches!(
                element,
                Value::Num(NumVal {
                    kind: NumKind::Int(_),
                    ..
                })
            )
        }) {
            value::ExactRep::Int
        } else {
            value::ExactRep::Real
        };
        return Value::Tensor(Rc::new(TensorData {
            verified_element: None,
            shape,
            storage: Storage::TypedExact(values, rep),
            dim,
            display,
        }));
    }
    if elements
        .iter()
        .all(|element| matches!(element, Value::Bool(_)))
    {
        let mut values = Vec::with_capacity(elements.len());
        for element in &elements {
            if let Value::Bool(value) = element {
                values.push(*value);
            }
        }
        return Value::Tensor(Rc::new(TensorData {
            verified_element: None,
            shape,
            storage: Storage::Bool(values),
            dim,
            display: None,
        }));
    }
    Value::Tensor(Rc::new(TensorData {
        verified_element: None,
        shape,
        storage: Storage::Boxed(elements),
        dim,
        display: None,
    }))
}

/// Formats a value using the module's dimension environment.
#[must_use]
pub fn render(value: &Value) -> String {
    value::show(value)
}

/// A convenience wrapper used by the CLI: show a scalar with its unit.
#[must_use]
pub fn render_scalar(num: &NumVal) -> String {
    format_scalar(num)
}

/// A runtime type only when all of its relevant type information survives
/// erasure. In particular exact tensors do not preserve Int versus Real.
#[derive(Clone, PartialEq, Eq)]
enum ObservableType {
    Num(RepKind, Dim),
    Tensor(Vec<usize>, RepKind, Dim),
    Record(Vec<(String, ObservableType)>),
    Adt(aevia_types::keys::AdtKey, Vec<ObservableType>),
    Bool,
    Str,
    Unit,
    Prop,
}

fn observable_type(value: &Value) -> Option<ObservableType> {
    match value {
        | Value::Num(num) => {
            if !num.dim.vars().is_empty() {
                return None;
            }
            let rep = match num.kind {
                | NumKind::Int(_) => RepKind::Int,
                | NumKind::Exact(_) | NumKind::Sym(_) => RepKind::Real,
                | NumKind::Float(_) => RepKind::Float,
                | NumKind::Complex(..) => RepKind::Complex,
            };
            Some(ObservableType::Num(rep, num.dim.clone()))
        },
        | Value::Tensor(tensor) => {
            if !tensor.dim.vars().is_empty() {
                return None;
            }
            let rep = match tensor.storage.backing() {
                | Storage::Dense(_) | Storage::View(_) => RepKind::Float,
                // Exact storage loses the Int/Real distinction, even if all
                // values happen to be integers.
                | Storage::Exact(_) => return None,
                | Storage::TypedExact(_, value::ExactRep::Int) => RepKind::Int,
                | Storage::TypedExact(_, value::ExactRep::Real) => RepKind::Real,
                | Storage::Bool(_) => return None,
                | Storage::Boxed(_) | Storage::SharedSlice(_) => return None,
            };
            Some(ObservableType::Tensor(
                tensor.shape.clone(),
                rep,
                tensor.dim.clone(),
            ))
        },
        | Value::Record(record) => {
            let mut fields = record
                .fields
                .iter()
                .map(|(name, value)| Some((name.clone(), observable_type(value)?)))
                .collect::<Option<Vec<_>>>()?;
            fields.sort_by(|a, b| a.0.cmp(&b.0));
            Some(ObservableType::Record(fields))
        },
        | Value::Variant(variant) => {
            let args = if adt::param_kinds_of(variant.adt).is_empty() {
                Vec::new()
            } else if let Some(types) = &variant.type_args {
                if !variant_type_args_valid(variant, types) {
                    return None;
                }
                types
                    .iter()
                    .map(observable_type_from_type)
                    .collect::<Option<Vec<_>>>()?
            } else {
                inferred_variant_args(variant)?
            };
            Some(ObservableType::Adt(adt::key_of(variant.adt), args))
        },
        | Value::Bool(_) => Some(ObservableType::Bool),
        | Value::Str(_) => Some(ObservableType::Str),
        | Value::Unit => Some(ObservableType::Unit),
        | Value::Prop(_) => Some(ObservableType::Prop),
        | _ => None,
    }
}

fn observable_type_from_type(ty: &aevia_types::Type) -> Option<ObservableType> {
    use aevia_types::Type;
    match ty {
        | Type::Scalar(rep, dim) if dim.vars().is_empty() => {
            Some(ObservableType::Num(RepKind::of(*rep)?, dim.clone()))
        },
        | Type::Tensor { shape, elem } => {
            match &**elem {
                | Type::Scalar(rep, dim) if dim.vars().is_empty() => {
                    Some(ObservableType::Tensor(
                        shape.extents()?,
                        RepKind::of(*rep)?,
                        dim.clone(),
                    ))
                },
                | _ => None,
            }
        },
        | Type::Record(row) if row.tail_var().is_none() => {
            let mut fields = row
                .fields()
                .into_iter()
                .map(|(name, ty)| Some((name, observable_type_from_type(&ty)?)))
                .collect::<Option<Vec<_>>>()?;
            fields.sort_by(|a, b| a.0.cmp(&b.0));
            Some(ObservableType::Record(fields))
        },
        | Type::Adt(id, args) => {
            Some(ObservableType::Adt(
                adt::key_of(*id),
                args.iter()
                    .map(observable_type_from_type)
                    .collect::<Option<Vec<_>>>()?,
            ))
        },
        | Type::Bool => Some(ObservableType::Bool),
        | Type::Str => Some(ObservableType::Str),
        | Type::Unit => Some(ObservableType::Unit),
        | Type::Prop => Some(ObservableType::Prop),
        | _ => None,
    }
}

fn bind_runtime_type(
    key: (bool, u32),
    actual: Option<ObservableType>,
    bindings: &mut HashMap<(bool, u32), Option<ObservableType>>,
) -> bool {
    match bindings.get(&key) {
        | Some(Some(previous)) => actual.as_ref() == Some(previous),
        | Some(None) => false,
        | None => {
            bindings.insert(key, actual);
            true
        },
    }
}

fn runtime_type_matches(
    pattern: &aevia_types::Type,
    actual: &aevia_types::Type,
    bindings: &mut HashMap<(bool, u32), Option<ObservableType>>,
) -> bool {
    use aevia_types::Type;
    match (pattern, actual) {
        | (Type::Var(id), _) | (Type::Param(id), _) => {
            bind_runtime_type(
                (matches!(pattern, Type::Param(_)), *id),
                observable_type_from_type(actual),
                bindings,
            )
        },
        | (Type::Adt(a, pa), Type::Adt(b, aa)) => {
            adt::key_of(*a) == adt::key_of(*b)
                && pa.len() == aa.len()
                && pa
                    .iter()
                    .zip(aa)
                    .all(|(p, a)| runtime_type_matches(p, a, bindings))
        },
        | (Type::Tensor { elem: p, .. }, Type::Tensor { elem: a, .. }) => {
            aevia_types::class::matches_head(pattern, actual)
                && runtime_type_matches(p, a, bindings)
        },
        | (Type::Record(p), Type::Record(a)) => {
            let pf = p.fields();
            let af = a.fields();
            (p.tail_var().is_some() || (a.tail_var().is_none() && pf.len() == af.len()))
                && pf.iter().all(|(name, ty)| {
                    af.iter()
                        .find(|(n, _)| n == name)
                        .is_some_and(|(_, other)| runtime_type_matches(ty, other, bindings))
                })
        },
        | _ => aevia_types::class::matches_head(pattern, actual),
    }
}

/// Check public type metadata against the constructor before trusting it.
/// A nullary constructor is valid with explicit arguments even though none can
/// be recovered from its payload.
fn variant_type_args_valid(
    variant: &value::VariantValue,
    args: &[aevia_types::Type],
) -> bool {
    use aevia_types::Type;
    let Some(definition) = adt::get(variant.adt) else {
        return false;
    };
    let Some(constructor) = definition.ctors.get(variant.index) else {
        return false;
    };
    let kinds = adt::param_kinds_of(variant.adt);
    if args.len() != kinds.len()
        || constructor.fields.len() != variant.fields.len()
        || args.iter().any(aevia_types::class::has_variables)
    {
        return false;
    }
    let mut types = vec![None; args.len()];
    let mut dims = HashMap::new();
    for (index, (kind, arg)) in kinds.iter().zip(args).enumerate() {
        match (kind, arg) {
            | (adt::ParamKind::Type, ty) => types[index] = Some(ty.clone()),
            | (adt::ParamKind::Dim(id), Type::UnitOf(dim)) => {
                dims.insert(*id, dim.clone());
            },
            | _ => return false,
        }
    }
    let mut dimensions = aevia_dim::DimUnifier::new();
    for (field, value) in constructor.fields.iter().zip(&variant.fields) {
        let specialised = field.substitute_kinded(&types, &dims, &HashMap::new(), &HashMap::new());
        if !runtime_head_matches(&specialised, value, &mut HashMap::new())
            || !constructor_field_dimensions(&specialised, value, &mut dimensions)
        {
            return false;
        }
    }
    true
}

/// Derive ADT arguments only from declared constructor fields. Every field
/// must match and every parameter must be observed; missing evidence stays
/// unknown (notably `None` and phantom parameters).
fn inferred_variant_args(variant: &value::VariantValue) -> Option<Vec<ObservableType>> {
    let definition = adt::get(variant.adt)?;
    let constructor = definition.ctors.get(variant.index)?;
    if constructor.fields.len() != variant.fields.len() {
        return None;
    }
    let mut bindings = HashMap::new();
    let mut dimensions = aevia_dim::DimUnifier::new();
    for (field, value) in constructor.fields.iter().zip(&variant.fields) {
        if !runtime_head_matches(field, value, &mut bindings)
            || !constructor_field_dimensions(field, value, &mut dimensions)
        {
            return None;
        }
    }
    (0..definition.params.len())
        .map(|id| {
            bindings
                .get(&(true, u32::try_from(id).ok()?))
                .cloned()
                .flatten()
        })
        .collect()
}

/// Constructor declarations carry dimensional requirements even though
/// instance heads intentionally dispatch scalars by representation alone.
fn constructor_field_dimensions(
    pattern: &aevia_types::Type,
    value: &Value,
    dimensions: &mut aevia_dim::DimUnifier,
) -> bool {
    use aevia_types::Type;
    match (pattern, value) {
        | (Type::Scalar(_, expected), Value::Num(num)) => {
            num.dim.vars().is_empty() && dimensions.unify(expected, &num.dim).is_ok()
        },
        | (Type::Tensor { elem, .. }, Value::Tensor(tensor)) => {
            match &**elem {
                | Type::Scalar(_, expected) => {
                    tensor.dim.vars().is_empty() && dimensions.unify(expected, &tensor.dim).is_ok()
                },
                | _ => true,
            }
        },
        | (Type::Record(row), Value::Record(record)) => {
            row.fields().iter().all(|(name, field)| {
                record
                    .fields
                    .get(name)
                    .is_some_and(|value| constructor_field_dimensions(field, value, dimensions))
            })
        },
        | (Type::Adt(_, args), Value::Variant(variant)) if !args.is_empty() => {
            if let Some(actual) = &variant.type_args {
                args.len() == actual.len()
                    && args
                        .iter()
                        .zip(actual)
                        .all(|(p, a)| constructor_type_dimensions(p, a, dimensions))
            } else if let Some(actual) = inferred_variant_args(variant) {
                args.len() == actual.len()
                    && args
                        .iter()
                        .zip(&actual)
                        .all(|(p, a)| constructor_observable_dimensions(p, a, dimensions))
            } else {
                false
            }
        },
        | _ => true,
    }
}

fn constructor_type_dimensions(
    pattern: &aevia_types::Type,
    actual: &aevia_types::Type,
    dimensions: &mut aevia_dim::DimUnifier,
) -> bool {
    use aevia_types::Type;
    match (pattern, actual) {
        | (Type::Scalar(_, p), Type::Scalar(_, a))
        | (Type::Point(p), Type::Point(a))
        | (Type::UnitOf(p), Type::UnitOf(a)) => dimensions.unify(p, a).is_ok(),
        | (Type::Tensor { elem: p, .. }, Type::Tensor { elem: a, .. }) => {
            constructor_type_dimensions(p, a, dimensions)
        },
        | (Type::Adt(_, p), Type::Adt(_, a)) => {
            p.len() == a.len()
                && p.iter()
                    .zip(a)
                    .all(|(p, a)| constructor_type_dimensions(p, a, dimensions))
        },
        | (Type::Record(p), Type::Record(a)) => {
            let fields = a.fields();
            p.fields().iter().all(|(name, ty)| {
                fields
                    .iter()
                    .find(|(n, _)| n == name)
                    .is_some_and(|(_, actual)| constructor_type_dimensions(ty, actual, dimensions))
            })
        },
        | _ => true,
    }
}

fn constructor_observable_dimensions(
    pattern: &aevia_types::Type,
    actual: &ObservableType,
    dimensions: &mut aevia_dim::DimUnifier,
) -> bool {
    use aevia_types::Type;
    match (pattern, actual) {
        | (Type::Scalar(_, p), ObservableType::Num(_, a)) => dimensions.unify(p, a).is_ok(),
        | (Type::Tensor { elem, .. }, ObservableType::Tensor(_, rep, dim)) => {
            constructor_observable_dimensions(
                elem,
                &ObservableType::Num(*rep, dim.clone()),
                dimensions,
            )
        },
        | (Type::Adt(_, p), ObservableType::Adt(_, a)) => {
            p.len() == a.len()
                && p.iter()
                    .zip(a)
                    .all(|(p, a)| constructor_observable_dimensions(p, a, dimensions))
        },
        | (Type::Record(p), ObservableType::Record(a)) => {
            p.fields().iter().all(|(name, ty)| {
                a.iter()
                    .find(|(n, _)| n == name)
                    .is_some_and(|(_, actual)| {
                        constructor_observable_dimensions(ty, actual, dimensions)
                    })
            })
        },
        | _ => true,
    }
}

fn runtime_observable_matches(
    pattern: &aevia_types::Type,
    actual: &ObservableType,
    bindings: &mut HashMap<(bool, u32), Option<ObservableType>>,
) -> bool {
    use aevia_types::Type;
    match (pattern, actual) {
        | (Type::Var(id), _) | (Type::Param(id), _) => {
            bind_runtime_type(
                (matches!(pattern, Type::Param(_)), *id),
                Some(actual.clone()),
                bindings,
            )
        },
        | (Type::Scalar(rep, _), ObservableType::Num(actual, _)) => {
            matches!(rep, Rep::Var(_)) || RepKind::of(*rep) == Some(*actual)
        },
        | (Type::Adt(id, args), ObservableType::Adt(key, actual)) => {
            adt::key_of(*id) == *key
                && args.len() == actual.len()
                && args
                    .iter()
                    .zip(actual)
                    .all(|(p, a)| runtime_observable_matches(p, a, bindings))
        },
        | (Type::Tensor { shape, elem }, ObservableType::Tensor(extents, rep, dim)) => {
            usize::try_from(shape.rank()).ok() == Some(extents.len())
                && extents
                    .iter()
                    .enumerate()
                    .all(|(i, n)| shape.extent(i).is_none_or(|e| e == *n))
                && runtime_observable_matches(
                    elem,
                    &ObservableType::Num(*rep, dim.clone()),
                    bindings,
                )
        },
        | (Type::Record(row), ObservableType::Record(fields)) => {
            let pattern_fields = row.fields();
            (row.tail_var().is_some() || pattern_fields.len() == fields.len())
                && pattern_fields.iter().all(|(name, ty)| {
                    fields
                        .iter()
                        .find(|(n, _)| n == name)
                        .is_some_and(|(_, actual)| runtime_observable_matches(ty, actual, bindings))
                })
        },
        | (Type::Bool, ObservableType::Bool)
        | (Type::Str, ObservableType::Str)
        | (Type::Unit, ObservableType::Unit)
        | (Type::Prop, ObservableType::Prop) => true,
        | _ => false,
    }
}

fn runtime_structural_show(value: &Value) -> bool {
    match value {
        | Value::Num(_)
        | Value::Bool(_)
        | Value::Prop(_)
        | Value::Str(_)
        | Value::Unit
        | Value::Dyn(_)
        | Value::UnitOf(_)
        | Value::Io(_) => true,
        | Value::Tensor(tensor) => {
            match tensor.storage.backing() {
                | Storage::Boxed(_) => {
                    !tensor.storage.is_empty()
                        && (0..tensor.storage.len()).all(|index| {
                            tensor
                                .get_flat(index)
                                .is_some_and(|value| runtime_structural_show(&value))
                        })
                },
                | _ => true,
            }
        },
        | Value::Record(record) => record.fields.values().all(runtime_structural_show),
        | _ => false,
    }
}

/// Match an instance head against what the value actually preserves. A repeated
/// head variable binds once and every later occurrence must agree.
fn runtime_head_matches(
    head: &aevia_types::Type,
    value: &Value,
    bindings: &mut HashMap<(bool, u32), Option<ObservableType>>,
) -> bool {
    use aevia_types::Type;
    match (head, value) {
        | (Type::Var(id), _) | (Type::Param(id), _) => {
            bind_runtime_type(
                (matches!(head, Type::Param(_)), *id),
                observable_type(value),
                bindings,
            )
        },
        | (Type::Scalar(rep, _), Value::Num(num)) => {
            match (rep, &num.kind) {
                | (Rep::Var(_), _) => true,
                | (Rep::Int, NumKind::Int(_)) => true,
                | (Rep::Real, NumKind::Exact(_) | NumKind::Sym(_)) => true,
                | (Rep::Float, NumKind::Float(_)) => true,
                | (Rep::Complex, NumKind::Complex(..)) => true,
                | _ => false,
            }
        },
        | (Type::Tensor { shape, elem }, Value::Tensor(tensor)) => {
            if usize::try_from(shape.rank()).ok() != Some(tensor.shape.len()) {
                return false;
            }
            if tensor
                .shape
                .iter()
                .enumerate()
                .any(|(i, actual)| shape.extent(i).is_some_and(|n| n != *actual))
            {
                return false;
            }
            match (&**elem, tensor.storage.backing()) {
                | (Type::Var(id) | Type::Param(id), storage) => {
                    let actual = if !tensor.dim.vars().is_empty() {
                        None
                    } else {
                        match storage {
                            | Storage::Dense(_) | Storage::View(_) => {
                                Some(ObservableType::Num(RepKind::Float, tensor.dim.clone()))
                            },
                            | Storage::TypedExact(_, value::ExactRep::Int) => {
                                Some(ObservableType::Num(RepKind::Int, tensor.dim.clone()))
                            },
                            | Storage::TypedExact(_, value::ExactRep::Real) => {
                                Some(ObservableType::Num(RepKind::Real, tensor.dim.clone()))
                            },
                            | Storage::Bool(_) => Some(ObservableType::Bool),
                            | Storage::Exact(_) | Storage::Boxed(_) | Storage::SharedSlice(_) => {
                                None
                            },
                        }
                    };
                    bind_runtime_type((matches!(&**elem, Type::Param(_)), *id), actual, bindings)
                },
                | (Type::Scalar(Rep::Float, _), Storage::Dense(_) | Storage::View(_)) => true,
                | (Type::Scalar(Rep::Int, _), Storage::TypedExact(_, value::ExactRep::Int)) => true,
                | (Type::Scalar(Rep::Real, _), Storage::TypedExact(_, value::ExactRep::Real)) => {
                    true
                },
                | (Type::Bool, Storage::Bool(_)) => true,
                // Neither Int nor Real can be proved from exact storage.
                | _ => false,
            }
        },
        | (Type::Record(row), Value::Record(record)) => {
            let fields = row.fields();
            if row.tail_var().is_none() && fields.len() != record.fields.len() {
                return false;
            }
            fields.iter().all(|(name, ty)| {
                record
                    .fields
                    .get(name)
                    .is_some_and(|value| runtime_head_matches(ty, value, bindings))
            })
        },
        | (Type::Adt(id, args), Value::Variant(variant)) => {
            if adt::key_of(*id) != adt::key_of(variant.adt) {
                return false;
            }
            if args.is_empty() {
                return true;
            }
            if let Some(actual) = &variant.type_args {
                variant_type_args_valid(variant, actual)
                    && args.len() == actual.len()
                    && args
                        .iter()
                        .zip(actual)
                        .all(|(pattern, actual)| runtime_type_matches(pattern, actual, bindings))
            } else if let Some(actual) = inferred_variant_args(variant) {
                args.len() == actual.len()
                    && args.iter().zip(&actual).all(|(pattern, actual)| {
                        runtime_observable_matches(pattern, actual, bindings)
                    })
            } else {
                false
            }
        },
        | (Type::Bool, Value::Bool(_))
        | (Type::Str, Value::Str(_))
        | (Type::Unit, Value::Unit)
        | (Type::Prop, Value::Prop(_)) => true,
        | _ => false,
    }
}

/// The run-time witness of a value, mirroring `aevia_types::keys::witness_of`.
///
/// A quantified class-method site dispatches by this witness (§3.8.3): the
/// checker proved the candidate instances' witnesses pairwise disjoint, so at
/// most one candidate can accept the argument. `None` means the value has no
/// witness at all (a function, a `Dyn`, an empty boxed tensor), and the runtime
/// then refuses to guess.
#[must_use]
pub fn witness_of_value(value: &Value) -> Option<Witness> {
    match value {
        | Value::Num(num) => {
            Some(Witness::Num(match &num.kind {
                | NumKind::Int(_) => RepKind::Int,
                | NumKind::Exact(_) | NumKind::Sym(_) => RepKind::Real,
                | NumKind::Float(_) => RepKind::Float,
                | NumKind::Complex(..) => RepKind::Complex,
            }))
        },
        | Value::Tensor(tensor) => {
            let storage = match tensor.storage.backing() {
                | Storage::Dense(_) | Storage::View(_) => StorageClass::Float,
                | Storage::Exact(_) | Storage::TypedExact(..) => StorageClass::Exact,
                // A `Bool` tensor has no element representation in the witness
                // table (`witness_of` requires a scalar element).
                | Storage::Bool(_) | Storage::SharedSlice(_) => return None,
                | Storage::Boxed(_) => {
                    match tensor.get_flat(0).as_ref() {
                        | Some(Value::Num(num)) => {
                            match &num.kind {
                                | NumKind::Int(_) | NumKind::Exact(_) | NumKind::Sym(_) => {
                                    StorageClass::Exact
                                },
                                | NumKind::Float(_) => StorageClass::Float,
                                | NumKind::Complex(..) => StorageClass::Complex,
                            }
                        },
                        | _ => return None,
                    }
                },
            };
            Some(Witness::Tensor {
                rank: u32::try_from(tensor.rank()).ok()?,
                storage,
            })
        },
        | Value::Variant(variant) => {
            // Constructor payloads are not type arguments: e.g. `Some(x)`
            // exposes one parameter but `None` exposes none, and a non-generic
            // constructor can have arbitrarily many payload fields.
            if !adt::param_kinds_of(variant.adt).is_empty() {
                return None;
            }
            Some(Witness::Adt(adt::key_of(variant.adt), Vec::new()))
        },
        | Value::Record(record) => {
            let mut labels: Vec<String> = record.fields.keys().cloned().collect();
            labels.sort();
            Some(Witness::Record(labels))
        },
        | Value::Bool(_) => Some(Witness::Bool),
        | Value::Str(_) => Some(Witness::String),
        | Value::Unit => Some(Witness::Unit),
        | Value::Prop(_) => Some(Witness::Prop),
        | Value::Closure(_) | Value::Native(_) | Value::Ctor { .. } | Value::Io(_) => None,
        | Value::Dyn(_)
        | Value::UnitOf(_)
        | Value::TypeRep(_)
        | Value::Callback(_)
        | Value::KernelValue(_)
        | Value::KernelError(_)
        | Value::Error => None,
    }
}

/// Finds a user constructor by its defining module and name (§3.6.2/D6).
///
/// `adt::find_ctor` searches every registered type and returns the first name
/// match; two modules may both declare `type Shape = | Circle(…)`, so a
/// name-only lookup would construct whichever module was checked first. This
/// scans for the nominal identity instead.
#[must_use]
pub fn ctor_in_module(
    module: &keys::ModulePath,
    name: &str,
) -> Option<(adt::AdtId, usize, adt::Adt)> {
    (0..adt::count()).find_map(|index| {
        let id = u32::try_from(index).ok()?;
        let registered = adt::get(id)?;
        if adt::key_of(id).module != *module {
            return None;
        }
        let position = registered.ctors.iter().position(|ctor| ctor.name == name)?;
        Some((id, position, (*registered).clone()))
    })
}

fn builtin_result_schema(
    name: &str,
    args: &[Value],
) -> Option<aevia_types::Type> {
    use aevia_types::Type;
    // Only implementations that construct the result themselves can mint
    // provenance; a builtin that forwards a callback result cannot.
    if !matches!(name, "find" | "parse_int" | "parse_float") {
        return None;
    }
    let mut derived: Option<Type> = None;
    for scheme in aevia_types::builtins::schemes(name)? {
        if !scheme.constraints.is_empty() {
            continue;
        }
        let mut infer = aevia_types::infer::Infer::new();
        let Type::Fn(params, ret) = infer.instantiate(&scheme) else {
            continue;
        };
        if params.len() != args.len() {
            continue;
        }
        let mut inputs = Vec::new();
        let mut valid = true;
        for (param, arg) in params.iter().zip(args) {
            if matches!(param, Type::Fn(..)) {
                if !matches!(
                    arg,
                    Value::Ctor { .. } | Value::Closure(_) | Value::Native(_)
                ) {
                    valid = false;
                    break;
                }
                continue;
            }
            let Some(schema) = crate::host_witness::runtime_schema(arg) else {
                valid = false;
                break;
            };
            if crate::host_witness::capture(arg, &schema, false).is_none()
                || infer.unify(param, &schema).is_err()
            {
                valid = false;
                break;
            }
            inputs.push((param, schema));
        }
        if !valid
            || inputs.iter().any(|(param, schema)| {
                !crate::host_witness::strict_schema_equal(&infer.resolve(param), schema)
            })
        {
            continue;
        }
        let result = infer.resolve(&ret);
        if aevia_types::class::has_variables(&result) || !matches!(result, Type::Adt(..)) {
            continue;
        }
        if derived
            .as_ref()
            .is_some_and(|previous| !crate::host_witness::strict_schema_equal(previous, &result))
        {
            return None;
        }
        derived = Some(result);
    }
    derived
}

/// A constructor value of one module's type, ready to apply.
#[must_use]
pub fn ctor_value(
    module: &keys::ModulePath,
    name: &str,
) -> Option<Value> {
    let (adt, index, _) = ctor_in_module(module, name)?;
    ctor_value_at(adt, index)
}

/// Materializes the constructor witness chosen by one exact compilation.
#[must_use]
pub fn ctor_value_at(
    adt: adt::AdtId,
    index: usize,
) -> Option<Value> {
    let registered = adt::get(adt)?;
    let name = &registered.ctors.get(index)?.name;
    let arity = registered
        .ctors
        .get(index)
        .map_or(0, |ctor| ctor.fields.len());
    if arity == 0 {
        return Some(make_variant(adt, index, name, Vec::new()));
    }
    Some(Value::Ctor {
        adt,
        index,
        name: name.to_string(),
        arity,
        args: Vec::new(),
        origin: None,
    })
}

pub(crate) fn source_ctor_value_at(
    adt: adt::AdtId,
    index: usize,
) -> Option<Value> {
    let mut value = ctor_value_at(adt, index)?;
    crate::value::mark_source_ctor(&mut value);
    Some(value)
}

/// Constructor name lookup, re-exported for the runtime's pattern matching.
#[must_use]
pub fn variant_name(
    adt: adt::AdtId,
    index: usize,
) -> Option<String> {
    ctor_name(adt, index)
}

/// The value of `main`, if the module defines one.
#[must_use]
pub fn main_action(interp: &Interp) -> Option<Value> {
    interp.globals.get("main").cloned()
}

#[cfg(test)]
mod measurement_tests {
    //! §8.14 unary negation: the value negates, the uncertainty (a magnitude)
    //! does not.
    use super::*;

    fn interpreter() -> Interp {
        let compilation = crate::compile(
            "measure-unit.aev",
            "fn main(): IO Unit = do\n    println(\"\")\n",
        );
        assert!(!compilation.diagnostics.has_errors());
        let mut interp = Interp::new(&compilation.checked).with_buffer();
        let diagnostics = interp.load(&compilation.module.expect("a module"));
        assert!(!diagnostics.has_errors());
        interp
    }

    #[test]
    fn negating_a_measurement_keeps_the_uncertainty_positive() {
        let mut interp = interpreter();
        let measurement = measurement_record(
            NumVal::float(2.5, Dim::one()),
            NumVal::float(0.3, Dim::one()),
        );
        let negated = interp
            .unary(UnOp::Neg, measurement, Span::default())
            .expect("the negation runs");
        let (value, uncertainty) = measurement_parts(&negated).expect("still a measurement");
        assert_eq!(value.to_f64(), Some(-2.5));
        assert_eq!(uncertainty.to_f64(), Some(0.3));

        // `scalar - measurement` is not a declared §8.14 operator: it stays an
        // `E0302` from the ordinary numeric path.
        let error = interp
            .apply_binary(
                "-",
                Value::Num(NumVal::float(0.0, Dim::one())),
                negated,
                Span::default(),
            )
            .expect_err("scalar - measurement is not declared");
        assert_eq!(error.code, "E0302");
    }
}

#[cfg(test)]
mod runtime_type_dispatch_tests {
    use super::*;
    use aevia_types::Type;

    fn record(
        first: Value,
        second: Value,
    ) -> Value {
        let fields = [("first".to_string(), first), ("second".to_string(), second)]
            .into_iter()
            .collect();
        Value::Record(Rc::new(value::RecordValue { fields }))
    }

    #[test]
    fn adt_payload_evidence_requires_every_parameter_and_consistency() {
        let id = adt::allocate_in(
            keys::ModulePath::new(["DispatchEvidenceTest"]),
            "Pair",
            vec!["a".to_string()],
        );
        adt::set_ctors(
            id,
            vec![adt::Ctor {
                name: "Pair".to_string(),
                fields: vec![Type::Param(0), Type::Param(0)],
            }],
        );
        let head = Type::Adt(id, vec![Type::int()]);
        let pair = make_variant(id, 0, "Pair", vec![Value::int(1), Value::int(2)]);
        assert!(runtime_head_matches(&head, &pair, &mut HashMap::new()));
        let contradiction = make_variant(id, 0, "Pair", vec![Value::int(1), Value::string("x")]);
        assert!(!runtime_head_matches(
            &head,
            &contradiction,
            &mut HashMap::new()
        ));
        let incomplete = make_variant(id, 0, "Pair", vec![Value::int(1)]);
        assert!(!runtime_head_matches(
            &head,
            &incomplete,
            &mut HashMap::new()
        ));

        let nested_field = adt::allocate_in(
            keys::ModulePath::new(["DispatchEvidenceTest"]),
            "NestedField",
            vec!["a".to_string()],
        );
        adt::set_ctors(
            nested_field,
            vec![adt::Ctor {
                name: "NestedField".into(),
                fields: vec![Type::option(Type::Param(0)), Type::Param(0)],
            }],
        );
        let consistent = make_variant(
            nested_field,
            0,
            "NestedField",
            vec![
                make_variant(adt::OPTION_ID, 1, "Some", vec![Value::int(1)]),
                Value::int(1),
            ],
        );
        assert!(runtime_head_matches(
            &Type::Adt(nested_field, vec![Type::int()]),
            &consistent,
            &mut HashMap::new(),
        ));
        let inconsistent = make_variant(
            nested_field,
            0,
            "NestedField",
            vec![
                make_variant(adt::OPTION_ID, 1, "Some", vec![Value::string("x")]),
                Value::int(1),
            ],
        );
        assert!(!runtime_head_matches(
            &Type::Adt(nested_field, vec![Type::int()]),
            &inconsistent,
            &mut HashMap::new(),
        ));

        let phantom = adt::allocate_in(
            keys::ModulePath::new(["DispatchEvidenceTest"]),
            "Phantom",
            vec!["a".to_string()],
        );
        adt::set_ctors(
            phantom,
            vec![adt::Ctor {
                name: "Empty".into(),
                fields: vec![],
            }],
        );
        let value = make_variant(phantom, 0, "Empty", vec![]);
        assert!(!runtime_head_matches(
            &Type::Adt(phantom, vec![Type::int()]),
            &value,
            &mut HashMap::new(),
        ));

        let nested = make_variant(
            adt::OPTION_ID,
            1,
            "Some",
            vec![make_variant(adt::OPTION_ID, 1, "Some", vec![Value::int(1)])],
        );
        assert!(runtime_head_matches(
            &Type::option(Type::option(Type::int())),
            &nested,
            &mut HashMap::new(),
        ));
        let absent = make_variant(adt::OPTION_ID, 0, "None", vec![]);
        assert!(!runtime_head_matches(
            &Type::option(Type::int()),
            &absent,
            &mut HashMap::new()
        ));
        let mut typed_absent = absent;
        if let Value::Variant(variant) = &mut typed_absent {
            Rc::make_mut(variant).type_args = Some(vec![Type::int()]);
        }
        assert!(runtime_head_matches(
            &Type::option(Type::int()),
            &typed_absent,
            &mut HashMap::new()
        ));
        let mut forged = make_variant(adt::OPTION_ID, 1, "Some", vec![Value::string("x")]);
        if let Value::Variant(variant) = &mut forged {
            Rc::make_mut(variant).type_args = Some(vec![Type::int()]);
        }
        assert!(!runtime_head_matches(
            &Type::option(Type::int()),
            &forged,
            &mut HashMap::new()
        ));
    }

    #[test]
    fn constructor_evidence_checks_dimensions_across_fields() {
        let id = adt::allocate_in(
            keys::ModulePath::new(["DispatchEvidenceTest"]),
            "Measured",
            vec!["a".to_string()],
        );
        let dimension = Dim::var(7_001);
        adt::set_ctors(
            id,
            vec![adt::Ctor {
                name: "Measured".into(),
                fields: vec![
                    Type::float(dimension.clone()),
                    Type::float(dimension),
                    Type::Param(0),
                ],
            }],
        );
        let scalar = |dim| Value::Num(NumVal::float(1.0, dim));
        let consistent = make_variant(
            id,
            0,
            "Measured",
            vec![
                scalar(Dim::base("L")),
                scalar(Dim::base("L")),
                Value::int(1),
            ],
        );
        let contradictory = make_variant(
            id,
            0,
            "Measured",
            vec![
                scalar(Dim::base("L")),
                scalar(Dim::base("T")),
                Value::int(1),
            ],
        );
        let head = Type::Adt(id, vec![Type::int()]);
        assert!(runtime_head_matches(
            &head,
            &consistent,
            &mut HashMap::new()
        ));
        assert!(!runtime_head_matches(
            &head,
            &contradictory,
            &mut HashMap::new()
        ));

        let fixed = adt::allocate_in(
            keys::ModulePath::new(["DispatchEvidenceTest"]),
            "FixedDimension",
            vec!["a".to_string()],
        );
        adt::set_ctors(
            fixed,
            vec![adt::Ctor {
                name: "FixedDimension".into(),
                fields: vec![Type::float(Dim::base("L")), Type::Param(0)],
            }],
        );
        let bad = make_variant(
            fixed,
            0,
            "FixedDimension",
            vec![scalar(Dim::base("T")), Value::int(1)],
        );
        assert!(!runtime_head_matches(
            &Type::Adt(fixed, vec![Type::int()]),
            &bad,
            &mut HashMap::new(),
        ));
    }

    #[test]
    fn erased_exact_tensors_are_not_guessed_equal() {
        let tensor = || {
            Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape: vec![0],
                storage: Storage::Exact(Vec::new()),
                dim: Dim::one(),
                display: None,
            }))
        };
        let head = Type::record(vec![
            ("first".into(), Type::Var(9)),
            ("second".into(), Type::Var(9)),
        ]);
        assert!(!runtime_head_matches(
            &head,
            &record(tensor(), tensor()),
            &mut HashMap::new()
        ));
        let one_wildcard = Type::record(vec![
            ("first".into(), Type::Var(9)),
            ("second".into(), Type::Var(10)),
        ]);
        assert!(runtime_head_matches(
            &one_wildcard,
            &record(tensor(), tensor()),
            &mut HashMap::new()
        ));
    }

    #[test]
    fn repeated_variables_compare_runtime_types_and_dimensions() {
        let head = Type::record(vec![
            ("first".into(), Type::Var(9)),
            ("second".into(), Type::Var(9)),
        ]);
        let int = || Value::int(1);
        assert!(runtime_head_matches(
            &head,
            &record(int(), int()),
            &mut HashMap::new()
        ));
        assert!(!runtime_head_matches(
            &head,
            &record(int(), Value::string("different")),
            &mut HashMap::new(),
        ));
        let metre = Dim::base("L");
        let second = Dim::base("T");
        assert!(!runtime_head_matches(
            &head,
            &record(
                Value::Num(NumVal::float(1.0, metre)),
                Value::Num(NumVal::float(1.0, second)),
            ),
            &mut HashMap::new(),
        ));
        let unknown = || Value::Num(NumVal::float(1.0, Dim::var(8_001)));
        assert!(!runtime_head_matches(
            &head,
            &record(unknown(), unknown()),
            &mut HashMap::new()
        ));
    }
}

#[cfg(test)]
mod ast_stack_budget_tests {
    #[test]
    fn ast_reaches_the_default_call_guard_and_restores_lexical_state() {
        std::thread::Builder::new().stack_size(256*1024*1024).spawn(|| {
            let source = "fn down(n: Int): Int = 1 + down(n + 1)\nfn main(): IO Unit = do\n    println(show(down(0)))\n";
            let compilation = crate::compile("ast-call-budget.aev",source);
            assert!(!compilation.diagnostics.has_errors(),"{:?}",compilation.diagnostics.items);
            let mut interp = super::Interp::new(&compilation.checked).with_mode(crate::Mode::Ast).with_buffer();
            assert_eq!(interp.max_depth,2048,"preserve the language's existing default guard");
            let loaded = interp.load(compilation.module.as_ref().unwrap());
            assert!(!loaded.has_errors(),"{:?}",loaded.items);
            let failure = interp.run_main().unwrap_err();
            assert_eq!(failure.code,"E0805");
            assert!(failure.stack.iter().any(|frame| frame.function.as_ref() == "down"));
            assert_eq!(interp.depth,0);
            assert_eq!(interp.nodes,0);
            assert!(interp.frames.is_empty());
            assert!(interp.bindings.is_none());
        }).unwrap().join().unwrap();
    }
}

/// A unit's scale as a dimensionless number in the phase of the value it
/// scales: an f64 for a `Float` (so `Float` stays `Float`), exact otherwise,
/// with a factor of `pi` kept symbolic (`deg`).
fn unit_scale_value(
    unit: &UnitDef,
    float: bool,
) -> NumVal {
    if float {
        return NumVal::float(unit.scale.to_f64(), Dim::one());
    }
    if unit.scale.pi == 0 {
        return NumVal::exact(unit.scale.rat.clone(), Dim::one());
    }
    let term = crate::sym::Term::mul(
        crate::sym::Term::Num(unit.scale.rat.clone()),
        crate::sym::Term::pow(
            crate::sym::Term::Sym("pi".to_string()),
            BigRational::from_integer(BigInt::from(unit.scale.pi)),
        ),
    );
    NumVal::symbolic(term, Dim::one())
}

/// The display unit of `a op b` (§5.4.2).
///
/// * `+`/`-`: the same display unit on both sides is kept. A point and a
///   difference (`25 °C + 10.0 K`, §5.6.3) keep the point's unit, because
///   the result is that point moved; two points subtract to a difference,
///   which has no display of its own.
/// * `*`/`/`: when one side is dimensionless, the other side's display is
///   kept (`2 * 3 km` is `6 km`), unless that display is affine — scaling a
///   temperature point has no meaning in the display unit.
/// * everything else drops the display and prints in SI.
fn propagated_display(
    op: BinOp,
    a: &NumVal,
    b: &NumVal,
) -> Option<Rc<UnitDef>> {
    let same = |x: &Rc<UnitDef>, y: &Rc<UnitDef>| x.display == y.display && x.dim == y.dim;
    match op {
        | BinOp::Add | BinOp::Sub => {
            match (&a.display, &b.display) {
                | (Some(x), Some(y)) if x.affine && y.affine => None,
                | (Some(x), _) if x.affine => Some(x.clone()),
                | (_, Some(y)) if y.affine && op == BinOp::Add => Some(y.clone()),
                | (Some(x), Some(y)) if same(x, y) => Some(x.clone()),
                | _ => None,
            }
        },
        | BinOp::Mul => {
            if b.dim.is_one() && b.display.is_none() {
                a.display.clone().filter(|unit| !unit.affine)
            } else if a.dim.is_one() && a.display.is_none() {
                b.display.clone().filter(|unit| !unit.affine)
            } else {
                None
            }
        },
        | BinOp::Div if b.dim.is_one() && b.display.is_none() => {
            a.display.clone().filter(|unit| !unit.affine)
        },
        | _ => None,
    }
}

/// `s[i]` and `s[i..j]` / `s[i:j:k]` on a string (§8.1.4): positions count
/// grapheme clusters, so `"é"` written as `e` + combining acute is one
/// position. Negative positions count from the end, as on tensors.
fn index_string(
    text: &str,
    indices: &[SliceValue],
    span: Span,
) -> Result<Value, EvalError> {
    use unicode_segmentation::UnicodeSegmentation as _;
    let clusters: Vec<&str> = text.graphemes(true).collect();
    let extent = clusters.len();
    let [index] = indices else {
        return Err(EvalError::new(
            "E0302",
            format!("a string has one axis, found {} indices", indices.len()),
        )
        .at(span));
    };
    match index {
        | SliceValue::Index(value) => {
            let coordinate = value
                .ok_or_else(|| EvalError::new("E0802", "index must be an Int").at(span))?
                as isize;
            let position = normalize_index(coordinate, extent).ok_or_else(|| {
                EvalError::new(
                    "E0802",
                    format!(
                        "index {coordinate} is out of bounds for a string of {extent} characters"
                    ),
                )
                .at(span)
            })?;
            Ok(Value::string(clusters[position]))
        },
        | SliceValue::Range {
            start,
            end,
            inclusive,
            step,
        } => {
            if let Some(step) = step {
                let (first, count, stride) = walk_range(*start, *end, *step, extent, span)?;
                let mut out = String::new();
                for offset in 0..count {
                    let position = first as isize + offset as isize * stride;
                    if let Some(cluster) =
                        usize::try_from(position).ok().and_then(|p| clusters.get(p))
                    {
                        out.push_str(cluster);
                    }
                }
                return Ok(Value::string(out));
            }
            let start_value = start.map_or(0, |value| value as isize);
            let end_value = end.map_or(extent as isize, |value| value as isize);
            let first = normalize_bound(start_value, extent, false);
            let last = normalize_bound(end_value, extent, *inclusive);
            if first > last {
                return Err(EvalError::new("E0802", "empty slice").at(span));
            }
            Ok(Value::string(clusters[first..last].concat()))
        },
    }
}

/// Removes the axes an integer index selected (§6.6.4).
///
/// The slice that produced `value` kept them with extent one; the storage is
/// reshaped (a view stays a view when its elements are contiguous) rather than
/// copied element by element.
fn drop_indexed_axes(
    value: Value,
    dropped: &[bool],
) -> Value {
    let Value::Tensor(tensor) = &value else {
        return value;
    };
    if !dropped.iter().any(|drop| *drop) {
        return value;
    }
    let shape: Vec<usize> = tensor
        .shape
        .iter()
        .enumerate()
        .filter(|(axis, _)| !dropped.get(*axis).copied().unwrap_or(false))
        .map(|(_, extent)| *extent)
        .collect();
    let mut data = (**tensor).clone();
    match tensor.reshape_storage(&shape) {
        | Some(storage) => data.storage = storage,
        | None => {
            let elements = tensor.elements();
            let rebuilt = build_tensor(elements, shape.clone(), tensor.dim.clone());
            let Value::Tensor(rebuilt) = rebuilt else {
                return value;
            };
            data.storage = rebuilt.storage.clone();
        },
    }
    data.shape = shape;
    data.verified_element = None;
    Value::Tensor(Rc::new(data))
}
