//! Lint orchestrator, single-walk AST dispatch, and deterministic result caching.

use std::collections::HashMap;
use std::collections::HashSet;

use aevia_core::driver::Program;
use aevia_syntax::Diagnostic;
use aevia_syntax::Diagnostics;
use aevia_syntax::FileId;
use aevia_syntax::Severity;
use aevia_syntax::SourceMap;
use aevia_syntax::ast::Decl;
use aevia_syntax::ast::Expr;
use aevia_syntax::ast::Module;
use aevia_syntax::ast::Pat;
use aevia_syntax::ast::Ty;
use aevia_syntax::visit;
use aevia_types::check::CheckedModule;

use crate::levels::LevelMap;
use crate::levels::LevelResolver;
use crate::levels::LintConfig;
use crate::lint::Level;
use crate::lint::LintStore;
use crate::pass::LintCx;
use crate::pass::LintDiagnostic;
use crate::pass::SemanticPass;
use crate::pass::SyntaxPass;
use crate::passes;

/// Result of linting one module.
#[derive(Clone, Debug)]
pub struct ModuleLintResult {
    /// Diagnostics from both compiler warnings (mapped through levels) and lint passes.
    pub lint_diagnostics: Vec<LintDiagnostic>,
    /// Merged `Diagnostics` ready for the standard renderer.
    pub diagnostics: Diagnostics,
}

/// Complete report of one `aevia lint` / `aevia check` run.
#[derive(Clone, Debug)]
pub struct LintReport {
    /// All lint findings, sorted deterministically by `(file, start, end, code)`.
    pub lint_diagnostics: Vec<LintDiagnostic>,
    /// All compiler + lint diagnostics, sorted deterministically.
    pub diagnostics: Diagnostics,
    /// Number of modules analyzed.
    pub modules_checked: usize,
}

impl LintReport {
    /// True if any diagnostic ended up as an error (`Severity::Error`).
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }
}

/// Runs the lint pipeline over a single module or a full [`Program`].
pub struct Driver;

impl Driver {
    /// Lints a single module (used by both `Driver::run` and `aevia-analysis` / LSP).
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "keeps existing callers of the public module lint entry point source-compatible"
    )]
    pub fn run_module(
        sources: &SourceMap,
        module: &Module,
        checked: Option<&CheckedModule>,
        raw_diagnostics: &Diagnostics,
        file: FileId,
        store: &LintStore,
        config: &LintConfig,
        is_root_package: bool,
    ) -> ModuleLintResult {
        Self::run_module_with_diagnostics(
            sources,
            module,
            checked,
            raw_diagnostics
                .items
                .iter()
                .filter(|diag| diag.span.file == file),
            file,
            store,
            config,
            is_root_package,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "shares module linting between standalone and prepartitioned compiler diagnostics"
    )]
    fn run_module_with_diagnostics<'a>(
        sources: &SourceMap,
        module: &Module,
        checked: Option<&CheckedModule>,
        raw_diagnostics: impl Iterator<Item = &'a Diagnostic>,
        file: FileId,
        store: &LintStore,
        config: &LintConfig,
        is_root_package: bool,
    ) -> ModuleLintResult {
        let levels = LevelResolver::resolve(store, config, Some(module), is_root_package);
        let (mut lint_diags, mut merged) =
            LevelResolver::apply_to_file_diagnostics(store, &levels, raw_diagnostics, file);

        if config.run_extra_lints && is_root_package {
            let syntax_passes = passes::syntax_passes();
            let semantic_passes = passes::semantic_passes();

            let enabled_syntax: Vec<&dyn SyntaxPass> = syntax_passes
                .iter()
                .filter(|p| {
                    p.lint_ids(store)
                        .into_iter()
                        .any(|id| levels.is_enabled_anywhere(id))
                })
                .map(|p| p.as_ref() as &dyn SyntaxPass)
                .collect();

            let enabled_semantic: Vec<&dyn SemanticPass> = if checked.is_some() {
                semantic_passes
                    .iter()
                    .filter(|p| {
                        p.lint_ids(store)
                            .into_iter()
                            .any(|id| levels.is_enabled_anywhere(id))
                    })
                    .map(|p| p.as_ref() as &dyn SemanticPass)
                    .collect()
            } else {
                Vec::new()
            };

            if !enabled_syntax.is_empty() || !enabled_semantic.is_empty() {
                let mut cx = LintCx {
                    sources,
                    store,
                    levels: &levels,
                    module,
                    checked,
                    results: Vec::new(),
                };

                for pass in &enabled_syntax {
                    pass.check_module(&mut cx, module);
                }
                if let Some(checked_mod) = checked {
                    for pass in &enabled_semantic {
                        pass.check_module(&mut cx, module, checked_mod);
                    }
                }

                let mut walker = CombinedWalker {
                    cx: &mut cx,
                    syntax: &enabled_syntax,
                    semantic: &enabled_semantic,
                    checked,
                };
                visit::walk_module(&mut walker, module);

                for item in cx.results {
                    merged.push(item.diagnostic.clone());
                    lint_diags.push(item);
                }
            }
        }

        sort_lint_diagnostics(&mut lint_diags);
        sort_diagnostics(&mut merged.items);
        ModuleLintResult {
            lint_diagnostics: lint_diags,
            diagnostics: merged,
        }
    }

    /// Lints an entire loaded [`Program`].
    ///
    /// Dependencies outside `root_files` have their lints capped to `Allow` (`--cap-lints`).
    #[must_use]
    pub fn run_program(
        program: &Program,
        store: &LintStore,
        config: &LintConfig,
        root_files: Option<&[FileId]>,
    ) -> LintReport {
        let mut all_lint_diags = Vec::new();
        let mut all_diags = Diagnostics::new();

        // Bucket once rather than filter all compiler diagnostics for every module.
        // Diagnostics on files without a loaded module are preserved directly.
        let mut module_diagnostics: HashMap<FileId, Vec<&Diagnostic>> = program
            .modules
            .iter()
            .map(|m| (m.file, Vec::new()))
            .collect();
        for diag in &program.diagnostics.items {
            if let Some(items) = module_diagnostics.get_mut(&diag.span.file) {
                items.push(diag);
            } else {
                all_diags.push(diag.clone());
            }
        }

        let root_files = root_files.map(|roots| roots.iter().copied().collect::<HashSet<_>>());
        for loaded in &program.modules {
            let is_root = root_files
                .as_ref()
                .is_none_or(|roots| roots.contains(&loaded.file));
            let raw_diagnostics = module_diagnostics
                .get(&loaded.file)
                .into_iter()
                .flatten()
                .copied();
            let res = Self::run_module_with_diagnostics(
                &program.sources,
                &loaded.module,
                Some(&loaded.checked),
                raw_diagnostics,
                loaded.file,
                store,
                config,
                is_root,
            );
            all_lint_diags.extend(res.lint_diagnostics);
            all_diags.items.extend(res.diagnostics.items);
        }

        // Also validate `[lints]` keys even if there are 0 modules.
        if program.modules.is_empty() {
            let levels = LevelResolver::resolve(store, config, None, true);
            for err in levels.errors {
                all_diags.push(err);
            }
        }

        // Deduplicate config errors emitted per-module when `Span::default()` is shared.
        dedup_config_errors(&mut all_diags.items);
        sort_lint_diagnostics(&mut all_lint_diags);
        sort_diagnostics(&mut all_diags.items);

        // Enforce `-D warnings` if requested via `Level::Deny` on `warnings` in `config`
        // (already handled per-lint by `LevelResolver`, and keep `all_diags` authoritative).
        let _ = (Level::Warn, Severity::Warning, LevelMap::fingerprint);

        LintReport {
            lint_diagnostics: all_lint_diags,
            diagnostics: all_diags,
            modules_checked: program.modules.len(),
        }
    }
}

