//! Lexical type parameters across nested alias and instance annotations.

use aevia_types::CheckResult;
use aevia_types::Export;
use aevia_types::Type;
use aevia_types::check::check_module;
use aevia_types::class::matches_head;

fn check(source: &str) -> CheckResult {
    let tokens = aevia_syntax::lex(0, source).unwrap();
    let (module, diagnostics) = aevia_syntax::parse_module(0, source, &tokens);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    check_module(0, &module)
}

#[test]
fn alias_arguments_can_reenter_while_outer_parameters_shadow_a_named_type() {
    let checked = check(
        r#"type Item = Int
type Wrap<a> = { payload: a }
type Pair<a, b> = { first: a, second: b }
fn reverse<Item, Other>(x: Pair<Wrap<Item>, Other>): Pair<Other, Item> =
    { first = x.second, second = x.first.payload }
let result: Pair<Bool, String> = reverse({ first = { payload = "text" }, second = true })
let outside: Item = 7
"#,
    );
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics.items
    );
    let export = |name| {
        let Export::Value { scheme, .. } = &checked.module.exports[name] else {
            panic!("expected a value export");
        };
        &scheme.ty
    };
    assert_eq!(
        export("result"),
        &Type::record(vec![
            ("first".into(), Type::Bool),
            ("second".into(), Type::Str),
        ])
    );
    assert_eq!(export("outside"), &Type::int());
}

#[test]
fn nested_alias_arguments_and_outer_instance_fields_share_implicit_variables() {
    let checked = check(
        r#"type Wrap<a> = { payload: a }
type Pair<a, b> = { first: a, second: b }
class Same a where
    same(x: a): String
instance Same for { nested: Pair<Wrap<t>, t>, last: t } where
    same(x) = "same"
let result = same({ nested = { first = { payload = 1 }, second = 2 }, last = 3 })
"#,
    );
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics.items
    );
    let argument = |second, last| {
        Type::record(vec![
            (
                "nested".into(),
                Type::record(vec![
                    (
                        "first".into(),
                        Type::record(vec![("payload".into(), Type::int())]),
                    ),
                    ("second".into(), second),
                ]),
            ),
            ("last".into(), last),
        ])
    };
    let head = &checked.module.instances[0].head;
    assert!(matches_head(head, &argument(Type::int(), Type::int())));
    assert!(!matches_head(head, &argument(Type::Str, Type::int())));
    assert!(!matches_head(head, &argument(Type::int(), Type::Str)));
}

#[test]
fn unknown_alias_arguments_keep_diagnostic_order_and_byte_spans() {
    let source = r#"type Pair<a, b> = { first: a, second: b }
fn broken<a>(x: Pair<{ value: ζ }, Missing>, y: a): a = y
"#;
    let checked = check(source);
    let unknown: Vec<_> = checked
        .diagnostics
        .items
        .iter()
        .filter(|item| item.code == "E0301")
        .map(|item| {
            (
                item.message.as_str(),
                &source[item.span.start as usize..item.span.end as usize],
            )
        })
        .collect();
    assert_eq!(
        unknown,
        [
            ("unknown type variable `ζ`", "ζ"),
            ("unknown type `Missing`", "Missing"),
        ]
    );
}
