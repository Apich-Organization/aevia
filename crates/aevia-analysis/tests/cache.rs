//! The analysis cache: a hit when nothing changed, a reload when anything the
//! load depended on did.
//!
//! Every test builds a real package in a temp directory, so invalidation is
//! exercised against the filesystem the loader actually reads.

use aevia_analysis::Analysis;
use aevia_analysis::AnalysisCache;
use aevia_analysis::Document;

/// A temporary package that removes itself.
struct Package {
    root: std::path::PathBuf,
}
impl Package {
    fn new(name: &str) -> Self {
        let mut root = std::env::temp_dir();
        root.push(format!("aevia-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).expect("create package");
        std::fs::write(
            root.join("aevia.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nentry = \"src/B.aev\"\n",
        )
        .expect("write manifest");
        Self { root }
    }

    fn write(
        &self,
        name: &str,
        source: &str,
    ) -> std::path::PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, source).expect("write source");
        path
    }

    fn path(
        &self,
        name: &str,
    ) -> std::path::PathBuf {
        self.root.join(name)
    }
}
impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A package whose `B` imports `A`, and B's text.
fn imported_package(name: &str) -> (Package, String) {
    let package = Package::new(name);
    package.write("src/A.aev", "module A\n\nfn value(): Int = 1\n");
    let source = "module B\n\nimport A\n\nlet got = A.value()\n".to_string();
    package.write("src/B.aev", &source);
    (package, source)
}

fn document(source: &str) -> Document {
    Document::new("B.aev", 1, source)
}

/// The hover text for `needle` in the document.
fn hover(
    analysis: &Analysis<'_>,
    source: &str,
    needle: &str,
) -> String {
    let offset = u32::try_from(source.find(needle).expect("needle")).expect("offset");
    analysis
        .hover(offset)
        .map_or_else(String::new, |hover| hover.contents)
}

#[test]
fn an_unchanged_package_is_loaded_once() {
    let (package, source) = imported_package("hit");
    let path = package.path("src/B.aev");
    let mut cache = AnalysisCache::new();
    let first = cache.program(&path);
    let second = cache.program(&path);
    assert_eq!(cache.loads(), 1, "the second request was a cache miss");
    assert_eq!(cache.len(), 1);
    assert!(
        std::rc::Rc::ptr_eq(&first, &second),
        "the program was reloaded"
    );
    let document = document(&source);
    let analysis = Analysis::from_program(&document, second);
    assert!(analysis.is_multi_module());
    assert!(
        analysis.diagnostics().is_empty(),
        "{:#?}",
        analysis.diagnostics()
    );
}

#[test]
fn editing_an_import_invalidates_and_the_new_value_is_seen() {
    let (package, source) = imported_package("import-edit");
    let path = package.path("src/B.aev");
    let mut cache = AnalysisCache::new();
    let first = cache.program(&path);
    let document = document(&source);
    let before = hover(&Analysis::from_program(&document, first), &source, "value");
    assert!(before.contains("Int"), "{before}");

    // A new return type changes the import's content and length.
    package.write("src/A.aev", "module A\n\nfn value(): Float = 1.0\n");
    let second = cache.program(&path);
    assert_eq!(cache.loads(), 2, "the changed import did not invalidate");
    let after = hover(&Analysis::from_program(&document, second), &source, "value");
    assert!(after.contains("Float"), "{after}");
    assert!(!after.contains("Int"), "the old scheme was served: {after}");
}

#[test]
fn adding_a_missing_module_invalidates() {
    let package = Package::new("missing-add");
    package.write("src/B.aev", "module B\n\nimport A\n\nlet got = A.value()\n");
    let path = package.path("src/B.aev");
    let source = std::fs::read_to_string(&path).expect("reads");
    let mut cache = AnalysisCache::new();
    let first = cache.program(&path);
    let document = document(&source);
    let missing = Analysis::from_program(&document, first);
    assert!(
        !missing.diagnostics().is_empty(),
        "a missing import was clean"
    );

    // Creating the module must invalidate: the walk sees a new `.aev`.
    package.write("src/A.aev", "module A\n\nfn value(): Int = 1\n");
    let second = cache.program(&path);
    assert_eq!(cache.loads(), 2, "the new module did not invalidate");
    let resolved = Analysis::from_program(&document, second);
    assert!(resolved.is_multi_module());
    assert!(
        resolved.diagnostics().is_empty(),
        "{:#?}",
        resolved.diagnostics()
    );
}

#[test]
fn deleting_an_imported_module_invalidates() {
    let (package, source) = imported_package("missing-delete");
    let path = package.path("src/B.aev");
    let mut cache = AnalysisCache::new();
    let first = cache.program(&path);
    let document = document(&source);
    assert!(
        Analysis::from_program(&document, first)
            .diagnostics()
            .is_empty()
    );
    std::fs::remove_file(package.path("src/A.aev")).expect("delete");
    let second = cache.program(&path);
    assert_eq!(cache.loads(), 2, "the deletion did not invalidate");
    let missing = Analysis::from_program(&document, second);
    let codes: Vec<String> = missing
        .diagnostics()
        .into_iter()
        .filter_map(|diagnostic| diagnostic.code)
        .collect();
    assert_eq!(
        codes.iter().filter(|code| *code == "E0607").count(),
        1,
        "{codes:?}"
    );
}

#[test]
fn an_unrelated_file_outside_the_roots_does_not_invalidate() {
    let (package, source) = imported_package("unrelated");
    let path = package.path("src/B.aev");
    let mut cache = AnalysisCache::new();
    let _ = cache.program(&path);
    // `docs/` is not a resolution root: it cannot change an import graph.
    std::fs::create_dir_all(package.root.join("docs")).expect("docs");
    std::fs::write(package.root.join("docs/notes.aev"), "module Notes\n").expect("write");
    let _ = cache.program(&path);
    assert_eq!(cache.loads(), 1, "an unrelated file invalidated the cache");
    let _ = source;
}

#[test]
fn a_same_length_edit_of_the_entry_is_detected_by_the_content_hash() {
    let (package, _) = imported_package("entry-edit");
    let path = package.path("src/B.aev");
    let mut cache = AnalysisCache::new();
    let _ = cache.program(&path);
    // Same length, different content: only the content hash can see this.
    package.write(
        "src/B.aev",
        "module B\n\nimport A\n\nlet got = A.value(9)\n",
    );
    let _ = cache.program(&path);
    assert_eq!(cache.loads(), 2, "a same-length entry edit was not seen");
}

#[test]
fn a_changed_entry_file_is_not_served_to_a_stale_buffer() {
    let (package, source) = imported_package("entry-stale");
    let path = package.path("src/B.aev");
    let mut cache = AnalysisCache::new();
    let first = cache.program(&path);
    let document = document(&source);
    assert!(Analysis::from_program(&document, first).is_multi_module());

    // The file changes on disk while the editor buffer is still the old text.
    package.write(
        "src/B.aev",
        "module B\n\nimport A\n\nlet got = A.value() + 1\nlet more = 2\n",
    );
    let second = cache.program(&path);
    assert_eq!(cache.loads(), 2);
    // The old document text no longer matches the loaded entry, so the answer
    // must fall back instead of describing source that is not there.
    assert!(!Analysis::from_program(&document, second).is_multi_module());
}

#[test]
fn a_stale_program_is_not_served_to_a_dirty_buffer() {
    let (package, source) = imported_package("dirty");
    let path = package.path("src/B.aev");
    let mut cache = AnalysisCache::new();
    let program = cache.program(&path);
    let edited = format!("{source}\nlet extra = 1\n");
    let document = document(&edited);
    let analysis = Analysis::from_program(&document, program);
    assert!(!analysis.is_multi_module());
    // The cached program was still a hit; only the answer falls back.
    assert_eq!(cache.loads(), 1);
}

#[test]
fn a_dirty_buffer_keeps_its_imports() {
    let (package, source) = imported_package("dirty-imports");
    let path = package.path("src/B.aev");
    let mut cache = AnalysisCache::new();
    let dirty = format!("{source}\nlet extra = A.value() + 1\n");
    let program = cache.program_with_source(&path, &dirty);
    let document = document(&dirty);
    let analysis = Analysis::from_program(&document, program);
    assert!(analysis.is_multi_module());
    assert!(
        analysis.diagnostics().is_empty(),
        "a dirty buffer with valid imports is not clean: {:#?}",
        analysis.diagnostics()
    );
    // The same buffer asked again is a cache hit.
    let _ = cache.program_with_source(&path, &dirty);
    assert_eq!(cache.loads(), 1, "the unchanged dirty buffer reloaded");
}

#[test]
fn a_dirty_buffer_edit_is_rebuilt_and_reflected() {
    let (package, source) = imported_package("dirty-edit");
    let path = package.path("src/B.aev");
    let mut cache = AnalysisCache::new();
    let first = cache.program_with_source(&path, &source);
    assert_eq!(cache.loads(), 1);
    // The edit keeps the import (`A.value()`) and adds a real type error, so a
    // reflected edit is distinguishable from a false import error.
    let edited = format!("{source}\nlet extra = A.value()\nlet bad: Int = \"not an int\"\n");
    let second = cache.program_with_source(&path, &edited);
    assert_eq!(cache.loads(), 2, "an edit did not rebuild the program");
    let document = document(&edited);
    let analysis = Analysis::from_program(&document, second);
    assert!(analysis.is_multi_module());
    let codes: Vec<String> = analysis
        .diagnostics()
        .into_iter()
        .filter_map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.iter().any(|code| code == "E0302"), "{codes:?}");
    assert!(
        !codes.iter().any(|code| code == "E0301" || code == "E0607"),
        "the dirty edit lost its imports: {codes:?}"
    );
    // Unused so the first program is kept alive only by the cache.
    let _ = first;
}

