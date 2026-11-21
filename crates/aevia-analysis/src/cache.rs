//! Per-editor cache keyed by the exact inputs the module loader observed.
//!
//! Each program retains source reads and all import candidates, including
//! missing ones. A hit compares read contents, candidate existence/physical
//! identity and package resolution context. No directory walk, timestamp
//! assumption or unrelated open buffer can keep stale types alive or force a
//! reload. Unsaved entry and imported buffers share one source snapshot.
use crate::analysis::Analysis;
use crate::analysis::ProgramQueryIndex;
use crate::document::Document;
use aevia_core::driver::CheckCache;
use aevia_core::driver::Program;
use aevia_core::driver::{
    self,
};
use aevia_core::manifest::Package;
use aevia_core::manifest::Roots;
use aevia_core::source::SourceOverlays;
use std::cell::OnceCell;
use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

/// Maximum programs retained by one editor, in least-recently-used order.
pub const CAPACITY: usize = 4;

struct CacheEntry {
    path: PathBuf,
    resolution: String,
    program: Rc<Program>,
    checks: Rc<RefCell<CheckCache>>,
    query_index: OnceCell<Rc<ProgramQueryIndex>>,
}

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

/// Source lookup watches explicit candidates rather than scanning directories,
/// so a new standalone buffer also has a bounded adjacent resolution context.
pub(crate) fn analysable(path: &Path) -> bool {
    path.is_file()
        || path.parent().is_some_and(Path::is_dir)
        || Package::owning(path).ok().flatten().is_some()
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "select the filesystem or overlay loader at the cache boundary"
    )
)]
pub(crate) fn load_program(
    path: &Path,
    overlay: Option<&str>,
) -> Program {
    let roots = roots_for(path);
    match overlay {
        | Some(source) => {
            driver::load_with_roots_and_source(
                path,
                roots,
                source,
                &aevia_types::check::CheckOptions::default(),
            )
        },
        | None => driver::load_with_roots(path, roots),
    }
}

/// Loaded programs by entry path. Source snapshots are compared exactly, so
/// same-size writes with restored mtimes still invalidate a dependent entry.
#[derive(Default)]
pub struct AnalysisCache {
    entries: Vec<CacheEntry>,
    loads: u64,
}

impl AnalysisCache {
    /// A cache with no programs.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of loads; hits do not count.
    #[must_use]
    pub const fn loads(&self) -> u64 {
        self.loads
    }

    /// Number of retained programs.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache retains no programs.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Forgets every retained program.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Answers queries from `program`, sharing its lazily built syntax tables
    /// when it is the exact snapshot retained by this cache.
    #[must_use]
    pub fn analyze_program<'a>(
        &self,
        document: &'a Document,
        program: Rc<Program>,
    ) -> Analysis<'a> {
        let index = self
            .entries
            .iter()
            .find(|entry| Rc::ptr_eq(&entry.program, &program))
            .map(|entry| {
                Rc::clone(
                    entry
                        .query_index
                        .get_or_init(|| Rc::new(ProgramQueryIndex::new(&entry.program))),
                )
            });
        Analysis::from_indexed_program(document, program, index)
    }

    /// Loads a saved entry and its effective disk imports.
    pub fn program(
        &mut self,
        path: &Path,
    ) -> Rc<Program> {
        self.program_with_overlays(path, &SourceOverlays::new())
    }

    /// Loads an unsaved entry; imported buffers can be supplied with
    /// [`Self::program_with_overlays`].
    pub fn program_with_source(
        &mut self,
        path: &Path,
        source: &str,
    ) -> Rc<Program> {
        let mut overlays = SourceOverlays::new();
        overlays.insert(path, source);
        self.program_with_overlays(path, &overlays)
    }

    /// Loads against a single snapshot of unsaved buffers for the whole graph.
    /// Only observed dependencies affect a hit; unrelated buffer edits do not.
    pub fn program_with_overlays(
        &mut self,
        path: &Path,
        overlays: &SourceOverlays,
    ) -> Rc<Program> {
        let roots = roots_for(path);
        // Roots contains ordered, process-local source-resolution data and
        // nominal owners; retain it exactly rather than accepting hash collisions.
        let resolution = format!("{roots:?}");
        if let Some(index) = self.entries.iter().position(|entry| {
            entry.path == path
                && entry.resolution == resolution
                && !entry.program.dependencies.is_empty()
                && entry
                    .program
                    .dependencies
                    .iter()
                    .all(|input| input.is_current(overlays))
        }) {
            let entry = self.entries.remove(index);
            let program = entry.program.clone();
            self.entries.push(entry);
            return program;
        }
        self.loads = self.loads.saturating_add(1);
        let checks = self
            .entries
            .iter()
            .find(|entry| entry.path == path && entry.resolution == resolution)
            .map_or_else(Rc::default, |entry| Rc::clone(&entry.checks));
        let (program, _) = driver::load_incremental(
            path,
            roots,
            overlays,
            &aevia_types::check::CheckOptions::default(),
            &checks,
        );
        let program = Rc::new(program);
        self.entries.retain(|entry| entry.path != path);
        // A file may change while it is being compiled. Cache only the actual
        // observed snapshot if it is still current after the load completes.
        if !program.dependencies.is_empty()
            && program
                .dependencies
                .iter()
                .all(|input| input.is_current(overlays))
        {
            self.entries.push(CacheEntry {
                path: path.to_path_buf(),
                resolution,
                program: program.clone(),
                checks,
                query_index: OnceCell::new(),
            });
            while self.entries.len() > CAPACITY {
                self.entries.remove(0);
            }
        }
        program
    }

    /// An editor document with its imports resolved from disk.
    /// Scratch documents outside any bounded package use single-file analysis.
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

    /// An editor document with all open physical buffers in one snapshot.
    /// The entry's own text wins when a host supplies duplicate path aliases.
    pub fn program_for_document_with_overlays(
        &mut self,
        path: &Path,
        source: &str,
        overlays: &SourceOverlays,
    ) -> Option<Rc<Program>> {
        if !analysable(path) {
            return None;
        }
        if overlays.get(path) == Some(source) {
            return Some(self.program_with_overlays(path, overlays));
        }
        let mut snapshot = overlays.clone();
        snapshot.insert(path, source);
        Some(self.program_with_overlays(path, &snapshot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_buffer_reuses_checked_imports() {
        let directory =
            std::env::temp_dir().join(format!("aevia-editor-recheck-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("Helper.aev"),
            "module Helper\nlet value: Int = 3\n",
        )
        .unwrap();
        let entry = directory.join("main.aev");
        let mut cache = AnalysisCache::new();
        let first = cache.program_with_source(&entry, "import Helper\nlet n = Helper.value + 1\n");
        assert!(!first.diagnostics.has_errors());
        let second = cache.program_with_source(&entry, "import Helper\nlet n = Helper.value + 2\n");
        assert!(!second.diagnostics.has_errors());
        assert!(!Rc::ptr_eq(&first, &second));
        assert_eq!(cache.loads(), 2);
        let checks = cache.entries.last().unwrap().checks.borrow();
        assert_eq!(checks.checked, 1);
        assert_eq!(checks.reused, 1);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
