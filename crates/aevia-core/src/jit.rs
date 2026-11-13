//! Native code for `@jit` functions.
//!
//! Pure `Float`/`Int`/`Bool` functions are lowered to RSSN's LIR (a typed
//! register machine with real branches, loops and calls) and compiled with
//! Cranelift; see the section "Native compilation through LIR" below. The
//! straight-line [`Builder`] remains for the device seam ([`map_floats`]),
//! which needs a tape a GPU can run. Effects, captures, exact numbers, units
//! and tensors stay in the interpreter.
use crate::value::NumKind;
use crate::value::NumVal;
use crate::value::Value;
use aevia_dim::Dim;
use aevia_ir::Atom;
use aevia_ir::Block;
use aevia_ir::CallKind;
use aevia_ir::Function;
use aevia_ir::Pattern;
use aevia_ir::Rhs;
use aevia_ir::Stmt;
use aevia_ir::Tail;
use aevia_rssn::jit::Cmp;
use aevia_rssn::jit::Inst;
#[cfg(test)]
use aevia_rssn::jit::LoopProgram;
#[cfg(test)]
use aevia_rssn::jit::Program;
use aevia_rssn::jit::Reg;
use aevia_syntax::Span;
use aevia_syntax::ast::BinOp;
use aevia_syntax::ast::PatLit;
use aevia_syntax::ast::UnOp;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::rc::Weak;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Kind {
    Float,
    Bool,
    Int,
}
#[derive(Clone, Copy)]
struct Operand {
    reg: Reg,
    kind: Kind,
}
struct Builder<'a> {
    insts: Vec<Inst>,
    literals: &'a [(Span, u64)],
}

impl Builder<'_> {
    fn emit(
        &mut self,
        inst: Inst,
        kind: Kind,
    ) -> Option<Operand> {
        let reg = u32::try_from(self.insts.len()).ok()?;
        self.insts.push(inst);
        Some(Operand { reg, kind })
    }

    fn atom(
        &mut self,
        atom: &Atom,
        slots: &[Option<Operand>],
    ) -> Option<Operand> {
        match atom {
            | Atom::Local(slot) => *slots.get(*slot as usize)?,
            | Atom::Float(x) => self.emit(Inst::Const(*x), Kind::Float),
            | Atom::Bool(x) => self.emit(Inst::Const(if *x { 1.0 } else { 0.0 }), Kind::Bool),
            | _ => None,
        }
    }

    fn statements(
        &mut self,
        block: &Block,
        slots: &mut [Option<Operand>],
    ) -> Option<()> {
        for stmt in &block.stmts {
            match stmt {
                | Stmt::Let(item) => {
                    let value = match &item.rhs {
                        | Rhs::Atom(atom) => self.atom(atom, slots)?,
                        | Rhs::NumericLiteral(_) => {
                            let (_, bits) =
                                self.literals.iter().find(|(span, _)| *span == item.span)?;
                            self.emit(Inst::Const(f64::from_bits(*bits)), Kind::Float)?
                        },
                        | Rhs::Unary { op, operand } => {
                            let a = self.atom(operand, slots)?;
                            match (op, a.kind) {
                                | (UnOp::Pos, Kind::Float) => a,
                                | (UnOp::Neg, Kind::Float) => {
                                    self.emit(Inst::Neg(a.reg), Kind::Float)?
                                },
                                | (UnOp::Not, Kind::Bool) => {
                                    let zero = self.emit(Inst::Const(0.0), Kind::Bool)?;
                                    self.emit(Inst::Cmp(Cmp::Eq, a.reg, zero.reg), Kind::Bool)?
                                },
                                | _ => return None,
                            }
                        },
                        | Rhs::Binary { op, left, right } => {
                            let a = self.atom(left, slots)?;
                            let b = self.atom(right, slots)?;
                            if a.kind != b.kind
                                || (a.kind == Kind::Bool && !matches!(op, BinOp::Eq | BinOp::Ne))
                            {
                                return None;
                            }
                            let (inst, kind) = match op {
                                | BinOp::Add => (Inst::Add(a.reg, b.reg), Kind::Float),
                                | BinOp::Sub => (Inst::Sub(a.reg, b.reg), Kind::Float),
                                | BinOp::Mul => (Inst::Mul(a.reg, b.reg), Kind::Float),
                                | BinOp::Div => (Inst::Div(a.reg, b.reg), Kind::Float),
                                | BinOp::Eq => (Inst::Cmp(Cmp::Eq, a.reg, b.reg), Kind::Bool),
                                | BinOp::Ne => (Inst::Cmp(Cmp::Ne, a.reg, b.reg), Kind::Bool),
                                | BinOp::Lt => (Inst::Cmp(Cmp::Lt, a.reg, b.reg), Kind::Bool),
                                | BinOp::Le => (Inst::Cmp(Cmp::Le, a.reg, b.reg), Kind::Bool),
                                | BinOp::Gt => (Inst::Cmp(Cmp::Gt, a.reg, b.reg), Kind::Bool),
                                | BinOp::Ge => (Inst::Cmp(Cmp::Ge, a.reg, b.reg), Kind::Bool),
                                | _ => return None,
                            };
                            self.emit(inst, kind)?
                        },
                        | Rhs::AndOr { op, left, right } if matches!(op, BinOp::And | BinOp::Or) =>
                        {
                            let left = self.atom(left, slots)?;
                            if left.kind != Kind::Bool {
                                return None;
                            }
                            // Only a fully pure compiled RHS may be evaluated
                            // eagerly; calls, errors and effects reject compilation.
                            let right = self.block(right, &mut slots.to_vec())?;
                            if right.kind != Kind::Bool {
                                return None;
                            }
                            let constant = self.emit(
                                Inst::Const(if matches!(op, BinOp::And) {
                                    0.0
                                } else {
                                    1.0
                                }),
                                Kind::Bool,
                            )?;
                            let (yes, no) = if matches!(op, BinOp::And) {
                                (right.reg, constant.reg)
                            } else {
                                (constant.reg, right.reg)
                            };
                            self.emit(Inst::Select(left.reg, yes, no), Kind::Bool)?
                        },
                        | Rhs::Block(block) => self.block(block, &mut slots.to_vec())?,
                        | _ => return None,
                    };
                    *slots.get_mut(item.slot as usize)? = Some(value);
                },
                | Stmt::Bind {
                    pattern: Pattern::Bind { slot, inner: None },
                    atom,
                    ..
                } => {
                    let value = self.atom(atom, slots)?;
                    *slots.get_mut(*slot as usize)? = Some(value);
                },
                | _ => return None,
            }
        }
        Some(())
    }

    fn block(
        &mut self,
        block: &Block,
        slots: &mut [Option<Operand>],
    ) -> Option<Operand> {
        self.statements(block, slots)?;
        match &block.tail {
            | Tail::Return { atom, .. } => self.atom(atom, slots),
            | Tail::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                let c = self.atom(cond, slots)?;
                if c.kind != Kind::Bool {
                    return None;
                }
                let a = self.block(then_block, &mut slots.to_vec())?;
                let b = self.block(else_block, &mut slots.to_vec())?;
                if a.kind != b.kind {
                    return None;
                }
                self.emit(Inst::Select(c.reg, a.reg, b.reg), a.kind)
            },
            | _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Native compilation through LIR
// ---------------------------------------------------------------------------
//
// A `@jit` function is lowered to RSSN's LIR (`rssn::backend::lir`), a typed
// register machine with real branches, loops and calls, and compiled to
// machine code. Compared with a straight-line tape this means:
//
// * branches are real: only the taken arm runs, so arms may contain calls,
//   checked arithmetic and other code that must not run speculatively;
// * every self tail call becomes a jump with a fuel check, from any number of
//   call sites, and a self call in value position is a real recursive call;
// * `Int` is a native 64-bit integer whose `+ - *` are checked: an overflow
//   *bails* and the interpreter re-runs the call with arbitrary precision;
// * the elementary functions of §8 are guarded so that every case in which
//   the interpreter would not simply return `f(x)` (a domain error, an
//   extended-real special value) bails instead.
//
// Native code is pure — it reads its arguments and writes nothing — so a bail
// is invisible: the interpreter repeats the call from the same arguments.
//
// Two LIR functions are generated per activation. The *entry* (function 0)
// returns `(status, result, next arguments…, fuel left)`: status `0` is a
// value, status `k + 1` means the tail call at site `k` ran out of fuel and
// the interpreter continues the trampoline from the returned arguments. The
// *value* form (function 1) returns the result alone; it serves recursive
// calls in value position, and running out of fuel there bails.

use aevia_rssn::jit::lir;
use aevia_rssn::jit::lir::FunctionBuilder;
use aevia_rssn::jit::lir::Inst as L;
use aevia_rssn::jit::lir::Reg as LReg;
use aevia_rssn::jit::lir::Ty;

/// A numeric literal resolved under one activation.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Lit {
    /// A `Float`, by its exact bits (so `-0.0` and `0.0` differ).
    F(u64),
    /// An `Int` that fits 64 bits.
    I(i64),
}

