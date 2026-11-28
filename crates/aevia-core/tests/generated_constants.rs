//! The generated physical constants preserve symbolic scale factors.
use aevia_core::Interp;
use aevia_core::compile;
use aevia_core::value::NumKind;
use aevia_core::value::Value;

#[test]
fn generated_hbar_keeps_exact_pi_and_approximates_correctly() {
    let mut source = aevia_rssn::codegen::constants(aevia_rssn::registry());
    source.push_str("\nlet hbar_approx = approx(hbar)\n");
    let compilation = compile("Constants.aev", &source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let mut interpreter = Interp::new(&compilation.checked);
    let diagnostics = interpreter.load(&compilation.module.expect("module"));
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    let exact = interpreter.eval_toplevel("hbar").expect("hbar");
    assert!(
        matches!(exact, Value::Num(ref n) if matches!(n.kind, NumKind::Sym(_))),
        "hbar must retain symbolic pi: {exact:?}"
    );
    let approximate = interpreter
        .eval_toplevel("hbar_approx")
        .expect("approximation");
    let Value::Variant(result) = approximate else {
        panic!("expected Result: {approximate:?}");
    };
    assert_eq!(result.index, 0, "approximation failed: {result:?}");
    let Some(Value::Num(n)) = result.fields.first() else {
        panic!("expected numerical result: {result:?}");
    };
    let NumKind::Float(value) = n.kind else {
        panic!("expected float result: {n:?}");
    };
    let expected = 6.62607015e-34 / (2.0 * std::f64::consts::PI);
    assert!(
        (value / expected - 1.0).abs() < 1e-12,
        "{value} != {expected}"
    );
}
