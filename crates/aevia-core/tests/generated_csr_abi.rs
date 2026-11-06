//! The generated numerical matrix namespace exposes a real sparse solve.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::value::Value;
use aevia_rssn::bindings::Coverage;
use aevia_syntax::Span;

#[test]
fn generated_sparse_cg_runs_in_both_engines() {
    let binding = aevia_rssn::bindings::catalog(aevia_rssn::registry())
        .into_iter()
        .find(|entry| entry.op.name == "num_csr_cg")
        .expect("num_csr_cg binding");
    let dynamic = !matches!(binding.coverage, Coverage::Typed);
    let matrix =
        "{ rows = 2, cols = 2, indptr = [0, 1, 2], indices = [0, 1], values = [2.0, 4.0] }";
    let args = if dynamic {
        format!("[to_value({matrix}), to_value([2.0, 8.0]), to_value(0.0000000001), to_value(40)]")
    } else {
        format!("{matrix}, [2.0, 8.0], 0.0000000001, 40")
    };
    let source = format!(
        "import Rssn_numerical_matrix\nlet output = Rssn_numerical_matrix.{}({args})\nfn decode(v: Value) = from_value<{{ x: Tensor<1, Float>, residual: Float, iterations: Int, converged: Bool }}>(v)\n",
        binding.name
    );
    let directory =
        std::env::temp_dir().join(format!("aevia-generated-csr-{}", std::process::id()));
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
                panic!("expected kernel Result")
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
        let Value::Record(record) = value else {
            panic!("expected solver record")
        };
        assert!(
            matches!(record.fields["converged"], Value::Bool(true)),
            "{mode:?}"
        );
        let Value::Tensor(solution) = &record.fields["x"] else {
            panic!("expected solution tensor")
        };
        assert_eq!(solution.shape, vec![2]);
        for (i, expected) in [1.0, 2.0].into_iter().enumerate() {
            let actual = solution.get_flat(i).unwrap().as_f64().unwrap();
            assert!(
                (actual - expected).abs() < 1e-8,
                "{mode:?}: x[{i}] = {actual}"
            );
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}
