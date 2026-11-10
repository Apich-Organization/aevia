//! The editor tooling beyond the first six queries: diagnostics the checker
//! does not produce itself, code actions, and semantic tokens.
//!
//! Every code action is applied to the document and re-analysed here, so a
//! test asserts the same thing the server promises: the edit it offers
//! produces a document with no errors.

use aevia_analysis::Analysis;
use aevia_analysis::CodeAction;
use aevia_analysis::CodeActionKind;
use aevia_analysis::Document;
use aevia_analysis::Range;
use aevia_analysis::Severity;
use aevia_analysis::TokenKind;

fn document(source: &str) -> Document {
    Document::new("test.aev", 1, source)
}

fn offset_of(
    source: &str,
    needle: &str,
    nth: usize,
) -> u32 {
    let mut from = 0;
    let mut found = None;
    for _ in 0..nth.max(1) {
        let at = source
            .get(from..)
            .and_then(|rest| rest.find(needle))
            .map(|at| at + from)
            .unwrap_or_else(|| panic!("`{needle}` does not occur {nth} times in {source:?}"));
        found = Some(at);
        from = at + needle.len();
    }
    u32::try_from(found.expect("at least one")).expect("offset")
}

/// A zero-width request at the byte `offset`.
fn cursor(
    document: &Document,
    offset: u32,
) -> Range {
    Range::new(document.position(offset), document.position(offset))
}

/// The source text a range covers.
fn text_of<'a>(
    document: &Document,
    source: &'a str,
    range: Range,
) -> &'a str {
    let start = document.offset(range.start).expect("start");
    let end = document.offset(range.end).expect("end");
    &source[start as usize..end as usize]
}

/// Applies a code action's edits, returning the new document text.
fn apply(
    document: &Document,
    action: &CodeAction,
) -> String {
    let mut text = document.text().to_string();
    let mut edits: Vec<_> = action.edits.iter().collect();
    edits.sort_by_key(|edit| std::cmp::Reverse(document.offset(edit.range.start).expect("start")));
    for edit in edits {
        let start = document.offset(edit.range.start).expect("start") as usize;
        let end = document.offset(edit.range.end).expect("end") as usize;
        text.replace_range(start..end, &edit.new_text);
    }
    text
}

/// The action whose title contains `needle`.
fn action<'a>(
    actions: &'a [CodeAction],
    needle: &str,
) -> &'a CodeAction {
    actions
        .iter()
        .find(|action| action.title.contains(needle))
        .unwrap_or_else(|| panic!("no action containing `{needle}`: {actions:#?}"))
}

/// Re-analysis of a rewritten document, which must be clean.
fn assert_clean(text: &str) {
    let document = Document::new("rewritten.aev", 1, text);
    let analysis = Analysis::new(&document);
    assert!(
        analysis.diagnostics().is_empty(),
        "the rewritten document does not compile: {:#?}",
        analysis.diagnostics()
    );
}

// --- diagnostics -----------------------------------------------------------

#[test]
fn an_unused_local_binding_is_reported_once() {
    // The checker owns `W0101`; the analysis layer must convert and place it
    // exactly once — an editor lint that duplicated the compiler's would show
    // two squiggles for one problem.
    let source = "fn f(x: Int): Int = 1\n";
    let document = document(source);
    let analysis = Analysis::new(&document);
    let unused: Vec<_> = analysis
        .diagnostics()
        .into_iter()
        .filter(|diagnostic| diagnostic.code.as_deref() == Some("W0101"))
        .collect();
    assert_eq!(unused.len(), 1, "{unused:#?}");
    assert_eq!(unused[0].severity, Severity::Warning);
    let start = offset_of(source, "x: Int", 1);
    assert_eq!(unused[0].range, document.range_of(start, start + 1));
    assert!(
        unused[0].message.to_lowercase().contains("unused"),
        "{}",
        unused[0].message
    );
}

#[test]
fn a_used_or_underscored_binding_is_not_warned_about() {
    for source in [
        "fn f(x: Int): Int = x + 1\n",
        "fn f(_x: Int): Int = 1\n",
        "let top = 1\n",
        "fn g() =\n    let y = 2\n    y\n",
    ] {
        let document = document(source);
        let analysis = Analysis::new(&document);
        assert!(
            !analysis
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code.as_deref() == Some("W0101")),
            "a false W0101 for {source:?}: {:#?}",
            analysis.diagnostics()
        );
    }
}

