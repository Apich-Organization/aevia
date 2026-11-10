//! Finite representation relations shared by checking and actual call proofs.
//!
//! This module is deliberately independent of the inference substitution. A
//! caller maps variables into its own universe, then propagates the four
//! concrete representations without replacing a dependent result by an input.

use std::collections::BTreeMap;
use std::fmt;

use crate::ty::LiteralKind;
use crate::ty::Rep;
use crate::ty::Type;

/// Numeric requirements transported with a type scheme. Literal constraints
/// also cover type-kinded parameters that have not yet been forced to Scalar.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NumericConstraint {
    Literal {
        ty: Type,
        kind: LiteralKind,
    },
    /// A private literal reading computed after its independent source
    /// parameters are selected, rather than an arbitrary result type choice.
    DefaultLiteral {
        ty: Type,
        kind: LiteralKind,
        inputs: Vec<Rep>,
    },
    Relation(RepRelation),
}

impl NumericConstraint {
    pub fn types(&self) -> Vec<Type> {
        match self {
            | Self::Literal { ty, .. } => vec![ty.clone()],
            | Self::DefaultLiteral { ty, inputs, .. } => {
                std::iter::once(ty.clone())
                    .chain(
                        inputs
                            .iter()
                            .map(|rep| Type::Scalar(*rep, aevia_dim::Dim::one())),
                    )
                    .collect()
            },
            | Self::Relation(relation) => {
                relation
                    .operands()
                    .into_iter()
                    .map(|rep| Type::Scalar(rep, aevia_dim::Dim::one()))
                    .collect()
            },
        }
    }

    #[must_use]
    pub fn substitute(
        &self,
        substitution: &crate::TypeSubstitution,
    ) -> Self {
        match self {
            | Self::Literal { ty, kind } => {
                Self::Literal {
                    ty: substitution.apply(ty),
                    kind: *kind,
                }
            },
            | Self::Relation(relation) => Self::Relation(relation.map(|rep| substitution.rep(rep))),
            | Self::DefaultLiteral { ty, kind, inputs } => {
                Self::DefaultLiteral {
                    ty: substitution.apply(ty),
                    kind: *kind,
                    inputs: inputs.iter().map(|rep| substitution.rep(*rep)).collect(),
                }
            },
        }
    }
}

/// A finite subset of the concrete numeric representations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RepDomain(u8);

impl RepDomain {
    pub const ALL: Self = Self(0b1111);
    pub const CONCRETE: [Rep; 4] = [Rep::Int, Rep::Float, Rep::Real, Rep::Complex];
    pub const EMPTY: Self = Self(0);

    #[must_use]
    pub const fn singleton(rep: Rep) -> Self {
        match rep {
            | Rep::Int => Self(1),
            | Rep::Float => Self(2),
            | Rep::Real => Self(4),
            | Rep::Complex => Self(8),
            | Rep::Var(_) => Self::EMPTY,
        }
    }

    #[must_use]
    pub fn from_reps(reps: impl IntoIterator<Item = Rep>) -> Self {
        Self(
            reps.into_iter()
                .fold(0, |mask, rep| mask | Self::singleton(rep).0),
        )
    }

    #[must_use]
    pub const fn contains(
        self,
        rep: Rep,
    ) -> bool {
        self.0 & Self::singleton(rep).0 != 0
    }

    #[must_use]
    pub const fn intersection(
        self,
        other: Self,
    ) -> Self {
        Self(self.0 & other.0)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub fn unique(self) -> Option<Rep> {
        (self.0.count_ones() == 1)
            .then(|| Self::CONCRETE.into_iter().find(|rep| self.contains(*rep)))
            .flatten()
    }

    pub fn iter(self) -> impl Iterator<Item = Rep> {
        Self::CONCRETE
            .into_iter()
            .filter(move |rep| self.contains(*rep))
    }
}

/// A relation retained with a generic numeric signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RepRelation {
    Domain { rep: Rep, allowed: RepDomain },
    AtLeast { upper: Rep, lower: Rep },
    Join { left: Rep, right: Rep, result: Rep },
    Divide { input: Rep, result: Rep },
}

