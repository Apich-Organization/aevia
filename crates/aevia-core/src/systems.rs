//! Runtime conversions between SI and a unit system (§5.11.5).
//!
//! A value in a unit system is an SI quantity of the system's representative
//! dimension (see `aevia_dim::systems`), so a conversion is a multiplication
//! by the system's factor `Π Qᵢ^λᵢ` and a change of the dimension witness.
//! The factor keeps the value's phase: a `Float` is multiplied in `f64`; an
//! exact value stays exact when every exponent is an integer (the factor is a
//! rational times a power of `pi`) and becomes a symbolic `Real` when some
//! exponent is a fraction (a charge in Heaviside-Lorentz natural units).

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

use crate::fxhash::FxHashMap;
use crate::sym::Term;
use crate::value::NumKind;
use crate::value::NumVal;
use aevia_dim::Dim;
use aevia_dim::Scale;
use aevia_dim::UnitDef;
use aevia_dim::systems::UnitSystem;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::Zero;
use std::rc::Rc;

/// The exact factor as a symbolic term.
#[allow(
    clippy::single_call_fn,
    reason = "Encodes one symbolic scale rather than the entire conversion"
)]
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
/// Returns an error for an invalid basis or an exact zero conversion factor.
#[allow(clippy::single_call_fn, reason = "Runtime conversion entry point")]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "Exact products use BigRational; floating factors use IEEE arithmetic"
)]
pub fn convert(
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
                | NumKind::Int(value) => Term::Num(value.to_rational()),
                | NumKind::Exact(value) => Term::Num(value.as_ref().clone()),
                | NumKind::Sym(term) => (**term).clone(),
                | _ => unreachable!("matched above"),
            };
            match system.factor_scale(&reduction) {
                | Some(scale) if scale.pi == 0 => {
                    let factor = if direction > 0 {
                        scale.rat
                    } else {
                        if scale.rat.is_zero() {
                            return Err(format!(
                                "unit system `{}` has a zero conversion factor",
                                system.name
                            ));
                        }
                        num_traits::Inv::inv(scale.rat)
                    };
                    match value {
                        | Term::Num(value) => NumKind::Exact(Rc::new(value * factor)),
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
#[allow(clippy::single_call_fn, reason = "Runtime lookup entry point")]
pub fn lookup(name: &str) -> Option<std::sync::Arc<UnitSystem>> {
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
        let mut units_by_dimension = FxHashMap::default();
        display_in(system, units, &mut units_by_dimension, value, 0)
    }
}

fn display_in(
    system: &UnitSystem,
    units: &aevia_dim::UnitTable,
    units_by_dimension: &mut FxHashMap<Dim, Option<Rc<UnitDef>>>,
    value: crate::Value,
    depth: usize,
) -> crate::Value {
    use crate::Value;
    if depth > 16 {
        return value;
    }
    match value {
        | Value::Num(mut num) if num.display.is_none() && !num.dim.is_one() => {
            num.display = if depth == 0 {
                display_unit(system, units, &num.dim)
            } else if let Some(unit) = units_by_dimension.get(&num.dim) {
                unit.clone()
            } else {
                // A rendering uses one lexical unit table. Bound scratch
                // space even for a record with many distinct dimensions.
                let unit = display_unit(system, units, &num.dim);
                if units_by_dimension.len() < 64 {
                    units_by_dimension.insert(num.dim.clone(), unit.clone());
                }
                unit
            };
            Value::Num(num)
        },
        | Value::Variant(mut variant) => {
            for field in &mut Rc::make_mut(&mut variant).fields {
                let value = std::mem::replace(field, Value::Unit);
                *field = display_in(
                    system,
                    units,
                    units_by_dimension,
                    value,
                    depth.saturating_add(1),
                );
            }
            Value::Variant(variant)
        },
        | Value::Record(mut record) => {
            for field in Rc::make_mut(&mut record).fields.values_mut() {
                let value = std::mem::replace(field, Value::Unit);
                *field = display_in(
                    system,
                    units,
                    units_by_dimension,
                    value,
                    depth.saturating_add(1),
                );
            }
            Value::Record(record)
        },
        | other => other,
    }
}

fn display_unit(
    system: &UnitSystem,
    units: &aevia_dim::UnitTable,
    dim: &Dim,
) -> Option<Rc<UnitDef>> {
    let (unit, power) = system.display_of(dim)?;
    let base = units.lookup(&unit)?;
    let power_i32 = num_traits::ToPrimitive::to_i32(&power)?;
    let exponent = BigRational::from_integer(power);
    let scale = base.scale.pow(&exponent)?;
    let name = if power_i32 == 1 {
        base.display
    } else {
        format!("{}^{power_i32}", base.display)
    };
    Some(Rc::new(UnitDef {
        name: name.clone(),
        aliases: Vec::new(),
        dim: dim.clone(),
        scale,
        offset: None,
        prefixable: false,
        affine: false,
        display: name,
        category: "system".to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aevia_dim::systems::Generator;
    use aevia_dim::systems::Unity;

    #[test]
    fn exact_conversion_reports_zero_inverse_factors() {
        let length = Dim::base("Length");
        let system = UnitSystem {
            name: "zero".into(),
            description: String::new(),
            unity: vec![Unity {
                symbol: "q".into(),
                label: "q".into(),
                dim: length.clone(),
                value: Scale::rational(BigRational::zero()),
            }],
            generators: [
                "Mass",
                "Time",
                "Current",
                "Temperature",
                "Amount",
                "Luminosity",
            ]
            .map(|name| {
                Generator {
                    name: name.to_owned(),
                    dim: Dim::base(name),
                    display: name.to_owned(),
                }
            })
            .to_vec(),
        };
        let input = NumVal::int(BigInt::from(1));
        let error = convert(&system, &length, &input, -1, Dim::one()).unwrap_err();
        assert!(error.contains("zero conversion factor"));
    }
}
