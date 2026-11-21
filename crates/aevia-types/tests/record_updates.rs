//! End-to-end checks for wide structural record updates.

use aevia_syntax::lex;
use aevia_syntax::parse_module;
use aevia_types::Export;
use aevia_types::Scheme;
use aevia_types::Type;
use aevia_types::check::CheckResult;
use aevia_types::check::check_module;
use std::fmt::Write;

fn filler() -> String {
    (0..20).fold(String::new(), |mut fields, i| {
        write!(fields, ", filler{i:02} = true").unwrap();
        fields
    })
}

fn check(source: &str) -> CheckResult {
    let lexed = lex(0, source).unwrap();
    let (parsed, diagnostics) = parse_module(0, source, &lexed);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    check_module(0, &parsed)
}

fn scheme(checked: &CheckResult) -> &Scheme {
    let Some(Export::Value { scheme, .. }) = checked.module.exports.get("expand") else {
        panic!("expected an exported function")
    };
    scheme
}

fn errors_at<'a>(
    checked: &'a CheckResult,
    source: &'a str,
) -> Vec<(&'a str, &'a str)> {
    checked
        .diagnostics
        .items
        .iter()
        .filter(|diagnostic| diagnostic.severity == aevia_syntax::Severity::Error)
        .map(|diagnostic| {
            let range = diagnostic.span.start as usize..diagnostic.span.end as usize;
            (diagnostic.code.as_str(), &source[range])
        })
        .collect()
}

#[test]
fn wide_update_replaces_in_place_and_retains_new_label_order() {
    let definition = format!(
        "fn expand(p: {{ a: Bool, m: Bool, z: Bool, ...r }}) = \
         {{ p with y = \"y\", z = \"z\"{}, b = true, a = \"a\" }}\n",
        filler()
    );
    let checked = check(&definition);
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics.items
    );
    let scheme = scheme(&checked);
    let Type::Fn(params, result) = &scheme.ty else {
        panic!("expected a function type")
    };
    let ([Type::Record(param)], Type::Record(result)) = (params.as_slice(), result.as_ref()) else {
        panic!("expected an open record argument and result")
    };
    assert_eq!(result.tail_var(), param.tail_var());
    assert!(result.tail_var().is_some());
    let mut expected = vec![
        ("a".to_string(), Type::Str),
        ("m".to_string(), Type::Bool),
        ("z".to_string(), Type::Str),
        ("y".to_string(), Type::Str),
    ];
    expected.extend((0..20).map(|i| (format!("filler{i:02}"), Type::Bool)));
    expected.push(("b".to_string(), Type::Bool));
    assert_eq!(result.fields(), expected);

    let valid = check(&format!(
        "{definition}let keep = expand({{ a = true, m = true, z = true, extra = false }})\n"
    ));
    assert!(
        !valid.diagnostics.has_errors(),
        "{:?}",
        valid.diagnostics.items
    );

    let invalid = check(&format!(
        "{definition}let clash = expand({{ a = true, m = true, z = true, b = false }})\n"
    ));
    assert!(
        invalid
            .diagnostics
            .items
            .iter()
            .any(|diagnostic| { diagnostic.code == "E0208" && diagnostic.message.contains("`b`") }),
        "{:?}",
        invalid.diagnostics.items
    );
}

#[test]
fn each_rhs_is_checked_before_its_open_tail_constraint() {
    let source = format!(
        "fn expand(p: {{ known: Bool, ...r }}) = \
         {{ p with inferred = p.inferred{}, unknown = absent_name }}\n",
        filler()
    );
    let checked = check(&source);
    assert_eq!(
        errors_at(&checked, &source),
        [("E0208", "p.inferred"), ("E0301", "absent_name")]
    );
}

#[test]
fn recovery_reports_a_duplicate_before_rhs_errors() {
    let source = format!(
        "fn expand(p: {{ known: Bool, ...r }}) = \
         {{ p with duplicate = absent_first{}, duplicate = absent_second }}\n",
        filler()
    );
    let lexed = lex(0, &source).unwrap();
    let (parsed, diagnostics) = parse_module(0, &source, &lexed);
    assert!(diagnostics.items.iter().any(|item| item.code == "E0208"));
    let checked = check_module(0, &parsed);
    assert_eq!(
        errors_at(&checked, &source),
        [
            ("E0208", "absent_second"),
            ("E0301", "absent_first"),
            ("E0301", "absent_second"),
        ]
    );
}
