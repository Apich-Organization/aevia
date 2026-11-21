//! The runtime representation of `Int`.
//!
//! Almost every integer a program computes fits in a machine word, but the
//! language promises arbitrary precision (§3.4.1). [`Int`] keeps the common
//! case inline — no allocation, no `gcd`, no vector arithmetic — and moves to
//! a shared [`BigInt`] only when a result leaves the `i64` range.
//!
//! Results are normalized back to `Small` whenever they fit. Comparisons and
//! conversions also accept a host-constructed `Big` within the `i64` range.

#![cfg_attr(
    not(test),
    deny(
        missing_docs,
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

use num_bigint::BigInt;
use num_bigint::Sign;
use num_rational::BigRational;
use num_traits::ToPrimitive;
use num_traits::Zero;
use std::cmp::Ordering;
use std::fmt;
use std::rc::Rc;

/// An arbitrary-precision integer with an allocation-free small case.
#[derive(Clone)]
pub enum Int {
    /// A value in the `i64` range.
    Small(i64),
    /// A shared arbitrary-precision value. Arithmetic produces this variant outside the `i64` range.
    Big(Rc<BigInt>),
}

impl Int {
    /// One.
    pub const ONE: Self = Self::Small(1);
    /// Zero.
    pub const ZERO: Self = Self::Small(0);

    /// The canonical form of a big integer.
    #[must_use]
    pub fn from_big(value: BigInt) -> Self {
        value
            .to_i64()
            .map_or_else(|| Self::Big(Rc::new(value)), Self::Small)
    }

    /// The value as a `BigInt` (allocates for nonzero values).
    #[must_use]
    pub fn to_big(&self) -> BigInt {
        match self {
            | Self::Small(value) => BigInt::from(*value),
            | Self::Big(value) => (**value).clone(),
        }
    }

    /// The value as an exact rational.
    #[must_use]
    pub fn to_rational(&self) -> BigRational {
        BigRational::from_integer(self.to_big())
    }

    /// The value, when it fits in `i64`.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            | Self::Small(value) => Some(*value),
            | Self::Big(value) => value.to_i64(),
        }
    }

    /// True for zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        match self {
            | Self::Small(value) => *value == 0,
            | Self::Big(value) => value.is_zero(),
        }
    }

    /// True for one.
    #[must_use]
    pub fn is_one(&self) -> bool {
        self.as_i64() == Some(1)
    }

    /// True below zero.
    #[must_use]
    pub fn is_negative(&self) -> bool {
        match self {
            | Self::Small(value) => *value < 0,
            | Self::Big(value) => value.sign() == Sign::Minus,
        }
    }

    /// True above zero.
    #[must_use]
    pub fn is_positive(&self) -> bool {
        match self {
            | Self::Small(value) => *value > 0,
            | Self::Big(value) => value.sign() == Sign::Plus,
        }
    }

    /// `-1`, `0` or `1`.
    #[must_use]
    pub fn signum(&self) -> i32 {
        match self {
            | Self::Small(value) => value.signum() as i32,
            | Self::Big(value) => {
                match value.sign() {
                    | Sign::Minus => -1,
                    | Sign::NoSign => 0,
                    | Sign::Plus => 1,
                }
            },
        }
    }

    /// The absolute value.
    #[must_use]
    pub fn abs(&self) -> Self {
        match self {
            | Self::Small(value) => {
                value.checked_abs().map_or_else(
                    || Self::from_big(num_traits::Signed::abs(&BigInt::from(*value))),
                    Self::Small,
                )
            },
            | Self::Big(value) => Self::from_big(num_traits::Signed::abs(&**value)),
        }
    }

    /// The nearest `f64`.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "conversion rounds integers to the nearest binary64 value"
    )]
    pub fn to_f64(&self) -> f64 {
        match self {
            | Self::Small(value) => *value as f64,
            | Self::Big(value) => value.to_f64().unwrap_or(f64::NAN),
        }
    }

    /// `self ^ exponent`.
    #[must_use]
    pub fn pow(
        &self,
        exponent: u32,
    ) -> Self {
        match self {
            | Self::Small(base) => {
                base.checked_pow(exponent).map_or_else(
                    || Self::from_big(num_traits::Pow::pow(BigInt::from(*base), exponent)),
                    Self::Small,
                )
            },
            | Self::Big(base) => Self::from_big(num_traits::Pow::pow(base.as_ref(), exponent)),
        }
    }
}

