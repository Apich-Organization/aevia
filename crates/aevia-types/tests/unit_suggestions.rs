//! "Did you mean" for a misspelt unit prefers the smallest edit.

use aevia_syntax::lex;
use aevia_syntax::parse_module;
use aevia_types::check::check_module;

/// The help lines of every diagnostic `source` produces.
fn helps_of(source: &str) -> Vec<String> {
    let lexed = lex(0, source).expect("lexes");
    let (module, parsed) = parse_module(0, source, &lexed);
    assert!(!parsed.has_errors());
    check_module(0, &module)
        .diagnostics
        .items
        .iter()
        .flat_map(|diagnostic| diagnostic.helps.clone())
        .collect()
}

fn suggestion(unit: &str) -> Option<String> {
    helps_of(&format!("let x = 3 {unit}\n"))
        .into_iter()
        .find(|help| help.starts_with("did you mean"))
}

#[test]
fn kmm_is_km_with_a_stray_letter_not_klm() {
    assert_eq!(suggestion("kmm").as_deref(), Some("did you mean `km`?"));
}

#[test]
fn other_doubled_letters_lose_the_extra_one() {
    assert_eq!(suggestion("mss").as_deref(), Some("did you mean `ms`?"));
    assert_eq!(suggestion("kgg").as_deref(), Some("did you mean `kg`?"));
}

#[test]
fn a_far_misspelling_gets_no_suggestion() {
    assert_eq!(suggestion("zzzzz"), None);
}
