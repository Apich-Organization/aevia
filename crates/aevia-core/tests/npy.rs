//! NPY read/write (§8.11): the last two chapter-8 names, implemented locally.
//!
//! The codec is hand-written (magic, version-1.0 header dict, little-endian
//! payload) because no binary IO existed: `IoAction` carried UTF-8 text only.

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::builtins::npy_decode;
use aevia_core::builtins::npy_encode;
use aevia_core::compile;
use aevia_core::value::IoAction;
use aevia_core::value::Storage;
use aevia_core::value::TensorData;
use aevia_core::value::Value;
use aevia_syntax::Span;
use num_bigint::BigInt;
use num_rational::BigRational;
use std::rc::Rc;

fn tensor(
    shape: Vec<usize>,
    values: Vec<f64>,
) -> TensorData {
    TensorData {
        shape,
        storage: Storage::Dense(Rc::new(values)),
        dim: aevia_dim::Dim::one(),
        display: None,
    }
}

fn dense(tensor: &TensorData) -> Vec<f64> {
    match &tensor.storage {
        | Storage::Dense(values) => values.as_ref().clone(),
        | other => panic!("expected dense storage, found {other:?}"),
    }
}

fn exact_tensor(
    shape: Vec<usize>,
    values: Vec<i64>,
) -> TensorData {
    TensorData {
        shape,
        storage: Storage::Exact(
            values
                .into_iter()
                .map(|value| BigRational::from_integer(BigInt::from(value)))
                .collect(),
        ),
        dim: aevia_dim::Dim::one(),
        display: None,
    }
}

/// A `.npy` file with `header` (no padding) and `data`, padded the way NumPy
/// writes it.
fn file_with(
    header: &str,
    data: &[u8],
) -> Vec<u8> {
    let mut header = header.to_string();
    let padding = (64 - ((10 + header.len() + 1) % 64)) % 64;
    header.push_str(&" ".repeat(padding));
    header.push('\n');
    let mut bytes = b"\x93NUMPY".to_vec();
    bytes.push(1);
    bytes.push(0);
    bytes.extend_from_slice(
        &u16::try_from(header.len())
            .expect("small header")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(data);
    bytes
}

// ---------------------------------------------------------------------------
// round trips
// ---------------------------------------------------------------------------

#[test]
fn float64_2d_and_1d_round_trip() {
    let values = vec![1.0, 2.0, 3.0, 4.5, -5.0, 6.25];
    let source = tensor(vec![3, 2], values.clone());
    let bytes = npy_encode(&source).expect("encode");
    let decoded = npy_decode(&bytes).expect("decode");
    assert_eq!(decoded.shape, vec![3, 2]);
    assert_eq!(dense(&decoded), values);

    let line = tensor(vec![4], vec![0.5, -1.5, 2.0, 3.5]);
    let decoded = npy_decode(&npy_encode(&line).expect("encode")).expect("decode");
    assert_eq!(decoded.shape, vec![4]);
    assert_eq!(dense(&decoded), vec![0.5, -1.5, 2.0, 3.5]);
}

#[test]
fn an_empty_and_a_scalar_tensor_round_trip() {
    let empty = tensor(vec![0], Vec::new());
    let decoded = npy_decode(&npy_encode(&empty).expect("encode")).expect("decode");
    assert_eq!(decoded.shape, vec![0]);
    assert!(dense(&decoded).is_empty());

    let scalar = tensor(Vec::new(), vec![7.5]);
    let decoded = npy_decode(&npy_encode(&scalar).expect("encode")).expect("decode");
    assert!(decoded.shape.is_empty());
    assert_eq!(dense(&decoded), vec![7.5]);
}

#[test]
fn float32_is_promoted_to_float_storage() {
    let mut data = Vec::new();
    data.extend_from_slice(&1.5f32.to_le_bytes());
    data.extend_from_slice(&(-2.25f32).to_le_bytes());
    let bytes = file_with(
        "{'descr': '<f4', 'fortran_order': False, 'shape': (2,), }",
        &data,
    );
    let decoded = npy_decode(&bytes).expect("decode");
    assert_eq!(decoded.shape, vec![2]);
    assert_eq!(dense(&decoded), vec![1.5, -2.25]);
}

/// A known 3×2 tensor writes exactly the header NumPy would write: magic,
/// version 1.0, the header dict padded to a 64-byte data boundary.
#[test]
fn a_known_header_is_byte_exact() {
    let bytes =
        npy_encode(&tensor(vec![3, 2], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0])).expect("encode");
    assert_eq!(&bytes[..6], b"\x93NUMPY");
    assert_eq!(&bytes[6..8], &[1, 0]);
    let header_len = usize::from(u16::from_le_bytes([bytes[8], bytes[9]]));
    assert_eq!(header_len, 118, "padded header length");
    assert_eq!(
        (10 + header_len) % 64,
        0,
        "data starts on a 64-byte boundary"
    );
    let header = std::str::from_utf8(&bytes[10..10 + header_len]).expect("header text");
    let mut expected = "{'descr': '<f8', 'fortran_order': False, 'shape': (3, 2), }".to_string();
    expected.push_str(&" ".repeat(58));
    expected.push('\n');
    assert_eq!(header, expected);
    assert_eq!(bytes.len(), 10 + header_len + 6 * 8);
    let first = f64::from_le_bytes(
        bytes[10 + header_len..10 + header_len + 8]
            .try_into()
            .unwrap(),
    );
    assert_eq!(first, 1.0);
}

