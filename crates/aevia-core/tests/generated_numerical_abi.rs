//! Public RSSN numerical inventory is usable through its generated namespace.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::value::Value;
use aevia_rssn::bindings::Coverage;
use aevia_syntax::Span;

#[test]
fn generated_numerical_special_namespace_runs_erfi_in_both_engines() {
    let registry = aevia_rssn::registry();
    let binding = aevia_rssn::bindings::catalog(registry)
        .into_iter()
        .find(|row| row.op.name == "num_erfi")
        .expect("num_erfi binding");
    let dynamic = !matches!(binding.coverage, Coverage::Typed);
    let args = if dynamic {
        "[to_value(0.0)]"
    } else {
        "0.0"
    };
    let source = format!(
        "import Rssn_numerical_special\nlet output = Rssn_numerical_special.{}({args})\nfn decode(v: Value) = from_value<Float>(v)\n",
        binding.name
    );
    let directory =
        std::env::temp_dir().join(format!("aevia-generated-numerics-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("main.aev");
    std::fs::write(&path, source).unwrap();
    let program = aevia_core::driver::load(&path);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let mut value = interp.eval_toplevel("output").unwrap();
        if dynamic {
            let Value::Variant(result) = value else {
                panic!("expected dynamic Result")
            };
            assert_eq!(result.index, 0, "{mode:?}: {:?}", result.fields);
            let decode = interp.eval_toplevel("decode").unwrap();
            let decoded = interp
                .apply(decode, vec![result.fields[0].clone()], Span::default())
                .unwrap();
            let Value::Variant(result) = decoded else {
                panic!("expected checked Result")
            };
            assert_eq!(result.index, 0, "{mode:?}: {:?}", result.fields);
            value = result.fields[0].clone();
        }
        assert_eq!(value.as_f64(), Some(0.0), "{mode:?}");
    }
    std::fs::remove_dir_all(directory).unwrap();
}