#[test]
fn a_dirty_buffer_still_sees_an_import_edit() {
    let (package, source) = imported_package("dirty-import-edit");
    let path = package.path("src/B.aev");
    let dirty = format!("{source}\nlet extra = A.value()\n");
    let mut cache = AnalysisCache::new();
    let _ = cache.program_with_source(&path, &dirty);
    package.write("src/A.aev", "module A\n\nfn value(): Float = 1.0\n");
    let second = cache.program_with_source(&path, &dirty);
    assert_eq!(cache.loads(), 2, "an edited import did not invalidate");
    let document = document(&dirty);
    let source_text = analysis_hover(&Analysis::from_program(&document, second), &dirty);
    assert!(source_text.contains("Float"), "{source_text}");
}

/// The hover text of `A.value` in `source`.
fn analysis_hover(
    analysis: &Analysis<'_>,
    source: &str,
) -> String {
    let offset = u32::try_from(source.find("value").expect("needle")).expect("offset");
    analysis
        .hover(offset)
        .map_or_else(String::new, |hover| hover.contents)
}

#[test]
fn the_cache_is_bounded() {
    let mut cache = AnalysisCache::new();
    let mut packages = Vec::new();
    for index in 0..aevia_analysis::cache::CAPACITY + 1 {
        let (package, _) = imported_package(&format!("bound-{index}"));
        cache.program(&package.path("src/B.aev"));
        packages.push(package);
    }
    assert_eq!(cache.loads(), (aevia_analysis::cache::CAPACITY + 1) as u64);
    assert_eq!(cache.len(), aevia_analysis::cache::CAPACITY);
}

