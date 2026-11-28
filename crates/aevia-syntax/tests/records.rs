//! Duplicate record fields (§2.3.8, `E0208`).

mod support;

use aevia_syntax::ast::DeclKind;
use aevia_syntax::ast::ExprKind;
use aevia_syntax::ast::TyKind;
use support::expect_clean;
use support::expect_error;
use support::span_text;

#[test]
fn a_duplicate_field_in_a_literal_is_e0208() {
    let source = "let x = { a = 1, a = 2 }\n";
    let diagnostic = expect_error(source, "E0208");
    assert_eq!(span_text(source, &diagnostic), "a");
    // The second `a`, not the first: the underline is on the repetition.
    assert_eq!(diagnostic.span.start, 17);
}

#[test]
fn a_duplicate_field_in_an_update_is_e0208() {
    let source = "let x = { p with a = 1, a = 2 }\n";
    let diagnostic = expect_error(source, "E0208");
    assert_eq!(span_text(source, &diagnostic), "a");
}

#[test]
fn a_duplicate_field_in_a_record_type_is_e0208() {
    let source = "let x: { a: Int, a: String } = {}\n";
    let diagnostic = expect_error(source, "E0208");
    assert_eq!(span_text(source, &diagnostic), "a");
}

#[test]
fn a_duplicate_punned_field_is_e0208() {
    let source = "let x = { a, a }\n";
    let diagnostic = expect_error(source, "E0208");
    assert_eq!(span_text(source, &diagnostic), "a");
}

#[test]
fn distinct_fields_are_still_accepted() {
    let module = expect_clean("let x = { a = 1, b = 2 }\n");
    let DeclKind::Let { value, .. } = &module.decls[0].kind else {
        panic!("expected a let");
    };
    let ExprKind::Record { fields, .. } = &value.kind else {
        panic!("expected a record");
    };
    assert_eq!(fields.len(), 2);

    let module = expect_clean("let x: { a: Int, b: Int } = { a = 1, b = 2 }\n");
    let DeclKind::Let { ty, .. } = &module.decls[0].kind else {
        panic!("expected a let");
    };
    let Some(ty) = ty else {
        panic!("expected an annotation")
    };
    let TyKind::Record { fields, .. } = &ty.kind else {
        panic!("expected a record type");
    };
    assert_eq!(fields.len(), 2);
}

#[test]
fn records_compare_normalized_names_and_keep_source_order() {
    let source = "let x = { z = 0, é = 1, a = 2, e\u{301} = 3 }\n";
    let parsed = support::parse(source);
    assert_eq!(support::codes(&parsed.diagnostics), ["E0208"]);
    assert_eq!(span_text(source, &parsed.diagnostics.items[0]), "e\u{301}");
    let DeclKind::Let { value, .. } = &parsed.module.decls[0].kind else {
        panic!("expected a let");
    };
    let ExprKind::Record { fields, .. } = &value.kind else {
        panic!("expected a record");
    };
    let names: Vec<_> = fields.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["z", "é", "a", "é"]);

    let source = "let x: { z: Int, é: Int, a: Int, e\u{301}: Int } = {}\n";
    let diagnostic = expect_error(source, "E0208");
    assert_eq!(span_text(source, &diagnostic), "e\u{301}");
}
