//! End-to-end checks for runtime evidence used by quantified class calls.

use aevia_core::{Interp, Mode, compile};
use aevia_core::value::{ExactRep, Storage, Value};

fn run(source: &str, expected: &[(&str, &str)]) {
    let compiled = compile("runtime-types.aev", source);
    assert!(!compiled.diagnostics.has_errors(), "compile: {:?}", compiled.diagnostics.items);
    let module = compiled.module.expect("module");
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compiled.checked).with_buffer().with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(!diagnostics.has_errors(), "{mode:?}: {:?}", diagnostics.items);
        for (name, expected) in expected {
            match interp.eval_toplevel(name).expect("binding") {
                | Value::Str(value) => assert_eq!(value.as_ref(), *expected, "{mode:?}: {name}"),
                | other => panic!("{mode:?}: unexpected {other:?}"),
            }
        }
    }
}

#[test]
fn generic_adt_dispatch_handles_payload_and_empty_constructors() {
    run(r#"class Describe a where
    describe(x: a): String
instance Describe for Option<Int> where
    describe(x) = "integer"
instance Describe for Option<String> where
    describe(x) = "string"
let classify = fn x => describe(x)
let one = classify(Some(1))
let words = classify(Some("hi"))
let empty_int: Option<Int> = None
let empty_string: Option<String> = None
let no_int = classify(empty_int)
let no_string = classify(empty_string)
"#, &[("one", "integer"), ("words", "string"), ("no_int", "integer"), ("no_string", "string")]);
}

#[test]
fn exact_tensor_types_survive_empty_values_slices_and_arithmetic() {
    run(r#"class Describe a where
    describe(x: a): String
instance Describe for Tensor<1, Int> where
    describe(x) = "integer"
instance Describe for Tensor<1, Real> where
    describe(x) = "real"
let classify = fn x => describe(x)
let ints: Tensor<1, Int> = [1, 2, 3]
let reals: Tensor<1, Real> = [1, 2, 3]
let a = classify(ints)
let b = classify(reals)
let c = classify(zeros<Int>(0))
let d = classify(zeros<Real>(0))
let e = classify(ints[0:2])
let f = classify(reals[0:2])
let g = classify(ints + ints)
let h = classify(reals + reals)
let empty_int: Tensor<1, Int> = []
let empty_real: Tensor<1, Real> = []
let i = classify(empty_int)
let j = classify(empty_real)
let k = classify(ints[0:0])
let l = classify(reals[0:0])
let m = classify(zeros<Int>(0) + zeros<Int>(0))
let n = classify(zeros<Real>(0) + zeros<Real>(0))
"#, &[("a", "integer"), ("b", "real"), ("c", "integer"), ("d", "real"), ("e", "integer"), ("f", "real"), ("g", "integer"), ("h", "real"), ("i", "integer"), ("j", "real"), ("k", "integer"), ("l", "real"), ("m", "integer"), ("n", "real")]);
}

#[test]
fn exact_tensor_elements_keep_representation_when_materialized_and_reshaped() {
    let int_tensor = aevia_core::eval::build_tensor(vec![Value::int(1), Value::int(2)], vec![2], aevia_dim::Dim::one());
    let Value::Tensor(tensor) = int_tensor else { panic!("tensor") };
    assert!(matches!(tensor.storage, Storage::TypedExact(_, ExactRep::Int)));
    assert!(matches!(tensor.get_flat(0), Some(Value::Num(aevia_core::value::NumVal { kind: aevia_core::value::NumKind::Int(_), .. }))));
    assert!(matches!(tensor.reshape_storage(&[1, 2]), Some(Storage::TypedExact(_, ExactRep::Int))));
}

#[test]
fn mixed_integer_and_real_elements_widen_to_real() {
    let real = Value::Num(aevia_core::value::NumVal::exact(
        num_rational::BigRational::new(1.into(), 2.into()),
        aevia_dim::Dim::one(),
    ));
    let result = aevia_core::eval::build_tensor(vec![Value::int(1), real], vec![2], aevia_dim::Dim::one());
    let Value::Tensor(tensor) = result else { panic!("tensor") };
    assert!(matches!(tensor.storage, Storage::TypedExact(_, ExactRep::Real)));
    assert!(matches!(tensor.get_flat(0), Some(Value::Num(aevia_core::value::NumVal { kind: aevia_core::value::NumKind::Exact(_), .. }))));
}
