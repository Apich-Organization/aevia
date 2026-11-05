//! Clause-by-clause tests for chapter 06 (evaluation semantics) plus the
//! chapter-09 guarantees those clauses rest on: the call stack of §9.3.4, the
//! tensor runtime of §9.5 and the empty/zero-size edges of §6.6.
//!
//! Every assertion is about a semantic observable — the phase of a scalar
//! (`NumKind`), the storage class of a tensor (`Storage`), a shape, a
//! diagnostic code, a call-stack frame — not about a rendered string. `repr`
//! appears only where `repr` itself is the subject.
//!
//! The tests are grouped by clause so a failure names the clause that broke.

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::compile;
use aevia_core::value::NumKind;
use aevia_core::value::Storage;
use aevia_core::value::Value;
use aevia_core::value::kahan_sum;
use aevia_syntax::Span;

// ---------------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------------

fn build(source: &str) -> (aevia_syntax::Module, aevia_types::CheckedModule) {
    let compilation = compile("chapter6.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "compile errors in:\n{source}\n{:?}",
        compilation
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    (
        compilation.module.expect("a parsed module"),
        compilation.checked,
    )
}

fn interp(source: &str) -> Interp {
    let (module, checked) = build(source);
    let mut interpreter = Interp::new(&checked).with_buffer();
    let diagnostics = interpreter.load(&module);
    assert!(
        !diagnostics.has_errors(),
        "load errors in:\n{source}\n{:?}",
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    interpreter
}

/// The first diagnostic a program raises while loading (top-level `let`s are
/// eager, so a runtime failure in one is reported at load time).
fn load_error(source: &str) -> (String, String) {
    let compilation = compile("chapter6.aev", source);
    let (module, checked) = if compilation.diagnostics.has_errors() {
        (
            compilation.module.expect("a parsed module"),
            compilation.checked,
        )
    } else {
        (
            compilation.module.expect("a parsed module"),
            compilation.checked,
        )
    };
    let mut interpreter = Interp::new(&checked).with_buffer();
    let diagnostics = interpreter.load(&module);
    assert!(
        diagnostics.has_errors(),
        "expected a diagnostic from:\n{source}"
    );
    let first = &diagnostics.items[0];
    (first.code.clone(), first.message.clone())
}

fn compile_errors(source: &str) -> Vec<String> {
    compile("chapter6.aev", source)
        .diagnostics
        .items
        .iter()
        .map(|item| item.code.clone())
        .collect()
}

/// The part of an `EvalError` these tests inspect, in a `Send` form so the
/// program can run on the CLI's stack (see [`run_error`]).
struct Failure {
    code: &'static str,
    #[allow(dead_code)]
    message: String,
    /// `(function name, call-site byte offset)`, outermost first.
    stack: Vec<(String, u32)>,
}

/// Runs `main` and returns the runtime error it raised.
///
/// The program runs on the stack the CLI gives the interpreter (§9.9: 256 MiB),
/// because §6.9.3 judges a legal program against the language's limit, not the
/// test harness's.
fn run_error(source: &str) -> Failure {
    let source = source.to_string();
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            let mut interpreter = interp(&source);
            match interpreter.run_main() {
                | Ok(_) => panic!("expected a runtime error from:\n{source}"),
                | Err(error) => {
                    Failure {
                        code: error.code,
                        message: error.message,
                        stack: error
                            .stack
                            .iter()
                            .map(|frame| (frame.function.to_string(), frame.span.start))
                            .collect(),
                    }
                },
            }
        })
        .expect("spawn")
        .join()
        .expect("the test body panicked")
}

fn value(
    source: &str,
    name: &str,
) -> Value {
    let mut interpreter = interp(source);
    interpreter.eval_toplevel(name).expect("the name is bound")
}

fn num(
    source: &str,
    name: &str,
) -> aevia_core::value::NumVal {
    match value(source, name) {
        | Value::Num(number) => number,
        | other => panic!("expected a scalar, found {}", other.type_name()),
    }
}

fn tensor_value(
    source: &str,
    name: &str,
) -> std::rc::Rc<aevia_core::value::TensorData> {
    match value(source, name) {
        | Value::Tensor(tensor) => tensor,
        | other => panic!("expected a tensor, found {}", other.type_name()),
    }
}

fn float_of(value: &Value) -> f64 {
    value.as_f64().expect("a number")
}

// ---------------------------------------------------------------------------
// §6.1 — strict, left-to-right evaluation
// ---------------------------------------------------------------------------

/// §6.1: "二元运算符先求左操作数，再求右操作数." The only observable is which
/// failure happens first, so each operand fails with a *different* code.
#[test]
fn binary_operands_evaluate_left_to_right() {
    let error = run_error(
        r#"fn main(): IO Unit = do
    let x = (1 / 0) + zeros(2)[5]
    println(show(x))
"#,
    );
    assert_eq!(error.code, "E0801", "the left operand failed first");
}

/// §6.1: "函数参数在调用前求值" and the arguments are evaluated in order.
#[test]
fn arguments_evaluate_left_to_right() {
    let error = run_error(
        r#"fn add(a: Real, b: Float): Real = a + b

fn main(): IO Unit = do
    let x = add(1 / 0, zeros(2)[5])
    println(show(x))
"#,
    );
    assert_eq!(error.code, "E0801", "the first argument failed first");
}

