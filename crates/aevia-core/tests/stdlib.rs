//! Semantic tests for the chapter-8 standard library.
//!
//! Every assertion is about a *semantic* fact — the `NumKind` of a scalar, the
//! storage class and shape of a tensor, the constructor of an error — never
//! about a rendering (§3 of the plan). Where a program has to go through the
//! checker as well as the runtime, the test compiles and runs it.

#![allow(clippy::result_large_err)]

use aevia_core::builtins::call_simple;
use aevia_core::value::{NumKind, Storage, Value};
use aevia_core::{compile, EvalError, Interp};
use num_bigint::BigInt;
use num_rational::BigRational;

fn int(value: i64) -> Value {
    Value::int(value)
}

fn float(value: f64) -> Value {
    Value::float(value)
}

fn tensor(shape: Vec<usize>, values: Vec<f64>) -> Value {
    Value::Tensor(std::rc::Rc::new(aevia_core::value::TensorData {
        shape,
        storage: Storage::Dense(std::rc::Rc::new(values)),
        dim: aevia_dim::Dim::one(),
        display: None,
    }))
}

fn exact(values: Vec<i64>) -> Value {
    let elements: Vec<Value> = values
        .into_iter()
        .map(|value| {
            Value::Num(aevia_core::value::NumVal::int(BigInt::from(value)))
        })
        .collect();
    let len = elements.len();
    aevia_core::eval::build_tensor(elements, vec![len], aevia_dim::Dim::one())
}

fn call(name: &str, args: Vec<Value>) -> Value {
    call_simple(name, args).unwrap_or_else(|error| panic!("`{name}` failed: {error:?}"))
}

fn call_error(name: &str, args: Vec<Value>) -> EvalError {
    match call_simple(name, args) {
        Ok(value) => panic!("`{name}` answered {value:?} instead of failing"),
        Err(error) => error,
    }
}

fn num(value: &Value) -> &aevia_core::value::NumVal {
    match value {
        Value::Num(number) => number,
        other => panic!("expected a scalar, found {}", other.type_name()),
    }
}

fn as_f64(value: &Value) -> f64 {
    value.as_f64().expect("a numeric value")
}

fn tensor_of(value: &Value) -> &aevia_core::value::TensorData {
    match value {
        Value::Tensor(tensor) => tensor,
        other => panic!("expected a tensor, found {}", other.type_name()),
    }
}

/// Runs a program and returns the value of a top-level name.
fn run_value(source: &str, name: &str) -> Value {
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
    assert!(!diagnostics.has_errors(), "load errors: {diagnostics:?}");
    interp.eval_toplevel(name).expect("the name evaluates")
}

#[test]
fn the_combinators_compose_functions() {
    // `compose(f, g)(x) = f(g(x))`, and `curry`/`uncurry` are inverses.
    let source = r#"
let twice = compose(fn x => x * 2, fn x => x + 1)
let a = twice(3)
let b = apply(fn x => x - 1, 10)
let c = curry(fn (x, y) => x * 10 + y)(4)(2)
let d = uncurry(fn x => fn y => x * 10 + y)(4, 2)
let e = partial(fn (x, y) => x - y, 10)(4)
let f = flip(fn (x, y) => x - y)(1, 10)
"#;
    assert_eq!(as_f64(&run_value(source, "a")), 8.0);
    assert_eq!(as_f64(&run_value(source, "b")), 9.0);
    assert_eq!(as_f64(&run_value(source, "c")), 42.0);
    assert_eq!(as_f64(&run_value(source, "d")), 42.0);
    assert_eq!(as_f64(&run_value(source, "e")), 6.0);
    assert_eq!(as_f64(&run_value(source, "f")), 9.0);
}