impl From<f64> for Lit {
    fn from(x: f64) -> Self {
        Self::F(x.to_bits())
    }
}

/// Bail code: the interpreter must handle this call (a domain error, an
/// unmatched `match`, fuel running out in a recursive call).
const BAIL_INTERPRET: u32 = 100;

const fn lir_ty(kind: Kind) -> Ty {
    match kind {
        | Kind::Float => Ty::F64,
        | Kind::Int => Ty::I64,
        | Kind::Bool => Ty::Bool,
    }
}

/// Whether the global `name` is the builtin of that name (not shadowed).
pub(crate) type BuiltinCheck<'a> = &'a mut dyn FnMut(&str) -> bool;

/// What native execution asks the interpreter before entering.
pub(crate) enum Ask<'a> {
    /// Does the self call whose callee is written at this span still call
    /// this very activation?
    IsSelf(Span),
    /// Is this global the builtin of that name?
    Builtin(&'a str),
}

/// Where a block's value goes.
#[derive(Clone, Copy)]
enum Cont {
    /// The function returns it.
    Return,
    /// It is assigned to the result register of a value-position block, and
    /// control continues at `join`.
    Assign { join: lir::BlockId },
}

/// The activation being compiled.
struct Lower<'a> {
    f: FunctionBuilder,
    function: &'a Function,
    literals: &'a [(Span, Lit)],
    builtin: BuiltinCheck<'a>,
    params: &'a [Kind],
    result: Kind,
    /// `true` for the entry form, `false` for the value form.
    entry: bool,
    head: lir::BlockId,
    head_params: Vec<LReg>,
    fuel: LReg,
    /// Spans `(call, callee)` of the self calls, in discovery order.
    sites: Vec<(Span, Span)>,
    /// The result register of the value-position block being lowered.
    assign: Vec<Option<(LReg, Kind)>>,
    /// Number of tail self calls (native loops).
    tails: usize,
    /// Builtins the code calls, re-checked before every native run.
    used: Vec<String>,
}

type Env = Vec<Option<(LReg, Kind)>>;

