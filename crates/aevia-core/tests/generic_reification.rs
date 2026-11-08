//! Runtime generic evidence belongs to the actual callable and its activation.
use aevia_core::Compilation;
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::Value;
use aevia_syntax::Span;
use aevia_types::Rep;
use aevia_types::Shape;
use aevia_types::Type;

fn checked(source: &str) -> Compilation {
    let result = compile("generics.aev", source);
    assert!(
        !result.diagnostics.has_errors(),
        "{:?}",
        result.diagnostics.items
    );
    result
}
fn install(
    compilation: &Compilation,
    mode: Mode,
) -> Interp {
    let mut interp = Interp::new(&compilation.checked).with_mode(mode);
    let diagnostics = interp.load(compilation.module.as_ref().unwrap());
    assert!(
        !diagnostics.has_errors(),
        "{mode:?}: {:?}",
        diagnostics.items
    );
    interp
}
fn witness(
    interp: &mut Interp,
    name: &str,
) -> Type {
    let Value::TypeRep(ty) = interp.eval_toplevel(name).unwrap() else {
        panic!("{name}: expected type witness")
    };
    (*ty).clone()
}

#[test]
fn type_dimension_natural_row_and_representation_arguments_are_reified() {
    let source = r#"fn reflect<a>(x: a) = type_rep<a>()
fn phantom<a>() = type_rep<a>()
fn dimensional<d>(x: Float[d]) = type_rep<Float[d]>()
fn extent<n>(x: Tensor<[n], Float>) = type_rep<Tensor<[n], Float>>()
fn row<r: Row>(x: { x: Int, ...r }) = type_rep<{ x: Int, ...r }>()
fn representation<r: Rep>(x: r) = type_rep<r>()
fn uppercase<T>(x: T) = type_rep<T>()
let integer = reflect(7)
let string = reflect("seven")
let chosen = phantom<Bool>()
let dim = dimensional(2.0 m)
let shape = extent([1.0, 2.0, 3.0])
let record = row({ x = 1, y = "yes" })
let rep = representation(2.0)
let capital = uppercase(true)
"#;
    let compilation = checked(source);
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        assert_eq!(witness(&mut interp, "integer"), Type::int(), "{mode:?}");
        assert_eq!(witness(&mut interp, "string"), Type::Str, "{mode:?}");
        assert_eq!(witness(&mut interp, "chosen"), Type::Bool, "{mode:?}");
        assert_eq!(
            witness(&mut interp, "dim"),
            Type::float(aevia_dim::Dim::base("Length")),
            "{mode:?}"
        );
        assert_eq!(
            witness(&mut interp, "shape"),
            Type::tensor(
                Shape::refined([Some(3)]),
                Type::float(aevia_dim::Dim::one())
            ),
            "{mode:?}"
        );
        assert_eq!(
            witness(&mut interp, "record"),
            Type::record([("x".into(), Type::int()), ("y".into(), Type::Str)]),
            "{mode:?}"
        );
        assert_eq!(
            witness(&mut interp, "rep"),
            Type::Scalar(Rep::Float, aevia_dim::Dim::one()),
            "{mode:?}"
        );
        assert_eq!(witness(&mut interp, "capital"), Type::Bool, "{mode:?}");
    }
}

#[test]
fn returned_closures_keep_their_own_bindings_and_nested_binders_shadow() {
    let compilation = checked(
        r#"fn factory<a>(x: a) = fn () => type_rep<a>()
fn nested<a>(x: a) =
    fn inner<a>(y: a) = type_rep<a>()
    inner("inner")
let first = factory(1)
let second = factory("second")
let first_type = first()
let second_type = second()
let nested_type = nested(true)
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        assert_eq!(witness(&mut interp, "first_type"), Type::int(), "{mode:?}");
        assert_eq!(witness(&mut interp, "second_type"), Type::Str, "{mode:?}");
        assert_eq!(witness(&mut interp, "nested_type"), Type::Str, "{mode:?}");
    }
}

