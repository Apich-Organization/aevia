//! Shared read-only AST visitor over [`crate::ast`].
//!
//! Tools (`aevia-analysis`, `aevia-lint`) implement [`Visitor`] and call the
//! `walk_*` functions rather than duplicating recursive matches over `ExprKind`,
//! `PatKind`, `TyKind`, and `DeclKind`.

use crate::ast::BlockStmt;
use crate::ast::Decl;
use crate::ast::DeclKind;
use crate::ast::DoStmt;
use crate::ast::Expr;
use crate::ast::ExprKind;
use crate::ast::Module;
use crate::ast::Pat;
use crate::ast::PatKind;
use crate::ast::SliceAst;
use crate::ast::StrPiece;
use crate::ast::Ty;
use crate::ast::TyKind;
use crate::ast::TypeBody;

/// Read-only AST visitor.
///
/// Default method implementations simply recurse via the matching `walk_*`
/// helper. Override a `visit_*` method to inspect a node and call `walk_*` to
/// continue into its children.
pub trait Visitor<'ast>: Sized {
    /// Visits a whole module.
    fn visit_module(
        &mut self,
        module: &'ast Module,
    ) {
        walk_module(self, module);
    }

    /// Visits a top-level or nested declaration.
    fn visit_decl(
        &mut self,
        decl: &'ast Decl,
    ) {
        walk_decl(self, decl);
    }

    /// Visits an expression.
    fn visit_expr(
        &mut self,
        expr: &'ast Expr,
    ) {
        walk_expr(self, expr);
    }

    /// Visits a pattern.
    fn visit_pat(
        &mut self,
        pat: &'ast Pat,
    ) {
        walk_pat(self, pat);
    }

    /// Visits a type annotation.
    fn visit_ty(
        &mut self,
        ty: &'ast Ty,
    ) {
        walk_ty(self, ty);
    }
}

/// Walks every declaration in `module` in source order.
pub fn walk_module<'ast, V: Visitor<'ast>>(
    visitor: &mut V,
    module: &'ast Module,
) {
    for decl in &module.decls {
        visitor.visit_decl(decl);
    }
}

/// Walks the children of `decl`.
pub fn walk_decl<'ast, V: Visitor<'ast>>(
    visitor: &mut V,
    decl: &'ast Decl,
) {
    for attr in &decl.attrs {
        for arg in &attr.args {
            visitor.visit_expr(&arg.value);
        }
    }
    match &decl.kind {
        | DeclKind::Import { .. } | DeclKind::Dimension { .. } | DeclKind::Unit { .. } => {},
        | DeclKind::Type { body, .. } => {
            match body {
                | TypeBody::Alias(ty) => visitor.visit_ty(ty),
                | TypeBody::Variants(ctors) => {
                    for ctor in ctors {
                        for arg in &ctor.args {
                            visitor.visit_ty(arg);
                        }
                    }
                },
                | TypeBody::Abstract => {},
            }
        },
        | DeclKind::Class { methods, .. } => {
            for method in methods {
                visitor.visit_ty(&method.ty);
            }
        },
        | DeclKind::Instance { ty, methods, .. } => {
            visitor.visit_ty(ty);
            for method in methods {
                for param in &method.params {
                    visitor.visit_pat(&param.pattern);
                    if let Some(ann) = &param.ty {
                        visitor.visit_ty(ann);
                    }
                }
                if let Some(ret) = &method.ret {
                    visitor.visit_ty(ret);
                }
                visitor.visit_expr(&method.body);
            }
        },
        | DeclKind::Derive { ty, .. } => visitor.visit_ty(ty),
        | DeclKind::Fn { args, ret, body, .. } => {
            for arg in args {
                visitor.visit_pat(&arg.pattern);
                if let Some(ann) = &arg.ty {
                    visitor.visit_ty(ann);
                }
            }
            if let Some(ret) = ret {
                visitor.visit_ty(ret);
            }
            visitor.visit_expr(body);
        },
        | DeclKind::Let {
            pattern, ty, value, ..
        } => {
            visitor.visit_pat(pattern);
            if let Some(ann) = ty {
                visitor.visit_ty(ann);
            }
            visitor.visit_expr(value);
        },
        | DeclKind::Test { body, .. } => visitor.visit_expr(body),
    }
}