#[test]
fn clearing_forgets_everything() {
    let (package, _) = imported_package("clear");
    let path = package.path("src/B.aev");
    let mut cache = AnalysisCache::new();
    let _ = cache.program(&path);
    assert_eq!(cache.len(), 1);
    cache.clear();
    assert!(cache.is_empty());
    let _ = cache.program(&path);
    assert_eq!(cache.loads(), 2);
}

#[test]
fn a_manifest_only_nominal_origin_change_invalidates_the_cached_program() {
    let (package, _) = imported_package("manifest-origin");
    let path = package.path("src/B.aev");
    let mut cache = AnalysisCache::new();
    let first = cache.program(&path);
    let unchanged = cache.program(&path);
    assert!(std::rc::Rc::ptr_eq(&first, &unchanged));
    let manifest_path = package.path("aevia.toml");
    let manifest = std::fs::read_to_string(&manifest_path).unwrap();
    std::fs::write(manifest_path, manifest.replace("0.1.0", "0.2.0")).unwrap();
    let revised = cache.program(&path);
    assert_eq!(cache.loads(), 2);
    assert!(!std::rc::Rc::ptr_eq(&first, &revised));
    assert_ne!(
        first.entry().unwrap().checked.module_path(),
        revised.entry().unwrap().checked.module_path()
    );
}