impl Default for Int {
    fn default() -> Self {
        Self::ZERO
    }
}

impl From<i64> for Int {
    fn from(value: i64) -> Self {
        Self::Small(value)
    }
}

impl From<i32> for Int {
    fn from(value: i32) -> Self {
        Self::Small(i64::from(value))
    }
}

impl From<u32> for Int {
    fn from(value: u32) -> Self {
        Self::Small(i64::from(value))
    }
}

impl From<usize> for Int {
    fn from(value: usize) -> Self {
        i64::try_from(value).map_or_else(|_| Self::from_big(BigInt::from(value)), Self::Small)
    }
}

impl From<u64> for Int {
    fn from(value: u64) -> Self {
        i64::try_from(value).map_or_else(|_| Self::from_big(BigInt::from(value)), Self::Small)
    }
}

impl From<i128> for Int {
    fn from(value: i128) -> Self {
        i64::try_from(value).map_or_else(|_| Self::from_big(BigInt::from(value)), Self::Small)
    }
}

impl From<BigInt> for Int {
    fn from(value: BigInt) -> Self {
        Self::from_big(value)
    }
}

impl From<&BigInt> for Int {
    fn from(value: &BigInt) -> Self {
        value
            .to_i64()
            .map_or_else(|| Self::Big(Rc::new(value.clone())), Self::Small)
    }
}

impl From<Int> for BigInt {
    fn from(value: Int) -> Self {
        match value {
            | Int::Small(value) => Self::from(value),
            | Int::Big(value) => Rc::try_unwrap(value).unwrap_or_else(|shared| (*shared).clone()),
        }
    }
}

impl TryFrom<&Int> for i64 {
    type Error = ();

    fn try_from(value: &Int) -> Result<Self, ()> {
        value.as_i64().ok_or(())
    }
}

impl TryFrom<Int> for i64 {
    type Error = ();

    fn try_from(value: Int) -> Result<Self, ()> {
        value.as_i64().ok_or(())
    }
}

impl From<&Int> for BigInt {
    fn from(value: &Int) -> Self {
        value.to_big()
    }
}

impl PartialEq for Int {
    fn eq(
        &self,
        other: &Self,
    ) -> bool {
        match (self, other) {
            | (Self::Small(a), Self::Small(b)) => a == b,
            | (Self::Big(a), Self::Big(b)) => a == b,
            | (Self::Small(a), Self::Big(b)) | (Self::Big(b), Self::Small(a)) => {
                b.to_i64() == Some(*a)
            },
        }
    }
}

impl Eq for Int {}

impl PartialEq<BigInt> for Int {
    fn eq(
        &self,
        other: &BigInt,
    ) -> bool {
        match self {
            | Self::Small(a) => other.to_i64() == Some(*a),
            | Self::Big(a) => **a == *other,
        }
    }
}

impl PartialEq<i64> for Int {
    fn eq(
        &self,
        other: &i64,
    ) -> bool {
        self.as_i64() == Some(*other)
    }
}

