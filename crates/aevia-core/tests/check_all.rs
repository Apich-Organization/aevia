//! Single-pass package checking (`check --all`, task-10 part 1).
//!
//! `driver::load_all_with_roots` loads every entry through one loader, so a
//! dependency shared by several entries is parsed once and checked once. The
//! test exposes the counter that distinguishes that from the quadratic
//! "load each entry separately" shape.

use aevia_core::driver;
use aevia_core::manifest::Roots;
use aevia_types::check::CheckOptions;
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("aevia-check-all-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create scratch directory");
        Self(path)
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(&path, source).expect("write module");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn roots(entry: &Path) -> Roots {
    Roots::adjacent(entry)
}

/// Two entries share one dependency: the pass checks three modules, not six.
#[test]
fn shared_dependencies_are_checked_once() {
    let scratch = Scratch::new("shared");
    scratch.write("Dep.aev", "module Dep\n\nlet shared = 1\n");
    let first = scratch.write(
        "A.aev",
        "module A\n\nimport Dep\n\nlet a = Dep.shared + 1\n",
    );
    let second = scratch.write(
        "B.aev",
        "module B\n\nimport Dep\n\nlet b = Dep.shared + 2\n",
    );
    let entries = vec![first, second];
    let (program, stats) =
        driver::load_all_with_roots(&entries, roots(&entries[0]), &CheckOptions::default());
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
    let mut paths: Vec<String> = program
        .modules
        .iter()
        .map(|module| module.path.join("."))
        .collect();
    paths.sort();
    assert_eq!(paths, vec!["A", "B", "Dep"]);
    assert_eq!(stats.entries, 2);
    assert_eq!(stats.files, 3);
    assert_eq!(stats.modules, 3);
    assert_eq!(
        stats.checks, 3,
        "a shared module is checked once, not once per entry (quadratic: 6)"
    );
    assert_eq!(stats.diagnostics, 0);
}

/// Entries with no imports are just their own modules.
#[test]
fn independent_entries_are_one_check_each() {
    let scratch = Scratch::new("independent");
    let first = scratch.write("A.aev", "module A\n\nlet a = 1\n");
    let second = scratch.write("B.aev", "module B\n\nlet b = 2\n");
    let entries = vec![first, second];
    let (program, stats) =
        driver::load_all_with_roots(&entries, roots(&entries[0]), &CheckOptions::default());
    assert!(!program.diagnostics.has_errors());
    assert_eq!(stats.entries, 2);
    assert_eq!(stats.modules, 2);
    assert_eq!(stats.checks, 2);
}

/// A malformed entry reports its diagnostic once, not once per importer.
#[test]
fn diagnostics_are_deduplicated() {
    let scratch = Scratch::new("dedupe");
    scratch.write("Dep.aev", "module Dep\n\nimport Missing\n\nlet shared = 1\n");
    let first = scratch.write("A.aev", "module A\n\nimport Dep\n\nlet a = 1\n");
    let second = scratch.write("B.aev", "module B\n\nimport Dep\n\nlet b = 2\n");
    let entries = vec![first, second];
    let (program, stats) =
        driver::load_all_with_roots(&entries, roots(&entries[0]), &CheckOptions::default());
    assert!(program.diagnostics.has_errors(), "the missing import is reported");
    let missing = program
        .diagnostics
        .items
        .iter()
        .filter(|item| item.message.contains("cannot find module `Missing`"))
        .count();
    assert_eq!(missing, 1, "the shared module's failure is reported once");
    assert_eq!(stats.diagnostics, program.diagnostics.items.len());
}

/// A dirty editor buffer can be loaded while its imports still resolve from
/// disk; the provided text wins over the file on disk.
#[test]
fn a_dirty_buffer_keeps_its_imports_and_overrides_disk() {
    let scratch = Scratch::new("dirty");
    scratch.write("Dep.aev", "module Dep\n\nlet shared = 1\n");
    let entry = scratch.write(
        "Main.aev",
        "module Main\n\nimport Dep\n\nlet answer = Dep.shared + 1\n",
    );
    let dirty = "module Main\n\nimport Dep\n\nlet answer = Dep.shared + 41\n";
    let program = driver::load_with_source(&entry, dirty);
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
    assert_eq!(program.modules.len(), 2, "the import was still resolved");
    let checked = &program.entry().expect("entry module").checked;
    let mut interp = aevia_core::Interp::new(checked).with_buffer();
    let diagnostics = driver::run_modules(&mut interp, &program);
    assert!(!diagnostics.has_errors(), "load errors");
    let answer = interp.eval_toplevel("answer").expect("answer is bound");
    assert_eq!(
        answer.as_f64().expect("a number"),
        42.0,
        "the dirty text is what was checked and run"
    );
}

/// No entries is an empty pass, not a panic.
#[test]
fn an_empty_pass_is_empty() {
    let (program, stats) = driver::load_all_with_roots(
        &[],
        Roots::adjacent(Path::new("unused.aev")),
        &CheckOptions::default(),
    );
    assert!(program.modules.is_empty());
    assert_eq!(stats, driver::LoadStats::default());
}
