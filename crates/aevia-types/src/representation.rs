//! Finite representation relations shared by checking and actual call proofs.
//!
//! This module is deliberately independent of the inference substitution. A
//! caller maps variables into its own universe, then propagates the four
//! concrete representations without replacing a dependent result by an input.

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

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;

use crate::ty::LiteralKind;
use crate::ty::Rep;
use crate::ty::Type;

/// Numeric requirements transported with a type scheme. Literal constraints
/// also cover type-kinded parameters that have not yet been forced to Scalar.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
    bincode_next::Fingerprint,
)]
pub enum NumericConstraint {
    /// A literal requires a compatible scalar type.
    Literal {
        /// Type inferred for the literal.
        ty: Type,
        /// Literal syntax determines the admissible representations.
        kind: LiteralKind,
    },
    /// A private literal reading computed after its independent source
    /// parameters are selected, rather than an arbitrary result type choice.
    DefaultLiteral {
        /// Type inferred for the literal.
        ty: Type,
        /// Literal syntax determines the admissible representations.
        kind: LiteralKind,
        /// Representations whose selections determine the default.
        inputs: Vec<Rep>,
    },
    /// A relation between numeric representations.
    Relation(RepRelation),
}

impl NumericConstraint {
    /// Visits only the variables that connect numeric decisions.
    pub(crate) fn visit_numeric_variables(
        &self,
        mut visit: impl FnMut(u32),
    ) {
        match self {
            | Self::Literal { ty, .. } => ty.visit_numeric_variables(visit),
            | Self::DefaultLiteral { ty, inputs, .. } => {
                ty.visit_numeric_variables(&mut visit);
                for rep in inputs {
                    if let Rep::Var(id) = rep {
                        visit(*id);
                    }
                }
            },
            | Self::Relation(relation) => {
                for rep in relation.operands().into_iter().flatten() {
                    if let Rep::Var(id) = rep {
                        visit(id);
                    }
                }
            },
        }
    }

    /// Returns the types and representation operands carrying free variables.
    #[must_use]
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
                    .flatten()
                    .map(|rep| Type::Scalar(rep, aevia_dim::Dim::one()))
                    .collect()
            },
        }
    }

    /// Maps the constraint into another variable universe.
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
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    bincode_next::Encode,
    bincode_next::Fingerprint,
)]
pub struct RepDomain(u8);

impl<'de> serde::Deserialize<'de> for RepDomain {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename = "RepDomain")]
        struct Bits(u8);
        let bits = Bits::deserialize(decoder)?.0;
        Self::from_bits(bits)
            .ok_or_else(|| serde::de::Error::custom("invalid representation domain"))
    }
}

impl<Context> bincode_next::Decode<Context> for RepDomain {
    fn decode<D: bincode_next::de::Decoder<Context = Context>>(
        decoder: &mut D
    ) -> Result<Self, bincode_next::error::DecodeError> {
        Self::from_bits(u8::decode(decoder)?).ok_or(bincode_next::error::DecodeError::Other(
            "invalid representation domain",
        ))
    }
}

bincode_next::impl_borrow_decode!(RepDomain);

impl RepDomain {
    /// A domain accepting every concrete representation.
    pub const ALL: Self = Self(0b1111);
    /// Concrete representations in domain-bit order.
    pub const CONCRETE: [Rep; 4] = [Rep::Int, Rep::Float, Rep::Real, Rep::Complex];
    /// A domain accepting no concrete representations.
    pub const EMPTY: Self = Self(0);

    const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !Self::ALL.0 == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    /// A domain containing one concrete representation. A variable yields an empty domain.
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

    /// Builds a domain from concrete representations, skipping variables.
    #[must_use]
    pub fn from_reps(reps: impl IntoIterator<Item = Rep>) -> Self {
        Self(
            reps.into_iter()
                .fold(0, |mask, rep| mask | Self::singleton(rep).0),
        )
    }

    /// Checks whether the domain accepts a concrete representation.
    #[must_use]
    pub const fn contains(
        self,
        rep: Rep,
    ) -> bool {
        self.0 & Self::singleton(rep).0 != 0
    }

    /// Keeps representations accepted by both domains.
    #[must_use]
    pub const fn intersection(
        self,
        other: Self,
    ) -> Self {
        Self(self.0 & other.0)
    }

    /// Checks whether the domain accepts no concrete representation.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns the only concrete representation when the domain is a singleton.
    #[must_use]
    pub fn unique(self) -> Option<Rep> {
        self.0
            .is_power_of_two()
            .then(|| self.iter().next())
            .flatten()
    }

    /// Iterates over the accepted concrete representations.
    pub fn iter(self) -> impl Iterator<Item = Rep> {
        Self::CONCRETE
            .into_iter()
            .filter(move |rep| self.contains(*rep))
    }
}

