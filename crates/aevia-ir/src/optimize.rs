//! Conservative release optimizations over checked Core IR.
//!
//! Numeric arithmetic is deliberately left to the typed literal/constant
//! machinery: reassociating scientific computations can change their result.
//! These rewrites only remove control flow whose boolean condition is known,
//! without moving an effect past another. All surviving nodes keep their
//! source spans and type evidence, including value-return boundaries.

use crate::Atom;
use crate::Block;
use crate::DoStep;
use crate::Function;
use crate::Module;
use crate::Rhs;
use crate::Stmt;
use crate::Tail;
use aevia_syntax::ast::BinOp;
use aevia_syntax::ast::UnOp;
use std::collections::HashMap;
use std::rc::Rc;

/// Optimize all functions, initializers and instance methods in a module.
pub fn module(module: &mut Module) {
    for declaration in &mut module.fns {
        function(&mut declaration.function);
    }
    for declaration in &mut module.lets {
        block(&mut declaration.block);
    }
    for instance in &mut module.instances {
        for method in &mut instance.methods {
            function(&mut method.function);
        }
    }
}

/// Optimize a declaration about to be installed in an incremental session.
pub fn declaration(declaration: &mut crate::lower::LoweredDecl) {
    match declaration {
        | crate::lower::LoweredDecl::Fn(declaration) => function(&mut declaration.function),
        | crate::lower::LoweredDecl::Let(declaration) => block(&mut declaration.block),
        | crate::lower::LoweredDecl::Instance(instance) => {
            for method in &mut instance.methods {
                function(&mut method.function);
            }
        },
    }
}

fn function(function: &mut Rc<Function>) {
    // Lowering produces owned trees. A host may also share an IR node; leave
    // that node alone rather than changing another owner's program.
    if let Some(function) = Rc::get_mut(function) {
        block(&mut function.body);
    }
}

fn block(block: &mut Rc<Block>) {
    if let Some(block) = Rc::get_mut(block) {
        optimize_block(block, &mut BooleanFacts::default());
    }
}

#[derive(Default)]
struct BooleanFacts {
    values: HashMap<u32, bool>,
    changes: Vec<FactChange>,
    recording: bool,
}

enum FactChange {
    Slot(u32, Option<bool>),
    Cleared(HashMap<u32, bool>),
}

impl BooleanFacts {
    fn set(
        &mut self,
        slot: u32,
        value: Option<bool>,
    ) {
        let previous = match value {
            | Some(value) => self.values.insert(slot, value),
            | None => self.values.remove(&slot),
        };
        if self.recording && previous != value {
            self.changes.push(FactChange::Slot(slot, previous));
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "pattern bindings may discard facts that another arm still needs"
    )]
    fn clear(&mut self) {
        if self.recording && !self.values.is_empty() {
            self.changes
                .push(FactChange::Cleared(std::mem::take(&mut self.values)));
        } else {
            self.values.clear();
        }
    }

    /// A conditional arm can borrow its parent's facts. Only modifications
    /// are recorded and reversed before optimizing a sibling arm.
    #[allow(
        clippy::single_call_fn,
        reason = "one rollback scope encloses each conditional arm"
    )]
    fn optimize_arm(
        &mut self,
        block: &mut Block,
    ) {
        let checkpoint = self.changes.len();
        let recording = std::mem::replace(&mut self.recording, true);
        optimize_block(block, self);
        self.recording = false;
        while self.changes.len() > checkpoint {
            match self.changes.pop() {
                | Some(FactChange::Slot(slot, value)) => self.set(slot, value),
                | Some(FactChange::Cleared(values)) => self.values = values,
                | None => break,
            }
        }
        self.recording = recording;
    }
}

fn optimize_block(
    block: &mut Block,
    known: &mut BooleanFacts,
) {
    for statement in &mut block.stmts {
        match statement {
            | Stmt::Let(binding) => {
                rhs(&mut binding.rhs, &known.values);
                let value = match &binding.rhs {
                    | Rhs::Atom(atom) => boolean(atom, &known.values),
                    | _ => None,
                };
                known.set(binding.slot, value);
            },
            | Stmt::Bind { pattern, atom, .. } => {
                let value = boolean(atom, &known.values);
                // Pattern binding can populate several slots. Clearing is
                // conservative even for IR supplied directly by a host.
                known.clear();
                if let (crate::Pattern::Bind { slot, inner: None }, Some(value)) = (pattern, value)
                {
                    known.set(*slot, Some(value));
                }
            },
        }
    }
    match &mut block.tail {
        | Tail::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            if let Some(value) = boolean(cond, &known.values) {
                *cond = Atom::Bool(value);
                // Only the selected arm can execute. It occupies the parent
                // block's tail, so its facts have no later sibling to restore.
                let selected = if value {
                    then_block
                } else {
                    else_block
                };
                if let Some(selected) = Rc::get_mut(selected) {
                    optimize_block(selected, known);
                    block.stmts.append(&mut selected.stmts);
                    block.tail = std::mem::replace(
                        &mut selected.tail,
                        Tail::Return {
                            atom: Atom::Unit,
                            span: aevia_syntax::Span::default(),
                        },
                    );
                }
            } else {
                for child in [then_block, else_block] {
                    if let Some(child) = Rc::get_mut(child) {
                        known.optimize_arm(child);
                    }
                }
            }
        },
        | Tail::Match { arms, .. } => {
            for arm in arms {
                if let Some(guard) = &mut arm.guard {
                    self::block(guard);
                }
                self::block(&mut arm.body);
            }
        },
        | Tail::Return { .. } | Tail::Call(_) => {},
    }
}

