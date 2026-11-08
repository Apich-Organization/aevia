//! Unit systems (§5.11): natural, Planck, atomic and geometrized units.
//!
//! A unit system sets a few physical quantities to one. Setting `c = 1`
//! identifies `Length` with `Time`; setting `c = ħ = k_B = ε₀ = 1` makes
//! every mechanical, thermal and electromagnetic dimension a power of
//! energy. The definitions (which quantities, which generators) come from
//! the kernel manifest; this module does the arithmetic on [`Dim`].
//!
//! # The decomposition
//!
//! With unity dimensions `q₁ … q_k` and generators `g₁ … g_m` spanning the
//! seven SI bases, every SI dimension decomposes uniquely as
//! `d = Π qᵢ^λᵢ · Π gⱼ^μⱼ`. The **representative** of `d` is `Π gⱼ^μⱼ`,
//! and a quantity `x` of dimension `d` has the system value
//! `x / Π Qᵢ^λᵢ`, where `Qᵢ` is the SI value of the i-th unity quantity.
//!
//! That system value is itself an honest SI quantity of the representative
//! dimension: `1 m` in natural units is `1 m / (ħ c) ≈ 5.07e24 J⁻¹`. This is
//! what makes the design sound in a dimension-typed language — a module in
//! natural units *reduces the dimensions written in it* (annotations, unit
//! literals) but never reinterprets a value, so a value can cross into an
//! SI module and is still correctly typed there (§5.11.3).
//!
//! Non-SI atoms — dimension variables and user base dimensions — are left
//! untouched: they are not in the span of the SI bases.

use crate::Dim;
use crate::Scale;
use crate::dim::Atom;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;
use num_traits::ToPrimitive;
use num_traits::Zero;

/// The SI bases, in the order the decomposition uses.
const BASES: [&str; 7] = [
    "Length",
    "Mass",
    "Time",
    "Current",
    "Temperature",
    "Amount",
    "Luminosity",
];

/// One quantity a system sets to one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unity {
    /// The constant's symbol (`hbar`).
    pub symbol: String,
    /// How the system writes it (`4 pi epsilon_0`).
    pub label: String,
    /// Its SI dimension.
    pub dim: Dim,
    /// Its exact SI value, multiplier included.
    pub value: Scale,
}

/// One generator of the representatives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generator {
    /// What it measures (`Energy`).
    pub name: String,
    /// Its SI dimension.
    pub dim: Dim,
    /// The unit values of this generator display in (`eV`).
    pub display: String,
}

/// A unit system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitSystem {
    /// The name a module selects it by (`natural`).
    pub name: String,
    /// One-line description.
    pub description: String,
    /// The quantities set to one.
    pub unity: Vec<Unity>,
    /// The generators of the representatives.
    pub generators: Vec<Generator>,
}

/// The decomposition of one dimension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reduction {
    /// The representative (non-SI atoms carried through).
    pub dim: Dim,
    /// `(index into unity, λ)` for every non-zero power of a unity quantity.
    pub factors: Vec<(usize, BigRational)>,
}

impl Reduction {
    /// Whether the system leaves the dimension unchanged.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.factors.is_empty()
    }

    /// Whether every exponent is an integer, so the factor is an exact
    /// [`Scale`].
    #[must_use]
    pub fn is_integral(&self) -> bool {
        self.factors
            .iter()
            .all(|(_, lambda)| lambda.denom().is_one())
    }
}

type Vector = [BigRational; 7];

fn zero() -> Vector {
    std::array::from_fn(|_| BigRational::zero())
}

/// Splits a dimension into its SI exponent vector and the rest.
fn split(dim: &Dim) -> (Vector, Vec<(Atom, BigRational)>) {
    let mut si = zero();
    let mut rest = Vec::new();
    for (atom, exponent) in dim.factors() {
        match &atom {
            | Atom::Base(name) => {
                if let Some(index) = BASES.iter().position(|base| *base == &**name) {
                    si[index] += exponent;
                } else {
                    rest.push((atom.clone(), exponent.clone()));
                }
            },
            | _ => rest.push((atom.clone(), exponent.clone())),
        }
    }
    (si, rest)
}

fn vector_of(dim: &Dim) -> Option<Vector> {
    let (si, rest) = split(dim);
    rest.is_empty().then_some(si)
}

