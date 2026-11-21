//! Element order and callback behavior of the binary tensor map.

use std::rc::Rc;

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::builtins::call_simple;
use aevia_core::compile;
use aevia_core::value::ExactRep;
use aevia_core::value::NativeFn;
use aevia_core::value::Storage;
use aevia_core::value::TensorData;
use aevia_core::value::Value;
use aevia_core::value::repr;
use aevia_dim::Dim;
use num_rational::BigRational;

fn load(
    source: &str,
    mode: Mode,
) -> Interp {
    let compilation = compile("map2_stream.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let mut interp = Interp::new(&compilation.checked)
        .with_mode(mode)
        .with_buffer();
    let diagnostics = interp.load(compilation.module.as_ref().unwrap());
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    interp
}

#[test]
fn binary_maps_keep_view_indices_exact_representations_and_empty_axes() {
    const PROGRAM: &str = r#"fn combine(a: Float, b: Float): Float = 2.0 * a - b

fn main(): IO Unit = do
    let base = unwrap_or(reshape<[128, 128]>(linspace(0.0, 16383.0, 16384)), zeros(128, 128))
    let left = base[2:126, ::-2]
    let right = base[3:127, 1:128:2]
    let c = map2(combine, left, right)
    let d = zip_with(combine, left, right)
    println("{shape(c)} {c[0,0]} {c[123,-1]} {c == d}")
    let wide: Tensor<1, Int> = [9007199254740993, 18446744073709551616, -9007199254740993]
    let ints = map2(fn (a, b) => a + b, wide[::-1], [1, -1, 7])
    let exact: Tensor<1, Real> = [1 / 3, 2 / 3, 5 / 7]
    let exact_sum = zip_with(fn (a, b) => a + b, exact, exact[::-1])
    println("{ints} {exact_sum}")
    let empty = zeros(0, 3)
    let out = map2(combine, empty, empty)
    println("{shape(out)} {length(out)}")
"#;
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = load(PROGRAM, mode);
        interp.run_main().unwrap();
        assert_eq!(
            interp.captured(),
            concat!(
                "[124, 64] 381.0 15747.0 true\n",
                "[-9007199254740992, 18446744073709551615, 9007199254741000] [22/21, 4/3, 22/21]\n",
                "[0, 3] 0\n",
            ),
            "{mode:?}"
        );
    }
}

#[test]
fn the_first_logical_pair_keeps_its_callback_error_span() {
    const PROGRAM: &str = r#"fn fail(a: Float, b: Float): Float =
    if a == 0.0 then at([b], 3)
    else if a == 1.0 then at([b], 4)
    else b
fn main(): IO Unit = do
    let xs = linspace(1023.0, 0.0, 1024)[::-1]
    let ys = linspace(1023.0, 0.0, 1024)
    println(zip_with(fail, xs, ys))
"#;
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = load(PROGRAM, mode);
        let error = interp.run_main().unwrap_err();
        assert_eq!(error.code, "E0802");
        let span = error.span.unwrap();
        assert_eq!(
            &PROGRAM[span.start as usize..span.end as usize],
            "at([b], 3)"
        );
        assert!(interp.captured().is_empty());
    }
}

#[test]
fn unchecked_host_buffers_keep_filter_then_zip_order() {
    let integer = |n: i64| BigRational::from_integer(n.into());
    let half = BigRational::new(1.into(), 2.into());
    let as_tensor = |data: Vec<BigRational>| {
        Value::Tensor(Rc::new(TensorData::new(
            vec![3],
            Storage::TypedExact(data, ExactRep::Int),
            Dim::one(),
            None,
        )))
    };
    let left = as_tensor(vec![integer(10), half.clone(), integer(20)]);
    let right = as_tensor(vec![half, integer(1), integer(2)]);
    let pair = Value::Native(Rc::new(NativeFn(Box::new(|_, args, _| {
        Ok(Value::string(format!(
            "{}+{}",
            repr(&args[0]),
            repr(&args[1])
        )))
    }))));
    let Value::Tensor(result) = call_simple("map2", vec![pair, left, right]).unwrap() else {
        panic!("expected a tensor");
    };
    let pairs: Vec<_> = result.elements().iter().map(repr).collect();
    assert_eq!(pairs, ["\"10+1\"", "\"20+2\""]);
}
