//! The interpreter over the Core IR.
//!
//! This is the loop §9.2.2 asks for: blocks are straight-line statements over a
//! flat slot vector, a closure is a code pointer plus an ordered environment,
//! and a call in tail position returns to the loop instead of recursing.
//!
//! It is *not* a second runtime. Arithmetic, comparison, tensor construction,
//! indexing, pattern matching against the AST, IO, builtins, the kernel bridge
//! and `Rssn` all go through the same primitives `eval.rs` uses; what is new is
//! the shape of the walk, and the three things that shape buys:
//!
//! * a tail call is a jump, so a self-tail-recursive loop is constant stack;
//! * a variable read is a vector index, not a `HashMap` chain;
//! * every call site and every node carries a span, so a frame names its source.

use crate::eval::EvalError;
use crate::eval::Frame;
use crate::eval::Interp;
use crate::eval::SliceValue;
use crate::eval::build_tensor;
use crate::value::Closure;
use crate::value::ClosureBody;
use crate::value::ClosureEnv;
use crate::value::IoAction;
use crate::value::IrEnv;
use crate::value::NumKind;
use crate::value::NumVal;
use crate::value::RecordValue;
use crate::value::Scope;
use crate::value::Value;
use crate::value::{
    self,
};
use aevia_ir::Action;
use aevia_ir::Atom;
use aevia_ir::Block;
use aevia_ir::Call;
use aevia_ir::CallKind;
use aevia_ir::Capture;
use aevia_ir::DoPlan;
use aevia_ir::DoTerminal;
use aevia_ir::Function;
use aevia_ir::Pattern;
use aevia_ir::Rhs;
use aevia_ir::Slice;
use aevia_ir::Stmt;
use aevia_ir::StrPart;
use aevia_ir::Tail;
use aevia_syntax::Span;
use aevia_syntax::ast::BinOp;
use aevia_syntax::ast::PatLit;
use aevia_types::adt;
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::ToPrimitive;
use std::cell::RefCell;
use std::rc::Rc;

/// What evaluating a block produced.
///
/// A `Tail` is a call to an IR closure in tail position: the enclosing
/// `run_closure` loop rebinds its frame and continues instead of recursing.
enum Flow {
    Value(Value),
    Tail {
        closure: Rc<Closure>,
        args: Vec<Value>,
        span: Span,
        instance: Option<Rc<crate::generics::FunctionProof>>,
    },
}

/// The closure a module-level function or instance method becomes.
///
/// Its environment is empty — every top-level name resolves through the module
/// scope — but it still carries that scope, so a body that reads a top-level
/// `let` finds it exactly as the AST path's lexical chain would.
#[must_use]
pub fn module_closure(
    function: &Rc<Function>,
    scope: &Rc<Scope>,
) -> Value {
    let env = Rc::new(IrEnv {
        slots: RefCell::new(Vec::new()),
        scope: Some(scope.clone()),
    });
    build_closure(function, &[], &env, scope.types.as_ref())
}

/// Evaluates a top-level `let`'s value.
///
/// The block runs with an empty frame and the module scope; the resulting value
/// is bound by the same pattern binder the AST path uses, because a top-level
/// binding writes into the module's name table rather than a slot.
///
/// # Errors
///
/// Propagates the value's own runtime failure.
pub fn eval_top(
    interp: &mut Interp,
    block: &Block,
    nslots: u32,
    scope: &Rc<Scope>,
) -> Result<Value, EvalError> {
    let previous = interp.enter_scope(scope)?;
    let env = Rc::new(IrEnv {
        slots: RefCell::new(Vec::new()),
        scope: Some(scope.clone()),
    });
    let mut frame = vec![Value::Unit; usize::try_from(nslots).unwrap_or(0)];
    let result = eval_block(interp, block, &mut frame, &env).and_then(|flow| force(interp, flow));
    interp.restore_context(previous);
    result
}

