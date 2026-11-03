//! Lowering from the typed AST to the Core IR.
//!
//! The walk is total: every `ExprKind` becomes an IR form, so the interpreter
//! never falls back to the AST for a construct. The only things that stay
//! AST-shaped are the *data* a form carries — a top-level `let` keeps its
//! pattern, a record keeps its labels, a `do` block keeps its statement order —
//! never a sub-expression to evaluate.
//!
//! # Shape
//!
//! A block is a list of statements and a [`Tail`]. A statement is either a
//! named intermediate (`let x = rhs`) or a destructuring bind. Compound
//! expressions become a statement and yield an [`Atom`]; a call therefore takes
//! atoms, and an `if` tests an atom. That is the ANF invariant, and it is what
//! makes evaluation order explicit.
//!
//! # Closure conversion
//!
//! A name bound in an enclosing function is turned into an ordered capture: the
//! innermost function records `Capture::Local(slot)` and every function between
//! records `Capture::Capture(index)`. A capture is added the first time the
//! name is mentioned, so the environment order is deterministic and testable.

use crate::{
    Action, Arm, Atom, Block, Call, CallKind, Capture, DoPlan, DoTerminal, FnDecl, Function,
    InstanceDecl, Let, LetDecl, MethodDecl, Module, Pattern, Rhs, Slice, Stmt, StrPart, Tail,
};
use aevia_syntax::ast::{
    BinOp, BlockStmt, Decl, DeclKind, DoStmt, Expr, ExprKind, MatchArm, Param, Pat, PatKind,
    SliceAst, StrPiece,
};
use aevia_syntax::Span;
use aevia_types::check::{DictionaryUse, RssnCall};
use std::collections::HashMap;
use std::rc::Rc;

/// Lowers a whole module.
///
/// `rssn_calls` and `dictionaries` are the checker's records: the IR does not
/// rediscover the overload of a `Rssn.<op>` call or the instance of a class
/// method, it carries the span the checker pinned (PLAN D1, D3).
#[must_use]
pub fn lower_module(
    module: &aevia_syntax::ast::Module,
    rssn_calls: &[RssnCall],
    dictionaries: &[DictionaryUse],
) -> Module {
    let mut lowerer = Lowerer::new(rssn_calls, dictionaries);
    let mut out = Module::default();
    for decl in &module.decls {
        match &decl.kind {
            DeclKind::Fn {
                name, args, body, ..
            } => out.fns.push(FnDecl {
                name: name.clone(),
                function: lowerer.top_function(name, args, body, decl.span),
                span: decl.span,
            }),
            DeclKind::Let { pattern, value, .. } => {
                let (block, nslots) = lowerer.top_value(value);
                out.lets.push(LetDecl {
                    pattern: pattern.clone(),
                    block: Rc::new(block),
                    nslots,
                    span: decl.span,
                });
            }
            DeclKind::Instance { class, ty, methods } => {
                let mut lowered = Vec::with_capacity(methods.len());
                for method in methods {
                    lowered.push(MethodDecl {
                        name: method.name.clone(),
                        function: lowerer.top_function(
                            &method.name,
                            &method.params,
                            &method.body,
                            method.span,
                        ),
                        span: method.span,
                    });
                }
                out.instances.push(InstanceDecl {
                    class: class.clone(),
                    head: ty.clone(),
                    methods: lowered,
                });
            }
            _ => {}
        }
    }
    out
}

/// One lowered top-level declaration, for the REPL (§9.8).
#[derive(Debug)]
pub enum LoweredDecl {
    /// A `fn`.
    Fn(FnDecl),
    /// A `let`.
    Let(LetDecl),
    /// An `instance`.
    Instance(InstanceDecl),
}

