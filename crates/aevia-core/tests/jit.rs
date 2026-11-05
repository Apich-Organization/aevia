#![cfg(feature = "jit")]

use aevia_core::value::{NumKind, Value};
use aevia_core::{Interp, Mode, compile};

fn result(
    source: &str,
    mode: Mode,
    name: &str,
) -> Value {
    let compilation = compile("jit.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let mut interp = Interp::new(&compilation.checked).with_mode(mode);
    let diagnostics = interp.load(compilation.module.as_ref().unwrap());
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    interp.eval_toplevel(name).expect("evaluates")
}

#[test]
fn annotated_scalar_branches_match_interpreter() {
    let source = r#"@jit
fn choose(x: Float, y: Float): Float =
    if x < y then x * 2.0 + y else x / y - 1.0
let a = choose(2.0, 3.0)
let b = choose(8.0, 2.0)
let c = choose(0.0, 0.0)
"#;
    for name in ["a", "b", "c"] {
        let a = result(source, Mode::Ast, name);
        let b = result(source, Mode::Ir, name);
        let (Value::Num(a), Value::Num(b)) = (a, b) else {
            panic!("numeric result expected");
        };
        let (NumKind::Float(a), NumKind::Float(b)) = (a.kind, b.kind) else {
            panic!("float result expected");
        };
        assert!(
            a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()),
            "{name}: {a} != {b}"
        );
    }
}

#[test]
fn unsupported_recursive_function_falls_back() {
    let source = r#"@jit
fn count(n: Int): Int = if n == 0 then 0 else count(n - 1)
let result = count(20)
"#;
    assert_eq!(
        aevia_core::value::repr(&result(source, Mode::Ir, "result")),
        "0"
    );
}

#[test]
fn float_tail_recursion_uses_the_existing_budget() {
    let source = r#"@jit
fn sum(n: Float, acc: Float): Float = if n <= 0.0 then acc else sum(n - 1.0, acc + n)
let result = sum(10.0, 0.0)
"#;
    for mode in [Mode::Ast, Mode::Ir] {
        assert_eq!(
            aevia_core::value::repr(&result(source, mode, "result")),
            "55.0"
        );
    }
    let compilation = compile("budget.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let mut interp = Interp::new(&compilation.checked).with_mode(Mode::Ir);
    interp.max_tail_calls = 3;
    let diagnostics = interp.load(compilation.module.as_ref().unwrap());
    // Top-level lets evaluate during loading.
    assert!(
        diagnostics.items.iter().any(|d| d.code == "E0805"),
        "{:?}",
        diagnostics.items
    );
}

#[test]
fn ieee_nan_and_signed_zero_match_ast() {
    let source = r#"@jit
fn different(x: Float, y: Float): Bool = x != y
@jit
fn identity(x: Float): Float = x
let nan = different(0.0 / 0.0, 0.0)
let zero = identity(-0.0)
"#;
    for mode in [Mode::Ast, Mode::Ir] {
        assert!(matches!(result(source, mode, "nan"), Value::Bool(true)));
        let Value::Num(value) = result(source, mode, "zero") else {
            panic!("number expected")
        };
        let NumKind::Float(value) = value.kind else {
            panic!("float expected")
        };
        assert_eq!(value.to_bits(), (-0.0f64).to_bits());
    }
}

#[test]
fn tail_jump_respects_rebound_function() {
    use aevia_core::value::ClosureEnv;
    let source = r#"@jit
fn sum(n: Float, acc: Float): Float = if n <= 0.0 then acc else sum(n - 1.0, acc + n)
fn replacement(n: Float, acc: Float): Float = 42.0
"#;
    let compilation = compile("rebind.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let mut interp = Interp::new(&compilation.checked).with_mode(Mode::Ir);
    let diagnostics = interp.load(compilation.module.as_ref().unwrap());
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    let original = interp.eval_toplevel("sum").unwrap();
    let replacement = interp.eval_toplevel("replacement").unwrap();
    let Value::Closure(closure) = &original else {
        panic!("closure expected")
    };
    let ClosureEnv::Ir(env) = &closure.env else {
        panic!("IR closure expected")
    };
    env.scope
        .as_ref()
        .unwrap()
        .vars
        .borrow_mut()
        .insert("sum".into(), replacement);
    let value = interp
        .apply(
            original,
            vec![Value::float(3.0), Value::float(0.0)],
            Default::default(),
        )
        .unwrap();
    assert_eq!(aevia_core::value::repr(&value), "42.0");
}

#[test]
fn native_loop_preserves_budget_boundary_and_error_location() {
    let source = "@jit\nfn sum(n: Float, acc: Float): Float = if n <= 0.0 then acc else sum(n - 1.0, acc + n)\nlet result = sum(3.0, 0.0)\n";
    let interpreted = source.replacen("@jit", "    ", 1);
    for budget in [0, 2, 3] {
        let run = |source: &str| {
            let compilation = compile("budget-boundary.aev", source);
            assert!(
                !compilation.diagnostics.has_errors(),
                "{:?}",
                compilation.diagnostics.items
            );
            let mut interp = Interp::new(&compilation.checked).with_mode(Mode::Ir);
            interp.max_tail_calls = budget;
            let diagnostics = interp.load(compilation.module.as_ref().unwrap());
            let errors: Vec<_> = diagnostics
                .items
                .iter()
                .map(|d| (d.code.clone(), d.span))
                .collect();
            let value = if errors.is_empty() {
                Some(aevia_core::value::repr(
                    &interp.eval_toplevel("result").unwrap(),
                ))
            } else {
                None
            };
            (errors, value)
        };
        let expected = run(&interpreted);
        let actual = run(source);
        assert_eq!(actual, expected, "budget {budget}");
        if budget == 3 {
            assert_eq!(actual.1.as_deref(), Some("6.0"));
        }
    }
}
