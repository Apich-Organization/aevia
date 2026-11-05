//! Conservative lowering of pure scalar Core IR to RSSN's machine-code tape.
//!
//! The current backend has straight-line SSA plus select, but no jumps or
//! function calls. Pure branches can be selected eagerly, and a single
//! self-tail-call site becomes a machine-code loop with a bounded back edge.
//! Other calls, effects, captures, exact numbers, units, and tensors fall back.
use crate::value::{NumKind, NumVal, Value};
use aevia_dim::Dim;
use aevia_ir::{Atom, Block, CallKind, Function, Pattern, Rhs, Stmt, Tail};
use aevia_rssn::jit::{Cmp, Inst, LoopProgram, Program, Reg};
use aevia_syntax::Span;
use aevia_syntax::ast::{BinOp, UnOp};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Float,
    Bool,
}
#[derive(Clone, Copy)]
struct Operand {
    reg: Reg,
    kind: Kind,
}
struct Builder {
    insts: Vec<Inst>,
}

impl Builder {
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
                        | Rhs::Unary { op, operand } => {
                            let a = self.atom(operand, slots)?;
                            if a.kind != Kind::Float {
                                return None;
                            }
                            match op {
                                | UnOp::Pos => a,
                                | UnOp::Neg => self.emit(Inst::Neg(a.reg), Kind::Float)?,
                                | _ => return None,
                            }
                        },
                        | Rhs::Binary { op, left, right } => {
                            let a = self.atom(left, slots)?;
                            let b = self.atom(right, slots)?;
                            if a.kind != Kind::Float || b.kind != Kind::Float {
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

    // A tail-recursive iteration is compiled into a multi-output step. The
    // existing interpreter trampoline performs the jump and enforces fuel.
    // Each branch remains pure Float arithmetic; the unselected branch can
    // therefore be evaluated without changing observable behaviour.
    fn step_block(
        &mut self,
        block: &Block,
        slots: &mut [Option<Operand>],
        name: &str,
        arity: usize,
        tail_span: &mut Option<Span>,
    ) -> Option<Vec<Operand>> {
        self.statements(block, slots)?;
        match &block.tail {
            | Tail::Return { atom, .. } => {
                let result = self.atom(atom, slots)?;
                if result.kind != Kind::Float {
                    return None;
                }
                let zero = self.emit(Inst::Const(0.0), Kind::Float)?;
                let mut outputs = vec![zero, result];
                outputs.resize(2 + arity, zero);
                Some(outputs)
            },
            | Tail::Call(call)
                if matches!(call.kind, CallKind::Normal)
                    && matches!(&call.callee, Atom::Global(callee) if callee == name)
                    && call.args.len() == arity =>
            {
                // One source site keeps error stack spans exact.
                if tail_span.is_some() {
                    return None;
                }
                *tail_span = Some(call.span);
                let one = self.emit(Inst::Const(1.0), Kind::Float)?;
                let zero = self.emit(Inst::Const(0.0), Kind::Float)?;
                let mut outputs = vec![one, zero];
                for atom in &call.args {
                    let value = self.atom(atom, slots)?;
                    if value.kind != Kind::Float {
                        return None;
                    }
                    outputs.push(value);
                }
                Some(outputs)
            },
            | Tail::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                let condition = self.atom(cond, slots)?;
                if condition.kind != Kind::Bool {
                    return None;
                }
                let a = self.step_block(then_block, &mut slots.to_vec(), name, arity, tail_span)?;
                let b = self.step_block(else_block, &mut slots.to_vec(), name, arity, tail_span)?;
                a.into_iter()
                    .zip(b)
                    .map(|(a, b)| self.emit(Inst::Select(condition.reg, a.reg, b.reg), Kind::Float))
                    .collect()
            },
            | _ => None,
        }
    }
}

enum Compiled {
    Scalar {
        program: Program,
        result: Kind,
    },
    Loop {
        program: LoopProgram,
        tail_span: Span,
    },
}
fn compile(function: &Function) -> Option<Compiled> {
    if !function.jit || !function.captures.is_empty() {
        return None;
    }
    let mut builder = Builder { insts: Vec::new() };
    let mut slots = vec![None; function.nslots as usize];
    for (index, pattern) in function.params.iter().enumerate() {
        let Pattern::Bind { slot, inner: None } = pattern else {
            return None;
        };
        let value = builder.emit(Inst::Input(u32::try_from(index).ok()?), Kind::Float)?;
        *slots.get_mut(*slot as usize)? = Some(value);
    }
    let mut tail_span = None;
    if let Some(outputs) = builder.step_block(
        &function.body,
        &mut slots.clone(),
        &function.name,
        function.params.len(),
        &mut tail_span,
    ) {
        if let Some(span) = tail_span {
            let program = LoopProgram::compile(
                builder.insts,
                function.params.len(),
                outputs.iter().map(|v| v.reg).collect(),
            )?;
            return Some(Compiled::Loop {
                program,
                tail_span: span,
            });
        }
    }
    // Start again: the speculative step lowering can have emitted operations.
    let mut builder = Builder { insts: Vec::new() };
    let mut slots = vec![None; function.nslots as usize];
    for (index, pattern) in function.params.iter().enumerate() {
        let Pattern::Bind { slot, inner: None } = pattern else {
            return None;
        };
        let value = builder.emit(Inst::Input(u32::try_from(index).ok()?), Kind::Float)?;
        *slots.get_mut(*slot as usize)? = Some(value);
    }
    let result = builder.block(&function.body, &mut slots)?;
    let program = Program::compile(builder.insts, function.params.len(), vec![result.reg])?;
    Some(Compiled::Scalar {
        program,
        result: result.kind,
    })
}

// The weak reference guards against pointer reuse; sweeping dead entries bounds
// the cache across REPL reloads. Machine code is never shared across threads.
type Entry = (Weak<Function>, Option<Compiled>);
thread_local! { static CACHE: RefCell<HashMap<usize, Entry>> = RefCell::new(HashMap::new()); }

pub(crate) enum Outcome {
    Value(Value),
    Tail {
        args: Vec<Value>,
        span: Span,
        consumed: usize,
    },
}

pub(crate) fn run(
    function: &Rc<Function>,
    args: &[Value],
    fuel: usize,
    is_self: impl FnOnce() -> bool,
) -> Option<Outcome> {
    if !function.jit || args.len() != function.params.len() {
        return None;
    }
    let values: Option<Vec<f64>> = args
        .iter()
        .map(|value| match value {
            | Value::Num(NumVal {
                kind: NumKind::Float(x),
                dim,
                display: None,
            }) if *dim == Dim::one() => Some(*x),
            | _ => None,
        })
        .collect();
    let values = values?;
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let key = Rc::as_ptr(function) as usize;
        if cache.len() >= 256 {
            cache.retain(|_, (function, _)| function.strong_count() > 0);
        }
        let entry = cache
            .entry(key)
            .or_insert_with(|| (Rc::downgrade(function), compile(function)));
        if !entry.0.ptr_eq(&Rc::downgrade(function)) {
            *entry = (Rc::downgrade(function), compile(function));
        }
        match entry.1.as_ref()? {
            | Compiled::Loop { program, tail_span } => {
                // The pure loop cannot mutate the binding, so one check before
                // entering native code is enough even in a mutable REPL scope.
                if !is_self() {
                    return None;
                }
                let limit = u64::try_from(fuel).ok()?;
                let result = program.run(&values, limit)?;
                if result.outputs[0] != 0.0 {
                    let consumed = usize::try_from(limit.checked_sub(result.remaining)?).ok()?;
                    Some(Outcome::Tail {
                        args: result.outputs[2..]
                            .iter()
                            .map(|value| Value::float(*value))
                            .collect(),
                        span: *tail_span,
                        consumed,
                    })
                } else {
                    Some(Outcome::Value(Value::float(result.outputs[1])))
                }
            },
            | Compiled::Scalar { program, result } => {
                let result_value = program.call(&values)?[0];
                Some(Outcome::Value(match result {
                    | Kind::Float => Value::Num(NumVal::float(result_value, Dim::one())),
                    | Kind::Bool => Value::Bool(result_value != 0.0),
                }))
            },
        }
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
        let compiled = compile(&function).expect("scalar branch compiles to machine code");
        let Compiled::Scalar { program, .. } = compiled else {
            panic!("expected scalar");
        };
        assert_eq!(program.call(&[3.0]), Some(vec![6.0]));
        assert_eq!(program.call(&[-3.0]), Some(vec![3.0]));
        assert!(run(&function, &[Value::float(3.0)], 0, || true).is_some());
    }

