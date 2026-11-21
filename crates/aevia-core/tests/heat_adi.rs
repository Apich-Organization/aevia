//! The short Aevia heat solver's stiff time step through AST and IR.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_dim::Dim;
use std::path::Path;

fn assert_frames(
    result: Value,
    expected_centers: &[f64],
    mode: Mode,
) {
    let Value::Variant(result) = result else {
        panic!("{mode:?} expected a Result")
    };
    assert_eq!(result.name, "Ok", "{mode:?} {result:?}");
    let [Value::Tensor(frames)] = result.fields.as_slice() else {
        panic!("{mode:?} expected tensor of snapshots")
    };
    assert_eq!(frames.shape, [expected_centers.len()]);
    let edges = [4.0, 5.0, 6.0, 5.0, 0.0, 7.0, 6.0, 7.0, 8.0];
    for (step, &center) in expected_centers.iter().enumerate() {
        let Value::Record(frame) = frames.get_flat(step).unwrap() else {
            panic!("{mode:?} frame {step} must be a record")
        };
        let Value::Tensor(values) = &frame.fields["values"] else {
            panic!("{mode:?} frame {step} must contain a tensor")
        };
        assert_eq!(values.shape, [3, 3]);
        assert_eq!(values.dim, Dim::one());
        for (index, edge) in edges.iter().enumerate() {
            let got = values.get_flat(index).unwrap().as_f64().unwrap();
            if index == 4 {
                assert!((got - center).abs() < 2e-13, "{mode:?} step {step}: {got}");
            } else {
                assert_eq!(got, *edge, "{mode:?} step {step}, edge {index}");
            }
        }
    }
}

#[test]
fn the_stiff_heat_mode_preserves_snapshots_and_initial_edges() {
    let source = r#"import Pde { solve_heat }
let grid = { nx = 3, ny = 3, h = 1.0 }
let initial = [[4.0, 5.0, 6.0], [5.0, 7.0, 7.0], [6.0, 7.0, 8.0]]
let stiff = solve_heat({ grid = grid, alpha = 1.0, dt = 2.0, steps = 2, initial = initial, method = "adi" })
let no_steps = solve_heat({ grid = grid, alpha = 1.0, dt = 2.0, steps = 0, initial = initial, method = "adi" })
let explicit = solve_heat({ grid = grid, alpha = 1.0, dt = 0.2, steps = 1, initial = initial })
let named_explicit = solve_heat({ grid = grid, alpha = 1.0, dt = 0.2, steps = 1, initial = initial, method = "explicit" })
let beyond_cfl = solve_heat({ grid = grid, alpha = 1.0, dt = 2.0, steps = 2, initial = initial })
let wrong_method = solve_heat({ grid = grid, alpha = 1.0, dt = 2.0, steps = 2, initial = initial, method = "unknown" })
let wrong_time = solve_heat({ grid = grid, alpha = 1.0, dt = 0.0, steps = 1, initial = initial, method = "adi" })
"#;
    let program = driver::load_with_source(Path::new("heat_adi.aev"), source);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics
    );

    // Subtract the steady harmonic field 4 + x + y. Only the center is
    // nonzero, so the two directional CN factors are both (1 - r)/(1 + r).
    let expected = [7.0, 6.0 + 1.0 / 9.0, 6.0 + 1.0 / 81.0];
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let loads = driver::run_modules(&mut interp, &program);
        assert!(!loads.has_errors(), "{mode:?} {loads:?}");
        assert_frames(interp.eval_toplevel("stiff").unwrap(), &expected, mode);
        assert_frames(interp.eval_toplevel("no_steps").unwrap(), &[7.0], mode);
        for name in ["explicit", "named_explicit"] {
            assert_frames(interp.eval_toplevel(name).unwrap(), &[7.0, 6.2], mode);
        }
        for name in ["beyond_cfl", "wrong_method", "wrong_time"] {
            let Value::Variant(result) = interp.eval_toplevel(name).unwrap() else {
                panic!("{mode:?} {name} must return a Result")
            };
            assert_eq!(result.name, "Err", "{mode:?} {name}");
            let [Value::Variant(error)] = result.fields.as_slice() else {
                panic!("{mode:?} {name} must return a PdeError")
            };
            assert_eq!(error.name, "InvalidSpec", "{mode:?} {name}");
        }
    }
}

#[test]
fn the_generated_heat_guide_example_runs_in_both_evaluators() {
    let guide = include_str!("../../aevia-std/stdlib/Pde.aev");
    let (import, call) = guide
        .lines()
        .zip(guide.lines().skip(1))
        .find(|(import, call)| {
            import.contains("import Pde { solve_heat }") && call.contains("method = \"adi\"")
        })
        .unwrap();
    let source = format!(
        "{}\nlet answer = {}\n",
        import.strip_prefix("## ").unwrap(),
        call.strip_prefix("## ").unwrap()
    );
    let program = driver::load_with_source(Path::new("heat_guide.aev"), &source);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        assert!(!driver::run_modules(&mut interp, &program).has_errors());
        let Value::Variant(result) = interp.eval_toplevel("answer").unwrap() else {
            panic!("{mode:?} expected a Result");
        };
        assert_eq!(result.name, "Ok");
        let [Value::Tensor(frames)] = result.fields.as_slice() else {
            panic!("{mode:?} expected snapshots");
        };
        assert_eq!(frames.shape, [2]);
        let Value::Record(last) = frames.get_flat(1).unwrap() else {
            panic!("{mode:?} expected a field");
        };
        let Value::Tensor(values) = &last.fields["values"] else {
            panic!("{mode:?} expected field samples");
        };
        let center = values.get_flat(4).unwrap().as_f64().unwrap();
        assert!((center - 1.0 / 9.0).abs() < 1e-14);
    }
}