/// Runs an IR closure to completion, trampolining its tail calls.
///
/// The depth guard lives in [`Interp::apply`] and counts *calls*; a tail call
/// does not add one, which is the point. A tail loop that never terminates is
/// caught by [`Interp::max_tail_calls`] instead, so "a legal program terminates,
/// a runaway one is `E0805`" still holds.
pub(crate) fn run_closure(
    interp: &mut Interp,
    closure: &Rc<Closure>,
    args: Vec<Value>,
    span: Span,
    instance: Option<Rc<crate::generics::FunctionProof>>,
) -> Result<Value, EvalError> {
    let (ClosureBody::Ir(first), ClosureEnv::Ir(first_env)) = (&closure.body, &closure.env) else {
        return Err(EvalError::new("E0809", "not an IR closure").at(span));
    };
    let mut function = first.clone();
    let mut env = first_env.clone();
    #[cfg(feature = "jit")]
    let mut current_closure = closure.clone();
    let mut args = args;
    let mut current_instance = instance;
    let mut bindings = interp.bindings.clone();
    if function.params.len() != args.len() {
        return Err(EvalError::new(
            "E0315",
            format!(
                "this function takes {} argument(s) but {} were given",
                function.params.len(),
                args.len()
            ),
        )
        .at(span));
    }
    interp.frames.push(Frame {
        function: function.name.clone(),
        span,
    });
    let mut tail_calls = 0usize;
    // One frame buffer for the whole trampoline. A tail call reuses the frame
    // by definition, so allocating a fresh `Vec` per iteration made the
    // cheapest thing a loop does the most expensive: a million iterations were
    // a million allocations.
    let mut frame: Vec<Value> = Vec::new();
    let outcome = 'run: loop {
        if let Some(scope) = &env.scope {
            if let Err(error) = interp.enter_scope(scope) {
                break 'run Err(error);
            }
        }
        interp.bindings = bindings.clone();
        #[cfg(feature = "jit")]
        let compiled_flow = {
            let remaining = interp.max_tail_calls.saturating_sub(tail_calls);
            // Host calls can pass values that differ from a source signature.
            // Native specialization is permitted only by an authenticated,
            // fully concrete activation whose parameter/result kinds agree.
            let expected = current_instance
                .as_ref()
                .filter(|proof| proof.verified)
                .and_then(|proof| {
                    let aevia_types::Type::Fn(parameters, result) = &proof.signature else {
                        return None;
                    };
                    if parameters.len() != args.len()
                        || aevia_types::class::has_variables(&proof.signature)
                    {
                        return None;
                    }
                    let kind = |ty: &aevia_types::Type| {
                        match ty {
                            | aevia_types::Type::Bool => Some(crate::jit::Kind::Bool),
                            | aevia_types::Type::Scalar(aevia_types::Rep::Float, dim)
                                if dim.is_one() =>
                            {
                                Some(crate::jit::Kind::Float)
                            },
                            | _ => None,
                        }
                    };
                    for (parameter, value) in parameters.iter().zip(&args) {
                        match (kind(parameter), value) {
                            | (Some(crate::jit::Kind::Bool), Value::Bool(_)) => {},
                            | (
                                Some(crate::jit::Kind::Float),
                                Value::Num(crate::value::NumVal {
                                    kind: crate::value::NumKind::Float(_),
                                    dim,
                                    display: None,
                                }),
                            ) if dim.is_one() => {},
                            | _ => return None,
                        }
                    }
                    kind(result)
                });
            let literals = crate::jit::specialization(&function, |number, span| {
                let number = match number {
                    | aevia_syntax::ast::NumberLit::Int(value) => {
                        interp.integer_literal(value, span)
                    },
                    | aevia_syntax::ast::NumberLit::Decimal(value) => {
                        interp.float_literal(value, span)
                    },
                }
                .ok()?;
                match number {
                    | crate::value::NumVal {
                        kind: crate::value::NumKind::Float(value),
                        dim,
                        display: None,
                    } if dim == aevia_dim::Dim::one() => Some(value),
                    | _ => None,
                }
            });
            let outcome = expected
                .zip(literals.as_deref())
                .and_then(|(expected, literals)| {
                    crate::jit::run(
                        &function,
                        &args,
                        remaining,
                        literals,
                        expected,
                        |callee_span| {
                            // Native code may loop only while this name still points to the
                            // same canonical closure and the actual recursive source call
                            // preserves every selected generic binding.
                            let Ok(target) = eval_atom(
                                interp,
                                &Atom::Global(function.name.to_string()),
                                &mut frame,
                                &env,
                                function.span,
                            ) else {
                                return false;
                            };
                            interp.self_specialization_matches(
                                &current_closure,
                                current_instance.as_ref(),
                                &target,
                                callee_span,
                                &args,
                            )
                        },
                    )
                });
            match outcome {
                | Some(crate::jit::Outcome::Value(value)) => Some(Flow::Value(value)),
                | Some(crate::jit::Outcome::Tail { args, span, consumed }) => {
                    tail_calls += consumed;
                    if consumed > 0 {
                        if let Some(current) = interp.frames.last_mut() {
                            current.span = span;
                        }
                    }
                    Some(Flow::Tail {
                        closure: current_closure.clone(),
                        args,
                        span,
                        instance: current_instance.clone(),
                    })
                },
                | None => None,
            }
        };
        #[cfg(not(feature = "jit"))]
        let compiled_flow: Option<Flow> = None;
        let flow = match compiled_flow {
            | Some(flow) => flow,
            | None => {
                let slots = usize::try_from(function.nslots).unwrap_or(0);
                frame.clear();
                frame.resize(slots, Value::Unit);
                for (pattern, argument) in function.params.iter().zip(args.iter_mut()) {
                    // Move plain arguments into the reused trampoline frame.
                    if !bind_parameter(pattern, argument, &mut frame) {
                        break 'run Err(EvalError::new(
                            "E0308",
                            format!("pattern does not match value {}", value::show(argument)),
                        )
                        .at(function.span));
                    }
                }
                match eval_block(interp, &function.body, &mut frame, &env) {
                    | Ok(flow) => flow,
                    | Err(error) => break 'run Err(error),
                }
            },
        };
        let (next, mut next_args, next_span, next_instance) = match flow {
            | Flow::Value(value) => break 'run Ok(value),
            | Flow::Tail {
                closure,
                args,
                span,
                instance,
            } => (closure, args, span, instance),
        };
        if tail_calls >= interp.max_tail_calls {
            // Not a stack overflow: a tail call reuses the frame, so the stack
            // is one frame deep however long the loop runs. Only a program that
            // never terminates reaches this budget, and calling it "stack
            // overflow" would send the reader looking for the wrong bug.
            break 'run Err(EvalError::new(
                "E0805",
                format!(
                    "the program made more than {} tail calls without terminating",
                    interp.max_tail_calls
                ),
            )
            .at(next_span)
            .with_note(
                "tail calls reuse the frame, so this is a tail-call budget, not a stack overflow"
                    .to_string(),
            )
            .with_stack(interp.frames.clone()));
        }
        tail_calls += 1;
        let (ClosureBody::Ir(next_function), ClosureEnv::Ir(next_env)) = (&next.body, &next.env)
        else {
            break 'run interp.apply_instantiated(
                Value::Closure(next),
                next_args,
                next_span,
                next_instance,
            );
        };
        if next_function.params.len() != next_args.len() {
            break 'run Err(EvalError::new(
                "E0315",
                format!(
                    "this function takes {} argument(s) but {} were given",
                    next_function.params.len(),
                    next_args.len()
                ),
            )
            .at(next_span));
        }
        function = next_function.clone();
        env = next_env.clone();
        current_instance = match next_instance {
            | Some(instance) => Some(instance),
            | None => {
                match interp.prepare_function_call(
                    &Value::Closure(next.clone()),
                    &mut next_args,
                    None,
                ) {
                    | Ok(instance) => instance,
                    | Err(error) => break 'run Err(error),
                }
            },
        };
        bindings = current_instance
            .as_ref()
            .map(|instance| instance.bindings())
            .or_else(|| next.captured_bindings.clone());
        #[cfg(feature = "jit")]
        {
            current_closure = next.clone();
        }
        args = next_args;
        if let Some(current) = interp.frames.last_mut() {
            // A self tail call keeps the name, and cloning it per iteration was
            // one allocation per loop step.
            if current.function.as_ref() != function.name.as_ref() {
                current.function = function.name.clone();
            }
            current.span = next_span;
        }
    };
    // §9.3.4: the failing function's own frame is part of the trace, so the
    // stack is taken before the frame is popped. It is taken *only* when the
    // error has no stack yet: cloning a `Vec<String>` on every return was the
    // cheapest thing a tail loop did, and a nested `apply` already attached the
    // same stack on its way out.
    let needs_stack = matches!(
        &outcome,
        Err(error) if error.payload.is_none() && error.stack.is_empty()
    );
    let stack = needs_stack.then(|| interp.frames.clone());
    interp.frames.pop();
    match outcome {
        // `?` returns from the enclosing function by unwinding to its call,
        // which is where the `Err`/`None` payload belongs.
        | Err(error) if error.payload.is_some() => Ok(error.payload.expect("checked above")),
        | Err(error) => {
            Err(match stack {
                | Some(stack) => error.with_stack(stack),
                | None => error,
            })
        },
        | other => other,
    }
}

