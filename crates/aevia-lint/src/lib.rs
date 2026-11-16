//! Aevia lint engine: shared between `aevia lint`, `aevia check`, CI, and `aevia-lsp`.

#![forbid(unsafe_code)]

pub mod driver;
pub mod fix;
pub mod format;
pub mod levels;
#[allow(missing_docs)]
pub mod lint;
pub mod pass;
pub mod passes;

pub use driver::Driver;
pub use driver::LintReport;
pub use driver::ModuleLintResult;
pub use fix::FixEngine;
pub use fix::FixSummary;
pub use format::OutputFormat;
pub use format::render;
pub use format::render_json;
pub use format::render_sarif;
pub use levels::CliOverride;
pub use levels::LevelMap;
pub use levels::LevelResolver;
pub use levels::LintConfig;
pub use lint::Level;
pub use lint::Lint;
pub use lint::LintGroup;
pub use lint::LintId;
pub use lint::LintStore;
pub use pass::Applicability;
pub use pass::Fix;
pub use pass::LintCx;
pub use pass::LintDiagnostic;
pub use pass::TextEdit;