/// A relation retained with a generic numeric signature.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
    bincode_next::Fingerprint,
)]
pub enum RepRelation {
    /// Restricts one representation to a finite domain.
    Domain {
        /// Operand being restricted.
        rep: Rep,
        /// Admissible concrete representations.
        allowed: RepDomain,
    },
    /// A representation can express another without a narrower numeric domain.
    AtLeast {
        /// Required upper representation.
        upper: Rep,
        /// Representation supplying the lower bound.
        lower: Rep,
    },
    /// Result of combining representations through their numeric join.
    Join {
        /// Left input to the join.
        left: Rep,
        /// Right input to the join.
        right: Rep,
        /// Required joined representation.
        result: Rep,
    },
    /// Division promotes integral operations to an exact real result.
    Divide {
        /// Representation before division.
        input: Rep,
        /// Result after division.
        result: Rep,
    },
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

    const fn operands(self) -> [Option<Rep>; 3] {
        match self {
            | Self::Domain { rep, .. } => [Some(rep), None, None],
            | Self::AtLeast { upper, lower } => [Some(upper), Some(lower), None],
            | Self::Join { left, right, result } => [Some(left), Some(right), Some(result)],
            | Self::Divide { input, result } => [Some(input), Some(result), None],
        }
    }

    fn accepts(
        self,
        [first, second, third]: [Rep; 3],
    ) -> bool {
        match self {
            | Self::Domain { allowed, .. } => allowed.contains(first),
            | Self::AtLeast { .. } => first.at_least(second),
            | Self::Join { .. } => first.join_concrete(second) == Some(third),
            | Self::Divide { .. } => {
                let result = if first == Rep::Int {
                    Rep::Real
                } else {
                    first
                };
                result == second
            },
        }
    }
}

/// An impossible representation relation, retained for a useful diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepConflict {
    /// First relation that lost all of its assignments.
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
    /// Returns the current domain of an operand.
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

    /// Iterates over tracked variables in identifier order.
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
///
/// # Errors
///
/// Returns the first relation with no remaining supported assignment.
pub fn propagate(
    relations: &[RepRelation],
    mut resolve: impl FnMut(Rep) -> Rep,
) -> Result<RepDomains, RepConflict> {
    const MIN_INDEXED_RELATIONS: usize = 64;
    const INITIAL_SWEEPS: usize = 4;
    let relations: Vec<_> = relations
        .iter()
        .map(|relation| relation.map(&mut resolve))
        .collect();
    let mut domains = RepDomains::default();
    for relation in &relations {
        for operand in relation.operands().into_iter().flatten() {
            if let Rep::Var(id) = operand {
                domains.variables.entry(id).or_insert(RepDomain::ALL);
            }
        }
    }
    let mut sweeps = 0usize;
    loop {
        let mut changed = false;
        for relation in &relations {
            refine_relation(*relation, &mut domains, |_| changed = true)?;
        }
        if !changed {
            return Ok(domains);
        }
        sweeps = sweeps.saturating_add(1);
        if sweeps == INITIAL_SWEEPS && relations.len() >= MIN_INDEXED_RELATIONS {
            propagate_changed(&relations, &mut domains)?;
            return Ok(domains);
        }
    }
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "schedule relations that another relation has narrowed"
    )
)]
fn propagate_changed(
    relations: &[RepRelation],
    domains: &mut RepDomains,
) -> Result<(), RepConflict> {
    let mut affected = BTreeMap::<u32, Vec<usize>>::new();
    for (index, relation) in relations.iter().enumerate() {
        for operand in relation.operands().into_iter().flatten() {
            if let Rep::Var(id) = operand {
                let watchers = affected.entry(id).or_default();
                if watchers.last() != Some(&index) {
                    watchers.push(index);
                }
            }
        }
    }

    let mut pending = (0..relations.len()).collect::<BTreeSet<_>>();
    let mut deferred = BTreeSet::new();
    while !pending.is_empty() {
        while let Some(index) = pending.pop_first() {
            if let Some(&relation) = relations.get(index) {
                refine_relation(relation, domains, |id| {
                    if let Some(watchers) = affected.get(&id) {
                        for &watcher in watchers {
                            // Keep sweep order so the first reported conflict is stable.
                            if watcher <= index {
                                deferred.insert(watcher);
                            } else {
                                pending.insert(watcher);
                            }
                        }
                    }
                })?;
            }
        }
        std::mem::swap(&mut pending, &mut deferred);
    }
    Ok(())
}

