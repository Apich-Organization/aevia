//! `W0706` (`float-equality`): `==` or `!=` on `Float` values.

use aevia_syntax::ast::BinOp;
use aevia_syntax::ast::Expr;
use aevia_syntax::ast::ExprKind;
use aevia_types::check::CheckedModule;
use aevia_types::ty::Rep;
use aevia_types::ty::Type;

use crate::lint::Lint;
use crate::lint::W0706_FLOAT_EQUALITY;
use crate::pass::LintCx;
use crate::pass::LintPassInfo;
use crate::pass::SemanticPass;

/// Flags `==` / `!=` where either operand has type `Float[d]`.
pub struct FloatEqualityPass;

impl LintPassInfo for FloatEqualityPass {
    fn lints(&self) -> &'static [&'static Lint] {
        static LINTS: &[&Lint] = &[&W0706_FLOAT_EQUALITY];
        LINTS
    }
}

impl SemanticPass for FloatEqualityPass {
    fn check_expr(
        &self,
        cx: &mut LintCx<'_>,
        expr: &Expr,
        checked: &CheckedModule,
    ) {
        let ExprKind::Binary { op, left, right } = &expr.kind else {
            return;
        };
        if !matches!(op, BinOp::Eq | BinOp::Ne) {
            return;
        }
        let is_float = |span| {
            matches!(
                checked.expr_types.get(&span),
                Some(Type::Scalar(Rep::Float, _))
            )
        };
        if is_float(left.span) || is_float(right.span) {
            cx.emit(
                &W0706_FLOAT_EQUALITY,
                expr.span,
                "strict equality comparison on `Float` values",
            )
            .help("compare with a tolerance instead, e.g. `abs(a - b) < eps`");
        }
    }
}
