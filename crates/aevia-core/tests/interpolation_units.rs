//! Unit-aware coverage of the clamped bilinear interpolation boundary.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::driver;
use aevia_core::value::Storage;
use aevia_core::value::Value;
use aevia_dim::Dim;
use std::path::Path;

#[test]
fn interp2_extreme_axes_keep_independent_units_and_clamped_edges() {
    let source = r#"let xs = [-1.5e308 m, 1.5e308 m]
let ys = [0.0 s, 4.0 s]
let grid = [[0.0 kg, 2.0 kg], [6000.0 g, 8000.0 g]]
let middle = interp2(xs, ys, grid, 0.0 m, 2000.0 ms)
let edge = interp2(xs, ys, grid, 1.6e308 m, -1.0 s)
let ordinary = interp2([0.0 m, 2.0 m, 5.0 m], [0.0 s, 1.0 s],
    [[0.0 kg, 2.0 kg, 5.0 kg], [4.0 kg, 6.0 kg, 9.0 kg]], 1.0 m, 0.5 s)
"#;
    let compilation = compile("interpolation_units.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interpreter = Interp::new(&compilation.checked).with_mode(mode);
        let loads = interpreter.load(compilation.module.as_ref().expect("a parsed module"));
        assert!(!loads.has_errors(), "{loads:?}");
        for (name, expected) in [("middle", 4.0), ("edge", 2.0), ("ordinary", 3.0)] {
            let value = interpreter.eval_toplevel(name).unwrap();
            let Value::Num(number) = value else {
                panic!("{name} should be numeric");
            };
            assert_eq!(number.dim, Dim::base("Mass"), "{mode:?} {name}");
            assert_eq!(number.to_f64(), Some(expected), "{mode:?} {name}");
        }
    }
}

#[test]
fn interp2_requires_the_query_to_match_its_axis_dimension() {
    let compilation = compile(
        "interpolation_wrong_axis.aev",
        "let bad = interp2([0.0 m, 2.0 m], [0.0 s, 1.0 s], [[0.0 kg, 2.0 kg], [6.0 kg, 8.0 kg]], 1.0 s, 0.5 s)\n",
    );
    assert!(
        compilation
            .diagnostics
            .items
            .iter()
            .any(|diagnostic| diagnostic.code == "E0401"),
        "{:?}",
        compilation.diagnostics,
    );
}

#[test]
fn interp2_batch_import_preserves_order_float_dtype_and_grid_dimension() {
    let source = r#"import Interpolate { interp2Batch }
let xs = [0.0 m, 2.0 m, 5.0 m]
let ys = [0.0 s, 1.0 s]
let grid = [[0.0 kg, 2000.0 g, 5.0 kg], [4.0 kg, 6.0 kg, 9.0 kg]]
let query_xs = reverse_axis([1.0 m, -1.0 m, 1.0 m, 7.0 m], 0)
let query_ys = reverse_axis([0.5 s, 500.0 ms, 0.5 s, 2.0 s], 0)
let batch = interp2Batch(xs, ys, grid, query_xs, query_ys)
let extreme = interp2Batch([-1.5e308 m, 1.5e308 m], ys,
    [[0.0 kg, 2.0 kg], [6.0 kg, 8.0 kg]], [0.0 m], [500.0 ms])
let empty_x: Tensor<1, Float[L]> = []
let empty_y: Tensor<1, Float[T]> = []
let empty = interp2Batch(xs, ys, grid, empty_x, empty_y)
let from_ints: Tensor<1, Float[1]> =
    interp2Batch([0, 2], [0, 2], [[0, 2], [6, 8]], [1], [1])
"#;
    let program = driver::load_with_source(Path::new("interpolation_batch.aev"), source);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interpreter = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let loads = driver::run_modules(&mut interpreter, &program);
        assert!(!loads.has_errors(), "{loads:?}");
        for (name, expected, dim) in [
            ("batch", &[9.0, 3.0, 2.0, 3.0][..], Dim::base("Mass")),
            ("extreme", &[4.0], Dim::base("Mass")),
            ("empty", &[], Dim::base("Mass")),
            ("from_ints", &[4.0], Dim::one()),
        ] {
            let Value::Tensor(tensor) = interpreter.eval_toplevel(name).unwrap() else {
                panic!("{name} should be a tensor");
            };
            assert_eq!(tensor.shape, [expected.len()], "{mode:?} {name}");
            assert_eq!(tensor.dim, dim, "{mode:?} {name}");
            let Storage::Dense(values) = &tensor.storage else {
                panic!("{name} should use Float storage");
            };
            assert_eq!(values.as_slice(), expected, "{mode:?} {name}");
        }
    }
}

#[test]
fn interp2_batch_requires_each_query_dimension() {
    let prefix = r#"let xs = [0.0 m, 2.0 m]
let ys = [0.0 s, 1.0 s]
let grid = [[0.0 kg, 2.0 kg], [6.0 kg, 8.0 kg]]
"#;
    for (x, y) in [("[1.0 s]", "[0.5 s]"), ("[1.0 m]", "[0.5 m]")] {
        let source = format!("{prefix}let bad = interp2Batch(xs, ys, grid, {x}, {y})\n");
        let compilation = compile("interpolation_batch_wrong_axis.aev", &source);
        assert!(
            compilation
                .diagnostics
                .items
                .iter()
                .any(|diagnostic| diagnostic.code == "E0401"),
            "{:?}",
            compilation.diagnostics,
        );
    }
}

#[test]
fn interp2_batch_rejects_bad_shapes_and_nonfinite_coordinates() {
    let prefix = r#"let xs = [0.0, 2.0, 5.0]
let ys = [0.0, 1.0]
let grid = [[0.0, 2.0, 5.0], [4.0, 6.0, 9.0]]
let empty: Tensor<1, Float[1]> = []
"#;
    for (call, code) in [
        ("interp2Batch(xs, ys, grid, [1.0], [0.5, 1.0])", "E0802"),
        (
            "interp2Batch(xs, ys, transpose(grid), [1.0], [0.5])",
            "E0802",
        ),
        (
            "interp2Batch(empty, ys, zeros(2, 0), [1.0], [0.5])",
            "E0802",
        ),
        (
            "interp2Batch([0.0], ys, [[2.0], [4.0]], [1.0], [0.5])",
            "E0806",
        ),
        (
            "interp2Batch([0.0, 2.0, 2.0], ys, grid, [1.0], [0.5])",
            "E0806",
        ),
        (
            "interp2Batch(xs, ys, grid, [1.0, nan], [0.5, 1.0])",
            "E0806",
        ),
    ] {
        let source = format!("{prefix}let bad = {call}\n");
        let compilation = compile("interpolation_batch_rejected.aev", &source);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{call}: {:?}",
            compilation.diagnostics,
        );
        for mode in [Mode::Ast, Mode::Ir] {
            let mut interpreter = Interp::new(&compilation.checked).with_mode(mode);
            let diagnostics = interpreter.load(compilation.module.as_ref().unwrap());
            assert!(
                diagnostics
                    .items
                    .iter()
                    .any(|diagnostic| diagnostic.code == code),
                "{call} in {mode:?}: {diagnostics:?}"
            );
        }
    }
}
