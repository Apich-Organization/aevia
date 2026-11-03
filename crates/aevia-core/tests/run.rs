//! End-to-end execution tests: source in, values and output out.
//!
//! Every assertion here is about semantics — the `NumKind` of a scalar, the
//! storage class of a tensor, an error code, or `repr` when `repr` is the
//! thing under test (§3 of the plan: "tests assert semantics, not
//! renderings"). `show` is only asserted where the rendering *is* the
//! observable: a display unit, or a `println` in a `do` block.

#![allow(clippy::result_large_err)]

use aevia_core::value::{NumKind, Storage, Value};
use aevia_core::{compile, EvalError, Interp};

/// Runs a test body on a thread with the stack the CLI gives the interpreter
/// (§9.9: 256 MiB). The harness's default test stack is far smaller, and a
/// legal program must not be judged by the harness's limit.
fn with_cli_stack<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(body)
        .expect("spawn")
        .join()
        .expect("the test body panicked")
}

fn build(source: &str) -> (aevia_syntax::Module, aevia_types::CheckedModule) {
    let compilation = compile("test.aev", source);
    if compilation.diagnostics.has_errors() {
        eprintln!("--- source ---
{source}--- end ---");
    }
    assert!(
        !compilation.diagnostics.has_errors(),
        "compile errors: {:?}",
        compilation
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message, item.span.start, item.span.end))
            .collect::<Vec<_>>()
    );
    let module = compilation.module.expect("module");
    (module, compilation.checked)
}

/// Loads a program and returns its interpreter, ready to run `main`.
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

/// Runs `main` and returns the runtime error it raised.
fn run_error(source: &str) -> EvalError {
    let mut interp = interp(source);
    match interp.run_main() {
        Ok(_) => panic!("expected a runtime error"),
        Err(error) => error,
    }
}

fn value(source: &str, name: &str) -> Value {
    let mut interp = interp(source);
    interp.eval_toplevel(name).expect("value exists")
}

fn repr(source: &str, name: &str) -> String {
    aevia_core::value::repr(&value(source, name))
}

fn num(source: &str, name: &str) -> aevia_core::value::NumVal {
    match value(source, name) {
        Value::Num(number) => number,
        other => panic!("expected a scalar, found {}", other.type_name()),
    }
}

fn storage(source: &str, name: &str) -> Storage {
    match value(source, name) {
        Value::Tensor(tensor) => tensor.storage.clone(),
        other => panic!("expected a tensor, found {}", other.type_name()),
    }
}

#[test]
fn kinematics_program_keeps_both_phases() {
    // `2 * h / g` is exact (both operands are exact quantities), so `sqrt` of
    // it has no exact root and stays symbolic (§6.3.1). `approx` is the
    // explicit step to a number — that is the language's story, and this
    // program now shows it instead of hiding it in a float.
    let output = run(
        r#"module Kinematics

let g = 9.80665 m/s^2

fn fall_time(h: Real[L]): Real[T] =
    sqrt(2 * h / g)

fn main(): IO Unit = do
    let h = 20.0 m
    println("fall time from {h} = {fall_time(h)}")
    match approx(fall_time(h)) with
    | Ok(t) -> println("numerically = {t}")
    | Err(e) -> println("approx failed: {e}")
"#,
    );
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 2, "unexpected output: {output}");
    assert!(
        lines[0].starts_with("fall time from 20 m = sqrt("),
        "the exact answer must stay a root: {output}"
    );
    assert!(lines[0].ends_with(" s"), "the dimension must survive: {output}");
    let numeric: f64 = lines[1]
        .trim_start_matches("numerically = ")
        .trim_end_matches(" s")
        .parse()
        .expect("a number");
    assert!(
        (numeric - 2.019_619_977_102_552_3).abs() < 1e-9,
        "unexpected fall time: {numeric}"
    );
}

#[test]
fn a_float_mixed_with_an_exact_value_is_real() {
    // §6.3.2: `Float ⊗ Exact -> Real`. `1.0 + 2` used to be `Float 3`, which
    // made the exactness guarantee untestable.
    let sum = num("let a = 1.0 + 2\n", "a");
    assert!(
        matches!(sum.kind, NumKind::Exact(_)),
        "expected an exact value, found {}",
        sum.kind.type_name()
    );
    assert_eq!(repr("let a = 1.0 + 2\n", "a"), "3");

    let third = num("let a = 1/3 + 0.5\n", "a");
    assert!(
        matches!(third.kind, NumKind::Exact(_)),
        "expected an exact value, found {}",
        third.kind.type_name()
    );
    assert_eq!(repr("let a = 1/3 + 0.5\n", "a"), "5/6");
}

