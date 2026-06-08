//! Dimensional type-checker pass.
//!
//! Walks a parsed `SourceFile` AST and:
//!   1. Registers all `type` aliases and `struct` fields into the environment.
//!   2. Registers all top-level function signatures (params + return type).
//!   3. Checks every expression for dimensional consistency.
//!   4. Verifies `let` annotation vs. inferred type.
//!   5. Verifies function return expression vs. declared return type.

use crate::ast::{
    BinOp, Expr, FunctionBody, Item, MatchArm, Pattern, SourceFile, Spanned, Stmt, UnOp,
};
use rust_decimal::prelude::ToPrimitive;
use crate::modules::ImportBindings;
use crate::types::{
    dim::{resolve, DimVector},
    env::TypeEnv,
    error::TypeError,
};
use crate::ast::DimExpr;
use miette::SourceSpan;

/// Result of checking a full source file.
pub struct CheckResult {
    /// All dimensional type errors found (may be empty = success).
    pub errors: Vec<TypeError>,
}

impl CheckResult {
    #[must_use]
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

// ── Public entry point ─────────────────────────────────────────────────────────

/// Run the dimensional type checker over a parsed source file.
pub fn check(file: &SourceFile) -> CheckResult {
    check_with_imports(file, &ImportBindings::default())
}

/// Type-check with `use` import bindings from the module resolver.
pub fn check_with_imports(file: &SourceFile, imports: &ImportBindings) -> CheckResult {
    let mut ctx = Ctx {
        env: TypeEnv::new(),
        errors: Vec::new(),
    };
    crate::modules::apply_imports(&mut ctx.env, imports);
    // Pass 1: register all top-level type aliases and function signatures so
    // that forward references resolve correctly.
    for item in &file.items {
        ctx.register_item(item);
    }
    // Pass 2: type-check all items.
    for item in &file.items {
        ctx.check_item(item);
    }
    CheckResult { errors: ctx.errors }
}

/// Resolve a `DimExpr` to a `DimVector`, using both the built-in base unit
/// table **and** any user-defined type aliases registered in `env`.
///
/// This handles cases like `type Acc = m/s^2;  fn f(a: Acc) …` where `Acc`
/// is a `Base("Acc")` node that `resolve()` alone won't recognise.
fn resolve_with_env(span: &Spanned<crate::ast::DimExpr>, env: &TypeEnv) -> Option<DimVector> {
    // First, attempt the purely structural resolution (base units + arithmetic).
    if let Some(dim) = resolve(span) {
        return Some(dim);
    }
    // For a bare Base node that failed structural resolution, check the env alias table.
    if let DimExpr::Base(name) = &span.node {
        return env.lookup_alias(name);
    }
    // For compound expressions containing unrecognised bases, walk and substitute.
    resolve_compound_with_env(&span.node, env)
}

fn resolve_compound_with_env(expr: &DimExpr, env: &TypeEnv) -> Option<DimVector> {
    match expr {
        DimExpr::Base(name) => {
            crate::types::dim::base_unit(name).or_else(|| env.lookup_alias(name))
        }
        DimExpr::Mul(lhs, rhs) => {
            Some(resolve_compound_with_env(&lhs.node, env)?
                .mul(resolve_compound_with_env(&rhs.node, env)?))
        }
        DimExpr::Div(lhs, rhs) => {
            Some(resolve_compound_with_env(&lhs.node, env)?
                .div(resolve_compound_with_env(&rhs.node, env)?))
        }
        DimExpr::Power(base, exp) => {
            Some(resolve_compound_with_env(&base.node, env)?.pow(*exp))
        }
    }
}



struct Ctx {
    env: TypeEnv,
    errors: Vec<TypeError>,
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
                // Pre-register the return type so recursive calls can be checked.
                if let Some(ret_span) = return_type {
                    if let Some(dim) = resolve_with_env(ret_span, &self.env) {
                        self.env.register_fn(name.clone(), dim);
                    }
                } else {
                    // Dimensionless return (no annotation).
                    self.env.register_fn(name.clone(), DimVector::DIMENSIONLESS);
                }
                // Register params as global stubs (will be re-scoped per call).
                for param in params {
                    if resolve_with_env(&param.dim, &self.env).is_none() {
                        self.errors.push(TypeError::unknown_unit(
                            param.dim.span,
                            &param.name,
                        ));
                    }
                }
            }
            Item::Struct { name: _, fields, .. } => {
                for field in fields {
                    if resolve_with_env(&field.dim, &self.env).is_none() {
                        self.errors.push(TypeError::unknown_unit(
                            field.dim.span,
                            &field.name,
                        ));
                    }
                }
            }
            // CustomOp dimensions are verified during check_item.
            Item::CustomOp { .. } | Item::Use { .. } | Item::Module { .. } => {}
        }
    }

    // ── Check pass ────────────────────────────────────────────────────────

    fn check_item(&mut self, item: &Spanned<Item>) {
        match &item.node {
            Item::Function { name: _fn_name, params, return_type, body, .. } => {
                self.env.push_scope();

                // Bind parameters into scope.
                for param in params {
                    if let Some(dim) = resolve_with_env(&param.dim, &self.env) {
                        self.env.define(param.name.clone(), dim);
                    }
                }

                let declared_return = return_type
                    .as_ref()
                    .and_then(|rt| resolve_with_env(rt, &self.env));

                match body {
                    FunctionBody::Expression(expr) => {
                        if let Some(inferred) = self.infer_expr(expr) {
                            if let Some(declared) = declared_return {
                                if declared != inferred {
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
            // Already registered; no further checking needed here.
            Item::TypeAlias { .. } | Item::Struct { .. }
            | Item::Use { .. } | Item::Module { .. } => {}
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
                        (Some(declared_dim), Some(inferred_dim)) => {
                            if declared_dim != inferred_dim {
                                self.errors.push(TypeError::annotation_conflict(
                                    init.span,
                                    &declared_dim.to_string(),
                                    &inferred_dim.to_string(),
                                ));
                            } else {
                                self.env.define(name.clone(), declared_dim);
                            }
                        }
                        (None, _) => {
                            self.errors.push(TypeError::unknown_unit(
                                declared_span.span,
                                name,
                            ));
                        }
                        (Some(declared_dim), None) => {
                            // Use the declared type even if we couldn't infer.
                            self.env.define(name.clone(), declared_dim);
                        }
                    }
                } else if let Some(dim) = inferred {
                    self.env.define(name.clone(), dim);
                }
            }
            Stmt::Assign { target, value } => {
                let expected = self.env.lookup(target);
                if let (Some(exp), Some(got)) = (expected, self.infer_expr(value)) {
                    if exp != got {
                        self.errors.push(TypeError::dimension_mismatch(
                            value.span,
                            &got.to_string(),
                            &exp.to_string(),
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

    /// Infer the `DimVector` of an expression, recording any errors.
    /// Returns `None` when the type cannot be determined (e.g. unknown var).
    fn infer_expr(&mut self, expr: &Spanned<Expr>) -> Option<DimVector> {
        match &expr.node {
            Expr::Literal { suffix, .. } => {
                if let Some(unit_span) = suffix {
                    match resolve_with_env(unit_span, &self.env) {
                        Some(dim) => Some(dim),
                        None => {
                            self.errors.push(TypeError::unknown_unit(
                                unit_span.span,
                                "literal suffix",
                            ));
                            None
                        }
                    }
                } else {
                    Some(DimVector::DIMENSIONLESS)
                }
            }

            Expr::Variable(name) => {
                match self.env.lookup(name) {
                    Some(dim) => Some(dim),
                    None => {
                        // Unknown variable — soft failure: we can't infer but
                        // don't emit a type error (it may be caught by a future
                        // name-resolution pass).
                        None
                    }
                }
            }

            Expr::BinaryOp { op, lhs, rhs } => {
                let l = self.infer_expr(lhs);
                let r = self.infer_expr(rhs);
                self.check_binop(*op, l, r, &rhs.node, expr.span)
            }

            Expr::UnaryOp { op: UnOp::Neg, expr: inner } => {
                self.infer_expr(inner) // negation preserves dimension
            }

            Expr::Call { func, args } => {
                // Resolve argument types (for future argument-checking).
                for arg in args {
                    self.infer_expr(arg);
                }
                // Return the registered return type of the function.
                self.env.lookup_fn(func)
            }

            Expr::If { cond, then_branch, else_branch } => {
                self.infer_expr(cond); // condition is typically dimensionless
                let then_dim = self.infer_expr(then_branch);
                if let Some(else_expr) = else_branch {
                    let else_dim = self.infer_expr(else_expr);
                    match (then_dim, else_dim) {
                        (Some(t), Some(e)) if t != e => {
                            self.errors.push(TypeError::dimension_mismatch(
                                else_expr.span,
                                &e.to_string(),
                                &t.to_string(),
                            ));
                            None
                        }
                        (t, _) => t,
                    }
                } else {
                    then_dim
                }
            }

            Expr::Block(stmts) => {
                self.env.push_scope();
                let mut last: Option<DimVector> = None;
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
                Some(DimVector::DIMENSIONLESS)
            }

            Expr::While { cond, body } => {
                // Condition should be dimensionless (comparison result).
                self.infer_expr(cond);
                self.env.push_scope();
                for stmt in body {
                    self.check_stmt(stmt);
                }
                self.env.pop_scope();
                Some(DimVector::DIMENSIONLESS)
            }

            Expr::For { var, start, end, body } => {
                // start and end must be dimensionless (loop counters).
                let start_dim = self.infer_expr(start);
                let end_dim = self.infer_expr(end);
                if let (Some(s), Some(e)) = (&start_dim, &end_dim) {
                    if !s.addable_with(*e) {
                        self.errors.push(TypeError::dimension_mismatch(
                            end.span,
                            &e.to_string(),
                            &s.to_string(),
                        ));
                    }
                }
                self.env.push_scope();
                // Bind the loop variable as dimensionless.
                self.env.define(var.clone(), DimVector::DIMENSIONLESS);
                for stmt in body {
                    self.check_stmt(stmt);
                }
                self.env.pop_scope();
                Some(DimVector::DIMENSIONLESS)
            }

            Expr::Match { scrutinee, arms } => {
                let scrutinee_dim = self.infer_expr(scrutinee);
                let mut result_dim: Option<DimVector> = None;
                for MatchArm { pattern, body } in arms {
                    self.env.push_scope();
                    match pattern {
                        Pattern::Binding { name, type_guard } => {
                            let bound_dim = if let Some(guard_span) = type_guard {
                                // Verify the type guard matches the scrutinee dimension.
                                let guard_dim = resolve_with_env(guard_span, &self.env);
                                if let (Some(sd), Some(gd)) = (&scrutinee_dim, &guard_dim) {
                                    if sd != gd {
                                        self.errors.push(TypeError::annotation_conflict(
                                            guard_span.span,
                                            &gd.to_string(),
                                            &sd.to_string(),
                                        ));
                                    }
                                }
                                guard_dim.or(scrutinee_dim)
                            } else {
                                scrutinee_dim
                            };
                            if let Some(dim) = bound_dim {
                                self.env.define(name.clone(), dim);
                            }
                        }
                        Pattern::Wildcard | Pattern::Literal(_) => {}
                    }
                    let arm_dim = self.infer_expr(body);
                    self.env.pop_scope();
                    // All arms must produce the same dimension.
                    match (&result_dim, &arm_dim) {
                        (Some(rd), Some(ad)) if rd != ad => {
                            self.errors.push(TypeError::dimension_mismatch(
                                body.span,
                                &ad.to_string(),
                                &rd.to_string(),
                            ));
                        }
                        (None, _) => result_dim = arm_dim,
                        _ => {}
                    }
                }
                result_dim
            }

            Expr::UnsafeTransmute { expr: inner, assume_unit, .. } => {
                let _ = self.infer_expr(inner);
                if let Some(unit_span) = assume_unit {
                    resolve_with_env(unit_span, &self.env)
                } else {
                    Some(DimVector::DIMENSIONLESS)
                }
            }
        }
    }

    /// Check a binary operation for dimensional consistency and return the
    /// resulting `DimVector`.
    fn check_binop(
        &mut self,
        op: BinOp,
        lhs: Option<DimVector>,
        rhs: Option<DimVector>,
        rhs_expr: &Expr,
        span: SourceSpan,
    ) -> Option<DimVector> {
        match op {
            // Addition / subtraction require identical dimensions.
            BinOp::Add | BinOp::Sub => {
                match (lhs, rhs) {
                    (Some(l), Some(r)) => {
                        if l.addable_with(r) {
                            Some(l)
                        } else {
                            self.errors.push(TypeError::dimension_mismatch(
                                span,
                                &r.to_string(),
                                &l.to_string(),
                            ));
                            None
                        }
                    }
                    (Some(l), None) => Some(l),
                    (None, Some(r)) => Some(r),
                    (None, None) => None,
                }
            }

            // Multiplication: dimensions combine (add exponents).
            BinOp::Mul => {
                match (lhs, rhs) {
                    (Some(l), Some(r)) => Some(l.mul(r)),
                    (Some(l), None) | (None, Some(l)) => Some(l),
                    (None, None) => None,
                }
            }

            // Division: dimensions combine (subtract exponents).
            BinOp::Div => {
                match (lhs, rhs) {
                    (Some(l), Some(r)) => Some(l.div(r)),
                    (Some(l), None) => Some(l),
                    (None, _) => None,
                }
            }

            // Modulo: result dimension matches LHS.
            BinOp::Mod => lhs,

            // Exponentiation: RHS must be a dimensionless integer literal.
            BinOp::Pow => match (lhs, rhs_expr) {
                (Some(base), Expr::Literal { value, suffix: None }) => {
                    if let Some(exp) = value.to_i64() {
                        if exp >= 0 {
                            Some(base.pow(exp as i32))
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
                        lhs
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

            // Comparisons: operands must be same dimension; result is dimensionless.
            BinOp::Lt | BinOp::Gt | BinOp::Eq => {
                if let (Some(l), Some(r)) = (lhs, rhs) {
                    if !l.addable_with(r) {
                        self.errors.push(TypeError::dimension_mismatch(
                            span,
                            &r.to_string(),
                            &l.to_string(),
                        ));
                    }
                }
                Some(DimVector::DIMENSIONLESS)
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
        // force = mass * acceleration → Newton (kg·m/s²)
        let result = check_src("pub fn force(m: kg, a: m/s^2) -> N := m * a;");
        assert!(result.ok(), "errors: {:?}", result.errors.iter().map(|e| &e.message).collect::<Vec<_>>());
    }

    #[test]
    fn test_let_annotation_conflict() {
        // Declares `x: s` but assigns `m` → conflict
        let result = check_src("fn bad(x: m) { let y: s = x; }");
        assert!(!result.ok());
        assert!(result.errors.iter().any(|e| e.code == TypeErrorCode::AnnotationConflict));
    }

    #[test]
    fn test_return_type_mismatch() {
        // Returns `m` but declared `s`
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
        imports
            .functions
            .insert("energy".to_string(), crate::types::dim::DimVector::JOULE);
        let file = parse_source("fn main() -> J := energy(1.0, 1.0);").unwrap();
        let result = check_with_imports(&file, &imports);
        assert!(result.ok(), "errors: {:?}", result.errors);
    }

    #[test]
    fn test_add_mismatch() {
        // Cannot add m + kg
        let result = check_src("fn bad(x: m, y: kg) -> m := x + y;");
        assert!(!result.ok());
        assert!(result.errors.iter().any(|e| e.code == TypeErrorCode::DimensionMismatch));
    }

    #[test]
    fn test_mul_infers_compound() {
        // 0.5 * m * s^-1 → velocity
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
}
