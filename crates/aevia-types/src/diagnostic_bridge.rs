//! Maps inference failures to the diagnostic codes of appendix B.
//!
//! Keeping the mapping in one function means the checker, the REPL and the
//! language server all report the same code for the same failure.

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

use crate::infer::UnifyError;

/// The stable code for a unification failure.
#[must_use]
pub const fn code(error: &UnifyError) -> &'static str {
    match error {
        | UnifyError::Dim(_) => "E0401",
        | UnifyError::Occurs(_) => "E0303",
        | UnifyError::MissingField { .. } => "E0306",
        | UnifyError::DuplicateField { .. } => "E0208",
        // A shape mismatch the checker can see is a type error: `E0802` is a
        // *runtime* code and must not be reported for a static failure.
        | UnifyError::ShapeMismatch { .. }
        | UnifyError::LiteralMismatch { .. }
        | UnifyError::Mismatch { .. }
        | UnifyError::Numeric(_)
        | UnifyError::NatMismatch { .. }
        | UnifyError::NatRigid { .. } => "E0302",
        // §3.5.1 classifies a nonlinear Nat equation as a rank inference failure.
        | UnifyError::NatNonLinear { .. } => "E0304",
    }
}
