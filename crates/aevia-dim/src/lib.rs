//! Dimensions and units for Aevia.
//!
//! This crate is the type-level and compile-time half of the quantity system
//! described in chapters 04 and 05 of the specification:
//!
//! * [`Dim`] is an element of the free abelian group over base dimensions and
//!   dimension variables, with exact rational exponents.
//! * [`DimUnifier`] solves dimension equations with an occurs check.
//! * [`DimEnv`] holds the base dimensions and the transparent aliases
//!   (`Velocity`, `Energy`, ...) and knows how to parse and print them.
//! * [`UnitTable`] holds the units themselves: exact scales, SI prefixes and
//!   affine temperature scales.
//!
//! The crate has no dependency on the parser or the runtime. The parser builds
//! [`UnitExpr`] values and the checker resolves them against a [`UnitTable`],
//! which keeps unit resolution out of the grammar (spec §1.8.1).

#![forbid(unsafe_code)]

pub mod dim;
pub mod rat;
pub mod units;

pub use dim::{Atom, Dim, DimEnv, DimMismatch, DimParseError, DimUnifier};
pub use rat::{format_rational, format_rational_decimal, parse_decimal, ratio};
pub use units::{ResolvedUnit, Scale, UnitDef, UnitError, UnitExpr, UnitTable};

/// The canonical names of the seven SI base dimensions, in SI order.
pub const SI_BASE_DIMENSIONS: [&str; 7] = [
    "Length",
    "Mass",
    "Time",
    "Current",
    "Temperature",
    "Amount",
    "Luminosity",
];

/// The single-letter display aliases of the SI base dimensions.
pub const SI_BASE_ALIASES: [&str; 7] = ["L", "M", "T", "I", "Θ", "Amt", "Lum"];
