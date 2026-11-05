//! The one AST walk the tools share.
//!
//! Code actions and semantic tokens both need to visit every expression in a
//! module — the first to find the thing under the cursor, the second to find
//! the unit literals. Doing that with one recursive walk keeps them from
//! disagreeing about which node is where, which is the same reason the
//! occurrence walk exists for names.
//!
//! The walk is total over [`ExprKind`]: every child expression is visited,
//! including `do` statements, block statements, match guards, index slices and
//! string interpolations.

use aevia_syntax::ast::*;

/// Visits every expression reachable from `declaration`, outermost first.
pub(crate) fn declaration_exprs<'a>(declaration: &'a Decl, visit: &mut impl FnMut(&'a Expr)) {
    match &declaration.kind {
        DeclKind::Fn { body, .. } => expression(body, visit),
        DeclKind::Let { value, .. } => expression(value, visit),
        DeclKind::Test { body, .. } => expression(body, visit),
        DeclKind::Instance { methods, .. } => {
            for method in methods {
                expression(&method.body, visit);
            }
        }
        DeclKind::Import { .. }
        | DeclKind::Dimension { .. }
        | DeclKind::Unit { .. }
        | DeclKind::Type { .. }
        | DeclKind::Class { .. }
        | DeclKind::Derive { .. } => {}
    }
}

/// Visits every expression in `module`, in source order.
pub(crate) fn module_exprs<'a>(module: &'a Module, mut visit: impl FnMut(&'a Expr)) {
    for declaration in &module.decls {
        declaration_exprs(declaration, &mut visit);
    }
}

/// Visits `expr` and every expression inside it, outermost first.
pub(crate) fn expression<'a>(expr: &'a Expr, visit: &mut impl FnMut(&'a Expr)) {
    visit(expr);
    match &expr.kind {
        ExprKind::UnitLit
        | ExprKind::Error
        | ExprKind::Bool(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Quantity { .. }
        | ExprKind::Var(_)
        | ExprKind::QName(_)
        | ExprKind::Placeholder => {}
        ExprKind::Str(pieces) => {
            for piece in pieces {
                if let StrPiece::Expr(inner) = piece {
                    expression(inner, visit);
                }
            }
        }
        ExprKind::Unary { operand, .. } => expression(operand, visit),
        ExprKind::Binary { left, right, .. } => {
            expression(left, visit);
            expression(right, visit);
        }
        ExprKind::Call { callee, args } => {
            expression(callee, visit);
            for argument in args {
                expression(argument, visit);
            }
        }
        ExprKind::TypeApp { callee, .. } => expression(callee, visit),
        ExprKind::Lambda { body, .. } => expression(body, visit),
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expression(cond, visit);
            expression(then_branch, visit);
            expression(else_branch, visit);
        }
        ExprKind::Match { scrutinee, arms } => {
            expression(scrutinee, visit);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    expression(guard, visit);
                }
                expression(&arm.body, visit);
            }
        }
        ExprKind::LetIn { value, body, .. } => {
            expression(value, visit);
            expression(body, visit);
        }
        ExprKind::Block { stmts, tail } => {
            for statement in stmts {
                match statement {
                    BlockStmt::Let { value, .. } => expression(value, visit),
                    BlockStmt::Fn(declaration) => declaration_exprs(declaration, visit),
                }
            }
            if let Some(tail) = tail {
                expression(tail, visit);
            }
        }
        ExprKind::Do { stmts } => {
            for statement in stmts {
                match statement {
                    DoStmt::Let { value, .. } | DoStmt::Bind { value, .. } => {
                        expression(value, visit);
                    }
                    DoStmt::Expr(inner) => expression(inner, visit),
                }
            }
        }
        ExprKind::Record { base, fields } => {
            if let Some(base) = base {
                expression(base, visit);
            }
            for (_, value) in fields {
                expression(value, visit);
            }
        }
        ExprKind::Field { base, .. } => expression(base, visit),
        ExprKind::Tuple(items) | ExprKind::Tensor(items) => {
            for item in items {
                expression(item, visit);
            }
        }
        ExprKind::Index { base, indices } => {
            expression(base, visit);
            for index in indices {
                match index {
                    SliceAst::Index(inner) => expression(inner, visit),
                    SliceAst::Range {
                        start, end, step, ..
                    } => {
                        for part in [start, end, step].into_iter().flatten() {
                            expression(part, visit);
                        }
                    }
                }
            }
        }
        ExprKind::Annot { expr, .. } => expression(expr, visit),
        ExprKind::Try(inner) => expression(inner, visit),
        ExprKind::Factorial { expr, .. } => expression(expr, visit),
    }
}

/// The expression whose span is exactly `span`, when there is one.
pub(crate) fn exactly(module: &Module, span: aevia_syntax::Span) -> Option<&Expr> {
    let mut found = None;
    module_exprs(module, |expr| {
        if expr.span == span {
            found = Some(expr);
        }
    });
    found
}

/// True when `span` and the request `[start, end)` touch.
///
/// A cursor is a zero-width request; then "touch" means the cursor is inside
/// the span, which is what a menu opened at a position expects.
pub(crate) fn touches(span: aevia_syntax::Span, start: u32, end: u32) -> bool {
    if start == end {
        span.start <= start && start <= span.end
    } else {
        span.start <= end && start <= span.end
    }
}
