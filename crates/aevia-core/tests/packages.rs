//! Package roots: `import` inside a package's `src/`, a dependency's modules
//! under the dependency's name, and a lock file that re-resolves to the same
//! bytes.

use aevia_core::driver;
use aevia_core::manifest::Lock;
use aevia_core::manifest::Manifest;
use aevia_core::manifest::Package;
use std::path::Path;
use std::path::PathBuf;

/// A scratch directory that removes itself.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("aevia-packages-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create scratch directory");
        Self(path)
    }

    fn write(
        &self,
        name: &str,
        source: &str,
    ) -> PathBuf {
        let path = self.0.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(&path, source).expect("write file");
        path
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Loads `entry` the way the CLI does: through its package when it has one.
fn load(entry: &Path) -> driver::Program {
    match Package::owning(entry).expect("manifest parses") {
        | Some(package) => {
            let roots = package.roots().expect("roots resolve");
            driver::load_with_roots(entry, roots)
        },
        | None => driver::load(entry),
    }
}

fn run(entry: &Path) -> String {
    let program = load(entry);
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
    let entry = program.entry().expect("entry module");
    let mut interp = aevia_core::Interp::new(&entry.checked).with_buffer();
    let diagnostics = driver::run_modules(&mut interp, &program);
    assert!(!diagnostics.has_errors(), "load errors");
    interp.run_main().expect("main runs");
    interp.captured()
}

#[test]
fn an_import_resolves_inside_the_packages_src() {
    let scratch = Scratch::new("src-root");
    scratch.write(
        "aevia.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    scratch.write("src/Helper.aev", "fn value(): Int = 7\n");
    let entry = scratch.write(
        "src/main.aev",
        "import Helper\n\nfn main(): IO Unit = println(\"{Helper.value()}\")\n",
    );
    assert_eq!(run(&entry), "7\n");
}

#[test]
fn a_dependency_is_importable_under_its_name() {
    let scratch = Scratch::new("dependency");
    scratch.write(
        "dep/aevia.toml",
        "[package]\nname = \"units_extra\"\nversion = \"0.1.0\"\n",
    );
    scratch.write("dep/src/Convert.aev", "fn mm(x: Int): Int = x * 1000\n");
    scratch.write(
        "pkg/aevia.toml",
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\n\n[dependencies]\nUnitsExtra = { path = \"../dep\" }\n",
    );
    let entry = scratch.write(
        "pkg/src/main.aev",
        "import UnitsExtra.Convert as C\n\nfn main(): IO Unit = println(\"{C.mm(2)}\")\n",
    );
    assert_eq!(run(&entry), "2000\n");
}

#[test]
fn a_dependencys_own_dependency_is_reachable_too() {
    // A module in a dependency may import a module of *its* dependency, so the
    // roots are resolved transitively.
    let scratch = Scratch::new("transitive");
    scratch.write(
        "base/aevia.toml",
        "[package]\nname = \"base\"\nversion = \"0.1.0\"\n",
    );
    scratch.write("base/src/Numbers.aev", "fn two(): Int = 2\n");
    scratch.write(
        "mid/aevia.toml",
        "[package]\nname = \"mid\"\nversion = \"0.1.0\"\n\n[dependencies]\nBase = { path = \"../base\" }\n",
    );
    scratch.write(
        "mid/src/Use.aev",
        "import Base.Numbers\n\nfn four(): Int = Base.Numbers.two() * 2\n",
    );
    scratch.write(
        "pkg/aevia.toml",
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\n\n[dependencies]\nMid = { path = \"../mid\" }\n",
    );
    let entry = scratch.write(
        "pkg/src/main.aev",
        "import Mid.Use\n\nfn main(): IO Unit = println(\"{Mid.Use.four()}\")\n",
    );
    assert_eq!(run(&entry), "4\n");
}

#[test]
fn a_test_file_reaches_the_packages_src_and_its_own_neighbours() {
    // §7.5.3: `src/`, `tests/` and `examples/` are the package's module trees.
    // A test therefore imports the package's modules by their `src/` path and
    // a helper next to itself.
    let scratch = Scratch::new("tests-tree");
    scratch.write(
        "aevia.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    scratch.write("src/Helper.aev", "fn one(): Int = 1\n");
    scratch.write("tests/Local.aev", "fn two(): Int = 2\n");
    let entry = scratch.write(
        "tests/check.aev",
        "import Helper\nimport Local\n\nfn main(): IO Unit = println(\"{Helper.one() + Local.two()}\")\n",
    );
    assert_eq!(run(&entry), "3\n");
}

#[test]
fn two_packages_claiming_one_import_name_is_e0606() {
    let scratch = Scratch::new("conflict");
    for (name, value) in [("one", "1"), ("two", "2")] {
        scratch.write(
            &format!("{name}/aevia.toml"),
            &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
        );
        scratch.write(
            &format!("{name}/src/Shared.aev"),
            &format!("fn value(): Int = {value}\n"),
        );
    }
    scratch.write(
        "pkg/aevia.toml",
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\n\n[dependencies]\nOne = { path = \"../one\" }\nTwo = { path = \"../two\" }\n",
    );
    // Both `one` and `two` claim the import root `Shared`.
    scratch.write(
        "one/aevia.toml",
        "[package]\nname = \"one\"\nversion = \"0.1.0\"\n\n[dependencies]\nShared = { path = \"../base1\" }\n",
    );
    scratch.write(
        "two/aevia.toml",
        "[package]\nname = \"two\"\nversion = \"0.1.0\"\n\n[dependencies]\nShared = { path = \"../base2\" }\n",
    );
    for name in ["base1", "base2"] {
        scratch.write(
            &format!("{name}/aevia.toml"),
            &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
        );
        scratch.write(&format!("{name}/src/X.aev"), "let x = 1\n");
    }
    let entry = scratch.write("pkg/src/main.aev", "fn main(): IO Unit = ()\n");
    let package = Package::owning(&entry).expect("parses").expect("owned");
    let error = package.roots().expect_err("conflicting root");
    assert_eq!(error.code, "E0606");
}

#[test]
fn a_missing_dependency_is_e0607() {
    let scratch = Scratch::new("missing-dep");
    scratch.write(
        "pkg/aevia.toml",
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\n\n[dependencies]\nGone = { path = \"../nowhere\" }\n",
    );
    let entry = scratch.write("pkg/src/main.aev", "fn main(): IO Unit = ()\n");
    let package = Package::owning(&entry).expect("parses").expect("owned");
    let error = package.roots().expect_err("missing dependency");
    assert_eq!(error.code, "E0607");
}

#[test]
fn a_file_outside_src_keeps_the_adjacent_root() {
    // The behaviour that must not break: a file that is not part of a package
    // resolves its imports next to itself.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let entry = root.join("examples").join("modules").join("main.aev");
    assert!(
        Package::owning(&entry).expect("parses").is_none(),
        "an example under `examples/` is not owned by a package"
    );
    let program = driver::load(&entry);
    assert!(!program.diagnostics.has_errors());
    assert_eq!(program.modules.len(), 2, "the example imports one module");
}

#[test]
fn the_lock_file_is_byte_identical_when_nothing_changed() {
    let scratch = Scratch::new("lock");
    scratch.write(
        "dep/aevia.toml",
        "[package]\nname = \"dep\"\nversion = \"0.2.0\"\n",
    );
    scratch.write("dep/src/A.aev", "let a = 1\n");
    scratch.write(
        "pkg/aevia.toml",
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\n\n[dependencies]\nDep = { path = \"../dep\" }\n",
    );
    let entry = scratch.write("pkg/src/main.aev", "fn main(): IO Unit = ()\n");
    let package = Package::owning(&entry).expect("parses").expect("owned");
    let first = package.resolve().expect("resolves");
    let second = package.resolve().expect("resolves again");
    assert_eq!(first, second);
    assert_eq!(first.render(), second.render());
    // The rendering parses back to the same set, so a lock file on disk is
    // exactly what the resolver would write.
    let parsed = Lock::parse(&first.render(), "aevia.lock").expect("parses");
    assert_eq!(parsed, first);
    // The root package is first, the dependency names its path relatively.
    assert_eq!(parsed.packages[0].source, "root");
    assert_eq!(parsed.packages[1].source, "path+../dep");
    assert_eq!(parsed.packages[1].import, "Dep");
    assert_eq!(parsed.packages[1].version, "0.2.0");
    assert!(parsed.packages[1].hash.is_some());
}

#[test]
fn the_lock_changes_when_a_dependency_changes() {
    // A lock file is only worth having if a diff means a real change.
    let scratch = Scratch::new("lock-change");
    scratch.write(
        "dep/aevia.toml",
        "[package]\nname = \"dep\"\nversion = \"0.1.0\"\n",
    );
    let file = scratch.write("dep/src/A.aev", "let a = 1\n");
    scratch.write(
        "pkg/aevia.toml",
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\n\n[dependencies]\nDep = { path = \"../dep\" }\n",
    );
    let entry = scratch.write("pkg/src/main.aev", "fn main(): IO Unit = ()\n");
    let package = Package::owning(&entry).expect("parses").expect("owned");
    let before = package.resolve().expect("resolves").render();
    std::fs::write(&file, "let a = 2\n").expect("rewrite");
    let after = package.resolve().expect("resolves").render();
    assert_ne!(before, after);
}

#[test]
fn the_manifest_round_trips_through_disk() {
    let scratch = Scratch::new("manifest");
    let path = scratch.write(
        "pkg/aevia.toml",
        "[package]\nname = \"pkg\"\nversion = \"1.2.3\"\nentry = \"src/main.aev\"\n\n[dependencies]\nA = { path = \"../a\" }\nB = { path = \"../b\" }\n",
    );
    let manifest = Manifest::load(&path).expect("loads");
    assert_eq!(manifest.version, "1.2.3");
    manifest.save(&path).expect("saves");
    assert_eq!(Manifest::load(&path).expect("loads"), manifest);
    // Dependencies are written in key order, so the file is stable.
    let text = std::fs::read_to_string(&path).expect("reads");
    let a = text.find("A = {").expect("A");
    let b = text.find("B = {").expect("B");
    assert!(a < b);
    assert_eq!(scratch.path(), path.parent().unwrap().parent().unwrap());
}

#[test]
fn a_bare_dependency_import_names_the_packages_entry() {
    // `import Lib` (no sub-path) names the package itself, which is its entry
    // file. Joining the package's `src/` with no segment used to produce
    // `src.aev`, so a dependency could only be imported through a module
    // inside it — never by its own name.
    let scratch = Scratch::new("bare");
    scratch.write(
        "lib/aevia.toml",
        "[package]\nname = \"lib\"\nversion = \"0.1.0\"\n",
    );
    scratch.write(
        "lib/src/main.aev",
        "module Lib\n\nfn greet(): String = \"hello from lib\"\n",
    );
    scratch.write(
        "pkg/aevia.toml",
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\n\n[dependencies]\nLib = { path = \"../lib\" }\n",
    );
    let entry = scratch.write(
        "pkg/src/main.aev",
        "import Lib\n\nfn main(): IO Unit = println(Lib.greet())\n",
    );
    assert_eq!(run(&entry), "hello from lib\n");
}

#[test]
fn a_dependency_with_a_custom_entry_is_found_by_its_name() {
    // The entry is a manifest field, so a bare import must follow it rather
    // than assuming `src/main.aev`.
    let scratch = Scratch::new("custom-entry");
    scratch.write(
        "lib/aevia.toml",
        "[package]\nname = \"lib\"\nversion = \"0.1.0\"\nentry = \"src/lib.aev\"\n",
    );
    scratch.write(
        "lib/src/lib.aev",
        "module Lib\n\nfn greet(): String = \"custom\"\n",
    );
    scratch.write(
        "pkg/aevia.toml",
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\n\n[dependencies]\nLib = { path = \"../lib\" }\n",
    );
    let entry = scratch.write(
        "pkg/src/main.aev",
        "import Lib\n\nfn main(): IO Unit = println(Lib.greet())\n",
    );
    assert_eq!(run(&entry), "custom\n");
}
