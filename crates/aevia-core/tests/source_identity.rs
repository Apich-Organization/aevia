//! Stable nominal identity across paths, editor saves, compile APIs and loads.
use aevia_core::compile;
use aevia_core::compile_with_source_identity;
use aevia_core::driver;
use aevia_core::source::SourceIdentity;
use aevia_types::AdtKey;
use aevia_types::Export;
use std::path::Path;
use std::path::PathBuf;

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("aevia-source-{name}-{}", std::process::id()));
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
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A well-formed file URI for a physical path on any platform: a Windows drive
/// path gains the leading slash and forward separators that RFC 8089 requires.
fn file_uri(
    authority: &str,
    path: &Path,
) -> String {
    let text = path
        .to_string_lossy()
        .replace('\\', "/")
        .replace(' ', "%20");
    let text = if text.starts_with('/') {
        text
    } else {
        format!("/{text}")
    };
    format!("file://{authority}{text}")
}

fn key(checked: &aevia_types::CheckedModule) -> AdtKey {
    match checked.exports.get("Token").unwrap() {
        | Export::Type { adt } | Export::TypeCtor { adt, .. } => aevia_types::adt::key_of(*adt),
        | _ => panic!("Token type"),
    }
}
const SOURCE: &str = "module Shared\ntype Token = | Token(Int)\n";

#[test]
fn independent_files_with_the_same_declared_module_have_distinct_nominal_keys() {
    let scratch = Scratch::new("independent");
    let first = scratch.write("one/main.aev", SOURCE);
    let second = scratch.write("two/main.aev", SOURCE);
    let a = compile(first.to_str().unwrap(), SOURCE);
    let b = compile(second.to_str().unwrap(), SOURCE);
    assert_ne!(key(&a.checked), key(&b.checked));
    assert_eq!(a.checked.module_path().dotted(), "Shared");
}

#[test]
fn saving_a_new_file_and_switching_from_compile_to_driver_preserves_identity() {
    let scratch = Scratch::new("save");
    let path = scratch.0.join("new/Shared.aev");
    let before = compile(path.to_str().unwrap(), SOURCE);
    scratch.write("new/Shared.aev", SOURCE);
    let after = compile(path.to_str().unwrap(), SOURCE);
    let loaded = driver::load(&path);
    assert_eq!(key(&before.checked), key(&after.checked));
    assert_eq!(key(&after.checked), key(&loaded.entry().unwrap().checked));
}

#[test]
fn file_uri_and_path_names_share_one_identity_including_percent_encoded_spaces() {
    let scratch = Scratch::new("file-uri");
    let path = scratch.write("space name.aev", SOURCE);
    let uri = file_uri("", &path);
    assert_eq!(
        SourceIdentity::for_path(&path),
        SourceIdentity::from_name(&uri)
    );
    assert_eq!(
        SourceIdentity::for_path(&path),
        SourceIdentity::from_name(&format!("file:{}", &file_uri("", &path)["file://".len()..]))
    );
}

#[test]
fn package_custom_entry_uses_the_same_identity_in_compile_and_module_loading() {
    let scratch = Scratch::new("package-entry");
    scratch.write(
        "aevia.toml",
        "[package]\nname = \"identity\"\nversion = \"1.2.3\"\nentry = \"api/lib.aev\"\n",
    );
    let path = scratch.write("api/lib.aev", SOURCE);
    let package = aevia_core::manifest::Package::owning(&path)
        .unwrap()
        .unwrap();
    let compiled = compile(path.to_str().unwrap(), SOURCE);
    let loaded = driver::load_with_roots(&path, package.roots().unwrap());
    assert_eq!(
        key(&compiled.checked),
        key(&loaded.entry().unwrap().checked)
    );
}

#[test]
fn nominal_keys_do_not_depend_on_module_traversal_file_ids() {
    let scratch = Scratch::new("traversal");
    scratch.write("Model.aev", "module Model\ntype Token = | Token(Int)\n");
    scratch.write("Aux.aev", "module Aux\nlet value = 1\n");
    let first = scratch.write(
        "one.aev",
        "import Aux\nimport Model\nlet value = Model.Token(1)\n",
    );
    let second = scratch.write(
        "two.aev",
        "import Model\nimport Aux\nlet value = Model.Token(1)\n",
    );
    let a = driver::load(&first);
    let b = driver::load(&second);
    assert!(!a.diagnostics.has_errors(), "{:?}", a.diagnostics.items);
    assert!(!b.diagnostics.has_errors(), "{:?}", b.diagnostics.items);
    let a = a
        .modules
        .iter()
        .find(|module| module.path == ["Model"])
        .unwrap();
    let b = b
        .modules
        .iter()
        .find(|module| module.path == ["Model"])
        .unwrap();
    assert_ne!(a.file, b.file);
    assert_eq!(key(&a.checked), key(&b.checked));
}

#[test]
fn explicit_virtual_sources_are_stable_across_revisions_and_distinct_between_sessions() {
    let first = SourceIdentity::virtual_source("aevia-repl://host/one");
    let second = SourceIdentity::virtual_source("aevia-repl://host/two");
    let a = compile_with_source_identity("<repl>", SOURCE, &first);
    let revised =
        compile_with_source_identity("<repl>", &format!("{SOURCE}let other = 1\n"), &first);
    let b = compile_with_source_identity("<repl>", SOURCE, &second);
    assert_eq!(key(&a.checked), key(&revised.checked));
    assert_ne!(key(&a.checked), key(&b.checked));
}

#[cfg(unix)]
#[test]
fn a_directory_symlink_to_a_missing_buffer_matches_its_future_physical_file() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::new("symlink-save");
    std::fs::create_dir(scratch.0.join("real")).unwrap();
    symlink(scratch.0.join("real"), scratch.0.join("linked")).unwrap();
    let physical = scratch.0.join("real/Shared.aev");
    let linked = scratch.0.join("linked/Shared.aev");
    let before = SourceIdentity::for_path(&linked);
    scratch.write("real/Shared.aev", SOURCE);
    assert_eq!(before, SourceIdentity::for_path(&physical));
    assert_eq!(before, SourceIdentity::for_path(&linked));
    assert_eq!(
        SourceIdentity::for_path(&physical),
        SourceIdentity::for_path(&physical.parent().unwrap().join(Path::new("./Shared.aev")))
    );
}

#[test]
fn virtual_uri_schemes_without_double_slashes_remain_virtual_sources() {
    assert_eq!(
        SourceIdentity::from_name("untitled:buffer-one"),
        SourceIdentity::virtual_source("untitled:buffer-one")
    );
    assert_eq!(
        SourceIdentity::from_name("memory+editor:buffer-two"),
        SourceIdentity::virtual_source("memory+editor:buffer-two")
    );
}

#[test]
fn localhost_file_uri_uses_the_same_source_identity_as_the_editor_path() {
    let scratch = Scratch::new("localhost-uri");
    let path = scratch.write("space name.aev", SOURCE);
    let uri = file_uri("localhost", &path);
    assert_eq!(
        SourceIdentity::for_path(&path),
        SourceIdentity::from_name(&uri)
    );
}
