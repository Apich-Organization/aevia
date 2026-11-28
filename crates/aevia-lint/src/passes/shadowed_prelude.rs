//! `W0705` (`shadowed-prelude-name`): local/top-level bindings that shadow prelude builtins.

use aevia_syntax::ast::Decl;
use aevia_syntax::ast::DeclKind;
use aevia_syntax::ast::Pat;
use aevia_syntax::ast::PatKind;

use crate::lint::Lint;
use crate::lint::W0705_SHADOWED_PRELUDE_NAME;
use crate::pass::LintCx;
use crate::pass::LintPassInfo;
use crate::pass::SyntaxPass;

const PRELUDE_NAMES: &[&str] = &[
    "abs", "acos", "asin", "atan", "atan2", "ceil", "cos", "cosh", "exp", "filter", "floor",
    "fold", "len", "ln", "log10", "map", "max", "min", "ones", "println", "print", "range",
    "round", "sin", "sinh", "sqrt", "sum", "tan", "tanh", "zeros",
];

/// Flags declarations or pattern bindings that shadow a common prelude builtin.
pub struct ShadowedPreludePass;

impl LintPassInfo for ShadowedPreludePass {
    fn lints(&self) -> &'static [&'static Lint] {
        static LINTS: &[&Lint] = &[&W0705_SHADOWED_PRELUDE_NAME];
        LINTS
    }
}

impl SyntaxPass for ShadowedPreludePass {
    fn check_decl(
        &self,
        cx: &mut LintCx<'_>,
        decl: &Decl,
    ) {
        if let DeclKind::Fn { name, .. } = &decl.kind
            && is_prelude_name(name)
        {
            cx.emit(
                &W0705_SHADOWED_PRELUDE_NAME,
                decl.span,
                format!("function `{name}` shadows the prelude builtin `{name}`"),
            );
        }
    }

    fn check_pat(
        &self,
        cx: &mut LintCx<'_>,
        pat: &Pat,
    ) {
        if let PatKind::Var(name) = &pat.kind
            && is_prelude_name(name)
        {
            cx.emit(
                &W0705_SHADOWED_PRELUDE_NAME,
                pat.span,
                format!("binding `{name}` shadows the prelude builtin `{name}`"),
            );
        }
    }
}

fn is_prelude_name(name: &str) -> bool {
    PRELUDE_NAMES.contains(&name)
}