impl Ord for Int {
    fn cmp(
        &self,
        other: &Self,
    ) -> Ordering {
        match (self, other) {
            | (Self::Small(a), Self::Small(b)) => a.cmp(b),
            | (Self::Big(a), Self::Big(b)) => a.cmp(b),
            | (Self::Small(a), Self::Big(b)) => {
                b.to_i64().map_or_else(
                    || {
                        if b.sign() == Sign::Minus {
                            Ordering::Greater
                        } else {
                            Ordering::Less
                        }
                    },
                    |b| a.cmp(&b),
                )
            },
            | (Self::Big(a), Self::Small(b)) => {
                a.to_i64().map_or_else(
                    || {
                        if a.sign() == Sign::Minus {
                            Ordering::Less
                        } else {
                            Ordering::Greater
                        }
                    },
                    |a| a.cmp(b),
                )
            },
        }
    }
}

impl PartialOrd for Int {
    fn partial_cmp(
        &self,
        other: &Self,
    ) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::hash::Hash for Int {
    fn hash<H: std::hash::Hasher>(
        &self,
        state: &mut H,
    ) {
        match self {
            | Self::Small(value) => value.hash(state),
            | Self::Big(value) => {
                if let Some(small) = value.to_i64() {
                    small.hash(state);
                } else {
                    value.hash(state);
                }
            },
        }
    }
}

impl fmt::Display for Int {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            | Self::Small(value) => fmt::Display::fmt(value, f),
            | Self::Big(value) => fmt::Display::fmt(&**value, f),
        }
    }
}

impl fmt::Debug for Int {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl ToPrimitive for Int {
    fn to_i64(&self) -> Option<i64> {
        self.as_i64()
    }

    fn to_u64(&self) -> Option<u64> {
        match self {
            | Self::Small(value) => u64::try_from(*value).ok(),
            | Self::Big(value) => value.to_u64(),
        }
    }

    fn to_i128(&self) -> Option<i128> {
        match self {
            | Self::Small(value) => Some(i128::from(*value)),
            | Self::Big(value) => value.to_i128(),
        }
    }

    fn to_f64(&self) -> Option<f64> {
        Some(Self::to_f64(self))
    }
}

impl Zero for Int {
    fn zero() -> Self {
        Self::ZERO
    }

    fn is_zero(&self) -> bool {
        Self::is_zero(self)
    }
}

impl num_traits::One for Int {
    fn one() -> Self {
        Self::ONE
    }
}

macro_rules! arith {
    ($trait:ident, $method:ident, $checked:ident, $op:tt, $assign:tt, $reverse:expr) => {
        impl std::ops::$trait<&Int> for &Int {
            type Output = Int;

            #[inline]
            #[allow(clippy::arithmetic_side_effects, reason = "operation is checked for i64 and exact for BigInt")]
            fn $method(
                self,
                other: &Int,
            ) -> Int {
                match (self, other) {
                    | (Int::Small(a), Int::Small(b)) => a.$checked(*b).map_or_else(
                        || Int::from_big(BigInt::from(*a) $op *b),
                        Int::Small,
                    ),
                    | (Int::Big(a), Int::Big(b)) => Int::from_big(a.as_ref() $op b.as_ref()),
                    | (Int::Big(a), Int::Small(b)) => Int::from_big(a.as_ref() $op *b),
                    | (Int::Small(a), Int::Big(b)) => Int::from_big(*a $op b.as_ref()),
                }
            }
        }

        impl std::ops::$trait<Int> for Int {
            type Output = Int;

            #[inline]
            #[allow(clippy::arithmetic_side_effects, reason = "mixed arithmetic is exact for BigInt")]
            fn $method(
                self,
                other: Int,
            ) -> Int {
                // An owned, unaliased BigInt can keep both its handle and its limbs.
                match (self, other) {
                    | (Int::Big(mut left), Int::Small(right)) => {
                        if let Some(value) = Rc::get_mut(&mut left) {
                            *value $assign right;
                            value.to_i64().map_or(Int::Big(left), Int::Small)
                        } else {
                            Int::from_big(left.as_ref() $op right)
                        }
                    },
                    | (Int::Small(left), Int::Big(mut right)) => {
                        if let Some(value) = Rc::get_mut(&mut right) {
                            ($reverse)(value, left);
                            value.to_i64().map_or(Int::Big(right), Int::Small)
                        } else {
                            Int::from_big(left $op right.as_ref())
                        }
                    },
                    | (left, right) => (&left).$method(&right),
                }
            }
        }

        impl std::ops::$trait<&Int> for Int {
            type Output = Int;

            #[inline]
            fn $method(
                self,
                other: &Int,
            ) -> Int {
                (&self).$method(other)
            }
        }

        impl std::ops::$trait<Int> for &Int {
            type Output = Int;

            #[inline]
            fn $method(
                self,
                other: Int,
            ) -> Int {
                self.$method(&other)
            }
        }
    };
}

arith!(Add, add, checked_add, +, +=, |value: &mut BigInt, left| *value += left);
arith!(Sub, sub, checked_sub, -, -=, |value: &mut BigInt, left| {
    *value = -std::mem::take(value);
    *value += left;
});
arith!(Mul, mul, checked_mul, *, *=, |value: &mut BigInt, left| *value *= left);

impl std::ops::Neg for &Int {
    type Output = Int;