#[test]
fn two_floats_stay_floats() {
    let sum = num("let a = 0.1 + 0.2\n", "a");
    assert!(matches!(sum.kind, NumKind::Float(_)));
    // Shortest round-trip: the text is the value, not a rounding of it.
    assert_eq!(repr("let a = 0.1 + 0.2\n", "a"), "0.30000000000000004");
    assert_eq!(
        repr("let a = 0.1 + 0.2 == 0.3\n", "a"),
        "false",
        "the printed value and `==` must agree"
    );
}

#[test]
fn a_root_of_an_exact_value_is_symbolic_and_squares_back_exactly() {
    let root = num("let a = sqrt(2)\n", "a");
    assert!(
        matches!(root.kind, NumKind::Sym(_)),
        "sqrt(2) must stay symbolic, found {}",
        root.kind.type_name()
    );
    assert_eq!(repr("let a = sqrt(2)\n", "a"), "sqrt(2)");

    // `sqrt(2)^2` was `2.0000000000000004`; it is exactly `2`.
    assert_eq!(repr("let a = sqrt(2)^2\n", "a"), "2");
    assert_eq!(repr("let a = sqrt(2)^2 == 2\n", "a"), "true");

    // A perfect square stays exact.
    assert_eq!(repr("let a = sqrt(4)\n", "a"), "2");
}

#[test]
fn unit_literals_fold_to_coherent_values() {
    // The display unit is the observable here.
    assert_eq!(aevia_core::show(&value("let d = 3.2 km\n", "d")), "3.2 km");
    assert_eq!(
        aevia_core::show(&value("let d = 3.2 km\nlet m = value_in(km, d)\n", "m")),
        "3.2"
    );
    // `3.2 km` and `3200 m` are the same value.
    assert_eq!(
        repr(
            "let a = 3.2 km == 3200 m\n",
            "a"
        ),
        "true"
    );
}

#[test]
fn affine_temperatures_keep_their_display_unit() {
    assert_eq!(aevia_core::show(&value("let t = 25.0 °C\n", "t")), "25 °C");
    assert_eq!(
        aevia_core::show(&value("let t = 25.0 °C\nlet k = value_in(K, t)\n", "k")),
        "298.15"
    );
}

#[test]
fn symbolic_differentiation_matches_the_spec_example() {
    // §6.13 step 4: `df(2)` is the exact number `9`, not a symbolic value that
    // happens to print as `9`.
    let source = r#"let x = symbol("x")
let f = fn t => t^3 - 3*t + 1
let df = diff(f)
let a = df(2)
"#;
    let derivative = num(source, "a");
    assert!(
        matches!(derivative.kind, NumKind::Exact(_) | NumKind::Int(_)),
        "expected a number, found {}",
        derivative.kind.type_name()
    );
    assert_eq!(repr(source, "a"), "9");
}

#[test]
fn recursion_and_pattern_matching_work() {
    let output = run(
        r#"type Shape =
    | Circle(Real[L])
    | Rect(Real[L], Real[L])

fn area(s: Shape): Real[L^2] =
    match s with
    | Circle(r) -> 3.0 * r^2
    | Rect(w, h) -> w * h

fn main(): IO Unit = do
    println(area(Circle(2.0 m)))
    println(area(Rect(2.0 m, 3.0 m)))
"#,
    );
    assert_eq!(output, "12 m^2\n6 m^2\n");
}

#[test]
fn tensors_index_and_multiply() {
    let source = r#"let m = [[1.0, 2.0], [3.0, 4.0]]
let v = [5.0, 6.0]
let p = m @ v
"#;
    assert_eq!(repr(source, "p"), "[17.0, 39.0]");
    assert!(matches!(storage(source, "p"), Storage::Dense(_)));
}

#[test]
fn reductions_and_statistics() {
    let source = r#"let xs = [1.0, 2.0, 3.0, 4.0]
let s = sum(xs)
let m = mean(xs)
let n = length(xs)
"#;
    assert_eq!(repr(source, "s"), "10.0");
    assert_eq!(repr(source, "m"), "2.5");
    assert_eq!(repr(source, "n"), "4");
}

#[test]
fn records_and_updates() {
    let source = r#"let p = { x = 1.0 m, y = 2.0 m }
let q = { p with x = 3.0 m }
let d = q.x
"#;
    assert_eq!(aevia_core::show(&value(source, "d")), "3 m");
}