    #[test]
    fn tail_recursive_step_really_compiles() {
        let function = function(
            "@jit\nfn sum(n: Float, acc: Float): Float = if n <= 0.0 then acc else sum(n - 1.0, acc + n)\n",
        );
        let compiled = compile(&function).expect("tail loop compiles to machine code");
        let Compiled::Loop { program, .. } = compiled else {
            panic!("expected native loop");
        };
        let exhausted = program.run(&[3.0, 0.0], 2).unwrap();
        assert_eq!(exhausted.outputs, vec![1.0, 0.0, 0.0, 6.0]);
        assert_eq!(exhausted.remaining, 0);
        let done = program.run(&[3.0, 0.0], 3).unwrap();
        assert_eq!(done.outputs[0..2], [0.0, 6.0]);
        assert_eq!(done.remaining, 0);
        assert!(matches!(
            run(
                &function,
                &[Value::float(3.0), Value::float(0.0)],
                2,
                || true
            ),
            Some(Outcome::Tail { consumed: 2, .. })
        ));
        assert!(
            run(
                &function,
                &[Value::float(3.0), Value::float(0.0)],
                2,
                || false
            )
            .is_none()
        );
    }

    #[test]
    fn calls_are_never_speculated_in_branches() {
        let function = function("@jit\nfn f(x: Float): Float = if x > 0.0 then x else sqrt(x)\n");
        assert!(compile(&function).is_none());
    }
}
