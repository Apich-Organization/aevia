//! Dimensional vector algebra over the 7 SI base dimensions.
//!
//! Each physical quantity is represented as a vector of integer exponents:
//!   `[m, s, kg, A, K, mol, cd]`
//!
//! Examples:
//!   - velocity  = m / s    → [1, -1, 0, 0, 0, 0, 0]
//!   - force     = kg·m/s²  → [1, -2, 1, 0, 0, 0, 0]
//!   - frequency = 1 / s    → [0, -1, 0, 0, 0, 0, 0]

use crate::ast::{DimExpr, Spanned};
use std::fmt;

/// Index constants into `DimVector` for each SI base dimension.
pub mod idx {
    pub const M: usize = 0;   // metre  (length)
    pub const S: usize = 1;   // second (time)
    pub const KG: usize = 2;  // kilogram (mass)
    pub const A: usize = 3;   // ampere  (electric current)
    pub const K: usize = 4;   // kelvin  (temperature)
    pub const MOL: usize = 5; // mole    (amount of substance)
    pub const CD: usize = 6;  // candela (luminous intensity)
}

/// Fixed-size exponent vector over the seven SI base dimensions.
/// `DimVector([m, s, kg, A, K, mol, cd])`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DimVector(pub [i8; 7]);

impl DimVector {
    pub const LEN: usize = 7;

    /// The dimensionless scalar (all exponents zero).
    pub const DIMENSIONLESS: Self = Self([0; 7]);

    // ── Named constructors for all 7 SI bases ──────────────────────────────

    pub const M: Self   = Self([1, 0, 0, 0, 0, 0, 0]); // length
    pub const S: Self   = Self([0, 1, 0, 0, 0, 0, 0]); // time
    pub const KG: Self  = Self([0, 0, 1, 0, 0, 0, 0]); // mass
    pub const A: Self   = Self([0, 0, 0, 1, 0, 0, 0]); // current
    pub const K: Self   = Self([0, 0, 0, 0, 1, 0, 0]); // temperature
    pub const MOL: Self = Self([0, 0, 0, 0, 0, 1, 0]); // amount
    pub const CD: Self  = Self([0, 0, 0, 0, 0, 0, 1]); // luminosity

    // ── Common derived SI units ────────────────────────────────────────────

    /// Velocity: m / s
    pub const VELOCITY: Self = Self([1, -1, 0, 0, 0, 0, 0]);
    /// Acceleration: m / s²
    pub const ACCELERATION: Self = Self([1, -2, 0, 0, 0, 0, 0]);
    /// Newton: kg·m / s²
    pub const NEWTON: Self = Self([1, -2, 1, 0, 0, 0, 0]);
    /// Joule: kg·m² / s²
    pub const JOULE: Self = Self([2, -2, 1, 0, 0, 0, 0]);
    /// Pascal: kg / (m·s²)
    pub const PASCAL: Self = Self([-1, -2, 1, 0, 0, 0, 0]);
    /// Watt: kg·m² / s³
    pub const WATT: Self = Self([2, -3, 1, 0, 0, 0, 0]);
    /// Hertz: 1 / s
    pub const HERTZ: Self = Self([0, -1, 0, 0, 0, 0, 0]);

    // ── Arithmetic ─────────────────────────────────────────────────────────

    /// Multiply two quantities: exponents add  (a · b → a + b).
    #[must_use]
    pub fn mul(self, other: Self) -> Self {
        let mut out = self.0;
        for (a, b) in out.iter_mut().zip(other.0) {
            *a = a.saturating_add(b);
        }
        Self(out)
    }

    /// Divide two quantities: exponents subtract  (a / b → a − b).
    #[must_use]
    pub fn div(self, other: Self) -> Self {
        let mut out = self.0;
        for (a, b) in out.iter_mut().zip(other.0) {
            *a = a.saturating_sub(b);
        }
        Self(out)
    }

    /// Raise to an integer power: exponents scale  (aⁿ → a · n).
    #[must_use]
    pub fn pow(self, n: i32) -> Self {
        let mut out = self.0;
        for a in out.iter_mut() {
            *a = (*a as i32).saturating_mul(n) as i8;
        }
        Self(out)
    }