/// Solves `Σ xᵢ columnsᵢ = rhs` over ℚ; `None` when the columns are not a
/// basis.
fn solve(
    columns: &[Vector],
    rhs: &Vector,
) -> Option<Vec<BigRational>> {
    let n = 7;
    if columns.len() != n {
        return None;
    }
    let mut m: Vec<Vec<BigRational>> = (0..n)
        .map(|row| {
            let mut line: Vec<BigRational> =
                columns.iter().map(|column| column[row].clone()).collect();
            line.push(rhs[row].clone());
            line
        })
        .collect();
    for col in 0..n {
        let pivot = (col..n).find(|&row| !m[row][col].is_zero())?;
        m.swap(col, pivot);
        let lead = m[col][col].clone();
        for value in &mut m[col] {
            *value /= &lead;
        }
        for row in 0..n {
            if row != col && !m[row][col].is_zero() {
                let factor = m[row][col].clone();
                for k in col..=n {
                    let delta = &factor * &m[col][k];
                    m[row][k] -= delta;
                }
            }
        }
    }
    Some(
        m.into_iter()
            .map(|mut row| row.pop().unwrap_or_else(BigRational::zero))
            .collect(),
    )
}

impl UnitSystem {
    fn columns(&self) -> Option<Vec<Vector>> {
        self.unity
            .iter()
            .map(|unity| vector_of(&unity.dim))
            .chain(self.generators.iter().map(|g| vector_of(&g.dim)))
            .collect()
    }