/// Walks the children of `expr`.
pub fn walk_expr<'ast, V: Visitor<'ast>>(
    visitor: &mut V,
    expr: &'ast Expr,
) {
    match &expr.kind {
        | ExprKind::UnitLit
        | ExprKind::Error
        | ExprKind::Bool(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Quantity { .. }
        | ExprKind::Var(_)
        | ExprKind::QName(_)
        | ExprKind::Placeholder => {},
        | ExprKind::Str(pieces) => {
            for piece in pieces {
                if let StrPiece::Expr(inner) = piece {
                    visitor.visit_expr(inner);
                }
            }
        },
        | ExprKind::Unary { operand, .. } => visitor.visit_expr(operand),
        | ExprKind::Binary { left, right, .. } => {
            visitor.visit_expr(left);
            visitor.visit_expr(right);
        },
        | ExprKind::Call { callee, args } => {
            visitor.visit_expr(callee);
            for arg in args {
                visitor.visit_expr(arg);
            }
        },
        | ExprKind::TypeApp { callee, args } => {
            visitor.visit_expr(callee);
            for arg in args {
                visitor.visit_ty(arg);
            }
        },
        | ExprKind::Lambda { params, ret, body } => {
            for param in params {
                visitor.visit_pat(&param.pattern);
                if let Some(ann) = &param.ty {
                    visitor.visit_ty(ann);
                }
            }
            if let Some(ret) = ret {
                visitor.visit_ty(ret);
            }
            visitor.visit_expr(body);
        },
        | ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            visitor.visit_expr(cond);
            visitor.visit_expr(then_branch);
            visitor.visit_expr(else_branch);
        },
        | ExprKind::Match { scrutinee, arms } => {
            visitor.visit_expr(scrutinee);
            for arm in arms {
                visitor.visit_pat(&arm.pattern);
                if let Some(guard) = &arm.guard {
                    visitor.visit_expr(guard);
                }
                visitor.visit_expr(&arm.body);
            }
        },
        | ExprKind::LetIn {
            pattern,
            ty,
            value,
            body,
        } => {
            visitor.visit_pat(pattern);
            if let Some(ann) = ty {
                visitor.visit_ty(ann);
            }
            visitor.visit_expr(value);
            visitor.visit_expr(body);
        },
        | ExprKind::Block { stmts, tail } => {
            for stmt in stmts {
                match stmt {
                    | BlockStmt::Let {
                        pattern, ty, value, ..
                    } => {
                        visitor.visit_pat(pattern);
                        if let Some(ann) = ty {
                            visitor.visit_ty(ann);
                        }
                        visitor.visit_expr(value);
                    },
                    | BlockStmt::Fn(decl) => visitor.visit_decl(decl),
                }
            }
            if let Some(tail) = tail {
                visitor.visit_expr(tail);
            }
        },
        | ExprKind::Do { stmts } => {
            for stmt in stmts {
                match stmt {
                    | DoStmt::Let {
                        pattern, ty, value, ..
                    } => {
                        visitor.visit_pat(pattern);
                        if let Some(ann) = ty {
                            visitor.visit_ty(ann);
                        }
                        visitor.visit_expr(value);
                    },
                    | DoStmt::Bind { pattern, value, .. } => {
                        visitor.visit_pat(pattern);
                        visitor.visit_expr(value);
                    },
                    | DoStmt::Expr(inner) => visitor.visit_expr(inner),
                }
            }
        },
        | ExprKind::Record { base, fields } => {
            if let Some(base) = base {
                visitor.visit_expr(base);
            }
            for (_, value) in fields {
                visitor.visit_expr(value);
            }
        },
        | ExprKind::Field { base, .. } => visitor.visit_expr(base),
        | ExprKind::Tuple(items) | ExprKind::Tensor(items) => {
            for item in items {
                visitor.visit_expr(item);
            }
        },
        | ExprKind::Index { base, indices } => {
            visitor.visit_expr(base);
            for index in indices {
                match index {
                    | SliceAst::Index(inner) => visitor.visit_expr(inner),
                    | SliceAst::Range { start, end, step, .. } => {
                        for part in [start, end, step].into_iter().flatten() {
                            visitor.visit_expr(part);
                        }
                    },
                }
            }
        },
        | ExprKind::Annot { expr, ty } => {
            visitor.visit_expr(expr);
            visitor.visit_ty(ty);
        },
        | ExprKind::Try(inner) => visitor.visit_expr(inner),
        | ExprKind::Factorial { expr, .. } => visitor.visit_expr(expr),
    }
}

