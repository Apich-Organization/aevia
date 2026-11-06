//! Variadic tensor constructor identity survives imports and value aliases.
mod common;
use common::Package;
use common::errors;
use common::rendered;

#[test]
fn imports_aliases_and_reexports_run_in_both_engines() {
    let package = Package::with_stdlib("tensor-intrinsic-alias");
    package.write(
        "Forward.aev",
        "module Forward\nimport Tensor { zeros, ones, fill }\ntype Count = Int\nlet make = zeros\nlet repeat = fill\nlet ints = zeros<Count>\nlet choose = if false then zeros else ones\n",
    );
    let program = package.compile_program(
        "probe",
        r#"module Probe
import Forward { make, repeat, ints, choose }
import Tensor as T
let again = make
let a: Tensor<2, Int> = again<Int>(2, 3)
let b: Tensor<1, Float> = T.ones(4)
let c: Tensor<2, Int> = repeat(7, 2, 3)
fn identity(x) = x
let fixed = identity(ints)
let d: Tensor<1, Int> = fixed(2)
let e: Tensor<4, Float> = T.zeros(1, 1, 1, 1)
let family = identity(choose)
let f: Tensor<1, Int> = family<Int>(2)
let g = if true then T.zeros else T.ones
let h: Tensor<1, Int> = g<Int>(2)
let typed = if false then T.zeros<Int> else T.ones<Int>
let j: Tensor<1, Int> = typed(1)
fn main(): IO Unit = println("{a[0, 0]} {b[0]} {c[0, 0]} {d[0]} {f[0]} {h[0]} {j[0]}")
"#,
    );
    assert!(errors(&program).is_empty(), "{}", rendered(&program));
    for mode in [aevia_core::Mode::Ast, aevia_core::Mode::Ir] {
        let mut interp = aevia_core::Interp::new(&program.entry().unwrap().checked)
            .with_buffer()
            .with_mode(mode);
        let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        interp.run_main().expect("main runs");
        assert_eq!(interp.captured(), "0 1 7 0 1 0 1\n", "{mode:?}");
        let value = interp.eval_toplevel("d").expect("specialized tensor");
        let aevia_core::value::Value::Tensor(tensor) = value else {
            panic!("expected tensor")
        };
        let element = tensor.get_flat(0).expect("element");
        assert!(
            matches!(
                element.as_num().unwrap().kind,
                aevia_core::value::NumKind::Int(_)
            ),
            "{mode:?}: specialized Int storage was lost"
        );
    }
}

#[test]
fn wrong_dimensions_element_types_and_arity_are_rejected() {
    let package = Package::with_stdlib("tensor-intrinsic-errors");
    for (case, expression, code) in [
        ("extent", "zeros(\"bad\")", "E0302"),
        ("fill_extent", "fill(1, false)", "E0302"),
        ("fill_value", "fill(false, 2)", "E0302"),
        ("empty_fill", "fill()", "E0315"),
        ("distinct_families", "if true then zeros else fill", "E0302"),
        (
            "different_specializations",
            "if true then zeros<Int> else ones<Real>",
            "E0302",
        ),
        (
            "unspecialized_mixed",
            "if true then zeros<Int> else ones",
            "E0302",
        ),
        ("element", "zeros<Bool>(2)", "E0302"),
        ("rank", "let x: Tensor<2, Float> = zeros(3)", "E0302"),
        (
            "specialized_type",
            "let z = zeros<Int>\nlet x: Tensor<1, Float> = z(3)",
            "E0302",
        ),
        (
            "higher_order_call",
            "fn use(f) = f(1)\nlet x = use(zeros)",
            "E0302",
        ),
        (
            "specialized_twice",
            "let z = zeros<Int>\nlet x = z<Float>(3)",
            "E0315",
        ),
    ] {
        let source = if expression.starts_with("let ") || expression.starts_with("fn ") {
            expression.to_string()
        } else {
            format!("let x = {expression}")
        };
        let source = format!("module Probe\nimport Tensor {{ zeros, ones, fill }}\n{source}\n");
        let program = package.compile_program(case, &source);
        assert!(
            program
                .diagnostics
                .items
                .iter()
                .any(|item| item.code == code),
            "{case}: expected {code}: {}",
            rendered(&program)
        );
        if case == "different_specializations" {
            let labels = program
                .diagnostics
                .items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                labels.contains("Int") && labels.contains("Real"),
                "specializations missing from diagnostic: {}",
                rendered(&program)
            );
        }
        if case == "distinct_families" {
            assert!(
                program
                    .diagnostics
                    .items
                    .iter()
                    .any(|item| item.label.contains("fill constructor")),
                "fill family missing from diagnostic: {}",
                rendered(&program)
            );
        }
        if case == "higher_order_call" {
            assert!(
                program
                    .diagnostics
                    .items
                    .iter()
                    .flat_map(|item| &item.helps)
                    .any(|help| help.contains("lambda")),
                "missing higher-order guidance: {}",
                rendered(&program)
            );
        }
    }
}

#[test]
fn ordinary_functions_shadow_builtin_names() {
    let package = Package::new("tensor-intrinsic-shadow");
    let program = package.compile_program("probe", "module Probe\nfn zeros(x: Int): Int = x + 1\nlet result: Int = zeros<Int>(2)\nfn main(): IO Unit = println(\"{result}\")\n");
    assert!(errors(&program).is_empty(), "{}", rendered(&program));
    for mode in [aevia_core::Mode::Ast, aevia_core::Mode::Ir] {
        let mut interp = aevia_core::Interp::new(&program.entry().unwrap().checked)
            .with_buffer()
            .with_mode(mode);
        let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        interp.run_main().unwrap();
        assert_eq!(interp.captured(), "3\n");
    }
}
