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

fn manifest(
    name: &str,
    entry: &str,
    dependencies: &str,
) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nentry = \"{entry}\"\n\n[dependencies]\n{dependencies}"
    )
}

#[test]
fn library_local_imports_capture_the_librarys_own_module_in_both_evaluators() {
    let scratch = Scratch::new("local-scopes");
    scratch.write("lib/aevia.toml", &manifest("lib", "src/entry.aev", ""));
    scratch.write("lib/src/Helper.aev", "module Helper\nfn value(): Int = 7\n");
    scratch.write(
        "lib/src/entry.aev",
        "module Lib\nimport Helper\nfn value(): Int = Helper.value()\n",
    );
    scratch.write(
        "pkg/aevia.toml",
        &manifest("pkg", "src/main.aev", "Renamed = { path = \"../lib\" }\n"),
    );
    scratch.write(
        "pkg/src/Helper.aev",
        "module Helper\nfn value(): Int = 11\n",
    );
    let entry = scratch.write("pkg/src/main.aev", "import Helper\nimport Renamed\nfn main(): IO Unit = println(\"{Helper.value()} {Renamed.value()}\")\n");
    let program = load(&entry);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [aevia_core::Mode::Ast, aevia_core::Mode::Ir] {
        let mut interp = aevia_core::Interp::new(&program.entry().unwrap().checked)
            .with_buffer()
            .with_mode(mode);
        assert!(!driver::run_modules(&mut interp, &program).has_errors());
        interp.run_main().expect("runs");
        assert_eq!(interp.captured(), "11 7\n", "{mode:?}");
    }
}

#[test]
fn dependency_aliases_share_one_module_but_are_all_locked() {
    let scratch = Scratch::new("aliases");
    scratch.write("lib/aevia.toml", &manifest("lib", "src/lib.aev", ""));
    scratch.write("lib/src/lib.aev", "module Lib\nfn value(): Int = 4\n");
    scratch.write(
        "pkg/aevia.toml",
        &manifest(
            "pkg",
            "src/main.aev",
            "First = { path = \"../lib\" }\nSecond = { path = \"../lib/.\" }\n",
        ),
    );
    let entry = scratch.write("pkg/src/main.aev", "import First\nimport Second\nfn main(): IO Unit = println(\"{First.value() + Second.value()}\")\n");
    assert_eq!(run(&entry), "8\n");
    let program = load(&entry);
    assert_eq!(program.modules.len(), 2, "one physical library parsed once");
    let package = Package::owning(&entry).unwrap().unwrap();
    let lock = package.resolve().unwrap();
    assert_eq!(lock.packages.len(), 3);
    assert_eq!(lock.packages[1].import, "First");
    assert_eq!(lock.packages[2].import, "Second");
    assert_eq!(lock.packages[1].hash, lock.packages[2].hash);
    assert_eq!(lock.packages[1].source, "path+../lib");
    assert_eq!(lock.packages[1].source, lock.packages[2].source);
}

#[test]
fn a_package_qualified_module_declaration_survives_a_dependency_rename() {
    let scratch = Scratch::new("qualified-name");
    scratch.write("lib/aevia.toml", &manifest("library", "src/lib.aev", ""));
    scratch.write(
        "lib/src/Numbers.aev",
        "module Library.Numbers\nfn value(): Int = 42\n",
    );
    scratch.write(
        "pkg/aevia.toml",
        &manifest("pkg", "src/main.aev", "Renamed = { path = \"../lib\" }\n"),
    );
    let entry = scratch.write(
        "pkg/src/main.aev",
        "import Renamed.Numbers\nfn main(): IO Unit = println(\"{Renamed.Numbers.value()}\")\n",
    );
    assert_eq!(run(&entry), "42\n");
}

#[test]
fn lock_resolution_rejects_the_same_alias_conflicts_as_module_resolution() {
    let scratch = Scratch::new("lock-conflict");
    for name in ["one", "two", "base1", "base2"] {
        let dependencies = match name {
            | "one" => "Shared = { path = \"../base1\" }\n",
            | "two" => "Shared = { path = \"../base2\" }\n",
            | _ => "",
        };
        scratch.write(
            &format!("{name}/aevia.toml"),
            &manifest(name, "src/main.aev", dependencies),
        );
    }
    scratch.write(
        "pkg/aevia.toml",
        &manifest(
            "pkg",
            "src/main.aev",
            "One = { path = \"../one\" }\nTwo = { path = \"../two\" }\n",
        ),
    );
    let entry = scratch.write("pkg/src/main.aev", "let x = 1\n");
    let package = Package::owning(&entry).unwrap().unwrap();
    assert_eq!(package.roots().unwrap_err().code, "E0606");
    assert_eq!(package.resolve().unwrap_err().code, "E0606");
}

