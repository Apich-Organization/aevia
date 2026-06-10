//! Dimensional type-checker pass.
//!
//! Walks a parsed `SourceFile` AST and:
//!   1. Registers all `type` aliases and `struct` fields into the environment.
//!   2. Registers all top-level function signatures (params + return type).
//!   3. Checks every expression for dimensional and shape consistency.
//!   4. Verifies `let` annotation vs. inferred type.
//!   5. Verifies function return expression vs. declared return type.

use crate::ast::{
    BinOp, Expr, FunctionBody, Item, MatchArm, Pattern, SourceFile, Spanned, Stmt, UnOp,
};
use rust_decimal::prelude::ToPrimitive;
use crate::modules::ImportBindings;
use crate::types::{
    dim::{resolve, tensor_shape, DimVector, PhysicalType},
    env::TypeEnv,
    error::TypeError,
};
use crate::ast::DimExpr;
use miette::SourceSpan;

use std::collections::HashMap;

/// Result of checking a full source file.
pub struct CheckResult {
    pub errors: Vec<TypeError>,
}

impl CheckResult {
    #[must_use]
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

// ── Public entry points ────────────────────────────────────────────────────────

pub fn check(file: &SourceFile) -> CheckResult {
    check_with_imports(file, &ImportBindings::default())
}

pub fn check_with_imports(file: &SourceFile, imports: &ImportBindings) -> CheckResult {
    let mut ctx = Ctx { env: TypeEnv::new(), errors: Vec::new(), struct_defs: HashMap::new() };
    crate::modules::apply_imports(&mut ctx.env, imports);
    for item in &file.items {
        ctx.register_item(item);
    }
    for item in &file.items {
        ctx.check_item(item);
    }
    CheckResult { errors: ctx.errors }
}

// ── Dimension resolution helpers ───────────────────────────────────────────────

fn resolve_with_env(span: &Spanned<crate::ast::DimExpr>, env: &TypeEnv) -> Option<DimVector> {
    if let Some(dim) = resolve(span) {
        return Some(dim);
    }
    if let DimExpr::Base(name) = &span.node {
        return env.lookup_alias(name);
    }
    resolve_compound_with_env(&span.node, env)
}

fn resolve_compound_with_env(expr: &DimExpr, env: &TypeEnv) -> Option<DimVector> {
    match expr {
        DimExpr::Base(name) => {
            crate::types::dim::base_unit(name).or_else(|| env.lookup_alias(name))
        }
        DimExpr::Mul(lhs, rhs) => Some(
            resolve_compound_with_env(&lhs.node, env)?
                .mul(resolve_compound_with_env(&rhs.node, env)?),
        ),
        DimExpr::Div(lhs, rhs) => Some(
            resolve_compound_with_env(&lhs.node, env)?
                .div(resolve_compound_with_env(&rhs.node, env)?),
        ),
        DimExpr::Power(base, exp) => {
            Some(resolve_compound_with_env(&base.node, env)?.pow(*exp))
        }
        DimExpr::Tensor { base, .. } => resolve_compound_with_env(&base.node, env),
    }
}

/// Resolve a DimExpr to a full PhysicalType (dim + optional shape).
fn resolve_phys(span: &Spanned<DimExpr>, env: &TypeEnv) -> Option<PhysicalType> {
    let dim = resolve_with_env(span, env)?;
    let shape = tensor_shape(span);
    Some(PhysicalType { dim, shape, struct_name: None })
}

// ── Internal display helpers ───────────────────────────────────────────────────

fn shape_display(shape: &Option<Vec<usize>>) -> String {
    match shape {
        None => "scalar".to_string(),
        Some(s) => {
            let parts: Vec<String> = s.iter().map(|n| n.to_string()).collect();
            format!("[{}]", parts.join(", "))
        }
    }
}

// ── Checker context ────────────────────────────────────────────────────────────

struct Ctx {
    env: TypeEnv,
    errors: Vec<TypeError>,
    /// Struct definitions: struct name -> list of (field_name, field_dim).
    struct_defs: HashMap<String, Vec<(String, DimVector)>>,
}

impl Ctx {
    // ── Registration pass ─────────────────────────────────────────────────

