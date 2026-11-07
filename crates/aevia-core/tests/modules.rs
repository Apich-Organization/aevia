//! Multi-file programs: imports, aliases, name lists and cycles.

use aevia_core::driver;
use std::path::Path;
use std::path::PathBuf;

/// A scratch directory that removes itself.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("aevia-modules-{name}-{}", std::process::id()));
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
        std::fs::write(&path, source).expect("write module");
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

fn run(entry: &Path) -> String {
    let program = driver::load(entry);
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
fn a_declared_module_name_must_match_its_import_path() {
    let scratch = Scratch::new("e0601");
    // The file is imported as `Widgets` but declares `Gadgets`.
    scratch.write("Widgets.aev", "module Gadgets\n\nlet n = 1\n");
    let entry = scratch.write(
        "main.aev",
        "import Widgets\n\nfn main(): IO Unit = println(\"{Widgets.n}\")\n",
    );
    let program = driver::load(&entry);
    assert!(
        program
            .diagnostics
            .items
            .iter()
            .any(|item| item.code == "E0601"),
        "a name/path mismatch is E0601, found {:?}",
        program
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );

    // A matching declaration loads clean.
    let scratch = Scratch::new("e0601-ok");
    scratch.write("Widgets.aev", "module Widgets\n\nlet n = 1\n");
    let entry = scratch.write(
        "main.aev",
        "import Widgets\n\nfn main(): IO Unit = println(\"{Widgets.n}\")\n",
    );
    let program = driver::load(&entry);
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
}

#[test]
fn a_qualified_import_resolves() {
    let scratch = Scratch::new("qualified");
    scratch.write(
        "Math/Geometry.aev",
        "module Math.Geometry\n\nfn square(s: Real[L]): Real[L^2] = s^2\n",
    );
    let entry = scratch.write(
        "main.aev",
        "import Math.Geometry\n\nfn main(): IO Unit = do\n    println(\"{Math.Geometry.square(3.0 m)}\")\n",
    );
    assert_eq!(run(&entry), "9 m^2\n");
}

#[test]
fn an_alias_import_resolves() {
    let scratch = Scratch::new("alias");
    scratch.write(
        "Math/Geometry.aev",
        "module Math.Geometry\n\nlet twice = 2.0\n",
    );
    let entry = scratch.write(
        "main.aev",
        "import Math.Geometry as G\n\nfn main(): IO Unit = println(\"{G.twice}\")\n",
    );
    assert_eq!(run(&entry), "2.0\n");
}

#[test]
fn a_name_list_imports_only_those_names() {
    let scratch = Scratch::new("names");
    scratch.write(
        "Math/Geometry.aev",
        "module Math.Geometry\n\nlet two = 2.0\nlet three = 3.0\n",
    );
    let entry = scratch.write(
        "main.aev",
        "import Math.Geometry { two }\n\nfn main(): IO Unit = println(\"{two}\")\n",
    );
    assert_eq!(run(&entry), "2.0\n");
}

#[test]
fn modules_are_checked_before_their_importers() {
    let scratch = Scratch::new("order");
    scratch.write(
        "A/Base.aev",
        "module A.Base\n\nfn double(x: Int): Int = 2 * x\n",
    );
    scratch.write(
        "A/Mid.aev",
        "module A.Mid\nimport A.Base\n\nfn quad(x: Int): Int = A.Base.double(A.Base.double(x))\n",
    );
    let entry = scratch.write(
        "main.aev",
        "import A.Mid\n\nfn main(): IO Unit = println(\"{A.Mid.quad(3)}\")\n",
    );
    assert_eq!(run(&entry), "12\n");
}

#[test]
fn a_missing_module_is_reported() {
    let scratch = Scratch::new("missing");
    let entry = scratch.write("main.aev", "import No.Such\n\nfn main(): IO Unit = ()\n");
    let program = driver::load(&entry);
    assert!(program.diagnostics.has_errors());
    let codes: Vec<&str> = program
        .diagnostics
        .items
        .iter()
        .map(|item| item.code.as_str())
        .collect();
    assert!(codes.contains(&"E0607"), "unexpected codes: {codes:?}");
}

#[test]
fn an_import_cycle_is_reported() {
    let scratch = Scratch::new("cycle");
    scratch.write("A/One.aev", "module A.One\nimport A.Two\n\nlet one = 1\n");
    scratch.write("A/Two.aev", "module A.Two\nimport A.One\n\nlet two = 2\n");
    let entry = scratch.write("main.aev", "import A.One\n\nfn main(): IO Unit = ()\n");
    let program = driver::load(&entry);
    let codes: Vec<&str> = program
        .diagnostics
        .items
        .iter()
        .map(|item| item.code.as_str())
        .collect();
    assert!(codes.contains(&"E0602"), "unexpected codes: {codes:?}");
}

#[test]
fn two_modules_may_share_a_private_name() {
    let scratch = Scratch::new("shadow");
    scratch.write("A/One.aev", "module A.One\n\nfn helper(): Int = 1\n");
    scratch.write("A/Two.aev", "module A.Two\n\nfn helper(): Int = 2\n");
    let entry = scratch.write(
        "main.aev",
        "import A.One\nimport A.Two\n\nfn main(): IO Unit = do\n    println(\"{A.One.helper()}\")\n    println(\"{A.Two.helper()}\")\n",
    );
    assert_eq!(run(&entry), "1\n2\n");
}

#[test]
fn the_entry_module_keeps_main_unqualified() {
    let scratch = Scratch::new("entry");
    let entry = scratch.write("main.aev", "fn main(): IO Unit = println(\"hi\")\n");
    assert_eq!(run(&entry), "hi\n");
    let _ = scratch.path();
}

// --- regressions found by the second review ---------------------------------

#[test]
fn private_declarations_are_not_exported() {
    let scratch = Scratch::new("private");
    scratch.write(
        "A/Geo.aev",
        "module A.Geo\n\nprivate fn secret(x: Int): Int = x * 100\nfn open_(x: Int): Int = x * 2\n",
    );
    let entry = scratch.write(
        "main.aev",
        "import A.Geo\n\nfn main(): IO Unit = println(\"{A.Geo.secret(2)}\")\n",
    );
    let program = driver::load(&entry);
    let messages: Vec<String> = program
        .diagnostics
        .items
        .iter()
        .map(|item| item.message.clone())
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("no exported name")),
        "unexpected diagnostics: {messages:?}"
    );
}

