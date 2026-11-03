//! Layout: block versus layout group, checked `INDENT`/`DEDENT`, and the
//! `NEWLINE` that ends the last logical line (§1.4, §A.1).

mod support;

use aevia_syntax::ast::{DeclKind, ExprKind};
use aevia_syntax::token::TokenKind;
use support::{codes, expect_clean, expect_code, lex_only, parse};

#[test]
fn let_in_is_a_layout_group_body() {
    // §A.1: `layout_group ::= NEWLINE INDENT expr NEWLINE? DEDENT`, and a
    // `let ... in` is an `expr` (§A.3), so this is a legal body.
    let source = "fn f() =\n    let y = 2 in y + 1\n";
    let module = expect_clean(source);
    let DeclKind::Fn { body, .. } = &module.decls[0].kind else {
        panic!("expected a function");
    };
    assert!(
        matches!(body.kind, ExprKind::LetIn { .. }),
        "expected `let ... in`, got {:?}",
        body.kind
    );
}

#[test]
fn let_in_with_a_nested_let_on_the_value_side_is_a_group_too() {
    let source = "fn f() =\n    let a = let b = 1 in b + 1 in a * 2\n";
    let module = expect_clean(source);
    let DeclKind::Fn { body, .. } = &module.decls[0].kind else {
        panic!("expected a function");
    };
    let ExprKind::LetIn { value, .. } = &body.kind else {
        panic!("expected an outer `let ... in`, got {:?}", body.kind);
    };
    assert!(matches!(value.kind, ExprKind::LetIn { .. }));
}

#[test]
fn a_let_statement_still_selects_a_block() {
    // The value side of the first `let` is itself a `let ... in`; that must not
    // turn the whole group into a `let_expr`.
    let source = "fn f() =\n    let a = let b = 1 in b\n    a + 1\n";
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
fn a_block_without_a_final_expression_is_e0201() {
    // §2.3.1: "块必须至少有一个最终表达式".
    let source = "fn f() =\n    let a = 1\n";
    let diagnostic = expect_code(source, "E0201");
    assert_eq!(&source[diagnostic.span.start as usize..diagnostic.span.end as usize], "\n");
}

#[test]
fn instance_where_without_an_indented_block_is_rejected() {
    // §A.2: `where` is followed by `NEWLINE INDENT { method_def NEWLINE } DEDENT`.
    let source = "instance Show for Vec2 where\nlet x = 1\n";
    let parsed = parse(source);
    assert_eq!(codes(&parsed.diagnostics), vec!["E0103".to_string()]);
    // The declaration after it is still parsed: the enclosing DEDENT was not
    // stolen by the instance body.
    assert_eq!(parsed.module.decls.len(), 2);
}

#[test]
fn instance_where_with_a_block_keeps_the_enclosing_block_closed() {
    let source = "instance Show for Vec2 where\n    show(v) = 1\nlet x = 1\n";
    let parsed = parse(source);
    assert!(
        parsed.diagnostics.items.is_empty(),
        "unexpected diagnostics: {:?}",
        codes(&parsed.diagnostics)
    );
    assert_eq!(parsed.module.decls.len(), 2);
}

#[test]
fn class_where_without_an_indented_block_is_rejected() {
    let source = "class Show a where\nlet x = 1\n";
    let parsed = parse(source);
    assert_eq!(codes(&parsed.diagnostics), vec!["E0103".to_string()]);
    assert_eq!(parsed.module.decls.len(), 2);
}

#[test]
fn inconsistent_indentation_is_e0102() {
    // §1.4.1: a level that is not on the stack is `E0102`. The lexer reports it
    // once per mismatching line, and — see the lexer unit test
    // `an_unmatched_indent_is_not_pushed` — does not adopt the bogus level.
    let source = "fn f() =\n      let x = 1\n    let y = 2\n";
    let error = aevia_syntax::lex(0, source).expect_err("E0102");
    let codes = support::codes(&error);
    assert_eq!(codes, vec!["E0102".to_string()]);
}

#[test]
fn the_last_line_ends_with_a_newline_even_without_a_trailing_newline() {
    // §1.4.1: every logical line ends with `NEWLINE`. Without this the parser
    // cannot tell a complete declaration from a truncated one.
    let lexed = lex_only("let x = 1");
    let kinds: Vec<&TokenKind> = lexed.tokens.iter().map(|token| &token.kind).collect();
    let last = kinds.len().saturating_sub(2);
    assert!(
        matches!(kinds[last], TokenKind::Newline),
        "expected a final NEWLINE, got {kinds:?}"
    );
}

#[test]
fn a_file_ending_in_a_newline_does_not_get_a_second_one() {
    let lexed = lex_only("let x = 1\n");
    let newlines = lexed
        .tokens
        .iter()
        .filter(|token| matches!(token.kind, TokenKind::Newline))
        .count();
    assert_eq!(newlines, 1);
}

#[test]
fn a_block_that_loses_its_dedent_is_reported() {
    // The inner `let` is followed by a line at the *same* indent, so the block
    // has no tail expression; the parser must not silently steal the outer
    // `DEDENT`.
    let source = "fn f() =\n    let a =\n        let b = 1\n    a\n";
    let parsed = parse(source);
    assert!(
        !parsed.diagnostics.items.is_empty(),
        "expected a layout diagnostic"
    );
    assert_eq!(parsed.module.decls.len(), 1);
}