    fn register_item(&mut self, item: &Spanned<Item>) {
        match &item.node {
            Item::TypeAlias { name, dimension_expr, .. } => {
                if let Some(dim) = resolve_with_env(dimension_expr, &self.env) {
                    self.env.register_alias(name.clone(), dim);
                } else {
                    self.errors.push(TypeError::unknown_unit(
                        dimension_expr.span,
                        &format!("{name} alias RHS"),
                    ));
                }
            }
            Item::Function { name, params, return_type, .. } => {
                if let Some(ret_span) = return_type {
                    if let Some(dim) = resolve_with_env(ret_span, &self.env) {
                        self.env.register_fn(name.clone(), dim);
                    }
                } else {
                    self.env.register_fn(name.clone(), DimVector::DIMENSIONLESS);
                }
                for param in params {
                    if resolve_with_env(&param.dim, &self.env).is_none() {
                        self.errors.push(TypeError::unknown_unit(param.dim.span, &param.name));
                    }
                }
            }
            Item::Struct { name, fields, .. } => {
                let mut field_dims = Vec::new();
                for field in fields {
                    if let Some(dim) = resolve_with_env(&field.dim, &self.env) {
                        field_dims.push((field.name.clone(), dim));
                    } else {
                        self.errors.push(TypeError::unknown_unit(field.dim.span, &field.name));
                    }
                }
                self.struct_defs.insert(name.clone(), field_dims);
            }
            Item::CustomOp { .. }
            | Item::Use { .. }
            | Item::Module { .. }
            | Item::MacroDef { .. }
            | Item::MacroCall { .. } => {}
            Item::Const { name, dim, value, .. } => {
                // Register the const's declared type (or infer from value).
                let declared_dim = dim.as_ref().and_then(|d| resolve_with_env(d, &self.env));
                let inferred = self.infer_expr(value);
                match (declared_dim, inferred) {
                    (Some(ddim), Some(inferred_pt)) => {
                        if ddim != inferred_pt.dim {
                            self.errors.push(TypeError::annotation_conflict(
                                value.span,
                                &ddim.to_string(),
                                &inferred_pt.dim.to_string(),
                            ));
                        } else {
                            self.env.define(name.clone(), PhysicalType { dim: ddim, shape: inferred_pt.shape, struct_name: None });
                        }
                    }
                    (Some(ddim), None) => {
                        self.env.define(name.clone(), PhysicalType::scalar(ddim));
                    }
                    (None, Some(pt)) => {
                        self.env.define(name.clone(), pt);
                    }
                    (None, None) => {}
                }
            }
        }
    }

    // ── Check pass ────────────────────────────────────────────────────────

    fn check_item(&mut self, item: &Spanned<Item>) {
        match &item.node {
            Item::Function { params, return_type, body, .. } => {
                self.env.push_scope();

                for param in params {
                    if let Some(pt) = resolve_phys(&param.dim, &self.env) {
                        self.env.define(param.name.clone(), pt);
                    }
                }

                let declared_return = return_type
                    .as_ref()
                    .and_then(|rt| resolve_with_env(rt, &self.env));

                match body {
                    FunctionBody::Expression(expr) => {
                        if let Some(inferred) = self.infer_expr(expr) {
                            if let Some(declared) = declared_return {
                                if declared != inferred.dim {
                                    self.errors.push(TypeError::return_type_mismatch(
                                        expr.span,
                                        &declared.to_string(),
                                        &inferred.to_string(),
                                    ));
                                }
                            }
                        }
                    }
                    FunctionBody::Block(stmts) => {
                        self.check_block(stmts, declared_return, item.span);
                    }
                }

                self.env.pop_scope();
            }
            Item::CustomOp { input_dim, output_dim, .. } => {
                if resolve_with_env(input_dim, &self.env).is_none() {
                    self.errors.push(TypeError::unknown_unit(input_dim.span, "op input"));
                }
                if resolve_with_env(output_dim, &self.env).is_none() {
                    self.errors.push(TypeError::unknown_unit(output_dim.span, "op output"));
                }
            }
            Item::TypeAlias { .. }
            | Item::Struct { .. }
            | Item::Use { .. }
            | Item::Module { .. }
            | Item::MacroDef { .. }
            | Item::MacroCall { .. }
            | Item::Const { .. } => {}
        }
    }

