//! Exact manifest dimensions must survive both checker and runtime paths.
use aevia_core::eval::Mode;
use aevia_core::value::Value;
use aevia_core::{Interp, compile};
use aevia_dim::{Dim, ratio};

#[test]
fn pinned_dimension_parameter_reaches_tensor_result_in_both_evaluators() {
    let source = "let path: Tensor<2, Real[Time^(1/2)]> = Rssn.sde_wiener(1, 1, 2, 1.0 s)\n";
    let compilation = compile("dimensions.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.expect("module");
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let Value::Tensor(value) = interp.eval_toplevel("path").expect("path") else {
            panic!("expected tensor")
        };
        assert_eq!(value.dim, Dim::base("Time").pow(&ratio(1, 2)), "{mode:?}");
    }
}

#[test]
fn numerical_dimension_policies_preserve_products_and_record_fields() {
    let source = r#"
let dot: Float[Length*Time] = Rssn.num_dot([1.0 m, 2.0 m], [3.0 s, 4.0 s])
let svd = Rssn.num_svd([[3.0 m, 0.0 m], [0.0 m, 2.0 m]])
let tt = Rssn.num_tt_svd([2.0 m, 0.0 m, 0.0 m, 2.0 m], [2, 2], 0.0, 2)
"#;
    let compilation = compile("numeric-dimensions.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.expect("module");
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        let diagnostics = interp.load(&module);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        let dot = interp.eval_toplevel("dot").unwrap();
        assert_eq!(
            aevia_core::value::value_dim(&dot),
            Dim::base("Length").mul(&Dim::base("Time"))
        );
        let Value::Record(svd) = interp.eval_toplevel("svd").unwrap() else {
            panic!("svd record");
        };
        assert_eq!(
            aevia_core::value::value_dim(&svd.fields["s"]),
            Dim::base("Length")
        );
        assert_eq!(aevia_core::value::value_dim(&svd.fields["u"]), Dim::one());
        assert_eq!(aevia_core::value::value_dim(&svd.fields["v"]), Dim::one());
        let Value::Record(tt) = interp.eval_toplevel("tt").unwrap() else {
            panic!("tt record");
        };
        assert_eq!(
            aevia_core::value::value_dim(&tt.fields["scale"]),
            Dim::base("Length")
        );
        assert_eq!(
            aevia_core::value::value_dim(&tt.fields["error"]),
            Dim::base("Length")
        );
        assert_eq!(
            aevia_core::value::value_dim(&tt.fields["cores"]),
            Dim::one()
        );
    }
}

#[test]
fn numerical_equal_dimension_policy_rejects_incompatible_inputs() {
    let compilation = compile(
        "mismatched-dimensions.aev",
        "let bad = Rssn.num_vector_add([1.0 m], [1.0 s])\n",
    );
    assert!(
        compilation
            .diagnostics
            .items
            .iter()
            .any(|d| d.code == "E0401"),
        "{:?}",
        compilation.diagnostics.items
    );
}

#[test]
fn empty_tensor_representation_is_evidence_not_guessed_from_parameter() {
    use aevia_core::value::{ExactRep, Storage, TensorData};
    use std::rc::Rc;
    let op = aevia_rssn::registry().op("num_norm").expect("numeric norm");
    let key = op.key(0).expect("pinned key");
    let make = |storage| {
        Value::Tensor(Rc::new(TensorData::new(vec![0], storage, Dim::one(), None)))
    };
    let int_tensor = make(Storage::TypedExact(Vec::new(), ExactRep::Int));
    let error = aevia_core::rssn_module::call_operator(
        key,
        vec![int_tensor],
        aevia_syntax::Span::default(),
    )
    .unwrap_err();
    assert!(matches!(error.code, "E1003" | "E1001"), "{error:?}");
    let unknown_tensor = make(Storage::Boxed(Vec::new()));
    let error = aevia_core::rssn_module::call_operator(
        key,
        vec![unknown_tensor],
        aevia_syntax::Span::default(),
    )
    .unwrap_err();
    assert_eq!(error.code, "E1001");
    let float_tensor = make(Storage::Dense(Rc::new(Vec::new())));
    let result = aevia_core::rssn_module::call_operator(
        key,
        vec![float_tensor],
        aevia_syntax::Span::default(),
    )
    .expect("proven Float tensor");
    assert_eq!(result.as_f64(), Some(0.0));
}
