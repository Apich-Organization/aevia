//! Host-origin nominal and empty structured schemas survive the opaque boundary.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::NumKind;
use aevia_core::value::Value;
use aevia_core::value::make_variant;
use aevia_syntax::Span;

fn result(value: Value) -> (usize, Value) {
    let Value::Variant(result) = value else {
        panic!("expected Result")
    };
    (
        result.index,
        result.fields.first().cloned().expect("result payload"),
    )
}

#[test]
fn source_variants_and_empty_structured_tensors_roundtrip_in_both_engines() {
    let source = r#"type Ticket<a> = | Ticket(a) | Blank
let ticket: Ticket<Int> = Ticket(7)
let blank: Ticket<Int> = Blank
let record = { ticket = ticket, blank = blank }
let packed = to_value(record)
let unpacked = from_value<{ ticket: Ticket<Int>, blank: Ticket<Int> }>(packed)
let empty: Tensor<1, { x: Int }> = []
let empty_packed = to_value(empty)
let empty_unpacked = from_value<Tensor<1, { x: Int }>>(empty_packed)
"#;
    let compilation = compile("nominal_roundtrip.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let (index, record) = result(interp.eval_toplevel("unpacked").unwrap());
        assert_eq!(index, 0, "{mode:?}: {record:?}");
        let Value::Record(record) = record else {
            panic!("expected record")
        };
        let Value::Variant(ticket) = &record.fields["ticket"] else {
            panic!("expected Ticket")
        };
        assert_eq!(ticket.name, "Ticket");
        assert!(matches!(
            ticket.fields[0].as_num().unwrap().kind,
            NumKind::Int(_)
        ));
        let Value::Variant(blank) = &record.fields["blank"] else {
            panic!("expected Blank")
        };
        assert_eq!(blank.name, "Blank");
        let (index, empty) = result(interp.eval_toplevel("empty_unpacked").unwrap());
        assert_eq!(index, 0, "{mode:?}: {empty:?}");
        let Value::Tensor(empty) = empty else {
            panic!("expected tensor")
        };
        assert_eq!(empty.shape, vec![0]);
    }
}

#[test]
fn host_injected_unverified_phantom_cannot_gain_a_type_from_a_variable_read() {
    let source = r#"type Phantom<a> = | Empty
fn pack(x: Phantom<Int>) = to_value(x)
fn read(v: Value) = from_value<Phantom<Int>>(v)
let converter = to_value
"#;
    let compilation = compile("phantom_host.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let adt = match compilation.checked.exports.get("Phantom").unwrap() {
        | aevia_types::Export::Type { adt } => *adt,
        | other => panic!("expected type, found {other:?}"),
    };
    let module = compilation.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        assert!(!interp.load(&module).has_errors());
        let pack = interp.eval_toplevel("pack").unwrap();
        let read = interp.eval_toplevel("read").unwrap();
        let mut host_value = make_variant(adt, 0, "Empty", Vec::new());
        if let Value::Variant(variant) = &mut host_value {
            // Mutable dispatch hints from a host are not a construction proof.
            std::rc::Rc::make_mut(variant).type_args = Some(vec![aevia_types::Type::int()]);
        }
        let opaque = interp
            .apply(pack, vec![host_value.clone()], Span::default())
            .unwrap();
        let (index, _) = result(interp.apply(read, vec![opaque], Span::default()).unwrap());
        assert_eq!(
            index, 1,
            "{mode:?}: unverified host phantom must fail closed"
        );
        let converter = interp.eval_toplevel("converter").unwrap();
        let reused_span = compilation
            .checked
            .expr_types
            .iter()
            .find_map(|(span, ty)| {
                matches!(ty, aevia_types::Type::Adt(id, _) if *id == adt).then_some(*span)
            })
            .expect("a concrete checked phantom span");
        let opaque = interp
            .apply(converter, vec![host_value], reused_span)
            .unwrap();
        let read = interp.eval_toplevel("read").unwrap();
        let (index, _) = result(interp.apply(read, vec![opaque], reused_span).unwrap());
        assert_eq!(
            index, 1,
            "{mode:?}: host apply must not treat a reused span as source proof"
        );
    }
}

#[test]
fn tampering_with_a_host_origin_payload_is_detected() {
    let source = r#"type Ticket<a> = | Ticket(a)
let original: Ticket<Int> = Ticket(7)
let packed = to_value(original)
fn read(v: Value) = from_value<Ticket<Int>>(v)
"#;
    let compilation = compile("nominal_mutation.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        assert!(!interp.load(&module).has_errors());
        let mut opaque = interp.eval_toplevel("packed").unwrap();
        let Value::KernelValue(value) = &mut opaque else {
            panic!("expected opaque value")
        };
        std::rc::Rc::make_mut(value).payload = aevia_rssn::Val::Variant {
            tag: "Other".into(),
            payload: Vec::new(),
        };
        let read = interp.eval_toplevel("read").unwrap();
        let (index, _) = result(interp.apply(read, vec![opaque], Span::default()).unwrap());
        assert_eq!(index, 1, "{mode:?}: altered payload must fail validation");
    }
}

