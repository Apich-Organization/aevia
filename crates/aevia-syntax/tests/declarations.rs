//! Declaration termination (§A.2: `decl ... NEWLINE`).
//!
//! Every declaration ends at the end of its logical line. Before the fix the
//! parser called `eat_newline()`, so a second declaration on the same line was
//! accepted and the trailing junk was blamed on the *next* declaration.

mod support;

use aevia_syntax::ast::DeclKind;
use aevia_syntax::ast::ExprKind;
use aevia_syntax::ast::Param;
use aevia_syntax::ast::PatKind;
use aevia_syntax::ast::TyKind;
use support::expect_clean;
use support::expect_error;
use support::parse;
use support::span_text;

#[test]
fn two_let_declarations_on_one_line_are_rejected() {
    let source = "let x = 1 let y = 2\n";
    let diagnostic = expect_error(source, "E0202");
    // The error belongs to the second `let`, not to the next declaration.
    assert_eq!(span_text(source, &diagnostic), "let");
    assert_eq!(diagnostic.span.start, 10);
}

#[test]
fn two_function_declarations_on_one_line_are_rejected() {
    let source = "fn f() = 1 fn g() = 2\n";
    let diagnostic = expect_error(source, "E0202");
    assert_eq!(span_text(source, &diagnostic), "fn");
    assert_eq!(diagnostic.span.start, 11);
}

#[test]
fn trailing_junk_after_a_declaration_is_blamed_on_the_junk() {
    let source = "let x = 1 2\n";
    let diagnostic = expect_error(source, "E0202");
    assert_eq!(span_text(source, &diagnostic), "2");
    assert_eq!(diagnostic.span.start, 10);
}

#[test]
fn two_imports_on_one_line_are_rejected() {
    let source = "import A import B\n";
    let diagnostic = expect_error(source, "E0202");
    assert_eq!(span_text(source, &diagnostic), "import");
}

#[test]
fn two_constructors_on_one_line_are_rejected() {
    let source = "type T =\n    | A\n    | B | C\n";
    let diagnostic = expect_error(source, "E0202");
    assert_eq!(span_text(source, &diagnostic), "|");
}

#[test]
fn a_declaration_at_end_of_file_needs_no_newline() {
    let module = expect_clean("let x = 1");
    assert_eq!(module.decls.len(), 1);
    let module = expect_clean("fn f() =\n    1");
    let DeclKind::Fn { body, .. } = &module.decls[0].kind else {
        panic!("expected a function");
    };
    assert!(matches!(body.kind, ExprKind::Int(_)));
}

#[test]
fn a_block_body_does_not_need_a_trailing_newline() {
    let source = "fn f() =\n    let y = 1\n    y";
    let module = expect_clean(source);
    let DeclKind::Fn { body, .. } = &module.decls[0].kind else {
        panic!("expected a function");
    };
    let ExprKind::Block { stmts, tail } = &body.kind else {
        panic!("expected a block, got {:?}", body.kind);
    };
    assert_eq!(stmts.len(), 1);
    assert!(tail.is_some());
}

#[test]
fn a_single_element_tuple_is_rejected() {
    // §2.3.14: single-element tuples are not supported.
    let source = "let x = (1,)\n";
    let diagnostic = expect_error(source, "E0202");
    assert_eq!(span_text(source, &diagnostic), "(1,)");
}

#[test]
fn a_two_element_tuple_is_still_accepted() {
    let module = expect_clean("let x = (1, 2)\n");
    let DeclKind::Let { value, .. } = &module.decls[0].kind else {
        panic!("expected a let");
    };
    assert!(matches!(&value.kind, ExprKind::Tuple(items) if items.len() == 2));
}

#[test]
fn private_on_an_import_is_rejected() {
    // §2.1: `private` marks a declaration as not exported, and `import` is not
    // one of the declarations it may precede.
    let source = "private import A\n";
    let diagnostic = expect_error(source, "E0202");
    assert_eq!(span_text(source, &diagnostic), "private");
}

#[test]
fn private_on_a_let_type_or_function_is_accepted() {
    expect_clean("private let x = 1\n");
    expect_clean("private type T = Int\n");
    expect_clean("private fn f() = 1\n");
}

#[test]
fn match_arms_do_not_start_on_the_with_line() {
    // §A.3: `match_body` always starts with `NEWLINE`.
    let source = "let x = match 1 with | 0 -> 1\n";
    let diagnostic = expect_error(source, "E0202");
    assert_eq!(span_text(source, &diagnostic), "|");
}

