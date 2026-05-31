//! Lowering from Aevia AST to `rssn-advanced` expression DAG. Phase 3.

use crate::ast::{BinOp, Expr, FunctionBody, Item, SourceFile, Spanned, Stmt, UnOp};
use crate::diagnostics::AeviaError;
use rssn_advanced::dag::builder::DagBuilder;
use rssn_advanced::dag::node::DagNodeId;
use std::collections::HashMap;

/// Traverses Aevia AST and constructs structurally deduplicated nodes in `DagBuilder`.
pub struct Lowerer {
    pub builder: DagBuilder,
    pub functions: HashMap<String, Item>,
}

impl Default for Lowerer {
    fn default() -> Self {
        Self::new()
    }
}

impl Lowerer {
    /// Creates a new `Lowerer` with a fresh `DagBuilder`.
    pub fn new() -> Self {
        Self {
            builder: DagBuilder::new(),
            functions: HashMap::new(),
        }
    }

    /// Registers top-level items so they can be referenced/inlined during lowering.
    pub fn register_items(&mut self, file: &SourceFile) {
        for item in &file.items {
            match &item.node {
                Item::Function { name, .. } => {
                    self.functions.insert(name.clone(), item.node.clone());
                }
                Item::CustomOp { name, .. } => {
                    self.functions.insert(name.clone(), item.node.clone());
                }
                _ => {}
            }
        }
    }

    /// Lowers a value/computational expression recursively.
    pub fn lower_expr(
        &mut self,
        expr: &Spanned<Expr>,
        env: &mut HashMap<String, DagNodeId>,
    ) -> Result<DagNodeId, AeviaError> {
        match &expr.node {
            Expr::Literal { value, .. } => {
                use rust_decimal::prelude::ToPrimitive;
                let val = value.to_f64().ok_or_else(|| {
                    AeviaError::message(format!("invalid float literal: {}", value))
                })?;
                Ok(self.builder.constant(val))
            }
            Expr::Variable(name) => {
                if let Some(&node_id) = env.get(name) {
                    Ok(node_id)
                } else {
                    // Default to creating a new variable node in the builder
                    Ok(self.builder.variable(name))
                }
            }
            Expr::BinaryOp { op, lhs, rhs } => {
                let left = self.lower_expr(lhs, env)?;
                let right = self.lower_expr(rhs, env)?;
                match op {
                    BinOp::Add => Ok(self.builder.add(left, right)),
                    BinOp::Sub => Ok(self.builder.sub(left, right)),
                    BinOp::Mul => Ok(self.builder.mul(left, right)),
                    BinOp::Div => Ok(self.builder.div(left, right)),
                    BinOp::Pow => Ok(self.builder.pow(left, right)),
                    BinOp::Mod => Ok(self.builder.modulo(left, right)),
                    BinOp::Lt => {
                        let fn_id = self.builder.intern_function("lt");
                        Ok(self.builder.function_call(fn_id, &[left, right]))
                    }
                    BinOp::Gt => {
                        let fn_id = self.builder.intern_function("gt");
                        Ok(self.builder.function_call(fn_id, &[left, right]))
                    }
                    BinOp::Eq => {
                        let fn_id = self.builder.intern_function("eq");
                        Ok(self.builder.function_call(fn_id, &[left, right]))
                    }
                }
            }
            Expr::UnaryOp { op, expr: inner } => {
                let val = self.lower_expr(inner, env)?;
                match op {
                    UnOp::Neg => Ok(self.builder.neg(val)),
                }
            }
            Expr::Call { func, args } => {
                if let Some(Item::Function { params, body, .. }) = self.functions.get(func).cloned() {
                    if args.len() != params.len() {
                        return Err(AeviaError::message(format!(
                            "arity mismatch: expected {} arguments, got {}",
                            params.len(),
                            args.len()
                        )));
                    }
                    let mut local_env = env.clone();
                    for (param, arg) in params.iter().zip(args.iter()) {
                        let arg_node = self.lower_expr(arg, env)?;
                        local_env.insert(param.name.clone(), arg_node);
                    }
                    match body {
                        FunctionBody::Expression(ref body_expr) => {
                            return self.lower_expr(body_expr, &mut local_env);
                        }
                        FunctionBody::Block(ref stmts) => {
                            return self.lower_block(stmts, &mut local_env);
                        }
                    }
                }

                let mut arg_nodes = Vec::new();
                for arg in args {
                    arg_nodes.push(self.lower_expr(arg, env)?);
                }
                let fn_id = self.builder.intern_function(func);
                Ok(self.builder.function_call(fn_id, &arg_nodes))
            }
            Expr::Block(stmts) => self.lower_block(stmts, env),
            Expr::If { cond, then_branch, else_branch } => {
                let cond_node = self.lower_expr(cond, env)?;
                let then_node = self.lower_expr(then_branch, env)?;
                let else_node = if let Some(eb) = else_branch {
                    self.lower_expr(eb, env)?
                } else {
                    self.builder.constant(0.0)
                };
                Ok(self.builder.if_else(cond_node, then_node, else_node))
            }
            Expr::Loop { body } => {
                let init = self.builder.constant(0.0);
                let limit = self.builder.constant(1000.0);
                let step = self.builder.constant(1.0);
                let mut body_env = env.clone();
                let body_node = self.lower_block(body, &mut body_env)?;
                Ok(self.builder.for_loop(init, limit, step, body_node))
            }
            Expr::UnsafeTransmute { expr: inner, .. } => self.lower_expr(inner, env),
        }
    }

