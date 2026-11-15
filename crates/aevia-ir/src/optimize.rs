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
        optimize_block(block, &HashMap::new());
    }
}

fn optimize_block(
    block: &mut Block,
    inherited: &HashMap<u32, bool>,
) {
    let mut known = inherited.clone();
    for statement in &mut block.stmts {
        match statement {
            | Stmt::Let(binding) => {
                rhs(&mut binding.rhs, &known);
                let value = match &binding.rhs {
                    | Rhs::Atom(atom) => boolean(atom, &known),
                    | _ => None,
                };
                known.remove(&binding.slot);
                if let Some(value) = value {
                    known.insert(binding.slot, value);
                }
            },
            | Stmt::Bind { pattern, atom, .. } => {
                let value = boolean(atom, &known);
                // Pattern binding can populate several slots. Clearing is
                // conservative even for IR supplied directly by a host.
                known.clear();
                if let (crate::Pattern::Bind { slot, inner: None }, Some(value)) = (pattern, value)
                {
                    known.insert(*slot, value);
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
            if let Some(value) = boolean(cond, &known) {
                *cond = Atom::Bool(value);
            }
            for child in [then_block, else_block] {
                if let Some(child) = Rc::get_mut(child) {
                    optimize_block(child, &known);
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
    // No expression is evaluated, duplicated or discarded here: reading a
    // boolean atom cannot fail, and the unused arm never ran in the first place.
    if let Tail::If {
        cond: Atom::Bool(condition),
        then_block,
        else_block,
        ..
    } = &mut block.tail
    {
        let selected = if *condition {
            then_block
        } else {
            else_block
        };
        if let Some(selected) = Rc::get_mut(selected) {
            block.stmts.append(&mut selected.stmts);
            block.tail = std::mem::replace(
                &mut selected.tail,
                Tail::Return {
                    atom: Atom::Unit,
                    span: aevia_syntax::Span::default(),
                },
            );
        }
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

fn rhs(
    rhs: &mut Rhs,
    known: &HashMap<u32, bool>,
) {
    match rhs {
        | Rhs::Unary {
            op: UnOp::Not,
            operand,
        }
        | Rhs::AndOr { left: operand, .. } => {
            if let Some(value) = boolean(operand, known) {
                *operand = Atom::Bool(value);
            }
        },
        | _ => {},
    }
    match rhs {
        | Rhs::Block(inner) | Rhs::AndOr { right: inner, .. } => block(inner),
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
    match rhs {
        | Rhs::Unary {
            op: UnOp::Not,
            operand: Atom::Bool(value),
        } => {
            *rhs = Rhs::Atom(Atom::Bool(!*value));
        },
        | Rhs::AndOr {
            op: BinOp::And,
            left: Atom::Bool(false),
            ..
        } => {
            *rhs = Rhs::Atom(Atom::Bool(false));
        },
        | Rhs::AndOr {
            op: BinOp::Or,
            left: Atom::Bool(true),
            ..
        } => {
            *rhs = Rhs::Atom(Atom::Bool(true));
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
}