#[test]
fn do_blocks_sequence_io() {
    let output = run(
        r#"fn main(): IO Unit = do
    let name = "aevia"
    println("hello, {name}")
    println(1 + 1)
"#,
    );
    assert_eq!(output, "hello, aevia\n2\n");
}

#[test]
fn result_and_question_mark() {
    let output = run(
        r#"fn safe_div(a: Float, b: Float): Result<Float, String> =
    if b == 0.0 then Err("zero") else Ok(a / b)

fn show_it(a: Float, b: Float): IO Unit =
    match safe_div(a, b) with
    | Ok(v) -> println("ok {v}")
    | Err(e) -> println("err {e}")

fn main(): IO Unit = do
    show_it(6.0, 3.0)
    show_it(1.0, 0.0)
"#,
    );
    assert_eq!(output, "ok 2\nerr zero\n");
}

#[test]
fn strings_interpolate_values() {
    let output = run(
        r#"fn main(): IO Unit = do
    let x = 2.0 m
    println("x = {x}, twice = {2 * x}")
"#,
    );
    assert_eq!(output, "x = 2 m, twice = 4 m\n");
}

#[test]
fn local_recursive_functions_terminate() {
    let output = run(
        r#"fn factorial(n: Int): Int =
    if n <= 1 then 1 else n * factorial(n - 1)

fn main(): IO Unit = do
    println(factorial(5))
"#,
    );
    assert_eq!(output, "120\n");
}

#[test]
fn pipes_and_placeholders_apply() {
    let source = r#"fn add(a: Int, b: Int): Int = a + b
let inc = add(1, _)
let three = 2 |> inc
"#;
    assert_eq!(repr(source, "three"), "3");
}

// --- regressions found by review -------------------------------------------

#[test]
fn deep_recursion_reports_or_computes_instead_of_aborting() {
    let output = run(
        r#"fn count(n: Int): Int = if n == 0 then 0 else count(n - 1)

fn main(): IO Unit = do
    println(count(200))
"#,
    );
    assert_eq!(output, "0\n");
}

#[test]
fn multiplication_keeps_exactness() {
    // `(2/3)*(3/4)` is exactly `1/2`; it used to print `0.5`, which is the
    // same text a `Float` would have produced.
    assert_eq!(repr("let a = (1/3) * 7\n", "a"), "7/3");
    assert_eq!(repr("let a = (2/3) * (3/4)\n", "a"), "1/2");
    assert!(matches!(
        num("let a = (2/3) * (3/4)\n", "a").kind,
        NumKind::Exact(_)
    ));
}

#[test]
fn large_integers_compare_exactly() {
    let source = r#"let a = 2^60
let b = a + 1
let eq = a == b
let lt = a < b
"#;
    assert_eq!(repr(source, "eq"), "false");
    assert_eq!(repr(source, "lt"), "true");
}

#[test]
fn question_mark_returns_the_error_value() {
    let output = run(
        r#"fn safe(p: String): Result<Int, String> =
    let x = parse_int(p)?
    Ok(x)

fn main(): IO Unit = do
    match safe("abc") with
    | Ok(v) -> println("ok {v}")
    | Err(e) -> println("err")
"#,
    );
    assert_eq!(output, "err\n");
}

#[test]
fn empty_tensors_do_not_panic() {
    assert_eq!(
        aevia_core::show(&value(
            "let xs = filter(fn (x: Int) => x > 10, [1, 2, 3])\nlet a = xs + 1\n",
            "a"
        )),
        "[]"
    );
}

#[test]
fn symbols_with_the_same_display_name_stay_distinct() {
    let source = r#"let x = symbol("x")
let y = symbol("x")
let s = x + y
"#;
    // Both print as `x`, but they are different symbols (§6.3.6) — `repr`
    // keeps the identity that `show` hides.
    assert_eq!(repr(source, "s"), "x + x#2");
    assert_eq!(aevia_core::show(&value(source, "s")), "x + x");
}

#[test]
fn diff_uses_a_fresh_variable() {
    let source = r#"let x = symbol("x")
let g = fn t => x + t
let d = diff(g)(3)
"#;
    assert_eq!(repr(source, "d"), "1");
}

#[test]
fn sqrt_of_a_negative_stays_formal() {
    assert_eq!(
        repr("let a = sqrt(0.0 - 1.0)\n", "a"),
        "sqrt(-1)"
    );
}

// --- item 3: structural equality -------------------------------------------

// --- item 2: show, repr, to_latex, to_typst --------------------------------

