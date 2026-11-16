//! Integration tests for `aevia-lint`: level resolution, attributes, passes, fixes, and formats.

use std::collections::BTreeMap;
use std::path::Path;

use aevia_core::driver;
use aevia_core::manifest::LintLevel;
use aevia_core::manifest::LintSetting;
use aevia_lint::CliOverride;
use aevia_lint::Driver;
use aevia_lint::FixEngine;
use aevia_lint::Level;
use aevia_lint::LintConfig;
use aevia_lint::LintStore;
use aevia_lint::OutputFormat;
use aevia_lint::render;
use aevia_syntax::Style;

fn lint_source(
    source: &str,
    config: LintConfig,
) -> (aevia_core::driver::Program, aevia_lint::LintReport) {
    let store = LintStore::with_builtins();
    let program = driver::load_with_source(Path::new("lint_test.aev"), source);
    let root_files: Option<Vec<aevia_syntax::FileId>> = program.entry().map(|m| vec![m.file]);
    let report = Driver::run_program(&program, &store, &config, root_files.as_deref());
    (program, report)
}

#[test]
fn level_precedence_and_forbid_cannot_be_lowered() {
    let source = r#"
@allow(unused-variable)
fn ok_fn(x: Int): Int = 1

@forbid(unused-variable)
@allow(unused-variable)
fn bad_override(y: Int): Int = 2
"#;
    let config = LintConfig::default();
    let (_, report) = lint_source(source, config);
    assert!(
        report.diagnostics.items.iter().any(|d| d.code == "E0211"),
        "expected E0211 for lowering @forbid, got: {:?}",
        report.diagnostics.items
    );
    // `x` in `ok_fn` is allowed, `y` in `bad_override` is forbidden (error).
    let w0101: Vec<_> = report
        .diagnostics
        .items
        .iter()
        .filter(|d| d.code == "W0101")
        .collect();
    assert_eq!(w0101.len(), 1, "expected only y to emit W0101: {w0101:?}");
    assert_eq!(w0101[0].severity, aevia_syntax::Severity::Error);
}

#[test]
fn unknown_lint_in_attribute_or_manifest_reports_e0212() {
    let source = r#"
@allow(no-such-lint)
fn f(): Int = 1
"#;
    let (_, report) = lint_source(source, LintConfig::default());
    assert!(
        report.diagnostics.items.iter().any(|d| d.code == "E0212"),
        "expected E0212: {:?}",
        report.diagnostics.items
    );

    let mut manifest_lints = BTreeMap::new();
    manifest_lints.insert(
        "totally-fake-lint".to_string(),
        LintSetting {
            level: LintLevel::Warn,
            reason: None,
        },
    );
    let (_, report2) = lint_source(
        "fn f(): Int = 1\n",
        LintConfig {
            manifest_lints,
            ..LintConfig::default()
        },
    );
    assert!(report2.diagnostics.items.iter().any(|d| d.code == "E0212"));
}

