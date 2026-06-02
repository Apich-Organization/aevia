//! Smoke test for `examples/integration/`.

use std::path::PathBuf;

#[test]
fn integration_example_type_checks() {
    let main = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/integration/src/main.ae");
    let program = aevia::modules::load_program(&main).expect("load program");
    let ast = aevia::modules::entry_ast(&program);
    let imports = aevia::modules::entry_imports(&program);
    let result = aevia::types::checker::check_with_imports(ast, imports);
    assert!(
        result.ok(),
        "integration example failed: {:?}",
        result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}
