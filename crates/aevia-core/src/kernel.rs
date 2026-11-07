//! The runtime's side of the rssn bridge.
//!
//! `aevia-core` never names an rssn type. It converts its own values into the
//! bridge's neutral [`Val`] and back, which keeps the kernel's ABI in one
//! crate and makes the conversion auditable in one file.
//!
//! Two rules from the design drive the shape of this module:
//!
//! * the term graph is **dimensionless** (§10.3.6): dimensions live in the
//!   static type, so a dimension is dropped on the way in and re-attached on
//!   the way out from the caller's static knowledge;
//! * a symbolic result that this runtime cannot represent stays in the kernel
//!   as [`Term::Foreign`], so `integral(exp(-x^2))` prints `sqrt(pi)*erf(x)/2`
//!   instead of failing.

use crate::eval::EvalError;
use crate::sym::Term;
use crate::value::NumKind;
use crate::value::NumVal;
use crate::value::RecordValue;
use crate::value::Storage;
use crate::value::TensorData;
use crate::value::Value;
use crate::value::VariantValue;
use aevia_dim::Dim;
use aevia_rssn::Expr;
use aevia_rssn::KernelError;
use aevia_rssn::Num;
use aevia_rssn::Sym;
use aevia_rssn::Val;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;
use num_traits::ToPrimitive;
use num_traits::Zero;
use std::collections::HashMap;
use std::rc::Rc;

/// Turns a bridge failure into a runtime error.
#[must_use]
pub fn error(failure: &KernelError) -> EvalError {
    let mut error = EvalError::new(failure.code, failure.message.clone());
    if let Some(help) = &failure.help {
        error = error.with_note(help.clone());
    }
    error
}

/// Converts an Aevia value to the bridge's model.
///
/// # Errors
///
/// A closure or a constructor has no counterpart in the kernel and is
/// reported as `E1003`.
pub fn to_val(value: &Value) -> Result<Val, EvalError> {
    Ok(match value {
        | Value::KernelValue(value) => value.payload.clone(),
        | Value::Unit => {
            Val::Variant {
                tag: "Unit".to_string(),
                payload: Vec::new(),
            }
        },
        | Value::Bool(flag) => Val::Bool(*flag),
        | Value::Str(text) => Val::Str(text.to_string()),
        | Value::Num(number) => {
            match &number.kind {
                | NumKind::Int(value) => Val::Num(Num::Int(value.clone())),
                | NumKind::Exact(value) => Val::Num(Num::Rat(value.clone())),
                | NumKind::Float(value) => Val::Num(Num::Float(*value)),
                | NumKind::Complex(re, im) => Val::Complex { re: *re, im: *im },
                | NumKind::Sym(term) => {
                    match term.as_foreign() {
                        | Some(sym) => Val::Term(sym),
                        | None => {
                            let kernel = aevia_rssn::kernel();
                            kernel
                                .build(&to_expr(term))
                                .map_err(|failure| error(&failure))?
                        },
                    }
                },
            }
        },
        | Value::Tensor(tensor) => {
            let mut data = Vec::with_capacity(tensor.storage.len());
            for index in 0..tensor.storage.len() {
                if let Some(item) = tensor.get_flat(index) {
                    data.push(to_val(&item)?);
                }
            }
            Val::Tensor {
                shape: tensor.shape.clone(),
                data,
            }
        },
        | Value::Record(record) => {
            let mut fields = Vec::with_capacity(record.fields.len());
            for (name, item) in &record.fields {
                fields.push((name.clone(), to_val(item)?));
            }
            Val::Record(fields)
        },
        | Value::Variant(variant) => {
            let mut payload = Vec::with_capacity(variant.fields.len());
            for item in &variant.fields {
                payload.push(to_val(item)?);
            }
            Val::Variant {
                tag: variant.name.clone(),
                payload,
            }
        },
        // A bare unit is a definition, not a quantity. It crosses with its
        // dimension and scale, so `Rssn.quantity(3.0, Units.m)` sends the
        // kernel a length and not a string it will refuse.
        | Value::UnitOf(unit) => Val::Unit(aevia_rssn::UnitVal::from_def(unit)),
        | Value::Prop(proposition) => {
            return Err(EvalError::new(
                "E1004",
                format!(
                    "`{}` is a proposition, not a value the kernel can take",
                    crate::value::show(&Value::Prop(proposition.clone()))
                ),
            ));
        },
        // A `Dyn` is a quantity with a run-time dimension. The kernel's term
        // graph is dimensionless (§10.3.6), so the payload crosses and the
        // dimension stays host-side, exactly as for a `Scalar`.
        | Value::Dyn(dynamic) => return to_val(&Value::Num(dynamic.as_scalar())),
        | Value::Error => {
            return Err(EvalError::new(
                "E0899",
                "a poisoned value reached the kernel after an earlier error",
            ));
        },
        | Value::Closure(_)
        | Value::Native(_)
        | Value::Ctor { .. }
        | Value::Io(_)
        | Value::TypeRep(_)
        | Value::KernelError(_)
        | Value::Callback(_) => {
            return Err(EvalError::new(
                "E1003",
                "a function cannot be passed to the kernel",
            ));
        },
    })
}