#[test]
fn host_calls_solve_from_actual_data_and_never_reuse_an_unrelated_span() {
    let compilation = checked(
        "fn reflect<a>(x: a) = type_rep<a>()\nfn phantom<a>() = type_rep<a>()\nlet unrelated = type_rep<Int>()\n",
    );
    let borrowed_span = compilation
        .checked
        .expr_types
        .iter()
        .find_map(|(span, ty)| (*ty == Type::int()).then_some(*span))
        .unwrap_or_default();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        let reflect = interp.eval_toplevel("reflect").unwrap();
        let Value::TypeRep(ty) = interp
            .apply(reflect, vec![Value::Str("host".into())], borrowed_span)
            .unwrap()
        else {
            panic!("type witness")
        };
        assert_eq!(*ty, Type::Str, "{mode:?}");
        let phantom = interp.eval_toplevel("phantom").unwrap();
        assert_eq!(
            interp
                .apply(phantom, vec![], borrowed_span)
                .unwrap_err()
                .code,
            "E0304",
            "{mode:?}"
        );
    }
}

#[test]
fn typed_tail_calls_preserve_the_selected_activation_without_stack_growth() {
    let compilation = checked(
        "fn loop<a>(n: Int, x: a) = if n <= 0 then type_rep<a>() else loop<a>(n - 1, x)\nlet witness = loop<String>(2000, \"tail\")\n",
    );
    let mut interp = install(&compilation, Mode::Ir);
    assert_eq!(witness(&mut interp, "witness"), Type::Str);
}

#[test]
fn local_generic_self_calls_keep_their_declared_headers_and_environment() {
    let compilation = checked(
        "fn outer<a>(x: a) =\n    fn inner<b>(n: Int, y: b) = if n <= 0 then type_rep<b>() else inner<b>(n - 1, y)\n    inner<a>(8, x)\nlet selected = outer(\"local\")\n",
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        assert_eq!(witness(&mut interp, "selected"), Type::Str, "{mode:?}");
    }
}

#[test]
fn explicit_user_type_arguments_are_checked_at_actual_calls() {
    for (source, expected) in [
        (
            "fn identity<a>(x: a) = x\nlet bad = identity<Int>(\"wrong\")\n",
            "E0302",
        ),
        (
            "fn identity<a>(x: a) = x\nlet bad = identity<Int, String>(1)\n",
            "E0315",
        ),
    ] {
        let compilation = compile("generic_mismatch.aev", source);
        assert!(
            compilation
                .diagnostics
                .items
                .iter()
                .any(|diagnostic| diagnostic.code == expected),
            "accepted {source}: {:?}",
            compilation.diagnostics.items
        );
    }
}

#[test]
fn a_representation_polymorphic_alias_can_be_generalized_and_selected_again() {
    let compilation = checked(
        "fn reflect<r: Rep>(x: r) = type_rep<r>()\nlet alias = reflect\nlet integer = alias<Int>(2)\nlet floating = alias<Float>(2.0)\n",
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        assert_eq!(witness(&mut interp, "integer"), Type::int(), "{mode:?}");
        assert_eq!(
            witness(&mut interp, "floating"),
            Type::float(aevia_dim::Dim::one()),
            "{mode:?}"
        );
    }
}

