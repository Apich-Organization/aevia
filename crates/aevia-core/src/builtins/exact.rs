//! Integer and rational arithmetic for the scalar prelude.
#![cfg_attr(
    not(test),
    deny(
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::nursery,
        clippy::panic,
        clippy::pedantic,
        clippy::single_call_fn,
        clippy::unwrap_used
    )
)]

use super::checked_element_count;
use super::domain_error;
use super::numeric_error;
use crate::eval::EvalError;
use crate::eval::Interp;
use crate::int::Int;
use crate::value::ExactRep;
use crate::value::NumKind;
use crate::value::NumVal;
use crate::value::Storage;
use crate::value::TensorData;
use crate::value::Value;
use aevia_dim::Dim;
use aevia_syntax::Span;
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::Euclid;
use num_traits::FromPrimitive;
use num_traits::Signed;
use num_traits::ToPrimitive;
use std::borrow::Cow;
use std::ops::Add;
use std::ops::Div;
use std::ops::Rem;
use std::ops::Sub;
use std::rc::Rc;

#[expect(
    clippy::single_call_fn,
    reason = "keeps checked small and arbitrary-precision division together"
)]
pub(super) fn divide(
    a: &Int,
    b: &Int,
    euclidean: bool,
    span: Span,
) -> Result<Value, EvalError> {
    if b.is_zero() {
        return Err(EvalError::new("E0801", "division by zero").at(span));
    }
    let value = match (a.as_i64(), b.as_i64()) {
        | (Some(x), Some(y)) if euclidean => x.checked_div_euclid(y),
        | (Some(x), Some(y)) => x.checked_div(y),
        | _ => None,
    };
    let value = value.map_or_else(
        || {
            let (x, y) = (a.to_big(), b.to_big());
            Int::from_big(if euclidean {
                x.div_euclid(&y)
            } else {
                x.div(y)
            })
        },
        Int::Small,
    );
    Ok(Value::Num(NumVal::int(value)))
}

#[expect(
    clippy::single_call_fn,
    reason = "keeps representation-dependent remainder out of the builtin classifier"
)]
pub(super) fn remainder(
    a: NumVal,
    b: &NumVal,
    euclidean: bool,
    span: Span,
) -> Result<Value, EvalError> {
    if b.is_zero() {
        return Err(EvalError::new("E0801", "modulo by zero").at(span));
    }
    if let (NumKind::Int(x), NumKind::Int(y)) = (&a.kind, &b.kind) {
        let value = match (x.as_i64(), y.as_i64()) {
            | (Some(x), Some(y)) if euclidean => x.checked_rem_euclid(y),
            | (Some(x), Some(y)) => x.checked_rem(y),
            | _ => None,
        };
        let value = value.map_or_else(
            || {
                let (x, y) = (x.to_big(), y.to_big());
                Int::from_big(if euclidean {
                    x.rem_euclid(&y)
                } else {
                    x.rem(y)
                })
            },
            Int::Small,
        );
        return Ok(Value::Num(NumVal {
            kind: NumKind::Int(value),
            display: None,
            dim: a.dim,
        }));
    }
    if matches!(&a.kind, NumKind::Int(_) | NumKind::Exact(_))
        && matches!(&b.kind, NumKind::Int(_) | NumKind::Exact(_))
        && let (Some(x), Some(y)) = (a.to_rational(), b.to_rational())
    {
        let mut value = x.rem(&y);
        if euclidean && value.is_negative() {
            value = value.add(y.abs());
        }
        return Ok(Value::Num(NumVal::exact(value, a.dim)));
    }
    let x = a.to_f64().unwrap_or(f64::NAN);
    let y = b.to_f64().unwrap_or(f64::NAN);
    let value = if euclidean {
        x.rem_euclid(y)
    } else {
        x.rem(y)
    };
    Ok(Value::Num(NumVal::float(value, a.dim)))
}

#[expect(
    clippy::single_call_fn,
    reason = "shares machine-word and arbitrary-precision number-theory paths"
)]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "the product of signed-word magnitudes fits in i128"
)]
pub(super) fn common_multiple_or_divisor(
    a: &Int,
    b: &Int,
    multiple: bool,
) -> Value {
    if let (Some(a), Some(b)) = (a.as_i64(), b.as_i64()) {
        let (a, b) = (a.unsigned_abs(), b.unsigned_abs());
        let divisor = a.gcd(&b);
        if !multiple {
            return Value::Num(NumVal::int(divisor));
        }
        let scaled = a.checked_div(divisor).unwrap_or(0);
        let result = scaled
            .checked_mul(b)
            .map_or_else(|| Int::from(i128::from(scaled) * i128::from(b)), Int::from);
        return Value::Num(NumVal::int(result));
    }
    let (a, b) = (borrow_big_integer(a), borrow_big_integer(b));
    let value = if multiple {
        a.lcm(b.as_ref())
    } else {
        a.gcd(b.as_ref())
    };
    Value::Num(NumVal::int(value))
}

