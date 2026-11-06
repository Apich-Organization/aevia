//! Manifest-generated typed callback facades preserve the kernel contract.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::value::Value;
use aevia_rssn::bindings::catalog;

fn alias(name: &str) -> String {
    catalog(aevia_rssn::registry())
        .into_iter()
        .find(|row| row.op.name == name)
        .and_then(|row| row.typed_callback_name)
        .unwrap_or_else(|| panic!("{name} must expose an exact typed callback alias"))
}
fn load(
    source: &str,
    name: &str,
) -> (aevia_core::driver::Program, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "aevia-typed-callback-{name}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.aev");
    std::fs::write(&path, source).unwrap();
    (aevia_core::driver::load(&path), dir)
}
fn payload(
    value: Value,
    mode: Mode,
    name: &str,
) -> Value {
    let Value::Variant(result) = value else {
        panic!("{mode:?} {name}: expected Result")
    };
    assert_eq!(result.index, 0, "{mode:?} {name}: {:?}", result.fields);
    result.fields[0].clone()
}
#[test]
fn generated_typed_brent_and_vector_ode_run_in_both_engines() {
    let brent = alias("num_brent");
    let methods = [
        "num_ode_dopri5",
        "num_ode_rosenbrock23",
        "num_ode_radau5",
        "num_ode_bdf",
    ];
    let mut source = format!(
        r#"import Rssn_numerical_callback
let period: Float[Time] = 1.0 s
let target: Float[Time] = 0.5 s
fn derivative(t: Float[Time], y: Tensor<[1], Float[Length]>): Tensor<1, Float[Length/Time]> = y / period
let root = Rssn_numerical_callback.{brent}(0.0 s, 1.0 s, 0.000001 s, 100, fn (t: Float[Time]) => t - target)
"#
    );
    for (index, method) in methods.iter().enumerate() {
        let name = alias(method);
        source.push_str(&format!("let ode{index} = Rssn_numerical_callback.{name}(0.0 s, 0.1 s, [1.0 m], 0.000001 m, 0.00001, 0.01 s, 1000, derivative)\n"));
    }
    let (program, dir) = load(&source, "positive");
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let Value::Record(root) = payload(interp.eval_toplevel("root").unwrap(), mode, "root")
        else {
            panic!("expected root record")
        };
        assert!((root.fields["root"].as_f64().unwrap() - 0.5).abs() < 1e-5);
        for index in 0..methods.len() {
            let name = format!("ode{index}");
            let Value::Record(record) = payload(interp.eval_toplevel(&name).unwrap(), mode, &name)
            else {
                panic!("expected ODE record")
            };
            let Value::Tensor(times) = &record.fields["times"] else {
                panic!("expected times")
            };
            let Value::Tensor(states) = &record.fields["states"] else {
                panic!("expected states")
            };
            assert_eq!(states.shape, vec![times.shape[0], 1]);
            let actual = states
                .get_flat(states.storage.len() - 1)
                .unwrap()
                .as_f64()
                .unwrap();
            assert!(
                (actual - 0.1_f64.exp()).abs() < 0.002,
                "{mode:?} {name}: {actual}"
            );
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn generated_typed_facade_rejects_wrong_callback_dimensions() {
    let name = alias("num_ode_dopri5");
    let source = format!(
        r#"import Rssn_numerical_callback
let bad = Rssn_numerical_callback.{name}(0.0 s, 0.1 s, [1.0 m], 0.000001 m, 0.00001, 0.01 s, 1000, fn (t: Float[Time], y: Tensor<[1], Float[Length]>) => y)
"#
    );
    let (program, dir) = load(&source, "negative");
    assert!(
        program.diagnostics.has_errors(),
        "wrong derivative units must be rejected"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn pinned_callback_calls_reject_stale_keys_and_preserve_callback_errors() {
    let name = alias("num_brent");
    let source = format!(
        r#"import Rssn_numerical_callback
fn pick(x: Float, i: Int) = [x][i]
let failed = Rssn_numerical_callback.{name}(0.0, 1.0, 0.000001, 100, fn (x: Float) => pick(x, 9))
let stale = Rssn.call_ho_key("num_brent", "00000000000000000000000000000000", [to_value(0.0), to_value(1.0), to_value(0.000001), to_value(100)], [to_callback(fn (x: Float) => x)])
"#
    );
    let (program, dir) = load(&source, "errors");
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        for (name, code) in [("failed", "E0802"), ("stale", "E1005")] {
            let Value::Variant(result) = interp.eval_toplevel(name).unwrap() else {
                panic!("expected Result")
            };
            assert_eq!(result.index, 1, "{mode:?} {name}: {:?}", result.fields);
            let Value::KernelError(error) = &result.fields[0] else {
                panic!("expected OpError")
            };
            assert_eq!(error.code, code, "{mode:?} {name}");
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn generated_typed_facade_rejects_mismatched_callback_extent() {
    let name = alias("num_ode_dopri5");
    let source = format!(
        r#"import Rssn_numerical_callback
let period: Float[Time] = 1.0 s
fn wrong(t: Float[Time], y: Tensor<[2], Float[Length]>): Tensor<1, Float[Length/Time]> = y / period
let bad = Rssn_numerical_callback.{name}(0.0 s, 0.1 s, [1.0 m], 0.000001 m, 0.00001, 0.01 s, 1000, wrong)
"#
    );
    let (program, dir) = load(&source, "wrong-extent");
    assert!(
        program.diagnostics.has_errors(),
        "state and callback extent must agree"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn generated_least_squares_fits_dimensioned_vector_and_reports_budget() {
    let name = alias("num_least_squares");
    let source = format!(
        r#"import Rssn_numerical_callback
fn residual(x: Tensor<[1], Float[Length]>): Tensor<1, Float[Time]> = [x[0] / (1.0 m) * (1.0 s) - 2.0 s, x[0] / (1.0 m) * (2.0 s) - 4.0 s]
let fit = Rssn_numerical_callback.{name}([0.0 m], 0.00000001 s, 0.00000001, 0.00000001, 0.001, 100, 1000, residual)
let limited = Rssn_numerical_callback.{name}([0.0 m], 0.00000001 s, 0.00000001, 0.00000001, 0.001, 100, 1, residual)
"#
    );
    let (program, dir) = load(&source, "least-squares");
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let Value::Record(fit) = payload(interp.eval_toplevel("fit").unwrap(), mode, "fit") else {
            panic!("expected fit record")
        };
        let Value::Tensor(x) = &fit.fields["x"] else {
            panic!("expected vector")
        };
        assert!((x.get_flat(0).unwrap().as_f64().unwrap() - 2.0).abs() < 1e-5);
        assert!(matches!(fit.fields["converged"], Value::Bool(true)));
        assert!(fit.fields["evaluations"].as_f64().unwrap() > 0.0);
        let Value::Record(limited) =
            payload(interp.eval_toplevel("limited").unwrap(), mode, "limited")
        else {
            panic!("expected limited record")
        };
        assert!(matches!(limited.fields["converged"], Value::Bool(false)));
        assert!(
            matches!(&limited.fields["status"], Value::Str(status) if status.as_ref() == "evaluation_limit")
        );
        assert_eq!(limited.fields["evaluations"].as_f64(), Some(1.0));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn generated_least_squares_rejects_wrong_residual_units() {
    let name = alias("num_least_squares");
    let source = format!(
        r#"import Rssn_numerical_callback
fn residual(x: Tensor<[1], Float[Length]>): Tensor<1, Float[Length]> = x
let bad = Rssn_numerical_callback.{name}([0.0 m], 0.01 s, 0.001, 0.001, 0.001, 10, 100, residual)
"#
    );
    let (program, dir) = load(&source, "least-squares-units");
    assert!(
        program.diagnostics.has_errors(),
        "wrong residual dimension must be rejected"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
