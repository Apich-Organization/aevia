//! The command line, one test per command of §9.9 that this tool provides.
//!
//! Each test drives the real binary in a scratch directory, because what is
//! being tested is the command — its exit code, the files it writes and the
//! files it refuses to write.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

    fn write(&self, name: &str, source: &str) -> PathBuf {
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

fn aevia(dir: &Path, args: &[&str]) -> Output {
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
    assert!(stdout(&output).contains("hello from demo"), "{}", stdout(&output));
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
    assert!(stderr(&output).contains("already exists"), "{}", stderr(&output));
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
    scratch.write(
        "dep/src/Convert.aev",
        "fn mm(x: Int): Int = x * 1000\n",
    );
    let output = aevia(&demo, &["add", "../dep"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stdout(&output).contains("added `Dep`"), "{}", stdout(&output));

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
    assert!(stdout(&output).contains("up to date"), "{}", stdout(&output));
    assert_eq!(std::fs::read_to_string(demo.join("aevia.lock")).expect("reads"), first);

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
    assert!(stdout(&output).contains("would reformat a.aev"), "{}", stdout(&output));
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
    assert!(table.contains("mean"), "{table}");
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
    assert!(loop_line.contains("100"), "{loop_line}");

    // `--filter` narrows the run, and a run with no failures exits 0.
    let output = aevia(scratch.path(), &["bench", "--filter", "loop", "bench.aev"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(!stdout(&output).contains("FAILED"), "{}", stdout(&output));
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
fn the_commands_that_do_not_exist_are_not_offered() {
    // `clean` and `lsp` are deliberately absent: there is no build cache to
    // remove and no `aevia-lsp` crate, so accepting them would be a lie.
    let scratch = Scratch::new("absent");
    for command in ["clean", "lsp"] {
        let output = aevia(scratch.path(), &[command]);
        assert_eq!(code(&output), 2, "`{command}` was accepted");
        assert!(
            stderr(&output).contains("unknown command"),
            "{}",
            stderr(&output)
        );
    }
}

#[test]
fn the_help_lists_the_commands_that_exist() {
    let scratch = Scratch::new("help");
    let output = aevia(scratch.path(), &["--help"]);
    assert_eq!(code(&output), 0);
    let text = stdout(&output);
    for command in ["run", "check", "test", "repl", "fmt", "doc", "bench", "init", "add", "update"] {
        assert!(text.contains(command), "`{command}` is missing from the help");
    }
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
fn an_argument_the_runtime_cannot_use_is_reported() {
    // `run file -- args` is in §9.9 but this runtime calls a zero-argument
    // `main`, so the argument has nowhere to go. Dropping it silently would
    // let a user believe their program received it.
    let scratch = Scratch::new("args");
    scratch.write("a.aev", "fn main(): IO Unit = println(\"hi\")\n");
    let output = aevia(scratch.path(), &["run", "a.aev", "--", "1"]);
    assert_ne!(code(&output), 0);
    assert!(
        stderr(&output).contains("unexpected argument"),
        "{}",
        stderr(&output)
    );
    // `check --all` is not implemented either, and says so.
    let output = aevia(scratch.path(), &["check", "--all"]);
    assert_ne!(code(&output), 0);
    assert!(
        stderr(&output).contains("unknown option"),
        "{}",
        stderr(&output)
    );
}