/// §6.1: "记录字段按书写顺序求值." Written `b` before `a` is not the same as
/// sorted `a`/`b` when only the order of failures is observable.
#[test]
fn record_fields_evaluate_in_written_order() {
    let error = run_error(
        r#"fn main(): IO Unit = do
    let r = { b = zeros(2)[5], a = 1 / 0 }
    println("done")
"#,
    );
    assert_eq!(error.code, "E0802", "the field written first failed first");
}

/// §6.1: "`if` 只求值被选中的分支."
#[test]
fn if_evaluates_only_the_selected_branch() {
    let number = num("let x = if true then zeros(2)[0] else zeros(2)[5]\n", "x");
    assert!(matches!(number.kind, NumKind::Float(value) if value == 0.0));
}

/// §6.1: "`&&`/`||` 在条件可判定时短路."
#[test]
fn and_and_or_short_circuit() {
    let source = r#"let a = false && (zeros(2)[5] == 1.0)
let b = true || (zeros(2)[5] == 1.0)
let c = true && (zeros(2)[0] == 0.0)
"#;
    assert!(matches!(value(source, "a"), Value::Bool(false)));
    assert!(matches!(value(source, "b"), Value::Bool(true)));
    assert!(matches!(value(source, "c"), Value::Bool(true)));
}

/// §6.1: "`match` 只求值匹配的分支." The arm that is not taken indexes out of
/// bounds, which would be `E0802` if it ran.
#[test]
fn match_evaluates_only_the_matching_arm() {
    let source = r#"let x = match Some(1) with
    | Some(value) -> value
    | None -> to_int(zeros(2)[5])
"#;
    assert!(matches!(
        value(source, "x"),
        Value::Num(number) if matches!(number.kind, NumKind::Int(_))
    ));
}

/// §6.1: "张量元素按行优先顺序求值."
#[test]
fn tensor_elements_evaluate_in_row_major_order() {
    // The first failing element is row 1, column 0; if the flat tail were
    // evaluated first the diagnostic would be the `asin(2.0)` domain `E0806`.
    let error = run_error(
        r#"fn main(): IO Unit = do
    let t = [[1.0, 2.0], [zeros(2)[5], asin(2.0)]]
    println(show(t))
"#,
    );
    assert_eq!(
        error.code, "E0802",
        "row-major: the earlier element fails first"
    );
}

/// §6.1: "`let` 的右侧立即求值，然后绑定."
#[test]
fn a_let_binding_is_eager() {
    let error = run_error(
        r#"fn main(): IO Unit = do
    let x = 1 / 0
    println(show(1))
"#,
    );
    assert_eq!(error.code, "E0801");
}

// ---------------------------------------------------------------------------
// §6.2 — values and closures
// ---------------------------------------------------------------------------

/// §6.2.2: a closure captures the lexical environment it was created in, so a
/// later shadowing binding in the same block does not change it.
#[test]
fn a_closure_keeps_the_binding_it_captured() {
    let mut interpreter = interp(
        r#"fn main(): IO Unit = do
    let x = 1
    let f = fn () => x
    let x = 2
    println(show(f()))
"#,
    );
    interpreter.run_main().expect("main runs");
    assert_eq!(interpreter.captured(), "1\n");
}

/// §6.2.2: "递归函数是值，不是特殊形式." A local `fn` is a value and can be
/// passed as one.
#[test]
fn a_recursive_function_is_a_value() {
    let source = r#"fn factorial(n: Int): Int =
    if n <= 1 then 1 else n * factorial(n - 1)

let apply_twice = fn (f, x) => f(f(x))
let six = apply_twice(fn (n) => n + 3, 0)
let fact4 = factorial(4)
"#;
    assert_eq!(float_of(&value(source, "six")), 6.0);
    assert_eq!(float_of(&value(source, "fact4")), 24.0);
}

// ---------------------------------------------------------------------------
// §6.3 — exact, float and symbolic phases
// ---------------------------------------------------------------------------

/// §6.3.2: "`Int / Int` 产生 `Exact` 有理数，不产生 `Float`."
#[test]
fn integer_division_is_an_exact_rational() {
    let number = num("let third = 1 / 3\n", "third");
    assert!(
        matches!(&number.kind, NumKind::Exact(_)),
        "1/3 must be Exact, found {}",
        aevia_core::value::repr(&Value::Num(number.clone()))
    );
    assert_eq!(aevia_core::value::repr(&Value::Num(number)), "1/3");
}

