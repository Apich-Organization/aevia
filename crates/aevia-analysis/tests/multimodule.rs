//! Multi-module analysis over the real loader.
//!
//! The single-file path is the unsaved-buffer fallback; a saved file is loaded
//! with its import graph through `aevia_core::driver`, so imported names
//! resolve instead of being reported as unbound. These tests build a small
//! package in a temporary directory and ask the same questions an editor does.

use aevia_analysis::Analysis;
use aevia_analysis::AnalysisCache;
use aevia_analysis::Document;
use aevia_analysis::Range;

/// A temporary package that removes itself.
struct Package {
    root: std::path::PathBuf,
}
impl Package {
    fn new(name: &str) -> Self {
        let mut root = std::env::temp_dir();
        root.push(format!("aevia-analysis-{name}-{}", std::process::id()));
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

/// The byte offset of `needle` in `source`.
fn offset_of(
    source: &str,
    needle: &str,
) -> u32 {
    u32::try_from(
        source
            .find(needle)
            .unwrap_or_else(|| panic!("no {needle:?}")),
    )
    .expect("offset")
}

/// The source text a range covers in `source`.
fn text_of(
    source: &str,
    range: Range,
) -> &str {
    let index = aevia_analysis::LineIndex::new(source);
    let start = index.offset(source, range.start).expect("start") as usize;
    let end = index.offset(source, range.end).expect("end") as usize;
    &source[start..end]
}

/// A two-module package: `A` exports a function and a record type.
fn package(name: &str) -> (Package, String) {
    let package = Package::new(name);
    package.write(
        "src/A.aev",
        "module A\n\n## A length, unchanged.\nfn sqrt(x: Real[L]): Real[L] = x\n\ntype Point = { x: Real[L], y: Real[L] }\n",
    );
    let source = "module B\n\nimport A\n\nlet length = A.sqrt(2.0 m)\n".to_string();
    package.write("src/B.aev", &source);
    (package, source)
}

fn analysis<'a>(
    package: &Package,
    source: &'a str,
) -> (Document, std::path::PathBuf) {
    let path = package.path("src/B.aev");
    (Document::new(path.display().to_string(), 1, source), path)
}

#[test]
fn a_saved_file_with_imports_has_no_false_diagnostics() {
    let (package, source) = package("clean");
    let (document, path) = analysis(&package, &source);
    let analysis = Analysis::from_path(&document, &path);
    assert!(analysis.is_multi_module());
    let diagnostics = analysis.diagnostics();
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}

#[test]
fn the_single_file_fallback_still_reports_the_unresolved_import() {
    // The difference this task is about: without the loader, `A.sqrt` is
    // unbound and `import A` is not resolved.
    let (package, source) = package("fallback-diff");
    let (document, _) = analysis(&package, &source);
    let analysis = Analysis::new(&document);
    assert!(!analysis.is_multi_module());
    let diagnostics = analysis.diagnostics();
    let codes: Vec<&str> = diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.code.as_deref())
        .collect();
    // Without the loader an imported name cannot resolve to a type or a
    // function; the exact code depends on where resolution gives up, but it is
    // not silent.
    assert!(!codes.is_empty(), "the fallback claimed a clean file");
}

#[test]
fn hover_on_an_imported_qualified_name_reports_its_scheme() {
    let (package, source) = package("hover");
    let (document, path) = analysis(&package, &source);
    let analysis = Analysis::from_path(&document, &path);
    let hover = analysis
        .hover(offset_of(&source, "sqrt"))
        .unwrap_or_else(|| panic!("no hover: {:?}", analysis.diagnostics()));
    assert!(hover.contents.contains("sqrt"), "{}", hover.contents);
    assert!(hover.contents.contains("Real[L]"), "{}", hover.contents);
    assert!(
        hover.contents.contains("imported from `A`"),
        "{}",
        hover.contents
    );
}

#[test]
fn completion_includes_the_imported_module_and_its_exports() {
    let (package, source) = package("completion");
    let (document, path) = analysis(&package, &source);
    let analysis = Analysis::from_path(&document, &path);
    let labels: Vec<String> = analysis
        .completion(0)
        .into_iter()
        .map(|item| item.label)
        .collect();
    assert!(labels.iter().any(|label| label == "A"), "{labels:?}");
    assert!(labels.iter().any(|label| label == "A.sqrt"), "{labels:?}");
    assert!(labels.iter().any(|label| label == "A.Point"), "{labels:?}");
}

