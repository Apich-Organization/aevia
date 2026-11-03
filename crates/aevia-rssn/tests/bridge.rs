//! The bridge's semantics: overload identity, the dimension mirror, units,
//! and the failure channel.
//!
//! Every test asserts a *property* — the `Value` variant, the `OpKey`, the
//! diagnostic code, the `OpError` variant — and never a substring of a
//! rendered value. A test that looks for `"2"` in `show(x)` passes for `19`; a
//! test that asks whether the value is `Value::Bool` cannot.

use aevia_rssn::{Failure, Num, OpEntry, Sig, SigType, UnitVal, Val, kernel, registry};
use num_bigint::BigInt;
use rssn::kernel::result::{Condition, ConditionKind, Solutions};
use rssn::kernel::signature::DimExpr;
use rssn::kernel::value::{SparseValue, Value};

// ---------------------------------------------------------------------------
// 1. One decision: name plus argument types to a pinned key
// ---------------------------------------------------------------------------

/// `resolve_key` is the checker's entry point: name plus argument types to a
/// pinned key, decided by `kernel::overload`'s lattice and nothing else.
#[test]
fn a_concrete_argument_type_pins_the_numeric_overload() {
    let sin = registry().op("sin").expect("sin is registered");
    let key = aevia_rssn::resolve_key("sin", &[SigType::Real]).expect("unambiguous");
    assert_eq!(key, sin.keys[0], "`Real` is the numeric overload");
    assert_ne!(key, sin.keys[1], "and not the symbolic one");
    assert_eq!(key.rule_set, sin.rule_set);
    assert_eq!(key.name, "sin");
}

/// The lattice refuses a tie instead of picking one. `det` has a numeric
/// overload on `Tensor<2, Real>` and a symbolic one on `Dyn<term>`; a tensor of
/// terms is `Generic` for both, so the honest answer is `E1002`.
#[test]
fn an_ambiguous_overload_is_e1002() {
    let det = registry().op("det").expect("det is registered");
    let types = [SigType::Tensor {
        rank: Some(2),
        elem: Box::new(SigType::Dyn),
    }];
    let error = aevia_rssn::resolve_key("det", &types).expect_err("two overloads tie");
    assert_eq!(error.code, "E1002");
    assert_eq!(error.kind, Failure::Ambiguous);
    assert_eq!(error.operator.as_deref(), Some("det"));
    assert!(
        error.cause.is_none(),
        "a tie is a resolution failure, not a kernel failure"
    );
    assert_eq!(det.keys.len(), 2, "the tie is between two overloads");
}

/// A name that is not in the manifest is `E1003`, not a panic and not a
/// fallback to the dynamic ABI.
#[test]
fn an_unknown_name_is_e1003() {
    let error = aevia_rssn::resolve_key("no_such_operator", &[]).expect_err("unknown");
    assert_eq!(error.code, "E1003");
    assert_eq!(error.kind, Failure::Unknown);
}

/// The same ambiguity, through the runtime entry point: a rank-2 tensor whose
/// elements are kernel terms. The compatibility path resolves once and reports
/// the tie; it never falls back to a second resolver.
#[test]
fn the_runtime_reports_the_ambiguity_instead_of_choosing() {
    let kernel = kernel::kernel();
    let cell = || {
        let handle = kernel
            .term_of(&Val::Symbol("x".to_string()))
            .expect("a symbol has a term form");
        Val::Term(handle)
    };
    let matrix = Val::Tensor {
        shape: vec![2, 2],
        data: vec![cell(), cell(), cell(), cell()],
    };
    let error = kernel
        .resolve("det", &[matrix])
        .expect_err("a tensor of terms ties");
    assert_eq!(error.code, "E1002");
    assert_eq!(error.kind, Failure::Ambiguous);
    assert_eq!(error.operator.as_deref(), Some("det"));
}

/// The keyed path never re-resolves: `det`'s numeric key runs the numeric
/// overload on a numeric matrix, and the answer is the exact determinant.
#[test]
fn a_pinned_key_executes_the_overload_it_names() {
    let det = registry().op("det").expect("det");
    let key = det.keys[0].clone();
    let matrix = Val::Tensor {
        shape: vec![2, 2],
        data: vec![
            Val::Num(Num::Int(BigInt::from(1))),
            Val::Num(Num::Int(BigInt::from(2))),
            Val::Num(Num::Int(BigInt::from(3))),
            Val::Num(Num::Int(BigInt::from(4))),
        ],
    };
    let value = kernel::kernel()
        .call_key(&key, &[matrix])
        .expect("a numeric determinant");
    assert_eq!(
        value,
        Val::Num(Num::Int(BigInt::from(-2))),
        "det([[1,2],[3,4]]) is exactly -2, and it came from the pinned key"
    );
}

