//! Error recovery must not poison the later phases (§4 of the audit).
//!
//! A parse failure produces an `Error` node, not a `UnitLit` expression or a
//! `Wildcard` pattern, so the type checker has nothing to invent a second
//! diagnostic about.

mod support;

use aevia_syntax::ast::{DeclKind, ExprKind, PatKind};
use support::{codes, expect_clean, parse};

#[test]
fn a_missing_operand_is_an_error_node() {
    let source = "let x = 1 +\n";
    let parsed = parse(source);
    assert_eq!(codes(&parsed.diagnostics), vec!["E0202".to_string()]);
    let DeclKind::Let { value, .. } = &parsed.module.decls[0].kind else {
        panic!("expected a let");
    };
    let ExprKind::Binary { right, .. } = &value.kind else {
        panic!("expected a binary expression, got {:?}", value.kind);
    };
    assert!(
        matches!(right.kind, ExprKind::Error),
        "the missing operand became {:?}",
        right.kind
    );
}

#[test]
fn a_missing_expression_is_an_error_node() {
    let source = "let x = )\n";
    let parsed = parse(source);
    assert_eq!(codes(&parsed.diagnostics), vec!["E0202".to_string()]);
    let DeclKind::Let { value, .. } = &parsed.module.decls[0].kind else {
        panic!("expected a let");
    };
    assert!(matches!(value.kind, ExprKind::Error));
}

#[test]
fn a_missing_pattern_is_an_error_node() {
    let source = "fn f(=) = 1\n";
    let parsed = parse(source);
    assert_eq!(codes(&parsed.diagnostics), vec!["E0202".to_string()]);
    let DeclKind::Fn { args, .. } = &parsed.module.decls[0].kind else {
        panic!("expected a function");
    };
    assert!(
        matches!(args[0].pattern.kind, PatKind::Error),
        "the missing pattern became {:?}",
        args[0].pattern.kind
    );
}

#[test]
fn a_missing_in_keyword_after_let_is_an_error_node() {
    let source = "let x = let y = 1\n";
    let parsed = parse(source);
    assert!(!parsed.diagnostics.items.is_empty());
    let DeclKind::Let { value, .. } = &parsed.module.decls[0].kind else {
        panic!("expected a let");
    };
    assert!(
        matches!(value.kind, ExprKind::Error),
        "a `let` without `in` became {:?}",
        value.kind
    );
}

#[test]
fn a_do_without_a_block_is_an_error_node() {
    let source = "let x = do\n";
    let parsed = parse(source);
    assert_eq!(codes(&parsed.diagnostics), vec!["E0103".to_string()]);
    let DeclKind::Let { value, .. } = &parsed.module.decls[0].kind else {
        panic!("expected a let");
    };
    assert!(matches!(value.kind, ExprKind::Error));
}

#[test]
fn a_unit_literal_is_still_a_unit_literal() {
    // The recovery node must not swallow the real `()`.
    let module = expect_clean("let x = ()\n");
    let DeclKind::Let { value, .. } = &module.decls[0].kind else {
        panic!("expected a let");
    };
    assert!(matches!(value.kind, ExprKind::UnitLit));
}

#[test]
fn a_wildcard_pattern_is_still_a_wildcard() {
    let module = expect_clean("let _ = 1\n");
    let DeclKind::Let { pattern, .. } = &module.decls[0].kind else {
        panic!("expected a let");
    };
    assert!(matches!(pattern.kind, PatKind::Wildcard));
}

#[test]
fn one_syntax_error_produces_one_diagnostic() {
    // The parser must not report the same bad token twice (once as "expected an
    // expression", once as "expected a declaration").
    for source in [
        "let x = 1 +\n",
        "let x = )\n",
        "let x = 1 2\n",
        "fn f(=) = 1\n",
    ] {
        let parsed = parse(source);
        assert_eq!(
            parsed.diagnostics.error_count(),
            1,
            "{source:?} produced {:?}",
            codes(&parsed.diagnostics)
        );
    }
}