#[test]
fn definition_on_an_imported_name_jumps_to_the_importing_file() {
    let (package, source) = package("definition");
    let (document, path) = analysis(&package, &source);
    let analysis = Analysis::from_path(&document, &path);
    let location = analysis
        .definition(offset_of(&source, "sqrt"))
        .expect("a definition for the imported function");
    assert!(
        location.document.ends_with("A.aev"),
        "{}",
        location.document
    );
    let a_source = std::fs::read_to_string(package.path("src/A.aev")).expect("reads");
    assert_eq!(text_of(&a_source, location.range), "sqrt");
}

#[test]
fn an_opened_import_resolves_a_bare_name() {
    let package = Package::new("opened");
    package.write(
        "src/A.aev",
        "module A\n\nfn sqrt(x: Real[L]): Real[L] = x\n",
    );
    let source = "module C\n\nopen A\n\nlet length = sqrt(2.0 m)\n";
    package.write("src/C.aev", source);
    let path = package.path("src/C.aev");
    let document = Document::new(path.display().to_string(), 1, source);
    let analysis = Analysis::from_path(&document, &path);
    assert!(
        analysis.diagnostics().is_empty(),
        "{:#?}",
        analysis.diagnostics()
    );
    let hover = analysis.hover(offset_of(source, "sqrt")).expect("hover");
    assert!(hover.contents.contains("sqrt"), "{}", hover.contents);
    let location = analysis.definition(offset_of(source, "sqrt")).expect("def");
    assert!(
        location.document.ends_with("A.aev"),
        "{}",
        location.document
    );
}

#[test]
fn a_record_field_of_an_imported_type_is_named() {
    let package = Package::new("field");
    package.write(
        "src/A.aev",
        "module A\n\ntype Point = { x: Real[L], y: Real[L] }\n",
    );
    let source = "module D\n\nimport A\n\nlet p: A.Point = { x = 1.0 m, y = 2.0 m }\nlet x = p.x\n";
    package.write("src/D.aev", source);
    let path = package.path("src/D.aev");
    let document = Document::new(path.display().to_string(), 1, source);
    let analysis = Analysis::from_path(&document, &path);
    assert!(
        analysis.diagnostics().is_empty(),
        "{:#?}",
        analysis.diagnostics()
    );
    let hover = analysis
        .hover(offset_of(source, "p.x") + 2)
        .expect("a field hover");
    assert!(hover.contents.contains("field x"), "{}", hover.contents);
    assert!(hover.contents.contains("Real[L]"), "{}", hover.contents);
}

#[test]
fn an_unsaved_buffer_keeps_its_imports_and_its_edits() {
    let (package, source) = package("unsaved");
    let path = package.path("src/B.aev");
    // The editor's text no longer matches the file on disk, but the import
    // graph still resolves from disk.
    let edited = format!("{source}\nlet extra = A.sqrt(3.0 m)\n");
    let document = Document::new(path.display().to_string(), 2, &edited);
    let analysis = Analysis::from_path(&document, &path);
    assert!(analysis.is_multi_module());
    assert!(
        analysis.diagnostics().is_empty(),
        "{:#?}",
        analysis.diagnostics()
    );
    // The unsaved line is visible to the queries: no false unbound name.
    let hover = analysis
        .hover(offset_of(&edited, "A.sqrt"))
        .expect("hover on the imported name in the dirty buffer");
    assert!(hover.contents.contains("sqrt"), "{}", hover.contents);

    // An unsaved edit that is wrong is reported, and still not as a missing
    // import.
    let broken = format!("{edited}\nlet bad: Int = A.sqrt(1.0 m)\n");
    let broken_document = Document::new(path.display().to_string(), 3, &broken);
    let broken_analysis = Analysis::from_path(&broken_document, &path);
    let codes: Vec<String> = broken_analysis
        .diagnostics()
        .into_iter()
        .filter_map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.iter().any(|code| code == "E0302"), "{codes:?}");
    assert!(
        !codes.iter().any(|code| code == "E0301" || code == "E0607"),
        "a dirty buffer with valid imports reported import errors: {codes:?}"
    );
}

#[test]
fn definition_from_a_dirty_buffer_points_at_the_imported_declaration() {
    // The editor path always loads the buffer as an entry overlay; a
    // cross-file definition must still compute its range in the imported
    // file's own text.
    let (package, source) = package("dirty-definition");
    let path = package.path("src/B.aev");
    let dirty = format!("{source}\nlet more = A.sqrt(4.0 m)\n");
    let document = Document::new(path.display().to_string(), 2, &dirty);
    let analysis = Analysis::from_path(&document, &path);
    assert!(analysis.is_multi_module());
    let location = analysis
        .definition(offset_of(&dirty, "sqrt"))
        .expect("a definition through an overlay load");
    assert!(
        location.document.ends_with("A.aev"),
        "{}",
        location.document
    );
    let a_source = std::fs::read_to_string(package.path("src/A.aev")).expect("reads");
    assert_eq!(text_of(&a_source, location.range), "sqrt");
}