/// Walks the children of `pat`.
pub fn walk_pat<'ast, V: Visitor<'ast>>(
    visitor: &mut V,
    pat: &'ast Pat,
) {
    match &pat.kind {
        | PatKind::Wildcard | PatKind::Error | PatKind::Var(_) | PatKind::Lit(_) => {},
        | PatKind::Ctor { args, .. } | PatKind::Tuple(args) => {
            for arg in args {
                visitor.visit_pat(arg);
            }
        },
        | PatKind::Record(fields) => {
            for (_, inner) in fields {
                if let Some(inner) = inner {
                    visitor.visit_pat(inner);
                }
            }
        },
        | PatKind::Tensor { items, .. } => {
            for item in items {
                visitor.visit_pat(item);
            }
        },
        | PatKind::As { pattern, .. } => visitor.visit_pat(pattern),
        | PatKind::Annot { pattern, ty } => {
            visitor.visit_pat(pattern);
            visitor.visit_ty(ty);
        },
    }
}

/// Walks the children of `ty`.
pub fn walk_ty<'ast, V: Visitor<'ast>>(
    visitor: &mut V,
    ty: &'ast Ty,
) {
    match &ty.kind {
        | TyKind::Int
        | TyKind::Real(_)
        | TyKind::Float(_)
        | TyKind::Complex(_)
        | TyKind::Bool
        | TyKind::Prop
        | TyKind::Str
        | TyKind::Unit
        | TyKind::DimArg(_)
        | TyKind::NatArg(_)
        | TyKind::ShapeArg(_)
        | TyKind::Point(_)
        | TyKind::UnitOf(_)
        | TyKind::Dyn(_)
        | TyKind::SymFun(_, _)
        | TyKind::Var(_) => {},
        | TyKind::Tensor { elem, .. }
        | TyKind::Matrix { elem, .. }
        | TyKind::Sparse { elem, .. }
        | TyKind::Option(elem)
        | TyKind::Set(elem)
        | TyKind::Table(elem)
        | TyKind::RateOf(elem)
        | TyKind::DiffusionOf(elem)
        | TyKind::Io(elem)
        | TyKind::Forall(_, elem) => visitor.visit_ty(elem),
        | TyKind::Result(ok, err) => {
            visitor.visit_ty(ok);
            visitor.visit_ty(err);
        },
        | TyKind::Row(items) | TyKind::Tuple(items) => {
            for item in items {
                visitor.visit_ty(item);
            }
        },
        | TyKind::Fn(params, ret) => {
            for param in params {
                visitor.visit_ty(param);
            }
            visitor.visit_ty(ret);
        },
        | TyKind::Record { fields, .. } => {
            for (_, field_ty) in fields {
                visitor.visit_ty(field_ty);
            }
        },
        | TyKind::Named { args, .. } => {
            for arg in args {
                visitor.visit_ty(arg);
            }
        },
    }
}
