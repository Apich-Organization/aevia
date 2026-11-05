//! The embedded standard library (§8.20) is importable from user source.
//!
//! `aevia-std` embeds the chapter-8 modules; the loader resolves an import the
//! package roots do not provide through `aevia_std::stdlib`, under a synthetic
//! `stdlib/…` key that participates in post-order, cycle detection and
//! `run_modules` like any file module.

use aevia_core::driver;
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("aevia-stdlib-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create scratch directory");
        Self(path)
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, source).expect("write program");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(entry: &Path) -> String {
    let program = driver::load(entry);
    assert!(
        !program.diagnostics.has_errors(),
        "diagnostics: {:?}",
        program
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let checked = &program.entry().expect("entry module").checked;
    let mut interp = aevia_core::Interp::new(checked).with_buffer();
    let diagnostics = driver::run_modules(&mut interp, &program);
    assert!(
        !diagnostics.has_errors(),
        "load errors: {:?}",
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    interp.run_main().expect("main runs");
    interp.captured()
}

/// `import Math { sqrt }` resolves to the embedded module; the imported function
/// is callable and computes.
#[test]
fn a_math_import_resolves_from_the_embedded_stdlib() {
    let scratch = Scratch::new("math");
    let entry = scratch.write(
        "main.aev",
        r#"module Main

import Math { sqrt }

fn main(): IO Unit = do
    println("{sqrt(16.0)}")
"#,
    );
    assert_eq!(run(&entry), "4\n");
}

/// A module that itself imports other stdlib modules (the real §8.20 shape)
/// loads: `Physics.Ising` is embedded and imports its dependencies.
#[test]
fn a_stdlib_module_with_its_own_imports_loads() {
    let scratch = Scratch::new("physics");
    let entry = scratch.write(
        "main.aev",
        r#"module Main

import Physics.Ising

fn main(): IO Unit = println("loaded")
"#,
    );
    assert_eq!(run(&entry), "loaded\n");
}
