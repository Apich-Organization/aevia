//! `W0704` (`redundant-annotation`): obvious duplicate type annotations.

use aevia_syntax::ast::Expr;
use aevia_syntax::ast::ExprKind;
use aevia_syntax::ast::TyKind;

use crate::lint::Lint;
use crate::lint::W0704_REDUNDANT_ANNOTATION;
use crate::pass::Applicability;
use crate::pass::Fix;
use crate::pass::LintCx;
use crate::pass::LintPassInfo;
use crate::pass::SyntaxPass;
use crate::pass::TextEdit;

/// Flags redundant annotations like `(true : Bool)` or `(("hi" : String) : String)`.
pub struct RedundantAnnotationPass;

impl LintPassInfo for RedundantAnnotationPass {
    fn lints(&self) -> &'static [&'static Lint] {
        static LINTS: &[&Lint] = &[&W0704_REDUNDANT_ANNOTATION];
        LINTS
    }
}

impl SyntaxPass for RedundantAnnotationPass {
    fn check_expr(
        &self,
        cx: &mut LintCx<'_>,
        expr: &Expr,
    ) {
        let ExprKind::Annot { expr: inner, ty } = &expr.kind else {
            return;
        };
        let redundant = match (&inner.kind, &ty.kind) {
            | (ExprKind::Bool(_), TyKind::Bool)
            | (ExprKind::Str(_), TyKind::Str)
            | (ExprKind::UnitLit, TyKind::Unit)
            | (ExprKind::Annot { .. }, _) => true,
            | _ => false,
        };
        if redundant {
            let replacement = cx.sources.file(inner.span.file).and_then(|file| {
                let start = usize::try_from(inner.span.start).ok()?;
                let end = usize::try_from(inner.span.end).ok()?;
                file.text.get(start..end).map(str::to_string)
            });
            let builder = cx
                .emit(
                    &W0704_REDUNDANT_ANNOTATION,
                    expr.span,
                    "redundant type annotation",
                )
                .help("remove the outer `: Type` annotation");
            if let Some(inner_text) = replacement {
                builder.fix(Fix {
                    title: "remove redundant type annotation".to_string(),
                    applicability: Applicability::MachineApplicable,
                    edits: vec![TextEdit {
                        span: expr.span,
                        replacement: inner_text,
                    }],
                });
            }
        }
    }
}
