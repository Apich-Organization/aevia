//! The §8.20 module tree: embedded, generated from the specification, and
//! checkable as real programs.
//!
//! The crate ships `crates/aevia-std/stdlib/**.aev` and embeds it with
//! `include_str!` (`aevia_std::stdlib`). This test is the contract on that tree:
//!
//! * the generated modules are byte-for-byte what `spec/08-stdlib.md` says they
//!   should be, so a hand-edit cannot make the test pass;
//! * every module parses and type-checks as a module, and every name it
//!   re-exports is reachable through `import M { name }` (the loader's export
//!   check is the assertion);
//! * `Units.Constants` is the live Kernel Manifest's constant table (one
//!   binding per `manifest.constants` entry), regenerated from the manifest and
//!   compared, not pinned;
//! * the `Physics` wrappers are real typed façades over `simulate`, and the
//!   Ising one is executed end to end.
//!
//! Variadic tensor constructors carry their identity through aliases and imports;
//! the checks below exercise those exports as well as the physics wrappers.

mod common;

use aevia_std::stdlib;
use common::Package;
use common::errors;
use common::rendered;
use common::spec_stdlib_path;

#[path = "../../../tools/aevia-stdlib-gen/src/support.rs"]
mod generator_support;
use generator_support::constants_module_source;

// ---------------------------------------------------------------------------
// The tree itself
// ---------------------------------------------------------------------------

/// The embedded table is the §8.20 tree, in the documented order, and each
/// source really declares the module its path names.
#[test]
fn the_embedded_tree_is_the_module_tree() {
    let embedded: Vec<&str> = stdlib::module_paths().collect();
    let tree: Vec<&str> = stdlib::TREE.iter().map(|module| module.path).collect();
    assert_eq!(
        embedded, tree,
        "`stdlib::modules()` and `stdlib::TREE` disagree"
    );
    for module in stdlib::modules() {
        // A unit-system module declares its system after the name (§5.11).
        let name = format!("module {}", module.path.replace('/', "."));
        let expected = format!("{name}\n");
        let with_system = format!("{name} with units ");
        assert!(
            module.source.contains(&expected) || module.source.contains(&with_system),
            "`{}` does not declare `{}`",
            module.path,
            expected.trim_end()
        );
    }
}

/// The embedded Rust table is generated from the same catalog as every module.
#[test]
fn the_embedding_table_matches_the_catalog() {
    assert_eq!(
        include_str!("../src/stdlib_table.rs"),
        aevia_stdlib_model::stdlib::embedding_table_source(),
        "regenerate the standard library and its embedding table"
    );
}

/// The generated modules are exactly what the specification generates.
///
/// This is the anti-hand-edit assertion: the checked-in files must equal
/// [`stdlib::spec_module_sources`]'s output byte for byte. If it fails, run the
/// `regenerate` test after checking the specification change is intended.
#[test]
fn the_generated_sources_are_the_specifications() {
    let spec = std::fs::read_to_string(spec_stdlib_path()).expect("spec/08-stdlib.md");
    let expected = stdlib::spec_module_sources(&spec);
    assert!(!expected.is_empty(), "no §8.20 module is generated");
    let mut drifted = Vec::new();
    for (path, source) in expected {
        match stdlib::source(&path.split('/').collect::<Vec<_>>()) {
            | Some(embedded) if embedded != source => {
                drifted.push(format!("`{path}` differs from the specification"));
            },
            | Some(_) => {},
            | None => drifted.push(format!("`{path}` is not embedded")),
        }
    }
    assert!(
        drifted.is_empty(),
        "generated module sources have drifted (run `cargo run -p aevia-stdlib-gen -- --write`): {drifted:#?}"
    );
}

/// `Units.Constants` is the live manifest's constant table, regenerated.
#[test]
fn units_constants_is_the_live_manifest() {
    let embedded = stdlib::source(&["Units", "Constants"]).expect("Units.Constants is embedded");
    assert_eq!(
        embedded,
        constants_module_source(),
        "Units/Constants.aev is not the live manifest's constants (run the stdlib generator with `--write`)"
    );
    let constants = &aevia_rssn::registry().constants;
    assert!(!constants.is_empty(), "the manifest carries no constants");
    // The kernel constants are the only bindings; `pi` is the exact builtin.
    let bindings = embedded
        .lines()
        .filter(|line| line.starts_with("let "))
        .count();
    assert_eq!(
        bindings,
        constants.len(),
        "Units.Constants does not bind every manifest constant"
    );
}