/// The element dimension is not part of the NPY format, but the *values* are
/// written for a dense float tensor whose elements are exact rationals.
#[test]
fn an_exact_tensor_is_rejected_unless_dense_float() {
    let error = npy_encode(&exact_tensor(vec![2], vec![1, 2])).expect_err("not dense f64");
    assert!(error.starts_with("E0902:"), "found {error}");
}

// ---------------------------------------------------------------------------
// rejection paths
// ---------------------------------------------------------------------------

fn decode_error(bytes: &[u8]) -> String {
    npy_decode(bytes).expect_err("the file must be rejected")
}

#[test]
fn a_bad_magic_is_e0903() {
    let mut bytes = npy_encode(&tensor(vec![1], vec![1.0])).expect("encode");
    bytes[0] = 0;
    assert!(decode_error(&bytes).starts_with("E0903:"));
}

#[test]
fn an_unsupported_version_is_e0904() {
    let mut bytes = npy_encode(&tensor(vec![1], vec![1.0])).expect("encode");
    bytes[6] = 9;
    assert!(decode_error(&bytes).starts_with("E0904:"));
}

#[test]
fn fortran_order_and_unsupported_dtypes_are_e0904() {
    let data = [0u8; 8];
    let fortran = file_with(
        "{'descr': '<f8', 'fortran_order': True, 'shape': (1, 1), }",
        &data,
    );
    assert!(decode_error(&fortran).starts_with("E0904:"));
    let big_endian = file_with(
        "{'descr': '>f8', 'fortran_order': False, 'shape': (1, 1), }",
        &data,
    );
    assert!(decode_error(&big_endian).starts_with("E0904:"));
    let integer = file_with(
        "{'descr': '<i8', 'fortran_order': False, 'shape': (1, 1), }",
        &data,
    );
    assert!(decode_error(&integer).starts_with("E0904:"));
}

#[test]
fn truncated_or_extra_data_is_e0903() {
    let mut bytes = npy_encode(&tensor(vec![2], vec![1.0, 2.0])).expect("encode");
    bytes.pop();
    assert!(decode_error(&bytes).starts_with("E0903:"));
    let mut bytes = npy_encode(&tensor(vec![2], vec![1.0, 2.0])).expect("encode");
    bytes.push(0);
    assert!(decode_error(&bytes).starts_with("E0903:"));

    let header = "{'descr': '<f8', 'fortran_order': False, 'shape': (2,), }";
    let mut oversized = file_with(header, &[0u8; 8]);
    // Claim a 11 KiB header: the codec bounds the header before reading data.
    let len = 11u16 * 1024;
    let mut bytes = b"\x93NUMPY".to_vec();
    bytes.extend_from_slice(&[1, 0]);
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.extend_from_slice(&vec![b' '; usize::from(len)]);
    assert!(decode_error(&bytes).starts_with("E0903:"));
    oversized.clear();
}

