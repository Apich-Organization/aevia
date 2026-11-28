//! Native generic and opaque algebraic objects run through real generated bindings.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_syntax::DeclKind;

#[test]
fn native_backend_handles_preserve_compiled_programs_and_mixed_units() {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| {
            for name in ["native_backend.aev"] {
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../examples")
                    .join(name);
                let program = driver::load(&path);
                assert!(
                    !program.diagnostics.has_errors(),
                    "{name}: {:?}",
                    program.diagnostics.items
                );
                let entry = program.entry().unwrap();
                for mode in [Mode::Ast, Mode::Ir] {
                    let mut interp = Interp::new(&entry.checked).with_mode(mode);
                    let diagnostics = driver::run_modules(&mut interp, &program);
                    assert!(
                        !diagnostics.has_errors(),
                        "{name} {mode:?}: {:?}",
                        diagnostics.items
                    );
                    let root = interp.root();
                    let mut assertions = 0;
                    for declaration in &entry.module.decls {
                        if let DeclKind::Test {
                            name: assertion,
                            body,
                        } = &declaration.kind
                        {
                            let outcome = interp.eval_source_test(body, &root);
                            assert!(
                                matches!(outcome, Ok(Value::Bool(true))),
                                "{name} {mode:?}: {assertion}: {outcome:?}"
                            );
                            assertions += 1;
                        }
                    }
                    assert!(assertions > 0, "{name} has no assertions");
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn source_backend_permissions_cover_dynamic_and_pinned_calls() {
    let source = r#"
let cpu = Rssn.num_backend_named("cpu")
let dynamic_gpu = Rssn.dynamic("num_backend_named", [to_value("gpu")])
let dynamic_mutation = Rssn.dynamic("num_backend_select_default", [to_value("cpu")])
fn pinned_gpu() = Rssn.num_backend_named("gpu")
fn pinned_mutation() = Rssn.num_backend_select_default("cpu")
"#;
    let compilation = aevia_core::compile("backend_caps.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.as_ref().unwrap();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        interp.capabilities = 0;
        let loaded = interp.load(module);
        assert!(!loaded.has_errors(), "{mode:?}: {:?}", loaded.items);
        assert!(interp.eval_toplevel("cpu").is_ok());
        for name in ["dynamic_gpu", "dynamic_mutation"] {
            let value = interp.eval_toplevel(name).unwrap();
            let Value::Variant(result) = &value else {
                panic!("{mode:?}: {value:?}");
            };
            assert_eq!(result.name, "Err");
            assert!(
                matches!(
                    result.fields.as_slice(),
                    [Value::KernelError(error)] if error.code == "E0902"
                ),
                "{mode:?}: {value:?}"
            );
        }
        for name in ["pinned_gpu", "pinned_mutation"] {
            let function = interp.eval_toplevel(name).unwrap();
            let error = interp
                .apply(function, Vec::new(), aevia_syntax::Span::default())
                .expect_err("an embedded source function has no such permission");
            assert_eq!(error.code, "E0902", "{mode:?}: {error:?}");
        }
        interp.capabilities = aevia_core::value::caps::ALL;
        let function = interp.eval_toplevel("pinned_mutation").unwrap();
        let error = interp
            .apply(function, Vec::new(), aevia_syntax::Span::default())
            .expect_err("GPU permission does not grant control of the host process default");
        assert_eq!(error.code, "E0902", "{mode:?}: {error:?}");
    }
}
