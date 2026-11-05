//! The semantic queries, exercised the way a client calls them: through a
//! document, a byte offset, and the public result types.

use aevia_analysis::Analysis;
use aevia_analysis::CompletionKind;
use aevia_analysis::Document;
use aevia_analysis::DocumentStore;
use aevia_analysis::Origin;
use aevia_analysis::Position;
use aevia_analysis::Range;
use aevia_analysis::Severity;
use aevia_analysis::SymbolKind;

/// Runs `body` against an analysis of `source`.
fn with<R>(
    source: &str,
    body: impl FnOnce(&Analysis<'_>) -> R,
) -> R {
    let document = Document::new("test.aev", 1, source);
    let analysis = Analysis::new(&document);
    body(&analysis)
}

/// The byte offset of the `nth` occurrence of `needle`, counting from one.
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

/// The source text a range covers, which is what a client shows.
fn text_of<'a>(
    document: &Document,
    source: &'a str,
    range: Range,
) -> &'a str {
    let start = document.offset(range.start).expect("start");
    let end = document.offset(range.end).expect("end");
    &source[start as usize..end as usize]
}

#[test]
fn a_type_error_becomes_a_placed_diagnostic() {
    let source = "let x = 1 m + 1\n";
    with(source, |analysis| {
        let diagnostics = analysis.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.code.as_deref(), Some("E0401"));
        assert_eq!(diagnostic.severity, Severity::Error);
        // `1 m + 1` starts at byte 8 and ends at 15.
        assert_eq!(diagnostic.range, analysis.document().range_of(8, 15));
        assert_eq!(diagnostic.range.start, Position::new(0, 8));
        assert!(diagnostic.message.contains("dimension mismatch"));
        // The primary label and the two secondary ones all survive.
        assert!(diagnostic.message.contains("expected 1, found L"));
        assert_eq!(diagnostic.related.len(), 2, "{diagnostic:?}");
        assert_eq!(diagnostic.related[0].message, "this is L");
    });
}

#[test]
fn a_clean_file_has_no_diagnostics() {
    with("let g = 9.80665 m/s^2\n", |analysis| {
        assert!(
            analysis.diagnostics().is_empty(),
            "{:?}",
            analysis.diagnostics()
        );
    });
}

#[test]
fn hover_reports_the_exported_scheme_of_a_binding() {
    let source = "let g = 9.80665 m/s^2\n";
    with(source, |analysis| {
        let hover = analysis.hover(offset_of(source, "g", 1)).expect("hover");
        assert!(hover.contents.contains("g:"), "{}", hover.contents);
        assert!(hover.contents.contains("Float"), "{}", hover.contents);
        // The range covers the identifier, not the document.
        assert_eq!(hover.range, Some(analysis.document().range_of(4, 5)));
    });
}

#[test]
fn hover_reports_the_scheme_of_a_function() {
    let source = "fn double(x: Int): Int = x + x\nlet y = double(2)\n";
    with(source, |analysis| {
        let hover = analysis
            .hover(offset_of(source, "double", 2))
            .expect("hover on the call");
        assert!(
            hover.contents.contains("double: Int -> Int"),
            "{}",
            hover.contents
        );
    });
}

#[test]
fn hover_reports_a_prelude_builtin() {
    let source = "let a = sqrt(4.0)\n";
    with(source, |analysis| {
        let hover = analysis.hover(offset_of(source, "sqrt", 1)).expect("hover");
        assert!(hover.contents.contains("sqrt:"), "{}", hover.contents);
        assert!(hover.contents.contains("prelude"), "{}", hover.contents);
    });
}

#[test]
fn hover_reports_the_pinned_overload_of_a_kernel_call() {
    let source = "let v = Rssn.sin(0.5)\n";
    with(source, |analysis| {
        let hover = analysis
            .hover(offset_of(source, "Rssn.sin", 1))
            .expect("hover on the kernel call");
        assert!(hover.contents.contains("Rssn.sin"), "{}", hover.contents);
        assert!(
            hover.contents.contains("pinned overload"),
            "{}",
            hover.contents
        );
    });
}

#[test]
fn hover_reports_the_class_of_a_resolved_method_call() {
    let source = "let s = show(1.0)\n";
    with(source, |analysis| {
        let hover = analysis.hover(offset_of(source, "show", 1)).expect("hover");
        assert!(
            hover.contents.contains("class method"),
            "{}",
            hover.contents
        );
        assert!(hover.contents.contains("Show"), "{}", hover.contents);
    });
}

