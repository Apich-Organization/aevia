//! §9.11: incremental checking. A module whose source and imported
//! interfaces are unchanged is reused; an edit that leaves a module's
//! interface unchanged does not recheck its importers (early cutoff).

use aevia_core::driver::CheckCache;
use aevia_core::driver::load_incremental;
use aevia_core::manifest::Roots;
use aevia_core::source::SourceOverlays;
use aevia_types::check::CheckOptions;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

fn write(
    path: &Path,
    text: &str,
) {
    std::fs::write(path, text).unwrap();
}

#[test]
fn unchanged_modules_are_reused_and_interface_preserving_edits_cut_off() {
    let directory = std::env::temp_dir().join(format!("aevia-incremental-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("Geometry")).unwrap();
    let shapes = directory.join("Geometry").join("Shapes.aev");
    let main = directory.join("main.aev");
    write(
        &shapes,
        "module Geometry.Shapes\n\nfn area(r: Float[L]): Float[L^2] = 3.14159 * r * r\n",
    );
    write(
        &main,
        "import Geometry.Shapes\nimport Math\n\nlet a = Geometry.Shapes.area(2.0 m)\n",
    );
    let cache = Rc::new(RefCell::new(CheckCache::new()));
    let load = || {
        load_incremental(
            &main,
            Roots::adjacent(&main),
            &SourceOverlays::new(),
            &CheckOptions::default(),
            &cache,
        )
    };

    let (first, stats) = load();
    assert!(
        !first.diagnostics.has_errors(),
        "{:?}",
        first.diagnostics.items
    );
    assert_eq!(
        stats.checks, stats.modules,
        "a cold cache checks everything"
    );
    assert!(stats.modules >= 3);

    // Nothing changed: nothing is checked.
    let (_, stats) = load();
    assert_eq!(stats.checks, 0);
    assert_eq!(cache.borrow().reused, stats.modules);

    // An edit to the entry rechecks the entry only.
    write(
        &main,
        "import Geometry.Shapes\nimport Math\n\nlet a = Geometry.Shapes.area(3.0 m)\n",
    );
    let (second, stats) = load();
    assert!(!second.diagnostics.has_errors());
    assert_eq!(stats.checks, 1, "only the entry changed");

    // A trailing comment changes the dependency's text but not its
    // interface: the dependency is rechecked, its importer is not.
    write(
        &shapes,
        "module Geometry.Shapes\n\nfn area(r: Float[L]): Float[L^2] = 3.14159 * r * r\n# note\n",
    );
    let (_, stats) = load();
    assert_eq!(stats.checks, 1, "early cutoff: the importer is reused");

    // An interface change reaches the importer, and its error is reported.
    write(
        &shapes,
        "module Geometry.Shapes\n\nfn area(r: Float[L]): Float[L^3] = 3.14159 * r * r * r\n",
    );
    write(
        &main,
        "import Geometry.Shapes\nimport Math\n\nlet a: Float[L^2] = Geometry.Shapes.area(3.0 m)\n",
    );
    let (third, stats) = load();
    assert_eq!(stats.checks, 2);
    assert!(
        third.diagnostics.has_errors(),
        "the importer sees the new interface"
    );
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn a_changed_transitive_interface_invalidates_the_consumer_cache() {
    let directory = std::env::temp_dir().join(format!(
        "aevia-incremental-transitive-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let leaf = directory.join("Leaf.aev");
    let middle = directory.join("Middle.aev");
    let main = directory.join("main.aev");
    write(&leaf, "module Leaf\nlet value: Int = 1\n");
    write(
        &middle,
        "module Middle\nimport Leaf\nlet constant: Int = 7\n",
    );
    write(&main, "import Middle\nlet result: Int = Middle.constant\n");
    let cache = Rc::new(RefCell::new(CheckCache::new()));
    let load = || {
        let (program, stats) = load_incremental(
            &main,
            Roots::adjacent(&main),
            &SourceOverlays::new(),
            &CheckOptions::default(),
            &cache,
        );
        assert!(
            !program.diagnostics.has_errors(),
            "{:?}",
            program.diagnostics.items
        );
        stats.checks
    };

    assert_eq!(load(), 3);
    assert_eq!(load(), 0);
    write(&leaf, "module Leaf\nlet value: Int = 2\n");
    assert_eq!(load(), 1);
    write(&leaf, "module Leaf\nlet value: Float = 2.0\n");
    assert_eq!(load(), 3, "the entire visible interface closure is checked");
    assert_eq!(load(), 0);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn cached_syntax_diagnostics_follow_file_ids_and_fresh_text() {
    let directory =
        std::env::temp_dir().join(format!("aevia-incremental-syntax-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let main = directory.join("main.aev");
    let a = directory.join("A.aev");
    let b = directory.join("B.aev");
    write(&a, "module A\nlet value: Int = 1\n");
    write(&b, "module B\nlet invalid = )\n");
    write(&main, "import A\nimport B\nlet value = A.value\n");
    let cache = Rc::new(RefCell::new(CheckCache::new()));
    let load = || {
        load_incremental(
            &main,
            Roots::adjacent(&main),
            &SourceOverlays::new(),
            &CheckOptions::default(),
            &cache,
        )
        .0
    };
    let error_file = |program: &aevia_core::driver::Program| {
        program
            .diagnostics
            .items
            .iter()
            .find(|diagnostic| {
                diagnostic.severity == aevia_syntax::Severity::Error
                    && program
                        .sources
                        .file(diagnostic.span.file)
                        .is_some_and(|file| file.name.ends_with("B.aev"))
            })
            .expect("the bad source must keep its diagnostic")
            .span
            .file
    };

    let first_file = error_file(&load());
    assert_eq!(error_file(&load()), first_file);
    write(&main, "import B\nimport A\nlet value = A.value\n");
    assert_ne!(error_file(&load()), first_file);
    write(&b, "module B\nlet valid: Int = 2\n");
    assert!(!load().diagnostics.has_errors());
    assert_eq!(cache.borrow().len(), 3);
    write(&main, "import A\nlet value = A.value\n");
    assert!(!load().diagnostics.has_errors());
    assert_eq!(cache.borrow().len(), 2);
    std::fs::remove_dir_all(directory).unwrap();
}
