//! Principal complex roots through the typed AST and IR interpreters.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::NumKind;
use aevia_core::value::NumVal;
use aevia_core::value::Value;
use aevia_dim::Dim;
use aevia_syntax::Span;

fn complex_parts(value: Value) -> (f64, f64, Dim) {
    let Value::Num(NumVal {
        kind: NumKind::Complex(re, im),
        dim,
        ..
    }) = value
    else {
        panic!("expected a complex scalar");
    };
    (re, im, dim)
}

#[test]
fn complex_sqrt_preserves_scale_and_the_branch_cut_in_source() {
    let source = r#"let near = complex_sqrt(complex(1.0e300, 1.0))
let large = complex_sqrt(complex(1.7e308, -1.7e308))
let above = complex_sqrt(complex(-4.0, 0.0))
let below = complex_sqrt(complex(-4.0, -1.0e-200))
"#;
    let compilation = compile("complex_sqrt_scale.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        let (re, im, dim) = complex_parts(interp.eval_toplevel("near").unwrap());
        assert_eq!(dim, Dim::one());
        assert!((re / 1e150 - 1.0).abs() <= f64::EPSILON);
        assert!((im / 5e-151 - 1.0).abs() <= f64::EPSILON);
        let (re, im, _) = complex_parts(interp.eval_toplevel("large").unwrap());
        let scale = 1.7e308_f64.sqrt();
        let real = ((2.0_f64.sqrt() + 1.0) * 0.5).sqrt() * scale;
        let imaginary = -((2.0_f64.sqrt() - 1.0) * 0.5).sqrt() * scale;
        assert!(re.to_bits().abs_diff(real.to_bits()) <= 2, "{mode:?}");
        assert!(im.to_bits().abs_diff(imaginary.to_bits()) <= 2, "{mode:?}");
        let (re, im, _) = complex_parts(interp.eval_toplevel("above").unwrap());
        assert_eq!(re.to_bits(), 0, "{mode:?}");
        assert_eq!(im.to_bits(), 2.0_f64.to_bits(), "{mode:?}");
        let (re, im, _) = complex_parts(interp.eval_toplevel("below").unwrap());
        assert!((re / 2.5e-201 - 1.0).abs() <= f64::EPSILON);
        assert_eq!(im.to_bits(), (-2.0_f64).to_bits(), "{mode:?}");
    }
}

#[test]
fn complex_sqrt_halves_the_dimension_and_preserves_existing_runtime_errors() {
    let source = "fn root_with_unit(z: Complex[L^2]): Complex[L] = complex_sqrt(z)\n";
    let compilation = compile("complex_sqrt_units.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics
    );
    let length = Dim::base("Length");
    let area = length.mul(&length);
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        let root = interp.eval_toplevel("root_with_unit").unwrap();
        let quantity = Value::Num(NumVal {
            kind: NumKind::Complex(3.0e300, -4.0e300),
            dim: area.clone(),
            display: None,
        });
        let result = interp
            .apply(root.clone(), vec![quantity], Span::default())
            .unwrap();
        let (re, im, dim) = complex_parts(result);
        assert_eq!(dim, length);
        assert!((re / 2e150 - 1.0).abs() <= f64::EPSILON);
        assert!((im / -1e150 - 1.0).abs() <= f64::EPSILON);

        for (real, expected) in [(-4.0, -2.0_f64), (0.0, -0.0)] {
            let signed = Value::Num(NumVal {
                kind: NumKind::Complex(real, -0.0),
                dim: area.clone(),
                display: None,
            });
            let (re, im, dim) = complex_parts(
                interp
                    .apply(root.clone(), vec![signed], Span::default())
                    .unwrap(),
            );
            assert_eq!(re.to_bits(), 0, "{mode:?}");
            assert_eq!(im.to_bits(), expected.to_bits(), "{mode:?}");
            assert_eq!(dim, length);
        }

        let error = interp
            .call_named("complex_sqrt", vec![Value::float(2.0)], Span::default())
            .expect_err("the runtime expects a complex argument");
        assert_eq!(error.code, "E0302");
    }
}