    fn check_block(
        &mut self,
        stmts: &[Spanned<Stmt>],
        _expected_return: Option<DimVector>,
        _fn_span: SourceSpan,
    ) {
        for stmt in stmts {
            self.check_stmt(stmt);
        }
    }

    fn check_stmt(&mut self, stmt: &Spanned<Stmt>) {
        match &stmt.node {
            Stmt::Let { name, declared_type, init, .. } => {
                let inferred = self.infer_expr(init);

                if let Some(declared_span) = declared_type {
                    match (resolve_with_env(declared_span, &self.env), inferred) {
                        (Some(declared_dim), Some(ref inferred_pt)) => {
                            if declared_dim != inferred_pt.dim {
                                self.errors.push(TypeError::annotation_conflict(
                                    init.span,
                                    &declared_dim.to_string(),
                                    &inferred_pt.dim.to_string(),
                                ));
                            } else {
                                let declared_shape = tensor_shape(declared_span);
                                // Shape annotation takes priority; fall back to inferred shape.
                                let shape = declared_shape.or_else(|| inferred_pt.shape.clone());
                                self.env.define(name.clone(), PhysicalType { dim: declared_dim, shape, struct_name: None });
                            }
                        }
                        (None, _) => {
                            self.errors.push(TypeError::unknown_unit(declared_span.span, name));
                        }
                        (Some(declared_dim), None) => {
                            let shape = tensor_shape(declared_span);
                            self.env.define(name.clone(), PhysicalType { dim: declared_dim, shape, struct_name: None });
                        }
                    }
                } else if let Some(pt) = inferred {
                    self.env.define(name.clone(), pt);
                }
            }
            Stmt::Assign { target, value } => {
                let expected = self.env.lookup(target);
                if let (Some(exp), Some(got)) = (expected, self.infer_expr(value)) {
                    if exp.dim != got.dim {
                        self.errors.push(TypeError::dimension_mismatch(
                            value.span,
                            &got.dim.to_string(),
                            &exp.dim.to_string(),
                        ));
                    } else if exp.shape.is_some() && exp.shape != got.shape {
                        self.errors.push(TypeError::shape_mismatch(
                            value.span,
                            &shape_display(&got.shape),
                            &shape_display(&exp.shape),
                        ));
                    }
                }
            }
            Stmt::Expr(expr) => {
                self.infer_expr(expr);
            }
            Stmt::Break | Stmt::Continue => {}
        }
    }

    // ── Expression inference ──────────────────────────────────────────────

