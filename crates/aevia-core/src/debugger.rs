//! Source-level observation for hosts running the typed AST evaluator.

use crate::Interp;
use crate::Mode;
use crate::eval::Frame;
use crate::value::Scope;
use aevia_syntax::Span;
use std::rc::Rc;

/// Observes a serial interpreter without changing the program's values.
pub trait Debugger {
    /// Called before executing a source expression. A host can block here
    /// while a user inspects the stack and lexical environment.
    fn expression(
        &self,
        span: Span,
        stack: &[Frame],
        scope: &Scope,
    );

    /// Receives output that would otherwise go to the process console.
    fn output(
        &self,
        text: &str,
        newline: bool,
        stderr: bool,
    );
}

impl Interp {
    /// Selects the typed source evaluator and observes executions on this
    /// interpreter. A debug host controls its own cancellation and input.
    pub fn set_debugger(
        &mut self,
        debugger: Rc<dyn Debugger>,
    ) {
        self.mode = Mode::Ast;
        self.optimize_ir = false;
        self.replica = None;
        self.debugger = Some(debugger);
    }
}