/// Converts a symbolic term to the bridge's value model.
///
/// # Errors
///
/// Propagates the kernel's failure when a node cannot be built.
pub fn term_val(term: &Term) -> Result<Val, EvalError> {
    match term {
        | Term::Foreign { sym, .. } => Ok(Val::Term(*sym)),
        | other => {
            aevia_rssn::kernel()
                .build(&to_expr(other))
                .map_err(|failure| error(&failure))
        },
    }
}

/// Converts a symbolic tree to a build request.
#[must_use]
pub fn to_expr(term: &Term) -> Expr {
    match term {
        // Integers are sent as integers: several kernels key on the exact
        // representation (`integral` recognises `x^2` but not `x^(2/1)`).
        | Term::Num(value) if value.denom().is_one() => Expr::Num(Num::Int(value.numer().clone())),
        | Term::Num(value) => Expr::Num(Num::Rat(value.clone())),
        | Term::Sym(name) => Expr::Sym(name.clone()),
        | Term::Add(items) => Expr::App("add".to_string(), items.iter().map(to_expr).collect()),
        | Term::Mul(items) => Expr::App("mul".to_string(), items.iter().map(to_expr).collect()),
        | Term::Pow(base, exponent) => {
            Expr::App(
                "pow".to_string(),
                vec![to_expr(base), exponent_expr(exponent)],
            )
        },
        | Term::Fun(function, args) => {
            Expr::App(
                function.as_str().to_string(),
                args.iter().map(to_expr).collect(),
            )
        },
        | Term::Foreign { sym, .. } => Expr::Term(*sym),
    }
}

/// An exponent as a build request, integers included.
fn exponent_expr(value: &BigRational) -> Expr {
    if value.denom().is_one() {
        Expr::Num(Num::Int(value.numer().clone()))
    } else {
        Expr::Num(Num::Rat(value.clone()))
    }
}

/// The kernel's rendering of a term, spelt the way Aevia source spells it.
///
/// The kernel prints its own parseable grammar: Euler's number is `E` and a
/// square root is `x^(1/2)`. Aevia's prelude constant is `e` and its root is
/// `sqrt` (§8.2), and a value's display should read as the source that would
/// produce it, so both are respelt; internal symbol suffixes are dropped.
#[must_use]
pub fn foreign_text(text: &str) -> String {
    respell_square_roots(&respell_euler(&clean_internal_names(text)))
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '#'
}

/// `E` as a whole identifier becomes `e`.
fn respell_euler(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (index, &c) in chars.iter().enumerate() {
        let before = index.checked_sub(1).map(|i| chars[i]);
        let after = chars.get(index + 1).copied();
        if c == 'E'
            && !before.is_some_and(|b| is_ident_char(b) || b == '.')
            && !after.is_some_and(|a| is_ident_char(a) || a == '(')
        {
            out.push('e');
        } else {
            out.push(c);
        }
    }
    out
}