impl Lower<'_> {
    fn emit(
        &mut self,
        inst: L,
        kind: Kind,
    ) -> (LReg, Kind) {
        (self.f.ins_typed(inst, lir_ty(kind)), kind)
    }

    /// Branches to a fresh bail block when `flag` holds.
    fn bail_if(
        &mut self,
        flag: LReg,
    ) {
        let bail = self.f.new_block();
        let next = self.f.new_block();
        self.f.branch(flag, bail, &[], next, &[]);
        self.f.switch_to(bail);
        self.f.bail(BAIL_INTERPRET);
        self.f.switch_to(next);
    }

    fn literal(
        &self,
        span: Span,
    ) -> Option<Lit> {
        self.literals
            .iter()
            .find(|(s, _)| *s == span)
            .map(|(_, lit)| *lit)
    }

    fn atom(
        &mut self,
        atom: &Atom,
        env: &Env,
    ) -> Option<(LReg, Kind)> {
        match atom {
            | Atom::Local(slot) => *env.get(*slot as usize)?,
            | Atom::Float(x) => Some(self.emit(L::F(*x), Kind::Float)),
            | Atom::Bool(x) => Some(self.emit(L::B(*x), Kind::Bool)),
            | Atom::Int(x) => {
                let x = i64::try_from(x).ok()?;
                Some(self.emit(L::I(x), Kind::Int))
            },
            | _ => None,
        }
    }

    fn float(
        &mut self,
        v: (LReg, Kind),
    ) -> LReg {
        match v.1 {
            | Kind::Int => self.f.ins(L::IToF(v.0)),
            | _ => v.0,
        }
    }

    /// `a == b` on booleans.
    fn bool_eq(
        &mut self,
        a: LReg,
        b: LReg,
    ) -> LReg {
        let both = self.f.ins(L::And(a, b));
        let na = self.f.ins(L::Not(a));
        let nb = self.f.ins(L::Not(b));
        let neither = self.f.ins(L::And(na, nb));
        self.f.ins(L::Or(both, neither))
    }

    /// `plain_scalar_binary` of the interpreter, natively.
    fn binary(
        &mut self,
        op: BinOp,
        a: (LReg, Kind),
        b: (LReg, Kind),
    ) -> Option<(LReg, Kind)> {
        let cmp = match op {
            | BinOp::Eq => Some(Cmp::Eq),
            | BinOp::Ne => Some(Cmp::Ne),
            | BinOp::Lt => Some(Cmp::Lt),
            | BinOp::Le => Some(Cmp::Le),
            | BinOp::Gt => Some(Cmp::Gt),
            | BinOp::Ge => Some(Cmp::Ge),
            | _ => None,
        };
        Some(match (a.1, b.1) {
            | (Kind::Int, Kind::Int) => {
                match op {
                    | BinOp::Add => self.emit(L::IAdd(a.0, b.0), Kind::Int),
                    | BinOp::Sub => self.emit(L::ISub(a.0, b.0), Kind::Int),
                    | BinOp::Mul => self.emit(L::IMul(a.0, b.0), Kind::Int),
                    | _ => self.emit(L::ICmp(cmp?, a.0, b.0), Kind::Bool),
                }
            },
            | (Kind::Float, Kind::Float) => {
                match op {
                    | BinOp::Add => self.emit(L::FAdd(a.0, b.0), Kind::Float),
                    | BinOp::Sub => self.emit(L::FSub(a.0, b.0), Kind::Float),
                    | BinOp::Mul => self.emit(L::FMul(a.0, b.0), Kind::Float),
                    | BinOp::Div => self.emit(L::FDiv(a.0, b.0), Kind::Float),
                    | _ => self.emit(L::FCmp(cmp?, a.0, b.0), Kind::Bool),
                }
            },
            | (Kind::Int, Kind::Float) | (Kind::Float, Kind::Int) => {
                // `Int` promotes to `Float` on the arithmetic path only.
                let (x, y) = (self.float(a), self.float(b));
                match op {
                    | BinOp::Add => self.emit(L::FAdd(x, y), Kind::Float),
                    | BinOp::Sub => self.emit(L::FSub(x, y), Kind::Float),
                    | BinOp::Mul => self.emit(L::FMul(x, y), Kind::Float),
                    | BinOp::Div => self.emit(L::FDiv(x, y), Kind::Float),
                    | _ => return None,
                }
            },
            | (Kind::Bool, Kind::Bool) => {
                match op {
                    | BinOp::Eq => (self.bool_eq(a.0, b.0), Kind::Bool),
                    | BinOp::Ne => {
                        let eq = self.bool_eq(a.0, b.0);
                        self.emit(L::Not(eq), Kind::Bool)
                    },
                    | _ => return None,
                }
            },
            | _ => return None,
        })
    }

    /// A guarded elementary function of a `Float`: the native result when
    /// the interpreter would return exactly `f(x)`, a bail otherwise.
    fn builtin(
        &mut self,
        name: &str,
        x: (LReg, Kind),
    ) -> Option<(LReg, Kind)> {
        if !(self.builtin)(name) {
            return None;
        }
        if !self.used.iter().any(|u| u == name) {
            self.used.push(name.to_owned());
        }
        self.elementary(name, x)
    }

    fn elementary(
        &mut self,
        name: &str,
        x: (LReg, Kind),
    ) -> Option<(LReg, Kind)> {
        use aevia_rssn::jit::Intrinsic as I;
        let x = match x.1 {
            | Kind::Float => x.0,
            | _ => return None,
        };
        extern "C" fn log2(x: f64) -> f64 {
            x.log2()
        }
        extern "C" fn log10(x: f64) -> f64 {
            x.log10()
        }
        let unary = |i: I| L::FUnary(i, x);
        let y = match name {
            | "sqrt" | "cbrt" => {
                // `Root(n)`: `x.powf(1/n)`, an odd root of a negative float is
                // `-(-x).powf(1/n)`, an even one a domain error.
                let degree = if name == "sqrt" { 2.0 } else { 3.0 };
                let zero = self.f.ins(L::F(0.0));
                let negative = self.f.ins(L::FCmp(Cmp::Lt, x, zero));
                let e = self.f.ins(L::F(1.0 / degree));
                if name == "sqrt" {
                    self.bail_if(negative);
                    self.f.ins(L::FPow(x, e))
                } else {
                    let minus = self.f.ins(L::FNeg(x));
                    let magnitude = self.f.ins(L::Select(negative, minus, x));
                    let root = self.f.ins(L::FPow(magnitude, e));
                    let negated = self.f.ins(L::FNeg(root));
                    self.f.ins(L::Select(negative, negated, root))
                }
            },
            | "exp" | "ln" | "log" => {
                // §6.3.4's extended-real values at 0 and ±oo are exact
                // answers of the interpreter: leave them to it.
                let inf = self.f.ins(L::F(f64::INFINITY));
                let magnitude = self.f.ins(L::FUnary(I::Abs, x));
                let infinite = self.f.ins(L::FCmp(Cmp::Eq, magnitude, inf));
                self.bail_if(infinite);
                if name == "exp" {
                    self.f.ins(unary(I::Exp))
                } else {
                    let zero = self.f.ins(L::F(0.0));
                    let at_zero = self.f.ins(L::FCmp(Cmp::Eq, x, zero));
                    self.bail_if(at_zero);
                    self.f.ins(unary(I::Ln))
                }
            },
            | "log2" => self.f.ins(L::Extern1(log2, x)),
            | "log10" => self.f.ins(L::Extern1(log10, x)),
            | "sin" => self.f.ins(unary(I::Sin)),
            | "cos" => self.f.ins(unary(I::Cos)),
            | "tan" => self.f.ins(unary(I::Tan)),
            | "asin" => self.f.ins(unary(I::Asin)),
            | "acos" => self.f.ins(unary(I::Acos)),
            | "atan" => self.f.ins(unary(I::Atan)),
            | "sinh" => self.f.ins(unary(I::Sinh)),
            | "cosh" => self.f.ins(unary(I::Cosh)),
            | "tanh" => self.f.ins(unary(I::Tanh)),
            | "asinh" => self.f.ins(unary(I::Asinh)),
            | "acosh" => self.f.ins(unary(I::Acosh)),
            | "atanh" => self.f.ins(unary(I::Atanh)),
            | _ => return None,
        };
        // A NaN from a non-NaN argument is the interpreter's domain error.
        let y_nan = self.f.ins(L::FCmp(Cmp::Ne, y, y));
        let x_nan = self.f.ins(L::FCmp(Cmp::Ne, x, x));
        let x_ok = self.f.ins(L::Not(x_nan));
        let domain = self.f.ins(L::And(y_nan, x_ok));
        self.bail_if(domain);
        Some((y, Kind::Float))
    }

    /// Whether `call` calls the function being compiled.
    fn is_self_call(
        &self,
        call: &aevia_ir::Call,
    ) -> bool {
        matches!(call.kind, CallKind::Normal | CallKind::Typed { .. })
            && matches!(&call.callee, Atom::Global(callee) if **callee == *self.function.name)
            && call.args.len() == self.params.len()
    }

    /// The argument registers of a self call, checked against the
    /// parameter kinds.
    fn self_args(
        &mut self,
        call: &aevia_ir::Call,
        env: &Env,
    ) -> Option<Vec<LReg>> {
        let mut values = Vec::with_capacity(call.args.len());
        for (atom, &kind) in call.args.iter().zip(self.params) {
            let v = self.atom(atom, env)?;
            if v.1 != kind {
                return None;
            }
            values.push(v.0);
        }
        self.sites.push((call.span, call.callee_span));
        Some(values)
    }

    /// A call in value position.
    fn call_value(
        &mut self,
        call: &aevia_ir::Call,
        env: &Env,
    ) -> Option<(LReg, Kind)> {
        if self.is_self_call(call) {
            let mut args = self.self_args(call, env)?;
            args.push(self.fuel);
            let r = self
                .f
                .ins_typed(L::Call(lir::FuncId(1), args), lir_ty(self.result));
            return Some((r, self.result));
        }
        match (
            &call.callee,
            matches!(call.kind, CallKind::Normal),
            call.args.as_slice(),
        ) {
            | (Atom::Global(name), true, [arg]) => {
                let x = self.atom(arg, env)?;
                self.builtin(name, x)
            },
            | _ => None,
        }
    }

    fn rhs(
        &mut self,
        item: &aevia_ir::Let,
        env: &Env,
    ) -> Option<(LReg, Kind)> {
        match &item.rhs {
            | Rhs::Atom(atom) => self.atom(atom, env),
            | Rhs::NumericLiteral(_) => {
                match self.literal(item.span)? {
                    | Lit::F(bits) => Some(self.emit(L::F(f64::from_bits(bits)), Kind::Float)),
                    | Lit::I(x) => Some(self.emit(L::I(x), Kind::Int)),
                }
            },
            | Rhs::Unary { op, operand } => {
                let a = self.atom(operand, env)?;
                match (op, a.1) {
                    | (UnOp::Pos, Kind::Float | Kind::Int) => Some(a),
                    | (UnOp::Neg, Kind::Float) => Some(self.emit(L::FNeg(a.0), Kind::Float)),
                    | (UnOp::Neg, Kind::Int) => Some(self.emit(L::INeg(a.0), Kind::Int)),
                    | (UnOp::Not, Kind::Bool) => Some(self.emit(L::Not(a.0), Kind::Bool)),
                    | (UnOp::Sqrt, Kind::Float) => self.elementary("sqrt", a),
                    | _ => None,
                }
            },
            | Rhs::Binary { op, left, right } => {
                let a = self.atom(left, env)?;
                let b = self.atom(right, env)?;
                self.binary(*op, a, b)
            },
            | Rhs::AndOr { op, left, right } => {
                let a = self.atom(left, env)?;
                if a.1 != Kind::Bool {
                    return None;
                }
                // Short circuit with a real branch: the right operand runs
                // only when it decides the answer.
                let result = self.f.reg(Ty::Bool);
                let (eval, join) = (self.f.new_block(), self.f.new_block());
                let decided = self.f.ins(L::B(matches!(op, BinOp::Or)));
                self.f.assign(result, L::Copy(decided));
                match op {
                    | BinOp::And => self.f.branch(a.0, eval, &[], join, &[]),
                    | BinOp::Or => self.f.branch(a.0, join, &[], eval, &[]),
                    | _ => return None,
                }
                self.f.switch_to(eval);
                let value = self.value_block(right, env)?;
                if value.1 != Kind::Bool {
                    return None;
                }
                self.f.assign(result, L::Copy(value.0));
                self.f.jump(join, &[]);
                self.f.switch_to(join);
                Some((result, Kind::Bool))
            },
            | Rhs::Call(call) => self.call_value(call, env),
            | Rhs::Block(block) => self.value_block(block, env),
            | _ => None,
        }
    }

    fn statements(
        &mut self,
        block: &Block,
        env: &mut Env,
    ) -> Option<()> {
        for stmt in &block.stmts {
            match stmt {
                | Stmt::Let(item) => {
                    let value = self.rhs(item, env)?;
                    *env.get_mut(item.slot as usize)? = Some(value);
                },
                | Stmt::Bind { pattern, atom, .. } => {
                    match pattern {
                        | Pattern::Wildcard => {},
                        | Pattern::Bind { slot, inner: None } => {
                            let value = self.atom(atom, env)?;
                            *env.get_mut(*slot as usize)? = Some(value);
                        },
                        | _ => return None,
                    }
                },
            }
        }
        Some(())
    }

    /// Lowers a block in value position and returns its result register.
    fn value_block(
        &mut self,
        block: &Block,
        env: &Env,
    ) -> Option<(LReg, Kind)> {
        let join = self.f.new_block();
        self.assign.push(None);
        let lowered = self.block(block, &mut env.clone(), Cont::Assign { join });
        let result = self.assign.pop().flatten();
        lowered?;
        self.f.switch_to(join);
        result
    }

    /// Delivers a value to the continuation.
    fn deliver(
        &mut self,
        value: (LReg, Kind),
        cont: Cont,
    ) -> Option<()> {
        match cont {
            | Cont::Return => {
                if value.1 != self.result {
                    return None;
                }
                if self.entry {
                    let status = self.f.ins(L::I(0));
                    let mut out = vec![status, value.0];
                    for &kind in self.params {
                        let zero = match kind {
                            | Kind::Float => self.f.ins(L::F(0.0)),
                            | Kind::Int => self.f.ins(L::I(0)),
                            | Kind::Bool => self.f.ins(L::B(false)),
                        };
                        out.push(zero);
                    }
                    out.push(self.fuel);
                    self.f.ret(&out);
                } else {
                    self.f.ret(&[value.0]);
                }
            },
            | Cont::Assign { join } => {
                let slot = self.assign.last_mut()?;
                let target = match *slot {
                    | Some((reg, kind)) if kind == value.1 => reg,
                    | Some(_) => return None,
                    | None => {
                        let reg = self.f.reg(lir_ty(value.1));
                        *self.assign.last_mut()? = Some((reg, value.1));
                        reg
                    },
                };
                self.f.assign(target, L::Copy(value.0));
                self.f.jump(join, &[]);
            },
        }
        Some(())
    }

    fn block(
        &mut self,
        block: &Block,
        env: &mut Env,
        cont: Cont,
    ) -> Option<()> {
        self.statements(block, env)?;
        match &block.tail {
            | Tail::Return { atom, .. } => {
                let value = self.atom(atom, env)?;
                self.deliver(value, cont)
            },
            | Tail::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                let c = self.atom(cond, env)?;
                if c.1 != Kind::Bool {
                    return None;
                }
                let (t, e) = (self.f.new_block(), self.f.new_block());
                self.f.branch(c.0, t, &[], e, &[]);
                self.f.switch_to(t);
                self.block(then_block, &mut env.clone(), cont)?;
                self.f.switch_to(e);
                self.block(else_block, &mut env.clone(), cont)
            },
            | Tail::Match { scrutinee, arms, .. } => {
                let s = self.atom(scrutinee, env)?;
                for arm in arms {
                    let mut arm_env = env.clone();
                    // The test of the pattern.
                    let matched = match &arm.pattern {
                        | Pattern::Wildcard => None,
                        | Pattern::Bind { slot, inner: None } => {
                            *arm_env.get_mut(*slot as usize)? = Some(s);
                            None
                        },
                        | Pattern::Lit(PatLit::Int(n)) if s.1 == Kind::Int => {
                            let n = self.f.ins(L::I(i64::try_from(n).ok()?));
                            Some(self.f.ins(L::ICmp(Cmp::Eq, s.0, n)))
                        },
                        | Pattern::Lit(PatLit::Bool(b)) if s.1 == Kind::Bool => {
                            let b = self.f.ins(L::B(*b));
                            Some(self.bool_eq(s.0, b))
                        },
                        | _ => return None,
                    };
                    let (body, next) = (self.f.new_block(), self.f.new_block());
                    match matched {
                        | Some(flag) => self.f.branch(flag, body, &[], next, &[]),
                        | None => self.f.jump(body, &[]),
                    }
                    self.f.switch_to(body);
                    if let Some(guard) = &arm.guard {
                        let g = self.value_block(guard, &arm_env)?;
                        if g.1 != Kind::Bool {
                            return None;
                        }
                        let taken = self.f.new_block();
                        self.f.branch(g.0, taken, &[], next, &[]);
                        self.f.switch_to(taken);
                    }
                    self.block(&arm.body, &mut arm_env, cont)?;
                    self.f.switch_to(next);
                }
                // No arm matched: the interpreter reports it.
                self.f.bail(BAIL_INTERPRET);
                Some(())
            },
            | Tail::Call(call) => {
                if self.is_self_call(call) && matches!(cont, Cont::Return) {
                    let args = self.self_args(call, env)?;
                    let site = self.sites.len();
                    self.tails += 1;
                    // A tail jump costs one unit of fuel; with none left the
                    // interpreter continues the trampoline.
                    let zero = self.f.ins(L::I(0));
                    let empty = self.f.ins(L::ICmp(Cmp::Le, self.fuel, zero));
                    let (stop, go) = (self.f.new_block(), self.f.new_block());
                    self.f.branch(empty, stop, &[], go, &[]);
                    self.f.switch_to(stop);
                    if self.entry {
                        let status = self.f.ins(L::I(i64::try_from(site).ok()?));
                        let dummy = match self.result {
                            | Kind::Float => self.f.ins(L::F(0.0)),
                            | Kind::Int => self.f.ins(L::I(0)),
                            | Kind::Bool => self.f.ins(L::B(false)),
                        };
                        let mut out = vec![status, dummy];
                        out.extend_from_slice(&args);
                        out.push(self.fuel);
                        self.f.ret(&out);
                    } else {
                        self.f.bail(BAIL_INTERPRET);
                    }
                    self.f.switch_to(go);
                    let one = self.f.ins(L::I(1));
                    let left = self.f.ins(L::ISub(self.fuel, one));
                    let mut jump = args;
                    jump.push(left);
                    self.f.jump(self.head, &jump);
                    return Some(());
                }
                let value = self.call_value(call, env)?;
                self.deliver(value, cont)
            },
        }
    }
}

