//! Maps inference failures to the diagnostic codes of appendix B.
//!
//! Keeping the mapping in one function means the checker, the REPL and the
//! language server all report the same code for the same failure.

use crate::infer::UnifyError;

/// The stable code for a unification failure.
#[must_use]
pub fn code(error: &UnifyError) -> &'static str {
    match error {
        | UnifyError::Dim(_) => "E0401",
        | UnifyError::Occurs(_) => "E0303",
        | UnifyError::MissingField { .. } => "E0306",
        // A shape mismatch the checker can see is a type error: `E0802` is a
        // *runtime* code and must not be reported for a static failure.
        | UnifyError::ShapeMismatch { .. } => "E0302",
        | UnifyError::DuplicateField { .. } => "E0208",
        | UnifyError::LiteralMismatch { .. } => "E0302",
        | UnifyError::Mismatch { .. } => "E0302",
        | UnifyError::Numeric(_) => "E0302",
        // §3.5.1: an inconsistent Nat equation is a type error; a non-linear
        // one is "cannot infer" (`E0304`, the appendix's rank inference code).
        | UnifyError::NatMismatch { .. } => "E0302",
        | UnifyError::NatNonLinear { .. } => "E0304",
    }
}