    /// Kept for backwards-compatibility with the existing stub.
    #[must_use]
    pub fn add(self, other: Self) -> Self {
        self.mul(other)
    }

    /// Kept for backwards-compatibility with the existing stub.
    #[must_use]
    pub fn sub(self, other: Self) -> Self {
        self.div(other)
    }

    /// Returns `true` if all exponents are zero (dimensionless scalar).
    #[must_use]
    pub fn is_dimensionless(self) -> bool {
        self.0.iter().all(|&e| e == 0)
    }

    /// Returns `true` if addition/subtraction is dimensionally legal
    /// (both operands share the same dimension).
    #[must_use]
    pub fn addable_with(self, other: Self) -> bool {
        self == other
    }
}

impl fmt::Display for DimVector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const NAMES: [&str; 7] = ["m", "s", "kg", "A", "K", "mol", "cd"];
        let mut parts_pos: Vec<String> = Vec::new();
        let mut parts_neg: Vec<String> = Vec::new();

        for (i, &exp) in self.0.iter().enumerate() {
            match exp.cmp(&0) {
                std::cmp::Ordering::Greater => {
                    if exp == 1 {
                        parts_pos.push(NAMES[i].to_string());
                    } else {
                        parts_pos.push(format!("{}^{}", NAMES[i], exp));
                    }
                }
                std::cmp::Ordering::Less => {
                    if exp == -1 {
                        parts_neg.push(NAMES[i].to_string());
                    } else {
                        parts_neg.push(format!("{}^{}", NAMES[i], -exp));
                    }
                }
                std::cmp::Ordering::Equal => {}
            }
        }

        if parts_pos.is_empty() && parts_neg.is_empty() {
            return write!(f, "dimensionless");
        }

        let pos_str = if parts_pos.is_empty() {
            "1".to_string()
        } else {
            parts_pos.join("·")
        };

        if parts_neg.is_empty() {
            write!(f, "{pos_str}")
        } else {
            write!(f, "{}/{}", pos_str, parts_neg.join("·"))
        }
    }
}

// ── PhysicalType ──────────────────────────────────────────────────────────────

/// A full physical type: SI dimensional exponents paired with an optional tensor shape.
///
/// `shape = None` means a scalar; `shape = Some(dims)` means a rank-N tensor where
/// each element carries the `dim` SI unit.
/// `struct_name = Some(name)` means this is a struct type (composite of multiple fields).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalType {
    pub dim: DimVector,
    pub shape: Option<Vec<usize>>,
    pub struct_name: Option<String>,
}

impl PhysicalType {
    pub fn scalar(dim: DimVector) -> Self {
        Self { dim, shape: None, struct_name: None }
    }

    pub fn tensor(dim: DimVector, shape: Vec<usize>) -> Self {
        Self { dim, shape: Some(shape), struct_name: None }
    }

    pub fn struct_type(name: String) -> Self {
        Self { dim: DimVector::DIMENSIONLESS, shape: None, struct_name: Some(name) }
    }

    pub fn is_scalar(&self) -> bool {
        self.shape.is_none()
    }
}

impl fmt::Display for PhysicalType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.shape {
            None => write!(f, "{}", self.dim),
            Some(shape) => {
                let s: Vec<String> = shape.iter().map(|n| n.to_string()).collect();
                write!(f, "{}[{}]", self.dim, s.join(", "))
            }
        }
    }
}

/// Extract the tensor shape from a `DimExpr` annotation, returning `None` for scalar types.
pub fn tensor_shape(dim_expr: &Spanned<DimExpr>) -> Option<Vec<usize>> {
    match &dim_expr.node {
        DimExpr::Tensor { shape, .. } => Some(shape.clone()),
        _ => None,
    }
}

// ── DimExpr → DimVector resolution ────────────────────────────────────────────