/// Lowers one form (entry or value) of an activation.
fn lower_form(
    function: &Function,
    literals: &[(Span, Lit)],
    builtin: BuiltinCheck<'_>,
    params: &[Kind],
    result: Kind,
    entry: bool,
) -> Option<(lir::Function, Vec<(Span, Span)>, usize, Vec<String>)> {
    let mut sig: Vec<Ty> = params.iter().map(|&k| lir_ty(k)).collect();
    sig.push(Ty::I64);
    let results: Vec<Ty> = if entry {
        let mut r = vec![Ty::I64, lir_ty(result)];
        r.extend(params.iter().map(|&k| lir_ty(k)));
        r.push(Ty::I64);
        r
    } else {
        vec![lir_ty(result)]
    };
    let mut f = FunctionBuilder::new(&function.name, &sig, &results);
    let head = f.new_block();
    let head_params: Vec<LReg> = params
        .iter()
        .map(|&k| f.block_param(head, lir_ty(k)))
        .collect();
    let fuel = f.block_param(head, Ty::I64);
    let args: Vec<LReg> = (0..=params.len()).map(|i| f.param(i)).collect();
    f.jump(head, &args);
    f.switch_to(head);
    let mut lower = Lower {
        f,
        function,
        literals,
        builtin,
        params,
        result,
        entry,
        head,
        head_params: head_params.clone(),
        fuel,
        sites: Vec::new(),
        assign: Vec::new(),
        tails: 0,
        used: Vec::new(),
    };
    let mut env: Env = vec![None; function.nslots as usize];
    for (index, pattern) in function.params.iter().enumerate() {
        let Pattern::Bind { slot, inner: None } = pattern else {
            return None;
        };
        *env.get_mut(*slot as usize)? = Some((*lower.head_params.get(index)?, *params.get(index)?));
    }
    lower.block(&function.body, &mut env, Cont::Return)?;
    let (sites, tails, used) = (lower.sites, lower.tails, lower.used);
    Some((lower.f.finish().ok()?, sites, tails, used))
}

/// Native code for one activation.
struct Native {
    module: aevia_rssn::jit::NativeModule,
    /// Self-call sites `(call span, callee span)`; tail status `k + 1`
    /// refers to site `k` of the entry form's list.
    sites: Vec<(Span, Span)>,
    /// Whether the function has a tail self call (a native loop).
    #[cfg_attr(not(test), allow(dead_code))]
    loops: bool,
    result: Kind,
    /// Builtins the code calls.
    used: Vec<String>,
}

fn compile_native(
    function: &Function,
    literals: &[(Span, Lit)],
    parameters: &[Kind],
    expected_result: Kind,
    builtin: BuiltinCheck<'_>,
) -> Option<Native> {
    if !function.jit || !function.captures.is_empty() || parameters.len() != function.params.len() {
        return None;
    }
    let (entry, sites, tails, used) = lower_form(
        function,
        literals,
        &mut *builtin,
        parameters,
        expected_result,
        true,
    )?;
    let (value, ..) = lower_form(
        function,
        literals,
        builtin,
        parameters,
        expected_result,
        false,
    )?;
    let loops = tails > 0;
    let module = lir::Module {
        functions: vec![entry, value],
    };
    let module = aevia_rssn::jit::NativeModule::compile(&module)?;
    Some(Native {
        module,
        sites,
        loops,
        result: expected_result,
        used,
    })
}

/// Whether `function` compiles natively for these kinds (without builtins).
#[cfg(test)]
fn compile(
    function: &Function,
    literals: &[(Span, Lit)],
    parameters: &[Kind],
    expected_result: Kind,
) -> Option<()> {
    compile_native(function, literals, parameters, expected_result, &mut |_| {
        false
    })
    .map(|_| ())
}

/// Straight-line SSA for a pure function (no tail loop): the instruction
/// sequence and its result operand. Shared by the scalar machine-code path and
/// by [`map_floats`].
fn lower_scalar(
    function: &Function,
    literals: &[(Span, u64)],
    parameters: &[Kind],
    expected_result: Kind,
) -> Option<(Vec<Inst>, Operand)> {
    if !function.jit || !function.captures.is_empty() || parameters.len() != function.params.len() {
        return None;
    }
    let mut builder = Builder {
        insts: Vec::new(),
        literals,
    };
    let mut slots = vec![None; function.nslots as usize];
    for (index, pattern) in function.params.iter().enumerate() {
        let Pattern::Bind { slot, inner: None } = pattern else {
            return None;
        };
        let value = builder.emit(Inst::Input(u32::try_from(index).ok()?), parameters[index])?;
        *slots.get_mut(*slot as usize)? = Some(value);
    }
    let result = builder.block(&function.body, &mut slots)?;
    if result.kind != expected_result {
        return None;
    }
    Some((builder.insts, result))
}

