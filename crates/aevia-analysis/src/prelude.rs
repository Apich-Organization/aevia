//! The names the prelude binds.
//!
//! `aevia_types::builtins` decides what a builtin *means*, but it answers one
//! question at a time (`schemes("sqrt")`), so it cannot be enumerated. The
//! list below is the set of names that function accepts, transcribed from it;
//! [`NAMES`] is filtered through `builtins::is_builtin` before use, and
//! `tests::every_prelude_name_is_a_builtin` fails if a name here stops being
//! one. The list is the only thing this crate knows that the checker does not
//! expose, and it is checked rather than trusted.
//!
//! It is not a second copy of the *types*: the scheme always comes from the
//! checker, so a completion can never disagree with what `check_module` would
//! infer.

use aevia_types::builtins;

/// Every name `aevia_types::builtins::schemes` answers for (the list lives
/// next to the schemes so the checker can offer spelling suggestions too).
pub use aevia_types::builtins::PRELUDE_NAMES as NAMES;

/// The prelude names that are still builtins.
///
/// A name that the checker no longer knows is dropped rather than offered: an
/// editor that completes `foo` into an error is worse than one that stays
/// quiet, and the test below makes the drift visible either way.
pub fn names() -> impl Iterator<Item = &'static str> {
    NAMES
        .iter()
        .copied()
        .filter(|name| builtins::is_builtin(name) || *name == "to_callback")
}