#[test]
fn match_arms_on_their_own_lines_are_accepted() {
    let module = expect_clean("let x = match 1 with\n| 0 -> 1\n| _ -> 2\n");
    let DeclKind::Let { value, .. } = &module.decls[0].kind else {
        panic!("expected a let");
    };
    let ExprKind::Match { arms, .. } = &value.kind else {
        panic!("expected a match, got {:?}", value.kind);
    };
    assert_eq!(arms.len(), 2);
}

#[test]
fn a_function_name_must_be_lowercase() {
    // §A.2: `fn_decl ::= "fn" IDENT ...`.
    let source = "fn Foo(x) = x\n";
    let diagnostic = expect_error(source, "E0202");
    assert_eq!(span_text(source, &diagnostic), "Foo");
}

#[test]
fn a_capitalised_lambda_parameter_binds_a_variable() {
    // `fn Foo => ...` is not a declaration; the name form of `fn_expr` is
    // `IDENT`. A lambda parameter is a binding position, where a bare
    // capitalised name binds a variable (§2.4, revision 11).
    let module = parse("let f = fn (Foo) => Foo\n").module;
    let DeclKind::Let { value, .. } = &module.decls[0].kind else {
        panic!("expected a let");
    };
    let ExprKind::Lambda { params, .. } = &value.kind else {
        panic!("expected a lambda, got {:?}", value.kind);
    };
    let Param { pattern, .. } = &params[0];
    assert!(matches!(&pattern.kind, PatKind::Var(name) if name == "Foo"));
}

#[test]
fn a_type_declaration_needs_an_equals_before_a_variant_block() {
    // §A.2: `type_body` (including the indented variant list) is only
    // reachable after `=`.
    let source = "type T\n    | A\n";
    let diagnostic = expect_error(source, "E0202");
    assert_eq!(span_text(source, &diagnostic), "\n");
}

#[test]
fn an_abstract_type_needs_no_body() {
    let module = expect_clean("type Table<r>\n");
    let DeclKind::Type { body, .. } = &module.decls[0].kind else {
        panic!("expected a type");
    };
    assert!(matches!(body, aevia_syntax::ast::TypeBody::Abstract));
}

#[test]
fn a_variant_type_with_equals_and_a_block_is_accepted() {
    let module =
        expect_clean("type Shape =\n    | Circle(Real[L])\n    | Rect(Real[L], Real[L])\n");
    let DeclKind::Type { body, .. } = &module.decls[0].kind else {
        panic!("expected a type");
    };
    let aevia_syntax::ast::TypeBody::Variants(variants) = body else {
        panic!("expected variants");
    };
    assert_eq!(variants.len(), 2);
}

#[test]
fn derive_accepts_a_lowercase_class_name() {
    // §A.2: `derive name_list for type`, and `name_list` is `IDENT | UIDENT`.
    expect_clean("derive show for Vec2\n");
}

#[test]
fn a_typed_let_still_parses() {
    let module = expect_clean("let x: Tensor<1, Float> = [1.0, 2.0]\n");
    let DeclKind::Let { ty, .. } = &module.decls[0].kind else {
        panic!("expected a let");
    };
    assert!(matches!(
        ty.as_ref().map(|ty| &ty.kind),
        Some(TyKind::Tensor { .. })
    ));
}

#[test]
fn a_reserved_word_cannot_be_a_binding() {
    // §B.4 E0206: a keyword where a binder belongs is a reserved-word error, not
    // a generic syntax error.
    for source in ["let if = 1\n", "let x = match 1 with\n    | match -> 0\n"] {
        let diagnostic = support::expect_code(source, "E0206");
        assert_eq!(diagnostic.severity, aevia_syntax::Severity::Error);
    }
}

#[test]
fn an_invalid_pattern_is_e0204() {
    // §B.4 E0204: a float literal cannot be a pattern.
    assert_eq!(
        support::expect_error("let x = match 1 with\n    | 1.5 -> 0\n", "E0204",).code,
        "E0204"
    );
}

#[test]
fn a_capitalised_parameter_with_arguments_is_still_a_constructor_pattern() {
    let module = parse("let f = fn (Some(x)) => x\n").module;
    let DeclKind::Let { value, .. } = &module.decls[0].kind else {
        panic!("expected a let");
    };
    let ExprKind::Lambda { params, .. } = &value.kind else {
        panic!("expected a lambda, got {:?}", value.kind);
    };
    assert!(matches!(params[0].pattern.kind, PatKind::Ctor { .. }));
}

#[test]
fn a_capitalised_let_binds_a_variable() {
    let module = parse("let M = 3\n").module;
    let DeclKind::Let { pattern, .. } = &module.decls[0].kind else {
        panic!("expected a let");
    };
    assert!(matches!(&pattern.kind, PatKind::Var(name) if name == "M"));
}