fn refine_relation(
    relation: RepRelation,
    domains: &mut RepDomains,
    mut changed: impl FnMut(u32),
) -> Result<(), RepConflict> {
    let operands = relation.operands();
    let masks =
        operands.map(|rep| rep.map_or(RepDomain::singleton(Rep::Int), |rep| domains.domain(rep)));
    let supported = supported_values(relation, operands, masks);
    if supported.iter().any(|domain| domain.is_empty()) {
        return Err(RepConflict { relation });
    }
    for (operand, support) in operands.into_iter().zip(supported) {
        if let Some(Rep::Var(id)) = operand {
            let domain = domains.variables.entry(id).or_insert(RepDomain::ALL);
            let next = domain.intersection(support);
            if next != *domain {
                *domain = next;
                changed(id);
            }
        }
    }
    Ok(())
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "evaluate support for one representation relation"
    )
)]
fn supported_values(
    relation: RepRelation,
    [first, second, third]: [Option<Rep>; 3],
    [first_choices, second_choices, third_choices]: [RepDomain; 3],
) -> [RepDomain; 3] {
    let mut support = [RepDomain::EMPTY; 3];
    let agrees =
        |a: Option<Rep>, b: Option<Rep>, value_a: Rep, value_b: Rep| a != b || value_a == value_b;
    // Padding uses a singleton, and the actual relation has at most three slots.
    for a in first_choices.iter() {
        for b in second_choices.iter() {
            if !agrees(first, second, a, b) {
                continue;
            }
            for c in third_choices.iter() {
                if agrees(first, third, a, c)
                    && agrees(second, third, b, c)
                    && relation.accepts([a, b, c])
                {
                    for (domain, rep) in support.iter_mut().zip([a, b, c]) {
                        domain.0 |= RepDomain::singleton(rep).0;
                    }
                }
            }
        }
    }
    support
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_sweeps(relations: &[RepRelation]) -> (Result<RepDomains, RepConflict>, usize) {
        let mut domains = RepDomains::default();
        let mut sweeps = 0;
        loop {
            sweeps += 1;
            let mut changed = false;
            for relation in relations {
                let operands = relation.operands();
                let masks = operands.map(|rep| {
                    rep.map_or(RepDomain::singleton(Rep::Int), |rep| domains.domain(rep))
                });
                let support = supported_values(*relation, operands, masks);
                if support.iter().any(|domain| domain.is_empty()) {
                    return (Err(RepConflict { relation: *relation }), sweeps);
                }
                for (operand, support) in operands.into_iter().zip(support) {
                    if let Some(Rep::Var(id)) = operand {
                        let domain = domains.variables.entry(id).or_insert(RepDomain::ALL);
                        let next = domain.intersection(support);
                        changed |= next != *domain;
                        *domain = next;
                    }
                }
            }
            if !changed {
                return (Ok(domains), sweeps);
            }
        }
    }

    #[test]
    fn scheduled_fixpoint_preserves_domains_and_the_first_delayed_conflict() {
        let width = 72;
        let other = 1_000;
        let independent = 2_000;
        let mut relations = Vec::new();
        // Both seeds travel backwards, despite opposite orderings of their
        // operands. Repeated positions in a third relation share one variable.
        for i in 0..width {
            relations.push(RepRelation::AtLeast {
                upper: Rep::Var(i),
                lower: Rep::Var(i + 1),
            });
            relations.push(RepRelation::AtLeast {
                upper: Rep::Var(other + i + 1),
                lower: Rep::Var(other + i),
            });
        }
        relations.extend([
            RepRelation::Domain {
                rep: Rep::Var(width),
                allowed: RepDomain::singleton(Rep::Complex),
            },
            RepRelation::Domain {
                rep: Rep::Var(other + width),
                allowed: RepDomain::singleton(Rep::Int),
            },
            RepRelation::Join {
                left: Rep::Var(independent),
                right: Rep::Real,
                result: Rep::Var(independent),
            },
        ]);
        let (expected, sweeps) = full_sweeps(&relations);
        assert!(sweeps > 4);
        let actual = propagate(&relations, |rep| rep);
        assert_eq!(actual, expected);
        let domains = actual.unwrap();
        assert_eq!(domains.domain(Rep::Var(0)).unique(), Some(Rep::Complex));
        assert_eq!(domains.domain(Rep::Var(other)).unique(), Some(Rep::Int));
        assert_eq!(
            domains.domain(Rep::Var(independent)),
            RepDomain::from_reps([Rep::Real, Rep::Complex])
        );

        let left = RepRelation::AtLeast {
            upper: Rep::Var(other),
            lower: Rep::Var(0),
        };
        let right = RepRelation::AtLeast {
            upper: Rep::Var(other + 1),
            lower: Rep::Var(1),
        };
        for tail in [[left, right], [right, left]] {
            let mut conflict = relations.clone();
            conflict.extend(tail);
            let (expected, sweeps) = full_sweeps(&conflict);
            assert!(
                expected.is_err() && sweeps > 4,
                "{expected:?} after {sweeps}"
            );
            assert_eq!(propagate(&conflict, |rep| rep), expected);
        }
    }

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

    #[test]
    fn representation_domain_decoder_rejects_unknown_bits() {
        let config = bincode_next::config::standard();
        let bytes = bincode_next::encode_to_vec(RepDomain::ALL, config).expect("encode domain");
        assert_eq!(bytes, [0b1111]);
        assert_eq!(
            bincode_next::decode_from_slice::<RepDomain, _>(&bytes, config)
                .expect("decode domain")
                .0,
            RepDomain::ALL,
        );
        assert!(bincode_next::decode_from_slice::<RepDomain, _>(&[0b1_0000], config).is_err());
    }
}
