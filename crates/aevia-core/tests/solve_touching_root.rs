//! Standard Solve roots across large and small residual scales.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_dim::Dim;
use std::path::Path;

#[test]
fn solve_finds_rescaled_double_roots_and_rejects_nonzero_minima() {
    let source = r#"import Solve { root, nsolve_f }
fn well(x: Float[L]): Float[1] = ((x / (1.0 m))^2 - 2.0)^2
fn small(x: Float[L]): Float[T] = well(x) * (1.0e-200 s)
fn large(x: Float[L]): Float[T] = well(x) * (1.0e200 s)
fn lifted(x: Float[L]): Float[T] = (well(x) + 1.0e-24) * (1.0e-200 s)
fn constant(_x: Float[L]): Float[T] = 1.0e-200 s
let small_root = root(small, 0.0 m)
let large_root = nsolve_f(large, 0.0 m)
let lifted_minimum = root(lifted, 0.0 m)
let constant_minimum = nsolve_f(constant, 0.0 m)
"#;
    let program = driver::load_with_source(Path::new("solve_touching_root.aev"), source);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = driver::run_modules(&mut interp, &program);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        for name in ["small_root", "large_root"] {
            let Value::Variant(answer) = interp.eval_toplevel(name).unwrap() else {
                panic!("{mode:?} {name} must return a Result");
            };
            assert_eq!(answer.name, "Ok", "{mode:?} {name}: {answer:?}");
            let [Value::Num(root)] = answer.fields.as_slice() else {
                panic!("{mode:?} {name} must return a numeric root");
            };
            assert_eq!(root.dim, Dim::base("Length"), "{mode:?} {name}");
            let actual = root.to_f64().unwrap();
            assert!(
                (actual.abs() - 2.0_f64.sqrt()).abs() < 2e-14,
                "{mode:?} {name}: {actual}"
            );
        }
        for name in ["lifted_minimum", "constant_minimum"] {
            let Value::Variant(answer) = interp.eval_toplevel(name).unwrap() else {
                panic!("{mode:?} {name} must return a Result");
            };
            assert_eq!(answer.name, "Err", "{mode:?} {name}: {answer:?}");
        }
    }
}
