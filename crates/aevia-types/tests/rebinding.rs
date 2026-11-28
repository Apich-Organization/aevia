//! Top-level declarations shadow earlier values without equating their types.

use aevia_syntax::SourceMap;
use aevia_syntax::lex;
use aevia_syntax::parse_module;
use aevia_types::CheckedModule;
use aevia_types::Export;
use aevia_types::Type;
use aevia_types::check::check_module;

fn checked(source: &str) -> CheckedModule {
    let mut sources = SourceMap::new();
    let file = sources.add("rebinding.aev", source);
    let lexed = lex(file, source).unwrap();
    let (module, diagnostics) = parse_module(file, source, &lexed);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    let result = check_module(file, &module);
    assert!(
        !result.diagnostics.has_errors(),
        "{}",
        result.diagnostics.render(&sources)
    );
    result.module
}

fn ty(
    module: &CheckedModule,
    name: &str,
) -> Type {
    let Export::Value { scheme, .. } = &module.exports[name] else {
        panic!("value")
    };
    scheme.ty.clone()
}

#[test]
fn dimensions_can_change_and_earlier_values_keep_their_types() {
    let module = checked("let a = 1km\nlet b = 1s\nlet before = b\nlet b = 1km\nlet c = a + b\n");
    let Type::Scalar(_, before) = ty(&module, "before") else {
        panic!("scalar")
    };
    let Type::Scalar(_, latest) = ty(&module, "b") else {
        panic!("scalar")
    };
    let Type::Scalar(_, result) = ty(&module, "c") else {
        panic!("scalar")
    };
    assert_eq!(before.to_plain_string(), "Time");
    assert_eq!(latest.to_plain_string(), "Length");
    assert_eq!(result, latest);
}

#[test]
fn rebinding_changes_value_kinds_and_initializers_can_use_previous_values() {
    let module = checked(
        "let x = 1\nlet original = x\nlet x = x + 1\nlet incremented = x\nlet x = \"text\"\nlet latest = x\n",
    );
    assert!(matches!(ty(&module, "original"), Type::Scalar(..)));
    assert!(matches!(ty(&module, "incremented"), Type::Scalar(..)));
    assert_eq!(ty(&module, "x"), Type::Str);
    assert_eq!(ty(&module, "latest"), Type::Str);
}

#[test]
fn old_closures_and_function_references_keep_their_original_signatures() {
    let module = checked(
        "let x = 1s\nlet old = fn() => x\nlet x = 1km\nlet new = fn() => x\nfn f(): Int = 1\nlet earlier = f\nfn f(): String = \"new\"\nlet old_result = earlier()\nlet new_result = f()\n",
    );
    let Type::Fn(_, old) = ty(&module, "old") else {
        panic!("function")
    };
    let Type::Fn(_, new) = ty(&module, "new") else {
        panic!("function")
    };
    let Type::Scalar(_, old_dim) = *old else {
        panic!("scalar")
    };
    let Type::Scalar(_, new_dim) = *new else {
        panic!("scalar")
    };
    assert_eq!(old_dim.to_plain_string(), "Time");
    assert_eq!(new_dim.to_plain_string(), "Length");
    assert!(matches!(ty(&module, "old_result"), Type::Scalar(..)));
    assert_eq!(ty(&module, "new_result"), Type::Str);
}

#[test]
fn forward_references_still_use_the_first_declaration() {
    let module = checked(
        "let result = helper()\nfn helper(): Int = 42\nlet early = value\nlet value = 1s\nlet value = 1km\n",
    );
    assert!(matches!(ty(&module, "result"), Type::Scalar(..)));
    let Type::Scalar(_, dim) = ty(&module, "early") else {
        panic!("scalar")
    };
    assert_eq!(dim.to_plain_string(), "Time");
}

#[test]
fn generic_function_aliases_and_recursive_rebindings_use_the_correct_binders() {
    let module = checked(
        "fn ident<a>(x: a): a = x\nlet old = ident\nfn ident(x: String): String = x\nlet number = old(42)\nlet text = ident(\"text\")\nlet f = fn(n: Int) => n\nlet rec f = fn(n: Int) => if n == 0 then 0 else f(n - 1)\nlet result = f(3)\n",
    );
    assert!(matches!(ty(&module, "number"), Type::Scalar(..)));
    assert_eq!(ty(&module, "text"), Type::Str);
    assert!(matches!(ty(&module, "result"), Type::Scalar(..)));
}

#[test]
fn a_destructured_binding_is_visible_to_its_later_rebinding() {
    let module = checked("let (a, b) = (1, 2)\nlet b = b + a\nlet result = b\n");
    assert!(matches!(ty(&module, "result"), Type::Scalar(..)));
}

#[test]
fn the_latest_binding_controls_interface_visibility_and_cross_kind_metadata() {
    let module = checked(
        "fn f<a>(x: a): a = x\nlet old = f\nprivate let f = \"private\"\nlet first = old(1)\nprivate let hidden = 1s\nlet hidden = 1km\nlet cross = 1s\nfn cross(): String = \"function\"\nlet called = cross()\nlet cross = true\n",
    );
    assert!(!module.exports.contains_key("f"));
    assert!(!module.function_headers.contains_key("f"));
    assert!(!module.callable_exports.contains_key("f"));
    assert!(matches!(ty(&module, "first"), Type::Scalar(..)));
    let Type::Scalar(_, dim) = ty(&module, "hidden") else {
        panic!("scalar")
    };
    assert_eq!(dim.to_plain_string(), "Length");
    assert_eq!(ty(&module, "called"), Type::Str);
    assert_eq!(ty(&module, "cross"), Type::Bool);
    assert!(!module.function_headers.contains_key("cross"));
    assert!(!module.callable_exports.contains_key("cross"));
}

