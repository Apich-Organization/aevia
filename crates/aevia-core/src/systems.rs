//! Runtime conversions between SI and a unit system (§5.11.5).
//!
//! A value in a unit system is an SI quantity of the system's representative
//! dimension (see `aevia_dim::systems`), so a conversion is a multiplication
//! by the system's factor `Π Qᵢ^λᵢ` and a change of the dimension witness.
//! The factor keeps the value's phase: a `Float` is multiplied in `f64`; an
//! exact value stays exact when every exponent is an integer (the factor is a
//! rational times a power of `pi`) and becomes a symbolic `Real` when some
//! exponent is a fraction (a charge in Heaviside-Lorentz natural units).

use crate::sym::Term;
use crate::value::NumKind;
use crate::value::NumVal;
use aevia_dim::Dim;
use aevia_dim::Scale;
use aevia_dim::systems::UnitSystem;
use num_bigint::BigInt;
use num_rational::BigRational;
use std::rc::Rc;

/// The exact factor as a symbolic term.
fn scale_term(scale: &Scale) -> Term {
    let mut term = Term::Num(scale.rat.clone());
    if scale.pi != 0 {
        term = Term::mul(
            term,
            Term::pow(
                Term::Sym("pi".to_string()),
                BigRational::from_integer(BigInt::from(scale.pi)),
            ),
        );
    }
    term
}

/// Multiplies `num` by the factor of the decomposition of `si`, raised to
/// `direction` (`1` from the system to SI, `-1` from SI into the system), and
/// gives it the dimension `target`.
///
/// # Errors
///
/// A message when the value is not a real scalar.
pub(crate) fn convert(
    system: &UnitSystem,
    si: &Dim,
    num: &NumVal,
    direction: i32,
    target: Dim,
) -> Result<NumVal, String> {
    let reduction = system
        .reduce(si)
        .ok_or_else(|| format!("unit system `{}` is not a basis", system.name))?;
    let sign = BigRational::from_integer(BigInt::from(direction));
    let kind = match &num.kind {
        | NumKind::Float(value) => {
            let factor = system.factor_f64(&reduction);
            NumKind::Float(if direction > 0 {
                value * factor
            } else {
                value / factor
            })
        },
        | NumKind::Complex(re, im) => {
            let factor = system.factor_f64(&reduction);
            let factor = if direction > 0 {
                factor
            } else {
                1.0 / factor
            };
            NumKind::Complex(re * factor, im * factor)
        },
        | NumKind::Int(_) | NumKind::Exact(_) | NumKind::Sym(_) => {
            let value = match &num.kind {
                | NumKind::Int(value) => Term::Num(BigRational::from_integer(value.clone())),
                | NumKind::Exact(value) => Term::Num(value.clone()),
                | NumKind::Sym(term) => (**term).clone(),
                | _ => unreachable!("matched above"),
            };
            match system.factor_scale(&reduction) {
                | Some(scale) if scale.pi == 0 => {
                    let factor = if direction > 0 {
                        scale.rat
                    } else {
                        num_traits::Inv::inv(scale.rat)
                    };
                    match value {
                        | Term::Num(value) => NumKind::Exact(value * factor),
                        | other => NumKind::Sym(Rc::new(Term::mul(other, Term::Num(factor)))),
                    }
                },
                | _ => {
                    let mut term = value;
                    for (scale, lambda) in system.factor_terms(&reduction) {
                        term = Term::mul(term, Term::pow(scale_term(&scale), lambda * &sign));
                    }
                    NumKind::Sym(Rc::new(term))
                },
            }
        },
    };
    Ok(NumVal {
        kind,
        dim: target,
        display: None,
    })
}

/// The system called `name`, from the kernel manifest.
pub(crate) fn lookup(name: &str) -> Option<std::sync::Arc<UnitSystem>> {
    aevia_rssn::registry::registry().unit_system(name)
}

impl crate::Interp {
    /// §5.11.6: in a module with a unit system, a quantity with no display
    /// unit of its own is shown in the system's generator unit — an inverse
    /// energy as `eV^-1` in natural units rather than `s^2/(m^2*kg)`. The
    /// value is unchanged; only its display unit is chosen. Records, variants
    /// (`Ok(x)` from `approx`) and tuples are displayed field by field.
    pub(crate) fn system_display(
        &self,
        value: crate::Value,
    ) -> crate::Value {
        let units = &self.context.evidence.units;
        let Some(system) = units.system() else {
            return value;
        };
        display_in(system, units, value, 0)
    }
}

fn display_in(
    system: &UnitSystem,
    units: &aevia_dim::UnitTable,
    value: crate::Value,
    depth: usize,
) -> crate::Value {
    use crate::Value;
    if depth > 16 {
        return value;
    }
    match value {
        | Value::Num(num) if num.display.is_none() && !num.dim.is_one() => {
            let Some((unit, power)) = system.display_of(&num.dim) else {
                return Value::Num(num);
            };
            let Some(base) = units.lookup(&unit) else {
                return Value::Num(num);
            };
            let Some(power_i32) = num_traits::ToPrimitive::to_i32(&power) else {
                return Value::Num(num);
            };
            let exponent = BigRational::from_integer(power.clone());
            let Some(scale) = base.scale.pow(&exponent) else {
                return Value::Num(num);
            };
            let name = if power_i32 == 1 {
                base.display.clone()
            } else {
                format!("{}^{power_i32}", base.display)
            };
            let def = aevia_dim::UnitDef {
                name: name.clone(),
                aliases: Vec::new(),
                dim: num.dim.clone(),
                scale,
                offset: None,
                prefixable: false,
                affine: false,
                display: name,
                category: "system".to_string(),
            };
            Value::Num(num.with_display(Rc::new(def)))
        },
        | Value::Variant(variant) => {
            let mut variant = (*variant).clone();
            for field in &mut variant.fields {
                *field = display_in(system, units, field.clone(), depth + 1);
            }
            Value::Variant(Rc::new(variant))
        },
        | Value::Record(record) => {
            let mut record = (*record).clone();
            for field in record.fields.values_mut() {
                *field = display_in(system, units, field.clone(), depth + 1);
            }
            Value::Record(Rc::new(record))
        },
        | other => other,
    }
}