/// §6.3.2's exactness matrix, as observable values:
///
/// * `Int ⊗ Int` stays an integer;
/// * `Float ⊗ Exact` is the exact rational the float denotes (`1.0 + 2` is
///   exactly `3`, not a float `3.0`);
/// * a fraction plus a terminating decimal is the exact sum (`1/3 + 0.5` is
///   exactly `5/6`);
/// * a `Float` literal is a float, and a `Float`-typed binding keeps it.
///
/// This also pins the IR literal representation after floats are parsed once at
/// lowering: the phases a literal produces are unchanged.
#[test]
fn the_exactness_matrix_keeps_its_phases() {
    let integers = num("let x = 1 + 2\n", "x");
    assert!(matches!(integers.kind, NumKind::Int(_)), "1 + 2 is an Int");

    let mixed = num("let x = 1.0 + 2\n", "x");
    assert!(
        matches!(&mixed.kind, NumKind::Exact(_)),
        "Float ⊗ Exact is Real, found {}",
        aevia_core::value::repr(&Value::Num(mixed.clone()))
    );
    assert_eq!(aevia_core::value::repr(&Value::Num(mixed)), "3");

    let fractions = num("let x = 1 / 3 + 0.5\n", "x");
    assert!(matches!(&fractions.kind, NumKind::Exact(_)));
    assert_eq!(aevia_core::value::repr(&Value::Num(fractions)), "5/6");

    let decimal = num("let x = 1.5\n", "x");
    assert!(matches!(decimal.kind, NumKind::Float(value) if value == 1.5));
    let annotated = num("let x: Float = 1.5\n", "x");
    assert!(matches!(annotated.kind, NumKind::Float(value) if value == 1.5));

    // §3.10.1: a literal takes the representation of its context. An `Int`
    // literal in a `Float` slot is a float at run time, so the static type and
    // the value agree (the measured soundness hole this table closes).
    let coerced_int = num("let y: Float = 1\n", "y");
    assert!(
        matches!(coerced_int.kind, NumKind::Float(value) if value == 1.0),
        "`let y: Float = 1` must be a Float at run time"
    );
    let coerced_sum = num("fn f(x: Float): Float = x + 1\nlet r = f(2.0)\n", "r");
    assert!(
        matches!(coerced_sum.kind, NumKind::Float(value) if value == 3.0),
        "`2.0 + 1` in a Float context is a Float 3.0, found {}",
        aevia_core::value::repr(&Value::Num(coerced_sum))
    );
    let coerced_quantity = num("let v: Float[L] = 3 m\n", "v");
    assert!(
        matches!(coerced_quantity.kind, NumKind::Float(value) if value == 3.0),
        "a unit literal in a Float slot is a float"
    );
}

/// §6.3.2: exact division by zero is `E0801`; the float path is IEEE.
#[test]
fn division_by_zero_depends_on_the_phase() {
    assert_eq!(load_error("let x = 1 / 0\n").0, "E0801");
    assert_eq!(load_error("let x = 0 / 0\n").0, "E0801");

    let infinite = num("let x = 1.0 / 0.0\n", "x");
    assert!(matches!(infinite.kind, NumKind::Float(value) if value.is_infinite() && value > 0.0));
    let not_a_number = num("let x = 0.0 / 0.0\n", "x");
    assert!(matches!(not_a_number.kind, NumKind::Float(value) if value.is_nan()));
}

/// §6.3.4: a negative `Real` root is a formal term, never a float, and
/// `approx` of a formal expression outside its domain is `E0806` (through the
/// `Result`, which is how the recoverable class of §6.9.2 travels).
#[test]
fn a_formal_root_outside_its_domain_is_e0806_on_approximation() {
    let root = num("let x = sqrt(-1)\n", "x");
    assert!(
        matches!(&root.kind, NumKind::Sym(_)),
        "sqrt(-1) stays formal"
    );
    let approximate = value("let r = approx(sqrt(-1))\n", "r");
    match approximate {
        | Value::Variant(result) => {
            assert_eq!(result.index, 1, "approx answers `Err`");
            let payload = aevia_core::value::show(&result.fields[0]);
            assert!(
                payload.contains("E0806"),
                "the payload names the domain code: {payload}"
            );
        },
        | other => panic!("approx answers a Result, found {}", other.type_name()),
    }
    // The `Float` phase reports the domain error directly.
    assert_eq!(load_error("let x = asin(2.0)\n").0, "E0806");
}

/// §6.3.5: "`simplify` 是 `eval` 的同义词." Both simplify the same symbolic
/// term to the same value.
#[test]
fn eval_and_simplify_are_the_same_operation() {
    let source = r#"let x = symbol("x")
let a = eval((x + 0) * 1)
let b = simplify((x + 0) * 1)
"#;
    let a = value(source, "a");
    let b = value(source, "b");
    // Symbolic values are identities, so their semantic equality is their
    // term; the canonical rendering is the only handle on that here.
    assert_eq!(
        aevia_core::value::repr(&a),
        aevia_core::value::repr(&b),
        "eval and simplify disagree"
    );
}

/// §6.3.5: `approx` needs an expression without free symbols; a free symbol is
/// the recoverable `E0803` class (carried in the `Result`, §6.9.2).
#[test]
fn approx_of_a_free_symbol_is_an_error_result() {
    let approximate = value("let x = symbol(\"x\")\nlet r = approx(x)\n", "r");
    let Value::Variant(result) = approximate else {
        panic!("approx answers a Result")
    };
    assert_eq!(result.index, 1, "approx of a free symbol is `Err`");
}

/// §6.3.6: `symbol("x")` twice is two symbols; `subst` replaces by identity.
#[test]
fn symbols_are_fresh_and_subst_uses_identity() {
    let source = r#"let x = symbol("x")
let y = symbol("x")
let e = x + y
let replaced = subst(x, 1, e)
let resolved = subst(y, 2, replaced)
let untouched = subst(x, 2, y)
"#;
    // Replacing the first symbol leaves the second free; replacing the second
    // then gives a closed expression, which folds to a number.
    let resolved = num(source, "resolved");
    assert!(
        matches!(&resolved.kind, NumKind::Int(_) | NumKind::Exact(_)),
        "both symbols replaced, so the value is exact"
    );
    assert_eq!(float_of(&Value::Num(resolved)), 3.0);
    // The two `symbol("x")` calls are distinct, so `subst(x, …)` does not touch
    // `y`'s term.
    let untouched = num(source, "untouched");
    assert!(
        matches!(untouched.kind, NumKind::Sym(_)),
        "subst(x, …) must not touch the other symbol"
    );
}

