use aevia_syntax::lex;
use aevia_syntax::parse_module;
use aevia_types::check::check_module;

fn errors(source: &str) -> Vec<String> {
    let tokens = lex(0, source).expect("lex");
    let (module, parse) = parse_module(0, source, &tokens);
    assert!(!parse.has_errors(), "{:?}", parse.items);
    check_module(0, &module)
        .diagnostics
        .items
        .into_iter()
        .filter(|item| item.severity == aevia_syntax::Severity::Error)
        .map(|item| item.code)
        .collect()
}

#[test]
fn concrete_functions_of_different_arities_and_aliases_are_accepted() {
    let source = "let callback = to_callback\nlet a = callback(fn (x: Int) => x + 1)\nlet b = to_callback(fn (x: Int, y: Int) => x + y)\n";
    let diagnostics = errors(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn nonfunctions_and_wrong_arity_are_rejected() {
    let diagnostics = errors("let x = to_callback(1)\n");
    assert!(diagnostics.contains(&"E0302".to_string()), "{diagnostics:?}");
    assert!(errors("let x = to_callback()\n").contains(&"E0315".to_string()));
}

#[test]
fn a_user_function_can_shadow_the_intrinsic() {
    assert!(errors("fn to_callback(x: Int) = x + 1\nlet x = to_callback(2)\n").is_empty());
}

#[test]
fn generic_definitions_defer_callback_specialization_to_the_call() {
    assert!(errors("fn make<a>(f: a -> a) = to_callback(f)\n").is_empty());
}

#[test]
fn callback_callee_expression_evidence_preserves_intrinsic_identity() {
    let source = "fn square(x: Float) = x\nlet alias = to_callback\nlet a = to_callback(square)\nlet b = alias(square)\n";
    let tokens = lex(0, source).unwrap();
    let (module, parse) = parse_module(0, source, &tokens);
    assert!(!parse.has_errors(), "{:?}", parse.items);
    let checked = check_module(0, &module);
    assert!(!checked.diagnostics.has_errors(), "{:?}", checked.diagnostics.items);
    for needle in ["to_callback(square)", "alias(square)"] {
        let start = source.find(needle).unwrap() as u32;
        let end = start + needle.find('(').unwrap() as u32;
        assert!(checked.module.expr_types.iter().any(|(span, ty)| span.start == start && span.end == end && matches!(ty, aevia_types::Type::ToCallbackIntrinsic)), "missing intrinsic callee evidence for {needle}");
    }
}