/// `atom^(1/2)` becomes `sqrt(atom)`, where `atom` is an identifier, a number,
/// a bracketed group or a call. Anything it cannot delimit is left alone.
fn respell_square_roots(text: &str) -> String {
    const HALF: &str = "^(1/2)";
    let mut current = text.to_string();
    // Each pass rewrites the leftmost occurrence; nested roots take one pass
    // each, and the bound keeps a malformed text from looping.
    for _ in 0..64 {
        let Some(at) = current.find(HALF) else {
            break;
        };
        let chars: Vec<char> = current[..at].chars().collect();
        let mut start = chars.len();
        if chars.last() == Some(&')') {
            let mut depth = 0usize;
            let mut found = None;
            for index in (0..chars.len()).rev() {
                match chars[index] {
                    | ')' => depth += 1,
                    | '(' => {
                        depth -= 1;
                        if depth == 0 {
                            found = Some(index);
                            break;
                        }
                    },
                    | _ => {},
                }
            }
            let Some(open) = found else {
                break;
            };
            start = open;
            // A call keeps its function name: `f(x)^(1/2)`.
            while start > 0 && is_ident_char(chars[start - 1]) {
                start -= 1;
            }
        } else {
            while start > 0 && (is_ident_char(chars[start - 1]) || chars[start - 1] == '.') {
                start -= 1;
            }
        }
        if start == chars.len() {
            break;
        }
        let atom: String = chars[start..].iter().collect();
        let inner = if atom.starts_with('(') && atom.ends_with(')') {
            atom[1..atom.len() - 1].to_string()
        } else {
            atom
        };
        let prefix: String = chars[..start].iter().collect();
        current = format!("{prefix}sqrt({inner}){}", &current[at + HALF.len()..]);
    }
    current
}

#[cfg(test)]
mod spelling_tests {
    use super::foreign_text;

    #[test]
    fn kernel_text_is_respelt_in_aevia_terms() {
        assert_eq!(foreign_text("E"), "e");
        assert_eq!(foreign_text("E^x + 1"), "e^x + 1");
        assert_eq!(foreign_text("pi^(1/2)"), "sqrt(pi)");
        assert_eq!(foreign_text("2*(x + 1)^(1/2)"), "2*sqrt(x + 1)");
        assert_eq!(foreign_text("exp(x)^(1/2)/3"), "sqrt(exp(x))/3");
        assert_eq!(foreign_text("Ei(x) + EE"), "Ei(x) + EE");
        assert_eq!(foreign_text("x^(1/3)"), "x^(1/3)");
    }
}

