//! `W0710` (`duplicate-import`): importing the same module path multiple times.

use std::collections::HashMap;

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
        let mut seen: HashMap<String, usize> = HashMap::new();
        for (idx, decl) in module.decls.iter().enumerate() {
            let DeclKind::Import { path, .. } = &decl.kind else {
                continue;
            };
            let key = path.join(".");
            if seen.insert(key.clone(), idx).is_some() {
                cx.emit(
                    &W0710_DUPLICATE_IMPORT,
                    decl.span,
                    format!("duplicate import of `{key}`"),
                )
                .help("combine duplicate imports from the same module into one declaration");
            }
        }
    }
}
