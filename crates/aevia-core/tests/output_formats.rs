//! Output formats retain structure, dimensions and rendering errors.

use std::rc::Rc;

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::display::Format;
use aevia_core::display::render;
use aevia_core::sym::Term;
use aevia_core::value::ExactRep;
use aevia_core::value::NumVal;
use aevia_core::value::RecordValue;
use aevia_core::value::Storage;
use aevia_core::value::TensorData;
use aevia_core::value::Value;
use aevia_dim::Dim;
use aevia_dim::Scale;
use aevia_dim::UnitDef;
use aevia_rssn::Expr;
use aevia_rssn::Num;
use aevia_rssn::Sym;
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_rational::BigRational;

fn rational(
    n: i64,
    d: i64,
) -> BigRational {
    BigRational::new(BigInt::from(n), BigInt::from(d))
}

fn exact(
    n: i64,
    d: i64,
) -> Value {
    Value::Num(NumVal::exact(rational(n, d), Dim::one()))
}

#[test]
fn exact_fractions_and_symbolic_matrices_have_real_multiline_layout() {
    let fraction = exact(1, 3);
    assert_eq!(render(&fraction, Format::Latex).unwrap(), r"\frac{1}{3}");
    assert_eq!(render(&fraction, Format::Typst).unwrap(), "frac(1, 3)");
    let pretty = render(&fraction, Format::Pretty).unwrap();
    assert!(pretty.contains('\n'), "{pretty}");
    assert!(pretty.contains('─'), "{pretty}");
    let x = Value::Num(NumVal::symbolic(Term::Sym("x#17".into()), Dim::one()));
    let matrix = Value::Tensor(Rc::new(TensorData::new(
        vec![2, 2],
        Storage::Boxed(vec![fraction, x, Value::int(12), Value::int(4)]),
        Dim::one(),
        None,
    )));
    let pretty = render(&matrix, Format::Pretty).unwrap();
    assert!(pretty.contains('\n') && pretty.contains('─'), "{pretty}");
    assert!(
        pretty.contains("x#17"),
        "symbol identity must reach the structured renderer: {pretty}"
    );
    assert!(pretty.contains("12"), "{pretty}");
    let latex = render(&matrix, Format::Latex).unwrap();
    assert!(
        latex.contains(r"\begin{pmatrix}") && latex.contains(r"\frac{1}{3}"),
        "{latex}"
    );
}

#[test]
fn sliced_and_reversed_matrix_views_keep_mathematical_layout() {
    let backing = TensorData::new(
        vec![3, 3],
        Storage::Dense(Rc::new((1..=9).map(f64::from).collect())),
        Dim::one(),
        None,
    );
    let view = backing.slice_view(&[(0, 2), (1, 3)]).unwrap();
    let reversed = backing.slice_step_view(&[(1, 2, -1), (1, 2, -1)]).unwrap();
    for view in [view, reversed] {
        assert_eq!(view.storage.len(), 4);
        let pretty = render(&Value::Tensor(Rc::new(view)), Format::Pretty).unwrap();
        assert!(pretty.contains('⎛') && pretty.contains('⎠'), "{pretty}");
        assert_eq!(pretty.lines().count(), 2, "{pretty}");
    }
}

