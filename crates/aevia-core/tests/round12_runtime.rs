//! Round 12: binding-position capitals, the stack budget, and call-proof
//! reuse. Assertions are on values and error codes, not renderings.

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::compile;
use aevia_core::value::NumKind;
use aevia_core::value::Value;

fn on_stack<T: Send + 'static>(
    bytes: usize,
    body: impl FnOnce() -> T + Send + 'static,
) -> T {
    std::thread::Builder::new()
        .stack_size(bytes)
        .spawn(body)
        .expect("spawn")
        .join()
        .expect("the test body panicked")
}

fn interp(source: &str) -> Interp {
    let compilation = compile("test.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "compile errors: {:?}",
        compilation
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let module = compilation.module.expect("module");
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = interp.load(&module);
    assert!(!diagnostics.has_errors(), "load errors");
    interp
}

fn int(
    source: &str,
    name: &str,
) -> i64 {
    let source = source.to_string();
    let name = name.to_string();
    on_stack(256 << 20, move || {
        let mut interp = interp(&source);
        match interp.eval_toplevel(&name).expect("value exists") {
            | Value::Num(number) => {
                match number.kind {
                    | NumKind::Int(value) => i64::try_from(value).expect("small"),
                    | other => panic!("expected an Int, found {other:?}"),
                }
            },
            | other => panic!("expected a scalar, found {}", other.type_name()),
        }
    })
}

fn float(
    source: &str,
    name: &str,
) -> f64 {
    let source = source.to_string();
    let name = name.to_string();
    on_stack(256 << 20, move || {
        let mut interp = interp(&source);
        match interp.eval_toplevel(&name).expect("value exists") {
            | Value::Num(number) => {
                match number.kind {
                    | NumKind::Float(value) => value,
                    | other => panic!("expected a Float, found {other:?}"),
                }
            },
            | other => panic!("expected a scalar, found {}", other.type_name()),
        }
    })
}

/// Runs `main` with an optional declared stack budget; `Ok` is its output.
fn run_main(
    source: &str,
    budget: Option<usize>,
) -> Result<String, (String, Vec<String>)> {
    let source = source.to_string();
    on_stack(256 << 20, move || {
        if let Some(budget) = budget {
            aevia_core::install_stack_budget(budget);
        }
        let mut interp = interp(&source);
        if budget.is_some() {
            interp.max_depth = usize::MAX;
        }
        match interp.run_main() {
            | Ok(_) => Ok(interp.captured()),
            | Err(error) => Err((error.code.to_string(), error.trace())),
        }
    })
}

// --- binding-position capitals (§2.4) -------------------------------------

#[test]
fn a_capital_let_binds_a_variable() {
    let source = "let M = 3\nlet K = M + 4\n";
    assert_eq!(int(source, "K"), 7);
}

#[test]
fn a_capital_parameter_binds_a_variable() {
    let source = "fn twice(N: Int): Int = N * 2\nlet r = twice(21)\n";
    assert_eq!(int(source, "r"), 42);
}

#[test]
fn a_capital_lambda_parameter_binds_a_variable() {
    let source = "let f = fn X => X + 1\nlet r = f(1)\n";
    assert_eq!(int(source, "r"), 2);
}

#[test]
fn a_capital_shadows_a_unit_name_as_a_value() {
    // `A` is the ampere; as a binding it is an ordinary variable.
    let source = "open Units.SI\nlet A = 2.5\nlet r = A * 2.0\n";
    assert!((float(source, "r") - 5.0).abs() < 1e-12);
}

#[test]
fn a_capital_inside_a_tuple_binding_binds() {
    let source = "let (P, Q) = (2, 5)\nlet r = P * Q\n";
    assert_eq!(int(source, "r"), 10);
}

#[test]
fn a_match_arm_capital_is_still_a_constructor() {
    let source = "type T = | Lo | Hi\n\
                  fn rank(t: T): Int = match t with\n    | Lo -> 1\n    | Hi -> 2\n\
                  let r = rank(Hi) * 10 + rank(Lo)\n";
    assert_eq!(int(source, "r"), 21);
}

#[test]
fn a_constructor_pattern_with_arguments_still_destructures_in_let() {
    let source = "type Box = | Box(Int)\nlet Box(n) = Box(9)\nlet r = n\n";
    assert_eq!(int(source, "r"), 9);
}

// --- stack budget (§9.3.2) ------------------------------------------------

const DEEP: &str = "fn g(n: Int): Int = if n == 0 then 0 else 1 + g(n - 1)\n";

#[test]
fn non_tail_recursion_is_bounded_by_the_declared_stack_not_a_call_count() {
    // 4000 nested calls are past the old fixed 2048-call guard and well within
    // a 256 MiB stack in any build profile.
    let source = format!(
        "{DEEP}fn main(): IO Unit = println(\"{{g(4000)}}\")
"
    );
    let output = run_main(&source, Some(256 << 20)).expect("runs");
    assert_eq!(output.trim(), "4000");
}

#[test]
fn an_exhausted_stack_budget_is_e0805_with_a_compressed_trace() {
    let source = format!(
        "{DEEP}fn main(): IO Unit = println(\"{{g(100000000)}}\")
"
    );
    let (code, trace) = run_main(&source, Some(64 << 20)).expect_err("overflows");
    assert_eq!(code, "E0805");
    assert!(
        trace.len() < 10,
        "a deep recursion trace is compressed, found {} lines",
        trace.len()
    );
    assert!(
        trace
            .iter()
            .any(|line| line.contains("more recursive frames")),
        "{trace:?}"
    );
}

#[test]
fn without_a_declared_budget_the_call_count_guard_still_applies() {
    let source = format!(
        "{DEEP}fn main(): IO Unit = println(\"{{g(100000000)}}\")
"
    );
    let (code, _) = run_main(&source, None).expect_err("overflows");
    assert_eq!(code, "E0805");
}

// --- call-proof reuse -----------------------------------------------------

#[test]
fn a_memoized_call_proof_does_not_leak_between_representations() {
    // The same generic function is called with Int and Float arguments from
    // the same call shapes; each must keep its own representation.
    let source = "fn twice<a>(x: a, f: (a) -> a): a = f(f(x))\n\
                  fn loop_i(n: Int, acc: Int): Int = if n == 0 then acc else loop_i(n - 1, acc + 2)\n\
                  fn loop_f(n: Int, acc: Float): Float = if n == 0 then acc else loop_f(n - 1, acc + 0.5)\n\
                  let ri = loop_i(1000, 0)\n\
                  let rf = loop_f(1000, 0.0)\n\
                  let rt = twice(3, fn v => v + 1)\n";
    assert_eq!(int(source, "ri"), 2000);
    assert!((float(source, "rf") - 500.0).abs() < 1e-12);
    assert_eq!(int(source, "rt"), 5);
}

#[test]
fn a_dimension_polymorphic_loop_keeps_each_call_sites_dimension() {
    let source = "open Units.SI\n\
                  fn acc<d>(n: Int, x: Float[d], total: Float[d]): Float[d] =\n\
                  \x20   if n == 0 then total else acc(n - 1, x, total + x)\n\
                  let metres = acc(100, 1.0 m, 0.0 m) / (1.0 m)\n\
                  let seconds = acc(50, 2.0 s, 0.0 s) / (1.0 s)\n";
    assert!((float(source, "metres") - 100.0).abs() < 1e-12);
    assert!((float(source, "seconds") - 100.0).abs() < 1e-12);
}
