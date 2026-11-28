//! `W0704` (`redundant-annotation`): obvious duplicate type annotations.

use aevia_syntax::SourceMap;
use aevia_syntax::Span;
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
            | (ExprKind::UnitLit, TyKind::Unit) => true,
            | (ExprKind::Annot { ty: inner_ty, .. }, _) => {
                let spelling = cx.sources.text(ty.span);
                !spelling.is_empty() && spelling == cx.sources.text(inner_ty.span)
            },
            | _ => false,
        };
        if redundant && cx.is_enabled(&W0704_REDUNDANT_ANNOTATION, expr.span) {
            let removal = annotation_suffix(cx.sources, inner.span, ty.span);
            let builder = cx
                .emit(
                    &W0704_REDUNDANT_ANNOTATION,
                    expr.span,
                    "redundant type annotation",
                )
                .help("remove the outer `: Type` annotation");
            if let Some(span) = removal {
                builder.fix(Fix {
                    title: "remove redundant type annotation".to_string(),
                    applicability: Applicability::MachineApplicable,
                    edits: vec![TextEdit {
                        span,
                        replacement: String::new(),
                    }],
                });
            }
        }
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "identifies the removable colon and type without consuming expression grouping"
)]
fn annotation_suffix(
    sources: &SourceMap,
    expression: Span,
    ty: Span,
) -> Option<Span> {
    if expression.file != ty.file {
        return None;
    }
    let source = &sources.file(ty.file)?.text;
    let after_expression = usize::try_from(expression.end).ok()?;
    let before_type = usize::try_from(ty.start).ok()?;
    let end = usize::try_from(ty.end).ok()?;
    let (before_colon, after_colon) = source.get(after_expression..before_type)?.split_once(':')?;
    if !before_colon
        .bytes()
        .all(|b| b.is_ascii_whitespace() || b == b')')
        || !after_colon.bytes().all(|b| b.is_ascii_whitespace())
    {
        return None;
    }
    let start = after_expression.checked_add(before_colon.trim_end().len())?;
    (!source.get(start..end)?.contains('#')).then(|| Span::new(ty.file, start, end))
}
