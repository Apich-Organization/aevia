//! Built-in functions.
//!
//! Dispatch is by name, which keeps values cheap to clone and lets a builtin be
//! passed as a value (`xs |> map(f)`). The type checker owns the signatures;
//! this module owns the behaviour, and the two lists are kept in step by the
//! conformance tests.

use crate::eval::EvalError;
use crate::eval::Interp;
use crate::eval::build_tensor;
use crate::eval::type_argument_name;
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
use std::rc::Rc;

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
    let element = types
        .first()
        .map_or_else(|| "Float".to_string(), type_argument_name);
    match name {
        | "zeros" | "ones" => {
            let mut shape = Vec::with_capacity(args.len());
            for index in 0..args.len() {
                shape.push(expect_int(&args, index, name, span)?.max(0) as usize);
            }
            let count = checked_element_count(&shape, span)?;
            let one = name == "ones";
            let storage = match element.as_str() {
                | "Int" | "Real" => {
                    Storage::TypedExact(
                        vec![
                            if one {
                                BigRational::one()
                            } else {
                                BigRational::zero()
                            };
                            count
                        ],
                        if element == "Int" {
                            ExactRep::Int
                        } else {
                            ExactRep::Real
                        },
                    )
                },
                | "Float" => Storage::Dense(Rc::new(vec![if one { 1.0 } else { 0.0 }; count])),
                | other => {
                    return Err(EvalError::new(
                        "E1001",
                        format!(
                            "`{name}<{other}>` needs a representation this runtime cannot store"
                        ),
                    )
                    .at(span));
                },
            };
            Ok(Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape,
                storage,
                dim: Dim::one(),
                display: None,
            })))
        },
        | "zeros_of" | "ones_of" => {
            let Some(shape) = types.first().and_then(shape_of_type_argument) else {
                return Err(EvalError::new(
                    "E0310",
                    format!("`{name}` needs a concrete shape, as in `{name}<[3, 2]>()`"),
                )
                .at(span));
            };
            let count = checked_element_count(&shape, span)?;
            let fill = if name == "ones_of" {
                1.0
            } else {
                0.0
            };
            Ok(Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape,
                storage: Storage::Dense(Rc::new(vec![fill; count])),
                dim: Dim::one(),
                display: None,
            })))
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
            if TensorData::element_count(&shape) != tensor.storage.len() {
                return Ok(err(shape_error(format!(
                    "cannot reshape {} element(s) into {:?}",
                    tensor.storage.len(),
                    shape
                ))));
            }
            // A view's strides describe the shape it was sliced from, so they
            // cannot survive a change of shape; it is materialised.
            let Some(storage) = tensor.reshape_storage(&shape) else {
                return Ok(err(shape_error(
                    "this tensor's elements cannot be reshaped".to_string(),
                )));
            };
            let result = Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape,
                storage,
                dim: tensor.dim.clone(),
                display: tensor.display.clone(),
            }));
            Ok(ok(crate::host_witness::preserve_tensor_schema(
                result, &tensor,
            )))
        },
        | _ => call(interp, name, args, span),
    }
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
        | NatAst::Add(left, right) => Some(nat_value(left)? + nat_value(right)?),
        | NatAst::Mul(left, right) => Some(nat_value(left)? * nat_value(right)?),
        | NatAst::Var(_) => None,
    }
}

