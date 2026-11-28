//! A REPL reports parser roots without checker cascades from recovered ASTs.
use aevia_core::Interp;
use aevia_core::driver::ReplCompiler;
use aevia_core::driver::ReplRuntime;
use aevia_core::driver::{
    self,
};
use aevia_core::source::SourceIdentity;
use std::path::PathBuf;

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "aevia-repl-parse-stages-{name}-{}",
            std::process::id()
        ));
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
        std::fs::write(&path, source).unwrap();
        path
    }

    fn compiler(&self) -> ReplCompiler {
        ReplCompiler::new(
            &self.0,
            SourceIdentity::virtual_source(format!("test://parse-stage/{:?}", self.0)),
        )
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn malformed_entry_preserves_parser_roots_ast_and_source_without_checker_noise() {
    let directory = Scratch::new("entry");
    let mut compiler = directory.compiler();
    let source = "let first = f(1 + )\nlet second = g(, 2)\n";
    let compilation = compiler.compile(source, None);
    assert_eq!(
        compilation
            .diagnostics
            .items
            .iter()
            .filter(|diagnostic| diagnostic.code == "E0202")
            .count(),
        2,
        "{:?}",
        compilation.diagnostics.items
    );
    assert!(
        !compilation
            .diagnostics
            .items
            .iter()
            .any(|diagnostic| diagnostic.code.starts_with("E03")),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.as_ref().unwrap();
    assert_eq!(compilation.sources.file(module.file).unwrap().text, source);
    assert!(!module.decls.is_empty());
    assert!(compilation.checked.exports.is_empty());
}

#[test]
fn malformed_dependencies_block_their_importers_but_preserve_independent_type_errors() {
    let directory = Scratch::new("dependencies");
    directory.write("Bad.aev", "module Bad\nlet broken = f(1 + )\n");
    directory.write("Other.aev", "module Other\nfn wrong(): Int = \"bad\"\n");
    let mut compiler = directory.compiler();
    let compilation = compiler.compile(
        "import Bad\nimport Other\nlet dependent = Bad.broken\n",
        None,
    );
    assert!(
        compilation
            .diagnostics
            .items
            .iter()
            .any(|diagnostic| diagnostic.code == "E0202")
    );
    let type_errors: Vec<_> = compilation
        .diagnostics
        .items
        .iter()
        .filter(|diagnostic| diagnostic.code.starts_with("E03"))
        .collect();
    assert_eq!(type_errors.len(), 1, "{:?}", compilation.diagnostics.items);
    assert_eq!(type_errors[0].code, "E0302");
    assert!(
        compilation
            .sources
            .file(type_errors[0].span.file)
            .unwrap()
            .name
            .ends_with("Other.aev")
    );
    assert_eq!(compilation.program.modules.len(), 3);
    assert!(
        compilation.checked.exports.is_empty(),
        "dependent entry was type-checked"
    );
}

#[test]
fn lexical_dependency_failures_do_not_create_missing_export_cascades() {
    let directory = Scratch::new("lexical");
    directory.write("Bad.aev", "module Bad\nlet broken = \u{0}\n");
    let mut compiler = directory.compiler();
    let compilation = compiler.compile("import Bad { broken }\nlet dependent = broken\n", None);
    assert!(
        compilation
            .diagnostics
            .items
            .iter()
            .any(|diagnostic| diagnostic.code.starts_with("E01")),
        "{:?}",
        compilation.diagnostics.items
    );
    assert!(
        !compilation
            .diagnostics
            .items
            .iter()
            .any(|diagnostic| diagnostic.code.starts_with("E03")),
        "{:?}",
        compilation.diagnostics.items
    );
    assert_eq!(compilation.program.modules.len(), 2);
}

#[test]
fn failed_parse_is_not_cached_as_an_interface_and_a_corrected_import_can_run() {
    let directory = Scratch::new("retry");
    directory.write("Bad.aev", "module Bad\nlet broken = f(1 + )\n");
    let mut compiler = directory.compiler();
    let mut failed_file = None;
    for _ in 0..2 {
        let failed = compiler.compile("import Bad\n", None);
        let bad = failed
            .program
            .modules
            .iter()
            .find(|module| module.path == ["Bad"])
            .unwrap();
        failed_file = Some(bad.file);
        assert!(bad.checked.exports.is_empty());
        assert!(
            failed
                .diagnostics
                .items
                .iter()
                .all(|diagnostic| !diagnostic.code.starts_with("E03"))
        );
    }
    directory.write("Bad.aev", "module Bad\nlet answer = 42\n");
    let fixed = compiler.compile("import Bad\nlet result = Bad.answer\n", None);
    assert!(
        !fixed.diagnostics.has_errors(),
        "{:?}",
        fixed.diagnostics.items
    );
    assert_eq!(
        fixed
            .program
            .modules
            .iter()
            .find(|module| module.path == ["Bad"])
            .unwrap()
            .file,
        failed_file.unwrap()
    );
    let mut runtime = ReplRuntime::default();
    let mut interp = Interp::new(&Default::default());
    let diagnostics = runtime.synchronize(&mut interp, &fixed, 0, usize::MAX);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    assert_eq!(
        aevia_core::show(&interp.eval_toplevel("result").unwrap()),
        "42"
    );
}

#[test]
fn ordinary_loader_keeps_recovered_names_available_for_editor_analysis() {
    let directory = Scratch::new("editor");
    let file = directory.write("Editor.aev", "let valid = 42\nlet broken = f(1 + )\n");
    let program = driver::load(&file);
    assert!(program.diagnostics.has_errors());
    assert!(
        program
            .entry()
            .unwrap()
            .checked
            .exports
            .contains_key("valid")
    );
    assert!(
        program
            .entry()
            .unwrap()
            .checked
            .exports
            .contains_key("broken")
    );
}
