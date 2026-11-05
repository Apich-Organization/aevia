//! Runtime glue for §3.5.3 (tensors and units), §4.9.3 (`Dyn` arithmetic) and
//! §8.9 (`odeint`'s flatten contract).
//!
//! Assertions are semantic: an element dimension, a storage class, a payload
//! value, a diagnostic code. `show`/`to` appear only where a display unit *is*
//! the observable.

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::compile;
use aevia_core::value::NumKind;
use aevia_core::value::Value;
use aevia_dim::Dim;
use aevia_syntax::Span;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------------

fn interp(source: &str) -> Interp {
    let compilation = compile("runtime_dim.aev", source);
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
    let module = compilation.module.expect("a parsed module");
    let mut interpreter = Interp::new(&compilation.checked).with_buffer();
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

fn value(
    source: &str,
    name: &str,
) -> Value {
    interp(source)
        .eval_toplevel(name)
        .expect("the name is bound")
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

fn tensor(
    source: &str,
    name: &str,
) -> Rc<aevia_core::value::TensorData> {
    match value(source, name) {
        | Value::Tensor(tensor) => tensor,
        | other => panic!("expected a tensor, found {}", other.type_name()),
    }
}

fn float_of(value: &Value) -> f64 {
    value.as_f64().expect("a number")
}

fn length() -> Dim {
    Dim::base("Length")
}

fn time() -> Dim {
    Dim::base("Time")
}

fn scalar(
    value: f64,
    dim: Dim,
) -> Value {
    Value::Num(aevia_core::value::NumVal::float(value, dim))
}

// ---------------------------------------------------------------------------
// §3.5.3 / §4.5.2 — a unit scales a tensor elementwise
// ---------------------------------------------------------------------------

/// `A * u` multiplies every element's dimension by the unit's, keeps the
/// element representation (`Float` stays `Float`), and the display unit travels
/// with the tensor so `show`/`to`/`value_in` report the scaled quantity.
#[test]
fn a_tensor_scaled_by_a_unit_carries_the_dimension_of_every_element() {
    let source = r#"let t = [[1.0, 2.0], [3.0, 4.0]] * ohm
let x = t[0, 0]
let in_ohm = value_in(ohm, x)
let scaled = [1000.0, 2000.0] * m
let in_km = value_in(km, scaled[0])
let per_second = [1.0, 2.0] / s
"#;
    let mut interpreter = interp(source);
    let t = match interpreter.eval_toplevel("t").expect("t") {
        | Value::Tensor(tensor) => tensor,
        | other => panic!("t is a tensor, found {}", other.type_name()),
    };
    assert_eq!(t.shape, vec![2, 2], "scaling a tensor does not reshape it");
    assert!(
        matches!(t.storage, aevia_core::value::Storage::Dense(_)),
        "a Float tensor stays an f64 buffer after a unit scale"
    );

    // The element of the scaled tensor carries the tensor's element dimension,
    // and `value_in` converts from the display unit: `1 Ω` is `1 Ω`.
    let x = match interpreter.eval_toplevel("x").expect("x") {
        | Value::Num(number) => number,
        | other => panic!("x is a number, found {}", other.type_name()),
    };
    assert!(matches!(x.kind, NumKind::Float(value) if value == 1.0));
    assert_eq!(
        x.dim, t.dim,
        "an element of the scaled tensor carries its element dimension"
    );
    assert!(
        x.display.is_some(),
        "the element is displayed in the unit it was scaled by"
    );
    assert_eq!(
        float_of(&interpreter.eval_toplevel("in_ohm").expect("in_ohm")),
        1.0
    );

    // `[1000, 2000] * m` read in km is `[1, 2]`.
    assert_eq!(
        float_of(&interpreter.eval_toplevel("in_km").expect("in_km")),
        1.0,
        "value_in(km, 1000 m) is 1 km"
    );

    // `A / u` divides the element dimension.
    let per_second = match interpreter.eval_toplevel("per_second").expect("per_second") {
        | Value::Tensor(tensor) => tensor,
        | other => panic!("per_second is a tensor, found {}", other.type_name()),
    };
    let expected = num("let w = 1 / (1 s)\n", "w").dim;
    assert_eq!(
        per_second.dim, expected,
        "Tensor / Unit divides the element dimension"
    );
}

/// `Unit * A` is the same operation with the operands swapped, and the scale is
/// dimensional metadata rather than an elementwise pass, so the buffer is
/// shared with the unscaled source.
#[test]
fn a_unit_on_the_left_scales_the_same_way_and_shares_the_buffer() {
    let source = r#"let plain = [[1.0, 2.0], [3.0, 4.0]]
let a = plain * ohm
let b = ohm * [[1.0, 2.0], [3.0, 4.0]]
"#;
    let mut interpreter = interp(source);
    let plain = match interpreter.eval_toplevel("plain").expect("plain") {
        | Value::Tensor(tensor) => tensor,
        | other => panic!("plain is a tensor, found {}", other.type_name()),
    };
    let a = match interpreter.eval_toplevel("a").expect("a") {
        | Value::Tensor(tensor) => tensor,
        | other => panic!("a is a tensor, found {}", other.type_name()),
    };
    let b = match interpreter.eval_toplevel("b").expect("b") {
        | Value::Tensor(tensor) => tensor,
        | other => panic!("b is a tensor, found {}", other.type_name()),
    };
    assert_eq!(a.shape, b.shape);
    assert_eq!(
        a.dim, b.dim,
        "unit * tensor multiplies the element dimension"
    );
    assert_eq!(
        a.elements().iter().map(float_of).collect::<Vec<_>>(),
        b.elements().iter().map(float_of).collect::<Vec<_>>()
    );
    assert!(
        Rc::ptr_eq(
            plain.buffer().expect("plain buffer"),
            a.buffer().expect("scaled buffer")
        ),
        "a unit scale is dimensional metadata, not an elementwise pass"
    );
}

// ---------------------------------------------------------------------------
// §4.9.3 — Dyn arithmetic
// ---------------------------------------------------------------------------

fn dyn_of(value: Value) -> Value {
    aevia_core::builtins::call_simple("dyn", vec![value]).expect("dyn wraps a scalar")
}

fn dyn_value(value: &Value) -> Rc<aevia_core::value::DynValue> {
    match value {
        | Value::Dyn(dynamic) => dynamic.clone(),
        | other => panic!("expected a Dyn, found {}", other.type_name()),
    }
}

fn binary(
    name: &str,
    left: Value,
    right: Value,
) -> Result<Value, aevia_core::EvalError> {
    let mut interpreter = Interp::new(&aevia_types::CheckedModule::default());
    interpreter.apply_binary(name, left, right, Span::default())
}

/// `+` and `-` require equal run-time dimensions and keep them (§4.9.3).
#[test]
fn dyn_addition_keeps_the_dimension_and_rejects_a_mismatch() {
    let metre = length();
    let left = dyn_of(scalar(1.5, metre.clone()));
    let right = dyn_of(scalar(2.5, metre.clone()));
    let sum = binary("+", left, right).expect("same dimensions add");
    assert_eq!(dyn_value(&sum).dim, metre);
    assert_eq!(
        dyn_value(&sum).value.to_f64().expect("a number"),
        4.0,
        "the payloads add"
    );

    let seconds = dyn_of(scalar(1.0, time()));
    let error =
        binary("+", dyn_of(scalar(1.0, metre)), seconds).expect_err("adding L and T must fail");
    assert_eq!(error.code, "E0412", "Dyn dimension check failed");

    // `-` is the same rule.
    let difference = binary(
        "-",
        dyn_of(scalar(3.0, length())),
        dyn_of(scalar(1.0, length())),
    )
    .expect("same dimensions subtract");
    assert_eq!(dyn_value(&difference).dim, length());
    assert_eq!(
        dyn_value(&difference).value.to_f64().expect("a number"),
        2.0
    );
}

/// `*` adds the dimensions, `/` subtracts them (§4.9.3).
#[test]
fn dyn_multiplication_and_division_combine_dimensions() {
    let product = binary(
        "*",
        dyn_of(scalar(2.0, length())),
        dyn_of(scalar(3.0, length())),
    )
    .expect("multiply");
    let dynamic = dyn_value(&product);
    assert_eq!(dynamic.dim, length().mul(&length()), "L * L = L^2");
    assert_eq!(dynamic.value.to_f64().expect("a number"), 6.0);

    let quotient = binary(
        "/",
        dyn_of(scalar(6.0, length())),
        dyn_of(scalar(3.0, time())),
    )
    .expect("divide");
    let dynamic = dyn_value(&quotient);
    assert_eq!(dynamic.dim, length().div(&time()), "L / T");
    assert!((dynamic.value.to_f64().expect("a number") - 2.0).abs() < 1e-12);
}

/// `Dyn ^ Int` multiplies the dimension exponent; `sqrt(Dyn)` halves it.
#[test]
fn dyn_powers_and_roots_scale_the_dimension_exponent() {
    let squared = binary("^", dyn_of(scalar(3.0, length())), Value::int(2)).expect("Dyn ^ Int");
    let dynamic = dyn_value(&squared);
    assert_eq!(dynamic.payload().to_f64().expect("a number"), 9.0);
    let factors = dynamic.dim.base_factors();
    assert_eq!(factors.len(), 1, "one base dimension");
    assert_eq!(
        *factors[0].1,
        num_rational::BigRational::from_integer(2.into()),
        "the exponent is multiplied: L^2"
    );

    let root = aevia_core::builtins::call_simple(
        "sqrt",
        vec![dyn_of(scalar(9.0, length().mul(&length())))],
    )
    .expect("sqrt of a Dyn");
    let dynamic = dyn_value(&root);
    assert_eq!(dynamic.dim, length(), "sqrt of L^2 is L");
    assert_eq!(dynamic.payload().to_f64().expect("a number"), 3.0);
}

/// §4.9.4: a `Dyn` must be `check`ed before it meets a `Scalar`; the mixed
/// operation is an error at run time, not a silent conversion.
#[test]
fn dyn_and_scalar_do_not_mix_implicitly() {
    let error = binary("+", dyn_of(scalar(1.0, length())), scalar(2.0, length()))
        .expect_err("Dyn + Scalar is not defined");
    assert_eq!(error.code, "E0302");
}

// ---------------------------------------------------------------------------
// §4.9.4 — `check<d>` compares the materialised dimension at run time
// ---------------------------------------------------------------------------

/// The checker materialises `d` for `check<d>` and records it by span; the
/// runtime compares it with the dimension the `Dyn` carries. A mismatch is the
/// `Err(DimError)` (`E0412`) the signature promises, a match is the scalar.
#[test]
fn check_compares_the_materialised_dimension() {
    let source = r#"fn main(): IO Unit = do
    let time = dyn(1.0 s)
    let wrong = check<L>(time)
    match wrong with
    | Ok(_) -> println("accepted")
    | Err(_) -> println("rejected")
    let length = dyn(2.0 m)
    let right = check<L>(length)
    match right with
    | Ok(value) -> println("value {value}")
    | Err(_) -> println("rejected")
"#;
    let mut interpreter = interp(source);
    interpreter.run_main().expect("main runs");
    let output = interpreter.captured();
    assert_eq!(
        output, "rejected\nvalue 2 m\n",
        "check<L> rejects a Time Dyn and accepts a Length Dyn"
    );
}

/// Unary `-` preserves the dimension of a `Dyn` (§4.9.3).
#[test]
fn negating_a_dyn_preserves_its_dimension() {
    let source = r#"let d = dyn(2.0 m)
let negated = check<L>(-d)
"#;
    let mut interpreter = interp(source);
    let negated = interpreter.eval_toplevel("negated").expect("negated");
    let Value::Variant(result) = negated else {
        panic!("check answers a Result")
    };
    assert_eq!(result.index, 0, "the dimension is preserved: {result:?}");
    let Value::Num(number) = &result.fields[0] else {
        panic!("the payload is a scalar")
    };
    assert_eq!(number.to_f64().expect("a number"), -2.0);
    assert_eq!(number.dim, length(), "- preserves L");
}

// ---------------------------------------------------------------------------
// §6.10 — rng and par_map
// ---------------------------------------------------------------------------

/// `rng(seed)` is the pure generator: the same seed reproduces the same stream,
/// a different seed gives a different one, and successive calls advance.
#[test]
fn rng_is_a_pure_reproducible_stream() {
    let source = r#"let g = rng(7)
let first = g()
let second = g()
let same_seed = rng(7)
let again = same_seed()
let other_seed = rng(8)
let other = other_seed()
"#;
    let first = float_of(&value(source, "first"));
    let second = float_of(&value(source, "second"));
    let again = float_of(&value(source, "again"));
    let other = float_of(&value(source, "other"));
    assert_eq!(first, again, "same seed, same stream");
    assert_ne!(first, other, "a different seed gives a different stream");
    assert_ne!(first, second, "each call advances the stream");
    assert!((0.0..1.0).contains(&first));
}

/// `par_map(f, xs)` applies `f` to every element and preserves the input order
/// (§6.10); the result has the input's length and the mapped element values.
#[test]
fn par_map_preserves_the_input_order() {
    let source = r#"let ints = par_map(fn x => x * 2, [1, 2, 3, 4])
let floats = par_map(fn x => x - 0.5, [1.0, 2.0, 3.0])
"#;
    let ints = tensor(source, "ints");
    assert_eq!(ints.shape, vec![4]);
    assert_eq!(
        ints.elements().iter().map(float_of).collect::<Vec<_>>(),
        vec![2.0, 4.0, 6.0, 8.0]
    );
    let floats = tensor(source, "floats");
    assert_eq!(
        floats.elements().iter().map(float_of).collect::<Vec<_>>(),
        vec![0.5, 1.5, 2.5]
    );
}

// ---------------------------------------------------------------------------
// §8.9 — odeint's flatten contract
// ---------------------------------------------------------------------------

/// A record state with `RateOf` fields — the §11.2 shape — integrates through
/// the flatten/rebuild contract: the result is a `Tensor<1, State>` whose
/// elements carry the same fields, in the same order, with the values the ODE
/// produces. The state is chosen so a field swap cannot pass: with `x' = v`,
/// `v' = a`, `x(0)=0`, `v(0)=1`, at `t=1` the answer is `x=1.5`, `v=2`.
#[test]
fn an_ode_with_a_record_state_flattens_and_rebuilds_in_field_order() {
    let source = r#"type State = {
    x: Float[L],
    v: Float[L/T],
}

type Rate = {
    x: Float[L/T],
    v: Float[L/T^2],
}

fn rhs(_t: Float[T], s: State): Rate =
    {
        x = s.v,
        v = 1.0 m/s^2,
    }

let path = odeint(
    rhs,
    { x = 0.0 m, v = 1.0 m/s },
    linspace(0.0 s, 1.0 s, 11),
)
let last = path[-1]
"#;
    let mut interpreter = interp(source);
    let path = match interpreter.eval_toplevel("path").expect("path") {
        | Value::Tensor(tensor) => tensor,
        | other => panic!("the trajectory is a tensor, found {}", other.type_name()),
    };
    assert_eq!(
        path.shape,
        vec![11],
        "one state per time point, Tensor<1, State>"
    );
    let last = interpreter.eval_toplevel("last").expect("last");
    let Value::Record(last) = last else {
        panic!("a trajectory element is a State record")
    };
    let x = float_of(&last.fields["x"]);
    let v = float_of(&last.fields["v"]);
    assert!(
        (x - 1.5).abs() < 1e-3,
        "x(1) = v0*t + a*t^2/2 = 1.5, found {x}"
    );
    assert!((v - 2.0).abs() < 1e-3, "v(1) = v0 + a*t = 2, found {v}");
    // Every element is a full State: a short or reordered flatten would drop a
    // field here.
    let elements = path.elements();
    let Value::Record(first) = &elements[0] else {
        panic!("the first trajectory element is a record")
    };
    assert_eq!(
        first.fields.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["x", "v"],
        "the record is rebuilt with its declared field order"
    );
}

/// A right-hand side that answers a different number of components than the
/// state is a runtime error, not a silently zero-padded integration. The
/// checker's `RateOf` prevents this from a well-typed program, so the guard is
/// driven through the builtin with a native right-hand side.
#[test]
fn an_ode_rhs_with_the_wrong_rate_count_is_an_error() {
    use aevia_core::value::NativeFn;
    let rhs = Value::Native(Rc::new(NativeFn(Box::new(
        |_interp: &mut Interp, _args: Vec<Value>, _span: Span| {
            // Two rates for a one-component state.
            Ok(aevia_core::eval::build_tensor(
                vec![Value::float(1.0), Value::float(2.0)],
                vec![2],
                Dim::one(),
            ))
        },
    ))));
    let initial = Value::float(0.0);
    let times = aevia_core::eval::build_tensor(
        vec![
            Value::float(0.0),
            Value::float(0.25),
            Value::float(0.5),
            Value::float(1.0),
        ],
        vec![4],
        time(),
    );
    let mut interpreter = Interp::new(&aevia_types::CheckedModule::default());
    let error = interpreter
        .call_named("odeint", vec![rhs, initial, times], Span::default())
        .expect_err("a rate-count mismatch is an error");
    assert_eq!(error.code, "E0302");
    assert!(
        error.message.contains("2 rate") && error.message.contains("1 component"),
        "the message names both counts: {}",
        error.message
    );
}