#[test]
fn empty_structured_schema_survives_element_preserving_operations() {
    use aevia_core::value::Storage;
    use aevia_core::value::TensorData;
    use std::rc::Rc;
    let source = r#"fn a(matrix: Tensor<2, { x: Int }>) = from_value<Tensor<2, { x: Int }>>(to_value(transpose(matrix)))
fn b(matrix: Tensor<2, { x: Int }>) = from_value<Tensor<2, { x: Int }>>(to_value(matrix[0..0]))
fn c(matrix: Tensor<2, { x: Int }>) = from_value<Tensor<1, { x: Int }>>(to_value(flatten(matrix)))
fn reshaped(matrix: Tensor<2, { x: Int }>) = reshape<[0]>(matrix)
fn read_flat(v: Value) = from_value<Tensor<1, { x: Int }>>(v)
"#;
    let compilation = compile("empty_schema.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        assert!(!interp.load(&module).has_errors());
        let element = aevia_types::Type::Record(aevia_types::Row::closed([(
            "x".to_string(),
            aevia_types::Type::int(),
        )]));
        let matrix = Value::Tensor(Rc::new(
            TensorData::checked(
                vec![1, 0],
                Storage::Boxed(vec![]),
                aevia_dim::Dim::one(),
                None,
                element,
            )
            .unwrap(),
        ));
        for name in ["a", "b", "c"] {
            let function = interp.eval_toplevel(name).unwrap();
            let (index, value) = result(
                interp
                    .apply(function, vec![matrix.clone()], Span::default())
                    .unwrap(),
            );
            assert_eq!(index, 0, "{mode:?}: {name}: {value:?}");
        }
        let function = interp.eval_toplevel("reshaped").unwrap();
        let (index, reshaped) = result(
            interp
                .apply(function, vec![matrix], Span::default())
                .unwrap(),
        );
        assert_eq!(index, 0, "{mode:?}: reshape failed");
        let opaque = aevia_core::builtins::call_simple("to_value", vec![reshaped]).unwrap();
        let reader = interp.eval_toplevel("read_flat").unwrap();
        let (index, value) = result(interp.apply(reader, vec![opaque], Span::default()).unwrap());
        assert_eq!(index, 0, "{mode:?}: reshaped schema lost: {value:?}");
    }
}

#[test]
fn host_checked_empty_schema_is_explicit_and_unverified_storage_fails_closed() {
    use aevia_core::value::Storage;
    use aevia_core::value::TensorData;
    use aevia_dim::Dim;
    use aevia_types::Row;
    use aevia_types::Type;
    use std::rc::Rc;
    let element = Type::Record(Row::closed([("x".to_string(), Type::int())]));
    assert!(
        TensorData::checked(
            vec![1],
            Storage::Boxed(vec![]),
            Dim::one(),
            None,
            element.clone()
        )
        .is_err()
    );
    assert!(
        TensorData::checked(
            vec![1],
            Storage::Boxed(vec![Value::string("wrong")]),
            Dim::one(),
            None,
            Type::int()
        )
        .is_err(),
        "checked host construction must validate actual elements"
    );
    let checked =
        TensorData::checked(vec![0], Storage::Boxed(vec![]), Dim::one(), None, element).unwrap();
    let unverified = TensorData::new(vec![0], Storage::Boxed(vec![]), Dim::one(), None);
    let source = "fn read(v: Value) = from_value<Tensor<1, { x: Int }>>(v)\n";
    let compilation = compile("host_checked.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        assert!(!interp.load(&module).has_errors());
        let read = interp.eval_toplevel("read").unwrap();
        let mut tampered = checked.clone();
        tampered.shape = vec![1];
        for (tensor, expected) in [(checked.clone(), 0), (unverified.clone(), 1), (tampered, 1)] {
            let opaque = match aevia_core::builtins::call_simple(
                "to_value",
                vec![Value::Tensor(Rc::new(tensor))],
            ) {
                | Ok(value) => value,
                | Err(_) if expected == 1 => continue,
                | Err(error) => panic!("{mode:?}: {error:?}"),
            };
            let (index, _) = result(
                interp
                    .apply(read.clone(), vec![opaque], Span::default())
                    .unwrap(),
            );
            assert_eq!(index, expected, "{mode:?}");
        }
    }
}