#[test]
fn a_non_exhaustive_match_is_reported_as_e0309() {
    // The checker reports this one; the test pins that the analysis layer
    // passes it through (§9.10 lists it as an editor diagnostic).
    let source = "type Shape = | Circle(Real[L]) | Dot\n\nfn area(s: Shape): Int =\n    match s with\n    | Dot -> 1\n";
    let document = document(source);
    let analysis = Analysis::new(&document);
    let missing: Vec<_> = analysis
        .diagnostics()
        .into_iter()
        .filter(|diagnostic| diagnostic.code.as_deref() == Some("E0309"))
        .collect();
    assert_eq!(missing.len(), 1, "{missing:#?}");
    assert!(
        missing[0].message.contains("non-exhaustive"),
        "{}",
        missing[0].message
    );
}

// --- code actions ----------------------------------------------------------

#[test]
fn a_binder_without_an_annotation_gets_one() {
    let source = "let x = 1.0\n";
    let document = document(source);
    let analysis = Analysis::new(&document);
    let actions = analysis.code_actions(cursor(&document, offset_of(source, "x", 1)));
    let action = action(&actions, "Add type annotation");
    assert_eq!(action.kind, CodeActionKind::QuickFix);
    assert!(action.title.contains("x: Float"), "{}", action.title);
    let rewritten = apply(&document, action);
    assert!(rewritten.contains("let x: Float = 1.0"), "{rewritten}");
    assert_clean(&rewritten);
}

#[test]
fn a_celsius_literal_is_converted_to_kelvin() {
    let source = "let t = 25.0 °C\n";
    let document = document(source);
    let analysis = Analysis::new(&document);
    let actions = analysis.code_actions(cursor(&document, offset_of(source, "°C", 1)));
    let action = action(&actions, "Convert");
    let rewritten = apply(&document, action);
    assert!(rewritten.contains("K"), "{rewritten}");
    assert!(!rewritten.contains("°C"), "{rewritten}");
    // The replacement is the interpreter's own conversion, not a guess.
    assert!(rewritten.contains("298.15"), "{rewritten}");
    assert_clean(&rewritten);
}

#[test]
fn an_expression_can_be_extracted_into_a_let() {
    let source = "let y = 1 + 2 * 3\n";
    let document = document(source);
    let analysis = Analysis::new(&document);
    let actions = analysis.code_actions(cursor(&document, offset_of(source, "*", 1)));
    let action = action(&actions, "Extract");
    assert_eq!(action.kind, CodeActionKind::RefactorExtract);
    let rewritten = apply(&document, action);
    assert!(rewritten.contains("let __extracted = 2 * 3"), "{rewritten}");
    assert!(rewritten.contains("let y = 1 + __extracted"), "{rewritten}");
    assert_clean(&rewritten);
}

#[test]
fn a_dimension_mismatch_is_fixed_by_adding_the_missing_unit() {
    let source = "let x = 1 m + 1\n";
    let document = document(source);
    let analysis = Analysis::new(&document);
    assert!(
        !analysis.diagnostics().is_empty(),
        "the source must fail first"
    );
    let actions = analysis.code_actions(cursor(&document, offset_of(source, "+", 1)));
    let action = action(&actions, "Add `m`");
    assert_eq!(action.kind, CodeActionKind::QuickFix);
    let rewritten = apply(&document, action);
    assert_eq!(rewritten, "let x = 1 m + 1 m\n");
    assert_clean(&rewritten);
}

#[test]
fn a_position_with_nothing_to_fix_offers_no_action() {
    let source = "let x = 1.0\n";
    let document = document(source);
    let analysis = Analysis::new(&document);
    let end = document.len() as u32;
    assert!(
        analysis.code_actions(cursor(&document, end)).is_empty(),
        "{:#?}",
        analysis.code_actions(cursor(&document, end))
    );
}

#[test]
fn a_unit_literal_jumps_to_its_declaration() {
    // §9.10 asks for unit-declaration jump targets. The unit inside a literal
    // is not a name occurrence in the AST, but the walk recovers it, so
    // "go to definition" on `usd` lands on the `unit usd` declaration.
    let source = "dimension Currency\nunit usd : Currency = 1\n\nlet price = 3.0 usd\n";
    let document = document(source);
    let analysis = Analysis::new(&document);
    let location = analysis
        .definition(offset_of(source, "usd", 2))
        .expect("a definition for the unit in the literal");
    let declaration = offset_of(source, "usd", 1);
    assert_eq!(
        location.range,
        document.range_of(declaration, declaration + 3)
    );
}