#[test]
fn id_const_and_zero_keep_their_representation() {
    assert_eq!(as_f64(&call("id", vec![int(3)])), 3.0);
    assert!(matches!(
        call("id", vec![int(3)]).as_num().map(|n| &n.kind),
        Some(NumKind::Int(_))
    ));
    assert_eq!(as_f64(&call("const", vec![int(1), Value::string("x")])), 1.0);
    // `zero` is the exact zero of `fold((+), zero, xs)`.
    let zero = call("zero", Vec::new());
    assert!(zero.as_num().is_some_and(aevia_core::value::NumVal::is_zero));
    assert!(matches!(zero.as_num().map(|n| &n.kind), Some(NumKind::Exact(_))));
}

#[test]
fn compare_returns_an_option_of_ordering() {
    let less = call("compare", vec![int(1), int(2)]);
    match &less {
        Value::Variant(variant) => {
            assert_eq!(variant.name, "Some");
            let inner = &variant.fields[0];
            assert!(
                matches!(inner, Value::Variant(ordering) if ordering.name == "Less"),
                "{inner:?}"
            );
        }
        other => panic!("expected an Option, found {}", other.type_name()),
    }
    // `partial_compare` refuses to order a NaN (§8.1.2).
    let unordered = call("partial_compare", vec![float(f64::NAN), float(1.0)]);
    assert!(matches!(&unordered, Value::Variant(variant) if variant.name == "None"));
    let total = call("total_compare", vec![int(2), int(1)]);
    assert!(matches!(&total, Value::Variant(variant) if variant.name == "Greater"));
}

#[test]
fn the_symbolic_comparisons_build_propositions() {
    let proposition = call("sym_lt", vec![int(1), int(2)]);
    assert!(matches!(proposition, Value::Prop(_)));
    // A proposition over concrete numbers is decidable.
    let decided = call("decide", vec![proposition]);
    match &decided {
        Value::Variant(variant) => {
            assert_eq!(variant.name, "Some");
            assert!(matches!(variant.fields[0], Value::Bool(true)));
        }
        other => panic!("expected an Option, found {}", other.type_name()),
    }
    assert!(matches!(
        call("sym_eq", vec![int(2), int(2)]),
        Value::Prop(_)
    ));
}

#[test]
fn statistics_keep_exactness_and_report_shapes() {
    // `median` of an exact sample is exact: 2, not 2.0.
    let median = call("median", vec![exact(vec![3, 1, 2])]);
    assert!(matches!(median.as_num().map(|n| &n.kind), Some(NumKind::Int(_))));
    assert_eq!(as_f64(&median), 2.0);

    // A float sample stays a float.
    let float_median = call("median", vec![tensor(vec![2], vec![1.0, 2.0])]);
    assert!(matches!(
        float_median.as_num().map(|n| &n.kind),
        Some(NumKind::Float(_))
    ));

    // The type-7 quantile of [0, 10] at 1/4 is exactly 5/2.
    let quantile = call(
        "quantile",
        vec![exact(vec![0, 10]), Value::Num(aevia_core::value::NumVal::exact(
            BigRational::new(BigInt::from(1), BigInt::from(4)),
            aevia_dim::Dim::one(),
        ))],
    );
    assert_eq!(
        num(&quantile).to_rational(),
        Some(BigRational::new(BigInt::from(5), BigInt::from(2)))
    );

    // The sample covariance of (1,2,3) with (2,4,6) is exactly 2.
    let covariance = call("cov", vec![exact(vec![1, 2, 3]), exact(vec![2, 4, 6])]);
    assert_eq!(num(&covariance).to_rational(), Some(BigRational::from_integer(2.into())));

    // The correlation of two identical lines is 1.
    let correlation = call(
        "corr",
        vec![tensor(vec![3], vec![1.0, 2.0, 3.0]), tensor(vec![3], vec![2.0, 4.0, 6.0])],
    );
    assert!((as_f64(&correlation) - 1.0).abs() < 1e-12);
}

