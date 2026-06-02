//! Phase 6: doc comments, pow inference, simplify pipeline.

use std::path::PathBuf;

#[test]
fn doc_comments_parsed() {
    let src = r#"//! Crate docs

/// Computes force
pub fn force(m: kg, a: m/s^2) -> N := m * a;
"#;
    let file = aevia::parser::items::parse_source(src).unwrap();
    assert_eq!(file.doc.as_deref(), Some("Crate docs"));
    if let aevia::ast::Item::Function { doc, name, .. } = &file.items[0].node {
        assert_eq!(name, "force");
        assert!(doc.as_ref().unwrap().contains("Computes force"));
    } else {
        panic!("expected function");
    }
}

#[test]
fn pow_typechecks_kinetic_energy() {
    let result = aevia::types::checker::check(
        &aevia::parser::items::parse_source("fn ke(m: kg, v: m/s) -> J := 0.5 * m * v^2;").unwrap(),
    );
    assert!(result.ok(), "{:?}", result.errors);
}

#[test]
fn integration_physics_with_pow() {
    let physics = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/integration/src/modules/physics.ae");
    let program = aevia::modules::load_program(&physics).unwrap();
    let ast = aevia::modules::modules(&program)
        .find(|m| m.path.ends_with("physics.ae"))
        .map(|m| &m.ast)
        .expect("physics module");
    let result = aevia::types::checker::check(ast);
    assert!(result.ok(), "{:?}", result.errors);
}