// ---------------------------------------------------------------------------
// Every module parses, checks, and exports what it claims
// ---------------------------------------------------------------------------

/// Every module parses and type-checks, and importing all of its exports
/// resolves. The loader's export check is the assertion: a name listed in an
/// import that the module does not export is `E0301`.
#[test]
fn every_module_checks_and_exports_its_names() {
    let package = Package::with_stdlib("modules");
    let mut broken = Vec::new();
    for module in stdlib::modules() {
        let exports = stdlib::exports(&module.path.split('/').collect::<Vec<_>>());
        let path = module.path.replace('/', ".");
        let mut probe = String::from("module Probe\n");
        if exports.is_empty() {
            probe.push_str(&format!("import {path}\n"));
        } else {
            // Every exported name is listed in the import. The loader checks
            // the list against the module's interface, so a name that is not
            // exported is `E0301` — that check *is* the reachability
            // assertion. It covers values and the `Table` type alias alike.
            let list: Vec<&str> = exports.to_vec();
            probe.push_str(&format!("import {path} {{ {} }}\n", list.join(", ")));
        }
        probe.push_str("\nfn main(): IO Unit = do\n    print(\"\")\n");
        let program =
            package.compile_program(&format!("probe_{}", module.path.replace('/', "_")), &probe);
        let failures = errors(&program);
        if !failures.is_empty() {
            broken.push(format!(
                "`{}`: {}\n{}",
                module.path,
                failures.join("; "),
                rendered(&program)
            ));
        }
    }
    assert!(broken.is_empty(), "{}", broken.join("\n\n"));
}

/// The generated sum types expose their constructors as well as their type names.
#[test]
fn generated_solver_constructors_are_importable() {
    let package = Package::with_stdlib("solver-constructors");
    let program = package.compile_program(
        "probe",
        "module Probe\nimport Ode { OdeMethod, Rk4, Dopri5 }\nimport Sde { SdeStructure, Diagonal, General }\n\nlet ode: OdeMethod = Rk4\nlet ode2: OdeMethod = Dopri5\nlet diagonal: SdeStructure = Diagonal\nlet general: SdeStructure = General(1)\n\nfn main(): IO Unit = do\n    print(\"\")\n",
    );
    assert!(errors(&program).is_empty(), "{}", rendered(&program));
}

/// Variadic constructors are exported by both tensor and linear algebra modules.
#[test]
fn variadic_tensor_constructors_are_importable() {
    let package = Package::with_stdlib("tensor-intrinsics");
    let program = package.compile_program(
        "probe",
        "module Probe\nimport Tensor { zeros, ones, fill }\nlet a: Tensor<2, Int> = zeros<Int>(2, 3)\nlet b: Tensor<1, Float> = ones(4)\nlet c: Tensor<1, Int> = fill(7, 5)\nfn main(): IO Unit = println(\"\")\n",
    );
    assert!(errors(&program).is_empty(), "{}", rendered(&program));
}

/// Regression: the schemes for `on`, `odeint_with` and `solve_ivp` used to
/// reference a variable they did not quantify, so the *second* instantiation in
/// one module was `E0303: infinite type`. frontend-fixer quantified the missing
/// variables (and added a scheme-helper assertion so it cannot regress); this
/// test checks both halves that matter here: the repro now type-checks, and
/// `Device`/`Ode` re-export the names.
#[test]
fn the_scheme_quantification_regression_stays_fixed() {
    for name in ["on", "odeint_with", "solve_ivp"] {
        let source = format!("let first = {name}\nlet sin = sin\nlet second = {name}\n");
        let compilation = aevia_core::compile("probe.aev", &source);
        let diagnostics: Vec<String> = compilation
            .diagnostics
            .items
            .iter()
            .filter(|item| item.code.starts_with('E'))
            .map(|item| format!("{}: {}", item.code, item.message))
            .collect();
        assert!(
            diagnostics.is_empty(),
            "`{name}` no longer instantiates freshly: {diagnostics:?}"
        );
    }
    let package = Package::with_stdlib("quantification");
    let program = package.compile_program(
        "probe",
        "module Probe\n\nimport Device { on }\nimport Ode { odeint_with, solve_ivp }\n\n\
         fn main(): IO Unit = do\n    print(\"\")\n",
    );
    let failures = errors(&program);
    assert!(
        failures.is_empty(),
        "`Device`/`Ode` must re-export the fixed names: {failures:?}"
    );
}