impl RepRelation {
    /// Remap one variable universe into another exactly once.
    #[must_use]
    pub fn map(
        self,
        mut apply: impl FnMut(Rep) -> Rep,
    ) -> Self {
        match self {
            | Self::Domain { rep, allowed } => {
                Self::Domain {
                    rep: apply(rep),
                    allowed,
                }
            },
            | Self::AtLeast { upper, lower } => {
                Self::AtLeast {
                    upper: apply(upper),
                    lower: apply(lower),
                }
            },
            | Self::Join { left, right, result } => {
                Self::Join {
                    left: apply(left),
                    right: apply(right),
                    result: apply(result),
                }
            },
            | Self::Divide { input, result } => {
                Self::Divide {
                    input: apply(input),
                    result: apply(result),
                }
            },
        }
    }

    fn operands(self) -> Vec<Rep> {
        match self {
            | Self::Domain { rep, .. } => vec![rep],
            | Self::AtLeast { upper, lower } => vec![upper, lower],
            | Self::Join { left, right, result } => vec![left, right, result],
            | Self::Divide { input, result } => vec![input, result],
        }
    }

    fn accepts(
        self,
        values: &[Rep],
    ) -> bool {
        match self {
            | Self::Domain { allowed, .. } => allowed.contains(values[0]),
            | Self::AtLeast { .. } => values[0].at_least(values[1]),
            | Self::Join { .. } => values[0].join_concrete(values[1]) == Some(values[2]),
            | Self::Divide { .. } => {
                let result = if values[0] == Rep::Int {
                    Rep::Real
                } else {
                    values[0]
                };
                result == values[1]
            },
        }
    }
}

/// An impossible representation relation, retained for a useful diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepConflict {
    pub relation: RepRelation,
}

impl fmt::Display for RepConflict {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        write!(
            formatter,
            "numeric representations cannot satisfy {:?}",
            self.relation
        )
    }
}

impl std::error::Error for RepConflict {}

/// Remaining domains after propagation. A non-singleton domain is still a
/// generic variable; propagation never chooses an arbitrary default reading.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepDomains {
    variables: BTreeMap<u32, RepDomain>,
}

impl RepDomains {
    #[must_use]
    pub fn domain(
        &self,
        rep: Rep,
    ) -> RepDomain {
        match rep {
            | Rep::Var(id) => self.variables.get(&id).copied().unwrap_or(RepDomain::ALL),
            | concrete => RepDomain::singleton(concrete),
        }
    }

    pub fn variables(&self) -> impl Iterator<Item = (u32, RepDomain)> + '_ {
        self.variables.iter().map(|(id, domain)| (*id, *domain))
    }
}

