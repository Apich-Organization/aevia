//! Registry dependencies through the `aevia` binary, entirely offline.
//!
//! The registry cache (`.aevia/registry`) is written by hand here exactly as
//! `aevia-pkg` leaves it: an index file and a verified, unpacked source with
//! its checksum marker. Everything `aevia` does with it must work without the
//! network and without `aevia-pkg` on the machine.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

const CHECKSUM: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("aevia-cli-registry-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }

    fn write(
        &self,
        name: &str,
        text: &str,
    ) {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).expect("parent");
        std::fs::write(path, text).expect("write");
    }

    fn read(
        &self,
        name: &str,
    ) -> String {
        std::fs::read_to_string(self.0.join(name)).unwrap_or_default()
    }

    fn aevia(
        &self,
        args: &[&str],
    ) -> Output {
        Command::new(env!("CARGO_BIN_EXE_aevia"))
            .args(args)
            .current_dir(self.0.join("app"))
            // No registry client on purpose: offline must not need it.
            .env("AEVIA_PKG", self.0.join("no-such-aevia-pkg"))
            .env_remove("AEVIA_REGISTRY_DIR")
            .output()
            .expect("run aevia")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn index_line(
    version: &str,
    yanked: bool,
) -> String {
    format!(
        "{{\"name\":\"apich/units\",\"vers\":\"{version}\",\"deps\":[],\"cksum\":\"{CHECKSUM}\",\"yanked\":{yanked}}}\n"
    )
}

/// A project with a cached index of `apich/units` and a vendored 1.2.0.
fn project(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(
        "app/aevia.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n",
    );
    scratch.write(
        "app/src/main.aev",
        "import Units.Numbers\nfn main(): IO Unit = println(\"{Units.Numbers.value()}\")\n",
    );
    let index =
        index_line("1.0.0", false) + &index_line("1.2.0", false) + &index_line("1.3.0", true);
    scratch.write("app/.aevia/registry/index/apich/units", &index);
    let dir = "app/.aevia/registry/src/apich/units/1.2.0";
    scratch.write(
        &format!("{dir}/aevia.toml"),
        "[package]\nname = \"units\"\nversion = \"1.2.0\"\n",
    );
    scratch.write(
        &format!("{dir}/src/Numbers.aev"),
        "module Units.Numbers\nfn value(): Int = 42\n",
    );
    scratch.write(
        &format!("{dir}/.aevia-pkg-checksum"),
        &format!("{CHECKSUM}\n"),
    );
    scratch
}

#[test]
fn add_resolves_from_the_cache_writes_manifest_and_lock_and_the_program_runs() {
    let scratch = project("add");
    let added = scratch.aevia(&["add", "apich/units@^1", "--offline"]);
    assert!(added.status.success(), "{}", text(&added.stderr));
    let manifest = scratch.read("app/aevia.toml");
    assert!(
        manifest.contains("Units = { package = \"apich/units\", version = \"^1\" }"),
        "{manifest}"
    );
    let lock = scratch.read("app/aevia.lock");
    assert!(lock.contains("name = \"apich/units\""), "{lock}");
    assert!(
        lock.contains("version = \"1.2.0\""),
        "newest non-yanked: {lock}"
    );
    assert!(lock.contains("source = \"registry+aevia\""), "{lock}");
    assert!(
        lock.contains(&format!("checksum = \"{CHECKSUM}\"")),
        "{lock}"
    );

    // The compiler reads the vendored copy.
    let run = scratch.aevia(&["--locked", "run"]);
    assert!(run.status.success(), "{}", text(&run.stderr));
    assert_eq!(text(&run.stdout), "42\n");

    // The tree names both kinds of package.
    let tree = scratch.aevia(&["tree", "--offline"]);
    assert!(tree.status.success(), "{}", text(&tree.stderr));
    let tree = text(&tree.stdout);
    assert!(tree.starts_with("app 0.1.0\n"), "{tree}");
    assert!(
        tree.contains("Units apich/units 1.2.0 [registry]"),
        "{tree}"
    );

    // Re-adding is a no-op; `update` and `fetch` agree with the lock.
    let again = scratch.aevia(&["add", "apich/units@^1", "--offline"]);
    assert!(
        text(&again.stdout).contains("already a dependency"),
        "{}",
        text(&again.stdout)
    );
    let update = scratch.aevia(&["--locked", "update", "--offline"]);
    assert!(update.status.success(), "{}", text(&update.stderr));
    let fetch = scratch.aevia(&["fetch", "--offline"]);
    assert!(fetch.status.success(), "{}", text(&fetch.stderr));
    assert!(
        text(&fetch.stdout).contains("vendored and verified"),
        "{}",
        text(&fetch.stdout)
    );
}

#[test]
fn add_without_a_requirement_writes_a_caret_on_the_newest_release() {
    let scratch = project("default-req");
    let added = scratch.aevia(&["add", "apich/units", "--offline", "--as", "Units"]);
    assert!(added.status.success(), "{}", text(&added.stderr));
    assert!(
        scratch
            .read("app/aevia.toml")
            .contains("Units = { package = \"apich/units\", version = \"^1.2.0\" }"),
        "{}",
        scratch.read("app/aevia.toml")
    );
}

#[test]
fn a_requirement_nothing_satisfies_is_explained_and_nothing_is_written() {
    let scratch = project("unsat");
    let before = scratch.read("app/aevia.toml");
    let added = scratch.aevia(&["add", "apich/units@^7", "--offline"]);
    assert!(!added.status.success());
    let error = text(&added.stderr);
    assert!(error.contains("E0606"), "{error}");
    assert!(error.contains("available: 1.0.0, 1.2.0"), "{error}");
    assert_eq!(scratch.read("app/aevia.toml"), before);
    assert!(!Path::new(&scratch.0.join("app/aevia.lock")).exists());
}

#[test]
fn offline_names_the_missing_index_instead_of_guessing() {
    let scratch = project("missing");
    let added = scratch.aevia(&["add", "someone/else@^1", "--offline"]);
    assert!(!added.status.success());
    let error = text(&added.stderr);
    assert!(
        error.contains("someone/else") && error.contains("--offline"),
        "{error}"
    );
}

#[test]
fn building_refuses_an_unverified_or_missing_copy() {
    let scratch = project("tamper");
    assert!(
        scratch
            .aevia(&["add", "apich/units@^1", "--offline"])
            .status
            .success()
    );
    scratch.write(
        "app/.aevia/registry/src/apich/units/1.2.0/.aevia-pkg-checksum",
        "sha256:feedface\n",
    );
    let run = scratch.aevia(&["run"]);
    assert!(!run.status.success());
    assert!(
        text(&run.stderr).contains("verified archive"),
        "{}",
        text(&run.stderr)
    );
    let fetch = scratch.aevia(&["fetch", "--offline"]);
    assert!(!fetch.status.success());

    std::fs::remove_dir_all(scratch.0.join("app/.aevia/registry/src")).expect("remove");
    let run = scratch.aevia(&["run"]);
    assert!(!run.status.success());
    assert!(
        text(&run.stderr).contains("not vendored"),
        "{}",
        text(&run.stderr)
    );
}

#[test]
fn clean_keeps_the_vendored_registry_unless_asked() {
    let scratch = project("clean");
    scratch.write("app/.aevia/cache/x.aevi", "junk");
    let clean = scratch.aevia(&["clean"]);
    assert!(clean.status.success(), "{}", text(&clean.stderr));
    assert!(!scratch.0.join("app/.aevia/cache").exists());
    assert!(
        scratch
            .0
            .join("app/.aevia/registry/index/apich/units")
            .exists()
    );
    let clean = scratch.aevia(&["clean", "--registry"]);
    assert!(clean.status.success(), "{}", text(&clean.stderr));
    assert!(!scratch.0.join("app/.aevia").exists());
}

#[test]
fn path_only_packages_never_need_the_registry_client() {
    let scratch = Scratch::new("path-only");
    scratch.write(
        "app/aevia.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nLib = { path = \"../lib\" }\n",
    );
    scratch.write("app/src/main.aev", "fn main(): IO Unit = println(\"ok\")\n");
    scratch.write(
        "lib/aevia.toml",
        "[package]\nname = \"lib\"\nversion = \"0.2.0\"\n",
    );
    scratch.write("lib/src/main.aev", "let z = 1\n");
    let update = scratch.aevia(&["update"]);
    assert!(update.status.success(), "{}", text(&update.stderr));
    let tree = scratch.aevia(&["tree"]);
    assert!(
        text(&tree.stdout).contains("Lib lib 0.2.0 (path ../lib)"),
        "{}",
        text(&tree.stdout)
    );
    assert!(!scratch.read("app/aevia.lock").contains("registry"));
}