#[test]
fn a_top_level_let_may_refer_to_a_later_one() {
    let scratch = Scratch::new("forward");
    let entry = scratch.write(
        "main.aev",
        "let a = b + 1\nlet b = 41\n\nfn main(): IO Unit = println(\"{a}\")\n",
    );
    assert_eq!(run(&entry), "42\n");
}

#[test]
fn opening_two_modules_with_different_names_is_not_a_clash() {
    let scratch = Scratch::new("two-opens");
    scratch.write("G/One.aev", "module G.One\n\nlet one = 1\n");
    scratch.write("G/Two.aev", "module G.Two\n\nlet two = 2\n");
    let entry = scratch.write(
        "main.aev",
        "open G.One\nopen G.Two\n\nfn main(): IO Unit = println(\"{one + two}\")\n",
    );
    assert_eq!(run(&entry), "3\n");
}

#[test]
fn opening_the_same_module_twice_is_idempotent() {
    let scratch = Scratch::new("idempotent");
    scratch.write("G/One.aev", "module G.One\n\nlet one = 1\n");
    let entry = scratch.write(
        "main.aev",
        "open G.One\nopen G.One\n\nfn main(): IO Unit = println(\"{one}\")\n",
    );
    assert_eq!(run(&entry), "1\n");
}

#[test]
fn a_type_and_its_constructor_may_share_a_name() {
    let scratch = Scratch::new("typector");
    scratch.write(
        "A/T.aev",
        "module A.T\n\ntype P2 = | P2(Float, Float)\n\nfn mk(a: Float, b: Float): P2 = P2(a, b)\n",
    );
    let entry = scratch.write(
        "main.aev",
        "import A.T\n\nfn mk2(a: Float, b: Float): A.T.P2 = A.T.mk(a, b)\n\nfn sum2(p: A.T.P2): Float =\n    match p with\n    | P2(a, b) -> a + b\n\nfn main(): IO Unit = println(\"{sum2(mk2(1.0, 2.0))}\")\n",
    );
    assert_eq!(run(&entry), "3.0\n");
}

#[test]
fn every_module_contributes_its_pinned_keys() {
    // A `Rssn.<op>` call in an imported module is pinned against *that* file's
    // span, so the evaluator needs every module's table, not just the entry
    // module's (PLAN D1). The program is not run: `aevia-types` still types a
    // pinned call as the operator itself (see the report), which makes an
    // annotated call site unprovable.
    let scratch = Scratch::new("rssn-call");
    scratch.write(
        "A/Num.aev",
        "module A.Num

let root = Rssn.sqrt(9)
",
    );
    let entry = scratch.write(
        "main.aev",
        "import A.Num

fn main(): IO Unit = println(\"x\")
",
    );
    let program = driver::load(&entry);
    let entry_module = program.entry().expect("entry module");
    let mut interp = aevia_core::Interp::new(&entry_module.checked);
    let imported = program
        .modules
        .iter()
        .find(|module| module.path != entry_module.path)
        .expect("the imported module");
    assert!(
        !imported.checked.rssn_calls.is_empty(),
        "the imported module pinned no key"
    );
    driver::run_modules(&mut interp, &program);
    for call in &imported.checked.rssn_calls {
        assert!(
            interp.rssn_calls.contains_key(&call.span),
            "the imported module's pinned key was not registered"
        );
    }
}
