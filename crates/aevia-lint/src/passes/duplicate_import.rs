//! `W0710` (`duplicate-import`): importing the same module path multiple times.

use std::collections::HashSet;

use aevia_syntax::ast::DeclKind;
use aevia_syntax::ast::Module;

use crate::lint::Lint;
use crate::lint::W0710_DUPLICATE_IMPORT;
use crate::pass::LintCx;
use crate::pass::LintPassInfo;
use crate::pass::SyntaxPass;

/// Flags duplicate `import` declarations for the same module path in a module.
pub struct DuplicateImportPass;

impl LintPassInfo for DuplicateImportPass {
    fn lints(&self) -> &'static [&'static Lint] {
        static LINTS: &[&Lint] = &[&W0710_DUPLICATE_IMPORT];
        LINTS
    }
}

impl SyntaxPass for DuplicateImportPass {
    fn check_module(
        &self,
        cx: &mut LintCx<'_>,
        module: &Module,
    ) {
        let mut seen = HashSet::new();
        for decl in &module.decls {
            let DeclKind::Import { path, .. } = &decl.kind else {
                continue;
            };
            if !seen.insert(path.as_slice()) {
                cx.emit(
                    &W0710_DUPLICATE_IMPORT,
                    decl.span,
                    format!("duplicate import of `{}`", path.join(".")),
                )
                .help("combine duplicate imports from the same module into one declaration");
            }
        }
    }
}
