//! Duplicate record fields (§2.3.8, `E0208`).

mod support;

use aevia_syntax::ast::{DeclKind, ExprKind, TyKind};
use support::{expect_clean, expect_error, span_text};

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
    let Some(ty) = ty else { panic!("expected an annotation") };
    let TyKind::Record { fields, .. } = &ty.kind else {
        panic!("expected a record type");
    };
    assert_eq!(fields.len(), 2);
}