/// Calls a builtin.
///
/// # Errors
///
/// Returns `E1001` for names that are typed but not implemented, and the
/// builtin's own runtime errors otherwise.
pub fn call(
    interp: &mut Interp,
    name: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, EvalError> {
    match name {
        // ----- output -----------------------------------------------------
        | "print" => io_print(interp, &args, false, false, span),
        | "println" => io_print(interp, &args, true, false, span),
        | "eprintln" => io_print(interp, &args, true, true, span),
        // §6.12.3: four renderers, four jobs. `repr` is round-trippable and
        // `to_latex`/`to_typst` emit their own syntax; aliasing them to `show`
        // is what let every test believe `3`, `3/1` and `3.0` were one value.
        | "show" => {
            let value = args.first().ok_or_else(|| arity(name, 1, 0, span))?;
            Ok(Value::string(crate::value::show(value)))
        },
        | "repr" => {
            let value = args.first().ok_or_else(|| arity(name, 1, 0, span))?;
            Ok(Value::string(crate::value::repr(value)))
        },
        | "to_latex" | "to_typst" => {
            let value = args.first().ok_or_else(|| arity(name, 1, 0, span))?;
            let syntax = if name == "to_latex" {
                crate::value::MathSyntax::Latex
            } else {
                crate::value::MathSyntax::Typst
            };
            crate::value::to_math(value, syntax)
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
        | "sqrt" => unary_math(interp, name, &args, span, MathKind::Root(2)),
        | "cbrt" => unary_math(interp, name, &args, span, MathKind::Root(3)),
        | "exp" => unary_math(interp, name, &args, span, MathKind::Float(f64::exp)),
        | "ln" | "log" => unary_math(interp, name, &args, span, MathKind::Float(f64::ln)),
        | "log2" => unary_math(interp, name, &args, span, MathKind::Float(f64::log2)),
        | "log10" => unary_math(interp, name, &args, span, MathKind::Float(f64::log10)),
        | "sin" => unary_math(interp, name, &args, span, MathKind::Float(f64::sin)),
        | "cos" => unary_math(interp, name, &args, span, MathKind::Float(f64::cos)),
        | "tan" => unary_math(interp, name, &args, span, MathKind::Float(f64::tan)),
        | "asin" => unary_math(interp, name, &args, span, MathKind::Float(f64::asin)),
        | "acos" => unary_math(interp, name, &args, span, MathKind::Float(f64::acos)),
        | "atan" => unary_math(interp, name, &args, span, MathKind::Float(f64::atan)),
        | "sinh" => unary_math(interp, name, &args, span, MathKind::Float(f64::sinh)),
        | "cosh" => unary_math(interp, name, &args, span, MathKind::Float(f64::cosh)),
        | "tanh" => unary_math(interp, name, &args, span, MathKind::Float(f64::tanh)),
        | "asinh" => unary_math(interp, name, &args, span, MathKind::Float(f64::asinh)),
        | "acosh" => unary_math(interp, name, &args, span, MathKind::Float(f64::acosh)),
        | "atanh" => unary_math(interp, name, &args, span, MathKind::Float(f64::atanh)),
        | "gamma" => gamma(interp, name, &args, span),
        | "erf" => unary_math(interp, name, &args, span, MathKind::Float(erf)),
        | "erfc" => unary_math(interp, name, &args, span, MathKind::Float(|x| 1.0 - erf(x))),
        | "atan2" => {
            let y = expect_num(&args, 0, name, span)?;
            let x = expect_num(&args, 1, name, span)?;
            Ok(Value::Num(NumVal::float(
                y.to_f64()
                    .unwrap_or(f64::NAN)
                    .atan2(x.to_f64().unwrap_or(f64::NAN)),
                Dim::one(),
            )))
        },
        | "hypot" => {
            let a = expect_num(&args, 0, name, span)?;
            let b = expect_num(&args, 1, name, span)?;
            Ok(Value::Num(NumVal::float(
                a.to_f64()
                    .unwrap_or(f64::NAN)
                    .hypot(b.to_f64().unwrap_or(f64::NAN)),
                a.dim.clone(),
            )))
        },

        // ----- numeric ----------------------------------------------------
        | "abs" => {
            map_scalar(&args, name, span, |num| {
                let kind = match num.kind {
                    | NumKind::Int(value) => NumKind::Int(value.abs()),
                    | NumKind::Exact(value) => NumKind::Exact(value.abs()),
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
        | "sign" => {
            map_scalar(&args, name, span, |num| {
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
        | "min" | "max" => {
            let a = expect_num(&args, 0, name, span)?;
            let b = expect_num(&args, 1, name, span)?;
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
        | "clamp" => {
            let x = expect_num(&args, 0, name, span)?;
            let lo = expect_num(&args, 1, name, span)?;
            let hi = expect_num(&args, 2, name, span)?;
            let value = x.to_f64().unwrap_or(f64::NAN);
            let low = lo.to_f64().unwrap_or(f64::NAN);
            let high = hi.to_f64().unwrap_or(f64::NAN);
            let clamped = value.max(low).min(high);
            Ok(Value::Num(NumVal::float(clamped, x.dim)))
        },
        | "floor" | "ceil" | "round" | "trunc" => {
            map_scalar(&args, name, span, |num| {
                let value = num.to_f64().unwrap_or(f64::NAN);
                let rounded = match name {
                    | "floor" => value.floor(),
                    | "ceil" => value.ceil(),
                    | "round" => value.round(),
                    | _ => value.trunc(),
                };
                NumVal::float(rounded, num.dim)
            })
        },
        | "mod" | "rem" => {
            let a = expect_num(&args, 0, name, span)?;
            let b = expect_num(&args, 1, name, span)?;
            if b.is_zero() {
                return Err(EvalError::new("E0801", "modulo by zero").at(span));
            }
            let x = a.to_f64().unwrap_or(f64::NAN);
            let y = b.to_f64().unwrap_or(f64::NAN);
            let value = if name == "mod" {
                x.rem_euclid(y)
            } else {
                x % y
            };
            Ok(Value::Num(NumVal::float(value, a.dim)))
        },
        | "div" | "quot" => {
            let a = expect_int(&args, 0, name, span)?;
            let b = expect_int(&args, 1, name, span)?;
            if b == 0 {
                return Err(EvalError::new("E0801", "division by zero").at(span));
            }
            let value = if name == "div" {
                a.div_euclid(b)
            } else {
                a / b
            };
            Ok(Value::Num(NumVal::int(BigInt::from(value))))
        },
        | "gcd" | "lcm" => {
            let a = expect_int(&args, 0, name, span)?;
            let b = expect_int(&args, 1, name, span)?;
            let value = if name == "gcd" {
                gcd(a, b)
            } else if a == 0 || b == 0 {
                0
            } else {
                (a * b).abs() / gcd(a, b)
            };
            Ok(Value::Num(NumVal::int(BigInt::from(value))))
        },
        | "factorial" | "double_factorial" => {
            let n = expect_int(&args, 0, name, span)?;
            let step = if name == "factorial" { 1 } else { 2 };
            let mut result = BigInt::one();
            let mut current = n;
            while current > 0 {
                result *= BigInt::from(current);
                current -= step;
            }
            Ok(Value::Num(NumVal::int(result)))
        },
        | "to_float" => {
            let num = expect_num(&args, 0, name, span)?;
            match num.to_f64() {
                | Some(value) => Ok(ok(Value::Num(NumVal::float(value, num.dim)))),
                | None => Ok(err(Value::string("the value has free symbols"))),
            }
        },
        | "to_real" => {
            let num = expect_num(&args, 0, name, span)?;
            Ok(Value::Num(match num.to_rational() {
                | Some(value) => NumVal::exact(value, num.dim),
                | None => num,
            }))
        },
        | "to_int" => {
            let num = expect_num(&args, 0, name, span)?;
            let value = num.to_f64().unwrap_or(f64::NAN).trunc();
            Ok(Value::Num(NumVal::int(BigInt::from(value as i64))))
        },

        // ----- units ------------------------------------------------------
        | "to" | "delta_to" => {
            let unit = expect_unit(&args, 0, name, span)?;
            let mut num = expect_num(&args, 1, name, span)?;
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
            num.display = Some(unit);
            Ok(Value::Num(num))
        },
        | "value_in" => {
            let unit = expect_unit(&args, 0, name, span)?;
            let num = expect_num(&args, 1, name, span)?;
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
        | "base" => {
            let mut num = expect_num(&args, 0, name, span)?;
            num.display = None;
            Ok(Value::Num(num))
        },
        | "point" => {
            let coefficient = expect_float(&args, 0, name, span)?;
            let unit = expect_unit(&args, 1, name, span)?;
            let scale = unit.scale.to_f64();
            let offset = unit.offset.as_ref().map_or(0.0, aevia_dim::Scale::to_f64);
            let mut num = NumVal::float(coefficient * scale + offset, unit.dim.clone());
            num.display = Some(unit);
            Ok(Value::Num(num))
        },

        // ----- tensors ----------------------------------------------------
        | "zeros" | "ones" => {
            let mut shape = Vec::with_capacity(args.len());
            for index in 0..args.len() {
                shape.push(expect_int(&args, index, name, span)?.max(0) as usize);
            }
            let count = checked_element_count(&shape, span)?;
            // No type argument: the element type is the checker's business and
            // the historical default is `Float`. `zeros<Int>` goes through
            // `call_typed`, which builds exact storage instead (§8.4).
            let fill = if name == "ones" { 1.0 } else { 0.0 };
            Ok(Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape,
                storage: Storage::Dense(Rc::new(vec![fill; count])),
                dim: Dim::one(),
                display: None,
            })))
        },
        | "fill" => {
            let first = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            let mut shape = Vec::with_capacity(args.len().saturating_sub(1));
            for index in 1..args.len() {
                shape.push(expect_int(&args, index, name, span)?.max(0) as usize);
            }
            let count = checked_element_count(&shape, span)?;
            let dim = crate::value::value_dim(&first);
            Ok(build_tensor(vec![first; count], shape, dim))
        },
        | "linspace" => {
            let start = expect_num(&args, 0, name, span)?;
            let end = expect_num(&args, 1, name, span)?;
            let count = expect_int(&args, 2, name, span)?.max(0) as usize;
            let a = start.to_f64().unwrap_or(f64::NAN);
            let b = end.to_f64().unwrap_or(f64::NAN);
            let values = if count == 0 {
                Vec::new()
            } else if count == 1 {
                vec![a]
            } else {
                (0..count)
                    .map(|index| a + (b - a) * (index as f64) / ((count - 1) as f64))
                    .collect()
            };
            Ok(Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape: vec![count],
                storage: Storage::Dense(Rc::new(values)),
                dim: start.dim,
                display: None,
            })))
        },
        | "range" => {
            let start = expect_int(&args, 0, name, span)?;
            let end = expect_int(&args, 1, name, span)?;
            let step = if args.len() > 2 {
                expect_int(&args, 2, name, span)?
            } else {
                1
            };
            if step == 0 {
                return Err(EvalError::new("E0802", "range step must not be zero").at(span));
            }
            let mut values = Vec::new();
            let mut current = start;
            while (step > 0 && current < end) || (step < 0 && current > end) {
                values.push(BigRational::from_integer(BigInt::from(current)));
                current += step;
            }
            let len = values.len();
            Ok(Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape: vec![len],
                storage: Storage::TypedExact(values, ExactRep::Int),
                dim: Dim::one(),
                display: None,
            })))
        },
        | "length" => {
            match args.first() {
                | Some(Value::Str(text)) => {
                    Ok(Value::Num(NumVal::int(BigInt::from(text.chars().count()))))
                },
                | Some(Value::Tensor(tensor)) => {
                    Ok(Value::Num(NumVal::int(BigInt::from(
                        tensor.shape.first().copied().unwrap_or(0),
                    ))))
                },
                | _ => Err(arity(name, 1, args.len(), span)),
            }
        },
        | "shape" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
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
        | "rank" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            Ok(Value::Num(NumVal::int(BigInt::from(tensor.rank()))))
        },
        | "sum" => {
            // §8.5: `sum(f, a, b)` is the calculus overload. Without this arm
            // the only `sum` was the tensor reduction, so every summation over
            // a range was `E0302`.
            if args.len() == 3 && matches!(args[0], Value::Closure(_) | Value::Native(_)) {
                return sum_range(interp, &args, span);
            }
            let tensor = expect_tensor(&args, 0, name, span)?;
            if args.len() > 1 {
                let axis = expect_int(&args, 1, name, span)?.max(0) as usize;
                return sum_axis(interp, &tensor, axis, span);
            }
            // §6.6.6: the empty sum is 0, in the phase the elements would have
            // had — a `Float` tensor answers `0.0`, an exact one `0`.
            if tensor.storage.is_empty() {
                return Ok(Value::Num(zero_of(&tensor)));
            }
            // §9.13: a dense buffer folds without boxing a `Value` per step.
            // The fold is the compensated one (§6.6.6), and the boxed `Float`
            // path uses the same fold so the class stays unobservable.
            if let Some(num) = tensor.dense_sum() {
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
                    total.display = only.display.clone();
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
        | "mean" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            // §6.6.6: the mean of an empty `Float` tensor is `NaN` (IEEE), and
            // the mean of an empty `Real` tensor is `E0802` — `Real` has no
            // NaN. The storage class is the phase here: an `f64` buffer is
            // what `Float` storage looks like.
            if tensor.storage.is_empty() {
                if tensor.dense_flat().is_some() {
                    return Ok(Value::Num(NumVal::float(f64::NAN, tensor.dim.clone())));
                }
                return Err(EvalError::new("E0802", "`mean` of an empty tensor").at(span));
            }
            // §8.7 types `mean` as `div_rep(ρ)`, which is `Real` for exact
            // input: the mean of `[1, 2]` is the rational `3/2`. An `f64`
            // buffer — dense or a view — is `Float` by construction, so it
            // takes the float branch without boxing an element.
            if let Some(values) = tensor.dense_flat() {
                let total = crate::value::kahan_sum(&values);
                return Ok(Value::Num(NumVal::float(
                    total / values.len() as f64,
                    tensor.dim.clone(),
                )));
            }
            if let Some(values) = exact_values(&tensor) {
                let value = aevia_std::stats::mean_exact(&values)
                    .map_err(|error| numeric_error(name, error, span))?;
                return Ok(exact_number(value, tensor.dim.clone()));
            }
            let values = tensor_values(&tensor)?;
            let total = crate::value::kahan_sum(&values);
            Ok(Value::Num(NumVal::float(
                total / values.len() as f64,
                tensor.dim.clone(),
            )))
        },
        | "var" | "std" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let count = tensor.storage.len();
            let population = args
                .get(1)
                .map(|value| matches!(value, Value::Bool(true)))
                .unwrap_or(false);
            if count == 0 {
                return Err(
                    EvalError::new("E0802", format!("`{name}` of an empty tensor")).at(span),
                );
            }
            let squared = tensor.dim.pow(&BigRational::from_integer(BigInt::from(2)));
            if let Some(values) = exact_values(&tensor) {
                let value = aevia_std::stats::var_exact(&values, population)
                    .map_err(|error| numeric_error(name, error, span))?;
                if name == "var" {
                    return Ok(exact_number(value, squared));
                }
                return Ok(match aevia_std::linalg::exact_sqrt(&value) {
                    | Some(root) => exact_number(root, tensor.dim.clone()),
                    | None => {
                        Value::Num(NumVal::float(
                            aevia_dim::rat::to_f64(&value).sqrt(),
                            tensor.dim.clone(),
                        ))
                    },
                });
            }
            if count < 2 {
                return Err(EvalError::new(
                    "E0802",
                    format!("`{name}` needs at least two elements"),
                )
                .at(span));
            }
            let values = tensor_values(&tensor)?;
            let mean = values.iter().sum::<f64>() / count as f64;
            let divisor = if population {
                count as f64
            } else {
                (count - 1) as f64
            };
            let variance = values
                .iter()
                .map(|value| (value - mean) * (value - mean))
                .sum::<f64>()
                / divisor;
            let dim = if name == "var" {
                squared
            } else {
                tensor.dim.clone()
            };
            let value = if name == "var" {
                variance
            } else {
                variance.sqrt()
            };
            Ok(Value::Num(NumVal::float(value, dim)))
        },
        | "min_all" | "max_all" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
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
                num.display = tensor.display.clone();
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
        | "norm" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let order = if args.len() > 1 {
                expect_int(&args, 1, name, span)?
            } else {
                2
            };
            let norm = match tensor.dense_flat() {
                | Some(values) => norm_of(&values, order),
                | None => {
                    let values: Vec<f64> = tensor
                        .elements()
                        .iter()
                        .map(|element| element.as_f64().unwrap_or(f64::NAN))
                        .collect();
                    norm_of(&values, order)
                },
            };
            Ok(Value::Num(NumVal::float(norm, tensor.dim.clone())))
        },
        | "normalize" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let norm = tensor
                .elements()
                .iter()
                .map(|element| element.as_f64().unwrap_or(0.0).powi(2))
                .sum::<f64>()
                .sqrt();
            let elements = tensor
                .elements()
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
        | "dot" | "matmul" | "matvec" | "vecmat" | "outer" => {
            let left = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let right = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            match name {
                | "dot" => interp.apply_binary("@", left, right, span),
                | "matmul" | "matvec" | "vecmat" => interp.apply_binary("@", left, right, span),
                | "outer" => outer_product(interp, left, right, span),
                | _ => unreachable!(),
            }
        },
        | "transpose" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            if tensor.rank() != 2 {
                return Err(EvalError::new("E0802", "transpose expects a rank-2 tensor").at(span));
            }
            // §9.5: a transpose of an `f64` tensor permutes the strides and
            // moves no data. An exact or boxed tensor has no buffer to point
            // into, so it is built element by element as before.
            if let Some(view) = tensor.transposed_view() {
                return Ok(Value::Tensor(Rc::new(view)));
            }
            let (rows, columns) = (tensor.shape[0], tensor.shape[1]);
            let mut elements = Vec::with_capacity(rows * columns);
            for column in 0..columns {
                for row in 0..rows {
                    elements.push(
                        tensor
                            .get(&[row as isize, column as isize])
                            .unwrap_or(Value::Unit),
                    );
                }
            }
            let result = build_tensor(elements, vec![columns, rows], tensor.dim.clone());
            Ok(crate::host_witness::preserve_tensor_schema(result, &tensor))
        },
        | "trace" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            if tensor.rank() != 2 {
                return Err(EvalError::new("E0802", "trace expects a rank-2 tensor").at(span));
            }
            let size = tensor.shape[0].min(tensor.shape[1]);
            let mut total: Option<Value> = None;
            for index in 0..size {
                let element = tensor
                    .get(&[index as isize, index as isize])
                    .unwrap_or(Value::Unit);
                total = Some(match total {
                    | None => element,
                    | Some(acc) => interp.apply_binary("+", acc, element, span)?,
                });
            }
            Ok(total.unwrap_or_else(|| Value::float(0.0)))
        },
        | "det" => determinant(interp, &args, span),
        | "inv" => inverse(&args, span),
        | "linsolve" => linsolve(interp, &args, span),
        | "eye" => {
            let n = expect_int(&args, 0, name, span)?.max(0) as usize;
            let mut values = vec![0.0; n * n];
            for index in 0..n {
                values[index * n + index] = 1.0;
            }
            Ok(Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape: vec![n, n],
                storage: Storage::Dense(Rc::new(values)),
                dim: Dim::one(),
                display: None,
            })))
        },
        | "diag" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            if tensor.rank() == 1 {
                let n = tensor.shape[0];
                let mut values = vec![0.0; n * n];
                for index in 0..n {
                    values[index * n + index] = tensor
                        .get(&[index as isize])
                        .and_then(|value| value.as_f64())
                        .unwrap_or(0.0);
                }
                Ok(Value::Tensor(Rc::new(TensorData {
                    verified_element: None,
                    shape: vec![n, n],
                    storage: Storage::Dense(Rc::new(values)),
                    dim: tensor.dim.clone(),
                    display: None,
                })))
            } else {
                let size = tensor.shape[0].min(tensor.shape[1]);
                let values: Vec<f64> = (0..size)
                    .map(|index| {
                        tensor
                            .get(&[index as isize, index as isize])
                            .and_then(|value| value.as_f64())
                            .unwrap_or(0.0)
                    })
                    .collect();
                Ok(Value::Tensor(Rc::new(TensorData {
                    verified_element: None,
                    shape: vec![size],
                    storage: Storage::Dense(Rc::new(values)),
                    dim: tensor.dim.clone(),
                    display: None,
                })))
            }
        },
        | "flatten" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let elements = tensor.elements();
            let len = elements.len();
            let result = build_tensor(elements, vec![len], tensor.dim.clone());
            Ok(crate::host_witness::preserve_tensor_schema(result, &tensor))
        },
        | "at" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let index = expect_int(&args, 1, name, span)?;
            let extent = tensor.shape.first().copied().unwrap_or(0);
            let normalized = if index < 0 {
                extent as i64 + index
            } else {
                index
            };
            if normalized < 0 || normalized >= extent as i64 {
                return Err(EvalError::new("E0802", "index out of range").at(span));
            }
            if tensor.rank() == 1 {
                tensor
                    .get(&[normalized as isize])
                    .ok_or_else(|| EvalError::new("E0802", "index out of range").at(span))
            } else {
                let remaining = &tensor.shape[1..];
                let count = TensorData::element_count(remaining);
                let mut elements = Vec::with_capacity(count);
                for offset in 0..count {
                    let mut coordinates = vec![normalized as isize];
                    let mut rest = offset;
                    for extent in remaining.iter().rev() {
                        coordinates.push((rest % extent.max(&1)) as isize);
                        rest /= extent.max(&1);
                    }
                    coordinates[1..].reverse();
                    if let Some(element) = tensor.get(&coordinates) {
                        elements.push(element);
                    }
                }
                let result = build_tensor(elements, remaining.to_vec(), tensor.dim.clone());
                Ok(crate::host_witness::preserve_tensor_schema(result, &tensor))
            }
        },
        // §6.10 / §3.8.4: explicit parallelism, gated by the checker's
        // `Parallel` constraint (only `Int`/`Float` and tensors of them reach
        // here; symbolic `Real` is rejected statically). The *guarantee* is
        // that the result order is the input order, and that holds however the
        // work is scheduled. This build runs the map sequentially — the
        // interpreter is `!Send`, so a worker cannot borrow it — which is a
        // documented deviation from §9.7.2's rayon wording, not from its
        // result.
        | "par_map" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let tensor = expect_tensor(&args, 1, name, span)?;
            if tensor.rank() != 1 {
                return Err(EvalError::new("E0802", "`par_map` expects a rank-1 tensor").at(span));
            }
            let mut elements = Vec::with_capacity(tensor.storage.len());
            for element in tensor.elements() {
                elements.push(interp.apply(function.clone(), vec![element], span)?);
            }
            let dim = elements
                .first()
                .map_or_else(Dim::one, crate::value::value_dim);
            let len = elements.len();
            Ok(build_tensor(elements, vec![len], dim))
        },
        | "map" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            match args.get(1) {
                | Some(Value::Tensor(tensor)) => {
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
                | _ => Err(arity(name, 2, args.len(), span)),
            }
        },
        | "map2" | "zip_with" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 3, 0, span))?;
            let a = expect_tensor(&args, 1, name, span)?;
            let b = expect_tensor(&args, 2, name, span)?;
            if a.shape != b.shape {
                return Err(EvalError::new("E0802", "map2 expects equal shapes").at(span));
            }
            let mut elements = Vec::with_capacity(a.storage.len());
            for (x, y) in a.elements().into_iter().zip(b.elements()) {
                elements.push(interp.apply(function.clone(), vec![x, y], span)?);
            }
            let dim = elements
                .first()
                .map_or_else(Dim::one, crate::value::value_dim);
            Ok(build_tensor(elements, a.shape.clone(), dim))
        },
        | "filter" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            // §8.11 overloads `filter` on a table: the predicate sees a row.
            if matches!(args.get(1), Some(Value::Record(_))) {
                let table = table_from(&args, 1, name, span)?;
                let mut kept = Vec::with_capacity(table.len());
                for row in 0..table.len() {
                    let value = row_value(&table, row);
                    if matches!(
                        interp.apply(function.clone(), vec![value], span)?,
                        Value::Bool(true)
                    ) {
                        kept.push(table.rows[row].clone());
                    }
                }
                return Ok(table_value(&aevia_std::table::Table {
                    columns: table.columns,
                    units: table.units,
                    rows: kept,
                }));
            }
            let tensor = expect_tensor(&args, 1, name, span)?;
            let mut elements = Vec::new();
            for element in tensor.elements() {
                if matches!(
                    interp.apply(function.clone(), vec![element.clone()], span)?,
                    Value::Bool(true)
                ) {
                    elements.push(element);
                }
            }
            let len = elements.len();
            Ok(build_tensor(elements, vec![len], tensor.dim.clone()))
        },
        | "fold" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 3, 0, span))?;
            let mut accumulator = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 3, 1, span))?;
            let tensor = expect_tensor(&args, 2, name, span)?;
            for element in tensor.elements() {
                accumulator = interp.apply(function.clone(), vec![accumulator, element], span)?;
            }
            Ok(accumulator)
        },
        | "take" | "drop" => {
            let count = expect_int(&args, 0, name, span)?.max(0) as usize;
            let tensor = expect_tensor(&args, 1, name, span)?;
            let elements = tensor.elements();
            let selected: Vec<Value> = if name == "take" {
                elements.into_iter().take(count).collect()
            } else {
                elements.into_iter().skip(count).collect()
            };
            let len = selected.len();
            Ok(build_tensor(selected, vec![len], tensor.dim.clone()))
        },
        | "reverse" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            // §9.5: reversing a rank-1 `f64` tensor is a view — the same
            // buffer walked backwards — so `xs[::-1]` allocates nothing and
            // still shares `xs`'s buffer. A rank-2 operand keeps the older
            // behaviour below, which flattens it: `reverse` is typed
            // `Tensor<1,a> -> Tensor<1,a>` (§8), and changing what it does to
            // a matrix is a semantic change, not a representation one.
            if tensor.rank() == 1 {
                if let Some(view) = tensor.reversed_view(0) {
                    return Ok(Value::Tensor(Rc::new(view)));
                }
            }
            let mut elements = tensor.elements();
            elements.reverse();
            let len = elements.len();
            Ok(build_tensor(elements, vec![len], tensor.dim.clone()))
        },
        | "sort" => {
            // §3.8.4: `sort` needs `TotalOrd`, and the checker rejects a type
            // without one. The order must therefore be the *total* one —
            // comparing `as_f64` made a tensor of strings sort as `NaN`, which
            // silently left it in source order.
            let tensor = expect_tensor(&args, 0, name, span)?;
            let mut elements = tensor.elements();
            elements.sort_by(total_order);
            let len = elements.len();
            Ok(build_tensor(elements, vec![len], tensor.dim.clone()))
        },
        | "concat" => {
            match (args.first(), args.get(1)) {
                | (Some(Value::Str(a)), Some(Value::Str(b))) => {
                    Ok(Value::string(format!("{a}{b}")))
                },
                | (Some(Value::Tensor(a)), Some(Value::Tensor(b))) => {
                    let mut elements = a.elements();
                    elements.extend(b.elements());
                    let len = elements.len();
                    Ok(build_tensor(elements, vec![len], a.dim.clone()))
                },
                | _ => Err(arity(name, 2, args.len(), span)),
            }
        },
        | "any" | "all" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let tensor = expect_tensor(&args, 1, name, span)?;
            let mut result = name == "all";
            for element in tensor.elements() {
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
        | "find" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let tensor = expect_tensor(&args, 1, name, span)?;
            for element in tensor.elements() {
                if matches!(
                    interp.apply(function.clone(), vec![element.clone()], span)?,
                    Value::Bool(true)
                ) {
                    return Ok(make_variant(adt::OPTION_ID, 1, "Some", vec![element]));
                }
            }
            Ok(make_variant(adt::OPTION_ID, 0, "None", Vec::new()))
        },
        | "prod" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
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
        | "centroid" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let count = tensor.storage.len();
            let mut total = 0.0;
            for element in tensor.elements() {
                total += element.as_f64().unwrap_or(f64::NAN);
            }
            Ok(Value::Num(NumVal::float(
                total / count.max(1) as f64,
                tensor.dim.clone(),
            )))
        },

        // ----- strings ----------------------------------------------------
        | "join" => {
            // §8.11 overloads `join` on tables; the first argument decides.
            if matches!(args.first(), Some(Value::Record(_))) {
                return join_tables(&args, name, span);
            }
            let separator = expect_str(&args, 0, name, span)?;
            let tensor = expect_tensor(&args, 1, name, span)?;
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
        | "split" => {
            let text = expect_str(&args, 0, name, span)?;
            let separator = expect_str(&args, 1, name, span)?;
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
        | "replace" => {
            let text = expect_str(&args, 0, name, span)?;
            let from = expect_str(&args, 1, name, span)?;
            let to = expect_str(&args, 2, name, span)?;
            Ok(Value::string(text.replace(&from, &to)))
        },
        | "trim" => Ok(Value::string(expect_str(&args, 0, name, span)?.trim())),
        | "upper" => {
            Ok(Value::string(
                expect_str(&args, 0, name, span)?.to_uppercase(),
            ))
        },
        | "lower" => {
            Ok(Value::string(
                expect_str(&args, 0, name, span)?.to_lowercase(),
            ))
        },
        | "contains" | "starts_with" | "ends_with" => {
            let text = expect_str(&args, 0, name, span)?;
            let pattern = expect_str(&args, 1, name, span)?;
            let value = match name {
                | "contains" => text.contains(&pattern),
                | "starts_with" => text.starts_with(&pattern),
                | _ => text.ends_with(&pattern),
            };
            Ok(Value::Bool(value))
        },
        | "parse_int" => {
            let text = expect_str(&args, 0, name, span)?;
            match text.trim().parse::<BigInt>() {
                | Ok(value) => Ok(ok(Value::Num(NumVal::int(value)))),
                | Err(error) => Ok(err(Value::string(error.to_string()))),
            }
        },
        | "parse_float" => {
            let text = expect_str(&args, 0, name, span)?;
            match text.trim().parse::<f64>() {
                | Ok(value) => Ok(ok(Value::Num(NumVal::float(value, Dim::one())))),
                | Err(error) => Ok(err(Value::string(error.to_string()))),
            }
        },
        | "chars" => {
            let text = expect_str(&args, 0, name, span)?;
            let parts: Vec<Value> = text.chars().map(|c| Value::string(c.to_string())).collect();
            let len = parts.len();
            Ok(build_tensor(parts, vec![len], Dim::one()))
        },
        | "format" => {
            let template = expect_str(&args, 0, name, span)?;
            let tensor = expect_tensor(&args, 1, name, span)?;
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
            let mut out = template;
            for (index, part) in parts.iter().enumerate() {
                out = out.replace(&format!("{{{index}}}"), part);
            }
            Ok(Value::string(out))
        },

        // ----- Option / Result -------------------------------------------
        | "and_then" => {
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
                | _ => Err(arity(name, 2, args.len(), span)),
            }
        },
        | "unwrap_or" => {
            let fallback = args.get(1).cloned().unwrap_or(Value::Unit);
            match args.first() {
                | Some(Value::Variant(variant)) if variant.adt == adt::OPTION_ID => {
                    if variant.index == 1 {
                        Ok(variant.fields.first().cloned().unwrap_or(fallback))
                    } else {
                        Ok(fallback)
                    }
                },
                | _ => Ok(fallback),
            }
        },
        | "is_some" | "is_none" => {
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
        | "map_err" => {
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
        | "symbol" => {
            let display = expect_str(&args, 0, name, span)?;
            let count = interp.symbol_counts.entry(display.clone()).or_insert(0);
            *count += 1;
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
        | "symfun" => {
            let _ = expect_str(&args, 0, name, span)?;
            // Applying an unknown function symbol needs a term form for
            // uninterpreted functions, which arrives with the backend bridge.
            Ok(Value::Native(Rc::new(NativeFn(Box::new(
                move |_interp, _args, span| {
                    Err(EvalError::new(
                        "E1001",
                        "`symfun` application needs the rssn symbolic backend",
                    )
                    .at(span))
                },
            )))))
        },
        | "diff" => {
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
        | "diff_at" => {
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
        | "antiderivative" => {
            // The argument is not known yet, so the integral is built against a
            // dimensionless variable and the dimension is recovered when the
            // returned function is applied (`e*d`, §8.6).
            let (variable, applied) = apply_to_fresh(interp, name, &args, span, Dim::one())?;
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
        | "defint" => {
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
                apply_to_fresh(interp, name, &args, span, variable_dim.clone())?;
            let symbol = symbolic_variable(&variable, variable_dim);
            let dim = crate::value::value_dim(&applied).mul(&crate::value::value_dim(&lower));
            math_result(bridge::call(
                "defint",
                &[applied, symbol, lower, upper],
                dim,
            ))
        },
        | "limit" => {
            let at = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            let variable_dim = crate::value::value_dim(&at);
            let (variable, applied) =
                apply_to_fresh(interp, name, &args, span, variable_dim.clone())?;
            let symbol = symbolic_variable(&variable, variable_dim);
            let dim = crate::value::value_dim(&applied);
            math_result(bridge::call("limit", &[applied, symbol, at], dim))
        },
        | "series" => {
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
                apply_to_fresh(interp, name, &args, span, variable_dim.clone())?;
            let symbol = symbolic_variable(&variable, variable_dim);
            let dim = crate::value::value_dim(&applied);
            bridge::call("taylor", &[applied, symbol, at, order], dim)
        },
        | "solve" => {
            // No argument fixes the variable's dimension, so the roots stay
            // dimensionless at run time; the static type is `Solutions<Real,d>`.
            let (variable, applied) = apply_to_fresh(interp, name, &args, span, Dim::one())?;
            let symbol = symbolic_variable(&variable, Dim::one());
            match solve("solve", &applied, &symbol) {
                | Ok(solutions) => Ok(ok(solutions_value(&solutions, Dim::one()))),
                | Err(error) => Ok(err(Value::string(error.message))),
            }
        },
        | "nsolve" => {
            let start = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            let variable_dim = crate::value::value_dim(&start);
            let (variable, applied) =
                apply_to_fresh(interp, name, &args, span, variable_dim.clone())?;
            let symbol = symbolic_variable(&variable, variable_dim.clone());
            let dim = variable_dim;
            // `nsolve` only answers in the numeric phase; the symbolic phase
            // has no root to give.
            math_result(bridge::compute(
                "nsolve",
                &[applied, symbol, start],
                &aevia_rssn::Opts::numeric(1e-10),
                dim,
            ))
        },
        | "eval" | "simplify" => {
            let value = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            let dim = crate::value::value_dim(&value);
            let term = value_to_term(&value)?;
            let sym = bridge::term_val(&term)?;
            let handle = match sym {
                | aevia_rssn::Val::Term(handle) => handle,
                | _ => return Ok(value),
            };
            let computed = aevia_rssn::kernel()
                .compute(handle, &aevia_rssn::Opts::symbolic())
                .map_err(|failure| bridge::error(&failure))?;
            Ok(bridge::from_val(&computed.val, dim))
        },
        | "approx" => {
            let value = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            let dim = crate::value::value_dim(&value);
            // A concrete value is already its own approximation: `approx(9)`
            // is `Ok(9.0)` (§6.13 step 5), not a kernel round trip.
            if let Value::Num(number) = &value {
                if !number.is_symbolic() {
                    if let Some(approximate) = number.to_f64() {
                        return Ok(ok(Value::Num(NumVal::float(approximate, dim))));
                    }
                }
            }
            let term = value_to_term(&value)?;
            let free = term.try_free_symbols()?;
            if !free.is_empty() {
                return Ok(err(Value::string(format!(
                    "free symbols: {}",
                    free.join(", ")
                ))));
            }
            let sym = bridge::term_val(&term)?;
            let handle = match sym {
                | aevia_rssn::Val::Term(handle) => handle,
                | _ => return Ok(err(Value::string("not numeric"))),
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
                        | None => Ok(err(Value::string("not numeric"))),
                    }
                },
                | Err(failure) => Ok(err(Value::string(failure.message))),
            }
        },
        | "subst" => {
            let symbol = expect_num(&args, 0, name, span)?;
            let replacement = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 3, 1, span))?;
            let target = args
                .get(2)
                .cloned()
                .ok_or_else(|| arity(name, 3, 2, span))?;
            let replacement = value_to_term(&replacement)?;
            let target = value_to_term(&target)?;
            let Some(symbol_name) = symbol_name(&symbol) else {
                return Err(EvalError::new("E0302", "`subst` expects a symbol").at(span));
            };
            let substituted = target.substitute(&symbol_name, &replacement)?;
            let dim = crate::value::value_dim(&args[2]);
            // A substitution that leaves nothing free is a number, not a
            // symbolic value: `subst(x, 1, x^2)` is `1`, and `1 == 1` is a
            // `Bool` (§6.3.6).
            if let Some(folded) = bridge::fold(&substituted, dim.clone()) {
                return Ok(folded);
            }
            Ok(Value::Num(NumVal::symbolic(substituted, dim)))
        },
        | "to_value" => {
            let value = args.first().ok_or_else(|| arity(name, 1, 0, span))?;
            let payload = bridge::to_val(value)?;
            let witness = crate::rssn_module::host_dimension_witness(value);
            let host_witness = match value {
                | Value::KernelValue(existing) => existing.host_witness.clone(),
                | _ => {
                    crate::host_witness::runtime_schema(value)
                        .and_then(|schema| crate::host_witness::capture(value, &schema, false))
                        .map(Rc::new)
                },
            };
            Ok(Value::KernelValue(Rc::new(crate::value::KernelValue {
                payload,
                witness,
                host_witness,
            })))
        },
        | "to_callback" => {
            if args.len() != 1 {
                return Err(arity(name, 1, args.len(), span));
            }
            let schema = interp.concrete_callable_type(&args[0]).ok_or_else(|| {
                EvalError::new("E0304", "`to_callback` requires an authenticated concrete function signature").at(span)
            })?;
            let function = interp.reify_callable(&args[0], &schema)?;
            Ok(Value::Callback(Rc::new(crate::value::CallbackValue { function, schema })))
        },
        | "from_value" => {
            // Plain host or higher-order application carries no authenticated source call evidence.
            let result: Result<Value, String> =
                Err("missing concrete result type evidence".to_string());
            Ok(match result {
                | Ok(value) => ok(value),
                | Err(reason) => err(crate::rssn_module::value_error("TypeMismatch", reason)),
            })
        },
        | "decide" => Ok(decide(&args, span)),
        | "assuming" | "by_cases" => branch_on(interp, name, &args, span),

        // ----- IO ---------------------------------------------------------
        | "read_file" => {
            let path = expect_str(&args, 0, name, span)?;
            Ok(Value::Io(Rc::new(crate::value::IoAction::ReadFile(path))))
        },
        | "write_file" => {
            let path = expect_str(&args, 0, name, span)?;
            let contents = expect_str(&args, 1, name, span)?;
            Ok(Value::Io(Rc::new(crate::value::IoAction::WriteFile {
                path,
                contents,
            })))
        },
        | "read_line" => Ok(Value::Io(Rc::new(crate::value::IoAction::ReadLine))),
        // §9.12: the program arguments the host set with `Interp::set_args`,
        // as a rank-1 `Tensor<1, String>`.
        | "args" => {
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
        | "time_now" => Ok(Value::Io(Rc::new(crate::value::IoAction::TimeNow))),
        | "random" => Ok(Value::Io(Rc::new(crate::value::IoAction::Random))),
        // §6.10: the pure, seeded generator. `rng(seed)` answers `() -> Float`,
        // a closure over its own LCG state, so the same seed always produces
        // the same stream and nothing is consumed from the session's RNG — the
        // purity the standard asks for ("rng(seed) 是纯的、确定的").
        | "rng" => {
            let seed = expect_int(&args, 0, name, span)?;
            let state = Rc::new(std::cell::RefCell::new(seed as u64));
            Ok(Value::Native(Rc::new(NativeFn(Box::new(
                move |_interp: &mut Interp, _args: Vec<Value>, _span: Span| {
                    let value = crate::eval::next_unit(&mut state.borrow_mut());
                    Ok(Value::float(value))
                },
            )))))
        },

        // ----- testing ----------------------------------------------------
        | "assert" => {
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
        | "assert_eq" => {
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
        | "approx_eq" => {
            let a = expect_num(&args, 0, name, span)?;
            let b = expect_num(&args, 1, name, span)?;
            let tolerance = expect_num(&args, 2, name, span)?;
            let tolerance = NumVal {
                kind: match tolerance.kind {
                    | NumKind::Int(value) => NumKind::Int(value.abs()),
                    | NumKind::Exact(value) => NumKind::Exact(value.abs()),
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
            let difference = match (a.to_rational(), b.to_rational()) {
                | (Some(a), Some(b)) => (a - b).abs(),
                | _ => {
                    BigRational::from_float(
                        (a.to_f64().unwrap_or(f64::NAN) - b.to_f64().unwrap_or(f64::NAN)).abs(),
                    )
                    .unwrap_or_else(BigRational::zero)
                },
            };
            let limit = tolerance.to_rational().unwrap_or_else(BigRational::zero);
            Ok(Value::Bool(difference <= limit))
        },
        | "forall" => forall(interp, &args, span),

        // ----- plotting ---------------------------------------------------
        | "line" | "scatter" => {
            let x = args.first().cloned().unwrap_or(Value::Unit);
            let y = args.get(1).cloned().unwrap_or(Value::Unit);
            let mut fields = IndexMap::new();
            fields.insert("kind".to_string(), Value::string(name));
            fields.insert("x".to_string(), x);
            fields.insert("y".to_string(), y);
            fields.insert("label".to_string(), Value::string(""));
            Ok(Value::Record(Rc::new(RecordValue { fields })))
        },
        | "label" => {
            let mut series = match args.first().cloned() {
                | Some(Value::Record(record)) => (*record).clone(),
                | _ => return Err(arity(name, 2, args.len(), span)),
            };
            let text = expect_str(&args, 1, name, span)?;
            series
                .fields
                .insert("label".to_string(), Value::string(text));
            Ok(Value::Record(Rc::new(series)))
        },
        | "combine" => {
            let series = args.first().cloned().unwrap_or(Value::Unit);
            let mut fields = IndexMap::new();
            fields.insert("series".to_string(), series);
            Ok(Value::Record(Rc::new(RecordValue { fields })))
        },
        | "plot" | "save" => {
            let spec = args.last().cloned().unwrap_or(Value::Unit);
            let html = render_plot(&spec);
            if name == "plot" {
                Ok(Value::Io(Rc::new(crate::value::IoAction::Plot(html))))
            } else {
                let path = expect_str(&args, 0, name, span)?;
                Ok(Value::Io(Rc::new(crate::value::IoAction::Save {
                    path,
                    html,
                })))
            }
        },

        // ----- §8.2 constants ---------------------------------------------
        // `Interp::seed_constants` binds these in the global scope, which is
        // where a bare `pi` is found. These arms exist so that reaching the
        // builtin table — `map(pi, xs)`, or a call through `apply` — gives the
        // same value instead of "typed but not implemented".
        | "pi" | "e" | "oo" => {
            if name == "e" {
                let one_value = Value::Num(NumVal::int(BigInt::one()));
                return crate::kernel::call("exp", &[one_value], Dim::one());
            }
            crate::kernel::call(name, &[], Dim::one())
        },
        | "nan" => Ok(Value::Num(NumVal::float(f64::NAN, Dim::one()))),
        | "i" => {
            Ok(Value::Num(NumVal {
                kind: NumKind::Complex(0.0, 1.0),
                dim: Dim::one(),
                display: None,
            }))
        },
        | "lgamma" => {
            let num = expect_num(&args, 0, name, span)?;
            let value = num.to_f64().unwrap_or(f64::NAN);
            Ok(Value::Num(NumVal::float(
                aevia_std::special::ln_gamma(value),
                num.dim,
            )))
        },

        // ----- §8.1.5 containers that arrived with the chapter-8 sweep -----
        | "enumerate" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let mut elements = Vec::with_capacity(tensor.storage.len());
            for (index, element) in tensor.elements().into_iter().enumerate() {
                elements.push(record_of(vec![
                    (
                        "index",
                        Value::int(i64::try_from(index).unwrap_or(i64::MAX)),
                    ),
                    ("value", element),
                ]));
            }
            let len = elements.len();
            Ok(build_tensor(elements, vec![len], Dim::one()))
        },
        | "zip" => {
            let left = expect_tensor(&args, 0, name, span)?;
            let right = expect_tensor(&args, 1, name, span)?;
            let mut elements = Vec::with_capacity(left.storage.len());
            for (a, b) in left.elements().into_iter().zip(right.elements()) {
                elements.push(tuple(vec![a, b]));
            }
            let len = elements.len();
            Ok(build_tensor(elements, vec![len], Dim::one()))
        },
        | "fold_left" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 3, 0, span))?;
            let mut accumulator = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 3, 1, span))?;
            let tensor = expect_tensor(&args, 2, name, span)?;
            for element in tensor.elements() {
                accumulator = interp.apply(function.clone(), vec![accumulator, element], span)?;
            }
            Ok(accumulator)
        },
        | "sort_by" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let tensor = expect_tensor(&args, 1, name, span)?;
            let mut elements = tensor.elements();
            // An insertion sort over `Value`s: the comparator is an Aevia
            // function, so it can fail, and a fallible comparator cannot be
            // handed to `sort_by`.
            let mut failure = None;
            for index in 1..elements.len() {
                let mut position = index;
                while position > 0 {
                    let ordering = interp.apply(
                        function.clone(),
                        vec![elements[position - 1].clone(), elements[position].clone()],
                        span,
                    );
                    let ordering = match ordering {
                        | Ok(Value::Variant(variant)) => variant.index,
                        | Ok(other) => {
                            failure = Some(EvalError::new(
                                "E0302",
                                format!(
                                    "a comparator must return an `Ordering`, found {}",
                                    other.type_name()
                                ),
                            ));
                            break;
                        },
                        | Err(error) => {
                            failure = Some(error);
                            break;
                        },
                    };
                    if ordering != 2 {
                        break;
                    }
                    elements.swap(position - 1, position);
                    position -= 1;
                }
                if failure.is_some() {
                    break;
                }
            }
            if let Some(error) = failure {
                return Err(error.at(span));
            }
            let len = elements.len();
            Ok(build_tensor(elements, vec![len], tensor.dim.clone()))
        },
        | "sort_float" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let nan_last = args
                .get(1)
                .map(|value| matches!(value, Value::Bool(true)))
                .unwrap_or(true);
            let mut elements = tensor.elements();
            elements.sort_by(|a, b| {
                let x = a.as_f64().unwrap_or(f64::NAN);
                let y = b.as_f64().unwrap_or(f64::NAN);
                match (x.is_nan(), y.is_nan()) {
                    | (true, true) => std::cmp::Ordering::Equal,
                    | (true, false) => {
                        if nan_last {
                            std::cmp::Ordering::Greater
                        } else {
                            std::cmp::Ordering::Less
                        }
                    },
                    | (false, true) => {
                        if nan_last {
                            std::cmp::Ordering::Less
                        } else {
                            std::cmp::Ordering::Greater
                        }
                    },
                    | (false, false) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                }
            });
            let len = elements.len();
            Ok(build_tensor(elements, vec![len], tensor.dim.clone()))
        },

        // ----- §8.1.1 combinators ----------------------------------------
        | "id" => Ok(args.first().cloned().unwrap_or(Value::Unit)),
        | "const" => Ok(args.first().cloned().unwrap_or(Value::Unit)),
        | "apply" => {
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
        | "compose" => {
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
        | "flip" => {
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
        | "curry" => {
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
        | "uncurry" => {
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
        | "partial" => {
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
        | "not" => {
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
        | "xor" => {
            match (args.first(), args.get(1)) {
                | (Some(Value::Bool(a)), Some(Value::Bool(b))) => Ok(Value::Bool(a != b)),
                | _ => Err(EvalError::new("E0302", "`xor` expects two Bools").at(span)),
            }
        },
        | "compare" => {
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
        | "partial_compare" => {
            let left = expect_float(&args, 0, name, span)?;
            let right = expect_float(&args, 1, name, span)?;
            if left.is_nan() || right.is_nan() {
                Ok(none())
            } else {
                Ok(some(ordering_value(
                    left.partial_cmp(&right)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )))
            }
        },
        | "total_compare" => {
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
        | "hash" => {
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
        | "sym_eq" | "sym_ne" | "sym_lt" => {
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
        | "zero" => Ok(Value::Num(NumVal::exact(BigRational::zero(), Dim::one()))),

        // ----- §8.2.2 special functions -----------------------------------
        | "beta" => {
            let a = expect_float(&args, 0, name, span)?;
            let b = expect_float(&args, 1, name, span)?;
            Ok(Value::float(aevia_std::special::beta(a, b)))
        },
        | "zeta" => {
            let x = expect_float(&args, 0, name, span)?;
            Ok(Value::float(aevia_std::special::zeta(x)))
        },
        | "bessel_j0" => {
            Ok(Value::float(aevia_std::special::bessel_j0(expect_float(
                &args, 0, name, span,
            )?)))
        },
        | "bessel_j1" | "j1" => {
            Ok(Value::float(aevia_std::special::bessel_j1(expect_float(
                &args, 0, name, span,
            )?)))
        },
        | "bessel_y0" => {
            Ok(Value::float(aevia_std::special::bessel_y0(expect_float(
                &args, 0, name, span,
            )?)))
        },
        | "bessel_y1" | "y1" => {
            Ok(Value::float(aevia_std::special::bessel_y1(expect_float(
                &args, 0, name, span,
            )?)))
        },
        | "bessel_jn" | "bessel_yn" => {
            let order = expect_int(&args, 0, name, span)?;
            let x = expect_float(&args, 1, name, span)?;
            let order = i32::try_from(order).unwrap_or(0);
            Ok(Value::float(if name == "bessel_jn" {
                aevia_std::special::bessel_jn(order, x)
            } else {
                aevia_std::special::bessel_yn(order, x)
            }))
        },
        | "legendre_p" | "hermite_h" | "laguerre_l" | "chebyshev_t" | "chebyshev_u" => {
            classical_polynomial(&args, name, span)
        },

        // ----- §8.15 complex ----------------------------------------------
        | "complex" => {
            let re = expect_float(&args, 0, name, span)?;
            let im = expect_float(&args, 1, name, span)?;
            Ok(Value::Num(NumVal {
                kind: NumKind::Complex(re, im),
                dim: Dim::one(),
                display: None,
            }))
        },
        | "re" | "im" | "conj" | "magnitude" | "arg" => {
            let num = expect_num(&args, 0, name, span)?;
            let NumKind::Complex(re, im) = num.kind else {
                return Err(
                    EvalError::new("E0302", format!("`{name}` expects a complex number")).at(span),
                );
            };
            Ok(match name {
                | "re" => Value::Num(NumVal::float(re, num.dim.clone())),
                | "im" => Value::Num(NumVal::float(im, num.dim.clone())),
                | "conj" => {
                    Value::Num(NumVal {
                        kind: NumKind::Complex(re, -im),
                        dim: num.dim.clone(),
                        display: None,
                    })
                },
                | "magnitude" => Value::Num(NumVal::float(re.hypot(im), num.dim.clone())),
                | _ => Value::Num(NumVal::float(im.atan2(re), Dim::one())),
            })
        },
        | "complex_sqrt" | "complex_exp" | "complex_ln" | "complex_sin" | "complex_cos" => {
            let num = expect_num(&args, 0, name, span)?;
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
                num.dim.clone()
            };
            Ok(Value::Num(NumVal {
                kind: NumKind::Complex(result.re, result.im),
                dim,
                display: None,
            }))
        },

        // ----- §8.16 number theory, combinatorics, geometry ---------------
        | "factor" => {
            let n = expect_int(&args, 0, name, span)?;
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
        | "is_prime" => {
            Ok(Value::Bool(aevia_std::number::is_prime(
                u64::try_from(expect_int(&args, 0, name, span)?).unwrap_or(0),
            )))
        },
        | "primes" => {
            let limit = expect_int(&args, 0, name, span)?;
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
        | "binomial" => {
            let n = expect_int(&args, 0, name, span)?;
            let k = expect_int(&args, 1, name, span)?;
            Ok(Value::Num(NumVal::int(aevia_std::number::binomial(n, k))))
        },
        | "permutations" => {
            let n = expect_int(&args, 0, name, span)?;
            Ok(Value::Num(NumVal::int(aevia_std::number::permutations(n))))
        },
        | "polygon_area" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let points = tensor_values(&tensor)?;
            let area = aevia_std::geom::polygon_area(&points);
            Ok(Value::Num(NumVal::float(
                area,
                tensor.dim.pow(&BigRational::from_integer(BigInt::from(2))),
            )))
        },
        | "distance" => {
            let left = expect_tensor(&args, 0, name, span)?;
            let right = expect_tensor(&args, 1, name, span)?;
            let distance =
                aevia_std::geom::distance(&tensor_values(&left)?, &tensor_values(&right)?)
                    .map_err(|error| numeric_error(name, error, span))?;
            Ok(Value::Num(NumVal::float(distance, left.dim.clone())))
        },

        // ----- §8.14 measurement ------------------------------------------
        | "measure" => {
            let value = expect_num(&args, 0, name, span)?;
            let uncertainty = expect_num(&args, 1, name, span)?;
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
        | "value" | "uncertainty" => {
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
        | "compile_fail" => {
            let source = expect_str(&args, 0, name, span)?;
            let compilation = crate::compile("compile_fail.aev", &source);
            Ok(Value::Bool(compilation.diagnostics.has_errors()))
        },

        // ----- §8.4 decompositions and shape operations -------------------
        | "lstsq" => {
            let matrix = expect_tensor(&args, 0, name, span)?;
            let (rows, cols, entries) = tensor_matrix(&matrix, name, span)?;
            let vector = expect_tensor(&args, 1, name, span)?;
            let rhs = tensor_values(&vector)?;
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
            Ok(lin_result(
                aevia_std::linalg::lstsq(rows, cols, &entries, &rhs)
                    .map(|solution| float_tensor(vec![cols], solution, dim)),
            ))
        },
        | "eig" => {
            let matrix = expect_tensor(&args, 0, name, span)?;
            let (rows, _, entries) = tensor_matrix(&matrix, name, span)?;
            let dim = matrix.dim.clone();
            let (values, vectors) = aevia_std::linalg::eig(rows, rows, &entries)
                .map_err(|error| numeric_error(name, error, span))?;
            Ok(tuple(vec![
                complex_tensor(vec![values.len()], &values, dim),
                complex_tensor(vec![rows, rows], &vectors, Dim::one()),
            ]))
        },
        | "eigvals" => {
            let matrix = expect_tensor(&args, 0, name, span)?;
            let (rows, _, entries) = tensor_matrix(&matrix, name, span)?;
            let dim = matrix.dim.clone();
            let values = aevia_std::linalg::eigvals(rows, rows, &entries)
                .map_err(|error| numeric_error(name, error, span))?;
            Ok(complex_tensor(vec![values.len()], &values, dim))
        },
        | "svd" => {
            let matrix = expect_tensor(&args, 0, name, span)?;
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
        | "qr" => {
            let matrix = expect_tensor(&args, 0, name, span)?;
            let (rows, cols, entries) = tensor_matrix(&matrix, name, span)?;
            let dim = matrix.dim.clone();
            // §8.4 keeps `ρ`, so an exact matrix has an exact factorisation
            // when the norms have exact square roots; otherwise the float
            // Gram-Schmidt answers.
            if let Some(exact) = exact_values(&matrix) {
                if rows <= 8 && cols <= 8 {
                    if let Ok((q, r)) = aevia_std::linalg::qr_exact(rows, cols, &exact) {
                        return Ok(tuple(vec![
                            rational_tensor(vec![rows, rows], &q, Dim::one()),
                            rational_tensor(vec![rows, cols], &r, dim),
                        ]));
                    }
                }
            }
            let (q, r) = aevia_std::linalg::qr(rows, cols, &entries)
                .map_err(|error| numeric_error(name, error, span))?;
            Ok(tuple(vec![
                float_tensor(vec![rows, rows], q, Dim::one()),
                float_tensor(vec![rows, cols], r, dim),
            ]))
        },
        | "lu" => {
            let matrix = expect_tensor(&args, 0, name, span)?;
            let (rows, cols, entries) = tensor_matrix(&matrix, name, span)?;
            let dim = matrix.dim.clone();
            if rows == cols && rows <= 32 {
                if let Some(exact) = exact_values(&matrix) {
                    let (p, l, u) = aevia_std::linalg::lu_exact(rows, &exact)
                        .map_err(|error| numeric_error(name, error, span))?;
                    return Ok(tuple(vec![
                        rational_tensor(vec![rows, rows], &p, Dim::one()),
                        rational_tensor(vec![rows, rows], &l, Dim::one()),
                        rational_tensor(vec![rows, cols], &u, dim),
                    ]));
                }
            }
            let (p, l, u) = aevia_std::linalg::lu(rows, cols, &entries)
                .map_err(|error| numeric_error(name, error, span))?;
            Ok(tuple(vec![
                float_tensor(vec![rows, rows], p, Dim::one()),
                float_tensor(vec![rows, rows], l, Dim::one()),
                float_tensor(vec![rows, cols], u, dim),
            ]))
        },
        | "cholesky" => {
            let matrix = expect_tensor(&args, 0, name, span)?;
            let (rows, _, entries) = tensor_matrix(&matrix, name, span)?;
            let dim = matrix
                .dim
                .pow(&BigRational::new(BigInt::one(), BigInt::from(2)));
            Ok(lin_result(
                aevia_std::linalg::cholesky(rows, rows, &entries)
                    .map(|l| float_tensor(vec![rows, rows], l, dim)),
            ))
        },
        | "tril" | "triu" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            if tensor.rank() != 2 {
                return Err(
                    EvalError::new("E0802", format!("`{name}` expects a rank-2 tensor")).at(span),
                );
            }
            let (rows, cols) = (tensor.shape[0], tensor.shape[1]);
            let mut elements = Vec::with_capacity(tensor.storage.len());
            for row in 0..rows {
                for column in 0..cols {
                    let keep = if name == "tril" {
                        column <= row
                    } else {
                        column >= row
                    };
                    elements.push(if keep {
                        tensor
                            .get(&[row as isize, column as isize])
                            .unwrap_or(Value::Unit)
                    } else {
                        zero_like(&tensor)
                    });
                }
            }
            Ok(build_tensor(elements, vec![rows, cols], tensor.dim.clone()))
        },
        // `reshape`/`zeros_of`/`ones_of` read their shape from the type
        // argument (`call_typed`). Reaching this arm means the call was
        // written without one, and the runtime has no shape to build.
        | "reshape" | "zeros_of" | "ones_of" => {
            Err(EvalError::new(
                "E0310",
                format!("`{name}` needs its shape type argument, as in `{name}<[3, 2]>(…)`"),
            )
            .at(span))
        },
        | "reshape_dyn" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let shape_tensor = expect_tensor(&args, 1, name, span)?;
            let mut shape = Vec::new();
            for element in shape_tensor.elements() {
                let extent = element.as_f64().unwrap_or(-1.0);
                if extent < 0.0 {
                    return Ok(err(shape_error(
                        "a shape extent cannot be negative".to_string(),
                    )));
                }
                shape.push(extent as usize);
            }
            if TensorData::element_count(&shape) != tensor.storage.len() {
                return Ok(err(shape_error(format!(
                    "cannot reshape {} element(s) into {:?}",
                    tensor.storage.len(),
                    shape
                ))));
            }
            let Some(storage) = tensor.reshape_storage(&shape) else {
                return Ok(err(shape_error(
                    "this tensor's elements cannot be reshaped".to_string(),
                )));
            };
            Ok(Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape,
                storage,
                dim: tensor.dim.clone(),
                display: tensor.display.clone(),
            })))
        },
        | "set" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let indices = expect_tensor(&args, 1, name, span)?;
            let value = args
                .get(2)
                .cloned()
                .ok_or_else(|| arity(name, 3, 2, span))?;
            let mut coordinates = Vec::new();
            for element in indices.elements() {
                coordinates.push(element.as_f64().unwrap_or(0.0) as isize);
            }
            if coordinates.len() != tensor.rank() {
                return Err(EvalError::new(
                    "E0802",
                    format!(
                        "`set` needs {} index(es) for a rank-{} tensor",
                        tensor.rank(),
                        tensor.rank()
                    ),
                )
                .at(span));
            }
            let mut elements = tensor.elements();
            let flat = flat_index(&tensor.shape, &coordinates)
                .ok_or_else(|| EvalError::new("E0802", "index out of range").at(span))?;
            elements[flat] = value;
            Ok(build_tensor(
                elements,
                tensor.shape.clone(),
                tensor.dim.clone(),
            ))
        },
        | "map_indexed" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let tensor = expect_tensor(&args, 1, name, span)?;
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
        | "approx_map" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let tensor = expect_tensor(&args, 1, name, span)?;
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
        | "det_unit" => {
            // §4.9.5: `det_unit(A, u)` answers with a `Dyn`, because the
            // determinant of an `n × n` matrix over `d` has dimension `d^n`
            // and `n` is only known at run time.
            let tensor = expect_tensor(&args, 0, name, span)?;
            let unit = expect_unit(&args, 1, name, span)?;
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
        | "dyn" => {
            let num = expect_num(&args, 0, name, span)?;
            Ok(Value::Dyn(Rc::new(DynValue::new(&num))))
        },
        | "dyn_with" => {
            let num = expect_num(&args, 0, name, span)?;
            let dim = expect_dim_value(&args, 1, name, span)?;
            let mut dynamic = DynValue::new(&num);
            dynamic.dim = dim;
            Ok(Value::Dyn(Rc::new(dynamic)))
        },
        | "check" => {
            let dynamic = expect_dyn(&args, 0, name, span)?;
            // §4.9.4: `check<d>(x)` is type-directed — the concrete `d` never
            // reaches the runtime in the argument list, so the checker records
            // it by span. Compare it with the dimension the value carries; a
            // mismatch is the appendix-B `E0412` class, carried as the
            // recoverable `Err(DimError)` the signature promises.
            if let Some(expected) = interp.context.evidence.dyn_calls.get(&span) {
                if dynamic.dim != *expected {
                    return Ok(err(dim_error(expected, &dynamic.dim)));
                }
            }
            Ok(ok(Value::Num(dynamic.as_scalar())))
        },
        | "check_with" => {
            let dim = expect_dim_value(&args, 0, name, span)?;
            let dynamic = expect_dyn(&args, 1, name, span)?;
            if dynamic.dim == dim {
                Ok(ok(Value::Num(dynamic.payload())))
            } else {
                Ok(err(dim_error(&dim, &dynamic.dim)))
            }
        },
        | "dim_of" => {
            let dynamic = expect_dyn(&args, 0, name, span)?;
            Ok(dim_value(&dynamic.dim))
        },
        | "value_of" => {
            let dynamic = expect_dyn(&args, 0, name, span)?;
            Ok(Value::Num(dynamic.payload()))
        },
        | "pow_dyn" => {
            let dynamic = expect_dyn(&args, 0, name, span)?;
            let exponent = expect_int(&args, 1, name, span)?;
            let payload = interp.apply_binary(
                "^",
                Value::Num(dynamic.payload()),
                Value::Num(NumVal::int(BigInt::from(exponent))),
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
        | "from_dense" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let (rows, cols, entries) = tensor_matrix(&tensor, name, span)?;
            let mut values = Vec::new();
            let mut columns = Vec::new();
            let mut row_pointer = Vec::with_capacity(rows + 1);
            row_pointer.push(Value::int(0));
            for row in 0..rows {
                for column in 0..cols {
                    let entry = entries[row * cols + column];
                    if entry != 0.0 {
                        values.push(entry);
                        columns.push(Value::int(i64::try_from(column).unwrap_or(0)));
                    }
                }
                row_pointer.push(Value::int(i64::try_from(values.len()).unwrap_or(0)));
            }
            let column_count = columns.len();
            let pointer_count = row_pointer.len();
            Ok(record_of(vec![
                ("format", Value::string("Csr")),
                ("rows", Value::int(i64::try_from(rows).unwrap_or(0))),
                ("cols", Value::int(i64::try_from(cols).unwrap_or(0))),
                (
                    "values",
                    float_tensor(vec![values.len()], values, tensor.dim.clone()),
                ),
                (
                    "col_indices",
                    build_tensor(columns, vec![column_count], Dim::one()),
                ),
                (
                    "row_ptr",
                    build_tensor(row_pointer, vec![pointer_count], Dim::one()),
                ),
            ]))
        },
        | "to_dense" => {
            let Value::Record(sparse) = args.first().cloned().unwrap_or(Value::Unit) else {
                return Err(EvalError::new("E0302", "`to_dense` expects a sparse matrix").at(span));
            };
            let rows = sparse
                .fields
                .get("rows")
                .and_then(Value::as_f64)
                .unwrap_or(0.0) as usize;
            let cols = sparse
                .fields
                .get("cols")
                .and_then(Value::as_f64)
                .unwrap_or(0.0) as usize;
            let values = sparse
                .fields
                .get("values")
                .and_then(|value| {
                    match value {
                        | Value::Tensor(tensor) => Some(tensor.clone()),
                        | _ => None,
                    }
                })
                .ok_or_else(|| {
                    EvalError::new("E0306", "the sparse matrix has no `values`").at(span)
                })?;
            let indices = sparse
                .fields
                .get("col_indices")
                .and_then(|value| {
                    match value {
                        | Value::Tensor(tensor) => Some(tensor.clone()),
                        | _ => None,
                    }
                })
                .ok_or_else(|| {
                    EvalError::new("E0306", "the sparse matrix has no `col_indices`").at(span)
                })?;
            let row_ptr = sparse
                .fields
                .get("row_ptr")
                .and_then(|value| {
                    match value {
                        | Value::Tensor(tensor) => Some(tensor.clone()),
                        | _ => None,
                    }
                })
                .ok_or_else(|| {
                    EvalError::new("E0306", "the sparse matrix has no `row_ptr`").at(span)
                })?;
            let numbers = tensor_values(&values)?;
            let pointers: Vec<usize> = row_ptr
                .elements()
                .iter()
                .map(|value| value.as_f64().unwrap_or(0.0) as usize)
                .collect();
            let mut dense = vec![0.0; rows * cols];
            for row in 0..rows {
                let start = pointers.get(row).copied().unwrap_or(0);
                let end = pointers.get(row + 1).copied().unwrap_or(start);
                for entry in start..end {
                    let column = indices
                        .elements()
                        .get(entry)
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0) as usize;
                    if column < cols {
                        dense[row * cols + column] = numbers.get(entry).copied().unwrap_or(0.0);
                    }
                }
            }
            Ok(float_tensor(vec![rows, cols], dense, values.dim.clone()))
        },
        | "nnz" => {
            let Some(Value::Record(sparse)) = args.first() else {
                return Err(EvalError::new("E0302", "`nnz` expects a sparse matrix").at(span));
            };
            let count = sparse
                .fields
                .get("values")
                .and_then(|value| {
                    match value {
                        | Value::Tensor(tensor) => Some(tensor.storage.len()),
                        | _ => None,
                    }
                })
                .unwrap_or(0);
            Ok(Value::int(i64::try_from(count).unwrap_or(i64::MAX)))
        },

        // ----- §8.7 statistics --------------------------------------------
        | "median" | "mode" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            if let Some(mut exact) = exact_values(&tensor) {
                let value = if name == "median" {
                    aevia_std::stats::median_exact(&mut exact)
                } else {
                    aevia_std::stats::mode_exact(&exact)
                }
                .map_err(|error| numeric_error(name, error, span))?;
                return Ok(exact_number(value, tensor.dim.clone()));
            }
            let mut values = tensor_values(&tensor)?;
            let value = if name == "median" {
                aevia_std::stats::median(&mut values)
            } else {
                aevia_std::stats::mode(&values)
            }
            .map_err(|error| numeric_error(name, error, span))?;
            Ok(Value::Num(NumVal::float(value, tensor.dim.clone())))
        },
        | "quantile" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let fraction = expect_num(&args, 1, name, span)?;
            if let (Some(mut exact), Some(q)) = (exact_values(&tensor), fraction.to_rational()) {
                let value = aevia_std::stats::quantile_exact(&exact, &q)
                    .map_err(|error| numeric_error(name, error, span))?;
                exact.sort();
                return Ok(exact_number(value, tensor.dim.clone()));
            }
            let q = fraction.to_f64().ok_or_else(|| {
                EvalError::new("E0806", "`quantile` needs a concrete fraction").at(span)
            })?;
            let value = aevia_std::stats::quantile(&tensor_values(&tensor)?, q)
                .map_err(|error| numeric_error(name, error, span))?;
            Ok(Value::Num(NumVal::float(value, tensor.dim.clone())))
        },
        | "cov" => {
            let left = expect_tensor(&args, 0, name, span)?;
            let right = expect_tensor(&args, 1, name, span)?;
            let dim = left.dim.mul(&right.dim);
            if let (Some(a), Some(b)) = (exact_values(&left), exact_values(&right)) {
                let value = aevia_std::stats::cov_exact(&a, &b, false)
                    .map_err(|error| numeric_error(name, error, span))?;
                return Ok(exact_number(value, dim));
            }
            let value =
                aevia_std::stats::cov(&tensor_values(&left)?, &tensor_values(&right)?, false)
                    .map_err(|error| numeric_error(name, error, span))?;
            Ok(Value::Num(NumVal::float(value, dim)))
        },
        | "corr" => {
            let left = expect_tensor(&args, 0, name, span)?;
            let right = expect_tensor(&args, 1, name, span)?;
            let value =
                aevia_std::stats::corr(&tensor_values(&left)?, &tensor_values(&right)?, false)
                    .map_err(|error| numeric_error(name, error, span))?;
            Ok(Value::Num(NumVal::float(value, Dim::one())))
        },
        | "skewness" | "kurtosis" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let values = tensor_values(&tensor)?;
            let value = if name == "skewness" {
                aevia_std::stats::skewness(&values)
            } else {
                aevia_std::stats::kurtosis(&values)
            }
            .map_err(|error| numeric_error(name, error, span))?;
            Ok(Value::Num(NumVal::float(value, Dim::one())))
        },
        | "histogram" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let bins = expect_int(&args, 1, name, span)?.max(1) as usize;
            let (counts, edges) = aevia_std::stats::histogram(&tensor_values(&tensor)?, bins)
                .map_err(|error| numeric_error(name, error, span))?;
            let counts: Vec<Value> = counts.into_iter().map(Value::int).collect();
            let len = counts.len();
            let dim = tensor.dim.clone();
            Ok(record_of(vec![
                ("counts", build_tensor(counts, vec![len], Dim::one())),
                ("edges", float_tensor(vec![edges.len()], edges, dim)),
            ]))
        },

        // ----- §8.5 numeric calculus --------------------------------------
        | "numeric_deriv" => {
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
        | "quadrature" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 3, 0, span))?;
            let lower = expect_num(&args, 1, name, span)?;
            let upper = expect_num(&args, 2, name, span)?;
            let (a, b) = (
                lower.to_f64().unwrap_or(f64::NAN),
                upper.to_f64().unwrap_or(f64::NAN),
            );
            let mut failure = None;
            let codomain = apply_scalar_value(interp, &function, a, &lower.dim, span, &mut failure);
            if let Some(error) = failure.take() {
                return Err(error);
            }
            let mut f =
                |value: f64| apply_scalar(interp, &function, value, &lower.dim, span, &mut failure);
            let outcome = aevia_std::calc::quadrature(&mut f, a, b);
            if let Some(error) = failure {
                return Err(error);
            }
            let dim = crate::value::value_dim(&codomain).mul(&lower.dim);
            Ok(match outcome {
                | Ok(value) => ok(Value::Num(NumVal::float(value, dim))),
                | Err(error) => err(Value::string(error.to_string())),
            })
        },
        | "product" | "product_dyn" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 3, 0, span))?;
            let lower = expect_num(&args, 1, name, span)?;
            let upper = expect_num(&args, 2, name, span)?;
            let (a, b) = match (lower.to_rational(), upper.to_rational()) {
                | (Some(a), Some(b)) => (a, b),
                | _ => {
                    return Err(EvalError::new(
                        "E1001",
                        "`product` needs numeric bounds; a symbolic one needs the \
                         `core.symbolic` capability",
                    )
                    .at(span));
                },
            };
            let mut index = a;
            let mut total = Value::Num(NumVal::int(BigInt::one()));
            let mut guard = 0;
            while index <= b {
                guard += 1;
                if guard > 1_000_000 {
                    return Err(
                        EvalError::new("E0808", "`product` over more than 10^6 terms").at(span),
                    );
                }
                let argument = exact_number(index.clone(), lower.dim.clone());
                let term = interp.apply(function.clone(), vec![argument], span)?;
                total = interp.apply_binary("*", total, term, span)?;
                index += BigRational::one();
            }
            Ok(total)
        },
        | "sum_f" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 3, 0, span))?;
            let lower = expect_num(&args, 1, name, span)?;
            let upper = expect_num(&args, 2, name, span)?;
            let (a, b) = (
                lower.to_f64().unwrap_or(f64::NAN).round() as i64,
                upper.to_f64().unwrap_or(f64::NAN).round() as i64,
            );
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
            }
            if let Some(error) = failure {
                return Err(error);
            }
            Ok(Value::Num(NumVal::float(
                total,
                crate::value::value_dim(&codomain),
            )))
        },
        | "grad" | "grad_f" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let point = expect_tensor(&args, 1, name, span)?;
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
        | "jacobian" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let point = expect_tensor(&args, 1, name, span)?;
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
        | "hessian" | "laplacian" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let point = expect_tensor(&args, 1, name, span)?;
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
        | "divergence" | "curl" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let point = expect_tensor(&args, 1, name, span)?;
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
        | "solve_complex" => {
            let (variable, applied) = apply_to_fresh(interp, name, &args, span, Dim::one())?;
            let symbol = symbolic_variable(&variable, Dim::one());
            match solve("solve_complex", &applied, &symbol) {
                | Ok(solutions) => Ok(ok(solutions_value(&solutions, Dim::one()))),
                | Err(error) => Ok(err(Value::string(error.message))),
            }
        },
        | "nsolve_f" | "root" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let start = expect_num(&args, 1, name, span)?;
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
        | "solve_system" | "least_squares" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let point = expect_tensor(&args, 1, name, span)?;
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
        | "minimize" | "maximize" | "minimize_f" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 2, 0, span))?;
            let point = expect_tensor(&args, 1, name, span)?;
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
        | "pdf" | "cdf" => {
            let distribution = distribution_from(&args, name, span)?;
            let point = expect_num(&args, 1, name, span)?;
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
        | "sample" => {
            let distribution = distribution_from(&args, name, span)?;
            let dim = distribution_dim(&args, span)?;
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
        | "sample_n" => {
            let distribution = distribution_from(&args, name, span)?;
            let count = expect_int(&args, 1, name, span)?.max(0) as usize;
            let dim = distribution_dim(&args, span)?;
            // §8.7.1 types the third argument as `Option<Int>`: a *seed*.
            // A seeded call is reproducible and does not perturb the session's
            // sequence; `None` (or a missing argument, or the `()` shape a
            // direct dispatch uses) draws from the session's RNG, which is the
            // historical behaviour. Ignoring the seed made `sample_n` depend on
            // whatever happened to run before it, which is not a pure function.
            let mut seed = sample_seed(&args, span)?;
            let mut values = Vec::with_capacity(count);
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
        | "fft" | "ifft" | "fft_norm" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let mut input = complex_values(&tensor)?;
            let norm = if name == "fft_norm" {
                let Some(Value::Variant(variant)) = args.get(1) else {
                    return Err(
                        EvalError::new("E0302", "`fft_norm` expects a `Norm` value").at(span)
                    );
                };
                match variant.name.as_str() {
                    | "Forward" => aevia_std::fft::Norm::Forward,
                    | "Unitary" => aevia_std::fft::Norm::Unitary,
                    | _ => aevia_std::fft::Norm::Backward,
                }
            } else {
                aevia_std::fft::Norm::Backward
            };
            let output = match name {
                | "ifft" => aevia_std::fft::ifft(&input),
                | "fft_norm" => aevia_std::fft::fft_norm(&input, norm),
                | _ => aevia_std::fft::fft(&input),
            };
            input.clear();
            let len = output.len();
            Ok(complex_tensor(vec![len], &output, tensor.dim.clone()))
        },
        | "rfft" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let signal = tensor_values(&tensor)?;
            let spectrum = aevia_std::fft::rfft(&signal);
            let len = spectrum.len();
            Ok(complex_tensor(vec![len], &spectrum, tensor.dim.clone()))
        },
        | "irfft" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let spectrum = complex_values(&tensor)?;
            let signal = aevia_std::fft::irfft(&spectrum, 2 * (spectrum.len().saturating_sub(1)));
            let len = signal.len();
            Ok(float_tensor(vec![len], signal, tensor.dim.clone()))
        },
        | "fftfreq" => {
            let count = expect_int(&args, 0, name, span)?.max(0) as usize;
            let spacing = expect_num(&args, 1, name, span)?;
            let d = spacing.to_f64().unwrap_or(1.0);
            let frequencies = aevia_std::fft::fftfreq(count, d);
            let len = frequencies.len();
            Ok(float_tensor(vec![len], frequencies, spacing.dim.inverse()))
        },
        | "interp1" | "spline" => {
            let xs = expect_tensor(&args, 0, name, span)?;
            let ys = expect_tensor(&args, 1, name, span)?;
            let at = expect_num(&args, 2, name, span)?;
            let x = at.to_f64().unwrap_or(f64::NAN);
            let outcome = if name == "spline" {
                aevia_std::interp::spline(&tensor_values(&xs)?, &tensor_values(&ys)?, x)
            } else {
                aevia_std::interp::interp1(&tensor_values(&xs)?, &tensor_values(&ys)?, x)
            };
            let value = outcome.map_err(|error| numeric_error(name, error, span))?;
            Ok(Value::Num(NumVal::float(value, ys.dim.clone())))
        },
        | "interp2" => {
            let xs = expect_tensor(&args, 0, name, span)?;
            let ys = expect_tensor(&args, 1, name, span)?;
            let grid = expect_tensor(&args, 2, name, span)?;
            let at_x = expect_num(&args, 3, name, span)?;
            let at_y = expect_num(&args, 4, name, span)?;
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
        | "polyfit" | "chebyshev_fit" => {
            let xs = expect_tensor(&args, 0, name, span)?;
            let ys = expect_tensor(&args, 1, name, span)?;
            let degree = expect_int(&args, 2, name, span)?.max(0) as usize;
            let outcome = if name == "polyfit" {
                aevia_std::interp::polyfit(&tensor_values(&xs)?, &tensor_values(&ys)?, degree)
            } else {
                aevia_std::interp::chebyshev_fit(&tensor_values(&xs)?, &tensor_values(&ys)?, degree)
            };
            let coefficients = outcome.map_err(|error| numeric_error(name, error, span))?;
            let len = coefficients.len();
            Ok(float_tensor(vec![len], coefficients, ys.dim.clone()))
        },
        | "polyval" => {
            let coefficients = expect_tensor(&args, 0, name, span)?;
            let at = expect_num(&args, 1, name, span)?;
            let values = tensor_values(&coefficients)?;
            let x = at.to_f64().unwrap_or(f64::NAN);
            Ok(Value::Num(NumVal::float(
                aevia_std::interp::polyval(&values, x),
                coefficients.dim.clone(),
            )))
        },

        // ----- §8.9 ODE ---------------------------------------------------
        | "odeint" | "odeint_with" => {
            let function = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 3, 0, span))?;
            let initial = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 3, 1, span))?;
            let times = expect_tensor(&args, 2, name, span)?;
            let grid = tensor_values(&times)?;
            let options = if name == "odeint_with" {
                ode_options(&args, 3, span)?
            } else {
                aevia_std::ode::Options::default()
            };
            let event = if name == "odeint_with" {
                ode_event(&args, 3, span)?
            } else {
                None
            };
            let (grid, states) = run_ode(
                interp, function, &initial, &grid, &times.dim, options, event, name, span,
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
        | "solve_ivp" => {
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
            let options = ode_options(&args, 3, span)?;
            let samples = options.steps.max(2);
            let time_dim = interval
                .fields
                .get("0")
                .map_or_else(Dim::one, crate::value::value_dim);
            let grid: Vec<f64> = (0..samples)
                .map(|index| start + (end - start) * index as f64 / (samples - 1) as f64)
                .collect();
            let event = ode_event(&args, 3, span)?;
            let (grid, states) = run_ode(
                interp, function, &initial, &grid, &time_dim, options, event, name, span,
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

        // ----- §8.9.1 stochastic differential equations -------------------
        | "wiener" | "sde_order" | "sde_ensemble_seed" | "sdeint_diag" | "sdeint_general" => {
            sde_call(interp, name, &args, span)
        },

        // ----- §8.10 PDE --------------------------------------------------
        | "solve_poisson" => {
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
            let iterations = spec
                .fields
                .get("iterations")
                .and_then(Value::as_f64)
                .map_or(10_000.0, |value| value.max(1.0)) as usize;
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
        | "solve_heat" | "solve_wave" | "solve_advection" => {
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
            let steps = number("steps", 10.0).max(0.0) as usize;
            let dt = number("dt", 0.01);
            let initial = field_of("initial")?;
            let outcome = match name {
                | "solve_heat" => {
                    aevia_std::pde::solve_heat(&aevia_std::pde::HeatSpec {
                        grid,
                        alpha: number("alpha", 1.0),
                        steps,
                        dt,
                        initial,
                    })
                    .map(|frames| frames.iter().map(field_value).collect::<Vec<_>>())
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
        | "read_npy" => {
            let path = expect_str(&args, 0, name, span)?;
            let extension = std::path::Path::new(&path)
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if extension != "npy" {
                return Ok(err(Value::string(format!(
                    "E0904: `read_npy` reads .npy files, not `{}`",
                    if extension.is_empty() {
                        "a file with no extension"
                    } else {
                        &extension
                    }
                ))));
            }
            Ok(Value::Io(Rc::new(crate::value::IoAction::ReadNpy(path))))
        },
        | "write_npy" => {
            let path = expect_str(&args, 0, name, span)?;
            let extension = std::path::Path::new(&path)
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if extension != "npy" {
                return Ok(err(Value::string(format!(
                    "E0904: `write_npy` writes .npy files, not `{}`",
                    if extension.is_empty() {
                        "a file with no extension"
                    } else {
                        &extension
                    }
                ))));
            }
            let value = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            let Value::Tensor(tensor) = value else {
                return Ok(err(Value::string(format!(
                    "E0902: `write_npy` needs a tensor, found {}",
                    value.type_name()
                ))));
            };
            match npy_encode(&tensor) {
                | Ok(bytes) => {
                    Ok(Value::Io(Rc::new(crate::value::IoAction::WriteNpy {
                        path,
                        bytes,
                    })))
                },
                | Err(message) => Ok(err(Value::string(message))),
            }
        },
        | "read_csv" | "read_csv_dyn" => {
            let path = expect_str(&args, 0, name, span)?;
            // Appendix B E0904: an extension this reader cannot decode is a
            // typed `Err`, not a generic parse failure later.
            let extension = std::path::Path::new(&path)
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !matches!(extension.as_str(), "csv" | "tsv") {
                return Ok(err(Value::string(format!(
                    "E0904: `read_csv` reads .csv/.tsv files, not `{}`",
                    if extension.is_empty() {
                        "a file with no extension"
                    } else {
                        &extension
                    }
                ))));
            }
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
                    let result = match aevia_std::table::parse_csv(&text) {
                        | Ok(table) => ok(table_value(&table)),
                        | Err(error) => err(Value::string(format!("E0903: {error}"))),
                    };
                    let _ = span;
                    Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(result))))
                }),
            })))
        },
        | "write_csv" => {
            let path = expect_str(&args, 0, name, span)?;
            let table = table_from(&args, 1, name, span)?;
            let text = aevia_std::table::write_csv(&table);
            Ok(Value::Io(Rc::new(crate::value::IoAction::Bind {
                action: Rc::new(crate::value::IoAction::WriteFile { path, contents: text }),
                func: native(|_interp, values, _span| {
                    // Propagate a failed write with its taxonomy code instead of
                    // reporting success.
                    if let Some(Value::Variant(result)) = values.first() {
                        if result.adt == adt::RESULT_ID && result.index == 1 {
                            let failed = result.fields.first().cloned().unwrap_or_else(|| {
                                Value::string("E0901: the file could not be written")
                            });
                            return Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(err(
                                failed,
                            )))));
                        }
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
        | "to_json" => args.first().cloned().ok_or_else(|| arity(name, 1, 0, span)),
        | "from_json" => {
            let value = args
                .first()
                .cloned()
                .ok_or_else(|| arity(name, 1, 0, span))?;
            Ok(ok(value))
        },
        | "read_json" => {
            let path = expect_str(&args, 0, name, span)?;
            let extension = std::path::Path::new(&path)
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if extension != "json" {
                return Ok(err(Value::string(format!(
                    "E0904: `read_json` reads .json files, not `{}`",
                    if extension.is_empty() {
                        "a file with no extension"
                    } else {
                        &extension
                    }
                ))));
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
        | "write_json" => {
            let path = expect_str(&args, 0, name, span)?;
            let value = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            let json = json_from_value(&value).map_err(|error| error.at(span))?;
            let text = aevia_std::table::write_json(&json);
            Ok(Value::Io(Rc::new(crate::value::IoAction::Bind {
                action: Rc::new(crate::value::IoAction::WriteFile { path, contents: text }),
                func: native(|_interp, values, _span| {
                    if let Some(Value::Variant(result)) = values.first() {
                        if result.adt == adt::RESULT_ID && result.index == 1 {
                            let failed = result.fields.first().cloned().unwrap_or_else(|| {
                                Value::string("E0901: the file could not be written")
                            });
                            return Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(err(
                                failed,
                            )))));
                        }
                    }
                    Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(ok(
                        Value::Unit,
                    )))))
                }),
            })))
        },
        | "column" | "column_dyn" => {
            let table = table_from(&args, 0, name, span)?;
            let column = expect_str(&args, 1, name, span)?;
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
        | "row" => {
            let table = table_from(&args, 0, name, span)?;
            let index = expect_int(&args, 1, name, span)?;
            let normalized = if index < 0 {
                table.len() as i64 + index
            } else {
                index
            };
            if normalized < 0 || normalized as usize >= table.len() {
                return Ok(err(Value::string("row index out of range")));
            }
            Ok(ok(row_value(&table, normalized as usize)))
        },
        | "select" => {
            let table = table_from(&args, 0, name, span)?;
            let names = expect_tensor(&args, 1, name, span)?;
            let mut selected = Vec::new();
            for name in names.elements() {
                if let Value::Str(text) = name {
                    selected.push(text.to_string());
                }
            }
            let mut columns = Vec::new();
            let mut units = Vec::new();
            for column in &selected {
                let index = table.columns.iter().position(|name| name == column);
                columns.push(column.clone());
                units.push(index.and_then(|index| table.units[index].clone()));
            }
            let mut rows = Vec::with_capacity(table.len());
            for row in 0..table.len() {
                let mut cells = Vec::with_capacity(selected.len());
                for column in &selected {
                    let index = table.columns.iter().position(|name| name == column);
                    cells.push(
                        index
                            .and_then(|index| table.rows[row].get(index).cloned())
                            .unwrap_or_default(),
                    );
                }
                rows.push(cells);
            }
            Ok(table_value(&aevia_std::table::Table {
                columns,
                units,
                rows,
            }))
        },
        | "schema" => {
            let table = table_from(&args, 0, name, span)?;
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
        | "to_table" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
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
        | "to_dynamic" => {
            let table = table_from(&args, 0, name, span)?;
            Ok(table_value(&table))
        },

        // ----- §8.12 plotting ---------------------------------------------
        | "line_with" | "scatter_with" | "hist" | "hist_with" | "heatmap" | "heatmap_with"
        | "errorbar" | "errorbar_with" => {
            let mut fields = IndexMap::new();
            fields.insert("kind".to_string(), Value::string(name));
            match name {
                | "hist" | "hist_with" => {
                    let values = expect_tensor(&args, 0, name, span)?;
                    let bins = expect_int(&args, 1, name, span)?.max(1) as usize;
                    let (counts, edges) =
                        aevia_std::stats::histogram(&tensor_values(&values)?, bins)
                            .map_err(|error| numeric_error(name, error, span))?;
                    let counts: Vec<Value> = counts
                        .into_iter()
                        .map(|count| Value::float(count as f64))
                        .collect();
                    let len = counts.len();
                    fields.insert(
                        "x".to_string(),
                        float_tensor(vec![edges.len()], edges.clone(), values.dim.clone()),
                    );
                    fields.insert("y".to_string(), build_tensor(counts, vec![len], Dim::one()));
                },
                | "heatmap" | "heatmap_with" => {
                    let values = expect_tensor(&args, 0, name, span)?;
                    fields.insert("x".to_string(), Value::Tensor(values));
                    fields.insert("y".to_string(), Value::Unit);
                },
                | _ => {
                    let x = args.first().cloned().unwrap_or(Value::Unit);
                    let y = args.get(1).cloned().unwrap_or(Value::Unit);
                    fields.insert("x".to_string(), x);
                    fields.insert("y".to_string(), y);
                    if name.starts_with("errorbar") {
                        let error = args.get(2).cloned().unwrap_or(Value::Unit);
                        fields.insert("error".to_string(), error);
                    }
                },
            }
            fields.insert("label".to_string(), Value::string(""));
            Ok(Value::Record(Rc::new(RecordValue { fields })))
        },
        | "default_plot_opts" => {
            Ok(record_of(vec![
                ("label", none()),
                ("color", none()),
                ("xunit", none()),
                ("yunit", none()),
                (
                    "xscale",
                    make_variant(
                        adt::intern(adt::Adt {
                            name: "AxisScale".to_string(),
                            params: Vec::new(),
                            ctors: Vec::new(),
                        }),
                        0,
                        "Linear",
                        Vec::new(),
                    ),
                ),
                (
                    "yscale",
                    make_variant(
                        adt::intern(adt::Adt {
                            name: "AxisScale".to_string(),
                            params: Vec::new(),
                            ctors: Vec::new(),
                        }),
                        0,
                        "Linear",
                        Vec::new(),
                    ),
                ),
                ("xlim", none()),
                ("ylim", none()),
            ]))
        },

        // ----- §8.17 simulation -------------------------------------------
        | "simulate" => {
            let scenario = expect_str(&args, 0, name, span)?;
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
        | "device_of" => {
            let _ = expect_tensor(&args, 0, name, span)?;
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
        | "to_device" => {
            let tensor = expect_tensor(&args, 0, name, span)?;
            let device = args.get(1).cloned().unwrap_or(Value::Unit);
            let name_of_device = match &device {
                | Value::Variant(variant) => variant.name.clone(),
                | _ => "Cpu".to_string(),
            };
            if name_of_device == "Cpu" {
                return Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(
                    Value::Tensor(tensor),
                ))));
            }
            Err(EvalError::new(
                "E1001",
                format!(
                    "`to_device` needs the `core.gpu` capability, which this build does \
                     not have (`{name_of_device}`)"
                ),
            )
            .at(span))
        },
        | "on" => {
            let device = args.first().cloned().unwrap_or(Value::Unit);
            let thunk = args
                .get(1)
                .cloned()
                .ok_or_else(|| arity(name, 2, 1, span))?;
            let name_of_device = match &device {
                | Value::Variant(variant) => variant.name.clone(),
                | _ => "Cpu".to_string(),
            };
            if name_of_device != "Cpu" {
                return Err(EvalError::new(
                    "E1001",
                    format!(
                        "`on` needs the `core.gpu` capability, which this build does \
                         not have (`{name_of_device}`)"
                    ),
                )
                .at(span));
            }
            let value = interp.apply(thunk, Vec::new(), span)?;
            Ok(Value::Io(Rc::new(crate::value::IoAction::Pure(value))))
        },

        // ----- §8.19 dynamic boundary -------------------------------------
        | "type_rep" => {
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
        let next = counter.get() + 1;
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

/// Wraps a fallible kernel answer in `Result<_, MathError>`.
///
/// A heavy request is a simplification boundary (§6.3.3), so an answer that is
/// a *closed* symbolic form is folded to the number it denotes: `limit(sin(t)/t,
/// 0)` answering `cos(0)` is the same answer as `1`, and leaving the unevaluated
/// form to the program would make the result depend on which phase the argument
/// happened to be written in (an exact `0` and a float `0.0` must agree).
/// Answers with free symbols are untouched.
fn math_result(result: Result<Value, EvalError>) -> Result<Value, EvalError> {
    Ok(match result {
        | Ok(value) => ok(fold_closed(value)),
        | Err(error) => err(Value::string(error.message)),
    })
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
    let mut indices = vec![0usize; arity_count];
    loop {
        let arguments: Vec<Value> = indices
            .iter()
            .map(|index| samples[*index].clone())
            .collect();
        let outcome = interp.apply(function.clone(), arguments, span);
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
        // Odometer over the cartesian product of the samples.
        let mut position = 0;
        loop {
            if position == arity_count {
                return if undecidable {
                    Err(
                        EvalError::new("E0804", "`forall` sampled a proposition it cannot decide")
                            .at(span),
                    )
                } else {
                    Ok(Value::Bool(true))
                };
            }
            indices[position] += 1;
            if indices[position] < samples.len() {
                break;
            }
            indices[position] = 0;
            position += 1;
        }
    }
}

/// `sum(f, a, b)` (§8.5).
///
/// The bound is exact: `f` is applied at every integer in `[a, b]` and the
/// values are added with the same promotion rules as `+`. A symbolic or
/// non-integral bound needs the kernel's summation and is `E1001`.
fn sum_range(
    interp: &mut Interp,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let function = args[0].clone();
    let lower = expect_int(args, 1, "sum", span)?;
    let upper = expect_int(args, 2, "sum", span)?;
    if upper < lower {
        return Ok(Value::Num(NumVal::int(BigInt::zero())));
    }
    let count = upper - lower + 1;
    if count > 1_000_000 {
        return Err(EvalError::new("E0808", "`sum` over more than 10^6 terms").at(span));
    }
    let mut total = Value::Num(NumVal::int(BigInt::zero()));
    for step in lower..=upper {
        let term = interp.apply(function.clone(), vec![Value::int(step)], span)?;
        total = interp.apply_binary("+", total, term, span)?;
    }
    Ok(total)
}

/// `gamma(x)`: the Gamma function, not its logarithm (§8.2).
///
/// A positive integer is exactly `(n-1)!`, so `gamma(5)` is `24` and not
/// `3.17805` (which is `ln 24`).
fn gamma(
    _interp: &mut Interp,
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let num = expect_num(args, 0, name, span)?;
    match &num.kind {
        | NumKind::Int(value) => {
            match value.to_i64() {
                | Some(n) if (1..=2000).contains(&n) => {
                    let mut result = BigInt::one();
                    for factor in 2..n {
                        result *= BigInt::from(factor);
                    }
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
            gamma(_interp, name, &[Value::Num(integer)], span)
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
    let mut count: usize = 1;
    for extent in shape {
        count = count
            .checked_mul(*extent)
            .ok_or_else(|| EvalError::new("E0802", "tensor shape is too large").at(span))?;
        if count > 100_000_000 {
            return Err(EvalError::new("E0802", "tensor shape is too large").at(span));
        }
    }
    Ok(count)
}

fn gcd(
    a: i64,
    b: i64,
) -> i64 {
    let mut a = a.abs();
    let mut b = b.abs();
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    a
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
        | None => Err(arity(name, index + 1, args.len(), span)),
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

fn expect_int(
    args: &[Value],
    index: usize,
    name: &str,
    span: Span,
) -> Result<i64, EvalError> {
    let num = expect_num(args, index, name, span)?;
    num.to_f64()
        .map(|value| value as i64)
        .ok_or_else(|| EvalError::new("E0302", format!("`{name}` expects an Int")).at(span))
}

fn expect_str(
    args: &[Value],
    index: usize,
    name: &str,
    span: Span,
) -> Result<String, EvalError> {
    match args.get(index) {
        | Some(Value::Str(text)) => Ok(text.to_string()),
        | _ => Err(arity(name, index + 1, args.len(), span)),
    }
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
        | None => Err(arity(name, index + 1, args.len(), span)),
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
        | _ => Err(arity(name, index + 1, args.len(), span)),
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
        | None => Err(arity(name, index + 1, args.len(), span)),
    }
}

/// The `DimValue` of a dimension (§4.9.2).
///
/// `DimValue` is opaque to the checker, so the runtime picks the
/// representation: a record holding the canonical text of the dimension
/// (`L/T`, `L^2`). It round-trips through `Dim::parse_plain`, which is what
/// makes `dyn_with` and `check_with` total.
fn dim_value(dim: &Dim) -> Value {
    record_of(vec![("dimension", Value::string(dim.to_plain_string()))])
}

fn as_dim_value(value: &Value) -> Option<Dim> {
    let Value::Record(record) = value else {
        return None;
    };
    let Some(Value::Str(text)) = record.fields.get("dimension") else {
        return None;
    };
    Dim::parse_plain(text).ok()
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
    _interp: &mut Interp,
    args: &[Value],
    newline: bool,
    stderr: bool,
    span: Span,
) -> Result<Value, EvalError> {
    let value = args.first().ok_or_else(|| arity("print", 1, 0, span))?;
    let text = show(value);
    Ok(Value::Io(Rc::new(crate::value::IoAction::Print {
        text,
        newline,
        stderr,
    })))
}

enum MathKind {
    Root(u32),
    Float(fn(f64) -> f64),
}

/// The symbolic node for an elementary function.
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
        | MathKind::Root(degree) => {
            let exponent = BigRational::new(BigInt::one(), BigInt::from(degree));
            let dim = num.dim.pow(&exponent);
            match &num.kind {
                | NumKind::Float(value) if *value < 0.0 => {
                    // §6.3.4: a negative radicand is a formal term, never NaN
                    // and never an error — `Real`'s roots are formal.
                    let rational = num
                        .to_rational()
                        .ok_or_else(|| domain_error(name, &num, span))?;
                    Ok(Value::Num(NumVal::symbolic(
                        Term::pow(Term::Num(rational), exponent),
                        dim,
                    )))
                },
                | NumKind::Float(value) => {
                    let result = value.powf(1.0 / f64::from(degree));
                    if result.is_nan() && !value.is_nan() {
                        return Err(domain_error(name, &num, span));
                    }
                    Ok(Value::Num(NumVal::float(result, dim)))
                },
                | NumKind::Complex(..) => Err(domain_error(name, &num, span)),
                | NumKind::Sym(term) => {
                    Ok(Value::Num(NumVal::symbolic(
                        Term::pow((**term).clone(), exponent),
                        dim,
                    )))
                },
                | NumKind::Int(_) | NumKind::Exact(_) => {
                    let rational = num
                        .to_rational()
                        .ok_or_else(|| domain_error(name, &num, span))?;
                    if let Some(root) = exact_root(&rational, degree) {
                        return Ok(Value::Num(NumVal::exact(root, dim)));
                    }
                    // No exact root: `sqrt(2)` and `sqrt(-1)` are formal terms
                    // (§6.3.4), never a float and never an error.
                    Ok(Value::Num(NumVal::symbolic(
                        Term::pow(Term::Num(rational), exponent),
                        dim,
                    )))
                },
            }
        },
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
            if is_exact_zero(num) {
                return Ok(Some(negative_infinity(interp, &num.dim, span)?));
            }
        },
        | "exp" => {
            match infinity_sign(num) {
                | Some(1) => return Ok(Some(Value::Num(num.clone()))),
                | Some(-1) => {
                    return Ok(Some(Value::Num(NumVal::exact(
                        BigRational::zero(),
                        num.dim.clone(),
                    ))));
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

fn is_exact_zero(num: &NumVal) -> bool {
    match &num.kind {
        | NumKind::Int(value) => value.is_zero(),
        | NumKind::Exact(value) => value.is_zero(),
        | _ => false,
    }
}

/// The `Real` value `-oo` in the same dimension as `dim`.
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

fn exact_root(
    value: &BigRational,
    degree: u32,
) -> Option<BigRational> {
    if value.is_negative() {
        return None;
    }
    let numerator = value.numer().to_f64()?;
    let denominator = value.denom().to_f64()?;
    let num_root = numerator.powf(1.0 / f64::from(degree)).round();
    let den_root = denominator.powf(1.0 / f64::from(degree)).round();
    if num_root.powi(degree as i32) == numerator && den_root.powi(degree as i32) == denominator {
        Some(BigRational::new(
            BigInt::from(num_root as i64),
            BigInt::from(den_root as i64),
        ))
    } else {
        None
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
fn one_of(tensor: &TensorData) -> NumVal {
    if tensor.dense_flat().is_some() {
        NumVal::float(1.0, tensor.dim.clone())
    } else {
        NumVal::int(BigInt::one())
    }
}

fn sum_axis(
    interp: &mut Interp,
    tensor: &TensorData,
    axis: usize,
    span: Span,
) -> Result<Value, EvalError> {
    if axis >= tensor.rank() {
        return Err(EvalError::new("E0802", "axis out of range").at(span));
    }
    let mut shape = tensor.shape.clone();
    shape[axis] = 1;
    let count = TensorData::element_count(&shape);
    // §6.6.6: the empty reduction is the phase's zero — `0.0` for a `Float`
    // tensor, the exact `0` for an `Exact`/boxed one.
    if tensor.shape[axis] == 0 {
        let zero = Value::Num(zero_of(tensor));
        return Ok(build_tensor(vec![zero; count], shape, tensor.dim.clone()));
    }
    // §9.13: the dense `f64` fast path. `dense_flat` borrows a dense buffer or a
    // contiguous window and materialises a strided view once at the kernel
    // boundary, so the fold below walks plain `f64`s instead of boxing a
    // `Value` per element. The compensated (Neumaier) step is the same one
    // `value::kahan_sum` performs, so the class stays unobservable.
    if let Some(values) = tensor.dense_flat() {
        let strides = crate::value::row_major_strides(&tensor.shape);
        let axis_stride = strides[axis];
        let extent = tensor.shape[axis];
        let mut out = Vec::with_capacity(count);
        for index in 0..count {
            // The base index in the *source* buffer, computed without a
            // per-output coordinate vector: for a 512×512 axis-0 reduction the
            // unravel allocation was one allocation per output.
            let mut base = 0usize;
            let mut remainder = index;
            for dim in (0..shape.len()).rev() {
                let extent = shape[dim].max(1);
                let coordinate = remainder % extent;
                remainder /= extent;
                if dim != axis {
                    base += coordinate * strides[dim];
                }
            }
            let mut total = 0.0f64;
            let mut compensation = 0.0f64;
            for step in 0..extent {
                let value = values[base + step * axis_stride];
                let next = total + value;
                if total.abs() >= value.abs() {
                    compensation += (total - next) + value;
                } else {
                    compensation += (value - next) + total;
                }
                total = next;
            }
            out.push(Value::Num(NumVal::float(
                total + compensation,
                tensor.dim.clone(),
            )));
        }
        return Ok(build_tensor(out, shape, tensor.dim.clone()));
    }
    sum_axis_elementwise(interp, tensor, axis, &shape, count, span)
}

/// The non-`f64` axis reduction: exact elements keep exact arithmetic, and a
/// boxed `Float` class is folded with the same compensated sum as the dense
/// path so the storage class cannot change an answer.
fn sum_axis_elementwise(
    interp: &mut Interp,
    tensor: &TensorData,
    axis: usize,
    shape: &[usize],
    count: usize,
    span: Span,
) -> Result<Value, EvalError> {
    let elements = tensor.elements();
    let all_float = elements.iter().all(|element| {
        matches!(
            element,
            Value::Num(NumVal {
                kind: NumKind::Float(_),
                ..
            })
        )
    });
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let coordinates = unravel(index, shape);
        if all_float {
            let mut group: Vec<f64> = Vec::with_capacity(tensor.shape[axis]);
            for step in 0..tensor.shape[axis] {
                let mut point = coordinates.clone();
                point[axis] = step;
                let point: Vec<isize> = point.iter().map(|value| *value as isize).collect();
                if let Some(Value::Num(num)) = tensor.get(&point) {
                    group.push(num.to_f64().unwrap_or(f64::NAN));
                }
            }
            // A one-element reduction is the element itself, display unit
            // included; longer groups take the compensated sum.
            let value = if tensor.shape[axis] == 1 {
                let mut point = coordinates.clone();
                point[axis] = 0;
                let point: Vec<isize> = point.iter().map(|value| *value as isize).collect();
                match tensor.get(&point) {
                    | Some(Value::Num(only)) => only,
                    | _ => NumVal::float(crate::value::kahan_sum(&group), tensor.dim.clone()),
                }
            } else {
                NumVal::float(crate::value::kahan_sum(&group), tensor.dim.clone())
            };
            out.push(Value::Num(value));
            continue;
        }
        let mut total: Option<Value> = None;
        for step in 0..tensor.shape[axis] {
            let mut point = coordinates.clone();
            point[axis] = step;
            let point: Vec<isize> = point.iter().map(|value| *value as isize).collect();
            if let Some(element) = tensor.get(&point) {
                total = Some(match total {
                    | None => element,
                    | Some(acc) => interp.apply_binary("+", acc, element, span)?,
                });
            }
        }
        out.push(total.unwrap_or_else(|| Value::Num(zero_of(tensor))));
    }
    Ok(build_tensor(out, shape.to_vec(), tensor.dim.clone()))
}

fn unravel(
    mut index: usize,
    shape: &[usize],
) -> Vec<usize> {
    let mut coordinates = vec![0; shape.len()];
    for axis in (0..shape.len()).rev() {
        let extent = shape[axis].max(1);
        coordinates[axis] = index % extent;
        index /= extent;
    }
    coordinates
}

fn outer_product(
    interp: &mut Interp,
    left: Value,
    right: Value,
    span: Span,
) -> Result<Value, EvalError> {
    let (Value::Tensor(a), Value::Tensor(b)) = (&left, &right) else {
        return Err(EvalError::new("E0302", "`outer` expects two vectors").at(span));
    };
    let mut elements = Vec::with_capacity(a.shape[0] * b.shape[0]);
    for x in a.elements() {
        for y in b.elements() {
            elements.push(interp.apply_binary("*", x.clone(), y.clone(), span)?);
        }
    }
    let dim = elements
        .first()
        .map_or_else(Dim::one, crate::value::value_dim);
    Ok(build_tensor(elements, vec![a.shape[0], b.shape[0]], dim))
}

fn determinant(
    interp: &mut Interp,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let tensor = expect_tensor(args, 0, "det", span)?;
    if tensor.rank() != 2 || tensor.shape[0] != tensor.shape[1] {
        return Err(EvalError::new("E0802", "`det` expects a square matrix").at(span));
    }
    let n = tensor.shape[0];
    if n == 0 {
        return Ok(Value::Num(NumVal::int(BigInt::one())));
    }
    let det_dim = tensor.dim.pow(&BigRational::from_integer(BigInt::from(n)));
    // §6.6.5: an exact matrix has an exact determinant. Copying into `f64`
    // made `det([[1,2],[3,4]])` a `Float` and `det([[1,2],[2,4]])` `-0`.
    if let Some(mut matrix) = exact_matrix(&tensor) {
        return Ok(exact_number(determinant_exact(&mut matrix), det_dim));
    }
    let mut matrix = vec![vec![0.0; n]; n];
    for row in 0..n {
        for column in 0..n {
            matrix[row][column] = tensor
                .get(&[row as isize, column as isize])
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
        }
    }
    let _ = interp;
    let determinant = match aevia_std::linalg::det(n, n, &flattened(&matrix)) {
        | Ok(value) => value,
        // The dispatcher cannot express this call; the elimination it replaced
        // still answers. `det` has no error case beyond the shape already
        // checked, so this arm is the fallback and not a result.
        | Err(_) => gaussian_determinant(&mut matrix),
    };
    Ok(Value::Num(NumVal::float(determinant, det_dim)))
}

/// A square `f64` matrix as the row-major slice the linear-algebra kernels take.
fn flattened(matrix: &[Vec<f64>]) -> Vec<f64> {
    let mut out = Vec::with_capacity(matrix.len() * matrix.len());
    for row in matrix {
        out.extend_from_slice(row);
    }
    out
}

/// The matrix as exact rationals, when every element is exact.
///
/// The bound keeps the O(n^3) exact elimination from dominating a session;
/// above it the `f64` path answers, which is the documented trade-off of
/// §6.6.5 (`faer` for the numeric path is a separate work item).
fn exact_matrix(tensor: &TensorData) -> Option<Vec<Vec<BigRational>>> {
    let n = tensor.shape[0];
    if n > 64 {
        return None;
    }
    let mut matrix = Vec::with_capacity(n);
    for row in 0..n {
        let mut values = Vec::with_capacity(n);
        for column in 0..n {
            let element = tensor.get(&[row as isize, column as isize])?;
            values.push(element.as_num()?.to_rational()?);
        }
        matrix.push(values);
    }
    Some(matrix)
}

/// A rational as the narrowest runtime scalar: `Int` when it is integral.
fn exact_number(
    value: BigRational,
    dim: Dim,
) -> Value {
    if value.denom().is_one() {
        Value::Num(NumVal {
            kind: NumKind::Int(value.numer().clone()),
            dim,
            display: None,
        })
    } else {
        Value::Num(NumVal::exact(value, dim))
    }
}

fn determinant_exact(matrix: &mut [Vec<BigRational>]) -> BigRational {
    let n = matrix.len();
    let mut determinant = BigRational::one();
    for column in 0..n {
        let pivot = (column..n).find(|row| !matrix[*row][column].is_zero());
        let Some(pivot) = pivot else {
            return BigRational::zero();
        };
        if pivot != column {
            matrix.swap(pivot, column);
            determinant = -determinant;
        }
        let lead = matrix[column][column].clone();
        determinant *= &lead;
        for row in column + 1..n {
            let factor = matrix[row][column].clone() / &lead;
            for inner in column..n {
                let term = factor.clone() * matrix[column][inner].clone();
                matrix[row][inner] -= term;
            }
        }
    }
    determinant
}

/// Gauss-Jordan over the rationals; `None` when the matrix is singular.
fn invert_exact(matrix: &[Vec<BigRational>]) -> Option<Vec<Vec<BigRational>>> {
    let n = matrix.len();
    let mut augmented: Vec<Vec<BigRational>> = (0..n)
        .map(|row| {
            let mut values = Vec::with_capacity(2 * n);
            values.extend(matrix[row].iter().cloned());
            values.extend((0..n).map(|column| {
                if row == column {
                    BigRational::one()
                } else {
                    BigRational::zero()
                }
            }));
            values
        })
        .collect();
    if !gauss_jordan_exact(&mut augmented, n) {
        return None;
    }
    Some(augmented.into_iter().map(|row| row[n..].to_vec()).collect())
}

fn gauss_jordan_exact(
    matrix: &mut [Vec<BigRational>],
    columns: usize,
) -> bool {
    let rows = matrix.len();
    for column in 0..columns {
        let pivot = (column..rows).find(|row| !matrix[*row][column].is_zero());
        let Some(pivot) = pivot else {
            return false;
        };
        matrix.swap(pivot, column);
        let lead = matrix[column][column].clone();
        for value in &mut matrix[column] {
            *value = &*value / &lead;
        }
        for row in 0..rows {
            if row == column || matrix[row][column].is_zero() {
                continue;
            }
            let factor = matrix[row][column].clone();
            for inner in 0..matrix[row].len() {
                let term = factor.clone() * matrix[column][inner].clone();
                matrix[row][inner] -= term;
            }
        }
    }
    true
}

fn gaussian_determinant(matrix: &mut [Vec<f64>]) -> f64 {
    let n = matrix.len();
    let mut determinant = 1.0;
    for column in 0..n {
        let mut pivot = column;
        for row in column + 1..n {
            if matrix[row][column].abs() > matrix[pivot][column].abs() {
                pivot = row;
            }
        }
        if matrix[pivot][column].abs() < f64::EPSILON {
            return 0.0;
        }
        if pivot != column {
            matrix.swap(pivot, column);
            determinant = -determinant;
        }
        determinant *= matrix[column][column];
        for row in column + 1..n {
            let factor = matrix[row][column] / matrix[column][column];
            for inner in column..n {
                matrix[row][inner] -= factor * matrix[column][inner];
            }
        }
    }
    determinant
}

fn inverse(
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let tensor = expect_tensor(args, 0, "inv", span)?;
    if tensor.rank() != 2 || tensor.shape[0] != tensor.shape[1] {
        return Ok(err(linalg_error_value(&aevia_std::NumericError::NotSquare)));
    }
    let n = tensor.shape[0];
    // §6.6.5: the inverse of an exact matrix is exact. `inv([[1,2],[3,4]])`
    // used to answer with four `f64`s.
    if let Some(matrix) = exact_matrix(&tensor) {
        let Some(inverted) = invert_exact(&matrix) else {
            return Ok(err(linalg_error_value(&aevia_std::NumericError::Singular)));
        };
        let dim = tensor.dim.inverse();
        let mut elements = Vec::with_capacity(n * n);
        for row in inverted {
            for value in row {
                elements.push(exact_number(value, dim.clone()));
            }
        }
        return Ok(ok(build_tensor(elements, vec![n, n], dim)));
    }
    let matrix: Vec<Vec<f64>> = (0..n)
        .map(|row| {
            (0..n)
                .map(|column| {
                    tensor
                        .get(&[row as isize, column as isize])
                        .and_then(|value| value.as_f64())
                        .unwrap_or(0.0)
                })
                .collect()
        })
        .collect();
    let inverse = match aevia_std::linalg::inv(n, n, &flattened(&matrix)) {
        | Ok(values) => values,
        | Err(aevia_std::NumericError::Singular) => {
            return Ok(err(linalg_error_value(&aevia_std::NumericError::Singular)));
        },
        // The dispatcher cannot express this call; the Gauss-Jordan it replaced
        // still answers, and reports singularity the way it always did.
        | Err(_) => {
            let mut augmented: Vec<Vec<f64>> = (0..n)
                .map(|row| {
                    let mut values = Vec::with_capacity(2 * n);
                    values.extend(matrix[row].iter().copied());
                    for column in 0..n {
                        values.push(if row == column { 1.0 } else { 0.0 });
                    }
                    values
                })
                .collect();
            if !gauss_jordan(&mut augmented) {
                return Ok(err(linalg_error_value(&aevia_std::NumericError::Singular)));
            }
            let mut values = Vec::with_capacity(n * n);
            for row in 0..n {
                for column in 0..n {
                    values.push(augmented[row][n + column]);
                }
            }
            values
        },
    };
    let mut elements = Vec::with_capacity(n * n);
    for value in inverse {
        elements.push(Value::Num(NumVal::float(value, tensor.dim.inverse())));
    }
    Ok(ok(build_tensor(elements, vec![n, n], tensor.dim.inverse())))
}

fn gauss_jordan(matrix: &mut [Vec<f64>]) -> bool {
    let rows = matrix.len();
    let columns = matrix.first().map_or(0, Vec::len) / 2;
    for column in 0..columns {
        let mut pivot = column;
        for row in column + 1..rows {
            if matrix[row][column].abs() > matrix[pivot][column].abs() {
                pivot = row;
            }
        }
        if matrix[pivot][column].abs() < f64::EPSILON {
            return false;
        }
        matrix.swap(pivot, column);
        let scale = matrix[column][column];
        for value in &mut matrix[column] {
            *value /= scale;
        }
        for row in 0..rows {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            if factor == 0.0 {
                continue;
            }
            for inner in 0..matrix[row].len() {
                matrix[row][inner] -= factor * matrix[column][inner];
            }
        }
    }
    true
}

fn linsolve(
    interp: &mut Interp,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let matrix = expect_tensor(args, 0, "linsolve", span)?;
    let vector = expect_tensor(args, 1, "linsolve", span)?;
    if matrix.rank() != 2 || vector.rank() != 1 || matrix.shape[0] != matrix.shape[1] {
        return Ok(err(linalg_error_value(&aevia_std::NumericError::NotSquare)));
    }
    let n = matrix.shape[0];
    if vector.shape[0] != n {
        return Ok(err(linalg_error_value(&aevia_std::NumericError::NotSquare)));
    }
    let dim = vector.dim.div(&matrix.dim);
    // §6.6.5: an exact system has an exact solution.
    if let Some(rows) = exact_matrix(&matrix) {
        let mut augmented: Vec<Vec<BigRational>> = (0..n)
            .map(|row| {
                let mut values = Vec::with_capacity(n + 1);
                values.extend(rows[row].iter().cloned());
                let entry = vector
                    .get(&[row as isize])
                    .and_then(|value| value.as_num().and_then(NumVal::to_rational));
                values.push(entry.unwrap_or_else(BigRational::zero));
                values
            })
            .collect();
        if !gauss_jordan_exact(&mut augmented, n) {
            return Ok(err(linalg_error_value(&aevia_std::NumericError::Singular)));
        }
        let elements: Vec<Value> = (0..n)
            .map(|row| exact_number(augmented[row][n].clone(), dim.clone()))
            .collect();
        return Ok(ok(build_tensor(elements, vec![n], dim)));
    }
    let coefficients: Vec<Vec<f64>> = (0..n)
        .map(|row| {
            (0..n)
                .map(|column| {
                    matrix
                        .get(&[row as isize, column as isize])
                        .and_then(|value| value.as_f64())
                        .unwrap_or(0.0)
                })
                .collect()
        })
        .collect();
    let rhs: Vec<f64> = (0..n)
        .map(|row| {
            vector
                .get(&[row as isize])
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0)
        })
        .collect();
    let solution = match aevia_std::linalg::solve(n, n, &flattened(&coefficients), &rhs) {
        | Ok(values) => values,
        | Err(aevia_std::NumericError::Singular) => {
            return Ok(err(linalg_error_value(&aevia_std::NumericError::Singular)));
        },
        // The dispatcher cannot express this call; the Gauss-Jordan it replaced
        // still answers, and reports singularity the way it always did.
        | Err(_) => {
            let mut augmented: Vec<Vec<f64>> = (0..n)
                .map(|row| {
                    let mut values = Vec::with_capacity(n + 1);
                    values.extend(coefficients[row].iter().copied());
                    values.push(rhs[row]);
                    values
                })
                .collect();
            if !gauss_jordan(&mut augmented) {
                return Ok(err(linalg_error_value(&aevia_std::NumericError::Singular)));
            }
            (0..n).map(|row| augmented[row][n]).collect()
        },
    };
    let _ = interp;
    let elements: Vec<Value> = solution
        .into_iter()
        .map(|value| Value::Num(NumVal::float(value, dim.clone())))
        .collect();
    Ok(ok(build_tensor(elements, vec![n], dim)))
}

fn value_to_term(value: &Value) -> Result<Term, EvalError> {
    match value {
        | Value::Num(num) => {
            match &num.kind {
                | NumKind::Int(value) => Ok(Term::Num(BigRational::from_integer(value.clone()))),
                | NumKind::Exact(value) => Ok(Term::Num(value.clone())),
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

fn render_plot(spec: &Value) -> String {
    let mut polylines = String::new();
    if let Value::Record(record) = spec {
        if let Some(Value::Tensor(series)) = record.fields.get("series") {
            for item in series.elements() {
                if let Value::Record(series) = item {
                    let label = series.fields.get("label").map_or_else(String::new, show);
                    let x = series.fields.get("x").cloned().unwrap_or(Value::Unit);
                    let y = series.fields.get("y").cloned().unwrap_or(Value::Unit);
                    if let (Value::Tensor(x), Value::Tensor(y)) = (&x, &y) {
                        let points: Vec<String> = x
                            .elements()
                            .iter()
                            .zip(y.elements())
                            .map(|(x, y)| {
                                format!(
                                    "{},{}",
                                    x.as_f64().unwrap_or(0.0),
                                    y.as_f64().unwrap_or(0.0)
                                )
                            })
                            .collect();
                        polylines.push_str(&format!(
                            "<polyline fill=\"none\" stroke=\"#c0392b\" stroke-width=\"2\" points=\"{}\"><title>{label}</title></polyline>\n",
                            points.join(" ")
                        ));
                    }
                }
            }
        }
    }
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Aevia plot</title></head>\
<body><svg viewBox=\"0 0 800 400\" width=\"800\" height=\"400\" style=\"background:#fff\">\
<rect x=\"40\" y=\"20\" width=\"720\" height=\"340\" fill=\"none\" stroke=\"#ccc\"/>\
{polylines}</svg></body></html>"
    )
}

/// A tensor's elements as complex numbers.
///
/// # Errors
///
/// `E0302` when an element is not a number.
fn complex_values(tensor: &TensorData) -> Result<Vec<aevia_std::Complex>, EvalError> {
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
#[derive(Clone, Debug)]
enum StateShape {
    /// A scalar with its dimension.
    Scalar(Dim),
    /// A tensor with its shape and element dimension.
    Tensor(Vec<usize>, Dim),
    /// A record, in field order.
    Record(Vec<(String, StateShape)>),
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

fn run_ode(
    interp: &mut Interp,
    function: Value,
    initial: &Value,
    grid: &[f64],
    time_dim: &Dim,
    options: aevia_std::ode::Options,
    event: Option<Value>,
    name: &str,
    span: Span,
) -> Result<(Vec<f64>, Vec<Vec<f64>>), EvalError> {
    let (shape, flat) = flatten_state(initial)?;
    let failure = std::cell::RefCell::new(None);
    let interp = std::cell::RefCell::new(interp);
    let mut rhs = |t: f64, state: &[f64]| {
        let args = vec![
            Value::Num(NumVal::float(t, time_dim.clone())),
            rebuild_state(&shape, state),
        ];
        match interp.borrow_mut().apply(function.clone(), args, span) {
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
        }
    };
    let solution = if let Some(event) = event {
        let mut event_fn = |t: f64, state: &[f64]| {
            let args = vec![
                Value::Num(NumVal::float(t, time_dim.clone())),
                rebuild_state(&shape, state),
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
    solution.map_err(|error| numeric_error(name, error, span))
}

// ----- §8.9.1 stochastic differential equations ---------------------------

use aevia_std::sde::SdeError;

// ----- §8.11 NPY codec -----------------------------------------------------

const NPY_MAGIC: &[u8] = b"\x93NUMPY";

/// Encodes a rank-1/rank-2 float tensor as a NumPy `.npy` file (version 1.0,
/// little-endian `float64`, C order).
///
/// The header is padded so the data starts on a 64-byte boundary, exactly as
/// NumPy writes it; `write_npy`'s round trip and the byte-exact test both rely
/// on this.
pub fn npy_encode(tensor: &crate::value::TensorData) -> Result<Vec<u8>, String> {
    if tensor.rank() > 2 {
        return Err(format!(
            "E0902: `write_npy` writes rank-1 or rank-2 tensors, not rank {}",
            tensor.rank()
        ));
    }
    let Some(values) = tensor.dense_flat() else {
        return Err("E0902: `write_npy` needs a dense float tensor".to_string());
    };
    let shape = if tensor.shape.is_empty() {
        "()".to_string()
    } else if tensor.shape.len() == 1 {
        format!("({},)", tensor.shape[0])
    } else {
        format!("({}, {})", tensor.shape[0], tensor.shape[1])
    };
    let mut header = format!("{{'descr': '<f8', 'fortran_order': False, 'shape': {shape}, }}");
    // magic(6) + version(2) + header length(2) + header must be a multiple of 64.
    let padding = (64 - ((10 + header.len() + 1) % 64)) % 64;
    header.push_str(&" ".repeat(padding));
    header.push('\n');
    let header_len = u16::try_from(header.len())
        .map_err(|_| "E0903: the NPY header is too large".to_string())?;
    let mut bytes = Vec::with_capacity(10 + header.len() + values.len() * 8);
    bytes.extend_from_slice(NPY_MAGIC);
    bytes.push(1); // major
    bytes.push(0); // minor
    bytes.extend_from_slice(&header_len.to_le_bytes());
    bytes.extend_from_slice(header.as_bytes());
    for value in values.iter() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    Ok(bytes)
}

/// Decodes a `.npy` file (§8.11).
///
/// # Errors
///
/// `E0903` for malformed/truncated data, `E0904` for an unsupported dtype or
/// Fortran order. The message carries the code, because the language-level
/// result is `Result<Tensor<…>, DataError>`.
pub fn npy_decode(bytes: &[u8]) -> Result<crate::value::TensorData, String> {
    if bytes.len() < 10 || &bytes[..6] != NPY_MAGIC {
        return Err("E0903: not a NumPy `.npy` file (bad magic)".to_string());
    }
    let major = bytes[6];
    let header_len = match major {
        | 1 => usize::from(u16::from_le_bytes([bytes[8], bytes[9]])),
        | 2 | 3 => {
            if bytes.len() < 12 {
                return Err("E0903: truncated NPY header".to_string());
            }
            usize::try_from(u32::from_le_bytes([
                bytes[8], bytes[9], bytes[10], bytes[11],
            ]))
            .unwrap_or(0)
        },
        | _ => {
            return Err(format!(
                "E0904: unsupported NPY version {major}.{}",
                bytes[7]
            ));
        },
    };
    if header_len > 10 * 1024 {
        return Err("E0903: the NPY header exceeds 10 KiB".to_string());
    }
    let start = if major == 1 { 10 } else { 12 };
    if bytes.len() < start + header_len {
        return Err("E0903: the NPY header is truncated".to_string());
    }
    let header = std::str::from_utf8(&bytes[start..start + header_len])
        .map_err(|_| "E0903: the NPY header is not UTF-8".to_string())?;

    let descr_start = header
        .find("'descr': '")
        .ok_or_else(|| "E0903: the NPY header has no `descr`".to_string())?
        + "'descr': '".len();
    let descr_end = header[descr_start..]
        .find('\'')
        .map(|offset| descr_start + offset)
        .ok_or_else(|| "E0903: the NPY `descr` is unterminated".to_string())?;
    let descr = &header[descr_start..descr_end];
    let width = match descr {
        | "<f8" | "|f8" | "=f8" => 8usize,
        | "<f4" | "|f4" | "=f4" => 4usize,
        | _ => {
            return Err(format!(
                "E0904: unsupported NPY dtype `{descr}` (only little-endian f4/f8)"
            ));
        },
    };
    if header.contains("'fortran_order': True") {
        return Err("E0904: Fortran-ordered NPY is not supported".to_string());
    }
    let shape_text = header
        .find("'shape': (")
        .ok_or_else(|| "E0903: the NPY header has no `shape`".to_string())?
        + "'shape': (".len();
    let shape_end = header[shape_text..]
        .find(')')
        .map(|offset| shape_text + offset)
        .ok_or_else(|| "E0903: the NPY `shape` is unterminated".to_string())?;
    let mut shape = Vec::new();
    for part in header[shape_text..shape_end].split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let extent = part
            .parse::<usize>()
            .map_err(|_| format!("E0903: bad NPY shape entry `{part}`"))?;
        shape.push(extent);
    }
    let count: usize = shape.iter().product();
    let data = &bytes[start + header_len..];
    let expected = count
        .checked_mul(width)
        .ok_or_else(|| "E0903: NPY shape overflows".to_string())?;
    if data.len() != expected {
        return Err(format!(
            "E0903: NPY data is {} byte(s), the shape needs {expected}",
            data.len()
        ));
    }
    let mut values = Vec::with_capacity(count);
    for chunk in data.chunks_exact(width) {
        let value = if width == 8 {
            f64::from_le_bytes(chunk.try_into().expect("eight bytes"))
        } else {
            f64::from(f32::from_le_bytes(chunk.try_into().expect("four bytes")))
        };
        values.push(value);
    }
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
        | _ => return None,
    })
}

/// The `SdeMethod` value the `SdePath` record reports.
fn sde_method_value(method: aevia_std::sde::Method) -> Value {
    let (index, name) = match method {
        | aevia_std::sde::Method::EulerMaruyama => (0, "EulerMaruyama"),
        | aevia_std::sde::Method::Milstein => (1, "Milstein"),
        | aevia_std::sde::Method::MilsteinStratonovich => (2, "MilsteinStratonovich"),
        | aevia_std::sde::Method::HeunStratonovich => (3, "HeunStratonovich"),
        | aevia_std::sde::Method::SrkPlaten => (4, "SrkPlaten"),
        | aevia_std::sde::Method::MilsteinNoncommutative => (5, "MilsteinNoncommutative"),
    };
    let id = adt::intern(adt::Adt {
        name: "SdeMethod".to_string(),
        params: Vec::new(),
        ctors: Vec::new(),
    });
    make_variant(id, index, name, Vec::new())
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
            let noise = variant
                .fields
                .first()
                .and_then(Value::as_f64)
                .ok_or_else(|| {
                    SdeError::Invalid("`General` carries its noise-channel count".to_string())
                })?;
            Ok(aevia_std::sde::Structure::General {
                noise: usize::try_from(noise.max(0.0) as u64).unwrap_or(0),
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

fn sde_int_field(
    record: &RecordValue,
    field: &str,
) -> Result<i64, SdeError> {
    record
        .fields
        .get(field)
        .and_then(Value::as_f64)
        .map(|value| value as i64)
        .ok_or_else(|| SdeError::Invalid(format!("the SDE options need `{field}`")))
}

fn sde_dim_field(
    record: &RecordValue,
    field: &str,
) -> Result<NumVal, SdeError> {
    match record.fields.get(field) {
        | Some(Value::Num(num)) => Ok(num.clone()),
        | _ => Err(SdeError::Invalid(format!("the SDE options need `{field}`"))),
    }
}

/// The `SdeOpts<T>` record as the standard library's `Options`, plus the time
/// dimension the path's `t` tensor reports.
fn sde_options(value: &Value) -> Result<(aevia_std::sde::Options, Dim), SdeError> {
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
    let method = match record.fields.get("method") {
        | Some(Value::Variant(method)) => {
            sde_method_named(&method.name).ok_or_else(|| {
                SdeError::Invalid(format!("`{}` is not an `SdeMethod`", method.name))
            })?
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
    let seed = sde_int_field(record, "seed")?.max(0) as u64;
    let terms = sde_int_field(record, "terms")?.max(0) as usize;
    let t0 = sde_dim_field(record, "t0")?;
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
                sde_int_field(record, "steps")?.max(0) as usize,
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
                sde_int_field(record, "max_steps")?.max(0) as usize,
                sde_int_field(record, "cells")?.max(0) as usize,
            )
            .with_terms(terms)
        },
        | other => {
            return Err(SdeError::Invalid(format!(
                "`{other}` is not a step policy (`Fixed`/`Adaptive`)"
            )));
        },
    };
    Ok((options, time_dim))
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
fn sde_call(
    interp: &mut Interp,
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    match name {
        | "wiener" => {
            let seed = expect_int(args, 0, name, span)?.max(0) as u64;
            let noise = expect_int(args, 1, name, span)?.max(0) as usize;
            let steps = expect_int(args, 2, name, span)?.max(0) as usize;
            let dt = expect_num(args, 3, name, span)?;
            let dt_value = dt.to_f64().ok_or_else(|| {
                EvalError::new("E0302", "`wiener` needs a concrete `dt`").at(span)
            })?;
            let increments = aevia_std::sde::wiener(seed, noise, steps, dt_value);
            let flat: Vec<f64> = increments.into_iter().flatten().collect();
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
        | "sde_ensemble_seed" => {
            let seed = expect_int(args, 0, name, span)?.max(0) as u64;
            let index = expect_int(args, 1, name, span)?.max(0) as usize;
            Ok(Value::Num(NumVal::int(BigInt::from(
                aevia_std::sde::ensemble_seed(seed, index),
            ))))
        },
        | _ => sde_integrate(interp, name, args, span),
    }
}

/// `sdeint_diag` / `sdeint_general` (§8.9.1).
fn sde_integrate(
    interp: &mut Interp,
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let drift = args
        .first()
        .cloned()
        .ok_or_else(|| arity(name, 4, 0, span))?;
    let diffusion = args
        .get(1)
        .cloned()
        .ok_or_else(|| arity(name, 4, 1, span))?;
    let initial = args
        .get(2)
        .cloned()
        .ok_or_else(|| arity(name, 4, 2, span))?;
    let options_value = args
        .get(3)
        .cloned()
        .ok_or_else(|| arity(name, 4, 3, span))?;
    let general = name == "sdeint_general";
    let (options, time_dim) = match sde_options(&options_value) {
        | Ok(pair) => pair,
        | Err(error) => return Ok(err(sde_error_value(&error))),
    };
    // The step-policy stubs carry a dimension-free `T` field only for typing;
    // the path reports the initial time's dimension.
    let (shape, flat) = flatten_state(&initial)?;
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
    if general && matches!(shape, StateShape::Record(_)) {
        return Ok(err(sde_error_value(&SdeError::Unsupported {
            capability: "sde.general.record",
            detail: "general noise with a record state is not statically expressible in this build"
                .to_string(),
        })));
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
    let drift_shape = shape.clone();
    let drift_time = time_dim.clone();
    let drift_value = drift;
    let drift_failure = &failure;
    let drift_interp = &interpreter;
    let drift_fn = move |t: f64, state: &[f64]| -> Result<Vec<f64>, SdeError> {
        let mut interp = drift_interp.borrow_mut();
        let argument = vec![
            Value::Num(NumVal::float(t, drift_time.clone())),
            rebuild_state(&drift_shape, state),
        ];
        match interp.apply(drift_value.clone(), argument, span) {
            | Ok(value) => {
                match flatten_state(&value) {
                    | Ok((_, rates)) if rates.len() == state.len() => Ok(rates),
                    | Ok((_, rates)) => {
                        Err(SdeError::Shape(format!(
                            "the drift returned {} rate(s) for a state of {}",
                            rates.len(),
                            state.len()
                        )))
                    },
                    | Err(error) => {
                        if drift_failure.borrow().is_none() {
                            *drift_failure.borrow_mut() = Some(error);
                        }
                        Err(SdeError::Invalid("the drift failed".to_string()))
                    },
                }
            },
            | Err(error) => {
                if drift_failure.borrow().is_none() {
                    *drift_failure.borrow_mut() = Some(error);
                }
                Err(SdeError::Invalid("the drift failed".to_string()))
            },
        }
    };
    let diffusion_shape = shape.clone();
    let diffusion_time = time_dim.clone();
    let diffusion_value = diffusion;
    let expected = coefficients;
    let diffusion_failure = &failure;
    let diffusion_interp = &interpreter;
    let diffusion_fn = move |t: f64, state: &[f64]| -> Result<Vec<f64>, SdeError> {
        let mut interp = diffusion_interp.borrow_mut();
        let argument = vec![
            Value::Num(NumVal::float(t, diffusion_time.clone())),
            rebuild_state(&diffusion_shape, state),
        ];
        match interp.apply(diffusion_value.clone(), argument, span) {
            | Ok(value) => {
                match flatten_state(&value) {
                    | Ok((_, values)) if values.len() == expected => Ok(values),
                    | Ok((_, values)) => {
                        Err(SdeError::Shape(format!(
                            "the diffusion returned {} coefficient(s); this structure needs {expected}",
                            values.len(),
                        )))
                    },
                    | Err(error) => {
                        if diffusion_failure.borrow().is_none() {
                            *diffusion_failure.borrow_mut() = Some(error);
                        }
                        Err(SdeError::Invalid("the diffusion failed".to_string()))
                    },
                }
            },
            | Err(error) => {
                if diffusion_failure.borrow().is_none() {
                    *diffusion_failure.borrow_mut() = Some(error);
                }
                Err(SdeError::Invalid("the diffusion failed".to_string()))
            },
        }
    };
    let result = aevia_std::sde::integrate(drift_fn, diffusion_fn, &flat, &options);
    let _interp = interpreter.into_inner();
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    match result {
        | Ok(path) => {
            let states: Vec<Value> = path
                .y
                .iter()
                .map(|state| rebuild_state(&shape, state))
                .collect();
            let len = states.len();
            Ok(ok(record_of(vec![
                (
                    "t",
                    float_tensor(vec![path.t.len()], path.t.clone(), time_dim),
                ),
                (
                    "y",
                    build_tensor(states, vec![len], crate::value::value_dim(&initial)),
                ),
                ("steps", Value::Num(NumVal::int(BigInt::from(path.steps)))),
                (
                    "rejected",
                    Value::Num(NumVal::int(BigInt::from(path.rejected))),
                ),
                ("nfev", Value::Num(NumVal::int(BigInt::from(path.nfev)))),
                ("seed", Value::Num(NumVal::int(BigInt::from(path.seed)))),
                ("noise", Value::Num(NumVal::int(BigInt::from(noise)))),
                ("method", sde_method_value(path.method)),
            ])))
        },
        | Err(error) => Ok(err(sde_error_value(&error))),
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
    let mut cursor = 0usize;
    rebuild_state_at(shape, flat, &mut cursor)
}

fn rebuild_state_at(
    shape: &StateShape,
    flat: &[f64],
    cursor: &mut usize,
) -> Value {
    match shape {
        | StateShape::Scalar(dim) => {
            let value = flat.get(*cursor).copied().unwrap_or(0.0);
            *cursor += 1;
            Value::Num(NumVal::float(value, dim.clone()))
        },
        | StateShape::Tensor(shape, dim) => {
            let count = TensorData::element_count(shape);
            let values: Vec<f64> = (0..count)
                .map(|index| flat.get(*cursor + index).copied().unwrap_or(0.0))
                .collect();
            *cursor += count;
            float_tensor(shape.clone(), values, dim.clone())
        },
        | StateShape::Record(fields) => {
            let mut map = IndexMap::new();
            for (name, field) in fields {
                map.insert(name.clone(), rebuild_state_at(field, flat, cursor));
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
    if let Some(rtol) = record.fields.get("rtol").and_then(Value::as_f64) {
        options.rtol = rtol;
    }
    if let Some(atol) = record.fields.get("atol").and_then(Value::as_f64) {
        options.atol = atol;
    }
    if let Some(value) = record.fields.get("steps") {
        let steps = match value {
            | Value::Variant(option) if option.adt == adt::OPTION_ID && option.name == "None" => {
                None
            },
            | Value::Variant(option)
                if option.adt == adt::OPTION_ID
                    && option.name == "Some"
                    && option.fields.len() == 1 =>
            {
                Some(option.fields[0].as_f64().ok_or_else(|| {
                    EvalError::new("E0302", "`steps` must be a positive integer").at(span)
                })?)
            },
            | Value::Unit => None,
            | other => {
                Some(other.as_f64().ok_or_else(|| {
                    EvalError::new("E0302", "`steps` must be an optional positive integer").at(span)
                })?)
            },
        };
        if let Some(steps) = steps {
            if !steps.is_finite()
                || steps < 1.0
                || steps.fract() != 0.0
                || steps >= usize::MAX as f64
            {
                return Err(EvalError::new("E0302", "`steps` must be a positive integer").at(span));
            }
            options.steps = steps as usize;
        }
    }
    let _ = ode_event(args, index, span)?;
    if let Some(Value::Variant(method)) = record.fields.get("method") {
        options.method = match method.name.as_str() {
            | "Rk4" => aevia_std::ode::Method::Rk4,
            | "Dopri5" => aevia_std::ode::Method::Dopri5,
            | "Radau" => aevia_std::ode::Method::Radau,
            | "Bdf" => aevia_std::ode::Method::Bdf,
            | "Rosenbrock23" => aevia_std::ode::Method::Rosenbrock23,
            | other => {
                return Err(
                    EvalError::new("E0302", format!("`{other}` is not an `OdeMethod`")).at(span),
                );
            },
        };
        if let Some(capability) = options.method.capability() {
            return Err(EvalError::new(
                "E1001",
                format!(
                    "the `{}` integrator needs the `{capability}` capability, which this \
                     build does not have; `Rk4`, `Dopri5` and `Radau` are available",
                    method.name
                ),
            )
            .at(span));
        }
    }
    Ok(options)
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
            Ok(Some(option.fields[0].clone()))
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
        grid.fields
            .get(field)
            .and_then(Value::as_f64)
            .map(|value| value.max(0.0) as usize)
            .ok_or_else(|| {
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
            float_tensor(
                vec![field.grid.ny, field.grid.nx],
                field.values.clone(),
                Dim::one(),
            ),
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
        | Value::Num(num) => {
            mix(state, b"n");
            match &num.kind {
                | NumKind::Sym(_) => {
                    mix(
                        state,
                        crate::value::repr(&Value::Num(num.clone())).as_bytes(),
                    )
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
        },
        | Value::Str(text) => {
            mix(state, b"s");
            mix(state, text.as_bytes());
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
            let mut names: Vec<&String> = record.fields.keys().collect();
            names.sort();
            for name in names {
                mix(state, name.as_bytes());
                hash_into(&record.fields[name], state)?;
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
            mix(state, &dynamic.dim.to_plain_string().into_bytes());
            hash_into(&Value::Num(dynamic.payload()), state)?;
        },
        | Value::Closure(_)
        | Value::Native(_)
        | Value::Ctor { .. }
        | Value::Io(_)
        | Value::TypeRep(_)
        | Value::Callback(_)
        | Value::KernelValue(_)
        | Value::KernelError(_)
        | Value::Error => {
            return None;
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
fn norm_of(
    values: &[f64],
    order: i64,
) -> f64 {
    match order {
        | 1 => values.iter().map(|value| value.abs()).sum::<f64>(),
        | i64::MAX => {
            values
                .iter()
                .fold(0.0_f64, |acc, value| acc.max(value.abs()))
        },
        | _ => values.iter().map(|value| value * value).sum::<f64>().sqrt(),
    }
}

/// A tensor's elements as `f64`.
///
/// # Errors
///
/// `E0302` when an element is not a number.
fn tensor_values(tensor: &TensorData) -> Result<Vec<f64>, EvalError> {
    // A contiguous `f64` buffer — dense, or a view with row-major strides —
    // is already the answer; a strided view is materialised here, and the
    // boxed loop below is the reference for every other storage class.
    if let Some(data) = tensor.dense_flat() {
        return Ok(data.into_owned());
    }
    let mut out = Vec::with_capacity(tensor.storage.len());
    for element in tensor.elements() {
        match element.as_f64() {
            | Some(value) => out.push(value),
            | None => {
                return Err(EvalError::new(
                    "E0302",
                    format!(
                        "expected a numeric tensor, found an element of type {}",
                        element.type_name()
                    ),
                ));
            },
        }
    }
    Ok(out)
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
fn numeric_error(
    name: &str,
    error: aevia_std::NumericError,
    span: Span,
) -> EvalError {
    let code = match &error {
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
fn lin_result(result: Result<Value, aevia_std::NumericError>) -> Value {
    match result {
        | Ok(value) => ok(value),
        | Err(error) => err(linalg_error_value(&error)),
    }
}

/// A tensor as `(rows, cols, row-major entries)`.
///
/// # Errors
///
/// `E0802` when the tensor is not rank 2.
fn tensor_matrix(
    tensor: &TensorData,
    name: &str,
    span: Span,
) -> Result<(usize, usize, Vec<f64>), EvalError> {
    if tensor.rank() != 2 {
        return Err(EvalError::new(
            "E0802",
            format!(
                "`{name}` expects a rank-2 tensor, found rank {}",
                tensor.rank()
            ),
        )
        .at(span));
    }
    Ok((tensor.shape[0], tensor.shape[1], tensor_values(tensor)?))
}

/// The flat index of a coordinate, or `None` when it is out of range.
fn flat_index(
    shape: &[usize],
    coordinates: &[isize],
) -> Option<usize> {
    if shape.len() != coordinates.len() {
        return None;
    }
    let mut flat = 0usize;
    for (index, extent) in coordinates.iter().zip(shape.iter()) {
        let extent = isize::try_from(*extent).ok()?;
        let normalized = if *index < 0 {
            extent + index
        } else {
            *index
        };
        if normalized < 0 || normalized >= extent {
            return None;
        }
        flat = flat * usize::try_from(extent).ok()? + usize::try_from(normalized).ok()?;
    }
    Some(flat)
}

/// A zero of the tensor's element representation and dimension.
fn zero_like(tensor: &TensorData) -> Value {
    match &tensor.storage {
        | Storage::Dense(_) | Storage::View(_) => {
            Value::Num(NumVal::float(0.0, tensor.dim.clone()))
        },
        | Storage::Exact(_) | Storage::TypedExact(_, ExactRep::Real) => {
            Value::Num(NumVal::exact(BigRational::zero(), tensor.dim.clone()))
        },
        | Storage::TypedExact(_, ExactRep::Int) => {
            Value::Num(NumVal {
                kind: NumKind::Int(BigInt::zero()),
                dim: tensor.dim.clone(),
                display: None,
            })
        },
        | Storage::Bool(_) => Value::Bool(false),
        | Storage::Boxed(_) => Value::Num(NumVal::exact(BigRational::zero(), tensor.dim.clone())),
    }
}

/// The elements as exact rationals, when every one of them is `Int`/`Real`.
///
/// A `Float` is not exact even though it has a rational value: keeping a float
/// sample's mean as a rational would change its phase (§6.3.2).
fn exact_values(tensor: &TensorData) -> Option<Vec<BigRational>> {
    // A contiguous `f64` tensor — dense, or a contiguous view — is all
    // `Float`, and `exact_values` exists precisely to exclude floats from the
    // exact path (§6.3.2). Skipping the walk is the same answer as walking it
    // and returning `None` at the first element.
    if tensor.is_contiguous() {
        return None;
    }
    let mut out = Vec::with_capacity(tensor.storage.len());
    for element in tensor.elements() {
        let num = element.as_num()?;
        match &num.kind {
            | NumKind::Int(value) => out.push(BigRational::from_integer(value.clone())),
            | NumKind::Exact(value) => out.push(value.clone()),
            | _ => return None,
        }
    }
    Some(out)
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

/// The distribution a `pdf`/`cdf`/`sample` argument describes (§8.7.1).
///
/// A distribution is a record, and its field names decide which one it is.
/// `ChiSquared` and `StudentT` have the same single field `df`, so a record
/// with only that field is ambiguous; §8.7.1's two types are structurally
/// identical, and the tie is broken towards `ChiSquared` (the first the
/// specification lists) unless a `kind` field says `"student_t"`.
fn distribution_from(
    args: &[Value],
    name: &str,
    span: Span,
) -> Result<aevia_std::dist::Distribution, EvalError> {
    use aevia_std::dist::Distribution;
    let Some(Value::Record(record)) = args.first() else {
        return Err(
            EvalError::new("E0302", format!("`{name}` expects a distribution record")).at(span),
        );
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
    let has = |field: &str| record.fields.contains_key(field);
    let kind = match record.fields.get("kind") {
        | Some(Value::Str(text)) => Some(text.to_string()),
        | _ => None,
    };
    if has("mu") && has("sigma") {
        return Ok(Distribution::Normal {
            mu: number("mu")?,
            sigma: number("sigma")?,
        });
    }
    if has("lo") && has("hi") {
        return Ok(Distribution::Uniform {
            lo: number("lo")?,
            hi: number("hi")?,
        });
    }
    if has("shape") && has("rate") {
        return Ok(Distribution::Gamma {
            shape: number("shape")?,
            rate: number("rate")?,
        });
    }
    if has("rate") {
        return Ok(Distribution::Exponential {
            rate: number("rate")?,
        });
    }
    if has("alpha") && has("beta") {
        return Ok(Distribution::Beta {
            alpha: number("alpha")?,
            beta: number("beta")?,
        });
    }
    if has("lambda") {
        return Ok(Distribution::Poisson {
            lambda: number("lambda")?,
        });
    }
    if has("n") && has("p") {
        let n = record
            .fields
            .get("n")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        return Ok(Distribution::Binomial {
            n: n.max(0.0) as u64,
            p: number("p")?,
        });
    }
    if has("df") {
        let df = number("df")?;
        return Ok(if kind.as_deref() == Some("student_t") {
            Distribution::StudentT { df }
        } else {
            Distribution::ChiSquared { df }
        });
    }
    Err(EvalError::new(
        "E0306",
        format!("`{name}` cannot tell which distribution this record describes"),
    )
    .at(span))
}

/// The dimension of a distribution's samples (§8.7.1).
fn distribution_dim(
    args: &[Value],
    span: Span,
) -> Result<Dim, EvalError> {
    let Some(Value::Record(record)) = args.first() else {
        return Err(EvalError::new("E0302", "expected a distribution record").at(span));
    };
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
fn classical_polynomial(
    args: &[Value],
    name: &str,
    span: Span,
) -> Result<Value, EvalError> {
    let order = expect_int(args, 0, name, span)?;
    let num = expect_num(args, 1, name, span)?;
    let order = u32::try_from(order.max(0)).unwrap_or(0);
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
    let value = match name {
        | "legendre_p" => aevia_std::special::legendre_p(order as i32, x),
        | "hermite_h" => aevia_std::special::hermite_h(order as i32, x),
        | "laguerre_l" => aevia_std::special::laguerre_l(order as i32, x),
        | "chebyshev_t" => aevia_std::special::chebyshev_t(order as i32, x),
        | _ => aevia_std::special::chebyshev_u(order as i32, x),
    };
    Ok(Value::Num(NumVal::float(value, num.dim)))
}

/// `join` on two tables, keyed by a column name (§8.11).
fn join_tables(
    args: &[Value],
    name: &str,
    span: Span,
) -> Result<Value, EvalError> {
    let left = table_from(&args, 0, name, span)?;
    let right = table_from(&args, 1, name, span)?;
    let key = expect_str(&args, 2, name, span)?;
    let Some(left_index) = left.columns.iter().position(|name| *name == key) else {
        return Ok(err(Value::string(format!(
            "the left table has no column `{key}`"
        ))));
    };
    let Some(right_index) = right.columns.iter().position(|name| *name == key) else {
        return Ok(err(Value::string(format!(
            "the right table has no column `{key}`"
        ))));
    };
    let mut columns = left.columns.clone();
    let mut units = left.units.clone();
    for (index, column) in right.columns.iter().enumerate() {
        if index == right_index {
            continue;
        }
        if columns.contains(column) {
            return Ok(err(Value::string(format!(
                "both tables have a column `{column}`"
            ))));
        }
        columns.push(column.clone());
        units.push(right.units[index].clone());
    }
    let mut rows = Vec::new();
    for left_row in &left.rows {
        let Some(value) = left_row.get(left_index) else {
            continue;
        };
        for right_row in &right.rows {
            if right_row.get(right_index) != Some(value) {
                continue;
            }
            let mut merged = left_row.clone();
            for (index, cell) in right_row.iter().enumerate() {
                if index != right_index {
                    merged.push(cell.clone());
                }
            }
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

    /// §6.9.2 / appendix B §B.10: each recoverable numeric failure has its own
    /// code. A non-converging iteration is `E0807`, not the shape/index
    /// `E0802` it used to share with every non-domain failure.
    #[test]
    fn numeric_kernel_failures_keep_their_documented_codes() {
        let code = |error| numeric_error("probe", error, Span::default()).code;
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
