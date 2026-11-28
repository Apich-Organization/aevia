//! Instance identity and pinned dictionary dispatch (§3.8.1/§3.8.4, round-2
//! decisions 1a/1c).
//!
//! These are the regressions for the dominant silent bug: an instance body used
//! to be bound as `Class@?::method` whenever its head was not a nominal type, so
//! `Pretty@Int` and `Pretty@String` overwrote one global and one of the two
//! bodies ran for both arguments. Every assertion is about *which body ran* —
//! distinguished by the value it returns — and every single-file program is run
//! through both evaluators, which must agree.

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::driver;
use aevia_core::value::Value;
use std::path::Path;
use std::path::PathBuf;

const PRETTY: &str = "\
class Pretty a where
    pretty(x: a): String
";

/// A scratch directory that removes itself.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("aevia-instances-{name}-{}", std::process::id()));
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
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn compile_ok(source: &str) -> (aevia_syntax::Module, aevia_types::CheckedModule) {
    let compilation = compile("instances.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "compile errors: {:?}",
        compilation
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    (
        compilation.module.expect("a parsed module"),
        compilation.checked,
    )
}

/// Runs one program on one evaluator and returns the top-level values asked for.
fn values_with_mode(
    source: &str,
    mode: Mode,
    names: &[&str],
) -> Vec<String> {
    let (module, checked) = compile_ok(source);
    let mut interpreter = Interp::new(&checked).with_buffer().with_mode(mode);
    let diagnostics = interpreter.load(&module);
    assert!(
        !diagnostics.has_errors(),
        "load errors: {:?}",
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    names
        .iter()
        .map(|name| {
            let value = interpreter.eval_toplevel(name).expect("the name is bound");
            match value {
                | Value::Str(text) => text.to_string(),
                | Value::Num(number) => aevia_core::value::repr(&Value::Num(number)),
                | other => aevia_core::value::repr(&other),
            }
        })
        .collect()
}

/// Runs the program on both evaluators, asserts they agree, and returns the
/// values (from the IR path, after agreement).
fn values(
    source: &str,
    names: &[&str],
) -> Vec<String> {
    let ast = values_with_mode(source, Mode::Ast, names);
    let ir = values_with_mode(source, Mode::Ir, names);
    assert_eq!(ast, ir, "the two evaluators disagree");
    ir
}

/// Runs a multi-file program on both evaluators and returns the captured output.
fn run_file_both_modes(entry: &Path) -> (String, String) {
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
    let checked = &program.entry().expect("entry module").checked;
    let mut outputs = Vec::new();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interpreter = Interp::new(checked).with_buffer().with_mode(mode);
        let diagnostics = driver::run_modules(&mut interpreter, &program);
        assert!(
            !diagnostics.has_errors(),
            "load errors: {:?}",
            diagnostics
                .items
                .iter()
                .map(|item| (&item.code, &item.message))
                .collect::<Vec<_>>()
        );
        interpreter.run_main().expect("main runs");
        outputs.push(interpreter.captured());
    }
    assert_eq!(outputs[0], outputs[1], "the two evaluators disagree");
    (outputs[0].clone(), outputs[1].clone())
}

// ---------------------------------------------------------------------------
// The dominant bug: two primitive-head instances
// ---------------------------------------------------------------------------

/// `instance Pretty for Int` and `instance Pretty for String` are two bodies,
/// and each call site runs the one the checker proved. Before the symbolic key,
/// both were bound under `Pretty@?::pretty` and the second declaration won.
#[test]
fn two_primitive_head_instances_keep_their_own_bodies() {
    let source = format!(
        "{PRETTY}
instance Pretty for Int where
    pretty(x) = \"int-body\"
instance Pretty for String where
    pretty(s) = \"string-body\"
let direct_int = pretty(1)
let direct_string = pretty(\"s\")
let describe = fn x => pretty(x)
let poly_int = describe(1)
let poly_string = describe(\"s\")
"
    );
    let values = values(
        &source,
        &["direct_int", "direct_string", "poly_int", "poly_string"],
    );
    assert_eq!(
        values,
        vec!["int-body", "string-body", "int-body", "string-body"]
    );
}

/// A quantified call site is dispatched by the argument's witness among the
/// checker's disjoint candidates; the two primitive instances are distinct
/// witnesses, so there is exactly one match each time.
#[test]
fn witness_dispatch_picks_the_matching_primitive_instance() {
    let source = format!(
        "{PRETTY}
instance Pretty for Int where
    pretty(x) = \"int-body\"
instance Pretty for Float where
    pretty(x) = \"float-body\"
instance Pretty for Bool where
    pretty(x) = \"bool-body\"
let describe = fn x => pretty(x)
let an_int = describe(7)
let a_float = describe(1.5)
let a_bool = describe(true)
"
    );
    let values = values(&source, &["an_int", "a_float", "a_bool"]);
    assert_eq!(values, vec!["int-body", "float-body", "bool-body"]);
}

/// §3.8.4's structural `Show` is a guarantee for records and `Dyn`: the pinned
/// `Builtin(ShowStructural)` target must run the builtin printer, not look for
/// an instance body.
#[test]
fn structural_show_is_the_builtin_printer() {
    let source = "\
let r = { x = 1, y = \"a\" }
let shown = show(r)
let d = dyn(3 m)
let shown_dyn = show(d)
";
    let (module, checked) = compile_ok(source);
    let mut interpreter = Interp::new(&checked).with_buffer();
    let diagnostics = interpreter.load(&module);
    assert!(!diagnostics.has_errors(), "load errors: {diagnostics:?}");
    let record = interpreter.eval_toplevel("r").expect("r");
    let shown = interpreter.eval_toplevel("shown").expect("shown");
    assert_eq!(
        aevia_core::value::repr(&shown),
        aevia_core::value::repr(&Value::string(aevia_core::value::show(&record))),
        "show of a record is the structural printer's answer"
    );
    let dynamic = interpreter.eval_toplevel("d").expect("d");
    let shown_dyn = interpreter.eval_toplevel("shown_dyn").expect("shown_dyn");
    assert_eq!(
        aevia_core::value::repr(&shown_dyn),
        aevia_core::value::repr(&Value::string(aevia_core::value::show(&dynamic))),
        "show of a Dyn is the structural printer's answer"
    );
}

// ---------------------------------------------------------------------------
// Nullary constructors are variants, payload constructors stay applicable
// ---------------------------------------------------------------------------

/// A constructor with no fields *is* its variant when referenced as a value
/// (§3.6.2). Before the fix it stayed a zero-argument `Value::Ctor`, so
/// `match x with | A -> …` failed with E0309 even though the checker accepted
/// it; the payload constructor must still be printable/applicable.
#[test]
fn nullary_constructors_match_and_payload_constructors_apply() {
    let source = "\
type M = | A | B

type Box = | Dot(Float)

let x: M = A
let y = match x with
    | A -> \"a\"
    | B -> \"b\"
let z = match B with
    | A -> 1
    | B -> 2
let d = Dot(2.5)
let payload = match d with
    | Dot(v) -> v
";
    let values = values(source, &["y", "z", "payload"]);
    assert_eq!(values, vec!["a", "2", "2.5"]);
}

/// The docs-aligner/frontend repro: a bare nullary constructor pattern against a
/// value that went through a function parameter.
#[test]
fn nullary_constructors_match_through_function_parameters() {
    let source = "\
type Color = | Red | Blue

fn pick(c: Color): Int =
    match c with
    | Red -> 1
    | Blue -> 2

let picked = pick(Red)
let other = pick(Blue)
";
    let values = values(source, &["picked", "other"]);
    assert_eq!(values, vec!["1", "2"]);
}

// ---------------------------------------------------------------------------
// Cross-module instances (P5) and same-named types (P6)
// ---------------------------------------------------------------------------

/// A class and its instance live in module A; module B imports A's function and
/// calls it. The instance body must be reachable from B's program, which only
/// works when the driver passes A's interface and the body is registered under
/// the symbolic key.
#[test]
fn an_imported_instance_runs() {
    let scratch = Scratch::new("imported");
    scratch.write(
        "A.aev",
        r#"module A

class Pretty a where
    pretty(x: a): String

type Colour = | Red | Blue

instance Pretty for Colour where
    pretty(c) = "A-colour"

fn describe(c: Colour): String = pretty(c)
"#,
    );
    let entry = scratch.write(
        "main.aev",
        r#"module Main

import A

fn main(): IO Unit = do
    println(A.describe(A.Red))
"#,
    );
    let (ast, ir) = run_file_both_modes(&entry);
    assert_eq!(ast, "A-colour\n");
    assert_eq!(ir, "A-colour\n");
}

/// P6: two modules may declare the *same* type name and their own instance of
/// a shared class. Nominal identity is per module, so each call runs its own
/// module's body even though the source spells both types `Shape` and both
/// constructors `Circle`.
#[test]
fn two_modules_with_same_type_name_dispatch_their_own_instances() {
    let scratch = Scratch::new("same-name");
    // The class is declared once; each module instantiates it for its own
    // `Shape` (the orphan rule allows a local type with an imported class).
    scratch.write(
        "C.aev",
        "module C\n\nclass Pretty a where\n    pretty(x: a): String\n",
    );
    scratch.write(
        "A.aev",
        r#"module A

import C

type Shape = | Circle(Float)

instance Pretty for Shape where
    pretty(s) = "body-a"

fn describe(s: Shape): String = pretty(s)
"#,
    );
    scratch.write(
        "B.aev",
        r#"module B

import C

type Shape = | Circle(Float)

instance Pretty for Shape where
    pretty(s) = "body-b"

fn describe(s: Shape): String = pretty(s)
"#,
    );
    let entry = scratch.write(
        "main.aev",
        r#"module Main

import A
import B

fn main(): IO Unit = do
    println(A.describe(A.Circle(1.0)))
    println(B.describe(B.Circle(2.0)))
"#,
    );
    let (ast, ir) = run_file_both_modes(&entry);
    assert_eq!(ast, "body-a\nbody-b\n");
    assert_eq!(ir, "body-a\nbody-b\n");
}