#[test]
fn function_aliases_preserve_ordered_unused_generic_headers_across_imports() {
    let root = std::env::temp_dir().join(format!("aevia-generic-headers-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("Factory.aev"),
        "module Factory\nfn choose<a, b>(x: a) = type_rep<b>()\nlet alias = choose\n",
    )
    .unwrap();
    std::fs::write(
        root.join("main.aev"),
        "import Factory as F\nlet pick = F.alias\nlet selected = pick<Int, String>(4)\n",
    )
    .unwrap();
    let program = aevia_core::driver::load(&root.join("main.aev"));
    assert!(
        !program.diagnostics.has_errors(),
        "{}",
        program.diagnostics.render(&program.sources)
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        assert_eq!(witness(&mut interp, "selected"), Type::Str, "{mode:?}");
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_selected_generic_function_cannot_be_rebound_by_a_false_host_signature() {
    let compilation = checked(
        "fn identity<a>(x: a) = x\nlet selected = identity<Int>\nfn wrap(f: String -> String) = to_callback(f)\n",
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        let selected = interp.eval_toplevel("selected").unwrap();
        let wrap = interp.eval_toplevel("wrap").unwrap();
        assert!(
            interp.apply(wrap, vec![selected], Span::default()).is_err(),
            "{mode:?}"
        );
    }
}

#[test]
fn a_later_shadow_cannot_replace_the_original_callable_schema() {
    let compilation = checked(
        "fn original(x: Int): Int = x\nlet saved = original\nlet original = fn (x: String) => x\nfn wrap(f: String -> String) = to_callback(f)\n",
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        let saved = interp.eval_toplevel("saved").unwrap();
        let shadow = interp.eval_toplevel("original").unwrap();
        let wrap = interp.eval_toplevel("wrap").unwrap();
        assert!(
            interp
                .apply(wrap.clone(), vec![saved], Span::default())
                .is_err(),
            "{mode:?}: original Int function acquired its String shadow's schema"
        );
        assert!(
            matches!(
                interp.apply(wrap, vec![shadow], Span::default()).unwrap(),
                Value::Callback(_)
            ),
            "{mode:?}"
        );
    }
}

#[test]
fn a_defaulted_literal_result_cannot_be_specialized_to_another_representation() {
    let compilation =
        checked("fn constant(x: Int) = 1\nfn wrap(f: Int -> Float) = to_callback(f)\n");
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        let constant = interp.eval_toplevel("constant").unwrap();
        let wrap = interp.eval_toplevel("wrap").unwrap();
        assert!(
            interp.apply(wrap, vec![constant], Span::default()).is_err(),
            "{mode:?}: a literal's default representation was rebound"
        );
    }
}

#[test]
fn generic_numeric_creation_preserves_the_selected_representation_and_precision() {
    let compilation = checked(
        "fn number<r: Rep>(): r = 123456789012345678901234567890123456789\nfn decimal<r: Rep>(): r = 0.1\nfn inferred<a>(): a = 1\nlet integer = number<Int>()\nlet real = number<Real>()\nlet floating = inferred<Float>()\nlet complex = inferred<Complex>()\nlet exact_decimal = decimal<Real>()\n",
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        let integer = interp.eval_toplevel("integer").unwrap();
        assert!(
            matches!(&integer.as_num().unwrap().kind,aevia_core::value::NumKind::Int(number) if number.to_string() == "123456789012345678901234567890123456789"),
            "{mode:?}"
        );
        let real = interp.eval_toplevel("real").unwrap();
        assert!(
            matches!(&real.as_num().unwrap().kind,aevia_core::value::NumKind::Exact(number) if number.numer().to_string() == "123456789012345678901234567890123456789" && number.is_integer()),
            "{mode:?}"
        );
        assert!(
            matches!(
                interp
                    .eval_toplevel("floating")
                    .unwrap()
                    .as_num()
                    .unwrap()
                    .kind,
                aevia_core::value::NumKind::Float(1.0)
            ),
            "{mode:?}"
        );
        assert!(
            matches!(
                interp
                    .eval_toplevel("complex")
                    .unwrap()
                    .as_num()
                    .unwrap()
                    .kind,
                aevia_core::value::NumKind::Complex(1.0, 0.0)
            ),
            "{mode:?}"
        );
        let decimal = interp.eval_toplevel("exact_decimal").unwrap();
        assert!(
            matches!(&decimal.as_num().unwrap().kind,aevia_core::value::NumKind::Exact(number) if number.to_string() == "1/10"),
            "{mode:?}"
        );
    }
}

#[test]
fn invalid_numeric_specializations_cannot_execute_or_mint_callbacks() {
    for source in [
        "fn number<a>(): a = 1\nlet bad = number<Bool>()\n",
        "fn decimal<r: Rep>(): r = 0.1\nlet bad = decimal<Int>()\n",
        "fn number<a>(): a = 1\nlet bad = to_callback(number<Bool>)\n",
    ] {
        let compilation = compile("bad_generic_literal.aev", source);
        if compilation.diagnostics.has_errors() {
            assert!(
                compilation
                    .diagnostics
                    .items
                    .iter()
                    .any(|diagnostic| diagnostic.code == "E0302"),
                "{:?}",
                compilation.diagnostics.items
            );
            continue;
        }
        for mode in [Mode::Ast, Mode::Ir] {
            let mut interp = Interp::new(&compilation.checked).with_mode(mode);
            let diagnostics = interp.load(compilation.module.as_ref().unwrap());
            assert!(
                diagnostics
                    .items
                    .iter()
                    .any(|diagnostic| diagnostic.code == "E0302"),
                "{mode:?}: accepted {source}: {:?}",
                diagnostics.items
            );
        }
    }
}

#[test]
fn unselected_numeric_materialization_fails_before_executing_the_body() {
    let compilation = checked("fn number<r: Rep>(): r = 1\n");
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        let function = interp.eval_toplevel("number").unwrap();
        assert_eq!(
            interp
                .apply(function, vec![], Span::default())
                .unwrap_err()
                .code,
            "E0304",
            "{mode:?}"
        );
    }
}

#[test]
fn returned_numeric_closures_retain_both_representation_and_lexical_revision() {
    let source = "fn factory<r: Rep>(x: r) = fn (): r => 1\n";
    let first = checked(source);
    let second = checked(&source.replace("=> 1", "=> 2"));
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&first, mode);
        let factory = interp.eval_toplevel("factory").unwrap();
        let old = interp
            .apply(factory, vec![Value::float(3.0)], Span::default())
            .unwrap();
        interp.prepare_module(&second.checked);
        assert!(!interp.load(second.module.as_ref().unwrap()).has_errors());
        let factory = interp.eval_toplevel("factory").unwrap();
        let new = interp
            .apply(factory, vec![Value::float(3.0)], Span::default())
            .unwrap();
        for (closure, expected) in [(old, 1.0), (new, 2.0)] {
            let value = interp.apply(closure, vec![], Span::default()).unwrap();
            assert!(
                matches!(value.as_num().unwrap().kind,aevia_core::value::NumKind::Float(number) if number == expected),
                "{mode:?}: {value:?}"
            );
        }
    }
}