/// Applies a pure `Float -> ... -> Float` function to whole columns on an RSSN
/// device (`spec/10-backend.md` §10.6.5): the batch seam behind
/// `Device.cpu_parallel`.
///
/// `columns[i][row]` is parameter `i` of row `row`. Returns `None` when the
/// function does not lower (the same conditions as `@jit`: no captures, only
/// pure `Float` arithmetic, comparisons and branches) or the device fails; the
/// caller then maps the interpreter over the rows, which gives the same
/// values on a CPU device. A device with `f32` arithmetic returns rounded
/// values, so a caller that promises bit-identical results must restrict
/// itself to the CPU devices.
///
/// No builtin calls this yet: registering `Device.map` needs entries in the
/// type and builtin tables, which the stdlib generator owns (see the spec).
#[allow(dead_code)]
pub(crate) fn map_floats(
    function: &Function,
    literals: &[(Span, u64)],
    columns: &[&[f64]],
    device: &dyn aevia_rssn::device::Device,
) -> Option<Vec<f64>> {
    let kinds = vec![Kind::Float; function.params.len()];
    let (insts, result) = lower_scalar(function, literals, &kinds, Kind::Float)?;
    let program = aevia_rssn::device::BatchProgram::new(insts, kinds.len(), vec![result.reg])?;
    program.run(device, columns)?.into_iter().next()
}

// The weak reference guards against pointer reuse. All live and dead entries
// count toward the hard cap; machine code is never shared across threads.
type Entry = (Weak<Function>, Option<Rc<Native>>);
type CacheKey = (usize, Vec<Kind>, Kind, Vec<(Span, Lit)>);
thread_local! { static CACHE: RefCell<HashMap<CacheKey, Entry>> = RefCell::new(HashMap::new()); }
#[cfg(test)]
thread_local! { static NATIVE_LOOPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
thread_local! { static NATIVE_SCALARS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

/// Resolve numeric literals under the actual activation before consulting the
/// machine-code cache. Failure is a normal interpreter fallback and never
/// inserts a negative cache entry for a different specialization.
pub(crate) fn specialization<T: Into<Lit>>(
    function: &Function,
    mut resolve: impl FnMut(&aevia_syntax::ast::NumberLit, Span) -> Option<T>,
) -> Option<Vec<(Span, Lit)>> {
    if !function.jit || !function.captures.is_empty() {
        return None;
    }
    fn visit<T: Into<Lit>>(
        block: &Block,
        out: &mut Vec<(Span, Lit)>,
        resolve: &mut impl FnMut(&aevia_syntax::ast::NumberLit, Span) -> Option<T>,
    ) -> Option<()> {
        for stmt in &block.stmts {
            if let Stmt::Let(item) = stmt {
                match &item.rhs {
                    | Rhs::NumericLiteral(number) => {
                        out.push((item.span, resolve(number, item.span)?.into()));
                    },
                    | Rhs::Block(inner) | Rhs::AndOr { right: inner, .. } => {
                        visit(inner, out, resolve)?;
                    },
                    | _ => {},
                }
            }
        }
        match &block.tail {
            | Tail::If {
                then_block,
                else_block,
                ..
            } => {
                visit(then_block, out, resolve)?;
                visit(else_block, out, resolve)?;
            },
            | Tail::Match { arms, .. } => {
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        visit(guard, out, resolve)?;
                    }
                    visit(&arm.body, out, resolve)?;
                }
            },
            | _ => {},
        }
        Some(())
    }
    let mut literals = Vec::new();
    visit(&function.body, &mut literals, &mut resolve)?;
    Some(literals)
}

pub(crate) enum Outcome {
    Value(Value),
    Tail {
        args: Vec<Value>,
        span: Span,
        consumed: usize,
    },
}

/// [`run_with`] without access to builtins (a call of `sqrt` and the like
/// then keeps the function in the interpreter).
#[cfg(test)]
pub(crate) fn run(
    function: &Rc<Function>,
    args: &[Value],
    fuel: usize,
    literals: &[(Span, Lit)],
    expected_result: Kind,
    mut is_self: impl FnMut(Span) -> bool,
) -> Option<Outcome> {
    run_with(function, args, fuel, literals, expected_result, |ask| {
        match ask {
            | Ask::IsSelf(span) => is_self(span),
            | Ask::Builtin(_) => false,
        }
    })
}