/// Concise documentation for a prelude builtin, when available.
#[must_use]
#[cfg_attr(
    not(test),
    expect(
        clippy::too_many_lines,
        reason = "keep the curated prelude documentation in one exhaustive lookup"
    )
)]
pub fn doc(name: &str) -> Option<&'static str> {
    Some(match name {
        // Constants
        | "pi" => "Archimedes' constant π (≈ 3.141592653589793).",
        | "e" => "Euler's number e (≈ 2.718281828459045).",
        | "oo" => "Positive infinity (∞).",
        | "nan" => "IEEE 754 Not-a-Number sentinel.",
        | "i" => "Imaginary unit i (i² = -1).",
        // IO & console
        | "print" => "Write a string to standard output without a trailing newline.",
        | "println" => "Write a string to standard output followed by a newline.",
        | "eprintln" => "Write a string to standard error followed by a newline.",
        | "read_line" => "Read one line from standard input as an `IO String`.",
        | "read_file" => "Read the entire UTF-8 contents of a file at the given path.",
        | "write_file" => "Write a UTF-8 string to a file at the given path.",
        | "time_now" => "Return the current wall-clock time as an `IO` action.",
        | "random" => "Generate a pseudo-random floating-point number in `[0, 1)`.",
        // Elementary math
        | "sqrt" => "Principal square root √x.",
        | "cbrt" => "Real cube root ∛x.",
        | "exp" => "Natural exponential function eˣ.",
        | "ln" => "Natural logarithm (base e).",
        | "log" => "Logarithm with the given base or natural logarithm.",
        | "log2" => "Base-2 logarithm.",
        | "log10" => "Base-10 logarithm.",
        | "sin" => "Sine of an angle.",
        | "cos" => "Cosine of an angle.",
        | "tan" => "Tangent of an angle.",
        | "asin" => "Inverse sine (arcsin).",
        | "acos" => "Inverse cosine (arccos).",
        | "atan" => "Inverse tangent (arctan).",
        | "atan2" => "Two-argument inverse tangent atan2(y, x).",
        | "sinh" => "Hyperbolic sine.",
        | "cosh" => "Hyperbolic cosine.",
        | "tanh" => "Hyperbolic tangent.",
        | "asinh" => "Inverse hyperbolic sine.",
        | "acosh" => "Inverse hyperbolic cosine.",
        | "atanh" => "Inverse hyperbolic tangent.",
        | "gamma" => "Euler gamma function Γ(x).",
        | "lgamma" => "Natural logarithm of |Γ(x)|.",
        | "erf" => "Gauss error function erf(x).",
        | "erfc" => "Complementary error function 1 - erf(x).",
        | "hypot" => "Euclidean hypotenuse √(x² + y²).",
        | "abs" => "Absolute value |x|.",
        | "sign" => "Signum (-1, 0, or 1).",
        | "min" => "Smaller of two values.",
        | "max" => "Larger of two values.",
        | "clamp" => "Restrict a value to the inclusive range `[lo, hi]`.",
        | "floor" => "Greatest integer ≤ x.",
        | "ceil" => "Least integer ≥ x.",
        | "round" => "Nearest integer to x.",
        | "trunc" => "Integer part of x, rounded toward zero.",
        | "is_nan" => "True when the value is NaN.",
        | "is_finite" => "True when the value is neither infinite nor NaN.",
        | "is_infinite" => "True when the value is +∞ or -∞.",
        // Units & conversions
        | "to" => "Convert a quantity to another unit of the same dimension.",
        | "delta_to" => "Convert a temperature difference to another unit.",
        | "value_in" => "Extract the dimensionless numeric magnitude in the given unit.",
        | "base" => "Convert a quantity to SI base units.",
        // Collections, vectors, matrices
        | "linspace" => "Generate evenly spaced values over an interval.",
        | "range" => "Generate a sequence of integers.",
        | "length" => "Number of elements in a list, vector, or string.",
        | "shape" => "Dimensions of a tensor or matrix.",
        | "rank" => "Number of axes of a tensor.",
        | "sum" => "Sum of all elements.",
        | "prod" => "Product of all elements.",
        | "mean" => "Arithmetic mean of all elements.",
        | "var" => "Variance of all elements.",
        | "std" => "Standard deviation of all elements.",
        | "norm" => "Euclidean (L2) norm.",
        | "normalize" => "Unit vector in the same direction.",
        | "dot" => "Dot (inner) product of two vectors.",
        | "matmul" => "Matrix-matrix multiplication.",
        | "transpose" => "Matrix transpose.",
        | "trace" => "Sum of diagonal entries of a square matrix.",
        | "det" => "Determinant of a square matrix.",
        | "inv" => "Multiplicative inverse of a square matrix.",
        | "linsolve" => "Solve a linear system A·x = b.",
        | "eye" => "Identity matrix of the given size.",
        | "diag" => "Diagonal matrix from a vector, or diagonal of a matrix.",
        | "map" => "Apply a function to every element of a list.",
        | "filter" => "Keep elements satisfying a predicate.",
        | "fold" | "fold_left" => "Reduce a list from the left with an accumulator.",
        | "zip" => "Pair corresponding elements of two lists.",
        | "enumerate" => "Pair each element with its 0-based index.",
        | "take" => "Take the first n elements.",
        | "drop" => "Drop the first n elements.",
        | "reverse" => "Reverse the order of elements.",
        | "sort" => "Sort elements in ascending order.",
        | "concat" => "Concatenate a list of lists.",
        | "any" => "True if any element satisfies the predicate.",
        | "all" => "True if all elements satisfy the predicate.",
        | "find" => "Return the first element satisfying the predicate.",
        // Strings & formatting
        | "join" => "Join strings with a separator.",
        | "split" => "Split a string on a delimiter.",
        | "replace" => "Replace occurrences of a substring.",
        | "trim" => "Strip leading and trailing whitespace.",
        | "upper" => "Convert ASCII/Unicode letters to uppercase.",
        | "lower" => "Convert ASCII/Unicode letters to lowercase.",
        | "contains" => "True if the string contains the given substring.",
        | "starts_with" => "True if the string starts with the given prefix.",
        | "ends_with" => "True if the string ends with the given suffix.",
        | "parse_int" => "Parse an integer from a string.",
        | "parse_float" => "Parse a floating-point number from a string.",
        | "format" => "Format a value using a format specification.",
        | "show" => "Convert a value to its human-readable string representation.",
        | "repr" => "Convert a value to its source-like representation.",
        | "to_pretty" => "Render a value as two-dimensional terminal mathematics.",
        | "to_latex" => "Render a value as LaTeX source.",
        | "to_typst" => "Render a value as Typst math source.",
        // Symbolic & assertions
        | "diff" => "Symbolic derivative with respect to a variable.",
        | "diff_at" => "Evaluate the derivative at a point.",
        | "antiderivative" => "Symbolic indefinite integral.",
        | "defint" => "Definite integral over `[a, b]`.",
        | "limit" => "Symbolic limit as a variable approaches a point.",
        | "series" => "Taylor/Maclaurin series expansion.",
        | "solve" => "Solve an equation symbolically.",
        | "nsolve" => "Solve an equation numerically.",
        | "eval" => "Evaluate a symbolic expression.",
        | "simplify" => "Simplify a symbolic expression algebraically.",
        | "expand" => "Expand products and powers in a symbolic expression.",
        | "subst" => "Substitute a value or expression for a symbol.",
        | "assert" => "Assert that a boolean condition is true.",
        | "assert_eq" => "Assert that two values are equal.",
        | "approx_eq" => "Assert that two floating-point values are approximately equal.",
        | _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_prelude_name_is_a_builtin() {
        let stale: Vec<&str> = NAMES
            .iter()
            .copied()
            .filter(|name| !builtins::is_builtin(name) && *name != "to_callback")
            .collect();
        assert!(
            stale.is_empty(),
            "these names are in the prelude list but not in the checker: {stale:?}"
        );
    }

    #[test]
    fn the_list_has_no_duplicates() {
        let mut sorted = NAMES.to_vec();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(before, sorted.len(), "the prelude list repeats a name");
    }

    #[test]
    fn a_scheme_is_available_for_every_name() {
        for name in names() {
            assert!(
                builtins::schemes(name).is_some() || name == "to_callback",
                "`{name}` is listed but has no scheme"
            );
        }
    }
}