#[test]
fn overwritten_generic_bodies_and_aliases_retain_their_class_requirements() {
    checked("fn f(x) = show(x)\nfn f(): Int = 1\n");
    let module = checked("fn f(x) = show(x)\nlet old = f\nfn f(): Int = 1\nlet result = old(42)\n");
    assert_eq!(ty(&module, "result"), Type::Str);
    let Export::Value { scheme, .. } = &module.exports["old"] else {
        panic!("value")
    };
    assert!(
        scheme
            .constraints
            .iter()
            .any(|constraint| constraint.class == "Show")
    );
    let original = module
        .functions
        .values()
        .find(|function| {
            function
                .scheme
                .constraints
                .iter()
                .any(|constraint| constraint.class == "Show")
        })
        .expect("original function evidence retains Show");
    assert!(matches!(original.scheme.ty, Type::Fn(..)));
}

#[test]
fn source_operator_and_rule_dimensions_belong_to_their_attributed_declarations() {
    let module = checked(
        "@op(\"rebinding/area\")\nfn area(x: Real[L]): Real[L^2] = x * x\nfn area(x: String): String = x\n@rule(\"rebinding/clock\")\nfn clock(x: Real[T]): Real[T] = x\nfn clock(x: Real[L]): Real[L] = x\n",
    );
    assert_eq!(module.registrations.len(), 2);
    let area = module
        .registrations
        .iter()
        .find(|registration| registration.name == "rebinding/area")
        .unwrap();
    assert_eq!(area.params[0].1.to_plain_string(), "Length");
    assert_eq!(area.result.to_plain_string(), "Length^2");
    let clock = module
        .registrations
        .iter()
        .find(|registration| registration.name == "rebinding/clock")
        .unwrap();
    assert_eq!(clock.params[0].1.to_plain_string(), "Time");
    assert_eq!(clock.result.to_plain_string(), "Time");
}

#[test]
fn imported_values_are_visible_only_in_the_first_shadowing_initializer() {
    let producer = checked("fn twice(n: Int): Int = n * 2\nfn ident<a>(x: a): a = x\n");
    let source = "let before = twice\nprivate let twice = twice(2)\nlet ident = ident\nlet number = ident(42)\nlet text = ident(\"text\")\n";
    let tokens = lex(1, source).unwrap();
    let (module, diagnostics) = parse_module(1, source, &tokens);
    assert!(!diagnostics.has_errors());
    let result = aevia_types::check::check_module_with_interfaces(
        1,
        &module,
        &[(String::new(), producer.exports.clone())],
        &[producer.interface()],
        &Default::default(),
    );
    assert!(
        !result.diagnostics.has_errors(),
        "{:?}",
        result.diagnostics.items
    );
    assert!(matches!(ty(&result.module, "before"), Type::Scalar(..)));
    assert!(!result.module.exports.contains_key("twice"));
    assert!(!result.module.function_headers.contains_key("twice"));
    assert!(!result.module.callable_exports.contains_key("twice"));
    assert!(matches!(ty(&result.module, "number"), Type::Scalar(..)));
    assert_eq!(ty(&result.module, "text"), Type::Str);
    assert!(result.module.function_headers.contains_key("ident"));
    assert!(result.module.callable_exports.contains_key("ident"));
}

#[test]
fn imported_source_functions_shadow_builtin_call_typing() {
    let producer = checked("fn sqrt(text: String): String = text\n");
    let source = "let result = sqrt(\"text\")\n";
    let tokens = lex(1, source).unwrap();
    let (module, diagnostics) = parse_module(1, source, &tokens);
    assert!(!diagnostics.has_errors());
    let result = aevia_types::check::check_module_with_interfaces(
        1,
        &module,
        &[(String::new(), producer.exports.clone())],
        &[producer.interface()],
        &Default::default(),
    );
    assert!(
        !result.diagnostics.has_errors(),
        "{:?}",
        result.diagnostics.items
    );
    assert_eq!(ty(&result.module, "result"), Type::Str);
}

#[test]
fn authored_generic_functions_named_like_type_directed_builtins_use_their_binders() {
    let producer =
        checked("fn check<a>(value: a): a = value\nfn reshape<a>(value: a): a = value\n");
    let source = "let number = check<Int>(42)\nlet text = reshape<String>(\"shape\")\n";
    let tokens = lex(1, source).unwrap();
    let (module, diagnostics) = parse_module(1, source, &tokens);
    assert!(!diagnostics.has_errors());
    let result = aevia_types::check::check_module_with_interfaces(
        1,
        &module,
        &[(String::new(), producer.exports.clone())],
        &[producer.interface()],
        &Default::default(),
    );
    assert!(
        !result.diagnostics.has_errors(),
        "{:?}",
        result.diagnostics.items
    );
    assert!(matches!(ty(&result.module, "number"), Type::Scalar(..)));
    assert_eq!(ty(&result.module, "text"), Type::Str);
}