/// §6.3.6 / §6.12.1: symbolic equality is a proposition, and `if` refuses it.
#[test]
fn symbolic_equality_is_a_proposition() {
    let proposition = value("let x = symbol(\"x\")\nlet p = x == x\n", "p");
    assert!(matches!(proposition, Value::Prop(_)));
    let errors = compile_errors(
        r#"let x = symbol("x")
let p = x == x
let y = if p then 1 else 2
"#,
    );
    assert!(
        errors.contains(&"E0302".to_string()),
        "a Prop in `if` is E0302, got {errors:?}"
    );
}

/// §6.12.1 and §8.1.2: an exact comparison is a `Prop` statically, and
/// `decide` turns it into a concrete `Bool` — even though type erasure leaves
/// the runtime holding the two exact values.
#[test]
fn decide_decides_an_exact_comparison() {
    let decided = value(
        r#"let third = 1 / 3
let d = decide(third == third)
"#,
        "d",
    );
    let Value::Variant(option) = decided else {
        panic!("decide answers an Option")
    };
    assert_eq!(option.index, 1, "Some");
    assert!(matches!(option.fields[0], Value::Bool(true)));
}

// ---------------------------------------------------------------------------
// §6.5 — pattern matching
// ---------------------------------------------------------------------------

/// §6.5: "按分支顺序尝试；第一个匹配的分支胜出."
#[test]
fn matching_takes_the_first_accepted_arm() {
    let source = r#"let a = match 2 with
    | 1 -> 10
    | 2 -> 20
    | _ -> 30
let b = match 2 with
    | n if n > 5 -> 1
    | n -> n
"#;
    assert_eq!(float_of(&value(source, "a")), 20.0);
    assert_eq!(
        float_of(&value(source, "b")),
        2.0,
        "the guard rejects the first arm"
    );
}

/// §6.5: a tensor pattern matches only rank 1, with at least the written
/// elements, and `..rest` binds the remaining elements as a slice.
#[test]
fn tensor_patterns_match_rank_one_only() {
    let source = r#"let ys = match [1, 2, 3] with
    | [a, b, ..rest] -> rest
    | _ -> [0]
let other = match zeros(2, 2) with
    | [a, b, ..rest] -> [1]
    | _ -> [2]
"#;
    let rest = tensor_value(source, "ys");
    assert_eq!(
        rest.elements().iter().map(float_of).collect::<Vec<_>>(),
        vec![3.0]
    );
    let other = tensor_value(source, "other");
    assert_eq!(
        other.elements().iter().map(float_of).collect::<Vec<_>>(),
        vec![2.0],
        "a rank-2 tensor does not match a rank-1 pattern"
    );
}

/// §6.5: a float literal is not a legal pattern; the parser rejects it.
#[test]
fn a_float_literal_is_not_a_pattern() {
    let errors = compile_errors(
        r#"let y = match 1.0 with
    | 1.0 -> 1
    | _ -> 2
"#,
    );
    assert!(
        !errors.is_empty(),
        "a float literal pattern must not compile"
    );
}

/// §6.5: a non-exhaustive match is `E0309` at compile time, so a program can
/// never fall out of a match at run time.
#[test]
fn a_non_exhaustive_match_is_rejected_at_compile_time() {
    let errors = compile_errors(
        r#"type Shape =
    | Circle(Real)
    | Rect(Real, Real)

fn area(s: Shape): Real =
    match s with
    | Circle(r) -> r
"#,
    );
    assert!(errors.contains(&"E0309".to_string()), "got {errors:?}");
}

// ---------------------------------------------------------------------------
// §6.6 — the tensor runtime
// ---------------------------------------------------------------------------

/// §6.6.1: "混合类型在构造时提升到最宽的存储；`[1, 2.0]` 用 `F64`."
#[test]
fn a_mixed_numeric_literal_promotes_to_f64_storage() {
    let tensor = tensor_value("let t = [1, 2.0]\n", "t");
    assert!(
        matches!(tensor.storage, Storage::Dense(_)),
        "a mixed Int/Float literal is the widest storage (F64)"
    );
    assert_eq!(
        tensor.elements().iter().map(float_of).collect::<Vec<_>>(),
        vec![1.0, 2.0],
        "the integer element is promoted into the f64 buffer"
    );
    // A homogeneous literal keeps the class its elements imply.
    assert!(matches!(
        tensor_value("let t = [1, 2]\n", "t").storage,
        Storage::Exact(_)
    ));
}

/// §6.6.6: "`sum` 空张量 = 0；`prod` 空张量 = 1", in the phase of the element
/// storage; the mean of an empty `Float` tensor is `NaN`, of an empty exact
/// tensor `E0802`.
#[test]
fn empty_tensor_reductions_follow_the_element_phase() {
    let float_sum = num("let x = sum(zeros(0))\n", "x");
    assert!(matches!(float_sum.kind, NumKind::Float(value) if value == 0.0));
    let float_prod = num("let x = prod(zeros(0))\n", "x");
    assert!(matches!(float_prod.kind, NumKind::Float(value) if value == 1.0));
    let exact_sum = num("let x = sum(range(0, 0))\n", "x");
    assert!(matches!(
        exact_sum.kind,
        NumKind::Int(_) | NumKind::Exact(_)
    ));
    let exact_prod = num("let x = prod(range(0, 0))\n", "x");
    assert!(matches!(
        exact_prod.kind,
        NumKind::Int(_) | NumKind::Exact(_)
    ));

    let float_mean = num("let x = mean(zeros(0))\n", "x");
    assert!(
        matches!(float_mean.kind, NumKind::Float(value) if value.is_nan()),
        "the mean of an empty Float tensor is NaN"
    );
    assert_eq!(
        load_error("let x = mean(range(0, 0))\n").0,
        "E0802",
        "the mean of an empty Real tensor is E0802"
    );
}

