//! The bridge's semantics: overload identity, the dimension mirror, units,
//! and the failure channel.
//!
//! Every test asserts a *property* — the `Value` variant, the `OpKey`, the
//! diagnostic code, the `OpError` variant — and never a substring of a
//! rendered value. A test that looks for `"2"` in `show(x)` passes for `19`; a
//! test that asks whether the value is `Value::Bool` cannot.

use aevia_rssn::Failure;
use aevia_rssn::Num;
use aevia_rssn::OpEntry;
use aevia_rssn::Sig;
use aevia_rssn::SigType;
use aevia_rssn::UnitVal;
use aevia_rssn::Val;
use aevia_rssn::kernel;
use aevia_rssn::registry;
use num_bigint::BigInt;
use rssn::kernel::result::Condition;
use rssn::kernel::result::ConditionKind;
use rssn::kernel::result::Solutions;
use rssn::kernel::signature::DimExpr;
use rssn::kernel::value::SparseValue;
use rssn::kernel::value::Value;

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
        shape: None,
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

/// The SDE noise operators the kernel added are typed, not dynamic: the
/// overlay fixes their dimensions, so the bridge's generated module can declare
/// `sde_wiener` with a `sqrt(T)` result and `sde_levy_areas` with a length-`T`
/// result. `sde_wiener(seed, m, steps, h)` roots the fourth argument (the step)
/// — that is the kernel's `DimRule::RootOf`, whose type-level face is the
/// `^(1/2)` in the return element; the bridge mirrors the signature and does not
/// re-derive dimension rules.
#[test]
fn the_sde_noise_operators_carry_their_sqrt_of_time_types() {
    use aevia_rssn::SigType;
    use aevia_rssn::registry;
    let registry = registry();
    let text = aevia_rssn::codegen::operators(registry);

    let wiener = registry.op("sde_wiener").expect("sde_wiener is registered");
    assert_eq!(wiener.source, aevia_rssn::Source::Overlay);
    assert_eq!(wiener.signatures.len(), 1, "one overload");
    assert_eq!(
        wiener.keys.len(),
        wiener.signatures.len(),
        "one key per overload"
    );
    let SigType::Tensor {
        rank: Some(2), elem, ..
    } = &wiener.signatures[0].ret
    else {
        panic!(
            "`sde_wiener` must return a rank-2 tensor: {:?}",
            wiener.signatures[0]
        );
    };
    let SigType::Dim(inner, dim) = &**elem else {
        panic!("`sde_wiener` elements must be dimensioned reals: {elem:?}");
    };
    assert!(
        matches!(&**inner, SigType::Real | SigType::Float),
        "the element base must be real: {inner:?}"
    );
    assert_eq!(
        dim.to_plain_string().matches("^(1/2)").count(),
        1,
        "a Brownian increment over `h` is `sqrt(h)`: {}",
        dim.to_plain_string()
    );
    assert_eq!(dim.vars().len(), 1, "one dimension variable");
    assert!(
        text.contains("Rssn.invoke(\"sde_wiener\", "),
        "`sde_wiener` has no generated declaration"
    );

    let areas = registry
        .op("sde_levy_areas")
        .expect("sde_levy_areas is registered");
    assert_eq!(areas.source, aevia_rssn::Source::Overlay);
    assert_eq!(areas.signatures.len(), 1);
    assert_eq!(areas.keys.len(), areas.signatures.len());
    let SigType::Tensor {
        rank: Some(1), elem, ..
    } = &areas.signatures[0].ret
    else {
        panic!(
            "`sde_levy_areas` must return a rank-1 tensor: {:?}",
            areas.signatures[0]
        );
    };
    let SigType::Dim(_, dim) = &**elem else {
        panic!("`sde_levy_areas` elements must be dimensioned reals: {elem:?}");
    };
    assert!(
        !dim.to_plain_string().contains('^'),
        "an iterated integral `I_jl` has the dimension of the step, not its root: {}",
        dim.to_plain_string()
    );
    assert!(
        text.contains("Rssn.invoke(\"sde_levy_areas\", "),
        "`sde_levy_areas` has no generated declaration"
    );
}