    fn infer_expr(&mut self, expr: &Spanned<Expr>) -> Option<PhysicalType> {
        match &expr.node {
            Expr::Literal { suffix, .. } => {
                if let Some(unit_span) = suffix {
                    match resolve_with_env(unit_span, &self.env) {
                        Some(dim) => {
                            let shape = tensor_shape(unit_span);
                            Some(PhysicalType { dim, shape, struct_name: None })
                        }
                        None => {
                            self.errors.push(TypeError::unknown_unit(
                                unit_span.span,
                                "literal suffix",
                            ));
                            None
                        }
                    }
                } else {
                    Some(PhysicalType::scalar(DimVector::DIMENSIONLESS))
                }
            }

            Expr::Variable(name) => self.env.lookup(name),

            Expr::BinaryOp { op, lhs, rhs } => {
                let l = self.infer_expr(lhs);
                let r = self.infer_expr(rhs);
                self.check_binop(*op, l, r, &rhs.node, expr.span)
            }

            Expr::UnaryOp { op: UnOp::Neg, expr: inner } => self.infer_expr(inner),

            Expr::Call { func, args } => {
                for arg in args {
                    self.infer_expr(arg);
                }
                self.env.lookup_fn(func).map(PhysicalType::scalar)
            }

            Expr::If { cond, then_branch, else_branch } => {
                self.infer_expr(cond);
                let then_pt = self.infer_expr(then_branch);
                if let Some(else_expr) = else_branch {
                    let else_pt = self.infer_expr(else_expr);
                    match (then_pt, else_pt) {
                        (Some(t), Some(e)) if t.dim != e.dim => {
                            self.errors.push(TypeError::dimension_mismatch(
                                else_expr.span,
                                &e.dim.to_string(),
                                &t.dim.to_string(),
                            ));
                            None
                        }
                        (t, _) => t,
                    }
                } else {
                    then_pt
                }
            }

            Expr::Block(stmts) => {
                self.env.push_scope();
                let mut last: Option<PhysicalType> = None;
                for stmt in stmts {
                    if let Stmt::Expr(inner) = &stmt.node {
                        last = self.infer_expr(inner);
                    } else {
                        self.check_stmt(stmt);
                    }
                }
                self.env.pop_scope();
                last
            }

            Expr::Loop { body } => {
                self.env.push_scope();
                for stmt in body {
                    self.check_stmt(stmt);
                }
                self.env.pop_scope();
                Some(PhysicalType::scalar(DimVector::DIMENSIONLESS))
            }

            Expr::While { cond, body } => {
                self.infer_expr(cond);
                self.env.push_scope();
                for stmt in body {
                    self.check_stmt(stmt);
                }
                self.env.pop_scope();
                Some(PhysicalType::scalar(DimVector::DIMENSIONLESS))
            }

            Expr::For { var, start, end, body } => {
                let start_pt = self.infer_expr(start);
                let end_pt = self.infer_expr(end);
                if let (Some(s), Some(e)) = (&start_pt, &end_pt) {
                    if !s.dim.addable_with(e.dim) {
                        self.errors.push(TypeError::dimension_mismatch(
                            end.span,
                            &e.dim.to_string(),
                            &s.dim.to_string(),
                        ));
                    }
                }
                self.env.push_scope();
                self.env.define(var.clone(), PhysicalType::scalar(DimVector::DIMENSIONLESS));
                for stmt in body {
                    self.check_stmt(stmt);
                }
                self.env.pop_scope();
                Some(PhysicalType::scalar(DimVector::DIMENSIONLESS))
            }

            Expr::Match { scrutinee, arms } => {
                let scrutinee_pt = self.infer_expr(scrutinee);
                let mut result_pt: Option<PhysicalType> = None;
                for MatchArm { pattern, body } in arms {
                    self.env.push_scope();
                    match pattern {
                        Pattern::Binding { name, type_guard } => {
                            let bound_pt = if let Some(guard_span) = type_guard {
                                let guard_pt = resolve_phys(guard_span, &self.env);
                                if let (Some(sd), Some(gd)) = (&scrutinee_pt, &guard_pt) {
                                    if sd.dim != gd.dim {
                                        self.errors.push(TypeError::annotation_conflict(
                                            guard_span.span,
                                            &gd.dim.to_string(),
                                            &sd.dim.to_string(),
                                        ));
                                    }
                                }
                                guard_pt.or_else(|| scrutinee_pt.clone())
                            } else {
                                scrutinee_pt.clone()
                            };
                            if let Some(pt) = bound_pt {
                                self.env.define(name.clone(), pt);
                            }
                        }
                        Pattern::Wildcard | Pattern::Literal(_) => {}
                    }
                    let arm_pt = self.infer_expr(body);
                    self.env.pop_scope();
                    match (&result_pt, &arm_pt) {
                        (Some(rd), Some(ad)) if rd.dim != ad.dim => {
                            self.errors.push(TypeError::dimension_mismatch(
                                body.span,
                                &ad.dim.to_string(),
                                &rd.dim.to_string(),
                            ));
                        }
                        (None, _) => result_pt = arm_pt,
                        _ => {}
                    }
                }
                result_pt
            }

            Expr::UnsafeTransmute { expr: inner, assume_unit, .. } => {
                let _ = self.infer_expr(inner);
                if let Some(unit_span) = assume_unit {
                    resolve_with_env(unit_span, &self.env).map(PhysicalType::scalar)
                } else {
                    Some(PhysicalType::scalar(DimVector::DIMENSIONLESS))
                }
            }

            Expr::MacroCall { .. } => {
                // Macro calls must be expanded before type-checking via the expansion pass.
                Some(PhysicalType::scalar(DimVector::DIMENSIONLESS))
            }

            Expr::Print { expr: inner } => {
                // print(expr) — type-check the inner expression, return dimensionless.
                let _ = self.infer_expr(inner);
                Some(PhysicalType::scalar(DimVector::DIMENSIONLESS))
            }

            Expr::Log { .. } => {
                // log("message") — always dimensionless.
                Some(PhysicalType::scalar(DimVector::DIMENSIONLESS))
            }

            Expr::StructLit { name, fields } => {
                // Look up struct definition and verify field dimensions.
                if let Some(def_fields) = self.struct_defs.get(name).cloned() {
                    for (field_name, field_expr) in fields {
                        let inferred = self.infer_expr(field_expr);
                        if let Some(expected_dim) = def_fields.iter().find(|(n, _)| n == field_name).map(|(_, d)| d) {
                            if let Some(ref pt) = inferred {
                                if pt.dim != *expected_dim {
                                    self.errors.push(TypeError::dimension_mismatch(
                                        field_expr.span,
                                        &pt.dim.to_string(),
                                        &expected_dim.to_string(),
                                    ));
                                }
                            }
                        } else {
                            self.errors.push(TypeError::new(
                                crate::types::error::TypeErrorCode::UnknownUnit,
                                format!("struct `{name}` has no field `{field_name}`"),
                                field_expr.span,
                            ));
                        }
                    }
                    Some(PhysicalType::struct_type(name.clone()))
                } else {
                    // Unknown struct — still infer field expressions for error reporting.
                    for (_, field_expr) in fields {
                        self.infer_expr(field_expr);
                    }
                    Some(PhysicalType::struct_type(name.clone()))
                }
            }

            Expr::FieldAccess { expr: inner, field } => {
                let inner_pt = self.infer_expr(inner);
                if let Some(pt) = inner_pt {
                    if let Some(struct_name) = &pt.struct_name {
                        if let Some(def_fields) = self.struct_defs.get(struct_name) {
                            if let Some((_, dim)) = def_fields.iter().find(|(n, _)| n == field) {
                                return Some(PhysicalType::scalar(*dim));
                            }
                        }
                        self.errors.push(TypeError::new(
                            crate::types::error::TypeErrorCode::UnknownUnit,
                            format!("struct `{struct_name}` has no field `{field}`"),
                            expr.span,
                        ));
                        None
                    } else {
                        self.errors.push(TypeError::new(
                            crate::types::error::TypeErrorCode::DimensionMismatch,
                            "field access requires a struct type",
                            expr.span,
                        ));
                        None
                    }
                } else {
                    None
                }
            }
        }
    }

