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
    // §B.4 E0204: a token that cannot start a pattern is an invalid pattern,
    // not a generic syntax error; the recovery node is what keeps the checker
    // from reporting it again.
    assert_eq!(codes(&parsed.diagnostics), vec!["E0204".to_string()]);
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

#[test]
fn a_number_followed_by_a_non_unit_is_e0108() {
    // `3 (2)`: the parenthesised part cannot be a unit at all. A malformed
    // unit expression (well-formed brackets, no unit inside) stays E0209.
    let parsed = parse("let x = 3 (2)\n");
    assert!(
        codes(&parsed.diagnostics).contains(&"E0108".to_string()),
        "got {:?}",
        codes(&parsed.diagnostics)
    );
    let parsed = parse("let x = 3 ()\n");
    assert!(
        codes(&parsed.diagnostics).contains(&"E0209".to_string()),
        "got {:?}",
        codes(&parsed.diagnostics)
    );
}

#[test]
fn a_partial_operator_section_is_e0205() {
    // §1.8.5: only the bare operator (`(+)`) or a lambda is a function.
    let parsed = parse("let f = (== 1)\n");
    assert_eq!(codes(&parsed.diagnostics), vec!["E0205".to_string()]);
    expect_clean("let f = (+)\nlet g = (-)\n");
}

#[test]
fn a_type_application_on_a_non_name_is_e0207() {
    // `3<Int>`: adjacent `<` reads as type application, but only a function
    // name can take type arguments (§2.7.2).
    let parsed = parse("let x = 3<Int>\n");
    assert!(
        codes(&parsed.diagnostics).contains(&"E0207".to_string()),
        "got {:?}",
        codes(&parsed.diagnostics)
    );
    // A named callee is the supported spelling.
    expect_clean("fn f(a: Int): Int = a\nlet g = f<Int>(1)\n");
}
