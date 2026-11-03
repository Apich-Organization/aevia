//! The Core IR of §9.2.2: administrative normal form, closure-converted, with
//! a span on every node and a call site on every call.
//!
//! This crate holds **data only** — the IR types and the lowering from the
//! typed AST. It depends on `aevia-syntax` and `aevia-types` and deliberately
//! not on `aevia-core`: `Value` lives there, and an interpreter that owned the
//! IR types as well would be a cycle.
//!
//! Three properties are the reason the IR exists (PLAN D2):
//!
//! * **ANF.** Every intermediate is named. A call takes atoms, an `if` takes an
//!   atom, and a compound argument becomes its own `let`, so evaluation order
//!   is explicit rather than implied by a recursive walk.
//! * **Closure conversion.** A function captures an explicit, ordered vector of
//!   atoms and a body reads a capture by index. There is no `HashMap` chain to
//!   walk per variable read.
//! * **Tail positions.** A block ends in a [`Tail`], and a call in tail
//!   position is marked so the interpreter can trampoline it instead of
//!   recursing.
//!
//! Every node carries the [`Span`] it came from, so a diagnostic or a stack
//! frame can name its source location, and the pinned `Rssn` overload and the
//! resolved class instance stay keyed by that span (PLAN D1, D3).

#![forbid(unsafe_code)]
#![allow(clippy::too_many_lines)]

pub mod lower;

use aevia_dim::UnitExpr;
use aevia_syntax::ast::{BinOp, NumberLit, PatLit, Ty, UnOp};
use aevia_syntax::Span;
use num_bigint::BigInt;
use std::rc::Rc;

/// An index into the current frame's slot vector.
pub type Slot = u32;

/// A value the interpreter can produce without further evaluation.
///
/// Locals and captures are indices into a flat vector; a `Global` is a name
/// resolved by the module's scope and the interpreter's tables (units,
/// constructors, class methods, builtins), never by a lexical chain.
#[derive(Clone, Debug, PartialEq)]
pub enum Atom {
    /// A slot of the current frame.
    Local(Slot),
    /// An entry of the closure environment.
    Capture(u32),
    /// A top-level name, a qualified name, a unit, a constructor or a builtin.
    Global(String),
    /// An integer literal.
    Int(BigInt),
    /// A float literal, kept as source text so the runtime reports the same
    /// `E0106` an invalid literal would (it is unreachable from the parser, but
    /// the two paths must agree on it).
    Float(String),
    /// A boolean literal.
    Bool(bool),
    /// A string literal with no interpolation.
    Str(String),
    /// `()`.
    Unit,
}

/// Where a captured value comes from when a closure is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capture {
    /// A slot of the frame that is building the closure.
    Local(Slot),
    /// A capture of the closure that is building it.
    Capture(u32),
}

/// A lowered function: parameters, body, captured environment and slot count.
#[derive(Debug)]
pub struct Function {
    /// The name, for stack frames.
    pub name: String,
    /// The parameter patterns, in order.
    pub params: Vec<Pattern>,
    /// The body.
    pub body: Rc<Block>,
    /// The environment, in capture order (first use, deterministic).
    pub captures: Vec<Capture>,
    /// How many slots the body uses.
    pub nslots: u32,
    /// Where the function is written.
    pub span: Span,
}

/// A straight-line sequence of statements followed by a terminating form.
#[derive(Debug)]
pub struct Block {
    /// The statements, in evaluation order.
    pub stmts: Vec<Stmt>,
    /// What the block produces.
    pub tail: Tail,
}

/// One statement of a block.
#[derive(Debug)]
pub enum Stmt {
    /// `let slot = rhs`.
    Let(Let),
    /// Bind an atom to a pattern.
    Bind {
        /// The pattern, with its binders assigned slots.
        pattern: Pattern,
        /// The value.
        atom: Atom,
        /// Where the pattern is written, for `E0308`.
        span: Span,
    },
}

/// One named intermediate.
#[derive(Debug)]
pub struct Let {
    /// The slot it is written to.
    pub slot: Slot,
    /// How it is computed.
    pub rhs: Rhs,
    /// Where it is written.
    pub span: Span,
}