/// Runs an activation natively when it compiles; `None` sends it to the
/// interpreter (it does not compile, an argument is not a plain number, a
/// self call no longer refers to this activation, or the code bailed).
///
/// `ask` answers the interpreter-side questions of [`Ask`].
pub(crate) fn run_with(
    function: &Rc<Function>,
    args: &[Value],
    fuel: usize,
    literals: &[(Span, Lit)],
    expected_result: Kind,
    mut ask: impl FnMut(Ask<'_>) -> bool,
) -> Option<Outcome> {
    use aevia_rssn::jit::lir::Value as V;
    if !function.jit || args.len() != function.params.len() {
        return None;
    }
    let plain = |num: &NumVal| num.dim == Dim::one() && num.display.is_none();
    let values: Option<Vec<(Kind, V<'static>)>> = args
        .iter()
        .map(|value| {
            match value {
                | Value::Num(
                    num @ NumVal {
                        kind: NumKind::Float(x),
                        ..
                    },
                ) if plain(num) => Some((Kind::Float, V::F(*x))),
                | Value::Num(
                    num @ NumVal {
                        kind: NumKind::Int(x),
                        ..
                    },
                ) if plain(num) => Some((Kind::Int, V::I(i64::try_from(x).ok()?))),
                | Value::Bool(value) => Some((Kind::Bool, V::B(*value))),
                | _ => None,
            }
        })
        .collect();
    let values = values?;
    let kinds: Vec<Kind> = values.iter().map(|(kind, _)| *kind).collect();
    let mut call_args: Vec<V<'static>> = values.into_iter().map(|(_, v)| v).collect();
    let limit = i64::try_from(fuel).unwrap_or(i64::MAX);
    call_args.push(V::I(limit));
    let native = CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let key = (
            Rc::as_ptr(function) as usize,
            kinds.clone(),
            expected_result,
            literals.to_vec(),
        );
        if cache.len() >= 256 {
            cache.retain(|_, (function, _)| function.strong_count() > 0);
        }
        if !cache.contains_key(&key) && cache.len() >= 256 {
            if let Some(evicted) = cache.keys().next().cloned() {
                cache.remove(&evicted);
            }
        }
        let mut check = |name: &str| ask(Ask::Builtin(name));
        let mut compile =
            || compile_native(function, literals, &kinds, expected_result, &mut check).map(Rc::new);
        let entry = cache
            .entry(key)
            .or_insert_with(|| (Rc::downgrade(function), compile()));
        if !entry.0.ptr_eq(&Rc::downgrade(function)) {
            *entry = (Rc::downgrade(function), compile());
        }
        entry.1.clone()
    })?;
    // Pure native code cannot rebind a name, so one check per self-call site
    // before entering is enough even in a mutable REPL scope.
    if !native
        .sites
        .iter()
        .all(|&(_, callee)| ask(Ask::IsSelf(callee)))
        || !native.used.iter().all(|name| ask(Ask::Builtin(name)))
    {
        return None;
    }
    let outcome = native.module.call(lir::FuncId(0), &mut call_args)?;
    let lir::Outcome::Returned(out) = outcome else {
        return None;
    };
    let to_value = |v: &V<'_>, kind: Kind| -> Option<Value> {
        Some(match (v, kind) {
            | (V::F(x), Kind::Float) => Value::float(*x),
            | (V::I(x), Kind::Int) => Value::int(*x),
            | (V::B(x), Kind::Bool) => Value::Bool(*x),
            | _ => return None,
        })
    };
    let V::I(status) = out.first()? else {
        return None;
    };
    #[cfg(test)]
    {
        let counter = if native.loops {
            &NATIVE_LOOPS
        } else {
            &NATIVE_SCALARS
        };
        counter.with(|count| count.set(count.get() + 1));
    }
    if *status == 0 {
        return Some(Outcome::Value(to_value(out.get(1)?, native.result)?));
    }
    let site = usize::try_from(*status).ok()?.checked_sub(1)?;
    let (span, _) = *native.sites.get(site)?;
    let V::I(left) = out.last()? else {
        return None;
    };
    let consumed = usize::try_from(limit.checked_sub(*left)?).ok()?;
    let next: Option<Vec<Value>> = out
        .get(2..2 + kinds.len())?
        .iter()
        .zip(&kinds)
        .map(|(v, &k)| to_value(v, k))
        .collect();
    Some(Outcome::Tail {
        args: next?,
        span,
        consumed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile as compile_source;
    use aevia_ir::lower::lower_module_with_literals;

    fn function(source: &str) -> Rc<Function> {
        let compilation = compile_source("jit-unit.aev", source);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let checked = compilation.checked;
        let module = lower_module_with_literals(
            compilation.module.as_ref().unwrap(),
            &checked.rssn_calls,
            &checked.dictionaries,
            &checked.literal_reps,
        );
        module.fns[0].function.clone()
    }

    fn dynamic_literal_function() -> Rc<Function> {
        let span = Span::default();
        Rc::new(Function {
            name: Rc::from("dynamic_literal"),
            jit: true,
            params: vec![Pattern::Bind { slot: 0, inner: None }],
            body: Rc::new(Block {
                stmts: vec![
                    Stmt::Let(aevia_ir::Let {
                        slot: 1,
                        rhs: Rhs::NumericLiteral(aevia_syntax::ast::NumberLit::Int(1.into())),
                        span,
                    }),
                    Stmt::Let(aevia_ir::Let {
                        slot: 2,
                        rhs: Rhs::Binary {
                            op: BinOp::Add,
                            left: Atom::Local(0),
                            right: Atom::Local(1),
                        },
                        span,
                    }),
                ],
                tail: Tail::Return {
                    atom: Atom::Local(2),
                    span,
                },
            }),
            captures: Vec::new(),
            nslots: 3,
            span,
        })
    }

    #[test]
    fn map_floats_runs_a_branching_function_on_the_cpu_devices() {
        let f = function(
            "@jit
fn f(x: Float): Float = if x > 0.0 then x * 2.0 else 0.0 - x
",
        );
        let xs: Vec<f64> = (0..5000).map(|i| f64::from(i) * 0.01 - 20.0).collect();
        let none: &[(Span, u64)] = &[];
        for selector in [
            aevia_rssn::device::Selector::Cpu,
            aevia_rssn::device::Selector::CpuParallel,
        ] {
            let device = aevia_rssn::device::resolve(selector).unwrap();
            let out = map_floats(&f, none, &[&xs], &*device).unwrap();
            for (x, y) in xs.iter().zip(&out) {
                let want = if *x > 0.0 { x * 2.0 } else { 0.0 - x };
                assert_eq!(y.to_bits(), want.to_bits());
            }
        }
        // Wrong arity of columns: refused, caller falls back to the interpreter.
        let device = aevia_rssn::device::resolve(aevia_rssn::device::Selector::Cpu).unwrap();
        assert!(map_floats(&f, none, &[], &*device).is_none());
    }

    #[test]
    fn dynamic_float_literals_compile_and_cache_by_exact_bits() {
        CACHE.with(|cache| cache.borrow_mut().clear());
        let function = dynamic_literal_function();
        assert!(specialization(&function, |_, _| None::<f64>).is_none());
        assert_eq!(CACHE.with(|cache| cache.borrow().len()), 0);
        for (literal, expected) in [(1.0_f64, 3.0_f64), (2.0, 4.0), (-0.0, 2.0), (0.0, 2.0)] {
            let constants = specialization(&function, |_, _| Some(literal)).unwrap();
            let Some(Outcome::Value(Value::Num(result))) = run(
                &function,
                &[Value::float(2.0)],
                0,
                &constants,
                Kind::Float,
                |_| true,
            ) else {
                panic!("Float specialization must execute machine code")
            };
            let NumKind::Float(result) = result.kind else {
                panic!("expected Float")
            };
            assert_eq!(result.to_bits(), expected.to_bits());
        }
        assert_eq!(CACHE.with(|cache| cache.borrow().len()), 4);
    }

    #[test]
    fn specialized_constant_preserves_negative_zero_bits() {
        let span = Span::default();
        let function = Rc::new(Function {
            name: Rc::from("signed_zero"),
            jit: true,
            params: vec![],
            body: Rc::new(Block {
                stmts: vec![Stmt::Let(aevia_ir::Let {
                    slot: 0,
                    rhs: Rhs::NumericLiteral(aevia_syntax::ast::NumberLit::Decimal("0.0".into())),
                    span,
                })],
                tail: Tail::Return {
                    atom: Atom::Local(0),
                    span,
                },
            }),
            captures: vec![],
            nslots: 1,
            span,
        });
        let constants = specialization(&function, |_, _| Some(-0.0)).unwrap();
        let Some(Outcome::Value(Value::Num(value))) =
            run(&function, &[], 0, &constants, Kind::Float, |_| true)
        else {
            panic!("expected machine-code constant")
        };
        let NumKind::Float(value) = value.kind else {
            panic!("expected Float")
        };
        assert_eq!(value.to_bits(), (-0.0_f64).to_bits());
    }

    #[test]
    fn cache_is_bounded_even_when_all_functions_are_live() {
        CACHE.with(|cache| cache.borrow_mut().clear());
        let functions: Vec<_> = (0..270).map(|_| dynamic_literal_function()).collect();
        for function in &functions {
            // An unsupported literal-free specialization still occupies a
            // negative entry, and must not bypass the live-entry limit.
            assert!(
                run(function, &[Value::float(1.0)], 0, &[], Kind::Float, |_| {
                    true
                })
                .is_none()
            );
        }
        assert_eq!(CACHE.with(|cache| cache.borrow().len()), 256);
    }

    #[test]
    fn source_generic_float_is_compiled_after_exact_calls_without_cache_poisoning() {
        CACHE.with(|cache| cache.borrow_mut().clear());
        let source = "@jit\nfn number<r: Rep>(): r = 1\nlet integer = number<Int>()\nlet real = number<Real>()\nlet float = number<Float>()\n";
        let compilation = compile_source("generic-jit.aev", source);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let mut interp = crate::Interp::new(&compilation.checked).with_mode(crate::Mode::Ir);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
        assert!(matches!(
            interp
                .eval_toplevel("integer")
                .unwrap()
                .as_num()
                .unwrap()
                .kind,
            NumKind::Int(_)
        ));
        assert!(matches!(
            interp.eval_toplevel("real").unwrap().as_num().unwrap().kind,
            NumKind::Exact(_)
        ));
        assert!(
            matches!(interp.eval_toplevel("float").unwrap().as_num().unwrap().kind, NumKind::Float(value) if value == 1.0)
        );
        CACHE.with(|cache| {
            let cache = cache.borrow();
            // The `Int` activation is native too (checked 64-bit arithmetic);
            // the exact `Real` one must stay out of the cache.
            assert_eq!(
                cache.len(),
                2,
                "exact calls must not enter the native cache"
            );
            assert!(
                cache.values().any(|(_, compiled)| compiled.is_some()),
                "Float activation must compile machine code"
            );
            assert!(cache.keys().any(|(_, _, _, literals)| {
                literals
                    .iter()
                    .any(|(_, bits)| *bits == Lit::F(1.0_f64.to_bits()))
            }));
        });
    }

    #[test]
    fn source_generic_self_tail_uses_native_loop_only_with_matching_proof() {
        CACHE.with(|cache| cache.borrow_mut().clear());
        NATIVE_LOOPS.with(|count| count.set(0));
        let source = "@jit\nfn sum<r: Rep>(n: Float, acc: r): r = if n <= 0.0 then acc else sum(n - 1.0, acc + 1)\nlet result = sum<Float>(10.0, 0.0)\n";
        let compilation = compile_source("generic-tail-jit.aev", source);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let mut interp = crate::Interp::new(&compilation.checked).with_mode(crate::Mode::Ir);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
        assert_eq!(interp.eval_toplevel("result").unwrap().as_f64(), Some(10.0));
        assert!(
            NATIVE_LOOPS.with(|count| count.get()) > 0,
            "generic same-proof tail recursion must execute native loop"
        );
    }

    #[test]
    fn generic_bool_and_float_activations_compile_separately() {
        CACHE.with(|cache| cache.borrow_mut().clear());
        NATIVE_SCALARS.with(|count| count.set(0));
        let source = "@jit\nfn identity<a>(x: a): a = x\nlet integer = identity<Int>(1)\nlet boolean = identity<Bool>(true)\nlet floating = identity<Float>(2.0)\n";
        let compilation = compile_source("bool-jit-cache.aev", source);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let mut interp = crate::Interp::new(&compilation.checked).with_mode(crate::Mode::Ir);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
        assert!(matches!(
            interp.eval_toplevel("boolean").unwrap(),
            Value::Bool(true)
        ));
        assert_eq!(
            interp.eval_toplevel("floating").unwrap().as_f64(),
            Some(2.0)
        );
        // `Int`, `Bool` and `Float` activations each run natively.
        assert_eq!(NATIVE_SCALARS.with(|count| count.get()), 3);
        CACHE.with(|cache| {
            let cache = cache.borrow();
            assert!(
                cache
                    .keys()
                    .any(|(_, inputs, result, _)| inputs == &[Kind::Bool] && *result == Kind::Bool)
            );
            assert!(cache.keys().any(|(_, inputs, result, _)| {
                inputs == &[Kind::Float] && *result == Kind::Float
            }));
        });
    }

    #[test]
    fn boolean_tail_state_and_result_execute_natively() {
        CACHE.with(|cache| cache.borrow_mut().clear());
        NATIVE_LOOPS.with(|count| count.set(0));
        let source = "@jit\nfn flip(n: Float, flag: Bool): Bool = if n <= 0.0 then flag else flip(n - 1.0, !flag)\nlet result = flip(3.0, false)\n";
        let compilation = compile_source("bool-jit-loop.aev", source);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let mut interp = crate::Interp::new(&compilation.checked).with_mode(crate::Mode::Ir);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
        assert!(matches!(
            interp.eval_toplevel("result").unwrap(),
            Value::Bool(true)
        ));
        assert!(NATIVE_LOOPS.with(|count| count.get()) > 0);
    }

    #[test]
    fn boolean_short_circuit_compiles_only_a_pure_rhs() {
        let pure = function("@jit\nfn both(a: Bool, b: Bool): Bool = a && !b\n");
        let compiled = compile(&pure, &[], &[Kind::Bool, Kind::Bool], Kind::Bool);
        assert!(compiled.is_some(), "pure boolean rhs should compile");
        let Some(Outcome::Value(Value::Bool(value))) = run(
            &pure,
            &[Value::Bool(true), Value::Bool(false)],
            0,
            &[],
            Kind::Bool,
            |_| true,
        ) else {
            panic!("expected Bool")
        };
        assert!(value);
        let effectful =
            function("@jit\nfn both(a: Bool): Bool = a && effect()\nfn effect(): Bool = true\n");
        assert!(compile(&effectful, &[], &[Kind::Bool], Kind::Bool).is_none());
    }

    #[test]
    fn boolean_tail_resume_preserves_argument_kinds_and_values() {
        let bool_function = function(
            "@jit\nfn flip(n: Float, flag: Bool): Bool = if n <= 0.0 then flag else flip(n - 1.0, !flag)\n",
        );
        let Some(Outcome::Tail { args, consumed, .. }) = run(
            &bool_function,
            &[Value::float(3.0), Value::Bool(false)],
            2,
            &[],
            Kind::Bool,
            |_| true,
        ) else {
            panic!("expected fuel-limited native tail step")
        };
        assert_eq!(consumed, 2);
        // The final body is evaluated after the two permitted backedges;
        // the returned arguments belong to its pending third tail dispatch.
        assert_eq!(args[0].as_f64(), Some(0.0));
        assert!(matches!(args[1], Value::Bool(true)));
        let Some(Outcome::Value(Value::Bool(value))) =
            run(&bool_function, &args, 0, &[], Kind::Bool, |_| true)
        else {
            panic!("expected resumed boolean result")
        };
        assert!(value);

        // The established Float loop uses exactly the same pending-dispatch
        // protocol and exposes the same state at the same budget boundary.
        let float_function = function(
            "@jit\nfn flip_float(n: Float, flag: Float): Float = if n <= 0.0 then flag else flip_float(n - 1.0, 1.0 - flag)\n",
        );
        let Some(Outcome::Tail { args, consumed, .. }) = run(
            &float_function,
            &[Value::float(3.0), Value::float(0.0)],
            2,
            &[],
            Kind::Float,
            |_| true,
        ) else {
            panic!("expected fuel-limited Float tail step")
        };
        assert_eq!(consumed, 2);
        assert_eq!(args[0].as_f64(), Some(0.0));
        assert_eq!(args[1].as_f64(), Some(1.0));
        let Some(Outcome::Value(value)) =
            run(&float_function, &args, 0, &[], Kind::Float, |_| true)
        else {
            panic!("expected resumed Float result")
        };
        assert_eq!(value.as_f64(), Some(1.0));
    }

    #[test]
    fn mismatched_host_argument_never_enters_a_native_boolean_specialization() {
        CACHE.with(|cache| cache.borrow_mut().clear());
        NATIVE_SCALARS.with(|count| count.set(0));
        let compilation = compile_source(
            "bool-jit-forged.aev",
            "@jit\nfn identity(flag: Bool): Bool = flag\n",
        );
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let mut interp = crate::Interp::new(&compilation.checked).with_mode(crate::Mode::Ir);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
        let function = interp.eval_toplevel("identity").unwrap();
        let _ = interp.apply(function, vec![Value::float(1.0)], Span::default());
        assert_eq!(NATIVE_SCALARS.with(|count| count.get()), 0);
        assert!(CACHE.with(|cache| cache.borrow().is_empty()));
    }

    #[test]
    fn rebound_specialized_clone_of_same_origin_stays_on_trampoline() {
        CACHE.with(|cache| cache.borrow_mut().clear());
        NATIVE_LOOPS.with(|count| count.set(0));
        let source = "@jit\nfn sum<r: Rep>(n: Float, acc: r): r = if n <= 0.0 then acc else sum(n - 1.0, acc + 1)\nlet original = sum<Float>\nlet sum = original\nlet result = original(3.0, 0.0)\n";
        let compilation = compile_source("generic-rebound-jit.aev", source);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let mut interp = crate::Interp::new(&compilation.checked).with_mode(crate::Mode::Ir);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
        assert_eq!(interp.eval_toplevel("result").unwrap().as_f64(), Some(3.0));
        assert_eq!(
            NATIVE_LOOPS.with(|count| count.get()),
            0,
            "a rebound selected clone must not take the native self loop"
        );
    }

    #[test]
    fn invalid_tape_cannot_reach_codegen() {
        assert!(Program::compile(vec![Inst::Input(1)], 1, vec![0]).is_none());
        assert!(Program::compile(vec![Inst::Neg(0)], 0, vec![0]).is_none());
        assert!(Program::compile(vec![Inst::Input(268_435_456)], 268_435_457, vec![0]).is_none());
        assert!(Program::compile(vec![Inst::Const(1.0)], 0, vec![1]).is_none());
        let program = Program::compile(vec![Inst::Input(0)], 1, vec![0]).unwrap();
        assert!(program.call(&[]).is_none());
        assert!(program.call(&[1.0, 2.0]).is_none());
        assert_eq!(program.call(&[2.0]), Some(vec![2.0]));
    }

    #[test]
    fn native_loop_preserves_fuel_bits_and_parameter_aliasing() {
        let swap = LoopProgram::compile(
            vec![
                Inst::Input(0),
                Inst::Input(1),
                Inst::Const(1.0),
                Inst::Const(0.0),
            ],
            2,
            vec![2, 3, 1, 0],
        )
        .unwrap();
        let first = swap.run(&[1.0, 2.0], 0).unwrap();
        assert_eq!(first.outputs, vec![1.0, 0.0, 2.0, 1.0]);
        assert_eq!(first.remaining, 0);
        let second = swap.run(&[1.0, 2.0], 1).unwrap();
        assert_eq!(second.outputs, vec![1.0, 0.0, 1.0, 2.0]);
        let third = swap.run(&[1.0, 2.0], 2).unwrap();
        assert_eq!(third.outputs, vec![1.0, 0.0, 2.0, 1.0]);
        let done =
            LoopProgram::compile(vec![Inst::Const(0.0), Inst::Const(7.0)], 0, vec![0, 1]).unwrap();
        let big_fuel = (1u64 << 53) + 1;
        let result = done.run(&[], big_fuel).unwrap();
        assert_eq!(result.outputs, vec![0.0, 7.0]);
        assert_eq!(result.remaining, big_fuel);
        assert_eq!(done.run(&[], u64::MAX).unwrap().remaining, u64::MAX);
        assert!(done.run(&[1.0], 0).is_none());
        let nan_flag =
            LoopProgram::compile(vec![Inst::Const(f64::NAN), Inst::Const(7.0)], 0, vec![0, 1])
                .unwrap();
        let result = nan_flag.run(&[], 1).unwrap();
        assert!(result.outputs[0].is_nan());
        assert_eq!(result.remaining, 0);
    }

    #[test]
    fn scalar_branches_really_compile_and_execute() {
        let function =
            function("@jit\nfn f(x: Float): Float = if x > 0.0 then x * 2.0 else 0.0 - x\n");
        compile(&function, &[], &[Kind::Float], Kind::Float)
            .expect("scalar branch compiles to machine code");
        for (x, want) in [(3.0, 6.0), (-3.0, 3.0)] {
            let Some(Outcome::Value(Value::Num(NumVal {
                kind: NumKind::Float(got),
                ..
            }))) = run(&function, &[Value::float(x)], 0, &[], Kind::Float, |_| true)
            else {
                panic!("expected a native Float")
            };
            assert_eq!(got.to_bits(), f64::to_bits(want));
        }
        assert!(
            run(&function, &[Value::float(3.0)], 0, &[], Kind::Float, |_| {
                true
            })
            .is_some()
        );
    }

    #[test]
    fn tail_recursive_step_really_compiles() {
        let function = function(
            "@jit\nfn sum(n: Float, acc: Float): Float = if n <= 0.0 then acc else sum(n - 1.0, acc + n)\n",
        );
        compile(&function, &[], &[Kind::Float, Kind::Float], Kind::Float)
            .expect("tail loop compiles to machine code");
        // Fuel 2: two jumps, then the third tail call is handed back.
        let Some(Outcome::Tail { args, .. }) = run(
            &function,
            &[Value::float(3.0), Value::float(0.0)],
            2,
            &[],
            Kind::Float,
            |_| true,
        ) else {
            panic!("expected a pending tail call")
        };
        assert_eq!(
            format!("{args:?}"),
            format!("{:?}", vec![Value::float(0.0), Value::float(6.0)])
        );
        let done = run(
            &function,
            &[Value::float(3.0), Value::float(0.0)],
            3,
            &[],
            Kind::Float,
            |_| true,
        );
        assert!(
            matches!(done, Some(Outcome::Value(v)) if format!("{v:?}") == format!("{:?}", Value::float(6.0)))
        );
        assert!(matches!(
            run(
                &function,
                &[Value::float(3.0), Value::float(0.0)],
                2,
                &[],
                Kind::Float,
                |_| true
            ),
            Some(Outcome::Tail { consumed: 2, .. })
        ));
        assert!(
            run(
                &function,
                &[Value::float(3.0), Value::float(0.0)],
                2,
                &[],
                Kind::Float,
                |_| false
            )
            .is_none()
        );
    }

    #[test]
    fn calls_are_never_speculated_in_branches() {
        let function = function("@jit\nfn f(x: Float): Float = if x > 0.0 then x else sqrt(x)\n");
        assert!(compile(&function, &[], &[Kind::Float], Kind::Float).is_none());
    }

    #[test]
    fn explicitly_specialized_tail_calls_require_the_runtime_proof_guard() {
        let function = function(
            "@jit\nfn f<r: Rep>(n: Float, x: r): r = if n <= 0.0 then x else f<r>(n - 1.0, x)\n",
        );
        assert!(
            compile(&function, &[], &[Kind::Float, Kind::Float], Kind::Float).is_some(),
            "a selected self callee can be lowered without becoming a numeric operand: {function:#?}"
        );
        assert!(
            run(
                &function,
                &[Value::float(3.0), Value::float(2.0)],
                10,
                &[],
                Kind::Float,
                |_| false
            )
            .is_none()
        );
    }

    #[test]
    fn explicit_generic_self_tail_executes_only_with_the_same_selected_proof() {
        CACHE.with(|cache| cache.borrow_mut().clear());
        NATIVE_LOOPS.with(|count| count.set(0));
        let compilation = compile_source(
            "typed-tail.aev",
            "@jit\nfn sum<r: Rep>(n: Float, acc: r): r = if n <= 0.0 then acc else sum<r>(n - 1.0, acc + 1)\nlet result = sum<Float>(10.0, 0.0)\n",
        );
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let mut interp = crate::Interp::new(&compilation.checked).with_mode(crate::Mode::Ir);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
        assert_eq!(interp.eval_toplevel("result").unwrap().as_f64(), Some(10.0));
        assert!(NATIVE_LOOPS.with(|count| count.get()) > 0);
    }

    #[test]
    fn explicit_tail_cannot_change_a_phantom_binding_inside_native_code() {
        CACHE.with(|cache| cache.borrow_mut().clear());
        NATIVE_LOOPS.with(|count| count.set(0));
        let source = "@jit\nfn f<a>(n: Float): Float = if n <= 0.0 then n else f<a>(n - 1.0)\nlet selected = f<Int>\nlet other = f<Bool>\n";
        let compilation = compile_source("phantom-tail.aev", source);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let selection_span = |text: &str| {
            *compilation
                .checked
                .callable_refs
                .keys()
                .find(|span| source.get(span.start as usize..span.end as usize) == Some(text))
                .expect("checked callable selection")
        };
        let same = selection_span("f<Int>");
        let changed = selection_span("f<Bool>");
        let mut interp = crate::Interp::new(&compilation.checked).with_mode(crate::Mode::Ir);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
        let selected = interp.eval_toplevel("selected").unwrap();
        let target = interp.eval_toplevel("f").unwrap();
        let Value::Closure(current) = &selected else {
            panic!("selected closure")
        };
        let mut args = vec![Value::float(3.0)];
        let proof = interp
            .prepare_function_call(&selected, &mut args, None)
            .unwrap()
            .unwrap();
        assert!(interp.self_specialization_matches(current, Some(&proof), &target, same, &args));
        assert!(
            !interp.self_specialization_matches(current, Some(&proof), &target, changed, &args),
            "equal Float signatures cannot authenticate a different phantom selection"
        );
        let crate::value::ClosureBody::Ir(function) = &current.body else {
            panic!("IR closure")
        };
        assert!(
            run(function, &args, 10, &[], Kind::Float, |_| {
                interp.self_specialization_matches(current, Some(&proof), &target, changed, &args)
            })
            .is_none()
        );
        assert_eq!(NATIVE_LOOPS.with(|count| count.get()), 0);
    }

    #[test]
    fn integers_recurse_natively_and_overflow_falls_back() {
        let function =
            function("@jit\nfn fact(n: Int): Int = if n <= 1 then 1 else n * fact(n - 1)\n");
        let literals = specialization(&function, |number, _| {
            match number {
                | aevia_syntax::ast::NumberLit::Int(v) => i64::try_from(v).ok().map(Lit::I),
                | aevia_syntax::ast::NumberLit::Decimal(_) => None,
            }
        })
        .unwrap();
        let fact = |n: i64| {
            run(
                &function,
                &[Value::int(n)],
                1000,
                &literals,
                Kind::Int,
                |_| true,
            )
        };
        assert!(
            matches!(fact(20), Some(Outcome::Value(v)) if format!("{v:?}") == format!("{:?}", Value::int(2_432_902_008_176_640_000)))
        );
        // 21! does not fit 64 bits: native code bails, the interpreter takes over.
        assert!(fact(21).is_none());
    }

    #[test]
    fn match_on_integers_and_value_position_recursion() {
        let function = function(
            "@jit
fn fib(n: Int): Int = match n with
  | 0 -> 0
  | 1 -> 1
  | _ -> fib(n - 1) + fib(n - 2)
",
        );
        let literals = specialization(&function, |number, _| {
            match number {
                | aevia_syntax::ast::NumberLit::Int(v) => i64::try_from(v).ok().map(Lit::I),
                | aevia_syntax::ast::NumberLit::Decimal(_) => None,
            }
        })
        .unwrap();
        let got = run(
            &function,
            &[Value::int(25)],
            10,
            &literals,
            Kind::Int,
            |_| true,
        );
        assert!(
            matches!(got, Some(Outcome::Value(v)) if format!("{v:?}") == format!("{:?}", Value::int(75_025)))
        );
    }

    #[test]
    fn builtins_are_guarded_and_must_not_be_shadowed() {
        let function =
            function("@jit\nfn f(x: Float): Float = if x > 0.0 then sqrt(x) + ln(x) else exp(x)\n");
        let ask = |builtins: bool| {
            move |ask: Ask<'_>| {
                match ask {
                    | Ask::IsSelf(_) => true,
                    | Ask::Builtin(_) => builtins,
                }
            }
        };
        let f = |x: f64, builtins: bool| {
            run_with(
                &function,
                &[Value::float(x)],
                0,
                &[],
                Kind::Float,
                ask(builtins),
            )
        };
        let want = 4.0_f64.powf(0.5) + 4.0_f64.ln();
        assert!(
            matches!(f(4.0, true), Some(Outcome::Value(v)) if format!("{v:?}") == format!("{:?}", Value::float(want)))
        );
        assert!(
            matches!(f(-1.0, true), Some(Outcome::Value(v)) if format!("{v:?}") == format!("{:?}", Value::float((-1.0_f64).exp())))
        );
        // `sqrt` of +oo is fine, `ln(+oo)` is the interpreter's exact answer.
        assert!(f(f64::INFINITY, true).is_none());
        // A shadowed builtin keeps the function in the interpreter.
        CACHE.with(|cache| cache.borrow_mut().clear());
        assert!(f(4.0, false).is_none());
    }
}
