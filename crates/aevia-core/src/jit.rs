//! Conservative lowering of pure Float/Bool Core IR to RSSN's machine-code tape.
//!
//! The current backend has straight-line SSA plus select, but no jumps or
//! function calls. Pure branches can be selected eagerly, and a single
//! self-tail-call site becomes a machine-code loop with a bounded back edge.
//! Other calls, effects, captures, exact numbers, units, and tensors fall back.
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
use aevia_rssn::jit::LoopProgram;
use aevia_rssn::jit::Program;
use aevia_rssn::jit::Reg;
use aevia_syntax::Span;
use aevia_syntax::ast::BinOp;
use aevia_syntax::ast::UnOp;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::rc::Weak;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Kind {
    Float,
    Bool,
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

    // A tail-recursive iteration is compiled into a multi-output step. The
    // existing interpreter trampoline performs the jump and enforces fuel.
    // Each branch remains pure Float/Bool computation; the unselected branch can
    // therefore be evaluated without changing observable behaviour.
    fn step_block(
        &mut self,
        block: &Block,
        slots: &mut [Option<Operand>],
        name: &str,
        arity: usize,
        tail_span: &mut Option<(Span, Span)>,
        parameters: &[Kind],
        result_kind: &mut Option<Kind>,
    ) -> Option<Vec<Operand>> {
        self.statements(block, slots)?;
        match &block.tail {
            | Tail::Return { atom, .. } => {
                let result = self.atom(atom, slots)?;
                if result_kind.is_some_and(|previous| previous != result.kind) {
                    return None;
                }
                *result_kind = Some(result.kind);
                let zero = self.emit(Inst::Const(0.0), Kind::Float)?;
                let mut outputs = vec![zero, result];
                outputs.resize(2 + arity, zero);
                Some(outputs)
            },
            | Tail::Call(call)
                if matches!(call.kind, CallKind::Normal | CallKind::Typed { .. })
                    && matches!(&call.callee, Atom::Global(callee) if callee == name)
                    && call.args.len() == arity =>
            {
                // One source site keeps error stack spans exact.
                if tail_span.is_some() {
                    return None;
                }
                *tail_span = Some((call.span, call.callee_span));
                let one = self.emit(Inst::Const(1.0), Kind::Float)?;
                let zero = self.emit(Inst::Const(0.0), Kind::Float)?;
                let mut outputs = vec![one, zero];
                for (atom, expected) in call.args.iter().zip(parameters) {
                    let value = self.atom(atom, slots)?;
                    if value.kind != *expected {
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
                let a = self.step_block(
                    then_block,
                    &mut slots.to_vec(),
                    name,
                    arity,
                    tail_span,
                    parameters,
                    result_kind,
                )?;
                let b = self.step_block(
                    else_block,
                    &mut slots.to_vec(),
                    name,
                    arity,
                    tail_span,
                    parameters,
                    result_kind,
                )?;
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
        callee_span: Span,
        result: Kind,
    },
}
fn compile(
    function: &Function,
    literals: &[(Span, u64)],
    parameters: &[Kind],
    expected_result: Kind,
) -> Option<Compiled> {
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
    let mut tail_span = None;
    let mut result_kind = None;
    if let Some(outputs) = builder.step_block(
        &function.body,
        &mut slots.clone(),
        &function.name,
        function.params.len(),
        &mut tail_span,
        parameters,
        &mut result_kind,
    ) {
        if let Some((span, callee_span)) = tail_span {
            if result_kind != Some(expected_result) {
                return None;
            }
            let program = LoopProgram::compile(
                builder.insts,
                function.params.len(),
                outputs.iter().map(|v| v.reg).collect(),
            )?;
            return Some(Compiled::Loop {
                program,
                tail_span: span,
                callee_span,
                result: result_kind?,
            });
        }
    }
    // Start again: the speculative step lowering can have emitted operations.
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
    let program = Program::compile(builder.insts, function.params.len(), vec![result.reg])?;
    Some(Compiled::Scalar {
        program,
        result: result.kind,
    })
}

// The weak reference guards against pointer reuse. All live and dead entries
// count toward the hard cap; machine code is never shared across threads.
type Entry = (Weak<Function>, Option<Compiled>);
type CacheKey = (usize, Vec<Kind>, Kind, Vec<(Span, u64)>);
thread_local! { static CACHE: RefCell<HashMap<CacheKey, Entry>> = RefCell::new(HashMap::new()); }
#[cfg(test)]
thread_local! { static NATIVE_LOOPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
thread_local! { static NATIVE_SCALARS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

/// Resolve numeric literals under the actual activation before consulting the
/// machine-code cache. Failure is a normal interpreter fallback and never
/// inserts a negative cache entry for a different specialization.
pub(crate) fn specialization(
    function: &Function,
    mut resolve: impl FnMut(&aevia_syntax::ast::NumberLit, Span) -> Option<f64>,
) -> Option<Vec<(Span, u64)>> {
    if !function.jit || !function.captures.is_empty() {
        return None;
    }
    fn visit(
        block: &Block,
        out: &mut Vec<(Span, u64)>,
        resolve: &mut impl FnMut(&aevia_syntax::ast::NumberLit, Span) -> Option<f64>,
    ) -> Option<()> {
        for stmt in &block.stmts {
            if let Stmt::Let(item) = stmt {
                match &item.rhs {
                    | Rhs::NumericLiteral(number) => {
                        out.push((item.span, resolve(number, item.span)?.to_bits()))
                    },
                    | Rhs::Block(inner) | Rhs::AndOr { right: inner, .. } => {
                        visit(inner, out, resolve)?
                    },
                    | _ => {},
                }
            }
        }
        if let Tail::If {
            then_block,
            else_block,
            ..
        } = &block.tail
        {
            visit(then_block, out, resolve)?;
            visit(else_block, out, resolve)?;
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

pub(crate) fn run(
    function: &Rc<Function>,
    args: &[Value],
    fuel: usize,
    literals: &[(Span, u64)],
    expected_result: Kind,
    is_self: impl FnOnce(Span) -> bool,
) -> Option<Outcome> {
    if !function.jit || args.len() != function.params.len() {
        return None;
    }
    let values: Option<Vec<(Kind, f64)>> = args
        .iter()
        .map(|value| {
            match value {
                | Value::Num(NumVal {
                    kind: NumKind::Float(x),
                    dim,
                    display: None,
                }) if *dim == Dim::one() => Some((Kind::Float, *x)),
                | Value::Bool(value) => Some((Kind::Bool, if *value { 1.0 } else { 0.0 })),
                | _ => None,
            }
        })
        .collect();
    let values = values?;
    let kinds: Vec<Kind> = values.iter().map(|(kind, _)| *kind).collect();
    let values: Vec<f64> = values.into_iter().map(|(_, value)| value).collect();
    CACHE.with(|cache| {
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
        let entry = cache.entry(key).or_insert_with(|| {
            (
                Rc::downgrade(function),
                compile(function, literals, &kinds, expected_result),
            )
        });
        if !entry.0.ptr_eq(&Rc::downgrade(function)) {
            *entry = (
                Rc::downgrade(function),
                compile(function, literals, &kinds, expected_result),
            );
        }
        match entry.1.as_ref()? {
            | Compiled::Loop {
                program,
                tail_span,
                callee_span,
                result: result_kind,
            } => {
                // The pure loop cannot mutate the binding, so one check before
                // entering native code is enough even in a mutable REPL scope.
                if !is_self(*callee_span) {
                    return None;
                }
                let limit = u64::try_from(fuel).ok()?;
                let result = program.run(&values, limit)?;
                #[cfg(test)]
                NATIVE_LOOPS.with(|count| count.set(count.get() + 1));
                if result.outputs[0] != 0.0 {
                    let consumed = usize::try_from(limit.checked_sub(result.remaining)?).ok()?;
                    Some(Outcome::Tail {
                        args: result.outputs[2..]
                            .iter()
                            .zip(&kinds)
                            .map(|(value, kind)| {
                                match kind {
                                    | Kind::Float => Value::float(*value),
                                    | Kind::Bool => Value::Bool(*value != 0.0),
                                }
                            })
                            .collect(),
                        span: *tail_span,
                        consumed,
                    })
                } else {
                    Some(Outcome::Value(match result_kind {
                        | Kind::Float => Value::float(result.outputs[1]),
                        | Kind::Bool => Value::Bool(result.outputs[1] != 0.0),
                    }))
                }
            },
            | Compiled::Scalar { program, result } => {
                let result_value = program.call(&values)?[0];
                #[cfg(test)]
                NATIVE_SCALARS.with(|count| count.set(count.get() + 1));
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
    fn dynamic_float_literals_compile_and_cache_by_exact_bits() {
        CACHE.with(|cache| cache.borrow_mut().clear());
        let function = dynamic_literal_function();
        assert!(specialization(&function, |_, _| None).is_none());
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
            assert_eq!(cache.len(), 1, "exact calls must not enter the Float cache");
            assert!(
                cache.values().any(|(_, compiled)| compiled.is_some()),
                "Float activation must compile machine code"
            );
            assert!(cache.keys().any(|(_, _, _, literals)| {
                literals.iter().any(|(_, bits)| *bits == 1.0_f64.to_bits())
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
        assert_eq!(NATIVE_SCALARS.with(|count| count.get()), 2);
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
        let compiled = compile(&function, &[], &[Kind::Float], Kind::Float)
            .expect("scalar branch compiles to machine code");
        let Compiled::Scalar { program, .. } = compiled else {
            panic!("expected scalar");
        };
        assert_eq!(program.call(&[3.0]), Some(vec![6.0]));
        assert_eq!(program.call(&[-3.0]), Some(vec![3.0]));
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
        let compiled = compile(&function, &[], &[Kind::Float, Kind::Float], Kind::Float)
            .expect("tail loop compiles to machine code");
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
}
