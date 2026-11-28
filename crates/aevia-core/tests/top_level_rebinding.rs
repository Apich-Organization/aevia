//! Rebinding keeps lexical source meaning while exposing the newest public value.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::Value;
use aevia_core::value::show;

fn interpreters(
    source: &str,
    incremental: bool,
) -> impl Iterator<Item = Interp> {
    let compilation = compile("rebinding.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    [(Mode::Ast, false), (Mode::Ir, false), (Mode::Ir, true)]
        .into_iter()
        .map(move |(mode, optimized)| {
            let mut interp = Interp::new(&compilation.checked)
                .with_mode(mode)
                .with_buffer();
            interp.optimize_ir = optimized;
            let module = compilation.module.as_ref().unwrap();
            if incremental {
                interp
                    .load_incremental_declarations(&compilation.checked, &[], &module.decls)
                    .unwrap();
            } else {
                let diagnostics = interp.load(module);
                assert!(
                    !diagnostics.has_errors(),
                    "{mode:?} optimized={optimized}: {:?}",
                    diagnostics.items
                );
            }
            interp
        })
}

fn dimension(
    interp: &mut Interp,
    name: &str,
) -> String {
    let Value::Num(value) = interp.eval_toplevel(name).unwrap() else {
        panic!("{name} is not numeric");
    };
    value.dim.to_plain_string()
}

#[test]
fn closures_keep_dimensions_and_values_from_their_definition_site() {
    let source =
        "let x = 1 s\nlet old = fn () => x\nlet x = 1 km\nlet retained = old()\nlet latest = x\n";
    for incremental in [false, true] {
        for mut interp in interpreters(source, incremental) {
            assert_eq!(dimension(&mut interp, "retained"), "Time");
            assert_eq!(dimension(&mut interp, "latest"), "Length");
            assert_eq!(dimension(&mut interp, "x"), "Length");
        }
    }
}

#[test]
fn ordinary_rebinding_initializers_use_the_previous_value() {
    let source =
        "let x = 1\nlet old = fn () => x\nlet x = x + 1\nlet x = x + 1\nlet earlier = old()\n";
    for incremental in [false, true] {
        for mut interp in interpreters(source, incremental) {
            assert_eq!(show(&interp.eval_toplevel("x").unwrap()), "3");
            assert_eq!(show(&interp.eval_toplevel("earlier").unwrap()), "1");
        }
    }
}

#[test]
fn a_rebinding_can_change_the_type_without_changing_old_closures() {
    let source =
        "let value = 41\nlet old = fn () => value\nlet value = \"new\"\nlet result = old() + 1\n";
    for incremental in [false, true] {
        for mut interp in interpreters(source, incremental) {
            assert_eq!(show(&interp.eval_toplevel("value").unwrap()), "new");
            assert_eq!(show(&interp.eval_toplevel("result").unwrap()), "42");
        }
    }
}

#[test]
fn forward_references_choose_the_first_definition_and_latest_exports_use_source_order() {
    // The first value initializer completes after the second one. The exported
    // name still denotes the second value, while early keeps the first value.
    let source = "let early = value\nlet value = later\nlet value = 2 km\nlet later = 1 s\nlet latest = value\n";
    for incremental in [false, true] {
        for mut interp in interpreters(source, incremental) {
            assert_eq!(dimension(&mut interp, "early"), "Time");
            assert_eq!(dimension(&mut interp, "latest"), "Length");
            assert_eq!(dimension(&mut interp, "value"), "Length");
        }
    }
}

#[test]
fn repeated_functions_keep_their_own_recursion_and_alias_types() {
    let source = "fn f(n: Int): Int = if n == 0 then 1 else f(n - 1)\nlet old = f\nfn f(text: String): String = text\nlet result = old(3)\nlet latest = f(\"new\")\n";
    for incremental in [false, true] {
        for mut interp in interpreters(source, incremental) {
            assert_eq!(show(&interp.eval_toplevel("result").unwrap()), "1");
            assert_eq!(show(&interp.eval_toplevel("latest").unwrap()), "new");
        }
    }
}

#[test]
fn mixed_function_and_let_bindings_preserve_aliases_and_source_order() {
    let source =
        "fn value(): Int = 41\nlet old = value\nlet value = \"new\"\nlet result = old() + 1\n";
    for incremental in [false, true] {
        for mut interp in interpreters(source, incremental) {
            assert_eq!(show(&interp.eval_toplevel("value").unwrap()), "new");
            assert_eq!(show(&interp.eval_toplevel("result").unwrap()), "42");
        }
    }
}

#[test]
fn destructuring_rebindings_preserve_previous_closures() {
    let source = "let (x, y) = (1 s, 2)\nlet old = fn () => x\nlet (x, y) = (1 km, \"new\")\nlet retained = old()\n";
    for incremental in [false, true] {
        for mut interp in interpreters(source, incremental) {
            assert_eq!(dimension(&mut interp, "retained"), "Time");
            assert_eq!(dimension(&mut interp, "x"), "Length");
            assert_eq!(show(&interp.eval_toplevel("y").unwrap()), "new");
        }
    }
}

#[test]
fn deferred_incremental_initializers_do_not_allocate_symbols_twice() {
    let source = "let early = (symbol(\"x\"), value)\nlet value = 1\nlet later = symbol(\"x\")\n";
    for interp in interpreters(source, true) {
        assert_eq!(interp.symbol_counts.get("x"), Some(&2));
    }
}

#[test]
fn local_parameters_and_bindings_shadow_module_revision_history() {
    let source = "let x = 1 s\nfn local(x: Int): Int = let x = x + 1 in x\nlet x = 1 km\nlet result = local(41)\n";
    for incremental in [false, true] {
        for mut interp in interpreters(source, incremental) {
            assert_eq!(show(&interp.eval_toplevel("result").unwrap()), "42");
            assert_eq!(dimension(&mut interp, "x"), "Length");
        }
    }
}

#[test]
fn prebound_function_after_a_let_keeps_both_source_bindings() {
    let source = "let value = \"old\"\nlet old = fn () => value\nfn value(): Int = 42\nlet latest = value()\nlet retained = old()\n";
    for incremental in [false, true] {
        for mut interp in interpreters(source, incremental) {
            assert_eq!(show(&interp.eval_toplevel("latest").unwrap()), "42");
            assert_eq!(show(&interp.eval_toplevel("retained").unwrap()), "old");
        }
    }
}

#[test]
fn first_initializer_aliases_and_wrappers_resolve_the_prelude() {
    let source = "let sin = fn (x: Float) => sin(x)\nlet old = sin\nfn sin(x: Float): Float = 42.0\nlet first = old(0.0)\nlet latest = sin(0.0)\nlet sqrt = sqrt\nlet root = sqrt(16.0)\nlet pi = pi\nlet circle = pi\n";
    for incremental in [false, true] {
        for mut interp in interpreters(source, incremental) {
            assert_eq!(show(&interp.eval_toplevel("first").unwrap()), "0.0");
            assert_eq!(show(&interp.eval_toplevel("latest").unwrap()), "42.0");
            assert_eq!(show(&interp.eval_toplevel("root").unwrap()), "4.0");
            assert_eq!(show(&interp.eval_toplevel("circle").unwrap()), "pi");
        }
    }
}

#[test]
fn forward_references_outside_a_first_initializer_still_use_the_source_binding() {
    let source = "let early = sin(0.0)\nlet sin = fn (x: Float) => 42.0\n";
    for incremental in [false, true] {
        for mut interp in interpreters(source, incremental) {
            assert_eq!(show(&interp.eval_toplevel("early").unwrap()), "42.0");
        }
    }
}
