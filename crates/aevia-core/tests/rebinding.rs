//! Runtime values must follow the same lexical rebinding rules as the checker.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::Value;
use aevia_core::value::show;

fn installed(
    source: &str,
    mode: Mode,
    optimized: bool,
    batch: bool,
) -> Interp {
    let compilation = compile("rebinding.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.as_ref().unwrap();
    let mut interp = Interp::new(&compilation.checked)
        .with_mode(mode)
        .with_buffer();
    interp.optimize_ir = optimized;
    if batch {
        interp
            .load_incremental_declarations(&compilation.checked, &[], &module.decls)
            .unwrap();
    } else {
        let diagnostics = interp.load(module);
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    }
    interp
}

fn dimension(
    interp: &mut Interp,
    name: &str,
) -> String {
    let Value::Num(number) = interp.eval_toplevel(name).unwrap() else {
        panic!("number")
    };
    number.dim.to_plain_string()
}

#[test]
fn old_values_and_closures_keep_their_dimensions_while_new_references_change() {
    let source = "let x = 1s\nlet old = fn() => x\nlet before = x\nlet x = 1km\nlet latest = fn() => x\nlet old_result = old()\nlet new_result = latest()\nlet x = \"text\"\nlet newest = x\n";
    for (mode, optimized) in [(Mode::Ast, false), (Mode::Ir, false), (Mode::Ir, true)] {
        for batch in [false, true] {
            let mut interp = installed(source, mode, optimized, batch);
            assert_eq!(dimension(&mut interp, "before"), "Time");
            assert_eq!(dimension(&mut interp, "old_result"), "Time");
            assert_eq!(dimension(&mut interp, "new_result"), "Length");
            assert_eq!(show(&interp.eval_toplevel("newest").unwrap()), "text");
        }
    }
}

#[test]
fn redefined_functions_preserve_old_aliases_and_recursive_implementations() {
    let source = "fn f(n: Int): Int = if n == 0 then 42 else f(n - 1)\nlet previous = f\nfn f(n: Int): String = \"new\"\nlet old_result = previous(3)\nlet new_result = f(3)\n";
    for (mode, optimized) in [(Mode::Ast, false), (Mode::Ir, false), (Mode::Ir, true)] {
        for batch in [false, true] {
            let mut interp = installed(source, mode, optimized, batch);
            assert_eq!(show(&interp.eval_toplevel("old_result").unwrap()), "42");
            assert_eq!(show(&interp.eval_toplevel("new_result").unwrap()), "new");
        }
    }
}

#[test]
fn aliases_of_overwritten_constrained_functions_keep_their_original_evidence() {
    let source =
        "fn f(x) = show(x)\nlet old = f\nfn f(): Int = 1\nlet result = old(42)\nlet latest = f()\n";
    for (mode, optimized) in [(Mode::Ast, false), (Mode::Ir, false), (Mode::Ir, true)] {
        for batch in [false, true] {
            let mut interp = installed(source, mode, optimized, batch);
            let result = interp.eval_toplevel("result").unwrap();
            assert!(matches!(result, Value::Str(_)));
            assert_eq!(show(&result), "42");
            assert_eq!(show(&interp.eval_toplevel("latest").unwrap()), "1");
        }
    }
}

#[test]
fn self_increment_and_forward_initializers_resolve_the_correct_version() {
    let source = "let x = 1\nlet x = x + 1\nlet incremented = x\nlet early = value\nlet value = 1s\nlet value = 1km\n";
    for (mode, optimized) in [(Mode::Ast, false), (Mode::Ir, false), (Mode::Ir, true)] {
        // The ordinary module loader supports forward value initializers;
        // incremental batches deliberately require initializer source order.
        let mut interp = installed(source, mode, optimized, false);
        assert_eq!(show(&interp.eval_toplevel("incremented").unwrap()), "2");
        assert_eq!(dimension(&mut interp, "early"), "Time");
        assert_eq!(dimension(&mut interp, "value"), "Length");
    }
}
