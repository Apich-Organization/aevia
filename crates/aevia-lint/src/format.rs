//! Output renderers for `aevia lint`: `human`, `json`, and `sarif` (2.1.0).

use aevia_syntax::Diagnostic;
use aevia_syntax::Severity;
use aevia_syntax::SourceMap;
use aevia_syntax::Span;
use aevia_syntax::Style;
use std::collections::HashMap;

use crate::driver::LintReport;
use crate::lint::Level;
use crate::lint::LintStore;
use crate::pass::Applicability;
use crate::pass::Fix;

/// CLI output format for `aevia lint`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    /// Compiler-style human diagnostics.
    Human,
    /// Stable JSON report.
    Json,
    /// SARIF 2.1.0 JSON log.
    Sarif,
}

impl OutputFormat {
    /// Parses `"human"`, `"json"`, or `"sarif"`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            | "human" => Some(Self::Human),
            | "json" => Some(Self::Json),
            | "sarif" => Some(Self::Sarif),
            | _ => None,
        }
    }
}

/// Renders `report` in the requested [`OutputFormat`].
#[must_use]
pub fn render(
    report: &LintReport,
    sources: &SourceMap,
    store: &LintStore,
    format: OutputFormat,
    style: Style,
) -> String {
    match format {
        | OutputFormat::Human => report.diagnostics.render_with(sources, style),
        | OutputFormat::Json => render_json(report, sources, store),
        | OutputFormat::Sarif => render_sarif(report, sources, store),
    }
}

/// Renders ordered reports using the source map belonging to each report.
///
/// Structured formats use a single envelope for all diagnostics.
#[must_use]
pub fn render_many<'a>(
    reports: impl IntoIterator<Item = (&'a LintReport, &'a SourceMap)>,
    store: &LintStore,
    format: OutputFormat,
    style: Style,
) -> String {
    match format {
        | OutputFormat::Human => {
            reports
                .into_iter()
                .map(|(report, sources)| report.diagnostics.render_with(sources, style))
                .collect()
        },
        | OutputFormat::Json => render_json_many(reports, store),
        | OutputFormat::Sarif => render_sarif_many(reports, store),
    }
}

#[derive(serde::Serialize)]
struct JsonReport<'a> {
    version: u32,
    has_errors: bool,
    diagnostics: Vec<JsonDiag<'a>>,
}

#[derive(serde::Serialize)]
struct JsonFix<'a> {
    title: &'a str,
    applicability: &'static str,
    edits: Vec<JsonEdit<'a>>,
}

#[derive(serde::Serialize)]
struct JsonEdit<'a> {
    file: String,
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
    replacement: &'a str,
}

#[derive(serde::Serialize)]
struct JsonDiag<'a> {
    code: &'a str,
    name: Option<&'a str>,
    severity: &'static str,
    file: String,
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
    message: &'a str,
    helps: &'a [String],
    notes: &'a [String],
    fixes: Vec<JsonFix<'a>>,
}

/// Renders a stable JSON report.
#[must_use]
pub fn render_json(
    report: &LintReport,
    sources: &SourceMap,
    store: &LintStore,
) -> String {
    render_json_many([(report, sources)], store)
}

fn render_json_many<'a>(
    reports: impl IntoIterator<Item = (&'a LintReport, &'a SourceMap)>,
    store: &LintStore,
) -> String {
    // For smaller reports a linear search avoids the cost of building an index.
    const MIN_INDEXED_REPORT_SIZE: usize = 1024;
    let mut has_errors = false;
    let mut diagnostics = Vec::new();
    for (report, sources) in reports {
        has_errors |= report.has_errors();
        let fix_index = (report.diagnostics.items.len() >= MIN_INDEXED_REPORT_SIZE
            && report.lint_diagnostics.len() >= MIN_INDEXED_REPORT_SIZE)
            .then(|| {
                let mut fixes = HashMap::with_capacity(report.lint_diagnostics.len());
                for lint in &report.lint_diagnostics {
                    fixes
                        .entry(diagnostic_key(&lint.diagnostic))
                        .or_insert(lint.fixes.as_slice());
                }
                fixes
            });
        diagnostics.extend(report.diagnostics.items.iter().map(|d| {
            let (file, sl, sc, el, ec) = locate(sources, d.span);
            let name = store.find_by_code(&d.code).map(|(_, l)| l.name);
            let matching_fixes = fix_index.as_ref().map_or_else(
                || {
                    report
                        .lint_diagnostics
                        .iter()
                        .find(|lint| {
                            lint.diagnostic.span == d.span
                                && lint.diagnostic.code == d.code
                                && lint.diagnostic.message == d.message
                        })
                        .map(|lint| lint.fixes.as_slice())
                },
                |fixes| fixes.get(&diagnostic_key(d)).copied(),
            );
            let fixes = matching_fixes.map_or_else(Vec::new, |fixes| convert_fixes(fixes, sources));
            JsonDiag {
                code: &d.code,
                name,
                severity: match d.severity {
                    | Severity::Error => "error",
                    | Severity::Warning => "warning",
                    | Severity::Note => "note",
                },
                file,
                start_line: sl,
                start_column: sc,
                end_line: el,
                end_column: ec,
                message: &d.message,
                helps: &d.helps,
                notes: &d.notes,
                fixes,
            }
        }));
    }
    let doc = JsonReport {
        version: 1,
        has_errors,
        diagnostics,
    };
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string()) + "\n"
}