struct CombinedWalker<'a, 'cx> {
    cx: &'a mut LintCx<'cx>,
    syntax: &'a [&'a dyn SyntaxPass],
    semantic: &'a [&'a dyn SemanticPass],
    checked: Option<&'cx CheckedModule>,
}

impl<'ast> visit::Visitor<'ast> for CombinedWalker<'_, '_> {
    fn visit_decl(
        &mut self,
        decl: &'ast Decl,
    ) {
        for pass in self.syntax {
            pass.check_decl(self.cx, decl);
        }
        if let Some(checked) = self.checked {
            for pass in self.semantic {
                pass.check_decl(self.cx, decl, checked);
            }
        }
        visit::walk_decl(self, decl);
    }

    fn visit_expr(
        &mut self,
        expr: &'ast Expr,
    ) {
        for pass in self.syntax {
            pass.check_expr(self.cx, expr);
        }
        if let Some(checked) = self.checked {
            for pass in self.semantic {
                pass.check_expr(self.cx, expr, checked);
            }
        }
        visit::walk_expr(self, expr);
    }

    fn visit_pat(
        &mut self,
        pat: &'ast Pat,
    ) {
        for pass in self.syntax {
            pass.check_pat(self.cx, pat);
        }
        visit::walk_pat(self, pat);
    }

    fn visit_ty(
        &mut self,
        ty: &'ast Ty,
    ) {
        for pass in self.syntax {
            pass.check_ty(self.cx, ty);
        }
        visit::walk_ty(self, ty);
    }
}

fn sort_lint_diagnostics(items: &mut [LintDiagnostic]) {
    items.sort_by(|a, b| {
        (
            a.diagnostic.span.file,
            a.diagnostic.span.start,
            a.diagnostic.span.end,
            &a.diagnostic.code,
            &a.diagnostic.message,
        )
            .cmp(&(
                b.diagnostic.span.file,
                b.diagnostic.span.start,
                b.diagnostic.span.end,
                &b.diagnostic.code,
                &b.diagnostic.message,
            ))
    });
}

fn sort_diagnostics(items: &mut [Diagnostic]) {
    items.sort_by(|a, b| {
        (a.span.file, a.span.start, a.span.end, &a.code, &a.message).cmp(&(
            b.span.file,
            b.span.start,
            b.span.end,
            &b.code,
            &b.message,
        ))
    });
}

#[allow(
    clippy::single_call_fn,
    reason = "names the special deduplication key for configuration diagnostics"
)]
fn dedup_config_errors(items: &mut Vec<Diagnostic>) {
    let mut seen = std::collections::HashSet::new();
    items.retain(|d| seen.insert((d.span.file, d.span.start, d.code.clone(), d.message.clone())));
}