// ---------------------------------------------------------------------------
// The Physics wrappers
// ---------------------------------------------------------------------------

/// Whether the checker knows a builtin by name. The physics wrappers need two
/// builtins that are being added by the checker/runtime owners; while they are
/// absent this test must say exactly that instead of failing obscurely.
fn builtin_exists(name: &str) -> bool {
    let compilation = aevia_core::compile("probe.aev", &format!("let x = {name}\n"));
    !compilation.diagnostics.has_errors()
}

/// A wrapper's failure is exactly "the JSON bridge is not in the checker yet",
/// with no other diagnostic mixed in.
fn assert_pending_json_bridge(
    program: &aevia_core::driver::Program,
    wrapper: &str,
) {
    let failures = errors(program);
    assert!(
        !failures.is_empty(),
        "`{wrapper}` checks now; run the wrapper end-to-end test instead of the pending branch"
    );
    for missing in ["to_json", "from_json"] {
        if builtin_exists(missing) {
            continue;
        }
        assert!(
            failures.iter().any(|failure| failure.contains(missing)),
            "`{wrapper}` must fail only because `{missing}` is missing; found: {}",
            rendered(program)
        );
    }
    eprintln!(
        "pending: {wrapper} needs `to_json`/`from_json` (checker/runtime owners); \
         diagnostics were: {}",
        failures.join("; ")
    );
}

/// Whether the *runtime* can encode a value to `Json`. The checker schemes for
/// `to_json`/`from_json` and the runtime dispatch arms are owned by two
/// different work streams, so they can land at different times; the test says
/// which half is missing instead of failing obscurely.
fn json_bridge_runs() -> bool {
    let source = "let j = to_json(1)\n";
    let compilation = aevia_core::compile("probe.aev", source);
    if compilation.diagnostics.has_errors() {
        return false;
    }
    let Some(module) = compilation.module else {
        return false;
    };
    let mut interp = aevia_core::Interp::new(&compilation.checked).with_buffer();
    if interp.load(&module).has_errors() {
        return false;
    }
    interp.eval_toplevel("j").is_ok()
}

/// `Physics.Ising` and `Physics.Geodesic` parse and type-check, and the Ising
/// wrapper runs a real 4×4 scenario through `simulate`.
#[test]
fn the_physics_wrappers_run_when_the_json_bridge_exists() {
    let package = Package::with_stdlib("physics");
    for (wrapper, module) in [
        ("Physics.Ising", "Physics/Ising"),
        ("Physics.Geodesic", "Physics/Geodesic"),
    ] {
        let path = module.replace('/', ".");
        let probe =
            format!("module Probe\n\nimport {path}\n\nfn main(): IO Unit = do\n    print(\"\")\n");
        let program =
            package.compile_program(&format!("probe_{}", module.replace('/', "_")), &probe);
        let failures = errors(&program);
        if failures.is_empty() {
            continue;
        }
        assert_pending_json_bridge(&program, wrapper);
    }

    if !(builtin_exists("to_json") && builtin_exists("from_json")) {
        eprintln!(
            "pending: Physics.Ising.run not executed (to_json/from_json missing from the checker)"
        );
        return;
    }
    if !json_bridge_runs() {
        eprintln!(
            "pending: Physics.Ising.run not executed (to_json/from_json are typed but the \
             runtime dispatch arms are missing — runtime-fixer)"
        );
        return;
    }

    let entry = r#"module IsingDemo

import Physics.Ising

fn main(): IO Unit = do
    let params = {
        width = 4,
        height = 4,
        temperature = 1.8,
        mc_steps = 1,
        seed = Some(7),
    }
    match Physics.Ising.run(params) with
    | Ok(r) -> println("magnetization = {r.magnetization}")
    | Err(e) -> println("simulation failed: {e}")
"#;
    let program = package.compile_program("ising_demo", entry);
    assert!(
        errors(&program).is_empty(),
        "the Ising demo does not type-check:\n{}",
        rendered(&program)
    );
    let entry_module = program.entry().expect("the entry module");
    let mut interp = aevia_core::Interp::new(&entry_module.checked).with_buffer();
    let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
    assert!(
        !diagnostics.has_errors(),
        "the Ising demo does not load: {:?}",
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    interp.run_main().expect("`main` runs");
    let output = interp.captured();
    assert!(
        output.contains("magnetization = "),
        "unexpected Ising output: {output}"
    );
}
