//! Local runtime declarations retain exact identity while interfaces expose
//! only visible source bindings and fully resolved public alias templates.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use std::path::Path;
use std::path::PathBuf;

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "aevia-runtime-interface-{name}-{}",
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
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(
    path: &Path,
    expected: &str,
) -> driver::Program {
    let program = driver::load(path);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked)
            .with_buffer()
            .with_mode(mode);
        let diagnostics = driver::run_modules(&mut interp, &program);
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
    program
}

#[test]
fn private_classes_and_interleaved_instances_use_their_exact_declaration_sites() {
    let scratch = Scratch::new("private-interleaved");
    let path = scratch.write("main.aev", r#"private class Hidden a where
    hidden(x: a): String
class Visible a where
    visible(x: a): String
instance Hidden for Int where
    hidden(x) = "hidden-int"
instance Visible for Bool where
    visible(x) = "visible-bool"
instance Hidden for Bool where
    hidden(x) = "hidden-bool"
instance Visible for Int where
    visible(x) = "visible-int"
let first_class_hidden = hidden
let first_class_visible = visible
fn generic(x) = hidden(x)
fn main(): IO Unit = println("{hidden(1)} {visible(true)} {generic(false)} {first_class_hidden(1)} {first_class_visible(1)}")
"#);
    let program = run(
        &path,
        "hidden-int visible-bool hidden-bool hidden-int visible-int\n",
    );
    let checked = &program.entry().unwrap().checked;
    assert_eq!(checked.local_classes.len(), 2);
    assert_eq!(checked.instance_sites.len(), 4);
    let interface = checked.interface();
    assert_eq!(interface.classes.len(), 1);
    assert!(
        interface
            .classes
            .keys()
            .all(|class| class.name == "Visible")
    );
    assert_eq!(interface.instances.len(), 2);
    assert!(
        interface
            .instances
            .iter()
            .all(|instance| instance.key.class.name == "Visible")
    );
}

#[test]
fn same_named_private_method_values_in_separate_modules_keep_their_class_keys() {
    let scratch = Scratch::new("private-method-values");
    for (module, text) in [("A", "one"), ("B", "two")] {
        scratch.write(
            &format!("{module}.aev"),
            &format!(
                r#"module {module}
private class Pretty a where
    pretty(x: a): String
instance Pretty for Int where
    pretty(x) = "{text}"
let implementation = pretty
fn label(): String = implementation(1)
"#
            ),
        );
    }
    let path = scratch.write(
        "main.aev",
        "import A\nimport B\nfn main(): IO Unit = println(\"{A.label()} {B.label()}\")\n",
    );
    run(&path, "one two\n");
}

#[test]
fn method_values_passed_as_arguments_and_captured_by_closures_retain_identity() {
    let scratch = Scratch::new("method-capture");
    let path = scratch.write(
        "main.aev",
        r#"private class Describe a where
    describe(x: a): String
instance Describe for Int where
    describe(x) = "integer"
fn apply(f, x) = f(x)
let capture = fn f => fn x => f(x)
let captured = capture(describe)
fn main(): IO Unit = println("{apply(describe, 1)} {captured(2)}")
"#,
    );
    run(&path, "integer integer\n");
}

#[test]
fn an_imported_constrained_function_is_called_as_a_function_not_an_instance_method() {
    let scratch = Scratch::new("constrained-function");
    scratch.write(
        "A.aev",
        r#"module A
class Pretty a where
    pretty(x: a): String
instance Pretty for Int where
    pretty(x) = "instance"
fn wrap(x) = "wrapped {pretty(x)}"
"#,
    );
    let path = scratch.write(
        "main.aev",
        "import A { wrap }\nfn main(): IO Unit = println(wrap(1))\n",
    );
    run(&path, "wrapped instance\n");
}

#[test]
fn public_alias_templates_resolve_private_generic_and_dimension_helpers_without_exporting_names() {
    let scratch = Scratch::new("private-alias-closure");
    scratch.write(
        "Api.aev",
        r#"module Api
private type Cell<a> = { value: a }
private type Metered<d: Dim> = Cell<Real[d]>
type Public<d: Dim> = Metered<d>
fn make(): Public<L> = { value = 2 m }
"#,
    );
    let path = scratch.write("main.aev", "import Api as A\nlet item: A.Public<L> = A.make()\nfn main(): IO Unit = println(\"{item.value}\")\n");
    let program = run(&path, "2 m\n");
    let api = program
        .modules
        .iter()
        .find(|module| module.path == ["Api"])
        .unwrap();
    let aliases = api.checked.interface().aliases;
    assert_eq!(
        aliases.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["Public"]
    );
    assert!(!api.checked.exports.contains_key("Cell"));
    assert!(
        api.checked.aliases.contains_key("Cell"),
        "runtime keeps local helpers"
    );
    let path = scratch.write(
        "private.aev",
        "import Api as A\nlet item: A.Cell<Int> = { value = 1 }\n",
    );
    let rejected = driver::load(&path);
    assert!(
        rejected
            .diagnostics
            .items
            .iter()
            .any(|diagnostic| diagnostic.code == "E0301"),
        "{:?}",
        rejected.diagnostics.items
    );
}

#[test]
fn imported_aliases_are_not_reexported_but_public_aliases_keep_their_semantic_closure() {
    let scratch = Scratch::new("transitive-alias");
    scratch.write("Base.aev", "module Base\ntype Item<a> = { value: a }\n");
    scratch.write("Mid.aev", "module Mid\nimport Base { Item }\ntype Public<a> = Item<a>\nfn make(): Public<Int> = { value = 7 }\n");
    let path = scratch.write("main.aev", "import Mid as M\nlet item: M.Public<Int> = M.make()\nfn main(): IO Unit = println(\"{item.value}\")\n");
    let program = run(&path, "7\n");
    let mid = program
        .modules
        .iter()
        .find(|module| module.path == ["Mid"])
        .unwrap();
    assert_eq!(mid.checked.interface().aliases.len(), 1);
    for (file, spelling) in [
        ("private.aev", "M.Item<Int>"),
        ("indirect.aev", "Base.Item<Int>"),
    ] {
        let path = scratch.write(
            file,
            &format!("import Mid as M\nlet item: {spelling} = {{ value = 1 }}\n"),
        );
        let rejected = driver::load(&path);
        assert!(
            rejected
                .diagnostics
                .items
                .iter()
                .any(|diagnostic| diagnostic.code == "E0301"),
            "{spelling}: {:?}",
            rejected.diagnostics.items
        );
    }
}