#[test]
fn the_three_renderers_are_not_aliases() {
    let latex = aevia_core::show(&value("let a = to_latex(1/2)\n", "a"));
    let typst = aevia_core::show(&value("let a = to_typst(1/2)\n", "a"));
    let shown = aevia_core::show(&value("let a = show(1/2)\n", "a"));
    assert_eq!(latex, r"\frac{1}{2}");
    assert_eq!(typst, "frac(1, 2)");
    assert_eq!(shown, "0.5");
    assert!(latex != shown && typst != shown && latex != typst);
}

#[test]
fn a_value_with_no_typeset_form_is_e1001() {
    // A variant payload has no mathematical reading; the renderer says so
    // instead of falling back to `show`.
    let error = run_error(
        r#"fn main(): IO Unit = do
    println(to_latex(Some(1)))
"#,
    );
    assert_eq!(error.code, "E1001");
}

#[test]
fn variants_compare_their_payloads() {
    let source = r#"let a = Some(1) == Some(2)
let b = Some(1) == Some(1)
let c = Ok(1) == Ok(2)
"#;
    assert_eq!(repr(source, "a"), "false");
    assert_eq!(repr(source, "b"), "true");
    assert_eq!(repr(source, "c"), "false");
}

#[test]
fn records_compare_their_values_not_their_text() {
    // `0.1 + 0.2` and `0.3` used to render as the same `0.3`, so comparing
    // the text said the records were equal. The values are not.
    let source = r#"let a = { x = 0.1 + 0.2 } == { x = 0.3 }
let b = { x = 1 } == { x = 1 }
let c = { x = 1, y = 2 } == { y = 2, x = 1 }
"#;
    assert_eq!(repr(source, "a"), "false");
    assert_eq!(repr(source, "b"), "true");
    assert_eq!(repr(source, "c"), "true", "field order is not identity");
}

#[test]
fn functions_have_no_equality() {
    // `{f = fn a => a} == {f = fn b => b}` used to be `true`: it compared the
    // rendering of two closures, both `<fn>`. The checker refuses the
    // comparison (`E0310`); the runtime arm is the second line of defence.
    let source = r#"fn main(): IO Unit = do
    let a = { f = fn a => a } == { f = fn b => b }
    println(a)
"#;
    let compilation = compile("test.aev", source);
    if compilation.diagnostics.has_errors() {
        let codes: Vec<&str> = compilation
            .diagnostics
            .items
            .iter()
            .map(|item| item.code.as_str())
            .collect();
        assert!(codes.contains(&"E0310"), "unexpected codes: {codes:?}");
        return;
    }
    assert_eq!(run_error(source).code, "E0302");
}

#[test]
fn tensors_compare_by_shape_and_element() {
    let source = r#"let a = [1, 2] == [1, 2]
let b = [1, 2] == [1, 3]
let c = [[1, 2], [3, 4]] == [[1, 2], [3, 4]]
let d = [0.1 + 0.2] == [0.3]
"#;
    assert_eq!(repr(source, "a"), "true");
    assert_eq!(repr(source, "b"), "false");
    assert_eq!(repr(source, "c"), "true");
    assert_eq!(repr(source, "d"), "false", "elements, not renderings");
}

// --- item 4: the IO model ---------------------------------------------------

#[test]
fn an_assertion_fires_when_the_action_runs() {
    // §6.8.1: building `do` must not run the assertion, so `before` is printed
    // even though the block fails.
    let mut interp = interp(
        r#"fn main(): IO Unit = do
    println("before")
    assert(1 == 2)
    println("after")
"#,
    );
    let error = interp.run_main().expect_err("the assertion must fail");
    assert_eq!(error.code, "E0899");
    assert_eq!(interp.captured(), "before\n");
}

#[test]
fn a_question_mark_in_an_io_block_is_not_silent() {
    // `fn main(): IO Unit = do let x = parse_int("abc")? …` used to exit 0
    // with no output at all.
    let compilation = compile(
        "test.aev",
        r#"fn main(): IO Unit = do
    let x = parse_int("abc")?
    println("never")
"#,
    );
    if compilation.diagnostics.has_errors() {
        let codes: Vec<&str> = compilation
            .diagnostics
            .items
            .iter()
            .map(|item| item.code.as_str())
            .collect();
        assert!(
            codes.contains(&"E0702") || codes.contains(&"E0302"),
            "unexpected codes: {codes:?}"
        );
        return;
    }
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let module = compilation.module.expect("module");
    interp.load(&module);
    let error = interp
        .run_main()
        .expect_err("`?` cannot return from an IO function");
    assert_eq!(error.code, "E0702");
}

