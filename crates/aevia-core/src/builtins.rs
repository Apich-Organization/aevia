//! Built-in functions.
//!
//! A builtin is *named* by a string, which keeps values cheap to clone and lets
//! a builtin be passed as a value (`xs |> map(f)`), but it is *dispatched* by a
//! [`BuiltinId`](crate::builtin_ids::BuiltinId): the name is resolved once per
//! call and the `match` in `call_inner` is then a jump table. The type checker
//! owns the signatures; this module owns the behaviour, and the two lists are
//! kept in step by the conformance tests.

use crate::builtin_ids::BuiltinId as B;
use crate::builtin_ids::BuiltinId;
use crate::eval::EvalError;
use crate::eval::Interp;
use crate::eval::SliceValue;
use crate::eval::build_tensor;
use crate::eval::type_argument_name;
use crate::int::Int;
use crate::kernel as bridge;
use crate::sym::Fun;
use crate::sym::Term;
use crate::value::DynValue;
use crate::value::ExactRep;
use crate::value::NativeFn;
use crate::value::NumKind;
use crate::value::NumVal;
use crate::value::RecordValue;
use crate::value::Storage;
use crate::value::TensorData;
use crate::value::Value;
use crate::value::make_variant;
use crate::value::show;
use aevia_dim::Dim;
use aevia_dim::UnitDef;
use aevia_syntax::Span;
use aevia_syntax::ast::BinOp;
use aevia_syntax::ast::NatAst;
use aevia_syntax::ast::ShapeAst;
use aevia_syntax::ast::ShapeItem;
use aevia_syntax::ast::Ty;
use aevia_syntax::ast::TyKind;
use aevia_types::adt;
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;
use num_traits::Signed;
use num_traits::ToPrimitive;
use num_traits::Zero;
use std::borrow::Cow;
use std::rc::Rc;

mod exact;
mod factorization;
mod matrix;
mod plot;
mod reduction;
mod sequence;

pub(crate) use plot::default_plot_opts;

/// Whether a builtin reads its explicit type argument (§8.4).
///
/// The list lives in `aevia_types::builtins` so the IR lowering and the
/// interpreter agree on which calls keep their type argument.
#[must_use]
pub fn takes_type_argument(name: &str) -> bool {
    aevia_types::builtins::takes_type_argument(name)
}

/// Calls a builtin that was written with an explicit type argument.
///
/// `zeros<Int>(3)` and `zeros<Float>(3)` have the same runtime shape but not
/// the same element type, so the type argument decides the storage (§8.4).
/// Ignoring it — which is what happened while `TypeApp` was erased — made
/// `zeros<Int>` a tensor of `f64` that the checker typed as `Int`.
///
/// `reshape<[3,2]>`, `zeros_of<[3,2]>` and `ones_of<[3,2]>` name their *shape*
/// in the same position, so the AST node — not a rendered name — is what the
/// runtime reads: the extents are type-level data the checker never lowers.
///
/// # Errors
///
/// Propagates the builtin's own failure.
pub fn call_typed(
    interp: &mut Interp,
    name: &str,
    types: &[Ty],
    args: Vec<Value>,
    span: Span,
) -> Result<Value, EvalError> {
    let _backend = interp.rssn_source_access();
    let _cancellation = interp.cancellation_scope();
    interp.check_cancelled().map_err(|error| error.at(span))?;
    let result = call_typed_inner(interp, name, types, args, span);
    interp.check_cancelled().map_err(|error| error.at(span))?;
    result
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps typed dispatch separate from the cancellation boundary"
    )
)]
fn call_typed_inner(
    interp: &mut Interp,
    name: &str,
    types: &[Ty],
    args: Vec<Value>,
    span: Span,
) -> Result<Value, EvalError> {
    let element = types
        .first()
        .map_or_else(|| "Float".to_string(), type_argument_name);
    match name {
        | "zeros" | "ones" => {
            let shape = matrix::shape_from_args(&args, 0, name, span)?;
            let one = name == "ones";
            match element.as_str() {
                | "Int" => matrix::repeat_exact(shape, one, ExactRep::Int, span),
                | "Real" => matrix::repeat_exact(shape, one, ExactRep::Real, span),
                | "Float" => matrix::repeat_float(shape, one, span),
                | other => {
                    Err(EvalError::new(
                        "E1001",
                        format!(
                            "`{name}<{other}>` needs a representation this runtime cannot store"
                        ),
                    )
                    .at(span))
                },
            }
        },
        | "zeros_of" | "ones_of" => {
            let Some(shape) = types.first().and_then(shape_of_type_argument) else {
                return Err(EvalError::new(
                    "E0310",
                    format!("`{name}` needs a concrete shape, as in `{name}<[3, 2]>()`"),
                )
                .at(span));
            };
            matrix::repeat_float(shape, name == "ones_of", span)
        },
        | "reshape" => {
            let Some(shape) = types.first().and_then(shape_of_type_argument) else {
                return Err(EvalError::new(
                    "E0310",
                    "`reshape` needs a concrete shape, as in `reshape<[3, 2]>(a)`",
                )
                .at(span));
            };
            let tensor = expect_tensor(&args, 0, name, span)?;
            Ok(matrix::reshape(&tensor, shape))
        },
        // `refine_shape<[3, ?]>(a)`: `Some(a)` when every stated extent
        // matches, `None` otherwise. The rank was checked statically.
        | "refine_shape" => {
            let Some(extents) = types.first().and_then(refine_extents) else {
                return Err(EvalError::new(
                    "E0310",
                    "`refine_shape` needs a concrete shape, as in `refine_shape<[3, ?]>(a)`",
                )
                .at(span));
            };
            let tensor = expect_tensor(&args, 0, name, span)?;
            let matches = tensor.shape.len() == extents.len()
                && tensor
                    .shape
                    .iter()
                    .zip(&extents)
                    .all(|(actual, wanted)| wanted.is_none_or(|wanted| wanted == *actual));
            if matches {
                Ok(some(Value::Tensor(tensor)))
            } else {
                Ok(make_variant(adt::OPTION_ID, 0, "None", Vec::new()))
            }
        },
        | "from_dense" | "convert" | "speye" | "spdiag" => {
            crate::sparse::call_typed(interp, name, types, &args, span)
        },
        | _ => call(interp, name, args, span),
    }
}

/// The extents of a `refine_shape` target; `None` entries are `?` (any extent).
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "decodes partial shapes used by the typed reshape path"
    )
)]
fn refine_extents(ty: &Ty) -> Option<Vec<Option<usize>>> {
    let ShapeAst::Refined(items) = (match &ty.kind {
        | TyKind::ShapeArg(shape) | TyKind::Tensor { shape, .. } => shape,
        | _ => return None,
    }) else {
        return None;
    };
    items
        .iter()
        .map(|item| {
            match item {
                | ShapeItem::Known(nat) => nat_value(nat).map(Some),
                | ShapeItem::Unknown => Some(None),
            }
        })
        .collect()
}

/// The extents a shape type argument states.
///
/// `reshape<[3, 2]>`, `zeros_of<[3, 2]>` and the spelled-out
/// `Tensor<[3, 2], Float>` all reach the runtime as a [`Ty`]; a `?` extent or a
/// type-level variable is not a concrete shape and yields `None`.
#[must_use]
pub fn shape_of_type_argument(ty: &Ty) -> Option<Vec<usize>> {
    match &ty.kind {
        | TyKind::ShapeArg(shape) | TyKind::Tensor { shape, .. } | TyKind::Sparse { shape, .. } => {
            shape_extents(shape)
        },
        | TyKind::Matrix { rows, cols, .. } => Some(vec![nat_value(rows)?, nat_value(cols)?]),
        | TyKind::NatArg(nat) => Some(vec![nat_value(nat)?]),
        | _ => None,
    }
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "validates the concrete shape behind the public type argument decoder"
    )
)]
fn shape_extents(shape: &ShapeAst) -> Option<Vec<usize>> {
    match shape {
        | ShapeAst::Rank(_) => None,
        | ShapeAst::Refined(items) => {
            items
                .iter()
                .map(|item| {
                    match item {
                        | ShapeItem::Known(nat) => nat_value(nat),
                        | ShapeItem::Unknown => None,
                    }
                })
                .collect()
        },
    }
}

fn nat_value(nat: &NatAst) -> Option<usize> {
    match nat {
        | NatAst::Lit(value) => Some(*value),
        | NatAst::Add(left, right) => nat_value(left)?.checked_add(nat_value(right)?),
        | NatAst::Mul(left, right) => nat_value(left)?.checked_mul(nat_value(right)?),
        | NatAst::Var(_) => None,
    }
}

/// Calls a builtin.
///
/// # Errors
///
/// Returns `E1001` for names that are typed but not implemented, and the
/// builtin's own runtime errors otherwise.
#[expect(
    clippy::needless_pass_by_value,
    reason = "preserve the public host calling convention"
)]
pub fn call(
    interp: &mut Interp,
    name: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, EvalError> {
    let _backend = interp.rssn_source_access();
    let _cancellation = interp.cancellation_scope();
    interp.check_cancelled().map_err(|error| error.at(span))?;
    let result = call_inner(interp, name, &args, span);
    // Cancellation escapes even Result-returning native APIs. It must not
    // become a mathematical Err value that the source program can continue.
    interp.check_cancelled().map_err(|error| error.at(span))?;
    result
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "the public caller owns cancellation for every builtin"
    )
)]
#[expect(
    clippy::too_many_lines,
    clippy::large_stack_frames,
    reason = "exhaustive builtin dispatch keeps stack limits at the interpreter boundary"
)]
fn call_inner(
    interp: &mut Interp,
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let Some(id) = BuiltinId::from_name(name) else {
        return Err(EvalError::new(
            "E1001",
            format!("builtin `{name}` is typed but not implemented yet"),
        )
        .at(span));
    };
    if let Some(result) = crate::sparse::call(interp, id, name, args, span) {
        return result;
    }
    match id {
        // ----- output -----------------------------------------------------
        | B::Print => io_print(interp, args, false, false, span),
        | B::Println => io_print(interp, args, true, false, span),
        | B::Eprintln => io_print(interp, args, true, true, span),
        // §6.12.3: four renderers, four jobs. `repr` is round-trippable and
        // `to_latex`/`to_typst` emit their own syntax; aliasing them to `show`
        // is what let every test believe `3`, `3/1` and `3.0` were one value.
        | B::Show => {
            let value = args.first().ok_or_else(|| arity(name, 1, 0, span))?;
            Ok(Value::string(crate::value::show(
                &interp.system_display(value.clone()),
            )))
        },
        | B::Repr => {
            let value = args.first().ok_or_else(|| arity(name, 1, 0, span))?;
            Ok(Value::string(crate::value::repr(value)))
        },
        | B::ToPretty | B::ToLatex | B::ToTypst => {
            let value = args.first().ok_or_else(|| arity(name, 1, 0, span))?;
            let format = match name {
                | "to_pretty" => crate::display::Format::Pretty,
                | "to_latex" => crate::display::Format::Latex,
                | _ => crate::display::Format::Typst,
            };
            crate::display::render(value, format)
                .map(Value::string)
                .map_err(|reason| {
                    EvalError::new(
                        "E1001",
                        format!("`{name}` cannot typeset this value: {reason}"),
                    )
                    .at(span)
                })
        },

        // ----- elementary functions --------------------------------------
        | B::Sqrt => unary_math(interp, name, args, span, MathKind::Root(2)),
        | B::Cbrt => unary_math(interp, name, args, span, MathKind::Root(3)),
        | B::Exp => unary_math(interp, name, args, span, MathKind::Float(f64::exp)),
        | B::Ln | B::Log => unary_math(interp, name, args, span, MathKind::Float(f64::ln)),
        | B::Log2 => unary_math(interp, name, args, span, MathKind::Float(f64::log2)),
        | B::Log10 => unary_math(interp, name, args, span, MathKind::Float(f64::log10)),
        | B::Sin => unary_math(interp, name, args, span, MathKind::Float(f64::sin)),
        | B::Cos => unary_math(interp, name, args, span, MathKind::Float(f64::cos)),
        | B::Tan => unary_math(interp, name, args, span, MathKind::Float(f64::tan)),
        | B::Asin => unary_math(interp, name, args, span, MathKind::Float(f64::asin)),
        | B::Acos => unary_math(interp, name, args, span, MathKind::Float(f64::acos)),
        | B::Atan => unary_math(interp, name, args, span, MathKind::Float(f64::atan)),
        | B::Sinh => unary_math(interp, name, args, span, MathKind::Float(f64::sinh)),
        | B::Cosh => unary_math(interp, name, args, span, MathKind::Float(f64::cosh)),
        | B::Tanh => unary_math(interp, name, args, span, MathKind::Float(f64::tanh)),
        | B::Asinh => unary_math(interp, name, args, span, MathKind::Float(f64::asinh)),
        | B::Acosh => unary_math(interp, name, args, span, MathKind::Float(f64::acosh)),
        | B::Atanh => unary_math(interp, name, args, span, MathKind::Float(f64::atanh)),
        | B::Gamma => gamma(name, args, span),
        | B::Erf => unary_math(interp, name, args, span, MathKind::Float(erf)),
        | B::Erfc => unary_math(interp, name, args, span, MathKind::Float(|x| 1.0 - erf(x))),
        // DLMF 7.2.7; exact arguments stay symbolic as the kernel's
        // `fresnels`/`fresnelc`, which it differentiates and integrates.
        | B::FresnelS => {
            unary_math(
                interp,
                name,
                args,
                span,
                MathKind::Float(aevia_rssn::fresnel_s),
            )
        },
        | B::FresnelC => {
            unary_math(
                interp,
                name,
                args,
                span,
                MathKind::Float(aevia_rssn::fresnel_c),
            )
        },
        | B::Atan2 => {
            let y = expect_num(args, 0, name, span)?;
            let x = expect_num(args, 1, name, span)?;
            Ok(Value::Num(NumVal::float(
                y.to_f64()
                    .unwrap_or(f64::NAN)
                    .atan2(x.to_f64().unwrap_or(f64::NAN)),
                Dim::one(),
            )))
        },
        | B::Hypot => {
            let a = expect_num(args, 0, name, span)?;
            let b = expect_num(args, 1, name, span)?;
            Ok(Value::Num(NumVal::float(
                a.to_f64()
                    .unwrap_or(f64::NAN)
                    .hypot(b.to_f64().unwrap_or(f64::NAN)),
                a.dim,
            )))
        },

        // ----- numeric ----------------------------------------------------
        | B::Abs => {
            map_scalar(args, name, span, |num| {
                let kind = match num.kind {
                    | NumKind::Int(value) => NumKind::Int(value.abs()),
                    | NumKind::Exact(value) => NumKind::Exact(Rc::new(value.abs())),
                    | NumKind::Float(value) => NumKind::Float(value.abs()),
                    | NumKind::Complex(re, im) => NumKind::Float(re.hypot(im)),
                    | NumKind::Sym(term) => {
                        NumKind::Sym(Rc::new(Term::fun(Fun::Abs, vec![(*term).clone()])))
                    },
                };
                NumVal {
                    kind,
                    dim: num.dim,
                    display: num.display,
                }
            })
        },
        | B::Sign => {
            map_scalar(args, name, span, |num| {
                let value = num.to_f64().unwrap_or(f64::NAN);
                NumVal::int(BigInt::from(if value > 0.0 {
                    1
                } else if value < 0.0 {
                    -1
                } else {
                    0
                }))
            })
        },
        | B::Min | B::Max => {
            let a = expect_num(args, 0, name, span)?;
            let b = expect_num(args, 1, name, span)?;
            let (x, y) = (
                a.to_f64().unwrap_or(f64::NAN),
                b.to_f64().unwrap_or(f64::NAN),
            );
            let take_a = if name == "min" {
                x <= y
            } else {
                x >= y
            };
            Ok(Value::Num(if take_a { a } else { b }))
        },
        | B::Clamp => {
            let x = expect_num(args, 0, name, span)?;
            let lo = expect_num(args, 1, name, span)?;
            let hi = expect_num(args, 2, name, span)?;
            let value = x.to_f64().unwrap_or(f64::NAN);
            let low = lo.to_f64().unwrap_or(f64::NAN);
            let high = hi.to_f64().unwrap_or(f64::NAN);
            let clamped = value.max(low).min(high);
            Ok(Value::Num(NumVal::float(clamped, x.dim)))
        },
        | B::IsNan | B::IsFinite | B::IsInfinite => {
            let num = expect_num(args, 0, name, span)?;
            let (nan, infinite) = match &num.kind {
                | NumKind::Int(_) | NumKind::Exact(_) => (false, false),
                | NumKind::Complex(re, im) => {
                    (
                        re.is_nan() || im.is_nan(),
                        re.is_infinite() || im.is_infinite(),
                    )
                },
                | _ => {
                    let value = num.to_f64().unwrap_or(0.0);
                    (
                        value.is_nan(),
                        value.is_infinite() || infinity_sign(&num).is_some(),
                    )
                },
            };
            Ok(Value::Bool(match id {
                | B::IsNan => nan,
                | B::IsFinite => !nan && !infinite,
                | _ => !nan && infinite,
            }))
        },
        | B::Round if args.len() == 2 => {
            let num = expect_num(args, 0, name, span)?;
            let digits = expect_int(args, 1, name, span)?;
            let value = num.to_f64().unwrap_or(f64::NAN);
            let power = 10f64.powi(i32::try_from(digits.unsigned_abs()).unwrap_or(i32::MAX));
            let rounded = if !value.is_finite() {
                value
            } else if digits < 0 {
                if power.is_infinite() {
                    0.0_f64.copysign(value)
                } else {
                    (value / power).round() * power
                }
            } else {
                let scaled = value * power;
                if scaled.is_finite() {
                    scaled.round() / power
                } else {
                    value
                }
            };
            Ok(Value::Num(NumVal::float(rounded, num.dim)))
        },
        | B::Floor | B::Ceil | B::Round | B::Trunc => {
            let num = expect_num(args, 0, name, span)?;
            let function = Fun::Other(match id {
                | B::Floor => "floor",
                | B::Ceil => "ceil",
                | B::Round => "round",
                | _ => "trunc",
            });
            let rounded = match &num.kind {
                | NumKind::Int(_) => num,
                | NumKind::Exact(value) => {
                    let rounded = match id {
                        | B::Floor => value.floor(),
                        | B::Ceil => value.ceil(),
                        | B::Round => value.round(),
                        | _ => value.trunc(),
                    };
                    NumVal::exact(rounded, num.dim)
                },
                | NumKind::Float(value) => NumVal::float(function.eval(*value), num.dim),
                | NumKind::Sym(_) if infinity_sign(&num).is_some() => num,
                | NumKind::Sym(term) => {
                    NumVal::symbolic(Term::fun(function, vec![(**term).clone()]), num.dim)
                },
                | NumKind::Complex(..) => return Err(domain_error(name, &num, span)),
            };
            Ok(Value::Num(rounded))
        },
        | B::Mod | B::Rem => {
            let a = expect_num(args, 0, name, span)?;
            let b = expect_num(args, 1, name, span)?;
            exact::remainder(a, &b, name == "mod", span)
        },
        | B::Div | B::Quot => {
            let a = expect_exact_int(args, 0, name, span)?;
            let b = expect_exact_int(args, 1, name, span)?;
            exact::divide(&a, &b, name == "div", span)
        },
        | B::Gcd | B::Lcm => {
            let a = expect_exact_int(args, 0, name, span)?;
            let b = expect_exact_int(args, 1, name, span)?;
            Ok(exact::common_multiple_or_divisor(&a, &b, name == "lcm"))
        },
        | B::ModPow => {
            let base = expect_exact_int(args, 0, name, span)?;
            let exponent = expect_exact_int(args, 1, name, span)?;
            let modulus = expect_exact_int(args, 2, name, span)?;
            exact::modular_power(&base, &exponent, &modulus, span)
        },
        | B::Factorial | B::DoubleFactorial => {
            let n = expect_int(args, 0, name, span)?;
            let result = crate::eval::factorial(interp, n, name == "double_factorial", span)?;
            Ok(Value::Num(NumVal::int(result)))
        },
        | B::ToFloat => {
            let num = expect_num(args, 0, name, span)?;
            match num.to_f64() {
                | Some(value) => Ok(ok(Value::Num(NumVal::float(value, num.dim)))),
                | None => Ok(err(Value::string("the value has free symbols"))),
            }
        },
        | B::ToReal => {
            let num = expect_num(args, 0, name, span)?;
            Ok(Value::Num(match num.to_rational() {
                | Some(value) => NumVal::exact(value, num.dim),
                | None => num,
            }))
        },
        | B::ToInt => {
            let num = expect_num(args, 0, name, span)?;
            exact::truncate(&num, span)
        },

        // ----- units ------------------------------------------------------
        | B::To | B::DeltaTo => {
            let unit = expect_unit(args, 0, name, span)?;
            let mut num = expect_num(args, 1, name, span)?;
            if num.dim != unit.dim {
                return Err(EvalError::new(
                    "E0509",
                    format!(
                        "cannot display {} in `{}`",
                        num.dim.to_plain_string(),
                        unit.display
                    ),
                )
                .at(span));
            }
            // A difference uses the unit's scale only: `delta_to(°C, 10.0 K)`
            // is a 10 °C rise, not the point -263.15 °C.
            let unit = if name == "delta_to" && unit.offset.is_some() {
                let mut linear = (*unit).clone();
                linear.offset = None;
                linear.affine = false;
                Rc::new(linear)
            } else {
                unit
            };
            num.display = Some(unit);
            Ok(Value::Num(num))
        },
        | B::ValueIn => {
            let unit = expect_unit(args, 0, name, span)?;
            let num = expect_num(args, 1, name, span)?;
            if num.dim != unit.dim {
                return Err(EvalError::new(
                    "E0509",
                    format!(
                        "cannot express {} in `{}`",
                        num.dim.to_plain_string(),
                        unit.display
                    ),
                )
                .at(span));
            }
            let raw = num.to_f64().unwrap_or(f64::NAN);
            let scale = unit.scale.to_f64();
            let offset = unit.offset.as_ref().map_or(0.0, aevia_dim::Scale::to_f64);
            let value = if scale == 0.0 {
                raw
            } else {
                (raw - offset) / scale
            };
            Ok(Value::Num(NumVal::float(value, Dim::one())))
        },
        | B::Base => {
            let mut num = expect_num(args, 0, name, span)?;
            num.display = None;
            Ok(Value::Num(num))
        },
        | B::Point => {
            let coefficient = expect_float(args, 0, name, span)?;
            let unit = expect_unit(args, 1, name, span)?;
            let scale = unit.scale.to_f64();
            let offset = unit.offset.as_ref().map_or(0.0, aevia_dim::Scale::to_f64);
            let mut num = NumVal::float(coefficient * scale + offset, unit.dim.clone());
            num.display = Some(unit);
            Ok(Value::Num(num))
        },

        // ----- tensors ----------------------------------------------------
        | B::Zeros | B::Ones => {
            let shape = matrix::shape_from_args(args, 0, name, span)?;
            matrix::repeat_float(shape, name == "ones", span)
        },
        | B::Fill => {
            let first = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            let shape = matrix::shape_from_args(args, 1, name, span)?;
            matrix::fill(shape, first, span)
        },
        | B::Linspace => {
            let start = expect_num(args, 0, name, span)?;
            let end = expect_num(args, 1, name, span)?;
            let count = expect_int(args, 2, name, span)?;
            matrix::linspace(&start, &end, count, span)
        },
        | B::Range => {
            let start = expect_exact_int(args, 0, name, span)?;
            let end = expect_exact_int(args, 1, name, span)?;
            let step = if args.len() > 2 {
                expect_exact_int(args, 2, name, span)?
            } else {
                Int::ONE
            };
            exact::range(interp, start, &end, &step, span)
        },
        | B::Length => {
            match args.first() {
                // §8.1.4: a string's length counts grapheme clusters.
                | Some(Value::Str(text)) => {
                    use unicode_segmentation::UnicodeSegmentation as _;
                    Ok(Value::Num(NumVal::int(BigInt::from(
                        text.graphemes(true).count(),
                    ))))
                },
                | Some(Value::Tensor(tensor)) => {
                    Ok(Value::Num(NumVal::int(BigInt::from(
                        tensor.shape.first().copied().unwrap_or(0),
                    ))))
                },
                | Some(Value::Variant(variant)) if variant.adt == aevia_types::adt::set_id() => {
                    let items = expect_set(args, 0, name, span)?;
                    Ok(Value::Num(NumVal::int(BigInt::from(items.len()))))
                },
                | _ => Err(arity(name, 1, args.len(), span)),
            }
        },
        | B::Shape => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let shape: Vec<BigRational> = tensor
                .shape
                .iter()
                .map(|extent| BigRational::from_integer(BigInt::from(*extent)))
                .collect();
            let len = shape.len();
            Ok(Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape: vec![len],
                storage: Storage::TypedExact(shape, ExactRep::Int),
                dim: Dim::one(),
                display: None,
            })))
        },
        | B::Rank => {
            let tensor = expect_tensor(args, 0, name, span)?;
            Ok(Value::Num(NumVal::int(BigInt::from(tensor.rank()))))
        },
        | B::Sum => {
            // §8.5: `sum(f, a, b)` is the calculus overload. Without this arm
            // the only `sum` was the tensor reduction, so every summation over
            // a range was `E0302`.
            if args.len() == 3 && matches!(args.first(), Some(Value::Closure(_) | Value::Native(_)))
            {
                return sum_range(interp, args, span);
            }
            let tensor = expect_tensor(args, 0, name, span)?;
            if args.len() > 1 {
                let axis = host_count_arg(args, 1, name, span)?;
                return reduction::sum_axis(interp, &tensor, axis, span);
            }
            // §6.6.6: the empty sum is 0, in the phase the elements would have
            // had — a `Float` tensor answers `0.0`, an exact one `0`.
            if tensor.storage.is_empty() {
                return Ok(Value::Num(zero_of(&tensor)));
            }
            // §9.13: a dense buffer folds without boxing a `Value` per step.
            // The fold is the compensated one (§6.6.6), and the boxed `Float`
            // path uses the same fold so the class stays unobservable.
            if let Some(num) = tensor.dense_sum_in_mode(interp.execution_mode()) {
                return Ok(Value::Num(num));
            }
            let elements = tensor.elements();
            if elements.iter().all(|element| {
                matches!(
                    element,
                    Value::Num(NumVal {
                        kind: NumKind::Float(_),
                        ..
                    })
                )
            }) {
                let values: Vec<f64> = elements
                    .iter()
                    .map(|element| element.as_f64().unwrap_or(f64::NAN))
                    .collect();
                let mut total = NumVal::float(crate::value::kahan_sum(&values), tensor.dim.clone());
                // A one-element reduction is the element itself, display unit
                // included — the dense path keeps it for the same reason, and
                // the two classes must stay indistinguishable.
                if let [Value::Num(only)] = elements.as_slice() {
                    total.display.clone_from(&only.display);
                }
                return Ok(Value::Num(total));
            }
            let mut total: Option<Value> = None;
            for element in elements {
                total = Some(match total {
                    | None => element,
                    | Some(acc) => interp.apply_binary("+", acc, element, span)?,
                });
            }
            Ok(total.unwrap_or_else(|| {
                Value::Num(NumVal::exact(BigRational::zero(), tensor.dim.clone()))
            }))
        },
        | B::Mean => {
            let tensor = expect_tensor(args, 0, name, span)?;
            reduction::mean(&tensor, span)
        },
        | B::Var | B::Std => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let population = matches!(args.get(1), Some(Value::Bool(true)));
            reduction::variance_or_std(&tensor, name, population, span)
        },
        | B::MinAll | B::MaxAll => {
            let tensor = expect_tensor(args, 0, name, span)?;
            // §8.1: `min`/`max` are the *binary* functions; the tensor reduction
            // is only the whole-tensor `min_all`/`max_all` (no axis form and no
            // `argmin`/`argmax` is declared). §9.13: a dense `f64` buffer is
            // scanned once without boxing a `Value` per element. The comparison
            // order and the NaN rule are exactly the element-wise loop's: a
            // comparison with NaN is false, so a NaN never displaces the current
            // best and a leading NaN stays sticky — the Float/TotalOrd rule from
            // §6.6.6, not a second semantics.
            if let Some(values) = tensor.dense_flat() {
                let find_min = name == "min_all";
                let mut best: Option<(usize, f64)> = None;
                for (index, value) in values.iter().copied().enumerate() {
                    best = Some(match best {
                        | None => (index, value),
                        | Some((best_index, current)) => {
                            let take = if find_min {
                                value < current
                            } else {
                                value > current
                            };
                            if take {
                                (index, value)
                            } else {
                                (best_index, current)
                            }
                        },
                    });
                }
                let Some((_, value)) = best else {
                    return Err(EvalError::new("E0802", "empty tensor").at(span));
                };
                let mut num = NumVal::float(value, tensor.dim.clone());
                num.display.clone_from(&tensor.display);
                return Ok(Value::Num(num));
            }
            let mut best: Option<Value> = None;
            for element in tensor.elements() {
                let value = element.as_f64().unwrap_or(f64::NAN);
                best = Some(match best {
                    | None => element,
                    | Some(current) => {
                        let current_value = current.as_f64().unwrap_or(f64::NAN);
                        let take = if name == "min_all" {
                            value < current_value
                        } else {
                            value > current_value
                        };
                        if take { element } else { current }
                    },
                });
            }
            best.ok_or_else(|| EvalError::new("E0802", "empty tensor").at(span))
        },
        | B::Norm => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let order = if args.len() > 1 {
                expect_int(args, 1, name, span)?
            } else {
                2
            };
            let mode = interp.execution_mode();
            let norm = match tensor.dense_flat() {
                | Some(values) => norm_of_in_mode(&values, order, mode),
                | None => {
                    let values: Vec<f64> = tensor
                        .elements()
                        .iter()
                        .map(|element| element.as_f64().unwrap_or(f64::NAN))
                        .collect();
                    norm_of_in_mode(&values, order, mode)
                },
            };
            Ok(Value::Num(NumVal::float(norm, tensor.dim.clone())))
        },
        | B::Normalize => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let mode = interp.execution_mode();
            if let Some(data) = tensor.dense_flat() {
                if data.is_empty() {
                    return Ok(float_tensor(tensor.shape.clone(), Vec::new(), Dim::one()));
                }
                let norm = aevia_rssn::par_norm2_f64_in_mode(&data, mode);
                let normalized =
                    aevia_rssn::par_map_f64_slice(&data, 32_768, mode, |value| value / norm);
                return Ok(float_tensor(tensor.shape.clone(), normalized, Dim::one()));
            }
            let raw = tensor.elements();
            let norm = raw
                .iter()
                .map(|element| element.as_f64().unwrap_or(0.0).powi(2))
                .sum::<f64>()
                .sqrt();
            let elements = raw
                .iter()
                .map(|element| {
                    Value::Num(NumVal::float(
                        element.as_f64().unwrap_or(0.0) / norm,
                        Dim::one(),
                    ))
                })
                .collect::<Vec<_>>();
            Ok(build_tensor(elements, tensor.shape.clone(), Dim::one()))
        },
        | B::Dot | B::Matmul | B::Matvec | B::Vecmat => {
            let left = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let right = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            interp.apply_binary("@", left, right, span)
        },
        | B::Outer => {
            let left = expect_tensor(args, 0, name, span)?;
            let right = expect_tensor(args, 1, name, span)?;
            matrix::outer(interp, &left, &right, span)
        },
        | B::Transpose => {
            let tensor = expect_tensor(args, 0, name, span)?;
            matrix::transpose(&tensor, span)
        },
        | B::Trace => {
            let tensor = expect_tensor(args, 0, name, span)?;
            matrix::trace(interp, &tensor, span)
        },
        | B::Det => {
            let tensor = expect_tensor(args, 0, name, span)?;
            factorization::determinant(interp, &tensor, span)
        },
        | B::Inv => {
            let tensor = expect_tensor(args, 0, name, span)?;
            factorization::inverse(interp, &tensor, span)
        },
        | B::Linsolve => {
            let matrix = expect_tensor(args, 0, name, span)?;
            let rhs = expect_tensor(args, 1, name, span)?;
            factorization::solve(interp, &matrix, &rhs, span)
        },
        | B::Eye => matrix::identity(expect_int(args, 0, name, span)?, span),
        | B::Diag => {
            let tensor = expect_tensor(args, 0, name, span)?;
            matrix::diagonal(&tensor, span)
        },
        | B::Flatten => {
            let tensor = expect_tensor(args, 0, name, span)?;
            if let Storage::Dense(buffer) = &tensor.storage
                && tensor.checked_element_count() == Some(buffer.len())
            {
                let result = Value::Tensor(Rc::new(TensorData::new(
                    vec![buffer.len()],
                    Storage::Dense(Rc::clone(buffer)),
                    tensor.dim.clone(),
                    tensor.display.clone(),
                )));
                return Ok(crate::host_witness::preserve_tensor_schema(result, &tensor));
            }
            if let Some(data) = tensor.dense_flat() {
                let result = Value::Tensor(Rc::new(TensorData::new(
                    vec![data.len()],
                    Storage::Dense(Rc::new(data.into_owned())),
                    tensor.dim.clone(),
                    tensor.display.clone(),
                )));
                return Ok(crate::host_witness::preserve_tensor_schema(result, &tensor));
            }
            let elements = tensor.elements();
            let len = elements.len();
            let result = build_tensor(elements, vec![len], tensor.dim.clone());
            Ok(crate::host_witness::preserve_tensor_schema(result, &tensor))
        },
        | B::At => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let index = isize::try_from(expect_int(args, 1, name, span)?)
                .map_err(|_| EvalError::new("E0802", "index out of range").at(span))?;
            interp.index_resolved(
                Value::Tensor(tensor),
                &[SliceValue::Index(index as i128)],
                span,
            )
        },
        // §6.10 / §3.8.4: the checker enforces `Parallel` and worker-local
        // interpreters share the same result ordering as the serial path.
        | B::ParMap => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let tensor = expect_tensor(args, 1, name, span)?;
            if tensor.rank() != 1 {
                return Err(EvalError::new("E0802", "`par_map` expects a rank-1 tensor").at(span));
            }
            if let Some(result) = dense_builtin_map(&function, &tensor, interp.execution_mode()) {
                interp.check_cancelled().map_err(|error| error.at(span))?;
                return Ok(result);
            }
            // §9.7: on worker replicas when the function and the elements
            // can cross threads; otherwise (and on any worker failure) the
            // sequential map, which gives the same value or the same error.
            let (parallel, inputs) = tensor.dense().map_or_else(
                || {
                    let values = tensor.elements();
                    (
                        interp.par_map_values(&function, &values, None),
                        Some(values),
                    )
                },
                |values| {
                    (
                        interp.par_map_dense_values(
                            &function,
                            values,
                            &tensor.dim,
                            tensor.display.as_deref(),
                        ),
                        None,
                    )
                },
            );
            interp.check_cancelled().map_err(|error| error.at(span))?;
            if let Some(result) = parallel {
                return Ok(result);
            }
            let inputs = inputs.unwrap_or_else(|| tensor.elements());
            let mut elements = Vec::with_capacity(inputs.len());
            for element in inputs {
                elements.push(interp.apply(function.clone(), vec![element], span)?);
            }
            let dim = elements
                .first()
                .map_or_else(Dim::one, crate::value::value_dim);
            let len = elements.len();
            Ok(build_tensor(elements, vec![len], dim))
        },
        // §8.3: `par_tabulate(n, f)` is `par_map(f, [0, …, n-1])`.
        | B::ParTabulate => {
            const MAX_OUTPUT: usize = isize::MAX as usize / std::mem::size_of::<Value>();
            let count = expect_num(args, 0, name, span)?;
            let NumKind::Int(count) = &count.kind else {
                return Err(
                    EvalError::new("E0302", "`par_tabulate` expects an Int count").at(span),
                );
            };
            let count = num_traits::ToPrimitive::to_usize(count).ok_or_else(|| {
                EvalError::new("E0802", "`par_tabulate` needs a non-negative count").at(span)
            })?;
            let function = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, args.len(), span))?;
            if count > MAX_OUTPUT {
                return Err(EvalError::new("E0802", "`par_tabulate` output is too large").at(span));
            }
            let parallel = interp.par_tabulate_values(&function, count);
            interp.check_cancelled().map_err(|error| error.at(span))?;
            if let Some(result) = parallel {
                return Ok(result);
            }
            let mut elements = Vec::with_capacity(count);
            for index in 0..count {
                let input = Value::Num(NumVal::int(index));
                elements.push(interp.apply(function.clone(), vec![input], span)?);
            }
            let dim = elements
                .first()
                .map_or_else(Dim::one, crate::value::value_dim);
            Ok(build_tensor(elements, vec![count], dim))
        },
        // §8.3: an associative reduction over fixed blocks (see
        // `parallel::Interp::par_reduce_values` for the determinism rule).
        | B::ParReduce => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 3, 0, span))?;
            let zero = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 3, args.len(), span))?;
            let tensor = expect_tensor(args, 2, name, span)?;
            if tensor.rank() != 1 {
                return Err(
                    EvalError::new("E0802", "`par_reduce` expects a rank-1 tensor").at(span),
                );
            }
            interp.par_reduce_values(&function, &zero, &tensor, span)
        },
        | B::Pure => {
            let value = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(value))))
        },
        // §9.7.3: concurrent tasks are IO actions.
        | B::Spawn => {
            let thunk = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            Ok(Value::Io(Rc::new(crate::value::IoAction::Spawn(thunk))))
        },
        | B::Await => {
            match args.first() {
                | Some(Value::Task(task)) => {
                    Ok(Value::Io(Rc::new(crate::value::IoAction::Await(
                        task.clone(),
                    ))))
                },
                | Some(other) => {
                    Err(EvalError::new(
                        "E0302",
                        format!("`await` expects a Task, found {}", other.type_name()),
                    )
                    .at(span))
                },
                | None => Err(arity(name, 1, 0, span)),
            }
        },
        | B::Map => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            match args.get(1) {
                | Some(Value::Tensor(tensor)) => {
                    if let Some(result) =
                        dense_builtin_map(&function, tensor, interp.execution_mode())
                    {
                        interp.check_cancelled().map_err(|error| error.at(span))?;
                        return Ok(result);
                    }
                    let mut elements = Vec::with_capacity(tensor.storage.len());
                    for element in tensor.elements() {
                        elements.push(interp.apply(function.clone(), vec![element], span)?);
                    }
                    let dim = elements
                        .first()
                        .map_or_else(Dim::one, crate::value::value_dim);
                    Ok(build_tensor(elements, tensor.shape.clone(), dim))
                },
                | Some(Value::Variant(variant)) if variant.adt == adt::OPTION_ID => {
                    if variant.index == 0 {
                        Ok(Value::Variant(variant.clone()))
                    } else {
                        let inner = variant.fields.first().cloned().unwrap_or(Value::Unit);
                        let mapped = interp.apply(function, vec![inner], span)?;
                        Ok(make_variant(adt::OPTION_ID, 1, "Some", vec![mapped]))
                    }
                },
                // `Ok` is variant 0 of `Result`, `Err` variant 1.
                | Some(Value::Variant(variant)) if variant.adt == adt::RESULT_ID => {
                    if variant.index == 1 {
                        Ok(Value::Variant(variant.clone()))
                    } else {
                        let inner = variant.fields.first().cloned().unwrap_or(Value::Unit);
                        let mapped = interp.apply(function, vec![inner], span)?;
                        Ok(make_variant(adt::RESULT_ID, 0, "Ok", vec![mapped]))
                    }
                },
                | _ => Err(arity(name, 2, args.len(), span)),
            }
        },
        | B::Map2 | B::ZipWith => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 3, 0, span))?;
            let a = expect_tensor(args, 1, name, span)?;
            let b = expect_tensor(args, 2, name, span)?;
            if a.shape != b.shape {
                return Err(EvalError::new("E0802", "map2 expects equal shapes").at(span));
            }
            let mut elements = Vec::with_capacity(a.storage.len());
            for (x, y) in a.iter_elements().zip(b.iter_elements()) {
                elements.push(interp.apply(function.clone(), vec![x, y], span)?);
            }
            let dim = elements
                .first()
                .map_or_else(Dim::one, crate::value::value_dim);
            Ok(build_tensor(elements, a.shape.clone(), dim))
        },
        | B::Filter => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            // §8.11 overloads `filter` on a table: the predicate sees a row.
            if matches!(args.get(1), Some(Value::Record(_))) {
                let table = table_from(args, 1, name, span)?;
                let mut kept = Vec::with_capacity(table.len());
                for (row, cells) in table.rows.iter().enumerate() {
                    let value = row_value(&table, row);
                    if matches!(
                        interp.apply(function.clone(), vec![value], span)?,
                        Value::Bool(true)
                    ) {
                        kept.push(cells.clone());
                    }
                }
                return Ok(table_value(&aevia_std::table::Table {
                    columns: table.columns,
                    units: table.units,
                    rows: kept,
                }));
            }
            let tensor = expect_tensor(args, 1, name, span)?;
            // §8.1.5: a tensor of rank >= 2 is a sequence of rows along its
            // first axis; the predicate sees a row and the result keeps the
            // rank.
            if tensor.rank() >= 2 {
                let axis = sequence::FirstAxis::new(&tensor, span)?;
                let mut kept = sequence::reserve_items(axis.rows, span)?;
                for index in 0..axis.rows {
                    let row = axis.row(index, span)?;
                    if matches!(
                        interp.apply(function.clone(), vec![row], span)?,
                        Value::Bool(true)
                    ) {
                        kept.push(index);
                    }
                }
                return axis.gather(kept.into_iter(), span);
            }
            let mut elements = Vec::new();
            for element in sequence::sequence_items(&tensor, span)? {
                if matches!(
                    interp.apply(function.clone(), vec![element.clone()], span)?,
                    Value::Bool(true)
                ) {
                    elements.push(element);
                }
            }
            let len = elements.len();
            Ok(sequence::rebuild_selection(&tensor, elements, vec![len]))
        },
        | B::Fold => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 3, 0, span))?;
            let mut accumulator = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 3, 1, span))?;
            let tensor = expect_tensor(args, 2, name, span)?;
            // §8.1.5: the step sees a row of a tensor of rank >= 2.
            for element in sequence::sequence_items(&tensor, span)? {
                accumulator = interp.apply(function.clone(), vec![accumulator, element], span)?;
            }
            Ok(accumulator)
        },
        | B::Take | B::Drop => {
            let count =
                usize::try_from(expect_int(args, 0, name, span)?.max(0)).unwrap_or(usize::MAX);
            let tensor = expect_tensor(args, 1, name, span)?;
            if tensor.rank() >= 2 {
                let axis = sequence::FirstAxis::new(&tensor, span)?;
                let split = count.min(axis.rows);
                let rows = if name == "take" {
                    0..split
                } else {
                    split..axis.rows
                };
                return axis.gather(rows, span);
            }
            let elements = sequence::sequence_items(&tensor, span)?;
            let selected: Vec<Value> = if name == "take" {
                elements.into_iter().take(count).collect()
            } else {
                elements.into_iter().skip(count).collect()
            };
            let len = selected.len();
            Ok(sequence::rebuild_selection(&tensor, selected, vec![len]))
        },
        | B::Reverse => {
            let tensor = expect_tensor(args, 0, name, span)?;
            // §9.5: reversing an `f64` tensor is a view — the same buffer
            // walked backwards — so `xs[::-1]` allocates nothing and still
            // shares `xs`'s buffer. §8.1.5: at rank >= 2 `reverse` flips the
            // first axis and keeps the shape (it used to flatten).
            if tensor.rank() >= 1 {
                return sequence::flip_axis(&tensor, 0, span);
            }
            let mut elements = sequence::sequence_items(&tensor, span)?;
            elements.reverse();
            let len = elements.len();
            Ok(sequence::rebuild_selection(&tensor, elements, vec![len]))
        },
        | B::ReverseAxis => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let axis = axis_argument(args, 1, tensor.rank(), name, span)?;
            sequence::flip_axis(&tensor, axis, span)
        },
        | B::Sort => {
            // §3.8.4: `sort` needs `TotalOrd`, and the checker rejects a type
            // without one. The order must therefore be the *total* one —
            // comparing `as_f64` made a tensor of strings sort as `NaN`, which
            // silently left it in source order.
            let tensor = expect_tensor(args, 0, name, span)?;
            // §8.1.5: at rank >= 2 the rows are ordered lexicographically by
            // their elements; the sort is stable.
            if tensor.rank() >= 2 {
                let rows = sequence::sequence_items(&tensor, span)?;
                let order = sequence::sorted_indices(&rows, total_order, span)?;
                return sequence::gather_sequence(&tensor, &rows, &order, span);
            }
            let mut elements = sequence::sequence_items(&tensor, span)?;
            elements.sort_by(total_order);
            let len = elements.len();
            Ok(sequence::rebuild_selection(&tensor, elements, vec![len]))
        },
        | B::Argsort => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let items = sequence::sequence_items(&tensor, span)?;
            let order = sequence::sorted_indices(&items, total_order, span)?;
            let count = order.len();
            if count == 0 {
                return Ok(Value::Tensor(Rc::new(TensorData::new(
                    vec![0],
                    Storage::TypedExact(Vec::new(), ExactRep::Int),
                    Dim::one(),
                    None,
                ))));
            }
            let mut indices = sequence::reserve_items(count, span)?;
            indices.extend(order.into_iter().map(sequence::index_value));
            Ok(build_tensor(indices, vec![count], Dim::one()))
        },
        | B::SortAxis => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let axis = axis_argument(args, 1, tensor.rank(), name, span)?;
            sequence::sort_axis(&tensor, axis, total_order, span)
        },
        | B::SortByKey => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let tensor = expect_tensor(args, 1, name, span)?;
            let items = sequence::sequence_items(&tensor, span)?;
            let mut keys = sequence::reserve_items(items.len(), span)?;
            for item in &items {
                keys.push(interp.apply(function.clone(), vec![item.clone()], span)?);
            }
            let order = sequence::sorted_indices(&keys, total_order, span)?;
            sequence::gather_sequence(&tensor, &items, &order, span)
        },
        | B::Concat => {
            match (args.first(), args.get(1)) {
                | (Some(Value::Str(a)), Some(Value::Str(b))) => {
                    Ok(Value::string(format!("{a}{b}")))
                },
                | (Some(Value::Tensor(a)), Some(Value::Tensor(b))) => sequence::concat(a, b, span),
                | _ => Err(arity(name, 2, args.len(), span)),
            }
        },
        | B::Any | B::All => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let tensor = expect_tensor(args, 1, name, span)?;
            let mut result = name == "all";
            for element in sequence::sequence_items(&tensor, span)? {
                let value = matches!(
                    interp.apply(function.clone(), vec![element], span)?,
                    Value::Bool(true)
                );
                if name == "any" && value {
                    return Ok(Value::Bool(true));
                }
                if name == "all" && !value {
                    return Ok(Value::Bool(false));
                }
                result = value;
            }
            Ok(Value::Bool(result))
        },
        | B::Find => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let tensor = expect_tensor(args, 1, name, span)?;
            for element in sequence::sequence_items(&tensor, span)? {
                if matches!(
                    interp.apply(function.clone(), vec![element.clone()], span)?,
                    Value::Bool(true)
                ) {
                    return Ok(make_variant(adt::OPTION_ID, 1, "Some", vec![element]));
                }
            }
            Ok(make_variant(adt::OPTION_ID, 0, "None", Vec::new()))
        },
        | B::Prod => {
            let tensor = expect_tensor(args, 0, name, span)?;
            // §6.6.6: the empty product is 1, in the phase the elements would
            // have had.
            if tensor.storage.is_empty() {
                return Ok(Value::Num(one_of(&tensor)));
            }
            if let Some(num) = tensor.dense_prod() {
                return Ok(Value::Num(num));
            }
            let mut total: Option<Value> = None;
            for element in tensor.elements() {
                total = Some(match total {
                    | None => element,
                    | Some(acc) => interp.apply_binary("*", acc, element, span)?,
                });
            }
            Ok(total.unwrap_or_else(|| Value::Num(NumVal::int(BigInt::one()))))
        },
        | B::Centroid => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let count = tensor.storage.len();
            let total = tensor.dense_flat().map_or_else(
                || {
                    tensor
                        .elements()
                        .iter()
                        .map(|element| element.as_f64().unwrap_or(f64::NAN))
                        .sum::<f64>()
                },
                |data| data.iter().copied().sum::<f64>(),
            );
            Ok(Value::Num(NumVal::float(
                total / count.max(1) as f64,
                tensor.dim.clone(),
            )))
        },

        // ----- strings ----------------------------------------------------
        | B::Join => {
            // §8.11 overloads `join` on tables; the first argument decides.
            if matches!(args.first(), Some(Value::Record(_))) {
                return join_tables(args, name, span);
            }
            let separator = expect_str(args, 0, name, span)?;
            let tensor = expect_tensor(args, 1, name, span)?;
            let parts: Vec<String> = tensor
                .elements()
                .iter()
                .map(|element| {
                    match element {
                        | Value::Str(text) => text.to_string(),
                        | other => show(other),
                    }
                })
                .collect();
            Ok(Value::string(parts.join(&separator)))
        },
        | B::Split => {
            let text = expect_str(args, 0, name, span)?;
            let separator = expect_str(args, 1, name, span)?;
            let parts: Vec<Value> = if separator.is_empty() {
                text.chars().map(|c| Value::string(c.to_string())).collect()
            } else {
                text.split(&separator)
                    .map(|part| Value::string(part.to_string()))
                    .collect()
            };
            let len = parts.len();
            Ok(build_tensor(parts, vec![len], Dim::one()))
        },
        | B::Replace => {
            let text = expect_str(args, 0, name, span)?;
            let from = expect_str(args, 1, name, span)?;
            let to = expect_str(args, 2, name, span)?;
            Ok(Value::string(text.replace(&from, &to)))
        },
        | B::Trim => Ok(Value::string(expect_str(args, 0, name, span)?.trim())),
        | B::Upper => {
            Ok(Value::string(
                expect_str(args, 0, name, span)?.to_uppercase(),
            ))
        },
        | B::Lower => {
            Ok(Value::string(
                expect_str(args, 0, name, span)?.to_lowercase(),
            ))
        },
        | B::Contains | B::StartsWith | B::EndsWith => {
            let text = expect_str(args, 0, name, span)?;
            let pattern = expect_str(args, 1, name, span)?;
            let value = match name {
                | "contains" => text.contains(&pattern),
                | "starts_with" => text.starts_with(&pattern),
                | _ => text.ends_with(&pattern),
            };
            Ok(Value::Bool(value))
        },
        | B::ParseInt => {
            let text = expect_str(args, 0, name, span)?;
            match text.trim().parse::<BigInt>() {
                | Ok(value) => Ok(ok(Value::Num(NumVal::int(value)))),
                | Err(error) => Ok(err(Value::string(error.to_string()))),
            }
        },
        | B::ParseFloat => {
            let text = expect_str(args, 0, name, span)?;
            match text.trim().parse::<f64>() {
                | Ok(value) => Ok(ok(Value::Num(NumVal::float(value, Dim::one())))),
                | Err(error) => Ok(err(Value::string(error.to_string()))),
            }
        },
        | B::Chars => {
            use unicode_segmentation::UnicodeSegmentation as _;
            let text = expect_str(args, 0, name, span)?;
            let parts: Vec<Value> = text.graphemes(true).map(Value::string).collect();
            let len = parts.len();
            Ok(build_tensor(parts, vec![len], Dim::one()))
        },
        | B::Format => {
            let template = expect_str(args, 0, name, span)?;
            let tensor = expect_tensor(args, 1, name, span)?;
            let parts: Vec<String> = tensor
                .elements()
                .iter()
                .map(|element| {
                    match element {
                        | Value::Str(text) => text.to_string(),
                        | other => show(other),
                    }
                })
                .collect();
            Ok(Value::string(fill_template(&template, &parts)))
        },

        // ----- Option / Result -------------------------------------------
        | B::AndThen => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            match args.get(1) {
                | Some(Value::Variant(variant)) if variant.adt == adt::OPTION_ID => {
                    if variant.index == 0 {
                        Ok(Value::Variant(variant.clone()))
                    } else {
                        let inner = variant.fields.first().cloned().unwrap_or(Value::Unit);
                        interp.apply(function, vec![inner], span)
                    }
                },
                | Some(Value::Variant(variant)) if variant.adt == adt::RESULT_ID => {
                    if variant.index == 1 {
                        Ok(Value::Variant(variant.clone()))
                    } else {
                        let inner = variant.fields.first().cloned().unwrap_or(Value::Unit);
                        interp.apply(function, vec![inner], span)
                    }
                },
                | _ => Err(arity(name, 2, args.len(), span)),
            }
        },
        | B::UnwrapOr => {
            let fallback = args.get(1).cloned().unwrap_or(Value::Unit);
            match args.first() {
                | Some(Value::Variant(variant)) if variant.adt == adt::OPTION_ID => {
                    if variant.index == 1 {
                        Ok(variant.fields.first().cloned().unwrap_or(fallback))
                    } else {
                        Ok(fallback)
                    }
                },
                | Some(Value::Variant(variant)) if variant.adt == adt::RESULT_ID => {
                    if variant.index == 0 {
                        Ok(variant.fields.first().cloned().unwrap_or(fallback))
                    } else {
                        Ok(fallback)
                    }
                },
                | _ => Ok(fallback),
            }
        },
        | B::IsSome | B::IsNone => {
            match args.first() {
                | Some(Value::Variant(variant)) => {
                    Ok(Value::Bool(if name == "is_some" {
                        variant.index == 1
                    } else {
                        variant.index == 0
                    }))
                },
                | _ => Err(arity(name, 1, args.len(), span)),
            }
        },
        | B::IsOk | B::IsErr => {
            match args.first() {
                | Some(Value::Variant(variant)) if variant.adt == adt::RESULT_ID => {
                    Ok(Value::Bool((variant.index == 0) == (name == "is_ok")))
                },
                | _ => Err(arity(name, 1, args.len(), span)),
            }
        },
        | B::Ok => {
            match args.first() {
                | Some(Value::Variant(variant)) if variant.adt == adt::RESULT_ID => {
                    if variant.index == 0 {
                        let inner = variant.fields.first().cloned().unwrap_or(Value::Unit);
                        Ok(make_variant(adt::OPTION_ID, 1, "Some", vec![inner]))
                    } else {
                        Ok(make_variant(adt::OPTION_ID, 0, "None", Vec::new()))
                    }
                },
                | _ => Err(arity(name, 1, args.len(), span)),
            }
        },
        | B::OkOr => {
            let error = args.get(1).cloned().unwrap_or(Value::Unit);
            match args.first() {
                | Some(Value::Variant(variant)) if variant.adt == adt::OPTION_ID => {
                    if variant.index == 1 {
                        Ok(ok(variant.fields.first().cloned().unwrap_or(Value::Unit)))
                    } else {
                        Ok(err(error))
                    }
                },
                | _ => Err(arity(name, 2, args.len(), span)),
            }
        },
        | B::First | B::Last => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let extent = tensor.shape.first().copied().unwrap_or(0);
            if extent == 0 {
                return Ok(make_variant(adt::OPTION_ID, 0, "None", Vec::new()));
            }
            let index = if name == "first" {
                0
            } else {
                extent.saturating_sub(1)
            };
            if tensor.rank() >= 2 {
                let row = sequence::FirstAxis::new(&tensor, span)?.row(index, span)?;
                return Ok(make_variant(adt::OPTION_ID, 1, "Some", vec![row]));
            }
            let element = tensor
                .get_flat(index)
                .ok_or_else(|| EvalError::new("E0802", "index out of range").at(span))?;
            Ok(make_variant(adt::OPTION_ID, 1, "Some", vec![element]))
        },
        | B::Cumsum | B::Cumprod => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let operator = if name == "cumsum" { "+" } else { "*" };
            let mut elements = Vec::with_capacity(tensor.storage.len());
            let mut running: Option<Value> = None;
            for element in tensor.elements() {
                let next = match running.take() {
                    | None => element,
                    | Some(acc) => interp.apply_binary(operator, acc, element, span)?,
                };
                elements.push(next.clone());
                running = Some(next);
            }
            let dim = elements
                .first()
                .map_or_else(|| tensor.dim.clone(), crate::value::value_dim);
            let len = elements.len();
            Ok(build_tensor(elements, vec![len], dim))
        },
        | B::Argmax | B::Argmin => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let mut best: Option<(usize, f64)> = None;
            for (index, element) in tensor.elements().iter().enumerate() {
                let value = element.as_f64().ok_or_else(|| {
                    EvalError::new(
                        "E0302",
                        format!(
                            "`{name}` needs numeric elements, found {}",
                            element.type_name()
                        ),
                    )
                    .at(span)
                })?;
                // A NaN never wins; the first of equal elements does.
                let better = match best {
                    | None => !value.is_nan(),
                    | Some((_, current)) => {
                        if name == "argmax" {
                            value > current
                        } else {
                            value < current
                        }
                    },
                };
                if better {
                    best = Some((index, value));
                }
            }
            let (index, _) = best.ok_or_else(|| {
                EvalError::new("E0802", format!("`{name}` of an empty tensor")).at(span)
            })?;
            Ok(Value::Num(NumVal::int(index)))
        },
        | B::MapErr => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            match args.get(1) {
                | Some(Value::Variant(variant)) if variant.adt == adt::RESULT_ID => {
                    if variant.index == 0 {
                        Ok(Value::Variant(variant.clone()))
                    } else {
                        let inner = variant.fields.first().cloned().unwrap_or(Value::Unit);
                        let mapped = interp.apply(function, vec![inner], span)?;
                        Ok(make_variant(adt::RESULT_ID, 1, "Err", vec![mapped]))
                    }
                },
                | _ => Err(arity(name, 2, args.len(), span)),
            }
        },

        // ----- symbolic ---------------------------------------------------
        | B::Symbol => {
            let display = expect_str(args, 0, name, span)?;
            let count = interp.symbol_counts.entry(display.clone()).or_insert(0);
            *count = count.checked_add(1).ok_or_else(|| {
                EvalError::new("E0808", "symbol identity counter exhausted").at(span)
            })?;
            // The first `x` keeps its name; later ones become `x#2`, `x#3`.
            // The suffix makes identity structural, while `show` hides it.
            let internal = if *count == 1 {
                display
            } else {
                format!("{display}#{count}")
            };
            Ok(Value::Num(NumVal::symbolic(
                Term::Sym(internal),
                Dim::one(),
            )))
        },
        | B::Symfun => {
            // §6.4.5: an unknown function symbol. Its application is the
            // kernel's undetermined-function node `apply(y, t)`, so `diff`
            // keeps `diff(y(t), t)` as a closed form and `dsolve` can solve
            // for it. Every `symfun` call is a fresh function, named like a
            // `symbol`: the first `y` keeps its name, later ones get `#n`.
            let display = expect_str(args, 0, name, span)?;
            let count = interp
                .symbol_counts
                .entry(format!("fn {display}"))
                .or_insert(0);
            *count = count.checked_add(1).ok_or_else(|| {
                EvalError::new("E0808", "symbol identity counter exhausted").at(span)
            })?;
            let internal = if *count == 1 {
                display
            } else {
                format!("{display}#{count}")
            };
            Ok(symfun_value(internal))
        },
        | B::Dsolve => dsolve(interp, args, span),
        | B::Diff => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            // A name that cannot collide with a user symbol (§6.4.1 creates a
            // fresh ξ; this is the same idea without a term-level name type).
            let variable = "x#diff";
            let symbol = Value::Num(NumVal::symbolic(
                Term::Sym(variable.to_string()),
                Dim::one(),
            ));
            let applied = interp.apply(function, vec![symbol], span)?;
            let term = value_to_term(&applied)?;
            let derivative = term.diff(variable)?;
            let dim = crate::value::value_dim(&applied);
            Ok(Value::Native(Rc::new(NativeFn(Box::new(
                move |_interp, args, span| {
                    let argument = args
                        .first()
                        .ok_or_else(|| arity("derivative", 1, 0, span))?;
                    let substituted = derivative.substitute(variable, &value_to_term(argument)?)?;
                    // §6.13 step 4: `df(2)` is the exact number `9`, not a
                    // symbolic value that happens to print as `9`.
                    if let Some(folded) = bridge::fold(&substituted, dim.clone()) {
                        return Ok(folded);
                    }
                    Ok(Value::Num(NumVal::symbolic(substituted, dim.clone())))
                },
            )))))
        },
        | B::DiffAt => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let at = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            let derivative = call(interp, "diff", vec![function], span)?;
            interp.apply(derivative, vec![at], span)
        },
        // ----- calculus, algebra and equations (rssn) ----------------------
        | B::Antiderivative => {
            // The argument is not known yet, so the integral is built against a
            // dimensionless variable and the dimension is recovered when the
            // returned function is applied (`e*d`, §8.6).
            let (variable, applied) = apply_to_fresh(interp, name, args, span, Dim::one())?;
            let symbol = symbolic_variable(&variable, Dim::one());
            let integral = bridge::call_term("integral", &[applied, symbol], Dim::one())?;
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            Ok(Value::Native(Rc::new(NativeFn(Box::new(
                move |interp, args, span| {
                    let argument = args
                        .first()
                        .cloned()
                        .ok_or_else(|| arity("antiderivative", 1, 0, span))?;
                    // `f` is `Real[d] -> Real[e]`; applying it to a variable of
                    // dimension `d` gives `e`, and the antiderivative is `e*d`.
                    let dim = argument_dim(interp, &function, &argument, span)?
                        .mul(&crate::value::value_dim(&argument));
                    let substituted = integral.substitute(&variable, &value_to_term(&argument)?)?;
                    // A value with nothing free in it is a number: fold it
                    // rather than print `1/3*3^3`.
                    if let Some(folded) = bridge::fold(&substituted, dim.clone()) {
                        return Ok(folded);
                    }
                    Ok(Value::Num(NumVal::symbolic(substituted, dim)))
                },
            )))))
        },
        | B::Defint => {
            let lower = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 3, 1, span))?;
            let upper = args
                .get(2)
                .cloned()
                .ok_or_else(|| arity(name, 3, 2, span))?;
            let variable_dim = crate::value::value_dim(&lower);
            let (variable, applied) =
                apply_to_fresh(interp, name, args, span, variable_dim.clone())?;
            let symbol = symbolic_variable(&variable, variable_dim);
            let dim = crate::value::value_dim(&applied).mul(&crate::value::value_dim(&lower));
            Ok(math_result(bridge::call(
                "defint",
                &[applied, symbol, lower, upper],
                dim,
            )))
        },
        | B::Limit => {
            let at = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            let variable_dim = crate::value::value_dim(&at);
            let (variable, applied) =
                apply_to_fresh(interp, name, args, span, variable_dim.clone())?;
            let symbol = symbolic_variable(&variable, variable_dim);
            let dim = crate::value::value_dim(&applied);
            Ok(math_result(bridge::call(
                "limit",
                &[applied, symbol, at],
                dim,
            )))
        },
        | B::Series => {
            let at = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 3, 1, span))?;
            let order = args
                .get(2)
                .cloned()
                .ok_or_else(|| arity(name, 3, 2, span))?;
            let variable_dim = crate::value::value_dim(&at);
            let (variable, applied) =
                apply_to_fresh(interp, name, args, span, variable_dim.clone())?;
            let symbol = symbolic_variable(&variable, variable_dim);
            let dim = crate::value::value_dim(&applied);
            bridge::call("taylor", &[applied, symbol, at, order], dim)
        },
        | B::Solve => {
            // No argument fixes the variable's dimension, so the roots stay
            // dimensionless at run time; the static type is `Solutions<Real,d>`.
            let (variable, applied) = apply_to_fresh(interp, name, args, span, Dim::one())?;
            let symbol = symbolic_variable(&variable, Dim::one());
            match solve("solve", &applied, &symbol) {
                | Ok(solutions) => Ok(ok(solutions_value(&solutions, Dim::one()))),
                | Err(error) => Ok(err(Value::string(error.message))),
            }
        },
        | B::Nsolve => {
            let start = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            let variable_dim = crate::value::value_dim(&start);
            let (variable, applied) =
                apply_to_fresh(interp, name, args, span, variable_dim.clone())?;
            let symbol = symbolic_variable(&variable, variable_dim.clone());
            let dim = variable_dim;
            // `nsolve` only answers in the numeric phase; the symbolic phase
            // has no root to give.
            Ok(math_result(bridge::compute(
                "nsolve",
                &[applied, symbol, start],
                &aevia_rssn::Opts::numeric(1e-10),
                dim,
            )))
        },
        | B::Eval | B::Simplify => {
            let value = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            let dim = crate::value::value_dim(&value);
            let term = value_to_term(&value)?;
            let sym = bridge::term_val(&term)?;
            let aevia_rssn::Val::Term(handle) = sym else {
                return Ok(value);
            };
            let computed = aevia_rssn::kernel()
                .compute(handle, &aevia_rssn::Opts::symbolic())
                .map_err(|failure| bridge::error(&failure))?;
            Ok(bridge::from_val(&computed.val, dim))
        },
        | B::Approx => {
            let value = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            let dim = crate::value::value_dim(&value);
            // A concrete value is already its own approximation: `approx(9)`
            // is `Ok(9.0)` (§6.13 step 5), not a kernel round trip.
            if let Value::Num(number) = &value
                && !number.is_symbolic()
                && let Some(approximate) = number.to_f64()
            {
                return Ok(ok(Value::Num(NumVal::float(approximate, dim))));
            }
            let term = value_to_term(&value)?;
            // `pi` and `e` are the mathematical constants, not unknowns: an
            // exact unit scale (`30 deg`, `hbar` in natural units) carries
            // `pi` and is still a number (`Term::is_numeric` agrees).
            let free: Vec<String> = term
                .try_free_symbols()?
                .into_iter()
                .filter(|name| name != "pi" && name != "e")
                .collect();
            if !free.is_empty() {
                return Ok(err(Value::string(format!(
                    "free symbols: {}",
                    free.join(", ")
                ))));
            }
            let sym = bridge::term_val(&term)?;
            let aevia_rssn::Val::Term(handle) = sym else {
                return Ok(err(Value::string("not numeric")));
            };
            match aevia_rssn::kernel().compute(handle, &aevia_rssn::Opts::numeric(1e-12)) {
                | Ok(computed) => {
                    match computed.numeric {
                        // §6.3.4: a *formal* expression outside its domain — the
                        // numeric target answers `NaN` for `asin(2)` — is a domain
                        // error `E0806`, not a successful `NaN`. A concrete `NaN`
                        // program value still approximates to itself above.
                        | Some(number) if number.is_nan() => {
                            Ok(err(Value::string(
                                "E0806: the expression has no real value (domain error)",
                            )))
                        },
                        | Some(number) => Ok(ok(Value::Num(NumVal::float(number, dim)))),
                        | None => {
                            Ok(direct_approx(&term, dim)
                                .unwrap_or_else(|| err(Value::string("not numeric"))))
                        },
                    }
                },
                // A closed term the kernel's numeric target declines (an
                // exact rational times a power of `pi` from a unit scale) is
                // still a number: evaluate it directly.
                | Err(failure) => {
                    Ok(direct_approx(&term, dim)
                        .unwrap_or_else(|| err(Value::string(failure.message))))
                },
            }
        },
        | B::Subst => {
            let symbol = expect_num(args, 0, name, span)?;
            let replacement = args.get(1).ok_or_else(|| arity(name, 3, 1, span))?;
            let target = args.get(2).ok_or_else(|| arity(name, 3, 2, span))?;
            let dim = crate::value::value_dim(target);
            let replacement = value_to_term(replacement)?;
            let target = value_to_term(target)?;
            let Some(symbol_name) = symbol_name(&symbol) else {
                return Err(EvalError::new("E0302", "`subst` expects a symbol").at(span));
            };
            let substituted = target.substitute(&symbol_name, &replacement)?;
            // A substitution that leaves nothing free is a number, not a
            // symbolic value: `subst(x, 1, x^2)` is `1`, and `1 == 1` is a
            // `Bool` (§6.3.6).
            if let Some(folded) = bridge::fold(&substituted, dim.clone()) {
                return Ok(folded);
            }
            Ok(Value::Num(NumVal::symbolic(substituted, dim)))
        },
        | B::ToValue => {
            let value = args.first().ok_or_else(|| arity(name, 1, 0, span))?;
            let payload = bridge::to_val(value)?;
            let host_witness = match value {
                | Value::KernelValue(existing) => existing.host_witness.clone(),
                | _ => {
                    crate::host_witness::runtime_schema(value)
                        .and_then(|schema| crate::host_witness::capture(value, &schema, false))
                        .map(Rc::new)
                },
            };
            let mut witness = crate::rssn_module::host_dimension_witness(value);
            // A tagged witness lets `from_value` decode an `Option` by its
            // structure, which is right for what the kernel returned. A host
            // variant without host evidence proves no element type, so its
            // tags must not become that proof: `None` from an `Option<Int>`
            // would otherwise read back as any `Option<T>`.
            if host_witness.is_none() && !matches!(value, Value::KernelValue(_)) {
                forget_variant_witnesses(&mut witness);
            }
            Ok(Value::KernelValue(Rc::new(crate::value::KernelValue {
                payload,
                witness,
                host_witness,
            })))
        },
        | B::ToCallback => {
            let [function] = args else {
                return Err(arity(name, 1, args.len(), span));
            };
            let schema = Interp::concrete_callable_type(function).ok_or_else(|| {
                EvalError::new(
                    "E0304",
                    "`to_callback` requires an authenticated concrete function signature",
                )
                .at(span)
            })?;
            let function = interp.reify_callable(function, &schema)?;
            Ok(Value::Callback(Rc::new(crate::value::CallbackValue {
                function,
                schema,
            })))
        },
        | B::FromValue => {
            // Plain host or higher-order application carries no authenticated source call evidence.
            let result: Result<Value, String> =
                Err("missing concrete result type evidence".to_string());
            Ok(match result {
                | Ok(value) => ok(value),
                | Err(reason) => err(crate::rssn_module::value_error("TypeMismatch", reason)),
            })
        },
        | B::Decide => Ok(decide(args, span)),
        | B::Assuming | B::ByCases => branch_on(interp, name, args, span),

        // ----- IO ---------------------------------------------------------
        | B::ReadFile => {
            let path = expect_str(args, 0, name, span)?;
            Ok(Value::Io(Rc::new(crate::value::IoAction::ReadFile(path))))
        },
        | B::WriteFile => {
            let path = expect_str(args, 0, name, span)?;
            let contents = expect_str(args, 1, name, span)?;
            Ok(Value::Io(Rc::new(crate::value::IoAction::WriteFile {
                path,
                contents,
            })))
        },
        | B::ReadLine => Ok(Value::Io(Rc::new(crate::value::IoAction::ReadLine))),
        | B::Input => {
            let prompt = expect_str(args, 0, name, span)?;
            Ok(Value::Io(Rc::new(crate::value::IoAction::Input(prompt))))
        },
        | B::LoadPlugin => {
            let path = match args.first() {
                | Some(Value::Str(path)) => path.to_string(),
                | _ => return Err(arity(name, 1, args.len(), span)),
            };
            Ok(Value::Io(Rc::new(crate::value::IoAction::LoadPlugin(path))))
        },
        // §9.12: the program arguments the host set with `Interp::set_args`,
        // as a rank-1 `Tensor<1, String>`.
        | B::Args => {
            if !args.is_empty() {
                return Err(arity(name, 0, args.len(), span));
            }
            let elements: Vec<Value> = interp
                .program_args
                .iter()
                .map(|argument| Value::string(argument.clone()))
                .collect();
            let len = elements.len();
            Ok(build_tensor(elements, vec![len], Dim::one()))
        },
        // §6.8.2: the clock is read when the action runs, and `random` advances
        // the RNG then too — building the description must not consume a value
        // from the sequence (§6.10).
        | B::TimeNow => Ok(Value::Io(Rc::new(crate::value::IoAction::TimeNow))),
        // §9.14 / §13.9.6: `with_cap("fs,rand", thunk)` describes running the
        // thunk's action with only those capabilities left; the narrowing
        // happens when the action runs, and nested blocks intersect.
        | B::WithCap => {
            let list = expect_str(args, 0, name, span)?;
            let allowed = crate::value::caps::parse(&list).map_err(|unknown| {
                EvalError::new(
                    "E0302",
                    format!("`{unknown}` is not a capability; the capabilities are fs, net, env, rand, gpu"),
                )
                .at(span)
            })?;
            let action = match args.get(1) {
                | Some(Value::Closure(thunk)) => {
                    Rc::new(crate::value::IoAction::Defer(thunk.clone()))
                },
                | Some(Value::Io(action)) => action.clone(),
                | _ => {
                    return Err(EvalError::new(
                        "E0302",
                        "`with_cap` expects a function `() -> IO a` as its second argument",
                    )
                    .at(span));
                },
            };
            Ok(Value::Io(Rc::new(crate::value::IoAction::WithCaps {
                allowed,
                action,
            })))
        },
        | B::Random => Ok(Value::Io(Rc::new(crate::value::IoAction::Random))),
        // §6.10: the pure, seeded generator. `rng(seed)` answers `() -> Float`,
        // a closure over its own LCG state, so the same seed always produces
        // the same stream and nothing is consumed from the session's RNG — the
        // purity the standard asks for ("rng(seed) 是纯的、确定的").
        | B::Rng => {
            let seed = expect_int(args, 0, name, span)?;
            let state = Rc::new(std::cell::RefCell::new(u64::from_ne_bytes(
                seed.to_ne_bytes(),
            )));
            Ok(Value::Native(Rc::new(NativeFn(Box::new(
                move |_interp: &mut Interp, _args: Vec<Value>, _span: Span| {
                    let value = crate::eval::next_unit(&mut state.borrow_mut());
                    Ok(Value::float(value))
                },
            )))))
        },

        // ----- testing ----------------------------------------------------
        | B::Assert => {
            match args.first() {
                | Some(Value::Bool(true)) => {
                    Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(
                        Value::Unit,
                    ))))
                },
                // §6.8.1: an assertion is part of the description. Failing while
                // the block is built would run it before the `println` above it.
                | Some(Value::Bool(false)) => {
                    Ok(Value::Io(Rc::new(crate::value::IoAction::Assert {
                        message: "assertion failed".to_string(),
                    })))
                },
                | Some(Value::Prop(prop)) => {
                    match decide_prop(prop) {
                        | Some(true) => {
                            Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(
                                Value::Unit,
                            ))))
                        },
                        | Some(false) => {
                            Ok(Value::Io(Rc::new(crate::value::IoAction::Assert {
                                message: format!("assertion failed: {}", prop.text),
                            })))
                        },
                        | None => {
                            Err(
                                EvalError::new("E0804", format!("cannot decide `{}`", prop.text))
                                    .at(span),
                            )
                        },
                    }
                },
                | _ => Err(arity(name, 1, args.len(), span)),
            }
        },
        | B::AssertEq => {
            let a = args.first().cloned().unwrap_or(Value::Unit);
            let b = args.get(1).cloned().unwrap_or(Value::Unit);
            // §6.12.1: equality is structural. Comparing renderings passed
            // `1` against `1.0` and `1/2` against `0.5`, which is the very
            // mistake the phase split exists to prevent.
            match crate::value::equal(&a, &b) {
                | Ok(true) => {
                    Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(
                        Value::Unit,
                    ))))
                },
                | Ok(false) => {
                    Ok(Value::Io(Rc::new(crate::value::IoAction::Assert {
                        message: format!(
                            "assertion failed: {} != {}",
                            crate::value::repr(&a),
                            crate::value::repr(&b)
                        ),
                    })))
                },
                | Err(reason) => Err(EvalError::new("E0302", reason.to_string()).at(span)),
            }
        },
        | B::ApproxEq => {
            let a = expect_num(args, 0, name, span)?;
            let b = expect_num(args, 1, name, span)?;
            let tolerance = expect_num(args, 2, name, span)?;
            let tolerance = NumVal {
                kind: match tolerance.kind {
                    | NumKind::Int(value) => NumKind::Int(value.abs()),
                    | NumKind::Exact(value) => NumKind::Exact(Rc::new(value.abs())),
                    | NumKind::Float(value) => NumKind::Float(value.abs()),
                    | other => other,
                },
                dim: tolerance.dim,
                display: tolerance.display,
            };
            // §7.5.4: all three arguments share a dimension; comparing `1 m`
            // with `1 s` is a bug, not an approximation.
            if a.dim != b.dim || a.dim != tolerance.dim {
                return Err(EvalError::new(
                    "E0401",
                    format!(
                        "`approx_eq` needs one dimension, found {} and {} and {}",
                        a.dim.to_plain_string(),
                        b.dim.to_plain_string(),
                        tolerance.dim.to_plain_string()
                    ),
                )
                .at(span));
            }
            if let (Some(left), Some(right), Some(limit)) =
                (a.to_rational(), b.to_rational(), tolerance.to_rational())
            {
                // BigRational subtraction cannot overflow.
                #[allow(clippy::arithmetic_side_effects)]
                let difference = (left - right).abs();
                return Ok(Value::Bool(difference <= limit));
            }
            let (Some(left), Some(right), Some(limit)) =
                (a.to_f64(), b.to_f64(), tolerance.to_f64())
            else {
                return Ok(Value::Bool(false));
            };
            Ok(Value::Bool(
                !limit.is_nan()
                    && (left
                        .partial_cmp(&right)
                        .is_some_and(std::cmp::Ordering::is_eq)
                        || (left.is_finite()
                            && right.is_finite()
                            && (left - right).abs() <= limit)),
            ))
        },
        | B::Forall => forall(interp, args, span),

        // ----- plotting ---------------------------------------------------
        | B::Label => {
            let mut series = match args.first().cloned() {
                | Some(Value::Record(record)) => (*record).clone(),
                | _ => return Err(arity(name, 2, args.len(), span)),
            };
            let text = expect_str(args, 1, name, span)?;
            series
                .fields
                .insert("label".to_string(), Value::string(text));
            Ok(Value::Record(Rc::new(series)))
        },
        | B::Combine => {
            let series = args.first().cloned().unwrap_or(Value::Unit);
            let mut fields = IndexMap::new();
            fields.insert("series".to_string(), series);
            Ok(Value::Record(Rc::new(RecordValue { fields })))
        },
        | B::Plot | B::Save => {
            let spec = args.last().unwrap_or(&Value::Unit);
            if name == "plot" {
                let html = plot::render_document(spec, false, &interp.units, span)?;
                Ok(Value::Io(Rc::new(crate::value::IoAction::Plot(html))))
            } else {
                let path = expect_str(args, 0, name, span)?;
                // The path's own extension decides whether SVG is standalone.
                let svg = std::path::Path::new(&path)
                    .extension()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
                let html = plot::render_document(spec, svg, &interp.units, span)?;
                Ok(Value::Io(Rc::new(crate::value::IoAction::Save {
                    path,
                    html,
                })))
            }
        },
        | B::PlotSaveGif => {
            let path = expect_str(args, 0, name, span)?;
            let result = match args.get(1) {
                | Some(Value::Tensor(specs)) => {
                    plot::render_animation(specs, args.get(2), &interp.units, span)
                },
                | _ => Err("plot_save_gif needs a vector of plot specifications".to_string()),
            };
            let action = match result {
                | Ok(animation) => crate::value::IoAction::WriteGif { path, animation },
                | Err(error) => {
                    crate::value::IoAction::Pure(err(Value::string(format!("E0902: {error}"))))
                },
            };
            Ok(Value::Io(Rc::new(action)))
        },

        // ----- §8.2 constants ---------------------------------------------
        // `Interp::seed_constants` binds these in the global scope, which is
        // where a bare `pi` is found. These arms exist so that reaching the
        // builtin table — `map(pi, xs)`, or a call through `apply` — gives the
        // same value instead of "typed but not implemented".
        | B::Pi | B::E | B::Oo => {
            if name == "e" {
                let one_value = Value::Num(NumVal::int(BigInt::one()));
                return crate::kernel::call("exp", &[one_value], Dim::one());
            }
            crate::kernel::call(name, &[], Dim::one())
        },
        | B::Nan => Ok(Value::Num(NumVal::float(f64::NAN, Dim::one()))),
        | B::I => {
            Ok(Value::Num(NumVal {
                kind: NumKind::Complex(0.0, 1.0),
                dim: Dim::one(),
                display: None,
            }))
        },
        | B::Lgamma => {
            let num = expect_num(args, 0, name, span)?;
            let value = num.to_f64().unwrap_or(f64::NAN);
            Ok(Value::Num(NumVal::float(
                aevia_std::special::ln_gamma(value),
                num.dim,
            )))
        },

        // ----- §8.1.5 containers that arrived with the chapter-8 sweep -----
        | B::Enumerate => {
            let tensor = expect_tensor(args, 0, name, span)?;
            // §8.1.5: at rank >= 2 `value` is a row.
            let items = sequence::sequence_items(&tensor, span)?;
            let mut elements = sequence::reserve_items(items.len(), span)?;
            for (index, element) in items.into_iter().enumerate() {
                elements.push(record_of(vec![
                    ("index", sequence::index_value(index)),
                    ("value", element),
                ]));
            }
            let len = elements.len();
            Ok(build_tensor(elements, vec![len], Dim::one()))
        },
        | B::Zip => {
            let left = expect_tensor(args, 0, name, span)?;
            let right = expect_tensor(args, 1, name, span)?;
            // §8.1.5: it pairs rows (or scalars) as far as the shorter runs.
            let left = sequence::sequence_items(&left, span)?;
            let right = sequence::sequence_items(&right, span)?;
            let mut elements = sequence::reserve_items(left.len().min(right.len()), span)?;
            for (a, b) in left.into_iter().zip(right) {
                elements.push(tuple(vec![a, b]));
            }
            let len = elements.len();
            Ok(build_tensor(elements, vec![len], Dim::one()))
        },
        | B::FoldLeft => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 3, 0, span))?;
            let mut accumulator = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 3, 1, span))?;
            let tensor = expect_tensor(args, 2, name, span)?;
            // §8.1.5: the step sees a row of a tensor of rank >= 2.
            for element in sequence::sequence_items(&tensor, span)? {
                accumulator = interp.apply(function.clone(), vec![accumulator, element], span)?;
            }
            Ok(accumulator)
        },
        | B::SortBy => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let tensor = expect_tensor(args, 1, name, span)?;
            // §8.1.5: the comparator sees two rows at rank >= 2.
            let items = sequence::sequence_items(&tensor, span)?;
            // A merge sort over indices: the comparator is an Aevia function,
            // so it can fail, and a fallible comparator cannot be handed to
            // the standard library's sort. It is stable.
            let order = sequence::merge_sort_indices(
                items.len(),
                &mut |left, right| {
                    let (Some(left), Some(right)) = (items.get(left), items.get(right)) else {
                        return Err(EvalError::new("E0802", "invalid internal sort index").at(span));
                    };
                    match interp.apply(function.clone(), vec![left.clone(), right.clone()], span)? {
                        | Value::Variant(variant) => Ok(variant.index == 2),
                        | other => {
                            Err(EvalError::new(
                                "E0302",
                                format!(
                                    "a comparator must return an `Ordering`, found {}",
                                    other.type_name()
                                ),
                            )
                            .at(span))
                        },
                    }
                },
                span,
            )?;
            sequence::gather_sequence(&tensor, &items, &order, span)
        },
        | B::SortFloat => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let nan_last = args
                .get(1)
                .is_none_or(|value| matches!(value, Value::Bool(true)));
            // At rank >= 2 the rows are ordered lexicographically with the
            // same NaN placement.
            let items = sequence::sequence_items(&tensor, span)?;
            let order = sequence::sorted_indices(
                &items,
                |left, right| sequence::float_sequence_order(left, right, nan_last),
                span,
            )?;
            sequence::gather_sequence(&tensor, &items, &order, span)
        },

        // ----- §8.1.1 combinators ----------------------------------------
        | B::Id | B::Const => Ok(args.first().cloned().unwrap_or(Value::Unit)),
        | B::Apply => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let argument = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            interp.apply(function, vec![argument], span)
        },
        | B::Compose => {
            let outer = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let inner = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            Ok(native(move |interp, call_args, span| {
                let value = interp.apply(inner.clone(), call_args, span)?;
                interp.apply(outer.clone(), vec![value], span)
            }))
        },
        | B::Flip => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            Ok(native(move |interp, mut call_args, span| {
                if call_args.len() >= 2 {
                    call_args.swap(0, 1);
                }
                interp.apply(function.clone(), call_args, span)
            }))
        },
        | B::Curry => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            Ok(native(move |_interp, call_args, _span| {
                let first = call_args.first().cloned().unwrap_or(Value::Unit);
                let function = function.clone();
                Ok(native(move |interp, rest, span| {
                    let mut all = vec![first.clone()];
                    all.extend(rest);
                    interp.apply(function.clone(), all, span)
                }))
            }))
        },
        | B::Uncurry => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            Ok(native(move |interp, call_args, span| {
                let mut current = interp.apply(
                    function.clone(),
                    vec![call_args.first().cloned().unwrap_or(Value::Unit)],
                    span,
                )?;
                for argument in call_args.iter().skip(1) {
                    current = interp.apply(current, vec![argument.clone()], span)?;
                }
                Ok(current)
            }))
        },
        | B::Partial => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let bound = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            Ok(native(move |interp, rest, span| {
                let mut all = vec![bound.clone()];
                all.extend(rest);
                interp.apply(function.clone(), all, span)
            }))
        },

        // ----- §8.1.2 booleans and comparisons ----------------------------
        | B::Not => {
            match args.first() {
                | Some(Value::Bool(flag)) => Ok(Value::Bool(!flag)),
                | other => {
                    Err(EvalError::new(
                        "E0302",
                        format!(
                            "`not` expects a Bool, found {}",
                            other.map_or("nothing", Value::type_name)
                        ),
                    )
                    .at(span))
                },
            }
        },
        | B::Xor => {
            match (args.first(), args.get(1)) {
                | (Some(Value::Bool(a)), Some(Value::Bool(b))) => Ok(Value::Bool(a != b)),
                | _ => Err(EvalError::new("E0302", "`xor` expects two Bools").at(span)),
            }
        },
        | B::Compare => {
            let left = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let right = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            match compare_values(&left, &right) {
                | Some(ordering) => Ok(some(ordering_value(ordering))),
                | None => Ok(none()),
            }
        },
        | B::PartialCompare => {
            let left = expect_float(args, 0, name, span)?;
            let right = expect_float(args, 1, name, span)?;
            if left.is_nan() || right.is_nan() {
                Ok(none())
            } else {
                Ok(some(ordering_value(
                    left.partial_cmp(&right)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )))
            }
        },
        | B::TotalCompare => {
            let left = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let right = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            Ok(ordering_value(total_order(&left, &right)))
        },
        // §3.8.4: `hash` is the `Hash` class method. The checker proves a
        // prelude instance, so the runtime applies the structural hash.
        | B::Hash => {
            let value = args.first().ok_or_else(|| arity(name, 1, 0, span))?;
            hash_value(value).map_or_else(
                || {
                    Err(EvalError::new(
                        "E0310",
                        format!("{} has no `Hash` instance", value.type_name()),
                    )
                    .at(span))
                },
                |hash| Ok(Value::int(i64::from_ne_bytes(hash.to_ne_bytes()))),
            )
        },
        | B::SymEq | B::SymNe | B::SymLt => {
            let left = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let right = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            let op = match name {
                | "sym_eq" => BinOp::Eq,
                | "sym_ne" => BinOp::Ne,
                | _ => BinOp::Lt,
            };
            Ok(proposition(op, left, right))
        },

        // ----- §8.1.3 `zero` ----------------------------------------------
        // `zero` is the identity of `fold((+), zero, xs)`. The dimension comes
        // from the static type, so the runtime value is dimensionless; adding
        // it to a dimensioned value needs the representation to be passed at
        // run time (see the report).
        | B::Zero => Ok(Value::Num(NumVal::exact(BigRational::zero(), Dim::one()))),

        // ----- §8.2.2 special functions -----------------------------------
        | B::Beta => {
            let a = expect_float(args, 0, name, span)?;
            let b = expect_float(args, 1, name, span)?;
            Ok(Value::float(aevia_std::special::beta(a, b)))
        },
        | B::Zeta => {
            let x = expect_float(args, 0, name, span)?;
            Ok(Value::float(aevia_std::special::zeta(x)))
        },
        | B::BesselJ0 => {
            Ok(Value::float(aevia_std::special::bessel_j0(expect_float(
                args, 0, name, span,
            )?)))
        },
        | B::BesselJ1 | B::J1 => {
            Ok(Value::float(aevia_std::special::bessel_j1(expect_float(
                args, 0, name, span,
            )?)))
        },
        | B::BesselY0 => {
            Ok(Value::float(aevia_std::special::bessel_y0(expect_float(
                args, 0, name, span,
            )?)))
        },
        | B::BesselY1 | B::Y1 => {
            Ok(Value::float(aevia_std::special::bessel_y1(expect_float(
                args, 0, name, span,
            )?)))
        },
        | B::BesselJn | B::BesselYn => {
            let order = expect_int(args, 0, name, span)?;
            let x = expect_float(args, 1, name, span)?;
            let order = i32::try_from(order).map_err(|_| {
                EvalError::new(
                    "E0802",
                    format!("`{name}` needs an order in the signed 32-bit range"),
                )
                .at(span)
            })?;
            Ok(Value::float(if name == "bessel_jn" {
                aevia_std::special::bessel_jn(order, x)
            } else {
                aevia_std::special::bessel_yn(order, x)
            }))
        },
        | B::LegendreP | B::HermiteH | B::LaguerreL | B::ChebyshevT | B::ChebyshevU => {
            classical_polynomial(args, name, span)
        },

        // ----- §8.15 complex ----------------------------------------------
        | B::Complex => {
            let re = expect_num(args, 0, name, span)?;
            let im = expect_num(args, 1, name, span)?;
            if re.dim != im.dim {
                return Err(EvalError::new(
                    "E0401",
                    "the real and imaginary parts must share a dimension",
                )
                .at(span));
            }
            let (Some(real), Some(imaginary)) = (re.to_f64(), im.to_f64()) else {
                return Err(
                    EvalError::new("E0806", "`complex` expects concrete real parts").at(span),
                );
            };
            Ok(Value::Num(NumVal {
                kind: NumKind::Complex(real, imaginary),
                dim: re.dim,
                display: None,
            }))
        },
        | B::Re | B::Im | B::Conj | B::Magnitude | B::Arg => {
            let num = expect_num(args, 0, name, span)?;
            let NumKind::Complex(re, im) = num.kind else {
                return Err(
                    EvalError::new("E0302", format!("`{name}` expects a complex number")).at(span),
                );
            };
            Ok(match name {
                | "re" => Value::Num(NumVal::float(re, num.dim)),
                | "im" => Value::Num(NumVal::float(im, num.dim)),
                | "conj" => {
                    Value::Num(NumVal {
                        kind: NumKind::Complex(re, -im),
                        dim: num.dim,
                        display: None,
                    })
                },
                | "magnitude" => Value::Num(NumVal::float(re.hypot(im), num.dim)),
                | _ => Value::Num(NumVal::float(im.atan2(re), Dim::one())),
            })
        },
        | B::ComplexSqrt | B::ComplexExp | B::ComplexLn | B::ComplexSin | B::ComplexCos => {
            let num = expect_num(args, 0, name, span)?;
            let NumKind::Complex(re, im) = num.kind else {
                return Err(
                    EvalError::new("E0302", format!("`{name}` expects a complex number")).at(span),
                );
            };
            let value = aevia_std::Complex::new(re, im);
            let result = match name {
                | "complex_sqrt" => value.sqrt(),
                | "complex_exp" => value.exp(),
                | "complex_ln" => value.ln(),
                | "complex_sin" => aevia_std::complex::sin(value),
                | _ => aevia_std::complex::cos(value),
            };
            let dim = if name == "complex_sqrt" {
                num.dim
                    .pow(&BigRational::new(BigInt::one(), BigInt::from(2)))
            } else {
                num.dim
            };
            Ok(Value::Num(NumVal {
                kind: NumKind::Complex(result.re, result.im),
                dim,
                display: None,
            }))
        },

        // ----- §8.16 number theory, combinatorics, geometry ---------------
        | B::Factor
            if args.first().is_some_and(|value| {
                !matches!(
                    value,
                    Value::Num(NumVal {
                        kind: NumKind::Int(_),
                        ..
                    })
                )
            }) =>
        {
            algebra("factor", args, span)
        },
        | B::Expand | B::Cancel | B::Together | B::Apart | B::Collect => algebra(name, args, span),
        | B::SetOf => {
            let tensor = expect_tensor(args, 0, name, span)?;
            Ok(make_set(tensor.elements()))
        },
        | B::Elements => {
            let items = expect_set(args, 0, name, span)?;
            let dim = items.first().map_or_else(Dim::one, crate::value::value_dim);
            let len = items.len();
            Ok(build_tensor(items.to_vec(), vec![len], dim))
        },
        | B::Member => {
            let needle = args.first().ok_or_else(|| arity(name, 2, 0, span))?;
            let items = expect_set(args, 1, name, span)?;
            Ok(Value::Bool(
                items
                    .binary_search_by(|item| total_order(item, needle))
                    .is_ok(),
            ))
        },
        | B::SetInsert | B::SetRemove => {
            let items = expect_set(args, 0, name, span)?;
            let item = args.get(1).ok_or_else(|| arity(name, 2, 1, span))?;
            let mut next = items.to_vec();
            match (
                next.binary_search_by(|existing| total_order(existing, item)),
                name,
            ) {
                | (Err(position), "set_insert") => next.insert(position, item.clone()),
                | (Ok(position), "set_remove") => {
                    next.remove(position);
                },
                | _ => return Ok(set_value(next)),
            }
            Ok(set_value(next))
        },
        | B::Union | B::Intersection | B::Difference | B::IsSubset => {
            let left = expect_set(args, 0, name, span)?;
            let right = expect_set(args, 1, name, span)?;
            let mut left = left.iter().peekable();
            let mut right = right.iter().peekable();
            let mut out = Vec::new();
            while let Some(a) = left.peek() {
                let Some(b) = right.peek() else {
                    if name == "is_subset" {
                        return Ok(Value::Bool(false));
                    }
                    if matches!(name, "union" | "difference") {
                        out.extend(left.by_ref().cloned());
                    }
                    break;
                };
                let order = total_order(a, b);
                if order.is_lt() && name == "is_subset" {
                    return Ok(Value::Bool(false));
                }
                if !order.is_gt() {
                    let keep = if order.is_eq() {
                        matches!(name, "union" | "intersection")
                    } else {
                        matches!(name, "union" | "difference")
                    };
                    if let Some(value) = left.next().filter(|_| keep) {
                        out.push(value.clone());
                    }
                }
                if !order.is_lt() {
                    let keep = order.is_gt() && name == "union";
                    if let Some(value) = right.next().filter(|_| keep) {
                        out.push(value.clone());
                    }
                }
            }
            if name == "is_subset" {
                return Ok(Value::Bool(true));
            }
            if name == "union" {
                out.extend(right.cloned());
            }
            Ok(set_value(out))
        },
        | B::Degree => {
            let value = algebra("degree", args, span)?;
            match value {
                | Value::Num(NumVal {
                    kind: NumKind::Int(_),
                    ..
                }) => Ok(value),
                | other => {
                    Err(EvalError::new(
                        "E0808",
                        format!(
                            "`degree` needs a polynomial in the variable, got {}",
                            crate::value::show(&other)
                        ),
                    )
                    .at(span))
                },
            }
        },
        | B::Factor => {
            let n = expect_int(args, 0, name, span)?;
            let factors = aevia_std::number::factor(n);
            let elements: Vec<Value> = factors
                .into_iter()
                .map(|(prime, exponent)| {
                    tuple(vec![Value::int(prime), Value::int(i64::from(exponent))])
                })
                .collect();
            let len = elements.len();
            Ok(build_tensor(elements, vec![len], Dim::one()))
        },
        | B::IsPrime => {
            Ok(Value::Bool(aevia_std::number::is_prime(
                u64::try_from(expect_int(args, 0, name, span)?).unwrap_or(0),
            )))
        },
        | B::Primes => {
            let limit = expect_int(args, 0, name, span)?;
            let values = aevia_std::number::primes(limit);
            let len = values.len();
            Ok(Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape: vec![len],
                storage: Storage::TypedExact(
                    values
                        .into_iter()
                        .map(|value| BigRational::from_integer(BigInt::from(value)))
                        .collect(),
                    ExactRep::Int,
                ),
                dim: Dim::one(),
                display: None,
            })))
        },
        | B::Binomial => {
            let n = expect_int(args, 0, name, span)?;
            let k = expect_int(args, 1, name, span)?;
            Ok(Value::Num(NumVal::int(aevia_std::number::binomial(n, k))))
        },
        | B::Permutations => {
            let n = expect_int(args, 0, name, span)?;
            Ok(Value::Num(NumVal::int(aevia_std::number::permutations(n))))
        },
        | B::PolygonArea => {
            let tensor = expect_tensor(args, 0, name, span)?;
            if !matches!(tensor.shape.as_slice(), [_, 2]) {
                return Err(EvalError::new(
                    "E0802",
                    format!(
                        "`polygon_area` expects shape [n, 2], found {:?}",
                        tensor.shape
                    ),
                )
                .at(span));
            }
            let points = tensor_entries(&tensor).map_err(|error| error.at(span))?;
            let area = aevia_std::geom::polygon_area(&points);
            Ok(Value::Num(NumVal::float(
                area,
                tensor.dim.pow(&BigRational::from_integer(BigInt::from(2))),
            )))
        },
        | B::TetrahedronVolume => {
            let vertices = expect_tensor(args, 0, name, span)?;
            if vertices.shape != [4, 3] {
                return Err(EvalError::new(
                    "E0802",
                    format!(
                        "`tetrahedron_volume` expects shape [4, 3], found {:?}",
                        vertices.shape
                    ),
                )
                .at(span));
            }
            let points = tensor_entries(&vertices).map_err(|error| error.at(span))?;
            if points.iter().any(|point| !point.is_finite()) {
                return Err(EvalError::new(
                    "E0806",
                    "`tetrahedron_volume` needs finite coordinates",
                )
                .at(span));
            }
            let volume = aevia_std::geom::tetrahedron_volume(&points);
            if !volume.is_finite() {
                return Err(EvalError::new(
                    "E0806",
                    "`tetrahedron_volume` could not compute a finite volume",
                )
                .at(span));
            }
            Ok(Value::Num(NumVal::float(
                volume,
                vertices
                    .dim
                    .pow(&BigRational::from_integer(BigInt::from(3))),
            )))
        },
        | B::Distance => {
            let left = expect_tensor(args, 0, name, span)?;
            let right = expect_tensor(args, 1, name, span)?;
            let distance =
                aevia_std::geom::distance(&tensor_entries(&left)?, &tensor_entries(&right)?)
                    .map_err(|error| numeric_error(name, error, span))?;
            Ok(Value::Num(NumVal::float(distance, left.dim.clone())))
        },

        // ----- §8.14 measurement ------------------------------------------
        | B::Measure => {
            let value = expect_num(args, 0, name, span)?;
            let uncertainty = expect_num(args, 1, name, span)?;
            if value.dim != uncertainty.dim {
                return Err(EvalError::new(
                    "E0401",
                    "a measurement needs one dimension for value and uncertainty",
                )
                .at(span));
            }
            Ok(record_of(vec![
                ("value", Value::Num(value)),
                ("uncertainty", Value::Num(uncertainty)),
            ]))
        },
        | B::Value | B::Uncertainty => {
            let Some(Value::Record(measurement)) = args.first() else {
                return Err(
                    EvalError::new("E0302", format!("`{name}` expects a measurement")).at(span),
                );
            };
            measurement.fields.get(name).cloned().ok_or_else(|| {
                EvalError::new("E0306", format!("a measurement has no field `{name}`")).at(span)
            })
        },

        // ----- §8.13 testing ----------------------------------------------
        | B::CompileFail => {
            let source = expect_str(args, 0, name, span)?;
            let compilation = crate::compile("compile_fail.aev", &source);
            Ok(Value::Bool(compilation.diagnostics.has_errors()))
        },

        // ----- §8.4 decompositions and shape operations -------------------
        | B::Lstsq => {
            let matrix = expect_tensor(args, 0, name, span)?;
            let (rows, cols, entries) = tensor_matrix(&matrix, name, span)?;
            let vector = expect_tensor(args, 1, name, span)?;
            let rhs = tensor_entries(&vector)?;
            if rhs.len() != rows {
                return Err(EvalError::new(
                    "E0802",
                    format!(
                        "`lstsq` needs {rows} right-hand side value(s), found {}",
                        rhs.len()
                    ),
                )
                .at(span));
            }
            let dim = vector.dim.div(&matrix.dim);
            lin_result(
                aevia_std::linalg::lstsq(rows, cols, &entries, &rhs)
                    .map(|solution| float_tensor(vec![cols], solution, dim)),
                span,
            )
        },
        | B::Eig => {
            let matrix = expect_tensor(args, 0, name, span)?;
            let (rows, cols, entries) = tensor_matrix(&matrix, name, span)?;
            let dim = matrix.dim.clone();
            let (values, vectors) = aevia_std::linalg::eig(rows, cols, &entries)
                .map_err(|error| numeric_error(name, error, span))?;
            Ok(tuple(vec![
                complex_tensor(vec![values.len()], &values, dim),
                complex_tensor(vec![rows, rows], &vectors, Dim::one()),
            ]))
        },
        | B::Eigvals => {
            let matrix = expect_tensor(args, 0, name, span)?;
            let (rows, cols, entries) = tensor_matrix(&matrix, name, span)?;
            let dim = matrix.dim.clone();
            let values = aevia_std::linalg::eigvals(rows, cols, &entries)
                .map_err(|error| numeric_error(name, error, span))?;
            Ok(complex_tensor(vec![values.len()], &values, dim))
        },
        | B::Svd => {
            let matrix = expect_tensor(args, 0, name, span)?;
            let (rows, cols, entries) = tensor_matrix(&matrix, name, span)?;
            let dim = matrix.dim.clone();
            let (u, s, vt) = aevia_std::linalg::svd(rows, cols, &entries)
                .map_err(|error| numeric_error(name, error, span))?;
            let k = s.len();
            Ok(tuple(vec![
                float_tensor(vec![rows, k], u, Dim::one()),
                float_tensor(vec![k], s, dim),
                float_tensor(vec![k, cols], vt, Dim::one()),
            ]))
        },
        | B::Qr => {
            let matrix = expect_tensor(args, 0, name, span)?;
            let (rows, cols, entries) = tensor_matrix(&matrix, name, span)?;
            let dim = matrix.dim.clone();
            // Small exact matrices retain their representation when the norms
            // have rational square roots. General inputs use the RSSN backend.
            if rows <= 8
                && cols <= 8
                && let Some(exact) = exact_values(&matrix)
                && let Ok((q, r)) = aevia_std::linalg::qr_exact(rows, cols, &exact)
            {
                return Ok(tuple(vec![
                    rational_tensor(vec![rows, rows], &q, Dim::one()),
                    rational_tensor(vec![rows, cols], &r, dim),
                ]));
            }
            let (q, r) = aevia_std::linalg::qr(rows, cols, &entries)
                .map_err(|error| numeric_error(name, error, span))?;
            Ok(tuple(vec![
                float_tensor(vec![rows, rows], q, Dim::one()),
                float_tensor(vec![rows, cols], r, dim),
            ]))
        },
        | B::Lu => {
            let matrix = expect_tensor(args, 0, name, span)?;
            let (rows, cols, entries) = tensor_matrix(&matrix, name, span)?;
            let dim = matrix.dim.clone();
            if rows == cols
                && rows <= 32
                && let Some(exact) = exact_values(&matrix)
            {
                let (p, l, u) = aevia_std::linalg::lu_exact(rows, &exact)
                    .map_err(|error| numeric_error(name, error, span))?;
                return Ok(tuple(vec![
                    rational_tensor(vec![rows, rows], &p, Dim::one()),
                    rational_tensor(vec![rows, rows], &l, Dim::one()),
                    rational_tensor(vec![rows, cols], &u, dim),
                ]));
            }
            let (p, l, u) = aevia_std::linalg::lu(rows, cols, &entries)
                .map_err(|error| numeric_error(name, error, span))?;
            Ok(tuple(vec![
                float_tensor(vec![rows, rows], p, Dim::one()),
                float_tensor(vec![rows, rows], l, Dim::one()),
                float_tensor(vec![rows, cols], u, dim),
            ]))
        },
        | B::Cholesky => {
            let matrix = expect_tensor(args, 0, name, span)?;
            let (rows, cols, entries) = tensor_matrix(&matrix, name, span)?;
            let dim = matrix
                .dim
                .pow(&BigRational::new(BigInt::one(), BigInt::from(2)));
            lin_result(
                aevia_std::linalg::cholesky(rows, cols, &entries)
                    .map(|l| float_tensor(vec![rows, rows], l, dim)),
                span,
            )
        },
        | B::Tril | B::Triu => {
            let tensor = expect_tensor(args, 0, name, span)?;
            matrix::triangle(&tensor, name, span)
        },
        // `reshape`/`zeros_of`/`ones_of` read their shape from the type
        // argument (`call_typed`). Reaching this arm means the call was
        // written without one, and the runtime has no shape to build.
        | B::Reshape | B::ZerosOf | B::OnesOf | B::RefineShape => {
            Err(EvalError::new(
                "E0310",
                format!("`{name}` needs its shape type argument, as in `{name}<[3, 2]>(…)`"),
            )
            .at(span))
        },
        | B::ReshapeDyn => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let shape_tensor = expect_tensor(args, 1, name, span)?;
            Ok(matrix::reshape_dyn(&tensor, &shape_tensor))
        },
        | B::Set => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let indices = expect_tensor(args, 1, name, span)?;
            let value = args
                .get(2)
                .cloned()
                .ok_or_else(|| arity(name, 3, 2, span))?;
            matrix::set(&tensor, &indices, value, span)
        },
        | B::MapIndexed => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let tensor = expect_tensor(args, 1, name, span)?;
            let mut elements = Vec::with_capacity(tensor.storage.len());
            for (index, element) in tensor.elements().into_iter().enumerate() {
                let index = Value::int(i64::try_from(index).unwrap_or(i64::MAX));
                elements.push(interp.apply(function.clone(), vec![index, element], span)?);
            }
            let dim = elements
                .first()
                .map_or_else(Dim::one, crate::value::value_dim);
            Ok(build_tensor(elements, tensor.shape.clone(), dim))
        },
        | B::ApproxMap => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let tensor = expect_tensor(args, 1, name, span)?;
            let mut values = Vec::with_capacity(tensor.storage.len());
            for element in tensor.elements() {
                let mapped = interp.apply(function.clone(), vec![element], span)?;
                match mapped.as_f64() {
                    | Some(value) => values.push(value),
                    | None => {
                        return Err(EvalError::new(
                            "E0806",
                            "`approx_map` needs a numeric result for every element",
                        )
                        .at(span));
                    },
                }
            }
            let dim = tensor.dim.clone();
            Ok(float_tensor(vec![values.len()], values, dim))
        },
        | B::DetUnit => {
            // §4.9.5: `det_unit(A, u)` answers with a `Dyn`, because the
            // determinant of an `n × n` matrix over `d` has dimension `d^n`
            // and `n` is only known at run time.
            let tensor = expect_tensor(args, 0, name, span)?;
            let unit = expect_unit(args, 1, name, span)?;
            let (rows, cols, entries) = tensor_matrix(&tensor, name, span)?;
            let size = rows.max(cols);
            let determinant = aevia_std::linalg::det(rows, cols, &entries)
                .map_err(|error| numeric_error(name, error, span))?;
            let dim = unit.dim.pow(&BigRational::from_integer(BigInt::from(size)));
            Ok(Value::Dyn(Rc::new(DynValue {
                value: NumVal::float(determinant, Dim::one()),
                dim,
                display: None,
            })))
        },

        // ----- §3.11 / §4.9 dynamic dimension -----------------------------
        // `dyn` is a type-directed primitive: the checker materialises the
        // static `d` into a `DimValue` at the call site. Until it does, the
        // value's own dimension is the one that travels — which is what makes
        // `dim_of` and `value_of` total, and `check` exact for a value that
        // has never left the program.
        | B::Dyn => {
            let num = expect_num(args, 0, name, span)?;
            Ok(Value::Dyn(Rc::new(DynValue::new(&num))))
        },
        | B::DynWith => {
            let num = expect_num(args, 0, name, span)?;
            let dim = expect_dim_value(args, 1, name, span)?;
            let mut dynamic = DynValue::new(&num);
            dynamic.dim = dim;
            Ok(Value::Dyn(Rc::new(dynamic)))
        },
        | B::Check => {
            let dynamic = expect_dyn(args, 0, name, span)?;
            // §4.9.4: `check<d>(x)` is type-directed — the concrete `d` never
            // reaches the runtime in the argument list, so the checker records
            // it by span. Compare it with the dimension the value carries; a
            // mismatch is the appendix-B `E0412` class, carried as the
            // recoverable `Err(DimError)` the signature promises.
            if let Some(expected) = interp.context.evidence.dyn_calls.get(&span)
                && dynamic.dim != *expected
            {
                return Ok(err(dim_error(expected, &dynamic.dim)));
            }
            Ok(ok(Value::Num(dynamic.as_scalar())))
        },
        // §5.11.5: the checker recorded the system and the SI dimension.
        | B::ToSystem | B::FromSystem => {
            let num = expect_num(args, 1, name, span)?;
            let (system_name, si) = interp
                .context
                .evidence
                .system_calls
                .get(&span)
                .cloned()
                .ok_or_else(|| {
                    EvalError::new("E0304", format!("`{name}` was not resolved by the checker"))
                        .at(span)
                })?;
            let system = crate::systems::lookup(&system_name).ok_or_else(|| {
                EvalError::new("E0512", format!("unknown unit system `{system_name}`")).at(span)
            })?;
            let (direction, target) = if name == "to_system" {
                (-1, system.reduce_dim(&si))
            } else {
                (1, si.clone())
            };
            crate::systems::convert(&system, &si, &num, direction, target)
                .map(Value::Num)
                .map_err(|message| EvalError::new("E0302", message).at(span))
        },
        | B::CheckWith => {
            let dim = expect_dim_value(args, 0, name, span)?;
            let dynamic = expect_dyn(args, 1, name, span)?;
            if dynamic.dim == dim {
                Ok(ok(Value::Num(dynamic.payload())))
            } else {
                Ok(err(dim_error(&dim, &dynamic.dim)))
            }
        },
        | B::DimOf => {
            let dynamic = expect_dyn(args, 0, name, span)?;
            Ok(dim_value(&dynamic.dim))
        },
        | B::ValueOf => {
            let dynamic = expect_dyn(args, 0, name, span)?;
            Ok(Value::Num(dynamic.payload()))
        },
        | B::PowDyn => {
            let dynamic = expect_dyn(args, 0, name, span)?;
            let exponent = expect_int(args, 1, name, span)?;
            let payload = interp.apply_binary(
                "^",
                Value::Num(dynamic.payload()),
                Value::Num(NumVal::int(exponent)),
                span,
            )?;
            let Value::Num(powered) = payload else {
                return Err(EvalError::new("E0302", "`pow_dyn` needs a numeric payload").at(span));
            };
            let mut value = powered;
            value.dim = Dim::one();
            value.display = None;
            Ok(Value::Dyn(Rc::new(DynValue {
                value,
                dim: dynamic
                    .dim
                    .pow(&BigRational::from_integer(BigInt::from(exponent))),
                display: None,
            })))
        },
        // ----- §8.7 statistics --------------------------------------------
        | B::Median | B::Mode => {
            let tensor = expect_tensor(args, 0, name, span)?;
            if let Some(mut exact) = exact_values(&tensor) {
                let value = if name == "median" {
                    aevia_std::stats::median_exact(exact.to_mut())
                } else {
                    aevia_std::stats::mode_exact(&exact)
                }
                .map_err(|error| numeric_error(name, error, span))?;
                return Ok(exact_number(value, tensor.dim.clone()));
            }
            let mut values = tensor_entries(&tensor)?;
            let value = if name == "median" {
                aevia_std::stats::median(values.to_mut())
            } else {
                aevia_std::stats::mode(&values)
            }
            .map_err(|error| numeric_error(name, error, span))?;
            Ok(Value::Num(NumVal::float(value, tensor.dim.clone())))
        },
        | B::Quantile => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let fraction = expect_num(args, 1, name, span)?;
            reduction::quantile(&tensor, &fraction, span)
        },
        | B::Quantiles => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let probabilities = expect_tensor(args, 1, name, span)?;
            reduction::quantiles(&tensor, &probabilities, span)
        },
        | B::Cov => {
            let left = expect_tensor(args, 0, name, span)?;
            let right = expect_tensor(args, 1, name, span)?;
            let dim = left.dim.mul(&right.dim);
            if let (Some(a), Some(b)) = (exact_values(&left), exact_values(&right)) {
                let value = aevia_std::stats::cov_exact(&a, &b, false)
                    .map_err(|error| numeric_error(name, error, span))?;
                return Ok(exact_number(value, dim));
            }
            let value =
                aevia_std::stats::cov(&tensor_entries(&left)?, &tensor_entries(&right)?, false)
                    .map_err(|error| numeric_error(name, error, span))?;
            Ok(Value::Num(NumVal::float(value, dim)))
        },
        | B::Corr => {
            let left = expect_tensor(args, 0, name, span)?;
            let right = expect_tensor(args, 1, name, span)?;
            let value =
                aevia_std::stats::corr(&tensor_entries(&left)?, &tensor_entries(&right)?, false)
                    .map_err(|error| numeric_error(name, error, span))?;
            Ok(Value::Num(NumVal::float(value, Dim::one())))
        },
        | B::Skewness | B::Kurtosis => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let values = tensor_entries(&tensor)?;
            let value = if name == "skewness" {
                aevia_std::stats::skewness(&values)
            } else {
                aevia_std::stats::kurtosis(&values)
            }
            .map_err(|error| numeric_error(name, error, span))?;
            Ok(Value::Num(NumVal::float(value, Dim::one())))
        },
        | B::Histogram => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let bins = expect_exact_int(args, 1, name, span)?;
            reduction::histogram(&tensor, &bins, span)
        },

        // ----- §8.5 numeric calculus --------------------------------------
        | B::NumericDeriv => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            Ok(native(move |interp, call_args, span| {
                let point = call_args
                    .first()
                    .cloned()
                    .ok_or_else(|| arity("numeric_deriv", 1, 0, span))?;
                let x = point.as_f64().unwrap_or(f64::NAN);
                let argument_dim = crate::value::value_dim(&point);
                let mut failure = None;
                let codomain =
                    apply_scalar_value(interp, &function, x, &argument_dim, span, &mut failure);
                if let Some(error) = failure.take() {
                    return Err(error);
                }
                let mut f = |value: f64| {
                    apply_scalar(interp, &function, value, &argument_dim, span, &mut failure)
                };
                let slope = aevia_std::calc::derivative(&mut f, x);
                if let Some(error) = failure {
                    return Err(error);
                }
                let dim = crate::value::value_dim(&codomain).div(&argument_dim);
                Ok(Value::Num(NumVal::float(slope, dim)))
            }))
        },
        | B::Quadrature => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 3, 0, span))?;
            let lower = expect_num(args, 1, name, span)?;
            let upper = expect_num(args, 2, name, span)?;
            let (a, b) = (
                lower.to_f64().unwrap_or(f64::NAN),
                upper.to_f64().unwrap_or(f64::NAN),
            );
            let mut failure = None;
            let mut codomain = None;
            let mut f = |x: f64| {
                if failure.is_some() {
                    return f64::NAN;
                }
                let value =
                    apply_scalar_value(interp, &function, x, &lower.dim, span, &mut failure);
                let Value::Num(number) = value else {
                    return f64::NAN;
                };
                if failure.is_some() {
                    return f64::NAN;
                }
                if codomain.as_ref().is_some_and(|dim| dim != &number.dim) {
                    failure = Some(
                        EvalError::new(
                            "E0401",
                            "quadrature integrand returned inconsistent dimensions",
                        )
                        .at(span),
                    );
                    return f64::NAN;
                }
                let sample = number.to_f64().unwrap_or(f64::NAN);
                codomain.get_or_insert(number.dim);
                sample
            };
            let outcome = aevia_std::calc::quadrature(&mut f, a, b);
            if let Some(error) = failure.take() {
                return Err(error);
            }
            Ok(match outcome {
                | Ok(value) => {
                    let codomain = if let Some(dim) = codomain {
                        dim
                    } else {
                        // An empty interval has no algorithm nodes. Keep its
                        // one type witness so the zero has the right dimension.
                        let sample = apply_scalar_value(
                            interp,
                            &function,
                            a,
                            &lower.dim,
                            span,
                            &mut failure,
                        );
                        if let Some(error) = failure {
                            return Err(error);
                        }
                        crate::value::value_dim(&sample)
                    };
                    ok(Value::Num(NumVal::float(value, codomain.mul(&lower.dim))))
                },
                | Err(error) => err(Value::string(error.to_string())),
            })
        },
        | B::Product | B::ProductDyn => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 3, 0, span))?;
            let lower = expect_num(args, 1, name, span)?;
            let upper = expect_num(args, 2, name, span)?;
            let Some((mut index, end)) = lower.to_rational().zip(upper.to_rational()) else {
                return symbolic_range("product", interp, args, &lower, &upper, span);
            };
            // BigRational subtraction cannot overflow.
            #[allow(clippy::arithmetic_side_effects)]
            let width = &end - &index;
            if width > BigRational::from_integer(BigInt::from(10_000)) {
                return symbolic_range("product", interp, args, &lower, &upper, span);
            }
            let mut total = Value::Num(NumVal::int(BigInt::one()));
            while index <= end {
                let argument = exact_number(index.clone(), lower.dim.clone());
                let term = interp.apply(function.clone(), vec![argument], span)?;
                total = interp.apply_binary("*", total, term, span)?;
                // BigRational addition cannot overflow.
                #[allow(clippy::arithmetic_side_effects)]
                {
                    index += BigRational::one();
                }
            }
            Ok(total)
        },
        | B::SumF => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 3, 0, span))?;
            let lower = expect_num(args, 1, name, span)?;
            let upper = expect_num(args, 2, name, span)?;
            let bound = |num: &NumVal| {
                num.to_f64()
                    .and_then(|value| value.round().to_i64())
                    .ok_or_else(|| {
                        EvalError::new(
                            "E0802",
                            format!("`{name}` needs finite bounds in the signed 64-bit range"),
                        )
                        .at(span)
                    })
            };
            let (a, b) = (bound(&lower)?, bound(&upper)?);
            let mut failure = None;
            let codomain =
                apply_scalar_value(interp, &function, a as f64, &lower.dim, span, &mut failure);
            if let Some(error) = failure.take() {
                return Err(error);
            }
            let mut total = 0.0;
            for step in a..=b {
                total += apply_scalar(
                    interp,
                    &function,
                    step as f64,
                    &lower.dim,
                    span,
                    &mut failure,
                );
                if let Some(error) = failure.take() {
                    return Err(error);
                }
            }
            Ok(Value::Num(NumVal::float(
                total,
                crate::value::value_dim(&codomain),
            )))
        },
        | B::Grad | B::GradF => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let point = expect_tensor(args, 1, name, span)?;
            let x = tensor_values(&point)?;
            let mut failure = None;
            let codomain = apply_tensor_value(interp, &function, &x, &point, span, &mut failure);
            if let Some(error) = failure.take() {
                return Err(error);
            }
            let mut f =
                |state: &[f64]| apply_tensor(interp, &function, state, &point, span, &mut failure);
            let gradient = aevia_std::calc::grad(&mut f, &x);
            if let Some(error) = failure {
                return Err(error);
            }
            let dim = crate::value::value_dim(&codomain).div(&point.dim);
            Ok(float_tensor(vec![gradient.len()], gradient, dim))
        },
        | B::Jacobian => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let point = expect_tensor(args, 1, name, span)?;
            let x = tensor_values(&point)?;
            let mut failure = None;
            let mut f = |state: &[f64]| -> Vec<f64> {
                apply_tensor_elements(interp, &function, state, &point, span, &mut failure)
            };
            let (rows, cols, entries) = aevia_std::calc::jacobian(&mut f, &x);
            if let Some(error) = failure {
                return Err(error);
            }
            Ok(float_tensor(
                vec![rows, cols],
                entries,
                Dim::one().div(&point.dim),
            ))
        },
        | B::Hessian | B::Laplacian => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let point = expect_tensor(args, 1, name, span)?;
            let x = tensor_values(&point)?;
            let mut failure = None;
            let codomain = apply_tensor_value(interp, &function, &x, &point, span, &mut failure);
            if let Some(error) = failure.take() {
                return Err(error);
            }
            let mut f =
                |state: &[f64]| apply_tensor(interp, &function, state, &point, span, &mut failure);
            let squared = point.dim.pow(&BigRational::from_integer(BigInt::from(2)));
            let dim = crate::value::value_dim(&codomain).div(&squared);
            if name == "laplacian" {
                let value = aevia_std::calc::laplacian(&mut f, &x);
                if let Some(error) = failure {
                    return Err(error);
                }
                return Ok(Value::Num(NumVal::float(value, dim)));
            }
            let entries = aevia_std::calc::hessian(&mut f, &x);
            if let Some(error) = failure {
                return Err(error);
            }
            let n = x.len();
            Ok(float_tensor(vec![n, n], entries, dim))
        },
        | B::Divergence | B::Curl => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let point = expect_tensor(args, 1, name, span)?;
            let x = tensor_values(&point)?;
            let mut failure = None;
            let mut f = |state: &[f64]| -> Vec<f64> {
                apply_tensor_elements(interp, &function, state, &point, span, &mut failure)
            };
            let dim = Dim::one().div(&point.dim);
            if name == "divergence" {
                let value = aevia_std::calc::divergence(&mut f, &x);
                if let Some(error) = failure {
                    return Err(error);
                }
                return Ok(Value::Num(NumVal::float(value, dim)));
            }
            let value = aevia_std::calc::curl(&mut f, &x)
                .map_err(|error| numeric_error(name, error, span))?;
            if let Some(error) = failure {
                return Err(error);
            }
            Ok(float_tensor(vec![value.len()], value, dim))
        },

        // ----- §8.6 solving and optimisation ------------------------------
        | B::SolveComplex => {
            let (variable, applied) = apply_to_fresh(interp, name, args, span, Dim::one())?;
            let symbol = symbolic_variable(&variable, Dim::one());
            match solve("solve_complex", &applied, &symbol) {
                | Ok(solutions) => Ok(ok(solutions_value(&solutions, Dim::one()))),
                | Err(error) => Ok(err(Value::string(error.message))),
            }
        },
        | B::NsolveF | B::Root => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let start = expect_num(args, 1, name, span)?;
            let x = start.to_f64().unwrap_or(f64::NAN);
            let mut failure = None;
            let mut f =
                |value: f64| apply_scalar(interp, &function, value, &start.dim, span, &mut failure);
            let outcome = aevia_std::calc::root(&mut f, x);
            if let Some(error) = failure {
                return Err(error);
            }
            Ok(match outcome {
                | Ok(value) => ok(Value::Num(NumVal::float(value, start.dim.clone()))),
                | Err(error) => err(Value::string(error.to_string())),
            })
        },
        | B::SolveSystem | B::LeastSquares => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let point = expect_tensor(args, 1, name, span)?;
            let x = tensor_values(&point)?;
            let mut failure = None;
            let mut f = |state: &[f64]| -> Vec<f64> {
                apply_tensor_elements(interp, &function, state, &point, span, &mut failure)
            };
            let outcome = if name == "solve_system" {
                aevia_std::calc::solve_system(&mut f, &x)
            } else {
                aevia_std::calc::least_squares(&mut f, &x)
            };
            if let Some(error) = failure {
                return Err(error);
            }
            Ok(match outcome {
                | Ok(solution) => {
                    ok(float_tensor(
                        vec![solution.len()],
                        solution,
                        point.dim.clone(),
                    ))
                },
                | Err(error) => err(Value::string(error.to_string())),
            })
        },
        | B::Minimize | B::Maximize | B::MinimizeF => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let point = expect_tensor(args, 1, name, span)?;
            let x = tensor_values(&point)?;
            let mut failure = None;
            let mut f =
                |state: &[f64]| apply_tensor(interp, &function, state, &point, span, &mut failure);
            let outcome = if name == "maximize" {
                aevia_std::calc::maximize(&mut f, &x)
            } else {
                aevia_std::calc::minimize(&mut f, &x)
            };
            if let Some(error) = failure {
                return Err(error);
            }
            Ok(match outcome {
                | Ok(solution) => {
                    ok(float_tensor(
                        vec![solution.len()],
                        solution,
                        point.dim.clone(),
                    ))
                },
                | Err(error) => err(Value::string(error.to_string())),
            })
        },

        // ----- §8.7.1 distributions ---------------------------------------
        | B::Pdf | B::Cdf => {
            let distribution = distribution_from(args, name, span)?;
            let point = expect_num(args, 1, name, span)?;
            let x = point.to_f64().ok_or_else(|| {
                EvalError::new("E0806", format!("`{name}` needs a concrete argument")).at(span)
            })?;
            let value = if name == "pdf" {
                distribution.pdf(x)
            } else {
                distribution.cdf(x)
            };
            if value.is_nan() {
                return Err(EvalError::new(
                    "E0806",
                    format!("`{name}` is not defined for these parameters"),
                )
                .at(span));
            }
            let dim = if name == "pdf" {
                point.dim.inverse()
            } else {
                Dim::one()
            };
            Ok(Value::Num(NumVal::float(value, dim)))
        },
        | B::CdfBatch => {
            let distribution = distribution_from(args, name, span)?;
            let points = expect_tensor(args, 1, name, span)?;
            if points.shape.len() != 1 {
                return Err(
                    EvalError::new("E0802", "`cdf_batch` needs a vector of points").at(span),
                );
            }
            let dimension = distribution_dim(args, span)?;
            if points.dim != dimension {
                return Err(EvalError::new(
                    "E0401",
                    format!(
                        "`cdf_batch` expects points of dimension {dimension}, found {}",
                        points.dim
                    ),
                )
                .at(span));
            }
            let entries = tensor_entries(&points).map_err(|error| error.at(span))?;
            let probabilities =
                aevia_rssn::par_map_f64_slice(&entries, 4096, interp.execution_mode(), |point| {
                    distribution.cdf(point)
                });
            // The kernel checks the parameters even at negative infinity.
            if probabilities.iter().any(|value| value.is_nan())
                || (entries.is_empty() && distribution.cdf(f64::NEG_INFINITY).is_nan())
            {
                return Err(EvalError::new(
                    "E0806",
                    "`cdf_batch` is not defined for these parameters and points",
                )
                .at(span));
            }
            Ok(float_tensor(
                points.shape.clone(),
                probabilities,
                Dim::one(),
            ))
        },
        | B::DistQuantile => {
            let distribution = distribution_from(args, name, span)?;
            let probability = expect_num(args, 1, name, span)?;
            if probability.dim != Dim::one() {
                return Err(EvalError::new(
                    "E0401",
                    "`dist_quantile` expects a dimensionless probability",
                )
                .at(span));
            }
            let probability = probability.to_f64().ok_or_else(|| {
                EvalError::new("E0806", "`dist_quantile` expects a concrete probability").at(span)
            })?;
            let value = distribution
                .quantile(probability)
                .map_err(|error| numeric_error(name, error, span))?;
            Ok(Value::Num(NumVal::float(
                value,
                distribution_dim(args, span)?,
            )))
        },
        | B::Sample => {
            let distribution = distribution_from(args, name, span)?;
            let dim = distribution_dim(args, span)?;
            Ok(Value::Io(Rc::new(crate::value::IoAction::Bind {
                action: Rc::new(crate::value::IoAction::Random),
                func: native(move |_interp, values, span| {
                    let uniform = values.first().and_then(Value::as_f64).ok_or_else(|| {
                        EvalError::new("E0809", "the RNG gave no number").at(span)
                    })?;
                    let value = distribution
                        .quantile(uniform)
                        .map_err(|error| numeric_error("sample", error, span))?;
                    Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(
                        Value::Num(NumVal::float(value, dim.clone())),
                    ))))
                }),
            })))
        },
        | B::SampleN => {
            let distribution = distribution_from(args, name, span)?;
            let count = host_count_arg(args, 1, name, span)?;
            let dim = distribution_dim(args, span)?;
            // §8.7.1 types the third argument as `Option<Int>`: a *seed*.
            // A seeded call is reproducible and does not perturb the session's
            // sequence; `None` (or a missing argument, or the `()` shape a
            // direct dispatch uses) draws from the session's RNG, which is the
            // historical behaviour. Ignoring the seed made `sample_n` depend on
            // whatever happened to run before it, which is not a pure function.
            let mut seed = sample_seed(args, span)?;
            let mut values = Vec::new();
            values
                .try_reserve_exact(count)
                .map_err(|_| EvalError::new("E0802", "`sample_n` output is too large").at(span))?;
            for _ in 0..count {
                let uniform = match seed.as_mut() {
                    | Some(state) => crate::eval::next_unit(state),
                    | None => interp.next_random(),
                };
                let value = distribution
                    .quantile(uniform)
                    .map_err(|error| numeric_error(name, error, span))?;
                values.push(value);
            }
            Ok(float_tensor(vec![values.len()], values, dim))
        },

        // ----- §8.8 FFT and interpolation ---------------------------------
        | B::Fft
        | B::Ifft
        | B::FftNorm
        | B::IfftNorm
        | B::FftAxis
        | B::IfftAxis
        | B::Fft2
        | B::Ifft2
        | B::Fftn
        | B::Ifftn
        | B::FftnAxes
        | B::IfftnAxes
        | B::FftnNorm
        | B::IfftnNorm => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let rank = tensor.shape.len();
            let input = complex_values(&tensor)?;
            let inverse = name.starts_with('i');
            let base = name.trim_start_matches('i');
            let axes = match base {
                | "fft" | "fft_norm" => vec![rank.saturating_sub(1)],
                | "fft_axis" => vec![axis_argument(args, 1, rank, name, span)?],
                | "fft2" => transform_axes_last_two(rank, name, span)?,
                | "fftn_axes" => axes_argument(args, 1, rank, name, span)?,
                | _ => (0..rank).collect(),
            };
            let norm = if base.ends_with("_norm") {
                norm_argument(args, 1, name, span)?
            } else {
                aevia_std::fft::Norm::Backward
            };
            let output = aevia_std::fft::fft_axes(&input, &tensor.shape, &axes, inverse, norm)
                .map_err(|message| {
                    EvalError::new("E0802", format!("`{name}`: {message}")).at(span)
                })?;
            Ok(complex_tensor(
                tensor.shape.clone(),
                &output,
                tensor.dim.clone(),
            ))
        },
        | B::Rfft | B::Rfft2 | B::Rfftn | B::RfftnAxes => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let rank = tensor.shape.len();
            let signal = tensor_entries(&tensor)?;
            let axes = match name {
                | "rfft" => vec![rank.saturating_sub(1)],
                | "rfft2" => transform_axes_last_two(rank, name, span)?,
                | "rfftn_axes" => axes_argument(args, 1, rank, name, span)?,
                | _ => (0..rank).collect(),
            };
            let (spectrum, shape) = aevia_std::fft::rfft_axes(&signal, &tensor.shape, &axes)
                .map_err(|message| {
                    EvalError::new("E0802", format!("`{name}`: {message}")).at(span)
                })?;
            Ok(complex_tensor(shape, &spectrum, tensor.dim.clone()))
        },
        | B::Irfft | B::Irfft2 | B::Irfftn | B::IrfftnAxes => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let rank = tensor.shape.len();
            let spectrum = complex_values(&tensor)?;
            let axes = match name {
                | "irfft" => vec![rank.saturating_sub(1)],
                | "irfft2" => transform_axes_last_two(rank, name, span)?,
                | "irfftn_axes" => axes_argument(args, 1, rank, name, span)?,
                | _ => (0..rank).collect(),
            };
            let last = axes.last().copied().ok_or_else(|| {
                EvalError::new("E0802", "a real transform needs at least one axis").at(span)
            })?;
            let bins = tensor.shape.get(last).copied().ok_or_else(|| {
                EvalError::new("E0802", "the real transform axis needs a positive rank").at(span)
            })?;
            let length_at = usize::from(name == "irfftn_axes").saturating_add(1);
            let invalid_length = || {
                EvalError::new("E0802", format!("`{name}`: invalid real transform length")).at(span)
            };
            let n_last = if args.len() > length_at {
                expect_exact_int(args, length_at, name, span)?
                    .to_usize()
                    .ok_or_else(invalid_length)?
            } else if bins == 1 {
                1
            } else {
                bins.saturating_sub(1)
                    .checked_mul(2)
                    .ok_or_else(invalid_length)?
            };
            let (signal, shape) =
                aevia_std::fft::irfft_axes(&spectrum, &tensor.shape, &axes, n_last).map_err(
                    |message| EvalError::new("E0802", format!("`{name}`: {message}")).at(span),
                )?;
            Ok(float_tensor(shape, signal, tensor.dim.clone()))
        },
        | B::Fftshift | B::Ifftshift | B::FftshiftAxis | B::IfftshiftAxis => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let rank = tensor.shape.len();
            let axes = if name.ends_with("_axis") {
                vec![axis_argument(args, 1, rank, name, span)?]
            } else {
                (0..rank).collect()
            };
            let len = checked_element_count(&tensor.shape, span)?;
            if len != tensor.storage.len() {
                return Err(EvalError::new("E0802", "tensor shape and storage disagree").at(span));
            }
            let inverse = name.starts_with('i');
            let shift_error =
                |message| EvalError::new("E0802", format!("`{name}`: {message}")).at(span);
            if let Some(data) = tensor.dense_flat() {
                let mut shifted = data.into_owned();
                rssn::kernels::fft::shift_axes_in_place(
                    &mut shifted,
                    &tensor.shape,
                    &axes,
                    inverse,
                )
                .map_err(shift_error)?;
                return Ok(Value::Tensor(Rc::new(TensorData {
                    shape: tensor.shape.clone(),
                    storage: Storage::Dense(Rc::new(shifted)),
                    dim: tensor.dim.clone(),
                    display: tensor.display.clone(),
                    verified_element: tensor.verified_element.clone(),
                })));
            }
            let permutation = aevia_std::fft::shift_permutation(&tensor.shape, &axes, inverse)
                .map_err(shift_error)?;
            let shifted = permutation
                .into_iter()
                .map(|index| {
                    tensor.get_flat(index).ok_or_else(|| {
                        EvalError::new("E0802", "this tensor's elements cannot be read").at(span)
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(sequence::rebuild_selection(
                &tensor,
                shifted,
                tensor.shape.clone(),
            ))
        },
        | B::Fftfreq | B::Rfftfreq => {
            let count = expect_exact_int(args, 0, name, span)?
                .to_usize()
                .ok_or_else(|| {
                    EvalError::new(
                        "E0802",
                        format!("`{name}` needs a nonnegative transform length"),
                    )
                    .at(span)
                })?;
            checked_element_count(&[count], span)?;
            let spacing = expect_num(args, 1, name, span)?;
            let d = spacing
                .to_f64()
                .filter(|d| d.is_finite() && *d != 0.0)
                .ok_or_else(|| {
                    EvalError::new(
                        "E0806",
                        format!("`{name}` needs finite nonzero sample spacing"),
                    )
                    .at(span)
                })?;
            let frequencies = if name == "rfftfreq" {
                aevia_std::fft::rfftfreq(count, d)
            } else {
                aevia_std::fft::fftfreq(count, d)
            };
            let len = frequencies.len();
            Ok(float_tensor(vec![len], frequencies, spacing.dim.inverse()))
        },
        | B::Interp1 | B::Spline => {
            let xs = expect_tensor(args, 0, name, span)?;
            let ys = expect_tensor(args, 1, name, span)?;
            let at = expect_num(args, 2, name, span)?;
            let x = at.to_f64().unwrap_or(f64::NAN);
            let outcome = if name == "spline" {
                aevia_std::interp::spline(&tensor_values(&xs)?, &tensor_values(&ys)?, x)
            } else {
                aevia_std::interp::interp1(&tensor_values(&xs)?, &tensor_values(&ys)?, x)
            };
            let value = outcome.map_err(|error| numeric_error(name, error, span))?;
            Ok(Value::Num(NumVal::float(value, ys.dim.clone())))
        },
        | B::Interp2 => {
            let xs = expect_tensor(args, 0, name, span)?;
            let ys = expect_tensor(args, 1, name, span)?;
            let grid = expect_tensor(args, 2, name, span)?;
            let at_x = expect_num(args, 3, name, span)?;
            let at_y = expect_num(args, 4, name, span)?;
            let value = aevia_std::interp::interp2(
                &tensor_values(&xs)?,
                &tensor_values(&ys)?,
                &tensor_values(&grid)?,
                at_x.to_f64().unwrap_or(f64::NAN),
                at_y.to_f64().unwrap_or(f64::NAN),
            )
            .map_err(|error| numeric_error(name, error, span))?;
            Ok(Value::Num(NumVal::float(value, grid.dim.clone())))
        },
        | B::Interp2Batch => {
            let xs = expect_tensor(args, 0, name, span)?;
            let ys = expect_tensor(args, 1, name, span)?;
            let grid = expect_tensor(args, 2, name, span)?;
            let query_xs = expect_tensor(args, 3, name, span)?;
            let vertical_queries = expect_tensor(args, 4, name, span)?;
            let ([nx], [ny], [n], [m]) = (
                xs.shape.as_slice(),
                ys.shape.as_slice(),
                query_xs.shape.as_slice(),
                vertical_queries.shape.as_slice(),
            ) else {
                return Err(EvalError::new(
                    "E0802",
                    "`interp2Batch` needs rank-one axes and query tensors",
                )
                .at(span));
            };
            if n != m {
                return Err(EvalError::new(
                    "E0802",
                    "`interp2Batch` needs equally long query tensors",
                )
                .at(span));
            }
            if grid.shape.as_slice() != [*ny, *nx] {
                return Err(EvalError::new(
                    "E0802",
                    format!("`interp2Batch` needs a grid with shape [{ny}, {nx}]"),
                )
                .at(span));
            }
            // Empty tensors may lack their statically known element dimension.
            if *n != 0 && (query_xs.dim != xs.dim || vertical_queries.dim != ys.dim) {
                return Err(EvalError::new(
                    "E0401",
                    "each `interp2Batch` query tensor must have the dimension of its axis",
                )
                .at(span));
            }
            let x_nodes = tensor_entries(&xs)?;
            let y_nodes = tensor_entries(&ys)?;
            let grid_values = tensor_entries(&grid)?;
            let prepared = aevia_std::interp::BilinearGrid::new(&x_nodes, &y_nodes, &grid_values)
                .map_err(|error| numeric_error(name, error, span))?;
            let x_values = tensor_entries(&query_xs)?;
            let y_values = tensor_entries(&vertical_queries)?;
            let mut output = Vec::with_capacity(*n);
            for (x_chunk, y_chunk) in x_values.chunks(1024).zip(y_values.chunks(1024)) {
                interp.check_cancelled().map_err(|error| error.at(span))?;
                for (&x, &y) in x_chunk.iter().zip(y_chunk) {
                    output.push(
                        prepared
                            .eval(x, y)
                            .map_err(|error| numeric_error(name, error, span))?,
                    );
                }
            }
            Ok(float_tensor(vec![*n], output, grid.dim.clone()))
        },
        | B::Polyfit | B::ChebyshevFit => {
            let xs = expect_tensor(args, 0, name, span)?;
            let ys = expect_tensor(args, 1, name, span)?;
            let degree = host_count_arg(args, 2, name, span)?;
            let outcome = if name == "polyfit" {
                aevia_std::interp::polyfit(&tensor_values(&xs)?, &tensor_values(&ys)?, degree)
            } else {
                aevia_std::interp::chebyshev_fit(&tensor_values(&xs)?, &tensor_values(&ys)?, degree)
            };
            let coefficients = outcome.map_err(|error| numeric_error(name, error, span))?;
            let len = coefficients.len();
            Ok(float_tensor(vec![len], coefficients, ys.dim.clone()))
        },
        | B::Polyval => {
            let coefficients = expect_tensor(args, 0, name, span)?;
            let at = expect_num(args, 1, name, span)?;
            let values = tensor_values(&coefficients)?;
            let x = at.to_f64().unwrap_or(f64::NAN);
            Ok(Value::Num(NumVal::float(
                aevia_std::interp::polyval(&values, x),
                coefficients.dim.clone(),
            )))
        },

        // ----- §8.9 ODE ---------------------------------------------------
        | B::Odeint | B::OdeintWith => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 3, 0, span))?;
            let initial = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 3, 1, span))?;
            let times = expect_tensor(args, 2, name, span)?;
            let grid = tensor_values(&times)?;
            let mut options = if name == "odeint_with" {
                ode_options(args, 3, span)?
            } else {
                aevia_std::ode::Options::default()
            };
            let event = if name == "odeint_with" {
                ode_event(args, 3, span)?
            } else {
                None
            };
            let scales = ode_component_scales(args, 3, &initial, &mut options, span)?;
            let (grid, states) = run_ode(
                interp, &function, &initial, &grid, &times.dim, &scales, options, event, name, span,
            )?;
            let (shape, _) = flatten_state(&initial)?;
            let elements: Vec<Value> = states
                .iter()
                .map(|state| rebuild_state(&shape, state))
                .collect();
            if name == "odeint" {
                let len = elements.len();
                let dim = crate::value::value_dim(&initial);
                return Ok(build_tensor(elements, vec![len], dim));
            }
            let len = elements.len();
            let dim = crate::value::value_dim(&initial);
            Ok(record_of(vec![
                ("t", float_tensor(vec![grid.len()], grid, times.dim.clone())),
                ("y", build_tensor(elements, vec![len], dim)),
            ]))
        },
        | B::SolveIvp => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 4, 0, span))?;
            let initial = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 4, 1, span))?;
            let Some(Value::Record(interval)) = args.get(2) else {
                return Err(
                    EvalError::new("E0302", "`solve_ivp` expects a `(t0, t1)` pair").at(span),
                );
            };
            let start = interval
                .fields
                .get("0")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let end = interval
                .fields
                .get("1")
                .and_then(Value::as_f64)
                .unwrap_or(1.0);
            let mut options = ode_options(args, 3, span)?;
            let samples = options.steps.max(2);
            let time_dim = interval
                .fields
                .get("0")
                .map_or_else(Dim::one, crate::value::value_dim);
            let intervals = samples.saturating_sub(1);
            let mut grid = Vec::new();
            grid.try_reserve_exact(samples).map_err(|_| {
                EvalError::new("E0802", "`solve_ivp` output grid is too large").at(span)
            })?;
            grid.extend((0..samples).map(|index| {
                if index == intervals {
                    end
                } else {
                    let fraction = index as f64 / intervals as f64;
                    start * (1.0 - fraction) + end * fraction
                }
            }));
            let event = ode_event(args, 3, span)?;
            let scales = ode_component_scales(args, 3, &initial, &mut options, span)?;
            let (grid, states) = run_ode(
                interp, &function, &initial, &grid, &time_dim, &scales, options, event, name, span,
            )?;
            let (shape, _) = flatten_state(&initial)?;
            let elements: Vec<Value> = states
                .iter()
                .map(|state| rebuild_state(&shape, state))
                .collect();
            let len = elements.len();
            let dim = crate::value::value_dim(&initial);
            Ok(record_of(vec![
                ("t", float_tensor(vec![grid.len()], grid, time_dim)),
                ("y", build_tensor(elements, vec![len], dim)),
            ]))
        },

        // ----- §8.9 dense output, events, higher order, BVP ----------------
        | B::OdeSolve => ode_solve_call(interp, args, span),
        | B::OdeBvp => ode_bvp_call(interp, args, span),
        | B::OdeintOrder | B::OdeintOrderWith => odeint_order_call(interp, name, args, span),

        // ----- §8.9.1 stochastic differential equations -------------------
        | B::Wiener
        | B::SdeOrder
        | B::SdeOrderWith
        | B::SdeEnsembleSeed
        | B::SdeintDiag
        | B::SdeintGeneral => sde_call(interp, name, args, span),
        | B::SdeAt | B::SdeResample | B::SdeInterp | B::SdeEvents => {
            sde_path_call(interp, name, args, span)
        },
        | B::SdeEnsembleDiag | B::SdeEnsembleGeneral => sde_ensemble_call(interp, name, args, span),
        // One ensemble member; only reached through `sde_ensemble_*` (and a
        // worker replica applying the partially applied builtin).
        | B::SdeMember => {
            let [Value::Str(base), member_args @ .., index] = args else {
                return Err(EvalError::new("E0302", "`sde_member` is internal").at(span));
            };
            let index = index_value(index, span)?;
            if member_args.len() != 4 {
                return Err(arity(name, 6, args.len(), span));
            }
            sde_integrate(interp, base, member_args, Some(index), span)
        },

        // ----- §8.10 PDE --------------------------------------------------
        | B::SolvePoisson => {
            let Some(Value::Record(spec)) = args.first() else {
                return Err(
                    EvalError::new("E0302", "`solve_poisson` expects a spec record").at(span),
                );
            };
            let grid = grid_from(spec, name, span)?;
            let source = spec
                .fields
                .get("source")
                .map(|value| {
                    match value {
                        | Value::Tensor(tensor) => tensor_values(tensor),
                        | _ => Err(EvalError::new("E0302", "`source` must be a tensor").at(span)),
                    }
                })
                .transpose()?
                .unwrap_or_else(|| vec![0.0; grid.len()]);
            let iterations = spec.fields.get("iterations").map_or(Ok(10_000), |value| {
                index_value(value, span).map(|count| count.max(1))
            })?;
            let tolerance = spec
                .fields
                .get("tolerance")
                .and_then(Value::as_f64)
                .unwrap_or(1e-10);
            let outcome = aevia_std::pde::solve_poisson(&aevia_std::pde::PoissonSpec {
                grid,
                source,
                iterations,
                tolerance,
            });
            Ok(match outcome {
                | Ok(field) => ok(field_value(&field)),
                | Err(error) => err(pde_error_value(&error)),
            })
        },
        | B::SolveHeat | B::SolveWave | B::SolveAdvection => {
            let Some(Value::Record(spec)) = args.first() else {
                return Err(
                    EvalError::new("E0302", format!("`{name}` expects a spec record")).at(span),
                );
            };
            let grid = grid_from(spec, name, span)?;
            let number = |field: &str, default: f64| -> f64 {
                spec.fields
                    .get(field)
                    .and_then(Value::as_f64)
                    .unwrap_or(default)
            };
            let field_of = |field: &str| -> Result<Vec<f64>, EvalError> {
                match spec.fields.get(field) {
                    | Some(Value::Tensor(tensor)) => tensor_values(tensor),
                    | _ => {
                        Err(EvalError::new(
                            "E0306",
                            format!("`{name}`: the spec needs a tensor field `{field}`"),
                        )
                        .at(span))
                    },
                }
            };
            let steps = spec
                .fields
                .get("steps")
                .map_or(Ok(10), |value| index_value(value, span))?;
            let dt = number("dt", 0.01);
            let initial = field_of("initial")?;
            let outcome = match name {
                | "solve_heat" => {
                    let solver = match spec.fields.get("method") {
                        | None => aevia_std::pde::solve_heat,
                        | Some(Value::Str(method)) if method.as_ref() == "explicit" => {
                            aevia_std::pde::solve_heat
                        },
                        | Some(Value::Str(method)) if method.as_ref() == "adi" => {
                            aevia_std::pde::solve_heat_adi
                        },
                        | _ => {
                            return Ok(err(pde_error_value(&aevia_std::NumericError::Domain(
                                "heat method must be `explicit` or `adi`".into(),
                            ))));
                        },
                    };
                    solver(&aevia_std::pde::HeatSpec {
                        grid,
                        alpha: number("alpha", 1.0),
                        steps,
                        dt,
                        initial,
                    })
                    .map(|frames| {
                        frames
                            .into_iter()
                            .map(field_value_owned)
                            .collect::<Vec<_>>()
                    })
                },
                | "solve_wave" => {
                    aevia_std::pde::solve_wave(&aevia_std::pde::WaveSpec {
                        grid,
                        c: number("c", 1.0),
                        steps,
                        dt,
                        initial,
                        velocity: field_of("velocity")?,
                    })
                    .map(|frames| frames.iter().map(field_value).collect::<Vec<_>>())
                },
                | _ => {
                    aevia_std::pde::solve_advection(&aevia_std::pde::AdvectionSpec {
                        grid,
                        vx: number("vx", 1.0),
                        vy: number("vy", 0.0),
                        steps,
                        dt,
                        initial,
                    })
                    .map(|frames| frames.iter().map(field_value).collect::<Vec<_>>())
                },
            };
            Ok(match outcome {
                | Ok(frames) => {
                    let len = frames.len();
                    ok(build_tensor(frames, vec![len], Dim::one()))
                },
                | Err(error) => err(pde_error_value(&error)),
            })
        },

        // ----- §8.11 data -------------------------------------------------
        | B::ReadNpy => {
            Err(EvalError::new(
                "E0314",
                "`read_npy` requires an explicit rank, for example `read_npy<2>`",
            )
            .at(span))
        },
        | B::WriteNpy => {
            let path = expect_str(args, 0, name, span)?;
            let clean_path = path.split(['?', '#']).next().unwrap_or(&path);
            let extension = std::path::Path::new(clean_path)
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if extension != "npy" {
                return Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(err(
                    Value::string(format!(
                        "E0904: `write_npy` writes .npy files, not `{}`",
                        if extension.is_empty() {
                            "a file with no extension"
                        } else {
                            &extension
                        }
                    )),
                )))));
            }
            let value = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            let Value::Tensor(tensor) = value else {
                return Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(err(
                    Value::string(format!(
                        "E0902: `write_npy` needs a tensor, found {}",
                        value.type_name()
                    )),
                )))));
            };
            match npy_encode(&tensor) {
                | Ok(bytes) => {
                    Ok(Value::Io(Rc::new(crate::value::IoAction::WriteNpy {
                        path,
                        bytes,
                    })))
                },
                | Err(message) => {
                    Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(err(
                        Value::string(message),
                    )))))
                },
            }
        },
        | B::ReadSvg => {
            let path = expect_str(args, 0, name, span)?;
            Ok(Value::Io(Rc::new(crate::value::IoAction::ReadSvg(path))))
        },
        | B::WriteSvg => {
            let path = expect_str(args, 0, name, span)?;
            let contents = expect_str(args, 1, name, span)?;
            Ok(Value::Io(Rc::new(crate::value::IoAction::WriteSvg {
                path,
                contents,
            })))
        },
        | B::ReadGif => {
            let path = expect_str(args, 0, name, span)?;
            Ok(Value::Io(Rc::new(crate::value::IoAction::ReadGif(path))))
        },
        | B::WriteGif => {
            let path = expect_str(args, 0, name, span)?;
            let value = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            match gif_animation_from_value(&value) {
                | Ok(animation) => {
                    Ok(Value::Io(Rc::new(crate::value::IoAction::WriteGif {
                        path,
                        animation,
                    })))
                },
                | Err(message) => {
                    Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(err(
                        Value::string(message),
                    )))))
                },
            }
        },
        | B::ReadCsv | B::ReadCsvDyn => {
            let path = expect_str(args, 0, name, span)?;
            // Appendix B E0904: an extension this reader cannot decode is a
            // typed `Err`, not a generic parse failure later.
            let clean_path = path.split(['?', '#']).next().unwrap_or(&path);
            let extension = std::path::Path::new(clean_path)
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !matches!(extension.as_str(), "csv" | "tsv") {
                let reason = Value::string(format!(
                    "E0904: `read_csv` reads .csv/.tsv files, not `{}`",
                    if extension.is_empty() {
                        "a file with no extension"
                    } else {
                        &extension
                    }
                ));
                return Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(err(
                    reason,
                )))));
            }
            let parse = if extension == "tsv" {
                aevia_std::table::parse_tsv
            } else {
                aevia_std::table::parse_csv
            };
            Ok(Value::Io(Rc::new(crate::value::IoAction::Bind {
                action: Rc::new(crate::value::IoAction::ReadFile(path)),
                func: native(move |_interp, values, span| {
                    // The bind continuation receives the `Result` from
                    // `read_file`: unwrap `Ok(contents)`, propagate `Err` with
                    // its taxonomy (E0901/E0902).
                    let text = match values.first() {
                        | Some(Value::Variant(result))
                            if result.adt == adt::RESULT_ID && result.index == 0 =>
                        {
                            match result.fields.first() {
                                | Some(Value::Str(text)) => text.to_string(),
                                | other => {
                                    let reason = Value::string(format!(
                                        "the file could not be read: {}",
                                        other.map_or("nothing", Value::type_name)
                                    ));
                                    let failed = Value::Io(Rc::new(crate::value::IoAction::Pure(
                                        err(reason),
                                    )));
                                    return Ok(failed);
                                },
                            }
                        },
                        | Some(Value::Variant(previous))
                            if previous.adt == adt::RESULT_ID && previous.index == 1 =>
                        {
                            let failed = previous.fields.first().cloned().unwrap_or_else(|| {
                                Value::string("E0901: the file could not be read")
                            });
                            let failed =
                                Value::Io(Rc::new(crate::value::IoAction::Pure(err(failed))));
                            return Ok(failed);
                        },
                        | other => {
                            let reason = Value::string(format!(
                                "the file could not be read: {}",
                                other.map_or("nothing", Value::type_name)
                            ));
                            let failed =
                                Value::Io(Rc::new(crate::value::IoAction::Pure(err(reason))));
                            return Ok(failed);
                        },
                    };
                    let result = match parse(&text) {
                        | Ok(table) => ok(table_value(&table)),
                        | Err(error) => err(Value::string(format!("E0903: {error}"))),
                    };
                    let _ = span;
                    Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(result))))
                }),
            })))
        },
        | B::WriteCsv => {
            let path = expect_str(args, 0, name, span)?;
            let table = table_from(args, 1, name, span)?;
            let text = aevia_std::table::write_csv(&table);
            Ok(Value::Io(Rc::new(crate::value::IoAction::Bind {
                action: Rc::new(crate::value::IoAction::WriteFile { path, contents: text }),
                func: native(|_interp, values, _span| {
                    // Propagate a failed write with its taxonomy code instead of
                    // reporting success.
                    if let Some(Value::Variant(result)) = values.first()
                        && result.adt == adt::RESULT_ID
                        && result.index == 1
                    {
                        let failed = result.fields.first().cloned().unwrap_or_else(|| {
                            Value::string("E0901: the file could not be written")
                        });
                        return Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(err(
                            failed,
                        )))));
                    }
                    Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(ok(
                        Value::Unit,
                    )))))
                }),
            })))
        },
        // §8.17: `Json` is the *decoded* value channel. `read_json` and
        // `simulate` already decode on the way in (`value_from_json`) and
        // `simulate`/`write_json` encode on the way out (`json_from_value`), so
        // `to_json`/`from_json` are the identity over that representation.
        // Returning JSON *text* here would make `simulate("ising", to_json(p))`
        // send a JSON string literal instead of the object.
        | B::ToJson => args.first().cloned().ok_or_else(|| arity(name, 1, 0, span)),
        | B::FromJson => {
            let value = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            Ok(ok(value))
        },
        | B::ReadJson => {
            let path = expect_str(args, 0, name, span)?;
            let clean_path = path.split(['?', '#']).next().unwrap_or(&path);
            let extension = std::path::Path::new(clean_path)
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if extension != "json" {
                let reason = Value::string(format!(
                    "E0904: `read_json` reads .json files, not `{}`",
                    if extension.is_empty() {
                        "a file with no extension"
                    } else {
                        &extension
                    }
                ));
                return Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(err(
                    reason,
                )))));
            }
            Ok(Value::Io(Rc::new(crate::value::IoAction::Bind {
                action: Rc::new(crate::value::IoAction::ReadFile(path)),
                func: native(|_interp, values, _span| {
                    let text = match values.first() {
                        | Some(Value::Variant(result))
                            if result.adt == adt::RESULT_ID && result.index == 0 =>
                        {
                            match result.fields.first() {
                                | Some(Value::Str(text)) => text.to_string(),
                                | other => {
                                    let reason = Value::string(format!(
                                        "the file could not be read: {}",
                                        other.map_or("nothing", Value::type_name)
                                    ));
                                    let failed = Value::Io(Rc::new(crate::value::IoAction::Pure(
                                        err(reason),
                                    )));
                                    return Ok(failed);
                                },
                            }
                        },
                        | Some(Value::Variant(previous))
                            if previous.adt == adt::RESULT_ID && previous.index == 1 =>
                        {
                            let failed = previous.fields.first().cloned().unwrap_or_else(|| {
                                Value::string("E0901: the file could not be read")
                            });
                            let failed =
                                Value::Io(Rc::new(crate::value::IoAction::Pure(err(failed))));
                            return Ok(failed);
                        },
                        | other => {
                            let reason = Value::string(format!(
                                "the file could not be read: {}",
                                other.map_or("nothing", Value::type_name)
                            ));
                            let failed =
                                Value::Io(Rc::new(crate::value::IoAction::Pure(err(reason))));
                            return Ok(failed);
                        },
                    };
                    let result = match aevia_std::table::parse_json(&text) {
                        | Ok(json) => ok(value_from_json(&json)),
                        | Err(error) => err(Value::string(format!("E0903: {error}"))),
                    };
                    Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(result))))
                }),
            })))
        },
        | B::WriteJson => {
            let path = expect_str(args, 0, name, span)?;
            let value = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            let json = json_from_value(&value).map_err(|error| error.at(span))?;
            let text = aevia_std::table::write_json(&json);
            Ok(Value::Io(Rc::new(crate::value::IoAction::Bind {
                action: Rc::new(crate::value::IoAction::WriteFile { path, contents: text }),
                func: native(|_interp, values, _span| {
                    if let Some(Value::Variant(result)) = values.first()
                        && result.adt == adt::RESULT_ID
                        && result.index == 1
                    {
                        let failed = result.fields.first().cloned().unwrap_or_else(|| {
                            Value::string("E0901: the file could not be written")
                        });
                        return Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(err(
                            failed,
                        )))));
                    }
                    Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(ok(
                        Value::Unit,
                    )))))
                }),
            })))
        },
        | B::Column | B::ColumnDyn => {
            let table = table_from(args, 0, name, span)?;
            let column = expect_str(args, 1, name, span)?;
            let cells = match table.column(&column) {
                | Ok(cells) => cells,
                | Err(error) => return Ok(err(Value::string(error.to_string()))),
            };
            let mut elements = Vec::with_capacity(cells.len());
            let mut numbers = Vec::with_capacity(cells.len());
            let mut all_numeric = true;
            for cell in &cells {
                match cell.parse::<f64>() {
                    | Ok(number) => numbers.push(number),
                    | Err(_) => all_numeric = false,
                }
                elements.push(match cell.parse::<f64>() {
                    | Ok(number) => Value::float(number),
                    | Err(_) => Value::string(*cell),
                });
            }
            let dim = Dim::one();
            let len = elements.len();
            let tensor = if all_numeric {
                float_tensor(vec![len], numbers, dim)
            } else {
                build_tensor(elements, vec![len], dim)
            };
            Ok(ok(tensor))
        },
        | B::Row => {
            let table = table_from(args, 0, name, span)?;
            let index = expect_exact_int(args, 1, name, span)?;
            let Some(row) = index_in_extent(&index, table.len()) else {
                return Ok(err(Value::string("row index out of range")));
            };
            Ok(ok(row_value(&table, row)))
        },
        | B::Select => {
            let table = table_from(args, 0, name, span)?;
            let names = expect_tensor(args, 1, name, span)?;
            let mut selected = Vec::new();
            for name in names.elements() {
                if let Value::Str(text) = name {
                    selected.push(text.to_string());
                }
            }
            let indices: Vec<_> = selected
                .iter()
                .map(|column| table.columns.iter().position(|name| name == column))
                .collect();
            let units = indices
                .iter()
                .map(|index| {
                    index
                        .and_then(|index| table.units.get(index).cloned())
                        .flatten()
                })
                .collect();
            let rows = table
                .rows
                .iter()
                .map(|row| {
                    indices
                        .iter()
                        .map(|index| {
                            index
                                .and_then(|index| row.get(index).cloned())
                                .unwrap_or_default()
                        })
                        .collect()
                })
                .collect();
            Ok(table_value(&aevia_std::table::Table {
                columns: selected,
                units,
                rows,
            }))
        },
        | B::Schema => {
            let table = table_from(args, 0, name, span)?;
            let mut elements = Vec::with_capacity(table.columns.len());
            for (index, column) in table.columns.iter().enumerate() {
                let numeric = table.rows.iter().all(|row| {
                    row.get(index)
                        .is_some_and(|cell| cell.parse::<f64>().is_ok())
                });
                elements.push(record_of(vec![
                    ("name", Value::string(column.clone())),
                    (
                        "dtype",
                        Value::string(if numeric { "Float" } else { "String" }),
                    ),
                    (
                        "unit",
                        match table.units.get(index).and_then(Option::as_ref) {
                            | Some(unit) => some(Value::string(unit.clone())),
                            | None => none(),
                        },
                    ),
                ]));
            }
            let len = elements.len();
            Ok(build_tensor(elements, vec![len], Dim::one()))
        },
        | B::ToTable => {
            let tensor = expect_tensor(args, 0, name, span)?;
            let mut columns: Vec<String> = Vec::new();
            let mut rows: Vec<Vec<String>> = Vec::new();
            for element in tensor.elements() {
                let Value::Record(record) = element else {
                    return Ok(err(Value::string("`to_table` needs a tensor of records")));
                };
                if columns.is_empty() {
                    columns = record.fields.keys().cloned().collect();
                }
                let mut row = Vec::with_capacity(columns.len());
                for column in &columns {
                    row.push(record.fields.get(column).map_or_else(String::new, |value| {
                        match value {
                            | Value::Str(text) => text.to_string(),
                            | other => crate::value::show(other),
                        }
                    }));
                }
                rows.push(row);
            }
            let units = vec![None; columns.len()];
            Ok(ok(table_value(&aevia_std::table::Table {
                columns,
                units,
                rows,
            })))
        },
        | B::ToDynamic => {
            let table = table_from(args, 0, name, span)?;
            Ok(table_value(&table))
        },

        // ----- §8.12 plotting ---------------------------------------------
        | B::Line
        | B::Scatter
        | B::LineWith
        | B::ScatterWith
        | B::Hist
        | B::HistWith
        | B::Heatmap
        | B::HeatmapWith
        | B::Errorbar
        | B::ErrorbarWith => plot::build_series(name, args, span),
        | B::DefaultPlotOpts => Ok(default_plot_opts()),

        // ----- §8.17 simulation -------------------------------------------
        | B::Simulate => {
            let scenario = expect_str(args, 0, name, span)?;
            let value = args.get(1).cloned().unwrap_or(Value::Unit);
            let json = json_from_value(&value).map_err(|error| error.at(span))?;
            let text = aevia_std::table::write_json(&json);
            match aevia_std::sim::simulate(&scenario, &text) {
                | Ok(result) => {
                    match aevia_std::table::parse_json(&result) {
                        | Ok(json) => Ok(ok(value_from_json(&json))),
                        | Err(error) => Ok(err(Value::string(error.to_string()))),
                    }
                },
                | Err(error) => Ok(err(Value::string(error))),
            }
        },

        // ----- §8.18 device -----------------------------------------------
        | B::DeviceOf => {
            let _ = expect_tensor(args, 0, name, span)?;
            Ok(make_variant(
                adt::intern(adt::Adt {
                    name: "Device".to_string(),
                    params: Vec::new(),
                    ctors: Vec::new(),
                }),
                0,
                "Cpu",
                Vec::new(),
            ))
        },
        | B::ToDevice => {
            let tensor = expect_tensor(args, 0, name, span)?;
            require_cpu_placement(args.get(1), name, span)?;
            Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(
                Value::Tensor(tensor),
            ))))
        },
        | B::On => {
            let thunk = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            require_cpu_placement(args.first(), name, span)?;
            let value = interp.apply(thunk, Vec::new(), span)?;
            Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(value))))
        },

        // ----- §8.19 dynamic boundary -------------------------------------
        | B::TypeRep => {
            Err(EvalError::new("E0304", "type_rep requires concrete type evidence").at(span))
        },
        | _ => {
            Err(EvalError::new(
                "E1001",
                format!("builtin `{name}` is typed but not implemented yet"),
            )
            .at(span))
        },
    }
}

fn require_cpu_placement(
    device: Option<&Value>,
    operation: &str,
    span: Span,
) -> Result<(), EvalError> {
    let Some(Value::Variant(device)) = device else {
        return Ok(());
    };
    let capability = match device.name.as_str() {
        | "Cpu" => return Ok(()),
        | "Distributed" => "core.distributed",
        | _ => "core.gpu",
    };
    Err(EvalError::new(
        "E1001",
        format!(
            "`{operation}` needs the `{capability}` placement capability for `{}`, which the interpreter does not provide",
            device.name
        ),
    )
    .at(span))
}

/// Applies a one-argument function to a fresh symbol and returns the variable
/// name with the resulting term.
///
/// Every calculus entry point works this way: `antiderivative(f)` is a request
/// about `f(ξ)`, and the user's closure is the only way to learn what `f` is.
/// Applies a one-argument function to a fresh symbolic variable.
///
/// The variable carries `dim`, the dimension of the argument the caller will
/// eventually supply. Dimensions are host-side (§10.3.6), so a body like
/// `fn t => t^2` only reports `L^2` if `t` is given `L`; passing a
/// dimensionless variable would make every calculus result dimensionless.
fn apply_to_fresh(
    interp: &mut Interp,
    name: &str,
    args: &[Value],
    span: Span,
    dim: Dim,
) -> Result<(String, Value), EvalError> {
    let function = args
        .first()
        .cloned()
        .ok_or_else(|| arity(name, 1, 0, span))?;
    let variable = fresh_variable_name();
    let applied = interp.apply(function, vec![symbolic_variable(&variable, dim)], span)?;
    if !matches!(applied, Value::Num(_)) {
        return Err(EvalError::new(
            "E0302",
            format!("`{name}` expects a function returning a number"),
        )
        .at(span));
    }
    Ok((variable, applied))
}

/// A name no other fresh variable uses.
///
/// A fixed name would let a nested call capture the outer variable: the inner
/// `antiderivative` would reuse `x` and the outer substitution would rewrite
/// both.
fn fresh_variable_name() -> String {
    use std::cell::Cell;
    thread_local! {
        static COUNTER: Cell<u64> = const { Cell::new(0) };
    }
    let id = COUNTER.with(|counter| {
        let next = counter.get().wrapping_add(1);
        counter.set(next);
        next
    });
    format!("x#diff{id}")
}

/// A symbolic variable with the given dimension.
fn symbolic_variable(
    name: &str,
    dim: Dim,
) -> Value {
    Value::Num(NumVal::symbolic(Term::Sym(name.to_string()), dim))
}

/// The dimension of `f(argument)`, by applying `f` to a variable of the
/// argument's dimension.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "isolates inference of the symbolic derivative dimension"
    )
)]
fn argument_dim(
    interp: &mut Interp,
    function: &Value,
    argument: &Value,
    span: Span,
) -> Result<Dim, EvalError> {
    let variable = fresh_variable_name();
    let applied = interp.apply(
        function.clone(),
        vec![symbolic_variable(
            &variable,
            crate::value::value_dim(argument),
        )],
        span,
    )?;
    Ok(crate::value::value_dim(&applied))
}

/// The value of `symfun(name)`: applying it builds `apply(name, args…)`.
///
/// The result's dimension is the codomain the checker gave this call site
/// (`SymFun<d, e>` is `Real[d] -> Real[e]`), read from the resolved
/// expression types, so `y(t)` for `y : SymFun<T, L>` is a length.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "owns the native closure that applies a symbolic function"
    )
)]
fn symfun_value(internal: String) -> Value {
    Value::Native(Rc::new(NativeFn(Box::new(move |interp, args, span| {
        let mut kernel_args = vec![aevia_rssn::Val::Symbol(internal.clone())];
        for arg in &args {
            let term = value_to_term(arg)?;
            kernel_args.push(bridge::term_val(&term)?);
        }
        let node = aevia_rssn::kernel()
            .apply_node("apply", &kernel_args)
            .map_err(|failure| bridge::error(&failure).at(span))?;
        let term = bridge::term_of(&node).ok_or_else(|| {
            EvalError::new(
                "E1003",
                "the kernel built no term for a function application",
            )
            .at(span)
        })?;
        let dim = match interp.expr_types.get(&span) {
            | Some(aevia_types::Type::Scalar(_, dim)) => dim.clone(),
            | _ => Dim::one(),
        };
        Ok(Value::Num(NumVal::symbolic(term, dim)))
    }))))
}

/// The kernel `eq(lhs, rhs)` node of an equation proposition.
fn equation_val(
    value: &Value,
    what: &str,
    span: Span,
) -> Result<aevia_rssn::Val, EvalError> {
    let Value::Prop(prop) = value else {
        return Err(EvalError::new(
            "E0302",
            format!(
                "`{what}` must be an equation `lhs == rhs`, found {}",
                value.type_name()
            ),
        )
        .at(span));
    };
    let Some(comparison) = prop.comparison.as_ref().filter(|c| c.op == BinOp::Eq) else {
        return Err(EvalError::new(
            "E0302",
            format!(
                "`{what}` must be an equation `lhs == rhs`, found `{}`",
                prop.text
            ),
        )
        .at(span));
    };
    let lhs = bridge::term_val(&value_to_term(&comparison.left)?)?;
    let rhs = bridge::term_val(&value_to_term(&comparison.right)?)?;
    aevia_rssn::kernel()
        .apply_node("eq", &[lhs, rhs])
        .map_err(|failure| bridge::error(&failure).at(span))
}

/// `dsolve(eq, y, t)` and `dsolve(eq, y, t, conditions)` (§6.4.5).
///
/// The kernel's `dsolve(eq, y(t))` (or with `list(conditions…)`) answers
/// `y(t) = F(t)`; the result is the function `s ↦ F(s)` with the codomain's
/// dimension. An implicit or parametric answer, or no answer, is
/// `Err(MathError)` carrying the kernel's rendering, never a guess.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "implements the symbolic differential equation boundary"
    )
)]
fn dsolve(
    interp: &mut Interp,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    if !(3..=4).contains(&args.len()) {
        return Err(arity("dsolve", 3, args.len(), span));
    }
    let [equation, function, variable, conditions @ ..] = args else {
        return Err(arity("dsolve", 3, args.len(), span));
    };
    let equation = equation_val(equation, "dsolve", span)?;
    let Value::Num(var_num) = variable else {
        return Err(EvalError::new("E0302", "`dsolve` needs a symbol as its variable").at(span));
    };
    let Some(var_name) = symbol_name(var_num) else {
        return Err(EvalError::new("E0302", "`dsolve` needs a symbol as its variable").at(span));
    };
    let unknown = interp.apply(function.clone(), vec![variable.clone()], span)?;
    let unknown_val = bridge::term_val(&value_to_term(&unknown)?)?;
    let result_dim = match &unknown {
        | Value::Num(num) => num.dim.clone(),
        | _ => Dim::one(),
    };
    let mut request = vec![equation, unknown_val.clone()];
    if let [conditions] = conditions {
        let Value::Tensor(list) = conditions else {
            return Err(
                EvalError::new("E0302", "`dsolve` conditions are a list of equations").at(span),
            );
        };
        let mut items = Vec::new();
        for condition in &list.elements() {
            items.push(equation_val(condition, "dsolve condition", span)?);
        }
        let list = aevia_rssn::kernel()
            .apply_node("list", &items)
            .map_err(|failure| bridge::error(&failure).at(span))?;
        request.push(list);
    }
    let kernel = aevia_rssn::kernel();
    let failure = |message: String| Ok(err(Value::string(message)));
    let node = match kernel.apply_node("dsolve", &request) {
        | Ok(node) => node,
        | Err(failure_value) => return failure(failure_value.message),
    };
    let aevia_rssn::Val::Term(handle) = node else {
        return failure("the kernel built no `dsolve` request".to_string());
    };
    let computed = match kernel.compute(handle, &aevia_rssn::Opts::symbolic()) {
        | Ok(computed) => computed,
        | Err(error) => return failure(error.message),
    };
    let aevia_rssn::Val::Term(answer) = &computed.val else {
        return failure(format!("dsolve answered `{}`", computed.val.text()));
    };
    let text = kernel.try_text(*answer).unwrap_or_default();
    if !computed.reduced {
        return failure(format!("dsolve found no closed-form solution: {text}"));
    }
    let explicit = kernel.structure(*answer).ok().and_then(|(op, children)| {
        let [lhs, rhs] = children.as_slice() else {
            return None;
        };
        let lhs_text = kernel.try_text(*lhs).ok()?;
        let unknown_text = match &unknown_val {
            | aevia_rssn::Val::Term(sym) => kernel.try_text(*sym).ok()?,
            | other => other.text(),
        };
        (op == "eq" && lhs_text == unknown_text).then_some(*rhs)
    });
    let Some(rhs) = explicit else {
        return failure(format!(
            "dsolve found an implicit or parametric solution: {text}"
        ));
    };
    let Some(solution) = bridge::term_of(&aevia_rssn::Val::Term(rhs)) else {
        return failure(format!("dsolve answered `{text}`, which has no term form"));
    };
    let solution = Rc::new(solution);
    let function = Value::Native(Rc::new(NativeFn(Box::new(move |_interp, args, span| {
        let Some(argument) = args.first() else {
            return Err(arity("dsolve solution", 1, 0, span));
        };
        let replacement = value_to_term(argument)?;
        let value = solution.substitute(&var_name, &replacement)?;
        Ok(fold_closed(Value::Num(NumVal::symbolic(
            value,
            result_dim.clone(),
        ))))
    }))));
    Ok(ok(function))
}

/// Wraps a fallible kernel answer in `Result<_, MathError>`.
///
/// A heavy request is a simplification boundary (§6.3.3), so an answer that is
/// a *closed* symbolic form is folded to the number it denotes: `limit(sin(t)/t,
/// 0)` answering `cos(0)` is the same answer as `1`, and leaving the unevaluated
/// form to the program would make the result depend on which phase the argument
/// happened to be written in (an exact `0` and a float `0.0` must agree).
/// Answers with free symbols are untouched.
fn math_result(result: Result<Value, EvalError>) -> Value {
    match result {
        | Ok(value) => ok(fold_closed(value)),
        | Err(error) => err(Value::string(error.message)),
    }
}

/// Folds a symbolic answer with no free symbols to its numeric value.
fn fold_closed(value: Value) -> Value {
    let Value::Num(num) = &value else {
        return value;
    };
    if !num.is_symbolic() {
        return value;
    }
    let Ok(term) = value_to_term(&value) else {
        return value;
    };
    bridge::fold(&term, num.dim.clone()).unwrap_or(value)
}

/// Runs a solver and returns its structured answer, as §8.6's record.
///
/// The answer is the kernel's `Solutions` record — `variable`, `roots`,
/// `multiplicities`, `conditions`, `complete` — not a bare root list. Three of
/// those fields cannot be read off a list, so flattening them away (which is
/// what `solve` used to do) made `complete` a constant `false` and left the
/// solver's multiplicities behind.
///
/// # Errors
///
/// Propagates the kernel's diagnostic.
fn solve(
    operator: &str,
    equation: &Value,
    variable: &Value,
) -> Result<aevia_rssn::Val, EvalError> {
    let equation = bridge::to_val(equation)?;
    let variable = bridge::to_val(variable)?;
    aevia_rssn::kernel()
        .solve_structured(operator, &equation, &variable)
        .map_err(|failure| bridge::error(&failure))
}

/// The `Solutions` record of §8.6, from the kernel's own record.
///
/// The kernel's `Solutions` is already a record with these five field names,
/// so the bridge hands it over unchanged. The one field whose *type* differs
/// is `variable`: the kernel writes a symbol (or the empty string for "none")
/// and §8.6 types it `Option<String>`, so it is reshaped here.
///
/// Nothing is derived. `multiplicities` is whatever the solver reported —
/// empty when it did not distinguish the roots, which is not the same as a
/// reported `1` — and `complete` is the solver's claim, not a constant.
fn solutions_value(
    solutions: &aevia_rssn::Val,
    roots_dim: Dim,
) -> Value {
    let aevia_rssn::Val::Record(fields) = solutions else {
        // The bridge always answers with the record; a value of any other
        // shape is a bridge defect, and showing it is better than inventing
        // five fields around it.
        return bridge::from_val(solutions, roots_dim);
    };
    let mut converted = IndexMap::new();
    for (name, value) in fields {
        let field = match name.as_str() {
            | "variable" => {
                match value {
                    | aevia_rssn::Val::Symbol(name) | aevia_rssn::Val::Str(name)
                        if !name.is_empty() =>
                    {
                        make_variant(
                            adt::OPTION_ID,
                            1,
                            "Some",
                            vec![Value::string(crate::kernel::clean_internal_names(name))],
                        )
                    },
                    | _ => make_variant(adt::OPTION_ID, 0, "None", Vec::new()),
                }
            },
            // The roots are values of the unknown, so they carry its
            // dimension; the three metadata fields do not.
            | "roots" => bridge::from_val(value, roots_dim.clone()),
            | _ => bridge::from_val(value, Dim::one()),
        };
        converted.insert(name.clone(), field);
    }
    Value::Record(Rc::new(RecordValue { fields: converted }))
}

/// Decides a proposition when it reduces to a comparison of numbers.
///
/// Exact operands are compared as rationals (§6.12.1): routing them through
/// `f64` made `decide(1/3 + 1/3 == 2/3)` depend on the rounding of `1/3`.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "converts the three-valued proposition into an Aevia option"
    )
)]
fn decide(
    args: &[Value],
    span: Span,
) -> Value {
    let _ = span;
    match args.first() {
        // §6.12.1: `Real == Real` is typed `Prop`, but after type erasure the
        // evaluator can hold the already-decided `Bool` (two exact values are
        // compared at run time). A decided proposition is its own verdict, so
        // `decide(x == x)` on exact values answers `Some(true)` instead of
        // `None` — the alternative is a guarantee the checker cannot see.
        | Some(Value::Bool(verdict)) => {
            make_variant(adt::OPTION_ID, 1, "Some", vec![Value::Bool(*verdict)])
        },
        | Some(Value::Prop(prop)) => {
            match decide_prop(prop) {
                | Some(verdict) => {
                    make_variant(adt::OPTION_ID, 1, "Some", vec![Value::Bool(verdict)])
                },
                | None => make_variant(adt::OPTION_ID, 0, "None", Vec::new()),
            }
        },
        | _ => make_variant(adt::OPTION_ID, 0, "None", Vec::new()),
    }
}

/// The truth value of a proposition, when it has one (§8.1.2).
fn decide_prop(prop: &crate::value::PropValue) -> Option<bool> {
    let comparison = prop.comparison.as_ref()?;
    let crate::value::Comparison { op, left, right } = comparison;
    if let (Value::Num(a), Value::Num(b)) = (left, right) {
        if a.is_symbolic() || b.is_symbolic() {
            return None;
        }
        if let (Some(a), Some(b)) = (a.to_rational(), b.to_rational()) {
            return Some(match op {
                | BinOp::Eq => a == b,
                | BinOp::Ne => a != b,
                | BinOp::Lt => a < b,
                | BinOp::Le => a <= b,
                | BinOp::Gt => a > b,
                | BinOp::Ge => a >= b,
                | _ => return None,
            });
        }
    }
    match op {
        | BinOp::Eq => crate::value::equal(left, right).ok(),
        | BinOp::Ne => crate::value::equal(left, right).ok().map(|equal| !equal),
        | _ => None,
    }
}

/// The symbolic term for the condition of a proposition.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "encodes a proposition comparison as a kernel term"
    )
)]
fn condition_term(prop: &crate::value::PropValue) -> Result<Term, EvalError> {
    let Some(comparison) = &prop.comparison else {
        return Err(EvalError::new(
            "E1001",
            "only a comparison can be turned into a symbolic condition",
        ));
    };
    let name = match comparison.op {
        | BinOp::Eq => "eq",
        | BinOp::Ne => "ne",
        | BinOp::Lt => "lt",
        | BinOp::Le => "le",
        | BinOp::Gt => "gt",
        | BinOp::Ge => "ge",
        | _ => {
            return Err(EvalError::new(
                "E1001",
                "only a comparison can be turned into a symbolic condition",
            ));
        },
    };
    Ok(Term::fun(
        Fun::Other(name),
        vec![
            value_to_term(&comparison.left)?,
            value_to_term(&comparison.right)?,
        ],
    ))
}

/// `by_cases(p, yes, no)` and `assuming(p, thunk)` (§8.1.2).
///
/// `by_cases` takes a branch only when the proposition is decidable; when it
/// is not, it builds the symbolic `ite` term that a piecewise definition
/// needs. Taking the `yes` branch regardless — which is what this did — makes
/// `by_cases(x > 0, fn () => 1, fn () => -1)` answer `1` for every `x`.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "owns conditional symbolic assembly and detects undecidable propositions"
    )
)]
fn branch_on(
    interp: &mut Interp,
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let proposition = args
        .first()
        .cloned()
        .ok_or_else(|| arity(name, 2, 0, span))?;
    let thunk = args
        .get(1)
        .cloned()
        .ok_or_else(|| arity(name, 2, args.len(), span))?;
    let verdict = match &proposition {
        | Value::Bool(flag) => Some(*flag),
        | Value::Prop(prop) => decide_prop(prop),
        | other => {
            return Err(EvalError::new(
                "E0302",
                format!(
                    "`{name}` expects a proposition, found {}",
                    other.type_name()
                ),
            )
            .at(span));
        },
    };
    if name == "assuming" {
        // §8.1.2: `assuming` states a fact about a symbol. A proposition the
        // runtime can prove false makes the assumption contradictory, which is
        // `E0804`; otherwise the thunk is evaluated under it.
        return match verdict {
            | Some(false) => Err(EvalError::new("E0804", "the assumption is false").at(span)),
            | _ => interp.apply(thunk, Vec::new(), span),
        };
    }
    let alternative = args
        .get(2)
        .cloned()
        .ok_or_else(|| arity(name, 3, args.len(), span))?;
    match verdict {
        | Some(true) => interp.apply(thunk, Vec::new(), span),
        | Some(false) => interp.apply(alternative, Vec::new(), span),
        | None => {
            // Undecidable: keep both branches in the term (§8.1.2).
            let Value::Prop(prop) = &proposition else {
                return Err(EvalError::new(
                    "E1001",
                    "`by_cases` needs a comparison to build a symbolic branch",
                )
                .at(span));
            };
            let condition = condition_term(prop).map_err(|error| error.at(span))?;
            let yes = interp.apply(thunk, Vec::new(), span)?;
            let no = interp.apply(alternative, Vec::new(), span)?;
            let term = Term::fun(
                Fun::Ite,
                vec![condition, value_to_term(&yes)?, value_to_term(&no)?],
            );
            Ok(Value::Num(NumVal::symbolic(
                term,
                crate::value::value_dim(&yes),
            )))
        },
    }
}

/// `forall(f)`: samples the argument and decides the proposition (§8.13).
///
/// The runtime does not know `a`, so it samples the numeric values every
/// `Arbitrary` instance of `Real`/`Int`/`Float` includes. A parameter that
/// cannot take a number is a missing capability, named as `E1001` — returning
/// `true` for every property was worse than saying so.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "implements finite proposition sampling outside the dispatch table"
    )
)]
fn forall(
    interp: &mut Interp,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let function = args
        .first()
        .cloned()
        .ok_or_else(|| arity("forall", 1, 0, span))?;
    let arity_count = match &function {
        | Value::Closure(closure) => closure.arity(),
        | _ => {
            return Err(EvalError::new(
                "E0302",
                format!(
                    "`forall` expects a function, found {}",
                    function.type_name()
                ),
            )
            .at(span));
        },
    };
    if arity_count == 0 || arity_count > 3 {
        return Err(EvalError::new(
            "E1001",
            format!("`forall` cannot sample a function of {arity_count} argument(s)"),
        )
        .at(span));
    }
    let samples = [
        Value::int(0),
        Value::int(1),
        Value::int(-1),
        Value::int(2),
        Value::int(-3),
        Value::Num(NumVal::exact(
            BigRational::new(BigInt::one(), BigInt::from(2)),
            Dim::one(),
        )),
    ];
    let mut undecidable = false;
    let mut sample_iters = vec![samples.iter().enumerate().cycle().skip(1); arity_count];
    let mut arguments = vec![Value::int(0); arity_count];
    loop {
        let outcome = interp.apply(function.clone(), arguments.clone(), span);
        let value = match outcome {
            | Ok(value) => value,
            // The sampled value does not fit the parameter type: `forall` has
            // no `Arbitrary` instance for that type.
            | Err(error) if matches!(error.code, "E0302" | "E0305" | "E0308") => {
                return Err(EvalError::new(
                    "E1001",
                    "`forall` has no `Arbitrary` instance for this argument type",
                )
                .at(span)
                .with_note(error.message));
            },
            | Err(error) => return Err(error),
        };
        match value {
            | Value::Prop(prop) => {
                match decide_prop(&prop) {
                    | Some(true) => {},
                    | Some(false) => return Ok(Value::Bool(false)),
                    | None => undecidable = true,
                }
            },
            | Value::Bool(true) => {},
            | Value::Bool(false) => return Ok(Value::Bool(false)),
            | other => {
                return Err(EvalError::new(
                    "E0302",
                    format!("`forall` expects a `Prop`, found {}", other.type_name()),
                )
                .at(span));
            },
        }
        // Advance the first sampling position and carry on wraparound.
        let mut exhausted = true;
        for (argument, iter) in arguments.iter_mut().zip(&mut sample_iters) {
            let Some((index, next)) = iter.next() else {
                break;
            };
            *argument = next.clone();
            if index != 0 {
                exhausted = false;
                break;
            }
        }
        if exhausted {
            return if undecidable {
                Err(
                    EvalError::new("E0804", "`forall` sampled a proposition it cannot decide")
                        .at(span),
                )
            } else {
                Ok(Value::Bool(true))
            };
        }
    }
}

/// `sum(f, a, b)` (§8.5).
///
/// The bound is exact: `f` is applied at every integer in `[a, b]` and the
/// values are added with the same promotion rules as `+`. A symbolic or
/// non-integral bound needs the kernel's summation and is `E1001`.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps exact integer summation and its symbolic fallback together"
    )
)]
fn sum_range(
    interp: &mut Interp,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let function = args
        .first()
        .ok_or_else(|| arity("sum", 3, args.len(), span))?;
    let lower_num = expect_num(args, 1, "sum", span)?;
    let upper_num = expect_num(args, 2, "sum", span)?;
    // Bounds are integers (§8.5); an exact non-integer bound is not silently
    // truncated.
    let integer = |num: &NumVal| {
        num.to_rational()
            .filter(|value| value.denom().is_one())
            .and_then(|value| value.numer().to_i64())
    };
    let concrete = match (integer(&lower_num), integer(&upper_num)) {
        | (Some(lower), Some(upper)) if upper < lower => {
            return Ok(Value::Num(NumVal::int(BigInt::zero())));
        },
        | (Some(lower), Some(upper)) if upper.abs_diff(lower) < 10_000 => Some((lower, upper)),
        | _ => None,
    };
    let Some((lower, upper)) = concrete else {
        for bound in [&lower_num, &upper_num] {
            if let Some(value) = bound.to_rational()
                && !value.denom().is_one()
            {
                return Err(EvalError::new(
                    "E0302",
                    format!(
                        "`sum` bounds are integers, found {}",
                        show(&Value::Num(bound.clone()))
                    ),
                )
                .at(span));
            }
        }
        // An infinite, symbolic or long range asks the kernel for the closed
        // form (`sum(1/k^2, 1, oo) = pi^2/6`) instead of looping.
        return symbolic_range("sum", interp, args, &lower_num, &upper_num, span);
    };
    let mut total = Value::Num(NumVal::int(BigInt::zero()));
    for step in lower..=upper {
        let term = interp.apply(function.clone(), vec![Value::int(step)], span)?;
        total = interp.apply_binary("+", total, term, span)?;
    }
    Ok(total)
}

/// `sum`/`product` of `f(k)` for `k` from `lower` to `upper` by the kernel.
///
/// Used when the range cannot (infinite, symbolic) or should not (more than
/// 10^4 terms) be looped over. A kernel answer that still contains the
/// unevaluated `sum(…)`/`product(…)` is reported as `E0808` rather than
/// passed off as a closed form.
fn symbolic_range(
    op: &str,
    interp: &mut Interp,
    args: &[Value],
    lower: &NumVal,
    upper: &NumVal,
    span: Span,
) -> Result<Value, EvalError> {
    let (variable, applied) = apply_to_fresh(interp, op, args, span, lower.dim.clone())?;
    let dim = crate::value::value_dim(&applied);
    let symbol = symbolic_variable(&variable, lower.dim.clone());
    let result = bridge::call(
        op,
        &[
            applied,
            symbol,
            Value::Num(lower.clone()),
            Value::Num(upper.clone()),
        ],
        dim,
    )
    .map_err(|error| error.at(span))?;
    let unevaluated = match &result {
        | Value::Num(NumVal {
            kind: NumKind::Sym(term),
            ..
        }) => term.to_string().starts_with(&format!("{op}(")),
        | _ => false,
    };
    if unevaluated {
        return Err(EvalError::new(
            "E0808",
            format!("`{op}` has no closed form the kernel can find for this range"),
        )
        .at(span)
        .with_note("give finite integer bounds (at most 10^4 terms are summed directly)"));
    }
    Ok(result)
}

/// `gamma(x)`: the Gamma function, not its logarithm (§8.2).
///
/// A positive integer is exactly `(n-1)!`, so `gamma(5)` is `24` and not
/// `3.17805` (which is `ln 24`).
fn gamma(
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let num = expect_num(args, 0, name, span)?;
    match &num.kind {
        | NumKind::Int(value) => {
            match value.to_i64() {
                | Some(n) if (1..=2000).contains(&n) => {
                    let result: BigInt = (2..n).map(BigInt::from).product();
                    Ok(Value::Num(NumVal::int(result)))
                },
                | Some(n) if n <= 0 => Err(domain_error(name, &num, span)),
                | _ => {
                    Ok(Value::Num(NumVal::float(
                        gamma_f64(num.to_f64().unwrap_or(f64::NAN)),
                        num.dim,
                    )))
                },
            }
        },
        | NumKind::Exact(value) if value.denom().is_one() => {
            let integer = NumVal::int(value.numer().clone());
            gamma(name, &[Value::Num(integer)], span)
        },
        | NumKind::Exact(_) | NumKind::Float(_) => {
            Ok(Value::Num(NumVal::float(
                gamma_f64(num.to_f64().unwrap_or(f64::NAN)),
                num.dim,
            )))
        },
        | NumKind::Sym(term) => {
            Ok(Value::Num(NumVal::symbolic(
                Term::fun(Fun::Other("gamma"), vec![(**term).clone()]),
                num.dim,
            )))
        },
        | NumKind::Complex(..) => Err(domain_error(name, &num, span)),
    }
}

fn gamma_f64(x: f64) -> f64 {
    if x < 0.5 {
        // Reflection, so a negative argument keeps its sign.
        std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * gamma_f64(1.0 - x))
    } else {
        lgamma_free(x).exp()
    }
}

fn lgamma_free(x: f64) -> f64 {
    // Lanczos approximation, enough for the examples and `gamma`.
    if x < 0.5 {
        return std::f64::consts::PI.ln()
            - (std::f64::consts::PI * x).sin().abs().ln()
            - lgamma_free(1.0 - x);
    }
    let coefficients = [
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    let x = x - 1.0;
    let mut a = 0.999_999_999_999_809_9;
    let t = x + 7.5;
    for (index, coefficient) in coefficients.iter().enumerate() {
        a += coefficient / (x + index as f64 + 1.0);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

fn erf(x: f64) -> f64 {
    // Abramowitz and Stegun 7.1.26.
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-x * x).exp();
    sign * y
}

/// Rejects shapes whose element count would exhaust memory or overflow.
fn checked_element_count(
    shape: &[usize],
    span: Span,
) -> Result<usize, EvalError> {
    crate::value::checked_element_count(shape)
        .filter(|&count| count <= 100_000_000)
        .ok_or_else(|| EvalError::new("E0802", "tensor shape is too large").at(span))
}

fn arity(
    name: &str,
    expected: usize,
    got: usize,
    span: Span,
) -> EvalError {
    EvalError::new(
        "E0315",
        format!("`{name}` takes {expected} argument(s), found {got}"),
    )
    .at(span)
}

fn ok(value: Value) -> Value {
    make_variant(adt::RESULT_ID, 0, "Ok", vec![value])
}

fn err(value: Value) -> Value {
    make_variant(adt::RESULT_ID, 1, "Err", vec![value])
}

fn expect_num(
    args: &[Value],
    index: usize,
    name: &str,
    span: Span,
) -> Result<NumVal, EvalError> {
    match args.get(index) {
        | Some(Value::Num(num)) => Ok(num.clone()),
        | Some(other) => {
            Err(EvalError::new(
                "E0302",
                format!("`{name}` expects a number, found {}", other.type_name()),
            )
            .at(span))
        },
        | None => Err(arity(name, index.saturating_add(1), args.len(), span)),
    }
}

fn expect_float(
    args: &[Value],
    index: usize,
    name: &str,
    span: Span,
) -> Result<f64, EvalError> {
    expect_num(args, index, name, span)?
        .to_f64()
        .ok_or_else(|| {
            EvalError::new("E0806", format!("`{name}` expects a concrete number")).at(span)
        })
}

/// Erases variant tags and declared Option element schemas from `witness`.
fn forget_variant_witnesses(witness: &mut aevia_rssn::witness::DimensionWitness) {
    use aevia_rssn::witness::DimensionWitness as W;
    match witness {
        | W::Variant { .. } | W::OptionSchema(_) => *witness = W::Unknown,
        | W::Tensor {
            element, elements, ..
        } => {
            if let Some(element) = element {
                forget_variant_witnesses(element);
            }
            elements.iter_mut().for_each(forget_variant_witnesses);
        },
        | W::Record(fields) => {
            for (_, field) in fields {
                forget_variant_witnesses(field);
            }
        },
        | W::Unknown | W::Scalar(_) => {},
    }
}

fn expect_int(
    args: &[Value],
    index: usize,
    name: &str,
    span: Span,
) -> Result<i64, EvalError> {
    let num = expect_num(args, index, name, span)?;
    match &num.kind {
        | NumKind::Int(value) => {
            value.as_i64().ok_or_else(|| {
                EvalError::new(
                    "E0802",
                    format!("`{name}` requires an Int in the signed 64-bit range"),
                )
                .at(span)
            })
        },
        | _ => {
            num.to_f64()
                .and_then(|value| value.to_i64())
                .ok_or_else(|| EvalError::new("E0302", format!("`{name}` expects an Int")).at(span))
        },
    }
}

fn expect_exact_int(
    args: &[Value],
    index: usize,
    name: &str,
    span: Span,
) -> Result<Int, EvalError> {
    let num = expect_num(args, index, name, span)?;
    match num.kind {
        | NumKind::Int(value) => Ok(value),
        | _ => Err(EvalError::new("E0302", format!("`{name}` expects an Int")).at(span)),
    }
}

fn expect_str(
    args: &[Value],
    index: usize,
    name: &str,
    span: Span,
) -> Result<String, EvalError> {
    match args.get(index) {
        | Some(Value::Str(text)) => Ok(text.to_string()),
        | _ => Err(arity(name, index.saturating_add(1), args.len(), span)),
    }
}

/// An axis argument, counted from the back when negative.
fn axis_argument(
    args: &[Value],
    index: usize,
    rank: usize,
    name: &str,
    span: Span,
) -> Result<usize, EvalError> {
    let axis = expect_exact_int(args, index, name, span)?;
    resolve_axis(&axis, rank, name, span)
}

fn resolve_axis(
    axis: &Int,
    rank: usize,
    name: &str,
    span: Span,
) -> Result<usize, EvalError> {
    let resolved = index_in_extent(axis, rank);
    resolved.ok_or_else(|| {
        EvalError::new(
            "E0802",
            format!("`{name}`: axis {axis} is out of range for rank {rank}"),
        )
        .at(span)
    })
}

fn index_in_extent(
    index: &Int,
    extent: usize,
) -> Option<usize> {
    let index = if index.is_negative() {
        index
            .abs()
            .to_usize()
            .and_then(|offset| extent.checked_sub(offset))
    } else {
        index.to_usize()
    };
    index.filter(|resolved| *resolved < extent)
}

/// A vector of axes (each as in [`axis_argument`]).
fn axes_argument(
    args: &[Value],
    index: usize,
    rank: usize,
    name: &str,
    span: Span,
) -> Result<Vec<usize>, EvalError> {
    let list = expect_tensor(args, index, name, span)?;
    let invalid = || {
        EvalError::new(
            "E0302",
            format!("`{name}` expects a vector of dimensionless Int axes"),
        )
        .at(span)
    };
    let [len] = list.shape.as_slice() else {
        return Err(invalid());
    };
    if *len != list.storage.len() {
        return Err(invalid());
    }
    (0..*len)
        .map(|index| {
            let axis = matrix::dimension_integer(&list, index).ok_or_else(invalid)?;
            resolve_axis(&axis, rank, name, span)
        })
        .collect()
}

/// The last two axes, for the `*2` transforms.
fn transform_axes_last_two(
    rank: usize,
    name: &str,
    span: Span,
) -> Result<Vec<usize>, EvalError> {
    let Some(first) = rank.checked_sub(2) else {
        return Err(EvalError::new(
            "E0802",
            format!("`{name}` needs a tensor of rank 2 or more, found rank {rank}"),
        )
        .at(span));
    };
    Ok((first..rank).collect())
}

/// A `Norm` argument.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "validates the language FFT normalization argument"
    )
)]
fn norm_argument(
    args: &[Value],
    index: usize,
    name: &str,
    span: Span,
) -> Result<aevia_std::fft::Norm, EvalError> {
    let Some(Value::Variant(variant)) = args.get(index) else {
        return Err(EvalError::new("E0302", format!("`{name}` expects a `Norm` value")).at(span));
    };
    Ok(match variant.name.as_str() {
        | "Forward" => aevia_std::fft::Norm::Forward,
        | "Unitary" => aevia_std::fft::Norm::Unitary,
        | _ => aevia_std::fft::Norm::Backward,
    })
}

fn expect_tensor(
    args: &[Value],
    index: usize,
    name: &str,
    span: Span,
) -> Result<Rc<TensorData>, EvalError> {
    match args.get(index) {
        | Some(Value::Tensor(tensor)) => Ok(tensor.clone()),
        | Some(other) => {
            Err(EvalError::new(
                "E0302",
                format!("`{name}` expects a tensor, found {}", other.type_name()),
            )
            .at(span))
        },
        | None => Err(arity(name, index.saturating_add(1), args.len(), span)),
    }
}

fn expect_unit(
    args: &[Value],
    index: usize,
    name: &str,
    span: Span,
) -> Result<Rc<UnitDef>, EvalError> {
    match args.get(index) {
        | Some(Value::UnitOf(unit)) => Ok(unit.clone()),
        | _ => Err(arity(name, index.saturating_add(1), args.len(), span)),
    }
}

/// A `Dyn<ρ>` argument (§3.11).
fn expect_dyn(
    args: &[Value],
    index: usize,
    name: &str,
    span: Span,
) -> Result<Rc<crate::value::DynValue>, EvalError> {
    match args.get(index) {
        | Some(Value::Dyn(dynamic)) => Ok(dynamic.clone()),
        | Some(other) => {
            Err(EvalError::new(
                "E0302",
                format!("`{name}` expects a Dyn, found {}", other.type_name()),
            )
            .at(span))
        },
        | None => Err(arity(name, index.saturating_add(1), args.len(), span)),
    }
}

/// The `DimValue` of a dimension (§4.9.2).
///
/// `DimValue` is opaque to the checker, so the runtime picks the
/// representation: a record holding lossless transport text of the dimension.
/// SI spellings remain readable; nominal bases encode their owner rather than
/// their current import alias. It round-trips through `Dim::parse_transport`.
/// Display text is never used as evidence for `dyn_with` or `check_with`.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps the runtime DimValue representation symmetric with its decoder"
    )
)]
fn dim_value(dim: &Dim) -> Value {
    record_of(vec![(
        "dimension",
        Value::string(dim.to_transport_string()),
    )])
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "validates the runtime DimValue representation"
    )
)]
fn as_dim_value(value: &Value) -> Option<Dim> {
    let Value::Record(record) = value else {
        return None;
    };
    let Some(Value::Str(text)) = record.fields.get("dimension") else {
        return None;
    };
    Dim::parse_transport(text).ok()
}

fn expect_dim_value(
    args: &[Value],
    index: usize,
    name: &str,
    span: Span,
) -> Result<Dim, EvalError> {
    args.get(index).and_then(as_dim_value).ok_or_else(|| {
        EvalError::new(
            "E0302",
            format!("`{name}` expects a DimValue (as `dim_of` returns)"),
        )
        .at(span)
    })
}

/// The `DimError` a failed `check`/`check_with` returns (§4.9.2).
///
/// This is the appendix-B `E0412` class — "`Dyn` runtime dimension check
/// failed" — travelling as the recoverable `Err` payload the signature
/// promises, not as a fatal diagnostic.
fn dim_error(
    expected: &Dim,
    found: &Dim,
) -> Value {
    let id = adt::intern(adt::Adt {
        name: "DimError".to_string(),
        params: Vec::new(),
        ctors: Vec::new(),
    });
    make_variant(
        id,
        0,
        "DimensionMismatch",
        vec![
            Value::string(expected.to_plain_string()),
            Value::string(found.to_plain_string()),
        ],
    )
}

/// The `ShapeError` a failed `reshape` returns (§8.4).
fn shape_error(message: String) -> Value {
    let id = adt::intern(adt::Adt {
        name: "ShapeError".to_string(),
        params: Vec::new(),
        ctors: Vec::new(),
    });
    make_variant(id, 0, "ElementCount", vec![Value::string(message)])
}

fn io_print(
    interp: &Interp,
    args: &[Value],
    newline: bool,
    stderr: bool,
    span: Span,
) -> Result<Value, EvalError> {
    let value = args.first().ok_or_else(|| arity("print", 1, 0, span))?;
    let text = show(&interp.system_display(value.clone()));
    Ok(Value::Io(Rc::new(crate::value::IoAction::Print {
        text,
        newline,
        stderr,
    })))
}

#[derive(Clone, Copy)]
enum MathKind {
    Root(u32),
    Float(fn(f64) -> f64),
}

/// The symbolic node for an elementary function.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "isolates the mapping from builtin spellings to symbolic operators"
    )
)]
fn fun_of(name: &str) -> Fun {
    match name {
        | "sqrt" => Fun::Sqrt,
        | "exp" => Fun::Exp,
        | "ln" | "log" => Fun::Ln,
        | "sin" => Fun::Sin,
        | "cos" => Fun::Cos,
        | "tan" => Fun::Tan,
        | "abs" => Fun::Abs,
        | other => {
            Fun::Other(match other {
                | "cbrt" => "cbrt",
                | "log2" => "log2",
                | "log10" => "log10",
                | "asin" => "asin",
                | "acos" => "acos",
                | "atan" => "atan",
                | "sinh" => "sinh",
                | "cosh" => "cosh",
                | "tanh" => "tanh",
                | "asinh" => "asinh",
                | "acosh" => "acosh",
                | "atanh" => "atanh",
                | "erf" => "erf",
                | "erfc" => "erfc",
                | "fresnel_s" => "fresnels",
                | "fresnel_c" => "fresnelc",
                | "gamma" => "gamma",
                | _ => "f",
            })
        },
    }
}

/// Applies an elementary function to a scalar (§6.3.1).
///
/// Three phases, and the phase of the argument decides the answer:
///
/// * a `Float` argument stays a `Float` — `sin(1.0)` is `0.8414709848078965`;
/// * a symbolic argument stays symbolic — `sin(1)` is `sin(1)`, not a float;
/// * an exact argument is folded **only when the result is exact** — `sqrt(4)`
///   is `2`, and `sqrt(2)` is `sqrt(2)`, because `1.4142135623730951` is a
///   different number and `sqrt(2)^2` would then be `2.0000000000000004`.
fn unary_math(
    interp: &mut Interp,
    name: &str,
    args: &[Value],
    span: Span,
    kind: MathKind,
) -> Result<Value, EvalError> {
    // §4.9.3: `sqrt : Dyn<ρ> -> Dyn<ρ>` divides the dimension exponent by 2.
    // The other elementary functions are not defined on `Dyn` — it must be
    // `check`ed back into a `Scalar` first (§4.9.4).
    if let Some(Value::Dyn(dynamic)) = args.first() {
        if name == "sqrt" {
            let root = unary_math(
                interp,
                name,
                &[Value::Num(dynamic.payload())],
                span,
                MathKind::Root(2),
            )?;
            let Value::Num(mut root) = root else {
                return Err(
                    EvalError::new("E0809", "sqrt of a Dyn payload is not a scalar").at(span),
                );
            };
            root.dim = Dim::one();
            let half = BigRational::new(BigInt::one(), BigInt::from(2));
            return Ok(Value::Dyn(Rc::new(DynValue {
                value: root,
                dim: dynamic.dim.pow(&half),
                display: None,
            })));
        }
        return Err(EvalError::new(
            "E0302",
            format!("`{name}` is not defined on Dyn; `check` it first (§4.9.4)"),
        )
        .at(span));
    }
    let num = expect_num(args, 0, name, span)?;
    // §6.3.4's extended-real boundary: `ln(0)` is `-oo`, `ln(oo)` is `oo`,
    // `exp(oo)` is `oo`, `exp(-oo)` is `0`. These are the spec's statements, not
    // a numeric approximation; a symbolic `ln(0)` left in the term tree would
    // print the unevaluated form the spec says it replaced.
    if let Some(special) = special_value(interp, name, &num, span)? {
        return Ok(special);
    }
    match kind {
        | MathKind::Root(degree) => unary_root(&num, degree, name, span),
        | MathKind::Float(function) => {
            let fun = fun_of(name);
            match &num.kind {
                | NumKind::Float(value) => {
                    let result = function(*value);
                    if result.is_nan() && !value.is_nan() {
                        return Err(domain_error(name, &num, span));
                    }
                    Ok(Value::Num(NumVal::float(result, num.dim)))
                },
                | NumKind::Complex(..) => Err(domain_error(name, &num, span)),
                | NumKind::Sym(term) => {
                    Ok(Value::Num(NumVal::symbolic(
                        Term::fun(fun, vec![(**term).clone()]),
                        num.dim,
                    )))
                },
                | NumKind::Int(_) | NumKind::Exact(_) => {
                    let rational = num
                        .to_rational()
                        .ok_or_else(|| domain_error(name, &num, span))?;
                    let term = Term::fun(fun, vec![Term::Num(rational)]);
                    match term {
                        | Term::Num(value) => Ok(Value::Num(NumVal::exact(value, num.dim))),
                        | other => Ok(Value::Num(NumVal::symbolic(other, num.dim))),
                    }
                },
            }
        },
    }
}

/// Evaluate a root while preserving the argument's scalar representation.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps exact, symbolic and IEEE root behavior together"
    )
)]
fn unary_root(
    num: &NumVal,
    degree: u32,
    name: &str,
    span: Span,
) -> Result<Value, EvalError> {
    if degree == 0 {
        return Err(domain_error(name, num, span));
    }
    let exponent = BigRational::new(BigInt::one(), BigInt::from(degree));
    let dim = num.dim.pow(&exponent);
    match &num.kind {
        | NumKind::Float(value) => {
            let result = match degree {
                | 2 => value.sqrt(),
                | 3 => value.cbrt(),
                | n if n % 2 == 1 => value.abs().powf(1.0 / f64::from(n)).copysign(*value),
                | n => value.powf(1.0 / f64::from(n)),
            };
            if result.is_nan() && !value.is_nan() {
                return Err(domain_error(name, num, span));
            }
            Ok(Value::Num(NumVal::float(result, dim)))
        },
        | NumKind::Complex(..) => Err(domain_error(name, num, span)),
        | NumKind::Sym(term) => {
            Ok(Value::Num(NumVal::symbolic(
                Term::pow((**term).clone(), exponent),
                dim,
            )))
        },
        | NumKind::Int(_) | NumKind::Exact(_) => {
            let rational = num
                .to_rational()
                .ok_or_else(|| domain_error(name, num, span))?;
            if let Some(root) = exact_root(&rational, degree) {
                return Ok(Value::Num(NumVal::exact(root, dim)));
            }
            Ok(Value::Num(NumVal::symbolic(
                Term::pow(Term::Num(rational), exponent),
                dim,
            )))
        },
    }
}

fn domain_error(
    name: &str,
    num: &NumVal,
    span: Span,
) -> EvalError {
    EvalError::new(
        "E0806",
        format!(
            "`{name}` is not defined for {}",
            show(&Value::Num(num.clone()))
        ),
    )
    .at(span)
}

/// §6.3.4's extended-real boundary values: `ln(0) = -oo`, `ln(oo) = oo`,
/// `exp(oo) = oo`, `exp(-oo) = 0`.
///
/// These are spec statements, not approximations: leaving `ln(0)` as a symbolic
/// term would print the unevaluated form the spec says it replaced. `ln(-oo)`
/// stays symbolic because the spec only defines the logarithm on nonnegative
/// reals (§6.3.4), and `nan` never takes a special value.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps the extended-real cases separate from ordinary scalar arithmetic"
    )
)]
fn special_value(
    interp: &mut Interp,
    name: &str,
    num: &NumVal,
    span: Span,
) -> Result<Option<Value>, EvalError> {
    match name {
        | "ln" | "log" => {
            if infinity_sign(num) == Some(1) {
                return Ok(Some(Value::Num(num.clone())));
            }
            if matches!(&num.kind, NumKind::Int(value) if value.is_zero())
                || matches!(&num.kind, NumKind::Exact(value) if value.is_zero())
            {
                return Ok(Some(negative_infinity(interp, &num.dim, span)?));
            }
        },
        | "exp" => {
            match infinity_sign(num) {
                | Some(1) => return Ok(Some(Value::Num(num.clone()))),
                | Some(-1) => {
                    let zero = if matches!(num.kind, NumKind::Float(_)) {
                        NumVal::float(0.0, num.dim.clone())
                    } else {
                        NumVal::exact(BigRational::zero(), num.dim.clone())
                    };
                    return Ok(Some(Value::Num(zero)));
                },
                | _ => {},
            }
        },
        | _ => {},
    }
    Ok(None)
}

/// `-1`/`+1` when `num` is `-oo`/`oo`, in either the `Real` symbolic form or
/// the `Float` IEEE form.
fn infinity_sign(num: &NumVal) -> Option<i8> {
    match &num.kind {
        | NumKind::Float(value) if value.is_infinite() => Some(if *value > 0.0 { 1 } else { -1 }),
        | NumKind::Sym(term) => infinity_term_sign(term),
        | _ => None,
    }
}

fn infinity_term_sign(term: &Term) -> Option<i8> {
    match term {
        | Term::Sym(name) if name == "oo" => Some(1),
        | Term::Foreign { text, .. } if text.as_ref() == "oo" => Some(1),
        | Term::Foreign { text, .. } if text.as_ref() == "-oo" => Some(-1),
        | Term::Mul(items) => {
            match items.as_slice() {
                | [Term::Num(coefficient), rest]
                    if coefficient == &BigRational::from_integer(BigInt::from(-1))
                        && infinity_term_sign(rest) == Some(1) =>
                {
                    Some(-1)
                },
                | _ => None,
            }
        },
        | _ => None,
    }
}

/// The `Real` value `-oo` in the same dimension as `dim`.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "constructs the global infinity witness with the requested physical dimension"
    )
)]
fn negative_infinity(
    interp: &mut Interp,
    dim: &Dim,
    span: Span,
) -> Result<Value, EvalError> {
    let infinity = interp.lookup_global("oo", None, span)?;
    let Value::Num(number) = infinity else {
        return Err(EvalError::new("E0809", "`oo` is not a scalar").at(span));
    };
    if let NumKind::Float(value) = number.kind {
        return Ok(Value::Num(NumVal::float(-value, dim.clone())));
    }
    let term = value_to_term(&Value::Num(number))?;
    Ok(Value::Num(NumVal::symbolic(Term::neg(term), dim.clone())))
}

/// Return a rational root only after verifying both integer powers.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "proves exact roots without depending on runtime scalar dispatch"
    )
)]
fn exact_root(
    value: &BigRational,
    degree: u32,
) -> Option<BigRational> {
    let is_negative = value.is_negative();
    if degree == 0 || value.denom().is_zero() || (is_negative && degree.is_multiple_of(2)) {
        return None;
    }
    let num = value.numer().magnitude();
    let den = value.denom().magnitude();
    let numerator = num.nth_root(degree);
    let denominator = den.nth_root(degree);
    if numerator.pow(degree) != *num || denominator.pow(degree) != *den {
        return None;
    }
    let sign = if is_negative {
        num_bigint::Sign::Minus
    } else {
        num_bigint::Sign::Plus
    };
    Some(BigRational::new(
        BigInt::from_biguint(sign, numerator),
        BigInt::from(denominator),
    ))
}

#[cfg(test)]
mod elementary_input_tests {
    use super::BigInt;
    use super::BigRational;
    use super::Dim;
    use super::Interp;
    use super::NumKind;
    use super::Span;
    use super::Value;
    use super::exact_root;
    use super::special_value;

    #[test]
    fn approximate_comparison_handles_nonfinite_values() {
        let mut interp = Interp::new(&aevia_types::CheckedModule::default());
        for (left, right, tolerance, expected) in [
            (f64::NAN, 1.0, 0.01, false),
            (1.0, f64::INFINITY, 0.01, false),
            (f64::NEG_INFINITY, f64::NEG_INFINITY, 0.0, true),
            (0.0, 0.0, f64::NAN, false),
            (1.0, 1.125, 0.125, true),
        ] {
            let args = [left, right, tolerance].map(Value::float).to_vec();
            let result = super::call(&mut interp, "approx_eq", args, Span::default()).unwrap();
            assert!(matches!(result, Value::Bool(actual) if actual == expected));
        }
    }

    #[test]
    fn dense_sum_rejects_a_negative_axis() {
        let mut interp = Interp::new(&aevia_types::CheckedModule::default());
        let tensor = super::build_tensor(vec![Value::int(4)], vec![1], Dim::one());
        assert!(
            super::call(
                &mut interp,
                "sum",
                vec![tensor, Value::int(-1)],
                Span::default()
            )
            .is_err()
        );
    }

    #[test]
    fn very_wide_integer_summation_reaches_the_symbolic_path() {
        let mut interp = Interp::new(&aevia_types::CheckedModule::default());
        let function = Value::Native(super::Rc::new(super::NativeFn(Box::new(|_, _, _| {
            Err(super::EvalError::new(
                "E0999",
                "symbolic argument reached the callback",
            ))
        }))));
        let args = [function, Value::int(i64::MIN), Value::int(i64::MAX)];
        let error = super::sum_range(&mut interp, &args, Span::default()).unwrap_err();
        assert_eq!(error.code, "E0999");
    }

    #[test]
    fn rational_roots_are_proven_beyond_float_precision() {
        let root = BigInt::from(1_u128 << 100);
        let square = root.pow(2);
        assert_eq!(
            exact_root(&BigRational::from_integer(square.clone()), 2),
            Some(BigRational::from_integer(root))
        );
        assert_eq!(
            exact_root(&BigRational::from_integer(square + BigInt::from(1)), 2),
            None
        );
        assert_eq!(
            exact_root(&BigRational::new((-8).into(), 27.into()), 3),
            Some(BigRational::new((-2).into(), 3.into()))
        );
        assert_eq!(exact_root(&BigRational::from_integer((-4).into()), 2), None);
    }

    #[test]
    fn exp_of_negative_float_infinity_retains_the_float_phase() {
        let mut interp = Interp::new(&aevia_types::CheckedModule::default());
        let value = super::NumVal::float(f64::NEG_INFINITY, Dim::one());
        let actual = special_value(&mut interp, "exp", &value, Span::default());
        assert!(
            matches!(actual, Ok(Some(Value::Num(num))) if matches!(num.kind, NumKind::Float(v) if v.to_bits() == 0))
        );
    }
    #[test]
    fn gif_channels_and_loop_count_must_be_integers() {
        fn gif(
            channel: Value,
            loop_count: Value,
        ) -> Value {
            let rgba = super::build_tensor(
                vec![channel, Value::int(0), Value::int(0), Value::int(255)],
                vec![4],
                Dim::one(),
            );
            let frame = super::record_of(vec![("delay_cs", Value::int(1)), ("rgba", rgba)]);
            super::record_of(vec![
                ("width", Value::int(1)),
                ("height", Value::int(1)),
                ("loop_count", loop_count),
                (
                    "frames",
                    super::build_tensor(vec![frame], vec![1], Dim::one()),
                ),
            ])
        }
        assert!(super::gif_animation_from_value(&gif(Value::int(9), Value::int(0))).is_ok());
        assert!(super::gif_animation_from_value(&gif(Value::float(9.5), Value::int(0))).is_err());
        assert!(
            super::gif_animation_from_value(&gif(Value::int(9), Value::float(f64::NAN))).is_err()
        );
    }

    #[test]
    fn event_direction_must_be_an_exact_choice() {
        let denominator = BigInt::from(1_u64 << 60);
        let almost_one = BigRational::new(denominator.clone() + BigInt::from(1), denominator);
        let event = super::record_of(vec![
            ("g", Value::Unit),
            ("terminal", Value::Bool(false)),
            (
                "direction",
                Value::Num(super::NumVal::exact(almost_one, Dim::one())),
            ),
        ]);
        let events = super::build_tensor(vec![event], vec![1], Dim::one());
        let actual = super::parse_events(Some(&events), "ode_solve", Span::default());
        assert!(matches!(actual, Err(error) if error.code == "E0802"));
    }
}

fn map_scalar<F>(
    args: &[Value],
    name: &str,
    span: Span,
    function: F,
) -> Result<Value, EvalError>
where
    F: Fn(NumVal) -> NumVal,
{
    let num = expect_num(args, 0, name, span)?;
    Ok(Value::Num(function(num)))
}

/// The additive identity of a tensor's element phase (§6.6.6).
///
/// An empty `f64` buffer is a `Float` tensor, so its empty sum is `0.0`; every
/// other storage class is exact, so its empty sum is the exact `0`.
fn zero_of(tensor: &TensorData) -> NumVal {
    if tensor.dense_flat().is_some() {
        NumVal::float(0.0, tensor.dim.clone())
    } else {
        NumVal::exact(BigRational::zero(), tensor.dim.clone())
    }
}

/// The multiplicative identity of a tensor's element phase (§6.6.6).
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps the empty product identity and its representation policy together"
    )
)]
fn one_of(tensor: &TensorData) -> NumVal {
    if tensor.dense_flat().is_some() {
        NumVal::float(1.0, tensor.dim.clone())
    } else {
        NumVal::int(BigInt::one())
    }
}

/// A rational as the narrowest runtime scalar: `Int` when it is integral.
fn exact_number(
    value: BigRational,
    dim: Dim,
) -> Value {
    if value.denom().is_one() {
        Value::Num(NumVal {
            kind: NumKind::Int(value.numer().into()),
            dim,
            display: None,
        })
    } else {
        Value::Num(NumVal::exact(value, dim))
    }
}

pub(crate) fn value_to_term(value: &Value) -> Result<Term, EvalError> {
    match value {
        | Value::Num(num) => {
            match &num.kind {
                | NumKind::Int(value) => Ok(Term::Num(value.to_rational())),
                | NumKind::Exact(value) => Ok(Term::Num(value.as_ref().clone())),
                | NumKind::Float(value) => {
                    Ok(Term::Num(
                        BigRational::from_float(*value).unwrap_or_else(BigRational::zero),
                    ))
                },
                | NumKind::Sym(term) => Ok((**term).clone()),
                | NumKind::Complex(..) => {
                    Err(EvalError::new(
                        "E0302",
                        "complex symbolic values are not supported",
                    ))
                },
            }
        },
        | other => {
            Err(EvalError::new(
                "E0302",
                format!("expected a scalar, found {}", other.type_name()),
            ))
        },
    }
}

fn symbol_name(num: &NumVal) -> Option<String> {
    match &num.kind {
        | NumKind::Sym(term) => {
            match &**term {
                | Term::Sym(name) => Some(name.clone()),
                | _ => None,
            }
        },
        | _ => None,
    }
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "validates the GIF record at the language and encoder boundary"
    )
)]
fn gif_animation_from_value(value: &Value) -> Result<rssn::io::GifAnimation, String> {
    let Value::Record(record) = value else {
        return Err(format!(
            "E0902: `write_gif` needs a GIF record, found {}",
            value.type_name()
        ));
    };
    let gif_integer = |value: &Value| {
        value.as_num().filter(|number| number.dim.is_one())?;
        nonnegative_integer(value)
    };
    let read_u16 = |record: &RecordValue, key: &str| -> Result<u16, String> {
        let Some(field) = record.fields.get(key) else {
            return Err(format!("E0902: GIF record is missing `{key}`"));
        };
        let raw = gif_integer(field)
            .ok_or_else(|| format!("E0902: GIF `{key}` must be a nonnegative integer"))?;
        u16::try_from(raw).map_err(|_| format!("E0902: GIF `{key}` out of u16 range: {raw}"))
    };
    let width = read_u16(record, "width")?;
    let height = read_u16(record, "height")?;
    let loop_count = read_u16(record, "loop_count")?;
    let expected_bytes = usize::from(width)
        .checked_mul(usize::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|count| *count > 0)
        .ok_or_else(|| {
            "E0902: GIF dimensions must be positive and fit the RGBA buffer".to_string()
        })?;
    let Some(Value::Tensor(frame_list)) = record.fields.get("frames") else {
        return Err("E0902: GIF record is missing `frames` list".to_string());
    };
    let mut frames = Vec::new();
    let frame_count = frame_list.storage.len();
    if frame_list.rank() != 1 || frame_list.checked_element_count() != Some(frame_count) {
        return Err("E0902: GIF `frames` must be a valid rank-one tensor".to_string());
    }
    frames
        .try_reserve_exact(frame_count)
        .map_err(|_| "E0902: GIF `frames` is too large".to_string())?;
    for index in 0..frame_count {
        let Some(Value::Record(frame_rec)) = frame_list.get_flat(index) else {
            return Err("E0902: each GIF frame must be a record".to_string());
        };
        let delay_cs = read_u16(&frame_rec, "delay_cs")?;
        let Some(Value::Tensor(rgba_list)) = frame_rec.fields.get("rgba") else {
            return Err("E0902: GIF frame is missing `rgba` list".to_string());
        };
        let rgba_count = rgba_list.storage.len();
        if rgba_list.rank() != 1 || rgba_list.checked_element_count() != Some(rgba_count) {
            return Err("E0902: GIF `rgba` must be a valid rank-one tensor".to_string());
        }
        if rgba_count != expected_bytes {
            return Err(format!(
                "E0902: GIF frame needs {expected_bytes} RGBA bytes, found {rgba_count}"
            ));
        }
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(rgba_count)
            .map_err(|_| "E0902: GIF `rgba` is too large".to_string())?;
        for index in 0..rgba_count {
            let raw = rgba_list
                .get_flat(index)
                .as_ref()
                .and_then(gif_integer)
                .ok_or_else(|| {
                    "E0902: GIF `rgba` elements must be nonnegative integers".to_string()
                })?;
            let byte = u8::try_from(raw)
                .map_err(|_| format!("E0902: GIF `rgba` byte out of range: {raw}"))?;
            rgba.push(byte);
        }
        frames.push(rssn::io::GifFrame {
            width,
            height,
            left: 0,
            top: 0,
            delay_cs,
            dispose: rssn::io::GifDisposal::Any,
            rgba,
        });
    }
    Ok(rssn::io::GifAnimation {
        width,
        height,
        loop_count,
        comment: None,
        frames,
    })
}

/// A tensor's elements as complex numbers.
///
/// # Errors
///
/// `E0302` when an element is not a number.
fn complex_values(tensor: &TensorData) -> Result<Vec<aevia_std::Complex>, EvalError> {
    if let Some(data) = tensor.dense_flat() {
        return Ok(data
            .iter()
            .map(|&sample| aevia_std::Complex::new(sample, 0.0))
            .collect());
    }
    let mut out = Vec::with_capacity(tensor.storage.len());
    for element in tensor.elements() {
        let Some(num) = element.as_num() else {
            return Err(EvalError::new(
                "E0302",
                format!(
                    "expected a numeric tensor, found an element of type {}",
                    element.type_name()
                ),
            ));
        };
        out.push(match &num.kind {
            | NumKind::Complex(re, im) => aevia_std::Complex::new(*re, *im),
            | _ => {
                match num.to_f64() {
                    | Some(value) => aevia_std::Complex::new(value, 0.0),
                    | None => {
                        return Err(EvalError::new(
                            "E0806",
                            "expected a concrete element in the transform",
                        ));
                    },
                }
            },
        });
    }
    Ok(out)
}

/// The shape of an ODE state, so that a flat `Vec<f64>` can be rebuilt (§8.9).
#[derive(Clone, Debug, PartialEq)]
enum StateShape {
    /// A scalar with its dimension.
    Scalar(Dim),
    /// A tensor with its shape and element dimension.
    Tensor(Vec<usize>, Dim),
    /// A record, in field order.
    Record(Vec<(String, Self)>),
}

/// The flat rate vector an ODE right-hand side returned (§8.9).
///
/// The right-hand side must answer one rate per state component, flattened by
/// the same contract as the state itself: fields are matched by name in state
/// order, and tensors are flattened in row-major order. Each leaf must have the
/// state's shape and dimension divided by the time dimension.
fn ode_rates(
    value: &Value,
    shape: &StateShape,
    time_dim: &Dim,
    expected: usize,
    span: Span,
    failure: &mut Option<EvalError>,
) -> Vec<f64> {
    fn collect_rate(
        value: &Value,
        shape: &StateShape,
        time_dim: &Dim,
        span: Span,
        out: &mut Vec<f64>,
    ) -> Result<(), EvalError> {
        let shape_error = || {
            EvalError::new(
                "E0302",
                "the ODE right-hand side must match the state structure and tensor shapes (§8.9)",
            )
            .at(span)
        };
        let check_dim = |actual: &Dim, state: &Dim| {
            if *actual == state.div(time_dim) {
                Ok(())
            } else {
                Err(EvalError::new("E0401", "an ODE rate must have its state component's dimension divided by the time dimension (§8.9)").at(span))
            }
        };
        match (shape, value) {
            | (StateShape::Scalar(dim), Value::Num(num)) => {
                check_dim(&num.dim, dim)?;
                out.push(num.to_f64().ok_or_else(shape_error)?);
            },
            | (StateShape::Tensor(expected, dim), Value::Tensor(tensor))
                if tensor.shape == *expected =>
            {
                check_dim(&tensor.dim, dim)?;
                out.extend(tensor_values(tensor)?);
            },
            | (StateShape::Record(fields), Value::Record(record))
                if fields.len() == record.fields.len() =>
            {
                for (name, field_shape) in fields {
                    let field = record.fields.get(name).ok_or_else(shape_error)?;
                    collect_rate(field, field_shape, time_dim, span, out)?;
                }
            },
            | _ => return Err(shape_error()),
        }
        Ok(())
    }

    let mut flat = Vec::with_capacity(expected);
    let result = collect_rate(value, shape, time_dim, span, &mut flat);
    match result {
        | Ok(()) => flat,
        | Err(error) => {
            if failure.is_none() {
                let error = match flatten_state(value) {
                    | Ok((_, values)) if values.len() != expected => EvalError::new("E0302", format!("the ODE right-hand side returned {} rate(s) for a state of {} component(s) (§8.9)", values.len(), expected)).at(span),
                    | _ => error,
                };
                *failure = Some(error);
            }
            vec![0.0; expected]
        },
    }
}

/// Physical absolute tolerances define a separate reference scale per state
/// leaf. The native error test then becomes `atol_i + rtol * abs(y_i)`, without
/// combining unlike physical units in one scalar absolute tolerance.
fn ode_component_scales(
    args: &[Value],
    index: usize,
    initial: &Value,
    options: &mut aevia_std::ode::Options,
    span: Span,
) -> Result<Vec<f64>, EvalError> {
    let (shape, flat) = flatten_state(initial)?;
    let Some(Value::Record(record)) = args.get(index) else {
        return Ok(vec![1.0; flat.len()]);
    };
    let Some(atol) = record.fields.get("atol") else {
        return Ok(vec![1.0; flat.len()]);
    };
    // Legacy dimensionless scalar atol applies to canonical component values.
    if atol.as_f64().is_some() && crate::value::value_dim(atol).is_one() {
        return Ok(vec![1.0; flat.len()]);
    }
    let mut failure = None;
    let scales = ode_rates(atol, &shape, &Dim::one(), flat.len(), span, &mut failure);
    if let Some(error) = failure {
        return Err(EvalError::new(
            error.code,
            "ODE `atol` must be a dimensionless scalar or match every state leaf's shape and physical dimension",
        ).at(span));
    }
    if scales.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err(EvalError::new(
            "E0802",
            "every physical ODE absolute tolerance must be finite and positive",
        )
        .at(span));
    }
    options.atol = 1.0;
    Ok(scales)
}

/// Restore physical values before crossing the callback or result boundary.
fn ode_restore(
    state: &[f64],
    scales: &[f64],
) -> Vec<f64> {
    state
        .iter()
        .zip(scales)
        .map(|(v, scale)| v * scale)
        .collect()
}

/// Normalize physical states/rates with the same per-component reference units.
fn ode_normalize(
    state: &[f64],
    scales: &[f64],
) -> Vec<f64> {
    state
        .iter()
        .zip(scales)
        .map(|(v, scale)| v / scale)
        .collect()
}

fn run_ode(
    interp: &mut Interp,
    function: &Value,
    initial: &Value,
    grid: &[f64],
    time_dim: &Dim,
    scales: &[f64],
    options: aevia_std::ode::Options,
    event: Option<Value>,
    name: &str,
    span: Span,
) -> Result<(Vec<f64>, Vec<Vec<f64>>), EvalError> {
    let (shape, flat) = flatten_state(initial)?;
    let flat = ode_normalize(&flat, scales);
    let failure = std::cell::RefCell::new(None);
    let interp = std::cell::RefCell::new(interp);
    let mut rhs = |t: f64, state: &[f64]| {
        if failure.borrow().is_some() {
            return vec![f64::NAN; state.len()];
        }
        let args = vec![
            Value::Num(NumVal::float(t, time_dim.clone())),
            rebuild_state(&shape, &ode_restore(state, scales)),
        ];
        let rate = match interp.borrow_mut().apply(function.clone(), args, span) {
            | Ok(value) => {
                ode_rates(
                    &value,
                    &shape,
                    time_dim,
                    state.len(),
                    span,
                    &mut failure.borrow_mut(),
                )
            },
            | Err(error) => {
                if failure.borrow().is_none() {
                    *failure.borrow_mut() = Some(error);
                }
                vec![0.0; state.len()]
            },
        };
        ode_normalize(&rate, scales)
    };
    let solution = if let Some(event) = event {
        let mut event_fn = |t: f64, state: &[f64]| {
            let args = vec![
                Value::Num(NumVal::float(t, time_dim.clone())),
                rebuild_state(&shape, &ode_restore(state, scales)),
            ];
            match interp.borrow_mut().apply(event.clone(), args, span) {
                | Ok(value) => {
                    match value.as_f64() {
                        | Some(number) if crate::value::value_dim(&value) == Dim::one() => number,
                        | _ => {
                            if failure.borrow().is_none() {
                                *failure.borrow_mut() = Some(
                                    EvalError::new(
                                        "E0302",
                                        "the ODE event must return a dimensionless number",
                                    )
                                    .at(span),
                                );
                            }
                            f64::NAN
                        },
                    }
                },
                | Err(error) => {
                    if failure.borrow().is_none() {
                        *failure.borrow_mut() = Some(error);
                    }
                    f64::NAN
                },
            }
        };
        aevia_std::ode::integrate_until_event(&mut rhs, &flat, grid, options, &mut event_fn)
    } else {
        aevia_std::ode::integrate(&mut rhs, &flat, grid, options)
            .map(|states| (grid.to_vec(), states))
    };
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    solution
        .map(|(times, states)| {
            (
                times,
                states.iter().map(|v| ode_restore(v, scales)).collect(),
            )
        })
        .map_err(|error| numeric_error(name, error, span))
}

// ----- §8.9 solutions: dense output, events, higher order, BVP -------------

/// One `OdeEvent` / `SdeEvent` record: `{ g, terminal, direction }`.
#[derive(Clone)]
struct ParsedEvent {
    g: Value,
    terminal: bool,
    direction: i8,
}

/// Reads the event list of `ode_solve` / `sde_events`.
fn parse_events(
    value: Option<&Value>,
    name: &str,
    span: Span,
) -> Result<Vec<ParsedEvent>, EvalError> {
    let Some(Value::Tensor(list)) = value else {
        return Err(EvalError::new(
            "E0302",
            format!("`{name}` expects a tensor of event records `{{ g, terminal, direction }}`"),
        )
        .at(span));
    };
    if list.rank() != 1 || list.checked_element_count() != Some(list.storage.len()) {
        return Err(EvalError::new(
            "E0302",
            format!("`{name}` expects a valid rank-one event tensor"),
        )
        .at(span));
    }
    let mut events = Vec::new();
    for index in 0..list.storage.len() {
        let Some(Value::Record(record)) = list.get_flat(index) else {
            return Err(EvalError::new(
                "E0302",
                format!("`{name}`: every event is a record `{{ g, terminal, direction }}`"),
            )
            .at(span));
        };
        let g = record.fields.get("g").cloned().ok_or_else(|| {
            EvalError::new("E0302", format!("`{name}`: an event needs a function `g`")).at(span)
        })?;
        let terminal = match record.fields.get("terminal") {
            | Some(Value::Bool(flag)) => *flag,
            | _ => {
                return Err(EvalError::new(
                    "E0302",
                    format!("`{name}`: an event needs a `Bool` field `terminal`"),
                )
                .at(span));
            },
        };
        let direction = record
            .fields
            .get("direction")
            .and_then(Value::as_num)
            .filter(|num| num.dim.is_one())
            .and_then(NumVal::to_rational)
            .filter(BigRational::is_integer)
            .and_then(|value| value.numer().to_i8())
            .filter(|direction| (-1..=1).contains(direction));
        let Some(direction) = direction else {
            return Err(EvalError::new(
                "E0802",
                format!("`{name}`: an event's `direction` is -1 (falling), 0 or 1 (rising)"),
            )
            .at(span));
        };
        events.push(ParsedEvent {
            g,
            terminal,
            direction,
        });
    }
    Ok(events)
}

/// Applies an event function `g(t, y)`; the answer must be a dimensionless
/// number. A failure is recorded once and answered with `NaN`.
fn event_value(
    interp: &mut Interp,
    event: &Value,
    shape: &StateShape,
    time_dim: &Dim,
    t: f64,
    state: &[f64],
    failure: &mut Option<EvalError>,
    span: Span,
) -> f64 {
    let args = vec![
        Value::Num(NumVal::float(t, time_dim.clone())),
        rebuild_state(shape, state),
    ];
    match interp.apply(event.clone(), args, span) {
        | Ok(value) => {
            match value.as_f64() {
                | Some(number) if crate::value::value_dim(&value) == Dim::one() => number,
                | _ => {
                    if failure.is_none() {
                        *failure = Some(
                            EvalError::new("E0302", "an event must return a dimensionless number")
                                .at(span),
                        );
                    }
                    f64::NAN
                },
            }
        },
        | Err(error) => {
            if failure.is_none() {
                *failure = Some(error);
            }
            f64::NAN
        },
    }
}

/// The `(t0, t1)` interval of `solve_ivp`-style calls.
fn ode_interval(
    value: Option<&Value>,
    name: &str,
    span: Span,
) -> Result<(f64, f64, Dim), EvalError> {
    let Some(Value::Record(interval)) = value else {
        return Err(
            EvalError::new("E0302", format!("`{name}` expects a `(t0, t1)` pair")).at(span),
        );
    };
    let start = interval.fields.get("0").and_then(Value::as_f64);
    let end = interval.fields.get("1").and_then(Value::as_f64);
    let (Some(start), Some(end)) = (start, end) else {
        return Err(
            EvalError::new("E0302", format!("`{name}` expects a `(t0, t1)` pair")).at(span),
        );
    };
    let time_dim = interval
        .fields
        .get("0")
        .map_or_else(Dim::one, crate::value::value_dim);
    Ok((start, end, time_dim))
}

/// The record `ode_solve` and `ode_bvp` answer with: the accepted nodes, the
/// callable continuous extension `at`, the located events and statistics.
fn ode_solution_value(
    solution: aevia_std::ode::Solution,
    shape: &StateShape,
    scales: &[f64],
    time_dim: &Dim,
    state_dim: Dim,
) -> Value {
    let solution = Rc::new(solution);
    let len = solution.t.len();
    let states: Vec<Value> = solution
        .y
        .iter()
        .map(|state| rebuild_state(shape, &ode_restore(state, scales)))
        .collect();
    let hits: Vec<Value> = solution
        .hits
        .iter()
        .map(|hit| {
            record_of(vec![
                ("index", Value::int(i64::try_from(hit.index).unwrap_or(0))),
                ("t", Value::Num(NumVal::float(hit.t, time_dim.clone()))),
                ("y", rebuild_state(shape, &ode_restore(&hit.y, scales))),
            ])
        })
        .collect();
    let hit_count = hits.len();
    let at = {
        let solution = Rc::clone(&solution);
        let shape = shape.clone();
        let scales = scales.to_vec();
        let time_dim = time_dim.clone();
        Value::Native(Rc::new(NativeFn(Box::new(move |_interp, args, span| {
            let Some(Value::Num(time)) = args.first() else {
                return Err(EvalError::new("E0302", "the solution's `at` takes a time").at(span));
            };
            if time.dim != time_dim {
                return Err(EvalError::new(
                    "E0401",
                    "the time given to `at` must have the solution's time dimension",
                )
                .at(span));
            }
            let t = time.to_f64().unwrap_or(f64::NAN);
            match solution.eval(t) {
                | Some(state) => Ok(rebuild_state(&shape, &ode_restore(&state, &scales))),
                | None => {
                    let (lo, hi) = solution.span();
                    Err(EvalError::new(
                        "E0802",
                        format!("`at({t})` lies outside the solution's span [{lo}, {hi}]"),
                    )
                    .at(span))
                },
            }
        }))))
    };
    record_of(vec![
        (
            "t",
            float_tensor(vec![len], solution.t.clone(), time_dim.clone()),
        ),
        ("y", build_tensor(states, vec![len], state_dim)),
        ("at", at),
        ("events", build_tensor(hits, vec![hit_count], Dim::one())),
        (
            "nfev",
            Value::int(i64::try_from(solution.nfev).unwrap_or(0)),
        ),
        (
            "rejected",
            Value::int(i64::try_from(solution.rejected).unwrap_or(0)),
        ),
        (
            "method",
            Value::string(match solution.method {
                | aevia_std::ode::Method::Rk4 => "Rk4",
                | aevia_std::ode::Method::Dopri5 => "Dopri5",
                | aevia_std::ode::Method::Radau => "Radau",
                | aevia_std::ode::Method::Bdf => "Bdf",
                | aevia_std::ode::Method::Rosenbrock23 => "Rosenbrock23",
                | aevia_std::ode::Method::Auto => "AutoSwitch",
                | aevia_std::ode::Method::StormerVerlet => "StormerVerlet",
                | aevia_std::ode::Method::Ruth4 => "Ruth4",
                | aevia_std::ode::Method::GaussLegendre4 => "GaussLegendre4",
                | aevia_std::ode::Method::GaussLegendre6 => "GaussLegendre6",
                | aevia_std::ode::Method::ImplicitMidpoint => "ImplicitMidpoint",
                | aevia_std::ode::Method::AdaptiveRk4 => "AdaptiveRk4",
                | aevia_std::ode::Method::AdaptiveStormerVerlet => "AdaptiveStormerVerlet",
                | aevia_std::ode::Method::AdaptiveRuth4 => "AdaptiveRuth4",
                | aevia_std::ode::Method::AdaptiveImplicitMidpoint => "AdaptiveImplicitMidpoint",
                | aevia_std::ode::Method::AdaptiveGaussLegendre4 => "AdaptiveGaussLegendre4",
                | aevia_std::ode::Method::AdaptiveGaussLegendre6 => "AdaptiveGaussLegendre6",
            }),
        ),
    ])
}

/// One right-hand-side evaluation, recording the first failure.
fn ode_rate_value(
    interp: &mut Interp,
    function: &Value,
    shape: &StateShape,
    time_dim: &Dim,
    t: f64,
    state: &[f64],
    failure: &mut Option<EvalError>,
    span: Span,
) -> Vec<f64> {
    if failure.is_some() {
        return vec![f64::NAN; state.len()];
    }
    let args = vec![
        Value::Num(NumVal::float(t, time_dim.clone())),
        rebuild_state(shape, state),
    ];
    match interp.apply(function.clone(), args, span) {
        | Ok(value) => ode_rates(&value, shape, time_dim, state.len(), span, failure),
        | Err(error) => {
            if failure.is_none() {
                *failure = Some(error);
            }
            vec![0.0; state.len()]
        },
    }
}

/// `ode_solve(rhs, y0, (t0, t1), opts, events)` (§8.9).
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps dense solution construction and event handling outside the dispatch match"
    )
)]
fn ode_solve_call(
    interp: &mut Interp,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let name = "ode_solve";
    let [function, initial, interval, _, event_values] = args else {
        return Err(arity(name, 5, args.len(), span));
    };
    let (start, end, time_dim) = ode_interval(Some(interval), name, span)?;
    let mut options = ode_options(args, 3, span)?;
    let scales = ode_component_scales(args, 3, initial, &mut options, span)?;
    let events = parse_events(Some(event_values), name, span)?;
    let (shape, flat) = flatten_state(initial)?;
    let flat = ode_normalize(&flat, &scales);
    let failure: std::cell::RefCell<Option<EvalError>> = std::cell::RefCell::new(None);
    let outcome = {
        let cell = std::cell::RefCell::new(&mut *interp);
        let mut rhs = |t: f64, state: &[f64]| {
            let physical = ode_restore(state, &scales);
            let rate = ode_rate_value(
                &mut cell.borrow_mut(),
                function,
                &shape,
                &time_dim,
                t,
                &physical,
                &mut failure.borrow_mut(),
                span,
            );
            ode_normalize(&rate, &scales)
        };
        let specs: Vec<aevia_std::ode::EventSpec> = events
            .iter()
            .map(|event| {
                aevia_std::ode::EventSpec {
                    terminal: event.terminal,
                    direction: event.direction,
                }
            })
            .collect();
        let mut watch = |index: usize, t: f64, state: &[f64]| {
            let Some(event) = events.get(index) else {
                failure.borrow_mut().get_or_insert_with(|| {
                    EvalError::new("E0802", "the ODE event index is out of bounds").at(span)
                });
                return f64::NAN;
            };
            event_value(
                &mut cell.borrow_mut(),
                &event.g,
                &shape,
                &time_dim,
                t,
                &ode_restore(state, &scales),
                &mut failure.borrow_mut(),
                span,
            )
        };
        aevia_std::ode::solve(&mut rhs, &flat, (start, end), options, &specs, &mut watch)
    };
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    let solution = outcome.map_err(|error| numeric_error(name, error, span))?;
    Ok(ode_solution_value(
        solution,
        &shape,
        &scales,
        &time_dim,
        crate::value::value_dim(initial),
    ))
}

/// `ode_bvp(rhs, guess, (t0, t1), bc, opts)` (§8.9): single shooting.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps the shooting solver and its boundary callback outside the dispatch match"
    )
)]
fn ode_bvp_call(
    interp: &mut Interp,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let name = "ode_bvp";
    let [function, guess, interval, condition, _] = args else {
        return Err(arity(name, 5, args.len(), span));
    };
    let (start, end, time_dim) = ode_interval(Some(interval), name, span)?;
    let mut options = ode_options(args, 4, span)?;
    let scales = ode_component_scales(args, 4, guess, &mut options, span)?;
    let (shape, flat) = flatten_state(guess)?;
    let flat = ode_normalize(&flat, &scales);
    let failure: std::cell::RefCell<Option<EvalError>> = std::cell::RefCell::new(None);
    let outcome =
        {
            let cell = std::cell::RefCell::new(&mut *interp);
            let mut rhs = |t: f64, state: &[f64]| {
                let physical = ode_restore(state, &scales);
                let rate = ode_rate_value(
                    &mut cell.borrow_mut(),
                    function,
                    &shape,
                    &time_dim,
                    t,
                    &physical,
                    &mut failure.borrow_mut(),
                    span,
                );
                ode_normalize(&rate, &scales)
            };
            let mut boundary =
                |left: &[f64], right: &[f64]| -> Vec<f64> {
                    let arguments = vec![
                        rebuild_state(&shape, &ode_restore(left, &scales)),
                        rebuild_state(&shape, &ode_restore(right, &scales)),
                    ];
                    let residual = cell.borrow_mut().apply(condition.clone(), arguments, span);
                    let outcome = match residual {
                | Ok(Value::Tensor(tensor)) => tensor_values(&tensor),
                | Ok(Value::Num(number)) => number.to_f64().map(|v| vec![v]).ok_or_else(|| {
                    EvalError::new("E0302", "a boundary residual must be a `Float`").at(span)
                }),
                | Ok(other) => Err(EvalError::new(
                    "E0302",
                    format!(
                        "the boundary condition must answer a tensor of residuals, found {}",
                        other.type_name()
                    ),
                )
                .at(span)),
                | Err(error) => Err(error),
            };
                    match outcome {
                        | Ok(values) => values,
                        | Err(error) => {
                            if failure.borrow().is_none() {
                                *failure.borrow_mut() = Some(error);
                            }
                            Vec::new()
                        },
                    }
                };
            aevia_std::ode::shoot(&mut rhs, &flat, (start, end), options, &mut boundary)
        };
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    let solution = outcome.map_err(|error| numeric_error(name, error, span))?;
    Ok(ode_solution_value(
        solution,
        &shape,
        &scales,
        &time_dim,
        crate::value::value_dim(guess),
    ))
}

/// `odeint_order(rhs, (y0, y1, …), times)` / `odeint_order_with(…, opts)`
/// (§8.9): the order-`n` equation `y⁽ⁿ⁾ = rhs(t, y, y′, …, y⁽ⁿ⁻¹⁾)` rewritten
/// as a first-order system on the stacked state. Every derivative keeps its
/// own dimension (`y⁽ᵏ⁾` is `y / T^k`), checked against the initial values
/// and against the right-hand side's answer.
#[expect(
    clippy::too_many_lines,
    reason = "keep stack layout, dimensions and the callback contract together"
)]
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "adapts the higher-order ODE builtins to one first-order solver"
    )
)]
fn odeint_order_call(
    interp: &mut Interp,
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let with_options = name == "odeint_order_with";
    let expected = if with_options { 4 } else { 3 };
    let bad_arity = || arity(name, expected, args.len(), span);
    let [function, initial_tuple, ..] = args else {
        return Err(bad_arity());
    };
    if args.len() != expected {
        return Err(bad_arity());
    }
    let invalid_tuple = || {
        EvalError::new(
            "E0302",
            format!("`{name}` expects the initial values as a tuple `(y, y', …)`"),
        )
        .at(span)
    };
    let Value::Record(derivatives) = initial_tuple else {
        return Err(invalid_tuple());
    };
    let times = expect_tensor(args, 2, name, span)?;
    if times.rank() != 1 {
        return Err(
            EvalError::new("E0802", format!("`{name}` needs a rank-one time grid")).at(span),
        );
    }
    let grid = tensor_values(&times)?;
    let time_dim = times.dim.clone();
    let mut options = if with_options {
        if !matches!(args.get(3), Some(Value::Record(_))) {
            return Err(
                EvalError::new("E0302", "`odeint_order_with` expects an OdeOpts record").at(span),
            );
        }
        if ode_event(args, 3, span)?.is_some() {
            return Err(EvalError::new(
                "E0302",
                "`odeint_order_with` does not support `events`; reduce the equation by hand and use `ode_solve`",
            )
            .at(span));
        }
        ode_options(args, 3, span)?
    } else {
        aevia_std::ode::Options::default()
    };
    let order = derivatives.fields.len();
    if order == 0 {
        return Err(EvalError::new(
            "E0302",
            format!("`{name}` needs at least one initial value"),
        )
        .at(span));
    }
    let first = derivatives.fields.get("0").ok_or_else(invalid_tuple)?;
    let y_dim = crate::value::value_dim(first);
    let (first_shape, mut flat) = flatten_state(first)?;
    let width = flat.len();
    if width == 0 {
        return Err(EvalError::new("E0802", format!("`{name}` needs a nonempty state")).at(span));
    }
    let too_large =
        || EvalError::new("E0802", format!("`{name}`: the stacked state is too large")).at(span);
    let state_width = order.checked_mul(width).ok_or_else(too_large)?;
    let argument_count = order.checked_add(1).ok_or_else(too_large)?;
    let mut shapes = Vec::new();
    shapes.try_reserve_exact(order).map_err(|_| too_large())?;
    flat.try_reserve_exact(state_width.saturating_sub(width))
        .map_err(|_| too_large())?;
    let mut divisor = time_dim.clone();
    for index in 1..order {
        let component = derivatives
            .fields
            .get(&index.to_string())
            .ok_or_else(invalid_tuple)?;
        let (shape, values) = flatten_state(component)?;
        if shape != scale_state_dims(&first_shape, &divisor) || values.len() != width {
            return Err(EvalError::new(
                if state_structure_matches(&first_shape, &shape) && values.len() == width {
                    "E0401"
                } else {
                    "E0302"
                },
                format!(
                    "initial value {index} of `{name}` must have the structure of `y` and the dimension of `y` divided by T^{index}"
                ),
            )
            .at(span));
        }
        shapes.push(shape);
        flat.extend(values);
        divisor = divisor.mul(&time_dim);
    }
    shapes.insert(0, first_shape);
    let Some(top_shape) = shapes.last() else {
        return Err(invalid_tuple());
    };
    // Tolerances and state values must be flattened in the same derivative order.
    let reordered = if with_options
        && derivatives
            .fields
            .keys()
            .enumerate()
            .any(|(index, key)| key.parse::<usize>() != Ok(index))
    {
        let fields = (0..order)
            .map(|index| {
                let field = index.to_string();
                derivatives
                    .fields
                    .get(&field)
                    .cloned()
                    .map(|value| (field, value))
                    .ok_or_else(invalid_tuple)
            })
            .collect::<Result<IndexMap<_, _>, _>>()?;
        Some(Value::Record(Rc::new(RecordValue { fields })))
    } else {
        None
    };
    let scales = ode_component_scales(
        args,
        3,
        reordered.as_ref().unwrap_or(initial_tuple),
        &mut options,
        span,
    )?;
    let flat = ode_normalize(&flat, &scales);
    let failure: std::cell::RefCell<Option<EvalError>> = std::cell::RefCell::new(None);
    let outcome = {
        let cell = std::cell::RefCell::new(&mut *interp);
        let mut reduced = aevia_std::ode::reduce_order(order, width, |t: f64, z: &[f64]| {
            let mut arguments = Vec::new();
            if arguments.try_reserve_exact(argument_count).is_err() {
                failure.borrow_mut().get_or_insert_with(too_large);
                return Vec::new();
            }
            arguments.push(Value::Num(NumVal::float(t, time_dim.clone())));
            arguments.extend(
                shapes
                    .iter()
                    .zip(z.chunks_exact(width))
                    .map(|(shape, component)| rebuild_state(shape, component)),
            );
            match cell.borrow_mut().apply(function.clone(), arguments, span) {
                | Ok(value) => {
                    ode_rates(
                        &value,
                        top_shape,
                        &time_dim,
                        width,
                        span,
                        &mut failure.borrow_mut(),
                    )
                },
                | Err(error) => {
                    failure.borrow_mut().get_or_insert(error);
                    Vec::new()
                },
            }
        });
        let mut normalized = |t: f64, state: &[f64]| {
            let physical = ode_restore(state, &scales);
            ode_normalize(&reduced(t, &physical), &scales)
        };
        aevia_std::ode::integrate(&mut normalized, &flat, &grid, options)
    };
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    let states = outcome.map_err(|error| numeric_error(name, error, span))?;
    let len = states.len();
    let mut ys = Vec::with_capacity(len);
    let mut all = Vec::with_capacity(len);
    for state in states {
        if state.len() != state_width {
            return Err(EvalError::new(
                "E0802",
                "the ODE solver returned a state of the wrong length",
            )
            .at(span));
        }
        let physical = ode_restore(&state, &scales);
        let fields = shapes
            .iter()
            .zip(physical.chunks_exact(width))
            .enumerate()
            .map(|(index, (shape, component))| (index.to_string(), rebuild_state(shape, component)))
            .collect::<IndexMap<_, _>>();
        let first = fields.get("0").ok_or_else(invalid_tuple)?;
        ys.push(first.clone());
        all.push(Value::Record(Rc::new(RecordValue { fields })));
    }
    Ok(record_of(vec![
        ("t", float_tensor(vec![grid.len()], grid, time_dim)),
        ("y", build_tensor(ys, vec![len], y_dim)),
        ("d", build_tensor(all, vec![len], Dim::one())),
    ]))
}

#[cfg(test)]
mod odeint_order_tests {
    use super::Dim;
    use super::Interp;
    use super::Span;
    use super::Value;
    use super::float_tensor;
    use super::odeint_order_call;
    use super::tuple;

    #[test]
    fn host_calls_check_argument_count_time_rank_and_state_width() {
        let mut interp = Interp::new(&aevia_types::CheckedModule::default());
        let span = Span::default();
        let time = Dim::base("Time");
        let initial = tuple(vec![Value::float(1.0)]);
        let regular_times = float_tensor(vec![2], vec![0.0, 1.0], time.clone());
        let ordinary = [Value::Unit, initial.clone(), regular_times.clone()];
        let missing = odeint_order_call(&mut interp, "odeint_order_with", &ordinary, span);
        assert!(matches!(missing, Err(error) if error.code == "E0315"));
        let extra = [
            Value::Unit,
            initial.clone(),
            regular_times.clone(),
            Value::Unit,
        ];
        let excess = odeint_order_call(&mut interp, "odeint_order", &extra, span);
        assert!(matches!(excess, Err(error) if error.code == "E0315"));
        let malformed_options = odeint_order_call(&mut interp, "odeint_order_with", &extra, span);
        assert!(matches!(malformed_options, Err(error) if error.code == "E0302"));

        let wrong_rank = float_tensor(vec![1, 2], vec![0.0, 1.0], time);
        let wrong_axis = [Value::Unit, initial, wrong_rank];
        let invalid = odeint_order_call(&mut interp, "odeint_order", &wrong_axis, span);
        assert!(
            matches!(invalid, Err(error) if error.code == "E0802" && error.message.contains("rank-one"))
        );

        let empty = tuple(vec![float_tensor(vec![0], Vec::new(), Dim::one())]);
        let args = [Value::Unit, empty, regular_times];
        let invalid = odeint_order_call(&mut interp, "odeint_order", &args, span);
        assert!(
            matches!(invalid, Err(error) if error.code == "E0802" && error.message.contains("nonempty state"))
        );
    }
}

/// The state structure with every dimension divided by `divisor`.
fn scale_state_dims(
    shape: &StateShape,
    divisor: &Dim,
) -> StateShape {
    match shape {
        | StateShape::Scalar(dim) => StateShape::Scalar(dim.div(divisor)),
        | StateShape::Tensor(extent, dim) => StateShape::Tensor(extent.clone(), dim.div(divisor)),
        | StateShape::Record(fields) => {
            StateShape::Record(
                fields
                    .iter()
                    .map(|(name, inner)| (name.clone(), scale_state_dims(inner, divisor)))
                    .collect(),
            )
        },
    }
}

/// Whether two states have the same structure, ignoring dimensions.
fn state_structure_matches(
    left: &StateShape,
    right: &StateShape,
) -> bool {
    match (left, right) {
        | (StateShape::Scalar(_), StateShape::Scalar(_)) => true,
        | (StateShape::Tensor(a, _), StateShape::Tensor(b, _)) => a == b,
        | (StateShape::Record(a), StateShape::Record(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|((na, sa), (nb, sb))| na == nb && state_structure_matches(sa, sb))
        },
        | _ => false,
    }
}

// ----- §8.9.1 continuous output, events and ensembles ----------------------

/// A numerical SDE path read back from its `SdePath` record.
struct PathParts {
    path: aevia_std::sde::Path,
    shape: StateShape,
    time_dim: Dim,
    state_dim: Dim,
    record: Rc<RecordValue>,
}

/// Reads the `t` and `y` fields of an `SdePath` (any record with those
/// fields): the nodes, the state structure and the dimensions.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "validates the SDE path record before the individual operations use it"
    )
)]
fn path_parts(
    value: Option<&Value>,
    name: &str,
    span: Span,
) -> Result<PathParts, EvalError> {
    let Some(Value::Record(record)) = value else {
        return Err(EvalError::new("E0302", format!("`{name}` expects an `SdePath`")).at(span));
    };
    let (Some(Value::Tensor(times)), Some(Value::Tensor(states))) =
        (record.fields.get("t"), record.fields.get("y"))
    else {
        return Err(EvalError::new(
            "E0302",
            format!("`{name}` expects an `SdePath` with the fields `t` and `y`"),
        )
        .at(span));
    };
    let t = tensor_values(times)?;
    let mut y = Vec::with_capacity(states.shape.first().copied().unwrap_or(0));
    let mut shape = None;
    for element in states.elements() {
        let (element_shape, flat) = flatten_state(&element)?;
        if let Some(first) = &shape {
            if !state_structure_matches(first, &element_shape) {
                return Err(EvalError::new(
                    "E0802",
                    format!("`{name}`: the path's states must have the same structure"),
                )
                .at(span));
            }
        } else {
            shape = Some(element_shape);
        }
        y.push(flat);
    }
    let Some(shape) = shape else {
        return Err(EvalError::new("E0802", format!("`{name}`: the path has no nodes")).at(span));
    };
    if t.len() != y.len() {
        return Err(EvalError::new(
            "E0802",
            format!("`{name}`: the path's `t` and `y` differ in length"),
        )
        .at(span));
    }
    let steps = record
        .fields
        .get("steps")
        .and_then(host_count)
        .unwrap_or_else(|| t.len().saturating_sub(1));
    Ok(PathParts {
        path: aevia_std::sde::Path {
            t,
            y,
            steps,
            rejected: 0,
            nfev: 0,
            seed: 0,
            noise: 0,
            method: aevia_std::sde::Method::EulerMaruyama,
        },
        shape,
        time_dim: times.dim.clone(),
        state_dim: states.dim.clone(),
        record: Rc::clone(record),
    })
}

/// The time of `sde_at` / `sde_interp`: a `Float` of the path's time dimension.
fn path_time(
    value: Option<&Value>,
    time_dim: &Dim,
    name: &str,
    span: Span,
) -> Result<f64, EvalError> {
    let Some(Value::Num(time)) = value else {
        return Err(EvalError::new("E0302", format!("`{name}` expects a time")).at(span));
    };
    if time.dim != *time_dim {
        return Err(EvalError::new(
            "E0401",
            format!("the time given to `{name}` must have the path's time dimension"),
        )
        .at(span));
    }
    time.to_f64().ok_or_else(|| {
        EvalError::new("E0302", format!("`{name}` expects a concrete time")).at(span)
    })
}

fn path_range_error(
    path: &aevia_std::sde::Path,
    t: f64,
    span: Span,
) -> EvalError {
    EvalError::new(
        "E0802",
        format!(
            "time {t} lies outside the path's span [{}, {}]",
            path.t.first().copied().unwrap_or(0.0),
            path.t.last().copied().unwrap_or(0.0)
        ),
    )
    .at(span)
}

/// `sde_at`, `sde_resample`, `sde_interp` and `sde_events` (§8.9.1).
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "groups the public SDE path operations outside the builtin dispatcher"
    )
)]
fn sde_path_call(
    interp: &mut Interp,
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let parts = path_parts(args.first(), name, span)?;
    match name {
        | "sde_at" => {
            let t = path_time(args.get(1), &parts.time_dim, name, span)?;
            let state = parts
                .path
                .eval(t)
                .ok_or_else(|| path_range_error(&parts.path, t, span))?;
            Ok(rebuild_state(&parts.shape, &state))
        },
        | "sde_resample" => {
            let Some(Value::Tensor(times)) = args.get(1) else {
                return Err(
                    EvalError::new("E0302", "`sde_resample` expects a tensor of times").at(span),
                );
            };
            if times.dim != parts.time_dim {
                return Err(EvalError::new(
                    "E0401",
                    "the times given to `sde_resample` must have the path's time dimension",
                )
                .at(span));
            }
            let wanted = tensor_values(times)?;
            let mut states = Vec::with_capacity(wanted.len());
            for &t in &wanted {
                let state = parts
                    .path
                    .eval(t)
                    .ok_or_else(|| path_range_error(&parts.path, t, span))?;
                states.push(rebuild_state(&parts.shape, &state));
            }
            let len = states.len();
            Ok(build_tensor(states, vec![len], parts.state_dim))
        },
        | "sde_interp" => {
            let PathParts {
                path,
                shape,
                time_dim,
                ..
            } = parts;
            let path = Rc::new(path);
            Ok(Value::Native(Rc::new(NativeFn(Box::new(
                move |_interp, args, span| {
                    let t = path_time(args.first(), &time_dim, "the interpolated path", span)?;
                    let state = path
                        .eval(t)
                        .ok_or_else(|| path_range_error(&path, t, span))?;
                    Ok(rebuild_state(&shape, &state))
                },
            )))))
        },
        | _ => sde_path_events(interp, parts, args.get(1), name, span),
    }
}

/// Locate SDE events and preserve the input path's additional record fields.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "isolates event callbacks and the resulting truncated path"
    )
)]
fn sde_path_events(
    interp: &mut Interp,
    parts: PathParts,
    event_arg: Option<&Value>,
    name: &str,
    span: Span,
) -> Result<Value, EvalError> {
    let events = parse_events(event_arg, name, span)?;
    let specs: Vec<aevia_std::sde::EventSpec> = events
        .iter()
        .map(|event| {
            aevia_std::sde::EventSpec {
                terminal: event.terminal,
                direction: event.direction,
            }
        })
        .collect();
    let failure: std::cell::RefCell<Option<EvalError>> = std::cell::RefCell::new(None);
    let (cut, hits) = {
        let cell = std::cell::RefCell::new(&mut *interp);
        let mut watch = |index: usize, t: f64, state: &[f64]| {
            let Some(event) = events.get(index) else {
                failure.borrow_mut().get_or_insert_with(|| {
                    EvalError::new("E0802", "the SDE event index is out of bounds").at(span)
                });
                return f64::NAN;
            };
            event_value(
                &mut cell.borrow_mut(),
                &event.g,
                &parts.shape,
                &parts.time_dim,
                t,
                state,
                &mut failure.borrow_mut(),
                span,
            )
        };
        aevia_std::sde::events(&parts.path, &specs, &mut watch)
    };
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    let hit_values: Vec<Value> = hits
        .iter()
        .map(|hit| {
            record_of(vec![
                ("index", Value::int(i64::try_from(hit.index).unwrap_or(0))),
                (
                    "t",
                    Value::Num(NumVal::float(hit.t, parts.time_dim.clone())),
                ),
                ("y", rebuild_state(&parts.shape, &hit.y)),
            ])
        })
        .collect();
    let hit_count = hit_values.len();
    let states: Vec<Value> = cut
        .y
        .iter()
        .map(|state| rebuild_state(&parts.shape, state))
        .collect();
    let len = states.len();
    // The cut path keeps every other field of the original record.
    let mut fields = parts.record.fields.clone();
    fields.insert(
        "t".to_string(),
        float_tensor(vec![len], cut.t, parts.time_dim),
    );
    fields.insert(
        "y".to_string(),
        build_tensor(states, vec![len], parts.state_dim),
    );
    if fields.contains_key("steps") {
        fields.insert(
            "steps".to_string(),
            Value::int(i64::try_from(cut.steps).unwrap_or(0)),
        );
    }
    Ok(record_of(vec![
        ("path", Value::Record(Rc::new(RecordValue { fields }))),
        (
            "hits",
            build_tensor(hit_values, vec![hit_count], Dim::one()),
        ),
    ]))
}

/// `sde_ensemble_diag` / `sde_ensemble_general`: `n` independent paths with
/// member seeds `sde_ensemble_seed(seed, i)`.
///
/// Member `i` depends only on `(seed, i)`, so the result is the same for every
/// execution order and thread count. The members run through the parallel
/// replicas of §9.7.2 when the drift and diffusion can cross threads and the
/// ensemble is large enough to pay for it, and sequentially on this thread
/// otherwise; both give the same value.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps replica scheduling and seed partitioning together"
    )
)]
fn sde_ensemble_call(
    interp: &mut Interp,
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let base = if name == "sde_ensemble_general" {
        "sdeint_general"
    } else {
        "sdeint_diag"
    };
    let count = expect_int(args, 4, name, span)?;
    if count <= 0 {
        return Err(EvalError::new("E0802", "an ensemble needs at least one path").at(span));
    }
    let count = usize::try_from(count)
        .map_err(|_| EvalError::new("E0802", "the ensemble size is out of range").at(span))?;
    let [drift, diffusion, initial, options, _] = args else {
        return Err(arity(name, 5, args.len(), span));
    };
    // A malformed policy is the members' `Err(SdeError)`, not a crash here.
    let member = Value::ctor(crate::value::CtorValue {
        adt: u32::MAX,
        index: usize::MAX,
        name: "sde_member".to_string(),
        arity: usize::MAX,
        args: vec![
            Value::string(base),
            drift.clone(),
            diffusion.clone(),
            initial.clone(),
            options.clone(),
        ],
        origin: None,
    });
    let mut inputs = Vec::new();
    inputs
        .try_reserve_exact(count)
        .map_err(|_| EvalError::new("E0802", "the ensemble is too large").at(span))?;
    let count_int = i64::try_from(count)
        .map_err(|_| EvalError::new("E0802", "the ensemble size is out of range").at(span))?;
    inputs.extend((0..count_int).map(Value::int));
    let parallel = interp.par_map_values(&member, &inputs, Some(Dim::one()));
    interp.check_cancelled().map_err(|error| error.at(span))?;
    if let Some(result) = parallel {
        return Ok(result);
    }
    let mut results = Vec::with_capacity(count);
    for input in inputs {
        results.push(interp.apply(member.clone(), vec![input], span)?);
    }
    Ok(build_tensor(results, vec![count], Dim::one()))
}

// ----- §8.9.1 stochastic differential equations ---------------------------

use aevia_std::sde::SdeError;

/// Construct an NPY read action with an authenticated concrete rank.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "the interpreter authenticates the runtime rank at this boundary"
    )
)]
pub(crate) fn read_npy_ranked(
    args: &[Value],
    rank: u32,
    span: Span,
) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(arity("read_npy", 1, args.len(), span));
    }
    let path = expect_str(args, 0, "read_npy", span)?;
    Ok(Value::Io(Rc::new(crate::value::IoAction::ReadNpy {
        path,
        rank,
    })))
}

// ----- §8.11 NPY codec -----------------------------------------------------

/// Encode a dimensionless Float tensor, preserving its shape.
///
/// # Errors
/// Rejects dimensioned, malformed or non-Float storage.
pub fn npy_encode(tensor: &crate::value::TensorData) -> Result<Vec<u8>, String> {
    if !tensor.dim.is_one() {
        return Err("E0902: NPY cannot store tensor physical dimensions".to_string());
    }
    aevia_std::npy::checked_element_count(&tensor.shape)
        .ok_or_else(|| "E0902: NPY tensor shape or strides overflow".to_string())?;
    let values = if matches!(tensor.storage.backing(), Storage::Boxed(_)) {
        if tensor.storage.is_empty()
            && !matches!(tensor.verified_element.as_deref(), Some(aevia_types::Type::Scalar(aevia_types::Rep::Float, dim)) if dim.is_one())
        {
            return Err("E0902: `write_npy` needs proven Float elements".to_string());
        }
        let values = (0..tensor.storage.len())
            .map(|index| {
                match tensor.get_flat(index) {
                    | Some(Value::Num(number)) if number.dim.is_one() => {
                        match &number.kind {
                            | NumKind::Float(value) => Some(*value),
                            | _ => None,
                        }
                    },
                    | _ => None,
                }
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| "E0902: `write_npy` needs Float elements".to_string())?;
        std::borrow::Cow::Owned(values)
    } else {
        tensor
            .dense_flat()
            .ok_or_else(|| "E0902: `write_npy` needs Float elements".to_string())?
    };
    aevia_std::npy::encode(&tensor.shape, &values)
}

/// Decode an NPY array into row-major Float storage.
///
/// # Errors
/// Returns the shared codec's E0903/E0904 errors.
pub fn npy_decode(bytes: &[u8]) -> Result<crate::value::TensorData, String> {
    let (shape, values) = aevia_std::npy::decode(bytes)?;
    Ok(crate::value::TensorData {
        verified_element: None,
        shape,
        storage: crate::value::Storage::Dense(std::rc::Rc::new(values)),
        dim: aevia_dim::Dim::one(),
        display: None,
    })
}

/// The Aevia spelling of an `SdeMethod` constructor → the standard scheme.
fn sde_method_named(name: &str) -> Option<aevia_std::sde::Method> {
    Some(match name {
        | "EulerMaruyama" => aevia_std::sde::Method::EulerMaruyama,
        | "Milstein" => aevia_std::sde::Method::Milstein,
        | "MilsteinStratonovich" => aevia_std::sde::Method::MilsteinStratonovich,
        | "HeunStratonovich" => aevia_std::sde::Method::HeunStratonovich,
        | "SrkPlaten" => aevia_std::sde::Method::SrkPlaten,
        | "MilsteinNoncommutative" => aevia_std::sde::Method::MilsteinNoncommutative,
        | "MilsteinNoncommutativeStratonovich" => {
            aevia_std::sde::Method::MilsteinNoncommutativeStratonovich
        },
        | "DriftImplicitEuler" => aevia_std::sde::Method::DriftImplicitEuler,
        | "ThetaMaruyama" => aevia_std::sde::Method::ThetaMaruyama,
        | "DriftImplicitMilstein" => aevia_std::sde::Method::DriftImplicitMilstein,
        | _ => return None,
    })
}

/// The `SdeMethod` value the `SdePath` record reports.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "preserves the source declaration witness when returning the selected method"
    )
)]
fn sde_method_value(
    method: aevia_std::sde::Method,
    source_adt: adt::AdtId,
) -> Value {
    let (index, name) = match method {
        | aevia_std::sde::Method::EulerMaruyama => (0, "EulerMaruyama"),
        | aevia_std::sde::Method::Milstein => (1, "Milstein"),
        | aevia_std::sde::Method::MilsteinStratonovich => (2, "MilsteinStratonovich"),
        | aevia_std::sde::Method::HeunStratonovich => (3, "HeunStratonovich"),
        | aevia_std::sde::Method::SrkPlaten => (4, "SrkPlaten"),
        | aevia_std::sde::Method::MilsteinNoncommutative => (5, "MilsteinNoncommutative"),
        | aevia_std::sde::Method::MilsteinNoncommutativeStratonovich => {
            (6, "MilsteinNoncommutativeStratonovich")
        },
        | aevia_std::sde::Method::DriftImplicitEuler => (7, "DriftImplicitEuler"),
        | aevia_std::sde::Method::ThetaMaruyama => (8, "ThetaMaruyama"),
        | aevia_std::sde::Method::DriftImplicitMilstein => (9, "DriftImplicitMilstein"),
    };
    // Preserve the declaration witness from the options; rebuilding an opaque
    // same-named host type would make a source constructor pattern never match.
    make_variant(source_adt, index, name, Vec::new())
}

/// The noise structure a `SdeOpts` record names.
///
/// The checker's option schema is deliberately loose (§8.9.1): the record is
/// validated at run time, and a malformed option is the recoverable
/// `SdeError::Invalid` (`E0802`), never a silent default.
fn sde_structure(value: &Value) -> Result<aevia_std::sde::Structure, SdeError> {
    let Value::Variant(variant) = value else {
        return Err(SdeError::Invalid(
            "`structure` must be an `SdeStructure`".to_string(),
        ));
    };
    match variant.name.as_str() {
        | "Diagonal" => Ok(aevia_std::sde::Structure::Diagonal),
        | "General" | "GeneralNoncommutative" => {
            let noise = variant.fields.first().and_then(host_count).ok_or_else(|| {
                SdeError::Invalid("`General` needs a nonnegative noise-channel count".to_string())
            })?;
            Ok(aevia_std::sde::Structure::General {
                noise,
                commutative: variant.name == "General",
            })
        },
        | other => {
            Err(SdeError::Invalid(format!(
                "`{other}` is not an `SdeStructure`"
            )))
        },
    }
}

fn sde_float_field(
    record: &RecordValue,
    field: &str,
) -> Result<f64, SdeError> {
    record
        .fields
        .get(field)
        .and_then(Value::as_f64)
        .ok_or_else(|| SdeError::Invalid(format!("the SDE options need `{field}`")))
}

/// Convert a concrete nonnegative integer without rounding exact values through Float.
fn nonnegative_integer(value: &Value) -> Option<u64> {
    let Value::Num(number) = value else {
        return None;
    };
    match &number.kind {
        | NumKind::Int(int) => int.to_u64(),
        | NumKind::Exact(rational) if rational.is_integer() => rational.numer().to_u64(),
        | NumKind::Float(float) if float.fract() == 0.0 => float.to_u64(),
        | _ => None,
    }
}

fn host_count(value: &Value) -> Option<usize> {
    usize::try_from(nonnegative_integer(value)?).ok()
}

fn sde_count_field(
    record: &RecordValue,
    field: &str,
) -> Result<usize, SdeError> {
    record
        .fields
        .get(field)
        .and_then(host_count)
        .ok_or_else(|| SdeError::Invalid(format!("the SDE options need an integer `{field}`")))
}

/// A seed: an exact `Int` taken modulo 2^64, so a full member seed
/// round-trips through the options record.
fn exact_seed(value: &Value) -> Option<u64> {
    let Value::Num(number) = value else {
        return None;
    };
    if let NumKind::Int(int) = &number.kind {
        use num_integer::Integer as _;
        const SEED_MODULUS: u128 = 1_u128 << u64::BITS;
        return int.to_big().mod_floor(&BigInt::from(SEED_MODULUS)).to_u64();
    }
    nonnegative_integer(value)
}

/// The `SdeOpts<T>` record as the standard library's `Options`, plus the time
/// dimension the path's `t` tensor reports.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "decodes the nominal solver methods and policies before integration"
    )
)]
fn sde_options(value: &Value) -> Result<(aevia_std::sde::Options, Dim, adt::AdtId), SdeError> {
    let Value::Variant(option) = value else {
        return Err(SdeError::Invalid(
            "the fourth argument must be `SdeOpts`".to_string(),
        ));
    };
    let Some(Value::Record(record)) = option.fields.first() else {
        return Err(SdeError::Invalid(
            "`SdeOpts` carries an options record".to_string(),
        ));
    };
    let (method, method_adt) = match record.fields.get("method") {
        | Some(Value::Variant(method)) => {
            let selected = sde_method_named(&method.name).ok_or_else(|| {
                SdeError::Invalid(format!("`{}` is not an `SdeMethod`", method.name))
            })?;
            (selected, method.adt)
        },
        | _ => {
            return Err(SdeError::Invalid(
                "the SDE options need `method`".to_string(),
            ));
        },
    };
    let structure = sde_structure(
        record
            .fields
            .get("structure")
            .ok_or_else(|| SdeError::Invalid("the SDE options need `structure`".to_string()))?,
    )?;
    let seed = record
        .fields
        .get("seed")
        .and_then(exact_seed)
        .ok_or_else(|| SdeError::Invalid("the SDE options need `seed`".to_string()))?;
    let terms = sde_count_field(record, "terms")?;
    let Some(Value::Num(t0)) = record.fields.get("t0") else {
        return Err(SdeError::Invalid("the SDE options need `t0`".into()));
    };
    let t0_value = t0
        .to_f64()
        .ok_or_else(|| SdeError::Invalid("`t0` must be concrete".to_string()))?;
    let time_dim = t0.dim.clone();
    let options = match option.name.as_str() {
        | "Fixed" => {
            aevia_std::sde::Options::fixed(
                method,
                structure,
                seed,
                t0_value,
                sde_float_field(record, "dt")?,
                sde_count_field(record, "steps")?,
            )
            .with_terms(terms)
        },
        | "Adaptive" => {
            aevia_std::sde::Options::adaptive(
                method,
                structure,
                seed,
                t0_value,
                sde_float_field(record, "t1")?,
                sde_float_field(record, "rtol")?,
                sde_float_field(record, "atol")?,
                sde_count_field(record, "max_steps")?,
                sde_count_field(record, "cells")?,
            )
            .with_terms(terms)
        },
        | other => {
            return Err(SdeError::Invalid(format!(
                "`{other}` is not a step policy (`Fixed`/`Adaptive`)"
            )));
        },
    };
    Ok((options, time_dim, method_adt))
}

/// An `SdeError` as the opaque error value the `Result` carries.
fn sde_error_value(error: &SdeError) -> Value {
    let (index, name, payload): (usize, &str, Vec<Value>) = match error {
        | SdeError::Invalid(message) => (0, "Invalid", vec![Value::string(message.clone())]),
        | SdeError::Shape(message) => (1, "Shape", vec![Value::string(message.clone())]),
        | SdeError::NonFinite(message) => (2, "NonFinite", vec![Value::string(message.clone())]),
        | SdeError::StepLimit(message) => (3, "StepLimit", vec![Value::string(message.clone())]),
        | SdeError::Unsupported { capability, detail } => {
            (
                4,
                "Unsupported",
                vec![Value::string(*capability), Value::string(detail.clone())],
            )
        },
    };
    let id = adt::intern(adt::Adt {
        name: "SdeError".to_string(),
        params: Vec::new(),
        ctors: Vec::new(),
    });
    make_variant(id, index, name, payload)
}

/// The five §8.9.1 names.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "groups the public SDE solver and Wiener operations outside the builtin dispatcher"
    )
)]
fn sde_call(
    interp: &mut Interp,
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    match name {
        | "wiener" => {
            let seed = u64::try_from(expect_int(args, 0, name, span)?.max(0)).unwrap_or_default();
            let noise = host_count_arg(args, 1, name, span)?;
            let steps = host_count_arg(args, 2, name, span)?;
            let dt = expect_num(args, 3, name, span)?;
            let dt_value = dt.to_f64().ok_or_else(|| {
                EvalError::new("E0302", "`wiener` needs a concrete `dt`").at(span)
            })?;
            let too_large =
                || EvalError::new("E0802", "the Wiener increment shape is too large").at(span);
            let flat = if steps == 0 || noise == 0 {
                Vec::new()
            } else {
                let count = steps.checked_mul(noise).ok_or_else(too_large)?;
                let mut flat = Vec::new();
                std::alloc::Layout::array::<Vec<f64>>(steps).map_err(|_| too_large())?;
                flat.try_reserve_exact(count).map_err(|_| too_large())?;
                flat.extend(
                    aevia_std::sde::wiener(seed, noise, steps, dt_value)
                        .into_iter()
                        .flatten(),
                );
                flat
            };
            let half = BigRational::new(BigInt::one(), BigInt::from(2));
            Ok(float_tensor(vec![steps, noise], flat, dt.dim.pow(&half)))
        },
        | "sde_order" => {
            let Some(Value::Variant(method)) = args.first() else {
                return Err(EvalError::new("E0302", "`sde_order` expects an `SdeMethod`").at(span));
            };
            let method = sde_method_named(&method.name).ok_or_else(|| {
                EvalError::new("E0302", format!("`{}` is not an `SdeMethod`", method.name)).at(span)
            })?;
            let order = aevia_std::sde::sde_order(method);
            Ok(record_of(vec![
                ("strong", Value::float(order.strong)),
                ("weak", Value::float(order.weak)),
            ]))
        },
        | "sde_order_with" => {
            let (Some(Value::Variant(method)), Some(structure)) = (args.first(), args.get(1))
            else {
                return Err(EvalError::new(
                    "E0302",
                    "`sde_order_with` expects an `SdeMethod` and an `SdeStructure`",
                )
                .at(span));
            };
            let method = sde_method_named(&method.name).ok_or_else(|| {
                EvalError::new("E0302", format!("`{}` is not an `SdeMethod`", method.name)).at(span)
            })?;
            let structure = sde_structure(structure)
                .map_err(|error| EvalError::new("E0302", error.to_string()).at(span))?;
            let order = aevia_std::sde::sde_order_for(method, structure);
            Ok(record_of(vec![
                ("strong", Value::float(order.strong)),
                ("weak", Value::float(order.weak)),
            ]))
        },
        | "sde_ensemble_seed" => {
            let seed = args
                .first()
                .and_then(exact_seed)
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let index = host_count_arg(args, 1, name, span)?;
            Ok(Value::Num(NumVal::int(BigInt::from(
                aevia_std::sde::ensemble_seed(seed, index),
            ))))
        },
        | _ => sde_integrate(interp, name, args, None, span),
    }
}

/// A non-negative `Int` index.
fn index_value(
    value: &Value,
    span: Span,
) -> Result<usize, EvalError> {
    host_count(value).ok_or_else(|| {
        EvalError::new(
            "E0302",
            "expected a nonnegative `Int` within the index range",
        )
        .at(span)
    })
}

fn host_count_arg(
    args: &[Value],
    index: usize,
    name: &str,
    span: Span,
) -> Result<usize, EvalError> {
    args.get(index)
        .ok_or_else(|| arity(name, index.saturating_add(1), args.len(), span))
        .and_then(|value| index_value(value, span))
}

/// `sdeint_diag` / `sdeint_general` (§8.9.1).
///
/// With `member = Some(i)` the run is member `i` of an ensemble: its seed is
/// `ensemble_seed(seed, i)` of the options' seed.
fn sde_integrate(
    interp: &mut Interp,
    name: &str,
    args: &[Value],
    member: Option<usize>,
    span: Span,
) -> Result<Value, EvalError> {
    let [drift, diffusion, initial, options_value, ..] = args else {
        return Err(arity(name, 4, args.len(), span));
    };
    let general = name == "sdeint_general";
    let (mut options, time_dim, method_adt) = match sde_options(options_value) {
        | Ok(pair) => pair,
        | Err(error) => return Ok(err(sde_error_value(&error))),
    };
    if let Some(index) = member {
        options.seed = aevia_std::sde::ensemble_seed(options.seed, index);
    }
    // The step-policy stubs carry a dimension-free `T` field only for typing;
    // the path reports the initial time's dimension.
    let (shape, flat) = flatten_state(initial)?;
    let state_count = flat.len();
    let noise = options.structure.noise(state_count);
    let diagonal = matches!(options.structure, aevia_std::sde::Structure::Diagonal);
    if diagonal == general {
        return Ok(err(sde_error_value(&SdeError::Invalid(if general {
            "`sdeint_general` needs a `General`/`GeneralNoncommutative` structure".to_string()
        } else {
            "`sdeint_diag` needs the `Diagonal` structure; use `sdeint_general`".to_string()
        }))));
    }
    let coefficients = if diagonal {
        state_count
    } else {
        match state_count.checked_mul(noise) {
            | Some(count) => count,
            | None => {
                return Ok(err(sde_error_value(&SdeError::Shape(
                    "the state × noise size overflows".to_string(),
                ))));
            },
        }
    };
    let failure: std::cell::RefCell<Option<EvalError>> = std::cell::RefCell::new(None);
    let interpreter = std::cell::RefCell::new(interp);
    let evaluate = |function: &Value, t: f64, state: &[f64]| {
        let argument = vec![
            Value::Num(NumVal::float(t, time_dim.clone())),
            rebuild_state(&shape, state),
        ];
        interpreter
            .borrow_mut()
            .apply(function.clone(), argument, span)
    };
    let failure_for = |error, name| {
        failure.borrow_mut().get_or_insert(error);
        SdeError::Invalid(format!("the {name} failed"))
    };
    let drift_fn = |t, state: &[f64]| {
        let rates = evaluate(drift, t, state)
            .and_then(|value| flatten_state(&value).map(|(_, rates)| rates))
            .map_err(|error| failure_for(error, "drift"))?;
        if rates.len() == state.len() {
            Ok(rates)
        } else {
            Err(SdeError::Shape(format!(
                "the drift returned {} rate(s) for a state of {}",
                rates.len(),
                state.len()
            )))
        }
    };
    let diffusion_fn = |t, state: &[f64]| {
        let values = evaluate(diffusion, t, state)
            .and_then(|value| flatten_by_shape(&shape, &value))
            .map_err(|error| failure_for(error, "diffusion"))?;
        if values.len() == coefficients {
            Ok(values)
        } else {
            Err(SdeError::Shape(format!(
                "the diffusion returned {} coefficient(s); this structure needs {coefficients}",
                values.len()
            )))
        }
    };
    let result = aevia_std::sde::integrate(drift_fn, diffusion_fn, &flat, &options);
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    match result {
        | Ok(path) => {
            Ok(ok(sde_path_value(
                path,
                &shape,
                crate::value::value_dim(initial),
                time_dim,
                method_adt,
                noise,
            )))
        },
        | Err(error) => Ok(err(sde_error_value(&error))),
    }
}

/// Convert a completed kernel path into the language's typed result record.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps path reconstruction and the method witness outside the solver callbacks"
    )
)]
fn sde_path_value(
    path: aevia_std::sde::Path,
    shape: &StateShape,
    state_dim: Dim,
    time_dim: Dim,
    method_adt: adt::AdtId,
    noise: usize,
) -> Value {
    let states: Vec<Value> = path
        .y
        .iter()
        .map(|state| rebuild_state(shape, state))
        .collect();
    let len = states.len();
    record_of(vec![
        ("t", float_tensor(vec![path.t.len()], path.t, time_dim)),
        ("y", build_tensor(states, vec![len], state_dim)),
        ("steps", Value::Num(NumVal::int(path.steps))),
        ("rejected", Value::Num(NumVal::int(path.rejected))),
        ("nfev", Value::Num(NumVal::int(path.nfev))),
        ("seed", Value::Num(NumVal::int(path.seed))),
        ("noise", Value::Num(NumVal::int(noise))),
        ("method", sde_method_value(path.method, method_adt)),
    ])
}

/// Flattens a diffusion value in the field order of the *state* it belongs to.
///
/// For a record state the diffusion is a record with the same field names
/// whose scalar fields hold the `m` noise coefficients of that component (a
/// `Tensor<1>` of `m`) and whose rank-`r` tensor fields become rank-`r + 1`
/// tensors (`[.., m]`); walking the state's fields by name and concatenating
/// them row-major gives exactly the kernel's `n x m` row-major matrix, whatever
/// order the diffusion record's fields were written in. Other states flatten
/// as usual (`Diagonal` coefficients, or the rank-2 `n x m` tensor).
///
/// # Errors
///
/// `E0302` for a missing field or a value that holds something other than
/// numbers.
fn flatten_by_shape(
    shape: &StateShape,
    value: &Value,
) -> Result<Vec<f64>, EvalError> {
    match (shape, value) {
        | (StateShape::Record(fields), Value::Record(record)) => {
            let mut flat = Vec::new();
            for (name, sub) in fields {
                let field = record.fields.get(name).ok_or_else(|| {
                    EvalError::new(
                        "E0302",
                        format!("the diffusion of a record state has no field `{name}`"),
                    )
                })?;
                flat.extend(flatten_by_shape(sub, field)?);
            }
            Ok(flat)
        },
        | (StateShape::Record(_), other) => {
            Err(EvalError::new(
                "E0302",
                format!(
                    "the diffusion of a record state must be a record, found {}",
                    other.type_name()
                ),
            ))
        },
        | _ => flatten_state(value).map(|(_, flat)| flat),
    }
}

/// # Errors
///
/// `E0302` for a state that holds something other than numbers — an `Int`,
/// a `Bool` or a function cannot be integrated.
fn flatten_state(value: &Value) -> Result<(StateShape, Vec<f64>), EvalError> {
    match value {
        | Value::Num(num) => {
            let scalar = num.to_f64().ok_or_else(|| {
                EvalError::new("E0302", "an ODE state must be a concrete `Float`")
            })?;
            Ok((StateShape::Scalar(num.dim.clone()), vec![scalar]))
        },
        | Value::Tensor(tensor) => {
            Ok((
                StateShape::Tensor(tensor.shape.clone(), tensor.dim.clone()),
                tensor_values(tensor)?,
            ))
        },
        | Value::Record(record) => {
            let mut shapes = Vec::with_capacity(record.fields.len());
            let mut flat = Vec::new();
            for (name, field) in &record.fields {
                let (shape, values) = flatten_state(field)?;
                shapes.push((name.clone(), shape));
                flat.extend(values);
            }
            Ok((StateShape::Record(shapes), flat))
        },
        | other => {
            Err(EvalError::new(
                "E0302",
                format!(
                    "an ODE state cannot hold a value of type {} (§8.9)",
                    other.type_name()
                ),
            ))
        },
    }
}

/// Rebuilds a state from its flat form.
fn rebuild_state(
    shape: &StateShape,
    flat: &[f64],
) -> Value {
    rebuild_state_at(shape, &mut flat.iter().copied())
}

fn rebuild_state_at(
    shape: &StateShape,
    flat: &mut impl Iterator<Item = f64>,
) -> Value {
    match shape {
        | StateShape::Scalar(dim) => {
            Value::Num(NumVal::float(flat.next().unwrap_or(0.0), dim.clone()))
        },
        | StateShape::Tensor(shape, dim) => {
            let count = TensorData::element_count(shape);
            let values = flat
                .by_ref()
                .chain(std::iter::repeat(0.0))
                .take(count)
                .collect();
            float_tensor(shape.clone(), values, dim.clone())
        },
        | StateShape::Record(fields) => {
            let mut map = IndexMap::new();
            for (name, field) in fields {
                map.insert(name.clone(), rebuild_state_at(field, flat));
            }
            Value::Record(Rc::new(RecordValue { fields: map }))
        },
    }
}

/// The options of an `OdeOpts<s>` record (§8.9).
///
/// # Errors
///
/// `E0302` for an invalid method or event option.
fn ode_options(
    args: &[Value],
    index: usize,
    span: Span,
) -> Result<aevia_std::ode::Options, EvalError> {
    let mut options = aevia_std::ode::Options::default();
    let Some(Value::Record(record)) = args.get(index) else {
        return Ok(options);
    };
    if let Some(value) = record.fields.get("rtol") {
        if !crate::value::value_dim(value).is_one() {
            return Err(EvalError::new("E0401", "ODE `rtol` must be dimensionless").at(span));
        }
        options.rtol = value
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .ok_or_else(|| {
                EvalError::new("E0802", "ODE `rtol` must be a finite nonnegative number").at(span)
            })?;
    }
    if let Some(atol) = record.fields.get("atol").and_then(Value::as_f64) {
        options.atol = atol;
    }
    if let Some(value) = record.fields.get("steps") {
        let steps = match value {
            | Value::Variant(option) if option.adt == adt::OPTION_ID && option.name == "None" => {
                None
            },
            | Value::Variant(option) if option.adt == adt::OPTION_ID && option.name == "Some" => {
                let [steps] = option.fields.as_slice() else {
                    return Err(EvalError::new(
                        "E0302",
                        "`steps` must be an optional positive integer",
                    )
                    .at(span));
                };
                Some(steps)
            },
            | Value::Unit => None,
            | other => Some(other),
        };
        if let Some(steps) = steps {
            options.steps = host_count(steps)
                .filter(|steps| *steps > 0)
                .ok_or_else(|| {
                    EvalError::new("E0302", "`steps` must be a positive integer").at(span)
                })?;
        }
    }
    for (field, destination) in [
        ("h0", &mut options.h0),
        ("h_min", &mut options.h_min),
        ("h_max", &mut options.h_max),
    ] {
        if let Some(value) = record.fields.get(field) {
            *destination = value.as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
                EvalError::new("E0302", format!("`{field}` must be a finite time step")).at(span)
            })?;
            if crate::value::value_dim(value) != Dim::base("Time") {
                return Err(
                    EvalError::new("E0401", format!("`{field}` must have time dimension")).at(span),
                );
            }
        }
    }
    for (field, destination) in [
        ("max_steps", &mut options.max_steps),
        ("max_evaluations", &mut options.max_evaluations),
    ] {
        if let Some(value) = record.fields.get(field) {
            *destination = host_count(value)
                .filter(|steps| *steps > 0)
                .ok_or_else(|| {
                    EvalError::new("E0302", format!("`{field}` must be a positive integer"))
                        .at(span)
                })?;
        }
    }
    let _ = ode_event(args, index, span)?;
    if let Some(Value::Variant(method)) = record.fields.get("method") {
        options.method = ode_method(&method.name, span)?;
    }
    Ok(options)
}

/// Decode an ODE method and check its build-time capability.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps the method and capability policy separate from scalar tolerances"
    )
)]
fn ode_method(
    name: &str,
    span: Span,
) -> Result<aevia_std::ode::Method, EvalError> {
    let method = match name {
        | "Rk4" => aevia_std::ode::Method::Rk4,
        | "Dopri5" => aevia_std::ode::Method::Dopri5,
        | "Radau" => aevia_std::ode::Method::Radau,
        | "Bdf" => aevia_std::ode::Method::Bdf,
        | "Rosenbrock23" => aevia_std::ode::Method::Rosenbrock23,
        | "AutoSwitch" => aevia_std::ode::Method::Auto,
        | "StormerVerlet" => aevia_std::ode::Method::StormerVerlet,
        | "Ruth4" => aevia_std::ode::Method::Ruth4,
        | "GaussLegendre4" => aevia_std::ode::Method::GaussLegendre4,
        | "GaussLegendre6" => aevia_std::ode::Method::GaussLegendre6,
        | "ImplicitMidpoint" => aevia_std::ode::Method::ImplicitMidpoint,
        | "AdaptiveRk4" => aevia_std::ode::Method::AdaptiveRk4,
        | "AdaptiveStormerVerlet" => aevia_std::ode::Method::AdaptiveStormerVerlet,
        | "AdaptiveRuth4" => aevia_std::ode::Method::AdaptiveRuth4,
        | "AdaptiveImplicitMidpoint" => aevia_std::ode::Method::AdaptiveImplicitMidpoint,
        | "AdaptiveGaussLegendre4" => aevia_std::ode::Method::AdaptiveGaussLegendre4,
        | "AdaptiveGaussLegendre6" => aevia_std::ode::Method::AdaptiveGaussLegendre6,

        | other => {
            return Err(
                EvalError::new("E0302", format!("`{other}` is not an `OdeMethod`")).at(span),
            );
        },
    };
    if let Some(capability) = method.capability() {
        return Err(EvalError::new(
            "E1001",
            format!(
                "the `{name}` integrator needs the `{capability}` capability, which this \
                 build does not have; `Rk4`, `Dopri5` and `Radau` are available",
            ),
        )
        .at(span));
    }
    Ok(method)
}

fn ode_event(
    args: &[Value],
    index: usize,
    span: Span,
) -> Result<Option<Value>, EvalError> {
    let Some(Value::Record(record)) = args.get(index) else {
        return Ok(None);
    };
    match record.fields.get("events") {
        | None | Some(Value::Unit) => Ok(None),
        | Some(Value::Variant(option)) if option.adt == adt::OPTION_ID && option.name == "None" => {
            Ok(None)
        },
        | Some(Value::Variant(option))
            if option.adt == adt::OPTION_ID
                && option.name == "Some"
                && option.fields.len() == 1 =>
        {
            Ok(option.fields.first().cloned())
        },
        | _ => {
            Err(EvalError::new(
                "E0302",
                "`events` must be `Option<(Float[T], s) -> Float[1]>",
            )
            .at(span))
        },
    }
}

/// The grid of a PDE spec.
///
/// # Errors
///
/// `E0306` when the spec has no `grid` field with `nx`, `ny` and `h`.
fn grid_from(
    record: &RecordValue,
    name: &str,
    span: Span,
) -> Result<aevia_std::pde::Grid, EvalError> {
    let Some(Value::Record(grid)) = record.fields.get("grid") else {
        return Err(
            EvalError::new("E0306", format!("`{name}`: the spec needs a `grid` record")).at(span),
        );
    };
    let extent = |field: &str| -> Result<usize, EvalError> {
        grid.fields.get(field).and_then(host_count).ok_or_else(|| {
            EvalError::new(
                "E0306",
                format!("`{name}`: the grid needs an integer `{field}`"),
            )
            .at(span)
        })
    };
    Ok(aevia_std::pde::Grid {
        nx: extent("nx")?,
        ny: extent("ny")?,
        h: grid.fields.get("h").and_then(Value::as_f64).unwrap_or(1.0),
    })
}

/// A PDE field as a record of grid and values.
fn field_value(field: &aevia_std::pde::Field) -> Value {
    field_value_owned(field.clone())
}

fn field_value_owned(field: aevia_std::pde::Field) -> Value {
    record_of(vec![
        (
            "grid",
            record_of(vec![
                ("nx", Value::int(i64::try_from(field.grid.nx).unwrap_or(0))),
                ("ny", Value::int(i64::try_from(field.grid.ny).unwrap_or(0))),
                ("h", Value::float(field.grid.h)),
            ]),
        ),
        (
            "values",
            float_tensor(vec![field.grid.ny, field.grid.nx], field.values, Dim::one()),
        ),
    ])
}

/// A `PdeError` constructor value.
fn pde_error_value(error: &aevia_std::NumericError) -> Value {
    let name = match error {
        | aevia_std::NumericError::NotConverged => "NotConverged",
        | aevia_std::NumericError::ShapeMismatch(_) => "ShapeMismatch",
        | _ => "InvalidSpec",
    };
    let index = match name {
        | "NotConverged" => 0,
        | "ShapeMismatch" => 1,
        | _ => 2,
    };
    error_constructor("PdeError", name, index)
}

/// A `Table` value: a record of columns, units and rows (§8.11).
fn table_value(table: &aevia_std::table::Table) -> Value {
    let columns: Vec<Value> = table
        .columns
        .iter()
        .map(|column| Value::string(column.clone()))
        .collect();
    let units: Vec<Value> = table
        .units
        .iter()
        .map(|unit| {
            unit.as_ref()
                .map_or_else(|| Value::string(""), |unit| Value::string(unit.clone()))
        })
        .collect();
    let rows: Vec<Value> = (0..table.len()).map(|row| row_value(table, row)).collect();
    let (column_count, unit_count, row_count) = (columns.len(), units.len(), rows.len());
    record_of(vec![
        (
            "columns",
            build_tensor(columns, vec![column_count], Dim::one()),
        ),
        ("units", build_tensor(units, vec![unit_count], Dim::one())),
        ("rows", build_tensor(rows, vec![row_count], Dim::one())),
    ])
}

/// One row of a table as a record.
fn row_value(
    table: &aevia_std::table::Table,
    row: usize,
) -> Value {
    let mut map = IndexMap::new();
    for (index, column) in table.columns.iter().enumerate() {
        let cell = table
            .rows
            .get(row)
            .and_then(|cells| cells.get(index))
            .map_or("", String::as_str);
        let value = match cell.parse::<f64>() {
            | Ok(number) => Value::float(number),
            | Err(_) => Value::string(cell),
        };
        map.insert(column.clone(), value);
    }
    Value::Record(Rc::new(RecordValue { fields: map }))
}

/// The table a runtime value describes.
///
/// # Errors
///
/// `E0302` when the value is not a table record.
fn table_from(
    args: &[Value],
    index: usize,
    name: &str,
    span: Span,
) -> Result<aevia_std::table::Table, EvalError> {
    let Some(Value::Record(record)) = args.get(index) else {
        return Err(EvalError::new("E0302", format!("`{name}` expects a table")).at(span));
    };
    let string_column = |field: &str| -> Vec<String> {
        match record.fields.get(field) {
            | Some(Value::Tensor(tensor)) => {
                tensor
                    .elements()
                    .iter()
                    .map(|element| {
                        match element {
                            | Value::Str(text) => text.to_string(),
                            | other => crate::value::show(other),
                        }
                    })
                    .collect()
            },
            | _ => Vec::new(),
        }
    };
    let columns = string_column("columns");
    let units: Vec<Option<String>> = string_column("units")
        .into_iter()
        .map(|unit| {
            if unit.is_empty() {
                None
            } else {
                Some(unit)
            }
        })
        .collect();
    let mut rows = Vec::new();
    if let Some(Value::Tensor(tensor)) = record.fields.get("rows") {
        for element in tensor.elements() {
            let Value::Record(row) = element else {
                return Err(EvalError::new("E0302", "a table row must be a record").at(span));
            };
            rows.push(
                columns
                    .iter()
                    .map(|column| {
                        row.fields.get(column).map_or_else(String::new, |value| {
                            match value {
                                | Value::Str(text) => text.to_string(),
                                | other => crate::value::show(other),
                            }
                        })
                    })
                    .collect(),
            );
        }
    }
    Ok(aevia_std::table::Table { columns, units, rows })
}

/// A runtime value as JSON (§8.11).
///
/// A record is an object, a tensor an array, and a scalar is itself.
///
/// # Errors
///
/// `E0302` for a value JSON cannot express.
fn json_from_value(value: &Value) -> Result<aevia_std::table::Json, EvalError> {
    use aevia_std::table::Json;
    Ok(match value {
        | Value::Unit => Json::Null,
        | Value::Bool(flag) => Json::Bool(*flag),
        | Value::Str(text) => Json::String(text.to_string()),
        | Value::Num(num) => {
            match num.to_f64() {
                | Some(number) => Json::Number(number),
                | None => return Err(EvalError::new("E0302", "a symbolic value has no JSON form")),
            }
        },
        | Value::Tensor(tensor) => {
            Json::Array(
                tensor
                    .elements()
                    .iter()
                    .map(json_from_value)
                    .collect::<Result<_, _>>()?,
            )
        },
        | Value::Record(record) => {
            Json::Object(
                record
                    .fields
                    .iter()
                    .map(|(name, field)| Ok((name.clone(), json_from_value(field)?)))
                    .collect::<Result<_, EvalError>>()?,
            )
        },
        | Value::Variant(variant) if variant.adt == adt::OPTION_ID => {
            // §8.17: a record field typed `Option<Int>` (e.g. `seed`) must
            // cross the JSON boundary. `None` is JSON null, `Some(x)` is `x`;
            // anything else keeps the historical variant encoding below.
            if variant.index == 0 {
                Json::Null
            } else {
                match variant.fields.first() {
                    | Some(value) => json_from_value(value)?,
                    // A malformed `Some` has no payload; JSON null is the only
                    // honest encoding rather than inventing one.
                    | None => Json::Null,
                }
            }
        },
        | Value::Variant(variant) if variant.fields.is_empty() => {
            Json::String(variant.name.clone())
        },
        | other => {
            return Err(EvalError::new(
                "E0302",
                format!("a value of type {} has no JSON form", other.type_name()),
            ));
        },
    })
}

/// JSON as a runtime value.
fn value_from_json(json: &aevia_std::table::Json) -> Value {
    use aevia_std::table::Json;
    match json {
        | Json::Null => Value::Unit,
        | Json::Bool(flag) => Value::Bool(*flag),
        | Json::Number(number) => Value::float(*number),
        | Json::String(text) => Value::string(text.clone()),
        | Json::Array(items) => {
            let elements: Vec<Value> = items.iter().map(value_from_json).collect();
            let len = elements.len();
            build_tensor(elements, vec![len], Dim::one())
        },
        | Json::Object(fields) => {
            record_of(
                fields
                    .iter()
                    .map(|(name, value)| (name.as_str(), value_from_json(value)))
                    .collect(),
            )
        },
    }
}

/// A helper for tests: call a builtin with no interpreter side effects.
///
/// # Errors
///
/// Propagates the builtin's error.
pub fn call_simple(
    name: &str,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    let mut interp = Interp::new(&aevia_types::CheckedModule::default());
    call(&mut interp, name, args, Span::default())
}

// ----- shared helpers for the chapter-8 names ------------------------------

/// A Rust-implemented function value.
fn native<F>(function: F) -> Value
where
    F: Fn(&mut Interp, Vec<Value>, Span) -> Result<Value, EvalError> + 'static,
{
    Value::Native(Rc::new(NativeFn(Box::new(function))))
}

/// `Some(value)`.
fn some(value: Value) -> Value {
    make_variant(adt::OPTION_ID, 1, "Some", vec![value])
}

/// `None`.
fn none() -> Value {
    make_variant(adt::OPTION_ID, 0, "None", Vec::new())
}

/// `Ordering.Less` / `Ordering.Equal` / `Ordering.Greater`.
fn ordering_value(ordering: std::cmp::Ordering) -> Value {
    match ordering {
        | std::cmp::Ordering::Less => make_variant(adt::ORDERING_ID, 0, "Less", Vec::new()),
        | std::cmp::Ordering::Equal => make_variant(adt::ORDERING_ID, 1, "Equal", Vec::new()),
        | std::cmp::Ordering::Greater => make_variant(adt::ORDERING_ID, 2, "Greater", Vec::new()),
    }
}

/// A total order on runtime values, when one exists (§8.1.2).
///
/// Numbers compare numerically (exactly when both are exact), strings
/// lexicographically, booleans `false < true`; anything else has no `Ord`
/// instance, which is why `compare` returns an `Option`.
/// The total order `TotalOrd` promises (§3.8.4).
///
/// A pair with no structural answer — a `NaN`, or two values of different
/// shape — still has to compare, so the rendering is the last resort, exactly
/// as `total_compare` uses it. `NaN` sorts last because `"nan"` sorts after
/// every digit.
fn total_order(
    left: &Value,
    right: &Value,
) -> std::cmp::Ordering {
    compare_values(left, right)
        .unwrap_or_else(|| crate::value::repr(left).cmp(&crate::value::repr(right)))
}

/// The FNV-1a offset basis.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// The FNV-1a prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn mix(
    state: &mut u64,
    bytes: &[u8],
) {
    for byte in bytes {
        *state ^= u64::from(*byte);
        *state = state.wrapping_mul(FNV_PRIME);
    }
}

/// A structural hash, consistent with `Eq` (§3.8.4).
///
/// Equal values hash equally: `1` and `1.0` compare equal numerically, so both
/// hash their rational; a record hashes its labels in sorted order because
/// record equality is order-insensitive. Unequal values may collide — a hash
/// is not a decision procedure. `None` means the value has no `Hash` instance,
/// which is a function, a constructor or an IO action.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "owns the offset and result of the recursive structural hashing API"
    )
)]
fn hash_value(value: &Value) -> Option<u64> {
    let mut state = FNV_OFFSET;
    hash_into(value, &mut state)?;
    Some(state)
}

fn hash_into(
    value: &Value,
    state: &mut u64,
) -> Option<()> {
    match value {
        | Value::Unit => mix(state, b"u"),
        | Value::Bool(flag) => {
            mix(state, b"b");
            mix(state, &[u8::from(*flag)]);
        },
        | Value::Num(num) => hash_number(num, state)?,
        | Value::Str(text) => {
            mix(state, b"s");
            mix(state, text.as_bytes());
        },
        | Value::Sparse(sparse) => {
            mix(state, b"S");
            for extent in &sparse.shape {
                mix(state, &extent.to_le_bytes());
            }
            for (coord, element) in sparse.stored() {
                for c in coord {
                    mix(state, &c.to_le_bytes());
                }
                hash_into(&element, state)?;
            }
        },
        | Value::Tensor(tensor) => {
            mix(state, b"T");
            for extent in &tensor.shape {
                mix(state, &extent.to_le_bytes());
            }
            for element in tensor.elements() {
                hash_into(&element, state)?;
            }
        },
        | Value::Record(record) => {
            mix(state, b"R");
            let mut fields: Vec<_> = record.fields.iter().collect();
            fields.sort_unstable_by_key(|(name, _)| *name);
            for (name, value) in fields {
                mix(state, name.as_bytes());
                hash_into(value, state)?;
            }
        },
        | Value::Variant(variant) => {
            mix(state, b"V");
            mix(state, &variant.adt.to_le_bytes());
            mix(state, &(variant.index as u64).to_le_bytes());
            for field in &variant.fields {
                hash_into(field, state)?;
            }
        },
        | Value::UnitOf(unit) => {
            mix(state, b"U");
            mix(state, unit.name.as_bytes());
        },
        | Value::Prop(prop) => {
            mix(state, b"P");
            mix(state, prop.text.as_bytes());
        },
        | Value::Dyn(dynamic) => {
            mix(state, b"D");
            mix(state, dynamic.dim.to_transport_string().as_bytes());
            hash_into(&Value::Num(dynamic.payload()), state)?;
        },
        | Value::Closure(_)
        | Value::Native(_)
        | Value::Ctor { .. }
        | Value::Io(_)
        | Value::TypeRep(_)
        | Value::Callback(_)
        | Value::Task(_)
        | Value::KernelValue(_)
        | Value::KernelError(_)
        | Value::Error => {
            return None;
        },
    }
    Some(())
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "isolates numerical representations and equality-compatible encoding"
    )
)]
fn hash_number(
    num: &NumVal,
    state: &mut u64,
) -> Option<()> {
    mix(state, b"n");
    match &num.kind {
        | NumKind::Sym(_) => {
            mix(
                state,
                crate::value::repr(&Value::Num(num.clone())).as_bytes(),
            );
        },
        | _ => {
            match num.to_rational() {
                | Some(rational) => {
                    mix(state, b"r");
                    mix(state, rational.numer().to_signed_bytes_le().as_slice());
                    mix(state, b"/");
                    mix(state, rational.denom().to_signed_bytes_le().as_slice());
                },
                | None => {
                    match &num.kind {
                        | NumKind::Complex(re, im) => {
                            mix(state, b"c");
                            mix(state, &re.to_bits().to_le_bytes());
                            mix(state, &im.to_bits().to_le_bytes());
                        },
                        | NumKind::Float(float) => {
                            mix(state, b"f");
                            mix(state, &float.to_bits().to_le_bytes());
                        },
                        | _ => return None,
                    }
                },
            }
        },
    }
    Some(())
}

fn compare_values(
    left: &Value,
    right: &Value,
) -> Option<std::cmp::Ordering> {
    match (left, right) {
        | (Value::Num(a), Value::Num(b)) => {
            match (a.to_rational(), b.to_rational()) {
                | (Some(a), Some(b)) => Some(a.cmp(&b)),
                | _ => {
                    a.to_f64()
                        .zip(b.to_f64())
                        .and_then(|(a, b)| a.partial_cmp(&b))
                },
            }
        },
        | (Value::Str(a), Value::Str(b)) => Some(a.as_ref().cmp(b.as_ref())),
        | (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        | (Value::Unit, Value::Unit) => Some(std::cmp::Ordering::Equal),
        | (Value::Tensor(a), Value::Tensor(b)) => {
            if a.shape != b.shape {
                return Some(a.shape.cmp(&b.shape));
            }
            for index in 0..a.storage.len() {
                let (x, y) = (a.get_flat(index)?, b.get_flat(index)?);
                let ordering = compare_values(&x, &y)?;
                if ordering != std::cmp::Ordering::Equal {
                    return Some(ordering);
                }
            }
            Some(std::cmp::Ordering::Equal)
        },
        | (Value::Variant(a), Value::Variant(b)) => {
            let ordering = a.index.cmp(&b.index);
            if ordering != std::cmp::Ordering::Equal {
                return Some(ordering);
            }
            for (x, y) in a.fields.iter().zip(b.fields.iter()) {
                let ordering = compare_values(x, y)?;
                if ordering != std::cmp::Ordering::Equal {
                    return Some(ordering);
                }
            }
            Some(std::cmp::Ordering::Equal)
        },
        | _ => None,
    }
}

/// A symbolic proposition, the `Prop` of §3.8.6.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "owns the shared symbolic rendering and proposition construction policy"
    )
)]
fn proposition(
    op: BinOp,
    left: Value,
    right: Value,
) -> Value {
    let text = format!(
        "{} {} {}",
        crate::value::show(&left),
        match op {
            | BinOp::Eq => "==",
            | BinOp::Ne => "!=",
            | BinOp::Lt => "<",
            | BinOp::Le => "<=",
            | BinOp::Gt => ">",
            | BinOp::Ge => ">=",
            | _ => "?",
        },
        crate::value::show(&right)
    );
    Value::Prop(Rc::new(crate::value::PropValue {
        text,
        comparison: Some(crate::value::Comparison { op, left, right }),
    }))
}

/// A record from name/value pairs, keeping their order.
fn record_of(fields: Vec<(&str, Value)>) -> Value {
    let mut map = IndexMap::new();
    for (name, value) in fields {
        map.insert(name.to_string(), value);
    }
    Value::Record(Rc::new(RecordValue { fields: map }))
}

/// A tuple, which the runtime models as a record with the fields `0`, `1`, ….
fn tuple(items: Vec<Value>) -> Value {
    let mut map = IndexMap::new();
    for (index, item) in items.into_iter().enumerate() {
        map.insert(index.to_string(), item);
    }
    Value::Record(Rc::new(RecordValue { fields: map }))
}

/// The `norm` reduction over a slice, for both storage classes.
///
/// The arithmetic is the same expression the boxed loop used, so a dense
/// tensor and its boxed mirror produce the same bits.
fn dense_builtin_map(
    function: &Value,
    tensor: &TensorData,
    mode: aevia_rssn::execution::ExecutionMode,
) -> Option<Value> {
    const GRAIN: usize = 16_384;
    let Value::Ctor(ctor) = function else {
        return None;
    };
    if ctor.arity != usize::MAX || !ctor.args.is_empty() || tensor.display.is_some() {
        return None;
    }
    let values = tensor.dense_flat()?;
    if values.is_empty() {
        return None;
    }
    if ctor.name == "abs" {
        let out = aevia_rssn::par_map_f64_slice(&values, GRAIN, mode, f64::abs);
        return Some(float_tensor(tensor.shape.clone(), out, tensor.dim.clone()));
    }
    if !tensor.dim.is_one() {
        return None;
    }
    let op: fn(f64) -> f64 = match ctor.name.as_str() {
        | "sin" => f64::sin,
        | "cos" => f64::cos,
        | "tan" => f64::tan,
        | "sinh" => f64::sinh,
        | "cosh" => f64::cosh,
        | "tanh" => f64::tanh,
        | "atan" => f64::atan,
        | "exp" => f64::exp,
        | "cbrt" => f64::cbrt,
        | "sqrt" => {
            if !values.iter().all(|v| v.is_finite() && *v >= 0.0) {
                return None;
            }
            f64::sqrt
        },
        | "ln" => {
            if !values.iter().all(|v| v.is_finite() && *v > 0.0) {
                return None;
            }
            f64::ln
        },
        | "log10" => {
            if !values.iter().all(|v| v.is_finite() && *v > 0.0) {
                return None;
            }
            f64::log10
        },
        | "log2" => {
            if !values.iter().all(|v| v.is_finite() && *v > 0.0) {
                return None;
            }
            f64::log2
        },
        | _ => return None,
    };
    if !matches!(ctor.name.as_str(), "sqrt" | "ln" | "log10" | "log2")
        && !values.iter().all(|v| v.is_finite())
    {
        return None;
    }
    let out = aevia_rssn::par_map_f64_slice(&values, GRAIN, mode, op);
    Some(float_tensor(tensor.shape.clone(), out, Dim::one()))
}

fn norm_of_in_mode(
    values: &[f64],
    order: i64,
    mode: aevia_rssn::execution::ExecutionMode,
) -> f64 {
    match order {
        | 1 => aevia_rssn::simd::l1_norm_f64(values),
        | i64::MAX => aevia_rssn::simd::linf_norm_f64(values),
        | _ => aevia_rssn::par_norm2_f64_in_mode(values, mode),
    }
}

/// A tensor's elements as `f64`, borrowing contiguous data and materializing
/// strided or non-float storage at the kernel boundary.
fn tensor_entries(tensor: &TensorData) -> Result<Cow<'_, [f64]>, EvalError> {
    let count = tensor.storage.len();
    if tensor.checked_element_count() != Some(count) {
        return Err(EvalError::new(
            "E0802",
            "tensor shape disagrees with backing storage",
        ));
    }
    if let Some(data) = tensor.dense_flat() {
        return Ok(data);
    }
    let mut out = Vec::new();
    out.try_reserve_exact(count)
        .map_err(|_| EvalError::new("E0802", "numeric tensor is too large to materialize"))?;
    for index in 0..count {
        let element = tensor
            .get_flat(index)
            .ok_or_else(|| EvalError::new("E0802", "tensor view exceeds its backing storage"))?;
        let value = element.as_f64().ok_or_else(|| {
            EvalError::new(
                "E0302",
                format!(
                    "expected a numeric tensor, found an element of type {}",
                    element.type_name()
                ),
            )
        })?;
        out.push(value);
    }
    Ok(Cow::Owned(out))
}

/// A tensor's elements as an owned `f64` buffer.
fn tensor_values(tensor: &TensorData) -> Result<Vec<f64>, EvalError> {
    tensor_entries(tensor).map(Cow::into_owned)
}

/// A tensor of exact rationals, keeping the narrowest representation.
fn rational_tensor(
    shape: Vec<usize>,
    data: &[BigRational],
    dim: Dim,
) -> Value {
    let elements = data
        .iter()
        .map(|value| exact_number(value.clone(), dim.clone()))
        .collect();
    build_tensor(elements, shape, dim)
}

/// A tensor of `f64`.
fn float_tensor(
    shape: Vec<usize>,
    data: Vec<f64>,
    dim: Dim,
) -> Value {
    Value::Tensor(Rc::new(TensorData {
        verified_element: None,
        shape,
        storage: Storage::Dense(Rc::new(data)),
        dim,
        display: None,
    }))
}

/// A tensor of complex numbers, in boxed storage.
fn complex_tensor(
    shape: Vec<usize>,
    data: &[aevia_std::Complex],
    dim: Dim,
) -> Value {
    let elements = data
        .iter()
        .map(|value| {
            Value::Num(NumVal {
                kind: NumKind::Complex(value.re, value.im),
                dim: dim.clone(),
                display: None,
            })
        })
        .collect();
    build_tensor(elements, shape, dim)
}

/// A numeric kernel failure as a runtime diagnostic.
///
/// Appendix B §B.10 gives each recoverable failure its own code: a domain
/// error is `E0806`, a non-converging iteration is `E0807`, and a shape/index
/// refusal is `E0802`. Collapsing every non-domain failure into `E0802` made a
/// solver that ran out of iterations report a shape error, which sends the
/// reader looking in the wrong place.
#[expect(
    clippy::needless_pass_by_value,
    reason = "matches the owned error argument supplied by the numeric Result adapters"
)]
fn numeric_error(
    name: &str,
    error: aevia_std::NumericError,
    span: Span,
) -> EvalError {
    let code = match &error {
        | aevia_std::NumericError::Cancelled => crate::cancellation::INTERRUPTED,
        | aevia_std::NumericError::Domain(_) => "E0806",
        | aevia_std::NumericError::NotConverged => "E0807",
        | _ => "E0802",
    };
    EvalError::new(code, format!("`{name}`: {error}")).at(span)
}

/// A `LinAlgError`/`MathError` constructor value, for the `Err` arm.
fn error_constructor(
    type_name: &str,
    constructor: &str,
    index: usize,
) -> Value {
    let id = adt::intern(adt::Adt {
        name: type_name.to_string(),
        params: Vec::new(),
        ctors: Vec::new(),
    });
    make_variant(id, index, constructor, Vec::new())
}

/// The `Err` payload for a numeric kernel failure.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "translates the library error enum to the language constructor witness"
    )
)]
fn linalg_error_value(error: &aevia_std::NumericError) -> Value {
    let name = error.constructor();
    let index = match name {
        | "Singular" => 0,
        | "NotConverged" => 1,
        | "NotPositiveDefinite" => 2,
        | _ => 3,
    };
    error_constructor("LinAlgError", name, index)
}

/// Wraps a kernel result in `Result<_, LinAlgError>`.
fn lin_result(
    result: Result<Value, aevia_std::NumericError>,
    span: Span,
) -> Result<Value, EvalError> {
    match result {
        | Err(aevia_std::NumericError::Cancelled) => {
            Err(EvalError::new(crate::cancellation::INTERRUPTED, "evaluation interrupted").at(span))
        },
        | Ok(value) => Ok(ok(value)),
        | Err(error) => Ok(err(linalg_error_value(&error))),
    }
}

/// A tensor as `(rows, cols, row-major entries)`.
///
/// # Errors
///
/// `E0802` when the tensor is not a valid rank-2 matrix.
fn tensor_matrix<'a>(
    tensor: &'a TensorData,
    name: &str,
    span: Span,
) -> Result<(usize, usize, Cow<'a, [f64]>), EvalError> {
    let [rows, cols] = tensor.shape.as_slice() else {
        return Err(EvalError::new(
            "E0802",
            format!(
                "`{name}` expects a rank-2 tensor, found rank {}",
                tensor.rank()
            ),
        )
        .at(span));
    };
    let entries = tensor_entries(tensor).map_err(|error| error.at(span))?;
    Ok((*rows, *cols, entries))
}

/// A zero of the tensor's element representation and dimension.
fn zero_like(tensor: &TensorData) -> Value {
    match tensor.storage.backing() {
        | Storage::Dense(_) | Storage::View(_) => {
            Value::Num(NumVal::float(0.0, tensor.dim.clone()))
        },
        | Storage::Exact(_) | Storage::TypedExact(_, ExactRep::Real) => {
            Value::Num(NumVal::exact(BigRational::zero(), tensor.dim.clone()))
        },
        | Storage::TypedExact(_, ExactRep::Int) => {
            Value::Num(NumVal {
                kind: NumKind::Int(crate::int::Int::ZERO),
                dim: tensor.dim.clone(),
                display: None,
            })
        },
        | Storage::Bool(_) => Value::Bool(false),
        | Storage::Boxed(_) => Value::Num(NumVal::exact(BigRational::zero(), tensor.dim.clone())),
        | Storage::SharedSlice(_) => unreachable!("unwrapped storage"),
    }
}

/// The elements as exact rationals, when every one of them is `Int`/`Real`.
///
/// A `Float` is not exact even though it has a rational value: keeping a float
/// sample's mean as a rational would change its phase (§6.3.2).
fn exact_values(tensor: &TensorData) -> Option<Cow<'_, [BigRational]>> {
    let count = tensor.checked_element_count()?;
    if count != tensor.storage.len() {
        return None;
    }
    match &tensor.storage {
        | Storage::Exact(items) | Storage::TypedExact(items, ExactRep::Real) => {
            return Some(Cow::Borrowed(items));
        },
        | Storage::TypedExact(items, ExactRep::Int) => {
            return items
                .iter()
                .all(BigRational::is_integer)
                .then_some(Cow::Borrowed(items));
        },
        | Storage::Dense(_) | Storage::View(_) | Storage::Bool(_) => return None,
        | Storage::SharedSlice(_) | Storage::Boxed(_) => {},
    }
    let mut out = Vec::new();
    out.try_reserve_exact(count).ok()?;
    for index in 0..count {
        let element = tensor.get_flat(index)?;
        let num = element.as_num()?;
        match &num.kind {
            | NumKind::Int(value) => out.push(value.to_rational()),
            | NumKind::Exact(value) => out.push(value.as_ref().clone()),
            | _ => return None,
        }
    }
    Some(Cow::Owned(out))
}

/// Applies a scalar function to a number and returns the value.
///
/// A failure is recorded in `failure` instead of being returned, because the
/// numeric kernels take a plain `FnMut(f64) -> f64` and cannot carry a `Result`.
fn apply_scalar_value(
    interp: &mut Interp,
    function: &Value,
    x: f64,
    dim: &Dim,
    span: Span,
    failure: &mut Option<EvalError>,
) -> Value {
    let argument = Value::Num(NumVal::float(x, dim.clone()));
    match interp.apply(function.clone(), vec![argument], span) {
        | Ok(value) => value,
        | Err(error) => {
            if failure.is_none() {
                *failure = Some(error);
            }
            Value::Num(NumVal::float(f64::NAN, dim.clone()))
        },
    }
}

/// Applies a scalar function to a number and returns it as `f64`.
fn apply_scalar(
    interp: &mut Interp,
    function: &Value,
    x: f64,
    dim: &Dim,
    span: Span,
    failure: &mut Option<EvalError>,
) -> f64 {
    apply_scalar_value(interp, function, x, dim, span, failure)
        .as_f64()
        .unwrap_or(f64::NAN)
}

/// The value of a function of a vector, for its dimension.
fn apply_tensor_value(
    interp: &mut Interp,
    function: &Value,
    state: &[f64],
    template: &TensorData,
    span: Span,
    failure: &mut Option<EvalError>,
) -> Value {
    let argument = float_tensor(template.shape.clone(), state.to_vec(), template.dim.clone());
    match interp.apply(function.clone(), vec![argument], span) {
        | Ok(value) => value,
        | Err(error) => {
            if failure.is_none() {
                *failure = Some(error);
            }
            Value::float(f64::NAN)
        },
    }
}

/// Applies a function of a vector and returns its scalar value.
fn apply_tensor(
    interp: &mut Interp,
    function: &Value,
    state: &[f64],
    template: &TensorData,
    span: Span,
    failure: &mut Option<EvalError>,
) -> f64 {
    apply_tensor_value(interp, function, state, template, span, failure)
        .as_f64()
        .unwrap_or(f64::NAN)
}

/// Applies a function of a vector and returns its vector value.
fn apply_tensor_elements(
    interp: &mut Interp,
    function: &Value,
    state: &[f64],
    template: &TensorData,
    span: Span,
    failure: &mut Option<EvalError>,
) -> Vec<f64> {
    let value = apply_tensor_value(interp, function, state, template, span, failure);
    match &value {
        | Value::Tensor(tensor) => tensor_values(tensor).unwrap_or_default(),
        | Value::Num(num) => vec![num.to_f64().unwrap_or(f64::NAN)],
        | _ => Vec::new(),
    }
}

/// `format`'s template filling (§8.1.4): `{0}`, `{1}`, … name an argument by
/// position and `{}` takes the next one. Anything else in braces, and a
/// placeholder past the last argument, is kept as written. The template
/// usually comes from a raw string (`r"{} m"`) or a file, because `{…}` in an
/// ordinary literal is an interpolation.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps the template parser independently testable"
    )
)]
fn fill_template(
    template: &str,
    parts: &[String],
) -> String {
    let mut out = String::with_capacity(template.len());
    let mut next = 0usize;
    let mut rest = template;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("{{") {
            out.push('{');
            rest = after;
        } else if let Some(after) = rest.strip_prefix("}}") {
            out.push('}');
            rest = after;
        } else if let Some(after) = rest.strip_prefix('{') {
            let Some((inside, after)) = after.split_once('}') else {
                out.push_str(rest);
                break;
            };
            let chosen = if inside.is_empty() {
                let index = next;
                next = next.saturating_add(1);
                parts.get(index)
            } else {
                inside
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| parts.get(index))
            };
            if let Some(part) = chosen {
                out.push_str(part);
            } else {
                out.push('{');
                out.push_str(inside);
                out.push('}');
            }
            rest = after;
        } else if let Some(after) = rest.strip_prefix('}') {
            out.push('}');
            rest = after;
        } else {
            let next_brace = rest.find(['{', '}']).unwrap_or(rest.len());
            // `find` yields a character boundary, including for Unicode text.
            let (literal, after) = rest.split_at(next_brace);
            out.push_str(literal);
            rest = after;
        }
    }
    out
}

#[cfg(test)]
mod template_tests {
    use super::fill_template;

    #[test]
    fn placeholders_are_positional_or_sequential() {
        let parts = vec!["a".to_string(), "b".to_string()];
        assert_eq!(fill_template("{} + {}", &parts), "a + b");
        assert_eq!(fill_template("{1}{0}{1}", &parts), "bab");
        assert_eq!(fill_template("{x} {} {9}", &parts), "{x} a {9}");
        assert_eq!(fill_template("open { brace", &parts), "open { brace");
        assert_eq!(fill_template("λ } 終{}", &parts), "λ } 終a");
        assert_eq!(
            fill_template("escaped {{lit}} and {} and }}", &parts),
            "escaped {lit} and a and }"
        );
    }
}

/// The constructors of §8.7.1's `Distribution<d>`.
const DISTRIBUTION_CTORS: &[&str] = &[
    "Normal",
    "Uniform",
    "Exponential",
    "Gamma",
    "Beta",
    "ChiSquared",
    "StudentT",
    "Poisson",
    "Binomial",
    "LogNormal",
    "Weibull",
    "Cauchy",
    "F",
    "NegativeBinomial",
    "Geometric",
];

/// The parameter record of a distribution argument, and the constructor that
/// wrapped it.
///
/// §8.7.1's form is the nominal `Distribution<d>` (`Normal({ mu, sigma })`);
/// a bare parameter record is still accepted, and then the field names decide.
fn distribution_parts(value: Option<&Value>) -> Option<(&RecordValue, Option<&'static str>)> {
    match value? {
        | Value::Record(record) => Some((record.as_ref(), None)),
        | Value::Variant(variant) => {
            let ctor = DISTRIBUTION_CTORS
                .iter()
                .copied()
                .find(|ctor| *ctor == variant.name)?;
            let [Value::Record(record)] = variant.fields.as_slice() else {
                return None;
            };
            Some((record.as_ref(), Some(ctor)))
        },
        | _ => None,
    }
}

/// The distribution a `pdf`/`cdf`/`sample` argument describes (§8.7.1).
///
/// A `Distribution<d>` constructor names the distribution. A bare record is
/// recognised by its field names; `ChiSquared` and `StudentT` have the same
/// single field `df`, so a record with only that field is ambiguous and the
/// tie is broken towards `ChiSquared` (the first the specification lists)
/// unless a `kind` field says `"student_t"`.
#[expect(
    clippy::too_many_lines,
    reason = "keeps every named distribution and its parameter schema in a single match"
)]
fn distribution_from(
    args: &[Value],
    name: &str,
    span: Span,
) -> Result<aevia_std::dist::Distribution, EvalError> {
    use aevia_std::dist::Distribution;
    let Some((record, ctor)) = distribution_parts(args.first()) else {
        return Err(EvalError::new(
            "E0302",
            format!(
                "`{name}` expects a distribution, such as `Normal({{ mu = 0.0, sigma = 1.0 }})`"
            ),
        )
        .at(span));
    };
    let number = |field: &str| -> Result<f64, EvalError> {
        record
            .fields
            .get(field)
            .and_then(Value::as_f64)
            .ok_or_else(|| {
                EvalError::new(
                    "E0306",
                    format!("`{name}`: the distribution has no numeric field `{field}`"),
                )
                .at(span)
            })
    };
    let constructor = ctor
        .or_else(|| inferred_distribution(record))
        .ok_or_else(|| {
            EvalError::new(
                "E0306",
                format!("`{name}` cannot tell which distribution this record describes"),
            )
            .at(span)
        })?;
    let trials = || binomial_trials(record.fields.get("n"), ctor.is_some(), span);
    Ok(match constructor {
        | "Normal" => {
            Distribution::Normal {
                mu: number("mu")?,
                sigma: number("sigma")?,
            }
        },
        | "LogNormal" => {
            Distribution::LogNormal {
                mu: number("mu")?,
                sigma: number("sigma")?,
            }
        },
        | "Uniform" => {
            Distribution::Uniform {
                lo: number("lo")?,
                hi: number("hi")?,
            }
        },
        | "Exponential" => {
            Distribution::Exponential {
                rate: number("rate")?,
            }
        },
        | "Gamma" => {
            Distribution::Gamma {
                shape: number("shape")?,
                rate: number("rate")?,
            }
        },
        | "Beta" => {
            Distribution::Beta {
                alpha: number("alpha")?,
                beta: number("beta")?,
            }
        },
        | "ChiSquared" => Distribution::ChiSquared { df: number("df")? },
        | "StudentT" => Distribution::StudentT { df: number("df")? },
        | "Poisson" => {
            Distribution::Poisson {
                lambda: number("lambda")?,
            }
        },
        | "Binomial" => {
            Distribution::Binomial {
                n: trials()?,
                p: number("p")?,
            }
        },
        | "Weibull" => {
            Distribution::Weibull {
                shape: number("shape")?,
                scale: number("scale")?,
            }
        },
        | "Cauchy" => {
            Distribution::Cauchy {
                loc: number("loc")?,
                scale: number("scale")?,
            }
        },
        | "F" => {
            Distribution::F {
                df1: number("df1")?,
                df2: number("df2")?,
            }
        },
        | "NegativeBinomial" => {
            Distribution::NegativeBinomial {
                r: number("r")?,
                p: number("p")?,
            }
        },
        | "Geometric" => Distribution::Geometric { p: number("p")? },
        | _ => {
            return Err(EvalError::new("E0306", "unrecognized distribution constructor").at(span));
        },
    })
}

/// Resolve legacy records in their established order, including ambiguous df.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps legacy shape inference separate from parameter conversion"
    )
)]
fn inferred_distribution(record: &RecordValue) -> Option<&'static str> {
    let has = |field: &str| record.fields.contains_key(field);
    let kind = match record.fields.get("kind") {
        | Some(Value::Str(text)) => Some(text.as_ref()),
        | _ => None,
    };
    for (fields, constructor) in [
        (["shape", "scale"], "Weibull"),
        (["loc", "scale"], "Cauchy"),
        (["df1", "df2"], "F"),
        (["r", "p"], "NegativeBinomial"),
    ] {
        if fields.into_iter().all(has) {
            return Some(constructor);
        }
    }
    if has("p") && !has("n") {
        return Some("Geometric");
    }
    if has("mu") && has("sigma") {
        return Some(if kind == Some("log_normal") {
            "LogNormal"
        } else {
            "Normal"
        });
    }
    let schemas: &[(&[&str], &str)] = &[
        (&["lo", "hi"], "Uniform"),
        (&["shape", "rate"], "Gamma"),
        (&["rate"], "Exponential"),
        (&["alpha", "beta"], "Beta"),
        (&["lambda"], "Poisson"),
        (&["n", "p"], "Binomial"),
    ];
    for &(fields, constructor) in schemas {
        if fields.iter().copied().all(has) {
            return Some(constructor);
        }
    }
    has("df").then_some(if kind == Some("student_t") {
        "StudentT"
    } else {
        "ChiSquared"
    })
}

/// The nominal constructor requires Int. Legacy records accept concrete integral values.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps the two Binomial input contracts explicit"
    )
)]
fn binomial_trials(
    n: Option<&Value>,
    nominal: bool,
    span: Span,
) -> Result<u64, EvalError> {
    let trials = if nominal {
        let Some(Value::Num(n)) = n else {
            return Err(EvalError::new("E0306", "Binomial needs an integer field `n`").at(span));
        };
        let NumKind::Int(n) = &n.kind else {
            return Err(
                EvalError::new("E0306", "Binomial trials must be a nonnegative integer").at(span),
            );
        };
        n.to_u64()
    } else {
        n.and_then(nonnegative_integer)
    };
    trials.ok_or_else(|| {
        EvalError::new("E0306", "Binomial trials must fit a nonnegative u64").at(span)
    })
}

/// The dimension of a distribution's samples (§8.7.1).
fn distribution_dim(
    args: &[Value],
    span: Span,
) -> Result<Dim, EvalError> {
    let Some((record, ctor)) = distribution_parts(args.first()) else {
        return Err(EvalError::new("E0302", "expected a distribution").at(span));
    };
    if ctor == Some("LogNormal")
        || matches!(record.fields.get("kind"), Some(Value::Str(kind)) if kind.as_ref() == "log_normal")
    {
        return Ok(Dim::one());
    }
    if let Some(Value::Num(scale)) = record.fields.get("scale") {
        return Ok(scale.dim.clone());
    }
    // `Normal<d>` carries `d` on `mu`; `Exponential<d>` carries `1/d` on `rate`.
    if let Some(Value::Num(mu)) = record.fields.get("mu") {
        return Ok(mu.dim.clone());
    }
    if let Some(Value::Num(lo)) = record.fields.get("lo") {
        return Ok(lo.dim.clone());
    }
    if let Some(Value::Num(rate)) = record.fields.get("rate") {
        return Ok(rate.dim.inverse());
    }
    Ok(Dim::one())
}

/// The seed of a `sample_n` call (§8.7.1).
///
/// The third argument is `Option<Int>`. `Some(seed)` makes the sample
/// reproducible and leaves the session's RNG untouched; `None` draws from the
/// session's RNG. A direct dispatch has no `Option` value to pass and uses
/// `()`, which is read as `None` so the runtime entry point stays callable.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "decodes the optional seed without consuming the session RNG"
    )
)]
fn sample_seed(
    args: &[Value],
    span: Span,
) -> Result<Option<u64>, EvalError> {
    let Some(argument) = args.get(2) else {
        return Ok(None);
    };
    match argument {
        | Value::Unit => Ok(None),
        | Value::Variant(option) if option.adt == adt::OPTION_ID => {
            if option.name != "Some" {
                return Ok(None);
            }
            let Some(Value::Num(seed)) = option.fields.first() else {
                return Err(
                    EvalError::new("E0302", "the seed of `sample_n` must be an Int").at(span),
                );
            };
            match &seed.kind {
                | NumKind::Int(value) => {
                    value.to_u64().map(Some).ok_or_else(|| {
                        EvalError::new("E0302", "the seed of `sample_n` is out of range").at(span)
                    })
                },
                | _ => {
                    Err(EvalError::new("E0302", "the seed of `sample_n` must be an Int").at(span))
                },
            }
        },
        | _ => {
            Err(EvalError::new("E0302", "`sample_n` expects `Option<Int>` as its seed").at(span))
        },
    }
}

/// The exact argument of a classical polynomial, when it has one.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps exact and approximate polynomial dispatch in one place"
    )
)]
fn classical_polynomial(
    args: &[Value],
    name: &str,
    span: Span,
) -> Result<Value, EvalError> {
    let order = expect_int(args, 0, name, span)?;
    let num = expect_num(args, 1, name, span)?;
    let invalid_order = || {
        EvalError::new(
            "E0802",
            format!("`{name}`: the polynomial order is out of range"),
        )
        .at(span)
    };
    let order = u32::try_from(order.max(0)).map_err(|_| invalid_order())?;
    let function = match name {
        | "legendre_p" => aevia_std::special::legendre_p_exact,
        | "hermite_h" => aevia_std::special::hermite_h_exact,
        | "laguerre_l" => aevia_std::special::laguerre_l_exact,
        | "chebyshev_t" => aevia_std::special::chebyshev_t_exact,
        | _ => aevia_std::special::chebyshev_u_exact,
    };
    // An exact argument stays exact: these are polynomials with rational
    // coefficients, so `legendre_p(2, 1/2)` is `-1/8` (§8.2.2 keeps ρ).
    if let Some(rational) = num.to_rational() {
        return Ok(Value::Num(NumVal::exact(
            function(order, &rational),
            num.dim,
        )));
    }
    let x = num.to_f64().unwrap_or(f64::NAN);
    let order = i32::try_from(order).map_err(|_| invalid_order())?;
    let value = match name {
        | "legendre_p" => aevia_std::special::legendre_p(order, x),
        | "hermite_h" => aevia_std::special::hermite_h(order, x),
        | "laguerre_l" => aevia_std::special::laguerre_l(order, x),
        | "chebyshev_t" => aevia_std::special::chebyshev_t(order, x),
        | _ => aevia_std::special::chebyshev_u(order, x),
    };
    Ok(Value::Num(NumVal::float(value, num.dim)))
}

/// `join` on two tables, keyed by a column name (§8.11).
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps join column, unit, and row invariants together"
    )
)]
fn join_tables(
    args: &[Value],
    name: &str,
    span: Span,
) -> Result<Value, EvalError> {
    use std::collections::HashMap;
    use std::collections::HashSet;

    let left = table_from(args, 0, name, span)?;
    let right = table_from(args, 1, name, span)?;
    let key = expect_str(args, 2, name, span)?;
    let invalid = |message| EvalError::new("E0903", message).at(span);
    let key_index = |columns: &[String], side| {
        columns
            .iter()
            .position(|column| *column == key)
            .ok_or_else(|| invalid(format!("the {side} table has no column `{key}`")))
    };
    let left_index = key_index(&left.columns, "left")?;
    let right_index = key_index(&right.columns, "right")?;
    let mut names = HashSet::with_capacity(left.columns.len());
    for column in &left.columns {
        if !names.insert(column.as_str()) {
            return Err(invalid(format!("ambiguous join column `{column}`")));
        }
    }
    let mut columns = left.columns.clone();
    let mut units = left.units.clone();
    for (index, column) in right.columns.iter().enumerate() {
        if index == right_index {
            continue;
        }
        if !names.insert(column.as_str()) {
            return Err(invalid(format!("ambiguous join column `{column}`")));
        }
        let unit = right
            .units
            .get(index)
            .ok_or_else(|| invalid(format!("the right table has no unit entry for `{column}`")))?;
        columns.push(column.clone());
        units.push(unit.clone());
    }
    let mut by_key: HashMap<&str, Vec<&[String]>> = HashMap::new();
    for row in &right.rows {
        if let Some(value) = row.get(right_index) {
            by_key.entry(value.as_str()).or_default().push(row);
        }
    }
    let mut rows = Vec::new();
    for left_row in &left.rows {
        let Some(matches) = left_row
            .get(left_index)
            .and_then(|value| by_key.get(value.as_str()))
        else {
            continue;
        };
        rows.try_reserve(matches.len()).map_err(|_| {
            EvalError::new("E0802", "join result is too large to allocate").at(span)
        })?;
        for right_row in matches {
            let mut merged = left_row.clone();
            merged.extend(right_row.iter().enumerate().filter_map(|(index, cell)| {
                if index == right_index {
                    None
                } else {
                    Some(cell.clone())
                }
            }));
            rows.push(merged);
        }
    }
    Ok(table_value(&aevia_std::table::Table {
        columns,
        units,
        rows,
    }))
}

#[cfg(test)]
mod tests {
    use super::json_from_value;
    use super::numeric_error;
    use crate::value::Value;
    use crate::value::make_variant;
    use aevia_std::table::Json;
    use aevia_syntax::Span;
    use aevia_types::adt;

    #[test]
    fn numeric_controls_keep_exact_counts_and_reject_nonintegers() {
        let exact = 9_007_199_254_740_993_i64;
        let value = Value::int(exact);
        assert_eq!(super::host_count(&value), usize::try_from(exact).ok());
        assert_eq!(
            super::nonnegative_integer(&value),
            u64::try_from(exact).ok()
        );
        assert_eq!(super::exact_seed(&Value::int(-1)), Some(u64::MAX));
        for invalid in [
            Value::float(3.5),
            Value::float(f64::INFINITY),
            Value::float(f64::NAN),
            Value::int(-1),
        ] {
            assert_eq!(super::host_count(&invalid), None);
        }
    }

    #[test]
    fn legacy_binomial_records_keep_exact_trials() {
        let exact = 9_007_199_254_740_993_i64;
        let record = |n| super::record_of(vec![("n", n), ("p", Value::float(0.5))]);
        let result = super::distribution_from(&[record(Value::int(exact))], "pdf", Span::default());
        assert!(
            matches!(result, Ok(aevia_std::dist::Distribution::Binomial { n, .. }) if Some(n) == u64::try_from(exact).ok())
        );
        let bad = super::distribution_from(&[record(Value::float(2.5))], "pdf", Span::default());
        assert_eq!(bad.unwrap_err().code, "E0306");
    }

    /// §6.9.2 / appendix B §B.10: each recoverable numeric failure has its own
    /// code. A non-converging iteration is `E0807`, not the shape/index
    /// `E0802` it used to share with every non-domain failure.
    #[test]
    fn numeric_kernel_failures_keep_their_documented_codes() {
        let code = |error| numeric_error("probe", error, Span::default()).code;
        assert_eq!(code(aevia_std::NumericError::Cancelled), "E0812");
        assert_eq!(
            super::lin_result(Err(aevia_std::NumericError::Cancelled), Span::default())
                .unwrap_err()
                .code,
            "E0812"
        );
        assert_eq!(code(aevia_std::NumericError::NotConverged), "E0807");
        assert_eq!(code(aevia_std::NumericError::Domain("x".into())), "E0806");
        assert_eq!(
            code(aevia_std::NumericError::ShapeMismatch("x".into())),
            "E0802"
        );
        assert_eq!(code(aevia_std::NumericError::NotSquare), "E0802");
        assert_eq!(code(aevia_std::NumericError::Empty), "E0802");
    }

    /// §8.17: an `Option` field crossing the JSON boundary is `null` for
    /// `None` and the payload for `Some`, so `Physics.*` wrappers with an
    /// optional field can reach `simulate`.
    #[test]
    fn an_option_field_encodes_as_null_or_its_payload() {
        let none = make_variant(adt::OPTION_ID, 0, "None", Vec::new());
        assert!(matches!(
            json_from_value(&none).expect("none encodes"),
            Json::Null
        ));
        let some = make_variant(adt::OPTION_ID, 1, "Some", vec![Value::int(7)]);
        assert!(
            matches!(
                json_from_value(&some).expect("some encodes"),
                Json::Number(number) if number == 7.0
            ),
            "Some(x) encodes as x"
        );
    }
}

/// An algebraic normal form computed by the kernel's polynomial rule set
/// (§8.5.1): `expand`, `factor`, `cancel`, `together`, `apart(e, x)`,
/// `collect(e, x)` and `degree(e, x)`.
///
/// The rewrite keeps the value, so the result has the first argument's
/// dimension. A form the kernel cannot produce leaves the request unevaluated,
/// which is reported as `E0808` rather than returned as an answer.
fn algebra(
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let expected = if matches!(name, "apart" | "collect" | "degree") {
        2
    } else {
        1
    };
    let Some(first) = args.first().filter(|_| args.len() == expected) else {
        return Err(EvalError::new(
            "E0315",
            format!("`{name}` takes {expected} argument(s), got {}", args.len()),
        )
        .at(span));
    };
    let dim = if name == "degree" {
        Dim::one()
    } else {
        crate::value::value_dim(first)
    };
    let mut terms = Vec::with_capacity(args.len());
    for arg in args {
        let term = value_to_term(arg)?;
        terms.push(Value::Num(NumVal::symbolic(term, Dim::one())));
    }
    // A direct operator call, not a `compute` request: the forms are equal by
    // construction, and the symbolic extractor would hand back the smallest
    // equal term — `x^4 - 1` instead of its factorization.
    let term = bridge::call_term(name, &terms, dim.clone()).map_err(|error| error.at(span))?;
    if term.to_string().starts_with(&format!("{name}(")) {
        return Err(EvalError::new(
            "E0808",
            format!("the kernel has no `{name}` form for this expression"),
        )
        .at(span));
    }
    Ok(bridge::fold(&term, dim.clone()).unwrap_or_else(|| Value::Num(NumVal::symbolic(term, dim))))
}

/// A set from arbitrary elements: sorted by `total_compare`, duplicates
/// removed (§8.1.9). The canonical form is what makes structural equality of
/// two sets their set equality.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "canonicalizes arbitrary values separately from already ordered set results"
    )
)]
fn make_set(mut items: Vec<Value>) -> Value {
    items.sort_by(total_order);
    items.dedup_by(|a, b| total_order(a, b) == std::cmp::Ordering::Equal);
    set_value(items)
}

/// A set from elements already in canonical order.
fn set_value(items: Vec<Value>) -> Value {
    let dim = items.first().map_or_else(Dim::one, crate::value::value_dim);
    let len = items.len();
    let elements = build_tensor(items, vec![len], dim);
    crate::value::make_variant(aevia_types::adt::set_id(), 0, "Set", vec![elements])
}

/// The canonical elements of a set value.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "borrows canonical boxed set elements without materializing dense storage"
    )
)]
pub(crate) fn set_items(value: &Value) -> Option<&[Value]> {
    let Value::Variant(variant) = value else {
        return None;
    };
    if variant.adt != aevia_types::adt::set_id() {
        return None;
    }
    match variant.fields.first() {
        | Some(Value::Tensor(tensor)) => {
            match &tensor.storage {
                | Storage::Boxed(items) => Some(items.as_slice()),
                | _ => None,
            }
        },
        | _ => None,
    }
}

fn expect_set<'a>(
    args: &'a [Value],
    index: usize,
    name: &str,
    span: Span,
) -> Result<std::borrow::Cow<'a, [Value]>, EvalError> {
    let value = args
        .get(index)
        .ok_or_else(|| arity(name, index.saturating_add(1), args.len(), span))?;
    if let Some(items) = set_items(value) {
        return Ok(std::borrow::Cow::Borrowed(items));
    }
    // A set whose elements were stored densely (all `Float`/`Int` scalars).
    if let Value::Variant(variant) = value
        && variant.adt == aevia_types::adt::set_id()
        && let Some(Value::Tensor(tensor)) = variant.fields.first()
    {
        return Ok(std::borrow::Cow::Owned(tensor.elements()));
    }
    Err(EvalError::new(
        "E0302",
        format!("`{name}` expects a set, found {}", value.type_name()),
    )
    .at(span))
}

/// `approx` of a closed term by direct `f64` evaluation, when it is finite.
fn direct_approx(
    term: &crate::sym::Term,
    dim: Dim,
) -> Option<Value> {
    let value = term.eval(&std::collections::HashMap::new()).ok()?;
    value
        .is_finite()
        .then(|| ok(Value::Num(NumVal::float(value, dim))))
}

#[cfg(test)]
mod dense_and_execution_upgrade_tests {
    use super::*;

    fn float_val(x: f64) -> Value {
        Value::Num(NumVal::float(x, Dim::one()))
    }

    #[test]
    fn normalize_dense_and_view_paths_match_boxed_reference() {
        let checked = aevia_types::CheckedModule::default();
        let mut interp = Interp::new(&checked);
        let span = Span::default();

        let dense = Value::Tensor(Rc::new(TensorData::new(
            vec![3],
            Storage::Dense(Rc::new(vec![3.0, 4.0, 12.0])),
            Dim::one(),
            None,
        )));
        let boxed = Value::Tensor(Rc::new(TensorData::new(
            vec![3],
            Storage::Boxed(vec![float_val(3.0), float_val(4.0), float_val(12.0)]),
            Dim::one(),
            None,
        )));
        let norm_dense = call(&mut interp, "normalize", vec![dense], span).unwrap();
        let norm_boxed = call(&mut interp, "normalize", vec![boxed], span).unwrap();
        let Value::Tensor(nd) = norm_dense else {
            panic!("expected tensor");
        };
        let Value::Tensor(nb) = norm_boxed else {
            panic!("expected tensor");
        };
        assert_eq!(nd.dense(), nb.dense());

        let matrix = TensorData::new(
            vec![2, 2],
            Storage::Dense(Rc::new(vec![1.0, 2.0, 3.0, 4.0])),
            Dim::one(),
            None,
        );
        let view = Value::Tensor(Rc::new(matrix.transposed_view().unwrap()));
        let Value::Tensor(norm_view) = call(&mut interp, "normalize", vec![view], span).unwrap()
        else {
            panic!("expected tensor");
        };
        assert_eq!(norm_view.shape, [2, 2]);
        assert!(matches!(norm_view.storage, Storage::Dense(_)));

        let empty = Value::Tensor(Rc::new(TensorData::new(
            vec![0],
            Storage::Dense(Rc::new(Vec::new())),
            Dim::one(),
            None,
        )));
        let Value::Tensor(norm_empty) = call(&mut interp, "normalize", vec![empty], span).unwrap()
        else {
            panic!("expected tensor");
        };
        assert_eq!(norm_empty.dense(), Some([].as_slice()));
    }

    #[test]
    fn flatten_and_centroid_preserve_dense_storage_and_values() {
        let checked = aevia_types::CheckedModule::default();
        let mut interp = Interp::new(&checked);
        let span = Span::default();

        let buf = Rc::new(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let matrix = Value::Tensor(Rc::new(TensorData::new(
            vec![2, 3],
            Storage::Dense(Rc::clone(&buf)),
            Dim::one(),
            None,
        )));
        let Value::Tensor(flat) = call(&mut interp, "flatten", vec![matrix.clone()], span).unwrap()
        else {
            panic!("expected tensor");
        };
        assert_eq!(flat.shape, [6]);
        let Storage::Dense(flat_buf) = &flat.storage else {
            panic!("expected shared Dense buffer");
        };
        assert!(Rc::ptr_eq(&buf, flat_buf));

        let cent_dense = call(&mut interp, "centroid", vec![matrix], span).unwrap();
        let boxed = Value::Tensor(Rc::new(TensorData::new(
            vec![2, 3],
            Storage::Boxed((1..=6).map(|i| float_val(f64::from(i))).collect()),
            Dim::one(),
            None,
        )));
        let cent_boxed = call(&mut interp, "centroid", vec![boxed], span).unwrap();
        assert_eq!(cent_dense.as_f64(), cent_boxed.as_f64());
        assert_eq!(cent_dense.as_f64(), Some(3.5));
    }

    #[test]
    fn cdf_batch_matches_under_serial_and_parallel_job_budgets() {
        let checked = aevia_types::CheckedModule::default();
        let mut interp = Interp::new(&checked);
        let span = Span::default();
        let mut fields = IndexMap::new();
        fields.insert("mu".to_string(), float_val(0.0));
        fields.insert("sigma".to_string(), float_val(1.0));
        let dist = Value::Record(Rc::new(RecordValue { fields }));
        let points = Value::Tensor(Rc::new(TensorData::new(
            vec![5000],
            Storage::Dense(Rc::new(
                (0..5000).map(|i| (f64::from(i) - 2500.0) * 0.002).collect(),
            )),
            Dim::one(),
            None,
        )));

        interp.set_jobs(1);
        let Value::Tensor(serial) = call(
            &mut interp,
            "cdf_batch",
            vec![dist.clone(), points.clone()],
            span,
        )
        .unwrap() else {
            panic!("expected tensor");
        };
        interp.set_jobs(2);
        let Value::Tensor(parallel) =
            call(&mut interp, "cdf_batch", vec![dist, points], span).unwrap()
        else {
            panic!("expected tensor");
        };
        assert_eq!(serial.dense(), parallel.dense());
    }
}
