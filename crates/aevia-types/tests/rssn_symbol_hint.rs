//! A RSSN binder mismatch points to a checked symbolic call.

use aevia_syntax::Diagnostic;
use aevia_types::check::check_module;

fn no_match(call: &str) -> (String, Diagnostic) {
    let source = format!("let x: Real = symbol(\"x\")\nlet result = {call}\n");
    let tokens = aevia_syntax::lex(0, &source).unwrap();
    let (module, diagnostics) = aevia_syntax::parse_module(0, &source, &tokens);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    let checked = check_module(0, &module);
    let error = checked
        .diagnostics
        .items
        .into_iter()
        .find(|diagnostic| diagnostic.code == "E1003")
        .unwrap();
    (source, error)
}

#[test]
fn the_no_match_diagnostic_labels_the_free_symbol_slot() {
    let (source, error) = no_match("Rssn.defint(x, x, 0, 1)");
    assert_eq!(error.labels.len(), 1);
    let begin = source.find("defint(x, x,").unwrap() + "defint(x, ".len();
    let label = &error.labels[0];
    assert_eq!(usize::try_from(label.span.start).unwrap(), begin);
    assert_eq!(usize::try_from(label.span.end).unwrap(), begin + 1);
    assert!(label.message.contains("free symbol"));
    assert!(
        error
            .helps
            .iter()
            .any(|help| help.contains("Rssn.typed") && help.contains("to_value"))
    );
}

#[test]
fn arity_and_known_float_mismatches_keep_the_general_help() {
    for call in ["Rssn.defint(x, x, 0)", "Rssn.defint(x, 3.0, 0, 1)"] {
        let (_, error) = no_match(call);
        assert!(error.labels.is_empty(), "{call}: {error:?}");
        assert!(error.helps.iter().any(|help| help.contains("Rssn.dynamic")));
    }
}
