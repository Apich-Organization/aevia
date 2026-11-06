//! Binder choices belong to actual checked source declarations, not Fn shapes.
use aevia_core::Compilation;
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::Scope;
use aevia_core::value::Value;
use aevia_syntax::DeclKind;
use aevia_syntax::PatKind;
use aevia_syntax::Span;
use aevia_types::Rep;
use aevia_types::Shape;
use aevia_types::Type;

fn checked(source: &str) -> Compilation {
    let result = compile("binder-identity.aev", source);
    assert!(
        !result.diagnostics.has_errors(),
        "{:?}",
        result.diagnostics.items
    );
    result
}
fn install(
    compiled: &Compilation,
    mode: Mode,
) -> Interp {
    let mut interp = Interp::new(&compiled.checked).with_mode(mode);
    let diagnostics = interp.load(compiled.module.as_ref().unwrap());
    assert!(
        !diagnostics.has_errors(),
        "{mode:?}: {:?}",
        diagnostics.items
    );
    interp
}
fn witness(value: Value) -> Type {
    let Value::TypeRep(ty) = value else {
        panic!("type witness")
    };
    (*ty).clone()
}
fn selected_expr(
    compiled: &Compilation,
    name: &str,
) -> aevia_syntax::Expr {
    compiled.module.as_ref().unwrap().decls.iter().find_map(|decl| match &decl.kind {
        DeclKind::Let { pattern, value, .. } if matches!(&pattern.kind, PatKind::Var(binding) if binding == name) => Some(value.clone()), _ => None,
    }).unwrap()
}

#[test]
fn an_identity_hof_retains_phantom_and_non_signature_header_order() {
    let compiled = checked(
        "fn choose<phantom, b, a>(x: a, y: b) = type_rep<phantom>()\nfn pass<t>(value: t) = value\nlet alias = pass(choose)\nlet selected = alias<Bool, String, Int>(7, \"seven\")\n",
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compiled, mode);
        assert_eq!(
            witness(interp.eval_toplevel("selected").unwrap()),
            Type::Bool,
            "{mode:?}"
        );
    }
}

#[test]
fn all_five_kinds_survive_forwarding_and_local_aliases() {
    let compiled = checked(
        r#"fn reflect<a, r: Rep, d: Dim, n: Nat, row: Row>(x: Int) = { a = type_rep<a>(), r = type_rep<r>(), d = type_rep<Float[d]>(), n = type_rep<Tensor<[n], Int>>(), row = type_rep<{ x: Int, ...row }>() }
fn pass<t>(value: t, flag: Bool) =
    let saved = value
    if flag then saved else value
let alias = pass(reflect, true)
let selected = alias<String, Float, Length, 3, { y: Bool }>(0)
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compiled, mode);
        let Value::Record(record) = interp.eval_toplevel("selected").unwrap() else {
            panic!("record")
        };
        for (name, expected) in [
            ("a", Type::Str),
            ("r", Type::Scalar(Rep::Float, aevia_dim::Dim::one())),
            ("d", Type::float(aevia_dim::Dim::base("Length"))),
            ("n", Type::tensor(Shape::refined([Some(3)]), Type::int())),
            (
                "row",
                Type::record([("x".into(), Type::int()), ("y".into(), Type::Bool)]),
            ),
        ] {
            assert_eq!(
                witness(record.fields[name].clone()),
                expected,
                "{mode:?} {name}"
            );
        }
    }
}

#[test]
fn a_same_shape_different_origin_cannot_borrow_phantom_selection() {
    let compiled = checked(
        "fn first<a, b>(x: Int) = type_rep<a>()\nfn second<b, a>(x: Int) = type_rep<a>()\nlet alias = first\nlet selected = alias<Int, String>\n",
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compiled, mode);
        let replacement = interp.eval_toplevel("second").unwrap();
        interp.globals.insert("alias".into(), replacement);
        let selected = interp
            .eval(&selected_expr(&compiled, "selected"), &Scope::root())
            .unwrap();
        let error = interp
            .apply(selected, vec![Value::int(0)], Span::default())
            .unwrap_err();
        assert_eq!(error.code, "E0302", "{mode:?}");
    }
}

#[test]
fn unchanged_generic_globals_remain_callable_after_append_only_rechecking() {
    let prefix = "fn choose<a, b>(x: Int) = type_rep<b>()\nlet alias = choose\n";
    let first = checked(prefix);
    let second = checked(&format!("{prefix}let selected = alias<Int, String>(0)\n"));
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&first, mode);
        let old = &first.module.as_ref().unwrap().decls;
        interp.prepare_incremental_module(&second.checked, old);
        for decl in &second.module.as_ref().unwrap().decls[old.len()..] {
            interp.load_decl(decl).unwrap();
        }
        assert_eq!(
            witness(interp.eval_toplevel("selected").unwrap()),
            Type::Str,
            "{mode:?}"
        );
    }
}

#[test]
fn reordered_revision_headers_cannot_relabel_a_retained_closure() {
    let first = checked("fn choose<a, b>(x: Int) = type_rep<a>()\nlet old = choose<Int, String>\n");
    let second =
        checked("fn choose<b, a>(x: Int) = type_rep<a>()\nlet selected = choose<Int, String>\n");
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&first, mode);
        let retained = interp.eval_toplevel("old").unwrap();
        interp.prepare_incremental_module(&second.checked, &[]);
        let selected = interp
            .eval(&selected_expr(&second, "selected"), &Scope::root())
            .unwrap();
        assert_eq!(
            interp
                .apply(selected, vec![Value::int(0)], Span::default())
                .unwrap_err()
                .code,
            "E0302",
            "{mode:?}"
        );
        assert_eq!(
            witness(
                interp
                    .apply(retained, vec![Value::int(0)], Span::default())
                    .unwrap()
            ),
            Type::int(),
            "{mode:?}"
        );
    }
}

#[test]
fn an_alias_contract_change_hidden_from_fn_shape_changes_the_origin() {
    let first = checked(
        "type Helper<a, b> = a\nfn choose<a, b>(x: Int) = type_rep<Helper<a, b>>()\nlet old = choose<Int, String>\n",
    );
    let second = checked(
        "type Helper<a, b> = b\nfn choose<a, b>(x: Int) = type_rep<Helper<a, b>>()\nlet selected = choose<Int, String>\n",
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&first, mode);
        let retained = interp.eval_toplevel("old").unwrap();
        interp.prepare_incremental_module(&second.checked, &[]);
        let selected = interp
            .eval(&selected_expr(&second, "selected"), &Scope::root())
            .unwrap();
        assert_eq!(
            interp
                .apply(selected, vec![Value::int(0)], Span::default())
                .unwrap_err()
                .code,
            "E0302",
            "{mode:?}"
        );
        assert_eq!(
            witness(
                interp
                    .apply(retained, vec![Value::int(0)], Span::default())
                    .unwrap()
            ),
            Type::int(),
            "{mode:?}"
        );
    }
}