#[test]
fn lock_hash_covers_manifest_resolution_and_an_entry_outside_src() {
    let scratch = Scratch::new("entry-hash");
    let manifest_path = scratch.write("lib/aevia.toml", &manifest("lib", "api/lib.aev", ""));
    let library = scratch.write("lib/api/lib.aev", "fn value(): Int = 1\n");
    scratch.write(
        "pkg/aevia.toml",
        &manifest("pkg", "src/main.aev", "Lib = { path = \"../lib\" }\n"),
    );
    let entry = scratch.write("pkg/src/main.aev", "let x = 1\n");
    let package = Package::owning(&entry).unwrap().unwrap();
    let before = package.resolve().unwrap();
    std::fs::write(library, "fn value(): Int = 2\n").unwrap();
    let changed_source = package.resolve().unwrap();
    assert_ne!(before.packages[1].hash, changed_source.packages[1].hash);
    std::fs::write(manifest_path, manifest("lib", "api/other.aev", "")).unwrap();
    let changed_entry = package.resolve().unwrap();
    assert_ne!(
        changed_source.packages[1].hash,
        changed_entry.packages[1].hash
    );
}

#[test]
fn resolving_an_in_memory_manifest_uses_the_proposed_dependencies() {
    let scratch = Scratch::new("proposed-manifest");
    scratch.write("pkg/aevia.toml", &manifest("pkg", "src/main.aev", ""));
    scratch.write("lib/aevia.toml", &manifest("lib", "src/main.aev", ""));
    let entry = scratch.write("pkg/src/main.aev", "let x = 1\n");
    let mut package = Package::owning(&entry).unwrap().unwrap();
    package.manifest.dependencies.insert(
        "Lib".into(),
        aevia_core::manifest::Dependency::Path {
            path: "../lib".into(),
        },
    );
    assert_eq!(package.resolve().unwrap().packages.len(), 2);
    assert!(package.roots().unwrap().packages.contains_key("Lib"));
}

#[cfg(unix)]
#[test]
fn symlink_aliases_have_one_package_identity_and_source_cycles_are_diagnosed() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::new("symlinks");
    scratch.write("lib/aevia.toml", &manifest("lib", "src/main.aev", ""));
    scratch.write("lib/src/Numbers.aev", "fn value(): Int = 1\n");
    symlink(scratch.path().join("lib"), scratch.path().join("linked")).unwrap();
    scratch.write(
        "pkg/aevia.toml",
        &manifest(
            "pkg",
            "src/main.aev",
            "First = { path = \"../lib\" }\nSecond = { path = \"../linked\" }\n",
        ),
    );
    let entry = scratch.write("pkg/src/main.aev", "import First.Numbers\nimport Second.Numbers\nfn main(): IO Unit = println(\"{First.Numbers.value() + Second.Numbers.value()}\")\n");
    assert_eq!(run(&entry), "2\n");
    assert_eq!(load(&entry).modules.len(), 2);
    let package = Package::owning(&entry).unwrap().unwrap();
    let lock = package.resolve().unwrap();
    assert_eq!(lock.packages[1].source, lock.packages[2].source);
    symlink(
        scratch.path().join("lib/src"),
        scratch.path().join("lib/src/Loop"),
    )
    .unwrap();
    let error = package.resolve().unwrap_err();
    assert_eq!(error.code, "E0607");
    assert!(error.message.contains("cycle"), "{}", error.message);
}