#[test]
fn a_histogram_reports_counts_and_edges() {
    let histogram = call(
        "histogram",
        vec![tensor(vec![5], vec![0.0, 1.0, 2.0, 3.0, 4.0]), int(2)],
    );
    let Value::Record(record) = &histogram else {
        panic!("expected a record, found {}", histogram.type_name());
    };
    let counts = tensor_of(&record.fields["counts"]);
    assert_eq!(counts.shape, vec![2]);
    assert!(matches!(counts.storage, Storage::Exact(_)));
    let edges = tensor_of(&record.fields["edges"]);
    assert_eq!(edges.shape, vec![3]);
    // Edges are [0, 2, 4]: 0 and 1 fall low, 2, 3 and 4 fall high.
    let counts: Vec<f64> = counts.elements().iter().map(as_f64).collect();
    assert_eq!(counts, vec![2.0, 3.0]);
}

#[test]
fn the_distributions_have_densities_and_cdfs() {
    // Normal{mu = 0, sigma = 1} at 0: pdf = 1/sqrt(2 pi), cdf = 1/2.
    let normal = aevia_core::value::make_variant(
        u32::MAX - 1,
        0,
        "Normal",
        Vec::new(),
    );
    let _ = normal;
    let distribution = record(vec![
        ("mu", float(0.0)),
        ("sigma", float(1.0)),
    ]);
    let pdf = call("pdf", vec![distribution.clone(), float(0.0)]);
    assert!((as_f64(&pdf) - 1.0 / (2.0 * std::f64::consts::PI).sqrt()).abs() < 1e-12);
    let cdf = call("cdf", vec![distribution, float(0.0)]);
    assert!((as_f64(&cdf) - 0.5).abs() < 1e-12);

    // A uniform density is the reciprocal of its width.
    let uniform = record(vec![("lo", float(0.0)), ("hi", float(4.0))]);
    assert!((as_f64(&call("pdf", vec![uniform.clone(), float(1.0)])) - 0.25).abs() < 1e-12);
    // `sample_n` returns a tensor of the requested length inside the support.
    let samples = call("sample_n", vec![uniform, int(16), Value::Unit]);
    let tensor = tensor_of(&samples);
    assert_eq!(tensor.shape, vec![16]);
    for element in tensor.elements() {
        assert!((0.0..=4.0).contains(&as_f64(&element)));
    }
}

fn record(fields: Vec<(&str, Value)>) -> Value {
    let mut map = indexmap::IndexMap::new();
    for (name, value) in fields {
        map.insert(name.to_string(), value);
    }
    Value::Record(std::rc::Rc::new(aevia_core::value::RecordValue { fields: map }))
}

#[test]
fn the_dense_decompositions_have_the_documented_shapes() {
    let matrix = tensor(vec![2, 2], vec![4.0, 7.0, 2.0, 6.0]);

    let qr = call("qr", vec![matrix.clone()]);
    let Value::Record(qr) = &qr else { panic!("qr must return a pair") };
    assert_eq!(tensor_of(&qr.fields["0"]).shape, vec![2, 2]);
    assert_eq!(tensor_of(&qr.fields["1"]).shape, vec![2, 2]);

    let lu = call("lu", vec![matrix.clone()]);
    let Value::Record(lu) = &lu else { panic!("lu must return a triple") };
    for field in ["0", "1", "2"] {
        assert_eq!(tensor_of(&lu.fields[field]).shape, vec![2, 2]);
    }

    // A positive definite matrix has a Cholesky factor.
    let cholesky = call("cholesky", vec![tensor(vec![2, 2], vec![4.0, 2.0, 2.0, 3.0])]);
    let Value::Variant(result) = &cholesky else { panic!("cholesky returns a Result") };
    assert_eq!(result.name, "Ok");
    assert_eq!(tensor_of(&result.fields[0]).shape, vec![2, 2]);
    // A matrix that is not positive definite is reported, not approximated.
    let indefinite = call("cholesky", vec![tensor(vec![2, 2], vec![1.0, 2.0, 2.0, 1.0])]);
    match &indefinite {
        Value::Variant(variant) => {
            assert_eq!(variant.name, "Err");
            assert!(matches!(&variant.fields[0], Value::Variant(e) if e.name == "NotPositiveDefinite"));
        }
        other => panic!("expected a Result, found {}", other.type_name()),
    }

    // The economy SVD of a 2x3 matrix is 2x2, 2, 2x3.
    let (u, s, vt) = match call("svd", vec![tensor(vec![2, 3], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0])]) {
        Value::Record(record) => (
            tensor_of(&record.fields["0"]).shape.clone(),
            tensor_of(&record.fields["1"]).shape.clone(),
            tensor_of(&record.fields["2"]).shape.clone(),
        ),
        other => panic!("expected a triple, found {}", other.type_name()),
    };
    assert_eq!((u, s, vt), (vec![2, 2], vec![2], vec![2, 3]));
}

