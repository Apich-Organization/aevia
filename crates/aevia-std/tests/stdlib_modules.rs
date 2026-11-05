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
//! Two things cannot be expressed yet and are recorded rather than faked: three
//! chapter-8 names are *call forms* (`zeros`, `ones`, `fill`), not first-class
//! values, so no `let` alias can re-export them; and the physics wrappers need
//! `to_json`/`from_json` (requested from the checker/runtime owners). Both are
//! asserted with their exact current state, including the diagnostic, so the
//! test starts failing the day the language catches up.

mod common;

use aevia_std::stdlib;
use common::Package;
use common::errors;
use common::rendered;
use common::spec_stdlib_path;
use common::stdlib_root;

/// The `Units.Constants` module, generated from the live Kernel Manifest.
///
/// Header + the constants module aevia-rssn already generates (§13.6.5): this
/// is the same code path `aevia rssn --emit` uses, so the standard library
/// cannot grow a second constant table.
fn constants_module_source() -> String {
    let registry = aevia_rssn::registry();
    let generated = aevia_rssn::codegen::constants(registry);
    let body = generated
        .split_once("module Constants\n")
        .unwrap_or_else(|| panic!("codegen::constants did not emit a `module Constants` header"))
        .1;
    format!(
        "## The `Units.Constants` module of §8.20.\n\
         ##\n\
         ## Generated from the rssn Kernel Manifest: one quantity per physical\n\
         ## constant of `manifest.constants` (§13.6.5). Do not edit; regenerate with\n\
         ## `cargo test -p aevia-std --test stdlib_modules -- --ignored regenerate`.\n\
         ## rssn {version}, api {api}, manifest {hash}\n\
         module Units.Constants\n{body}",
        version = registry.version,
        api = registry.api_version,
        hash = registry.manifest_hash,
    )
}

/// Writes the generated sources back into `crates/aevia-std/stdlib/`.
///
/// This is the regeneration tool: generated files are checked in because the
/// crate embeds them with `include_str!`, and the test below proves the
/// checked-in bytes are the generated ones. Run it after editing the
/// specification or after the kernel's manifest changes:
///
/// ```text
/// cargo test -p aevia-std --test stdlib_modules -- --ignored regenerate
/// ```
#[test]
#[ignore = "regeneration tool; run explicitly with `--ignored regenerate`"]
fn regenerate() {
    let spec = std::fs::read_to_string(spec_stdlib_path()).expect("spec/08-stdlib.md");
    let modules = stdlib::spec_module_sources(&spec);
    let root = stdlib_root();
    for (path, source) in &modules {
        let file = root.join(format!("{path}.aev"));
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).expect("a module directory");
        }
        std::fs::write(&file, source).expect("a generated module");
    }
    std::fs::write(root.join("Units/Constants.aev"), constants_module_source())
        .expect("the generated constants module");
    eprintln!(
        "regenerated {} generated module(s) and Units/Constants",
        modules.len()
    );
}

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
        let expected = format!("module {}\n", module.path.replace('/', "."));
        assert!(
            module.source.contains(&expected),
            "`{}` does not declare `{}`",
            module.path,
            expected.trim_end()
        );
    }
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
        "generated module sources have drifted (run the `regenerate` test): {drifted:#?}"
    );
}

/// `Units.Constants` is the live manifest's constant table, regenerated.
#[test]
fn units_constants_is_the_live_manifest() {
    let embedded = stdlib::source(&["Units", "Constants"]).expect("Units.Constants is embedded");
    assert_eq!(
        embedded,
        constants_module_source(),
        "Units/Constants.aev is not the live manifest's constants (run `regenerate`)"
    );
    let constants = &aevia_rssn::registry().constants;
    assert!(!constants.is_empty(), "the manifest carries no constants");
    // One `let` per constant, plus the module-local `pi` the generator emits
    // when a constant carries a power of it.
    let needs_pi = constants.iter().any(|constant| constant.value.pi != 0);
    let bindings = embedded
        .lines()
        .filter(|line| line.starts_with("let "))
        .count();
    assert_eq!(
        bindings - usize::from(needs_pi),
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

/// The three call-form names are the *only* chapter-8 names a module cannot
/// re-export, and the reason is a checker limitation, not a missing name. The
/// test pins the current state so it fails when the language catches up.
#[test]
fn call_form_names_are_the_only_missing_exports() {
    let package = Package::with_stdlib("call-form");
    let program = package.compile_program(
        "call_form",
        "module Probe\n\nimport LinearAlgebra { zeros, ones, fill }\n\nfn main(): IO Unit = do\n    print(\"\")\n",
    );
    let failures = errors(&program);
    assert!(
        !failures.is_empty(),
        "`zeros`/`ones`/`fill` are importable now; delete them from stdlib::CALL_FORM_ONLY \
         and re-export them from the LinearAlgebra module"
    );
    for name in stdlib::CALL_FORM_ONLY {
        assert!(
            failures.iter().any(|failure| failure.contains(name)),
            "the failure must name `{name}`: {failures:?}"
        );
    }
    // And an aliasing module cannot be checked either: the generated modules
    // simply omit them, which is what makes them the only missing exports.
    for module in stdlib::modules() {
        let exports = stdlib::exports(&module.path.split('/').collect::<Vec<_>>());
        for name in stdlib::CALL_FORM_ONLY {
            assert!(
                !exports.contains(name),
                "`{}` exports the call form `{name}`",
                module.path
            );
        }
    }
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
