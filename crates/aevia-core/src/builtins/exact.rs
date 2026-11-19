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
pub(super) fn common_multiple_or_divisor(
    a: &Int,
    b: &Int,
    multiple: bool,
) -> Value {
    if let (Some(a), Some(b)) = (a.as_i64(), b.as_i64()) {
        let (a, b) = (a.unsigned_abs(), b.unsigned_abs());
        let divisor = a.gcd(&b);
        let value = if !multiple {
            Some(divisor)
        } else if divisor == 0 {
            Some(0)
        } else {
            a.checked_div(divisor)
                .and_then(|scaled| scaled.checked_mul(b))
        };
        if let Some(value) = value {
            return Value::Num(NumVal::int(Int::from(value)));
        }
    }
    let (a, b) = (a.to_big(), b.to_big());
    let value = if multiple {
        a.lcm(&b)
    } else {
        a.gcd(&b)
    };
    Value::Num(NumVal::int(value))
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
        let gap = end.to_big().sub(current.to_big()).abs();
        let steps = gap
            .div_ceil(&step.to_big().abs())
            .to_usize()
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
    for index in 0..count {
        if index.is_multiple_of(256) {
            interp.check_cancelled().map_err(|error| error.at(span))?;
        }
        values.push(current.to_rational());
        current = current.add(step);
    }
    Ok(Value::Tensor(Rc::new(TensorData {
        verified_element: None,
        shape: vec![count],
        storage: Storage::TypedExact(values, ExactRep::Int),
        dim: Dim::one(),
        display: None,
    })))
}