#[test]
fn naming_shadowing_float_eq_dimension_literal_and_missing_doc_fire() {
    let source = r#"
type Bad_Type = | Bad_Ctor(Int)

fn badName(sqrt: Float[1]): Bool =
    let z: Real[L] = 0
    let _ = z
    sqrt == 1.0
"#;
    let mut manifest_lints = BTreeMap::new();
    manifest_lints.insert(
        "missing-doc-public".to_string(),
        LintSetting {
            level: LintLevel::Warn,
            reason: None,
        },
    );
    let config = LintConfig {
        manifest_lints,
        cli_overrides: Vec::new(),
        run_extra_lints: true,
    };
    let (program, report) = lint_source(source, config);
    let codes: Vec<&str> = report
        .diagnostics
        .items
        .iter()
        .map(|d| d.code.as_str())
        .collect();

    assert!(codes.contains(&"W0701"), "missing W0701 in {codes:?}");
    assert!(codes.contains(&"W0702"), "missing W0702 in {codes:?}");
    assert!(codes.contains(&"W0703"), "missing W0703 in {codes:?}");
    assert!(codes.contains(&"W0705"), "missing W0705 in {codes:?}");
    assert!(codes.contains(&"W0706"), "missing W0706 in {codes:?}");
    assert!(codes.contains(&"W0707"), "missing W0707 in {codes:?}");

    let store = LintStore::with_builtins();
    let json = render(
        &report,
        &program.sources,
        &store,
        OutputFormat::Json,
        Style::Plain,
    );
    let parsed_json: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    assert_eq!(parsed_json["version"], 1);
    assert!(!parsed_json["diagnostics"].as_array().unwrap().is_empty());

    let sarif = render(
        &report,
        &program.sources,
        &store,
        OutputFormat::Sarif,
        Style::Plain,
    );
    let parsed_sarif: serde_json::Value = serde_json::from_str(&sarif).expect("valid sarif");
    assert_eq!(parsed_sarif["version"], "2.1.0");
    assert!(
        !parsed_sarif["runs"][0]["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn cli_deny_warnings_promotes_warnings_to_errors() {
    let source = "fn f(unused: Int): Int = 1\n";
    let config = LintConfig {
        manifest_lints: BTreeMap::new(),
        cli_overrides: vec![CliOverride {
            level: Level::Deny,
            target: "warnings".to_string(),
        }],
        run_extra_lints: false,
    };
    let (_, report) = lint_source(source, config);
    assert!(report.has_errors());
}

#[test]
fn unused_import_has_machine_applicable_fix() {
    let dir = std::env::temp_dir().join(format!("aevia-lint-fix-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let dep = dir.join("Helper.aev");
    std::fs::write(&dep, "module Helper\nfn helper_val(): Int = 42\n").expect("write dep");
    let main = dir.join("Main.aev");
    std::fs::write(&main, "import Helper\nfn main(): Int = 1\n").expect("write main");

    let store = LintStore::with_builtins();
    let program = driver::load(&main);
    let root_files: Option<Vec<aevia_syntax::FileId>> = program.entry().map(|m| vec![m.file]);
    let config = LintConfig {
        run_extra_lints: true,
        ..LintConfig::default()
    };
    let report = Driver::run_program(&program, &store, &config, root_files.as_deref());
    assert!(
        report.diagnostics.items.iter().any(|d| d.code == "W0102"),
        "expected W0102: {:?}",
        report.diagnostics.items
    );

    let summary = FixEngine::apply(&report.lint_diagnostics, &program.sources).expect("apply fix");
    assert_eq!(summary.applied, 1);
    let after = std::fs::read_to_string(&main).expect("read fixed");
    assert_eq!(after, "fn main(): Int = 1\n");

    let reloaded = driver::load(&main);
    let root2: Option<Vec<aevia_syntax::FileId>> = reloaded.entry().map(|m| vec![m.file]);
    let report2 = Driver::run_program(&reloaded, &store, &config, root2.as_deref());
    assert!(!report2.diagnostics.items.iter().any(|d| d.code == "W0102"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn allow_by_diagnostic_code_works() {
    let source = r#"
@allow(W0101)
fn f(x: Int): Int = 1
"#;
    let (_, report) = lint_source(source, LintConfig::default());
    assert!(
        !report.diagnostics.items.iter().any(|d| d.code == "W0101"),
        "expected W0101 to be suppressed by @allow(W0101): {:?}",
        report.diagnostics.items
    );
}

#[test]
fn self_comparison_redundant_if_and_duplicate_import_fire() {
    let source = r#"
import Math
import Math

fn check_vals(x: Int, cond: Bool): Bool =
    let a = x == x
    let b = if cond then true else false
    let _ = a
    b
"#;
    let config = LintConfig {
        run_extra_lints: true,
        ..LintConfig::default()
    };
    let (program, report) = lint_source(source, config);
    let codes: Vec<&str> = report
        .diagnostics
        .items
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert!(codes.contains(&"W0708"), "missing W0708 in {codes:?}");
    assert!(codes.contains(&"W0709"), "missing W0709 in {codes:?}");
    assert!(codes.contains(&"W0710"), "missing W0710 in {codes:?}");

    let store = LintStore::with_builtins();
    let json = render(
        &report,
        &program.sources,
        &store,
        OutputFormat::Json,
        Style::Plain,
    );
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    let diags = parsed["diagnostics"].as_array().unwrap();
    let w0709 = diags.iter().find(|d| d["code"] == "W0709").unwrap();
    assert!(
        !w0709["fixes"].as_array().unwrap().is_empty(),
        "expected JSON fixes on W0709: {w0709}"
    );
}

#[test]
fn redundant_if_and_redundant_annotation_fixes_apply_cleanly() {
    let dir = std::env::temp_dir().join(format!("aevia-lint-fix2-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let main = dir.join("Main.aev");
    std::fs::write(
        &main,
        "fn f(c: Bool): Bool =\n    let x = (true : Bool)\n    let _ = x\n    if c then false else true\n",
    )
    .expect("write main");

    let mut manifest_lints = BTreeMap::new();
    manifest_lints.insert(
        "redundant-annotation".to_string(),
        LintSetting {
            level: LintLevel::Warn,
            reason: None,
        },
    );
    let store = LintStore::with_builtins();
    let program = driver::load(&main);
    let root_files: Option<Vec<aevia_syntax::FileId>> = program.entry().map(|m| vec![m.file]);
    let config = LintConfig {
        manifest_lints,
        cli_overrides: Vec::new(),
        run_extra_lints: true,
    };
    let report = Driver::run_program(&program, &store, &config, root_files.as_deref());
    let summary = FixEngine::apply(&report.lint_diagnostics, &program.sources).expect("apply fix");
    assert_eq!(summary.applied, 2);
    let after = std::fs::read_to_string(&main).expect("read fixed");
    assert_eq!(
        after,
        "fn f(c: Bool): Bool =\n    let x = (true)\n    let _ = x\n    !c\n"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn partial_selective_unused_import_reports_only_unused_name() {
    let dir = std::env::temp_dir().join(format!("aevia-lint-sel-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let dep = dir.join("Helper.aev");
    std::fs::write(
        &dep,
        "module Helper\nfn used_fn(): Int = 1\nfn unused_fn(): Int = 2\n",
    )
    .expect("write dep");
    let main = dir.join("Main.aev");
    std::fs::write(
        &main,
        "import Helper { used_fn, unused_fn }\nfn main(): Int = used_fn()\n",
    )
    .expect("write main");

    let store = LintStore::with_builtins();
    let program = driver::load(&main);
    let root_files: Option<Vec<aevia_syntax::FileId>> = program.entry().map(|m| vec![m.file]);
    let config = LintConfig {
        run_extra_lints: true,
        ..LintConfig::default()
    };
    let report = Driver::run_program(&program, &store, &config, root_files.as_deref());
    let w0102: Vec<_> = report
        .diagnostics
        .items
        .iter()
        .filter(|d| d.code == "W0102")
        .collect();
    assert_eq!(w0102.len(), 1, "expected 1 W0102: {w0102:?}");
    assert!(w0102[0].message.contains("unused_fn"));

    let _ = std::fs::remove_dir_all(&dir);
}