#[test]
fn unsaved_imports_share_one_source_snapshot_and_close_restores_disk() {
    let (package, source) = imported_package("all-overlays");
    let entry = package.path("src/B.aev");
    let imported = package.path("src/A.aev");
    let mut overlays = aevia_analysis::SourceOverlays::new();
    overlays.insert(&entry, source.clone());
    overlays.insert(&imported, "module A\n\nfn value(): Float = 2.0\n");
    let document = document(&source);
    let mut cache = AnalysisCache::new();
    let program = cache.program_with_overlays(&entry, &overlays);
    assert!(
        program.diagnostics.items.is_empty(),
        "{:?}",
        program.diagnostics.items
    );
    let after = hover(
        &Analysis::from_program(&document, program.clone()),
        &source,
        "value",
    );
    assert!(after.contains("Float"), "{after}");
    assert!(std::rc::Rc::ptr_eq(
        &program,
        &cache.program_with_overlays(&entry, &overlays)
    ));
    overlays.remove(&imported);
    let after = hover(
        &Analysis::from_program(&document, cache.program_with_overlays(&entry, &overlays)),
        &source,
        "value",
    );
    assert!(after.contains("Int"), "{after}");
    assert_eq!(cache.loads(), 2);
}

#[test]
fn an_unsaved_import_can_be_created_without_a_disk_file() {
    let package = Package::new("new-overlay-import");
    let source = "module B\n\nimport A\n\nlet got = A.value()\n";
    let entry = package.write("src/B.aev", source);
    let mut overlays = aevia_analysis::SourceOverlays::new();
    overlays.insert(&entry, source);
    let mut cache = AnalysisCache::new();
    assert!(
        cache
            .program_with_overlays(&entry, &overlays)
            .diagnostics
            .has_errors()
    );
    overlays.insert(
        &package.path("src/A.aev"),
        "module A\n\nfn value(): Int = 7\n",
    );
    let program = cache.program_with_overlays(&entry, &overlays);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    assert_eq!(cache.loads(), 2);
    assert!(!package.path("src/A.aev").exists());
    overlays.remove(&package.path("src/A.aev"));
    assert!(
        cache
            .program_with_overlays(&entry, &overlays)
            .diagnostics
            .has_errors()
    );
    assert_eq!(cache.loads(), 3);
}

#[test]
fn unrelated_disk_and_open_module_edits_do_not_reload_the_entry() {
    let (package, _) = imported_package("unrelated-in-root");
    let path = package.path("src/B.aev");
    let mut cache = AnalysisCache::new();
    let first = cache.program(&path);
    let unrelated = package.write("src/Unused.aev", "module Unused\nlet value = 1\n");
    let mut overlays = aevia_analysis::SourceOverlays::new();
    overlays.insert(&unrelated, "module Unused\nlet value = true\n");
    let second = cache.program_with_overlays(&path, &overlays);
    assert!(std::rc::Rc::ptr_eq(&first, &second));
    assert_eq!(cache.loads(), 1);
}

