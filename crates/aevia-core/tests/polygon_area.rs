//! Built-in polygon area through checked AST and IR execution.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::driver;
use aevia_core::value::Storage;
use aevia_core::value::TensorData;
use aevia_core::value::Value;
use aevia_dim::Dim;
use aevia_syntax::Span;
use std::path::Path;
use std::rc::Rc;

#[test]
fn translated_polygons_keep_area_and_squared_units_in_both_evaluators() {
    let source = r#"open Units.SI
import Math.Geometry { polygon_area }
let vertices = [
    [18014398509481984.0 m, 18014398509481984.0 m],
    [18014398509481988.0 m, 18014398509481984.0 m],
    [18014398509481988.0 m, 18014398509481988.0 m],
    [18014398509481984.0 m, 18014398509481988.0 m],
]
let direct: Real[Length^2] = polygon_area(vertices)
let reverse = polygon_area(vertices[::-1, :])
let swapped = polygon_area(vertices[:, ::-1])
let thin = polygon_area([[0.0 m, 0.0 m], [134217728.0 m, 134217727.0 m], [134217729.0 m, 134217728.0 m]])
"#;
    let program = driver::load_with_source(Path::new("polygon_area_scale.aev"), source);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let loads = driver::run_modules(&mut interp, &program);
        assert!(!loads.has_errors(), "{mode:?} {loads:?}");
        let length = Dim::base("Length");
        let area_dimension = length.mul(&length);
        for (name, expected) in [
            ("direct", 16.0),
            ("reverse", 16.0),
            ("swapped", 16.0),
            ("thin", 0.5),
        ] {
            let Value::Num(result) = interp.eval_toplevel(name).unwrap() else {
                panic!("{mode:?} {name} must be numeric");
            };
            assert_eq!(result.to_f64(), Some(expected), "{mode:?} {name}");
            assert_eq!(result.dim, area_dimension, "{mode:?} {name}");
        }
        let empty = Value::Tensor(Rc::new(TensorData::new(
            vec![0, 2],
            Storage::Dense(Rc::new(Vec::new())),
            length,
            None,
        )));
        let value = interp
            .call_named("polygon_area", vec![empty], Span::default())
            .unwrap();
        assert_eq!(value.as_f64(), Some(0.0));
        assert_eq!(value.as_num().unwrap().dim, area_dimension);
    }
}

#[test]
fn polygon_area_rejects_rows_with_other_than_two_coordinates() {
    let source = "let bad = polygon_area([[0.0, 0.0, 7.0], [1.0, 0.0, 7.0], [0.0, 1.0, 7.0]])\n";
    let program = compile("polygon_shape.aev", source);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.checked).with_mode(mode);
        let diagnostics = interp.load(program.module.as_ref().unwrap());
        assert!(
            diagnostics.items.iter().any(|diagnostic| {
                diagnostic.code == "E0802" && diagnostic.message.contains("shape [n, 2]")
            }),
            "{mode:?} {diagnostics:?}"
        );
    }
}