fn diagnostic_key(diagnostic: &Diagnostic) -> (Span, &str, &str) {
    (diagnostic.span, &diagnostic.code, &diagnostic.message)
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps nested fix and edit conversion out of diagnostic rendering"
)]
fn convert_fixes<'a>(
    fixes: &'a [Fix],
    sources: &SourceMap,
) -> Vec<JsonFix<'a>> {
    fixes
        .iter()
        .map(|f| {
            let applicability = match f.applicability {
                | Applicability::MachineApplicable => "machine-applicable",
                | Applicability::MaybeIncorrect => "maybe-incorrect",
                | Applicability::Manual => "manual",
            };
            let edits = f
                .edits
                .iter()
                .map(|e| {
                    let (file, sl, sc, el, ec) = locate(sources, e.span);
                    JsonEdit {
                        file,
                        start_line: sl,
                        start_column: sc,
                        end_line: el,
                        end_column: ec,
                        replacement: &e.replacement,
                    }
                })
                .collect();
            JsonFix {
                title: &f.title,
                applicability,
                edits,
            }
        })
        .collect()
}

/// Renders a SARIF 2.1.0 report for CI/GitHub code scanning.
#[must_use]
pub fn render_sarif(
    report: &LintReport,
    sources: &SourceMap,
    store: &LintStore,
) -> String {
    render_sarif_many([(report, sources)], store)
}

fn render_sarif_many<'a>(
    reports: impl IntoIterator<Item = (&'a LintReport, &'a SourceMap)>,
    store: &LintStore,
) -> String {
    let rules: Vec<serde_json::Value> = store
        .all()
        .iter()
        .map(|lint| {
            let default_level = match lint.default_level {
                | Level::Allow => "none",
                | Level::Warn => "warning",
                | Level::Deny | Level::Forbid => "error",
            };
            serde_json::json!({
                "id": lint.code,
                "name": lint.name,
                "shortDescription": { "text": lint.summary },
                "fullDescription": { "text": lint.explanation },
                "defaultConfiguration": { "level": default_level },
                "properties": {
                    "group": lint.group.as_str(),
                }
            })
        })
        .collect();

    let mut results = Vec::new();
    for (report, sources) in reports {
        results.extend(report.diagnostics.items.iter().map(|d| {
            let (file, sl, sc, el, ec) = locate(sources, d.span);
            let level = match d.severity {
                | Severity::Error => "error",
                | Severity::Warning => "warning",
                | Severity::Note => "note",
            };
            serde_json::json!({
                "ruleId": d.code,
                "level": level,
                "message": { "text": d.message },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": { "uri": file },
                        "region": {
                            "startLine": sl,
                            "startColumn": sc,
                            "endLine": el,
                            "endColumn": ec
                        }
                    }
                }]
            })
        }));
    }

    let sarif = serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "columnKind": "unicodeCodePoints",
            "tool": {
                "driver": {
                    "name": "aevia-lint",
                    "version": env!("CARGO_PKG_VERSION"),
                    "rules": rules
                }
            },
            "results": results
        }]
    });
    serde_json::to_string_pretty(&sarif).unwrap_or_else(|_| "{}".to_string()) + "\n"
}

fn locate(
    sources: &SourceMap,
    span: Span,
) -> (String, usize, usize, usize, usize) {
    let Some(file) = sources.file(span.file) else {
        return ("<unknown>".to_string(), 1, 1, 1, 1);
    };
    let (sl, sc) = file.line_col(span.start);
    let (el, ec) = file.line_col(span.end);
    (file.name.clone(), sl, sc, el, ec)
}
