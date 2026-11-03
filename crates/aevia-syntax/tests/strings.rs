//! String literals (§1.7): escapes are decoded in every form that has them,
//! and are literal in a raw string.

mod support;

use aevia_syntax::ast::{DeclKind, ExprKind, StrPiece};
use aevia_syntax::token::{StringPart, TokenKind};
use support::{expect_clean, lex_only};

/// The literal text of the only `Str` token in `source`.
fn literal_text(source: &str) -> String {
    let lexed = lex_only(source);
    let parts = lexed
        .tokens
        .iter()
        .find_map(|token| match &token.kind {
            TokenKind::Str(parts) => Some(parts.clone()),
            _ => None,
        })
        .expect("a string token");
    parts
        .into_iter()
        .map(|part| match part {
            StringPart::Text(text) => text,
            StringPart::Interpolation { .. } => "<interp>".to_string(),
        })
        .collect()
}

#[test]
fn a_multiline_string_decodes_a_tab() {
    // The bug: `"""x\ty"""` printed `xty`, because every `\X` was turned into
    // `X` instead of being decoded.
    assert_eq!(literal_text("let s = \"\"\"x\\ty\"\"\"\n"), "x\ty");
}

#[test]
fn a_multiline_string_decodes_the_other_escapes() {
    assert_eq!(
        literal_text("let s = \"\"\"a\\nb\\rc\\\\d\\\"e\"\"\"\n"),
        "a\nb\rc\\d\"e"
    );
}

#[test]
fn a_multiline_string_decodes_a_unicode_escape() {
    assert_eq!(literal_text("let s = \"\"\"\\u{41}\\u{1F600}\"\"\"\n"), "A\u{1F600}");
}

#[test]
fn a_multiline_string_decodes_a_nul_escape() {
    assert_eq!(literal_text("let s = \"\"\"a\\0b\"\"\"\n"), "a\0b");
}

#[test]
fn a_multiline_string_keeps_escaped_braces_literal() {
    // `\{` is a literal brace, not the start of an interpolation.
    let source = "let s = \"\"\"a\\{b\\}c\"\"\"\n";
    let module = expect_clean(source);
    let DeclKind::Let { value, .. } = &module.decls[0].kind else {
        panic!("expected a let");
    };
    let ExprKind::Str(pieces) = &value.kind else {
        panic!("expected a string, got {:?}", value.kind);
    };
    assert_eq!(pieces.len(), 1);
    let StrPiece::Text(text) = &pieces[0] else {
        panic!("expected literal text");
    };
    assert_eq!(text, "a{b}c");
}

#[test]
fn a_multiline_string_still_interpolates() {
    let source = "let s = \"\"\"a{x + 1}b\"\"\"\n";
    let module = expect_clean(source);
    let DeclKind::Let { value, .. } = &module.decls[0].kind else {
        panic!("expected a let");
    };
    let ExprKind::Str(pieces) = &value.kind else {
        panic!("expected a string");
    };
    assert_eq!(pieces.len(), 3);
    assert!(matches!(pieces[1], StrPiece::Expr(_)));
}

#[test]
fn a_raw_string_keeps_escapes_literal() {
    // §1.7: "原始字符串 r\"...\"：反斜杠和花括号都是字面量".
    assert_eq!(literal_text("let s = r\"a\\tb\\n\"\n"), "a\\tb\\n");
}

#[test]
fn a_raw_string_keeps_braces_literal() {
    assert_eq!(literal_text("let s = r\"a{b}c\"\n"), "a{b}c");
}

#[test]
fn a_single_line_string_decodes_a_nul_escape() {
    assert_eq!(literal_text("let s = \"a\\0b\"\n"), "a\0b");
}

#[test]
fn a_single_line_string_decodes_a_tab() {
    assert_eq!(literal_text("let s = \"a\\tb\"\n"), "a\tb");
}

#[test]
fn an_unknown_escape_in_a_multiline_string_is_e0105() {
    // A lexical error is fatal to the file (§1.10), so the diagnostic comes
    // from `lex` rather than from a parse.
    let source = "let s = \"\"\"a\\qb\"\"\"\n";
    let errors = aevia_syntax::lex(0, source).expect_err("E0105");
    assert_eq!(support::codes(&errors), vec!["E0105".to_string()]);
    // The span starts at the literal: the dedented text no longer lines up with
    // the source, so the literal is the smallest honest range.
    let span = errors.items[0].span;
    assert_eq!(
        &source[span.start as usize..span.end as usize],
        "\"\"\"a\\qb"
    );
}

#[test]
fn an_unknown_escape_in_a_single_line_string_is_e0105() {
    let errors = aevia_syntax::lex(0, "let s = \"a\\qb\"\n").expect_err("E0105");
    assert_eq!(support::codes(&errors), vec!["E0105".to_string()]);
}
