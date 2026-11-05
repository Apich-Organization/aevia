//! The interpreter: values in, values out.
//!
//! Evaluation is strict and left-to-right (§6.1). The interpreter walks the
//! typed AST directly; the Core IR of §9.2.2 is not materialised yet, which is
//! recorded in `STATUS.md`. What matters for behaviour is preserved: closures
//! capture lexical scope, `do` builds IO actions instead of running them,
//! pattern matching is first-match, and arithmetic promotes `Int < Real <
//! Float < Complex` while keeping exactness when it can.

use crate::value::{
    self, ctor_name, format_scalar, make_variant, Closure, ClosureBody, ClosureEnv, DynValue,
    IoAction, NativeFn, NumKind, NumVal, PropValue, RecordValue, Scope, Storage, TensorData, Value,
};
use aevia_dim::{Dim, DimEnv, UnitDef, UnitTable};
use aevia_syntax::ast::*;
use aevia_syntax::{Diagnostic, Span};
use aevia_types::adt;
use aevia_types::check::{
    BuiltinDispatch, CheckedModule, DictTarget, DictionaryUse, Export, InstanceExport,
    ModuleInterface,
};
use aevia_types::class::{has_variables, ClassTable, Resolution};
use aevia_types::keys::{self, ClassKey, InstanceKey, RepKind, StorageClass, Witness};
use aevia_types::ty::Rep;
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, ToPrimitive, Zero};
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
    /// The witness of the head type, for `DictTarget::Witness` dispatch.
    pub(crate) witness: Option<aevia_types::keys::Witness>,
    /// The head as the *source* spells it, when that spelling is also the
    /// name the checked type prints as (a nominal type). Kept for the legacy
    /// shape dispatch.
    pub(crate) head: Option<String>,
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
    fn accepts(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Nominal(name), Value::Variant(variant)) => {
                adt::name_of(variant.adt) == *name
            }
            (Self::Tensor, Value::Tensor(_)) => true,
            (Self::Record(labels), Value::Record(record)) => {
                record.fields.len() == labels.len()
                    && labels.iter().all(|label| record.fields.contains_key(label))
            }
            (Self::Scalar(rep), Value::Num(num)) => match (rep, &num.kind) {
                (&"Int", NumKind::Int(_)) => true,
                (&"Real", NumKind::Exact(_) | NumKind::Sym(_)) => true,
                (&"Float", NumKind::Float(_)) => true,
                (&"Complex", NumKind::Complex(..)) => true,
                _ => false,
            },
            (Self::Bool, Value::Bool(_))
            | (Self::Str, Value::Str(_))
            | (Self::Unit, Value::Unit)
            | (Self::Prop, Value::Prop(_)) => true,
            _ => false,
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
pub(crate) fn head_shape(ty: &Ty, aliases: &HashMap<String, aevia_types::ty::Type>) -> HeadShape {
    match &ty.kind {
        TyKind::Named { path, .. } => path.last().cloned().map_or(HeadShape::Unknown, |name| {
            if name.chars().next().is_some_and(char::is_uppercase) {
                // An alias is transparent: `type Vec2 = { x, y }` names a
                // record, and the value at run time is a record, not a variant.
                aliases
                    .get(&name)
                    .map_or(HeadShape::Nominal(name), |aliased| type_head_shape(aliased))
            } else {
                HeadShape::Unknown
            }
        }),
        TyKind::Tensor { .. } | TyKind::Matrix { .. } | TyKind::Sparse { .. } => HeadShape::Tensor,
        TyKind::Record { fields, .. } => {
            let mut labels: Vec<String> = fields.iter().map(|(name, _)| name.clone()).collect();
            labels.sort();
            HeadShape::Record(labels)
        }
        TyKind::Int => HeadShape::Scalar("Int"),
        TyKind::Real(_) => HeadShape::Scalar("Real"),
        TyKind::Float(_) => HeadShape::Scalar("Float"),
        TyKind::Complex(_) => HeadShape::Scalar("Complex"),
        TyKind::Bool => HeadShape::Bool,
        TyKind::Str => HeadShape::Str,
        TyKind::Unit => HeadShape::Unit,
        TyKind::Prop => HeadShape::Prop,
        _ => HeadShape::Unknown,
    }
}

