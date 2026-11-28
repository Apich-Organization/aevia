//! Interoperability fixtures created by NumPy, not by the Aevia encoder.
use aevia_core::builtins::npy_decode;
use aevia_core::builtins::npy_encode;
use aevia_core::value::Storage;
use aevia_core::value::TensorData;
use std::rc::Rc;

fn values(data: &TensorData) -> Vec<f64> {
    data.dense_flat().unwrap().into_owned()
}
fn file(
    header: &str,
    payload: &[u8],
) -> Vec<u8> {
    let mut header = header.to_owned();
    let padding = (64 - ((10 + header.len() + 1) % 64)) % 64;
    header.push_str(&" ".repeat(padding));
    header.push('\n');
    let mut bytes = b"\x93NUMPY\x01\x00".to_vec();
    bytes.extend_from_slice(&(header.len() as u16).to_le_bytes());
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(payload);
    bytes
}
#[test]
fn numpy_big_endian_fortran_and_rank_three_decode() {
    let matrix = npy_decode(include_bytes!("fixtures/npy/fortran-be-f64.npy")).unwrap();
    assert_eq!(matrix.shape, vec![2, 3]);
    assert_eq!(values(&matrix), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    let cube = npy_decode(include_bytes!("fixtures/npy/rank3-le-f32.npy")).unwrap();
    assert_eq!(cube.shape, vec![2, 3, 4]);
    assert_eq!(values(&cube), (0..24).map(f64::from).collect::<Vec<_>>());
    let fortran_cube = npy_decode(include_bytes!("fixtures/npy/rank3-fortran-be-f32.npy")).unwrap();
    assert_eq!(fortran_cube.shape, cube.shape);
    assert_eq!(values(&fortran_cube), values(&cube));
    let roundtrip = npy_decode(&npy_encode(&cube).unwrap()).unwrap();
    assert_eq!(roundtrip.shape, cube.shape);
    assert_eq!(values(&roundtrip), values(&cube));
}
#[test]
fn dictionary_spacing_order_and_quoted_keys_are_not_semantic() {
    let payload = 2.5f64.to_le_bytes();
    let bytes = file(
        r#"{ "shape" : (1,), "fortran_order":False, "descr" : "<f8" }"#,
        &payload,
    );
    let decoded = npy_decode(&bytes).unwrap();
    assert_eq!(decoded.shape, vec![1]);
    assert_eq!(values(&decoded), vec![2.5]);
}
#[test]
fn malformed_shape_and_storage_are_recoverable_errors() {
    let overflow = format!(
        "{{'descr':'<f8', 'fortran_order':False, 'shape':({},2),}}",
        usize::MAX
    );
    assert!(
        npy_decode(&file(&overflow, &[]))
            .unwrap_err()
            .starts_with("E0903")
    );
    let empty_overflow = format!(
        "{{'descr':'<f8','fortran_order':False,'shape':(0,{},2),}}",
        usize::MAX
    );
    assert!(
        npy_decode(&file(&empty_overflow, &[]))
            .unwrap_err()
            .starts_with("E0903")
    );
    let view = TensorData::new(
        vec![0, usize::MAX, 2],
        Storage::View(aevia_core::value::View {
            data: Rc::new(vec![]),
            offset: 0,
            strides: vec![0, 2, 1],
            reversed: vec![false; 3],
            len: 0,
        }),
        aevia_dim::Dim::one(),
        None,
    );
    assert!(npy_encode(&view).unwrap_err().starts_with("E0902"));
    let valid_empty = TensorData::new(
        vec![0, 3, 2],
        Storage::Dense(Rc::new(vec![])),
        aevia_dim::Dim::one(),
        None,
    );
    assert_eq!(
        npy_decode(&npy_encode(&valid_empty).unwrap())
            .unwrap()
            .shape,
        vec![0, 3, 2]
    );
    let duplicate = "{'descr':'<f8','descr':'<f4','fortran_order':False,'shape':(1,),}";
    assert!(
        npy_decode(&file(duplicate, &[]))
            .unwrap_err()
            .starts_with("E0903")
    );
    let missing = "{'descr':'<f8','shape':(1,),}";
    assert!(
        npy_decode(&file(missing, &[]))
            .unwrap_err()
            .starts_with("E0903")
    );
    let truncated = file(
        "{'descr':'<f8','fortran_order':False,'shape':(1,),}",
        &[0; 7],
    );
    assert!(npy_decode(&truncated).unwrap_err().starts_with("E0903"));
    let malformed = TensorData::new(
        vec![2, 2],
        Storage::Dense(Rc::new(vec![1.0])),
        aevia_dim::Dim::one(),
        None,
    );
    assert!(npy_encode(&malformed).unwrap_err().starts_with("E0902"));
    let dimensional = TensorData::new(
        vec![1],
        Storage::Dense(Rc::new(vec![1.0])),
        aevia_dim::Dim::base("Length"),
        None,
    );
    assert!(npy_encode(&dimensional).unwrap_err().starts_with("E0902"));
}

#[test]
fn rust_table_api_and_language_codec_share_the_same_checked_contract() {
    let bytes = include_bytes!("fixtures/npy/fortran-be-f64.npy");
    let (shape, values) = aevia_std::table::read_npy(bytes).unwrap();
    let tensor = npy_decode(bytes).unwrap();
    assert_eq!(shape, tensor.shape);
    assert_eq!(values, tensor.dense_flat().unwrap().as_ref());
    let scalar = aevia_std::table::try_write_npy(&[], &[3.0]).unwrap();
    let (shape, values) = aevia_std::table::read_npy(&scalar).unwrap();
    assert!(shape.is_empty());
    assert_eq!(values, [3.0]);
    assert!(aevia_std::table::try_write_npy(&[2], &[1.0]).is_err());
}

#[test]
fn boxed_float_elements_require_real_evidence() {
    use aevia_core::value::NumVal;
    use aevia_core::value::Value;
    use aevia_types::Rep;
    use aevia_types::Type;
    let dim = aevia_dim::Dim::one();
    let boxed = TensorData::new(
        vec![2],
        Storage::Boxed(vec![
            Value::Num(NumVal::float(1.25, dim.clone())),
            Value::Num(NumVal::float(2.5, dim.clone())),
        ]),
        dim.clone(),
        None,
    );
    assert_eq!(
        values(&npy_decode(&npy_encode(&boxed).unwrap()).unwrap()),
        vec![1.25, 2.5]
    );
    let unknown = TensorData::new(vec![0], Storage::Boxed(vec![]), dim.clone(), None);
    assert!(npy_encode(&unknown).is_err());
    let proven = TensorData::checked(
        vec![0],
        Storage::Boxed(vec![]),
        dim.clone(),
        None,
        Type::Scalar(Rep::Float, dim.clone()),
    )
    .unwrap();
    assert_eq!(
        npy_decode(&npy_encode(&proven).unwrap()).unwrap().shape,
        vec![0]
    );
    let wrong = TensorData::new(vec![1], Storage::Boxed(vec![Value::Bool(true)]), dim, None);
    assert!(npy_encode(&wrong).is_err());
}