/// Lowers one declaration, for an incremental session.
#[must_use]
pub fn lower_decl(
    decl: &Decl,
    rssn_calls: &[RssnCall],
    dictionaries: &[DictionaryUse],
) -> Option<LoweredDecl> {
    let mut lowerer = Lowerer::new(rssn_calls, dictionaries);
    match &decl.kind {
        DeclKind::Fn {
            name, args, body, ..
        } => Some(LoweredDecl::Fn(FnDecl {
            name: name.clone(),
            function: lowerer.top_function(name, args, body, decl.span),
            span: decl.span,
        })),
        DeclKind::Let { pattern, value, .. } => {
            let (block, nslots) = lowerer.top_value(value);
            Some(LoweredDecl::Let(LetDecl {
                pattern: pattern.clone(),
                block: Rc::new(block),
                nslots,
                span: decl.span,
            }))
        }
        DeclKind::Instance { class, ty, methods } => {
            let mut lowered = Vec::with_capacity(methods.len());
            for method in methods {
                lowered.push(MethodDecl {
                    name: method.name.clone(),
                    function: lowerer.top_function(
                        &method.name,
                        &method.params,
                        &method.body,
                        method.span,
                    ),
                    span: method.span,
                });
            }
            Some(LoweredDecl::Instance(InstanceDecl {
                class: class.clone(),
                head: ty.clone(),
                methods: lowered,
            }))
        }
        _ => None,
    }
}

/// One function being lowered, with its slot allocator and its lexical scopes.
struct FnState {
    name: String,
    nslots: u32,
    captures: Vec<Capture>,
    capture_index: HashMap<Capture, u32>,
    scopes: Vec<HashMap<String, Capture>>,
}

impl FnState {
    fn new(name: String, _span: Span) -> Self {
        Self {
            name,
            nslots: 0,
            captures: Vec::new(),
            capture_index: HashMap::new(),
            scopes: vec![HashMap::new()],
        }
    }
}

/// A block under construction.
#[derive(Default)]
struct Builder {
    stmts: Vec<Stmt>,
}

/// The lowering state: a stack of functions, innermost last.
struct Lowerer {
    /// Spans at which the checker pinned a `Rssn.<op>` overload.
    rssn: HashMap<Span, ()>,
    /// Spans at which the checker resolved a class method, with the method.
    dictionaries: HashMap<Span, Vec<String>>,
    fns: Vec<FnState>,
}

impl Lowerer {
    fn new(rssn_calls: &[RssnCall], dictionaries: &[DictionaryUse]) -> Self {
        let mut rssn = HashMap::with_capacity(rssn_calls.len());
        for call in rssn_calls {
            rssn.insert(call.span, ());
        }
        let mut table: HashMap<Span, Vec<String>> = HashMap::new();
        for use_site in dictionaries {
            table
                .entry(use_site.span)
                .or_default()
                .push(use_site.method.clone());
        }
        Self {
            rssn,
            dictionaries: table,
            fns: Vec::new(),
        }
    }

    // ----- slots, scopes and captures -------------------------------------

    fn state(&mut self) -> &mut FnState {
        self.fns.last_mut().expect("a function is being lowered")
    }

    fn fresh_slot(&mut self) -> u32 {
        let state = self.state();
        let slot = state.nslots;
        state.nslots += 1;
        slot
    }

    fn push_scope(&mut self) {
        self.state().scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.state().scopes.pop();
    }

    fn declare(&mut self, name: &str, place: Capture) {
        let state = self.state();
        state
            .scopes
            .last_mut()
            .expect("a scope is open")
            .insert(name.to_string(), place);
    }

    fn add_capture(&mut self, level: usize, key: Capture) -> u32 {
        let state = &mut self.fns[level];
        if let Some(index) = state.capture_index.get(&key) {
            return *index;
        }
        let index = u32::try_from(state.captures.len()).unwrap_or(u32::MAX);
        state.captures.push(key);
        state.capture_index.insert(key, index);
        index
    }