/// Propagate finite relations after the caller's current substitution.
///
/// Repeated occurrences of a variable are one assignment: e.g.
/// Join(r, Float, r) excludes Int instead of independently satisfying the
/// input and output positions. No pointer identity or global type ids enter
/// the result.
pub fn propagate(
    relations: &[RepRelation],
    mut resolve: impl FnMut(Rep) -> Rep,
) -> Result<RepDomains, RepConflict> {
    let relations: Vec<_> = relations
        .iter()
        .map(|relation| relation.map(&mut resolve))
        .collect();
    let mut domains = RepDomains::default();
    for relation in &relations {
        for operand in relation.operands() {
            if let Rep::Var(id) = operand {
                domains.variables.entry(id).or_insert(RepDomain::ALL);
            }
        }
    }
    loop {
        let mut changed = false;
        for relation in &relations {
            let operands = relation.operands();
            let choices: Vec<Vec<Rep>> = operands
                .iter()
                .map(|rep| domains.domain(*rep).iter().collect())
                .collect();
            let mut supported = BTreeMap::<u32, RepDomain>::new();
            let mut accepted = false;
            // Relations have at most three operands, so this enumerates at
            // most 64 tuples, independently of the number of type variables.
            for code in 0..4usize.pow(operands.len() as u32) {
                let mut code = code;
                let mut values = Vec::with_capacity(operands.len());
                let mut assignments = BTreeMap::new();
                let mut valid = true;
                for (operand, choices) in operands.iter().zip(&choices) {
                    let index = code % 4;
                    code /= 4;
                    let Some(value) = choices.get(index).copied() else {
                        valid = false;
                        break;
                    };
                    if let Rep::Var(id) = operand
                        && assignments
                            .insert(*id, value)
                            .is_some_and(|previous| previous != value)
                    {
                        valid = false;
                        break;
                    }
                    values.push(value);
                }
                if !valid || !relation.accepts(&values) {
                    continue;
                }
                accepted = true;
                for (id, value) in assignments {
                    let support = supported.entry(id).or_insert(RepDomain::EMPTY);
                    support.0 |= RepDomain::singleton(value).0;
                }
            }
            if !accepted {
                return Err(RepConflict { relation: *relation });
            }
            for (id, support) in supported {
                let old = domains.variables[&id];
                let next = old.intersection(support);
                if next.is_empty() {
                    return Err(RepConflict { relation: *relation });
                }
                if next != old {
                    domains.variables.insert(id, next);
                    changed = true;
                }
            }
        }
        if !changed {
            return Ok(domains);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_follow_the_existing_exact_and_complex_paths() {
        for left in RepDomain::CONCRETE {
            for right in RepDomain::CONCRETE {
                let relation = RepRelation::Join {
                    left,
                    right,
                    result: Rep::Var(0),
                };
                let domains = propagate(&[relation], |rep| rep).unwrap();
                assert_eq!(
                    domains.domain(Rep::Var(0)).unique(),
                    left.join_concrete(right)
                );
            }
        }
    }

    #[test]
    fn division_preserves_nonintegral_inputs_and_promotes_int() {
        for input in RepDomain::CONCRETE {
            let domains = propagate(
                &[RepRelation::Divide {
                    input,
                    result: Rep::Var(0),
                }],
                |rep| rep,
            )
            .unwrap();
            assert_eq!(
                domains.domain(Rep::Var(0)).unique(),
                Some(if input == Rep::Int {
                    Rep::Real
                } else {
                    input
                })
            );
        }
        assert!(
            propagate(
                &[RepRelation::Divide {
                    input: Rep::Int,
                    result: Rep::Int
                }],
                |rep| rep
            )
            .is_err()
        );
    }

    #[test]
    fn repeated_positions_are_correlated_instead_of_freshened_apart() {
        let relations = [
            RepRelation::Join {
                left: Rep::Var(0),
                right: Rep::Float,
                result: Rep::Var(0),
            },
            RepRelation::Join {
                left: Rep::Var(0),
                right: Rep::Real,
                result: Rep::Var(0),
            },
        ];
        let domains = propagate(&relations, |rep| rep).unwrap();
        assert_eq!(
            domains.domain(Rep::Var(0)),
            RepDomain::from_reps([Rep::Real, Rep::Complex])
        );
    }

    #[test]
    fn dependent_results_remain_generic_until_inputs_are_selected() {
        let relation = RepRelation::Divide {
            input: Rep::Var(0),
            result: Rep::Var(1),
        };
        let generic = propagate(&[relation], |rep| rep).unwrap();
        assert_eq!(generic.domain(Rep::Var(0)), RepDomain::ALL);
        assert_eq!(
            generic.domain(Rep::Var(1)),
            RepDomain::from_reps([Rep::Real, Rep::Float, Rep::Complex])
        );
        let selected = propagate(&[relation], |rep| {
            if rep == Rep::Var(0) {
                Rep::Int
            } else {
                rep
            }
        })
        .unwrap();
        assert_eq!(selected.domain(Rep::Var(1)).unique(), Some(Rep::Real));
    }

    #[test]
    fn decimal_domains_reject_int_and_lattice_bounds_can_force_complex() {
        let decimals = RepDomain::from_reps([Rep::Real, Rep::Float]);
        assert!(
            propagate(
                &[RepRelation::Domain {
                    rep: Rep::Int,
                    allowed: decimals
                }],
                |rep| rep
            )
            .is_err()
        );
        let domains = propagate(
            &[
                RepRelation::AtLeast {
                    upper: Rep::Var(0),
                    lower: Rep::Real,
                },
                RepRelation::AtLeast {
                    upper: Rep::Var(0),
                    lower: Rep::Float,
                },
            ],
            |rep| rep,
        )
        .unwrap();
        assert_eq!(domains.domain(Rep::Var(0)).unique(), Some(Rep::Complex));
    }
}
