//! UTF-8, layout and nesting at block comment boundaries.

use aevia_syntax::Span;
use aevia_syntax::TokenKind;
use aevia_syntax::ast::DeclKind;
use aevia_syntax::ast::ExprKind;
use aevia_syntax::ast::StrPiece;
use aevia_syntax::lex;
use aevia_syntax::lex_inline;
use aevia_syntax::parse_module;

#[test]
fn nested_comments_preserve_layout_and_byte_offsets() {
    for newline in ["\n", "\r", "\r\n"] {
        let source = format!(
            "fn f() ={newline}    let x = 1{newline}    #-甲 #-😀-#{newline}β-#let y = 2{newline}    x + y{newline}"
        );
        let output = lex(5, &source).expect("nested comments close");
        let (module, diagnostics) = parse_module(5, &source, &output);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        assert_eq!(module.decls.len(), 1);
        assert_eq!(
            output
                .tokens
                .iter()
                .filter(|token| matches!(token.kind, TokenKind::Indent))
                .count(),
            1,
        );
        let start = source.find("y = 2").expect("the binding after the comment");
        let binding = output
            .tokens
            .iter()
            .find(|token| matches!(&token.kind, TokenKind::Ident(name) if name == "y"))
            .expect("identifier after the comment");
        assert_eq!(binding.span, Span::new(5, start, start + 1));
    }
}

#[test]
fn interpolation_ignores_delimiters_inside_nested_comments() {
    let source = "let x = \"answer {#- 😀 r\\\" } #-猫-# \\\" -# 42}\"";
    let output = lex(7, source).expect("braces and quotes inside comments are ignored");
    let (module, diagnostics) = parse_module(7, source, &output);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let DeclKind::Let { value, .. } = &module.decls[0].kind else {
        panic!("expected a let declaration");
    };
    let ExprKind::Str(parts) = &value.kind else {
        panic!("expected a string expression");
    };
    let StrPiece::Expr(expr) = &parts[1] else {
        panic!("expected an interpolated expression");
    };
    let start = source.find("42").expect("source of interpolated number");
    assert_eq!(expr.span, Span::new(7, start, start + 2));
}

#[test]
fn an_unclosed_outer_comment_reports_its_full_original_span() {
    let source = "\u{feff}let x = 0\r\n#-甲 #-😀-#\r\n猫-";
    let start = source.find("#-").expect("start of outer comment");
    let diagnostics = lex(13, source).expect_err("the outer comment stays open");
    assert_eq!(diagnostics.items.len(), 1);
    let error = &diagnostics.items[0];
    assert_eq!(error.code, "E0104");
    assert_eq!(error.message, "unclosed block comment");
    assert_eq!(error.span, Span::new(13, start, source.len()));
}

// The reference consumes Unicode scalar values and keeps one stack entry per
// opening delimiter. The closing delimiter cannot reuse an opening byte.
fn reference_comment_end(body: &str) -> Option<usize> {
    let mut chars = body.char_indices().peekable();
    let mut openings = vec![()];
    while let Some((offset, ch)) = chars.next() {
        match (ch, chars.peek().map(|&(_, next)| next)) {
            | ('#', Some('-')) => {
                chars.next();
                openings.push(());
            },
            | ('-', Some('#')) => {
                chars.next();
                openings.pop();
                if openings.is_empty() {
                    return Some(offset + 2);
                }
            },
            | _ => {},
        }
    }
    None
}

#[test]
fn short_marker_sequences_match_a_scalar_reference() {
    let alphabet = ['#', '-', '甲', '😀', '\r', '\n'];
    let base = 37;
    for word_length in 0..=5 {
        for mut choice in 0..alphabet.len().pow(word_length) {
            let mut source = String::from("#-");
            for _ in 0..word_length {
                source.push(alphabet[choice % alphabet.len()]);
                choice /= alphabet.len();
            }
            source.push_str("-# 界");
            let expected = reference_comment_end(&source[2..]);
            let output = lex_inline(11, &source, base);
            if let Some(comment_end) = expected {
                // Compare tokens and spans with lexing the suffix on its own.
                let after = 2 + comment_end;
                let suffix = lex_inline(11, &source[after..], base + after);
                if suffix.tokens.len() == 1 && !suffix.tokens[0].span.is_empty() {
                    // Invalid inline expressions return EOF covering their full input.
                    assert_eq!(output.tokens.len(), 1, "{source:?}");
                    assert_eq!(output.tokens[0].kind, TokenKind::Eof);
                    assert_eq!(
                        output.tokens[0].span,
                        Span::new(11, base, base + source.len()),
                    );
                    assert!(output.docs.is_empty());
                } else {
                    assert_eq!(output.tokens, suffix.tokens, "{source:?}");
                    assert!(
                        output
                            .docs
                            .iter()
                            .map(|doc| (&doc.text, doc.span))
                            .eq(suffix.docs.iter().map(|doc| (&doc.text, doc.span))),
                        "{source:?}",
                    );
                }
            } else {
                let diagnostics = lex(11, &source).expect_err("the outer comment stays open");
                assert_eq!(diagnostics.items.len(), 1, "{source:?}");
                assert_eq!(diagnostics.items[0].code, "E0104", "{source:?}");
                assert_eq!(diagnostics.items[0].span, Span::new(11, 0, source.len()));
            }
        }
    }
}