fn force(
    interp: &mut Interp,
    flow: Flow,
) -> Result<Value, EvalError> {
    match flow {
        | Flow::Value(value) => Ok(value),
        | Flow::Tail {
            closure,
            args,
            span,
            instance,
        } => interp.apply_instantiated(Value::Closure(closure), args, span, instance),
    }
}

fn eval_block(
    interp: &mut Interp,
    block: &Block,
    frame: &mut Vec<Value>,
    env: &Rc<IrEnv>,
) -> Result<Flow, EvalError> {
    for stmt in &block.stmts {
        match stmt {
            | Stmt::Let(item) => {
                let value = eval_rhs(interp, &item.rhs, frame, env, item.span)?;
                let value = interp.attach_checked_type(value, item.span);
                let value = interp.bind_source_function(value, item.span);
                set(frame, item.slot, value);
            },
            | Stmt::Bind { pattern, atom, span } => {
                let value = eval_atom(interp, atom, frame, env, *span)?;
                if !match_pattern(pattern, &value, frame) {
                    return Err(EvalError::new(
                        "E0308",
                        format!("pattern does not match value {}", value::show(&value)),
                    )
                    .at(*span));
                }
            },
        }
    }
    eval_tail(interp, &block.tail, frame, env)
}

fn eval_tail(
    interp: &mut Interp,
    tail: &Tail,
    frame: &mut Vec<Value>,
    env: &Rc<IrEnv>,
) -> Result<Flow, EvalError> {
    match tail {
        | Tail::Return { atom, span } => {
            let value = eval_atom(interp, atom, frame, env, *span)?;
            let value = interp.attach_checked_type(value, *span);
            Ok(Flow::Value(interp.bind_source_function(value, *span)))
        },
        | Tail::Call(call) => eval_call(interp, call, frame, env, true),
        | Tail::If {
            cond,
            cond_span,
            then_block,
            else_block,
            ..
        } => {
            match eval_atom(interp, cond, frame, env, *cond_span)? {
                | Value::Bool(true) => eval_block(interp, then_block, frame, env),
                | Value::Bool(false) => eval_block(interp, else_block, frame, env),
                | other => {
                    Err(EvalError::new(
                        "E0302",
                        format!("`if` expects a Bool condition, found {}", other.type_name()),
                    )
                    .at(*cond_span))
                },
            }
        },
        | Tail::Match {
            scrutinee,
            arms,
            span,
        } => {
            let subject = eval_atom(interp, scrutinee, frame, env, *span)?;
            for arm in arms {
                if !match_pattern(&arm.pattern, &subject, frame) {
                    continue;
                }
                if let Some(guard) = &arm.guard {
                    let produced = eval_block(interp, guard, frame, env)?;
                    match force(interp, produced)? {
                        | Value::Bool(true) => {},
                        | Value::Bool(false) => continue,
                        | other => {
                            return Err(EvalError::new(
                                "E0308",
                                format!("a match guard must be Bool, found {}", other.type_name()),
                            )
                            .at(*span));
                        },
                    }
                }
                return eval_block(interp, &arm.body, frame, env);
            }
            Err(EvalError::new(
                "E0309",
                format!("no match arm accepted {}", value::show(&subject)),
            )
            .at(*span))
        },
    }
}

