//! The runtime representation of `Int`.
//!
//! Almost every integer a program computes fits in a machine word, but the
//! language promises arbitrary precision (§3.4.1). [`Int`] keeps the common
//! case inline — no allocation, no `gcd`, no vector arithmetic — and moves to
//! a shared [`BigInt`] only when a result leaves the `i64` range.
//!
//! The representation is canonical: a value that fits in `i64` is always
//! `Small`. Equality, ordering and hashing can therefore compare variants
//! structurally, and a `Big` is never equal to a `Small`.

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
    /// A value outside the `i64` range (never one inside it).
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
    pub const fn as_i64(&self) -> Option<i64> {
        match self {
            | Self::Small(value) => Some(*value),
            | Self::Big(_) => None,
        }
    }

    /// True for zero.
    #[must_use]
    pub const fn is_zero(&self) -> bool {
        matches!(self, Self::Small(0))
    }

    /// True for one.
    #[must_use]
    pub const fn is_one(&self) -> bool {
        matches!(self, Self::Small(1))
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
        if let Self::Small(base) = self
            && let Some(value) = base.checked_pow(exponent)
        {
            return Self::Small(value);
        }
        Self::from_big(num_traits::Pow::pow(self.to_big(), exponent))
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
            // Canonical: a big value is never in the small range.
            | _ => false,
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
        matches!(self, Self::Small(a) if a == other)
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
            // A big value lies beyond every small one, on its own side.
            | (Self::Small(_), Self::Big(b)) => {
                if b.sign() == Sign::Minus {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            },
            | (Self::Big(a), Self::Small(_)) => {
                if a.sign() == Sign::Minus {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
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
            | Self::Big(value) => value.hash(state),
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
    ($trait:ident, $method:ident, $checked:ident, $op:tt) => {
        impl std::ops::$trait<&Int> for &Int {
            type Output = Int;

            #[inline]
            fn $method(
                self,
                other: &Int,
            ) -> Int {
                if let (Int::Small(a), Int::Small(b)) = (self, other)
                    && let Some(value) = a.$checked(*b)
                {
                    return Int::Small(value);
                }
                Int::from_big(self.to_big() $op other.to_big())
            }
        }

        impl std::ops::$trait<Int> for Int {
            type Output = Int;

            #[inline]
            fn $method(
                self,
                other: Int,
            ) -> Int {
                (&self).$method(&other)
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

arith!(Add, add, checked_add, +);
arith!(Sub, sub, checked_sub, -);
arith!(Mul, mul, checked_mul, *);

impl std::ops::Neg for &Int {
    type Output = Int;

    fn neg(self) -> Int {
        match self {
            | Int::Small(value) => {
                value
                    .checked_neg()
                    .map_or_else(|| Int::from_big(-BigInt::from(*value)), Int::Small)
            },
            | Int::Big(value) => Int::from_big(-(**value).clone()),
        }
    }
}

impl std::ops::Neg for Int {
    type Output = Self;

    fn neg(self) -> Self {
        -&self
    }
}

impl serde::Serialize for Int {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        self.to_big().serialize(serializer)
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

    #[test]
    fn overflow_promotes_and_shrinks_back() {
        let max = Int::from(i64::MAX);
        let big = &max + &Int::ONE;
        assert!(matches!(big, Int::Big(_)));
        assert_eq!(big.to_string(), "9223372036854775808");
        let back = &big - &Int::ONE;
        assert!(matches!(back, Int::Small(i64::MAX)));
        assert_eq!(-Int::from(i64::MIN), Int::from_big(-BigInt::from(i64::MIN)));
        assert!(Int::from(i64::MIN) < big);
        // -2^63 is i64::MIN itself, so it is small again.
        assert_eq!(-&big, Int::from(i64::MIN));
        let below = &Int::from(i64::MIN) - &Int::ONE;
        assert!(matches!(below, Int::Big(_)) && below < Int::from(i64::MIN));
    }

    #[test]
    fn small_values_stay_small() {
        // `Value` is moved on every interpreter step; an `Int` must not grow it.
        assert_eq!(std::mem::size_of::<Int>(), 16);
    }
}
