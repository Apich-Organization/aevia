//! Source diagnostics keep the byte range of a Unicode misspelling.

use aevia_syntax::Span;
use aevia_syntax::lex;
use aevia_syntax::parse_module;
use aevia_types::check::check_module;

#[test]
fn a_unicode_suggestion_keeps_the_original_source_span() {
    let file = 19;
    let source = "let δelta = 1\nlet café = δetla\n";
    let lexed = lex(file, source).expect("valid tokens");
    let (module, parsed) = parse_module(file, source, &lexed);
    assert!(!parsed.has_errors(), "{parsed:?}");
    let checked = check_module(file, &module);
    let error = checked
        .diagnostics
        .items
        .iter()
        .find(|diagnostic| diagnostic.code == "E0301")
        .expect("an unbound-name diagnostic");
    let name = "δetla";
    let start = source.find(name).expect("spelling present in source");
    assert_eq!(error.span, Span::new(file, start, start + name.len()));
    assert_eq!(error.helps, ["did you mean `δelta`?"]);
}
