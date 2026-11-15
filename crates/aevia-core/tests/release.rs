//! The release optimizer must change IR while preserving observable behavior.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;

fn interpreter(
    source: &str,
    optimized: bool,
) -> Interp {
    let compilation = compile("release.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let mut interp = Interp::new(&compilation.checked)
        .with_mode(Mode::Ir)
        .with_buffer();
    interp.optimize_ir = optimized;
    let loaded = interp.load(compilation.module.as_ref().unwrap());
    assert!(!loaded.has_errors(), "{:?}", loaded.items);
    interp
}

#[test]
fn release_preserves_numeric_representations_closures_and_effect_order() {
    let source = r#"
fn exact(x: Real): Real = if true then x + 0.1 else x / 0
fn floating(x: Float): Float = if false then x / 0 else x + 0.1
fn factory(x: Int) = fn(y: Int) => if true then x + y else 0
let closure = factory(10)
fn main(): IO Unit = do
    println("first")
    println("{exact(0.2)} {floating(0.2)} {closure(32)}")
    println("last")
"#;
    let mut debug = interpreter(source, false);
    let mut release = interpreter(source, true);
    debug.run_main().unwrap();
    release.run_main().unwrap();
    assert_eq!(release.captured(), debug.captured());
    assert!(release.captured().starts_with("first\n"));
    assert!(release.captured().ends_with("42\nlast\n"));
}

#[test]
fn release_retains_error_locations_and_execution_budgets() {
    for source in [
        "fn explode(n: Int): Real = if true then 10 / n else 0\nfn main() = explode(0)\n",
        "fn loop(n: Int): Int = if true then loop(n + 1) else n\nfn main() = loop(0)\n",
    ] {
        let mut debug = interpreter(source, false);
        let mut release = interpreter(source, true);
        debug.max_tail_calls = 20;
        release.max_tail_calls = 20;
        let debug_error = debug.run_main().unwrap_err();
        let release_error = release.run_main().unwrap_err();
        assert_eq!(release_error.code, debug_error.code);
        assert_eq!(release_error.span, debug_error.span);
    }
}