fn eval_call(
    interp: &mut Interp,
    call: &Call,
    frame: &mut Vec<Value>,
    env: &Rc<IrEnv>,
    tail: bool,
) -> Result<Flow, EvalError> {
    match &call.kind {
        | CallKind::Typed { types } => {
            let function = eval_atom(interp, &call.callee, frame, env, call.span)?;
            let mut values = eval_call_args(interp, call, frame, env)?;
            let function = interp.type_apply_value(function, types, call.callee_span);
            let instance =
                interp.prepare_function_call(&function, &mut values, Some(call.callee_span))?;
            if tail {
                if let Value::Closure(closure) = &function {
                    if matches!(closure.body, ClosureBody::Ir(_)) {
                        return Ok(Flow::Tail {
                            closure: closure.clone(),
                            args: values,
                            span: call.span,
                            instance,
                        });
                    }
                }
            }
            if instance.is_some() {
                Ok(Flow::Value(interp.apply_instantiated(
                    function, values, call.span, instance,
                )?))
            } else {
                Ok(Flow::Value(interp.apply_source_call(
                    function,
                    values,
                    call.span,
                    call.callee_span,
                    &call.arg_spans,
                )?))
            }
        },
        | CallKind::Rssn { name } => {
            let values = eval_call_args(interp, call, frame, env)?;
            Ok(Flow::Value(interp.call_rssn(name, call.span, values)?))
        },
        | CallKind::Dictionary { method, target: _ } => {
            let values = eval_call_args(interp, call, frame, env)?;
            match interp.dictionary_use(call.span, method) {
                | Some(use_site) => {
                    // A dictionary call in tail position is a tail call like any
                    // other. Applying the instance body here would spend Rust
                    // stack on every step of a recursive instance method, which
                    // is exactly what the trampoline exists to avoid.
                    let dispatch =
                        interp.resolve_dictionary(&use_site, method, &values, call.span)?;
                    match dispatch {
                        | crate::eval::Dispatch::Instance(body) => {
                            let mut values = values;
                            let instance = interp.prepare_function_call(
                                &body,
                                &mut values,
                                Some(call.callee_span),
                            )?;
                            if tail {
                                if let Value::Closure(closure) = &body {
                                    if matches!(closure.body, ClosureBody::Ir(_)) {
                                        return Ok(Flow::Tail {
                                            closure: closure.clone(),
                                            args: values,
                                            span: call.span,
                                            instance,
                                        });
                                    }
                                }
                            }
                            Ok(Flow::Value(
                                interp.apply_instantiated(body, values, call.span, instance)?,
                            ))
                        },
                        | crate::eval::Dispatch::Builtin => {
                            Ok(Flow::Value(interp.call_named(method, values, call.span)?))
                        },
                    }
                },
                | None => {
                    let function = eval_atom(interp, &call.callee, frame, env, call.span)?;
                    Ok(Flow::Value(interp.apply(function, values, call.span)?))
                },
            }
        },
        | CallKind::Normal => {
            let function = eval_atom(interp, &call.callee, frame, env, call.callee_span)?;
            let mut values = eval_call_args(interp, call, frame, env)?;
            let instance =
                interp.prepare_function_call(&function, &mut values, Some(call.callee_span))?;
            if tail {
                if let Value::Closure(closure) = &function {
                    if matches!(closure.body, ClosureBody::Ir(_)) {
                        return Ok(Flow::Tail {
                            closure: closure.clone(),
                            args: values,
                            span: call.span,
                            instance,
                        });
                    }
                }
            }
            if instance.is_some() {
                Ok(Flow::Value(interp.apply_instantiated(
                    function, values, call.span, instance,
                )?))
            } else {
                Ok(Flow::Value(interp.apply_source_call(
                    function,
                    values,
                    call.span,
                    call.callee_span,
                    &call.arg_spans,
                )?))
            }
        },
    }
}