#[test]
fn generic_unit_literals_use_the_actual_selected_representation() {
    let compilation = checked(
        "private unit batch : 1 = 2\nfn amount<r: Rep>(): r = 3 batch\nlet real = amount<Real>()\nlet floating = amount<Float>()\nlet complex = amount<Complex>()\n",
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        assert!(
            matches!(
                interp.eval_toplevel("real").unwrap().as_num().unwrap().kind,
                aevia_core::value::NumKind::Exact(_)
            ),
            "{mode:?}"
        );
        assert!(
            matches!(
                interp
                    .eval_toplevel("floating")
                    .unwrap()
                    .as_num()
                    .unwrap()
                    .kind,
                aevia_core::value::NumKind::Float(6.0)
            ),
            "{mode:?}"
        );
        assert!(
            matches!(
                interp
                    .eval_toplevel("complex")
                    .unwrap()
                    .as_num()
                    .unwrap()
                    .kind,
                aevia_core::value::NumKind::Complex(6.0, 0.0)
            ),
            "{mode:?}"
        );
    }
}

#[test]
fn ordinary_body_literals_keep_concrete_readings_and_native_eligible_atoms() {
    let compilation = checked(
        "fn inc(x: Int): Int = x + 1\nfn shifted(x: Float): Float = x + 0.5\nlet integer = inc(4)\nlet floating = shifted(1.0)\n",
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        assert_eq!(
            interp.eval_toplevel("integer").unwrap().as_f64(),
            Some(5.0),
            "{mode:?}"
        );
        assert_eq!(
            interp.eval_toplevel("floating").unwrap().as_f64(),
            Some(1.5),
            "{mode:?}"
        );
    }
}

