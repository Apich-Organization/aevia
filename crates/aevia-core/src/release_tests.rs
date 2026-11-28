//! Private structural assertions for the public release execution option.
use crate::Interp;
use crate::Mode;
use crate::compile;
use crate::value::ClosureBody;
use crate::value::Value;
use aevia_ir::Tail;

fn interpreter(
    source: &str,
    optimized: bool,
) -> Interp {
    let compilation = compile("release.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let mut interp = Interp::new(&compilation.checked).with_mode(Mode::Ir);
    interp.optimize_ir = optimized;
    let loaded = interp.load(compilation.module.as_ref().unwrap());
    assert!(!loaded.has_errors(), "{:?}", loaded.items);
    interp
}

#[test]
fn release_rewrites_constant_branches_in_whole_and_incremental_modules() {
    let source = "fn choose(): Int = if true then 42 else 0\n";
    let mut debug = interpreter(source, false);
    let mut release = interpreter(source, true);
    for (interp, optimized) in [(&mut debug, false), (&mut release, true)] {
        let Value::Closure(closure) = interp.eval_toplevel("choose").unwrap() else {
            panic!("closure")
        };
        let ClosureBody::Ir(function) = &closure.body else {
            panic!("IR function")
        };
        assert_eq!(matches!(function.body.tail, Tail::If { .. }), !optimized);
    }

    let compilation = compile("incremental.aev", source);
    let mut interp = Interp::new(&compilation.checked);
    interp.optimize_ir = true;
    for declaration in &compilation.module.as_ref().unwrap().decls {
        interp.load_decl(declaration).unwrap();
    }
    let Value::Closure(closure) = interp.eval_toplevel("choose").unwrap() else {
        panic!("closure")
    };
    let ClosureBody::Ir(function) = &closure.body else {
        panic!("IR function")
    };
    assert!(!matches!(function.body.tail, Tail::If { .. }));
}