#[test]
fn definition_through_the_server_style_cache_path() {
    // Exactly what the server does: a package without a doc comment, the
    // buffer through `program_for_document` (the entry overlay), then a
    // definition on the imported name.
    let package = Package::new("server-style");
    package.write(
        "src/A.aev",
        "module A\n\nfn sqrt(x: Real[L]): Real[L] = x\n",
    );
    let source = "module B\n\nimport A\n\nlet length = A.sqrt(2.0 m)\n";
    package.write("src/B.aev", source);
    let path = package.path("src/B.aev");
    let document = Document::new(path.display().to_string(), 1, source);
    let mut cache = AnalysisCache::new();
    let program = cache
        .program_for_document(&path, source)
        .expect("a program for a file in a package");
    let analysis = Analysis::from_program(&document, program);
    assert!(analysis.is_multi_module());
    let location = analysis
        .definition(offset_of(source, "sqrt"))
        .expect("definition");
    assert!(
        location.document.ends_with("A.aev"),
        "{}",
        location.document
    );
    let a_source = std::fs::read_to_string(package.path("src/A.aev")).expect("reads");
    assert_eq!(text_of(&a_source, location.range), "sqrt");
}

#[test]
fn workspace_symbols_cover_every_loaded_module() {
    let (package, source) = package("workspace-symbols");
    let (document, path) = analysis(&package, &source);
    let analysis = Analysis::from_path(&document, &path);
    let all = analysis.workspace_symbols("");
    let names: Vec<&str> = all.iter().map(|entry| entry.symbol.name.as_str()).collect();
    for expected in ["sqrt", "Point", "length"] {
        assert!(names.contains(&expected), "`{expected}` missing: {names:?}");
    }
    // A symbol in another module names that module's file, not the open one.
    let sqrt = all
        .iter()
        .find(|entry| entry.symbol.name == "sqrt")
        .expect("sqrt");
    assert!(sqrt.document.ends_with("A.aev"), "{}", sqrt.document);
    // The query is a case-insensitive name filter.
    let filtered = analysis.workspace_symbols("SQRT");
    assert_eq!(filtered.len(), 1, "{filtered:#?}");
    assert_eq!(filtered[0].symbol.name, "sqrt");
    // Single-file mode answers for the current document only.
    let single = Analysis::new(&document);
    assert_eq!(single.workspace_symbols("").len(), single.symbols().len());
}

#[test]
fn a_missing_import_is_reported_once() {
    let package = Package::new("missing");
    package.write("src/A.aev", "module A\n\nfn value(): Int = 1\n");
    let source = "module E\n\nimport Missing\n\nlet x = 1\n";
    package.write("src/E.aev", source);
    let path = package.path("src/E.aev");
    let document = Document::new(path.display().to_string(), 1, source);
    let analysis = Analysis::from_path(&document, &path);
    let missing: Vec<_> = analysis
        .diagnostics()
        .into_iter()
        .filter(|diagnostic| diagnostic.code.as_deref() == Some("E0607"))
        .collect();
    assert_eq!(missing.len(), 1, "{missing:#?}");
    assert!(
        missing[0].message.contains("Missing"),
        "{}",
        missing[0].message
    );
}

#[test]
fn an_import_cycle_is_reported() {
    let package = Package::new("cycle");
    package.write("src/A.aev", "module A\n\nimport B\n\nfn a(): Int = 1\n");
    package.write("src/B.aev", "module B\n\nimport A\n\nfn b(): Int = 2\n");
    let path = package.path("src/B.aev");
    let source = std::fs::read_to_string(&path).expect("reads");
    let document = Document::new(path.display().to_string(), 1, &source);
    let analysis = Analysis::from_path(&document, &path);
    let codes: Vec<String> = analysis
        .diagnostics()
        .into_iter()
        .filter_map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.iter().any(|code| code == "E0602"), "{codes:?}");
}

#[test]
fn symbols_still_describe_the_open_document() {
    let (package, source) = package("symbols");
    let (document, path) = analysis(&package, &source);
    let analysis = Analysis::from_path(&document, &path);
    let names: Vec<String> = analysis
        .symbols()
        .into_iter()
        .map(|symbol| symbol.name)
        .collect();
    // The outline includes the import as a namespace and the binding it
    // introduces; only the open document's declarations are listed.
    assert_eq!(names, ["A", "length"]);
}
