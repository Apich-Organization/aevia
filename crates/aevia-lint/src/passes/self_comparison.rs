//! `W0708` (`self-comparison`): comparing an identical expression to itself.

use aevia_syntax::ast::BinOp;
use aevia_syntax::ast::Expr;
use aevia_syntax::ast::ExprKind;

use crate::lint::Lint;
use crate::lint::W0708_SELF_COMPARISON;
use crate::pass::LintCx;
use crate::pass::LintPassInfo;
use crate::pass::SyntaxPass;

/// Flags `x == x`, `x != x`, `x < x`, `x <= x`, `x > x`, `x >= x` on simple pure expressions.
pub struct SelfComparisonPass;

impl LintPassInfo for SelfComparisonPass {
    fn lints(&self) -> &'static [&'static Lint] {
        static LINTS: &[&Lint] = &[&W0708_SELF_COMPARISON];
        LINTS
    }
}

impl SyntaxPass for SelfComparisonPass {
    fn check_expr(
        &self,
        cx: &mut LintCx<'_>,
        expr: &Expr,
    ) {
        let ExprKind::Binary { op, left, right } = &expr.kind else {
            return;
        };
        let op_str = match op {
            | BinOp::Eq => "==",
            | BinOp::Ne => "!=",
            | BinOp::Lt => "<",
            | BinOp::Le => "<=",
            | BinOp::Gt => ">",
            | BinOp::Ge => ">=",
            | _ => return,
        };
        if same_simple_expr(left, right) {
            let builder = cx.emit(
                &W0708_SELF_COMPARISON,
                expr.span,
                format!("comparing an expression to itself with `{op_str}`"),
            );
            if *op == BinOp::Ne {
                builder.help("if checking for `NaN`, use `is_nan(x)` instead");
            } else {
                builder.help("check whether one side was meant to be a different variable");
            }
        }
    }
}

pub(crate) fn same_simple_expr(
    a: &Expr,
    b: &Expr,
) -> bool {
    match (&a.kind, &b.kind) {
        | (ExprKind::UnitLit, ExprKind::UnitLit) => true,
        | (ExprKind::Bool(x), ExprKind::Bool(y)) => x == y,
        | (ExprKind::Int(x), ExprKind::Int(y)) => x == y,
        | (ExprKind::Var(x), ExprKind::Var(y)) => x == y,
        | (ExprKind::QName(x), ExprKind::QName(y)) => x == y,
        | (ExprKind::Field { base: ba, name: na }, ExprKind::Field { base: bb, name: nb }) => {
            na == nb && same_simple_expr(ba, bb)
        },
        | _ => false,
    }
}
