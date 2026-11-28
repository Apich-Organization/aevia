//! Tensor element order and numeric representation at the static RSSN boundary.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::kernel::from_val;
use aevia_core::kernel::to_val;
use aevia_core::value::ExactRep;
use aevia_core::value::NumVal;
use aevia_core::value::Storage;
use aevia_core::value::TensorData;
use aevia_core::value::Value;
use aevia_core::value::View;
use aevia_dim::Dim;
use aevia_dim::Scale;
use aevia_dim::UnitDef;
use aevia_rssn::Num;
use aevia_rssn::Val;
use num_bigint::BigInt;
use num_rational::BigRational;
use std::rc::Rc;

fn bridge_tensor(tensor: TensorData) -> Val {
    to_val(&Value::Tensor(Rc::new(tensor))).expect("tensor crosses the bridge")
}

fn assert_float_tensor(
    tensor: TensorData,
    expected_shape: &[usize],
    expected: &[f64],
) {
    let Val::Tensor { shape, data } = bridge_tensor(tensor) else {
        panic!("expected a wire tensor");
    };
    assert_eq!(shape, expected_shape);
    assert_eq!(data.len(), expected.len());
    for (actual, expected) in data.iter().zip(expected) {
        let Val::Num(Num::Float(actual)) = actual else {
            panic!("expected a dimensionless wire float, got {actual:?}");
        };
        assert_eq!(actual.to_bits(), expected.to_bits());
    }
}

#[test]
fn dense_and_strided_float_tensors_cross_as_coherent_magnitudes_in_logical_order() {
    let nan = f64::from_bits(0xfff8_0000_0000_0011);
    let values = [-0.0, 0.13, f64::INFINITY, nan, -4.0, 17.0];
    let dim = Dim::base("Length");
    let cm = Rc::new(UnitDef::linear(
        "cm",
        dim.clone(),
        Scale::parse("1/100").unwrap(),
    ));
    let dense = TensorData::new(
        vec![2, 3],
        Storage::Dense(Rc::new(values.to_vec())),
        dim,
        Some(cm),
    );
    let contiguous = dense
        .slice_view(&[(1, 2), (0, 3)])
        .expect("contiguous view");
    let transposed = dense.transposed_view().expect("transposed view");
    let reversed = transposed.reversed_view(1).expect("negative stride");
    assert!(contiguous.dense().is_some());
    assert!(reversed.dense().is_none());
    assert_float_tensor(dense, &[2, 3], &values);
    assert_float_tensor(contiguous, &[1, 3], &[nan, -4.0, 17.0]);
    assert_float_tensor(
        transposed,
        &[3, 2],
        &[-0.0, nan, 0.13, -4.0, f64::INFINITY, 17.0],
    );
    assert_float_tensor(
        reversed,
        &[3, 2],
        &[nan, -0.0, -4.0, 0.13, 17.0, f64::INFINITY],
    );
}

#[test]
fn empty_and_malformed_windows_keep_their_boundary_behavior() {
    let make = |shape, offset, strides, reversed, len| {
        TensorData::new(
            shape,
            Storage::View(View {
                data: Rc::new(vec![1.0, 2.0]),
                offset,
                strides,
                reversed,
                len,
            }),
            Dim::one(),
            None,
        )
    };
    let empty = make(vec![2, 0], 2, vec![0, 1], vec![false, false], 0);
    assert_float_tensor(empty, &[2, 0], &[]);
    let reversed = make(vec![2], 1, vec![1], vec![true], 2);
    assert_float_tensor(reversed, &[2], &[2.0, 1.0]);
    for bad in [
        make(vec![2], 0, vec![], vec![true], 2),
        make(vec![2], 0, vec![1], vec![true], 2),
        make(vec![2], 1, vec![1], vec![false], 2),
        make(vec![3], 0, vec![1], vec![false], 2),
        make(vec![usize::MAX, 2], 0, vec![2, 1], vec![false, false], 2),
    ] {
        assert_eq!(
            to_val(&Value::Tensor(Rc::new(bad))).unwrap_err().code,
            "E0809"
        );
    }
}

#[test]
fn exact_integer_tensors_preserve_integer_representation_and_precision() {
    let wide = -(BigInt::from(1) << 160usize) - BigInt::from(19);
    let ratio_values =
        [BigInt::from(0), BigInt::from(17), wide.clone()].map(BigRational::from_integer);
    let make = |storage| TensorData::new(vec![3], storage, Dim::base("Time"), None);
    let Val::Tensor { data, .. } = bridge_tensor(make(Storage::TypedExact(
        ratio_values.to_vec(),
        ExactRep::Int,
    ))) else {
        panic!("expected an integer wire tensor");
    };
    assert_eq!(
        data,
        [BigInt::from(0), BigInt::from(17), wide.clone()].map(|n| Val::Num(Num::Int(n)))
    );

    let Val::Tensor { data, .. } = bridge_tensor(make(Storage::TypedExact(
        ratio_values.to_vec(),
        ExactRep::Real,
    ))) else {
        panic!("expected a real wire tensor");
    };
    assert_eq!(data, ratio_values.map(|n| Val::Num(Num::Rat(n))));

    let mixed = make(Storage::Boxed(vec![
        Value::Num(NumVal::int(wide.clone())),
        Value::Num(NumVal::float(17.0, Dim::one())),
        Value::Num(NumVal::exact(
            BigRational::new(1.into(), 3.into()),
            Dim::one(),
        )),
    ]));
    let Val::Tensor { data, .. } = bridge_tensor(mixed) else {
        panic!("expected a mixed wire tensor");
    };
    assert_eq!(
        data,
        [
            Val::Num(Num::Int(wide)),
            Val::Num(Num::Float(17.0)),
            Val::Num(Num::Rat(BigRational::new(1.into(), 3.into()))),
        ]
    );

    let bad = make(Storage::TypedExact(
        vec![
            BigRational::from_integer(1.into()),
            BigRational::new(7.into(), 2.into()),
            BigRational::from_integer(3.into()),
        ],
        ExactRep::Int,
    ));
    assert_eq!(
        to_val(&Value::Tensor(Rc::new(bad))).unwrap_err().code,
        "E0809"
    );
}