#[test]
fn packed_math_matches_boxed_and_keeps_storage_errors() {
    let length = Dim::base("Length");
    let make = |storage| {
        Value::Tensor(Rc::new(TensorData::new(
            vec![2, 2],
            storage,
            length.clone(),
            None,
        )))
    };
    let fractions = vec![
        rational(1, 3),
        rational(-2, 7),
        rational(4, 1),
        rational(0, 1),
    ];
    let integers = vec![
        rational(9_007_199_254_740_993, 1),
        rational(-2, 1),
        rational(4, 1),
        rational(0, 1),
    ];
    for storage in [
        Storage::Dense(Rc::new(vec![-0.0, 2.0, f64::INFINITY, 4.0])),
        Storage::Exact(fractions.clone()),
        Storage::TypedExact(fractions.clone(), ExactRep::Real),
        Storage::TypedExact(integers, ExactRep::Int),
    ] {
        let packed = make(storage);
        let Value::Tensor(tensor) = &packed else {
            unreachable!()
        };
        let boxed = make(Storage::Boxed(tensor.elements()));
        assert_eq!(
            render(&packed, Format::Pretty).unwrap(),
            render(&boxed, Format::Pretty).unwrap(),
        );
    }

    for malformed in [
        make(Storage::Dense(Rc::new(vec![1.0, 2.0, 3.0]))),
        make(Storage::TypedExact(fractions, ExactRep::Int)),
        make(Storage::Boxed(vec![
            Value::float(1.0),
            Value::string("ordinary"),
            Value::float(3.0),
            Value::float(4.0),
        ])),
    ] {
        assert_eq!(
            render(&malformed, Format::Pretty),
            render(&malformed, Format::Show)
        );
    }
    let oversized = make(Storage::Dense(Rc::new(vec![1.0, 2.0, 3.0, 4.0, 5.0])));
    assert_eq!(
        render(&oversized, Format::Pretty).unwrap_err(),
        "tensor shape and storage disagree"
    );
}

#[test]
fn scientific_and_non_finite_floats_use_target_math_not_source_identifiers() {
    assert_eq!(
        render(&Value::float(1e20), Format::Latex).unwrap(),
        r"1 \times 10^{20}"
    );
    assert_eq!(
        render(&Value::float(1e-9), Format::Typst).unwrap(),
        "1 times 10^(-9)"
    );
    assert_eq!(
        render(&Value::float(f64::INFINITY), Format::Latex).unwrap(),
        r"\infty"
    );
    assert_eq!(
        render(&Value::float(f64::NEG_INFINITY), Format::Typst).unwrap(),
        "-oo"
    );
    assert_eq!(
        render(&Value::float(f64::NAN), Format::Latex).unwrap(),
        r"\mathrm{NaN}"
    );
    assert_eq!(
        render(&Value::float(f64::NAN), Format::Typst).unwrap(),
        "upright(\"NaN\")"
    );
    assert_eq!(render(&Value::float(1e20), Format::Show).unwrap(), "1e20");
}

#[test]
fn repr_preserves_float_phase_exactness_and_symbol_identity_after_pretty() {
    for value in [
        Value::float(3.0),
        exact(1, 3),
        Value::Num(NumVal::symbolic(Term::Sym("x#42".into()), Dim::one())),
    ] {
        let before = aevia_core::value::repr(&value);
        let _ = render(&value, Format::Pretty).unwrap();
        assert_eq!(render(&value, Format::Repr).unwrap(), before);
    }
    assert_eq!(render(&Value::float(3.0), Format::Repr).unwrap(), "3.0");
    assert_eq!(render(&exact(1, 3), Format::Repr).unwrap(), "1/3");
}

#[test]
fn pretty_uses_correct_canonical_coefficients_for_scaled_and_affine_units() {
    let length = Dim::base("Length");
    let km = UnitDef::linear("km", length.clone(), Scale::rational(rational(1000, 1)));
    let distance = Value::Num(NumVal::exact(rational(3000, 1), length).with_display(Rc::new(km)));
    assert_eq!(render(&distance, Format::Show).unwrap(), "3 km");
    assert_eq!(render(&distance, Format::Pretty).unwrap(), "3000 m");
    let temperature = Dim::base("Temperature");
    let mut celsius = UnitDef::linear("degC", temperature.clone(), Scale::one());
    celsius.offset = Some(Scale::rational(rational(27315, 100)));
    celsius.affine = true;
    celsius.display = "°C".into();
    let temperature =
        Value::Num(NumVal::exact(rational(29315, 100), temperature).with_display(Rc::new(celsius)));
    assert_eq!(render(&temperature, Format::Show).unwrap(), "20 °C");
    assert_eq!(
        render(&temperature, Format::Pretty).unwrap(),
        format!(
            "{}\nunit: K",
            render(&exact(29315, 100), Format::Pretty).unwrap()
        )
    );
    let typst = render(&temperature, Format::Typst).unwrap();
    assert_eq!(typst, "frac(5863, 20) thin upright(\"K\")");
    assert!(!typst.contains('\\'));
}