#[test]
fn random_advances_only_when_the_action_runs() {
    // §6.10: a `let` that is never executed must not consume a value from the
    // sequence, so a program with an extra unused `random` still produces the
    // same first number.
    let with_extra = run(
        r#"fn main(): IO Unit = do
    let unused = random(())
    a <- random(())
    println("{a}")
"#,
    );
    let without = run(
        r#"fn main(): IO Unit = do
    a <- random(())
    println("{a}")
"#,
    );
    assert_eq!(
        with_extra, without,
        "a `let` perturbed the RNG when the block was built"
    );

    // Running the action does advance it, and the sequence is deterministic.
    let mut interp = Interp::new(&aevia_types::CheckedModule::default());
    let one = interp.next_random();
    let two = interp.next_random();
    assert!(one != two, "the RNG did not advance");
    assert!((0.0..1.0).contains(&one));
}

#[test]
fn time_now_reads_the_clock() {
    let output = run(
        r#"fn main(): IO Unit = do
    t <- time_now(())
    if t > 0.0 s then println("after 1970") else println("before 1970")
"#,
    );
    assert_eq!(output, "after 1970\n");
}

// --- item 5: symbolic operations are real or report the gap -----------------

#[test]
fn gamma_is_the_gamma_function() {
    // `gamma(5)` was `3.17805` because it returned `lgamma`.
    assert_eq!(repr("let a = gamma(5)\n", "a"), "24");
    let half = num("let a = gamma(0.5)\n", "a");
    let value = half.to_f64().expect("a number");
    assert!(
        (value - std::f64::consts::PI.sqrt()).abs() < 1e-12,
        "gamma(1/2) must be sqrt(pi), found {value}"
    );
}

#[test]
fn zeros_and_ones_honour_their_type_argument() {
    assert!(matches!(storage("let a = zeros<Int>(3)\n", "a"), Storage::Exact(_)));
    assert!(matches!(storage("let a = ones<Int>(3)\n", "a"), Storage::Exact(_)));
    assert!(matches!(storage("let a = zeros<Float>(3)\n", "a"), Storage::Dense(_)));
    assert!(matches!(storage("let a = ones<Float>(3)\n", "a"), Storage::Dense(_)));
}

#[test]
fn by_cases_decides_or_builds_a_term() {
    let source = r#"let x = symbol("x")
let a = by_cases(x > 0, fn () => 1, fn () => 0 - 1)
"#;
    let branch = num(source, "a");
    assert!(
        matches!(branch.kind, NumKind::Sym(_)),
        "an undecidable branch must stay symbolic, found {}",
        branch.kind.type_name()
    );
    assert_eq!(repr(source, "a"), "ite(gt(x, 0), 1, -1)");
}

#[test]
fn forall_samples_and_decides() {
    assert_eq!(
        repr("let a = forall(fn (t: Real) => t * 0 == 0)\n", "a"),
        "true"
    );
    assert_eq!(
        repr("let a = forall(fn (t: Real) => t > 0)\n", "a"),
        "false",
        "`forall` must not answer `true` for every property"
    );
}

#[test]
fn approx_eq_checks_the_dimension() {
    // The checker rejects `approx_eq(1 m, 1 s, 0.1)` outright (`E0401`), so
    // the runtime rule is exercised directly: a length is not within any
    // tolerance of a time.
    use aevia_core::value::NumVal;
    use aevia_dim::Dim;
    let metre = Value::Num(NumVal::exact(
        num_rational::BigRational::from_integer(1.into()),
        Dim::base("Length"),
    ));
    let second = Value::Num(NumVal::exact(
        num_rational::BigRational::from_integer(1.into()),
        Dim::base("Time"),
    ));
    let tolerance = Value::Num(NumVal::exact(
        num_rational::BigRational::new(1.into(), 10.into()),
        Dim::base("Length"),
    ));
    let error = aevia_core::builtins::call_simple(
        "approx_eq",
        vec![metre.clone(), second, tolerance.clone()],
    )
    .expect_err("dimensions must match");
    assert_eq!(error.code, "E0401");

    // With one dimension it is a real comparison, and it is exact.
    let also_metre = Value::Num(NumVal::exact(
        num_rational::BigRational::new(101.into(), 100.into()),
        Dim::base("Length"),
    ));
    let verdict = aevia_core::builtins::call_simple(
        "approx_eq",
        vec![metre, also_metre, tolerance],
    )
    .expect("same dimension");
    assert_eq!(aevia_core::value::repr(&verdict), "true");
}