/// The runtime shape of a *checked* type, used to see through an alias.
fn type_head_shape(ty: &aevia_types::ty::Type) -> HeadShape {
    use aevia_types::ty::Type;
    match ty {
        Type::Adt(id, _) => HeadShape::Nominal(adt::name_of(*id)),
        Type::Record(row) => {
            let mut labels = Vec::new();
            let mut current = row;
            while let aevia_types::ty::Row::Extend { label, tail, .. } = current {
                labels.push(label.clone());
                current = tail;
            }
            labels.sort();
            HeadShape::Record(labels)
        }
        Type::Tensor { .. } => HeadShape::Tensor,
        Type::Scalar(rep, _) => HeadShape::Scalar(rep.name()),
        Type::Bool => HeadShape::Bool,
        Type::Str => HeadShape::Str,
        Type::Unit => HeadShape::Unit,
        _ => HeadShape::Unknown,
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
}

impl EvalError {
    /// Builds an error.
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            span: None,
            notes: Vec::new(),
            stack: Vec::new(),
            payload: None,
        }
    }

    /// Attaches a span.
    #[must_use]
    pub fn at(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    /// Attaches a call stack, when the error does not have one yet.
    #[must_use]
    pub fn with_stack(mut self, stack: Vec<Frame>) -> Self {
        if self.stack.is_empty() {
            self.stack = stack;
        }
        self
    }

    /// The call stack rendered as notes, outermost call first.
    #[must_use]
    pub fn trace(&self) -> Vec<String> {
        let mut notes = Vec::with_capacity(self.stack.len());
        for (depth, frame) in self.stack.iter().enumerate() {
            notes.push(format!("{} in `{}`", depth + 1, frame.function.as_ref()));
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

/// The interpreter state for one program.
pub struct Interp {
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
    /// The non-derived `instance` exports of the module being loaded, in
    /// declaration order. The source `instance` declarations are matched to
    /// them so a body can be bound under the same `InstanceKey::mangle()` name
    /// the checker derived (round-2 decision 1a).
    pub instance_exports: Vec<InstanceExport>,
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
fn measurement_parts(value: &Value) -> Option<(NumVal, NumVal)> {
    let Value::Record(record) = value else {
        return None;
    };
    if record.fields.len() != 2 {
        return None;
    }
    match (
        record.fields.get("value"),
        record.fields.get("uncertainty"),
    ) {
        (Some(Value::Num(value)), Some(Value::Num(uncertainty)))
            if value.dim == uncertainty.dim =>
        {
            Some((value.clone(), uncertainty.clone()))
        }
        _ => None,
    }
}

/// A measurement record, fields in spec order (§8.14).
fn measurement_record(value: NumVal, uncertainty: NumVal) -> Value {
    let mut fields = IndexMap::new();
    fields.insert("value".to_string(), Value::Num(value));
    fields.insert("uncertainty".to_string(), Value::Num(uncertainty));
    Value::Record(Rc::new(RecordValue { fields }))
}

impl Interp {
    /// Creates an interpreter from a checked module.
    #[must_use]
    pub fn new(checked: &CheckedModule) -> Self {
        Self {
            globals: HashMap::new(),
            units: checked.units.clone(),
            dims: checked.dims.clone(),
            exports: checked.exports.clone(),
            adts: HashMap::new(),
            output: Output::Console,
            symbol_counts: HashMap::new(),
            seed: 0x2545_f491_4f6c_dd1d,
            max_depth: 2048,
            max_tail_calls: 100_000_000,
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
            instance_exports: checked
                .instances
                .iter()
                .filter(|export| !export.derived)
                .cloned()
                .collect(),
            next_instance_export: 0,
            class_keys: checked
                .classes
                .keys()
                .map(|key| (key.name.clone(), key.clone()))
                .collect(),
            current_module: checked.module_path(),
            instance_methods: HashMap::new(),
            method_classes: HashMap::new(),
            prelude_classes: ClassTable::prelude(),
            aliases: checked.aliases.iter().map(|(name, ty)| (name.clone(), ty.clone())).collect(),
        }
    }

    /// Sets the arguments `args()` returns (§9.12).
    ///
    /// The host calls this before `run_main`; the interpreter holds them as
    /// plain strings and `args()` answers a rank-1 `Tensor<1, String>`, so a
    /// program can index them like any other vector. Building the tensor is
    /// deferred to the call, so setting them cannot fail.
    pub fn set_args(&mut self, arguments: impl IntoIterator<Item = String>) {
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
    pub fn add_dictionaries(&mut self, uses: &[DictionaryUse]) {
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
    pub fn add_rssn_calls(&mut self, calls: &[aevia_types::check::RssnCall]) {
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
    pub fn prepare_module(&mut self, checked: &CheckedModule) {
        self.current_module = checked.module_path();
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
    pub fn add_interface(&mut self, interface: &ModuleInterface) {
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
        let export = self.instance_exports.get(self.next_instance_export).cloned();
        if export.is_some() {
            self.next_instance_export += 1;
        }
        export
    }

    /// The global name of one instance method body.
    ///
    /// `InstanceKey::mangle()` is the single spelling the checker and the
    /// runtime both use (round-2 decision 1a); `Pretty@Int` and
    /// `Pretty@String` therefore get different globals.
    #[must_use]
    pub fn instance_global(key: &InstanceKey, method: &str) -> String {
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
        let (key, witness) = export.map_or((None, None), |export| {
            (Some(export.key.clone()), keys::witness_of(&export.head))
        });
        let name = match &key {
            Some(key) => Self::instance_global(key, &method.name),
            None => format!(
                "{class}@{}::{}",
                spelled.as_deref().unwrap_or("?"),
                method.name
            ),
        };
        let closure = Value::Closure(Rc::new(Closure::ast(
            method.params.iter().map(|param| param.pattern.clone()).collect(),
            method.body.clone(),
            env.clone(),
            Some(name.clone()),
        )));
        self.globals.insert(name.clone(), closure);
        self.method_classes
            .insert(method.name.clone(), class.to_string());
        self.instance_methods
            .entry(format!("{class}::{}", method.name))
            .or_default()
            .push(InstanceEntry {
                key,
                witness,
                head: spelled,
                shape: head_shape(head, &self.aliases),
                global: name,
            });
    }

    /// The dictionary the checker recorded for a class-method call at `span`.
    pub(crate) fn dictionary_use(&self, span: Span, method: &str) -> Option<DictionaryUse> {
        self.dictionaries
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
    fn is_builtin_instance(&self, use_site: &DictionaryUse) -> bool {
        matches!(
            self.prelude_classes.resolve(&use_site.class, &use_site.head),
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
        let entries = self.instance_methods.get(&format!("{}::{method}", use_site.class));
        // The checker pinned the target (round-2 decision 1c); this is the
        // authority. A concrete instance runs by its symbolic key (so two
        // primitive heads never share a global), a quantified site dispatches
        // among the proved candidates by value witness, and §3.8.4's
        // structural `Show` falls through to the builtin printer.
        match &use_site.target {
            DictTarget::Instance(key) => {
                let global = Self::instance_global(key, method);
                if let Some(body) = self.globals.get(&global).cloned() {
                    return Some(Dispatch::Instance(body));
                }
                // The body may still be under the legacy spelling when the
                // caller loaded a default checked module; the shape fallback
                // below finds it.
            }
            DictTarget::Builtin(BuiltinDispatch::ShowStructural) => {
                return Some(Dispatch::Builtin);
            }
            DictTarget::Witness { candidates } => {
                if let (Some(pool), Some(first)) = (entries, args.first()) {
                    if let Some(witness) = witness_of_value(first) {
                        let mut chosen: Option<&InstanceEntry> = None;
                        let mut ambiguous = false;
                        for entry in pool {
                            let Some(entry_key) = &entry.key else { continue };
                            if !candidates.contains(entry_key) {
                                continue;
                            }
                            if entry.witness.as_ref() != Some(&witness) {
                                continue;
                            }
                            if chosen.is_some() {
                                ambiguous = true;
                            }
                            chosen = Some(entry);
                        }
                        if !ambiguous {
                            if let Some(entry) = chosen {
                                if let Some(body) = self.globals.get(&entry.global).cloned() {
                                    return Some(Dispatch::Instance(body));
                                }
                            }
                        }
                        // An ambiguous or unmatched witness is not guessed at:
                        // `resolve_dictionary` reports `E0310` with the class
                        // and head, which is the runtime defence behind the
                        // checker's `E0311` witness-disjointness rule.
                    }
                }
            }
        }
        // (a) The argument's runtime shape decides. At a concrete call site it
        //     agrees with the record; inside a polymorphic function it is the
        //     only thing that is concrete, because the instance depends on the
        //     caller.
        if let (Some(entries), Some(first)) = (entries, args.first()) {
            let mut matched = entries.iter().filter(|entry| entry.shape.accepts(first));
            if let Some(entry) = matched.next() {
                if matched.next().is_none() {
                    if let Some(body) = self.globals.get(&entry.global).cloned() {
                        return Some(Dispatch::Instance(body));
                    }
                }
            }
        }
        // (b) A prelude instance is the builtin operation (§3.8.4). Coherence
        //     keeps user heads disjoint from prelude heads, so this is the
        //     instance the checker proved.
        if self.is_builtin_instance(use_site) {
            return Some(Dispatch::Builtin);
        }
        // (c) A class with no user instance is a builtin class; the operation
        //     is the global of the method name.
        let Some(entries) = entries.filter(|entries| !entries.is_empty()) else {
            return Some(Dispatch::Builtin);
        };
        // (d) The head the checker recorded.
        let head = use_site.head.to_string();
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.head.as_deref() == Some(head.as_str()))
        {
            if let Some(body) = self.globals.get(&entry.global).cloned() {
                return Some(Dispatch::Instance(body));
            }
        }
        // (e) A class with a single user instance can only have proved that
        //     one.
        if entries.len() == 1 {
            if let Some(body) = self.globals.get(&entries[0].global).cloned() {
                return Some(Dispatch::Instance(body));
            }
        }
        // (f) The record is a snapshot taken before the head was known, and no
        //     user instance accepts the argument, so the operation is the
        //     prelude one.
        if has_variables(&use_site.head) {
            return Some(Dispatch::Builtin);
        }
        None
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
                let mut matched = entries.iter().filter(|entry| entry.shape.accepts(first));
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
    pub fn with_mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    /// The captured output, when buffering.
    #[must_use]
    pub fn captured(&self) -> String {
        match &self.output {
            Output::Buffer(buffer) => buffer.borrow().clone(),
            Output::Console => String::new(),
        }
    }

    fn write_out(&mut self, text: &str, newline: bool, stderr: bool) {
        match &self.output {
            Output::Console => {
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
            }
            Output::Buffer(buffer) => {
                let mut buffer = buffer.borrow_mut();
                buffer.push_str(text);
                if newline {
                    buffer.push('\n');
                }
            }
        }
    }

    /// The root scope.
    #[must_use]
    pub fn root(&self) -> Rc<Scope> {
        Scope::root()
    }

    // ----- evaluation -----------------------------------------------------

    /// Evaluates an expression in a scope.
    ///
    /// # Errors
    ///
    /// Returns [`EvalError`] for runtime failures (division by zero, shape
    /// mismatches, unmatched patterns, and so on).
    pub fn eval(&mut self, expr: &Expr, env: &Rc<Scope>) -> Result<Value, EvalError> {
        self.nodes += 1;
        if self.nodes > self.max_nodes {
            self.nodes -= 1;
            return Err(EvalError::new("E0805", "expression is nested too deeply")
                .at(expr.span)
                .with_stack(self.frames.clone()));
        }
        let result = self.eval_inner(expr, env);
        self.nodes -= 1;
        result
    }

    fn eval_inner(&mut self, expr: &Expr, env: &Rc<Scope>) -> Result<Value, EvalError> {
        match &expr.kind {
            // A parser recovery node: the syntax error was already reported.
            ExprKind::Error => Ok(Value::Unit),
            ExprKind::UnitLit => Ok(Value::Unit),
            ExprKind::Bool(value) => Ok(Value::Bool(*value)),
            // §3.10.1 / §6.3.2: the checker's instantiated representation is
            // the runtime's, so a literal in a `Float` context is an f64 and one
            // in a `Real` context is exact.
            ExprKind::Int(value) => Ok(Value::Num(self.integer_literal(value, expr.span))),
            ExprKind::Float(text) => Ok(Value::Num(self.float_literal(text, expr.span))),
            ExprKind::Str(pieces) => {
                let mut out = String::new();
                for piece in pieces {
                    match piece {
                        StrPiece::Text(text) => out.push_str(text),
                        StrPiece::Expr(inner) => {
                            let value = self.eval(inner, env)?;
                            out.push_str(&value::show(&value));
                        }
                    }
                }
                Ok(Value::string(out))
            }
            ExprKind::Quantity { value, unit } => {
                let as_float = self.literal_rep(expr.span) == Some(Rep::Float);
                self.quantity(value, unit, expr.span, as_float)
            }
            ExprKind::Var(name) => self.lookup(name, env, expr.span),
            ExprKind::QName(path) => {
                if path.len() == 2 && path[0] == "Rssn" {
                    return Ok(crate::rssn_module::member(&path[1]));
                }
                self.lookup(&path.join("."), env, expr.span)
            }
            ExprKind::Unary { op, operand } => {
                let value = self.eval(operand, env)?;
                self.unary(*op, value, expr.span)
            }
            ExprKind::Binary { op, left, right } => {
                if matches!(op, BinOp::And | BinOp::Or) {
                    let left_value = self.eval(left, env)?;
                    let short_circuit = match (op, &left_value) {
                        (BinOp::And, Value::Bool(false)) => Some(false),
                        (BinOp::Or, Value::Bool(true)) => Some(true),
                        _ => None,
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
            ExprKind::Call { callee, args } => {
                if args
                    .iter()
                    .any(|arg| matches!(arg.kind, ExprKind::Placeholder))
                {
                    // `f(_, x)` is a lambda over the placeholders (§2.3.6).
                    let mut params = Vec::new();
                    let mut call_args = Vec::new();
                    for (index, arg) in args.iter().enumerate() {
                        if matches!(arg.kind, ExprKind::Placeholder) {
                            let name = format!("__arg{index}");
                            params.push(Pat::new(PatKind::Var(name.clone()), arg.span));
                            call_args.push(Expr::new(ExprKind::Var(name), arg.span));
                        } else {
                            call_args.push(arg.clone());
                        }
                    }
                    let body = Expr::new(
                        ExprKind::Call {
                            callee: callee.clone(),
                            args: call_args,
                        },
                        expr.span,
                    );
                    return Ok(Value::Closure(Rc::new(Closure::ast(
                        params,
                        body,
                        env.clone(),
                        None,
                    ))));
                }
                // `zeros<Int>(3)` names the element type the result must have,
                // and `reshape<[3,2]>` names its target shape. Every other
                // path drops the type argument, which is why `zeros<Int>`
                // silently produced `f64` storage (§8.4) and why `reshape`
                // could not know what to reshape to.
                if let ExprKind::TypeApp {
                    callee: inner,
                    args: type_args,
                } = &callee.kind
                {
                    if let ExprKind::Var(name) = &inner.kind {
                        if crate::builtins::takes_type_argument(name) {
                            let mut values = Vec::with_capacity(args.len());
                            for arg in args {
                                values.push(self.eval(arg, env)?);
                            }
                            return crate::builtins::call_typed(
                                self, name, type_args, values, expr.span,
                            );
                        }
                    }
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
                let function = self.eval(callee, env)?;
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.eval(arg, env)?);
                }
                self.apply(function, values, expr.span)
            }
            ExprKind::TypeApp { callee, .. } => self.eval(callee, env),
            ExprKind::Lambda { params, body, .. } => Ok(Value::Closure(Rc::new(Closure::ast(
                params.iter().map(|param| param.pattern.clone()).collect(),
                (**body).clone(),
                env.clone(),
                None,
            )))),
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => match self.eval(cond, env)? {
                Value::Bool(true) => self.eval(then_branch, env),
                Value::Bool(false) => self.eval(else_branch, env),
                other => Err(EvalError::new(
                    "E0302",
                    format!("`if` expects a Bool condition, found {}", other.type_name()),
                )
                .at(cond.span)),
            },
            ExprKind::Match { scrutinee, arms } => {
                let subject = self.eval(scrutinee, env)?;
                for arm in arms {
                    let scope = Scope::child(env);
                    if self.match_pattern(&arm.pattern, &subject, &scope)? {
                        if let Some(guard) = &arm.guard {
                            match self.eval(guard, &scope)? {
                                Value::Bool(true) => {}
                                Value::Bool(false) => continue,
                                other => {
                                    return Err(EvalError::new(
                                        "E0308",
                                        format!(
                                            "a match guard must be Bool, found {}",
                                            other.type_name()
                                        ),
                                    )
                                    .at(guard.span));
                                }
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
            ExprKind::LetIn {
                pattern,
                value,
                body,
                ..
            } => {
                let bound = self.eval(value, env)?;
                let scope = Scope::child(env);
                self.bind_pattern(pattern, &bound, &scope)?;
                self.eval(body, &scope)
            }
            ExprKind::Block { stmts, tail } => {
                // One scope per *binding*, not one per block. A block-wide
                // scope is mutated by every `let`, so a closure created before
                // a later `let x = …` in the same block would read the later
                // value — a cell, not a value, and not what the lexical scope
                // says. Extending the chain keeps every captured scope frozen.
                let mut scope = Scope::child(env);
                for stmt in stmts {
                    match stmt {
                        BlockStmt::Let { pattern, value, .. } => {
                            let bound = self.eval(value, &scope)?;
                            let next = Scope::child(&scope);
                            self.bind_pattern(pattern, &bound, &next)?;
                            scope = next;
                        }
                        BlockStmt::Fn(decl) => {
                            if let DeclKind::Fn {
                                name, args, body, ..
                            } = &decl.kind
                            {
                                let closure = Value::Closure(Rc::new(Closure::ast(
                                    args.iter().map(|arg| arg.pattern.clone()).collect(),
                                    body.clone(),
                                    scope.clone(),
                                    Some(name.clone()),
                                )));
                                let next = Scope::child(&scope);
                                next.vars.borrow_mut().insert(name.clone(), closure);
                                scope = next;
                            }
                        }
                    }
                }
                match tail {
                    Some(tail) => self.eval(tail, &scope),
                    None => Ok(Value::Unit),
                }
            }
            ExprKind::Do { stmts } => self.eval_do(stmts, env),
            ExprKind::Record { base, fields } => {
                let mut collected: IndexMap<String, Value> = IndexMap::new();
                if let Some(base) = base {
                    match self.eval(base, env)? {
                        Value::Record(record) => {
                            for (name, value) in &record.fields {
                                collected.insert(name.clone(), value.clone());
                            }
                        }
                        other => {
                            return Err(EvalError::new(
                                "E0302",
                                format!("record update expects a record, found {}", other.type_name()),
                            )
                            .at(base.span));
                        }
                    }
                }
                for (name, value) in fields {
                    let evaluated = self.eval(value, env)?;
                    collected.insert(name.clone(), evaluated);
                }
                Ok(Value::Record(Rc::new(RecordValue { fields: collected })))
            }
            ExprKind::Field { base, name } => {
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
                Value::Record(record) => record.fields.get(name).cloned().ok_or_else(|| {
                    EvalError::new("E0306", format!("no field `{name}`")).at(expr.span)
                }),
                other => Err(EvalError::new(
                    "E0302",
                    format!("field access expects a record, found {}", other.type_name()),
                )
                .at(expr.span)),
                }
            }
            ExprKind::Tuple(items) => {
                let mut fields = IndexMap::new();
                for (index, item) in items.iter().enumerate() {
                    fields.insert(index.to_string(), self.eval(item, env)?);
                }
                Ok(Value::Record(Rc::new(RecordValue { fields })))
            }
            ExprKind::Tensor(items) => self.tensor_literal(items, env, expr.span),
            ExprKind::Index { base, indices } => {
                let tensor = self.eval(base, env)?;
                self.index_value(tensor, indices, env, expr.span)
            }
            ExprKind::Annot { expr: inner, .. } => self.eval(inner, env),
            ExprKind::Placeholder => Err(EvalError::new(
                "E0202",
                "`_` cannot be evaluated outside a call",
            )
            .at(expr.span)),
            ExprKind::Try(inner) => {
                let value = self.eval(inner, env)?;
                match &value {
                    Value::Variant(variant) if variant.adt == adt::RESULT_ID => {
                        if variant.index == 0 {
                            variant
                                .fields
                                .first()
                                .cloned()
                                .ok_or_else(|| EvalError::new("E0809", "malformed Ok"))
                        } else {
                            let mut error = EvalError::new("E0702", "propagated Err").at(expr.span);
                            error.payload = Some(value.clone());
                            Err(error)
                        }
                    }
                    Value::Variant(variant) if variant.adt == adt::OPTION_ID => {
                        if variant.index == 1 {
                            variant
                                .fields
                                .first()
                                .cloned()
                                .ok_or_else(|| EvalError::new("E0809", "malformed Some"))
                        } else {
                            let mut error = EvalError::new("E0702", "propagated None").at(expr.span);
                            error.payload = Some(value.clone());
                            Err(error)
                        }
                    }
                    other => Err(EvalError::new(
                        "E0702",
                        format!("`?` expects Result or Option, found {}", other.type_name()),
                    )
                    .at(expr.span)),
                }
            }
            ExprKind::Factorial { expr: inner, double } => {
                let value = self.eval(inner, env)?;
                let n = match &value {
                    Value::Num(num) => match &num.kind {
                        NumKind::Int(value) => value
                            .to_i64()
                            .ok_or_else(|| EvalError::new("E0801", "factorial is too large"))?,
                        _ => {
                            return Err(EvalError::new(
                                "E0302",
                                "factorial expects an Int",
                            )
                            .at(expr.span));
                        }
                    },
                    _ => {
                        return Err(
                            EvalError::new("E0302", "factorial expects an Int").at(expr.span)
                        );
                    }
                };
                Ok(Value::Num(NumVal::int(factorial(n, *double))))
            }
        }
    }

    fn lookup(&mut self, name: &str, env: &Rc<Scope>, span: Span) -> Result<Value, EvalError> {
        if let Some(value) = env.lookup(name) {
            return Ok(value);
        }
        self.lookup_global(name, None, span)
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
            });
        }
        if let Some(unit) = self.units.lookup(name) {
            return Ok(Value::UnitOf(Rc::new(unit)));
        }
        if let Some((adt, index, registered)) = adt::find_ctor(name) {
            let arity = registered
                .ctors
                .get(index)
                .map_or(0, |ctor| ctor.fields.len());
            // A nullary constructor referenced as a value *is* its variant
            // (§3.6.2): `let x: M = A` stores `A`, not a zero-argument
            // constructor waiting to be applied. Only a constructor with fields
            // stays applicable.
            if arity == 0 {
                return Ok(make_variant(adt, index, name, Vec::new()));
            }
            return Ok(Value::Ctor {
                adt,
                index,
                name: name.to_string(),
                arity,
                args: Vec::new(),
            });
        }
        Err(EvalError::new("E0301", format!("unbound name `{name}`")).at(span))
    }

    /// The representation the checker instantiated for the literal at `span`.
    fn literal_rep(&self, span: Span) -> Option<Rep> {
        self.literal_reps.get(&span).copied()
    }

    /// An integer literal in the representation its context chose (§3.10.1).
    fn integer_literal(&self, value: &BigInt, span: Span) -> NumVal {
        match self.literal_rep(span) {
            Some(Rep::Float) => NumVal::float(
                num_traits::ToPrimitive::to_f64(value).unwrap_or(f64::NAN),
                Dim::one(),
            ),
            Some(Rep::Real) => NumVal::exact(BigRational::from_integer(value.clone()), Dim::one()),
            Some(Rep::Complex) => NumVal {
                kind: NumKind::Complex(
                    num_traits::ToPrimitive::to_f64(value).unwrap_or(f64::NAN),
                    0.0,
                ),
                dim: Dim::one(),
                display: None,
            },
            _ => NumVal::int(value.clone()),
        }
    }

    /// A decimal literal in the representation its context chose.
    ///
    /// A `Real` context keeps the exact rational the decimal text denotes
    /// (§6.3.2); a `Float` context uses the nearest `f64`.
    fn float_literal(&self, text: &str, span: Span) -> NumVal {
        let cleaned = text.replace('_', "");
        let as_float = || cleaned.parse::<f64>().unwrap_or(f64::NAN);
        match self.literal_rep(span) {
            Some(Rep::Real) => aevia_dim::parse_decimal(&cleaned)
                .map_or_else(|| NumVal::float(as_float(), Dim::one()), |value| {
                    NumVal::exact(value, Dim::one())
                }),
            Some(Rep::Complex) => NumVal {
                kind: NumKind::Complex(as_float(), 0.0),
                dim: Dim::one(),
                display: None,
            },
            _ => NumVal::float(as_float(), Dim::one()),
        }
    }

    pub(crate) fn quantity(
        &mut self,
        value: &NumberLit,
        unit: &aevia_dim::UnitExpr,
        span: Span,
        as_float: bool,
    ) -> Result<Value, EvalError> {
        let resolved = self
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
            NumberLit::Int(coefficient) => {
                let scaled = BigRational::from_integer(coefficient.clone()) * resolved.scale.rat.clone();
                NumVal::exact(scaled, resolved.dim)
            }
            NumberLit::Decimal(text) => {
                let coefficient = aevia_dim::parse_decimal(text)
                    .ok_or_else(|| EvalError::new("E0106", "invalid decimal literal").at(span))?;
                let scaled = coefficient * resolved.scale.rat.clone();
                NumVal::exact(scaled, resolved.dim)
            }
        };
        Ok(Value::Num(num.with_display(display)))
    }

    pub(crate) fn unary(&mut self, op: UnOp, value: Value, span: Span) -> Result<Value, EvalError> {
        match op {
            UnOp::Pos => Ok(value),
            UnOp::Neg => {
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
                Value::Num(mut num) => {
                    num.kind = match num.kind {
                        NumKind::Int(value) => NumKind::Int(-value),
                        NumKind::Exact(value) => NumKind::Exact(-value),
                        NumKind::Float(value) => NumKind::Float(-value),
                        NumKind::Sym(term) => NumKind::Sym(Rc::new(crate::sym::Term::neg(
                            (*term).clone(),
                        ))),
                        NumKind::Complex(re, im) => NumKind::Complex(-re, -im),
                    };
                    Ok(Value::Num(num))
                }
                Value::Tensor(tensor) => {
                    let elements = tensor
                        .elements()
                        .into_iter()
                        .map(|element| self.unary(UnOp::Neg, element, span))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(build_tensor(elements, tensor.shape.clone(), tensor.dim.clone()))
                }
                // §4.9.3: unary `-` preserves the dimension of a `Dyn` and
                // negates its payload (the dimension lives beside the value).
                Value::Dyn(dynamic) => {
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
                }
                other => Err(EvalError::new(
                    "E0302",
                    format!("`-` is not defined for {}", other.type_name()),
                )
                .at(span)),
                }
            }
            UnOp::Not => match value {
                Value::Bool(value) => Ok(Value::Bool(!value)),
                Value::Prop(prop) => Ok(Value::Prop(Rc::new(PropValue {
                    text: format!("!({})", prop.text),
                    comparison: None,
                }))),
                other => Err(EvalError::new(
                    "E0302",
                    format!("`!` expects Bool, found {}", other.type_name()),
                )
                .at(span)),
            },
            UnOp::Sqrt => self.call_named("sqrt", vec![value], span),
        }
    }

    pub(crate) fn binary(
        &mut self,
        op: BinOp,
        left: Value,
        right: Value,
        span: Span,
    ) -> Result<Value, EvalError> {
        // §8.14: measurement arithmetic is the library type's own operator set,
        // with independent-error propagation. It is handled before the numeric
        // cases because both operands are records, not scalars.
        if let Some(result) = self.measurement_binary(op, &left, &right, span)? {
            return Ok(result);
        }
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow => {
                self.arithmetic(op, left, right, span)
            }
            BinOp::MatMul => self.matmul(left, right, span),
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                self.compare(op, left, right, span)
            }
            BinOp::And | BinOp::Or => match (&left, &right) {
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(if op == BinOp::And {
                    *a && *b
                } else {
                    *a || *b
                })),
                (Value::Prop(a), Value::Prop(b)) => Ok(Value::Prop(Rc::new(PropValue {
                    text: format!(
                        "({}) {} ({})",
                        a.text,
                        if op == BinOp::And { "&&" } else { "||" },
                        b.text
                    ),
                    comparison: None,
                }))),
                _ => Err(EvalError::new("E0302", "`&&`/`||` expect Bool operands").at(span)),
            },
            BinOp::Cons => {
                let mut elements = vec![left];
                match right {
                    Value::Tensor(tensor) => elements.extend(tensor.elements()),
                    other => elements.push(other),
                }
                let len = elements.len();
                let dim = value::value_dim(&elements[0]);
                Ok(build_tensor(elements, vec![len], dim))
            }
            BinOp::Pipe | BinOp::PipeBack => {
                Err(EvalError::new("E0809", "pipes must be desugared").at(span))
            }
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
            (Some((left_value, left_uncertainty)), Some((right_value, right_uncertainty))) => {
                match op {
                    BinOp::Add | BinOp::Sub => {
                        if left_value.dim != right_value.dim {
                            return Err(EvalError::new(
                                "E0401",
                                "measurement `+`/`-` need the same dimension",
                            )
                            .at(span));
                        }
                        let value = self.binary(
                            op,
                            Value::Num(left_value),
                            Value::Num(right_value),
                            span,
                        )?;
                        let uncertainty = self.quadrature(
                            Value::Num(left_uncertainty),
                            Value::Num(right_uncertainty),
                            span,
                        )?;
                        self.finish_measurement(value, uncertainty, span)
                    }
                    BinOp::Mul => {
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
                    }
                    BinOp::Div => {
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
                    }
                    _ => Ok(None),
                }
            }
            // A scalar scales a measurement; `measurement + scalar` is not a
            // declared operator, so it stays an `E0302` from the normal path.
            (Some((measurement_value, measurement_uncertainty)), None) => {
                let Value::Num(scalar) = right.clone() else {
                    return Ok(None);
                };
                match op {
                    BinOp::Mul | BinOp::Div => {
                        let value = self.binary(
                            op,
                            Value::Num(measurement_value),
                            Value::Num(scalar.clone()),
                            span,
                        )?;
                        let magnitude =
                            self.call_named("abs", vec![Value::Num(scalar)], span)?;
                        let uncertainty = self.binary(
                            op,
                            Value::Num(measurement_uncertainty),
                            magnitude,
                            span,
                        )?;
                        self.finish_measurement(value, uncertainty, span)
                    }
                    _ => Ok(None),
                }
            }
            (None, Some((measurement_value, measurement_uncertainty))) => {
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
            }
            (None, None) => Ok(None),
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
            return Err(EvalError::new(
                "E0809",
                "measurement arithmetic produced a non-scalar",
            )
            .at(span));
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
            (Value::Tensor(a), Value::Tensor(b)) => {
                self.tensor_arithmetic(op, a.as_ref(), b.as_ref(), span)
            }
            (Value::Tensor(tensor), Value::Num(num))
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
            }
            (Value::Num(a), Value::Num(b)) => {
                Ok(Value::Num(self.scalar_arithmetic(op, a.clone(), b.clone(), span)?))
            }
            (Value::UnitOf(a), Value::UnitOf(b)) => match op {
                BinOp::Mul => Ok(Value::UnitOf(Rc::new(combine_units(a, b, true)))),
                BinOp::Div => Ok(Value::UnitOf(Rc::new(combine_units(a, b, false)))),
                _ => Err(EvalError::new("E0302", "units support `*` and `/`").at(span)),
            },
            (Value::UnitOf(unit), Value::Num(num)) => {
                let mut result = num.clone();
                result.dim = match op {
                    BinOp::Mul => num.dim.mul(&unit.dim),
                    BinOp::Div => num.dim.div(&unit.dim),
                    _ => return Err(EvalError::new("E0302", "invalid unit arithmetic").at(span)),
                };
                result.display = Some(unit.clone());
                Ok(Value::Num(result))
            }
            (Value::Num(num), Value::UnitOf(unit)) if op == BinOp::Mul => {
                let mut result = num.clone();
                result.dim = num.dim.mul(&unit.dim);
                result.display = Some(unit.clone());
                Ok(Value::Num(result))
            }
            // §3.5.3 / §4.5.2: a unit scales a tensor elementwise. The
            // coefficient of every element is unchanged — only the element
            // dimension and the display metadata move — so the buffer is
            // shared. `Float` stays `Float`, `Real` stays `Real`.
            (Value::Tensor(tensor), Value::UnitOf(unit)) => {
                self.tensor_with_unit(op, tensor, unit, false, span)
            }
            (Value::UnitOf(unit), Value::Tensor(tensor)) => {
                self.tensor_with_unit(op, tensor, unit, true, span)
            }
            // §4.9.3: the `Dyn` escape layer's arithmetic. Dimensions are
            // checked/combined at run time because they are data there.
            (Value::Dyn(left), Value::Dyn(right)) => self.dyn_arithmetic(op, left, right, span),
            (Value::Dyn(dynamic), Value::Num(exponent)) if op == BinOp::Pow => {
                self.dyn_pow(dynamic, exponent, span)
            }
            _ => Err(EvalError::new(
                "E0302",
                format!(
                    "`{}` is not defined for {} and {}",
                    op.as_str(),
                    left.type_name(),
                    right.type_name()
                ),
            )
            .at(span)),
        }
    }

    pub(crate) fn scalar_arithmetic(
        &mut self,
        op: BinOp,
        left: NumVal,
        right: NumVal,
        span: Span,
    ) -> Result<NumVal, EvalError> {
        let dim = match op {
            BinOp::Add | BinOp::Sub => left.dim.clone(),
            BinOp::Mul => left.dim.mul(&right.dim),
            BinOp::Div => left.dim.div(&right.dim),
            BinOp::Pow => {
                let exponent = right
                    .to_rational()
                    .ok_or_else(|| EvalError::new("E0402", "exponent must be finite").at(span))?;
                left.dim.pow(&exponent)
            }
            _ => left.dim.clone(),
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
                BinOp::Add => crate::sym::Term::add(a.clone(), b.clone()),
                BinOp::Sub => crate::sym::Term::sub(a.clone(), b.clone()),
                BinOp::Mul => crate::sym::Term::mul(a.clone(), b.clone()),
                BinOp::Div => {
                    let inverse = crate::sym::Term::pow(
                        b.clone(),
                        BigRational::from_integer(BigInt::from(-1)),
                    );
                    crate::sym::Term::mul(a.clone(), inverse)
                }
                BinOp::Pow => {
                    let exponent = power.clone().expect("computed above");
                    crate::sym::Term::pow(a.clone(), exponent)
                }
                _ => return Err(EvalError::new("E0809", "unexpected operator").at(span)),
            };
            if let crate::sym::Term::Num(value) = &term {
                if has_infinity {
                    let unevaluated = match op {
                        BinOp::Add => crate::sym::Term::Add(vec![a, b]),
                        BinOp::Sub => crate::sym::Term::Add(vec![a, crate::sym::Term::neg(b)]),
                        BinOp::Mul => crate::sym::Term::Mul(vec![a, b]),
                        BinOp::Div => crate::sym::Term::Mul(vec![
                            a,
                            crate::sym::Term::pow(
                                b,
                                BigRational::from_integer(BigInt::from(-1)),
                            ),
                        ]),
                        BinOp::Pow => crate::sym::Term::Pow(
                            Box::new(a),
                            power.expect("computed above"),
                        ),
                        _ => crate::sym::Term::Num(value.clone()),
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
                BinOp::Add => (ar + br, ai + bi),
                BinOp::Sub => (ar - br, ai - bi),
                BinOp::Mul => (ar * br - ai * bi, ar * bi + ai * br),
                BinOp::Div => {
                    let denominator = br * br + bi * bi;
                    ((ar * br + ai * bi) / denominator, (ai * br - ar * bi) / denominator)
                }
                BinOp::Pow => {
                    let value = (ar, ai);
                    let exponent = br;
                    let magnitude = value.0.hypot(value.1).powf(exponent);
                    let angle = value.1.atan2(value.0) * exponent;
                    (magnitude * angle.cos(), magnitude * angle.sin())
                }
                _ => return Err(EvalError::new("E0809", "unexpected operator").at(span)),
            };
            return Ok(NumVal {
                kind: NumKind::Complex(re, im),
                dim,
                display: None,
            });
        }
        // §6.3.2: `F64 op F64` stays in the floating phase, and nothing else
        // does. A `Float` mixed with an exact value is first converted to the
        // rational it denotes, so `1.0 + 2` is the exact `3` and
        // `1/3 + 0.5` is exactly `5/6` — the language's central claim, and the
        // reason `Float ⊗ Exact -> Real` is a rule and not an aspiration.
        let both_float = matches!(left.kind, NumKind::Float(_))
            && matches!(right.kind, NumKind::Float(_));
        if both_float {
            let a = left.to_f64().unwrap_or(f64::NAN);
            let b = right.to_f64().unwrap_or(f64::NAN);
            let value = match op {
                BinOp::Add => a + b,
                BinOp::Sub => a - b,
                BinOp::Mul => a * b,
                BinOp::Div => a / b,
                BinOp::Pow => a.powf(b),
                _ => f64::NAN,
            };
            return Ok(NumVal::float(value, dim));
        }
        // The Real path: every operand is exact (a `Float` is the exact
        // rational it denotes), so an operation that is closed on the
        // rationals answers exactly, and one that is not answers symbolically.
        let both_int = matches!(left.kind, NumKind::Int(_)) && matches!(right.kind, NumKind::Int(_));
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
            BinOp::Add => Some(&a + &b),
            BinOp::Sub => Some(&a - &b),
            BinOp::Mul => Some(&a * &b),
            BinOp::Div => {
                if b.is_zero() {
                    return Err(EvalError::new("E0801", "division by zero").at(span));
                }
                Some(&a / &b)
            }
            BinOp::Pow => {
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
            }
            _ => None,
        };
        if let Some(value) = value {
            // Integer arithmetic stays `Int` when the result is an integer:
            // the narrowest representation is the honest one, and it keeps
            // `repr` agreeing with the checker's `Int`.
            if both_int && value.denom().is_one() {
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
            BinOp::Mul => tensor.dim.mul(&unit.dim),
            BinOp::Div if !unit_on_left => tensor.dim.div(&unit.dim),
            _ => {
                return Err(EvalError::new(
                    "E0302",
                    format!(
                        "`{}` is not defined between a tensor and a unit",
                        op.as_str()
                    ),
                )
                .at(span))
            }
        };
        let display = if op == BinOp::Mul {
            Some(unit.clone())
        } else {
            None
        };
        Ok(Value::Tensor(Rc::new(TensorData {
            shape: tensor.shape.clone(),
            storage: tensor.storage.clone(),
            dim,
            display,
        })))
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
            BinOp::Add | BinOp::Sub => {
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
                        "`+` and `-` on Dyn require equal runtime dimensions (§4.9.3)"
                            .to_string(),
                    ));
                }
                left.dim.clone()
            }
            BinOp::Mul => left.dim.mul(&right.dim),
            BinOp::Div => left.dim.div(&right.dim),
            _ => {
                return Err(EvalError::new(
                    "E0302",
                    format!("`{}` is not defined on Dyn", op.as_str()),
                )
                .at(span))
            }
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

    fn num_to_term(&self, num: &NumVal) -> crate::sym::Term {
        match &num.kind {
            NumKind::Int(value) => {
                crate::sym::Term::Num(BigRational::from_integer(value.clone()))
            }
            NumKind::Exact(value) => crate::sym::Term::Num(value.clone()),
            NumKind::Float(value) => crate::sym::Term::Num(
                BigRational::from_float(*value).unwrap_or_else(BigRational::zero),
            ),
            NumKind::Sym(term) => (**term).clone(),
            NumKind::Complex(re, _) => {
                crate::sym::Term::Num(BigRational::from_float(*re).unwrap_or_else(BigRational::zero))
            }
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
        let mut elements = Vec::with_capacity(count);
        for index in 0..count {
            let coordinates = unravel(index, &shape);
            let a = element_at_broadcast(left, &coordinates).ok_or_else(|| {
                EvalError::new("E0802", "shape mismatch").at(span)
            })?;
            let b = element_at_broadcast(right, &coordinates).ok_or_else(|| {
                EvalError::new("E0802", "shape mismatch").at(span)
            })?;
            elements.push(self.arithmetic(op, a, b, span)?);
        }
        let dim = elements
            .first()
            .map_or_else(|| left.dim.clone(), value::value_dim);
        Ok(build_tensor(elements, shape, dim))
    }

    fn matmul(&mut self, left: Value, right: Value, span: Span) -> Result<Value, EvalError> {
        let (Value::Tensor(a), Value::Tensor(b)) = (&left, &right) else {
            return Err(EvalError::new("E0302", "`@` expects two tensors").at(span));
        };
        if let Some(value) = a.dense_matmul(b) {
            return Ok(value);
        }
        match (a.rank(), b.rank()) {
            (2, 2) => {
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
                                None => product,
                                Some(acc) => self.arithmetic(BinOp::Add, acc, product, span)?,
                            });
                        }
                        elements.push(sum.unwrap_or_else(|| Value::float(0.0)));
                    }
                }
                let dim = elements
                    .first()
                    .map_or_else(|| a.dim.mul(&b.dim), value::value_dim);
                Ok(build_tensor(elements, vec![m, n], dim))
            }
            (2, 1) => {
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
                        let x = a.get(&[row as isize, inner as isize]).unwrap_or(Value::Unit);
                        let y = b.get(&[inner as isize]).unwrap_or(Value::Unit);
                        let product = self.arithmetic(BinOp::Mul, x, y, span)?;
                        sum = Some(match sum {
                            None => product,
                            Some(acc) => self.arithmetic(BinOp::Add, acc, product, span)?,
                        });
                    }
                    elements.push(sum.unwrap_or_else(|| Value::float(0.0)));
                }
                let dim = elements
                    .first()
                    .map_or_else(|| a.dim.mul(&b.dim), value::value_dim);
                Ok(build_tensor(elements, vec![m], dim))
            }
            (1, 2) => {
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
                        let y = b.get(&[inner as isize, column as isize]).unwrap_or(Value::Unit);
                        let product = self.arithmetic(BinOp::Mul, x, y, span)?;
                        sum = Some(match sum {
                            None => product,
                            Some(acc) => self.arithmetic(BinOp::Add, acc, product, span)?,
                        });
                    }
                    elements.push(sum.unwrap_or_else(|| Value::float(0.0)));
                }
                let dim = elements
                    .first()
                    .map_or_else(|| a.dim.mul(&b.dim), value::value_dim);
                Ok(build_tensor(elements, vec![n], dim))
            }
            (1, 1) => {
                if a.shape[0] != b.shape[0] {
                    return Err(EvalError::new("E0802", "shape mismatch in `dot`").at(span));
                }
                let mut sum: Option<Value> = None;
                for index in 0..a.shape[0] {
                    let x = a.get(&[index as isize]).unwrap_or(Value::Unit);
                    let y = b.get(&[index as isize]).unwrap_or(Value::Unit);
                    let product = self.arithmetic(BinOp::Mul, x, y, span)?;
                    sum = Some(match sum {
                        None => product,
                        Some(acc) => self.arithmetic(BinOp::Add, acc, product, span)?,
                    });
                }
                Ok(sum.unwrap_or_else(|| Value::float(0.0)))
            }
            _ => Err(EvalError::new(
                "E0802",
                format!("`@` is not defined for ranks {} and {}", a.rank(), b.rank()),
            )
            .at(span)),
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
            (Value::Num(a), Value::Num(b)) => {
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
                    return Ok(Value::Bool(if op == BinOp::Eq { equal } else { !equal }));
                }
                let both_exact = matches!(a.kind, NumKind::Int(_) | NumKind::Exact(_))
                    && matches!(b.kind, NumKind::Int(_) | NumKind::Exact(_));
                let result = if both_exact {
                    let a = a.to_rational().unwrap_or_else(BigRational::zero);
                    let b = b.to_rational().unwrap_or_else(BigRational::zero);
                    match op {
                        BinOp::Lt => a < b,
                        BinOp::Le => a <= b,
                        BinOp::Gt => a > b,
                        BinOp::Ge => a >= b,
                        _ => false,
                    }
                } else {
                    let (a, b) = match (a.to_f64(), b.to_f64()) {
                        (Some(a), Some(b)) => (a, b),
                        _ => return Ok(Value::Bool(false)),
                    };
                    match op {
                        BinOp::Lt => a < b,
                        BinOp::Le => a <= b,
                        BinOp::Gt => a > b,
                        BinOp::Ge => a >= b,
                        _ => false,
                    }
                };
                Ok(Value::Bool(result))
            }
            (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(match op {
                BinOp::Eq => a == b,
                BinOp::Ne => a != b,
                _ => false,
            })),
            (Value::Str(a), Value::Str(b)) => Ok(Value::Bool(match op {
                BinOp::Eq => a == b,
                BinOp::Ne => a != b,
                BinOp::Lt => a < b,
                BinOp::Le => a <= b,
                BinOp::Gt => a > b,
                BinOp::Ge => a >= b,
                _ => false,
            })),
            (Value::Unit, Value::Unit) => Ok(Value::Bool(matches!(op, BinOp::Eq | BinOp::Le | BinOp::Ge))),
            _ if matches!(op, BinOp::Eq | BinOp::Ne) => {
                // Structural equality for variants, records and tensors
                // (§6.12.1): payloads and elements are compared, never the
                // text they print as.
                match value::equal(&left, &right) {
                    Ok(equal) => Ok(Value::Bool(if op == BinOp::Eq { equal } else { !equal })),
                    Err(reason) => Err(EvalError::new("E0302", reason.to_string()).at(span)),
                }
            }
            _ => Err(EvalError::new(
                "E0302",
                format!(
                    "cannot compare {} with {}",
                    left.type_name(),
                    right.type_name()
                ),
            )
            .at(span)),
        }
    }

    // ----- calls ----------------------------------------------------------

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
        // §9.3.2: the depth limit counts *calls*, not AST nodes. Counting nodes
        // rejected a legal flat `a + a + …` chain of 2100 terms while letting
        // an unbounded call chain through, which is the opposite of the rule.
        self.depth += 1;
        if self.depth > self.max_depth {
            self.depth -= 1;
            return Err(EvalError::new("E0805", "stack overflow")
                .at(span)
                .with_stack(self.frames.clone()));
        }
        let result = self.apply_inner(function, args, span);
        self.depth -= 1;
        match result {
            Ok(value) => Ok(value),
            Err(error) => Err(error.with_stack(self.frames.clone())),
        }
    }

    fn apply_inner(
        &mut self,
        function: Value,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, EvalError> {
        match function {
            Value::Native(function) => (function.0)(self, args, span),
            Value::Closure(closure) => {
                // §9.2.2: a closure lowered to the Core IR runs on the IR
                // interpreter, which trampolines its tail calls. Everything
                // else about applying a function — the depth guard, the stack
                // frame, the `?` payload — is shared.
                if matches!(closure.body, ClosureBody::Ir(_)) {
                    return crate::eval_ir::run_closure(self, &closure, args, span);
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
                let scope = Scope::child(captured);
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
                match outcome {
                    // `?` returns from the enclosing function by unwinding to
                    // its call, which is where the `Err` value belongs.
                    Err(error) if error.payload.is_some() => {
                        Ok(error.payload.expect("checked above"))
                    }
                    other => other,
                }
            }
            Value::Ctor {
                adt,
                index,
                name,
                arity,
                args: collected,
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
                    });
                }
                if all.len() > arity {
                    return Err(EvalError::new(
                        "E0307",
                        format!("constructor `{name}` takes {arity} argument(s)"),
                    )
                    .at(span));
                }
                Ok(make_variant(adt, index, &name, all))
            }
            other => Err(EvalError::new(
                "E0305",
                format!("{} is not a function", other.type_name()),
            )
            .at(span)),
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
            "+" => BinOp::Add,
            "-" => BinOp::Sub,
            "*" => BinOp::Mul,
            "/" => BinOp::Div,
            "@" => BinOp::MatMul,
            "^" => BinOp::Pow,
            "==" => BinOp::Eq,
            "!=" => BinOp::Ne,
            "<" => BinOp::Lt,
            "<=" => BinOp::Le,
            ">" => BinOp::Gt,
            ">=" => BinOp::Ge,
            "&&" => BinOp::And,
            "||" => BinOp::Or,
            "::" => BinOp::Cons,
            other => {
                return Err(EvalError::new("E0809", format!("unknown operator `{other}`")).at(span));
            }
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
        let Some(key) = self.rssn_calls.get(&span).cloned() else {
            return Err(EvalError::new(
                "E1005",
                format!("no pinned overload for `Rssn.{op}` at this call site"),
            )
            .at(span)
            .with_note("the checked module did not record this call; re-check the module"));
        };
        crate::rssn_module::call_operator(&key, values, span)
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
            Dispatch::Instance(body) => self.apply(body, values, span),
            Dispatch::Builtin => self.call_named(method, values, span),
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
            Some(dispatch) => Ok(dispatch),
            // The checker proved a user instance the runtime cannot find.
            // Running the global of the method name here would be a silently
            // different program.
            None => Err(EvalError::new(
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
            ))),
        }
    }

    // ----- patterns -------------------------------------------------------

    fn match_pattern(
        &mut self,
        pattern: &Pat,
        value: &Value,
        scope: &Rc<Scope>,
    ) -> Result<bool, EvalError> {
        match &pattern.kind {
            // A parser recovery node: it matches nothing.
            PatKind::Error => Ok(false),
            PatKind::Wildcard => Ok(true),
            PatKind::Var(name) => {
                scope.vars.borrow_mut().insert(name.clone(), value.clone());
                Ok(true)
            }
            PatKind::Lit(lit) => Ok(match (lit, value) {
                (PatLit::Int(expected), Value::Num(num)) => match &num.kind {
                    NumKind::Int(actual) => actual == expected,
                    NumKind::Exact(actual) => {
                        actual == &BigRational::from_integer(expected.clone())
                    }
                    NumKind::Float(actual) => {
                        BigRational::from_float(*actual)
                            == Some(BigRational::from_integer(expected.clone()))
                    }
                    _ => false,
                },
                (PatLit::Str(expected), Value::Str(actual)) => actual.as_ref() == expected,
                (PatLit::Bool(expected), Value::Bool(actual)) => expected == actual,
                (PatLit::Unit, Value::Unit) => true,
                _ => false,
            }),
            PatKind::Ctor { name, args } => {
                let Value::Variant(variant) = value else {
                    return Ok(false);
                };
                if &variant.name != name || variant.fields.len() != args.len() {
                    return Ok(false);
                }
                for (sub, field) in args.iter().zip(variant.fields.iter()) {
                    if !self.match_pattern(sub, field, scope)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            PatKind::Record(fields) => {
                let Value::Record(record) = value else {
                    return Ok(false);
                };
                for (name, sub) in fields {
                    let Some(field) = record.fields.get(name) else {
                        return Ok(false);
                    };
                    match sub {
                        Some(sub) => {
                            if !self.match_pattern(sub, field, scope)? {
                                return Ok(false);
                            }
                        }
                        None => {
                            scope.vars.borrow_mut().insert(name.clone(), field.clone());
                        }
                    }
                }
                Ok(true)
            }
            PatKind::Tuple(items) => {
                let Value::Record(record) = value else {
                    return Ok(false);
                };
                for (index, item) in items.iter().enumerate() {
                    let Some(field) = record.fields.get(&index.to_string()) else {
                        return Ok(false);
                    };
                    if !self.match_pattern(item, field, scope)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            PatKind::Tensor { items, rest } => {
                let Value::Tensor(tensor) = value else {
                    return Ok(false);
                };
                if tensor.rank() != 1 {
                    return Ok(false);
                }
                let elements = tensor.elements();
                if elements.len() < items.len() {
                    return Ok(false);
                }
                for (index, item) in items.iter().enumerate() {
                    if !self.match_pattern(item, &elements[index], scope)? {
                        return Ok(false);
                    }
                }
                if let Some(name) = rest {
                    let remaining: Vec<Value> = elements[items.len()..].to_vec();
                    let len = remaining.len();
                    scope.vars.borrow_mut().insert(
                        name.clone(),
                        build_tensor(remaining, vec![len], tensor.dim.clone()),
                    );
                }
                Ok(true)
            }
            PatKind::As { pattern: inner, name } => {
                if self.match_pattern(inner, value, scope)? {
                    scope.vars.borrow_mut().insert(name.clone(), value.clone());
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            PatKind::Annot { pattern: inner, .. } => self.match_pattern(inner, value, scope),
        }
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
                format!(
                    "pattern does not match value {}",
                    value::show(value)
                ),
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
        if values.is_empty() {
            return Ok(Value::Tensor(Rc::new(TensorData {
                shape: vec![0],
                storage: Storage::Boxed(Vec::new()),
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
        if !matches!(base, Value::Tensor(_)) {
            return Err(EvalError::new("E0302", "indexing expects a tensor").at(span));
        }
        let mut resolved = Vec::with_capacity(indices.len());
        for index in indices {
            match index {
                SliceAst::Index(expr) => {
                    let value = self.eval(expr, env)?;
                    resolved.push(SliceValue::Index(value.as_f64()));
                }
                SliceAst::Range {
                    start,
                    end,
                    inclusive,
                    step,
                } => {
                    let start = match start {
                        Some(expr) => Some(self.eval(expr, env)?.as_f64().unwrap_or(0.0)),
                        None => None,
                    };
                    let end = match end {
                        Some(expr) => Some(self.eval(expr, env)?.as_f64().unwrap_or(0.0)),
                        None => None,
                    };
                    let step = match step {
                        Some(expr) => Some(self.eval(expr, env)?.as_f64().unwrap_or(f64::NAN)),
                        None => None,
                    };
                    resolved.push(SliceValue::Range {
                        start,
                        end,
                        inclusive: *inclusive,
                        step,
                    });
                }
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
                    let coordinate = value
                        .ok_or_else(|| EvalError::new("E0802", "index must be an Int"))?;
                    coordinates.push(coordinate as isize);
                }
            }
            if coordinates.len() == tensor.rank() {
                return tensor
                    .get(&coordinates)
                    .ok_or_else(|| EvalError::new("E0802", "index out of range").at(span));
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
                        return Err(EvalError::new("E0802", "index out of range").at(span));
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
                return Ok(build_tensor(
                    elements,
                    remaining.to_vec(),
                    tensor.dim.clone(),
                ));
            }
        }
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
                SliceValue::Index(value) => {
                    let coordinate = value.unwrap_or(0.0) as isize;
                    let normalized = normalize_index(coordinate, extent)
                        .ok_or_else(|| EvalError::new("E0802", "index out of range").at(span))?;
                    ranges.push((normalized, normalized + 1));
                    walks.push((normalized, 1, 1));
                }
                SliceValue::Range {
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
                }
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
            return Ok(build_tensor(elements, new_shape, tensor.dim.clone()));
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
        Ok(build_tensor(elements, new_shape, tensor.dim.clone()))
    }

    // ----- IO -------------------------------------------------------------

    /// Builds the action a `do` block describes.
    ///
    /// Plain statements are collected in a loop, so a flat block of 100k
    /// `println`s costs one frame rather than 100k (§9.3.2). A `let` or `<-`
    /// turns the *rest* of the block into a [`IoAction::Defer`] / `Bind`, so
    /// the statement runs when the action runs and not when it is built
    /// (§6.8.1) — and `run_io` drives that chain iteratively, so the stack does
    /// not grow with the number of bindings either.
    fn eval_do(&mut self, stmts: &[DoStmt], env: &Rc<Scope>) -> Result<Value, EvalError> {
        let mut actions: Vec<Rc<IoAction>> = Vec::new();
        for (index, stmt) in stmts.iter().enumerate() {
            let rest = &stmts[index + 1..];
            match stmt {
                DoStmt::Let {
                    pattern,
                    ty,
                    value,
                    ..
                } => {
                    // `do let x = e; rest` is `rest` evaluated in the scope
                    // `x` creates — but only when the action runs.
                    let body = Expr::new(
                        ExprKind::LetIn {
                            pattern: pattern.clone(),
                            ty: ty.clone(),
                            value: Box::new(value.clone()),
                            body: Box::new(Expr::new(
                                ExprKind::Do {
                                    stmts: rest.to_vec(),
                                },
                                first_span(stmt),
                            )),
                        },
                        first_span(stmt),
                    );
                    let thunk = Rc::new(Closure::ast(Vec::new(), body, env.clone(), None));
                    actions.push(Rc::new(IoAction::Defer(thunk)));
                    return Ok(Value::Io(Rc::new(IoAction::Seq(actions))));
                }
                DoStmt::Bind { pattern, value, .. } => {
                    let action = self.eval(value, env)?;
                    let Value::Io(action) = action else {
                        return Err(EvalError::new("E0701", "`<-` expects an IO action")
                            .at(value.span));
                    };
                    let rest_expr = Expr::new(
                        ExprKind::Do {
                            stmts: rest.to_vec(),
                        },
                        first_span(stmt),
                    );
                    let continuation = Value::Closure(Rc::new(Closure::ast(
                        vec![pattern.clone()],
                        rest_expr,
                        env.clone(),
                        None,
                    )));
                    actions.push(Rc::new(IoAction::Bind {
                        action,
                        func: continuation,
                    }));
                    return Ok(Value::Io(Rc::new(IoAction::Seq(actions))));
                }
                DoStmt::Expr(expr) => {
                    let action = self.eval(expr, env)?;
                    let Value::Io(action) = action else {
                        return Err(EvalError::new(
                            "E0701",
                            format!(
                                "a `do` statement must be an IO action, found {}",
                                action.type_name()
                            ),
                        )
                        .at(expr.span));
                    };
                    actions.push(action);
                }
            }
        }
        match actions.len() {
            0 => Ok(Value::Io(Rc::new(IoAction::Pure(Value::Unit)))),
            1 => Ok(Value::Io(actions.remove(0))),
            _ => Ok(Value::Io(Rc::new(IoAction::Seq(actions)))),
        }
    }

    /// Executes an IO action.
    ///
    /// Iterative: the pending actions and their continuations live on an
    /// explicit stack, so a `do` block of any length runs in constant Rust
    /// stack (§9.3.2).
    ///
    /// # Errors
    ///
    /// Reports IO failures (`E0810`), failed assertions (`E0899`) and a `?`
    /// that tries to leave an `IO` block (`E0702`).
    pub fn run_io(&mut self, action: &Rc<IoAction>) -> Result<Value, EvalError> {
        enum Work {
            Run(Rc<IoAction>),
            Continue(Value),
        }
        let mut pending: Vec<Work> = vec![Work::Run(action.clone())];
        let mut last = Value::Unit;
        while let Some(work) = pending.pop() {
            let action = match work {
                Work::Continue(function) => {
                    let value = last.clone();
                    let next = self.apply(function, vec![value], Span::default())?;
                    match next {
                        Value::Io(next) => {
                            pending.push(Work::Run(next));
                            continue;
                        }
                        other => {
                            return Err(EvalError::new(
                                "E0701",
                                format!(
                                    "continuation did not produce an IO action, found {}",
                                    other.type_name()
                                ),
                            ));
                        }
                    }
                }
                Work::Run(action) => action,
            };
            match &*action {
                IoAction::Pure(value) => last = value.clone(),
                IoAction::Print {
                    text,
                    newline,
                    stderr,
                } => {
                    self.write_out(text, *newline, *stderr);
                    last = Value::Unit;
                }
                IoAction::ReadFile(path) => {
                    last = match std::fs::read_to_string(path) {
                        Ok(contents) => make_variant(
                            adt::RESULT_ID,
                            0,
                            "Ok",
                            vec![Value::string(contents)],
                        ),
                        Err(error) => make_variant(
                            adt::RESULT_ID,
                            1,
                            "Err",
                            vec![Value::string(format!(
                                "{}: {error}",
                                io_error_code(&error)
                            ))],
                        ),
                    };
                }
                IoAction::WriteFile { path, contents } => {
                    last = match std::fs::write(path, contents) {
                        Ok(()) => make_variant(adt::RESULT_ID, 0, "Ok", vec![Value::Unit]),
                        Err(error) => make_variant(
                            adt::RESULT_ID,
                            1,
                            "Err",
                            vec![Value::string(format!(
                                "{}: {error}",
                                io_error_code(&error)
                            ))],
                        ),
                    };
                }
                IoAction::ReadNpy(path) => {
                    let result = match std::fs::read(path) {
                        Ok(bytes) => match crate::builtins::npy_decode(&bytes) {
                            Ok(tensor) => make_variant(
                                adt::RESULT_ID,
                                0,
                                "Ok",
                                vec![Value::Tensor(Rc::new(tensor))],
                            ),
                            Err(message) => {
                                make_variant(adt::RESULT_ID, 1, "Err", vec![Value::string(message)])
                            }
                        },
                        Err(error) => make_variant(
                            adt::RESULT_ID,
                            1,
                            "Err",
                            vec![Value::string(format!(
                                "{}: {error}",
                                io_error_code(&error)
                            ))],
                        ),
                    };
                    last = result;
                }
                IoAction::WriteNpy { path, bytes } => {
                    last = match std::fs::write(path, bytes) {
                        Ok(()) => make_variant(adt::RESULT_ID, 0, "Ok", vec![Value::Unit]),
                        Err(error) => make_variant(
                            adt::RESULT_ID,
                            1,
                            "Err",
                            vec![Value::string(format!(
                                "{}: {error}",
                                io_error_code(&error)
                            ))],
                        ),
                    };
                }
                IoAction::ReadLine => {
                    let mut line = String::new();
                    std::io::stdin()
                        .read_line(&mut line)
                        .map_err(|error| EvalError::new("E0810", error.to_string()))?;
                    last = Value::string(line.trim_end_matches(['\n', '\r']));
                }
                IoAction::Bind { action, func } => {
                    pending.push(Work::Continue(func.clone()));
                    pending.push(Work::Run(action.clone()));
                }
                IoAction::Seq(actions) => {
                    for action in actions.iter().rev() {
                        pending.push(Work::Run(action.clone()));
                    }
                }
                IoAction::Plot(html) => {
                    std::fs::write("plot.html", html)
                        .map_err(|error| io_error("E0905", "plot rendering failed", &error))?;
                    last = Value::Unit;
                }
                IoAction::Save { path, html } => {
                    std::fs::write(path, html)
                        .map_err(|error| io_error("E0905", "plot rendering failed", &error))?;
                    last = Value::Unit;
                }
                IoAction::Defer(thunk) => {
                    let produced = self.apply(
                        Value::Closure(thunk.clone()),
                        Vec::new(),
                        Span::default(),
                    )?;
                    match produced {
                        Value::Io(next) => pending.push(Work::Run(next)),
                        other => {
                            return Err(EvalError::new(
                                "E0702",
                                format!(
                                    "`?` in a `do` block tried to return {} from a function that \
                                     does not return a `Result`",
                                    other.type_name()
                                ),
                            ));
                        }
                    }
                }
                IoAction::Assert { message } => {
                    return Err(EvalError::new("E0899", message.clone())
                        .with_stack(self.frames.clone()));
                }
                IoAction::Random => {
                    last = Value::Num(NumVal::float(self.next_random(), Dim::one()));
                }
                IoAction::TimeNow => {
                    last = Value::Num(NumVal::float(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0.0, |elapsed| elapsed.as_secs_f64()),
                        Dim::base("Time"),
                    ));
                }
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
        std::io::ErrorKind::NotFound => "E0901",
        std::io::ErrorKind::PermissionDenied => "E0902",
        _ => "E0901",
    }
}

/// A fatal IO error with the taxonomy code and the OS message.
fn io_error(code: &'static str, context: &str, error: &std::io::Error) -> EvalError {
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
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

fn first_span(stmt: &DoStmt) -> Span {
    match stmt {
        DoStmt::Let { span, .. } | DoStmt::Bind { span, .. } => *span,
        DoStmt::Expr(expr) => expr.span,
    }
}

fn to_complex(num: &NumVal) -> (f64, f64) {
    match &num.kind {
        NumKind::Complex(re, im) => (*re, *im),
        _ => (num.to_f64().unwrap_or(f64::NAN), 0.0),
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
        TyKind::Named { path, args } if args.is_empty() => path.last().cloned(),
        _ => None,
    }
}

/// The class-method call sites of one module, by span.
fn dictionary_table(uses: &[DictionaryUse]) -> HashMap<Span, Vec<DictionaryUse>> {
    let mut table: HashMap<Span, Vec<DictionaryUse>> = HashMap::new();
    for use_site in uses {
        table.entry(use_site.span).or_default().push(use_site.clone());
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
        TyKind::Int => "Int".to_string(),
        TyKind::Real(_) => "Real".to_string(),
        TyKind::Float(_) => "Float".to_string(),
        TyKind::Complex(_) => "Complex".to_string(),
        TyKind::Bool => "Bool".to_string(),
        TyKind::Prop => "Prop".to_string(),
        TyKind::Str => "String".to_string(),
        TyKind::Unit => "()".to_string(),
        TyKind::Var(name) => name.clone(),
        TyKind::Tensor { .. } | TyKind::Matrix { .. } | TyKind::Sparse { .. } => "Tensor".to_string(),
        _ => "?".to_string(),
    }
}

fn combine_units(left: &UnitDef, right: &UnitDef, multiply: bool) -> UnitDef {
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

fn factorial(n: i64, double: bool) -> BigInt {
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

fn normalize_index(index: isize, extent: usize) -> Option<usize> {
    let extent = isize::try_from(extent).ok()?;
    let normalized = if index < 0 { extent + index } else { index };
    if normalized < 0 || normalized >= extent {
        None
    } else {
        Some(usize::try_from(normalized).ok()?)
    }
}

fn normalize_bound(value: isize, extent: usize, inclusive: bool) -> usize {
    let extent_i = isize::try_from(extent).unwrap_or(isize::MAX);
    let normalized = if value < 0 { extent_i + value } else { value };
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
    let from_end = |value: isize| if value < 0 { extent + value } else { value };
    let first = match start {
        Some(value) => {
            let high = if forward { extent } else { extent - 1 };
            from_end(value as isize).clamp(0, high)
        }
        None if forward => 0,
        None => extent - 1,
    };
    let stop = match end {
        Some(value) => {
            let high = if forward { extent } else { extent - 1 };
            from_end(value as isize).clamp(-1, high)
        }
        None if forward => extent,
        None => -1,
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
    Ok((if count == 0 { 0 } else { first as usize }, count, step))
}

fn broadcast_shape(left: &[usize], right: &[usize]) -> Option<Vec<usize>> {
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

fn unravel(mut index: usize, shape: &[usize]) -> Vec<usize> {
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
pub fn qualified_name(base: &Expr, field: &str) -> Option<String> {
    match &base.kind {
        ExprKind::QName(path) => Some(format!("{}.{field}", path.join("."))),
        ExprKind::Var(name) if name.chars().next().is_some_and(char::is_uppercase) => {
            Some(format!("{name}.{field}"))
        }
        _ => None,
    }
}

/// Whether an expression names the `Rssn` module itself.
#[must_use]
pub fn is_rssn_root(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Var(name) => name == "Rssn",
        ExprKind::QName(path) => path.len() == 1 && path[0] == "Rssn",
        _ => false,
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
        ExprKind::Field { base, name } if is_rssn_root(base) => name.clone(),
        ExprKind::QName(path) if path.len() == 2 && path[0] == "Rssn" => path[1].clone(),
        _ => return None,
    };
    if crate::rssn_module::is_entry_point(&name) || name == "overloads" {
        return None;
    }
    Some(name)
}

fn element_at_broadcast(tensor: &TensorData, coordinates: &[usize]) -> Option<Value> {
    let mut adjusted = Vec::with_capacity(coordinates.len());
    for (coordinate, extent) in coordinates.iter().zip(tensor.shape.iter()) {
        adjusted.push(if *extent == 1 { 0 } else { *coordinate } as isize);
    }
    tensor.get(&adjusted)
}

/// Builds a tensor from elements, choosing the narrowest storage.
#[must_use]
pub fn build_tensor(elements: Vec<Value>, shape: Vec<usize>, dim: Dim) -> Value {
    if elements.is_empty() {
        return Value::Tensor(Rc::new(TensorData {
            shape,
            storage: Storage::Boxed(Vec::new()),
            dim,
            display: None,
        }));
    }
    if elements
        .iter()
        .all(|element| matches!(element, Value::Num(NumVal { kind: NumKind::Float(_), .. })))
    {
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
                    Some(value) => values.push(value),
                    None => {
                        converted = false;
                        break;
                    }
                }
                if display.is_none() {
                    display = num.display.clone();
                }
            }
        }
        if converted {
            return Value::Tensor(Rc::new(TensorData {
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
        return Value::Tensor(Rc::new(TensorData {
            shape,
            storage: Storage::Exact(values),
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
            shape,
            storage: Storage::Bool(values),
            dim,
            display: None,
        }));
    }
    Value::Tensor(Rc::new(TensorData {
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
        Value::Num(num) => Some(Witness::Num(match &num.kind {
            NumKind::Int(_) => RepKind::Int,
            NumKind::Exact(_) | NumKind::Sym(_) => RepKind::Real,
            NumKind::Float(_) => RepKind::Float,
            NumKind::Complex(..) => RepKind::Complex,
        })),
        Value::Tensor(tensor) => {
            let storage = match &tensor.storage {
                Storage::Dense(_) | Storage::View(_) => StorageClass::Float,
                Storage::Exact(_) => StorageClass::Exact,
                // A `Bool` tensor has no element representation in the witness
                // table (`witness_of` requires a scalar element).
                Storage::Bool(_) => return None,
                Storage::Boxed(items) => match items.first() {
                    Some(Value::Num(num)) => match &num.kind {
                        NumKind::Int(_) | NumKind::Exact(_) | NumKind::Sym(_) => {
                            StorageClass::Exact
                        }
                        NumKind::Float(_) => StorageClass::Float,
                        NumKind::Complex(..) => StorageClass::Complex,
                    },
                    _ => return None,
                },
            };
            Some(Witness::Tensor {
                rank: u32::try_from(tensor.rank()).ok()?,
                storage,
            })
        }
        Value::Variant(variant) => {
            let args = variant
                .fields
                .iter()
                .map(witness_of_value)
                .collect::<Option<Vec<Witness>>>()?;
            Some(Witness::Adt(adt::key_of(variant.adt), args))
        }
        Value::Record(record) => {
            let mut labels: Vec<String> = record.fields.keys().cloned().collect();
            labels.sort();
            Some(Witness::Record(labels))
        }
        Value::Bool(_) => Some(Witness::Bool),
        Value::Str(_) => Some(Witness::String),
        Value::Unit => Some(Witness::Unit),
        Value::Prop(_) => Some(Witness::Prop),
        Value::Closure(_) | Value::Native(_) | Value::Ctor { .. } | Value::Io(_) => None,
        Value::Dyn(_) | Value::UnitOf(_) | Value::Error => None,
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
        Some((id, position, registered))
    })
}

/// A constructor value of one module's type, ready to apply.
///
/// A constructor with no fields is already its variant; only a constructor with
/// fields is a value that still needs arguments (§3.6.2).
#[must_use]
pub fn ctor_value(module: &keys::ModulePath, name: &str) -> Option<Value> {
    let (adt, index, registered) = ctor_in_module(module, name)?;
    let arity = registered.ctors.get(index).map_or(0, |ctor| ctor.fields.len());
    if arity == 0 {
        return Some(make_variant(adt, index, name, Vec::new()));
    }
    Some(Value::Ctor {
        adt,
        index,
        name: name.to_string(),
        arity,
        args: Vec::new(),
    })
}

/// Constructor name lookup, re-exported for the runtime's pattern matching.
#[must_use]
pub fn variant_name(adt: adt::AdtId, index: usize) -> Option<String> {
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
        let compilation = crate::compile("measure-unit.aev", "fn main(): IO Unit = do\n    println(\"\")\n");
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