/// Removes the disambiguating suffix from internal symbol names.
///
/// `symbol("x")` allocates `x`, `x#2`, ... so that two calls denote different
/// symbols (§6.3.6). The suffix must not reach the user: `show` strips it, and
/// so does the kernel's rendering, which is cached here.
#[must_use]
pub fn clean_internal_names(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(position) = rest.find("#diff") {
        out.push_str(&rest[..position]);
        let tail = &rest[position + "#diff".len()..];
        // Only the generated `x#diff12` is stripped; `symbol("a#b")` keeps its
        // name, because `a#b` is the name the user asked for.
        let digits = tail.len() - tail.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0
            || tail.is_empty()
            || !tail.starts_with(|c: char| c.is_alphanumeric() || c == '_')
        {
            rest = &tail[digits..];
            continue;
        }
        out.push_str("#diff");
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Runs a heavy request and returns its value.
///
/// # Errors
///
/// Propagates the kernel's diagnostic.
pub fn compute(
    name: &str,
    args: &[Value],
    opts: &aevia_rssn::Opts,
    dim: Dim,
) -> Result<Value, EvalError> {
    let mut converted = Vec::with_capacity(args.len());
    for arg in args {
        converted.push(to_val(arg)?);
    }
    let kernel = aevia_rssn::kernel();
    let computed = kernel
        .compute_call(name, &converted, opts)
        .map_err(|failure| error(&failure))?;
    Ok(from_val(&computed.val, dim))
}

/// Folds a term that has no free symbols.
///
/// `antiderivative(f)(3)` substitutes into the closed form and can leave
/// `1/3*3^3`; a value with nothing free in it should read as `9`. A handle the
/// bridge cannot read is not a closed form: [`Term::try_free_symbols`] reports
/// it instead of answering `Some(0)`.
#[must_use]
pub fn fold(
    term: &Term,
    dim: Dim,
) -> Option<Value> {
    if !term.try_free_symbols().ok()?.is_empty() {
        return None;
    }
    let value = term_val(term).ok()?;
    let aevia_rssn::Val::Term(handle) = value else {
        return Some(from_val(&value, dim));
    };
    let computed = aevia_rssn::kernel()
        .compute(handle, &aevia_rssn::Opts::symbolic())
        .ok()?;
    Some(from_val(&computed.val, dim))
}

/// Converts a bridge value back into an Aevia value.
///
/// `dim` is the dimension the static type assigned to the result; the kernel
/// does not track it on the static path (§10.4.1).
#[must_use]
pub fn from_val(
    value: &Val,
    dim: Dim,
) -> Value {
    match value {
        | Val::Num(Num::Int(value)) => {
            Value::Num(NumVal {
                kind: NumKind::Int(value.clone()),
                dim,
                display: None,
            })
        },
        | Val::Num(Num::Rat(value)) => {
            if value.denom().is_one() {
                Value::Num(NumVal {
                    kind: NumKind::Int(value.numer().clone()),
                    dim,
                    display: None,
                })
            } else {
                Value::Num(NumVal::exact(value.clone(), dim))
            }
        },
        | Val::Num(Num::Float(value)) => Value::Num(NumVal::float(*value, dim)),
        | Val::Bool(flag) => Value::Bool(*flag),
        | Val::Str(text) => Value::string(text.clone()),
        | Val::Symbol(name) => Value::Num(NumVal::symbolic(Term::Sym(name.clone()), dim)),
        | Val::Term(sym) => {
            let kernel = aevia_rssn::kernel();
            let text = foreign_text(&kernel.text(*sym));
            Value::Num(NumVal::symbolic(Term::foreign(*sym, text), dim))
        },
        | Val::Complex { re, im } => {
            Value::Num(NumVal {
                kind: NumKind::Complex(*re, *im),
                dim,
                display: None,
            })
        },
        | Val::Tensor { shape, data } => {
            let storage: Vec<Value> = data
                .iter()
                .map(|item| from_val(item, dim.clone()))
                .collect();
            Value::Tensor(Rc::new(TensorData {
                verified_element: None,
                shape: shape.clone(),
                storage: Storage::Boxed(storage),
                dim,
                display: None,
            }))
        },
        | Val::Record(fields) => {
            let mut map = indexmap::IndexMap::new();
            for (name, item) in fields {
                map.insert(name.clone(), from_val(item, Dim::one()));
            }
            Value::Record(Rc::new(RecordValue { fields: map }))
        },
        | Val::Variant { tag, payload } => {
            let items: Vec<Value> = payload
                .iter()
                .map(|item| from_val(item, Dim::one()))
                .collect();
            // The tag has to name a constructor this runtime knows; an
            // unknown tag is reported as text rather than invented.
            match aevia_types::adt::find_ctor(tag) {
                | Some((adt, index, _)) => {
                    Value::Variant(Rc::new(VariantValue {
                        adt,
                        index,
                        name: tag.clone(),
                        fields: items,
                        type_args: None,
                        verified_type: None,
                        constructor_origin: false,
                    }))
                },
                | None => {
                    let rendered: Vec<String> = items.iter().map(crate::value::show).collect();
                    Value::string(format!("{tag}({})", rendered.join(", ")))
                },
            }
        },
        // A unit is a value of type `Unit[d]`, not text: returning a `Str`
        // here would give the caller a different runtime variant than the
        // checker promised.
        | Val::Unit(unit) => Value::UnitOf(Rc::new(unit.to_def())),
        | Val::Quantity { value, unit } => {
            let inner = from_val(value, unit.dim.clone());
            match inner {
                // The unit's dimension is the value's dimension, and the
                // display unit is the unit it was written in — both were
                // dropped when a quantity travelled as an opaque string.
                | Value::Num(number) => {
                    let mut number = number.with_display(Rc::new(unit.to_def()));
                    number.dim = unit.dim.clone();
                    Value::Num(number)
                },
                | other => other,
            }
        },
        | Val::Opaque { kind, bytes } => Value::string(format!("<{kind}:{} bytes>", bytes.len())),
    }
}

/// Converts a bridge value into a term, when it has a term form.
#[must_use]
pub fn term_of(val: &Val) -> Option<Term> {
    match val {
        | Val::Num(Num::Int(value)) => Some(Term::Num(BigRational::from_integer(value.clone()))),
        | Val::Num(Num::Rat(value)) => Some(Term::Num(value.clone())),
        | Val::Num(Num::Float(value)) => {
            Some(Term::Num(
                BigRational::from_float(*value).unwrap_or_else(BigRational::zero),
            ))
        },
        | Val::Symbol(name) => Some(Term::Sym(name.clone())),
        | Val::Term(sym) => {
            let kernel = aevia_rssn::kernel();
            Some(Term::foreign(*sym, foreign_text(&kernel.text(*sym))))
        },
        | _ => None,
    }
}

/// The dimension of a value, defaulting to dimensionless.
#[must_use]
pub fn dim_of(value: &Value) -> Dim {
    crate::value::value_dim(value)
}

/// Evaluates a symbolic term to a float, with the given bindings.
#[must_use]
pub fn eval_term(
    term: &Term,
    bindings: &HashMap<String, f64>,
) -> Option<f64> {
    if let Term::Foreign { sym, .. } = term {
        let pairs: Vec<(String, f64)> = bindings
            .iter()
            .map(|(name, value)| (name.clone(), *value))
            .collect();
        return aevia_rssn::kernel().eval(*sym, &pairs);
    }
    term.eval(bindings).ok()
}

/// A symbolic term for a free symbol.
#[must_use]
pub fn symbol_term(name: &str) -> Term {
    Term::Sym(name.to_string())
}

/// An integer as the bridge's number.
#[must_use]
pub fn int_num(value: i64) -> Num {
    Num::Int(BigInt::from(value))
}

/// A float as the bridge's number.
#[must_use]
pub fn float_num(value: f64) -> Num {
    Num::Float(value)
}

/// The integer value of a bridge number, when it has one.
#[must_use]
pub fn as_i64(value: &Num) -> Option<i64> {
    match value {
        | Num::Int(value) => value.to_i64(),
        | Num::Rat(value) => {
            if value.denom().is_one() {
                value.numer().to_i64()
            } else {
                None
            }
        },
        | Num::Float(value) => {
            if value.fract() == 0.0 {
                Some(*value as i64)
            } else {
                None
            }
        },
    }
}

/// Calls a kernel operator with Aevia values.
///
/// # Errors
///
/// Propagates the kernel's diagnostic.
pub fn call(
    name: &str,
    args: &[Value],
    dim: Dim,
) -> Result<Value, EvalError> {
    let mut converted = Vec::with_capacity(args.len());
    for arg in args {
        converted.push(to_val(arg)?);
    }
    let kernel = aevia_rssn::kernel();
    let result = kernel
        .call(name, &converted)
        .map_err(|failure| error(&failure))?;
    Ok(from_val(&result, dim))
}

/// Calls a kernel operator and keeps the symbolic answer as a term.
///
/// # Errors
///
/// Propagates the kernel's diagnostic, and `E1003` when the answer has no term
/// form at all.
pub fn call_term(
    name: &str,
    args: &[Value],
    _dim: Dim,
) -> Result<Term, EvalError> {
    let mut converted = Vec::with_capacity(args.len());
    for arg in args {
        converted.push(to_val(arg)?);
    }
    let kernel = aevia_rssn::kernel();
    let result = kernel
        .call(name, &converted)
        .map_err(|failure| error(&failure))?;
    term_of(&result).ok_or_else(|| {
        EvalError::new(
            "E1003",
            format!("`{name}` answered with a value that has no term form"),
        )
    })
}

/// A free symbol owned by the kernel.
#[must_use]
pub fn kernel_symbol(name: &str) -> Sym {
    let kernel = aevia_rssn::kernel();
    let value = Val::Symbol(name.to_string());
    kernel.term_of(&value).unwrap_or(Sym::new(0, 0))
}