#[test]
fn subst_yields_a_number_when_nothing_is_free() {
    let source = r#"let x = symbol("x")
let a = subst(x, 1, x^2)
let b = a == 1
"#;
    assert_eq!(repr(source, "a"), "1");
    assert_eq!(repr(source, "b"), "true");
}

#[test]
fn decide_compares_exact_values_without_rounding() {
    // A proposition built from two exact rationals that agree in `f64` but not
    // exactly: `1/3` and the double nearest to it.
    use aevia_core::value::{Comparison, PropValue};
    use aevia_syntax::ast::BinOp;
    use std::rc::Rc;
    let third = Value::Num(aevia_core::value::NumVal::exact(
        num_rational::BigRational::new(1.into(), 3.into()),
        aevia_dim::Dim::one(),
    ));
    let nearest = Value::float(1.0 / 3.0);
    let proposition = Value::Prop(Rc::new(PropValue {
        text: "1/3 == 0.333…".to_string(),
        comparison: Some(Comparison {
            op: BinOp::Eq,
            left: third,
            right: nearest,
        }),
    }));
    let decided = aevia_core::builtins::call_simple("decide", vec![proposition]).expect("decides");
    assert_eq!(aevia_core::value::repr(&decided), "Some(false)");
}

// --- item 6: Solutions and exact linear algebra -----------------------------

#[test]
fn det_of_an_exact_matrix_is_exact() {
    let source = r#"let a = det([[1, 2], [3, 4]])
let b = det([[1, 2], [2, 4]])
let c = det([[1/3, 0], [0, 1]])
"#;
    let determinant = num(source, "a");
    assert!(
        matches!(determinant.kind, NumKind::Int(_)),
        "det of an Int matrix must be an Int, found {}",
        determinant.kind.type_name()
    );
    assert_eq!(repr(source, "a"), "-2");
    // A singular matrix is exactly zero, not `-0`.
    assert_eq!(repr(source, "b"), "0");
    assert_eq!(repr(source, "c"), "1/3");
}

#[test]
fn inv_and_linsolve_of_an_exact_system_are_exact() {
    let source = r#"let a = inv([[1, 2], [3, 4]])
let b = linsolve([[2, 1], [1, 3]], [3, 5])
"#;
    assert_eq!(
        repr(source, "a"),
        "Ok([[-2, 1], [3/2, -1/2]])",
        "an exact inverse stays exact"
    );
    assert_eq!(repr(source, "b"), "Ok([4/5, 7/5])");
}

/// The `Solutions` record of §8.6, with the `Ok` wrapper removed.
fn solutions_of(source: &str, name: &str) -> std::rc::Rc<aevia_core::value::RecordValue> {
    let solved = value(source, name);
    let Value::Variant(result) = solved else {
        panic!("expected a Result, found {}", solved.type_name());
    };
    assert_eq!(result.index, 0, "solve must succeed");
    let Some(Value::Record(record)) = result.fields.first() else {
        panic!("expected a Solutions record");
    };
    std::rc::Rc::clone(record)
}

/// The elements of a `Tensor<1, _>` field of a `Solutions` record.
fn field_elements(
    record: &aevia_core::value::RecordValue,
    field: &str,
) -> Vec<Value> {
    match record.fields.get(field) {
        Some(Value::Tensor(tensor)) => tensor.elements(),
        other => panic!("`{field}` should be a tensor, found {other:?}"),
    }
}

/// A `Bool` field of a `Solutions` record.
fn bool_field(record: &aevia_core::value::RecordValue, field: &str) -> bool {
    match record.fields.get(field) {
        Some(Value::Bool(flag)) => *flag,
        other => panic!("`{field}` should be a Bool, found {other:?}"),
    }
}

/// The roots as integers, sorted, so the assertion does not depend on the
/// order the solver happens to produce.
fn integer_roots(record: &aevia_core::value::RecordValue) -> Vec<i64> {
    let mut roots: Vec<i64> = field_elements(record, "roots")
        .iter()
        .map(|root| match root {
            Value::Num(number) => match &number.kind {
                NumKind::Int(value) => value.to_string().parse().expect("an Int fits an i64"),
                other => panic!("expected an integer root, found {}", other.type_name()),
            },
            other => panic!("expected a number, found {}", other.type_name()),
        })
        .collect();
    roots.sort_unstable();
    roots
}