#[test]
fn a_value_jumps_to_the_declaration_of_its_type() {
    // §9.10's "type definition": the value is a `Dot`, the type is `Shape`,
    // and the jump lands on the `type Shape` declaration.
    let source = "type Shape = | Circle(Real[L]) | Dot\nlet s = Dot\n";
    let shape_document = document(source);
    let analysis = Analysis::new(&shape_document);
    let location = analysis
        .type_definition(offset_of(source, "Dot", 2))
        .expect("a type definition");
    let declaration = offset_of(source, "Shape", 1);
    assert_eq!(
        location.range,
        shape_document.range_of(declaration, declaration + 5)
    );
    // A primitive has no declaration to jump to.
    let numeric_source = "let x = 1.0\n";
    let numeric = document(numeric_source);
    let analysis = Analysis::new(&numeric);
    assert_eq!(
        analysis.type_definition(offset_of(numeric_source, "x", 1)),
        None
    );
}

// --- semantic tokens -------------------------------------------------------

/// The text and kind of every token, in order.
fn token_list(source: &str) -> Vec<(String, TokenKind)> {
    let document = Document::new("tokens.aev", 1, source);
    let analysis = Analysis::new(&document);
    let tokens = analysis.semantic_tokens();
    // Ordered and non-overlapping is part of the contract.
    for pair in tokens.windows(2) {
        assert!(
            (pair[0].range.start.line, pair[0].range.start.column)
                < (pair[1].range.start.line, pair[1].range.start.column),
            "tokens are not ordered: {tokens:#?}"
        );
    }
    tokens
        .iter()
        .map(|token| {
            (
                text_of(&document, source, token.range).to_string(),
                token.kind,
            )
        })
        .collect()
}

fn kinds_of(
    tokens: &[(String, TokenKind)],
    text: &str,
) -> Vec<TokenKind> {
    tokens
        .iter()
        .filter(|(token, _)| token == text)
        .map(|(_, kind)| *kind)
        .collect()
}

#[test]
fn types_dimensions_and_units_are_highlighted() {
    let source = "dimension Currency\n\
                  unit usd : Currency = 1\n\
                  \n\
                  fn speed(d: Real[L], t: Real[T]): Real[L/T] = d / t\n\
                  \n\
                  let price = 3.0 usd\n";
    let tokens = token_list(source);
    assert_eq!(
        kinds_of(&tokens, "Currency"),
        [TokenKind::Dimension, TokenKind::Dimension],
        "{tokens:#?}"
    );
    assert_eq!(
        kinds_of(&tokens, "usd"),
        [TokenKind::Unit, TokenKind::Unit],
        "{tokens:#?}"
    );
    assert!(
        kinds_of(&tokens, "Real")
            .iter()
            .all(|kind| *kind == TokenKind::Type),
        "{tokens:#?}"
    );
    assert_eq!(kinds_of(&tokens, "L")[0], TokenKind::Dimension);
    assert_eq!(kinds_of(&tokens, "T")[0], TokenKind::Dimension);
    assert_eq!(kinds_of(&tokens, "speed")[0], TokenKind::Function);
    assert_eq!(kinds_of(&tokens, "price")[0], TokenKind::Variable);
    assert_eq!(kinds_of(&tokens, "d")[0], TokenKind::Variable);
}

#[test]
fn the_units_inside_a_literal_are_highlighted() {
    let tokens = token_list("let g = 9.80665 m/s^2\n");
    assert_eq!(kinds_of(&tokens, "m"), [TokenKind::Unit], "{tokens:#?}");
    assert_eq!(kinds_of(&tokens, "s"), [TokenKind::Unit], "{tokens:#?}");
    assert_eq!(kinds_of(&tokens, "g"), [TokenKind::Variable], "{tokens:#?}");
}

#[test]
fn a_resolved_kernel_operator_shows_its_provenance() {
    let tokens = token_list("let v = Rssn.sin(0.5)\n");
    assert_eq!(
        kinds_of(&tokens, "Rssn.sin"),
        [TokenKind::Operator],
        "{tokens:#?}"
    );
}

#[test]
fn a_class_method_call_is_an_operator() {
    let source = "let s = show(1.0)\n";
    let tokens = token_list(source);
    assert!(
        tokens
            .iter()
            .any(|(text, kind)| text == "show" && *kind == TokenKind::Operator),
        "{tokens:#?}"
    );
}

#[test]
fn a_variant_constructor_is_not_a_type() {
    let source = "type Shape = | Circle(Real[L]) | Dot\nlet s = Dot\n";
    let tokens = token_list(source);
    assert_eq!(kinds_of(&tokens, "Circle")[0], TokenKind::Constructor);
    assert_eq!(
        kinds_of(&tokens, "Dot"),
        [TokenKind::Constructor, TokenKind::Constructor]
    );
}
