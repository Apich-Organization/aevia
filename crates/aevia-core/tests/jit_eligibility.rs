//! Activation eligibility for scalar JIT dispatch.

#![cfg(feature = "jit")]

use aevia_core::CancellationToken;
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::eval_ir::module_closure;
use aevia_core::value::Scope;
use aevia_core::value::Value;
use aevia_syntax::Span;
use aevia_types::CheckedModule;

#[test]
fn host_ir_closure_without_checker_evidence_uses_the_interpreter() {
    let source = r#"@jit
fn evaluate(x: Float): Float =
    if x == 0.0 then x else x + 1.25
"#;
    let compilation = compile("jit-without-proof.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let lowered = aevia_ir::lower::lower_module(compilation.module.as_ref().unwrap(), &[], &[]);
    let function = &lowered.fns[0].function;
    assert!(function.jit);
    // The empty host scope supplies no checked module evidence to the closure.
    let function = module_closure(function, &Scope::root());
    let mut interp = Interp::new(&CheckedModule::default()).with_mode(Mode::Ir);
    for (input, expected) in [(-0.0, -0.0f64), (1.0, 2.25), (3.0, 4.25)] {
        let result = interp
            .apply(function.clone(), vec![Value::float(input)], Span::default())
            .unwrap();
        assert_eq!(result.as_f64().unwrap().to_bits(), expected.to_bits());
    }
}

#[test]
fn recursive_jit_and_cancellable_ir_agree_on_values_and_budget_errors() {
    const PROGRAM: &str = r#"@jit
fn sum_down(n: Float, acc: Float): Float =
    if n <= 0.0 then acc else sum_down(n - 1.0, acc + n)
fn main(): IO Float = pure(sum_down(15.0, 0.0))
"#;
    let compilation = compile("jit-cancel.aev", PROGRAM);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.as_ref().unwrap();
    let cases = [(Mode::Ast, false), (Mode::Ir, false), (Mode::Ir, true)];
    let mut budget_error = None;
    for (mode, cancellable) in cases {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        assert!(!interp.load(module).has_errors());
        let token = CancellationToken::new();
        if cancellable {
            interp.set_cancellation_token(token.clone());
        }
        for _ in 0..3 {
            let result = interp.run_main().unwrap().unwrap();
            assert_eq!(result.as_f64().unwrap().to_bits(), 120.0f64.to_bits());
        }
        if mode == Mode::Ir {
            interp.max_tail_calls = 3;
            let error = interp.run_main().unwrap_err();
            assert_eq!(error.code, "E0805");
            let observed = (error.message, error.span);
            if let Some(expected) = &budget_error {
                assert_eq!(&observed, expected);
            } else {
                budget_error = Some(observed);
            }
        }
        if cancellable {
            token.cancel();
            assert_eq!(interp.run_main().unwrap_err().code, "E0812");
        }
    }
}
