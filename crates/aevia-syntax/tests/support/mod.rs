//! Helpers shared by the conformance tests.
//!
//! Every test asserts on the diagnostic *code* and on the *span*, never on the
//! message text, so the wording stays free to change (PLAN.md §3.2).

#![allow(dead_code)]

use aevia_syntax::ast::Module;
use aevia_syntax::{lex, parse_module, Diagnostic, Diagnostics, LexOutput};

/// A parsed file: the tree and everything the lexer and parser reported.
pub struct Parsed {
    /// The tree, always produced even when there are errors.
    pub module: Module,
    /// Lexical and syntactic diagnostics, in source order.
    pub diagnostics: Diagnostics,
}

/// Lexes and parses `source` as the entry file.
#[must_use]
pub fn parse(source: &str) -> Parsed {
    let lexed = lex(0, source).expect("the file lexes");
    let (module, diagnostics) = parse_module(0, source, &lexed);
    Parsed {
        module,
        diagnostics,
    }
}

/// Lexes `source` and returns the token stream, for layout assertions.
#[must_use]
pub fn lex_only(source: &str) -> LexOutput {
    lex(0, source).expect("the file lexes")
}

/// The codes of every diagnostic, in source order.
#[must_use]
pub fn codes(diagnostics: &Diagnostics) -> Vec<String> {
    diagnostics
        .items
        .iter()
        .map(|item| item.code.clone())
        .collect()
}

/// The source text a diagnostic underlines.
#[must_use]
pub fn span_text<'a>(source: &'a str, diagnostic: &Diagnostic) -> &'a str {
    let start = usize::try_from(diagnostic.span.start).unwrap_or(0);
    let end = usize::try_from(diagnostic.span.end).unwrap_or(0);
    source.get(start..end).unwrap_or("<out of range>")
}

/// Asserts that `source` produces no diagnostics at all.
pub fn expect_clean(source: &str) -> Module {
    let parsed = parse(source);
    assert!(
        parsed.diagnostics.items.is_empty(),
        "expected no diagnostics, got {:#?}",
        parsed
            .diagnostics
            .items
            .iter()
            .map(|item| (item.code.clone(), span_text(source, item).to_string()))
            .collect::<Vec<_>>()
    );
    parsed.module
}

/// Asserts that `source` produces exactly one diagnostic, with `code`, and
/// returns it.
pub fn expect_error(source: &str, code: &str) -> Diagnostic {
    let mut parsed = parse(source);
    let found = codes(&parsed.diagnostics);
    assert_eq!(
        found,
        vec![code.to_string()],
        "expected exactly one {code}, got {:#?}",
        parsed
            .diagnostics
            .items
            .iter()
            .map(|item| (item.code.clone(), span_text(source, item).to_string()))
            .collect::<Vec<_>>()
    );
    parsed.diagnostics.items.remove(0)
}

/// Asserts that `source` produces `code` at least once, and returns the first
/// such diagnostic.
pub fn expect_code(source: &str, code: &str) -> Diagnostic {
    let parsed = parse(source);
    let found = codes(&parsed.diagnostics);
    parsed
        .diagnostics
        .items
        .into_iter()
        .find(|item| item.code == code)
        .unwrap_or_else(|| panic!("expected {code}, got {found:?}"))
}
