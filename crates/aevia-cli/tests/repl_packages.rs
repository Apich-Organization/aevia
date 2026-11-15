//! Package imports use the same resolver and retain the same session bindings.
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("aevia-repl-package-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(
        &self,
        path: &str,
        source: &str,
    ) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn run(
    directory: &Path,
    source: &str,
    optimized: bool,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_aevia"));
    command
        .args(["--color", "never"])
        .current_dir(directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if optimized {
        command.arg("--release");
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}
fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}
fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n")
}

#[test]
fn repl_imports_package_and_stdlib_with_aliases_open_and_selective_names() {
    let dir = Scratch::new("aliases");
    dir.write(
        "aevia.toml",
        "[package]\nname = \"repl_demo\"\nversion = \"0.1.0\"\nentry = \"src/main.aev\"\n",
    );
    dir.write(
        "src/Geometry.aev",
        "fn twice(x: Int): Int = x * 2\nfn triple(x: Int): Int = x * 3\n",
    );
    for optimized in [false, true] {
        let output = run(
            &dir.0,
            "import Geometry as G\nG.twice(21)\nimport Geometry { triple }\ntriple(14)\nopen Geometry\ntwice(21)\nimport Math as M\nM.sqrt(4)\n:type M.sqrt\n:imports\n",
            optimized,
        );
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(
            stdout(&output).matches("Int = 42").count(),
            3,
            "{}",
            stdout(&output)
        );
        assert!(stdout(&output).contains("Geometry as G"));
        assert!(stdout(&output).contains("Math as M"));
    }
}

#[test]
fn repl_resolves_path_dependencies_from_package_subdirectories() {
    let dir = Scratch::new("dependency");
    dir.write("app/aevia.toml", "[package]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.aev\"\n[dependencies]\nTools = { path = \"../tools\" }\n");
    dir.write(
        "app/src/main.aev",
        "fn main(): IO Unit = println(\"app\")\n",
    );
    dir.write(
        "tools/aevia.toml",
        "[package]\nname = \"tools\"\nversion = \"0.1.0\"\nentry = \"src/main.aev\"\n",
    );
    dir.write("tools/src/Numbers.aev", "fn answer(): Int = 42\n");
    let output = run(
        &dir.0.join("app/src"),
        "import Tools.Numbers as N\nN.answer()\n",
        false,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Int = 42"));
}

#[test]
fn load_uses_foreign_files_directory_then_keeps_its_imports_at_prompt() {
    let dir = Scratch::new("load-context");
    dir.write("other/Local.aev", "fn answer(): Int = 42\n");
    dir.write(
        "other/main.aev",
        "import Local as L\nfn main(): IO Unit = println(\"loaded={L.answer()}\")\n",
    );
    dir.write("Local.aev", "fn answer(): Int = 99\n");
    let source = format!(
        "let keep = 7\n:type keep\n:load \"{}\"\nL.answer() + keep\n:imports\n",
        dir.0.join("other/main.aev").display()
    );
    let output = run(&dir.0, &source, false);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("loaded=42"), "{}", stdout(&output));
    assert!(stdout(&output).contains("Int = 49"), "{}", stdout(&output));
}

#[test]
fn failed_import_preserves_bindings_and_does_not_poison_next_import() {
    let dir = Scratch::new("failed");
    let output = run(
        &dir.0,
        "let keep = 40\n:type keep\nimport MissingModule\n:imports\nimport Math as M\nM.sqrt(4) + keep\n",
        false,
    );
    assert!(!output.status.success());
    assert!(stderr(&output).contains("MissingModule"));
    assert!(
        stdout(&output).contains("= 42"),
        "stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
}

#[test]
fn type_and_calls_respect_imported_functions_that_shadow_builtins() {
    let dir = Scratch::new("type-shadowed-builtin");
    dir.write("Tools.aev", "fn sqrt(text: String): String = text\nfn check<a>(value: a): a = value\nfn reshape<a>(value: a): a = value\n");
    for directive in ["import Tools { sqrt, check, reshape }", "open Tools"] {
        for optimized in [false, true] {
            let output = run(
                &dir.0,
                &format!(
                    "{directive}\n:type sqrt\nsqrt(\"working\")\ncheck<Int>(42)\nreshape<String>(\"shape\")\n"
                ),
                optimized,
            );
            assert!(output.status.success(), "{}", stderr(&output));
            assert!(
                stdout(&output).contains("String -> String"),
                "{}",
                stdout(&output)
            );
            assert!(
                !stdout(&output).contains("sqrt : forall"),
                "{}",
                stdout(&output)
            );
            assert!(
                stdout(&output).contains("String = working"),
                "{}",
                stdout(&output)
            );
            assert!(stdout(&output).contains("Int = 42"), "{}", stdout(&output));
            assert!(
                stdout(&output).contains("String = shape"),
                "{}",
                stdout(&output)
            );
        }
    }
}