#[test]
fn same_named_modules_in_distinct_packages_keep_nominal_types_and_instances_separate() {
    let scratch = Scratch::new("nominal-packages");
    for (directory, label) in [("one", "first"), ("two", "second")] {
        // Identical manifest names and module declarations are intentional:
        // package source identity, rather than either spelling, distinguishes them.
        scratch.write(
            &format!("{directory}/aevia.toml"),
            &manifest("library", "src/Model.aev", ""),
        );
        scratch.write(&format!("{directory}/src/Model.aev"), &format!("module Model\ntype Token = | Token(Int)\nclass Pretty a where\n    pretty(x: a): String\ninstance Pretty for Token where\n    pretty(x) = \"{label}\"\nfn make(): Token = Token(1)\nfn label(): String = pretty(make())\n"));
    }
    scratch.write(
        "pkg/aevia.toml",
        &manifest(
            "pkg",
            "src/main.aev",
            "A = { path = \"../one\" }\nB = { path = \"../two\" }\n",
        ),
    );
    let entry = scratch.write(
        "pkg/src/main.aev",
        "import A\nimport B\nfn main(): IO Unit = println(\"{A.label()} {B.label()}\")\n",
    );
    let program = load(&entry);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    let keys: Vec<_> = program
        .modules
        .iter()
        .filter_map(|module| {
            match module.checked.exports.get("Token") {
                | Some(
                    aevia_types::Export::Type { adt } | aevia_types::Export::TypeCtor { adt, .. },
                ) => Some(aevia_types::adt::key_of(*adt)),
                | _ => None,
            }
        })
        .collect();
    assert_eq!(keys.len(), 2);
    assert_ne!(keys[0], keys[1]);
    assert_eq!(keys[0].module.dotted(), keys[1].module.dotted());
    for mode in [aevia_core::Mode::Ast, aevia_core::Mode::Ir] {
        let mut interp = aevia_core::Interp::new(&program.entry().unwrap().checked)
            .with_buffer()
            .with_mode(mode);
        let diagnostics = driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        interp.run_main().unwrap();
        assert_eq!(interp.captured(), "first second\n", "{mode:?}");
    }
}

#[test]
fn qualified_alias_types_follow_the_direct_import_binding() {
    let scratch = Scratch::new("alias-type-binding");
    scratch.write("lib/aevia.toml", &manifest("library", "src/Model.aev", ""));
    scratch.write(
        "lib/src/Model.aev",
        "module Model\ntype Pair = { x: Int, y: Int }\nfn make(): Pair = { x = 1, y = 2 }\n",
    );
    scratch.write(
        "pkg/aevia.toml",
        &manifest("pkg", "src/main.aev", "Renamed = { path = \"../lib\" }\n"),
    );
    let entry = scratch.write("pkg/src/main.aev", "import Renamed as R\nlet value: R.Pair = R.make()\nfn main(): IO Unit = println(\"{value.x + value.y}\")\n");
    assert_eq!(run(&entry), "3\n");
}

#[test]
fn selecting_a_type_imports_its_public_constructor_values() {
    let scratch = Scratch::new("selected-constructors");
    scratch.write("lib/aevia.toml", &manifest("lib", "src/Model.aev", ""));
    scratch.write(
        "lib/src/Model.aev",
        "module Model\ntype Choice = | First | Second\n",
    );
    scratch.write(
        "pkg/aevia.toml",
        &manifest("pkg", "src/main.aev", "Lib = { path = \"../lib\" }\n"),
    );
    let entry = scratch.write("pkg/src/main.aev", "import Lib { Choice }\nlet choice: Choice = First\nfn label(): String = match choice with\n    | First -> \"first\"\n    | Second -> \"second\"\nfn main(): IO Unit = println(label())\n");
    assert_eq!(run(&entry), "first\n");
}

#[test]
fn kernel_rule_set_namespaces_are_virtual_imports_with_physical_override() {
    let scratch = Scratch::new("virtual-kernel");
    let entry = scratch.write("main.aev", "import Rssn_numerical_vector as V\nfn main(): IO Unit = println(\"{V.num_dot([1.0, 2.0], [3.0, 4.0])}\")\n");
    assert_eq!(run(&entry), "11\n");
    let program = load(&entry);
    assert!(program.modules.iter().any(|module| {
        program
            .sources
            .file(module.file)
            .is_some_and(|source| source.name.starts_with("rssn://"))
    }));
    scratch.write("Rssn_numerical_vector.aev", "module Rssn_numerical_vector\nfn num_dot(a: Tensor<1, Float>, b: Tensor<1, Float>): Float = 99.0\n");
    assert_eq!(run(&entry), "99\n");
}

