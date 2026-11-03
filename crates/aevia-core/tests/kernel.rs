//! The rssn backend, end to end: heavy requests, the `Rssn` module, and the
//! errors the kernel reports.
//!
//! Assertions are on values, `NumKind`s, tensor shapes and error codes. The
//! old versions of several of these checked that a printed string *contained*
//! a digit, which is true of almost any answer — `contains("-2")` and
//! `contains('2')` are satisfied by the same character.

use aevia_core::value::{NumKind, Value};
use aevia_core::{compile, Interp};

fn build(source: &str) -> (aevia_syntax::Module, aevia_types::CheckedModule) {
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
    (module, compilation.checked)
}

fn interp(source: &str) -> Interp {
    let (module, checked) = build(source);
    let mut interp = Interp::new(&checked).with_buffer();
    let diagnostics = interp.load(&module);
    assert!(
        !diagnostics.has_errors(),
        "load errors: {:?}",
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    interp
}

fn run(source: &str) -> String {
    let mut interp = interp(source);
    interp.run_main().expect("main runs");
    interp.captured()
}

fn value(source: &str, name: &str) -> Value {
    let mut interp = interp(source);
    interp.eval_toplevel(name).expect("value exists")
}

fn num(source: &str, name: &str) -> aevia_core::value::NumVal {
    match value(source, name) {
        Value::Num(number) => number,
        other => panic!("expected a scalar, found {}", other.type_name()),
    }
}

/// The elements of a rank-1 tensor as exact rationals.
fn elements(value: &Value) -> Vec<num_rational::BigRational> {
    let Value::Tensor(tensor) = value else {
        panic!("expected a tensor, found {}", value.type_name());
    };
    assert_eq!(tensor.rank(), 1, "expected a vector");
    tensor
        .elements()
        .iter()
        .map(|element| {
            element
                .as_num()
                .and_then(aevia_core::value::NumVal::to_rational)
                .unwrap_or_else(|| panic!("expected a number, found {}", element.type_name()))
        })
        .collect()
}

fn rat(numerator: i64, denominator: i64) -> num_rational::BigRational {
    num_rational::BigRational::new(numerator.into(), denominator.into())
}

#[test]
fn the_antiderivative_of_x_squared_is_x_cubed_over_three() {
    // `a(3.0)` is the exact `9` — the kernel's `3^3/3` folded, not a string
    // that happens to contain a `9`.
    let derivative = num(
        r#"let f = fn t => t^2
let a = antiderivative(f)
let b = a(3.0)
"#,
        "b",
    );
    assert!(
        matches!(derivative.kind, NumKind::Int(_) | NumKind::Exact(_)),
        "the antiderivative at a point must be exact, found {}",
        derivative.kind.type_name()
    );
    assert_eq!(derivative.to_rational(), Some(rat(9, 1)));
}

#[test]
fn a_definite_integral_is_evaluated() {
    let output = run(
        r#"fn main(): IO Unit = do
    println("{defint(fn t => t^2, 0.0, 3.0)}")
"#,
    );
    assert_eq!(output, "Ok(9)\n");
}

#[test]
fn a_limit_is_evaluated() {
    let output = run(
        r#"fn main(): IO Unit = do
    println("{limit(fn t => sin(t) / t, 0.0)}")
"#,
    );
    assert_eq!(output, "Ok(1)\n");
}

#[test]
fn a_taylor_series_is_returned() {
    let output = run(
        r#"fn main(): IO Unit = do
    println("{series(fn t => exp(t), 0.0, 3)}")
"#,
    );
    // 1 + x + x^2/2 + x^3/6, in whatever order the kernel writes it.
    assert!(output.contains("1/6"), "unexpected series: {output}");
    assert!(output.contains("^3"), "unexpected series: {output}");
}

#[test]
fn solve_returns_every_root() {
    let roots = value("let s = solve(fn t => t^2 - 4)\n", "s");
    let Value::Variant(result) = roots else {
        panic!("solve must return a Result");
    };
    assert_eq!(result.index, 0, "solve failed");
    let Some(Value::Record(solutions)) = result.fields.first() else {
        panic!("expected a Solutions record");
    };
    let Some(roots) = solutions.fields.get("roots") else {
        panic!("Solutions has no roots");
    };
    let mut values = elements(roots);
    values.sort();
    // Both roots, exactly: `-2` and `2`.
    assert_eq!(values, vec![rat(-2, 1), rat(2, 1)]);
}

#[test]
fn nsolve_finds_a_numeric_root() {
    let output = run(
        r#"fn main(): IO Unit = do
    println("{nsolve(fn t => cos(t) - t, 1.0)}")
"#,
    );
    assert!(output.starts_with("Ok(0.739"), "unexpected root: {output}");
}

#[test]
fn simplify_folds_like_terms() {
    // `x + x` is exactly `2*x`; the old assertion accepted any output that
    // contained a `2` and an `x` — including `x + 2`.
    let simplified = num(
        r#"let x = symbol("x")
let s = simplify(x + x)
"#,
        "s",
    );
    assert!(
        matches!(simplified.kind, NumKind::Sym(_)),
        "expected a symbolic value, found {}",
        simplified.kind.type_name()
    );
    assert_eq!(
        aevia_core::value::repr(&Value::Num(simplified)),
        "2*x"
    );
}

#[test]
fn the_rssn_module_exposes_the_kernel_version() {
    let output = run(
        r#"fn main(): IO Unit = do
    println("{Rssn.Version}")
"#,
    );
    assert_eq!(output.trim(), aevia_rssn::registry().version);
}

#[test]
fn the_rssn_module_calls_operators_by_name() {
    let output = run(
        r#"fn main(): IO Unit = do
    println("{Rssn.sqrt(16)}")
    println("{Rssn.det([[1.0, 2.0], [3.0, 4.0]])}")
"#,
    );
    assert_eq!(output, "4\n-2\n");
}

#[test]
fn a_dynamic_call_crosses_the_boundary() {
    let answer = value(
        r#"let r = Rssn.dynamic("sin", [to_value(0.0)])
"#,
        "r",
    );
    let Value::Variant(result) = answer else {
        panic!("`Rssn.dynamic` must return a Result");
    };
    assert_eq!(result.index, 0, "the dynamic call failed");
    match result.fields.first() {
        Some(Value::Num(number)) => {
            assert!(
                matches!(number.kind, NumKind::Float(_)),
                "sin(0.0) is a Float, found {}",
                number.kind.type_name()
            );
            assert_eq!(number.to_f64(), Some(0.0), "sin(0) is exactly 0");
        }
        other => panic!("expected a number, found {other:?}"),
    }
}

#[test]
fn an_unknown_operator_is_reported_at_check_time() {
    let compilation = compile("test.aev", "let a = Rssn.not_an_operator(1)\n");
    assert!(compilation.diagnostics.has_errors());
    let codes: Vec<&str> = compilation
        .diagnostics
        .items
        .iter()
        .map(|item| item.code.as_str())
        .collect();
    assert!(codes.contains(&"E0301"), "unexpected codes: {codes:?}");
}

#[test]
fn overloads_are_listed() {
    let listed = value(
        r#"let name = "sin"
let l = Rssn.overloads(name)
"#,
        "l",
    );
    let Value::Tensor(tensor) = listed else {
        panic!("expected a list of overloads");
    };
    let expected = aevia_rssn::registry()
        .op("sin")
        .map_or(0, |entry| entry.signatures.len());
    assert_eq!(tensor.shape, vec![expected]);
    assert!(expected > 1, "`sin` should have several overloads");
}

// --- regressions found by the second review ---------------------------------

#[test]
fn calculus_results_carry_the_argument_dimension() {
    let output = run(
        r#"fn main(): IO Unit = do
    println("{antiderivative(fn t: Real[L] => t)(2.0 m)}")
    println("{defint(fn t: Real[L] => t, 0.0 m, 3.0 m)}")
    println("{limit(fn t: Real[L] => 2.0 * t, 2.0 m)}")
"#,
    );
    assert_eq!(output, "2 m^2\nOk(4.5 m^2)\nOk(4 m)\n");
}

#[test]
fn nested_antiderivatives_do_not_capture_each_other() {
    let output = run(
        r#"fn main(): IO Unit = do
    let inner = antiderivative(fn u => u)
    let outer = antiderivative(fn t => inner(t))
    println("{outer(2.0)}")
"#,
    );
    // inner(t) = t^2/2, so the outer integral is t^3/6; at 2 that is 4/3.
    assert_eq!(output, "4/3\n");
}

#[test]
fn solutions_are_a_record() {
    let roots = value("let s = solve(fn t => t^2 - 4)\n", "s");
    let Value::Variant(result) = roots else {
        panic!("solve must return a Result");
    };
    let Some(Value::Record(solutions)) = result.fields.first() else {
        panic!("expected a Solutions record");
    };
    let Some(Value::Tensor(roots)) = solutions.fields.get("roots") else {
        panic!("expected a roots tensor");
    };
    assert_eq!(roots.shape, vec![2], "two roots");
    let found: Vec<f64> = roots
        .elements()
        .iter()
        .map(|element| element.as_f64().expect("a number"))
        .collect();
    assert!(
        found.iter().any(|value| (*value + 2.0).abs() < 1e-12),
        "missing root -2: {found:?}"
    );
    assert!(
        found.iter().any(|value| (*value - 2.0).abs() < 1e-12),
        "missing root 2: {found:?}"
    );
}

#[test]
fn a_dynamic_call_reports_failure_as_a_value() {
    let output = run(
        r#"fn main(): IO Unit = do
    match Rssn.dynamic("no_such_operator", []) with
    | Ok(v) -> println("ok")
    | Err(e) -> println("err")
"#,
    );
    assert_eq!(output, "err\n");
}

#[test]
fn a_stale_signature_pin_is_an_error() {
    let output = run(
        r#"fn main(): IO Unit = do
    match Rssn.call_key("sin", "deadbeef", [to_value(0.5)]) with
    | Ok(v) -> println("ok")
    | Err(e) -> println("err")
"#,
    );
    assert_eq!(output, "err\n");
}

#[test]
fn a_user_symbol_may_contain_a_hash() {
    let text = aevia_core::value::repr(&value("let a = symbol(\"a#b\")\n", "a"));
    assert_eq!(text, "a#b");
}

// --- item 7: the pinned overload is the executed one -----------------------

#[test]
fn a_call_site_executes_the_overload_the_checker_pinned() {
    // `det` has a `Dyn` overload and a `Tensor` overload; the checker picks by
    // argument type and the runtime must run *that* key.
    let (module, checked) = build("let a = Rssn.det([[1.0, 2.0], [3.0, 4.0]])\n");
    assert_eq!(checked.rssn_calls.len(), 1, "the checker must pin one key");
    let pinned = &checked.rssn_calls[0];
    assert_eq!(pinned.name, "det");
    let entry = aevia_rssn::registry().op("det").expect("det");
    assert_eq!(
        entry.key(pinned.overload),
        Some(&pinned.key),
        "the pinned key must be the overload the checker chose"
    );
    let mut interp = Interp::new(&checked);
    interp.load(&module);
    let determinant = interp.eval_toplevel("a").expect("evaluates");
    assert_eq!(
        aevia_core::value::repr(&determinant),
        "-2.0",
        "the pinned overload must be the one that ran"
    );
}

#[test]
fn an_operator_without_a_pinned_key_is_e1005() {
    // An operator used as a value has no call site, so there is no key to
    // execute. Re-resolving by name here is what D1 forbids.
    let mut interp = Interp::new(&aevia_types::CheckedModule::default());
    let operator = aevia_core::rssn_module::member("det");
    let error = interp
        .apply(operator, vec![], aevia_syntax::Span::default())
        .expect_err("an operator is not a first-class value");
    assert_eq!(error.code, "E1005");
}

#[test]
fn a_matrix_returning_operator_returns_a_tensor_of_the_declared_rank() {
    // The kernel used to derive a tensor's shape from the outermost `list`
    // only, so `identity(3)` came back as an opaque term: the checker promised
    // `Tensor<2, Real>` and the value was a symbolic `list(list(...))`. Every
    // matrix-returning operator was violating its declared type.
    let matrix = match value("let m = Rssn.identity(3)", "m") {
        Value::Tensor(tensor) => tensor,
        other => panic!("identity must return a tensor, found {}", other.type_name()),
    };
    assert_eq!(matrix.shape, vec![3, 3], "shape");
    assert_eq!(matrix.storage.len(), 9, "element count");

    let transposed = match value("let t = Rssn.transpose([[1, 2], [3, 4]])", "t") {
        Value::Tensor(tensor) => tensor,
        other => panic!("transpose must return a tensor, found {}", other.type_name()),
    };
    assert_eq!(transposed.shape, vec![2, 2], "shape");
    let elements: Vec<String> = (0..transposed.storage.len())
        .filter_map(|index| transposed.get_flat(index))
        .map(|item| aevia_core::value::show(&item))
        .collect();
    assert_eq!(elements, ["1", "3", "2", "4"], "row-major order");
}