/// §6.6.6 / §9.5: "`sum` 的实现对 `F64` 用 pairwise/Kahan 求和."
#[test]
fn float_sums_are_compensated() {
    // A left-to-right fold rounds the small terms away: ten tenths sum to
    // 0.9999999999999999, and `1e16 + 1 - 1e16` folds to `0`. The compensated
    // sum is the correctly-rounded 1 in both cases.
    let tenths = num(
        "let x = sum([0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1])\n",
        "x",
    );
    assert!(
        matches!(&tenths.kind, NumKind::Float(value) if *value == 1.0),
        "expected the compensated sum 1.0, found {}",
        aevia_core::value::repr(&Value::Num(tenths))
    );
    let mixed = num("let x = sum([1.0e16, 1.0, -1.0e16])\n", "x");
    assert!(
        matches!(&mixed.kind, NumKind::Float(value) if (*value - 1.0).abs() < 1e-9),
        "expected the compensated sum 1.0, found {}",
        aevia_core::value::repr(&Value::Num(mixed))
    );
    // The compensation is the one the runtime exposes, so the two cannot drift.
    assert!((kahan_sum(&[1.0e16, 1.0, -1.0e16]) - 1.0).abs() < 1e-9);
    assert_eq!(kahan_sum(&[0.1; 10]), 1.0);
    // The mean is the same sum divided by the count.
    let mean = num("let x = mean([1.0e16, 1.0, -1.0e16])\n", "x");
    assert!(matches!(&mean.kind, NumKind::Float(value) if (*value - 1.0 / 3.0).abs() < 1e-9));
}

/// §6.6.3: same-rank broadcasting, and the error names both shapes and the
/// operation (§9.5: "错误包含两边形状和运算名").
#[test]
fn broadcasting_and_shape_errors() {
    let broadcast = tensor_value("let t = ones(2, 1) * ones(1, 3)\n", "t");
    assert_eq!(broadcast.shape, vec![2, 3]);
    assert_eq!(
        broadcast
            .elements()
            .iter()
            .map(float_of)
            .collect::<Vec<_>>(),
        vec![1.0; 6]
    );

    let (code, message) = load_error("let t = zeros(2, 3) + zeros(3, 2)\n");
    assert_eq!(code, "E0802");
    assert!(
        message.contains("`+`") || message.contains("+"),
        "the message names the operation: {message}"
    );
    assert!(
        message.contains("2, 3") && message.contains("3, 2"),
        "the message names both shapes: {message}"
    );

    // §6.6.3: different ranks are a compile error unless `reshape` is explicit.
    let errors = compile_errors("let t = zeros(2, 3) + zeros(3)\n");
    assert!(
        !errors.is_empty(),
        "rank mismatch must not compile: {errors:?}"
    );
}

/// §6.6.4: negative indices count from the end; out of range is `E0802`.
#[test]
fn indexing_and_slicing() {
    let last = num("let a = [10.0, 20.0, 30.0]\nlet x = a[-1]\n", "x");
    assert!(matches!(last.kind, NumKind::Float(value) if value == 30.0));
    assert_eq!(load_error("let a = zeros(2)\nlet x = a[5]\n").0, "E0802");
}

/// §6.6.4: "`a[i]` 返回子张量视图（rank 减一）", and a slice of a dense buffer is
/// a view.
#[test]
fn indexing_a_matrix_axis_yields_a_view() {
    let source = "let m = zeros(2, 3)\nlet row = m[1]\n";
    let row = tensor_value(source, "row");
    assert_eq!(row.shape, vec![3]);
    assert!(row.is_view(), "m[1] is a view of m's buffer");
}

