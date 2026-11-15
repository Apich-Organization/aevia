//! Package imports in a persistent virtual REPL retain one source/runtime instance.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver::ReplCompilation;
use aevia_core::driver::ReplCompiler;
use aevia_core::driver::ReplRuntime;
use aevia_core::source::SourceIdentity;
use std::path::Path;
use std::path::PathBuf;

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("aevia-repl-imports-{name}-{}", std::process::id()));
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

struct Session {
    compiler: ReplCompiler,
    runtime: ReplRuntime,
    interp: Interp,
    loaded: usize,
}
impl Session {
    fn new(
        directory: &Path,
        mode: Mode,
    ) -> Self {
        Self {
            compiler: ReplCompiler::new(
                directory,
                SourceIdentity::virtual_source(format!(
                    "test-repl://{}/{directory:?}",
                    std::process::id()
                )),
            ),
            runtime: ReplRuntime::default(),
            interp: Interp::new(&Default::default())
                .with_mode(mode)
                .with_buffer(),
            loaded: 0,
        }
    }

    fn submit(
        &mut self,
        source: &str,
        origin: Option<&Path>,
    ) -> ReplCompilation {
        let compilation = self.compiler.compile(source, origin);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{}",
            compilation.diagnostics.render(&compilation.sources)
        );
        let count = compilation.module.as_ref().unwrap().decls.len();
        let diagnostics =
            self.runtime
                .synchronize(&mut self.interp, &compilation, self.loaded, count);
        assert!(
            !diagnostics.has_errors(),
            "{}",
            diagnostics.render(&compilation.sources)
        );
        self.loaded = count;
        self.compiler.commit(&compilation);
        compilation
    }

    fn value(
        &mut self,
        name: &str,
    ) -> String {
        aevia_core::show(&self.interp.eval_toplevel(name).unwrap())
    }
}

#[test]
fn aliases_open_and_selective_imports_use_real_package_and_stdlib_resolution() {
    let scratch = Scratch::new("aliases");
    scratch.write(
        "aevia.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    scratch.write(
        "src/Tools.aev",
        "module Tools\nfn twice(n: Int): Int = n * 2\nfn triple(n: Int): Int = n * 3\n",
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut session = Session::new(&scratch.0, mode);
        session.submit("import Tools as T\nimport Tools { twice }\nopen Tools\nimport Math\nlet result = T.twice(2) + twice(3) + triple(4)\nlet root = Math.sqrt(4.0)\n", None);
        assert_eq!(session.value("result"), "22");
        assert!(
            session
                .compiler
                .imported_names()
                .iter()
                .any(|name| name == "T.twice")
        );
        assert!(
            session
                .compiler
                .imports()
                .iter()
                .any(|name| name.contains("Tools"))
        );
    }
}

#[test]
fn dependencies_initialize_once_and_keep_frozen_sources_stable_ids_and_nominal_types() {
    let scratch = Scratch::new("snapshot");
    scratch.write("Helper.aev", "module Helper\nlet ticket = symbol(\"dependency\")\nfn twice(n: Int): Int = n * 2\ntype Box = | Box(Int)\n");
    let mut session = Session::new(&scratch.0, Mode::Ir);
    let first_source =
        "import Helper as H\nlet token = H.ticket\nlet first = H.twice(2)\nlet box = H.Box(1)\n";
    let first = session.submit(first_source, None);
    let helper = first
        .program
        .modules
        .iter()
        .find(|module| module.path == ["Helper"])
        .unwrap();
    let file = helper.file;
    let adt = helper.checked.constructors["Box"].0;
    scratch.write(
        "Helper.aev",
        "module Helper\nfn twice(n: Int): Int = 999\ntype Box = | Box(String)\n",
    );
    let second = session.submit(
        &format!("{first_source}import Math\nlet second = H.twice(3)\nlet second_box = H.Box(2)\n"),
        None,
    );
    let helper = second
        .program
        .modules
        .iter()
        .find(|module| module.path == ["Helper"])
        .unwrap();
    assert_eq!(helper.file, file);
    assert_eq!(helper.checked.constructors["Box"].0, adt);
    assert_eq!(
        first.module.as_ref().unwrap().file,
        second.module.as_ref().unwrap().file
    );
    assert_eq!(session.value("second"), "6");
    assert_eq!(session.interp.symbol_counts["dependency"], 1);
}