fn boolean(
    atom: &Atom,
    known: &HashMap<u32, bool>,
) -> Option<bool> {
    match atom {
        | Atom::Bool(value) => Some(*value),
        | Atom::Local(slot) => known.get(slot).copied(),
        | _ => None,
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps right-hand-side rewriting separate from statement traversal"
)]
fn rhs(
    rhs: &mut Rhs,
    known: &HashMap<u32, bool>,
) {
    match rhs {
        | Rhs::Unary {
            op: UnOp::Not,
            operand,
        } => {
            if let Some(value) = boolean(operand, known) {
                *rhs = Rhs::Atom(Atom::Bool(!value));
            }
        },
        | Rhs::AndOr { op, left, right } => {
            if let Some(value) = boolean(left, known) {
                if matches!((*op, value), (BinOp::And, false) | (BinOp::Or, true)) {
                    *rhs = Rhs::Atom(Atom::Bool(value));
                    return;
                }
                *left = Atom::Bool(value);
            }
            block(right);
        },
        | Rhs::Block(inner) => block(inner),
        | Rhs::Closure(inner) | Rhs::RecClosure { function: inner, .. } => function(inner),
        | Rhs::Do(plan) => {
            if let Some(plan) = Rc::get_mut(plan) {
                for step in &mut plan.steps {
                    match step {
                        | DoStep::Let { value, .. }
                        | DoStep::Bind { value, .. }
                        | DoStep::Action { value, .. } => block(value),
                    }
                }
            }
        },
        | _ => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aevia_syntax::Span;

    fn returning(atom: Atom) -> Rc<Block> {
        Rc::new(Block {
            stmts: Vec::new(),
            tail: Tail::Return {
                atom,
                span: Span::default(),
            },
        })
    }

    fn conditional(
        cond: Atom,
        then_block: Rc<Block>,
        else_block: Rc<Block>,
    ) -> Tail {
        let span = Span::default();
        Tail::If {
            cond,
            cond_span: span,
            then_block,
            else_block,
            span,
        }
    }

    #[test]
    fn folds_booleans_and_propagates_them_into_branch_conditions() {
        let span = Span::default();
        let mut body = Rc::new(Block {
            stmts: vec![Stmt::Let(crate::Let {
                slot: 0,
                rhs: Rhs::Unary {
                    op: UnOp::Not,
                    operand: Atom::Bool(false),
                },
                span,
            })],
            tail: Tail::If {
                cond: Atom::Local(0),
                cond_span: span,
                then_block: returning(Atom::Str("selected".into())),
                else_block: returning(Atom::Str("unreachable".into())),
                span,
            },
        });
        block(&mut body);
        assert!(
            matches!(&body.tail, Tail::Return { atom: Atom::Str(text), .. } if text == "selected")
        );
        let Stmt::Let(binding) = &body.stmts[0] else {
            panic!("let")
        };
        assert!(matches!(binding.rhs, Rhs::Atom(Atom::Bool(true))));
    }

    #[test]
    fn short_circuit_folding_discards_only_the_unexecuted_operand() {
        for (op, value) in [(BinOp::And, false), (BinOp::Or, true)] {
            let mut expression = Rhs::AndOr {
                op,
                left: Atom::Bool(value),
                right: returning(Atom::Global("missing".into())),
            };
            rhs(&mut expression, &HashMap::new());
            assert!(matches!(expression, Rhs::Atom(Atom::Bool(result)) if result == value));
        }
        let mut dynamic = Rhs::AndOr {
            op: BinOp::And,
            left: Atom::Local(0),
            right: returning(Atom::Bool(true)),
        };
        rhs(&mut dynamic, &HashMap::new());
        assert!(matches!(dynamic, Rhs::AndOr { .. }));
    }

    #[test]
    fn effects_of_assignments_and_pattern_binds_stay_in_their_branch() {
        let span = Span::default();
        let select_slot = || {
            conditional(
                Atom::Local(0),
                returning(Atom::Str("true arm".into())),
                returning(Atom::Str("false arm".into())),
            )
        };
        let left = Rc::new(Block {
            stmts: vec![
                Stmt::Let(crate::Let {
                    slot: 0,
                    rhs: Rhs::Atom(Atom::Bool(false)),
                    span,
                }),
                Stmt::Bind {
                    pattern: crate::Pattern::Bind { slot: 1, inner: None },
                    atom: Atom::Bool(false),
                    span,
                },
            ],
            tail: select_slot(),
        });
        let right = Rc::new(Block {
            stmts: Vec::new(),
            tail: select_slot(),
        });
        let mut body = Rc::new(Block {
            stmts: vec![Stmt::Let(crate::Let {
                slot: 0,
                rhs: Rhs::Atom(Atom::Bool(true)),
                span,
            })],
            tail: conditional(Atom::Global("runtime condition".into()), left, right),
        });
        block(&mut body);
        let Tail::If {
            then_block,
            else_block,
            ..
        } = &body.tail
        else {
            panic!("top-level condition is dynamic");
        };
        assert!(matches!(
            then_block.tail,
            Tail::If {
                cond: Atom::Local(0),
                ..
            }
        ));
        assert!(
            matches!(&else_block.tail, Tail::Return { atom: Atom::Str(text), .. } if text == "true arm")
        );
    }
}