    // ── Binary operation checking ─────────────────────────────────────────

    fn check_binop(
        &mut self,
        op: BinOp,
        lhs: Option<PhysicalType>,
        rhs: Option<PhysicalType>,
        rhs_expr: &Expr,
        span: SourceSpan,
    ) -> Option<PhysicalType> {
        match op {
            BinOp::Add | BinOp::Sub => {
                match (lhs, rhs) {
                    (Some(l), Some(r)) => {
                        if !l.dim.addable_with(r.dim) {
                            self.errors.push(TypeError::dimension_mismatch(
                                span,
                                &r.dim.to_string(),
                                &l.dim.to_string(),
                            ));
                            None
                        } else if l.shape != r.shape {
                            self.errors.push(TypeError::shape_mismatch(
                                span,
                                &shape_display(&r.shape),
                                &shape_display(&l.shape),
                            ));
                            None
                        } else {
                            Some(l)
                        }
                    }
                    (Some(l), None) => Some(l),
                    (None, Some(r)) => Some(r),
                    (None, None) => None,
                }
            }

            BinOp::Mul => {
                match (lhs, rhs) {
                    (Some(l), Some(r)) => {
                        let result_dim = l.dim.mul(r.dim);
                        let result_shape = self.mul_shapes(&l.shape, &r.shape, span);
                        Some(PhysicalType { dim: result_dim, shape: result_shape, struct_name: None })
                    }
                    (Some(l), None) | (None, Some(l)) => Some(l),
                    (None, None) => None,
                }
            }

            BinOp::Div => {
                match (lhs, rhs) {
                    (Some(l), Some(r)) => {
                        let result_dim = l.dim.div(r.dim);
                        // tensor / scalar → tensor; tensor / tensor → scalar
                        let result_shape = match (&l.shape, &r.shape) {
                            (Some(s), None) => Some(s.clone()),
                            _ => None,
                        };
                        Some(PhysicalType { dim: result_dim, shape: result_shape, struct_name: None })
                    }
                    (Some(l), None) => Some(l),
                    (None, _) => None,
                }
            }

            BinOp::Mod => lhs,

            BinOp::Pow => match (lhs, rhs_expr) {
                (Some(base), Expr::Literal { value, suffix: None }) => {
                    if let Some(exp) = value.to_i64() {
                        if exp >= 0 {
                            Some(PhysicalType { dim: base.dim.pow(exp as i32), shape: base.shape, struct_name: None })
                        } else {
                            self.errors.push(TypeError::new(
                                crate::types::error::TypeErrorCode::DimensionMismatch,
                                "negative exponent is not supported in dimensional analysis",
                                span,
                            ));
                            None
                        }
                    } else {
                        self.errors.push(TypeError::new(
                            crate::types::error::TypeErrorCode::DimensionMismatch,
                            "exponent must be an integer literal",
                            span,
                        ));
                        Some(base)
                    }
                }
                (Some(_), Expr::Literal { suffix: Some(_), .. }) => {
                    self.errors.push(TypeError::new(
                        crate::types::error::TypeErrorCode::UnexpectedDimensionless,
                        "exponent must be dimensionless",
                        span,
                    ));
                    None
                }
                (Some(base), _) => {
                    self.errors.push(TypeError::new(
                        crate::types::error::TypeErrorCode::DimensionMismatch,
                        "exponent must be a dimensionless integer literal",
                        span,
                    ));
                    Some(base)
                }
                (None, _) => None,
            },

            BinOp::Lt | BinOp::Gt | BinOp::Eq => {
                if let (Some(l), Some(r)) = (&lhs, &rhs) {
                    if !l.dim.addable_with(r.dim) {
                        self.errors.push(TypeError::dimension_mismatch(
                            span,
                            &r.dim.to_string(),
                            &l.dim.to_string(),
                        ));
                    }
                    if l.shape != r.shape {
                        self.errors.push(TypeError::shape_mismatch(
                            span,
                            &shape_display(&r.shape),
                            &shape_display(&l.shape),
                        ));
                    }
                }
                Some(PhysicalType::scalar(DimVector::DIMENSIONLESS))
            }
        }
    }