#[test]
fn dependent_numeric_results_follow_inputs_instead_of_becoming_extra_headers() {
    let compilation = checked(
        r#"fn half(x) = x / 2
fn add_one(x) = x + 1
fn scalar_half<r: Rep>(x: r) = x / 2
fn reflect<a>(x: a) = type_rep<a>()
let exact = half(3)
let floating = half(3.0)
let selected = scalar_half<Float>
let through_alias = selected(5.0)
let integer = add_one(3)
let decimal = add_one(3.0)
let exact_type = reflect(exact)
let floating_type = reflect(floating)
let integer_type = reflect(integer)
let decimal_type = reflect(decimal)
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        assert_eq!(
            witness(&mut interp, "exact_type"),
            Type::real(aevia_dim::Dim::one()),
            "{mode:?}"
        );
        assert_eq!(
            witness(&mut interp, "floating_type"),
            Type::float(aevia_dim::Dim::one()),
            "{mode:?}"
        );
        assert_eq!(
            witness(&mut interp, "integer_type"),
            Type::int(),
            "{mode:?}"
        );
        assert_eq!(
            witness(&mut interp, "decimal_type"),
            Type::float(aevia_dim::Dim::one()),
            "{mode:?}"
        );
        assert_eq!(
            interp.eval_toplevel("through_alias").unwrap().as_f64(),
            Some(2.5),
            "{mode:?}"
        );
    }
}

#[test]
fn numeric_transfer_constraints_survive_first_class_aliases_and_callbacks() {
    let compilation = checked(
        r#"fn half<r: Rep>(x: r) = x / 2
let alias = half
let floating: Float -> Float = alias<Float>
let exact: Int -> Real = alias<Int>
let callback = to_callback(floating)
let floating_result = floating(7.0)
let exact_result = exact(7)
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        assert_eq!(
            interp.eval_toplevel("floating_result").unwrap().as_f64(),
            Some(3.5),
            "{mode:?}"
        );
        assert_eq!(
            interp.eval_toplevel("exact_result").unwrap().as_f64(),
            Some(3.5),
            "{mode:?}"
        );
    }
    for source in [
        "fn half(x) = x / 2\nlet impossible: Int -> Int = half\n",
        "fn half<r: Rep>(x: r) = x / 2\nlet impossible: Float -> Real = half<Float>\n",
        "fn half<r: Rep>(x: r) = x / 2\nlet impossible = half<Int, Real>(3)\n",
    ] {
        let result = compile("numeric-contract.aev", source);
        assert!(
            result.diagnostics.has_errors(),
            "numeric contract accepted: {source}"
        );
    }
}

#[test]
fn materialized_captured_numeric_values_cannot_be_reinterpreted() {
    let compilation = checked(
        r#"fn capture(x) = fn () => x
fn reflect<a>(x: a) = type_rep<a>()
let saved = capture(5)
let value = saved()
let schema = reflect(value)
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        assert_eq!(witness(&mut interp, "schema"), Type::int(), "{mode:?}");
    }
    let rejected = compile(
        "capture.aev",
        "fn capture(x) = fn () => x\nlet saved = capture(5)\nlet first: Int = saved()\nlet impossible: Float = saved()\n",
    );
    assert!(
        rejected.diagnostics.has_errors(),
        "materialized captured value was generalized"
    );
}

#[test]
fn host_higher_order_calls_derive_results_from_the_actual_numeric_contract() {
    let compilation = checked(
        "fn half<r: Rep>(x: r) = x / 2\nfn result<a, b>(f: a -> b, x: a) = type_rep<b>()\n",
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        let half = interp.eval_toplevel("half").unwrap();
        let result = interp.eval_toplevel("result").unwrap();
        let actual = interp
            .apply(result, vec![half, Value::int(3)], Span::default())
            .unwrap();
        let Value::TypeRep(actual) = actual else {
            panic!("expected actual derived result schema")
        };
        assert_eq!(*actual, Type::real(aevia_dim::Dim::one()), "{mode:?}");
    }
}