    /// Whether the unity dimensions and generators form a basis of the SI
    /// dimensions; a manifest entry that does not is ignored by the host.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let Some(columns) = self.columns() else {
            return false;
        };
        (0..7).all(|index| {
            let mut target = zero();
            target[index] = BigRational::one();
            solve(&columns, &target).is_some()
        })
    }

    /// Decomposes `dim`; `None` only for an invalid system.
    #[must_use]
    pub fn reduce(
        &self,
        dim: &Dim,
    ) -> Option<Reduction> {
        let (si, rest) = split(dim);
        if si.iter().all(Zero::is_zero) {
            return Some(Reduction {
                dim: dim.clone(),
                factors: Vec::new(),
            });
        }
        let columns = self.columns()?;
        let solution = solve(&columns, &si)?;
        let (lambdas, mus) = solution.split_at(self.unity.len());
        let mut factors: Vec<(Atom, BigRational)> = rest;
        for (generator, mu) in self.generators.iter().zip(mus) {
            if mu.is_zero() {
                continue;
            }
            for (atom, exponent) in generator.dim.factors() {
                factors.push((atom.clone(), exponent * mu));
            }
        }
        Some(Reduction {
            dim: Dim::from_factors(factors),
            factors: lambdas
                .iter()
                .enumerate()
                .filter(|(_, lambda)| !lambda.is_zero())
                .map(|(index, lambda)| (index, lambda.clone()))
                .collect(),
        })
    }

    /// The representative of `dim` (its decomposition's dimension).
    #[must_use]
    pub fn reduce_dim(
        &self,
        dim: &Dim,
    ) -> Dim {
        self.reduce(dim)
            .map_or_else(|| dim.clone(), |reduction| reduction.dim)
    }

    /// `Π Qᵢ^λᵢ` as an exact scale, when every exponent is an integer.
    #[must_use]
    pub fn factor_scale(
        &self,
        reduction: &Reduction,
    ) -> Option<Scale> {
        let mut scale = Scale::one();
        for (index, lambda) in &reduction.factors {
            if !lambda.denom().is_one() {
                return None;
            }
            let power = BigRational::from_integer(lambda.numer().clone());
            scale = scale.mul(&self.unity.get(*index)?.value.pow(&power)?);
        }
        Some(scale)
    }

    /// `Π Qᵢ^λᵢ` numerically, for any rational exponents.
    #[must_use]
    pub fn factor_f64(
        &self,
        reduction: &Reduction,
    ) -> f64 {
        reduction
            .factors
            .iter()
            .map(|(index, lambda)| {
                let base = self
                    .unity
                    .get(*index)
                    .map_or(f64::NAN, |unity| unity.value.to_f64());
                base.powf(lambda.to_f64().unwrap_or(f64::NAN))
            })
            .product()
    }

    /// The factor as `(SI value of the unity quantity, λ)` pairs, for a host
    /// that builds an exact symbolic factor for rational exponents.
    #[must_use]
    pub fn factor_terms(
        &self,
        reduction: &Reduction,
    ) -> Vec<(Scale, BigRational)> {
        reduction
            .factors
            .iter()
            .filter_map(|(index, lambda)| {
                Some((self.unity.get(*index)?.value.clone(), lambda.clone()))
            })
            .collect()
    }

    /// The display unit of a representative built from one generator to an
    /// integer power (`eV`, `eV^-2`), when there is one.
    #[must_use]
    pub fn display_of(
        &self,
        dim: &Dim,
    ) -> Option<(String, BigInt)> {
        for generator in &self.generators {
            let (g, _) = split(&generator.dim);
            let (d, rest) = split(dim);
            if !rest.is_empty() {
                return None;
            }
            // `d = g^k` for a rational k shared by every component.
            let mut ratio: Option<BigRational> = None;
            let mut consistent = true;
            for (gi, di) in g.iter().zip(&d) {
                match (gi.is_zero(), di.is_zero()) {
                    | (true, true) => {},
                    | (true, false) | (false, true) => {
                        consistent = false;
                        break;
                    },
                    | (false, false) => {
                        let k = di / gi;
                        if ratio.as_ref().is_some_and(|r| *r != k) {
                            consistent = false;
                            break;
                        }
                        ratio = Some(k);
                    },
                }
            }
            if let (true, Some(k)) = (consistent, ratio) {
                if k.denom().is_one() {
                    return Some((generator.display.clone(), k.numer().clone()));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DimEnv;

    fn natural() -> UnitSystem {
        let env = DimEnv::si();
        let dim = |text: &str| env.parse(text).unwrap();
        let scale = |text: &str, pi: i32| {
            let mut s = Scale::parse(text).unwrap();
            s.pi = pi;
            s
        };
        UnitSystem {
            name: "natural".into(),
            description: String::new(),
            unity: vec![
                Unity {
                    symbol: "c".into(),
                    label: "c".into(),
                    dim: dim("L/T"),
                    value: scale("299792458", 0),
                },
                Unity {
                    symbol: "hbar".into(),
                    label: "hbar".into(),
                    dim: dim("M*L^2/T"),
                    value: scale("3.313035075e-34", -1),
                },
                Unity {
                    symbol: "k_B".into(),
                    label: "k_B".into(),
                    dim: dim("M*L^2/(T^2*Θ)"),
                    value: scale("1.380649e-23", 0),
                },
                Unity {
                    symbol: "epsilon_0".into(),
                    label: "epsilon_0".into(),
                    dim: dim("I^2*T^4/(M*L^3)"),
                    value: scale("8.8541878188e-12", 0),
                },
            ],
            generators: vec![
                Generator {
                    name: "Energy".into(),
                    dim: dim("M*L^2/T^2"),
                    display: "eV".into(),
                },
                Generator {
                    name: "Amount".into(),
                    dim: dim("Amt"),
                    display: "mol".into(),
                },
                Generator {
                    name: "Luminosity".into(),
                    dim: dim("Lum"),
                    display: "cd".into(),
                },
            ],
        }
    }

    #[test]
    fn natural_units_reduce_to_powers_of_energy() {
        let system = natural();
        assert!(system.is_valid());
        let env = DimEnv::si();
        let energy = env.parse("M*L^2/T^2").unwrap();
        let length = system.reduce(&Dim::base("Length")).unwrap();
        assert_eq!(
            length.dim,
            energy.pow(&BigRational::from_integer((-1).into()))
        );
        assert!(length.is_integral());
        // 1 m = 1/(hbar c) J^-1, and hbar carries 1/pi.
        let scale = system.factor_scale(&length).unwrap();
        assert_eq!(scale.pi, -1);
        assert!(((1.0 / scale.to_f64()) - 3.163_029_e25).abs() / 3.163e25 < 1e-5);
        // Velocity is dimensionless, mass is energy.
        assert!(
            system
                .reduce(&env.parse("L/T").unwrap())
                .unwrap()
                .dim
                .is_one()
        );
        assert_eq!(system.reduce_dim(&Dim::base("Mass")), energy);
        // Charge needs (epsilon_0 hbar c)^(1/2): not an exact scale.
        let charge = system.reduce(&env.parse("I*T").unwrap()).unwrap();
        assert!(charge.dim.is_one());
        assert!(!charge.is_integral());
        assert!(system.factor_scale(&charge).is_none());
        assert!(system.factor_f64(&charge) > 0.0);
        // The representative's display unit.
        assert_eq!(
            system.display_of(&energy.pow(&BigRational::from_integer((-2).into()))),
            Some(("eV".to_string(), BigInt::from(-2)))
        );
    }

    #[test]
    fn variables_and_user_bases_pass_through() {
        let system = natural();
        let mixed = Dim::var(7).mul(&Dim::base("Length"));
        let reduced = system.reduce(&mixed).unwrap();
        assert!(reduced.dim.vars().contains(&7));
        let user = Dim::base("Credit");
        assert_eq!(system.reduce_dim(&user), user);
        assert!(system.reduce(&user).unwrap().is_identity());
    }
}