    /// Resolves a name to an atom, threading a capture through every function
    /// between the one that binds it and the one that uses it.
    fn resolve(&mut self, name: &str) -> Atom {
        let last = self.fns.len() - 1;
        let mut found: Option<(usize, Capture)> = None;
        for level in (0..=last).rev() {
            let state = &self.fns[level];
            for scope in state.scopes.iter().rev() {
                if let Some(place) = scope.get(name) {
                    found = Some((level, *place));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        let Some((level, place)) = found else {
            return Atom::Global(name.to_string());
        };
        if level == last {
            return match place {
                Capture::Local(slot) => Atom::Local(slot),
                Capture::Capture(index) => Atom::Capture(index),
            };
        }
        let mut key = place;
        let mut index = 0;
        for outer in (level + 1)..=last {
            index = self.add_capture(outer, key);
            key = Capture::Capture(index);
        }
        Atom::Capture(index)
    }

    fn emit(&mut self, builder: &mut Builder, rhs: Rhs, span: Span) -> Atom {
        let slot = self.fresh_slot();
        builder.stmts.push(Stmt::Let(Let { slot, rhs, span }));
        Atom::Local(slot)
    }

    // ----- functions ------------------------------------------------------

    fn top_function(&mut self, name: &str, params: &[Param], body: &Expr, span: Span) -> Rc<Function> {
        self.lambda(name.to_string(), params, body, span)
    }

    fn top_value(&mut self, value: &Expr) -> (Block, u32) {
        self.fns.push(FnState::new("<top>".to_string(), value.span));
        let block = self.block(value);
        let state = self.fns.pop().expect("the top-level state");
        debug_assert!(state.captures.is_empty(), "a top-level value captures nothing");
        (block, state.nslots)
    }

    fn lambda(&mut self, name: String, params: &[Param], body: &Expr, span: Span) -> Rc<Function> {
        self.fns.push(FnState::new(name, span));
        let patterns: Vec<Pattern> = params
            .iter()
            .map(|param| self.pattern(&param.pattern))
            .collect();
        let body_block = self.block(body);
        let state = self.fns.pop().expect("the function state");
        Rc::new(Function {
            name: state.name,
            params: patterns,
            body: Rc::new(body_block),
            captures: state.captures,
            nslots: state.nslots,
            span,
        })
    }

    // ----- blocks ---------------------------------------------------------

    fn block(&mut self, expr: &Expr) -> Block {
        self.push_scope();
        let block = self.block_inner(expr);
        self.pop_scope();
        block
    }

    fn block_inner(&mut self, expr: &Expr) -> Block {
        let mut builder = Builder::default();
        let tail = self.tail(expr, &mut builder);
        Block {
            stmts: builder.stmts,
            tail,
        }
    }

    fn tail(&mut self, expr: &Expr, builder: &mut Builder) -> Tail {
        match &expr.kind {
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let condition = self.value(cond, builder);
                Tail::If {
                    cond: condition,
                    cond_span: cond.span,
                    then_block: Rc::new(self.block(then_branch)),
                    else_block: Rc::new(self.block(else_branch)),
                    span: expr.span,
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                let subject = self.value(scrutinee, builder);
                Tail::Match {
                    scrutinee: subject,
                    arms: self.arms(arms),
                    span: expr.span,
                }
            }
            ExprKind::LetIn {
                pattern,
                value,
                body,
                ..
            } => {
                let atom = self.value(value, builder);
                self.push_scope();
                self.bind_value(pattern, atom, builder);
                let tail = self.tail(body, builder);
                self.pop_scope();
                tail
            }
            ExprKind::Block { stmts, tail } => {
                self.push_scope();
                for stmt in stmts {
                    self.statement(stmt, builder);
                }
                let result = match tail {
                    Some(tail) => self.tail(tail, builder),
                    None => Tail::Return {
                        atom: Atom::Unit,
                        span: expr.span,
                    },
                };
                self.pop_scope();
                result
            }
            ExprKind::Call { callee, args } => match self.call_rhs(expr, callee, args, builder) {
                Rhs::Call(call) => Tail::Call(call),
                other => {
                    let atom = self.emit(builder, other, expr.span);
                    Tail::Return {
                        atom,
                        span: expr.span,
                    }
                }
            },
            _ => {
                let atom = self.value(expr, builder);
                Tail::Return {
                    atom,
                    span: expr.span,
                }
            }
        }
    }

    fn statement(&mut self, stmt: &BlockStmt, builder: &mut Builder) {
        match stmt {
            BlockStmt::Let {
                pattern,
                value,
                span,
                ..
            } => {
                if let (PatKind::Var(name), ExprKind::Lambda { params, body, .. }) =
                    (&pattern.kind, &value.kind)
                {
                    // A block-local recursive function: the AST path lets the
                    // closure see the scope it is about to be bound in, so the
                    // IR captures its own slot and patches it after creation.
                    self.recursive(name, params, body, value.span, *span, builder);
                    return;
                }
                let atom = self.value(value, builder);
                self.bind_value(pattern, atom, builder);
            }
            BlockStmt::Fn(decl) => {
                if let DeclKind::Fn {
                    name, args, body, ..
                } = &decl.kind
                {
                    self.recursive(name, args, body, decl.span, decl.span, builder);
                }
            }
        }
    }

    fn recursive(
        &mut self,
        name: &str,
        params: &[Param],
        body: &Expr,
        body_span: Span,
        span: Span,
        builder: &mut Builder,
    ) {
        let slot = self.fresh_slot();
        self.declare(name, Capture::Local(slot));
        let function = self.lambda(name.to_string(), params, body, body_span);
        let self_capture = function
            .captures
            .iter()
            .position(|capture| *capture == Capture::Local(slot))
            .map(|index| u32::try_from(index).unwrap_or(u32::MAX));
        // A binding whose closure never mentions itself needs no patch step:
        // emit the plain closure, which is both cheaper and easier to read.
        let rhs = match self_capture {
            Some(index) => Rhs::RecClosure {
                function,
                self_capture: Some(index),
            },
            None => Rhs::Closure(function),
        };
        builder.stmts.push(Stmt::Let(Let { slot, rhs, span }));
    }

    /// Binds an atom to a pattern.
    ///
    /// A plain `let x = e` becomes a slot; anything with structure becomes a
    /// `Bind` statement, which the runtime matches exactly as `bind_pattern`
    /// does and reports `E0308` at the same span when it does not match.
    fn bind_value(&mut self, pattern: &Pat, atom: Atom, builder: &mut Builder) {
        match &pattern.kind {
            PatKind::Var(name) => {
                if let Atom::Local(slot) = atom {
                    self.declare(name, Capture::Local(slot));
                    return;
                }
                let slot = self.fresh_slot();
                self.declare(name, Capture::Local(slot));
                builder.stmts.push(Stmt::Let(Let {
                    slot,
                    rhs: Rhs::Atom(atom),
                    span: pattern.span,
                }));
            }
            PatKind::Wildcard => {}
            _ => {
                let lowered = self.pattern(pattern);
                builder.stmts.push(Stmt::Bind {
                    pattern: lowered,
                    atom,
                    span: pattern.span,
                });
            }
        }
    }

    fn arms(&mut self, arms: &[MatchArm]) -> Vec<Arm> {
        arms.iter()
            .map(|arm| {
                self.push_scope();
                let pattern = self.pattern(&arm.pattern);
                let guard = arm.guard.as_ref().map(|guard| Rc::new(self.block(guard)));
                let body = Rc::new(self.block(&arm.body));
                self.pop_scope();
                Arm {
                    pattern,
                    guard,
                    body,
                    span: arm.span,
                }
            })
            .collect()
    }

    fn pattern(&mut self, pattern: &Pat) -> Pattern {
        match &pattern.kind {
            PatKind::Error => Pattern::Error,
            PatKind::Wildcard => Pattern::Wildcard,
            PatKind::Var(name) => {
                let slot = self.fresh_slot();
                self.declare(name, Capture::Local(slot));
                Pattern::Bind { slot, inner: None }
            }
            PatKind::Ctor { name, args } => Pattern::Ctor {
                name: name.clone(),
                args: args.iter().map(|arg| self.pattern(arg)).collect(),
            },
            PatKind::Record(fields) => Pattern::Record(
                fields
                    .iter()
                    .map(|(name, sub)| {
                        let lowered = match sub {
                            Some(sub) => self.pattern(sub),
                            None => {
                                let slot = self.fresh_slot();
                                self.declare(name, Capture::Local(slot));
                                Pattern::Bind { slot, inner: None }
                            }
                        };
                        (name.clone(), lowered)
                    })
                    .collect(),
            ),
            PatKind::Tuple(items) => {
                Pattern::Tuple(items.iter().map(|item| self.pattern(item)).collect())
            }
            PatKind::Tensor { items, rest } => Pattern::Tensor {
                items: items.iter().map(|item| self.pattern(item)).collect(),
                rest: rest.as_ref().map(|name| {
                    let slot = self.fresh_slot();
                    self.declare(name, Capture::Local(slot));
                    slot
                }),
            },
            PatKind::Lit(literal) => Pattern::Lit(literal.clone()),
            PatKind::As { pattern: inner, name } => {
                let lowered = self.pattern(inner);
                let slot = self.fresh_slot();
                self.declare(name, Capture::Local(slot));
                Pattern::Bind {
                    slot,
                    inner: Some(Box::new(lowered)),
                }
            }
            PatKind::Annot { pattern: inner, .. } => self.pattern(inner),
        }
    }

    // ----- expressions ----------------------------------------------------

    fn value(&mut self, expr: &Expr, builder: &mut Builder) -> Atom {
        match &expr.kind {
            ExprKind::Error | ExprKind::UnitLit => Atom::Unit,
            ExprKind::Bool(value) => Atom::Bool(*value),
            ExprKind::Int(value) => Atom::Int(value.clone()),
            ExprKind::Float(text) => Atom::Float(text.clone()),
            ExprKind::Str(pieces) => {
                if pieces
                    .iter()
                    .all(|piece| matches!(piece, StrPiece::Text(_)))
                {
                    let mut text = String::new();
                    for piece in pieces {
                        if let StrPiece::Text(part) = piece {
                            text.push_str(part);
                        }
                    }
                    Atom::Str(text)
                } else {
                    let mut parts = Vec::with_capacity(pieces.len());
                    for piece in pieces {
                        match piece {
                            StrPiece::Text(text) => parts.push(StrPart::Text(text.clone())),
                            StrPiece::Expr(inner) => {
                                let atom = self.value(inner, builder);
                                parts.push(StrPart::Atom(atom));
                            }
                        }
                    }
                    self.emit(builder, Rhs::Str(parts), expr.span)
                }
            }
            ExprKind::Var(name) => self.resolve(name),
            ExprKind::QName(path) => {
                if path.len() == 2 && path[0] == "Rssn" {
                    self.emit(
                        builder,
                        Rhs::RssnMember {
                            name: path[1].clone(),
                        },
                        expr.span,
                    )
                } else {
                    Atom::Global(path.join("."))
                }
            }
            ExprKind::Unary { op, operand } => {
                let operand = self.value(operand, builder);
                self.emit(builder, Rhs::Unary { op: *op, operand }, expr.span)
            }
            ExprKind::Binary { op, left, right } => {
                if matches!(op, BinOp::And | BinOp::Or) {
                    let left = self.value(left, builder);
                    let right = Rc::new(self.block(right));
                    self.emit(
                        builder,
                        Rhs::AndOr {
                            op: *op,
                            left,
                            right,
                        },
                        expr.span,
                    )
                } else {
                    let left = self.value(left, builder);
                    let right = self.value(right, builder);
                    self.emit(
                        builder,
                        Rhs::Binary {
                            op: *op,
                            left,
                            right,
                        },
                        expr.span,
                    )
                }
            }
            ExprKind::Call { callee, args } => {
                let rhs = self.call_rhs(expr, callee, args, builder);
                self.emit(builder, rhs, expr.span)
            }
            ExprKind::TypeApp { callee, .. } => self.value(callee, builder),
            ExprKind::Lambda { params, body, .. } => {
                let function = self.lambda("<lambda>".to_string(), params, body, expr.span);
                self.emit(builder, Rhs::Closure(function), expr.span)
            }
            ExprKind::If { .. } | ExprKind::Match { .. } | ExprKind::LetIn { .. } => {
                let block = self.block(expr);
                self.emit(builder, Rhs::Block(Rc::new(block)), expr.span)
            }
            ExprKind::Block { .. } => {
                let block = self.block(expr);
                self.emit(builder, Rhs::Block(Rc::new(block)), expr.span)
            }
            ExprKind::Do { stmts } => {
                let plan = self.do_plan(stmts, builder);
                self.emit(builder, Rhs::Do(plan), expr.span)
            }
            ExprKind::Record { base, fields } => {
                let base = base.as_ref().map(|base| self.value(base, builder));
                let mut lowered = Vec::with_capacity(fields.len());
                for (name, value) in fields {
                    let atom = self.value(value, builder);
                    lowered.push((name.clone(), atom));
                }
                self.emit(
                    builder,
                    Rhs::Record {
                        base,
                        fields: lowered,
                    },
                    expr.span,
                )
            }
            ExprKind::Field { base, name } => {
                if is_rssn_root(base) {
                    return self.emit(
                        builder,
                        Rhs::RssnMember { name: name.clone() },
                        expr.span,
                    );
                }
                let qualified = qualified_name(base, name);
                let base = self.value(base, builder);
                self.emit(
                    builder,
                    Rhs::Field {
                        base,
                        qualified,
                        name: name.clone(),
                    },
                    expr.span,
                )
            }
            ExprKind::Tuple(items) => {
                let mut lowered = Vec::with_capacity(items.len());
                for item in items {
                    let atom = self.value(item, builder);
                    lowered.push(atom);
                }
                self.emit(builder, Rhs::Tuple(lowered), expr.span)
            }
            ExprKind::Tensor(items) => {
                let mut lowered = Vec::with_capacity(items.len());
                for item in items {
                    let atom = self.value(item, builder);
                    lowered.push(atom);
                }
                self.emit(builder, Rhs::Tensor(lowered), expr.span)
            }
            ExprKind::Index { base, indices } => {
                let base = self.value(base, builder);
                let mut lowered = Vec::with_capacity(indices.len());
                for index in indices {
                    match index {
                        SliceAst::Index(expr) => {
                            let atom = self.value(expr, builder);
                            lowered.push(Slice::Index(atom));
                        }
                        SliceAst::Range {
                            start,
                            end,
                            inclusive,
                            step,
                        } => {
                            let start = start.as_ref().map(|expr| self.value(expr, builder));
                            let end = end.as_ref().map(|expr| self.value(expr, builder));
                            let step = step.as_ref().map(|expr| self.value(expr, builder));
                            lowered.push(Slice::Range {
                                start,
                                end,
                                inclusive: *inclusive,
                                step,
                            });
                        }
                    }
                }
                self.emit(
                    builder,
                    Rhs::Index {
                        base,
                        indices: lowered,
                    },
                    expr.span,
                )
            }
            ExprKind::Annot { expr: inner, .. } => self.value(inner, builder),
            ExprKind::Placeholder => self.emit(
                builder,
                Rhs::Error {
                    code: "E0202",
                    message: "`_` cannot be evaluated outside a call".to_string(),
                },
                expr.span,
            ),
            ExprKind::Try(inner) => {
                let atom = self.value(inner, builder);
                self.emit(builder, Rhs::Try(atom), expr.span)
            }
            ExprKind::Factorial { expr: inner, double } => {
                let value = self.value(inner, builder);
                self.emit(
                    builder,
                    Rhs::Factorial {
                        value,
                        double: *double,
                    },
                    expr.span,
                )
            }
            ExprKind::Quantity { value, unit } => self.emit(
                builder,
                Rhs::Quantity {
                    value: value.clone(),
                    unit: unit.clone(),
                },
                expr.span,
            ),
        }
    }

    fn call_rhs(
        &mut self,
        expr: &Expr,
        callee: &Expr,
        args: &[Expr],
        builder: &mut Builder,
    ) -> Rhs {
        // `f(_, x)` is a lambda over the placeholders (§2.3.6).
        if args
            .iter()
            .any(|arg| matches!(arg.kind, ExprKind::Placeholder))
        {
            let mut params = Vec::new();
            let mut call_args = Vec::new();
            for (index, arg) in args.iter().enumerate() {
                if matches!(arg.kind, ExprKind::Placeholder) {
                    let name = format!("__arg{index}");
                    params.push(Param {
                        pattern: Pat::new(PatKind::Var(name.clone()), arg.span),
                        ty: None,
                        span: arg.span,
                    });
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
            let function = self.lambda("<lambda>".to_string(), &params, &body, expr.span);
            return Rhs::Closure(function);
        }
        // `zeros<Int>(3)` names the element type the result must have (§8.4).
        if let ExprKind::TypeApp {
            callee: inner,
            args: types,
        } = &callee.kind
        {
            if let ExprKind::Var(name) = &inner.kind {
                if aevia_types::builtins::takes_type_argument(name) {
                    let values = self.values(args, builder);
                    return Rhs::Call(Call {
                        callee: Atom::Unit,
                        args: values,
                        kind: CallKind::Typed {
                            name: name.clone(),
                            types: types.clone(),
                        },
                        span: expr.span,
                    });
                }
            }
        }
        // `Rssn.<op>(…)`: the checker pinned the overload at this span.
        if self.rssn.contains_key(&expr.span) {
            if let Some(name) = rssn_operator_name(callee) {
                let values = self.values(args, builder);
                return Rhs::Call(Call {
                    callee: Atom::Unit,
                    args: values,
                    kind: CallKind::Rssn { name },
                    span: expr.span,
                });
            }
        }
        // A class method the checker resolved to an instance (§3.8.1).
        if let ExprKind::Var(name) = &callee.kind {
            if self
                .dictionaries
                .get(&expr.span)
                .is_some_and(|methods| methods.iter().any(|method| method == name))
            {
                let values = self.values(args, builder);
                return Rhs::Call(Call {
                    callee: Atom::Global(name.clone()),
                    args: values,
                    kind: CallKind::Dictionary {
                        method: name.clone(),
                    },
                    span: expr.span,
                });
            }
        }
        let function = self.value(callee, builder);
        let values = self.values(args, builder);
        Rhs::Call(Call {
            callee: function,
            args: values,
            kind: CallKind::Normal,
            span: expr.span,
        })
    }

    fn values(&mut self, args: &[Expr], builder: &mut Builder) -> Vec<Atom> {
        args.iter().map(|arg| self.value(arg, builder)).collect()
    }

    // ----- `do` -----------------------------------------------------------

    fn do_plan(&mut self, stmts: &[DoStmt], builder: &mut Builder) -> DoPlan {
        let mut prefix = Vec::new();
        for (index, stmt) in stmts.iter().enumerate() {
            let rest = &stmts[index + 1..];
            match stmt {
                DoStmt::Expr(expr) => {
                    let atom = self.value(expr, builder);
                    prefix.push(Action {
                        atom,
                        span: expr.span,
                    });
                }
                DoStmt::Let {
                    pattern,
                    value,
                    span,
                    ..
                } => {
                    let thunk = self.do_thunk(Some((pattern, value)), None, rest, *span);
                    return DoPlan {
                        prefix,
                        terminal: DoTerminal::Let { thunk },
                    };
                }
                DoStmt::Bind {
                    pattern,
                    value,
                    span,
                } => {
                    let action = self.value(value, builder);
                    let continuation = self.do_thunk(None, Some(pattern), rest, *span);
                    return DoPlan {
                        prefix,
                        terminal: DoTerminal::Bind {
                            action,
                            span: value.span,
                            continuation,
                        },
                    };
                }
            }
        }
        DoPlan {
            prefix,
            terminal: DoTerminal::End,
        }
    }

    /// Lowers the rest of a `do` block as a thunk.
    ///
    /// A `do let p = e; rest` thunk binds `p` from `e` when it runs; a `p <- a;
    /// rest` continuation takes `p` as its parameter. In both cases the rest is
    /// lowered with the binders already in scope, so it reads them as slots of
    /// this frame rather than as globals.
    fn do_thunk(
        &mut self,
        binding: Option<(&Pat, &Expr)>,
        param: Option<&Pat>,
        rest: &[DoStmt],
        span: Span,
    ) -> Rc<Function> {
        self.fns.push(FnState::new("<do>".to_string(), span));
        let mut params = Vec::new();
        if let Some(param) = param {
            params.push(self.pattern(param));
        }
        let mut builder = Builder::default();
        if let Some((pattern, value)) = binding {
            let atom = self.value(value, &mut builder);
            self.bind_value(pattern, atom, &mut builder);
        }
        let plan = self.do_plan(rest, &mut builder);
        let atom = self.emit(&mut builder, Rhs::Do(plan), span);
        let block = Block {
            stmts: builder.stmts,
            tail: Tail::Return { atom, span },
        };
        let state = self.fns.pop().expect("the thunk state");
        Rc::new(Function {
            name: state.name,
            params,
            body: Rc::new(block),
            captures: state.captures,
            nslots: state.nslots,
            span,
        })
    }
}

/// Whether an expression names the `Rssn` module itself.
fn is_rssn_root(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Var(name) => name == "Rssn",
        ExprKind::QName(path) => path.len() == 1 && path[0] == "Rssn",
        _ => false,
    }
}

/// The operator a `Rssn.<op>` callee names, when it names one structurally.
///
/// Whether the *checker* resolved it is a separate question, answered by the
/// pinned call table: entry points such as `Rssn.dynamic` are not operators and
/// have no pinned key.
fn rssn_operator_name(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Field { base, name } if is_rssn_root(base) => Some(name.clone()),
        ExprKind::QName(path) if path.len() == 2 && path[0] == "Rssn" => Some(path[1].clone()),
        _ => None,
    }
}

/// The flat key a qualified field access names, when the base is a path or an
/// import alias.
fn qualified_name(base: &Expr, field: &str) -> Option<String> {
    match &base.kind {
        ExprKind::QName(path) => Some(format!("{}.{field}", path.join("."))),
        ExprKind::Var(name) if name.chars().next().is_some_and(char::is_uppercase) => {
            Some(format!("{name}.{field}"))
        }
        _ => None,
    }
}