/// The last thing a block does.
#[derive(Debug)]
pub enum Tail {
    /// Produce an atom.
    Return {
        /// The atom.
        atom: Atom,
        /// Where it is written.
        span: Span,
    },
    /// Call something. A call to a closure in tail position is trampolined.
    Call(Call),
    /// Branch. Both arms are in tail position.
    If {
        /// The condition, already evaluated.
        cond: Atom,
        /// Where the condition is written, for `E0302`.
        cond_span: Span,
        /// The true branch.
        then_block: Rc<Block>,
        /// The false branch.
        else_block: Rc<Block>,
        /// Where the `if` is written.
        span: Span,
    },
    /// Match. Every arm body is in tail position.
    Match {
        /// The scrutinee, already evaluated.
        scrutinee: Atom,
        /// The arms, in source order.
        arms: Vec<Arm>,
        /// Where the `match` is written, for `E0309`.
        span: Span,
    },
}

/// One `match` arm.
#[derive(Debug)]
pub struct Arm {
    /// The pattern, with its binders already assigned slots.
    pub pattern: Pattern,
    /// The optional guard, evaluated with the arm's bindings in place.
    pub guard: Option<Rc<Block>>,
    /// The body.
    pub body: Rc<Block>,
    /// Where the arm is written.
    pub span: Span,
}

/// A computation that produces one value.
#[derive(Debug)]
pub enum Rhs {
    /// Copy an atom.
    Atom(Atom),
    /// A failure the lowering can already see (`_` outside a call).
    Error {
        /// The diagnostic code.
        code: &'static str,
        /// The message.
        message: String,
    },
    /// A prefix operator.
    Unary {
        /// The operator.
        op: UnOp,
        /// Its operand.
        operand: Atom,
    },
    /// An infix operator that is not short-circuiting.
    Binary {
        /// The operator.
        op: BinOp,
        /// Left operand.
        left: Atom,
        /// Right operand.
        right: Atom,
    },
    /// `&&` / `||`: the right operand is only evaluated when the left one does
    /// not decide the answer, so it stays a block rather than an atom.
    AndOr {
        /// The operator.
        op: BinOp,
        /// Left operand.
        left: Atom,
        /// Right operand, evaluated lazily.
        right: Rc<Block>,
    },
    /// A call.
    Call(Call),
    /// A closure over the current frame.
    Closure(Rc<Function>),
    /// A closure that captures its own slot, for a block-local recursive
    /// binding. `self_capture` names the environment entry to patch after the
    /// closure exists.
    RecClosure {
        /// The function.
        function: Rc<Function>,
        /// The environment index that must be filled with the closure itself.
        self_capture: Option<u32>,
    },
    /// A nested block evaluated as a value: a value-position `if`/`match`, or a
    /// block expression. Any tail call inside is performed, not propagated.
    Block(Rc<Block>),
    /// A record field read, or a qualified name when one exists.
    Field {
        /// The record, or the qualifier.
        base: Atom,
        /// `Foo.bar`, when the base names a module or an import alias.
        qualified: Option<String>,
        /// The field name.
        name: String,
    },
    /// `Rssn.<name>` used as a value.
    RssnMember {
        /// The member name.
        name: String,
    },
    /// A record literal or update.
    Record {
        /// The record being updated.
        base: Option<Atom>,
        /// The fields, in source order.
        fields: Vec<(String, Atom)>,
    },
    /// A tuple, which is a record with numeric labels.
    Tuple(Vec<Atom>),
    /// A tensor literal.
    Tensor(Vec<Atom>),
    /// An index or slice.
    Index {
        /// The tensor.
        base: Atom,
        /// The indices.
        indices: Vec<Slice>,
    },
    /// `3 m`, `25 °C`.
    Quantity {
        /// The coefficient.
        value: NumberLit,
        /// The unit expression.
        unit: UnitExpr,
    },
    /// A string with interpolations.
    Str(Vec<StrPart>),
    /// `e?`
    Try(Atom),
    /// `e!` / `e!!`
    Factorial {
        /// The operand.
        value: Atom,
        /// True for `!!`.
        double: bool,
    },
    /// A `do` block's action plan.
    Do(DoPlan),
}

/// One piece of an interpolated string.
#[derive(Debug)]
pub enum StrPart {
    /// Literal text.
    Text(String),
    /// `{expr}`, rendered with `show`.
    Atom(Atom),
}

/// One index of a slice.
#[derive(Debug)]
pub enum Slice {
    /// A single index.
    Index(Atom),
    /// A range; a missing bound means "from the start" / "to the end".
    Range {
        /// The start.
        start: Option<Atom>,
        /// The end.
        end: Option<Atom>,
        /// Whether the range is inclusive.
        inclusive: bool,
        /// The step, when written. `None` means one.
        step: Option<Atom>,
    },
}

