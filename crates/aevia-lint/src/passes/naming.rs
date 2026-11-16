//! `W0701` (`non-snake-case`) and `W0702` (`non-camel-case-type`).

use aevia_syntax::ast::Decl;
use aevia_syntax::ast::DeclKind;
use aevia_syntax::ast::Pat;
use aevia_syntax::ast::PatKind;
use aevia_syntax::ast::TypeBody;

use crate::lint::Lint;
use crate::lint::W0701_NON_SNAKE_CASE;
use crate::lint::W0702_NON_CAMEL_CASE_TYPE;
use crate::pass::LintCx;
use crate::pass::LintPassInfo;
use crate::pass::SyntaxPass;

/// Checks `snake_case` for functions/bindings and `UpperCamelCase` for types/classes/constructors.
pub struct NamingPass;

impl LintPassInfo for NamingPass {
    fn lints(&self) -> &'static [&'static Lint] {
        static LINTS: &[&Lint] = &[&W0701_NON_SNAKE_CASE, &W0702_NON_CAMEL_CASE_TYPE];
        LINTS
    }
}

impl SyntaxPass for NamingPass {
    fn check_decl(
        &self,
        cx: &mut LintCx<'_>,
        decl: &Decl,
    ) {
        match &decl.kind {
            | DeclKind::Fn { name, .. } => {
                if name != "main" && !is_snake_case(name) {
                    cx.emit(
                        &W0701_NON_SNAKE_CASE,
                        decl.span,
                        format!("function `{name}` should have a `snake_case` name"),
                    )
                    .help(format!("consider renaming to `{}`", to_snake_case(name)));
                }
            },
            | DeclKind::Type { name, body, .. } => {
                if !is_upper_camel_case(name) {
                    cx.emit(
                        &W0702_NON_CAMEL_CASE_TYPE,
                        decl.span,
                        format!("type `{name}` should have an `UpperCamelCase` name"),
                    )
                    .help(format!(
                        "consider renaming to `{}`",
                        to_upper_camel_case(name)
                    ));
                }
                if let TypeBody::Variants(ctors) = body {
                    for ctor in ctors {
                        if !is_upper_camel_case(&ctor.name) {
                            cx.emit(
                                &W0702_NON_CAMEL_CASE_TYPE,
                                ctor.span,
                                format!(
                                    "constructor `{}` should have an `UpperCamelCase` name",
                                    ctor.name
                                ),
                            )
                            .help(format!(
                                "consider renaming to `{}`",
                                to_upper_camel_case(&ctor.name)
                            ));
                        }
                    }
                }
            },
            | DeclKind::Class { name, .. } => {
                if !is_upper_camel_case(name) {
                    cx.emit(
                        &W0702_NON_CAMEL_CASE_TYPE,
                        decl.span,
                        format!("class `{name}` should have an `UpperCamelCase` name"),
                    )
                    .help(format!(
                        "consider renaming to `{}`",
                        to_upper_camel_case(name)
                    ));
                }
            },
            | _ => {},
        }
    }

    fn check_pat(
        &self,
        cx: &mut LintCx<'_>,
        pat: &Pat,
    ) {
        if let PatKind::Var(name) = &pat.kind
            && !is_snake_case(name)
        {
            cx.emit(
                &W0701_NON_SNAKE_CASE,
                pat.span,
                format!("variable `{name}` should have a `snake_case` name"),
            )
            .help(format!("consider renaming to `{}`", to_snake_case(name)));
        }
    }
}

fn is_snake_case(name: &str) -> bool {
    if name.starts_with('_') {
        return true;
    }
    !name.chars().any(|c| c.is_uppercase())
}

fn is_upper_camel_case(name: &str) -> bool {
    let Some(first) = name.chars().next() else {
        return true;
    };
    first.is_uppercase() && !name.contains('_')
}

fn to_snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 && !out.ends_with('_') {
                out.push('_');
            }
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn to_upper_camel_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for part in name.split('_').filter(|s| !s.is_empty()) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            for upper in first.to_uppercase() {
                out.push(upper);
            }
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() {
        name.to_string()
    } else {
        out
    }
}
