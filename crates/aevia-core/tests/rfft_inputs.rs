//! Real forward transforms through the typed AST and IR interpreters.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::NumKind;
use aevia_core::value::NumVal;
use aevia_core::value::TensorData;
use aevia_core::value::Value;
use aevia_dim::Dim;
use std::borrow::Cow;
use std::rc::Rc;

fn tensor(
    interp: &mut Interp,
    name: &str,
) -> Rc<TensorData> {
    let Value::Tensor(tensor) = interp.eval_toplevel(name).unwrap() else {
        panic!("{name} should be a tensor");
    };
    tensor
}

#[test]
fn real_forward_fft_preserves_dims_and_view_order() {
    let source = r#"let dense = [1.0 m, 2.0 m, 3.0 m, 4.0 m]
let view = reverse_axis(dense, 0)
let slice = dense[1:3]
let ints: Tensor<1, Int> = [1, 2, 3, 4]
let signal = rfft(dense)
let reversed = rfft(view)
let sliced = rfft(slice)
let exact_input = rfft(ints)
let matrix = [[1.0 m, 2.0 m, 3.0 m], [4.0 m, 5.0 m, 6.0 m]]
let transposed = transpose(matrix)
let axis = rfftn_axes(transposed, [0])
let plane = rfft2(matrix)
let all = rfftn(matrix)
"#;
    let compilation = compile("rfft_inputs.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics
    );
    let length = Dim::base("Length");
    let vector = [(10.0, 0.0), (-2.0, 2.0), (-2.0, 0.0)];
    let reversed = [(10.0, 0.0), (2.0, -2.0), (2.0, 0.0)];
    let imaginary = 3.0_f64.sqrt() * 0.5;
    let axis = [
        (6.0, 0.0),
        (15.0, 0.0),
        (-1.5, imaginary),
        (-1.5, imaginary),
    ];
    let plane = [
        (21.0, 0.0),
        (-3.0, 2.0 * imaginary),
        (-9.0, 0.0),
        (0.0, 0.0),
    ];
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        assert!(tensor(&mut interp, "dense").storage.is_dense(), "{mode:?}");
        for name in ["slice", "view", "transposed"] {
            assert!(
                tensor(&mut interp, name).storage.is_view(),
                "{mode:?} {name}"
            );
        }
        for name in ["dense", "slice"] {
            assert!(
                matches!(
                    tensor(&mut interp, name).dense_flat(),
                    Some(Cow::Borrowed(_))
                ),
                "{mode:?} {name}",
            );
        }
        for name in ["view", "transposed"] {
            assert!(
                matches!(tensor(&mut interp, name).dense_flat(), Some(Cow::Owned(_))),
                "{mode:?} {name}",
            );
        }
        assert!(!tensor(&mut interp, "ints").storage.is_dense(), "{mode:?}");
        for (name, shape, bins, dim) in [
            ("signal", &[3][..], &vector[..], length.clone()),
            ("reversed", &[3][..], &reversed[..], length.clone()),
            (
                "sliced",
                &[2][..],
                &[(5.0, 0.0), (-1.0, 0.0)][..],
                length.clone(),
            ),
            ("exact_input", &[3][..], &vector[..], Dim::one()),
            ("axis", &[2, 2][..], &axis[..], length.clone()),
            ("plane", &[2, 2][..], &plane[..], length.clone()),
            ("all", &[2, 2][..], &plane[..], length.clone()),
        ] {
            let output = tensor(&mut interp, name);
            assert_eq!(output.shape, shape, "{mode:?} {name}");
            assert_eq!(output.dim, dim, "{mode:?} {name}");
            assert_eq!(output.storage.len(), bins.len(), "{mode:?} {name}");
            for (index, &(real, imaginary)) in bins.iter().enumerate() {
                let Some(Value::Num(NumVal {
                    kind: NumKind::Complex(re, im),
                    dim: found,
                    ..
                })) = output.get_flat(index)
                else {
                    panic!("expected a complex bin in {name}");
                };
                assert_eq!(found, dim, "{mode:?} {name} bin {index}");
                assert!(
                    (re - real).abs() < 1e-10 && (im - imaginary).abs() < 1e-10,
                    "{mode:?} {name} bin {index}: {re}+{im}i"
                );
            }
        }
    }
}

#[test]
fn real_forward_fft_preserves_empty_shapes_and_axis_errors() {
    for (expression, error) in [
        ("rfftn_axes(zeros(0, 4), [1])", None),
        ("rfftn_axes(zeros(0, 4), [0])", Some("nonempty")),
        ("rfftn_axes(zeros(0, 4), [1, 1])", Some("listed twice")),
    ] {
        let source = format!("let spectrum = {expression}\n");
        let compilation = compile("rfft_shapes.aev", &source);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics
        );
        for mode in [Mode::Ast, Mode::Ir] {
            let mut interp = Interp::new(&compilation.checked).with_mode(mode);
            let diagnostics = interp.load(compilation.module.as_ref().unwrap());
            if let Some(message) = error {
                assert!(
                    diagnostics.items.iter().any(|diagnostic| {
                        diagnostic.code == "E0802" && diagnostic.message.contains(message)
                    }),
                    "{mode:?} {expression}: {diagnostics:?}"
                );
            } else {
                assert!(!diagnostics.has_errors(), "{mode:?} {diagnostics:?}");
                let output = tensor(&mut interp, "spectrum");
                assert_eq!(output.shape, [0, 3], "{mode:?}");
                assert_eq!(output.storage.len(), 0, "{mode:?}");
            }
        }
    }
}