#[test]
fn text_and_record_labels_are_escaped_in_the_target_syntax() {
    let text = Value::string("cost_$ & {x} \\\"\nnext");
    let latex = render(&text, Format::Latex).unwrap();
    assert!(latex.contains(r"cost\_\$ \& \{x\}"), "{latex}");
    assert!(latex.contains(r"\textbackslash{}"), "{latex}");
    let typst = render(&text, Format::Typst).unwrap();
    assert!(typst.contains(r#"\\\"\nnext"#), "{typst}");
    let record = Value::Record(Rc::new(RecordValue {
        fields: IndexMap::from([("rate_\"$".into(), Value::int(1))]),
    }));
    assert!(
        render(&record, Format::Latex)
            .unwrap()
            .contains(r#"rate\_"\$"#)
    );
    assert!(
        render(&record, Format::Typst)
            .unwrap()
            .contains(r#""rate_\"$""#)
    );
    assert_eq!(
        render(&Value::string("hello"), Format::Pretty).unwrap(),
        "hello"
    );
}

#[test]
fn foreign_terms_use_authenticated_structure_instead_of_cached_display_text() {
    let kernel = aevia_rssn::kernel();
    let value = kernel
        .build(&Expr::App(
            "add".into(),
            vec![
                Expr::Sym("x#42".into()),
                Expr::Num(Num::Int(BigInt::from(1))),
            ],
        ))
        .unwrap();
    let handle = kernel.term_of(&value).unwrap();
    let inverse = Term::Pow(
        Box::new(Term::foreign(handle, "deliberately not source")),
        rational(-1, 1),
    );
    let value = Value::Num(NumVal::symbolic(inverse, Dim::one()));
    let latex = render(&value, Format::Latex).unwrap();
    assert!(
        latex.contains(r"\frac") && latex.contains(r"\#42"),
        "{latex}"
    );
    assert!(!latex.contains("deliberately"), "{latex}");
    let typst = render(&value, Format::Typst).unwrap();
    assert!(typst.contains("frac(") && typst.contains("x#42"), "{typst}");
    let stale = Value::Num(NumVal::symbolic(
        Term::foreign(Sym::new(u64::MAX, 0), "old"),
        Dim::one(),
    ));
    assert!(render(&stale, Format::Latex).is_err());
    assert!(render(&stale, Format::Pretty).is_err());
}

#[test]
fn scalar_tensors_have_a_scalar_typeset_form() {
    let scalar = Value::Tensor(Rc::new(TensorData::new(
        vec![],
        Storage::Boxed(vec![Value::int(7)]),
        Dim::one(),
        None,
    )));
    assert_eq!(render(&scalar, Format::Latex).unwrap(), "7");
    assert_eq!(render(&scalar, Format::Typst).unwrap(), "7");
}

#[test]
fn language_output_utilities_work_in_normal_and_optimized_ir_modes() {
    let source = r#"
let fraction: Real = 1 / 3
let pretty = to_pretty(fraction)
let latex = to_latex(fraction)
let typst = to_typst(fraction)
let function = fn(x: Int) => x
let action = println("must not run while rendering")
"#;
    for optimized in [false, true] {
        let compilation = compile("output.aev", source);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let mut interp = Interp::new(&compilation.checked)
            .with_mode(Mode::Ir)
            .with_buffer();
        interp.optimize_ir = optimized;
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
        assert!(aevia_core::value::show(&interp.eval_toplevel("pretty").unwrap()).contains('─'));
        assert_eq!(
            aevia_core::value::show(&interp.eval_toplevel("latex").unwrap()),
            r"\frac{1}{3}"
        );
        assert_eq!(
            aevia_core::value::show(&interp.eval_toplevel("typst").unwrap()),
            "frac(1, 3)"
        );
        for name in ["function", "action"] {
            let value = interp.eval_toplevel(name).unwrap();
            for format in [Format::Pretty, Format::Latex, Format::Typst] {
                assert!(render(&value, format).is_err());
            }
        }
        assert_eq!(interp.captured(), "");
    }
}