#[test]
fn eigenvalues_of_a_symmetric_matrix_are_complex_numbers_with_a_real_part() {
    let eigenvalues = call("eigvals", vec![tensor(vec![2, 2], vec![2.0, 1.0, 1.0, 2.0])]);
    let tensor = tensor_of(&eigenvalues);
    assert_eq!(tensor.shape, vec![2]);
    let mut reals: Vec<f64> = tensor
        .elements()
        .iter()
        .map(|element| match element.as_num().map(|n| &n.kind) {
            Some(NumKind::Complex(re, _)) => *re,
            other => panic!("expected a complex eigenvalue, found {other:?}"),
        })
        .collect();
    reals.sort_by(f64::total_cmp);
    assert!((reals[0] - 1.0).abs() < 1e-9);
    assert!((reals[1] - 3.0).abs() < 1e-9);
}

#[test]
fn a_singular_system_reports_singular_rather_than_a_number() {
    let outcome = call("linsolve", vec![
        tensor(vec![2, 2], vec![1.0, 2.0, 2.0, 4.0]),
        tensor(vec![2], vec![1.0, 2.0]),
    ]);
    match &outcome {
        Value::Variant(variant) => {
            assert_eq!(variant.name, "Err");
            assert!(matches!(&variant.fields[0], Value::Variant(e) if e.name == "Singular"));
        }
        other => panic!("expected a Result, found {}", other.type_name()),
    }
}

#[test]
fn least_squares_fits_a_line_through_three_points() {
    let outcome = call(
        "lstsq",
        vec![
            tensor(vec![3, 2], vec![1.0, 0.0, 1.0, 1.0, 1.0, 2.0]),
            tensor(vec![3], vec![1.0, 3.0, 5.0]),
        ],
    );
    let Value::Variant(result) = &outcome else { panic!("lstsq returns a Result") };
    assert_eq!(result.name, "Ok");
    let solution = tensor_of(&result.fields[0]);
    assert_eq!(solution.shape, vec![2]);
    let values: Vec<f64> = solution.elements().iter().map(as_f64).collect();
    assert!((values[0] - 1.0).abs() < 1e-9);
    assert!((values[1] - 2.0).abs() < 1e-9);
}

#[test]
fn the_sparse_layout_round_trips_through_csr() {
    let dense = tensor(vec![2, 3], vec![1.0, 0.0, 2.0, 0.0, 0.0, 3.0]);
    let sparse = call("from_dense", vec![dense.clone()]);
    assert_eq!(as_f64(&call("nnz", vec![sparse.clone()])), 3.0);
    let back = call("to_dense", vec![sparse]);
    let tensor = tensor_of(&back);
    assert_eq!(tensor.shape, vec![2, 3]);
    let values: Vec<f64> = tensor.elements().iter().map(as_f64).collect();
    assert_eq!(values, vec![1.0, 0.0, 2.0, 0.0, 0.0, 3.0]);
}

