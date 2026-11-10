//! The embedded standard library (§8.20) through the real loader.
//!
//! `aevia-std` embeds the chapter-8 modules; `driver::load` resolves an import
//! the package roots do not provide through `aevia_std::stdlib`, under a
//! synthetic `stdlib/…` key that takes part in post-order, cycle detection and
//! `run_modules` like any file module. These tests are the loader-level
//! conformance suite: every module imports cleanly, every chapter-8 name can be
//! imported from the module that declares it, the complete §11 programs load,
//! and the `Physics.Ising` wrapper runs end to end. A user package still
//! shadows a stdlib module by path.

#![allow(clippy::result_large_err)]

use aevia_core::driver;
use std::path::Path;
use std::path::PathBuf;

/// A temporary package root.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let mut root = std::env::temp_dir();
        root.push(format!(
            "aevia-stdlib-modules-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create scratch directory");
        Self { root }
    }

    /// Writes `relative` (creating its parent directories) and returns its path.
    fn write(
        &self,
        relative: &str,
        source: &str,
    ) -> PathBuf {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create module directory");
        }
        std::fs::write(&path, source).expect("write program");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The error diagnostics of a loaded program, as `(code, message)`.
fn errors(entry: &Path) -> Vec<(String, String)> {
    let program = driver::load(entry);
    program
        .diagnostics
        .items
        .iter()
        .filter(|item| item.code.starts_with('E'))
        .map(|item| (item.code.clone(), item.message.clone()))
        .collect()
}