/// §6.6.4 / §9.5: "`reshape` 在连续内存上返回视图，否则复制." A contiguous
/// window (`m[1..3, 0..3]`) is a view after a reshape; a strided one
/// (`m[0..3, 1..3]`) is materialised.
#[test]
fn reshape_is_a_view_only_when_the_window_is_contiguous() {
    let mut interpreter = interp(
        r#"let m = zeros(3, 3)
let contiguous = m[1..3, 0..3]
let strided = m[0..3, 1..3]
"#,
    );
    let source = interpreter.eval_toplevel("m").expect("m");
    let Value::Tensor(source) = &source else {
        panic!("m is a tensor")
    };
    let contiguous = interpreter.eval_toplevel("contiguous").expect("contiguous");
    let Value::Tensor(contiguous) = &contiguous else {
        panic!("the slice is a tensor")
    };
    assert!(contiguous.is_contiguous(), "the row window is contiguous");

    let shape = aevia_syntax::ast::Ty::new(
        aevia_syntax::ast::TyKind::ShapeArg(aevia_syntax::ast::ShapeAst::Refined(vec![
            aevia_syntax::ast::ShapeItem::Known(aevia_syntax::ast::NatAst::Lit(3)),
            aevia_syntax::ast::ShapeItem::Known(aevia_syntax::ast::NatAst::Lit(2)),
        ])),
        Span::default(),
    );
    let reshaped = aevia_core::builtins::call_typed(
        &mut interpreter,
        "reshape",
        &[shape],
        vec![aevia_core::value::Value::Tensor(contiguous.clone())],
        Span::default(),
    )
    .expect("a 2x3 window has six elements");
    let Value::Variant(result) = &reshaped else {
        panic!("reshape answers a Result")
    };
    assert_eq!(result.index, 0);
    let Value::Tensor(reshaped) = &result.fields[0] else {
        panic!("the Ok payload is a tensor")
    };
    assert_eq!(reshaped.shape, vec![3, 2]);
    assert!(
        reshaped.buffer().is_some() && source.buffer().is_some(),
        "both tensors are f64"
    );
    assert!(
        std::rc::Rc::ptr_eq(
            reshaped.buffer().expect("reshaped buffer"),
            source.buffer().expect("source buffer")
        ),
        "a contiguous reshape is a view of the same buffer, not a copy"
    );

    // The strided window cannot be a view: the warp of a reshape has no
    // strides that describe it, so it is copied in row-major order.
    let strided = interpreter.eval_toplevel("strided").expect("strided");
    let Value::Tensor(strided) = &strided else {
        panic!("the slice is a tensor")
    };
    let shape = aevia_syntax::ast::Ty::new(
        aevia_syntax::ast::TyKind::ShapeArg(aevia_syntax::ast::ShapeAst::Refined(vec![
            aevia_syntax::ast::ShapeItem::Known(aevia_syntax::ast::NatAst::Lit(3)),
            aevia_syntax::ast::ShapeItem::Known(aevia_syntax::ast::NatAst::Lit(2)),
        ])),
        Span::default(),
    );
    let reshaped = aevia_core::builtins::call_typed(
        &mut interpreter,
        "reshape",
        &[shape],
        vec![aevia_core::value::Value::Tensor(strided.clone())],
        Span::default(),
    )
    .expect("3x2 window");
    let Value::Variant(result) = &reshaped else {
        panic!("reshape answers a Result")
    };
    let Value::Tensor(reshaped) = &result.fields[0] else {
        panic!("the Ok payload is a tensor")
    };
    assert!(
        !std::rc::Rc::ptr_eq(
            reshaped.buffer().expect("materialised buffer"),
            source.buffer().expect("source buffer")
        ),
        "a strided reshape materialises"
    );
}

/// §6.6.4: "`@` 按秩分派: 2×2 → matmul, 2×1 → matvec, 1×2 → vecmat, 1×1 → dot."
#[test]
fn matrix_operator_dispatches_on_rank() {
    let source = r#"let a = [[1.0, 2.0], [3.0, 4.0]]
let v = [1.0, 1.0]
let dot = v @ v
let matvec = a @ v
let vecmat = v @ a
let matmul = a @ a
"#;
    assert!(matches!(
        value(source, "dot"),
        Value::Num(number) if matches!(number.kind, NumKind::Float(value) if value == 2.0)
    ));
    assert_eq!(tensor_value(source, "matvec").shape, vec![2]);
    assert_eq!(tensor_value(source, "vecmat").shape, vec![2]);
    assert_eq!(tensor_value(source, "matmul").shape, vec![2, 2]);
    assert_eq!(
        tensor_value(source, "matmul")
            .elements()
            .iter()
            .map(float_of)
            .collect::<Vec<_>>(),
        vec![7.0, 10.0, 15.0, 22.0]
    );
}

/// §6.6.6: `var` defaults to the sample variance, `var(xs, true)` divides by
/// `n`, and `std` is `sqrt(var)`.
#[test]
fn variance_defaults_are_the_sample_variance() {
    let source = r#"let xs = [1.0, 2.0, 3.0]
let sample = var(xs)
let population = var(xs, true)
let deviation = std(xs)
"#;
    assert!((float_of(&value(source, "sample")) - 1.0).abs() < 1e-12);
    assert!(
        (float_of(&value(source, "population")) - 2.0 / 3.0).abs() < 1e-12,
        "the population variance divides by n"
    );
    assert!((float_of(&value(source, "deviation")) - 1.0).abs() < 1e-12);
}

/// §6.6.6: "`norm` 默认 2-范数；`norm(xs, p)` 支持 1、2、∞."
#[test]
fn norm_orders() {
    let source = r#"let xs = [3.0, 4.0]
let two = norm(xs)
let one = norm(xs, 1)
let many = norm(xs, 2)
"#;
    assert!((float_of(&value(source, "two")) - 5.0).abs() < 1e-12);
    assert!((float_of(&value(source, "one")) - 7.0).abs() < 1e-12);
    assert!((float_of(&value(source, "many")) - 5.0).abs() < 1e-12);
}

