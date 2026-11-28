//! Retained closures obey their checked lexical revision, including instances.
use aevia_core::Compilation;
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile_with_source_identity;
use aevia_core::source::SourceIdentity;
use aevia_core::value::Value;
use aevia_syntax::Span;

fn compile(source: &str) -> Compilation {
    let compiled = compile_with_source_identity(
        "revision.aev",
        source,
        &SourceIdentity::virtual_source("aevia-test://lexical/revision"),
    );
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:?}",
        compiled.diagnostics.items
    );
    compiled
}
fn install(
    interp: &mut Interp,
    compiled: &Compilation,
) {
    interp.prepare_module(&compiled.checked);
    let diagnostics = interp.load(compiled.module.as_ref().unwrap());
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
}
fn apply(
    interp: &mut Interp,
    function: Value,
) -> Value {
    interp.apply(function, vec![], Span::default()).unwrap()
}
#[test]
fn same_span_literals_and_reified_types_keep_their_original_checked_revision() {
    let first = compile("fn number(): Float = 1\nfn witness() = type_rep<Real>()\n");
    let second = compile("fn number(): Int   = 1\nfn witness() = type_rep<Bool>()\n");
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&first.checked).with_mode(mode);
        install(&mut interp, &first);
        let old_number = interp.eval_toplevel("number").unwrap();
        let old_witness = interp.eval_toplevel("witness").unwrap();
        install(&mut interp, &second);
        let new_number = interp.eval_toplevel("number").unwrap();
        let new_witness = interp.eval_toplevel("witness").unwrap();
        assert!(
            matches!(
                apply(&mut interp, old_number),
                Value::Num(aevia_core::value::NumVal {
                    kind: aevia_core::value::NumKind::Float(_),
                    ..
                })
            ),
            "{mode:?}"
        );
        assert!(
            matches!(
                apply(&mut interp, new_number),
                Value::Num(aevia_core::value::NumVal {
                    kind: aevia_core::value::NumKind::Int(_),
                    ..
                })
            ),
            "{mode:?}"
        );
        let Value::TypeRep(old) = apply(&mut interp, old_witness) else {
            panic!("old witness")
        };
        let Value::TypeRep(new) = apply(&mut interp, new_witness) else {
            panic!("new witness")
        };
        assert_eq!(
            *old,
            aevia_types::Type::real(aevia_dim::Dim::one()),
            "{mode:?}"
        );
        assert_eq!(*new, aevia_types::Type::Bool, "{mode:?}");
    }
}
#[test]
fn old_closures_and_first_class_methods_keep_their_instance_implementation() {
    let source = "type Item = | Item(Int)\nclass Read a where\n    read(x: a): Int\ninstance Read for Item where\n    read(x) = 1\nfn value(): Int = read(Item(0))\nlet method = read\nlet item = Item(0)\n";
    let first = compile(source);
    let second = compile(&source.replace("read(x) = 1", "read(x) = 2"));
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&first.checked).with_mode(mode);
        install(&mut interp, &first);
        let old = interp.eval_toplevel("value").unwrap();
        let method = interp.eval_toplevel("method").unwrap();
        let item = interp.eval_toplevel("item").unwrap();
        install(&mut interp, &second);
        let new = interp.eval_toplevel("value").unwrap();
        assert_eq!(apply(&mut interp, old).as_f64(), Some(1.0), "{mode:?}");
        assert_eq!(apply(&mut interp, new).as_f64(), Some(2.0), "{mode:?}");
        assert_eq!(
            interp
                .apply(method, vec![item], Span::default())
                .unwrap()
                .as_f64(),
            Some(1.0),
            "{mode:?}"
        );
    }
}
#[test]
fn old_unit_literals_keep_their_defining_unit_revision() {
    let source = "private unit hop : Length = 2 m\nfn value(): Float[L] = 1.0 hop\n";
    let first = compile(source);
    let second = compile(&source.replace("= 2 m", "= 3 m"));
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&first.checked).with_mode(mode);
        install(&mut interp, &first);
        let old = interp.eval_toplevel("value").unwrap();
        install(&mut interp, &second);
        let new = interp.eval_toplevel("value").unwrap();
        assert_eq!(apply(&mut interp, old).as_f64(), Some(2.0), "{mode:?}");
        assert_eq!(apply(&mut interp, new).as_f64(), Some(3.0), "{mode:?}");
    }
}
#[test]
fn foreign_or_reset_interpreters_reject_captured_evidence() {
    let compiled = compile("fn value(): Int = 7\n");
    for mode in [Mode::Ast, Mode::Ir] {
        let mut first = Interp::new(&compiled.checked).with_mode(mode);
        install(&mut first, &compiled);
        let old = first.eval_toplevel("value").unwrap();
        let mut next = Interp::new(&compiled.checked).with_mode(mode);
        let error = next
            .apply(old.clone(), vec![], Span::default())
            .unwrap_err();
        assert_eq!(error.code, "E0809");
        drop(first);
        let error = next.apply(old, vec![], Span::default()).unwrap_err();
        assert_eq!(error.code, "E0809");
    }
}

