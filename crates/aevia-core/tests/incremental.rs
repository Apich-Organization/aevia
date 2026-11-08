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
