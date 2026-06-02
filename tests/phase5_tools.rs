//! Phase 5 toolchain smoke tests.

use std::path::PathBuf;

#[test]
fn integration_project_tests_pass() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/integration");
    let results = aevia::test_runner::run_project(&root).expect("run tests");
    assert!(
        results.iter().all(|r| r.passed),
        "failures: {:?}",
        results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| (&r.path, &r.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn lint_integration_entry() {
    let main = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/integration/src/main.ae");
    let report = aevia::lint::lint_file(&main).expect("lint");
    assert!(report.ok(), "{:?}", report.lints);
}

#[test]
fn doc_renders_markdown() {
    let main = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/integration/src/modules/physics.ae");
    let program = aevia::modules::load_program(&main).unwrap();
    let module = aevia::modules::modules(&program).next().unwrap();
    let md = aevia::doc::render_markdown(&module.path, &module.ast);
    assert!(md.contains("## fn `energy`"));
}