#[test]
fn loaded_fragments_keep_their_own_resolution_context_after_returning_to_the_prompt() {
    let scratch = Scratch::new("load-context");
    scratch.write("Local.aev", "module Local\nfn number(): Int = 1\n");
    let loaded = scratch.write("elsewhere/program.aev", "import Local as Foreign\n");
    scratch.write(
        "elsewhere/Local.aev",
        "module Local\nfn number(): Int = 42\n",
    );
    let mut session = Session::new(&scratch.0, Mode::Ir);
    let first = "import Local as Own\nlet own = Own.number()\n";
    session.submit(first, None);
    let second = format!("{first}import Local as Foreign\nlet foreign = Foreign.number()\n");
    session.submit(&second, Some(&loaded));
    session.submit(
        &format!("{second}let later = Own.number() + Foreign.number()\n"),
        None,
    );
    assert_eq!(session.value("own"), "1");
    assert_eq!(session.value("foreign"), "42");
    assert_eq!(session.value("later"), "43");
}

#[test]
fn failed_dependency_initialization_rolls_back_and_allows_a_corrected_source() {
    let scratch = Scratch::new("rollback");
    scratch.write("Broken.aev", "module Broken\nlet ticket = symbol(\"failed_dependency\")\nfn divide(n: Int): Real = 1 / n\nlet value = divide(0)\n");
    let mut session = Session::new(&scratch.0, Mode::Ir);
    let compilation = session.compiler.compile("import Broken\n", None);
    assert!(!compilation.diagnostics.has_errors());
    let diagnostics = session
        .runtime
        .synchronize(&mut session.interp, &compilation, 0, usize::MAX);
    assert!(diagnostics.has_errors());
    assert!(
        !session
            .interp
            .symbol_counts
            .contains_key("failed_dependency")
    );
    scratch.write(
        "Broken.aev",
        "module Broken\nlet ticket = symbol(\"failed_dependency\")\nlet value = 42\n",
    );
    session.submit("import Broken\nlet answer = Broken.value\n", None);
    assert_eq!(session.value("answer"), "42");
    assert_eq!(session.interp.symbol_counts["failed_dependency"], 1);
}

#[test]
fn path_dependencies_resolve_their_own_transitive_package_imports() {
    let scratch = Scratch::new("path-dependencies");
    scratch.write(
        "base/aevia.toml",
        "[package]\nname = \"base\"\nversion = \"0.1.0\"\n",
    );
    scratch.write("base/src/Numbers.aev", "fn two(): Int = 2\n");
    scratch.write("mid/aevia.toml", "[package]\nname = \"mid\"\nversion = \"0.1.0\"\n[dependencies]\nBase = { path = \"../base\" }\n");
    scratch.write(
        "mid/src/Use.aev",
        "import Base.Numbers\nfn four(): Int = Base.Numbers.two() * 2\n",
    );
    scratch.write("app/aevia.toml", "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\nMid = { path = \"../mid\" }\n");
    let mut session = Session::new(&scratch.0.join("app"), Mode::Ir);
    session.submit("import Mid.Use as M\nlet result = M.four()\n", None);
    assert_eq!(session.value("result"), "4");
}