    #[allow(
        clippy::arithmetic_side_effects,
        reason = "negation is checked for i64 and exact for BigInt"
    )]
    fn neg(self) -> Int {
        match self {
            | Int::Small(value) => {
                value
                    .checked_neg()
                    .map_or_else(|| Int::from_big(-BigInt::from(*value)), Int::Small)
            },
            | Int::Big(value) => Int::from_big(-value.as_ref()),
        }
    }
}

impl std::ops::Neg for Int {
    type Output = Self;

    fn neg(self) -> Self {
        std::ops::Neg::neg(&self)
    }
}

impl serde::Serialize for Int {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match self {
            | Self::Small(value) => BigInt::from(*value).serialize(serializer),
            | Self::Big(value) => value.as_ref().serialize(serializer),
        }
    }
}

impl<'de> serde::Deserialize<'de> for Int {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        BigInt::deserialize(deserializer).map(Self::from_big)
    }
}

#[cfg(test)]
mod tests {
    use super::Int;
    use num_bigint::BigInt;
    use num_traits::ToPrimitive;
    use std::rc::Rc;

    #[test]
    fn host_constructed_small_bigints_match_by_value() {
        use std::collections::HashSet;
        use std::rc::Rc;

        let host = |value| Int::Big(Rc::new(BigInt::from(value)));
        assert!(host(0).is_zero());
        assert!(host(1).is_one());
        assert_eq!(host(17).as_i64(), Some(17));
        let host_two = host(2);
        let small_two = Int::from(2);
        assert_eq!(host_two, small_two);
        assert_eq!(host_two.cmp(&small_two), std::cmp::Ordering::Equal);
        assert_eq!(HashSet::from([host_two, small_two]).len(), 1);
        let large = Int::from(i64::MAX) + Int::ONE;
        assert_eq!(&large - &large, Int::ZERO);
        assert_eq!(Int::ONE - &large, Int::from(-i64::MAX));
        assert_eq!(&large * Int::ONE, large);
    }

    #[test]
    fn overflow_promotes_and_shrinks_back() {
        let max = Int::from(i64::MAX);
        let big = &max + &Int::ONE;
        assert!(matches!(big, Int::Big(_)));
        assert_eq!(big.to_string(), "9223372036854775808");
        let back = &big - &Int::ONE;
        assert!(matches!(back, Int::Small(i64::MAX)));
        let minimum = Int::Small(i64::MIN);
        assert_eq!(-&minimum, Int::from_big(-BigInt::from(i64::MIN)));
        assert!(big > minimum);
        // -2^63 is i64::MIN itself, so it is small again.
        assert_eq!(-&big, minimum);
        let below = &minimum - &Int::ONE;
        assert!(matches!(below, Int::Big(_)) && below < minimum);
    }