#[test]
fn the_shape_operations_keep_the_element_count() {
    let matrix = tensor(vec![2, 3], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    let reshaped = call("reshape_dyn", vec![matrix, tensor(vec![2], vec![3.0, 2.0])]);
    assert_eq!(tensor_of(&reshaped).shape, vec![3, 2]);
    // A reshape that would drop elements answers with the `Err` its declared
    // `Result` promises, not with a diagnostic.
    let refused = call(
        "reshape_dyn",
        vec![tensor(vec![2, 3], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), tensor(vec![1], vec![4.0])],
    );
    match refused {
        Value::Variant(result) => assert_eq!(result.index, 1, "Err"),
        other => panic!("expected a Result, found {}", other.type_name()),
    }
    // `set` replaces one element.
    let updated = call(
        "set",
        vec![
            tensor(vec![2, 2], vec![1.0, 2.0, 3.0, 4.0]),
            tensor(vec![2], vec![0.0, 1.0]),
            float(9.0),
        ],
    );
    let values: Vec<f64> = tensor_of(&updated).elements().iter().map(as_f64).collect();
    assert_eq!(values, vec![1.0, 9.0, 3.0, 4.0]);
    // `tril` zeroes the upper triangle and keeps the storage class.
    let lower = call("tril", vec![tensor(vec![2, 2], vec![1.0, 2.0, 3.0, 4.0])]);
    let values: Vec<f64> = tensor_of(&lower).elements().iter().map(as_f64).collect();
    assert_eq!(values, vec![1.0, 0.0, 3.0, 4.0]);
    assert!(matches!(tensor_of(&lower).storage, Storage::Dense(_)));
}

#[test]
fn a_shape_type_argument_is_read_when_it_is_present() {
    // `reshape<…>` reads the shape from its type argument; without one there
    // is no shape to build, and the diagnostic says so instead of blaming the
    // program. (`reshape<Tensor<[3,2], Float>>(a)` is covered in `shapes.rs`,
    // which also pins the `[3,2]` spelling the parser does not accept yet.)
    let error = call_error("reshape", vec![tensor(vec![2], vec![1.0, 2.0])]);
    assert_eq!(error.code, "E0310");
    assert!(error.message.contains("shape type argument"), "{}", error.message);
    // NPY is implemented (§8.11): `read_npy` builds the IO action, and only
    // running it can fail (a missing file is the IO taxonomy's `E0901`).
    assert!(
        matches!(call("read_npy", vec![Value::string("x.npy")]), Value::Io(_)),
        "`read_npy` builds an IO action"
    );
    // The extension check is the recoverable `Err(E0904)` value, not a panic.
    let refused = call("read_npy", vec![Value::string("x.txt")]);
    match &refused {
        Value::Variant(result) => {
            assert_eq!(result.name, "Err");
            let Value::Str(message) = &result.fields[0] else {
                panic!("the byte payload is a message");
            };
            assert!(message.contains("E0904"), "{message}");
        }
        other => panic!("expected a Result, found {}", other.type_name()),
    }
}

#[test]
fn the_fourier_transform_round_trips() {
    let signal = vec![
        Value::Num(aevia_core::value::NumVal {
            kind: NumKind::Complex(1.0, -1.0),
            dim: aevia_dim::Dim::one(),
            display: None,
        }),
        Value::Num(aevia_core::value::NumVal {
            kind: NumKind::Complex(2.0, 0.5),
            dim: aevia_dim::Dim::one(),
            display: None,
        }),
        Value::Num(aevia_core::value::NumVal {
            kind: NumKind::Complex(-3.0, 2.0),
            dim: aevia_dim::Dim::one(),
            display: None,
        }),
    ];
    let input = aevia_core::eval::build_tensor(signal.clone(), vec![3], aevia_dim::Dim::one());
    let spectrum = call("fft", vec![input]);
    let restored = call("ifft", vec![spectrum]);
    let tensor = tensor_of(&restored);
    for (a, b) in signal.iter().zip(tensor.elements()) {
        let (Some(NumKind::Complex(ar, ai)), Some(NumKind::Complex(br, bi))) = (
            a.as_num().map(|n| &n.kind),
            b.as_num().map(|n| &n.kind),
        ) else {
            panic!("a transform of complex input is complex");
        };
        assert!((ar - br).abs() < 1e-12 && (ai - bi).abs() < 1e-12);
    }
    // `fftfreq` gives the sample frequencies, wrapping at the Nyquist bin.
    let frequencies = call("fftfreq", vec![int(4), float(1.0)]);
    let values: Vec<f64> = tensor_of(&frequencies).elements().iter().map(as_f64).collect();
    assert_eq!(values, vec![0.0, 0.25, -0.5, -0.25]);
}

#[test]
fn interpolation_and_polynomial_fitting() {
    let xs = tensor(vec![3], vec![0.0, 1.0, 2.0]);
    let ys = tensor(vec![3], vec![0.0, 10.0, 40.0]);
    let linear = call("interp1", vec![xs.clone(), ys.clone(), float(0.5)]);
    assert!((as_f64(&linear) - 5.0).abs() < 1e-12);
    let spline = call("spline", vec![xs, ys, float(0.5)]);
    assert!((as_f64(&spline) - 5.0).abs() < 2.0);

    let fit = call(
        "polyfit",
        vec![
            tensor(vec![4], vec![0.0, 1.0, 2.0, 3.0]),
            tensor(vec![4], vec![1.0, 3.0, 7.0, 13.0]),
            int(2),
        ],
    );
    // 1 + x + x^2, ascending coefficients.
    let coefficients: Vec<f64> = tensor_of(&fit).elements().iter().map(as_f64).collect();
    assert!((coefficients[0] - 1.0).abs() < 1e-8);
    assert!((coefficients[1] - 1.0).abs() < 1e-8);
    assert!((coefficients[2] - 1.0).abs() < 1e-8);
    let at = call("polyval", vec![fit, float(2.0)]);
    assert!((as_f64(&at) - 7.0).abs() < 1e-8);
}

#[test]
fn numeric_calculus_agrees_with_the_symbolic_answers() {
    // d/dx x^3 at 2 is 12.
    let derivative = run_value(
        "let df = numeric_deriv(fn x => x * x * x)\nlet d = df(2.0)\n",
        "d",
    );
    assert!((as_f64(&derivative) - 12.0).abs() < 1e-4);

}

/// A native `sin` function value, for the calculus tests that take a function.
fn native_capture() -> Value {
    Value::Native(std::rc::Rc::new(aevia_core::value::NativeFn(Box::new(
        |_interp, args, _span| {
            let value = args.first().and_then(Value::as_f64).unwrap_or(f64::NAN);
            Ok(Value::float(value.sin()))
        },
    ))))
}

#[test]
fn quadrature_minimisation_and_root_finding() {
    let integral = call(
        "quadrature",
        vec![native_capture(), float(0.0), float(std::f64::consts::PI)],
    );
    let Value::Variant(result) = &integral else { panic!("quadrature returns a Result") };
    assert_eq!(result.name, "Ok");
    assert!((as_f64(&result.fields[0]) - 2.0).abs() < 1e-8);

    // A minimum of a paraboloid.
    let minimum = run_value(
        "let m = minimize(fn p => (p[0] - 1.0) * (p[0] - 1.0) + (p[1] + 2.0) * (p[1] + 2.0), [0.0, 0.0])\n",
        "m",
    );
    let Value::Variant(result) = &minimum else { panic!("minimize returns a Result") };
    assert_eq!(result.name, "Ok");
    let point: Vec<f64> = tensor_of(&result.fields[0]).elements().iter().map(as_f64).collect();
    assert!((point[0] - 1.0).abs() < 1e-3);
    assert!((point[1] + 2.0).abs() < 1e-3);

    // The gradient of x^2 + 2y^2 at (1, 2) is (2, 8).
    let gradient = run_value(
        "let g = grad(fn p => p[0] * p[0] + 2.0 * p[1] * p[1], [1.0, 2.0])\n",
        "g",
    );
    let values: Vec<f64> = tensor_of(&gradient).elements().iter().map(as_f64).collect();
    assert!((values[0] - 2.0).abs() < 1e-4);
    assert!((values[1] - 8.0).abs() < 1e-4);

    // A root of x^3 - 2.
    let root = run_value("let r = root(fn x => x * x * x - 2.0, 1.0)\n", "r");
    let Value::Variant(result) = &root else { panic!("root returns a Result") };
    assert_eq!(result.name, "Ok");
    assert!((as_f64(&result.fields[0]) - 2.0f64.cbrt()).abs() < 1e-8);
}

#[test]
fn an_ode_solution_reaches_the_expected_state() {
    // y' = y, y(0) = 1: y(1) = e. The time grid carries `T`, as §8.9 says.
    //
    // The right-hand side returns `RateOf<s>`, not `s`: differentiating a
    // state with respect to `t` divides its dimension by `T`. A dimensionless
    // `y` therefore has a rate of `1/T`, which is why the body divides by
    // `1.0 s` — without that the program claims `dy/dt` and `y` share a
    // dimension, which is the `E0401` the checker now reports.
    let solution = run_value(
        r#"
fn rhs(t: Float[T], s: Tensor<1, Float[1]>): Tensor<1, Float[1/T]> = [s[0] / (1.0 s)]

let y = odeint(rhs, [1.0], [0.0 s, 0.5 s, 1.0 s])
"#,
        "y",
    );
    let tensor = tensor_of(&solution);
    assert_eq!(tensor.shape, vec![3]);
    let last = tensor.elements().pop().expect("a state");
    let last = tensor_of(&last);
    assert!((as_f64(&last.elements()[0]) - std::f64::consts::E).abs() < 1e-5);
}

#[test]
fn poisson_recovers_a_manufactured_solution() {
    let solution = run_value(
        r#"
let s = solve_poisson({ grid = { nx = 11, ny = 11, h = 0.1 }, source = zeros(11, 11), iterations = 100 })
"#,
        "s",
    );
    // The spec is a record; a zero source has the zero solution.
    let Value::Variant(result) = &solution else { panic!("solve_poisson returns a Result") };
    assert_eq!(result.name, "Ok");
    let Value::Record(field) = &result.fields[0] else { panic!("a field is a record") };
    let values = tensor_of(&field.fields["values"]);
    assert_eq!(values.shape, vec![11, 11]);
    assert!(values.elements().iter().all(|value| as_f64(value).abs() < 1e-9));
}

#[test]
fn the_table_layer_columns_selects_and_joins() {
    // A tensor of records becomes a table; a column is a tensor again.
    let rows = aevia_core::eval::build_tensor(
        vec![
            record(vec![("x", float(1.0)), ("y", Value::string("a"))]),
            record(vec![("x", float(2.0)), ("y", Value::string("b"))]),
        ],
        vec![2],
        aevia_dim::Dim::one(),
    );
    let table = call("to_table", vec![rows]);
    let Value::Variant(result) = &table else { panic!("to_table returns a Result") };
    assert_eq!(result.name, "Ok");
    let table = result.fields[0].clone();

    let column = call("column", vec![table.clone(), Value::string("x")]);
    let Value::Variant(result) = &column else { panic!("column returns a Result") };
    assert_eq!(result.name, "Ok");
    assert_eq!(tensor_of(&result.fields[0]).shape, vec![2]);
    assert!(matches!(tensor_of(&result.fields[0]).storage, Storage::Dense(_)));

    let row = call("row", vec![table.clone(), int(1)]);
    let Value::Variant(result) = &row else { panic!("row returns a Result") };
    assert_eq!(result.name, "Ok");
    assert!(matches!(&result.fields[0], Value::Record(_)));

    let names = aevia_core::eval::build_tensor(
        vec![Value::string("y")],
        vec![1],
        aevia_dim::Dim::one(),
    );
    let selected = call("select", vec![table.clone(), names]);
    let Value::Record(selected) = &selected else { panic!("select returns a table") };
    assert_eq!(tensor_of(&selected.fields["columns"]).elements().len(), 1);

    let schema = call("schema", vec![table]);
    assert_eq!(tensor_of(&schema).shape, vec![2]);
}

#[test]
fn complex_number_theory_and_geometry() {
    let z = call("complex", vec![float(3.0), float(4.0)]);
    assert!((as_f64(&call("magnitude", vec![z.clone()])) - 5.0).abs() < 1e-12);
    assert!((as_f64(&call("arg", vec![z.clone()])) - 4.0f64.atan2(3.0)).abs() < 1e-12);
    let conjugate = call("conj", vec![z]);
    assert!(matches!(conjugate.as_num().map(|n| &n.kind), Some(NumKind::Complex(_, im)) if *im == -4.0));

    let factors = call("factor", vec![int(12)]);
    assert_eq!(tensor_of(&factors).shape, vec![2]);
    assert!(matches!(call("is_prime", vec![int(97)]), Value::Bool(true)));
    assert_eq!(tensor_of(&call("primes", vec![int(10)])).shape, vec![4]);
    assert_eq!(as_f64(&call("binomial", vec![int(5), int(2)])), 10.0);

    let area = call(
        "polygon_area",
        vec![tensor(vec![4, 2], vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0])],
    );
    assert!((as_f64(&area) - 1.0).abs() < 1e-12);
    let distance = call(
        "distance",
        vec![tensor(vec![2], vec![0.0, 0.0]), tensor(vec![2], vec![3.0, 4.0])],
    );
    assert!((as_f64(&distance) - 5.0).abs() < 1e-12);
}