#[test]
fn imported_content_changes_even_when_length_and_mtime_are_restored() {
    let (package, _) = imported_package("restored-stamp");
    let path = package.path("src/B.aev");
    let imported = package.path("src/A.aev");
    let metadata = std::fs::metadata(&imported).unwrap();
    let mut cache = AnalysisCache::new();
    let before = cache.program(&path);
    let changed = "module A\n\nfn value(): Int = 2\n";
    std::fs::write(&imported, changed).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&imported)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(metadata.modified().unwrap()))
        .unwrap();
    assert_eq!(std::fs::metadata(&imported).unwrap().len(), metadata.len());
    assert_eq!(
        std::fs::metadata(&imported).unwrap().modified().unwrap(),
        metadata.modified().unwrap()
    );
    let after = cache.program(&path);
    assert!(!std::rc::Rc::ptr_eq(&before, &after));
    assert_eq!(cache.loads(), 2);
    let module = after
        .modules
        .iter()
        .find(|module| module.path == ["A"])
        .unwrap();
    assert_eq!(after.sources.file(module.file).unwrap().text, changed);
}

#[test]
fn an_unsaved_higher_priority_candidate_changes_resolution() {
    let package = Package::new("overlay-precedence");
    std::fs::create_dir_all(package.path("dependency/src")).unwrap();
    package.write(
        "dependency/aevia.toml",
        "[package]\nname = \"dependency\"\nversion = \"0.1.0\"\nentry = \"src/A.aev\"\n",
    );
    package.write("dependency/src/A.aev", "module A\n\nfn value(): Int = 1\n");
    package.write("aevia.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nentry = \"src/B.aev\"\n[dependencies]\nA = { path = \"dependency\" }\n");
    let source = "module B\n\nimport A\n\nlet got = A.value()\n";
    let entry = package.write("src/B.aev", source);
    let document = document(source);
    let mut cache = AnalysisCache::new();
    let first = cache.program(&entry);
    assert!(
        !first.diagnostics.has_errors(),
        "{:?}",
        first.diagnostics.items
    );
    let mut overlays = aevia_analysis::SourceOverlays::new();
    overlays.insert(
        &package.path("src/A.aev"),
        "module A\n\nfn value(): Float = 1.0\n",
    );
    let second = cache.program_with_overlays(&entry, &overlays);
    let text = hover(&Analysis::from_program(&document, second), source, "value");
    assert!(text.contains("Float"), "{text}");
    assert_eq!(cache.loads(), 2);
}

#[cfg(unix)]
#[test]
fn a_source_overlay_follows_physical_symlink_identity() {
    let (package, source) = imported_package("overlay-symlink");
    let alias = package.path("src/Alias.aev");
    std::os::unix::fs::symlink(package.path("src/A.aev"), &alias).unwrap();
    let mut overlays = aevia_analysis::SourceOverlays::new();
    overlays.insert(&alias, "module A\n\nfn value(): Float = 3.0\n");
    assert_eq!(overlays.len(), 1);
    let mut cache = AnalysisCache::new();
    let program = cache.program_with_overlays(&package.path("src/B.aev"), &overlays);
    let document = document(&source);
    let text = hover(
        &Analysis::from_program(&document, program),
        &source,
        "value",
    );
    assert!(text.contains("Float"), "{text}");
}

#[test]
fn a_new_standalone_buffer_uses_adjacent_imports_without_scanning_a_tree() {
    let dir = std::env::temp_dir().join(format!("aevia-standalone-overlay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("A.aev"), "module A\nfn value(): Int = 7\n").unwrap();
    let entry = dir.join("New.aev");
    let mut cache = AnalysisCache::new();
    let program = cache
        .program_for_document(&entry, "import A\nlet got = A.value()\n")
        .expect("bounded adjacent lookup");
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    assert!(!entry.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}