/// Pinning is only meaningful when every overload has a key. This is the
/// manifest invariant the whole design rests on, asserted once for the whole
/// table.
#[test]
fn every_overload_has_exactly_one_key() {
    for op in registry().ops() {
        assert_eq!(
            op.keys.len(),
            op.raw.len(),
            "`{}` has {} overloads but {} keys",
            op.name,
            op.raw.len(),
            op.keys.len()
        );
        for (index, key) in op.keys.iter().enumerate() {
            assert_eq!(
                op.key_index(key),
                Some(index),
                "key {index} of `{}` does not name overload {index}",
                op.name
            );
            assert!(
                key.signature_hash.iter().any(|byte| *byte != 0),
                "overload {index} of `{}` has the all-zero hash, which no call can pin",
                op.name
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 2. The dimension mirror
// ---------------------------------------------------------------------------

/// `Real[Any]` says "any dimension". Reading it as `1` turns the loosest
/// signature in the manifest into the strictest type in the language: it would
/// accept a plain `Real` and reject every quantity, which is the opposite of
/// what the kernel promises.
#[test]
fn an_unpinned_dimension_is_not_dimensionless() {
    let any = aevia_rssn::registry::convert_dim(&DimExpr::Any);
    assert!(!any.is_one(), "`Any` is not the dimensionless scalar");
    assert_eq!(any.vars().len(), 1, "it is one dimension variable");

    let another = aevia_rssn::registry::convert_dim(&DimExpr::Any);
    assert_ne!(
        any.vars(),
        another.vars(),
        "two `Any`s in one signature are independent"
    );

    // A dimension variable in the signature is not `1` either.
    let var = aevia_rssn::registry::convert_dim(&DimExpr::Var("D".to_string()));
    assert!(!var.is_one());
    assert_eq!(var.vars().len(), 1);

    // The concrete cases keep their meaning.
    assert_eq!(
        aevia_rssn::registry::convert_dim(&DimExpr::Dimless),
        aevia_dim::Dim::one()
    );
    assert_eq!(
        aevia_rssn::registry::convert_dim(&DimExpr::base("Length")),
        aevia_dim::Dim::base("Length")
    );
    assert_eq!(
        aevia_rssn::registry::convert_dim(&DimExpr::base("Length").pow(
            rssn::kernel::signature::Exp::Rat(rssn::kernel::signature::Rational::new(1, 2))
        )),
        aevia_dim::Dim::base("Length").pow(&num_rational::BigRational::new(
            BigInt::from(1),
            BigInt::from(2)
        ))
    );
}

/// A dimension with a variable survives the round trip to the signature
/// spelling: the base dimensions come back unchanged and the variable comes
/// back as a variable, never as `1`.
#[test]
fn a_dimension_variable_round_trips() {
    let dim = aevia_dim::Dim::base("Length").mul(&aevia_dim::Dim::var(7));
    let expr = aevia_rssn::registry::to_dim_expr(&dim);
    let back = aevia_rssn::registry::convert_dim(&expr);
    assert_eq!(back.base_factors(), dim.base_factors());
    assert!(
        !back.is_concrete(),
        "the variable must survive, not collapse to `1`"
    );
}

/// A signature whose dimension is a variable renders as `[_]`, not as the
/// internal variable id: `Real[d16777216]` is not a type this language can
/// write, and printing it would suggest otherwise.
#[test]
fn an_unpinned_dimension_renders_without_its_variable_id() {
    let ty = SigType::Dim(
        Box::new(SigType::Real),
        aevia_rssn::registry::convert_dim(&DimExpr::Any),
    );
    assert_eq!(aevia_rssn::registry::render_type(&ty), "Real[_]");
}

// ---------------------------------------------------------------------------
// 3. Units and quantities
// ---------------------------------------------------------------------------

fn metre() -> UnitVal {
    let unit = registry().unit("m").expect("the metre");
    UnitVal {
        name: unit.name.clone(),
        display: unit.display.clone(),
        dim: unit.dim.clone(),
        scale: unit.scale.clone(),
        offset: unit.offset.clone(),
        affine: unit.affine,
        prefixable: unit.prefixable,
        category: unit.category.clone(),
    }
}

/// A unit carries its dimension, not just its name.
#[test]
fn a_unit_keeps_its_dimension_across_the_bridge() {
    let unit = metre();
    let session = kernel::kernel().id();
    let value = Val::Unit(unit.clone())
        .to_rssn(session)
        .expect("a unit converts");
    let Value::Unit(back) = &value else {
        panic!("a unit must cross as Value::Unit, not {value:?}");
    };
    assert_eq!(back.dimension, DimExpr::base("Length"));
    let round_trip = UnitVal::from_rssn(back);
    assert_eq!(round_trip.dim, unit.dim);
    assert_eq!(round_trip.scale, unit.scale);
}

/// The reverse direction, which was worse: a unit result used to become
/// `Value::string(name)`, so an operator returning `Unit[d]` produced a `Str`.
#[test]
fn a_unit_result_stays_a_unit() {
    let unit = metre();
    let session = kernel::kernel().id();
    let Value::Unit(kernel_unit) = Val::Unit(unit.clone()).to_rssn(session).expect("converts")
    else {
        panic!("a unit must cross as Value::Unit");
    };
    let back = Val::from_rssn(&Value::Unit(kernel_unit), session).expect("converts back");
    let Val::Unit(back) = back else {
        panic!("a unit result became {back:?}");
    };
    assert_eq!(back.dim, aevia_dim::Dim::base("Length"));
    assert_eq!(back.name, "m");
}

/// `Rssn.quantity(3.0, Units.m)` type-checks, so it has to run — and to come
/// back as a quantity of length, not as text and not as an opaque payload.
#[test]
fn a_quantity_is_a_quantity_in_both_directions() {
    let value = kernel::kernel()
        .call(
            "quantity",
            &[Val::Num(Num::Float(3.0)), Val::Unit(metre())],
        )
        .expect("a quantity is a value the kernel takes");
    let Val::Quantity { value: amount, unit } = &value else {
        panic!("`quantity(3.0, m)` is {value:?}, not a quantity");
    };
    assert_eq!(unit.dim, aevia_dim::Dim::base("Length"));
    assert_eq!(**amount, Val::Num(Num::Float(3.0)));

    // And back: the unit's dimension survives the return trip.
    let session = kernel::kernel().id();
    let kernel_value = value.to_rssn(session).expect("converts");
    let Value::Quantity(quantity) = &kernel_value else {
        panic!("a quantity must cross as Value::Quantity, not {kernel_value:?}");
    };
    assert_eq!(quantity.unit.dimension, DimExpr::base("Length"));
}

// ---------------------------------------------------------------------------
// 4. The failure channel
// ---------------------------------------------------------------------------

/// A handle from another session is `E0899`, the caller can tell it apart from
/// "nothing matched" (`Failure::Stale`), and the diagnostic names the operator.
#[test]
fn a_foreign_handle_is_a_diagnostic_not_a_default_value() {
    let first = kernel::Kernel::new();
    let handle = first
        .term_of(&Val::Symbol("x".to_string()))
        .expect("a symbol has a term form");
    let second = kernel::Kernel::new();
    let error = second
        .diff(handle, "x")
        .expect_err("a handle from another session cannot be used");
    assert_eq!(error.code, "E0899");
    assert_eq!(error.kind, Failure::Stale);
    assert_eq!(error.operator.as_deref(), Some("diff"));
}

/// The same failure when the stale handle is an *argument*: the diagnostic
/// still names the operator that was called, not just the session.
#[test]
fn a_stale_argument_names_the_operator() {
    let first = kernel::Kernel::new();
    let handle = first
        .term_of(&Val::Symbol("x".to_string()))
        .expect("a symbol has a term form");
    let second = kernel::Kernel::new();
    let error = second
        .call("sin", &[Val::Term(handle)])
        .expect_err("a handle from another session cannot be an argument");
    assert_eq!(error.code, "E0899");
    assert_eq!(error.operator.as_deref(), Some("sin"));
}

/// A live handle differentiates to a term; the failure path above is not the
/// only path.
#[test]
fn a_live_handle_differentiates() {
    let kernel = kernel::Kernel::new();
    let handle = kernel
        .term_of(&Val::Symbol("x".to_string()))
        .expect("a symbol has a term form");
    let derivative = kernel.diff(handle, "x").expect("d(x)/dx");
    assert_eq!(derivative, Val::Num(Num::Int(BigInt::from(1))));
}

/// Every kernel call returns a typed error: the operator's name and the
/// kernel's own `OpError` variant travel with the diagnostic, so the runtime
/// can act on the reason instead of reading the message.
#[test]
fn a_kernel_failure_carries_its_operator_and_variant() {
    let kernel = kernel::kernel();

    // `sin` takes one argument; none is an arity failure.
    let error = kernel.call("sin", &[]).expect_err("sin needs an argument");
    assert_eq!(error.operator.as_deref(), Some("sin"));
    assert_eq!(error.variant(), Some("Arity"));
    assert_eq!(error.code, "E1003");

    // A key that names no live overload is the kernel's `UnknownOp`, not a
    // reason to pick another overload.
    let stale = aevia_rssn::OpKey::new("trig", "sin", None);
    let error = kernel
        .call_key(&stale, &[Val::Num(Num::Float(0.0))])
        .expect_err("a key with no hash names no overload");
    assert_eq!(error.variant(), Some("UnknownOp"));
    assert_eq!(error.operator.as_deref(), Some("sin"));
    assert_eq!(error.code, "E1003");
}

/// A failure the kernel reports about itself is `E0899` with
/// [`Failure::Kernel`]: the runtime must stop, not retry with another overload.
#[test]
fn a_kernel_failure_is_e0899_and_is_not_retried() {
    use rssn::kernel::call::OpError;
    let error = aevia_rssn::error::from_op_error(&OpError::Internal("no arena".to_string()))
        .at_operator("integral");
    assert_eq!(error.code, "E0899");
    assert_eq!(error.kind, Failure::Kernel);
    assert_eq!(error.operator.as_deref(), Some("integral"));
    assert_eq!(error.variant(), Some("Internal"));

    // An ordinary user-level failure keeps its own code even when the kernel's
    // variant is recorded.
    let error = aevia_rssn::error::from_op_error(&OpError::BudgetExceeded).at_operator("solve");
    assert_eq!(error.code, "E0808");
    assert_eq!(error.variant(), Some("BudgetExceeded"));
}

// ---------------------------------------------------------------------------
// 5. Lossy conversions that could be fixed
// ---------------------------------------------------------------------------

/// A sparse entry stored outside the shape used to be dropped on the floor,
/// which silently changed the matrix. It is a kernel inconsistency now.
#[test]
fn a_sparse_entry_outside_the_shape_is_an_error() {
    let sparse = Value::Sparse(SparseValue {
        shape: vec![2, 2],
        format: rssn::kernel::value::SparseFormat::Csr,
        indices: vec![0, 9],
        data: vec![Value::Float(1.0), Value::Float(2.0)],
    });
    let error = Val::from_rssn(&sparse, kernel::kernel().id())
        .expect_err("index 9 is outside a 2x2 matrix");
    assert_eq!(error.code, "E0809");
    assert_eq!(error.kind, Failure::Kernel);
}

/// The zeros a densified sparse matrix is filled with carry the element type:
/// a `Float` matrix must not come back with `Int` zeros.
#[test]
fn a_sparse_zero_carries_the_element_type() {
    let sparse = Value::Sparse(SparseValue {
        shape: vec![2],
        format: rssn::kernel::value::SparseFormat::Csr,
        indices: vec![1],
        data: vec![Value::Float(2.5)],
    });
    let dense = Val::from_rssn(&sparse, kernel::kernel().id()).expect("a well-formed sparse value");
    let Val::Tensor { shape, data } = dense else {
        panic!("a sparse value densifies into a tensor");
    };
    assert_eq!(shape, vec![2]);
    assert_eq!(data[0], Val::Num(Num::Float(0.0)), "the zero is a float");
    assert_eq!(data[1], Val::Num(Num::Float(2.5)));
}

/// An integer too large for `f64` is not an infinite number, and the bridge
/// can now say so.
#[test]
fn an_integer_too_large_for_f64_is_not_a_value_of_infinity() {
    let huge = Num::Int(BigInt::from(10).pow(400));
    assert_eq!(huge.try_to_f64(), None, "10^400 has no f64");
    assert!(huge.to_f64().is_infinite(), "the lossy form saturates");

    let negative = Num::Int(-BigInt::from(10).pow(400));
    assert_eq!(negative.try_to_f64(), None);
    assert_eq!(negative.to_f64(), f64::NEG_INFINITY);

    let small = Num::Int(BigInt::from(3));
    assert_eq!(small.try_to_f64(), Some(3.0));
}

// ---------------------------------------------------------------------------
// 6. Capabilities
// ---------------------------------------------------------------------------

/// A gap in the build is `E1001` naming the capability, not a generic type
/// error. `core.jit` is off in this build, which makes it a real gap.
#[test]
fn a_missing_capability_names_itself() {
    let capabilities = &registry().capabilities;
    let error = capabilities.require("core.jit").expect_err("jit is off");
    assert_eq!(error.code, "E1001");
    assert_eq!(error.kind, Failure::Capability);
    assert!(
        error.message.contains("core.jit"),
        "the message must name the capability: {}",
        error.message
    );
    assert!(capabilities.require("core.dynamic_call").is_ok());
}

/// The requirement is read from the operator, not from a list of names: an
/// operator that takes a callback cannot run in a build without
/// `core.higher_order`, and the diagnostic says which capability is missing.
#[test]
fn an_operator_needing_a_missing_capability_is_e1001() {
    let mut entry = registry().op("sin").expect("sin").clone();
    entry.higher_order = true;

    let without = aevia_rssn::Capabilities {
        higher_order: false,
        ..aevia_rssn::Capabilities::default()
    };
    let error = without.require_op(&entry).expect_err("no callbacks here");
    assert_eq!(error.code, "E1001");
    assert_eq!(error.kind, Failure::Capability);
    assert_eq!(error.operator.as_deref(), Some("sin"));
    assert!(error.message.contains("core.higher_order"));
    assert_eq!(without.missing_for(&entry), Some("core.higher_order"));

    // The build that has the capability accepts the same operator, and a
    // build with everything on has nothing to say about a plain operator.
    let full = registry().capabilities.clone();
    assert_eq!(full.missing_for(&entry), None);
    assert!(full.require_op(&entry).is_ok());
}

/// A signature that returns a structured record needs `core.structured_results`
/// in the same way.
#[test]
fn a_structured_result_needs_its_capability() {
    let entry = OpEntry {
        name: "solve_structured".to_string(),
        rule_set: "algebra".to_string(),
        keys: Vec::new(),
        signatures: vec![Sig {
            params: Vec::new(),
            ret: SigType::Record(vec![("roots".to_string(), SigType::Real)]),
            variadic: None,
        }],
        raw: Vec::new(),
        source: aevia_rssn::Source::Derived,
        stability: rssn::kernel::signature::Stability::Stable,
        doc: String::new(),
        arity: (0, Some(0)),
        higher_order: false,
    };
    let without = aevia_rssn::Capabilities {
        structured_results: false,
        ..aevia_rssn::Capabilities::default()
    };
    assert_eq!(
        without.missing_for(&entry),
        Some("core.structured_results")
    );
    assert!(without.require_op(&entry).is_err());
    assert!(registry().capabilities.require_op(&entry).is_ok());
}

/// One table drives `has`, `Rssn.Capabilities` and the generated flags, so a
/// name cannot be known to one and unknown to another.
#[test]
fn the_capability_table_is_the_single_source() {
    let capabilities = &registry().capabilities;
    for row in capabilities.table() {
        assert_eq!(
            capabilities.has(&row.name),
            row.enabled,
            "`has` disagrees with the table about `{}`",
            row.name
        );
        assert!(
            row.name.starts_with("core.") || row.name.starts_with("dim."),
            "`{}` is not a capability name",
            row.name
        );
    }
    assert_eq!(
        capabilities.names(),
        capabilities
            .table()
            .into_iter()
            .filter(|row| row.enabled)
            .map(|row| row.name)
            .collect::<Vec<_>>()
    );
    // The plugin kinds are rows too, so a new `Features` field cannot be
    // invisible to the generated module.
    for kind in &capabilities.plugin_kinds {
        assert!(
            capabilities.has(&format!("core.plugin.{kind}")),
            "plugin kind `{kind}` has no row"
        );
    }
}

// ---------------------------------------------------------------------------
// 7. The variadic ABI
// ---------------------------------------------------------------------------

/// A variadic operator is typed `[α] -> R` and takes N positional arguments at
/// the kernel. The bridge has to say so, because the runtime's spread and the
/// generated `concat([...], rest)` both key on this flag.
#[test]
fn a_variadic_operator_takes_its_arguments_positionally() {
    let and = registry().op("and").expect("and is registered");
    assert!(and.is_variadic(), "`and` takes a list of booleans");
    assert!(and.accepts(2) && and.accepts(0));
    let signature = and.primary().expect("a signature");
    assert_eq!(signature.ret, SigType::Bool, "`and` answers a Bool");
    let (min, max, _) = signature.variadic.as_ref().expect("a variadic tail");
    assert_eq!((*min, *max), (0, None));

    // The kernel, given the spread arguments, answers a `Bool` — the type the
    // checker promised.
    let value = kernel::kernel()
        .call("and", &[Val::Bool(true), Val::Bool(false)])
        .expect("`and` of two booleans");
    assert_eq!(value, Val::Bool(false));
}

/// A fixed-arity operator is not variadic, so the runtime must not spread it.
#[test]
fn a_fixed_arity_operator_is_not_variadic() {
    let sin = registry().op("sin").expect("sin");
    assert!(!sin.is_variadic());
    assert_eq!(sin.arity, (1, Some(1)));
}

// ---------------------------------------------------------------------------
// 8. `Solutions` crosses as the record of §8.6
// ---------------------------------------------------------------------------

/// A `Solutions` built by hand with every field populated survives the trip
/// unchanged. Flattening it to a root list used to lose `multiplicities`,
/// `conditions` and `complete`; the record carries all five fields, and the
/// round trip is exact — including multiplicities that are *reported* (`2, 1`)
/// rather than inferred from a repeated entry.
#[test]
fn a_solution_set_round_trips_through_the_bridge() {
    let solutions = Solutions {
        variable: Some(String::from("x")),
        roots: vec![Value::int(1_i32), Value::int(-1_i32)],
        multiplicities: vec![2, 1],
        conditions: vec![
            Condition::new(ConditionKind::NonZero, "a != 0")
                .with_expression(Value::Symbol(String::from("a"))),
        ],
        complete: true,
    };
    let kernel = kernel::kernel();
    let value =
        Val::from_solutions(&solutions, kernel.id()).expect("a solution set is a record");
    let Val::Record(fields) = &value else {
        panic!("a `Solutions` must cross as a record, found {value:?}");
    };
    assert_eq!(
        fields
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["variable", "roots", "multiplicities", "conditions", "complete"],
        "the field names are §8.6's, in declaration order"
    );
    let back = value
        .to_solutions(kernel.id())
        .expect("the record is a `Solutions`");
    assert_eq!(back, solutions);
    assert_eq!(back.multiplicity(0), 2);
    assert_eq!(back.multiplicity(1), 1);
}

/// "No multiplicities reported" is not "every multiplicity is one": an empty
/// list stays empty across the bridge, so the runtime can keep the
/// distinction §8.6 draws.
#[test]
fn an_unreported_multiplicity_stays_empty() {
    let solutions = Solutions {
        variable: Some(String::from("x")),
        roots: vec![Value::int(1_i32)],
        multiplicities: Vec::new(),
        conditions: Vec::new(),
        complete: false,
    };
    let kernel = kernel::kernel();
    let value = Val::from_solutions(&solutions, kernel.id()).expect("a record");
    let back = value.to_solutions(kernel.id()).expect("a `Solutions`");
    assert!(back.multiplicities.is_empty(), "empty must not become `1`");
    assert!(!back.complete);
    assert_eq!(back.multiplicity(0), 1, "the accessor's default is not a claim");
}

/// A value that is not a `Solutions` record is refused, not read as one.
#[test]
fn a_value_that_is_not_a_solution_set_is_refused() {
    let kernel = kernel::kernel();
    let error = Val::list(vec![])
        .to_solutions(kernel.id())
        .expect_err("a list of roots is not a `Solutions`");
    assert_eq!(error.code, "E0809");
    assert_eq!(error.kind, Failure::Kernel);
}

/// `solve_complex` has no structured entry point in the kernel, so the bridge
/// applies the kernel's classification to its answer. It must still produce
/// the same five-field record, with the unknown named.
#[test]
fn a_complex_solution_set_is_the_same_record() {
    use aevia_rssn::Expr;

    let kernel = kernel::kernel();
    let x = Val::Symbol(String::from("x"));
    let equation = kernel
        .build(&Expr::App(
            String::from("add"),
            vec![
                Expr::App(
                    String::from("pow"),
                    vec![Expr::Sym(String::from("x")), Expr::Num(Num::Int(BigInt::from(2)))],
                ),
                Expr::Num(Num::Int(BigInt::from(1))),
            ],
        ))
        .expect("x^2 + 1 builds");
    let record = kernel
        .solve_structured("solve_complex", &equation, &x)
        .expect("the complex roots of x^2 + 1");
    let solutions = record
        .to_solutions(kernel.id())
        .expect("the answer is a `Solutions`");
    assert_eq!(solutions.variable.as_deref(), Some("x"));
    assert!(!solutions.roots.is_empty(), "x^2 + 1 has complex roots");
}