/// §6.6.6: `sum(a, axis)` keeps the rank (the axis becomes 1, keepdims) and
/// `sum(a)` answers a scalar. The checker does not yet type the two-argument
/// form, so the runtime arm is exercised through its entry point — the
/// program-level gap is reported to the Lead rather than hidden here.
#[test]
fn a_reduction_along_an_axis_keeps_the_rank() {
    let mut interpreter = interp("let t = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]\n");
    let tensor = interpreter.eval_toplevel("t").expect("t");
    let reduced = interpreter
        .call_named("sum", vec![tensor.clone(), Value::int(0)], Span::default())
        .expect("sum along axis 0");
    let Value::Tensor(reduced) = reduced else {
        panic!("the axis reduction is a tensor")
    };
    assert_eq!(reduced.shape, vec![1, 3], "keepdims holds the rank");
    assert_eq!(
        reduced.elements().iter().map(float_of).collect::<Vec<_>>(),
        vec![5.0, 7.0, 9.0]
    );
    let total = interpreter
        .call_named("sum", vec![tensor], Span::default())
        .expect("sum");
    assert_eq!(float_of(&total), 21.0);
    let last = interpreter.eval_toplevel("t").expect("t");
    let out_of_range = interpreter
        .call_named("sum", vec![last, Value::int(9)], Span::default())
        .expect_err("an out-of-range axis is E0802");
    assert_eq!(out_of_range.code, "E0802");
}

/// §6.6.4: `set` is a functional update — the source tensor is unchanged.
#[test]
fn set_is_a_functional_update() {
    let mut interpreter = interp("let a = [1.0, 2.0]\n");
    let source = interpreter.eval_toplevel("a").expect("a");
    let indices =
        aevia_core::eval::build_tensor(vec![Value::int(0)], vec![1], aevia_dim::Dim::one());
    let updated = interpreter
        .call_named(
            "set",
            vec![source.clone(), indices, Value::float(9.0)],
            Span::default(),
        )
        .expect("set");
    let Value::Tensor(updated) = updated else {
        panic!("set answers a tensor")
    };
    let Value::Tensor(source) = source else {
        panic!("a is a tensor")
    };
    assert_eq!(
        updated.elements().iter().map(float_of).collect::<Vec<_>>(),
        vec![9.0, 2.0]
    );
    assert_eq!(
        source.elements().iter().map(float_of).collect::<Vec<_>>(),
        vec![1.0, 2.0],
        "the source tensor is unchanged"
    );
}

// ---------------------------------------------------------------------------
// §6.7 / §6.12 — records, equality and ordering
// ---------------------------------------------------------------------------

/// §6.7: a record update produces a new record and shares the unchanged
/// fields — the nested tensor keeps its buffer.
#[test]
fn a_record_update_shares_the_untouched_fields() {
    let source = r#"let r = { xs = [1.0, 2.0], y = 2 }
let s = { r with y = 3 }
"#;
    let mut interpreter = interp(source);
    let Value::Record(before) = interpreter.eval_toplevel("r").expect("r") else {
        panic!("r is a record")
    };
    let Value::Record(after) = interpreter.eval_toplevel("s").expect("s") else {
        panic!("s is a record")
    };
    let Value::Tensor(left) = &before.fields["xs"] else {
        panic!("xs is a tensor")
    };
    let Value::Tensor(right) = &after.fields["xs"] else {
        panic!("xs is a tensor")
    };
    assert!(
        std::rc::Rc::ptr_eq(
            left.buffer().expect("left buffer"),
            right.buffer().expect("right buffer")
        ),
        "the untouched field is shared, not copied"
    );
    assert!(matches!(
        &after.fields["y"],
        Value::Num(number) if matches!(number.kind, NumKind::Int(_))
    ));
}

/// §6.12.1: records compare structurally — field order is not identity — and
/// `Float` compares by IEEE (`NaN != NaN`, `-0.0 == 0.0`).
#[test]
fn equality_is_structural_and_ieee_for_floats() {
    let source = r#"let a = { x = 1, y = 2 }
let b = { y = 2, x = 1 }
let same = a == b
let nan_equal = (0.0 / 0.0) == (0.0 / 0.0)
let zero_equal = 0.0 == (0.0 - 0.0)
let tensors = [1.0, 2.0] == [1.0, 2.0]
"#;
    assert!(matches!(value(source, "same"), Value::Bool(true)));
    assert!(matches!(value(source, "nan_equal"), Value::Bool(false)));
    assert!(matches!(value(source, "zero_equal"), Value::Bool(true)));
    assert!(matches!(value(source, "tensors"), Value::Bool(true)));
}

/// §6.12.2: `Float::compare` is `None` for NaN, and `TotalOrd` (used by `sort`)
/// puts NaN last; `Float` itself is not `TotalOrd` (§3.8.6), which is why the
/// sort entry point is the `TotalFloat`-wrapped `sort_float`.
#[test]
fn float_ordering_has_no_position_for_nan() {
    let source = r#"let x = compare(0.0 / 0.0, 1.0)
let sorted = sort_float([2.0, 0.0 / 0.0, 1.0], true)
"#;
    let Value::Variant(ordering) = value(source, "x") else {
        panic!("compare answers an Option")
    };
    assert_eq!(ordering.index, 0, "NaN has no ordering: None");
    let sorted = tensor_value(source, "sorted");
    let values: Vec<f64> = sorted.elements().iter().map(float_of).collect();
    assert_eq!(values[0], 1.0);
    assert_eq!(values[1], 2.0);
    assert!(
        values[2].is_nan(),
        "NaN sorts last under `TotalFloat`, got {values:?}"
    );
    // `Float` is not `TotalOrd`; `total_compare` on it is a static error.
    let errors = compile_errors("let y = total_compare(1.0, 2.0)\n");
    assert!(
        errors.contains(&"E0310".to_string()),
        "Float has no TotalOrd instance, got {errors:?}"
    );
}