#[test]
fn solutions_carry_variable_and_completeness() {
    let solutions = solutions_of("let s = solve(fn t => t^2 - 4)\n", "s");
    for field in [
        "variable",
        "roots",
        "multiplicities",
        "conditions",
        "complete",
    ] {
        assert!(
            solutions.fields.contains_key(field),
            "`Solutions` is missing `{field}`"
        );
    }
    match solutions.fields.get("variable") {
        Some(Value::Variant(variable)) => {
            assert_eq!(variable.index, 1, "the solved variable must be named");
            match variable.fields.first() {
                Some(Value::Str(name)) => assert!(
                    !name.contains("#diff"),
                    "the internal suffix must not leak: {name}"
                ),
                other => panic!("expected a symbol name, found {other:?}"),
            }
        }
        other => panic!("expected an Option, found {other:?}"),
    }
    // The kernel answered with its exhaustive list form, so it claims
    // completeness; the bridge used to hardcode `false` here.
    assert!(
        bool_field(&solutions, "complete"),
        "a list answer is the solver's exhaustive form"
    );
    assert_eq!(integer_roots(&solutions), vec![-2, 2]);
    // §8.6 promises the program can read the flag, not just the host: the
    // record's `complete` field is in scope and typed `Bool`.
    let claimed = value(
        "let c = match solve(fn t => t^2 - 4) with
| Ok(s) -> s.complete
| Err(_) -> false
",
        "c",
    );
    match claimed {
        Value::Bool(flag) => assert!(flag, "the program sees the solver's claim"),
        other => panic!("`s.complete` should be a Bool, found {}", other.type_name()),
    }
}

/// A repeated root: the kernel lists it once and does *not* report a
/// multiplicity, so `multiplicities` is empty — "not distinguished", which
/// §8.6 keeps distinct from a reported `1`. Asserting `[2]` would be a
/// fiction: the solver never computed it.
#[test]
fn a_repeated_root_reports_no_multiplicity() {
    let solutions = solutions_of("let s = solve(fn t => (t - 1)^2)\n", "s");
    assert_eq!(integer_roots(&solutions), vec![1]);
    assert!(
        field_elements(&solutions, "multiplicities").is_empty(),
        "empty means the solver did not distinguish the roots"
    );
    assert!(bool_field(&solutions, "complete"));
}

/// When the solver's answer is a single term rather than a list, the kernel
/// does not claim completeness, and the bridge passes that claim through
/// instead of defaulting it. (The term happens to be a `list(...)` the kernel
/// cannot split into roots — see the report; the flag is still the solver's.)
#[test]
fn an_answer_that_is_not_a_list_does_not_claim_completeness() {
    let solutions = solutions_of("let s = solve(fn t => sin(t) - 1/2)\n", "s");
    assert!(
        !bool_field(&solutions, "complete"),
        "`complete` must be the solver's claim, not a constant"
    );
}

#[test]
fn sum_over_a_range_is_a_runtime_overload() {
    // §8.5: `sum(f, a, b)` is the calculus overload. The checker does not
    // offer the scheme yet (see the report), so the runtime arm is exercised
    // through the builtin entry point.
    use aevia_core::value::NativeFn;
    use std::rc::Rc;
    let mut interp = Interp::new(&aevia_types::CheckedModule::default());
    let square = Value::Native(Rc::new(NativeFn(Box::new(|_interp, args, _span| {
        let n = args.first().and_then(Value::as_f64).unwrap_or(0.0) as i64;
        Ok(Value::int(n * n))
    }))));
    let total = interp
        .call_named(
            "sum",
            vec![square, Value::int(1), Value::int(4)],
            aevia_syntax::Span::default(),
        )
        .expect("sums the range");
    assert_eq!(aevia_core::value::repr(&total), "30");

    // An empty range is the zero of the sum, not an error.
    let zero = interp
        .call_named(
            "sum",
            vec![
                Value::Native(Rc::new(NativeFn(Box::new(|_interp, _args, _span| {
                    Ok(Value::int(0))
                })))),
                Value::int(5),
                Value::int(4),
            ],
            aevia_syntax::Span::default(),
        )
        .expect("an empty range sums to zero");
    assert_eq!(aevia_core::value::repr(&zero), "0");
}

// --- item 8: recursion and stack safety ------------------------------------

#[test]
fn a_flat_do_block_does_not_use_the_rust_stack() {
    // 20k statements: the statement loop is iterative and the depth guard
    // counts calls, so this is a legal program (§9.3.2).
    with_cli_stack(|| {
        let mut source = String::from("fn main(): IO Unit = do\n");
        for _ in 0..20_000 {
            source.push_str("    println(\"x\")\n");
        }
        let mut interp = interp(&source);
        interp.run_main().expect("a flat block is legal");
        assert_eq!(interp.captured().lines().count(), 20_000);
    });
}

