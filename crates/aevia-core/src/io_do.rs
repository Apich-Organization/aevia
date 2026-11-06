//! Immutable do programs with replay-local cursors. No suffix cloning or
//! implementation closure can change source evaluation order or catch `?`.
use crate::EvalError;
use crate::Interp;
use crate::eval::Frame;
use crate::value::IoAction;
use crate::value::IrEnv;
use crate::value::Scope;
use crate::value::Value;
use aevia_syntax::Span;
use aevia_syntax::ast::DoStmt;
use aevia_types::check::TryBoundary;
use std::rc::Rc;

#[derive(Debug)]
enum Code {
    Ast(Rc<[DoStmt]>),
    Ir(Rc<aevia_ir::DoPlan>),
}
/// A deferred source IO program. Execution state is created afresh by run_io,
/// so running the same action twice never shares bindings or an instruction cursor.
#[derive(Debug)]
pub struct DoBlock {
    code: Code,
    scope: Rc<Scope>,
    env: Option<Rc<IrEnv>>,
    span: Span,
    origin: Frame,
}
impl DoBlock {
    pub(crate) fn ast(
        interp: &Interp,
        statements: &[DoStmt],
        scope: &Rc<Scope>,
        span: Span,
    ) -> Rc<Self> {
        Rc::new(Self {
            code: Code::Ast(statements.to_vec().into()),
            scope: Scope::child_with_types(scope, interp.bindings.clone()),
            env: None,
            span,
            origin: origin(interp, span),
        })
    }

    pub(crate) fn ir(
        interp: &Interp,
        plan: Rc<aevia_ir::DoPlan>,
        env: Rc<IrEnv>,
    ) -> Rc<Self> {
        let span = plan.span;
        Rc::new(Self {
            code: Code::Ir(plan),
            scope: env.scope.clone().expect("captured do scope"),
            env: Some(env),
            span,
            origin: origin(interp, span),
        })
    }

    pub(crate) fn source_span(&self) -> Span {
        self.span
    }

    pub(crate) fn cursor(self: &Rc<Self>) -> DoCursor {
        let slots = match &self.code {
            | Code::Ast(_) => 0,
            | Code::Ir(plan) => plan.nslots as usize,
        };
        DoCursor {
            block: self.clone(),
            scope: Scope::child(&self.scope),
            slots: vec![Value::Unit; slots],
            index: 0,
            awaiting: None,
        }
    }
}
fn origin(
    interp: &Interp,
    span: Span,
) -> Frame {
    Frame {
        function: interp
            .frames
            .last()
            .map_or_else(|| Rc::from("<do>"), |frame| frame.function.clone()),
        span,
    }
}
#[derive(Debug)]
pub(crate) struct DoCursor {
    block: Rc<DoBlock>,
    scope: Rc<Scope>,
    slots: Vec<Value>,
    index: usize,
    awaiting: Option<usize>,
}
pub(crate) enum Step {
    Continue,
    Action {
        action: Rc<IoAction>,
        resume: bool,
        span: Span,
    },
    Complete(Value),
}
impl DoCursor {
    pub(crate) fn span(&self) -> Span {
        let index = self.awaiting.unwrap_or(self.index);
        match &self.block.code {
            | Code::Ast(statements) => {
                statements.get(index).map_or(self.block.span, |statement| {
                    match statement {
                        | DoStmt::Let { span, .. } | DoStmt::Bind { span, .. } => *span,
                        | DoStmt::Expr(expr) => expr.span,
                    }
                })
            },
            | Code::Ir(plan) => {
                plan.steps.get(index).map_or(plan.span, |step| {
                    match step {
                        | aevia_ir::DoStep::Let { span, .. }
                        | aevia_ir::DoStep::Bind { span, .. }
                        | aevia_ir::DoStep::Action { span, .. } => *span,
                    }
                })
            },
        }
    }