#[test]
fn retained_import_scopes_pin_instance_only_modules_and_ir_tail_contexts() {
    let directory =
        std::env::temp_dir().join(format!("aevia-evidence-imports-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let library = directory.join("Library.aev");
    let library_source = "module Library\ntype Item = | Item(Int)\nclass Read a where\n    read(x: a): Int\ninstance Read for Item where\n    read(x) = 1\n";
    std::fs::write(&library, library_source).unwrap();
    std::fs::write(
        directory.join("Reify.aev"),
        "module Reify\nfn witness() = type_rep<Real>()\n",
    )
    .unwrap();
    let entry = directory.join("main.aev");
    std::fs::write(&entry,"import Library\nimport Reify\nfn value(): Int = read(Library.Item(0))\nfn witness() = Reify.witness()\n").unwrap();
    let first = aevia_core::driver::load(&entry);
    assert!(
        !first.diagnostics.has_errors(),
        "{:?}",
        first.diagnostics.items
    );
    std::fs::write(
        &library,
        library_source.replace("read(x) = 1", "read(x) = 2"),
    )
    .unwrap();
    std::fs::write(
        directory.join("Reify.aev"),
        "module Reify\nfn witness() = type_rep<Bool>()\n",
    )
    .unwrap();
    let second = aevia_core::driver::load(&entry);
    assert!(
        !second.diagnostics.has_errors(),
        "{:?}",
        second.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&first.entry().unwrap().checked).with_mode(mode);
        assert!(!aevia_core::driver::run_modules(&mut interp, &first).has_errors());
        let old_value = interp.eval_toplevel("value").unwrap();
        let old_witness = interp.eval_toplevel("witness").unwrap();
        assert!(!aevia_core::driver::run_modules(&mut interp, &second).has_errors());
        let new_value = interp.eval_toplevel("value").unwrap();
        let new_witness = interp.eval_toplevel("witness").unwrap();
        assert_eq!(
            apply(&mut interp, old_value).as_f64(),
            Some(1.0),
            "{mode:?}"
        );
        assert_eq!(
            apply(&mut interp, new_value).as_f64(),
            Some(2.0),
            "{mode:?}"
        );
        let Value::TypeRep(old) = apply(&mut interp, old_witness) else {
            panic!("old imported witness")
        };
        let Value::TypeRep(new) = apply(&mut interp, new_witness) else {
            panic!("new imported witness")
        };
        assert_eq!(
            *old,
            aevia_types::Type::real(aevia_dim::Dim::one()),
            "{mode:?}"
        );
        assert_eq!(*new, aevia_types::Type::Bool, "{mode:?}");
    }
    std::fs::remove_dir_all(directory).unwrap();
}