#[expect(
    clippy::single_call_fn,
    reason = "keeps machine-word and arbitrary-precision modular powers together"
)]
pub(super) fn modular_power(
    base: &Int,
    exponent: &Int,
    modulus: &Int,
    span: Span,
) -> Result<Value, EvalError> {
    let exponent_word = match exponent {
        | Int::Small(value) => u64::try_from(*value).ok(),
        | Int::Big(value) => value.to_u64(),
    };
    let answer = match (base.as_i64(), exponent_word, modulus.as_i64()) {
        | (Some(base), Some(exponent), Some(modulus)) => {
            aevia_std::number::mod_pow(base, exponent, modulus).map(Int::Small)
        },
        | _ => {
            aevia_std::number::mod_pow_exact(
                &borrow_big_integer(base),
                &borrow_big_integer(exponent),
                &borrow_big_integer(modulus),
            )
            .map(Int::from_big)
        },
    }
    .map_err(|error| numeric_error("mod_pow", error, span))?;
    Ok(Value::Num(NumVal::int(answer)))
}

#[expect(
    clippy::single_call_fn,
    reason = "keeps precision-preserving scalar conversions together"
)]
pub(super) fn truncate(
    num: &NumVal,
    span: Span,
) -> Result<Value, EvalError> {
    let value = match &num.kind {
        | NumKind::Int(value) => value.clone(),
        | NumKind::Exact(value) => Int::from_big(value.to_integer()),
        | _ => {
            let value = num
                .to_f64()
                .and_then(BigInt::from_f64)
                .ok_or_else(|| domain_error("to_int", num, span))?;
            Int::from_big(value)
        },
    };
    Ok(Value::Num(NumVal::int(value)))
}

#[expect(
    clippy::single_call_fn,
    reason = "isolates exact range sizing and bounded materialization"
)]
pub(super) fn range(
    interp: &Interp,
    mut current: Int,
    end: &Int,
    step: &Int,
    span: Span,
) -> Result<Value, EvalError> {
    if step.is_zero() {
        return Err(EvalError::new("E0802", "range step must not be zero").at(span));
    }
    let count = if (step.is_positive() && &current < end) || (step.is_negative() && &current > end)
    {
        let steps = match (current.as_i64(), end.as_i64(), step.as_i64()) {
            | (Some(start), Some(end), Some(step)) => {
                usize::try_from(start.abs_diff(end).div_ceil(step.unsigned_abs())).ok()
            },
            | _ => {
                let end = borrow_big_integer(end);
                let start = borrow_big_integer(&current);
                let step = borrow_big_integer(step);
                let (_, gap) = end.as_ref().sub(start.as_ref()).into_parts();
                gap.div_ceil(step.magnitude()).to_usize()
            },
        };
        let steps = steps
            .ok_or_else(|| EvalError::new("E0802", "range contains too many elements").at(span))?;
        checked_element_count(&[steps], span)?
    } else {
        0
    };
    interp.check_cancelled().map_err(|error| error.at(span))?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| EvalError::new("E0802", "the range does not fit in memory").at(span))?;
    let check_cancelled = |index: usize| -> Result<(), EvalError> {
        if index.is_multiple_of(256) {
            interp.check_cancelled().map_err(|error| error.at(span))?;
        }
        Ok(())
    };
    let mut indices = 0..count;
    if let Some(last) = indices.next_back() {
        for index in indices {
            check_cancelled(index)?;
            values.push(current.to_rational());
            current = current.add(step);
        }
        // The exclusive endpoint needs no successor, which can be much wider
        // than the emitted values when a large step overshoots a short range.
        check_cancelled(last)?;
        values.push(current.to_rational());
    }
    Ok(Value::Tensor(Rc::new(TensorData {
        verified_element: None,
        shape: vec![count],
        storage: Storage::TypedExact(values, ExactRep::Int),
        dim: Dim::one(),
        display: None,
    })))
}

fn borrow_big_integer(value: &Int) -> Cow<'_, BigInt> {
    match value {
        | Int::Small(value) => Cow::Owned(BigInt::from(*value)),
        | Int::Big(value) => Cow::Borrowed(value),
    }
}
