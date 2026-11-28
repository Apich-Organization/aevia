//! String literals (§1.7): escapes are decoded in every form that has them,
//! and are literal in a raw string.

mod support;

use aevia_syntax::ast::DeclKind;
use aevia_syntax::ast::ExprKind;
use aevia_syntax::ast::NumberLit;
use aevia_syntax::ast::StrPiece;
use aevia_syntax::token::StringPart;
use aevia_syntax::token::TokenKind;
use support::expect_clean;
use support::lex_only;
use support::span_text;

/// The literal text of the only `Str` token in `source`.
fn literal_text(source: &str) -> String {
    let lexed = lex_only(source);
    let parts = lexed
        .tokens
        .iter()
        .find_map(|token| {
            match &token.kind {
                | TokenKind::Str(parts) => Some(parts.clone()),
                | _ => None,
            }
        })
        .expect("a string token");
    parts
        .into_iter()
        .map(|part| {
            match part {
                | StringPart::Text(text) => text,
                | StringPart::Interpolation { .. } => "<interp>".to_string(),
            }
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
    assert_eq!(
        literal_text("let s = \"\"\"\\u{41}\\u{1F600}\"\"\"\n"),
        "A\u{1F600}"
    );
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

#[test]
fn single_line_string_spans_stop_before_every_line_ending() {
    for line_end in ["\n", "\r\n", "\r"] {
        for literal in ["\"unclosed", "r\"unclosed"] {
            let source = format!("let s = {literal}{line_end}let next = 1");
            let errors = aevia_syntax::lex(0, &source).expect_err("unclosed string");
            assert_eq!(support::codes(&errors), ["E0104"], "{source:?}");
            let span = errors.items[0].span;
            // The raw string scanner's span starts at the opening quote.
            assert_eq!(
                &source[span.start as usize..span.end as usize],
                "\"unclosed"
            );
        }
    }
}

fn expression_source<'s>(
    source: &'s str,
    expr: &aevia_syntax::Expr,
) -> &'s str {
    &source[expr.span.start as usize..expr.span.end as usize]
}

#[test]
fn dedented_interpolation_keeps_exact_decimal_text_and_original_spans() {
    for line_end in ["\n", "\r\n", "\r"] {
        let source = [
            "let s = \"\"\"",
            "    α{(",
            "      9_007_199_254_740_993.0",
            "    )} followed by {2e-400 m^1.5}",
            "    tail",
            "\"\"\"",
        ]
        .join(line_end);
        let module = expect_clean(&source);
        let DeclKind::Let { value, .. } = &module.decls[0].kind else {
            panic!("expected a let");
        };
        let ExprKind::Str(parts) = &value.kind else {
            panic!("expected a string");
        };
        let [
            StrPiece::Text(start),
            StrPiece::Expr(decimal),
            StrPiece::Text(mid),
            StrPiece::Expr(quantity),
            StrPiece::Text(end),
        ] = &parts[..]
        else {
            panic!("expected two interpolations, got {parts:?}");
        };
        assert_eq!(
            (start.as_str(), mid.as_str(), end.as_str()),
            ("α", " followed by ", "\ntail")
        );
        assert!(matches!(&decimal.kind, ExprKind::Float(text) if text == "9007199254740993.0"));
        assert_eq!(
            expression_source(&source, decimal),
            "9_007_199_254_740_993.0"
        );
        assert!(matches!(
            &quantity.kind,
            ExprKind::Quantity { value: NumberLit::Decimal(text), unit }
                if text == "2e-400" && unit.to_source() == "m^(3/2)"
        ));
        assert_eq!(expression_source(&source, quantity), "2e-400 m^1.5");
    }
}

#[test]
fn interpolation_reports_lexical_and_syntactic_errors_at_the_actual_tokens() {
    for (source, code, offending) in [
        ("\"\"\"\r\n    α{(1 + )}\r\n\"\"\"", "E0202", ")"),
        ("\"\"\"\r    α{ $ }\r\"\"\"", "E0105", "$"),
        (r#""{[\"ok\", $]}""#, "E0105", "$"),
        (r#""{[\"ok\", 1 + ]}""#, "E0202", "]"),
        (r#""{[\{ a = 1 \}, $]}""#, "E0105", "$"),
    ] {
        let diagnostics =
            aevia_syntax::parse_expression(7, source).expect_err("malformed interpolation");
        assert_eq!(support::codes(&diagnostics), [code], "{source:?}");
        let diagnostic = &diagnostics.items[0];
        assert_eq!(diagnostic.span.file, 7);
        assert_eq!(span_text(source, diagnostic), offending, "{source:?}");
    }
}

#[test]
fn protected_delimiters_keep_the_spelling_of_following_numbers() {
    let source = r#""{[\"ok\", \{ a = 1 \}, 1_5.25]}""#;
    let parsed = aevia_syntax::parse_expression(0, source).expect("protected delimiters");
    let ExprKind::Str(parts) = parsed.kind else {
        panic!("expected a string");
    };
    let [StrPiece::Expr(items)] = &parts[..] else {
        panic!("expected a single interpolation");
    };
    let ExprKind::Tensor(items) = &items.kind else {
        panic!("expected a tensor expression");
    };
    assert!(matches!(&items[2].kind, ExprKind::Float(text) if text == "15.25"));
    assert_eq!(expression_source(source, &items[2]), "1_5.25");
}

#[test]
fn protected_delimiter_offsets_compose_inside_nested_interpolation() {
    let source = r#""{ "nested {[\"ok\", 2.75]}" }""#;
    let parsed = aevia_syntax::parse_expression(0, source).expect("nested protected delimiters");
    let ExprKind::Str(parts) = parsed.kind else {
        panic!("expected a string");
    };
    let [StrPiece::Expr(nested)] = &parts[..] else {
        panic!("expected a single interpolation");
    };
    let ExprKind::Str(parts) = &nested.kind else {
        panic!("expected a nested string");
    };
    let [StrPiece::Text(_), StrPiece::Expr(items)] = &parts[..] else {
        panic!("expected a nested interpolation");
    };
    let ExprKind::Tensor(items) = &items.kind else {
        panic!("expected a tensor expression");
    };
    assert!(matches!(&items[1].kind, ExprKind::Float(text) if text == "2.75"));
    assert_eq!(expression_source(source, &items[1]), "2.75");
}

#[test]
fn nested_strings_consume_their_own_delimiters_and_escapes() {
    let source = r##""""before { [r"}\", "\{brace\} \"quote\"", """inside {1.25}"""] } after""""##;
    let parsed = aevia_syntax::parse_expression(0, source).expect("nested string interpolation");
    let ExprKind::Str(parts) = parsed.kind else {
        panic!("expected a string");
    };
    let [
        StrPiece::Text(start),
        StrPiece::Expr(items),
        StrPiece::Text(end),
    ] = &parts[..]
    else {
        panic!("expected a single interpolation");
    };
    assert_eq!((start.as_str(), end.as_str()), ("before ", " after"));
    let ExprKind::Tensor(items) = &items.kind else {
        panic!("expected a tensor expression");
    };
    assert!(matches!(&items[0].kind, ExprKind::Str(parts)
        if matches!(&parts[..], [StrPiece::Text(text)] if text == "}\\")));
    assert!(matches!(&items[1].kind, ExprKind::Str(parts)
        if matches!(&parts[..], [StrPiece::Text(text)] if text == "{brace} \"quote\"")));
    let ExprKind::Str(parts) = &items[2].kind else {
        panic!("expected a nested multiline string");
    };
    let [StrPiece::Text(start), StrPiece::Expr(decimal)] = &parts[..] else {
        panic!("expected a nested interpolation");
    };
    assert_eq!(start, "inside ");
    assert!(matches!(&decimal.kind, ExprKind::Float(text) if text == "1.25"));
    assert_eq!(expression_source(source, decimal), "1.25");
}

#[test]
fn comments_cannot_close_an_interpolation() {
    let source = "\"\"\"{(1 #- } \"\"\" #- } -# -# + # }\r\n  2)}\"\"\"";
    let parsed = aevia_syntax::parse_expression(0, source).expect("delimiters inside comments");
    let ExprKind::Str(parts) = parsed.kind else {
        panic!("expected a string");
    };
    assert!(
        matches!(&parts[..], [StrPiece::Expr(expr)] if matches!(expr.kind, ExprKind::Binary { .. }))
    );
}

#[test]
fn malformed_multiline_interpolation_reports_its_own_braces() {
    for (source, expected) in [
        ("\"\"\"x{ }y\"\"\"", "{ }"),
        (
            "\"\"\"\r\n    x{1 + 2\r\n\"\"\"\r\nlet after = 7",
            "{1 + 2\r\n",
        ),
    ] {
        let diagnostics =
            aevia_syntax::lex(0, source).expect_err("unterminated or empty interpolation");
        assert_eq!(support::codes(&diagnostics), ["E0104"]);
        assert_eq!(span_text(source, &diagnostics.items[0]), expected);
    }
}

#[test]
fn quote_delimiter_scanning_limits_recursion() {
    let source = format!("{}0{}", "\"{".repeat(128), "}\"".repeat(128));
    let diagnostics = aevia_syntax::lex(0, &source).expect_err("deeply nested interpolation");
    assert_eq!(support::codes(&diagnostics), ["E0104"]);
    assert!(diagnostics.items[0].message.contains("nesting is too deep"));
}

#[test]
fn bare_interpolation_names_keep_identifier_rules_and_spans() {
    let source = "\"\"\"{\r\n    α₁'} {\r\n    x1_'} {_} {Upper} {true} {e\u{301}}\"\"\"";
    let parsed = aevia_syntax::parse_expression(0, source).expect("interpolation words");
    let ExprKind::Str(parts) = &parsed.kind else {
        panic!("expected a string");
    };
    let values: Vec<_> = parts
        .iter()
        .filter_map(|part| {
            match part {
                | StrPiece::Text(_) => None,
                | StrPiece::Expr(value) => Some(value.as_ref()),
            }
        })
        .collect();
    assert_eq!(values.len(), 6);
    assert_eq!(expression_source(source, values[1]), "x1_'");
    assert!(matches!(&values[0].kind, ExprKind::Var(name) if name == "α₁'"));
    assert!(matches!(&values[1].kind, ExprKind::Var(name) if name == "x1_'"));
    assert!(matches!(&values[2].kind, ExprKind::Placeholder));
    assert!(matches!(&values[3].kind, ExprKind::Var(name) if name == "Upper"));
    assert!(matches!(&values[4].kind, ExprKind::Bool(true)));
    assert!(matches!(&values[5].kind, ExprKind::Var(name) if name == "é"));

    let diagnostics = aevia_syntax::parse_expression(0, "\"{\u{c}name}\"")
        .expect_err("form feed is not inline layout");
    assert_eq!(support::codes(&diagnostics), ["E0105"]);
}