#[test]
fn hover_on_a_local_binding_reports_its_type() {
    // The checker records a type per expression, so a local — which it never
    // exports — is no longer a hole in the answer.
    let source = "fn f(x: Int): Int = x + 1\n";
    with(source, |analysis| {
        let hover = analysis
            .hover(offset_of(source, "x + 1", 1))
            .expect("hover");
        assert_eq!(hover.contents, "```aevia\nx: Int\n```");
        assert_eq!(hover.range, Some(analysis.document().range_of(20, 21)));
    });
}

#[test]
fn hover_on_a_let_binder_and_its_use_agree() {
    let source = "fn g() =\n    let x = 1.0\n    x\n";
    with(source, |analysis| {
        let binder = analysis
            .hover(offset_of(source, "x = 1.0", 1))
            .expect("hover on the binder");
        let use_ = analysis
            .hover(offset_of(source, "x\n", 1))
            .expect("hover on the use");
        assert_eq!(binder.contents, "```aevia\nx: Float\n```");
        assert_eq!(use_.contents, binder.contents);
    });
}

#[test]
fn hover_on_an_expression_reports_its_type() {
    // `1.0 + 2` is `Real`: `Float ⊗ Exact → Real` (§6.3.2). The cursor is on
    // the operator, which is inside no name, so the answer can only come from
    // the expression table.
    let source = "let x = 1.0 + 2\n";
    with(source, |analysis| {
        let on_operator = analysis.hover(offset_of(source, "+", 1)).expect("hover");
        assert_eq!(on_operator.contents, "```aevia\nReal\n```");
        assert_eq!(on_operator.range, Some(analysis.document().range_of(8, 15)));
        // The literal on the left is the same type, and its own span.
        let on_literal = analysis.hover(offset_of(source, "1.0", 1)).expect("hover");
        assert_eq!(on_literal.contents, "```aevia\nReal\n```");
        assert_eq!(on_literal.range, Some(analysis.document().range_of(8, 11)));
    });
}

#[test]
fn hover_on_a_call_reports_the_result_not_the_function() {
    let source = "fn shout(n: Int): String = \"s\"\nlet y = shout(1)\n";
    with(source, |analysis| {
        // On the closing parenthesis the innermost expression is the call.
        let call = offset_of(source, "shout(1)", 1);
        let hover = analysis.hover(call + 7).expect("hover");
        assert_eq!(hover.contents, "```aevia\nString\n```");
        assert_eq!(
            hover.range,
            Some(analysis.document().range_of(call, call + 8))
        );
        // On the name, the function's own scheme is still the answer.
        let callee = analysis
            .hover(offset_of(source, "shout", 2))
            .expect("hover");
        assert!(
            callee.contents.contains("shout: Int -> String"),
            "{}",
            callee.contents
        );
    });
}

#[test]
fn hover_on_whitespace_is_silent() {
    let source = "let x = 1\n\nfn f(y: Int): Int =\n    y\n";
    with(source, |analysis| {
        // The blank line after the first declaration.
        assert_eq!(analysis.hover(offset_of(source, "\n\n", 1) + 1), None);
        // The indentation before the body: inside the block's span, but not an
        // expression.
        assert_eq!(analysis.hover(offset_of(source, "    y", 1) + 1), None);
    });
}

#[test]
fn hover_on_an_undecided_expression_is_silent() {
    // `x + 1.0` inside a polymorphic function still has representation and
    // dimension variables; showing `ρ3` would look like information.
    let source = "fn f(x) = x + 1.0\n";
    with(source, |analysis| {
        assert_eq!(analysis.hover(offset_of(source, "+", 1)), None);
    });
}

#[test]
fn a_binding_whose_inference_failed_does_not_report_a_type_variable() {
    // `bad` is exported, but its scheme is a bare variable because the
    // dimension error stopped inference. `t0` is not a type, and showing it
    // would look like information.
    let source = "let bad = 1 m + 1\n";
    with(source, |analysis| {
        assert_eq!(analysis.hover(offset_of(source, "bad", 1)), None);
        let item = analysis
            .completion(0)
            .into_iter()
            .find(|item| item.label == "bad")
            .expect("the binding is still offered");
        assert_eq!(item.detail, None);
    });
}

#[test]
fn hover_on_a_constructor_names_its_type() {
    let source = "type Shape = | Circle(Real[L]) | Dot\nlet s = Dot\n";
    with(source, |analysis| {
        let hover = analysis
            .hover(offset_of(source, "Dot", 2))
            .expect("hover on the constructor");
        assert!(
            hover.contents.contains("constructor of `Shape`"),
            "{}",
            hover.contents
        );
    });
}

