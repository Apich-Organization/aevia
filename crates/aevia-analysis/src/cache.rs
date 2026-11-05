//! A small in-process cache of loaded programs.
//!
//! An editor asks the same question many times: hover, completion and
//! diagnostics on one buffer each load the file and its import graph if nothing
//! is remembered. [`AnalysisCache`] remembers the last few programs, keyed by
//! what the load actually depended on:
//!
//! * the entry file's **content hash** (so an edit that keeps the length is
//!   still noticed), and
//! * a **(length, mtime) stamp for every `.aev` file** under the package's
//!   resolution roots, plus the entry file itself when it lives outside them.
//!
//! The stamp set is a directory walk, not a read of every file, so a lookup is
//! cheap; it also notices a module being **added or removed**, which is what
//! makes a previously missing import appear without restarting the editor. A
//! file outside the roots cannot change an import graph, so it does not
//! invalidate anything.
//!
//! The cache keeps [`CAPACITY`] programs in least-recently-used order. It is a
//! per-editor object, not a global: `Rc<Program>` and the `RefCell` the server
//! wraps it in are deliberately single-threaded, and a shared static cache would
//! keep whole programs alive across unrelated workspaces.
//!
//! Correctness is never traded for the hit: after a load, the key is compared
//! with the files again on the next request, and a changed dependency reloads.
//! A failure to stat the tree is a cache miss, not a stale hit.

use aevia_core::driver::Program;
use aevia_core::driver::{
    self,
};
use aevia_core::manifest::Package;
use aevia_core::manifest::Roots;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::SystemTime;

/// How many loaded programs the cache keeps.
pub const CAPACITY: usize = 4;

/// How deep the root walk goes; a module tree deeper than this is not a
/// package layout.
const MAX_DEPTH: usize = 32;

/// A cached program and the state it was loaded from.
struct CacheEntry {
    path: PathBuf,
    key: CacheKey,
    program: Rc<Program>,
}

/// Everything a load depended on, enough to tell when it is stale.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CacheKey {
    /// FNV-1a of the entry file's bytes.
    entry_hash: u64,
    /// `(path, length, mtime)` for every `.aev` under the roots, sorted.
    files: Vec<(PathBuf, u64, Option<SystemTime>)>,
}

impl CacheKey {
    /// The key for loading `path` from disk now, or `None` when the entry is
    /// unreadable.
    fn of(path: &Path) -> Option<Self> {
        let entry = std::fs::read(path).ok()?;
        Some(Self::build(path, &entry))
    }

    /// The key for loading `path` with `source` as the entry text.
    ///
    /// A dirty buffer has no readable entry file to complain about: its text is
    /// the input, and the imports are what the key protects.
    fn of_source(
        path: &Path,
        source: &str,
    ) -> Self {
        Self::build(path, source.as_bytes())
    }

    /// The key for one entry text and the files under the roots.
    fn build(
        path: &Path,
        entry: &[u8],
    ) -> Self {
        let entry_hash = hash_bytes(entry);
        let roots = roots_for(path);
        let mut files = Vec::new();
        let mut visited = HashSet::new();
        let mut dirs = vec![roots.source.clone()];
        if let Some(local) = &roots.local {
            dirs.push(local.clone());
        }
        dirs.extend(roots.packages.values().cloned());
        // A dependency may resolve `import <Key>` to its entry file, which can
        // live outside its `src/`; stamp its directory too.
        for entry_file in roots.entries.values() {
            if let Some(parent) = entry_file.parent() {
                dirs.push(parent.to_path_buf());
            }
            stamp_file(entry_file, &mut files);
        }
        for dir in dirs {
            walk(&dir, 0, &mut visited, &mut files);
        }
        stamp_file(path, &mut files);
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files.dedup_by(|left, right| left.0 == right.0);
        Self { entry_hash, files }
    }
}

/// The resolution roots a load of `path` uses — the same computation
/// [`load_program`] hands the driver.
fn roots_for(path: &Path) -> Roots {
    let roots = match Package::owning(path) {
        | Ok(Some(package)) => package.roots().unwrap_or_else(|_| Roots::adjacent(path)),
        | _ => Roots::adjacent(path),
    };
    match path.parent() {
        | Some(dir) => roots.with_local(dir),
        | None => roots,
    }
}

/// True when `path` is something a load can be keyed on: a file that exists, or
/// a file inside a package's module tree (a buffer for a module not yet
/// created).
///
/// A bare `playground.aev` that does not exist has no package and no bounded
/// roots — walking its parent to look for imports could walk a whole drive — so
/// such a document stays on the single-file path.
pub(crate) fn analysable(path: &Path) -> bool {
    path.is_file() || Package::owning(path).ok().flatten().is_some()
}

/// Loads `path` and its import graph with the package's roots.
pub(crate) fn load_program(path: &Path) -> Program {
    match Package::owning(path) {
        | Ok(Some(package)) => {
            match package.roots() {
                | Ok(roots) => driver::load_with_roots(path, roots),
                | Err(_) => driver::load(path),
            }
        },
        | _ => driver::load(path),
    }
}