fn eval_call_args(
    interp: &mut Interp,
    call: &Call,
    frame: &mut Vec<Value>,
    env: &Rc<IrEnv>,
) -> Result<Vec<Value>, EvalError> {
    let mut values = Vec::with_capacity(call.args.len());
    for (index, atom) in call.args.iter().enumerate() {
        let span = call.arg_spans.get(index).copied().unwrap_or(call.span);
        let value = eval_atom(interp, atom, frame, env, span)?;
        values.push(interp.attach_checked_type(value, span));
    }
    Ok(values)
}

fn eval_atoms(
    interp: &mut Interp,
    atoms: &[Atom],
    frame: &mut Vec<Value>,
    env: &Rc<IrEnv>,
    span: Span,
) -> Result<Vec<Value>, EvalError> {
    let mut values = Vec::with_capacity(atoms.len());
    for atom in atoms {
        values.push(eval_atom(interp, atom, frame, env, span)?);
    }
    Ok(values)
}

fn eval_atom(
    interp: &mut Interp,
    atom: &Atom,
    frame: &mut Vec<Value>,
    env: &Rc<IrEnv>,
    span: Span,
) -> Result<Value, EvalError> {
    Ok(match atom {
        // A slot outside the frame is a lowering bug, not a program. Reading it
        // as `Unit` would turn that bug into a silent wrong value, which is the
        // hardest kind to attribute, so it is an error.
        | Atom::Local(slot) => {
            frame.get(*slot as usize).cloned().ok_or_else(|| {
                EvalError::new(
                    "E0899",
                    format!(
                        "internal: local slot {slot} is outside a frame of {}",
                        frame.len()
                    ),
                )
                .at(span)
            })?
        },
        | Atom::Capture(index) => {
            env.slots
                .borrow()
                .get(*index as usize)
                .cloned()
                .ok_or_else(|| {
                    EvalError::new(
                        "E0899",
                        format!(
                            "internal: capture {index} is outside an environment of {}",
                            env.slots.borrow().len()
                        ),
                    )
                    .at(span)
                })?
        },
        | Atom::Global(name) => interp.lookup_global(name, env.scope.as_ref(), span)?,
        | Atom::Int(value) => Value::Num(NumVal::int(value.clone())),
        | Atom::Exact(value) => Value::Num(NumVal::exact(value.clone(), aevia_dim::Dim::one())),
        | Atom::Float(value) => Value::Num(NumVal::float(*value, aevia_dim::Dim::one())),
        | Atom::Complex { re, im } => {
            Value::Num(NumVal {
                kind: NumKind::Complex(*re, *im),
                dim: aevia_dim::Dim::one(),
                display: None,
            })
        },
        | Atom::Bool(value) => Value::Bool(*value),
        | Atom::Str(text) => Value::string(text.clone()),
        | Atom::Unit => Value::Unit,
    })
}

fn set(
    frame: &mut Vec<Value>,
    slot: u32,
    value: Value,
) {
    let index = slot as usize;
    if index >= frame.len() {
        frame.resize(index + 1, Value::Unit);
    }
    frame[index] = value;
}

fn build_closure(
    function: &Rc<Function>,
    frame: &[Value],
    env: &Rc<IrEnv>,
    bindings: Option<&Rc<crate::generics::CallBindings>>,
) -> Value {
    let mut slots = Vec::with_capacity(function.captures.len());
    for capture in &function.captures {
        slots.push(match capture {
            | Capture::Local(slot) => frame.get(*slot as usize).cloned().unwrap_or(Value::Unit),
            | Capture::Capture(index) => {
                env.slots
                    .borrow()
                    .get(*index as usize)
                    .cloned()
                    .unwrap_or(Value::Unit)
            },
        });
    }
    Value::Closure(Rc::new(Closure {
        canonical_origin: std::rc::Weak::new(),
        params: Vec::new(),
        body: ClosureBody::Ir(function.clone()),
        env: ClosureEnv::Ir(Rc::new(IrEnv {
            slots: RefCell::new(slots),
            scope: env.scope.clone(),
        })),
        name: Some(function.name.clone()),
        proof: env.scope.as_ref().and_then(|scope| {
            scope.context.as_ref().and_then(|context| {
                crate::generics::FunctionProof::at(context, function.span, bindings)
            })
        }),
        captured_bindings: bindings.cloned(),
    }))
}