/// Loads, checks, runs `main` and returns what it printed.
fn run(entry: &Path) -> String {
    let program = driver::load(entry);
    assert!(
        !program.diagnostics.has_errors(),
        "`{}` does not load: {:?}",
        entry.display(),
        program
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let entry_module = program.entry().expect("the entry module");
    let mut interp = aevia_core::Interp::new(&entry_module.checked).with_buffer();
    let diagnostics = driver::run_modules(&mut interp, &program);
    assert!(
        !diagnostics.has_errors(),
        "`{}` does not run: {:?}",
        entry.display(),
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    interp.run_main().expect("`main` runs");
    interp.captured()
}

/// Every embedded module is importable, transitively: one program imports all
/// of them, so the loader resolves the whole tree (including the modules that
/// import other stdlib modules) in post-order.
#[test]
fn every_embedded_module_loads() {
    let modules: Vec<String> = aevia_std::stdlib::module_paths()
        .map(|path| format!("import {}", path.replace('/', ".")))
        .collect();
    assert_eq!(modules.len(), 41, "the §8.20 tree has 41 modules");
    let source = format!(
        "module Main\n\n{}\n\nfn main(): IO Unit = println(\"loaded\")\n",
        modules.join("\n")
    );
    let scratch = Scratch::new("all");
    let entry = scratch.write("main.aev", &source);
    assert_eq!(run(&entry), "loaded\n");
}

/// Every name a module exports can be imported from that module. This is the
/// reachability half of §8.20: a generated module that forgets a re-export, or a
/// hand-written one whose declaration does not parse as the module's export,
/// shows up here as an unresolved import.
#[test]
fn every_export_is_importable_through_its_module() {
    let scratch = Scratch::new("exports");
    let mut failures = Vec::new();
    for module in aevia_std::stdlib::modules() {
        let components: Vec<&str> = module.path.split('/').collect();
        let names = aevia_std::stdlib::exports(&components);
        if names.is_empty() {
            // Documentation-only modules (`Units/*`): the units are ambient, so
            // there is nothing to import (see the module's own comment).
            continue;
        }
        let dotted = module.path.replace('/', ".");
        let source = format!(
            "module Main\n\nimport {dotted} {{ {} }}\n\nfn main(): IO Unit = println(\"ok\")\n",
            names.join(", ")
        );
        let entry = scratch.write(
            &format!("exports_{}.aev", module.path.replace('/', "_")),
            &source,
        );
        let errors = errors(&entry);
        if !errors.is_empty() {
            failures.push(format!("{} → {errors:?}", module.path));
        }
    }
    assert!(
        failures.is_empty(),
        "modules whose exports do not import cleanly:\n{}",
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// The complete §11 programs
// ---------------------------------------------------------------------------

struct Problem {
    section: String,
    module: String,
    source: String,
}

/// The complete `module …` programs of `spec/11-examples.md` (a skeleton with
/// `...` is not a program).
fn specification_programs() -> Vec<Problem> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/11-examples.md");
    let source = std::fs::read_to_string(&path).expect("spec/11-examples.md");
    let mut out = Vec::new();
    let mut section = String::new();
    let mut language = String::new();
    let mut in_block = false;
    let mut block = String::new();
    for raw in source.lines() {
        if !in_block {
            if let Some(rest) = raw.strip_prefix("## 11.") {
                let number = rest
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('.');
                section = format!("11.{number}");
                continue;
            }
            if let Some(fence) = raw.trim_start().strip_prefix("```") {
                in_block = true;
                language = fence.trim().to_string();
                block.clear();
            }
            continue;
        }
        if raw.trim_start().starts_with("```") {
            in_block = false;
            if language == "aevia"
                && !block.contains("...")
                && let Some(module) = block
                    .lines()
                    .find_map(|line| line.trim().strip_prefix("module "))
                    .map(str::trim)
            {
                out.push(Problem {
                    section: section.clone(),
                    module: module.to_string(),
                    source: block.clone(),
                });
            }
            continue;
        }
        block.push_str(raw);
        block.push('\n');
    }
    out
}

/// The extracted programs are the chapter's, in section order.
#[test]
fn the_extraction_is_the_chapters_complete_programs() {
    let programs = specification_programs();
    let modules: Vec<&str> = programs
        .iter()
        .map(|program| program.module.as_str())
        .collect();
    assert_eq!(
        modules,
        [
            "Kinematics",
            "Projectile",
            "Symbolic",
            "Circuits",
            "Thermo",
            "Analysis",
            "IsingDemo",
            "Regression",
            "Full",
        ]
    );
}

/// Each complete §11 program either loads or has a recorded cause. The two
/// failures are real language gaps with owners, not module-tree problems; an
/// `E0607` (unresolved import) here would be a stdlib failure.
#[test]
fn the_specification_programs_load_or_have_a_recorded_cause() {
    let scratch = Scratch::new("spec");
    let programs = specification_programs();
    assert_eq!(programs.len(), 9);
    // `(module, expected result)`; see `crates/aevia-std/tests/spec_programs.rs`
    // for the diagnostic table with the full root causes. Every complete §11
    // program passes now: §11.2 and §11.4 used to be recorded gaps (record
    // `RateOf` unification; `Refined`/`Rank` shape unification) and were fixed
    // by the round-2 frontend work.
    let expected = [
        ("Kinematics", "ok"),
        ("Projectile", "ok"),
        ("Symbolic", "ok"),
        ("Circuits", "ok"),
        ("Thermo", "ok"),
        ("Analysis", "ok"),
        ("IsingDemo", "ok"),
        ("Regression", "ok"),
        ("Full", "ok"),
    ];
    let mut observed = Vec::new();
    let mut imports = Vec::new();
    for program in &programs {
        let entry = scratch.write(
            &format!("{}.aev", program.module.to_lowercase()),
            &program.source,
        );
        let diagnostics = errors(&entry);
        for (code, message) in &diagnostics {
            if code == "E0607" {
                imports.push(format!("{} {}: {message}", program.section, program.module));
            }
        }
        let first = diagnostics
            .first()
            .map(|(code, _)| code.clone())
            .unwrap_or_else(|| "ok".to_string());
        observed.push((program.module.clone(), first));
        eprintln!(
            "{} {}: {}{}",
            program.section,
            program.module,
            observed.last().expect("just pushed").1,
            diagnostics
                .first()
                .map(|(_, message)| format!(" — {message}"))
                .unwrap_or_default()
        );
    }
    assert!(
        imports.is_empty(),
        "the embedded tree must resolve every import:\n{}",
        imports.join("\n")
    );
    assert_eq!(
        observed,
        expected
            .iter()
            .map(|(module, result)| ((*module).to_string(), (*result).to_string()))
            .collect::<Vec<_>>()
    );
}

/// `Physics.Ising.run` goes through `simulate`/`to_json`/`from_json` end to end.
#[test]
fn the_ising_wrapper_runs_through_the_loader() {
    let source = r#"module IsingLoaderDemo

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
    let scratch = Scratch::new("ising");
    let entry = scratch.write("main.aev", source);
    let output = run(&entry);
    let Some(value) = output
        .lines()
        .find_map(|line| line.trim().strip_prefix("magnetization = "))
    else {
        panic!("the Ising wrapper did not run: {output}");
    };
    let magnetization: f64 = value.parse().expect("a number");
    assert!(
        (-1.0..=1.0).contains(&magnetization),
        "magnetization {magnetization} is outside the range"
    );
}

/// A file under the package roots shadows the embedded module of the same path:
/// user code wins, as it does for any import.
#[test]
fn a_user_file_shadows_the_embedded_module() {
    let scratch = Scratch::new("shadow");
    scratch.write(
        "Math/Special.aev",
        r#"module Math.Special

fn alpha(): Float[1] = 1.0
"#,
    );
    let entry = scratch.write(
        "main.aev",
        r#"module Main

import Math.Special { alpha }

fn main(): IO Unit = println("{alpha()}")
"#,
    );
    assert_eq!(run(&entry), "1.0\n");
}
