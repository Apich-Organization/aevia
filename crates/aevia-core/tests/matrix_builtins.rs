//! Integration tests for `aevia_core::builtins::matrix` dense fast paths.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::NumKind;
use aevia_core::value::NumVal;
use aevia_core::value::Storage;
use aevia_core::value::Value;

fn eval_result(source: &str) -> Result<Value, String> {
    let compilation = compile("matrix_builtins.aev", source);
    if compilation.diagnostics.has_errors() {
        return Err(format!("{:?}", compilation.diagnostics.items));
    }
    let module = compilation.module.expect("a parsed module");
    let mut interp = Interp::new(&compilation.checked)
        .with_buffer()
        .with_mode(Mode::Ir);
    let diags = interp.load(&module);
    if diags.has_errors() {
        return Err(format!("{:?}", diags.items));
    }
    interp.eval_toplevel("result").map_err(|e| e.message)
}

#[test]
fn fill_and_set_preserve_dense_float_storage() {
    let val = eval_result(
        r#"
module M
let m = fill(1.5, 2, 3)
let result = set(m, [1, 2], 9.25)
"#,
    )
    .expect("evaluates");
    let Value::Tensor(tensor) = val else {
        panic!("expected tensor");
    };
    assert!(matches!(tensor.storage, Storage::Dense(_)));
    assert_eq!(tensor.dense().unwrap(), &[1.5, 1.5, 1.5, 1.5, 1.5, 9.25]);
}

#[test]
fn trace_dense_and_transposed_view_tensors() {
    let val = eval_result(
        r#"
module M
let m = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]
let result = trace(m) + trace(transpose(m))
"#,
    )
    .expect("evaluates");
    let Value::Num(NumVal {
        kind: NumKind::Float(sum),
        ..
    }) = val
    else {
        panic!("expected float");
    };
    assert!((sum - 12.0).abs() < 1e-12);
}

#[test]
fn matrix_operations_example_compiles_and_runs() {
    let source = include_str!("../../../examples/matrix_operations.aev");
    let compilation = compile("examples/matrix_operations.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "compile errors: {:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.expect("a parsed module");
    let mut interp = Interp::new(&compilation.checked)
        .with_buffer()
        .with_mode(Mode::Ir);
    let diags = interp.load(&module);
    assert!(!diags.has_errors(), "load errors: {:?}", diags.items);
    interp.run_main().expect("main runs");
}