#[test]
fn completion_offers_the_module_and_the_prelude_without_duplicates() {
    let source = "let mine = 1\n";
    with(source, |analysis| {
        let items = analysis.completion(0);
        let mine = items
            .iter()
            .find(|item| item.label == "mine")
            .expect("the module's own binding");
        assert_eq!(mine.kind, CompletionKind::Value);
        assert_eq!(mine.origin, Origin::Module);

        let sqrt = items
            .iter()
            .find(|item| item.label == "sqrt")
            .expect("a prelude function");
        assert_eq!(sqrt.kind, CompletionKind::Function);
        assert_eq!(sqrt.origin, Origin::Prelude);
        assert!(sqrt.detail.is_some(), "the prelude detail is the scheme");

        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        let mut sorted = labels.clone();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(before, sorted.len(), "a name is offered twice");
        // Module names sort before prelude ones.
        assert!(
            labels.iter().position(|l| *l == "mine") < labels.iter().position(|l| *l == "sqrt")
        );
    });
}

#[test]
fn completion_knows_units_dimensions_and_constructors() {
    let source =
        "dimension Currency\nunit usd : Currency = 1\ntype Shape = | Circle(Real[L]) | Dot\n";
    with(source, |analysis| {
        let items = analysis.completion(0);
        let kind = |label: &str| {
            items
                .iter()
                .find(|item| item.label == label)
                .unwrap_or_else(|| panic!("`{label}` is missing"))
                .kind
        };
        assert_eq!(kind("Currency"), CompletionKind::Dimension);
        assert_eq!(kind("usd"), CompletionKind::Unit);
        assert_eq!(kind("Circle"), CompletionKind::Constructor);
        assert_eq!(kind("Shape"), CompletionKind::Type);
    });
}

#[test]
fn a_module_name_shadows_the_prelude() {
    let source = "fn length(x: Int): Int = x\n";
    with(source, |analysis| {
        let items = analysis.completion(0);
        let shadows: Vec<_> = items.iter().filter(|item| item.label == "length").collect();
        assert_eq!(shadows.len(), 1);
        assert_eq!(shadows[0].origin, Origin::Module);
    });
}

#[test]
fn field_completion_offers_the_receivers_record_fields() {
    // `Point` is the language's affine-point type, so a record alias needs a
    // different name.
    let source = "type Sample = { x: Real[L], y: Real[L] }\nfn shift(p: Sample): Real[L] = p.x\n";
    with(source, |analysis| {
        // The position just after the dot in `p.x`.
        let after_dot = offset_of(source, "p.x", 1) + 2;
        let items = analysis.completion(after_dot);
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["x", "y"], "{labels:?}");
        assert!(
            items.iter().all(|item| item.kind == CompletionKind::Field),
            "{items:#?}"
        );
        assert_eq!(items[0].detail.as_deref(), Some("Real[L]"));
    });
}

#[test]
fn keyword_completion_appears_where_an_expression_starts() {
    let source = "fn f(): Int = \n    \n";
    with(source, |analysis| {
        let at_body = u32::try_from(source.find("    \n").expect("indent") + 4).expect("offset");
        let labels: Vec<String> = analysis
            .completion(at_body)
            .into_iter()
            .map(|item| item.label)
            .collect();
        for keyword in ["do", "let", "match", "with"] {
            assert!(
                labels.iter().any(|label| label == keyword),
                "`{keyword}` missing: {labels:?}"
            );
        }
    });
}

#[test]
fn definition_points_at_the_declaration() {
    let source = "fn double(x: Int): Int = x + x\nlet y = double(2)\n";
    with(source, |analysis| {
        let location = analysis
            .definition(offset_of(source, "double", 2))
            .expect("definition");
        assert_eq!(location.document, "test.aev");
        assert_eq!(location.range, analysis.document().range_of(3, 9));
    });
}

#[test]
fn definition_of_a_local_binding_points_at_its_parameter() {
    let source = "fn f(x: Int): Int =\n    x + 1\n";
    with(source, |analysis| {
        let location = analysis
            .definition(offset_of(source, "x + 1", 1))
            .expect("definition");
        // The parameter `x` is written at byte 5.
        assert_eq!(location.range, analysis.document().range_of(5, 6));
    });
}

#[test]
fn a_kernel_operator_has_no_definition() {
    let source = "let v = Rssn.sin(0.5)\n";
    with(source, |analysis| {
        assert_eq!(analysis.definition(offset_of(source, "Rssn.sin", 1)), None);
    });
}

#[test]
fn a_class_method_resolves_when_one_instance_declares_it() {
    let source = "class Pretty a where\n    pretty(x: a): String\n\ninstance Pretty for Float where\n    pretty(x: Float): String = \"f\"\n\nlet s = pretty(1.0)\n";
    with(source, |analysis| {
        let location = analysis
            .definition(offset_of(source, "pretty", 3))
            .expect("definition");
        assert_eq!(
            text_of(analysis.document(), source, location.range),
            "pretty"
        );
        // The instance method, on line 5, not the class declaration on line 2.
        assert_eq!(location.range.start.line, 4);
    });
}

