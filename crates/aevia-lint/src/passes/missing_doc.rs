//! `W0703` (`missing-doc-public`): public declarations without `##` documentation.

use aevia_syntax::ast::Decl;
use aevia_syntax::ast::DeclKind;
use aevia_syntax::ast::PatKind;

use crate::lint::Lint;
use crate::lint::W0703_MISSING_DOC_PUBLIC;
use crate::pass::LintCx;
use crate::pass::LintPassInfo;
use crate::pass::SyntaxPass;

/// Pedantic lint requiring `##` docs on public items.
pub struct MissingDocPass;

impl LintPassInfo for MissingDocPass {
    fn lints(&self) -> &'static [&'static Lint] {
        static LINTS: &[&Lint] = &[&W0703_MISSING_DOC_PUBLIC];
        LINTS
    }
}

impl SyntaxPass for MissingDocPass {
    fn check_decl(
        &self,
        cx: &mut LintCx<'_>,
        decl: &Decl,
    ) {
        if decl.private || decl.doc.is_some() {
            return;
        }
        let label = match &decl.kind {
            | DeclKind::Fn { name, .. } => Some(format!("function `{name}`")),
            | DeclKind::Let { pattern, .. } => {
                if let PatKind::Var(name) = &pattern.kind {
                    Some(format!("binding `{name}`"))
                } else {
                    Some("public binding".to_string())
                }
            },
            | DeclKind::Type { name, .. } => Some(format!("type `{name}`")),
            | DeclKind::Class { name, .. } => Some(format!("class `{name}`")),
            | DeclKind::Import { .. }
            | DeclKind::Dimension { .. }
            | DeclKind::Unit { .. }
            | DeclKind::Instance { .. }
            | DeclKind::Derive { .. }
            | DeclKind::Test { .. } => None,
        };
        if let Some(label) = label {
            cx.emit(
                &W0703_MISSING_DOC_PUBLIC,
                decl.span,
                format!("missing documentation for public {label}"),
            )
            .help("add a `##` doc comment immediately above this declaration");
        }
    }
}