    pub(crate) fn step(
        &mut self,
        interp: &mut Interp,
        last: &Value,
    ) -> Result<Step, EvalError> {
        let previous = interp.enter_scope(&self.scope)?;
        let frames = interp.frames.len();
        interp.frames.push(self.block.origin.clone());
        let outcome = self.step_inner(interp, last).map_err(|error| {
            if error.try_target.is_none() {
                error.with_stack(interp.frames.clone())
            } else {
                error
            }
        });
        interp.frames.truncate(frames);
        interp.restore_context(previous);
        match outcome {
            | Err(mut error) if error.try_target == Some(TryBoundary::Do(self.block.span)) => {
                error
                    .payload
                    .take()
                    .map(Step::Complete)
                    .ok_or_else(|| EvalError::new("E0809", "missing do return value"))
            },
            | other => other,
        }
    }

    fn step_inner(
        &mut self,
        interp: &mut Interp,
        last: &Value,
    ) -> Result<Step, EvalError> {
        let block = self.block.clone();
        match &block.code {
            | Code::Ast(statements) => {
                if let Some(index) = self.awaiting.take() {
                    let DoStmt::Bind { pattern, .. } = &statements[index] else {
                        unreachable!()
                    };
                    let next = Scope::child(&self.scope);
                    interp.bind_pattern_entry(pattern, last, &next)?;
                    self.scope = next;
                }
                let Some(statement) = statements.get(self.index) else {
                    return Ok(Step::Complete(Value::Unit));
                };
                let index = self.index;
                self.index += 1;
                match statement {
                    | DoStmt::Let { pattern, value, .. } => {
                        let value = interp.eval(value, &self.scope)?;
                        let next = Scope::child(&self.scope);
                        interp.bind_pattern_entry(pattern, &value, &next)?;
                        self.scope = next;
                        Ok(Step::Continue)
                    },
                    | DoStmt::Bind { value, .. } | DoStmt::Expr(value) => {
                        let action = interp.eval(value, &self.scope)?;
                        let Value::Io(action) = action else {
                            return Err(EvalError::new(
                                "E0701",
                                "do statement did not produce an IO action",
                            )
                            .at(value.span));
                        };
                        let binding = matches!(statement, DoStmt::Bind { .. });
                        if binding {
                            self.awaiting = Some(index);
                        }
                        Ok(Step::Action {
                            action,
                            resume: binding || self.index < statements.len(),
                            span: value.span,
                        })
                    },
                }
            },
            | Code::Ir(plan) => {
                let env = block.env.as_ref().expect("IR do environment");
                if let Some(index) = self.awaiting.take() {
                    let aevia_ir::DoStep::Bind { pattern, span, .. } = &plan.steps[index] else {
                        unreachable!()
                    };
                    crate::eval_ir::bind_do_pattern(pattern, last, &mut self.slots, *span)?;
                }
                let Some(step) = plan.steps.get(self.index) else {
                    return Ok(Step::Complete(Value::Unit));
                };
                let index = self.index;
                self.index += 1;
                let (value, span) = match step {
                    | aevia_ir::DoStep::Let { value, span, .. }
                    | aevia_ir::DoStep::Bind { value, span, .. }
                    | aevia_ir::DoStep::Action { value, span } => (value, *span),
                };
                let value =
                    crate::eval_ir::eval_do_expression(interp, value, &mut self.slots, env)?;
                if let aevia_ir::DoStep::Let { pattern, .. } = step {
                    crate::eval_ir::bind_do_pattern(pattern, &value, &mut self.slots, span)?;
                    return Ok(Step::Continue);
                }
                let Value::Io(action) = value else {
                    return Err(EvalError::new(
                        "E0701",
                        "do statement did not produce an IO action",
                    )
                    .at(span));
                };
                let binding = matches!(step, aevia_ir::DoStep::Bind { .. });
                if binding {
                    self.awaiting = Some(index);
                }
                Ok(Step::Action {
                    action,
                    resume: binding || self.index < plan.steps.len(),
                    span,
                })
            },
        }
    }
}