#[test]
fn an_ambiguous_class_method_has_no_definition() {
    // Two instances declare `pretty`, so the source alone cannot say which one
    // the call reaches — the checker's dictionary use is what knows.
    let source = "class Pretty a where\n    pretty(x: a): String\n\ninstance Pretty for Float where\n    pretty(x: Float): String = \"f\"\n\ninstance Pretty for Int where\n    pretty(x: Int): String = \"i\"\n\nlet s = pretty(1.0)\n";
    with(source, |analysis| {
        assert_eq!(analysis.definition(offset_of(source, "pretty", 4)), None);
    });
}

#[test]
fn the_outline_nests_constructors_and_methods() {
    let source = "type Shape = | Circle(Real[L]) | Dot\n\nclass Show2 a where\n    show2(a)\n\ninstance Show2 for Float where\n    show2(x: Float): String = \"f\"\n\nfn area(s: Shape): Real[L] =\n    match s with\n    | Dot -> 0.0 m^2\n    | Circle(r) -> r\n";
    with(source, |analysis| {
        let symbols = analysis.symbols();
        let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(
            names,
            ["Shape", "Show2", "instance Show2 for Float", "area"]
        );

        let shape = &symbols[0];
        assert_eq!(shape.kind, SymbolKind::Enum);
        let ctors: Vec<&str> = shape.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(ctors, ["Circle", "Dot"]);

        let instance = &symbols[2];
        assert_eq!(instance.kind, SymbolKind::Object);
        assert_eq!(instance.children[0].name, "show2");
        assert_eq!(instance.children[0].kind, SymbolKind::Method);

        let area = &symbols[3];
        assert_eq!(area.kind, SymbolKind::Function);
        assert_eq!(text_of(analysis.document(), source, area.selection), "area");
    });
}

#[test]
fn the_outline_includes_a_nested_function() {
    let source = "fn outer(x: Int): Int =\n    fn inner(y: Int): Int = y\n    inner(x)\n";
    with(source, |analysis| {
        let symbols = analysis.symbols();
        assert_eq!(symbols[0].name, "outer");
        assert_eq!(symbols[0].children[0].name, "inner");
        assert_eq!(symbols[0].children[0].kind, SymbolKind::Function);
    });
}

#[test]
fn formatting_is_offered_only_when_it_changes_something() {
    let messy = Document::new("m.aev", 1, "let  x=1+2\n");
    let edit = Analysis::new(&messy).formatting().expect("an edit");
    assert_eq!(edit.new_text, "let x = 1 + 2\n");
    assert_eq!(edit.range, messy.full_range());

    let tidy = Document::new("t.aev", 1, "let x = 1 + 2\n");
    assert_eq!(Analysis::new(&tidy).formatting(), None);
}

#[test]
fn formatting_a_file_with_a_syntax_error_returns_nothing() {
    let broken = Document::new("b.aev", 1, "fn f( = 1\n");
    assert_eq!(Analysis::new(&broken).formatting(), None);
}

#[test]
fn positions_survive_an_astral_character_on_an_earlier_line() {
    // The emoji is four bytes and two UTF-16 units; the binding on the next
    // line must still be found at column 4.
    let source = "let s = \"😀\"\nlet g = 1 m\n";
    with(source, |analysis| {
        let line = analysis
            .document()
            .offset(Position::new(1, 4))
            .expect("offset");
        assert_eq!(&source[line as usize..line as usize + 1], "g");
        let hover = analysis.hover(line).expect("hover");
        assert!(hover.contents.contains("g:"), "{}", hover.contents);
        assert_eq!(
            hover.range,
            Some(Range::new(Position::new(1, 4), Position::new(1, 5)))
        );
    });
}

#[test]
fn a_store_serves_the_document_that_was_edited() {
    let mut store = DocumentStore::new();
    store.open(Document::new("a.aev", 1, "let x = 1 m + 1\n"));
    let before = Analysis::new(store.get("a.aev").expect("open"));
    assert_eq!(before.diagnostics().len(), 1);
    drop(before);

    // Fix the error with one incremental edit: the trailing `1` gains a unit.
    let document = store.get_mut("a.aev").expect("open");
    document
        .apply(
            &aevia_analysis::Change::ranged(
                Range::new(Position::new(0, 14), Position::new(0, 15)),
                "1 m",
            ),
            2,
        )
        .expect("applies");
    assert_eq!(document.text(), "let x = 1 m + 1 m\n");
    let after = Analysis::new(store.get("a.aev").expect("open"));
    assert!(after.diagnostics().is_empty(), "{:?}", after.diagnostics());
}