#[test]
fn imported_modules_keep_numeric_and_dynamic_dimension_evidence_in_both_evaluators() {
    let scratch = Scratch::new("module-evidence");
    scratch.write("A.aev", "module A\nfn value(): Float = 1\nfn checked(): String = match check<L>(dyn(2.0 s)) with\n    | Ok(_) -> \"wrong\"\n    | Err(_) -> \"rejected\"\n");
    // The literals deliberately have identical byte offsets in distinct files.
    scratch.write("B.aev", "module B\nfn value(): Real  = 1\n");
    let entry = scratch.write("main.aev", "import A\nimport B\nlet float_value = A.value()\nlet exact_value = B.value()\nlet checked = A.checked()\nfn main(): IO Unit = println(checked)\n");
    let program = load(&entry);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [aevia_core::Mode::Ast, aevia_core::Mode::Ir] {
        let mut interp = aevia_core::Interp::new(&program.entry().unwrap().checked)
            .with_buffer()
            .with_mode(mode);
        let diagnostics = driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        assert!(matches!(
            interp.eval_toplevel("float_value").unwrap(),
            aevia_core::value::Value::Num(aevia_core::value::NumVal {
                kind: aevia_core::value::NumKind::Float(_),
                ..
            })
        ));
        assert!(matches!(
            interp.eval_toplevel("exact_value").unwrap(),
            aevia_core::value::Value::Num(aevia_core::value::NumVal {
                kind: aevia_core::value::NumKind::Exact(_),
                ..
            })
        ));
        interp.run_main().unwrap();
        assert_eq!(interp.captured(), "rejected\n");
        let dictionary_count = interp.dictionary_sites.len();
        let rssn_count = interp.rssn_call_sites.len();
        interp.prepare_module(&program.entry().unwrap().checked);
        assert_eq!(
            interp.dictionary_sites.len(),
            dictionary_count,
            "preparing twice is idempotent"
        );
        assert_eq!(interp.rssn_call_sites.len(), rssn_count);
    }
}

#[test]
fn same_named_private_units_resolve_in_the_defining_source_file() {
    let scratch = Scratch::new("module-units");
    scratch.write(
        "A.aev",
        "module A\nprivate unit hop : Length = 2 m\nfn value(): Float[L] = 1.0 hop\n",
    );
    scratch.write(
        "B.aev",
        "module B\nprivate unit hop : Length = 3 m\nfn value(): Float[L] = 1.0 hop\n",
    );
    let entry = scratch.write(
        "main.aev",
        "import A\nimport B\nlet a = A.value()\nlet b = B.value()\nfn main(): IO Unit = println(\"\")\n",
    );
    let program = load(&entry);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [aevia_core::Mode::Ast, aevia_core::Mode::Ir] {
        let mut interp = aevia_core::Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        assert_eq!(interp.eval_toplevel("a").unwrap().as_f64(), Some(2.0));
        assert_eq!(interp.eval_toplevel("b").unwrap().as_f64(), Some(3.0));
    }
}

#[test]
fn checked_value_lists_preserve_nested_tensor_argument_boundaries() {
    let source = "let values = [to_value([1.0, 2.0]), to_value([3.0])]\n";
    let compiled = aevia_core::compile("boxed-values.aev", source);
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:?}",
        compiled.diagnostics.items
    );
    for mode in [aevia_core::Mode::Ast, aevia_core::Mode::Ir] {
        let mut interp = aevia_core::Interp::new(&compiled.checked).with_mode(mode);
        let diagnostics = interp.load(compiled.module.as_ref().unwrap());
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let aevia_core::value::Value::Tensor(tensor) = interp.eval_toplevel("values").unwrap()
        else {
            panic!("list")
        };
        assert_eq!(tensor.shape, vec![2]);
        assert!(matches!(
            tensor.storage,
            aevia_core::value::Storage::Boxed(_)
        ));
        for (index, expected) in [(0, vec![1.0_f64, 2.0]), (1, vec![3.0_f64])] {
            let Some(aevia_core::value::Value::KernelValue(argument)) = tensor.get_flat(index)
            else {
                panic!("opaque tensor argument {index}")
            };
            let aevia_rssn::Val::Tensor { shape, data } = &argument.payload else {
                panic!("kernel tensor payload {index}")
            };
            assert_eq!(shape, &[expected.len()]);
            assert_eq!(data.len(), expected.len());
            for (actual, expected) in data.iter().zip(expected) {
                let aevia_rssn::Val::Num(aevia_rssn::Num::Float(actual)) = actual else {
                    panic!("Float tensor element")
                };
                assert_eq!(actual.to_bits(), expected.to_bits());
            }
            let aevia_rssn::witness::DimensionWitness::Tensor {
                dimension,
                element_repr,
                ..
            } = &argument.witness
            else {
                panic!("tensor dimension witness")
            };
            assert_eq!(dimension, &Some(aevia_dim::Dim::one()));
            assert_eq!(element_repr, &Some(aevia_rssn::SigType::Float));
        }
    }
}
