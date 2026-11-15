//! A pasted module shares one scope and prebinds its callable declarations.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::show;
use aevia_syntax::Span;

fn batch(
    interp: &mut Interp,
    source: &str,
) -> Result<(), (usize, aevia_core::EvalError)> {
    let compilation = compile("batch.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    interp.load_incremental_declarations(
        &compilation.checked,
        &[],
        &compilation.module.as_ref().unwrap().decls,
    )
}

fn modes() -> impl Iterator<Item = Interp> {
    [(Mode::Ast, false), (Mode::Ir, false), (Mode::Ir, true)]
        .into_iter()
        .map(|(mode, optimized)| {
            let mut interp = Interp::new(&Default::default()).with_mode(mode);
            interp.optimize_ir = optimized;
            interp
        })
}

fn call(
    interp: &mut Interp,
    name: &str,
) -> String {
    let function = interp.eval_toplevel(name).unwrap();
    show(&interp.apply(function, Vec::new(), Span::default()).unwrap())
}

#[test]
fn initializers_can_call_later_functions_and_mutually_recursive_functions() {
    for mut interp in modes() {
        batch(
            &mut interp,
            "let result = helper()\nfn helper(): Int = 42\n",
        )
        .unwrap();
        assert_eq!(show(&interp.eval_toplevel("result").unwrap()), "42");
        batch(&mut interp, "let parity = even(12)\nfn even(n: Int): Bool = if n == 0 then true else odd(n - 1)\nfn odd(n: Int): Bool = if n == 0 then false else even(n - 1)\n").unwrap();
        assert_eq!(show(&interp.eval_toplevel("parity").unwrap()), "true");
    }
}

#[test]
fn functions_observe_later_values_in_their_batch_and_keep_them_across_batches() {
    for mut interp in modes() {
        batch(&mut interp, "fn snapshot(): Int = value\nlet value = 1\n").unwrap();
        assert_eq!(call(&mut interp, "snapshot"), "1");
        batch(&mut interp, "let value = 2\nfn latest(): Int = value\n").unwrap();
        assert_eq!(call(&mut interp, "snapshot"), "1");
        assert_eq!(call(&mut interp, "latest"), "2");
    }
}

#[test]
fn failed_batches_restore_all_prior_bindings_and_symbol_state() {
    for mut interp in modes() {
        batch(
            &mut interp,
            "fn helper(): Int = 7\nlet original = symbol(\"x\")\n",
        )
        .unwrap();
        let counts = interp.symbol_counts.clone();
        let error = batch(&mut interp, "fn fail(n: Int): Real = 1 / n\nlet first = symbol(\"x\")\nlet bad = fail(0)\nlet later = 9\nfn helper(): Int = 42\n").unwrap_err();
        assert_eq!(error.0, 2);
        assert_eq!(error.1.code, "E0801");
        assert!(interp.eval_toplevel("first").is_err());
        assert!(interp.eval_toplevel("fail").is_err());
        assert!(interp.eval_toplevel("later").is_err());
        assert_eq!(interp.symbol_counts, counts);
        assert_eq!(call(&mut interp, "helper"), "7");
        batch(
            &mut interp,
            "fn verify(): Int = helper()\nfn helper(): Int = 7\n",
        )
        .unwrap();
        assert_eq!(call(&mut interp, "verify"), "7");
    }
}