    /// Lowers a block of statements and returns the value of the last statement.
    pub fn lower_block(
        &mut self,
        stmts: &[Spanned<Stmt>],
        env: &mut HashMap<String, DagNodeId>,
    ) -> Result<DagNodeId, AeviaError> {
        let mut last_node = self.builder.constant(0.0);
        for stmt in stmts {
            match &stmt.node {
                Stmt::Let { name, init, .. } => {
                    let val_node = self.lower_expr(init, env)?;
                    env.insert(name.clone(), val_node);
                    last_node = val_node;
                }
                Stmt::Assign { target, value } => {
                    let val_node = self.lower_expr(value, env)?;
                    env.insert(target.clone(), val_node);
                    last_node = val_node;
                }
                Stmt::Expr(expr) => {
                    last_node = self.lower_expr(expr, env)?;
                }
                Stmt::Break => {
                    last_node = self.builder.constant(0.0);
                }
            }
        }
        Ok(last_node)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::items::parse_source;
    use rssn_advanced::dag::symbol::{OpKind, SymbolKind};

    #[test]
    fn test_lowering_simple_expr() {
        let file = parse_source("fn main(x: m) := x * 2.5;").unwrap();
        let mut lowerer = Lowerer::new();
        lowerer.register_items(&file);

        let mut env = HashMap::new();
        // Intern variable 'x' first so it's mapped in the lowering environment
        let x_id = lowerer.builder.variable("x");
        env.insert("x".to_string(), x_id);

        let item = &file.items[0];
        if let Item::Function { body, .. } = &item.node {
            if let FunctionBody::Expression(expr) = body {
                let root = lowerer.lower_expr(expr, &mut env).unwrap();
                let node = lowerer.builder.arena().get(root).unwrap();
                // Root node should be Operator(Mul)
                assert_eq!(node.kind, SymbolKind::Operator(OpKind::Mul));
            } else {
                panic!("expected expression body");
            }
        } else {
            panic!("expected function");
        }
    }

    #[test]
    fn test_lowering_let_bindings() {
        let file = parse_source("fn main(x: m) { let y = x * 2.0; let z = y + 1.0; }").unwrap();
        let mut lowerer = Lowerer::new();
        lowerer.register_items(&file);

        let mut env = HashMap::new();
        let x_id = lowerer.builder.variable("x");
        env.insert("x".to_string(), x_id);

        let item = &file.items[0];
        if let Item::Function { body, .. } = &item.node {
            if let FunctionBody::Block(stmts) = body {
                let root = lowerer.lower_block(stmts, &mut env).unwrap();
                let node = lowerer.builder.arena().get(root).unwrap();
                // Root node should be Operator(Add) since z = y + 1.0
                assert_eq!(node.kind, SymbolKind::Operator(OpKind::Add));
            } else {
                panic!("expected block body");
            }
        } else {
            panic!("expected function");
        }
    }
}

