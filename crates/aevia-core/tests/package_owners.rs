//! A physical source may be instantiated in different package namespaces.
#![cfg(unix)]
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::manifest::Package;
use aevia_core::source::SourceIdentity;
use aevia_types::AdtKey;
use aevia_types::Export;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("aevia-package-owner-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(
        &self,
        name: &str,
        source: &str,
    ) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, source).unwrap();
        path
    }

    fn link(
        &self,
        source: &str,
        target: &str,
    ) {
        let target = self.0.join(target);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        symlink(self.0.join(source), target).unwrap();
    }

    fn manifest(
        &self,
        directory: &str,
        entry: &str,
        dependencies: &str,
    ) {
        self.write(&format!("{directory}/aevia.toml"), &format!("[package]\nname = \"library\"\nversion = \"0.1.0\"\nentry = \"{entry}\"\n[dependencies]\n{dependencies}"));
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn load(entry: &Path) -> driver::Program {
    let package = Package::owning(entry).unwrap().unwrap();
    driver::load_with_roots(entry, package.roots().unwrap())
}
fn run(
    program: &driver::Program,
    expected: &str,
) {
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked)
            .with_buffer()
            .with_mode(mode);
        let diagnostics = driver::run_modules(&mut interp, program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        interp
            .run_main()
            .unwrap_or_else(|error| panic!("{mode:?}: {error:?}"));
        assert_eq!(interp.captured(), expected, "{mode:?}");
    }
}
fn token_key(checked: &aevia_types::CheckedModule) -> AdtKey {
    match checked.exports.get("Token").unwrap() {
        | Export::Type { adt } | Export::TypeCtor { adt, .. } => aevia_types::adt::key_of(*adt),
        | _ => panic!("Token type"),
    }
}
const API: &str = "module Api\nimport Helper\ntype Token = | Token(Int)\nfn value(): Int = Helper.value()\nfn make(): Token = Token(1)\n";

#[test]
fn a_shared_source_file_uses_each_owners_helpers_and_nominal_identity_while_aliases_merge() {
    let scratch = Scratch::new("shared-file");
    scratch.write("shared/Api.aev", API);
    for (directory, value) in [("one", 7), ("two", 11)] {
        scratch.manifest(directory, "src/Api.aev", "");
        scratch.write(
            &format!("{directory}/src/Helper.aev"),
            &format!("module Helper\nfn value(): Int = {value}\n"),
        );
        scratch.link("shared/Api.aev", &format!("{directory}/src/Api.aev"));
    }
    scratch.manifest(
        "app",
        "src/main.aev",
        "A = { path = \"../one\" }\nAgain = { path = \"../one\" }\nB = { path = \"../two\" }\n",
    );
    scratch.write(
        "app/src/Helper.aev",
        "module Helper\nfn value(): Int = 99\n",
    );
    let entry = scratch.write("app/src/main.aev", "import A\nimport Again\nimport B\nlet same: A.Token = Again.make()\nlet other = B.make()\nfn main(): IO Unit = println(\"{A.value()} {Again.value()} {B.value()}\")\n");
    let program = load(&entry);
    run(&program, "7 7 11\n");
    assert_eq!(
        program.modules.len(),
        5,
        "two owner instances, aliases share one instance"
    );
    let apis: Vec<_> = program
        .modules
        .iter()
        .filter(|module| module.path == ["Api"])
        .collect();
    assert_eq!(apis.len(), 2);
    assert_ne!(apis[0].file, apis[1].file);
    assert_ne!(apis[0].source.owner, apis[1].source.owner);
    assert!(apis.iter().all(|module| module.source.owner.is_some()));
    assert_ne!(token_key(&apis[0].checked), token_key(&apis[1].checked));
    assert_eq!(
        program.sources.file(apis[0].file).unwrap().name,
        program.sources.file(apis[1].file).unwrap().name
    );
}

#[test]
fn external_symlinked_src_trees_retain_their_owner_and_do_not_capture_application_modules() {
    let scratch = Scratch::new("shared-src");
    scratch.write("shared/Api.aev", API);
    scratch.write("shared/Helper.aev", "module Helper\nfn value(): Int = 3\n");
    for directory in ["one", "two"] {
        scratch.manifest(directory, "src/Api.aev", "");
        scratch.link("shared", &format!("{directory}/src"));
    }
    scratch.manifest(
        "app",
        "src/main.aev",
        "A = { path = \"../one\" }\nB = { path = \"../two\" }\n",
    );
    scratch.write(
        "app/src/Helper.aev",
        "module Helper\nfn value(): Int = 99\n",
    );
    let entry = scratch.write(
        "app/src/main.aev",
        "import A\nimport B\nfn main(): IO Unit = println(\"{A.value()} {B.value()}\")\n",
    );
    let program = load(&entry);
    run(&program, "3 3\n");
    assert_eq!(
        program.modules.len(),
        5,
        "both physical sources are checked in both owners"
    );
    let keys: Vec<_> = program
        .modules
        .iter()
        .filter(|module| module.path == ["Api"])
        .map(|module| token_key(&module.checked))
        .collect();
    assert_eq!(keys.len(), 2);
    assert_ne!(keys[0], keys[1]);
}