#[test]
fn measurements_and_device_placement() {
    let measurement = call("measure", vec![float(9.8), float(0.1)]);
    assert!((as_f64(&call("value", vec![measurement.clone()])) - 9.8).abs() < 1e-12);
    assert!((as_f64(&call("uncertainty", vec![measurement])) - 0.1).abs() < 1e-12);

    let device = call("device_of", vec![tensor(vec![2], vec![1.0, 2.0])]);
    assert!(matches!(&device, Value::Variant(variant) if variant.name == "Cpu"));
    // Placing a tensor on the CPU is the identity.
    let placed = call("to_device", vec![tensor(vec![2], vec![1.0, 2.0]), device]);
    assert!(matches!(placed, Value::Io(_)));
}

#[test]
fn simulation_delegates_to_the_backend() {
    let parameters = record(vec![
        ("width", int(4)),
        ("height", int(4)),
        ("temperature", float(2.0)),
        ("mc_steps", int(1)),
    ]);
    let outcome = call("simulate", vec![Value::string("ising"), parameters]);
    match &outcome {
        Value::Variant(variant) => assert_eq!(variant.name, "Ok"),
        other => panic!("expected a Result, found {}", other.type_name()),
    }
    // An unknown scenario is an `Err`, not a panic.
    let unknown = call("simulate", vec![Value::string("nope"), record(Vec::new())]);
    assert!(matches!(&unknown, Value::Variant(variant) if variant.name == "Err"));
}

