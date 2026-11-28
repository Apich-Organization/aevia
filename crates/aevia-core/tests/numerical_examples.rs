//! Runnable dimensional examples exercise RSSN paths in both engines.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_syntax::DeclKind;

#[test]
fn dimensioned_math_and_portable_models_work_in_both_engines() {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| {
            for name in [
                "dimensioned_numerics.aev",
                "portable_models.aev",
                "physical_functions.aev",
                "native_autodiff.aev",
                "native_backend.aev",
                "native_execution.aev",
                "native_jacobian.aev",
                "native_ode_objects.aev",
                "native_linear_tensors.aev",
                "native_particle_engines.aev",
                "native_discrete_objects.aev",
                "native_continuum_models.aev",
                "native_flow_conservation.aev",
                "native_kernel_families.aev",
                "yee_plane_wave.aev",
                "sde_adaptive_jump.aev",
            ] {
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
                    // Each example and engine represents a separate CLI invocation.
                    aevia_rssn::reset();
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
fn generated_classical_and_sampled_functions_reject_wrong_physical_units() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for source in [
        "open Units.SI\nimport Rssn_numerical_special as P\nlet invalid = P.num_classical_photon_energy(1.0 s)\n",
        "open Units.SI\nimport Rssn_numerical_vector as F\nlet invalid = F.num_functional_inner_product({item0=[0.0 s,1.0 s],item1=[1.0 m,1.0 m]}, {item0=[0.0 m,1.0 m],item1=[1.0 kg,1.0 kg]})\n",
    ] {
        let program =
            driver::load_with_source(&root.join("physical_functions_invalid.aev"), source);
        assert!(
            program.diagnostics.items.iter().any(|d| d.code == "E0401"),
            "{:?}",
            program.diagnostics.items
        );
    }
}
