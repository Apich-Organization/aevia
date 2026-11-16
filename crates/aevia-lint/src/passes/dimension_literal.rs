//! `W0707` (`dimension-literal-default`): bare `0`/`0.0` coerced to a non-dimensionless quantity.

use aevia_syntax::ast::Expr;
use aevia_syntax::ast::ExprKind;
use aevia_types::check::CheckedModule;
use aevia_types::ty::Type;

use crate::lint::Lint;
use crate::lint::W0707_DIMENSION_LITERAL_DEFAULT;
use crate::pass::LintCx;
use crate::pass::LintPassInfo;
use crate::pass::SemanticPass;

/// Flags bare `0` or `0.0` literals that acquire a non-dimensionless unit from context.
pub struct DimensionLiteralPass;

impl LintPassInfo for DimensionLiteralPass {
    fn lints(&self) -> &'static [&'static Lint] {
        static LINTS: &[&Lint] = &[&W0707_DIMENSION_LITERAL_DEFAULT];
        LINTS
    }
}

impl SemanticPass for DimensionLiteralPass {
    fn check_expr(
        &self,
        cx: &mut LintCx<'_>,
        expr: &Expr,
        checked: &CheckedModule,
    ) {
        let is_zero = match &expr.kind {
            | ExprKind::Int(n) => n.to_string() == "0",
            | ExprKind::Float(s) => s.replace('_', "").parse::<f64>().is_ok_and(|f| f == 0.0),
            | _ => false,
        };
        if !is_zero {
            return;
        }
        let Some(ty) = checked.expr_types.get(&expr.span) else {
            return;
        };
        let dim = match ty {
            | Type::Scalar(_, d) | Type::Point(d) => d,
            | _ => return,
        };
        if *dim != aevia_dim::Dim::one() {
            cx.emit(
                &W0707_DIMENSION_LITERAL_DEFAULT,
                expr.span,
                format!("bare zero literal coerced to dimension `{dim}`"),
            )
            .help("write an explicit unit (e.g. `0 m`) so the physical unit is visible");
        }
    }
}