#[test]
fn aliased_materializations_keep_captured_literal_schema_across_imports() {
    let root = std::env::temp_dir().join(format!("aevia-numeric-contract-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("Numbers.aev"),"module Numbers\nfn half<r: Rep>(x: r) = x / 2\nfn capture<r: Rep>() =\n    let value: r = 1\n    fn () => value\nlet alias = capture\n").unwrap();
    std::fs::write(root.join("main.aev"),"import Numbers as N\nfn reflect<a>(x: a) = type_rep<a>()\nlet half = N.half\nlet exact = half<Int>(3)\nlet factory = N.alias\nlet saved = factory()\nlet value = saved()\nlet schema = reflect(value)\n").unwrap();
    let program = aevia_core::driver::load(&root.join("main.aev"));
    assert!(
        !program.diagnostics.has_errors(),
        "{}",
        program.diagnostics.render(&program.sources)
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        assert_eq!(witness(&mut interp, "schema"), Type::int(), "{mode:?}");
        assert_eq!(
            interp.eval_toplevel("exact").unwrap().as_f64(),
            Some(1.5),
            "{mode:?}"
        );
    }
    std::fs::write(root.join("main.aev"),"import Numbers as N\nlet factory = N.alias\nlet saved = factory()\nlet first: Int = saved()\nlet impossible: Float = saved()\n").unwrap();
    let rejected = aevia_core::driver::load(&root.join("main.aev"));
    assert!(
        rejected.diagnostics.has_errors(),
        "an imported factory's materialized value was generalized"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn private_literal_context_uses_only_connected_independent_inputs() {
    let compilation = checked(
        r#"fn unrelated<r: Rep>(x: r, unused: Real) = x + 1
fn mixed<r: Rep, s: Rep>(x: r, y: s) = x + y + 1
fn reflect<a>(x: a) = type_rep<a>()
let floating = unrelated<Float>(2.0, 3)
let exact = mixed<Float, Real>(2.0, 3)
let floating_type = reflect(floating)
let exact_type = reflect(exact)
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        assert_eq!(
            witness(&mut interp, "floating_type"),
            Type::float(aevia_dim::Dim::one()),
            "{mode:?}"
        );
        assert_eq!(
            witness(&mut interp, "exact_type"),
            Type::real(aevia_dim::Dim::one()),
            "{mode:?}"
        );
        assert_eq!(
            interp.eval_toplevel("floating").unwrap().as_f64(),
            Some(3.0),
            "{mode:?}"
        );
        assert_eq!(
            interp.eval_toplevel("exact").unwrap().as_f64(),
            Some(6.0),
            "{mode:?}"
        );
    }
}

#[test]
fn literal_arithmetic_uses_local_context_without_reinterpreting_a_called_function() {
    let compilation = checked(
        r#"let floating: Float = 0.1 + 2
let exact: Real = 0.1 + 2
let quotient: Real = 0.1 / 2
let nested: Real = (0.1 + 0.2) * 10
let voltage: Float[Voltage] = 1 V + 2.0 V
fn increment(x) = x + 1.0
let promoted = increment(1)
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        for (name, numerator, denominator) in
            [("exact", 21, 10), ("quotient", 1, 20), ("nested", 3, 1)]
        {
            let actual = interp.eval_toplevel(name).unwrap();
            let aevia_core::value::NumKind::Exact(actual) = &actual.as_num().unwrap().kind else {
                panic!("{mode:?} {name} must be exact")
            };
            assert_eq!(
                *actual,
                num_rational::BigRational::new(numerator.into(), denominator.into()),
                "{mode:?} {name}"
            );
        }
        assert!(
            matches!(
                interp
                    .eval_toplevel("floating")
                    .unwrap()
                    .as_num()
                    .unwrap()
                    .kind,
                aevia_core::value::NumKind::Float(_)
            ),
            "{mode:?}"
        );
        assert!(
            matches!(
                interp
                    .eval_toplevel("promoted")
                    .unwrap()
                    .as_num()
                    .unwrap()
                    .kind,
                aevia_core::value::NumKind::Float(_)
            ),
            "{mode:?}"
        );
        assert_eq!(
            interp.eval_toplevel("voltage").unwrap().as_f64(),
            Some(3.0),
            "{mode:?}"
        );
    }
    let rejected = compile(
        "published-callee.aev",
        "fn increment(x) = x + 1.0\nlet impossible: Int = increment(1)\n",
    );
    assert!(
        rejected.diagnostics.has_errors(),
        "a caller changed a published numeric transfer"
    );
}

#[test]
fn actual_arithmetic_representations_match_join_and_divide_contracts() {
    let compilation = checked(
        r#"fn mixed(x: Int, y: Float) = x + y
fn reversed(x: Float, y: Int) = x + y
fn exact_mixed(x: Real, y: Float) = x + y
fn divide(x: Int, y: Int) = x / y
fn inverse(x: Int) = x ^ -1
fn scale(xs: Tensor<1, Float>, n: Int) = xs * n
let left = mixed(2, 0.5)
let right = reversed(0.5, 2)
let rational = exact_mixed(2, 0.5)
let whole_quotient = divide(6, 2)
let whole_inverse = inverse(1)
let scaled = scale([1.0, 2.0], 2)
let literal_default = 1.0 + 2
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        for name in ["left", "right"] {
            let actual = interp.eval_toplevel(name).unwrap();
            assert!(
                matches!(
                    actual.as_num().unwrap().kind,
                    aevia_core::value::NumKind::Float(2.5)
                ),
                "{mode:?} {name}: {actual:?}"
            );
        }
        for (name, numerator, denominator) in [
            ("rational", 5, 2),
            ("whole_quotient", 3, 1),
            ("whole_inverse", 1, 1),
        ] {
            let actual = interp.eval_toplevel(name).unwrap();
            let aevia_core::value::NumKind::Exact(rational) = &actual.as_num().unwrap().kind else {
                panic!("{mode:?} {name}: expected Real, got {actual:?}")
            };
            assert_eq!(
                *rational,
                num_rational::BigRational::new(numerator.into(), denominator.into()),
                "{mode:?} {name}"
            );
        }
        // §3.10.2: an unannotated mixed literal sum is `Float`.
        assert!(
            matches!(
                interp
                    .eval_toplevel("literal_default")
                    .unwrap()
                    .as_num()
                    .unwrap()
                    .kind,
                aevia_core::value::NumKind::Float(value) if value == 3.0
            ),
            "{mode:?} literal_default"
        );
        let Value::Tensor(scaled) = interp.eval_toplevel("scaled").unwrap() else {
            panic!("expected tensor")
        };
        for index in 0..2 {
            assert!(
                matches!(
                    scaled.get_flat(index).unwrap().as_num().unwrap().kind,
                    aevia_core::value::NumKind::Float(_)
                ),
                "{mode:?}: scalar/tensor promotion disagrees"
            );
        }
    }
}

#[test]
fn quotient_inputs_default_before_a_tensor_zero_and_composites_keep_their_origin() {
    let compilation = checked(
        r#"let determinant = det([[1/3, 0], [0, 1]])
let grouped = (1.0 + 2.0) + 3
let direct = 1.0 + 2
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = install(&compilation, mode);
        let determinant = interp.eval_toplevel("determinant").unwrap();
        assert_eq!(aevia_core::value::repr(&determinant), "1/3", "{mode:?}");
        assert!(
            matches!(
                interp
                    .eval_toplevel("grouped")
                    .unwrap()
                    .as_num()
                    .unwrap()
                    .kind,
                aevia_core::value::NumKind::Float(6.0)
            ),
            "{mode:?}"
        );
        assert!(
            matches!(
                interp
                    .eval_toplevel("direct")
                    .unwrap()
                    .as_num()
                    .unwrap()
                    .kind,
                aevia_core::value::NumKind::Float(value) if value == 3.0
            ),
            "{mode:?}"
        );
    }
}