    /// Shape rules for multiplication:
    /// - scalar × scalar → scalar
    /// - scalar × tensor → tensor (broadcast)
    /// - tensor × scalar → tensor (broadcast)
    /// - tensor[M, N] × tensor[N, P] → tensor[M, P]  (2-D matrix mul)
    /// - tensor[N] × tensor[N] → scalar               (dot product)
    /// - same-shape tensors of other ranks → same shape (element-wise)
    fn mul_shapes(
        &mut self,
        lhs: &Option<Vec<usize>>,
        rhs: &Option<Vec<usize>>,
        span: SourceSpan,
    ) -> Option<Vec<usize>> {
        match (lhs, rhs) {
            (None, None) => None,
            (Some(s), None) | (None, Some(s)) => Some(s.clone()),
            (Some(ls), Some(rs)) => {
                if ls.len() == 2 && rs.len() == 2 {
                    if ls[1] != rs[0] {
                        self.errors.push(TypeError::shape_mismatch(
                            span,
                            &format!("[{}, {}]", rs[0], rs[1]),
                            &format!("[{}, {}]", ls[0], ls[1]),
                        ));
                        return None;
                    }
                    Some(vec![ls[0], rs[1]])
                } else if ls.len() == 1 && rs.len() == 1 && ls[0] == rs[0] {
                    // dot product → scalar
                    None
                } else if ls == rs {
                    Some(ls.clone())
                } else {
                    self.errors.push(TypeError::shape_mismatch(
                        span,
                        &shape_display(&Some(rs.clone())),
                        &shape_display(&Some(ls.clone())),
                    ));
                    None
                }
            }
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::items::parse_source;
    use crate::types::error::TypeErrorCode;

    fn check_src(src: &str) -> CheckResult {
        let file = parse_source(src).expect("parse failed");
        check(&file)
    }

    #[test]
    fn test_expr_fn_correct() {
        let result = check_src("pub fn force(m: kg, a: m/s^2) -> N := m * a;");
        assert!(result.ok(), "errors: {:?}", result.errors.iter().map(|e| &e.message).collect::<Vec<_>>());
    }

    #[test]
    fn test_let_annotation_conflict() {
        let result = check_src("fn bad(x: m) { let y: s = x; }");
        assert!(!result.ok());
        assert!(result.errors.iter().any(|e| e.code == TypeErrorCode::AnnotationConflict));
    }

    #[test]
    fn test_return_type_mismatch() {
        let result = check_src("fn wrong(x: m) -> s := x;");
        assert!(!result.ok());
        assert!(result.errors.iter().any(|e| e.code == TypeErrorCode::ReturnTypeMismatch));
    }

    #[test]
    fn test_type_alias_resolution() {
        let result = check_src("type Acc = m/s^2;\nfn f(a: Acc) -> Acc := a;");
        assert!(result.ok(), "errors: {:?}", result.errors.iter().map(|e| &e.message).collect::<Vec<_>>());
    }

    #[test]
    fn test_pow_integer_exponent() {
        let result = check_src("fn sq(x: m) -> m^2 := x^2;");
        assert!(result.ok(), "errors: {:?}", result.errors);
    }

    #[test]
    fn test_kinetic_energy_with_pow() {
        let result = check_src("fn ke(m: kg, v: m/s) -> J := 0.5 * m * v^2;");
        assert!(result.ok(), "errors: {:?}", result.errors);
    }

    #[test]
    fn test_use_import_fn() {
        let mut imports = crate::modules::ImportBindings::default();
        imports.functions.insert("energy".to_string(), crate::types::dim::DimVector::JOULE);
        let file = parse_source("fn main() -> J := energy(1.0, 1.0);").unwrap();
        let result = check_with_imports(&file, &imports);
        assert!(result.ok(), "errors: {:?}", result.errors);
    }

    #[test]
    fn test_add_mismatch() {
        let result = check_src("fn bad(x: m, y: kg) -> m := x + y;");
        assert!(!result.ok());
        assert!(result.errors.iter().any(|e| e.code == TypeErrorCode::DimensionMismatch));
    }

    #[test]
    fn test_mul_infers_compound() {
        let result = check_src("fn v(x: m, t: s) -> m/s := x / t;");
        assert!(result.ok(), "errors: {:?}", result.errors.iter().map(|e| &e.message).collect::<Vec<_>>());
    }

    #[test]
    fn test_block_fn_let_correct() {
        let result = check_src("fn f(x: m) { let y: m = x; }");
        assert!(result.ok(), "errors: {:?}", result.errors.iter().map(|e| &e.message).collect::<Vec<_>>());
    }

    #[test]
    fn test_unknown_unit() {
        let result = check_src("fn f(x: xyz) -> m := x;");
        assert!(!result.ok());
        assert!(result.errors.iter().any(|e| e.code == TypeErrorCode::UnknownUnit));
    }

    #[test]
    fn test_tensor_param_add_ok() {
        // Two kg[1024] tensors can be added together.
        let result = check_src("fn add_bufs(a: kg[1024], b: kg[1024]) -> kg[1024] := a + b;");
        assert!(result.ok(), "errors: {:?}", result.errors.iter().map(|e| &e.message).collect::<Vec<_>>());
    }

    #[test]
    fn test_tensor_shape_mismatch_add() {
        let result = check_src("fn bad(a: kg[1024], b: kg[512]) -> kg[1024] := a + b;");
        assert!(!result.ok());
        assert!(result.errors.iter().any(|e| e.code == TypeErrorCode::ShapeMismatch));
    }

    #[test]
    fn test_tensor_matrix_mul() {
        // Shape: [4, 4] × [4, 8] → [4, 8]; Dim: kg * kg → kg^2 per element.
        let result = check_src("fn matmul(a: kg[4, 4], b: kg[4, 8]) -> kg^2[4, 8] := a * b;");
        assert!(result.ok(), "errors: {:?}", result.errors.iter().map(|e| &e.message).collect::<Vec<_>>());
    }

    #[test]
    fn test_tensor_matrix_mul_bad() {
        // [4, 3] * [4, 4] → shape error: inner dims don't match
        let result = check_src("fn bad(a: kg[4, 3], b: kg[4, 4]) := a * b;");
        assert!(!result.ok());
        assert!(result.errors.iter().any(|e| e.code == TypeErrorCode::ShapeMismatch));
    }
}