#[test]
fn a_flat_arithmetic_chain_is_not_a_stack_overflow() {
    // The old guard counted AST nodes, so 2100 terms were rejected as `E0805`
    // even though the stack could hold them.
    with_cli_stack(|| {
        let mut expression = String::from("1");
        for _ in 0..2_100 {
            expression.push_str(" + 1");
        }
        let source = format!("let a = {expression}\n");
        assert_eq!(repr(&source, "a"), "2101");
    });
}

#[test]
fn a_runaway_recursion_is_e0805_and_carries_a_trace() {
    with_cli_stack(|| {
    // A tight budget on purpose. The default is far above any legal program, so
    // a runaway loop would take minutes to reach it; the guarantee under test
    // is "the budget stops a loop that never terminates", not "the default is
    // small".
    let mut interp = interp(
        r#"fn down(n: Int): Int = down(n + 1)

fn main(): IO Unit = do
    println(down(0))
"#,
    );
    interp.max_tail_calls = 10_000;
    let error = match interp.run_main() {
        Ok(_) => panic!("expected the budget to stop the loop"),
        Err(error) => error,
    };
    assert_eq!(error.code, "E0805");
    assert!(
        error.stack.iter().any(|frame| frame.function == "down"),
        "the error must carry a call stack: {:?}",
        error.stack
    );
    assert!(
        error.message.contains("tail calls"),
        "a budget stop must not be reported as a stack overflow: {}",
        error.message
    );
    });
}

#[test]
fn the_tail_budget_is_a_guard_not_a_limit() {
    // The default used to be 1.5 M, so a legal 1.6 M iteration count was
    // rejected — a semantic limit wearing a safety net's clothes. Two things
    // are asserted: the default is far above what a program does, and the
    // budget itself still stops a loop that never terminates at exactly the
    // bound it is given.
    with_cli_stack(|| {
        let source = r#"fn count(n: Int, acc: Int): Int =
    if n == 0 then acc else count(n - 1, acc + 1)

fn main(): IO Unit = do
    println(count(2000, 0))
"#;
        let mut generous = interp(source);
        assert!(
            generous.max_tail_calls > 1_600_000,
            "the default budget ({}) must not reject a legal program",
            generous.max_tail_calls
        );
        generous.max_tail_calls = 5000;
        generous.run_main().expect("2000 tail calls are within 5000");
        assert_eq!(generous.captured().trim(), "2000");

        let mut tight = interp(source);
        tight.max_tail_calls = 1000;
        let error = match tight.run_main() {
            Ok(_) => panic!("expected the budget to stop the loop"),
            Err(error) => error,
        };
        assert_eq!(error.code, "E0805");
        assert!(
            error.message.contains("1000 tail calls"),
            "the message must name the bound it enforced: {}",
            error.message
        );
    });
}

#[test]
fn prelude_constants_are_values_not_names() {
    // `pi` used to be a constructor that printed "pi" and failed in `2 * pi`.
    // The constant has to be symbolic, because no exact representation exists
    // and a float `pi` would make `2 * pi` inexact.
    let pi = num("let p = pi", "p");
    assert!(
        matches!(pi.kind, NumKind::Sym(_)),
        "pi must stay symbolic, found {:?}",
        pi.kind
    );
    let twice = num("let p = 2 * pi", "p");
    assert!(
        matches!(twice.kind, NumKind::Sym(_)),
        "2 * pi must stay symbolic, found {:?}",
        twice.kind
    );
    // ... and it must still be usable as a number when one is asked for.
    let output = run(
        r#"fn main(): IO Unit = do
    println("{approx(2 * pi)}")
"#,
    );
    let value: f64 = output
        .trim()
        .trim_start_matches("Ok(")
        .trim_end_matches(')')
        .parse()
        .expect("approx yields a number");
    assert!((value - std::f64::consts::TAU).abs() < 1e-12, "got {value}");
}

#[test]
fn nan_and_i_have_the_types_the_checker_promises() {
    let nan = num("let n = nan", "n");
    assert!(matches!(nan.kind, NumKind::Float(value) if value.is_nan()));
    let unit_imaginary = num("let j = i", "j");
    assert!(
        matches!(unit_imaginary.kind, NumKind::Complex(re, im) if re == 0.0 && im == 1.0)
    );
}

