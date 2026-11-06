//! A checked program owns its exact constructor witnesses, even after another
//! check of the same named module changes its constructors.
use aevia_core::value::Value;
use aevia_core::{Interp, Mode, compile};

#[test]
fn rechecking_a_module_does_not_replace_an_earlier_programs_constructor_witnesses() {
    let first = compile(
        "rechecked.aev",
        "module Rechecked\ntype Box = | Wrap(Int)\nlet value = Wrap(1)\n",
    );
    let second = compile(
        "rechecked.aev",
        "module Rechecked\ntype Box = | Wrap(Int, Int)\nlet value = Wrap(1, 2)\n",
    );
    for (compilation, fields) in [(&second, 2), (&first, 1)] {
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let &(adt, _) = compilation.checked.constructors.get("Wrap").unwrap();
        for mode in [Mode::Ast, Mode::Ir] {
            let mut interp = Interp::new(&compilation.checked).with_mode(mode);
            let diagnostics = interp.load(compilation.module.as_ref().unwrap());
            assert!(
                !diagnostics.has_errors(),
                "{mode:?}: {:?}",
                diagnostics.items
            );
            let Value::Variant(value) = interp.eval_toplevel("value").unwrap() else {
                panic!("variant")
            };
            assert_eq!(value.adt, adt, "the exact witness of this check");
            assert_eq!(value.fields.len(), fields);
        }
    }
}

#[test]
fn private_type_constructors_are_local_runtime_bindings_and_are_never_exported() {
    let compilation = compile(
        "private.aev",
        "module Private\nprivate type Hidden = | Secret(Int)\nlet answer = match Secret(7) with\n    | Secret(x) -> x\n",
    );
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    assert!(compilation.checked.constructors.contains_key("Secret"));
    assert!(!compilation.checked.exports.contains_key("Hidden"));
    assert!(!compilation.checked.exports.contains_key("Secret"));
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        assert_eq!(
            aevia_core::value::repr(&interp.eval_toplevel("answer").unwrap()),
            "7"
        );
    }
}

#[test]
fn abi_value_type_is_shared_without_local_abstract_redeclarations() {
    let compilation = compile("abi-types.aev", "fn identity(x: Symbol): Symbol = x\nlet values: Tensor<1, Value> = [to_value(1), to_value(2)]\n");
    assert!(!compilation.diagnostics.has_errors(), "{:?}", compilation.diagnostics.items);
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(!diagnostics.has_errors(), "{mode:?}: {:?}", diagnostics.items);
        let Value::Tensor(values) = interp.eval_toplevel("values").unwrap() else { panic!("value list") };
        assert!(matches!(values.storage, aevia_core::value::Storage::Boxed(_)));
        assert_eq!(values.shape, vec![2]);
    }
}
