//! Type reflection carries checked, concrete evidence rather than a record tag.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::Value;
use aevia_types::Type;

#[test]
fn concrete_type_witness_survives_aliases_and_specialization() {
    let source = "type Count = Int\nlet factory = type_rep\nlet specialized = factory<Count>\nfn identity(x) = x\nlet copy = identity(specialized)\nlet witness = copy()\nlet tensor = type_rep<Tensor<1, Float[L]>>()\nlet record = type_rep<{ x: Int, y: Float }>()\n";
    let compilation = compile("type_rep.aev", source);
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
        let witness = interp.eval_toplevel("witness").unwrap();
        let Value::TypeRep(ty) = witness else {
            panic!("{mode:?}: expected opaque TypeRep")
        };
        assert_eq!(*ty, Type::int(), "{mode:?}");
        let Value::TypeRep(tensor) = interp.eval_toplevel("tensor").unwrap() else {
            panic!("tensor witness")
        };
        assert!(
            matches!(tensor.as_ref(), Type::Tensor { shape, elem } if shape.rank() == 1 && matches!(elem.as_ref(), Type::Scalar(_, dim) if !dim.is_one())),
            "{mode:?}: {tensor}"
        );
        let Value::TypeRep(record) = interp.eval_toplevel("record").unwrap() else {
            panic!("record witness")
        };
        assert!(
            matches!(record.as_ref(), Type::Record(_)),
            "{mode:?}: {record}"
        );
    }
}

#[test]
fn missing_or_unresolved_type_evidence_is_rejected() {
    for source in ["let r = type_rep()\n", "let r = type_rep<Int>(1)\n"] {
        let compilation = compile("bad_type_rep.aev", source);
        assert!(compilation.diagnostics.has_errors(), "accepted {source}");
    }
}

#[test]
fn specialized_type_rep_survives_module_export() {
    let root = std::env::temp_dir().join(format!("aevia-type-rep-import-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("Factory.aev"),
        "module Factory\ntype Count = Int\nlet make = type_rep<Count>\n",
    )
    .unwrap();
    std::fs::write(
        root.join("main.aev"),
        "module Main\nimport Factory { make }\nlet witness = make()\n",
    )
    .unwrap();
    let program = aevia_core::driver::load(&root.join("main.aev"));
    assert!(
        !program.diagnostics.has_errors(),
        "{}",
        program.diagnostics.render(&program.sources)
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let Value::TypeRep(ty) = interp.eval_toplevel("witness").unwrap() else {
            panic!("{mode:?}: expected TypeRep")
        };
        assert_eq!(*ty, Type::int());
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn checked_from_value_specialization_alias_and_inference_match_in_both_engines() {
    let source = r#"let boxed = to_value(7)
let converter = from_value<Int>
fn identity(x) = x
let alias = identity(converter)
let good = alias(boxed)
let inferred: Result<Int, OpError> = from_value(boxed)
let bad = from_value<Float>(boxed)
"#;
    let compilation = compile("from_value.aev", source);
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
        for name in ["good", "inferred"] {
            let Value::Variant(result) = interp.eval_toplevel(name).unwrap() else {
                panic!("{name}: expected result")
            };
            assert_eq!(result.index, 0, "{mode:?}: {name}");
            assert!(matches!(
                result.fields[0].as_num().unwrap().kind,
                aevia_core::value::NumKind::Int(_)
            ));
        }
        let Value::Variant(result) = interp.eval_toplevel("bad").unwrap() else {
            panic!("expected result")
        };
        assert_eq!(result.index, 1, "{mode:?}: mismatch must fail");
        let Value::KernelError(error) = &result.fields[0] else {
            panic!("expected structured OpError")
        };
        assert_eq!(error.code, "TypeMismatch");
    }
}

#[test]
fn explicit_from_value_type_cannot_be_reinterpreted() {
    let source =
        "let boxed = to_value(7)\nlet bad: Result<Float, OpError> = from_value<Int>(boxed)\n";
    let compilation = compile("bad_from_value.aev", source);
    assert!(
        compilation
            .diagnostics
            .items
            .iter()
            .any(|item| item.code == "E0302")
    );
}
