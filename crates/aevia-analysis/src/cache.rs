//! Per-editor cache keyed by the exact inputs the module loader observed.
//!
//! Each program retains source reads and all import candidates, including
//! missing ones. A hit compares read contents, candidate existence/physical
//! identity and package resolution context. No directory walk, timestamp
//! assumption or unrelated open buffer can keep stale types alive or force a
//! reload. Unsaved entry and imported buffers share one source snapshot.
use aevia_core::driver::Program;
use aevia_core::driver::{
    self,
};
use aevia_core::manifest::Package;
use aevia_core::manifest::Roots;
use aevia_core::source::SourceOverlays;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

/// Maximum programs retained by one editor, in least-recently-used order.
pub const CAPACITY: usize = 4;

struct CacheEntry {
    path: PathBuf,
    resolution: String,
    program: Rc<Program>,
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

pub(crate) fn load_program(path: &Path) -> Program {
    driver::load_with_roots(path, roots_for(path))
}

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
    pub fn loads(&self) -> u64 {
        self.loads
    }

    /// Number of retained programs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache retains no programs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Forgets every retained program.
    pub fn clear(&mut self) {
        self.entries.clear();
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
        self.loads += 1;
        let program = Rc::new(driver::load_with_roots_and_overlays(
            path,
            roots,
            overlays,
            &aevia_types::check::CheckOptions::default(),
        ));
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
        let mut snapshot = overlays.clone();
        snapshot.insert(path, source);
        Some(self.program_with_overlays(path, &snapshot))
    }
}
