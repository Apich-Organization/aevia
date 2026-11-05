//! The command line, one test per command of §9.9 that this tool provides.
//!
//! Each test drives the real binary in a scratch directory, because what is
//! being tested is the command — its exit code, the files it writes and the
//! files it refuses to write.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

/// A scratch directory that removes itself.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("aevia-cli-{name}-{}", std::process::id()));
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

fn aevia(
    dir: &Path,
    args: &[&str],
) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aevia"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("runs aevia")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n")
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

#[test]
fn init_writes_a_package_that_runs() {
    let scratch = Scratch::new("init");
    let output = aevia(scratch.path(), &["init", "demo"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let package = scratch.path().join("demo");
    for file in ["aevia.toml", "aevia.lock", "src/main.aev"] {
        assert!(package.join(file).is_file(), "{file} was not created");
    }
    let manifest = std::fs::read_to_string(package.join("aevia.toml")).expect("reads");
    assert!(manifest.contains("name = \"demo\""), "{manifest}");
    assert!(manifest.contains("entry = \"src/main.aev\""), "{manifest}");

    // The generated program runs, and `run` with no file uses the entry.
    let output = aevia(&package, &["run"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        stdout(&output).contains("hello from demo"),
        "{}",
        stdout(&output)
    );
    let output = aevia(&package, &["check"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
}

#[test]
fn init_refuses_to_overwrite_a_directory() {
    let scratch = Scratch::new("init-exists");
    let existing = scratch.path().join("demo");
    std::fs::create_dir_all(&existing).expect("create");
    std::fs::write(existing.join("keep.txt"), "do not lose this").expect("write");
    let output = aevia(scratch.path(), &["init", "demo"]);
    assert_ne!(code(&output), 0);
    assert!(
        stderr(&output).contains("already exists"),
        "{}",
        stderr(&output)
    );
    assert_eq!(
        std::fs::read_to_string(existing.join("keep.txt")).expect("reads"),
        "do not lose this"
    );
    assert!(!existing.join("aevia.toml").exists());
}

#[test]
fn add_and_update_resolve_path_dependencies() {
    let scratch = Scratch::new("add");
    assert_eq!(code(&aevia(scratch.path(), &["init", "dep"])), 0);
    assert_eq!(code(&aevia(scratch.path(), &["init", "demo"])), 0);
    let demo = scratch.path().join("demo");
    scratch.write("dep/src/Convert.aev", "fn mm(x: Int): Int = x * 1000\n");
    let output = aevia(&demo, &["add", "../dep"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        stdout(&output).contains("added `Dep`"),
        "{}",
        stdout(&output)
    );

    let manifest = std::fs::read_to_string(demo.join("aevia.toml")).expect("reads");
    assert!(
        manifest.contains("Dep = { path = \"../dep\" }"),
        "{manifest}"
    );
    let lock = std::fs::read_to_string(demo.join("aevia.lock")).expect("reads");
    assert!(lock.contains("import = \"Dep\""), "{lock}");
    assert!(lock.contains("source = \"path+../dep\""), "{lock}");
    assert!(lock.contains("hash = \"fnv1a64:"), "{lock}");

    // Re-resolving an unchanged tree writes the same bytes.
    let first = std::fs::read_to_string(demo.join("aevia.lock")).expect("reads");
    let output = aevia(&demo, &["update"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        stdout(&output).contains("up to date"),
        "{}",
        stdout(&output)
    );
    assert_eq!(
        std::fs::read_to_string(demo.join("aevia.lock")).expect("reads"),
        first
    );

    // And the dependency is importable under its name.
    scratch.write(
        "demo/src/main.aev",
        "import Dep.Convert as C\n\nfn main(): IO Unit = println(\"{C.mm(2)}\")\n",
    );
    let output = aevia(&demo, &["run"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(stdout(&output), "2000\n");
}

#[test]
fn add_refuses_something_that_is_not_a_package() {
    let scratch = Scratch::new("add-bad");
    assert_eq!(code(&aevia(scratch.path(), &["init", "demo"])), 0);
    let demo = scratch.path().join("demo");
    std::fs::create_dir_all(scratch.path().join("plain")).expect("create");
    let output = aevia(&demo, &["add", "../plain"]);
    assert_ne!(code(&output), 0);
    assert!(
        stderr(&output).contains("is not a package"),
        "{}",
        stderr(&output)
    );
    // Nothing was written to the manifest.
    let manifest = std::fs::read_to_string(demo.join("aevia.toml")).expect("reads");
    assert!(!manifest.contains("path ="), "{manifest}");

    // A version range is not resolved by anything, so `add` says so.
    let output = aevia(&demo, &["add", "--version", "0.1"]);
    assert_ne!(code(&output), 0);
    assert!(
        stderr(&output).contains("no registry"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn fmt_formats_and_check_reports_without_writing() {
    let scratch = Scratch::new("fmt");
    let file = scratch.write("a.aev", "let  x=1+2*3\n");
    let output = aevia(scratch.path(), &["fmt", "--check", "a.aev"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(
        stdout(&output).contains("would reformat a.aev"),
        "{}",
        stdout(&output)
    );
    assert_eq!(
        std::fs::read_to_string(&file).expect("reads"),
        "let  x=1+2*3\n",
        "--check must not write"
    );

    let output = aevia(scratch.path(), &["fmt", "a.aev"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(
        std::fs::read_to_string(&file).expect("reads"),
        "let x = 1 + 2 * 3\n"
    );

    let output = aevia(scratch.path(), &["fmt", "--check", "a.aev"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
}

#[test]
fn fmt_leaves_a_file_it_cannot_read_alone() {
    // A syntax error is not something to reformat around: the file must come
    // back byte-identical and the message must say where.
    let scratch = Scratch::new("fmt-bad");
    let source = "fn f( = 1\n";
    let file = scratch.write("bad.aev", source);
    let output = aevia(scratch.path(), &["fmt", "bad.aev"]);
    assert_ne!(code(&output), 0);
    assert!(stderr(&output).contains("line 1"), "{}", stderr(&output));
    assert_eq!(std::fs::read_to_string(&file).expect("reads"), source);
}

#[test]
fn fmt_formats_a_whole_directory() {
    let scratch = Scratch::new("fmt-dir");
    scratch.write("src/a.aev", "let  a=1\n");
    scratch.write("src/nested/b.aev", "fn  f():Int=2\n");
    let output = aevia(scratch.path(), &["fmt", "src"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(
        std::fs::read_to_string(scratch.path().join("src/a.aev")).expect("reads"),
        "let a = 1\n"
    );
    assert_eq!(
        std::fs::read_to_string(scratch.path().join("src/nested/b.aev")).expect("reads"),
        "fn f(): Int = 2\n"
    );
}

#[test]
fn doc_writes_one_page_per_module_and_an_index() {
    let scratch = Scratch::new("doc");
    assert_eq!(code(&aevia(scratch.path(), &["init", "demo"])), 0);
    let demo = scratch.path().join("demo");
    scratch.write(
        "demo/src/geometry.aev",
        "## A circle's area.\nfn circle(r: Real[L]): Real[L^2] = r^2\n\nfn plain(): Int = 1\n",
    );
    let output = aevia(&demo, &["doc"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let page = std::fs::read_to_string(demo.join("docs/geometry.md")).expect("reads");
    assert!(page.contains("fn circle(r: Real[L]): Real[L^2]"), "{page}");
    assert!(page.contains("A circle's area."), "{page}");
    assert!(page.contains("## `fn plain`"), "{page}");
    // The undocumented declaration is listed and no prose was invented for it.
    let tail = page.split("## `fn plain`").nth(1).expect("listed");
    assert!(!tail.contains("A circle"), "{tail}");
    let index = std::fs::read_to_string(demo.join("docs/index.md")).expect("reads");
    assert!(index.contains("geometry.md"), "{index}");
    assert!(index.contains("main.md"), "{index}");

    // `--out` puts the pages elsewhere.
    let output = aevia(&demo, &["doc", "--out", "api"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(demo.join("api/index.md").is_file());

    // The index and the run report undocumented declarations instead of
    // hiding them. The generated `main` is documented; `plain` is not.
    let index = std::fs::read_to_string(demo.join("docs/index.md")).expect("reads");
    assert!(index.contains("Undocumented"), "{index}");
    assert!(index.contains("without a `##` doc comment"), "{index}");
    assert!(
        stdout(&output).contains("without a `##` doc comment"),
        "{}",
        stdout(&output)
    );

    // `--open` shows the docs; with no terminal to show them to it prints the
    // path rather than launching a viewer nobody asked for.
    let output = aevia(&demo, &["doc", "--open", "--out", "api"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stdout(&output).contains("index.md"), "{}", stdout(&output));
}

#[test]
fn bench_times_bench_functions_and_reports_failures() {
    let scratch = Scratch::new("bench");
    let file = scratch.write(
        "bench.aev",
        "let zero = 0\n\nfn add(n: Int, acc: Int): Int = if n == 0 then acc else add(n - 1, acc + n)\n\n\
         @bench\nfn loop(): Int = add(20, 0)\n\n\
         @bench\nfn with_arg(x: Int): Int = x\n\n\
         @bench\nfn broken(): Real = 1.0 / zero\n",
    );
    let output = aevia(scratch.path(), &["bench", "bench.aev"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    let table = stdout(&output);
    assert!(table.contains("benchmark"), "{table}");
    assert!(table.contains("iterations"), "{table}");
    // §9.12: report the median and a confidence interval, not only an average.
    assert!(table.contains("median"), "{table}");
    assert!(table.contains("mean"), "{table}");
    assert!(table.contains("95% CI"), "{table}");
    assert!(table.contains("±"), "{table}");
    assert!(table.contains("loop"), "{table}");
    // A benchmark that cannot run is failed, not timed at zero.
    assert!(table.contains("FAILED"), "{table}");
    assert!(
        table.contains("takes 1 parameter"),
        "the arity failure is reported: {table}"
    );
    assert!(
        table.contains("division by zero"),
        "the runtime failure is reported: {table}"
    );
    let loop_line = table
        .lines()
        .find(|line| line.starts_with("loop"))
        .expect("the loop row");
    assert!(!loop_line.contains("FAILED"), "{loop_line}");
    assert_eq!(
        loop_line.split_whitespace().nth(1),
        Some("100"),
        "{loop_line}"
    );
    // The table says what path was measured and states the comparison
    // protocol; the baseline gate is `--compare-baseline` (tested below).
    assert!(table.contains("Interp::eval"), "{table}");
    assert!(table.contains("allocation"), "{table}");
    assert!(table.contains("protocol:"), "{table}");
    assert!(table.contains("median of N iterations"), "{table}");
    // Debug timings are not comparable with release ones, and the command says
    // which build it is. (`cfg!(debug_assertions)` is true for this test.)
    assert!(
        stderr(&output).contains("debug build"),
        "{}",
        stderr(&output)
    );

    // `--filter` narrows the run, and a run with no failures exits 0.
    let output = aevia(scratch.path(), &["bench", "--filter", "loop", "bench.aev"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(!stdout(&output).contains("FAILED"), "{}", stdout(&output));

    // Iterations are the user's: a single-call benchmark that takes a second
    // cannot be timed a hundred times.
    let output = aevia(
        scratch.path(),
        &[
            "bench",
            "--filter",
            "loop",
            "--iterations",
            "7",
            "bench.aev",
        ],
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let filtered = stdout(&output);
    let loop_line = filtered
        .lines()
        .find(|line| line.starts_with("loop"))
        .unwrap_or_else(|| panic!("the loop row: {filtered}"));
    assert_eq!(
        loop_line.split_whitespace().nth(1),
        Some("7"),
        "{loop_line}"
    );

    // A count of zero is a mistake, not a request for the default.
    let output = aevia(scratch.path(), &["bench", "--iterations", "0", "bench.aev"]);
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("at least 1"),
        "{}",
        stderr(&output)
    );
    let _ = file;
}

#[test]
fn bench_says_so_when_there_is_nothing_to_measure() {
    // A command that runs nothing must not report success.
    let scratch = Scratch::new("bench-empty");
    scratch.write("plain.aev", "fn main(): IO Unit = println(\"hi\")\n");
    let output = aevia(scratch.path(), &["bench", "plain.aev"]);
    assert_ne!(code(&output), 0);
    assert!(
        stderr(&output).contains("no `@bench` functions"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn clean_removes_the_build_directory_and_says_when_there_is_none() {
    // §9.9/§9.11: `clean` removes the `.aevia/` build directory. This
    // toolchain writes no cache yet, so a package that has never been built
    // has nothing to clean — and `clean` says so rather than reporting a
    // removal that did not happen.
    let scratch = Scratch::new("clean");
    assert_eq!(code(&aevia(scratch.path(), &["init", "demo"])), 0);
    let demo = scratch.path().join("demo");
    let output = aevia(&demo, &["clean"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        stdout(&output).contains("nothing to clean"),
        "{}",
        stdout(&output)
    );

    scratch.write("demo/.aevia/target/obj.aev", "let x = 1\n");
    scratch.write("demo/.aevia/interfaces/Geometry.aevi", "interface\n");
    let output = aevia(&demo, &["clean"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stdout(&output).contains("removed"), "{}", stdout(&output));
    assert!(stdout(&output).contains("2 files"), "{}", stdout(&output));
    assert!(!demo.join(".aevia").exists(), ".aevia/ was not removed");

    // Outside a package there is no directory it could mean.
    let output = aevia(scratch.path(), &["clean"]);
    assert_ne!(code(&output), 0);
    assert!(
        stderr(&output).contains("aevia.toml"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn lsp_serves_the_protocol_over_stdio() {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(env!("CARGO_BIN_EXE_aevia"))
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawns");
    let mut stdin = child.stdin.take().expect("stdin");
    for message in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}"#,
        r#"{"jsonrpc":"2.0","method":"exit"}"#,
    ] {
        let frame = format!("Content-Length: {}\r\n\r\n{message}", message.len());
        stdin.write_all(frame.as_bytes()).expect("writes");
    }
    drop(stdin);
    let output = child.wait_with_output().expect("waits");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"capabilities\""), "{stdout}");
    assert!(stdout.contains("aevia-lsp"), "{stdout}");

    // Only the standard-I/O transport exists, and saying so beats hanging.
    let output = aevia(Path::new("."), &["lsp", "--tcp"]);
    assert_eq!(code(&output), 2);
    assert!(stderr(&output).contains("--stdio"), "{}", stderr(&output));
}

#[test]
fn rssn_emit_writes_the_generated_module_into_out() {
    let scratch = Scratch::new("rssn-emit");
    let output = aevia(scratch.path(), &["rssn", "--emit", "--out", "gen"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let generated = scratch.path().join("gen");
    assert!(generated.is_dir(), "`--out` was not created");
    let files: Vec<String> = std::fs::read_dir(&generated)
        .expect("reads")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    assert!(files.iter().any(|name| name == "Rssn.aev"), "{files:?}");
    let text = std::fs::read_to_string(generated.join("Rssn.aev")).expect("reads");
    assert!(!text.is_empty());

    // The old `--emit DIR` spelling still works, because a directory after
    // `--emit` can only have meant the destination.
    let output = aevia(scratch.path(), &["rssn", "--emit", "legacy"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(scratch.path().join("legacy/Rssn.aev").is_file());

    // `--emit` with no destination is refused rather than guessed.
    let output = aevia(scratch.path(), &["rssn", "--emit"]);
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("destination"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn locked_refuses_an_out_of_date_lock_instead_of_rewriting_it() {
    let scratch = Scratch::new("locked");
    assert_eq!(code(&aevia(scratch.path(), &["init", "demo"])), 0);
    let demo = scratch.path().join("demo");

    // A lock that already matches is accepted.
    let output = aevia(&demo, &["check", "--locked"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));

    // A stale lock is an error, and nothing is written by a read-only command.
    std::fs::write(demo.join("aevia.lock"), "# stale\nversion = 1\n").expect("writes");
    let output = aevia(&demo, &["check", "--locked"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("out of date"),
        "{}",
        stderr(&output)
    );
    assert_eq!(
        std::fs::read_to_string(demo.join("aevia.lock")).expect("reads"),
        "# stale\nversion = 1\n",
        "--locked must not rewrite the lock"
    );

    // `update --locked` refuses to write and says how to proceed.
    let output = aevia(&demo, &["update", "--locked"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("refuses to rewrite"),
        "{}",
        stderr(&output)
    );
    // Without `--locked` the lock is rewritten, and then `--locked` passes.
    assert_eq!(code(&aevia(&demo, &["update"])), 0);
    assert_eq!(code(&aevia(&demo, &["check", "--locked"])), 0);

    // Outside a package, `--locked` has no lock to check and says so.
    let output = aevia(scratch.path(), &["check", "--locked"]);
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("aevia.toml"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn the_flags_with_no_effect_are_reported_rather_than_dropped() {
    // §9.9 lists `--release` and `--jobs`. There is no code generator and no
    // parallel phase, so the flags are accepted and the user is told so.
    let scratch = Scratch::new("flags");
    scratch.write("a.aev", "let x = 1\n");
    let output = aevia(scratch.path(), &["check", "--release", "a.aev"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stderr(&output).contains("--release"), "{}", stderr(&output));
    assert!(stderr(&output).contains("no effect"), "{}", stderr(&output));

    let output = aevia(scratch.path(), &["check", "--jobs", "4", "a.aev"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stderr(&output).contains("--jobs 4"), "{}", stderr(&output));
    assert!(stderr(&output).contains("no effect"), "{}", stderr(&output));

    let output = aevia(scratch.path(), &["check", "--jobs", "0", "a.aev"]);
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("at least 1"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn test_filter_selects_by_name_and_refuses_an_empty_selection() {
    let scratch = Scratch::new("test-filter");
    scratch.write(
        "suite.aev",
        "test \"alpha works\" = true\n\ntest \"beta fails\" = false\n",
    );
    // No filter: both run, the failure is reported.
    let output = aevia(scratch.path(), &["test", "suite.aev"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(stdout(&output).contains("alpha works ... ok"));
    assert!(stdout(&output).contains("beta fails ... FAILED"));

    // `--filter` narrows the run to the matching test names.
    let output = aevia(scratch.path(), &["test", "--filter", "alpha", "suite.aev"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stdout(&output).contains("alpha works ... ok"));
    assert!(
        !stdout(&output).contains("beta fails"),
        "{}",
        stdout(&output)
    );

    // A filter that matches nothing must not report success.
    let output = aevia(scratch.path(), &["test", "--filter", "nope", "suite.aev"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("matched no test"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn check_all_checks_modules_the_entry_does_not_import() {
    let scratch = Scratch::new("check-all");
    assert_eq!(code(&aevia(scratch.path(), &["init", "demo"])), 0);
    let demo = scratch.path().join("demo");
    scratch.write("demo/src/orphan.aev", "let broken = 1 m + 1\n");
    // The entry does not import the orphan, so plain `check` passes.
    let output = aevia(&demo, &["check"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    // `--all` checks every module of the package.
    let output = aevia(&demo, &["check", "--all"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(stderr(&output).contains("E0401"), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("orphan.aev"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn check_all_loads_the_package_in_one_pass() {
    // A chain package: main imports H0, H0 imports H1, …, so a per-entry
    // implementation would parse and check the tail once per earlier entry
    // (quadratic). The one-pass loader checks every module exactly once.
    let scratch = Scratch::new("check-all-pass");
    assert_eq!(code(&aevia(scratch.path(), &["init", "demo"])), 0);
    let demo = scratch.path().join("demo");
    const HELPERS: usize = 6;
    for index in 0..HELPERS {
        let body = if index + 1 < HELPERS {
            format!(
                "module H{index}\n\nimport H{}\n\nfn value(): Int = H{}.value() + 1\n",
                index + 1,
                index + 1
            )
        } else {
            format!("module H{index}\n\nfn value(): Int = 1\n")
        };
        scratch.write(&format!("demo/src/H{index}.aev"), &body);
    }
    scratch.write(
        "demo/src/main.aev",
        "module Main\n\nimport H0\n\nfn main(): IO Unit = do\n    println(\"{H0.value()}\")\n",
    );
    let files = HELPERS + 1;
    let output = aevia(&demo, &["check", "--all"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains(&format!(
            "checked {files} entries: {files} modules, {files} checks"
        )),
        "single-pass stats missing: {text}"
    );
    assert_eq!(
        text.matches(": ok").count(),
        files,
        "one ok line per entry: {text}"
    );
    // The chain still reports a real error with --all.
    scratch.write(
        "demo/src/H5.aev",
        "module H5\n\nfn value(): Int = 1 m + 1\n",
    );
    let output = aevia(&demo, &["check", "--all"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(stderr(&output).contains("E0401"), "{}", stderr(&output));
    assert!(stderr(&output).contains("H5.aev"), "{}", stderr(&output));
}

#[test]
fn check_accepts_several_files() {
    // A shell that expands `examples/*.aev` hands the command several paths.
    let scratch = Scratch::new("check-files");
    scratch.write("one.aev", "let a = 1\n");
    scratch.write("two.aev", "let b = 2 m\n");
    let output = aevia(scratch.path(), &["check", "one.aev", "two.aev"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        stdout(&output).contains("one.aev: ok"),
        "{}",
        stdout(&output)
    );
    assert!(
        stdout(&output).contains("two.aev: ok"),
        "{}",
        stdout(&output)
    );

    // One bad file fails the run and is named; the good one is still checked.
    scratch.write("bad.aev", "let c = 1 m + 1\n");
    let output = aevia(scratch.path(), &["check", "one.aev", "bad.aev"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(
        stdout(&output).contains("one.aev: ok"),
        "{}",
        stdout(&output)
    );
    assert!(stderr(&output).contains("bad.aev"), "{}", stderr(&output));
}

#[test]
fn check_expands_a_pattern_the_shell_did_not() {
    // PowerShell passes `*.aev` to a native executable verbatim, so the tool
    // expands the final component itself; a POSIX shell expands it first and
    // both end up checking the same files.
    let scratch = Scratch::new("check-glob");
    scratch.write("a.aev", "let a = 1\n");
    scratch.write("b.aev", "let b = 2 m\n");
    scratch.write("notes.txt", "not aevia\n");
    let output = aevia(scratch.path(), &["check", "*.aev"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stdout(&output).contains("a.aev: ok"), "{}", stdout(&output));
    assert!(stdout(&output).contains("b.aev: ok"), "{}", stdout(&output));
    assert!(
        !stdout(&output).contains("notes.txt"),
        "{}",
        stdout(&output)
    );

    // A pattern that matches nothing is kept and reported, not silently empty.
    let output = aevia(scratch.path(), &["check", "*.missing"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(stderr(&output).contains("*.missing"), "{}", stderr(&output));
}

#[test]
fn the_help_lists_the_commands_that_exist() {
    let scratch = Scratch::new("help");
    let output = aevia(scratch.path(), &["--help"]);
    assert_eq!(code(&output), 0);
    let text = stdout(&output);
    for command in [
        "run", "check", "test", "repl", "fmt", "doc", "bench", "init", "add", "update", "clean",
        "lsp", "rssn",
    ] {
        assert!(
            text.contains(command),
            "`{command}` is missing from the help"
        );
    }
    for option in [
        "--locked",
        "--release",
        "--jobs",
        "--out",
        "--doc",
        "--save-baseline",
        "--compare-baseline",
        "--threshold",
        "--example",
    ] {
        assert!(text.contains(option), "`{option}` is missing from the help");
    }
    // The doctest and bench protocols are documented where the user looks.
    assert!(text.contains("compile_fail"), "{text}");
    assert!(text.contains("median of N"), "{text}");
    assert!(text.contains("no registry"), "{text}");
}

#[test]
fn a_package_import_works_from_a_subdirectory() {
    let scratch = Scratch::new("subdir");
    assert_eq!(code(&aevia(scratch.path(), &["init", "demo"])), 0);
    let demo = scratch.path().join("demo");
    scratch.write("demo/src/Helper.aev", "fn value(): Int = 41\n");
    scratch.write(
        "demo/src/main.aev",
        "import Helper\n\nfn main(): IO Unit = println(\"{Helper.value() + 1}\")\n",
    );
    std::fs::create_dir_all(demo.join("src/nested")).expect("create");
    let output = aevia(&demo.join("src/nested"), &["run"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(stdout(&output), "42\n");
}

#[test]
fn run_passes_arguments_only_after_a_separator() {
    // §9.9: `aevia run [file] [-- args…]`. The `args` builtin answers the
    // host-set vector (rank-1 `Tensor<1, String>`).
    let scratch = Scratch::new("run-args");
    scratch.write(
        "echo.aev",
        "fn main(): IO Unit = do\n    let sep = \",\"\n    println(\"{join(sep, args())}\")\n",
    );
    let output = aevia(scratch.path(), &["run", "echo.aev", "--", "a", "b"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(stdout(&output), "a,b\n", "{}", stderr(&output));

    // `--` with nothing after it is an empty argument vector, not an error.
    let output = aevia(scratch.path(), &["run", "echo.aev", "--"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(stdout(&output), "\n");

    // An extra positional without `--` is still refused: it could be a typo
    // for the file, and the runtime cannot know which the user meant.
    let output = aevia(scratch.path(), &["run", "echo.aev", "extra"]);
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("unexpected argument"),
        "{}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains("after `--`"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn run_example_finds_the_packages_examples_directory() {
    let scratch = Scratch::new("run-example");
    assert_eq!(code(&aevia(scratch.path(), &["init", "demo"])), 0);
    let demo = scratch.path().join("demo");
    std::fs::create_dir_all(demo.join("examples")).expect("examples");
    std::fs::write(
        demo.join("examples/greet.aev"),
        "fn main(): IO Unit = println(\"hello from an example\")\n",
    )
    .expect("write example");
    std::fs::write(
        demo.join("examples/other.aev"),
        "fn main(): IO Unit = println(\"other\")\n",
    )
    .expect("write example");
    let output = aevia(&demo, &["run", "--example", "greet"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(stdout(&output), "hello from an example\n");

    // A path-qualified name is refused rather than treated as a subdirectory.
    let output = aevia(&demo, &["run", "--example", "sub/greet"]);
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("no path separators"),
        "{}",
        stderr(&output)
    );

    // A missing example lists what does exist.
    let output = aevia(&demo, &["run", "--example", "nope"]);
    assert_eq!(code(&output), 2);
    let error = stderr(&output);
    assert!(error.contains("there is no example `nope`"), "{error}");
    assert!(
        error.contains("greet") && error.contains("other"),
        "{error}"
    );

    // `--example` and a file are different programs.
    let output = aevia(&demo, &["run", "--example", "greet", "src/main.aev"]);
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("different programs"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn an_unknown_check_option_is_reported() {
    let scratch = Scratch::new("check-option");
    let output = aevia(scratch.path(), &["check", "--nonsense"]);
    assert_ne!(code(&output), 0);
    assert!(
        stderr(&output).contains("unknown option"),
        "{}",
        stderr(&output)
    );
}

// --- §7.5.5 / §9.12 doctests ----------------------------------------------

/// The absolute path of a doctest fixture in this crate.
fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("doctests")
        .join(name)
        .display()
        .to_string()
}

#[test]
fn doctests_compile_the_documented_examples() {
    let scratch = Scratch::new("doctest-good");
    let output = aevia(scratch.path(), &["test", "--doc", &fixture("good.aev")]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("doctest "), "{text}");
    assert!(text.contains("5 passed, 0 failed"), "{text}");
    // The isolation limit is stated, not silent.
    assert!(text.contains("compiles as its own module"), "{text}");
}

#[test]
fn doctest_failures_report_every_kind() {
    let scratch = Scratch::new("doctest-bad");
    let output = aevia(scratch.path(), &["test", "--doc", &fixture("bad.aev")]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("1 passed, 3 failed"), "{text}");
    // A block with no marker that does not compile is a failure.
    assert!(text.contains("did not compile"), "{text}");
    // A mismatched expected code names both sides.
    assert!(text.contains("expected `E0302`"), "{text}");
    assert!(text.contains("E0401"), "{text}");
    // A `compile_fail` block that compiles is a failure too.
    assert!(text.contains("but the snippet compiled"), "{text}");
}

#[test]
fn doctests_run_over_a_directory_and_a_filter() {
    let scratch = Scratch::new("doctest-dir");
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("doctests")
        .display()
        .to_string();
    let output = aevia(scratch.path(), &["test", "--doc", &directory]);
    assert_eq!(code(&output), 1, "a failing fixture fails the run");
    assert!(
        stdout(&output).contains("6 passed, 3 failed"),
        "{}",
        stdout(&output)
    );

    // `--filter` selects by file path: only the good fixture runs.
    let output = aevia(
        scratch.path(),
        &["test", "--doc", "--filter", "good", &directory],
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        stdout(&output).contains("5 passed, 0 failed"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn doctest_mode_says_so_when_there_is_nothing_to_check() {
    // A mode that checks no documentation must not report success.
    let scratch = Scratch::new("doctest-none");
    scratch.write("plain.aev", "fn f(): Int = 1\n");
    let output = aevia(scratch.path(), &["test", "--doc", "plain.aev"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("no ```aevia code blocks"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn compile_fail_builtin_compiles_the_snippet_in_isolation() {
    // §8.13 / §7.5.4 contract: `compile_fail : String -> Bool`. The snippet has
    // no imports and no ambient declarations (prelude only); the result is
    // true iff there is at least one error.
    let scratch = Scratch::new("compile-fail");
    let output = aevia(scratch.path(), &["test", &fixture("contract.aev")]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        stdout(&output).contains("4 passed, 0 failed"),
        "{}",
        stdout(&output)
    );
}

// --- §9.12 benchmark baselines --------------------------------------------

/// A baseline document with one workload called `loop`.
fn synthetic_baseline(
    median_ns: u64,
    profile: &str,
    workload: &str,
) -> String {
    format!(
        "{{\n  \"format\": 1,\n  \"tool\": \"aevia bench\",\n  \"version\": \"test\",\n  \
         \"profile\": \"{profile}\",\n  \"iterations\": 2,\n  \"warmup\": 1,\n  \
         \"benchmarks\": {{ \"{workload}\": {{ \"median_ns\": {median_ns}, \
         \"mean_ns\": {median_ns}, \"ci_ns\": 0 }} }}\n}}\n"
    )
}

fn bench_profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

#[test]
fn bench_baselines_gate_regressions() {
    let scratch = Scratch::new("bench-baseline");
    scratch.write(
        "bench.aev",
        "fn add(n: Int, acc: Int): Int = if n == 0 then acc else add(n - 1, acc + n)\n\n\
         @bench\nfn loop(): Int = add(20, 0)\n",
    );
    let baseline = scratch.path().join("baseline.json");

    // Save: the file records the protocol and metadata, not only timings.
    let output = aevia(
        scratch.path(),
        &[
            "bench",
            "--iterations",
            "2",
            "--warmup",
            "1",
            "--save-baseline",
            "baseline.json",
            "bench.aev",
        ],
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        stdout(&output).contains("wrote baseline"),
        "{}",
        stdout(&output)
    );
    let saved = std::fs::read_to_string(&baseline).expect("baseline file");
    for needle in [
        "\"format\": 1",
        "\"profile\"",
        "\"iterations\": 2",
        "\"warmup\": 1",
        "\"loop\"",
        "\"median_ns\"",
    ] {
        assert!(saved.contains(needle), "`{needle}` missing from {saved}");
    }

    // Compare with an enormous threshold: this checks the saved file is
    // readable and compared, not that a debug build is stable (1000× headroom
    // absorbs the interpreter's run-to-run jitter).
    let output = aevia(
        scratch.path(),
        &[
            "bench",
            "--iterations",
            "2",
            "--warmup",
            "1",
            "--compare-baseline",
            "baseline.json",
            "--threshold",
            "1000",
            "bench.aev",
        ],
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("1 benchmark(s) compared"), "{text}");
    assert!(text.contains("loop:"), "{text}");
    assert!(text.contains("0 regression(s)"), "{text}");

    // A synthetic baseline of 1 ns makes the current run a regression.
    std::fs::write(&baseline, synthetic_baseline(1, bench_profile(), "loop")).expect("writes");
    let output = aevia(
        scratch.path(),
        &[
            "bench",
            "--iterations",
            "2",
            "--warmup",
            "1",
            "--compare-baseline",
            "baseline.json",
            "--threshold",
            "0.05",
            "bench.aev",
        ],
    );
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(
        stdout(&output).contains("REGRESSION"),
        "{}",
        stdout(&output)
    );

    // A baseline ten seconds slow cannot have been regressed past.
    std::fs::write(
        &baseline,
        synthetic_baseline(10_000_000_000, bench_profile(), "loop"),
    )
    .expect("writes");
    let output = aevia(
        scratch.path(),
        &[
            "bench",
            "--iterations",
            "2",
            "--warmup",
            "1",
            "--compare-baseline",
            "baseline.json",
            "bench.aev",
        ],
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains(" ok"), "{text}");

    // A baseline from another build profile is not comparable.
    let other = if bench_profile() == "debug" {
        "release"
    } else {
        "debug"
    };
    std::fs::write(&baseline, synthetic_baseline(1, other, "loop")).expect("writes");
    let output = aevia(
        scratch.path(),
        &[
            "bench",
            "--iterations",
            "2",
            "--warmup",
            "1",
            "--compare-baseline",
            "baseline.json",
            "bench.aev",
        ],
    );
    assert_eq!(code(&output), 2, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("not comparable"),
        "{}",
        stderr(&output)
    );

    // Workloads on one side only are reported, not counted as regressions.
    std::fs::write(
        &baseline,
        synthetic_baseline(10_000_000_000, bench_profile(), "old_loop"),
    )
    .expect("writes");
    let output = aevia(
        scratch.path(),
        &[
            "bench",
            "--iterations",
            "2",
            "--warmup",
            "1",
            "--compare-baseline",
            "baseline.json",
            "bench.aev",
        ],
    );
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("no baseline entry (new benchmark)"), "{text}");
    assert!(text.contains("in the baseline but not measured"), "{text}");

    // Usage errors are refusals, not silently ignored options.
    let output = aevia(
        scratch.path(),
        &[
            "bench",
            "--compare-baseline",
            "baseline.json",
            "--save-baseline",
            "other.json",
            "bench.aev",
        ],
    );
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("different runs"),
        "{}",
        stderr(&output)
    );
    let output = aevia(
        scratch.path(),
        &["bench", "--threshold", "nope", "bench.aev"],
    );
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("not a ratio"),
        "{}",
        stderr(&output)
    );
}
