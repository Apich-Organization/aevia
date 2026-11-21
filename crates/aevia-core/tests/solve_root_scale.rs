//! Scalar Solve roots with independent coordinate and residual dimensions.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_dim::Dim;
use std::path::Path;

#[test]
fn solve_root_and_nsolve_f_find_the_same_length_with_tiny_time_residuals() {
    let source = r#"import Solve { root, nsolve_f }
fn tiny(x: Float[L]): Float[T] = (x / (1.0 m) - 2.5) * (1.0e-200 s)
fn ordinary(x: Float[L]): Float[T] = (x / (1.0 m) - 2.5) * (1.0 s)
fn blocked(x: Float[L]): Float[T] =
    if x == 3.0 m then (1.0 s) / 0.0 else tiny(x)
let root_tiny = root(tiny, 0.0 m)
let nsolve_tiny = nsolve_f(tiny, 0.0 m)
let root_ordinary = root(ordinary, 0.0 m)
let root_blocked = root(blocked, 0.0 m)
"#;
    let program = driver::load_with_source(Path::new("solve_root_scale.aev"), source);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = driver::run_modules(&mut interp, &program);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        for name in ["root_tiny", "nsolve_tiny", "root_ordinary"] {
            let Value::Variant(result) = interp.eval_toplevel(name).unwrap() else {
                panic!("{mode:?} {name} must return a Result");
            };
            assert_eq!(result.name, "Ok", "{mode:?} {name}: {result:?}");
            let [Value::Num(root)] = result.fields.as_slice() else {
                panic!("{mode:?} {name} must carry a numeric root");
            };
            assert_eq!(root.dim, Dim::base("Length"), "{mode:?} {name}");
            let actual = root.to_f64().unwrap();
            assert!((actual - 2.5).abs() < 1e-12, "{mode:?} {name}: {actual}");
        }
        let Value::Variant(blocked) = interp.eval_toplevel("root_blocked").unwrap() else {
            panic!("{mode:?} root_blocked must return a Result");
        };
        assert_eq!(blocked.name, "Err", "{mode:?}: {blocked:?}");
        assert!(
            matches!(
                blocked.fields.as_slice(),
                [Value::Str(message)] if message.contains("invalid root-finding input")
            ),
            "{mode:?}: {blocked:?}",
        );
    }
}
