//! Contracts shared by the lint CLI, editor quick fixes and CI report consumers.

use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use aevia_core::manifest::LintLevel;
use aevia_core::manifest::LintSetting;
use aevia_lint::Applicability;
use aevia_lint::CliOverride;
use aevia_lint::Driver;
use aevia_lint::Fix;
use aevia_lint::FixEngine;
use aevia_lint::Level;
use aevia_lint::LevelResolver;
use aevia_lint::LintConfig;
use aevia_lint::LintDiagnostic;
use aevia_lint::LintReport;
use aevia_lint::LintStore;
use aevia_lint::TextEdit;
use aevia_lint::lint::W0101_UNUSED_VARIABLE;
use aevia_lint::render_json;
use aevia_lint::render_sarif;
use aevia_syntax::Diagnostic;
use aevia_syntax::Diagnostics;
use aevia_syntax::FileId;
use aevia_syntax::Severity;
use aevia_syntax::SourceMap;
use aevia_syntax::Span;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "aevia-lint-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).expect("create an isolated test directory");
        Self(path)
    }

    fn source(
        &self,
        map: &mut SourceMap,
        name: &str,
        text: &str,
    ) -> (PathBuf, FileId) {
        let path = self.0.join(name);
        std::fs::write(&path, text).expect("write the test source");
        let file = map.add(path.to_string_lossy(), text);
        (path, file)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn edit(
    file: FileId,
    from: usize,
    to: usize,
    replacement: &str,
) -> TextEdit {
    TextEdit {
        span: Span::new(file, from, to),
        replacement: replacement.into(),
    }
}

fn fix(
    applicability: Applicability,
    edits: Vec<TextEdit>,
) -> Fix {
    Fix {
        title: "Suggested change".into(),
        applicability,
        edits,
    }
}

fn finding(
    span: Span,
    message: &str,
    fixes: Vec<Fix>,
) -> LintDiagnostic {
    LintDiagnostic {
        diagnostic: Diagnostic::warning("W0101", message, span),
        lint: &W0101_UNUSED_VARIABLE,
        level: Level::Warn,
        fixes,
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read result")
}

#[test]
fn competing_edits_and_ineligible_fixes_leave_unaccepted_bytes_alone() {
    let scratch = Scratch::new();
    let mut sources = SourceMap::new();
    let (path, file) = scratch.source(&mut sources, "source.aev", "αBCD\r\n");
    let warnings = [finding(
        Span::new(file, 2, 3),
        "competing edits",
        vec![
            fix(Applicability::MaybeIncorrect, vec![edit(file, 4, 5, "?")]),
            fix(Applicability::Manual, vec![edit(file, 4, 5, "!")]),
            fix(Applicability::MachineApplicable, Vec::new()),
            fix(
                Applicability::MachineApplicable,
                vec![edit(file, 2, 3, "β")],
            ),
            fix(
                Applicability::MachineApplicable,
                vec![edit(file, 2, 4, "conflict"), edit(file, 4, 5, "partial")],
            ),
            fix(
                Applicability::MachineApplicable,
                vec![edit(file, 3, 4, "γ")],
            ),
        ],
    )];
    let result = FixEngine::apply(&warnings, &sources).expect("apply eligible fixes");
    assert_eq!(result.applied, 2);
    assert_eq!(result.files_modified, [path.to_string_lossy()]);
    assert_eq!(read(&path), "αβγD\r\n");
}

#[test]
fn a_conflict_inside_one_fix_leaves_every_file_unchanged() {
    let scratch = Scratch::new();
    let mut sources = SourceMap::new();
    let (first, a) = scratch.source(&mut sources, "first.aev", "αβγ");
    let (second, b) = scratch.source(&mut sources, "second.aev", "value");
    let warnings = [finding(
        Span::new(a, 0, 2),
        "overlap",
        vec![fix(
            Applicability::MachineApplicable,
            vec![edit(a, 0, 4, "x"), edit(a, 2, 6, "y"), edit(b, 0, 5, "z")],
        )],
    )];
    let result = FixEngine::apply(&warnings, &sources).expect("inspect a malformed fix");
    assert_eq!(result.applied, 0);
    assert!(result.files_modified.is_empty());
    assert_eq!(read(&first), "αβγ");
    assert_eq!(read(&second), "value");
}

#[test]
fn stale_sources_and_invalid_utf8_boundaries_skip_entire_fixes() {
    let scratch = Scratch::new();
    let mut sources = SourceMap::new();
    let (current, a) = scratch.source(&mut sources, "a.aev", "αnew");
    let (stale, b) = scratch.source(&mut sources, "b.aev", "other");
    std::fs::write(&stale, "newer").expect("simulate an editor save");
    let warnings = [finding(
        Span::new(a, 0, 2),
        "stale source",
        vec![
            fix(Applicability::MachineApplicable, vec![edit(a, 0, 1, "x")]),
            fix(
                Applicability::MachineApplicable,
                vec![edit(a, 2, 5, "old"), edit(b, 0, 5, "older")],
            ),
            fix(Applicability::MachineApplicable, vec![edit(a, 0, 2, "λ")]),
        ],
    )];
    let result = FixEngine::apply(&warnings, &sources).expect("apply the independent UTF-8 edit");
    assert_eq!(result.applied, 1);
    assert_eq!(result.files_modified, [current.to_string_lossy()]);
    assert_eq!(read(&current), "λnew");
    assert_eq!(read(&stale), "newer");
}

#[test]
fn adjacent_fixes_keep_their_insertions_but_competing_insertions_apply_once() {
    let scratch = Scratch::new();
    let mut sources = SourceMap::new();
    let (path, file) = scratch.source(&mut sources, "source.aev", "AB");
    let warnings = [finding(
        Span::new(file, 0, 2),
        "insertions",
        vec![
            fix(
                Applicability::MachineApplicable,
                vec![edit(file, 1, 2, "b"), edit(file, 1, 1, ":")],
            ),
            fix(
                Applicability::MachineApplicable,
                vec![edit(file, 1, 1, "?")],
            ),
            fix(
                Applicability::MachineApplicable,
                vec![edit(file, 0, 1, "a")],
            ),
            fix(
                Applicability::MachineApplicable,
                vec![edit(file, 2, 2, "!")],
            ),
        ],
    )];
    let result = FixEngine::apply(&warnings, &sources).expect("apply adjacent edits");
    assert_eq!(result.applied, 3);
    assert_eq!(read(&path), "a:b!");
}

#[test]
fn cli_config_and_attributes_respect_forbid_and_cap_dependency_lints() {
    let source = "@allow(unused_variable)\nfn f(unused: Int): Int = 1\n";
    let mut sources = SourceMap::new();
    let file = sources.add("source.aev", source);
    let lexed = aevia_syntax::lex(file, source).expect("lex");
    let (module, errors) = aevia_syntax::parse_module(file, source, &lexed);
    assert!(!errors.has_errors(), "{errors:?}");
    let lo = source.find("unused:").expect("parameter offset");
    let unused = Span::new(file, lo, lo + "unused".len());
    let store = LintStore::with_builtins();
    let (lint, _) = store.find("unused-variable").expect("registered lint");
    let mut config = LintConfig {
        manifest_lints: [(
            "unused-variable".into(),
            LintSetting {
                level: LintLevel::Warn,
                reason: None,
            },
        )]
        .into(),
        cli_overrides: vec![CliOverride {
            level: Level::Deny,
            target: "W0101".into(),
        }],
        run_extra_lints: false,
    };
    let scoped = LevelResolver::resolve(&store, &config, Some(&module), true);
    assert_eq!(scoped.level_at(lint, unused), Level::Allow);
    assert_eq!(
        LevelResolver::resolve(&store, &config, None, true).level_at(lint, unused),
        Level::Deny
    );
    config.cli_overrides.extend([
        CliOverride {
            level: Level::Forbid,
            target: "unused_variable".into(),
        },
        CliOverride {
            level: Level::Warn,
            target: "unused-variable".into(),
        },
    ]);
    let resolved = LevelResolver::resolve(&store, &config, Some(&module), true);
    assert_eq!(resolved.level_at(lint, unused), Level::Forbid);
    assert_eq!(
        resolved
            .errors
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
        ["E0211", "E0211"],
    );
    assert_ne!(scoped.fingerprint(), resolved.fingerprint());
    let dependency = LevelResolver::resolve(&store, &config, Some(&module), false);
    assert_eq!(dependency.level_at(lint, unused), Level::Allow);
    assert!(!dependency.is_enabled_anywhere(lint));
    assert!(dependency.errors.is_empty());
}

#[test]
fn json_attaches_only_matching_fixes_and_sarif_declares_its_column_unit() {
    let mut sources = SourceMap::new();
    let source = "注释\r\n🙂x";
    let file = sources.add("src/example.aev", source);
    let from = source.find('x').expect("reported byte");
    let span = Span::new(file, from, source.len());
    let mut first = finding(
        span,
        "replace the unused name",
        vec![fix(
            Applicability::MachineApplicable,
            vec![edit(file, from, source.len(), "_x")],
        )],
    );
    first.level = Level::Deny;
    first.diagnostic.severity = Severity::Error;
    let second = finding(span, "a different warning at that location", Vec::new());
    let report = LintReport {
        diagnostics: Diagnostics {
            items: vec![first.diagnostic.clone(), second.diagnostic.clone()],
        },
        lint_diagnostics: vec![second, first],
        modules_checked: 1,
    };
    let store = LintStore::with_builtins();
    let json: serde_json::Value =
        serde_json::from_str(&render_json(&report, &sources, &store)).expect("JSON lint report");
    let rendered = &json["diagnostics"];
    assert_eq!(json["version"], 1);
    assert_eq!(json["has_errors"], true);
    assert_eq!(rendered[0]["severity"], "error");
    assert_eq!(rendered[0]["start_line"], 2);
    assert_eq!(
        rendered[0]["fixes"][0]["applicability"],
        "machine-applicable"
    );
    assert_eq!(rendered[0]["fixes"][0]["edits"][0]["replacement"], "_x");
    assert_eq!(rendered[1]["fixes"], serde_json::json!([]));

    let sarif: serde_json::Value =
        serde_json::from_str(&render_sarif(&report, &sources, &store)).expect("SARIF lint report");
    let run = &sarif["runs"][0];
    assert_eq!(sarif["version"], "2.1.0");
    assert_eq!(run["columnKind"], "unicodeCodePoints");
    assert_eq!(run["results"][0]["ruleId"], "W0101");
    let physical = &run["results"][0]["locations"][0]["physicalLocation"];
    assert_eq!(physical["artifactLocation"]["uri"], "src/example.aev");
    assert_eq!(
        physical["region"],
        serde_json::json!({
            "startLine": 2, "startColumn": 2, "endLine": 2, "endColumn": 3,
        })
    );
}

#[test]
fn simplifying_identical_branches_preserves_a_condition_that_can_fail() {
    const ORIGINAL: &str = "fn choose(n: Int): Int = if rem(n, 0) == 1 then 2 else 2
fn keep(b: Bool): Bool = if b then true else false
fn negate(n: Int): Bool = if n < 4 then false else true
fn constant(): Int = if true then 42 else 42
";
    const EXPECTED: &str = "fn choose(n: Int): Int = if rem(n, 0) == 1 then 2 else 2
fn keep(b: Bool): Bool = b
fn negate(n: Int): Bool = !(n < 4)
fn constant(): Int = 42
";
    let scratch = Scratch::new();
    let mut sources = SourceMap::new();
    let (path, file) = scratch.source(&mut sources, "branches.aev", ORIGINAL);
    let lexed = aevia_syntax::lex(file, ORIGINAL).expect("lex");
    let (module, errors) = aevia_syntax::parse_module(file, ORIGINAL, &lexed);
    assert!(!errors.has_errors(), "{errors:?}");
    let report = Driver::run_module(
        &sources,
        &module,
        None,
        &errors,
        file,
        &LintStore::with_builtins(),
        &LintConfig {
            run_extra_lints: true,
            ..Default::default()
        },
        true,
    );
    let branches = report
        .lint_diagnostics
        .iter()
        .filter(|finding| finding.lint.code == "W0709")
        .collect::<Vec<_>>();
    assert_eq!(branches.len(), 4);
    assert_eq!(
        branches[0].fixes[0].applicability,
        Applicability::MaybeIncorrect
    );
    let fixed = FixEngine::apply(&report.lint_diagnostics, &sources).expect("apply safe fixes");
    assert_eq!(fixed.applied, 3);
    assert_eq!(read(&path), EXPECTED);
}

#[test]
fn type_aware_lints_distinguish_strict_float_equality_from_exact_comparisons() {
    let source = "fn compare_float(a: Float, b: Float): Bool = a == b
fn compare_int(a: Int, b: Int): Bool = a == b
fn compare_float_order(a: Float, b: Float): Bool = a < b
";
    let mut sources = SourceMap::new();
    let file = sources.add("compare.aev", source);
    let lexed = aevia_syntax::lex(file, source).expect("lex");
    let (module, errors) = aevia_syntax::parse_module(file, source, &lexed);
    assert!(!errors.has_errors(), "{errors:?}");
    let checked = aevia_types::check::check_module(file, &module);
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
    let store = LintStore::with_builtins();
    let config = LintConfig {
        run_extra_lints: true,
        ..Default::default()
    };
    for (module_types, is_root, expected) in [
        (Some(&checked.module), true, 1),
        (None, true, 0),
        (Some(&checked.module), false, 0),
    ] {
        let report = Driver::run_module(
            &sources,
            &module,
            module_types,
            &checked.diagnostics,
            file,
            &store,
            &config,
            is_root,
        );
        let float_equality = report
            .lint_diagnostics
            .iter()
            .filter(|warning| warning.lint.code == "W0706")
            .collect::<Vec<_>>();
        assert_eq!(float_equality.len(), expected);
        if let [warning] = float_equality.as_slice() {
            assert_eq!(warning.level, Level::Warn);
            assert_eq!(
                sources
                    .file(file)
                    .unwrap()
                    .line_col(warning.diagnostic.span.start)
                    .0,
                1
            );
        }
    }
}
