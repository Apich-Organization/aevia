//! Shared scaffolding for the standard-library tests.
//!
//! The embedded modules are written into a throwaway package directory and the
//! real loader is run against it: the module tree's contract is about programs,
//! so the tests compile real programs rather than inspecting strings where
//! they can avoid it.

#![allow(dead_code)]

use aevia_std::stdlib;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

/// A throwaway package directory.
pub struct Package {
    dir: PathBuf,
}

impl Package {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "aevia-stdlib-{name}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("a temporary package directory");
        Self { dir }
    }

    /// Writes the whole embedded standard library into the package.
    pub fn with_stdlib(name: &str) -> Self {
        let package = Self::new(name);
        for module in stdlib::modules() {
            package.write(&format!("{}.aev", module.path), module.source);
        }
        package
    }

    pub fn write(
        &self,
        relative: &str,
        source: &str,
    ) -> PathBuf {
        let path = self.dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a module directory");
        }
        std::fs::write(&path, source).expect("a module file");
        path
    }

    /// Loads `entry` (a package-relative path) and its imports.
    pub fn load(
        &self,
        entry: &str,
    ) -> aevia_core::driver::Program {
        let path = self.dir.join(entry);
        assert!(path.exists(), "the entry `{entry}` was not written");
        aevia_core::driver::load(&path)
    }

    /// Writes a one-module program and loads it.
    pub fn compile_program(
        &self,
        name: &str,
        source: &str,
    ) -> aevia_core::driver::Program {
        self.write(&format!("{name}.aev"), source);
        self.load(&format!("{name}.aev"))
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Error diagnostics as `code: message` pairs. Error codes start with `E`
/// (appendix B), so this needs no severity type from the syntax crate.
pub fn errors(program: &aevia_core::driver::Program) -> Vec<String> {
    program
        .diagnostics
        .items
        .iter()
        .filter(|item| item.code.starts_with('E'))
        .map(|item| format!("{}: {}", item.code, item.message))
        .collect()
}

/// The diagnostics rendered with their sources, for a failure report.
pub fn rendered(program: &aevia_core::driver::Program) -> String {
    program.diagnostics.render(&program.sources)
}

/// Where the checked-in module sources live.
pub fn stdlib_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("stdlib")
}

/// The specification chapter the tree is generated from.
pub fn spec_stdlib_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/08-stdlib.md")
}

/// The specification chapter with the complete example programs.
pub fn spec_programs_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/11-examples.md")
}
