//! Lint pass traits, emission context, and structured fixes.

use aevia_syntax::Diagnostic;
use aevia_syntax::Severity;
use aevia_syntax::SourceMap;
use aevia_syntax::Span;
use aevia_syntax::ast::Decl;
use aevia_syntax::ast::Expr;
use aevia_syntax::ast::Module;
use aevia_syntax::ast::Pat;
use aevia_syntax::ast::Ty;
use aevia_types::check::CheckedModule;

use crate::levels::LevelMap;
use crate::lint::Level;
use crate::lint::Lint;
use crate::lint::LintId;
use crate::lint::LintStore;

/// How safe an automated fix is to apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Applicability {
    /// Safe to apply blindly via `aevia lint --fix`.
    MachineApplicable,
    /// Likely right, shown as an editor quick-fix but not applied by `--fix`.
    MaybeIncorrect,
    /// Descriptive hint only.
    Manual,
}

/// A single byte-range text replacement.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TextEdit {
    /// Span to replace.
    pub span: Span,
    /// Replacement text.
    pub replacement: String,
}

/// A suggested fix attached to a [`LintDiagnostic`].
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fix {
    /// Short label shown in editors and CLI.
    pub title: String,
    /// Safety tier.
    pub applicability: Applicability,
    /// Disjoint byte-range edits.
    pub edits: Vec<TextEdit>,
}

/// A lint finding together with its resolved level and optional fixes.
#[derive(Clone, Debug)]
pub struct LintDiagnostic {
    /// Underlying compiler-formatted diagnostic.
    pub diagnostic: Diagnostic,
    /// Descriptor of the lint that fired.
    pub lint: &'static Lint,
    /// Effective level (`Warn`, `Deny`, or `Forbid`; `Allow` is never emitted).
    pub level: Level,
    /// Optional automated fixes.
    pub fixes: Vec<Fix>,
}

/// Builder returned by [`LintCx::emit`].
pub struct DiagnosticBuilder<'b, 'a> {
    cx: &'b mut LintCx<'a>,
    index: Option<usize>,
}

impl DiagnosticBuilder<'_, '_> {
    /// Attaches a `help:` line.
    pub fn help(
        self,
        message: impl Into<String>,
    ) -> Self {
        if let Some(i) = self.index {
            self.cx.results[i].diagnostic.helps.push(message.into());
        }
        self
    }

    /// Attaches a `= note:` line.
    pub fn note(
        self,
        message: impl Into<String>,
    ) -> Self {
        if let Some(i) = self.index {
            self.cx.results[i].diagnostic.notes.push(message.into());
        }
        self
    }

    /// Attaches a structured fix.
    pub fn fix(
        self,
        fix: Fix,
    ) -> Self {
        if let Some(i) = self.index {
            self.cx.results[i].fixes.push(fix);
        }
        self
    }
}

/// Emission context passed to every lint pass.
pub struct LintCx<'a> {
    /// Source map for reading raw source text / line boundaries.
    pub sources: &'a SourceMap,
    /// Central lint registry.
    pub store: &'a LintStore,
    /// Resolved per-scope levels for the current module.
    pub levels: &'a LevelMap,
    /// Current module AST.
    pub module: &'a Module,
    /// Checked module data when available.
    pub checked: Option<&'a CheckedModule>,
    pub(crate) results: Vec<LintDiagnostic>,
}

impl<'a> LintCx<'a> {
    /// Returns true if `lint` is enabled at `span`.
    #[must_use]
    pub fn is_enabled(
        &self,
        lint: &'static Lint,
        span: Span,
    ) -> bool {
        let Some((id, _)) = self.store.find_by_code(lint.code) else {
            return false;
        };
        self.levels.level_at(id, span) != Level::Allow
    }

    /// Emits a finding for `lint` at `span` if not suppressed (`Allow`).
    pub fn emit<'b>(
        &'b mut self,
        lint: &'static Lint,
        span: Span,
        message: impl Into<String>,
    ) -> DiagnosticBuilder<'b, 'a> {
        let Some((id, _)) = self.store.find_by_code(lint.code) else {
            return DiagnosticBuilder {
                cx: self,
                index: None,
            };
        };
        let level = self.levels.level_at(id, span);
        if level == Level::Allow {
            return DiagnosticBuilder {
                cx: self,
                index: None,
            };
        }
        let mut diag = if matches!(level, Level::Deny | Level::Forbid) {
            Diagnostic::error(lint.code, message, span)
        } else {
            Diagnostic::warning(lint.code, message, span)
        };
        diag.severity = match level {
            | Level::Warn => Severity::Warning,
            | Level::Deny | Level::Forbid => Severity::Error,
            | Level::Allow => unreachable!(),
        };
        self.results.push(LintDiagnostic {
            diagnostic: diag,
            lint,
            level,
            fixes: Vec::new(),
        });
        let index = Some(self.results.len() - 1);
        DiagnosticBuilder { cx: self, index }
    }
}

/// Metadata shared by all lint passes.
pub trait LintPassInfo {
    /// Lints this pass can emit; if all are `Allow` in a module, the pass is skipped.
    fn lints(&self) -> &'static [&'static Lint];

    /// Helper returning matching `LintId`s in `store`.
    fn lint_ids(
        &self,
        store: &LintStore,
    ) -> Vec<LintId> {
        self.lints()
            .iter()
            .filter_map(|l| store.find_by_code(l.code).map(|(id, _)| id))
            .collect()
    }
}

/// Syntactic lint pass (runs whenever parsing succeeded).
pub trait SyntaxPass: LintPassInfo {
    /// Runs once per module before node hooks.
    fn check_module(
        &self,
        _cx: &mut LintCx<'_>,
        _module: &Module,
    ) {
    }

    /// Runs on each declaration.
    fn check_decl(
        &self,
        _cx: &mut LintCx<'_>,
        _decl: &Decl,
    ) {
    }

    /// Runs on each expression.
    fn check_expr(
        &self,
        _cx: &mut LintCx<'_>,
        _expr: &Expr,
    ) {
    }

    /// Runs on each pattern.
    fn check_pat(
        &self,
        _cx: &mut LintCx<'_>,
        _pat: &Pat,
    ) {
    }

    /// Runs on each type annotation.
    fn check_ty(
        &self,
        _cx: &mut LintCx<'_>,
        _ty: &Ty,
    ) {
    }
}

/// Semantic lint pass (runs when `CheckedModule` is available).
pub trait SemanticPass: LintPassInfo {
    /// Runs once per checked module.
    fn check_module(
        &self,
        _cx: &mut LintCx<'_>,
        _module: &Module,
        _checked: &CheckedModule,
    ) {
    }

    /// Runs on each declaration.
    fn check_decl(
        &self,
        _cx: &mut LintCx<'_>,
        _decl: &Decl,
        _checked: &CheckedModule,
    ) {
    }

    /// Runs on each expression.
    fn check_expr(
        &self,
        _cx: &mut LintCx<'_>,
        _expr: &Expr,
        _checked: &CheckedModule,
    ) {
    }
}