#[test]
fn cancelled_initialization_does_not_commit_import_state() {
    let scratch = Scratch::new("cancelled");
    scratch.write(
        "Values.aev",
        "module Values\nlet ticket = symbol(\"cancelled_import\")\nlet answer = 42\n",
    );
    let mut session = Session::new(&scratch.0, Mode::Ir);
    let token = aevia_core::CancellationToken::new();
    session.interp.set_cancellation_token(token.clone());
    let compilation = session
        .compiler
        .compile("import Values\nlet answer = Values.answer\n", None);
    assert!(!compilation.diagnostics.has_errors());
    token.cancel();
    let diagnostics = session
        .runtime
        .synchronize(&mut session.interp, &compilation, 0, usize::MAX);
    assert!(
        diagnostics
            .items
            .iter()
            .any(|diagnostic| diagnostic.code == "E0812")
    );
    assert!(session.interp.symbol_counts.is_empty());
    token.reset();
    session.submit("import Values\nlet answer = Values.answer\n", None);
    assert_eq!(session.value("answer"), "42");
    assert_eq!(session.interp.symbol_counts["cancelled_import"], 1);
}

#[test]
#[cfg(unix)]
fn retargeted_import_symlinks_require_an_explicit_reset() {
    let scratch = Scratch::new("retargeted");
    let first = scratch.write("first.aev", "module Helper\nlet value = 1\n");
    let second = scratch.write("second.aev", "module Helper\nlet value = \"changed\"\n");
    let link = scratch.0.join("Helper.aev");
    std::os::unix::fs::symlink(first, &link).unwrap();
    let mut session = Session::new(&scratch.0, Mode::Ir);
    let source = "import Helper\nlet old = Helper.value\n";
    session.submit(source, None);
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(second, link).unwrap();
    let compilation = session
        .compiler
        .compile(&format!("{source}let latest = Helper.value\n"), None);
    assert!(compilation.diagnostics.items.iter().any(|diagnostic| {
        diagnostic.code == "E0607" && diagnostic.helps.iter().any(|help| help.contains(":reset"))
    }));
    assert_eq!(session.value("old"), "1");
}

#[test]
fn imported_bindings_can_be_overwritten_using_their_previous_value() {
    let scratch = Scratch::new("import-overwrite");
    scratch.write("Tools.aev", "module Tools\nfn twice(n: Int): Int = n * 2\n");
    for mode in [Mode::Ast, Mode::Ir] {
        let mut session = Session::new(&scratch.0, mode);
        let source = "import Tools { twice }\nlet twice = twice(2)\nopen Math\nprivate let sqrt = sqrt(4.0)\n";
        let compilation = session.submit(source, None);
        assert_eq!(session.value("twice"), "4");
        assert!(!compilation.checked.callable_exports.contains_key("twice"));
        assert!(!compilation.checked.function_headers.contains_key("twice"));
        assert!(!compilation.checked.exports.contains_key("sqrt"));
        session.submit(&format!("{source}let again = twice + 1\n"), None);
        assert_eq!(session.value("again"), "5");
    }
}

#[test]
fn nested_import_aliases_are_lexical_even_when_the_alias_is_also_a_unit() {
    let scratch = Scratch::new("nested-unit-alias");
    scratch.write("Base.aev", "module Base\nfn convert(n: Int): Int = n * 2\n");
    scratch.write("Facade.aev", "module Facade\nimport Base as B\nlet exported = B.convert\nfn apply(n: Int): Int = B.convert(n)\n");
    for (mode, optimized) in [(Mode::Ast, false), (Mode::Ir, false), (Mode::Ir, true)] {
        let mut session = Session::new(&scratch.0, mode);
        session.interp.optimize_ir = optimized;
        session.submit("import Facade as F\nlet answer = F.exported(21)\nlet called = F.apply(3)\nlet byte = 1 B\n", None);
        assert_eq!(session.value("answer"), "42");
        assert_eq!(session.value("called"), "6");
        // B.convert belongs only to Facade's import environment. The bare B
        // remains a byte unit in the entry, and the alias never leaks out.
        assert!(!session.interp.globals.contains_key("B.convert"));
        let aevia_core::value::Value::Num(byte) = session.interp.eval_toplevel("byte").unwrap()
        else {
            panic!("quantity")
        };
        assert!(byte.dim.is_one());
    }
}