    #[test]
    fn owned_mixed_arithmetic_matches_exact_values_and_preserves_shared_inputs() {
        let boundary = BigInt::from(1) << 63usize;
        let large: BigInt = (BigInt::from(3) << 4096usize) + 5;
        for initial in [
            BigInt::from(3),
            boundary.clone(),
            -boundary - 1,
            large.clone(),
            -large,
        ] {
            for word in [i64::MIN, -3, -1, 0, 1, 7, i64::MAX] {
                let number = BigInt::from(word);
                for shared in [false, true] {
                    let owner = Rc::new(initial.clone());
                    let value = || {
                        Int::Big(if shared {
                            Rc::clone(&owner)
                        } else {
                            Rc::new(initial.clone())
                        })
                    };
                    let small = Int::Small(word);
                    for (actual, exact) in [
                        (value() + small.clone(), &initial + &number),
                        (small.clone() + value(), &number + &initial),
                        (value() - small.clone(), &initial - &number),
                        (small.clone() - value(), &number - &initial),
                        (value() * small.clone(), &initial * &number),
                        (small * value(), &number * &initial),
                    ] {
                        assert_eq!(actual.to_big(), exact);
                        assert_eq!(matches!(actual, Int::Small(_)), exact.to_i64().is_some());
                    }
                    assert_eq!(owner.as_ref(), &initial);
                }
            }
        }

        let value = Rc::new(BigInt::from(i64::MAX) + 1);
        let previous = Rc::downgrade(&value);
        let result = Int::Big(value) + Int::Small(-1);
        assert_eq!(result, Int::Small(i64::MAX));
        assert!(previous.upgrade().is_none());
        assert!(matches!(Int::Small(i64::MIN) * Int::Small(-1), Int::Big(_)));
    }

    #[test]
    fn both_engines_dispatch_mixed_temporaries_and_keep_rational_fallbacks() {
        use crate::value::NumKind;
        use crate::value::Value;

        let seed: BigInt = (BigInt::from(3) << 256usize) + 5;
        let word = i64::MIN;
        let source = format!(
            "fn evaluate(seed: Int, word: Int): Int = word - ((seed + word) * (-3) - word)\n\
             fn demote(seed: Int, word: Int): Int = word - seed\n\
             let seed = {seed}\n\
             let mixed = evaluate(seed, {word})\n\
             let boundary = demote(9223372036854775808, 9223372036854775807)\n\
             let quotient = 7 / 2\n\
             let power = (-2) ^ 3\n"
        );
        let compilation = crate::compile("mixed_int.aev", &source);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics.items
        );
        let module = compilation.module.unwrap();
        let expected: BigInt = BigInt::from(word) - ((&seed + word) * -3i64 - word);
        for mode in [crate::Mode::Ast, crate::Mode::Ir] {
            let mut interp = crate::Interp::new(&compilation.checked).with_mode(mode);
            let diagnostics = interp.load(&module);
            assert!(
                !diagnostics.has_errors(),
                "{mode:?}: {:?}",
                diagnostics.items
            );
            for (name, expected) in [
                ("mixed", expected.to_string()),
                ("seed", seed.to_string()),
                ("quotient", "7/2".into()),
                ("power", "-8".into()),
            ] {
                let value = interp.eval_toplevel(name).unwrap();
                assert_eq!(crate::value::repr(&value), expected, "{mode:?}: {name}");
            }
            let Value::Num(boundary) = interp.eval_toplevel("boundary").unwrap() else {
                panic!("expected an integer result");
            };
            assert!(matches!(boundary.kind, NumKind::Int(Int::Small(-1))));
        }
    }

    #[test]
    fn small_values_stay_small() {
        // `Value` is moved on every interpreter step; an `Int` must not grow it.
        assert_eq!(std::mem::size_of::<Int>(), 16);
    }
}