#[test]
fn compile_fail_answers_whether_a_source_compiles() {
    assert!(matches!(
        call("compile_fail", vec![Value::string("let x: Int = \"s\"")]),
        Value::Bool(true)
    ));
    assert!(matches!(
        call("compile_fail", vec![Value::string("let x = 1")]),
        Value::Bool(false)
    ));
}

#[test]
fn an_exact_factorisation_stays_exact() {
    // `lu` of an exact matrix keeps rationals (§8.4 keeps ρ).
    let matrix = aevia_core::eval::build_tensor(
        (1..=4).map(|value| int(value)).collect(),
        vec![2, 2],
        aevia_dim::Dim::one(),
    );
    let lu = call("lu", vec![matrix]);
    let Value::Record(lu) = &lu else { panic!("lu returns a triple") };
    let u = tensor_of(&lu.fields["2"]);
    assert!(matches!(u.storage, Storage::Exact(_)));
    let entries: Vec<BigRational> = match &u.storage {
        Storage::Exact(values) => values.clone(),
        _ => unreachable!(),
    };
    // U = [[1, 2], [0, -2]].
    assert_eq!(entries[3], BigRational::from_integer(BigInt::from(-2)));

    // A float matrix keeps floats.
    let float_lu = call("lu", vec![tensor(vec![2, 2], vec![1.0, 2.0, 3.0, 4.0])]);
    let Value::Record(float_lu) = &float_lu else { panic!("lu returns a triple") };
    assert!(matches!(
        tensor_of(&float_lu.fields["2"]).storage,
        Storage::Dense(_)
    ));
}