fn eval_rhs(
    interp: &mut Interp,
    rhs: &Rhs,
    frame: &mut Vec<Value>,
    env: &Rc<IrEnv>,
    span: Span,
) -> Result<Value, EvalError> {
    match rhs {
        | Rhs::Atom(atom) => eval_atom(interp, atom, frame, env, span),
        | Rhs::ClassMethodRef { class, method } => {
            Ok(Interp::class_method_value(
                class.clone(),
                method.clone(),
                env.scope.clone().unwrap_or_else(|| interp.root()),
            ))
        },
        | Rhs::TypeApply { callee, types } => {
            let function = eval_atom(interp, callee, frame, env, span)?;
            Ok(interp.type_apply_value(function, types, span))
        },
        | Rhs::Error { code, message } => Err(EvalError::new(code, message.clone()).at(span)),
        | Rhs::Unary { op, operand } => {
            let value = eval_atom(interp, operand, frame, env, span)?;
            interp.unary(*op, value, span)
        },
        | Rhs::Binary { op, left, right } => {
            let left = eval_atom(interp, left, frame, env, span)?;
            let right = eval_atom(interp, right, frame, env, span)?;
            interp.binary(*op, left, right, span)
        },
        | Rhs::AndOr { op, left, right } => {
            let left = eval_atom(interp, left, frame, env, span)?;
            let short_circuit = match (op, &left) {
                | (BinOp::And, Value::Bool(false)) => Some(false),
                | (BinOp::Or, Value::Bool(true)) => Some(true),
                | _ => None,
            };
            if let Some(value) = short_circuit {
                return Ok(Value::Bool(value));
            }
            let produced = eval_block(interp, right, frame, env)?;
            let right = force(interp, produced)?;
            interp.binary(*op, left, right, span)
        },
        | Rhs::Call(call) => {
            match eval_call(interp, call, frame, env, false)? {
                | Flow::Value(value) => Ok(value),
                | Flow::Tail { .. } => unreachable!("a value-position call is not trampolined"),
            }
        },
        | Rhs::Closure(function) => {
            Ok(build_closure(
                function,
                frame,
                env,
                interp.bindings.as_ref(),
            ))
        },
        | Rhs::RecClosure {
            function,
            self_capture,
        } => {
            let closure = build_closure(function, frame, env, interp.bindings.as_ref());
            if let (Some(index), Value::Closure(inner)) = (self_capture, &closure) {
                if let ClosureEnv::Ir(ir_env) = &inner.env {
                    let mut slots = ir_env.slots.borrow_mut();
                    let position = *index as usize;
                    if position >= slots.len() {
                        slots.resize(position + 1, Value::Unit);
                    }
                    slots[position] = closure.clone();
                }
            }
            Ok(closure)
        },
        | Rhs::Block(block) => {
            let produced = eval_block(interp, block, frame, env)?;
            force(interp, produced)
        },
        | Rhs::Field {
            base,
            qualified,
            name,
        } => {
            if let Some(qualified) = qualified {
                if interp.globals.contains_key(qualified) {
                    return interp.lookup_global(qualified, env.scope.as_ref(), span);
                }
            }
            let base = eval_atom(interp, base, frame, env, span)?;
            match base {
                | Value::Record(record) => {
                    record.fields.get(name).cloned().ok_or_else(|| {
                        EvalError::new("E0306", format!("no field `{name}`")).at(span)
                    })
                },
                | other => {
                    Err(EvalError::new(
                        "E0302",
                        format!("field access expects a record, found {}", other.type_name()),
                    )
                    .at(span))
                },
            }
        },
        | Rhs::NumericLiteral(number) => {
            Ok(Value::Num(match number {
                | aevia_syntax::ast::NumberLit::Int(value) => {
                    interp.integer_literal(value, span)?
                },
                | aevia_syntax::ast::NumberLit::Decimal(text) => {
                    interp.float_literal(text, span)?
                },
            }))
        },
        | Rhs::RssnMember { name } => Ok(crate::rssn_module::member(name)),
        | Rhs::Record { base, fields } => {
            let mut collected: IndexMap<String, Value> = IndexMap::new();
            if let Some(base) = base {
                match eval_atom(interp, base, frame, env, span)? {
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
                        .at(span));
                    },
                }
            }
            for (name, atom) in fields {
                let value = eval_atom(interp, atom, frame, env, span)?;
                collected.insert(name.clone(), value);
            }
            Ok(Value::Record(Rc::new(RecordValue { fields: collected })))
        },
        | Rhs::Tuple(items) => {
            let mut fields = IndexMap::new();
            for (index, atom) in items.iter().enumerate() {
                let value = eval_atom(interp, atom, frame, env, span)?;
                fields.insert(index.to_string(), value);
            }
            Ok(Value::Record(Rc::new(RecordValue { fields })))
        },
        | Rhs::Tensor(items) => {
            let values = eval_atoms(interp, items, frame, env, span)?;
            interp.tensor_from_values(values, span)
        },
        | Rhs::Index { base, indices } => {
            let base = eval_atom(interp, base, frame, env, span)?;
            if !matches!(base, Value::Tensor(_)) {
                return Err(EvalError::new("E0302", "indexing expects a tensor").at(span));
            }
            let mut resolved = Vec::with_capacity(indices.len());
            for index in indices {
                match index {
                    | Slice::Index(atom) => {
                        resolved.push(SliceValue::Index(
                            eval_atom(interp, atom, frame, env, span)?.as_f64(),
                        ));
                    },
                    | Slice::Range {
                        start,
                        end,
                        inclusive,
                        step,
                    } => {
                        let start = match start {
                            | Some(atom) => {
                                Some(
                                    eval_atom(interp, atom, frame, env, span)?
                                        .as_f64()
                                        .unwrap_or(0.0),
                                )
                            },
                            | None => None,
                        };
                        let end = match end {
                            | Some(atom) => {
                                Some(
                                    eval_atom(interp, atom, frame, env, span)?
                                        .as_f64()
                                        .unwrap_or(0.0),
                                )
                            },
                            | None => None,
                        };
                        // A step that is not a number is `NaN` here, which the
                        // slicer refuses: `0` would be a plausible-looking
                        // step and `1` a plausible-looking default, and neither
                        // is what the program said.
                        let step = match step {
                            | Some(atom) => {
                                Some(
                                    eval_atom(interp, atom, frame, env, span)?
                                        .as_f64()
                                        .unwrap_or(f64::NAN),
                                )
                            },
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
            interp.index_resolved(base, &resolved, span)
        },
        | Rhs::Quantity {
            value,
            unit,
            as_float,
        } => interp.quantity(value, unit, span, *as_float),
        | Rhs::Str(parts) => {
            let mut out = String::new();
            for part in parts {
                match part {
                    | StrPart::Text(text) => out.push_str(text),
                    | StrPart::Atom(atom) => {
                        let value = eval_atom(interp, atom, frame, env, span)?;
                        out.push_str(&value::show(&value));
                    },
                }
            }
            Ok(Value::string(out))
        },
        | Rhs::Try(atom) => {
            let value = eval_atom(interp, atom, frame, env, span)?;
            match &value {
                | Value::Variant(variant) if variant.adt == adt::RESULT_ID => {
                    if variant.index == 0 {
                        variant
                            .fields
                            .first()
                            .cloned()
                            .ok_or_else(|| EvalError::new("E0809", "malformed Ok"))
                    } else {
                        let mut error = EvalError::new("E0702", "propagated Err").at(span);
                        error.payload = Some(value.clone());
                        Err(error)
                    }
                },
                | Value::Variant(variant) if variant.adt == adt::OPTION_ID => {
                    if variant.index == 1 {
                        variant
                            .fields
                            .first()
                            .cloned()
                            .ok_or_else(|| EvalError::new("E0809", "malformed Some"))
                    } else {
                        let mut error = EvalError::new("E0702", "propagated None").at(span);
                        error.payload = Some(value.clone());
                        Err(error)
                    }
                },
                | other => {
                    Err(EvalError::new(
                        "E0702",
                        format!("`?` expects Result or Option, found {}", other.type_name()),
                    )
                    .at(span))
                },
            }
        },
        | Rhs::Factorial { value, double } => {
            let value = eval_atom(interp, value, frame, env, span)?;
            let n = match &value {
                | Value::Num(num) => {
                    match &num.kind {
                        | NumKind::Int(value) => {
                            value
                                .to_i64()
                                .ok_or_else(|| EvalError::new("E0801", "factorial is too large"))?
                        },
                        | _ => {
                            return Err(
                                EvalError::new("E0302", "factorial expects an Int").at(span)
                            );
                        },
                    }
                },
                | _ => {
                    return Err(EvalError::new("E0302", "factorial expects an Int").at(span));
                },
            };
            Ok(Value::Num(NumVal::int(factorial(n, *double))))
        },
        | Rhs::Do(plan) => build_do(interp, plan, frame, env),
    }
}

fn build_do(
    interp: &mut Interp,
    plan: &DoPlan,
    frame: &mut Vec<Value>,
    env: &Rc<IrEnv>,
) -> Result<Value, EvalError> {
    let mut actions: Vec<Rc<IoAction>> = Vec::with_capacity(plan.prefix.len() + 1);
    for Action { atom, span } in &plan.prefix {
        match eval_atom(interp, atom, frame, env, *span)? {
            | Value::Io(action) => actions.push(action),
            | other => {
                return Err(EvalError::new(
                    "E0701",
                    format!(
                        "a `do` statement must be an IO action, found {}",
                        other.type_name()
                    ),
                )
                .at(*span));
            },
        }
    }
    match &plan.terminal {
        | DoTerminal::End => {},
        | DoTerminal::Let { thunk } => {
            match build_closure(thunk, frame, env, interp.bindings.as_ref()) {
                | Value::Closure(closure) => actions.push(Rc::new(IoAction::Defer(closure))),
                | _ => unreachable!("a thunk is a closure"),
            }
        },
        | DoTerminal::Bind {
            action,
            span: action_span,
            continuation,
        } => {
            let action = match eval_atom(interp, action, frame, env, *action_span)? {
                | Value::Io(action) => action,
                | _ => {
                    return Err(
                        EvalError::new("E0701", "`<-` expects an IO action").at(*action_span)
                    );
                },
            };
            let func = build_closure(continuation, frame, env, interp.bindings.as_ref());
            actions.push(Rc::new(IoAction::Bind { action, func }));
        },
    }
    match actions.len() {
        | 0 => Ok(Value::Io(Rc::new(IoAction::Pure(Value::Unit)))),
        | 1 => Ok(Value::Io(actions.remove(0))),
        | _ => Ok(Value::Io(Rc::new(IoAction::Seq(actions)))),
    }
}

fn factorial(
    n: i64,
    double: bool,
) -> BigInt {
    use num_traits::One;
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

/// Matches a pattern, writing its binders into the frame's slots.
///
/// A failed arm may have written some slots; the next arm uses different slots,
/// so nothing leaks — the same guarantee the AST path gets from a fresh child
/// scope per arm.
/// Binds one parameter, moving the value when the pattern is a plain binder.
///
/// `match_pattern` takes `&Value` and clones, which is right for a match arm
/// (the scrutinee is still needed) and wasteful for a parameter, whose vector
/// the caller has already given up.
fn bind_parameter(
    pattern: &Pattern,
    value: &mut Value,
    frame: &mut Vec<Value>,
) -> bool {
    match pattern {
        | Pattern::Bind { slot, inner: None } => {
            set(frame, *slot, std::mem::replace(value, Value::Unit));
            true
        },
        | other => match_pattern(other, value, frame),
    }
}

fn match_pattern(
    pattern: &Pattern,
    value: &Value,
    frame: &mut Vec<Value>,
) -> bool {
    match pattern {
        | Pattern::Error => false,
        | Pattern::Wildcard => true,
        | Pattern::Bind { slot, inner } => {
            if let Some(inner) = inner {
                if !match_pattern(inner, value, frame) {
                    return false;
                }
            }
            set(frame, *slot, value.clone());
            true
        },
        | Pattern::Ctor { name, args } => {
            let Value::Variant(variant) = value else {
                return false;
            };
            if &variant.name != name || variant.fields.len() != args.len() {
                return false;
            }
            for (sub, field) in args.iter().zip(variant.fields.iter()) {
                if !match_pattern(sub, field, frame) {
                    return false;
                }
            }
            true
        },
        | Pattern::Record(fields) => {
            let Value::Record(record) = value else {
                return false;
            };
            for (name, sub) in fields {
                let Some(field) = record.fields.get(name) else {
                    return false;
                };
                if !match_pattern(sub, field, frame) {
                    return false;
                }
            }
            true
        },
        | Pattern::Tuple(items) => {
            let Value::Record(record) = value else {
                return false;
            };
            for (index, item) in items.iter().enumerate() {
                let Some(field) = record.fields.get(&index.to_string()) else {
                    return false;
                };
                if !match_pattern(item, field, frame) {
                    return false;
                }
            }
            true
        },
        | Pattern::Tensor { items, rest } => {
            let Value::Tensor(tensor) = value else {
                return false;
            };
            if tensor.rank() != 1 {
                return false;
            }
            let elements = tensor.elements();
            if elements.len() < items.len() {
                return false;
            }
            for (index, item) in items.iter().enumerate() {
                if !match_pattern(item, &elements[index], frame) {
                    return false;
                }
            }
            if let Some(slot) = rest {
                let remaining: Vec<Value> = elements[items.len()..].to_vec();
                let len = remaining.len();
                set(
                    frame,
                    *slot,
                    build_tensor(remaining, vec![len], tensor.dim.clone()),
                );
            }
            true
        },
        | Pattern::Lit(literal) => {
            match (literal, value) {
                | (PatLit::Int(expected), Value::Num(num)) => {
                    match &num.kind {
                        | NumKind::Int(actual) => actual == expected,
                        | NumKind::Exact(actual) => {
                            actual == &BigRational::from_integer(expected.clone())
                        },
                        | NumKind::Float(actual) => {
                            BigRational::from_float(*actual)
                                == Some(BigRational::from_integer(expected.clone()))
                        },
                        | _ => false,
                    }
                },
                | (PatLit::Str(expected), Value::Str(actual)) => actual.as_ref() == expected,
                | (PatLit::Bool(expected), Value::Bool(actual)) => expected == actual,
                | (PatLit::Unit, Value::Unit) => true,
                | _ => false,
            }
        },
    }
}