/// The kernel's SDE scheme introspection is typed too: `sde_scheme_info(name)`
/// answers a `Tensor<1, Real>` of scheme flags, with a `String` argument and an
/// overlay signature, so the generated `Rssn` module declares it rather than
/// reaching it through `Rssn.dynamic`. It is *complementary* to §8.9.1's
/// `sde_order` (which answers strong/weak order for a language method value):
/// the kernel reports its scheme flags, the stdlib reports the language's
/// order table, and neither wraps the other.
#[test]
fn sde_scheme_info_is_a_typed_overlay_operator() {
    use aevia_rssn::SigType;
    use aevia_rssn::registry;
    let registry = registry();
    let op = registry
        .op("sde_scheme_info")
        .expect("sde_scheme_info is registered");
    assert_eq!(op.source, aevia_rssn::Source::Overlay);
    assert_eq!(op.signatures.len(), 1, "one overload");
    assert_eq!(op.keys.len(), op.signatures.len(), "one key per overload");
    let signature = &op.signatures[0];
    assert_eq!(
        signature.params,
        vec![SigType::Str],
        "the scheme name is a String"
    );
    assert_eq!(
        signature.ret,
        SigType::Tensor {
            rank: Some(1),
            shape: None,
            elem: Box::new(SigType::Real),
        },
        "the flags are a rank-1 tensor of exact reals"
    );
    let text = aevia_rssn::codegen::operators(registry);
    assert!(
        text.contains("Rssn.invoke(\"sde_scheme_info\", "),
        "`sde_scheme_info` has no generated declaration"
    );
    // The live manifest is the one the registry mirrors; the generated module
    // states its hash (the test above pins that connection).
    assert!(
        registry.op_count() > 1300,
        "the manifest's operators are live"
    );
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

/// An exact integral value crosses as the kernel's `Int`, never as a
/// non-normal `Rat` with denominator one.
///
/// The kernel's `Number` invariant is "a `Rat` has denominator > 1"
/// (`Number::rat` collapses integral rationals); a `Rat(n/1)` misses structural
/// rewrite patterns such as `cos(0) => 1`, because the pattern is `Int(0)`. The
/// runtime's literal coercion can produce an exact `0` from a `Real` context, so
/// this bridge is exactly where the bad form used to be created.
#[test]
fn an_integral_exact_value_crosses_in_normal_form() {
    use num_rational::BigRational;
    use rssn::graph::Number;

    let kernel = kernel::kernel();
    let two = Val::Num(Num::Rat(BigRational::from_integer(BigInt::from(2))));
    assert_eq!(
        two.to_rssn(kernel.id()).expect("an exact number"),
        Value::Int(BigInt::from(2)),
        "a whole rational is `Value::Int`, not `Value::Real(Rat(2/1))`"
    );
    let third = Val::Num(Num::Rat(BigRational::new(BigInt::from(1), BigInt::from(3))));
    assert_eq!(
        third.to_rssn(kernel.id()).expect("an exact number"),
        Value::Real(Number::Rat(BigRational::new(
            BigInt::from(1),
            BigInt::from(3)
        ))),
        "a genuine fraction stays an exact rational"
    );
    assert_eq!(
        Num::Rat(BigRational::from_integer(BigInt::from(-7))).to_number(),
        Number::Int(BigInt::from(-7)),
        "`Num::to_number` keeps the same normal form"
    );
}

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
        .call("quantity", &[Val::Num(Num::Float(3.0)), Val::Unit(metre())])
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

/// A request the kernel cannot reduce is `E0807`, not "the operator failed":
/// the difference is what tells a caller to try a numeric path (`nsolve`,
/// `approx`) instead of reporting a bug.
#[test]
fn an_unreduced_request_is_e0807() {
    use rssn::kernel::call::OpError;
    let error = aevia_rssn::error::from_op_error(&OpError::NotReduced("integral".to_string()))
        .at_operator("integral");
    assert_eq!(error.code, "E0807");
    assert_eq!(error.kind, Failure::Other);
    assert_eq!(error.variant(), Some("NotReduced"));
    assert_eq!(error.operator.as_deref(), Some("integral"));
    assert!(
        error.help.is_some(),
        "the diagnostic must suggest the numeric path"
    );
}

/// `Rssn.call_ho` with no callbacks is the dynamic call; the bridge method must
/// answer exactly what the name-based call answers.
#[test]
fn the_callback_free_call_ho_is_the_dynamic_call() {
    let kernel = kernel::kernel();
    let direct = kernel
        .call("sin", &[Val::Num(Num::Float(0.5))])
        .expect("`sin(0.5)`");
    let higher_order = kernel
        .call_ho("sin", &[Val::Num(Num::Float(0.5))])
        .expect("a callback-free `call_ho`");
    assert_eq!(
        direct, higher_order,
        "with an empty callback list the two kernel entry points are one call"
    );
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

/// A sparse value crosses the bridge as it is: nothing is densified, so there
/// are no zeros to give the wrong element type, and the stored entries keep
/// theirs.
#[test]
fn a_sparse_value_crosses_without_being_densified() {
    let sparse = Value::Sparse(SparseValue {
        shape: vec![2],
        format: rssn::kernel::value::SparseFormat::Csr,
        indices: vec![1],
        data: vec![Value::Float(2.5)],
    });
    let crossed =
        Val::from_rssn(&sparse, kernel::kernel().id()).expect("a well-formed sparse value");
    let Val::Sparse {
        format,
        shape,
        indices,
        data,
    } = &crossed
    else {
        panic!("a sparse value stays sparse");
    };
    assert_eq!(*format, aevia_rssn::SparseKind::Csr);
    assert_eq!(shape, &vec![2]);
    assert_eq!(indices, &vec![1]);
    assert_eq!(data, &vec![Val::Num(Num::Float(2.5))]);
    let back = crossed
        .to_rssn(kernel::kernel().id())
        .expect("converts back");
    assert_eq!(back, sparse, "the round trip is the identity");
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
/// error. A deliberately disabled `core.jit` makes the test feature-independent.
#[test]
fn a_missing_capability_names_itself() {
    let mut capabilities = registry().capabilities.clone();
    // Exercise the capability gap independently of Cargo feature unification.
    capabilities.jit = false;
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

/// The kernel's SDE noise primitives are a kernel capability (`Features.sde`,
/// task-12), so the single capability table carries a `core.sde` row with the
/// `sde` binding, and the generated `Rssn` module declares it. A build without
/// the rule set flips one row instead of losing the name in two consumers.
#[test]
fn the_sde_capability_is_a_row_and_a_binding() {
    let capabilities = &registry().capabilities;
    let row = capabilities
        .table()
        .into_iter()
        .find(|row| row.name == "core.sde")
        .expect("the capability table has a `core.sde` row");
    assert_eq!(row.binding, "sde", "`Rssn.sde` is the binding");
    assert_eq!(
        row.enabled,
        kernel::kernel().manifest().features.sde,
        "the row is the manifest's `Features.sde`"
    );
    assert_eq!(row.enabled, capabilities.has("core.sde"));
    let text = aevia_rssn::codegen::operators(registry());
    assert!(
        text.contains(&format!("let sde: Bool = {}", row.enabled)),
        "`Rssn.aev` must declare the capability binding"
    );
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
        dim_rule: None,
        dynamic_reason: None,
        eval_kind: rssn::kernel::signature::EvalKind::None,
        backend_support: rssn::kernel::signature::BackendSupport::default(),
        stability: rssn::kernel::signature::Stability::Stable,
        doc: String::new(),
        arity: (0, Some(0)),
        higher_order: false,
    };
    let without = aevia_rssn::Capabilities {
        structured_results: false,
        ..aevia_rssn::Capabilities::default()
    };
    assert_eq!(without.missing_for(&entry), Some("core.structured_results"));
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

/// Scoped callbacks and source-level plugins (§13.5.2) are both available,
/// as two distinct rows of the one capability table.
#[test]
fn callback_and_source_plugin_capabilities_are_distinct() {
    let capabilities = &registry().capabilities;
    assert!(capabilities.has("core.callback"));
    let row = capabilities
        .table()
        .into_iter()
        .find(|row| row.name == "core.plugin.source")
        .expect("`core.plugin.source` has a row in the capability table");
    assert!(row.enabled, "source plugins need only in-process plugins");
    assert_eq!(
        capabilities.has("core.plugin.source"),
        capabilities.has("core.plugin.in-process")
    );
    assert!(capabilities.has("core.plugin"));
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
    let value = Val::from_solutions(&solutions, kernel.id()).expect("a solution set is a record");
    let Val::Record(fields) = &value else {
        panic!("a `Solutions` must cross as a record, found {value:?}");
    };
    assert_eq!(
        fields
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        [
            "variable",
            "roots",
            "multiplicities",
            "conditions",
            "complete"
        ],
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
    assert_eq!(
        back.multiplicity(0),
        1,
        "the accessor's default is not a claim"
    );
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
                    vec![
                        Expr::Sym(String::from("x")),
                        Expr::Num(Num::Int(BigInt::from(2))),
                    ],
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

// ---------------------------------------------------------------------------
// 9. Kernel properties the bridge's correctness rests on
// ---------------------------------------------------------------------------
//
// These are the items the second review found broken in the kernel. The bridge
// must not merely *assume* they are fixed: each test asserts the live kernel
// behaviour the static lattice and the pinned key depend on.

/// `matches_type` compares a tensor's rank and a unit's dimension. If it did
/// not, a rank-1 value would be `Shaped` against a rank-2 parameter and beat
/// the `Dyn` overload, so the checker and the kernel would disagree about the
/// chosen overload.
#[test]
fn the_match_lattice_compares_rank_and_dimension() {
    use rssn::kernel::overload::Match;
    use rssn::kernel::overload::matches_type;
    use rssn::kernel::signature::DimExpr;
    use rssn::kernel::signature::NatExpr;
    use rssn::kernel::signature::TypeExpr;

    let tensor = |rank: u64| {
        TypeExpr::Tensor {
            rank: NatExpr::Lit(rank),
            shape: None,
            elem: Box::new(TypeExpr::Real),
        }
    };
    assert_eq!(matches_type(&tensor(2), &tensor(2)), Match::Exact);
    assert_eq!(
        matches_type(&tensor(2), &tensor(1)),
        Match::No,
        "a rank-1 tensor must not match a rank-2 parameter"
    );

    let length = DimExpr::base("Length");
    let time = DimExpr::base("Time");
    assert_eq!(
        matches_type(
            &TypeExpr::Unit(length.clone()),
            &TypeExpr::Unit(length.clone())
        ),
        Match::Exact
    );
    assert_eq!(
        matches_type(&TypeExpr::Unit(length.clone()), &TypeExpr::Unit(time)),
        Match::No,
        "a time is not a length"
    );
    // A unit is a dimension carrier: `Real[L]` and `Unit[L]` may meet.
    assert_eq!(
        matches_type(
            &TypeExpr::Dim(Box::new(TypeExpr::Real), length.clone()),
            &TypeExpr::Unit(length)
        ),
        Match::Exact
    );
}

/// The rank fix is visible through the bridge's own resolver: `det` has a
/// `Tensor<2, Real>` overload and a `Dyn` one, and a rank-1 tensor must fall
/// through to the `Dyn` overload instead of being scored `Shaped` against the
/// matrix overload.
#[test]
fn a_rank_mismatch_resolves_to_the_dynamic_overload() {
    use aevia_rssn::SigType;
    use aevia_rssn::resolve_key;

    let det = registry().op("det").expect("det");
    let key = resolve_key(
        "det",
        &[SigType::Tensor {
            rank: Some(1),
            shape: None,
            elem: Box::new(SigType::Real),
        }],
    )
    .expect("the `Dyn` overload accepts a rank-1 tensor");
    assert_eq!(key, det.keys[1], "the symbolic overload");
    assert_ne!(key, det.keys[0], "not the rank-2 numeric overload");
}

/// `call_key` executes only the overload the key names: the kernel checks the
/// pinned signature against the arguments, so a key whose overload does not
/// accept them is `UnknownOp`, never a different overload's execution.
#[test]
fn a_pinned_key_is_validated_against_its_own_overload() {
    use rssn::kernel::call::OpError;

    let quantity = registry().op("quantity").expect("quantity");
    // Overload 2 of `quantity` is `(Dyn, Symbol)`: its second parameter is a
    // free symbol, not a unit.
    let key = quantity.keys[1].clone();
    let error = kernel::kernel()
        .call_key(&key, &[Val::Num(Num::Float(3.0)), Val::Unit(metre())])
        .expect_err("a unit does not satisfy the `Symbol` parameter");
    assert_eq!(error.variant(), Some("UnknownOp"));
    match error.cause.as_deref() {
        | Some(OpError::UnknownOp(reported)) => assert_eq!(reported, &key),
        | other => panic!("expected `UnknownOp`, found {other:?}"),
    }
}

/// Every manifest key is the hash of the overload it names, and two overloads
/// of one operator never share a key. The old `Signature::hash` returned the
/// hash of the empty encoding on failure, which would have made every key
/// collide. A key must also carry the operator identity it was generated for,
/// or `call_key` would look the wrong registration up.
#[test]
fn every_key_is_the_hash_of_distinct_signatures() {
    for op in registry().ops() {
        let mut seen = std::collections::BTreeSet::new();
        for (index, signature) in op.raw.iter().enumerate() {
            let key = op
                .keys
                .get(index)
                .unwrap_or_else(|| panic!("`{}` has no key for overload {index}", op.name));
            assert_eq!(key.name, op.name, "key {index} names another operator");
            assert_eq!(
                key.rule_set, op.rule_set,
                "key {index} names another rule set"
            );
            assert_eq!(
                signature.hash(),
                key.signature_hash,
                "key {index} of `{}` is not its signature's hash",
                op.name
            );
            assert!(
                key.signature_hash.iter().any(|byte| *byte != 0),
                "key {index} of `{}` is the all-zero hash",
                op.name
            );
            assert!(
                seen.insert(key.signature_hash),
                "`{}` has two overloads with one key",
                op.name
            );
        }
    }
}

/// An ambiguous resolution reports how many overloads *tied*, not how many the
/// operator has. `quantity` has three overloads; a float and a unit make two
/// of them tie, and the diagnostic must say two.
#[test]
fn an_ambiguity_reports_the_tied_count() {
    let error = kernel::kernel()
        .resolve("quantity", &[Val::Num(Num::Float(3.0)), Val::Unit(metre())])
        .expect_err("two overloads tie");
    assert_eq!(error.kind, Failure::Ambiguous);
    assert!(
        error.message.contains("2 overloads"),
        "the message must quote the tied count, not the overload count: {}",
        error.message
    );
    assert_eq!(
        registry().op("quantity").map(|op| op.keys.len()),
        Some(3),
        "the operator really has three overloads"
    );
}

/// `manifest.units` is the session's unit registry, not a process-wide table:
/// a unit registered on one session appears in that session's manifest and in
/// no other. Aevia generates `Units.*` from that manifest, so the distinction
/// is the difference between "this program's unit" and "every program's unit".
#[test]
fn a_session_unit_reaches_its_own_manifest() {
    use num_rational::BigRational;
    use rssn::api::Session;
    use rssn::dim::DimExpr;
    use rssn::dim::Scale;
    use rssn::dim::UnitDef;

    let furlong = UnitDef::linear(
        "furlong",
        DimExpr::base("Length"),
        Scale {
            rat: BigRational::new(BigInt::from(201_168), BigInt::from(1_000)),
            pi: 0,
        },
    );
    let mut first = Session::new();
    let second = Session::new();
    first
        .register_unit(furlong)
        .expect("a session may register a unit");
    assert!(
        first
            .manifest()
            .units
            .iter()
            .any(|unit| unit.name == "furlong"),
        "the registered unit must reach the session's manifest"
    );
    assert!(
        !second
            .manifest()
            .units
            .iter()
            .any(|unit| unit.name == "furlong"),
        "a unit is session-scoped, not process-wide"
    );
}

/// `DimExpr::try_from_term` is `to_term`'s inverse. The static form is
/// canonicalised on the way back (an exponent of `-1` comes back as
/// `Inverse`, a product may be reordered), so the assertion compares the
/// `DimTerm` the recovered expression denotes: writing a dimension and reading
/// it back must not drop or change a factor.
#[test]
fn the_dimension_term_round_trip_is_exact() {
    use rssn::dim::DimArena;
    use rssn::dim::DimExpr;
    use rssn::kernel::signature::Exp;
    use rssn::kernel::signature::Rational;

    let mut arena = DimArena::new();
    let expressions = [
        DimExpr::Dimless,
        DimExpr::base("Length"),
        DimExpr::base("Length").pow(Exp::Rat(Rational::new(2, 1))),
        DimExpr::base("Length").pow(Exp::Rat(Rational::new(-1, 1))),
        DimExpr::Mul(vec![
            DimExpr::base("Length"),
            DimExpr::base("Time").pow(Exp::Rat(Rational::new(-2, 1))),
        ]),
        DimExpr::base("Length").pow(Exp::Symbol("n".to_string())),
    ];
    for expression in expressions {
        let term = expression.to_term(&mut arena).expect("to_term");
        let back = DimExpr::try_from_term(&term, &arena).expect("try_from_term");
        let again = back
            .to_term(&mut arena)
            .expect("to_term of the recovered form");
        assert_eq!(
            again, term,
            "`to_term(try_from_term(term))` must be `term` for {expression:?}"
        );
    }
}

#[test]
fn rssn_scalar_root_finders_handle_tiny_residuals() {
    use rssn::kernels::rootfind::RootError;
    use rssn::kernels::rootfind::brent;
    use rssn::kernels::rootfind::newton_bracketed;
    for scale in [1.0, 1e-200, 1e-300] {
        let f = |x: f64| scale * (x * x - 0.36);
        let b = brent(f, 0.0, 1.0, 1e-12, 200).unwrap();
        let n = newton_bracketed(f, |x| 2.0 * scale * x, 0.0, 1.0, 1e-12, 200).unwrap();
        assert!((b - 0.6).abs() < 1e-10, "Brent, scale {scale:e}: {b}");
        assert!((n - 0.6).abs() < 1e-10, "Newton, scale {scale:e}: {n}");
        assert_eq!(
            brent(|x| scale * (x * x + 1.0), -1.0, 1.0, 1e-12, 100),
            Err(RootError::NotBracketed)
        );
    }
    assert_eq!(
        brent(|_| f64::NAN, 0.0, 1.0, 1e-12, 100),
        Err(RootError::Invalid)
    );
    let f = |x: f64| (1e12 * x).exp() - 100.0_f64.exp();
    let df = |x: f64| 1e12 * (1e12 * x).exp();
    for (a, b) in [(0.0, 6e-10), (6e-10, 0.0)] {
        let root = newton_bracketed(f, df, a, b, 1e-12, 200).unwrap();
        assert!((root - 1e-10).abs() <= 1e-12, "Newton: {root:e}");
    }
}

#[cfg(feature = "jit")]
#[test]
fn rssn_jit_rejects_load_offsets_outside_cranelift_range() {
    use rssn::backend::jit::JitFunction;
    use rssn::backend::tape::Inst;
    use rssn::backend::tape::Tape;
    let limit = (i32::MAX / 8) as u32;
    for (is_input, index, accepted) in [
        (true, limit, true),
        (true, limit + 1, false),
        (false, limit, true),
        (false, limit + 1, false),
    ] {
        let tape = Tape {
            insts: vec![if is_input {
                Inst::Input(index)
            } else {
                Inst::Param(index)
            }],
            args: Vec::new(),
            inputs: if is_input {
                index as usize + 1
            } else {
                0
            },
            params: if is_input {
                0
            } else {
                index as usize + 1
            },
            outputs: vec![0],
        };
        assert_eq!(
            JitFunction::compile(&tape).is_ok(),
            accepted,
            "is_input={is_input}, index={index}"
        );
    }
}

#[test]
fn rssn_terminal_event_location_scales_with_step_and_time_spacing() {
    use rssn::kernels::ode::OdeSolverMethod;
    use rssn::kernels::ode::{
        self,
    };
    use rssn::kernels::ode_adaptive::Event;
    use rssn::kernels::ode_adaptive::OdeOptions;
    use rssn::kernels::ode_adaptive::{
        self,
    };
    for (start, width) in [(0.0_f64, 1e-15_f64), (1e12, 1.0)] {
        let end = start + width;
        let target = start + 0.6 * width;
        let rhs = |_t: f64, _y: &[f64], out: &mut [f64]| out[0] = 1.0;
        let event_fn = |t: f64, _y: &[f64]| ((t - start) / width).powi(2) - 0.36;
        let events = [Event {
            g: &event_fn,
            terminal: true,
            direction: 0,
        }];
        let (_, fixed_hits) = ode::solve_fixed_events(
            rhs,
            &[0.0],
            (start, end),
            1,
            OdeSolverMethod::RungeKutta4,
            &events,
        )
        .unwrap();
        let adaptive =
            ode_adaptive::dopri5(rhs, start, end, &[0.0], &OdeOptions::default(), &events).unwrap();
        let tolerance = (width * 1e-10).max(2.0 * (start.abs().next_up() - start.abs()));
        for found in [fixed_hits[0].t, adaptive.events[0].t] {
            assert!(
                (found - target).abs() <= tolerance,
                "start={start:e}, width={width:e}, got={found:e}, target={target:e}"
            );
        }
    }
}

#[test]
fn rssn_streaming_covariance_merges_large_offset_shards() {
    use rssn::kernels::stats::OnlineCovariance;
    use rssn::kernels::stats::OnlineStatsError;
    let mut all = OnlineCovariance::new(2).unwrap();
    let mut left = all.clone();
    let mut right = all.clone();
    for i in 0..100 {
        let sample = [1e12 + i as f64, -1e12 + 2.0 * i as f64];
        all.push(&sample).unwrap();
        if i < 43 {
            left.push(&sample).unwrap();
        } else {
            right.push(&sample).unwrap();
        }
    }
    left.merge(&right).unwrap();
    assert_eq!(left.count(), 100);
    for (got, expected) in left
        .sample_covariance()
        .unwrap()
        .iter()
        .zip(all.sample_covariance().unwrap())
    {
        assert!((got - expected).abs() < 1e-9);
    }
    let before = left.mean().unwrap().to_vec();
    assert_eq!(
        left.push(&[f64::NAN, 0.0]),
        Err(OnlineStatsError::NonFiniteInput)
    );
    assert_eq!(left.count(), 100);
    assert_eq!(left.mean().unwrap(), before);
}

#[test]
fn rssn_simplex_cubature_integrates_tetrahedron_moments() {
    use rssn::kernels::quadrature::SimplexError;
    use rssn::kernels::quadrature::integrate_simplex;
    let vertices = vec![
        vec![0., 0., 0.],
        vec![1., 0., 0.],
        vec![0., 1., 0.],
        vec![0., 0., 1.],
    ];
    let q = integrate_simplex(
        |x| x[0] * x[0] * x[1] * x[2],
        &vertices,
        1e-10,
        1e-10,
        200_000,
    )
    .unwrap();
    assert!(q.converged, "{q:?}");
    assert!((q.value - 1.0 / 2520.0).abs() < 1e-9, "{q:?}");
    assert_eq!(
        integrate_simplex(|_| f64::NAN, &vertices, 1e-6, 0.0, 100_000),
        Err(SimplexError::NonFiniteIntegrand)
    );
}

#[test]
fn rssn_large_natural_and_clamped_splines_use_linear_storage() {
    use rssn::kernels::approx::CubicSpline;
    use rssn::kernels::approx::SplineBc;
    let x: Vec<_> = (0..50_000).map(|i| f64::from(i) * 0.01).collect();
    let y: Vec<_> = x.iter().map(|x| 2.0 * x + 1.0).collect();
    for boundary in [SplineBc::Natural, SplineBc::Clamped(2.0, 2.0)] {
        let s = CubicSpline::new(&x, &y, boundary).unwrap();
        assert!((s.eval(250.125) - 501.25).abs() < 1e-10);
        assert!((s.derivative(250.125, 1) - 2.0).abs() < 1e-10);
    }
}
