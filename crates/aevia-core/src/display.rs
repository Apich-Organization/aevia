//! Value rendering shared by the language's output utilities and the REPL.
//!
//! Rendering consumes structured values and authenticated symbolic handles.
//! `Repr` retains Aevia's exact phase and symbol identity; no renderer parses
//! `Show` text back into an expression to obtain a different layout.

use crate::value::ExactRep;
use crate::value::MathSyntax;
use crate::value::NumKind;
use crate::value::Storage;
use crate::value::Value;
use crate::value::{
    self,
};

/// The output syntax requested by a user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Compact human-readable Aevia output.
    Show,
    /// Aevia representation preserving numeric phase and symbol identity.
    Repr,
    /// Multi-line Unicode fractions, powers, radicals and matrices.
    Pretty,
    /// LaTeX math source, without surrounding math delimiters.
    Latex,
    /// Typst math source, without surrounding math delimiters.
    Typst,
}

/// Renders a value in one of the language's supported output formats.
///
/// # Errors
/// Mathematical rendering rejects functions, unevaluated IO and other values
/// with no mathematical form; stale symbolic handles are reported explicitly.
pub fn render(
    value: &Value,
    format: Format,
) -> Result<String, String> {
    match format {
        | Format::Show => Ok(value::show(value)),
        | Format::Repr => Ok(value::repr(value)),
        | Format::Pretty => pretty(value),
        | Format::Latex => value::to_math(value, MathSyntax::Latex),
        | Format::Typst => value::to_math(value, MathSyntax::Typst),
    }
}

fn pretty(value: &Value) -> Result<String, String> {
    match value {
        | Value::Num(number) => {
            // Complex literals have no RSSN term form. Their ordinary numeric
            // spelling is already the useful one-line mathematical layout.
            if matches!(number.kind, NumKind::Complex(..)) {
                return Ok(value::show(value));
            }
            let body = pretty_math(value)?;
            Ok(with_unit(body, &value::si_unit_text(&number.dim)))
        },
        | Value::Tensor(tensor) => {
            // Boxed tensors can hold ordinary values, including records and
            // strings. Only mathematical tensors use the matrix printer.
            let logical = tensor
                .shape
                .iter()
                .try_fold(1usize, |count, extent| count.checked_mul(*extent))
                .ok_or_else(|| "tensor shape is too large to render".to_string())?;
            if tensor.rank() == 0 {
                return pretty(
                    &tensor
                        .get_flat(0)
                        .ok_or_else(|| "scalar tensor has no element".to_string())?,
                );
            }
            // The classification examines the logical prefix. Conversion
            // separately validates the complete storage length.
            let mathematical = match &tensor.storage {
                | Storage::Dense(values) => values.len() >= logical,
                | Storage::Exact(values) | Storage::TypedExact(values, ExactRep::Real) => {
                    values.len() >= logical
                },
                | Storage::TypedExact(values, ExactRep::Int) => {
                    values.get(..logical).is_some_and(|values| {
                        values.iter().all(num_rational::BigRational::is_integer)
                    })
                },
                | Storage::View(_) | Storage::SharedSlice(_) if tensor.dense().is_some() => true,
                | _ => {
                    (0..logical).all(|index| {
                        matches!(tensor.get_flat(index), Some(Value::Num(number)) if !matches!(number.kind, NumKind::Complex(..)))
                    })
                },
            };
            if mathematical {
                let body = pretty_math(value)?;
                Ok(with_unit(body, &value::si_unit_text(&tensor.dim)))
            } else {
                Ok(value::show(value))
            }
        },
        | Value::Dyn(dynamic) => pretty(&Value::Num(dynamic.as_scalar())),
        | Value::KernelValue(payload) => {
            aevia_rssn::kernel()
                .to_pretty(&payload.payload)
                .map_err(|error| error.message)
        },
        | Value::Closure(_) | Value::Native(_) => {
            Err("a function has no mathematical form; use show or repr".into())
        },
        | Value::Io(_) => {
            Err("an IO action has no mathematical form; execute it or use show".into())
        },
        | Value::Task(_) => Err("a task has no mathematical form; await it or use show".into()),
        | Value::Error => Err("an error value has no mathematical form".into()),
        // Ordinary data keeps Aevia's readable constructor/record/string
        // notation, instead of pretending it is a matrix or symbolic term.
        | _ => Ok(value::show(value)),
    }
}

fn pretty_math(value: &Value) -> Result<String, String> {
    let structured = crate::kernel::to_val(value).map_err(|error| error.message)?;
    aevia_rssn::kernel()
        .to_pretty(&structured)
        .map_err(|error| error.message)
}

fn with_unit(
    body: String,
    unit: &str,
) -> String {
    if unit.is_empty() {
        body
    } else if body.contains('\n') {
        // A unit appended to the final row of a fraction would look like part
        // of its denominator. Label the unit separately for multi-line output.
        format!("{body}\nunit: {unit}")
    } else {
        format!("{body} {unit}")
    }
}