/// Resolve a parsed `DimExpr` (from the AST) into a `DimVector`.
/// Returns `None` if an unknown base unit name is encountered.
pub fn resolve(dim_expr: &Spanned<DimExpr>) -> Option<DimVector> {
    resolve_inner(&dim_expr.node)
}

fn resolve_inner(expr: &DimExpr) -> Option<DimVector> {
    match expr {
        DimExpr::Base(name) => base_unit(name),
        DimExpr::Mul(lhs, rhs) => {
            Some(resolve_inner(&lhs.node)?.mul(resolve_inner(&rhs.node)?))
        }
        DimExpr::Div(lhs, rhs) => {
            Some(resolve_inner(&lhs.node)?.div(resolve_inner(&rhs.node)?))
        }
        DimExpr::Power(base, exp) => {
            Some(resolve_inner(&base.node)?.pow(*exp))
        }
        DimExpr::Tensor { base, .. } => {
            // Tensors carry SI element dimension; shape is not encoded in DimVector.
            resolve_inner(&base.node)
        }
    }
}

/// Map a base unit string to its `DimVector`, including common aliases.
pub fn base_unit(name: &str) -> Option<DimVector> {
    Some(match name {
        // SI base units
        "m"   => DimVector::M,
        "s"   => DimVector::S,
        "kg"  => DimVector::KG,
        "A"   => DimVector::A,
        "K"   => DimVector::K,
        "mol" => DimVector::MOL,
        "cd"  => DimVector::CD,
        // Dimensionless
        "1"   => DimVector::DIMENSIONLESS,
        // Common derived unit aliases (for convenience in type annotations)
        "N" | "Newton"       => DimVector::NEWTON,
        "J" | "Joule"        => DimVector::JOULE,
        "Pa" | "Pascal"      => DimVector::PASCAL,
        "W" | "Watt"         => DimVector::WATT,
        "Hz" | "Hertz"       => DimVector::HERTZ,
        "g"                  => DimVector::KG,  // gram — treated as kg for now
        _ => return None,
    })
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::DimExpr;
    use miette::SourceSpan;

    fn span() -> SourceSpan {
        SourceSpan::new(0.into(), 0)
    }

    fn sp<T>(node: T) -> Spanned<T> {
        Spanned::new(node, span())
    }

    #[test]
    fn test_mul_div_display() {
        // velocity = m / s
        let v = DimVector::M.div(DimVector::S);
        assert_eq!(v.to_string(), "m/s");

        // force = kg·m / s²
        let f = DimVector::KG.mul(DimVector::M).div(DimVector::S.pow(2));
        assert_eq!(f, DimVector::NEWTON);
        assert_eq!(f.to_string(), "m·kg/s^2");
    }

    #[test]
    fn test_dimensionless() {
        let d = DimVector::M.div(DimVector::M);
        assert!(d.is_dimensionless());
        assert_eq!(d.to_string(), "dimensionless");
    }

    #[test]
    fn test_pow() {
        let s2 = DimVector::S.pow(2);
        assert_eq!(s2.0[1], 2);
        let s_neg1 = DimVector::S.pow(-1);
        assert_eq!(s_neg1.0[1], -1);
    }

    #[test]
    fn test_resolve_simple() {
        let expr = sp(DimExpr::Base("m".to_string()));
        assert_eq!(resolve(&expr), Some(DimVector::M));
    }

    #[test]
    fn test_resolve_compound() {
        // m / s^2
        let s = sp(DimExpr::Base("s".to_string()));
        let s2 = sp(DimExpr::Power(Box::new(s), 2));
        let m = sp(DimExpr::Base("m".to_string()));
        let expr = sp(DimExpr::Div(Box::new(m), Box::new(s2)));
        assert_eq!(resolve(&expr), Some(DimVector::ACCELERATION));
    }

    #[test]
    fn test_addable_with() {
        assert!(DimVector::M.addable_with(DimVector::M));
        assert!(!DimVector::M.addable_with(DimVector::S));
    }

    #[test]
    fn test_unknown_base() {
        let expr = sp(DimExpr::Base("xyz".to_string()));
        assert_eq!(resolve(&expr), None);
    }
}