#[test]
fn an_unsaved_buffer_in_a_symlinked_source_tree_matches_its_saved_owned_identity() {
    let scratch = Scratch::new("dirty-src");
    scratch.manifest("pkg", "src/Api.aev", "");
    scratch.write("shared/Helper.aev", "module Helper\nfn value(): Int = 5\n");
    scratch.link("shared", "pkg/src");
    let entry = scratch.0.join("pkg/src/New.aev");
    let package = Package::owning(&entry).unwrap().unwrap();
    let source = "module New\nimport Helper\ntype Token = | Token(Int)\nfn main(): IO Unit = println(\"{Helper.value()}\")\n";
    let dirty = driver::load_with_roots_and_source(
        &entry,
        package.roots().unwrap(),
        source,
        &aevia_types::check::CheckOptions::default(),
    );
    run(&dirty, "5\n");
    scratch.write("shared/New.aev", source);
    let saved = driver::load_with_roots(&entry, package.roots().unwrap());
    run(&saved, "5\n");
    assert_eq!(
        token_key(&dirty.entry().unwrap().checked),
        token_key(&saved.entry().unwrap().checked)
    );
    let explicit = aevia_core::compile_with_source_identity(
        "shared/New.aev",
        source,
        &SourceIdentity::for_package(&package, &scratch.0.join("shared/New.aev")),
    );
    assert_eq!(
        token_key(&explicit.checked),
        token_key(&saved.entry().unwrap().checked)
    );
}

#[test]
fn cross_owner_reuse_is_not_a_cycle_but_returning_to_the_same_owner_is() {
    let scratch = Scratch::new("owner-cycle");
    scratch.write(
        "shared/Api.aev",
        "module Api\nimport Helper\nfn value(): Int = Helper.value()\n",
    );
    scratch.manifest("one", "src/Api.aev", "Next = { path = \"../two\" }\n");
    scratch.manifest("two", "src/Api.aev", "");
    for directory in ["one", "two"] {
        scratch.link("shared/Api.aev", &format!("{directory}/src/Api.aev"));
    }
    scratch.write(
        "one/src/Helper.aev",
        "module Helper\nimport Next\nfn value(): Int = Next.value() + 1\n",
    );
    scratch.write(
        "two/src/Helper.aev",
        "module Helper\nfn value(): Int = 10\n",
    );
    scratch.manifest("app", "src/main.aev", "A = { path = \"../one\" }\n");
    let entry = scratch.write(
        "app/src/main.aev",
        "import A\nfn main(): IO Unit = println(\"{A.value()}\")\n",
    );
    run(&load(&entry), "11\n");
    scratch.write(
        "two/src/Helper.aev",
        "module Helper\nimport A\nfn value(): Int = A.value()\n",
    );
    let cyclic = load(&entry);
    let diagnostic = cyclic
        .diagnostics
        .items
        .iter()
        .find(|diagnostic| diagnostic.code == "E0602")
        .expect("owner-aware import cycle");
    assert!(
        diagnostic.message.contains("[package"),
        "{}",
        diagnostic.message
    );
    assert!(
        diagnostic.message.contains("/one]"),
        "{}",
        diagnostic.message
    );
    assert!(
        diagnostic.message.contains("/two]"),
        "{}",
        diagnostic.message
    );
}

#[test]
fn cached_source_declarations_are_validated_for_each_logical_import_edge() {
    let scratch = Scratch::new("cached-declaration");
    scratch.write("shared/A.aev", "module A\nfn value(): Int = 1\n");
    scratch.manifest("lib", "src/A.aev", "");
    scratch.link("shared/A.aev", "lib/src/A.aev");
    scratch.link("shared/A.aev", "lib/src/B.aev");
    scratch.manifest("app", "src/main.aev", "Lib = { path = \"../lib\" }\n");
    let entry = scratch.write(
        "app/src/main.aev",
        "import Lib.A\nimport Lib.B\nfn main(): IO Unit = println(\"done\")\n",
    );
    let program = load(&entry);
    assert!(
        program
            .diagnostics
            .items
            .iter()
            .any(|diagnostic| diagnostic.code == "E0601" && diagnostic.message.contains("Lib.B")),
        "{:?}",
        program.diagnostics.items
    );
}

#[test]
fn an_unsaved_physical_buffer_is_shared_across_owning_instantiations() {
    let scratch = Scratch::new("shared-overlay");
    scratch.write(
        "shared/Api.aev",
        "module Api\nimport Helper\nfn value(): Int = Helper.value()\n",
    );
    scratch.manifest("one", "src/Api.aev", "Next = { path = \"../two\" }\n");
    scratch.manifest("two", "src/Api.aev", "");
    for directory in ["one", "two"] {
        scratch.link("shared/Api.aev", &format!("{directory}/src/Api.aev"));
    }
    scratch.write(
        "one/src/Helper.aev",
        "module Helper\nimport Next\nfn value(): Int = Next.value() + 1\n",
    );
    scratch.write(
        "two/src/Helper.aev",
        "module Helper\nfn value(): Int = 10\n",
    );
    let entry = scratch.0.join("one/src/Api.aev");
    let package = Package::owning(&entry).unwrap().unwrap();
    let edited = "module Api\nimport Helper\nfn value(): Int = Helper.value() + 1\nfn main(): IO Unit = println(\"{value()}\")\n";
    let program = driver::load_with_roots_and_source(
        &entry,
        package.roots().unwrap(),
        edited,
        &aevia_types::check::CheckOptions::default(),
    );
    run(&program, "13\n");
}