/// Loads `path` with `source` as the entry text — a dirty editor buffer — while
/// the imports still resolve from disk.
pub(crate) fn load_dirty_program(
    path: &Path,
    source: &str,
) -> Program {
    driver::load_with_roots_and_source(
        path,
        roots_for(path),
        source,
        &aevia_types::check::CheckOptions::default(),
    )
}

/// Adds the stamp of one file, when it can be read.
fn stamp_file(
    path: &Path,
    files: &mut Vec<(PathBuf, u64, Option<SystemTime>)>,
) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return;
    };
    let modified = metadata.modified().ok();
    files.push((path.to_path_buf(), metadata.len(), modified));
}

/// Collects every `.aev` under `dir`.
fn walk(
    dir: &Path,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    files: &mut Vec<(PathBuf, u64, Option<SystemTime>)>,
) {
    if depth > MAX_DEPTH || !visited.insert(dir.to_path_buf()) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, depth + 1, visited, files);
        } else if path.extension().is_some_and(|extension| extension == "aev") {
            stamp_file(&path, files);
        }
    }
}

/// FNV-1a: a small, deterministic content hash. The cache lives in one process,
/// but a stable digest keeps the key testable and independent of `std`'s
/// per-release hasher choice.
fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Loaded programs by entry path, most recently used last.
#[derive(Default)]
pub struct AnalysisCache {
    entries: Vec<CacheEntry>,
    loads: u64,
}

impl AnalysisCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many programs have been loaded (a cache hit does not count). This is
    /// the observable the tests use to tell a hit from a reload.
    #[must_use]
    pub fn loads(&self) -> u64 {
        self.loads
    }

    /// How many programs are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when nothing is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Forgets everything (the same operation as a workspace being closed).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The program for `path`, reloaded only when what it was built from
    /// changed.
    ///
    /// An entry that cannot be read is loaded uncached: the driver reports the
    /// failure, and there is no state to key.
    pub fn program(
        &mut self,
        path: &Path,
    ) -> Rc<Program> {
        let Some(key) = CacheKey::of(path) else {
            self.loads += 1;
            return Rc::new(load_program(path));
        };
        if let Some(program) = self.hit(path, &key) {
            return program;
        }
        self.loads += 1;
        let program = Rc::new(load_program(path));
        self.remember(path, key, program.clone());
        program
    }

    /// The program for `path` with `source` as the entry text — a dirty editor
    /// buffer — reusing it while the buffer text and its imports are unchanged.
    ///
    /// Imports resolve exactly as for the saved file: only the entry module is
    /// the buffer's text, which is what makes an unsaved file keep its imports
    /// instead of reporting them as unbound.
    pub fn program_with_source(
        &mut self,
        path: &Path,
        source: &str,
    ) -> Rc<Program> {
        let key = CacheKey::of_source(path, source);
        if let Some(program) = self.hit(path, &key) {
            return program;
        }
        self.loads += 1;
        let program = Rc::new(load_dirty_program(path, source));
        self.remember(path, key, program.clone());
        program
    }

    /// A program for an editor document: the buffer text as the entry overlay,
    /// imports resolved from disk.
    ///
    /// Returns `None` when the path is not an analysable file (no file and no
    /// package to bound the roots); the caller then uses [`Analysis::new`]
    /// rather than starting a walk without a boundary.
    ///
    /// [`Analysis::new`]: crate::Analysis::new
    pub fn program_for_document(
        &mut self,
        path: &Path,
        source: &str,
    ) -> Option<Rc<Program>> {
        if !analysable(path) {
            return None;
        }
        Some(self.program_with_source(path, source))
    }

    /// The cached program for this key, moved to the most-recently-used end.
    fn hit(
        &mut self,
        path: &Path,
        key: &CacheKey,
    ) -> Option<Rc<Program>> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.path == path && entry.key == *key)?;
        let entry = self.entries.remove(index);
        let program = entry.program.clone();
        self.entries.push(entry);
        Some(program)
    }

    /// Stores a program, keeping one entry per path and the LRU bound.
    fn remember(
        &mut self,
        path: &Path,
        key: CacheKey,
        program: Rc<Program>,
    ) {
        self.entries.retain(|entry| entry.path != path);
        self.entries.push(CacheEntry {
            path: path.to_path_buf(),
            key,
            program,
        });
        while self.entries.len() > CAPACITY {
            self.entries.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_content_hash_is_stable_and_content_sensitive() {
        assert_eq!(hash_bytes(b"let x = 1\n"), hash_bytes(b"let x = 1\n"));
        assert_ne!(hash_bytes(b"let x = 1\n"), hash_bytes(b"let x = 2\n"));
        // Same length, different content must not collide in a key.
        assert_ne!(hash_bytes(b"abc"), hash_bytes(b"abd"));
    }
}