// ---------------------------------------------------------------------------
// through the IO actions
// ---------------------------------------------------------------------------

fn interpreter() -> Interp {
    let compilation = compile("npy.aev", "fn main(): IO Unit = do\n    println(\"\")\n");
    assert!(!compilation.diagnostics.has_errors());
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = interp.load(&compilation.module.expect("a module"));
    assert!(!diagnostics.has_errors());
    interp
}

fn run_action(
    interp: &mut Interp,
    action: Value,
) -> Value {
    let Value::Io(action) = action else {
        panic!("expected an IO action, found {}", action.type_name());
    };
    interp.run_io(&action).expect("the action runs")
}

fn ok_tensor(value: Value) -> Rc<TensorData> {
    let Value::Variant(result) = value else {
        panic!("expected a Result");
    };
    assert_eq!(result.name, "Ok", "expected Ok, found {result:?}");
    match result.fields.first() {
        | Some(Value::Tensor(tensor)) => tensor.clone(),
        | other => panic!("the Ok payload is not a tensor: {other:?}"),
    }
}

fn err_text(value: &Value) -> String {
    let Value::Variant(result) = value else {
        panic!("expected a Result");
    };
    assert_eq!(result.name, "Err");
    match result.fields.first() {
        | Some(Value::Str(text)) => text.to_string(),
        | other => panic!("the Err payload is not a string: {other:?}"),
    }
}

#[test]
fn write_then_read_round_trips_through_the_io_actions() {
    let path = std::env::temp_dir().join(format!("aevia-npy-{}.npy", std::process::id()));
    let path_text = path.to_string_lossy().to_string();
    let mut interp = interpreter();
    let write = interp
        .call_named(
            "write_npy",
            vec![
                Value::string(path_text.clone()),
                Value::Tensor(Rc::new(tensor(vec![2, 2], vec![1.0, 2.0, 3.0, 4.0]))),
            ],
            Span::default(),
        )
        .expect("write_npy builds an action");
    let result = run_action(&mut interp, write);
    assert!(matches!(
        result,
        Value::Variant(ref result) if result.name == "Ok"
    ));

    let read = interp
        .call_named("read_npy", vec![Value::string(path_text)], Span::default())
        .expect("read_npy builds an action");
    let tensor = ok_tensor(run_action(&mut interp, read));
    assert_eq!(tensor.shape, vec![2, 2]);
    assert_eq!(dense(&tensor), vec![1.0, 2.0, 3.0, 4.0]);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_missing_npy_file_is_e0901_and_a_wrong_extension_is_e0904() {
    let mut interp = interpreter();
    let missing = std::env::temp_dir().join(format!("aevia-missing-{}.npy", std::process::id()));
    let read = interp
        .call_named(
            "read_npy",
            vec![Value::string(missing.to_string_lossy().to_string())],
            Span::default(),
        )
        .expect("read_npy builds an action");
    let text = err_text(&run_action(&mut interp, read));
    assert!(text.starts_with("E0901:"), "found {text}");

    let wrong = interp
        .call_named("read_npy", vec![Value::string("data.csv")], Span::default())
        .expect("read_npy builds a Result");
    assert!(err_text(&wrong).starts_with("E0904:"));
    let wrong = interp
        .call_named(
            "write_npy",
            vec![
                Value::string("data.csv"),
                Value::Tensor(Rc::new(tensor(vec![1], vec![1.0]))),
            ],
            Span::default(),
        )
        .expect("write_npy builds a Result");
    assert!(err_text(&wrong).starts_with("E0904:"));
}

/// The `plot` action type is still reachable (guards the `Plot`/`Save` names).
#[test]
fn npy_actions_are_reachable() {
    let mut interp = interpreter();
    let action = interp
        .call_named("read_npy", vec![Value::string("x.npy")], Span::default())
        .expect("read_npy");
    match action {
        | Value::Io(action) => assert!(matches!(*action, IoAction::ReadNpy(_))),
        | other => panic!("read_npy answers an IO action, found {}", other.type_name()),
    }
}
