//! `W0709` (`redundant-if`): `if` expressions that can be simplified.

use aevia_syntax::SourceMap;
use aevia_syntax::Span;
use aevia_syntax::ast::Expr;
use aevia_syntax::ast::ExprKind;

use crate::lint::Lint;
use crate::lint::W0709_REDUNDANT_IF;
use crate::pass::Applicability;
use crate::pass::Fix;
use crate::pass::LintCx;
use crate::pass::LintPassInfo;
use crate::pass::SyntaxPass;
use crate::pass::TextEdit;
use crate::passes::self_comparison::same_simple_expr;

/// Flags `if c then true else false`, `if c then false else true`, and `if c then x else x`.
pub struct RedundantIfPass;

impl LintPassInfo for RedundantIfPass {
    fn lints(&self) -> &'static [&'static Lint] {
        static LINTS: &[&Lint] = &[&W0709_REDUNDANT_IF];
        LINTS
    }
}

impl SyntaxPass for RedundantIfPass {
    fn check_expr(
        &self,
        cx: &mut LintCx<'_>,
        expr: &Expr,
    ) {
        let ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } = &expr.kind
        else {
            return;
        };
        match (&then_branch.kind, &else_branch.kind) {
            | (ExprKind::Bool(true), ExprKind::Bool(false)) => {
                let cond_src = slice_span(cx.sources, cond.span);
                let mut builder = cx
                    .emit(
                        &W0709_REDUNDANT_IF,
                        expr.span,
                        "redundant `if` returning `true`/`false` directly",
                    )
                    .help("replace with the condition expression directly");
                if let Some(cond_src) = cond_src {
                    builder = builder.fix(Fix {
                        title: "simplify `if` to condition".to_string(),
                        applicability: Applicability::MachineApplicable,
                        edits: vec![TextEdit {
                            span: expr.span,
                            replacement: cond_src,
                        }],
                    });
                }
                let _ = builder;
            },
            | (ExprKind::Bool(false), ExprKind::Bool(true)) => {
                let cond_src = slice_span(cx.sources, cond.span);
                let mut builder = cx
                    .emit(
                        &W0709_REDUNDANT_IF,
                        expr.span,
                        "redundant `if` inverting a boolean condition",
                    )
                    .help("replace with `!condition`");
                if let Some(cond_src) = cond_src {
                    let replacement = if matches!(cond.kind, ExprKind::Var(_) | ExprKind::QName(_))
                    {
                        format!("!{cond_src}")
                    } else {
                        format!("!({cond_src})")
                    };
                    builder = builder.fix(Fix {
                        title: "simplify `if` to negated condition".to_string(),
                        applicability: Applicability::MachineApplicable,
                        edits: vec![TextEdit {
                            span: expr.span,
                            replacement,
                        }],
                    });
                }
                let _ = builder;
            },
            | _ if same_simple_expr(then_branch, else_branch) => {
                let branch_src = slice_span(cx.sources, then_branch.span);
                let mut builder = cx
                    .emit(
                        &W0709_REDUNDANT_IF,
                        expr.span,
                        "`if` expression has identical `then` and `else` branches",
                    )
                    .help("both branches evaluate to the same value");
                if let Some(branch_src) = branch_src {
                    let applicability = if matches!(cond.kind, ExprKind::Bool(_)) {
                        Applicability::MachineApplicable
                    } else {
                        // Removing the condition also removes errors it can raise.
                        Applicability::MaybeIncorrect
                    };
                    builder = builder.fix(Fix {
                        title: "replace `if` with branch expression".to_string(),
                        applicability,
                        edits: vec![TextEdit {
                            span: expr.span,
                            replacement: branch_src,
                        }],
                    });
                }
                let _ = builder;
            },
            | _ => {},
        }
    }
}

fn slice_span(
    sources: &SourceMap,
    span: Span,
) -> Option<String> {
    let file = sources.file(span.file)?;
    let start = usize::try_from(span.start).ok()?;
    let end = usize::try_from(span.end).ok()?;
    file.text.get(start..end).map(str::to_string)
}