/// A call, with the resolution the checker already performed.
#[derive(Debug)]
pub struct Call {
    /// The callee. Unused for a [`CallKind::Typed`] call, which names its
    /// builtin outright.
    pub callee: Atom,
    /// The arguments, already evaluated to atoms.
    pub args: Vec<Atom>,
    /// How the call is resolved.
    pub kind: CallKind,
    /// Where the call is written. `Rssn` and dictionary lookups are by span.
    pub span: Span,
}

/// How a call is resolved.
#[derive(Debug)]
pub enum CallKind {
    /// Evaluate the callee and apply it.
    Normal,
    /// `Rssn.<op>(…)` with the overload pinned at `Call::span`.
    Rssn {
        /// The operator name, for the `E1005` message.
        name: String,
    },
    /// A class method with the instance the checker recorded at `Call::span`.
    Dictionary {
        /// The method name.
        method: String,
    },
    /// A builtin that reads an explicit type argument (`zeros<Int>(3)`).
    Typed {
        /// The builtin name.
        name: String,
        /// The type arguments, as written.
        types: Vec<Ty>,
    },
}

/// How a `do` block builds its action.
#[derive(Debug)]
pub struct DoPlan {
    /// Statements that are actions in their own right, in order.
    pub prefix: Vec<Action>,
    /// What the block does after them.
    pub terminal: DoTerminal,
}

/// One eagerly evaluated `do` statement.
#[derive(Debug)]
pub struct Action {
    /// The action.
    pub atom: Atom,
    /// Where it is written, for `E0701`.
    pub span: Span,
}

/// What ends a `do` block.
#[derive(Debug)]
pub enum DoTerminal {
    /// The block ended.
    End,
    /// `do let p = e; rest` — a deferred thunk.
    Let {
        /// The thunk: bind the pattern, then build the rest of the action.
        thunk: Rc<Function>,
    },
    /// `p <- action; rest` — a bind.
    Bind {
        /// The action, evaluated when the block is built.
        action: Atom,
        /// Where the action is written.
        span: Span,
        /// The continuation.
        continuation: Rc<Function>,
    },
}

/// A pattern whose binders are slots of the current frame.
#[derive(Debug)]
pub enum Pattern {
    /// `_`
    Wildcard,
    /// A parser recovery node: it matches nothing.
    Error,
    /// A binding, optionally of a sub-pattern (`p as x`).
    Bind {
        /// The slot the value is written to.
        slot: Slot,
        /// The sub-pattern, for `as`.
        inner: Option<Box<Pattern>>,
    },
    /// A constructor pattern.
    Ctor {
        /// The constructor name.
        name: String,
        /// Its arguments.
        args: Vec<Pattern>,
    },
    /// A record pattern, with every field bound.
    Record(Vec<(String, Pattern)>),
    /// A tuple pattern.
    Tuple(Vec<Pattern>),
    /// A tensor pattern.
    Tensor {
        /// The leading elements.
        items: Vec<Pattern>,
        /// The `..rest` slot.
        rest: Option<Slot>,
    },
    /// A literal pattern.
    Lit(PatLit),
}

/// One top-level `fn`.
#[derive(Debug)]
pub struct FnDecl {
    /// The name it binds.
    pub name: String,
    /// The lowered function.
    pub function: Rc<Function>,
    /// Where it is written.
    pub span: Span,
}

/// One top-level `let`.
///
/// The pattern stays an AST pattern: a top-level binding writes into the
/// module's name table rather than into a slot vector, so it is bound by the
/// same pattern binder the AST path uses.
#[derive(Debug)]
pub struct LetDecl {
    /// The pattern, as written.
    pub pattern: aevia_syntax::ast::Pat,
    /// The lowered value.
    pub block: Rc<Block>,
    /// The slots the value uses.
    pub nslots: u32,
    /// Where it is written.
    pub span: Span,
}

/// One `instance` method.
#[derive(Debug)]
pub struct MethodDecl {
    /// The method name.
    pub name: String,
    /// The lowered body.
    pub function: Rc<Function>,
    /// Where it is written.
    pub span: Span,
}

/// One `instance` declaration.
#[derive(Debug)]
pub struct InstanceDecl {
    /// The class.
    pub class: String,
    /// The head, as written.
    pub head: Ty,
    /// Its methods.
    pub methods: Vec<MethodDecl>,
}

/// A lowered module.
#[derive(Debug, Default)]
pub struct Module {
    /// The `fn` declarations, in source order.
    pub fns: Vec<FnDecl>,
    /// The `let` declarations, in source order.
    pub lets: Vec<LetDecl>,
    /// The `instance` declarations, in source order.
    pub instances: Vec<InstanceDecl>,
}
