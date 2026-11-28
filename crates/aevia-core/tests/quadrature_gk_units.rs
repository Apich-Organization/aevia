//! Gauss–Kronrod sampling and units at the Aevia runtime boundary.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::eval::EvalError;
use aevia_core::value::NativeFn;
use aevia_core::value::NumVal;
use aevia_core::value::Value;
use aevia_dim::Dim;
use aevia_syntax::Span;
use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

fn expect_result_number(
    value: &Value,
    dim: &Dim,
    expected: f64,
) {
    let Value::Variant(answer) = value else {
        panic!("expected Result");
    };
    assert_eq!(answer.name, "Ok", "{answer:?}");
    let [Value::Num(number)] = answer.fields.as_slice() else {
        panic!("expected a numeric integral");
    };
    assert_eq!(&number.dim, dim);
    let actual = number.to_f64().unwrap();
    assert!((actual - expected).abs() < 1e-12, "{actual} vs {expected}");
}

#[test]
fn quadrature_integrates_without_sampling_a_failing_endpoint() {
    let source = r#"fn sample(index: Int): Float[T] = [1.0 s][index]
fn density(x: Float[L]): Float[T] =
    if x == 0.0 m then sample(2) else 2.0 s
let integral = quadrature(density, 0.0 m, 1.0 m)
"#;
    let compilation = compile("quadrature_endpoint.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics
    );
    let dim = Dim::base("Length").mul(&Dim::base("Time"));
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        let loads = interp.load(compilation.module.as_ref().unwrap());
        assert!(!loads.has_errors(), "{mode:?}: {loads:?}");
        expect_result_number(&interp.eval_toplevel("integral").unwrap(), &dim, 2.0);
    }
}

#[test]
fn quadrature_supports_finite_bounds_with_an_overflowing_full_width() {
    let source = r#"fn density(_x: Float[L]): Float[T] = 1.0e-308 s
let wide = quadrature(density, -1.7e308 m, 1.7e308 m)
let empty = quadrature(density, 1.0 m, 1.0 m)
let reversed = quadrature(density, 1.0 m, 0.0 m)
"#;
    let compilation = compile("quadrature_range.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics
    );
    let dim = Dim::base("Length").mul(&Dim::base("Time"));
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        let loads = interp.load(compilation.module.as_ref().unwrap());
        assert!(!loads.has_errors(), "{mode:?}: {loads:?}");
        expect_result_number(&interp.eval_toplevel("wide").unwrap(), &dim, 3.4);
        expect_result_number(&interp.eval_toplevel("empty").unwrap(), &dim, 0.0);
        let Value::Variant(reversed) = interp.eval_toplevel("reversed").unwrap() else {
            panic!("expected Result");
        };
        assert_eq!(reversed.name, "Err", "{mode:?} {reversed:?}");
    }
}

#[test]
#[allow(
    clippy::result_large_err,
    reason = "native callbacks use the shared evaluator error"
)]
fn quadrature_host_callbacks_preserve_node_order_and_stop_at_the_first_error() {
    let compilation = compile("quadrature_host.aev", "");
    let mut interp = Interp::new(&compilation.checked);
    let coordinates = Rc::new(RefCell::new(Vec::<f64>::new()));
    let seen = Rc::clone(&coordinates);
    let callback = Value::Native(Rc::new(NativeFn(Box::new(move |_, args, _| {
        seen.borrow_mut().push(args[0].as_f64().unwrap());
        Ok(Value::Num(NumVal::float(2.0, Dim::base("Time"))))
    }))));
    let args = |callback, a, b| {
        vec![
            callback,
            Value::Num(NumVal::float(a, Dim::base("Length"))),
            Value::Num(NumVal::float(b, Dim::base("Length"))),
        ]
    };
    let answer = interp
        .call_named(
            "quadrature",
            args(callback.clone(), 0.0, 1.0),
            Span::default(),
        )
        .unwrap();
    let dim = Dim::base("Length").mul(&Dim::base("Time"));
    expect_result_number(&answer, &dim, 2.0);
    let visited = coordinates.take();
    assert_eq!(visited.len(), 15);
    assert_eq!(visited[0].to_bits(), 0.5_f64.to_bits());
    assert_eq!(
        visited
            .iter()
            .filter(|x| x.to_bits() == 0.5_f64.to_bits())
            .count(),
        1
    );

    let empty = interp
        .call_named(
            "quadrature",
            args(callback.clone(), 1.0, 1.0),
            Span::default(),
        )
        .unwrap();
    expect_result_number(&empty, &dim, 0.0);
    assert_eq!(coordinates.take(), [1.0]);
    interp
        .call_named("quadrature", args(callback, 1.0, 0.0), Span::default())
        .unwrap();
    assert!(coordinates.take().is_empty());

    let attempts = Rc::new(Cell::new(0));
    let count = Rc::clone(&attempts);
    let failed = Value::Native(Rc::new(NativeFn(Box::new(move |_, args, _| {
        count.set(count.get() + 1);
        if args[0].as_f64().unwrap().to_bits() == 0 {
            Ok(Value::Num(NumVal::float(2.0, Dim::base("Time"))))
        } else {
            Err(EvalError::new("E0802", "host callback failed"))
        }
    }))));
    let error = interp
        .call_named("quadrature", args(failed, 0.0, 1.0), Span::default())
        .unwrap_err();
    assert_eq!(error.code, "E0802");
    assert_eq!(error.message, "host callback failed");
    assert_eq!(attempts.get(), 1);

    let count = Rc::clone(&attempts);
    attempts.set(0);
    let mixed = Value::Native(Rc::new(NativeFn(Box::new(move |_, _, _| {
        count.set(count.get() + 1);
        let dim = if count.get() == 1 {
            "Time"
        } else {
            "Mass"
        };
        Ok(Value::Num(NumVal::float(2.0, Dim::base(dim))))
    }))));
    let error = interp
        .call_named("quadrature", args(mixed, 0.0, 1.0), Span::default())
        .unwrap_err();
    assert_eq!(error.code, "E0401");
    assert_eq!(attempts.get(), 2);
}
