//! `W0709` (`redundant-if`) and `W0711` (`constant-condition`).

use aevia_syntax::ast::Expr;
use aevia_syntax::ast::ExprKind;

use crate::lint::Lint;
use crate::lint::W0709_REDUNDANT_IF;
use crate::lint::W0711_CONSTANT_CONDITION;
use crate::pass::Applicability;
use crate::pass::Fix;
use crate::pass::LintCx;
use crate::pass::LintPassInfo;
use crate::pass::SyntaxPass;
use crate::pass::TextEdit;
use crate::pass::retained_source;
use crate::passes::self_comparison::same_simple_expr;

/// Flags boolean literal conditions and `if` expressions with redundant branches.
pub struct RedundantIfPass;

impl LintPassInfo for RedundantIfPass {
    fn lints(&self) -> &'static [&'static Lint] {
        static LINTS: &[&Lint] = &[&W0709_REDUNDANT_IF, &W0711_CONSTANT_CONDITION];
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
        if let ExprKind::Bool(value) = cond.kind
            && cx.is_enabled(&W0711_CONSTANT_CONDITION, cond.span)
        {
            cx.emit(
                &W0711_CONSTANT_CONDITION,
                cond.span,
                format!("`if` condition is always `{value}`"),
            )
            .help("check whether this condition should depend on a value");
        }
        if !cx.is_enabled(&W0709_REDUNDANT_IF, expr.span) {
            return;
        }
        match (&then_branch.kind, &else_branch.kind) {
            | (ExprKind::Bool(then), ExprKind::Bool(otherwise)) if then != otherwise => {
                let (message, help, title) = if *then {
                    (
                        "redundant `if` returning `true`/`false` directly",
                        "replace with the condition expression directly",
                        "simplify `if` to condition",
                    )
                } else {
                    (
                        "redundant `if` inverting a boolean condition",
                        "replace with `!condition`",
                        "simplify `if` to negated condition",
                    )
                };
                let cond_src = retained_source(cx.sources, expr.span, cond.span);
                let builder = cx.emit(&W0709_REDUNDANT_IF, expr.span, message).help(help);
                if let Some(cond_src) = cond_src {
                    let atomic = matches!(
                        cond.kind,
                        ExprKind::Bool(_) | ExprKind::Var(_) | ExprKind::QName(_)
                    );
                    // A compound condition needs grouping to keep its
                    // precedence when moved into the surrounding expression.
                    let replacement = match (*then, atomic) {
                        | (true, true) => cond_src.to_owned(),
                        | (true, false) => format!("({cond_src})"),
                        | (false, true) => format!("!{cond_src}"),
                        | (false, false) => format!("!({cond_src})"),
                    };
                    builder.fix(Fix {
                        title: title.to_owned(),
                        applicability: Applicability::MachineApplicable,
                        edits: vec![TextEdit {
                            span: expr.span,
                            replacement,
                        }],
                    });
                }
            },
            | _ if same_simple_expr(then_branch, else_branch) => {
                let branch_src = retained_source(cx.sources, expr.span, then_branch.span);
                let builder = cx
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
                    builder.fix(Fix {
                        title: "replace `if` with branch expression".to_string(),
                        applicability,
                        edits: vec![TextEdit {
                            span: expr.span,
                            replacement: branch_src.to_owned(),
                        }],
                    });
                }
            },
            | _ => {},
        }
    }
}