#[test]
fn return_fallback_preserves_empty_exact_mixed_and_malformed_values() {
    let dim = Dim::base("Time");
    let empty = Val::Tensor {
        shape: vec![2, 0],
        data: Vec::new(),
    };
    let Value::Tensor(empty) = from_val(&empty, dim.clone()) else {
        panic!("expected an empty tensor")
    };
    assert!(matches!(&empty.storage, Storage::Boxed(data) if data.is_empty()));

    let wide = (BigInt::from(1) << 160_usize) + BigInt::from(17);
    let fraction = BigRational::new(2.into(), 3.into());
    let exact_and_mixed = [
        vec![
            Val::Num(Num::Int(wide.clone())),
            Val::Num(Num::Rat(fraction.clone())),
        ],
        vec![
            Val::Num(Num::Float(8.25)),
            Val::Num(Num::Int(wide)),
            Val::Num(Num::Rat(fraction)),
        ],
    ];
    for data in exact_and_mixed {
        let expected = data
            .iter()
            .map(|value| from_val(value, dim.clone()))
            .collect::<Vec<_>>();
        let Value::Tensor(tensor) = from_val(&Val::list(data), dim.clone()) else {
            panic!("expected a tensor")
        };
        let Storage::Boxed(items) = &tensor.storage else {
            panic!("exact and mixed Float data must remain boxed")
        };
        assert_eq!(items.len(), expected.len());
        for (actual, expected) in items.iter().zip(&expected) {
            assert_eq!(
                aevia_core::value::repr(actual),
                aevia_core::value::repr(expected)
            );
            assert_eq!(actual.as_num().unwrap().dim, dim);
        }
    }

    for shape in [vec![2, 2], vec![usize::MAX, 2]] {
        let malformed = Val::Tensor {
            shape,
            data: vec![Val::Num(Num::Float(1.0))],
        };
        assert!(matches!(from_val(&malformed, dim.clone()), Value::Error));
    }
}

#[test]
fn a_pinned_dimensioned_rssn_operator_returns_dense_storage() {
    const SOURCE: &str =
        "fn main() = Rssn.num_matfun_sqrtm([[4.0 m^2, 0.0 m^2], [0.0 m^2, 9.0 m^2]])\n";
    let compilation = aevia_core::compile("rssn-dense-return.aev", SOURCE);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        assert!(
            !interp
                .load(compilation.module.as_ref().unwrap())
                .has_errors()
        );
        let Some(Value::Tensor(result)) = interp.run_main().unwrap() else {
            panic!("the pinned matrix square root did not return a tensor")
        };
        assert_eq!(result.shape, [2, 2]);
        assert_eq!(result.dim, Dim::base("Length"));
        assert!(result.display.is_none());
        let values = result.dense().unwrap();
        for (actual, expected) in values.iter().zip([2.0, 0.0, 0.0, 3.0]) {
            assert!((actual - expected).abs() < 1e-8);
        }
    }
}

#[test]
fn a_pinned_dimensioned_rssn_operator_accepts_large_dense_float_input() {
    const SOURCE: &str = "fn main() = Rssn.num_l1_norm(par_tabulate(512, fn i => -1.25 m))\n";
    let compiled = aevia_core::compile("rssn-dense-input.aev", SOURCE);
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:?}",
        compiled.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compiled.checked).with_mode(mode);
        assert!(!interp.load(compiled.module.as_ref().unwrap()).has_errors());
        let Some(Value::Num(result)) = interp.run_main().unwrap() else {
            panic!("expected a measured norm");
        };
        assert_eq!(result.dim, Dim::base("Length"));
        assert_eq!(Value::Num(result).as_f64(), Some(640.0));
    }
}

#[test]
fn witnessed_float_tensor_decoding_keeps_its_extent_and_dimension() {
    const SOURCE: &str = r#"fn main() =
    match from_value<Tensor<[2], Float[Length]>>(to_value([1.25 m, -2.5 m])) with
    | Ok(result) -> result
    | Err(_) -> [0.0 m, 0.0 m]
"#;
    let compiled = aevia_core::compile("rssn-witnessed-return.aev", SOURCE);
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:?}",
        compiled.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compiled.checked).with_mode(mode);
        assert!(!interp.load(compiled.module.as_ref().unwrap()).has_errors());
        let Some(Value::Tensor(result)) = interp.run_main().unwrap() else {
            panic!("witnessed conversion did not return a tensor")
        };
        assert_eq!(result.shape, [2]);
        assert_eq!(result.dim, Dim::base("Length"));
        assert_eq!(result.dense().unwrap(), &[1.25, -2.5]);
    }
}