#[test]
fn builtin_option_results_preserve_only_proven_element_schemas() {
    let source = r#"let some = find(fn (x: Int) => x > 0, [1])
let empty: Tensor<1, Int> = []
let none = find(fn (x: Int) => x > 0, empty)
let some_roundtrip = from_value<Option<Int>>(to_value(some))
let none_roundtrip = from_value<Option<Int>>(to_value(none))
let parsed_roundtrip = from_value<Result<Int, ParseError>>(to_value(parse_int("7")))
"#;
    let compilation = compile("builtin_option_witness.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        assert!(!interp.load(&module).has_errors());
        for name in ["some_roundtrip", "none_roundtrip", "parsed_roundtrip"] {
            let (index, value) = result(interp.eval_toplevel(name).unwrap());
            assert_eq!(index, 0, "{mode:?} {name}: {value:?}");
        }
    }
}

#[test]
fn builtin_identity_and_its_signature_defeat_forged_phantom_results() {
    use aevia_core::value::Storage;
    use aevia_core::value::TensorData;
    use std::rc::Rc;
    let source = r#"fn spoof(f: ((Int -> Bool), Tensor<1, Int>) -> Option<String>, p: Int -> Bool, xs: Tensor<1, Int>) = f(p, xs)
let actual = find
let predicate = fn (x: Int) => x > 0
fn read(v: Value) = from_value<Option<String>>(v)
"#;
    let compilation = compile("builtin_spoof.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        assert!(!interp.load(&module).has_errors());
        let spoof = interp.eval_toplevel("spoof").unwrap();
        let actual = interp.eval_toplevel("actual").unwrap();
        let predicate = interp.eval_toplevel("predicate").unwrap();
        let tensor = TensorData::checked(
            vec![0],
            Storage::Boxed(vec![]),
            aevia_dim::Dim::one(),
            None,
            aevia_types::Type::int(),
        )
        .unwrap();
        let value = interp
            .apply(
                spoof,
                vec![actual, predicate, Value::Tensor(Rc::new(tensor))],
                Span::default(),
            )
            .unwrap();
        let opaque = aevia_core::builtins::call_simple("to_value", vec![value]).unwrap();
        let reader = interp.eval_toplevel("read").unwrap();
        let (index, _) = result(interp.apply(reader, vec![opaque], Span::default()).unwrap());
        assert_eq!(index, 1, "{mode:?}: fabricated Option<String> evidence");
    }
}

#[test]
fn shadowed_builtin_and_host_application_do_not_get_source_call_evidence() {
    let source = r#"fn find(x: Int) = x + 1
let shadowed = find(3)
let converter = from_value
let packed = to_value(7)
let inferred: Result<Int, OpError> = from_value(packed)
"#;
    let compilation = compile("builtin_shadow_witness.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let reused_span = compilation
        .checked
        .expr_types
        .iter()
        .find_map(|(span, ty)| {
            matches!(ty, aevia_types::Type::Adt(id, _) if *id == aevia_types::adt::RESULT_ID)
                .then_some(*span)
        })
        .unwrap();
    let module = compilation.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        assert!(!interp.load(&module).has_errors());
        assert!(matches!(
            interp
                .eval_toplevel("shadowed")
                .unwrap()
                .as_num()
                .unwrap()
                .kind,
            NumKind::Int(_)
        ));
        let converter = interp.eval_toplevel("converter").unwrap();
        let packed = interp.eval_toplevel("packed").unwrap();
        let (index, _) = result(interp.apply(converter, vec![packed], reused_span).unwrap());
        assert_eq!(
            index, 1,
            "{mode:?}: host application reused source evidence"
        );
    }
}

#[test]
fn specialized_constructor_cannot_be_retyped_by_an_injected_host_signature() {
    let source = r#"type Phantom<a> = | Phantom(Int)
let make: Int -> Phantom<String> = Phantom
fn spoof(f: Int -> Phantom<Int>) = f(1)
fn read(v: Value) = from_value<Phantom<Int>>(v)
"#;
    let compilation = compile("ctor_specialization_spoof.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        assert!(!interp.load(&module).has_errors());
        let make = interp.eval_toplevel("make").unwrap();
        let spoof = interp.eval_toplevel("spoof").unwrap();
        let value = interp.apply(spoof, vec![make], Span::default()).unwrap();
        let opaque = aevia_core::builtins::call_simple("to_value", vec![value]).unwrap();
        let read = interp.eval_toplevel("read").unwrap();
        let (index, _) = result(interp.apply(read, vec![opaque], Span::default()).unwrap());
        assert_eq!(
            index, 1,
            "{mode:?}: specialized constructor lost its phantom type"
        );
    }
}