// ---------------------------------------------------------------------------
// §6.8 — IO
// ---------------------------------------------------------------------------

/// §6.8.2: "`read_file` 返回 `Result`，不抛异常" — a missing file is the `Err`
/// payload of the action, and the program keeps running.
#[test]
fn a_failed_read_file_is_an_error_value_not_an_exception() {
    let mut interpreter = interp(
        r#"fn main(): IO Unit = do
    outcome <- read_file("/aevia-chapter-6-no-such-file")
    match outcome with
    | Ok(text) -> println("read {text}")
    | Err(_) -> println("failed")
"#,
    );
    interpreter.run_main().expect("main runs");
    assert_eq!(interpreter.captured(), "failed\n");
}

/// §6.8.2 / §9.3.4: `?` propagates the `Result` of the enclosing function. A
/// `main : IO Unit` is not a `Result` function, so a payload cannot escape the
/// `IO` block: that is `E0702`. The checker now proves it statically; the
/// runtime arm remains the defence for a value that reaches it by another path.
#[test]
fn a_question_mark_that_escapes_an_io_action_is_e0702() {
    let errors = compile_errors(
        r#"fn main(): IO Unit = do
    outcome <- read_file("/aevia-chapter-6-no-such-file")
    let text = outcome?
    println(text)
"#,
    );
    assert!(
        errors.contains(&"E0702".to_string()),
        "`?` cannot return from an `IO Unit` function, got {errors:?}"
    );
}

// ---------------------------------------------------------------------------
// §6.9 — budgets, errors and resources
// ---------------------------------------------------------------------------

/// §6.9.2: every recoverable error class the chapter names is reachable from a
/// program with the code it documents.
#[test]
fn the_error_classes_are_reachable() {
    // `E0801` exact division by zero.
    assert_eq!(load_error("let x = 1 / 0\n").0, "E0801");
    // `E0802` shape/index.
    assert_eq!(load_error("let x = zeros(2)[5]\n").0, "E0802");
    // `E0806` domain error in the `Float` phase.
    assert_eq!(load_error("let x = asin(2.0)\n").0, "E0806");
    // `E0808` budget: the summation budget is 10^6 terms (§6.9.1).
    assert_eq!(
        load_error("let x = sum(fn k => k, 0, 2000000)\n").0,
        "E0808"
    );
    // `E0805` an unbounded recursion.
    let error = run_error(
        r#"fn down(n: Int): Int = 1 + down(n + 1)

fn main(): IO Unit = do
    println(show(down(0)))
"#,
    );
    assert_eq!(error.code, "E0805");
    // `E0899` a failed assertion is fatal when its action runs.
    let error = run_error(
        r#"fn main(): IO Unit = do
    println("before")
    assert(1 == 2)
"#,
    );
    assert_eq!(error.code, "E0899");
}

// ---------------------------------------------------------------------------
// §9.3.4 — errors carry a stack with function names and call sites
// ---------------------------------------------------------------------------

/// §9.3.4: an `EvalError` carries `stack: Vec<Frame>` — function name plus
/// call site — for the functions that were active when the failure happened.
#[test]
fn a_runtime_error_carries_the_call_stack_and_its_call_sites() {
    let source = r#"fn blow(x: Int): Float = zeros(2)[x]
fn middle(x: Int): Float = 1.0 + blow(x)
fn outer(x: Int): Float = 2.0 + middle(x)

fn main(): IO Unit = do
    let y = outer(9)
    println(show(y))
"#;
    let error = run_error(source);
    assert_eq!(error.code, "E0802");
    let names: Vec<&str> = error.stack.iter().map(|(name, _)| name.as_str()).collect();
    assert!(
        names.len() >= 3,
        "the stack names the active calls, found {names:?}"
    );
    assert_eq!(
        &names[names.len() - 3..],
        &["outer", "middle", "blow"],
        "the trace is outermost-first and ends at the function that failed"
    );
    // Each frame records where *it* was called; this is the "call site" half
    // of §9.3.4's `Frame`.
    let expected = |needle: &str| source.find(needle).expect("call site in source") as u32;
    assert_eq!(
        error.stack[names.len() - 1].1,
        expected("blow(x)"),
        "the failing frame carries its call site"
    );
    assert_eq!(
        error.stack[names.len() - 2].1,
        expected("middle(x)"),
        "the middle frame carries its call site"
    );
}

// ---------------------------------------------------------------------------
// §6.10 — determinism
// ---------------------------------------------------------------------------

/// §6.10: "纯求值完全确定；相同输入得到相同输出." Evaluating the same program
/// twice gives the same values, including the compensated sums.
#[test]
fn pure_evaluation_is_deterministic() {
    let source = r#"let x = symbol("x")
let e = eval((x + 1) * (x - 1))
let s = sum([1.0e16, 1.0, -1.0e16])
let t = [1.0, 2.0, 3.0] @ [4.0, 5.0, 6.0]
"#;
    let first = (
        aevia_core::value::repr(&value(source, "e")),
        float_of(&value(source, "s")),
        float_of(&value(source, "t")),
    );
    let second = (
        aevia_core::value::repr(&value(source, "e")),
        float_of(&value(source, "s")),
        float_of(&value(source, "t")),
    );
    assert_eq!(first, second);
}
