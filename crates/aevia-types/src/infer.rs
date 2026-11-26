//! Unification, instantiation, generalisation and defaulting.
//!
//! One engine solves types, dimensions, record rows and tensor extents. Record
//! fields can contain dimensioned scalars or tensors, and these constraints are
//! resolved along with the field types. Dimensional equations delegate to
//! [`aevia_dim::DimUnifier`], which implements the abelian-group unification of
//! §4.3. Deferred extent equations are retried when their variables are bound.
//!
//! Generalisation uses levels rather than a free-variable scan over the
//! environment: a variable created at level `l` may be quantified when the
//! binding sits at a level lower than `l`. This is the standard efficient
//! formulation and keeps `let` inference linear in practice.

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

use crate::representation::NumericConstraint;
use crate::representation::RepDomain;
use crate::representation::RepRelation;
use crate::ty::Constraint;
use crate::ty::LiteralKind;
use crate::ty::Nat;
use crate::ty::Rep;
use crate::ty::Row;
use crate::ty::Scheme;
use crate::ty::Shape;
use crate::ty::Type;
use aevia_dim::Dim;
use aevia_dim::DimMismatch;
use aevia_dim::DimUnifier;
use indexmap::IndexSet;
use num_bigint::BigInt;
use num_bigint::Sign;
use num_traits::One;
use num_traits::ToPrimitive;
use num_traits::Zero;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

/// Why two types could not be made equal.
#[derive(Clone, Debug)]
pub enum UnifyError {
    /// The constructors differ.
    Mismatch {
        /// The left type.
        left: Type,
        /// The right type.
        right: Type,
    },
    /// The dimensions differ.
    Dim(DimMismatch),
    /// A type variable would contain itself.
    Occurs(u32),
    /// A record is missing a field.
    MissingField {
        /// The field name.
        field: String,
        /// The row that lacks it.
        row: Row,
    },
    /// A row would gain a label it already has (`E0208`).
    DuplicateField {
        /// The label that would appear twice.
        field: String,
    },
    /// Two shapes are incompatible.
    ShapeMismatch {
        /// The left shape.
        left: Shape,
        /// The right shape.
        right: Shape,
    },
    /// A literal cannot take the requested representation.
    LiteralMismatch {
        /// The literal kind.
        kind: LiteralKind,
        /// The requested type.
        requested: Type,
    },
    /// Numeric representation domains or operation transfers conflict.
    Numeric(String),
    /// Two type-level `Nat` terms are inconsistent (§3.5.1): their difference
    /// has no non-negative integer solution.
    NatMismatch {
        /// The left term, as the reader wrote it.
        left: String,
        /// The right term, as the reader wrote it.
        right: String,
    },
    /// Two declared type-level variables (`fn f<n, m>`) were equated; they are
    /// distinct unknowns, not known to be equal.
    NatRigid {
        /// The first variable's name.
        left: String,
        /// The second variable's name.
        right: String,
    },
    /// A type-level `Nat` equation is not linear — a product of two variables.
    /// §3.5.1 only offers linear equations, so the checker reports `E0304`.
    NatNonLinear {
        /// The left term.
        left: Nat,
        /// The right term.
        right: Nat,
    },
}

impl std::fmt::Display for UnifyError {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            | Self::Mismatch { left, right } => write!(f, "expected {left}, found {right}"),
            | Self::Dim(mismatch) => {
                write!(
                    f,
                    "dimension mismatch: expected {}, found {}",
                    mismatch.right.to_plain_string(),
                    mismatch.left.to_plain_string()
                )
            },
            | Self::Numeric(message) => f.write_str(message),
            | Self::Occurs(id) => write!(f, "infinite type: t{id} occurs in its own definition"),
            | Self::MissingField { field, row } => {
                write!(f, "the record {{ {row} }} has no field `{field}`")
            },
            | Self::DuplicateField { field } => {
                write!(f, "the record would have two fields named `{field}`")
            },
            | Self::ShapeMismatch { left, right } => {
                write!(f, "shape mismatch: expected {left}, found {right}")
            },
            | Self::LiteralMismatch { kind, requested } => {
                match kind {
                    | LiteralKind::Int | LiteralKind::Decimal => {
                        write!(
                            f,
                            "a bare numeric literal cannot have type {requested}; write a unit (for example `1 m`)"
                        )
                    },
                    | _ => write!(f, "the literal cannot have type {requested}"),
                }
            },
            | Self::NatMismatch { left, right } => {
                write!(f, "type-level equation has no solution: {left} = {right}")
            },
            | Self::NatRigid { left, right } => {
                write!(
                    f,
                    "extents `{left}` and `{right}` are different type-level variables and are not known to be equal"
                )
            },
            | Self::NatNonLinear { left, right } => {
                write!(
                    f,
                    "type-level equation is not linear: {left} = {right}; only literals, `+` and `*` by a literal are supported"
                )
            },
        }
    }
}

/// A polynomial over type-level `Nat` variables: `Σ cₘ·m`, where each monomial
/// `m` is a sorted multiset of variables (the empty monomial is the constant).
///
/// Coefficients are exact even when a product of machine-sized extents exceeds
/// the range of any machine integer.
///
/// A *linear* equation (only degree-1 monomials) isolates a variable. A product
/// of two variables (`n * m`) is kept as an opaque monomial: it cancels against
/// an identical monomial in any order (`n * m ~ m * n`), and it is substituted
/// when one factor is later solved (`n = 2` turns `n * m` into `2 * m`), but it
/// is never solved for by itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NatLinear {
    terms: BTreeMap<Vec<u32>, BigInt>,
}

impl NatLinear {
    fn constant(value: BigInt) -> Self {
        let mut terms = BTreeMap::new();
        terms.insert(Vec::new(), value);
        Self { terms }.normalised()
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "arbitrary-precision coefficients cannot overflow"
    )]
    fn add(
        mut self,
        other: Self,
    ) -> Self {
        for (monomial, coefficient) in other.terms {
            *self.terms.entry(monomial).or_default() += coefficient;
        }
        self.normalised()
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "arbitrary-precision coefficients cannot overflow"
    )]
    fn sub(
        mut self,
        other: Self,
    ) -> Self {
        for (monomial, coefficient) in other.terms {
            *self.terms.entry(monomial).or_default() -= coefficient;
        }
        self.normalised()
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "arbitrary-precision coefficients cannot overflow"
    )]
    fn mul(
        &self,
        other: &Self,
    ) -> Self {
        let mut out: BTreeMap<Vec<u32>, BigInt> = BTreeMap::new();
        for (left, a) in &self.terms {
            for (right, b) in &other.terms {
                let mut monomial = left.clone();
                monomial.extend(right.iter().copied());
                monomial.sort_unstable();
                *out.entry(monomial).or_default() += a * b;
            }
        }
        Self { terms: out }.normalised()
    }

    fn normalised(mut self) -> Self {
        self.terms.retain(|_, coefficient| !coefficient.is_zero());
        self
    }

    /// The constant term.
    fn constant_part(&self) -> Option<&BigInt> {
        self.terms.get([].as_slice())
    }

    /// Whether there is no variable left.
    fn is_constant(&self) -> bool {
        self.terms.keys().all(Vec::is_empty)
    }

    /// Whether every monomial has degree at most one.
    fn is_linear(&self) -> bool {
        self.terms.keys().all(|monomial| monomial.len() <= 1)
    }

    /// The degree-one terms: `(variable, coefficient)`.
    fn linear_terms(&self) -> Vec<(u32, &BigInt)> {
        self.terms
            .iter()
            .filter_map(|(monomial, coefficient)| {
                let [id] = monomial.as_slice() else {
                    return None;
                };
                Some((*id, coefficient))
            })
            .collect()
    }

    /// Whether `id` occurs in a monomial of degree two or more.
    fn in_product(
        &self,
        id: u32,
    ) -> bool {
        self.terms
            .keys()
            .any(|monomial| monomial.len() > 1 && monomial.contains(&id))
    }

    /// Every variable of the polynomial.
    fn variables(&self) -> Vec<u32> {
        let mut out: Vec<u32> = self.terms.keys().flatten().copied().collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// Solve `coefficient * variable + constant = 0` when division is exact.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "BigInt division is guarded against zero"
)]
fn exact_nat_quotient(
    constant: &BigInt,
    coefficient: &BigInt,
) -> Option<BigInt> {
    (!coefficient.is_zero() && (constant % coefficient).is_zero())
        .then(|| -(constant / coefficient))
}

/// What solving one Nat equation did.
enum NatStep {
    /// The equation held or established a binding.
    Consumed,
    /// Nothing could be solved yet.
    Residual(NatLinear),
}

/// A type-level equation left unsolved at the end of a declaration.
#[derive(Clone, Debug)]
pub struct UnsolvedNat {
    /// `lhs = rhs`, with the variables' source names.
    pub text: String,
    /// Whether the equation is linear (otherwise it has a product of unknowns).
    pub linear: bool,
}

/// The inference engine state for one module.
#[derive(Clone, Debug)]
pub struct Infer {
    subst: HashMap<u32, Arc<Type>>,
    unified_vars: HashSet<(u32, u32)>,
    rep_subst: HashMap<u32, Rep>,
    dims: DimUnifier,
    row_subst: HashMap<u32, Row>,
    levels: HashMap<u32, u32>,
    rep_levels: HashMap<u32, u32>,
    dim_levels: HashMap<u32, u32>,
    row_levels: HashMap<u32, u32>,
    literals: HashMap<u32, LiteralKind>,
    rep_literals: HashMap<u32, LiteralKind>,
    /// Variables that must never be generalised (§3.3.3).
    ///
    /// One set for all four kinds: every id comes from the same counter, so
    /// the ids are disjoint.
    weak: HashSet<u32>,
    /// `ρ ⊒ σ` lower bounds recorded by `join` (§3.4.1).
    rep_lower: HashMap<u32, Vec<Rep>>,
    numeric: crate::numeric_index::NumericIndex<NumericConstraint>,
    division_inputs: HashMap<u32, Rep>,
    join_inputs: HashMap<u32, (Rep, Rep)>,
    #[cfg(test)]
    numeric_peak: usize,
    #[cfg(test)]
    numeric_work: std::cell::Cell<usize>,
    composite_literals: HashSet<u32>,
    numeric_universals: HashSet<u32>,
    numeric_errors: Vec<String>,
    /// `ρ lacks l` side conditions on row variables (§3.6.1).
    lacks: HashMap<u32, IndexSet<String>>,
    /// Type-level natural-number substitution (§3.5.1).
    nat_subst: HashMap<u32, Nat>,
    /// Level of each type-level natural-number variable, for generalisation.
    nat_levels: HashMap<u32, u32>,
    /// Equations that cannot yet be solved, indexed by their current variables.
    nat_pending: crate::numeric_index::NumericIndex<NatLinear>,
    #[cfg(test)]
    nat_retries: usize,
    /// Source names of type-level variables, for diagnostics.
    nat_names: HashMap<u32, String>,
    /// Declared (`fn f<n>`) type-level variables: never merged with each other.
    nat_rigid: HashSet<u32>,
    next: u32,
    level: u32,
}

impl Default for Infer {
    fn default() -> Self {
        Self::new()
    }
}

impl Infer {
    /// A fresh engine at level zero.
    #[must_use]
    pub fn new() -> Self {
        Self {
            subst: HashMap::new(),
            unified_vars: HashSet::new(),
            rep_subst: HashMap::new(),
            dims: DimUnifier::new(),
            row_subst: HashMap::new(),
            levels: HashMap::new(),
            rep_levels: HashMap::new(),
            dim_levels: HashMap::new(),
            row_levels: HashMap::new(),
            literals: HashMap::new(),
            rep_literals: HashMap::new(),
            weak: HashSet::new(),
            rep_lower: HashMap::new(),
            numeric: crate::numeric_index::NumericIndex::default(),
            division_inputs: HashMap::new(),
            join_inputs: HashMap::new(),
            #[cfg(test)]
            numeric_peak: 0,
            #[cfg(test)]
            numeric_work: std::cell::Cell::new(0),
            composite_literals: HashSet::new(),
            numeric_universals: HashSet::new(),
            numeric_errors: Vec::new(),
            lacks: HashMap::new(),
            nat_subst: HashMap::new(),
            nat_levels: HashMap::new(),
            nat_pending: crate::numeric_index::NumericIndex::default(),
            #[cfg(test)]
            nat_retries: 0,
            nat_names: HashMap::new(),
            nat_rigid: HashSet::new(),
            next: 0,
            level: 0,
        }
    }

    #[cfg_attr(
        not(test),
        expect(
            clippy::expect_used,
            reason = "exhaustion cannot produce a fresh 32-bit id"
        )
    )]
    const fn fresh_id(&mut self) -> u32 {
        let id = self.next;
        self.next = id.checked_add(1).expect("type variable id space exhausted");
        id
    }

    /// The current generalisation level.
    #[must_use]
    pub const fn level(&self) -> u32 {
        self.level
    }

    /// Enters a nested binding level.
    pub const fn enter_level(&mut self) {
        self.level = self.level.saturating_add(1);
    }

    /// The level assigned to newly created variables.
    #[must_use]
    pub const fn current_level(&self) -> u32 {
        self.level
    }

    /// Leaves a binding level.
    pub const fn leave_level(&mut self) {
        self.level = self.level.saturating_sub(1);
    }

    /// A fresh type variable at the current level.
    pub fn fresh_type(&mut self) -> Type {
        let id = self.fresh_id();
        self.levels.insert(id, self.level);
        Type::Var(id)
    }

    /// A fresh representation variable.
    pub fn fresh_rep(&mut self) -> Rep {
        let id = self.fresh_id();
        self.rep_levels.insert(id, self.level);
        Rep::Var(id)
    }

    /// A fresh dimension variable.
    pub fn fresh_dim(&mut self) -> Dim {
        let id = self.fresh_id();
        self.dim_levels.insert(id, self.level);
        Dim::var(id)
    }

    /// A fresh row variable.
    pub fn fresh_row(&mut self) -> Row {
        let id = self.fresh_id();
        self.row_levels.insert(id, self.level);
        Row::Var(id)
    }

    /// A fresh *weak* dimension variable (§3.3.3).
    ///
    /// A weak variable is never generalised: every use of the binding that
    /// owns it must agree on one dimension, which is what makes a symbol keep
    /// a single dimension across a program.
    pub fn fresh_weak_dim(&mut self) -> Dim {
        let dim = self.fresh_dim();
        for id in dim.vars() {
            self.weak.insert(id);
        }
        dim
    }

    /// A fresh weak type variable.
    pub fn fresh_weak_type(&mut self) -> Type {
        let ty = self.fresh_type();
        if let Type::Var(id) = ty {
            self.weak.insert(id);
        }
        ty
    }

    /// A fresh weak row variable.
    pub fn fresh_weak_row(&mut self) -> Row {
        let row = self.fresh_row();
        if let Row::Var(id) = row {
            self.weak.insert(id);
        }
        row
    }

    /// A materialized value keeps one schema across all later uses. The
    /// caller supplies lexical universals that remain activation parameters.
    pub fn weaken_materialized(
        &mut self,
        ty: &Type,
        protected: &crate::TypeSubstitution,
    ) {
        let mut free = crate::TypeSubstitution::default();
        free.register_variables(&self.resolve(ty), &mut Self::new());
        for id in free.types.keys().chain(free.representations.keys()) {
            if !protected.types.contains_key(id) && !protected.representations.contains_key(id) {
                self.weak.insert(*id);
            }
        }
        for id in free.dimensions.keys() {
            if !protected.dimensions.contains_key(id) {
                self.weak.insert(*id);
            }
        }
        for id in free.rows.keys() {
            if !protected.rows.contains_key(id) {
                self.weak.insert(*id);
            }
        }
        for id in free.naturals.keys() {
            if !protected.naturals.contains_key(id) {
                self.weak.insert(*id);
            }
        }
    }

    /// Whether a variable is weak.
    #[must_use]
    pub fn is_weak(
        &self,
        id: u32,
    ) -> bool {
        self.weak.contains(&id)
    }

    /// Whether a dimension variable is weak, directly or through a
    /// substitution: `let f = fn t => x * t` makes `t`'s dimension weak too.
    #[must_use]
    pub fn is_weak_dim(
        &self,
        id: u32,
    ) -> bool {
        if self.weak.contains(&id) {
            return true;
        }
        self.weak
            .iter()
            .any(|weak| self.dims.resolve(&Dim::var(*weak)).contains_var(id))
    }

    fn contains_numeric(
        &self,
        constraint: &NumericConstraint,
    ) -> bool {
        let constraint = self.resolve_numeric(constraint);
        let dependencies = Self::numeric_variables(&constraint);
        self.numeric.equivalent(&dependencies, |existing| {
            let existing = self.resolve_numeric(existing);
            match (&existing, &constraint) {
                | (
                    NumericConstraint::Literal {
                        ty: Type::Scalar(a, _),
                        kind: ak,
                    },
                    NumericConstraint::Literal {
                        ty: Type::Scalar(b, _),
                        kind: bk,
                    },
                ) if ak.is_quantity() && ak == bk => a == b,
                | _ => existing == constraint,
            }
        })
    }

    fn cache_numeric_transfer(
        &mut self,
        constraint: &NumericConstraint,
    ) {
        match constraint {
            | NumericConstraint::Relation(RepRelation::Divide {
                input,
                result: Rep::Var(id),
            }) => {
                self.division_inputs.insert(*id, *input);
            },
            | NumericConstraint::Relation(RepRelation::Join {
                left,
                right,
                result: Rep::Var(id),
            }) => {
                self.join_inputs.insert(*id, (*left, *right));
            },
            | _ => {},
        }
    }

    fn push_numeric(
        &mut self,
        constraint: &NumericConstraint,
    ) {
        let constraint = self.resolve_numeric(constraint);
        let dependencies = Self::numeric_variables(&constraint);
        self.cache_numeric_transfer(&constraint);
        self.numeric.insert(constraint, dependencies);
        #[cfg(test)]
        {
            self.numeric_peak = self.numeric_peak.max(self.numeric.len());
        }
    }

    fn numeric_changed(
        &mut self,
        variable: u32,
    ) {
        let affected = self.numeric.affected(variable);
        for id in affected {
            let Some(constraint) = self.numeric.get(id).cloned() else {
                continue;
            };
            let constraint = self.resolve_numeric(&constraint);
            let dependencies = Self::numeric_variables(&constraint);
            // Merging two variables can make constraints identical: the N
            // element literals of a tensor literal all become `Literal { ty:
            // root, kind }`. A conjunction is idempotent, so the duplicate is
            // retired instead of kept; otherwise every later binding on the
            // component re-resolved all N of them (quadratic checking of a
            // long literal: 2000 elements took seconds).
            let duplicate = dependencies.first().is_some_and(|first| {
                self.numeric.affected(*first).into_iter().any(|other| {
                    other != id
                        && self
                            .numeric
                            .get(other)
                            .is_some_and(|existing| self.resolve_numeric(existing) == constraint)
                })
            });
            if duplicate {
                self.numeric.retire(id);
                continue;
            }
            self.cache_numeric_transfer(&constraint);
            self.numeric.replace(id, constraint, dependencies);
        }
    }

    fn numeric_component(
        &self,
        roots: impl IntoIterator<Item = u32>,
    ) -> Vec<NumericConstraint> {
        let component = self.numeric.project(roots);
        #[cfg(test)]
        self.numeric_work
            .set(self.numeric_work.get() + component.len());
        component
            .into_iter()
            .filter_map(|id| self.numeric.get(id))
            .map(|constraint| self.resolve_numeric(constraint))
            .collect()
    }

    /// Records that a type variable came from a literal.
    pub fn mark_literal(
        &mut self,
        ty: &Type,
        kind: LiteralKind,
    ) {
        // Satisfied concrete facts cannot constrain any future variable.
        // Keeping one per visited literal made long concrete arithmetic scan
        // the entire already-checked prefix at every next operation.
        let satisfied = matches!(self.resolve(ty),Type::Scalar(rep,dim) if !matches!(rep,Rep::Var(_)) && kind.accepts_rep(rep) && (kind.is_quantity() || dim.is_one()));
        if !satisfied {
            let constraint = NumericConstraint::Literal { ty: ty.clone(), kind };
            if !self.contains_numeric(&constraint) {
                self.push_numeric(&constraint);
            }
        }
        match ty {
            | Type::Var(id) => {
                self.literals.insert(*id, kind);
            },
            | Type::Scalar(Rep::Var(id), _) => {
                self.rep_literals.insert(*id, kind);
            },
            | _ => {},
        }
    }

    /// The literal kind attached to a representation variable, if any.
    #[must_use]
    pub fn rep_literal_kind(
        &self,
        rep: Rep,
    ) -> Option<LiteralKind> {
        match rep {
            | Rep::Var(id) => self.rep_literals.get(&id).copied(),
            | _ => None,
        }
    }

    /// The literal kind attached to a variable, if any.
    #[must_use]
    pub fn literal_kind(
        &self,
        ty: &Type,
    ) -> Option<LiteralKind> {
        match ty {
            | Type::Var(id) => self.literals.get(id).copied(),
            | Type::Scalar(Rep::Var(id), _) => self.rep_literals.get(id).copied(),
            | _ => None,
        }
    }

    /// Registers numeric semantics in this inference universe.
    ///
    /// # Errors
    ///
    /// Returns the dimension, representation, or literal conflict exposed by the constraint.
    #[cfg_attr(
        not(test),
        expect(
            clippy::needless_pass_by_value,
            reason = "preserve the public owned-constraint API"
        )
    )]
    pub fn add_numeric_constraint(
        &mut self,
        constraint: NumericConstraint,
    ) -> Result<(), UnifyError> {
        if let NumericConstraint::Relation(RepRelation::Divide {
            input,
            result: Rep::Var(id),
        }) = &constraint
        {
            self.division_inputs.insert(*id, *input);
        }
        if let NumericConstraint::Relation(RepRelation::Join {
            left,
            right,
            result: Rep::Var(id),
        }) = &constraint
        {
            self.join_inputs.insert(*id, (*left, *right));
        }
        if let NumericConstraint::Literal { ty, kind } = &constraint {
            self.mark_literal(ty, *kind);
            if let Type::Scalar(_, dim) = self.resolve(ty)
                && !kind.is_quantity()
            {
                self.unify_dim(&dim, &Dim::one())?;
            }
        } else if !self.contains_numeric(&constraint) {
            self.push_numeric(&constraint);
        }
        self.propagate_numeric()
    }

    /// Applies current bindings to a transported numeric constraint.
    #[must_use]
    pub fn resolve_numeric(
        &self,
        constraint: &NumericConstraint,
    ) -> NumericConstraint {
        match constraint {
            | NumericConstraint::Literal { ty, kind } => {
                NumericConstraint::Literal {
                    ty: self.resolve(ty),
                    kind: *kind,
                }
            },
            | NumericConstraint::Relation(relation) => {
                NumericConstraint::Relation(relation.map(|rep| self.resolve_rep(rep)))
            },
            | NumericConstraint::DefaultLiteral { ty, kind, inputs } => {
                NumericConstraint::DefaultLiteral {
                    ty: self.resolve(ty),
                    kind: *kind,
                    inputs: inputs.iter().map(|rep| self.resolve_rep(*rep)).collect(),
                }
            },
        }
    }

    fn numeric_relations(
        &self,
        constraints: &[NumericConstraint],
    ) -> Result<Vec<RepRelation>, UnifyError> {
        let mut relations = Vec::new();
        for constraint in constraints {
            match self.resolve_numeric(constraint) {
                | NumericConstraint::Relation(relation) => relations.push(relation),
                | NumericConstraint::DefaultLiteral { .. } => {},
                | NumericConstraint::Literal { ty, kind } => {
                    match &ty {
                        | Type::Var(_) | Type::Error => {},
                        | Type::Scalar(rep, dim) if kind.is_quantity() || dim.is_one() => {
                            if !matches!(rep, Rep::Var(_)) && !kind.accepts_rep(*rep) {
                                return Err(UnifyError::LiteralMismatch { kind, requested: ty });
                            }
                            if matches!(rep, Rep::Var(_)) {
                                relations.push(RepRelation::Domain {
                                    rep: *rep,
                                    allowed: RepDomain::from_reps(
                                        RepDomain::CONCRETE
                                            .into_iter()
                                            .filter(|rep| kind.accepts_rep(*rep)),
                                    ),
                                });
                            }
                        },
                        | Type::Point(_) if kind.is_quantity() => {},
                        | _ => {
                            return Err(UnifyError::LiteralMismatch { kind, requested: ty });
                        },
                    }
                },
            }
        }
        Ok(relations)
    }

    /// Propagates active numeric constraints to a fixed point.
    ///
    /// # Errors
    ///
    /// Returns the first unsatisfiable representation or literal relation.
    pub fn propagate_numeric(&mut self) -> Result<(), UnifyError> {
        while let Some(component) = self.numeric.take_dirty_component() {
            #[cfg(test)]
            self.numeric_work
                .set(self.numeric_work.get() + component.len());
            let constraints: Vec<_> = component
                .iter()
                .filter_map(|id| self.numeric.get(*id))
                .cloned()
                .collect();
            let outcome = (|| {
                let relations = self.numeric_relations(&constraints)?;
                let domains =
                    crate::representation::propagate(&relations, |rep| self.resolve_rep(rep))
                        .map_err(|error| UnifyError::Numeric(error.to_string()))?;
                let mut changed = false;
                for (id, domain) in domains.variables() {
                    if let Some(rep) = domain.unique()
                        && matches!(self.resolve_rep(Rep::Var(id)), Rep::Var(_))
                    {
                        self.rep_subst.insert(id, rep);
                        self.numeric_changed(id);
                        changed = true;
                    }
                }
                if changed {
                    return Ok(());
                }
                for constraint in &constraints {
                    let NumericConstraint::DefaultLiteral { ty, kind, inputs } =
                        self.resolve_numeric(constraint)
                    else {
                        continue;
                    };
                    if inputs.iter().any(|rep| matches!(rep, Rep::Var(_))) {
                        continue;
                    }
                    let Type::Scalar(Rep::Var(id), _) = ty else {
                        continue;
                    };
                    let preferred = self.rep_literal_kind(Rep::Var(id)).unwrap_or(kind);
                    let contextual = inputs
                        .iter()
                        .copied()
                        .reduce(|a, b| a.join_concrete(b).unwrap_or(Rep::Complex));
                    let mut selected = None;
                    for candidate in contextual.into_iter().chain([
                        Rep::default_for_literal(preferred),
                        Rep::Int,
                        Rep::Float,
                        Rep::Real,
                        Rep::Complex,
                    ]) {
                        if !kind.accepts_rep(candidate) || !preferred.accepts_rep(candidate) {
                            continue;
                        }
                        let mut trial = relations.clone();
                        trial.push(RepRelation::Domain {
                            rep: Rep::Var(id),
                            allowed: RepDomain::singleton(candidate),
                        });
                        if crate::representation::propagate(&trial, |rep| self.resolve_rep(rep))
                            .is_ok()
                        {
                            selected = Some(candidate);
                            break;
                        }
                    }
                    let Some(rep) = selected else {
                        return Err(UnifyError::Numeric(
                            "no representation can materialize the dependent numeric literal"
                                .to_string(),
                        ));
                    };
                    self.rep_subst.insert(id, rep);
                    self.numeric_changed(id);
                    break;
                }
                Ok(())
            })();
            if let Err(error) = outcome {
                self.numeric.retry(&component);
                return Err(error);
            }
            for id in component {
                if self.numeric.get(id).is_some_and(|constraint| {
                    Self::numeric_variables(&self.resolve_numeric(constraint)).is_empty()
                }) {
                    self.numeric.retire(id);
                }
            }
        }
        Ok(())
    }

    /// Computes the possible concrete representations of an operand.
    ///
    /// # Errors
    ///
    /// Returns a conflict if the connected constraints have no valid assignment.
    pub fn representation_domain(
        &self,
        rep: Rep,
    ) -> Result<RepDomain, UnifyError> {
        let rep = self.resolve_rep(rep);
        if !matches!(rep, Rep::Var(_)) {
            return Ok(RepDomain::singleton(rep));
        }
        let roots = match rep {
            | Rep::Var(id) => vec![id],
            | _ => Vec::new(),
        };
        let constraints = self.numeric_component(roots);
        let relations = self.numeric_relations(&constraints)?;
        let domains = crate::representation::propagate(&relations, |rep| self.resolve_rep(rep))
            .map_err(|error| UnifyError::Numeric(error.to_string()))?;
        Ok(domains.domain(self.resolve_rep(rep)))
    }

    /// Computes or constrains the join of two numeric representations.
    ///
    /// # Errors
    ///
    /// Returns a conflict if the join contradicts active constraints.
    pub fn join_representation(
        &mut self,
        left: Rep,
        right: Rep,
    ) -> Result<Rep, UnifyError> {
        let left = self.resolve_rep(left);
        let right = self.resolve_rep(right);
        if left == right {
            return Ok(left);
        }
        // On all four representations join(divide(r), r) = divide(r).
        // Keep long literal-only arithmetic compact without equating a
        // general computed Join with either of its independent inputs.
        if self.is_join_with(left, right) {
            return Ok(left);
        }
        if self.is_join_with(right, left) {
            return Ok(right);
        }
        if self.is_division_of(left, right) {
            return Ok(left);
        }
        if self.is_division_of(right, left) {
            return Ok(right);
        }
        if !matches!(left, Rep::Var(_)) && !matches!(right, Rep::Var(_)) {
            return Ok(left.join_concrete(right).unwrap_or(Rep::Real));
        }
        let result = self.fresh_rep();
        self.add_numeric_constraint(NumericConstraint::Relation(RepRelation::Join {
            left,
            right,
            result,
        }))?;
        Ok(self.resolve_rep(result))
    }

    fn is_join_with(
        &self,
        result: Rep,
        input: Rep,
    ) -> bool {
        matches!(result,Rep::Var(id) if self.join_inputs.get(&id).is_some_and(|(left,right)| self.resolve_rep(*left) == input || self.resolve_rep(*right) == input))
    }

    fn is_division_of(
        &self,
        result: Rep,
        input: Rep,
    ) -> bool {
        matches!(result,Rep::Var(id) if self.division_inputs.get(&id).is_some_and(|original| self.resolve_rep(*original) == input))
    }

    /// Whether the variable `id` is an operand of a `join` whose other
    /// operand is exact: already `Real`, or an integer division's result
    /// (which is `Real` whatever its input, §3.4.2).
    fn meets_exact(
        &self,
        id: u32,
    ) -> bool {
        let target = self.resolve_rep(Rep::Var(id));
        self.join_inputs.values().any(|(left, right)| {
            let (left, right) = (self.resolve_rep(*left), self.resolve_rep(*right));
            let other = if left == target {
                right
            } else if right == target {
                left
            } else {
                return false;
            };
            match other {
                | Rep::Real => true,
                | Rep::Var(other) => self.division_inputs.contains_key(&other),
                | _ => false,
            }
        })
    }

    /// Whether some resolved arithmetic `join` met `Real` and `Float`
    /// (§3.4.1: the exact side wins, which `W0302` reports).
    #[must_use]
    pub fn has_real_float_join(&self) -> bool {
        self.join_inputs.values().any(|(left, right)| {
            matches!(
                (self.resolve_rep(*left), self.resolve_rep(*right)),
                (Rep::Real, Rep::Float) | (Rep::Float, Rep::Real)
            )
        })
    }

    /// Composite arithmetic retains literal domains, but it is no longer a
    /// direct source literal when a surrounding operation chooses readings.
    pub fn finish_literal_expression(
        &mut self,
        rep: Rep,
    ) {
        if let Rep::Var(id) = rep {
            self.composite_literals.insert(id);
        }
        if let Rep::Var(id) = self.resolve_rep(rep) {
            self.composite_literals.insert(id);
        }
    }

    /// Reading heuristics distinguish a literal leaf from an expression that
    /// retained the same domain. Generalization still sees the actual literal
    /// constraint, so a constructed arithmetic value is never generalized.
    #[must_use]
    pub fn direct_literal_kind(
        &self,
        rep: Rep,
    ) -> Option<LiteralKind> {
        match self.resolve_rep(rep) {
            | Rep::Var(id) if !self.composite_literals.contains(&id) => {
                self.rep_literals.get(&id).copied()
            },
            | _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn numeric_constraint_count(&self) -> usize {
        self.numeric.len()
    }

    #[cfg(test)]
    pub(crate) fn numeric_constraint_peak(&self) -> usize {
        self.numeric_peak
    }

    #[cfg(test)]
    pub(crate) fn numeric_constraint_work(&self) -> usize {
        self.numeric_work.get()
    }

    /// Computes or constrains the representation after division.
    ///
    /// # Errors
    ///
    /// Returns a conflict if the division result contradicts active constraints.
    pub fn divide_representation(
        &mut self,
        input: Rep,
    ) -> Result<Rep, UnifyError> {
        let input = self.resolve_rep(input);
        // Divide is idempotent: its result can never be Int again.
        if matches!(input,Rep::Var(id) if self.division_inputs.contains_key(&id)) {
            return Ok(input);
        }
        if !matches!(input, Rep::Var(_)) {
            return Ok(input.div_rep());
        }
        let result = self.fresh_rep();
        self.add_numeric_constraint(NumericConstraint::Relation(RepRelation::Divide {
            input,
            result,
        }))?;
        Ok(self.resolve_rep(result))
    }

    /// Input-constrained numeric variables are universal even when they also
    /// carry a literal domain (e.g. a branch returning its argument or zero).
    pub fn quantify_numeric_inputs(
        &self,
        scheme: &mut Scheme,
        inputs: &[Type],
        level: u32,
    ) {
        let mut free = crate::TypeSubstitution::default();
        let mut names = Self::new();
        for ty in inputs {
            free.register_variables(&self.resolve(ty), &mut names);
        }
        let mut types: Vec<_> = free.types.into_keys().collect();
        types.sort_unstable();
        for id in types {
            if !self.weak.contains(&id)
                && self.levels.get(&id).copied().unwrap_or(0) > level
                && !scheme.vars.contains(&id)
            {
                scheme.vars.push(id);
            }
        }
        self.quantify_representations(scheme, free.representations, level);
        self.attach_numeric_scheme(scheme);
    }

    /// Quantifies internal literal representations that are chosen per instantiation.
    pub fn quantify_numeric_existentials(
        &self,
        scheme: &mut Scheme,
        level: u32,
    ) {
        let mut free = crate::TypeSubstitution::default();
        let mut names = Self::new();
        for constraint in &scheme.numeric {
            // Private defaulted literal variables are existential, rather
            // than selectable parameters, but each activation needs a fresh
            // copy of them. Other intermediates generalize normally.
            if let NumericConstraint::DefaultLiteral { ty, .. } = constraint {
                free.register_variables(ty, &mut names);
            }
        }
        self.quantify_representations(scheme, free.representations, level);
    }

    fn quantify_representations(
        &self,
        scheme: &mut Scheme,
        representations: HashMap<u32, Rep>,
        level: u32,
    ) {
        let mut variables: Vec<_> = representations.into_keys().collect();
        variables.sort_unstable();
        for id in variables {
            if !self.weak.contains(&id)
                && self.rep_levels.get(&id).copied().unwrap_or(0) > level
                && !scheme.vars.contains(&id)
            {
                scheme.vars.push(id);
            }
        }
    }

    /// Defaults numeric literals whose representations are outside the protected set.
    ///
    /// # Errors
    ///
    /// Returns any type or representation conflict exposed by defaulting.
    pub fn default_unprotected_numeric(
        &mut self,
        protected: &HashSet<u32>,
    ) -> Result<(), UnifyError> {
        let constraints: Vec<_> = self.numeric.iter().cloned().collect();
        self.default_numeric_literals(&constraints, protected)?;
        self.propagate_numeric()
    }

    /// Drains buffered numeric errors for the checker to report.
    pub fn take_numeric_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.numeric_errors)
    }

    /// Chooses the default reading for each eligible unresolved literal.
    ///
    /// # Errors
    ///
    /// Returns a conflict when a selected reading violates current constraints.
    pub fn default_numeric_literals(
        &mut self,
        constraints: &[NumericConstraint],
        protected: &HashSet<u32>,
    ) -> Result<bool, UnifyError> {
        self.propagate_numeric()?;
        let mut changed = false;
        let mut defaults = HashSet::new();
        for constraint in constraints {
            if let NumericConstraint::DefaultLiteral { ty, .. } = constraint {
                self.resolve(ty).visit_numeric_variables(|id| {
                    defaults.insert(id);
                });
            }
        }
        let computed: HashSet<u32> = constraints
            .iter()
            .filter_map(|constraint| {
                match self.resolve_numeric(constraint) {
                    | NumericConstraint::Relation(
                        RepRelation::Join {
                            result: Rep::Var(id), ..
                        }
                        | RepRelation::Divide {
                            result: Rep::Var(id), ..
                        },
                    ) => Some(id),
                    | _ => None,
                }
            })
            .collect();
        for constraint in constraints {
            let NumericConstraint::Literal { ty, kind } = constraint else {
                continue;
            };
            let ty = self.resolve(ty);
            let mut unresolved = false;
            let mut protected_reading = false;
            ty.visit_numeric_variables(|id| {
                unresolved = true;
                protected_reading |=
                    protected.contains(&id) || defaults.contains(&id) || computed.contains(&id);
            });
            if !unresolved || protected_reading {
                continue;
            }
            let preferred = self.literal_kind(&ty).unwrap_or(*kind);
            // §3.10.1: a decimal literal that meets an exact operand is read
            // as its exact decimal, so `1/3 + 0.1` is `13/30`.
            let exact_partner = match &ty {
                | Type::Scalar(Rep::Var(id), _) | Type::Var(id) => self.meets_exact(*id),
                | _ => false,
            };
            let first = if exact_partner && preferred.accepts_rep(Rep::Real) {
                Rep::Real
            } else {
                Rep::default_for_literal(preferred)
            };
            for rep in [first, Rep::Int, Rep::Float, Rep::Real, Rep::Complex] {
                if !kind.accepts_rep(rep) || !preferred.accepts_rep(rep) {
                    continue;
                }
                let reading = match &ty {
                    | Type::Var(_) if !kind.is_quantity() => Type::Scalar(rep, Dim::one()),
                    | Type::Scalar(Rep::Var(_), dim) => Type::Scalar(rep, dim.clone()),
                    | _ => break,
                };
                if self.probe_numeric_reading(&ty, &reading).is_ok() {
                    self.unify(&ty, &reading)?;
                    changed = true;
                    break;
                }
            }
        }
        self.propagate_numeric()?;
        Ok(changed)
    }

    fn numeric_variables(constraint: &NumericConstraint) -> Vec<u32> {
        let mut ids = Vec::new();
        constraint.visit_numeric_variables(|id| ids.push(id));
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Keep precisely the connected numeric contract, including existential
    /// operation results that are not written in the function's argument list.
    pub fn attach_numeric_scheme(
        &self,
        scheme: &mut Scheme,
    ) {
        let mut roots = HashSet::new();
        for ty in std::iter::once(&scheme.ty)
            .chain(scheme.materializations.iter())
            .chain(
                scheme
                    .constraints
                    .iter()
                    .flat_map(|constraint| constraint.args.iter()),
            )
        {
            self.resolve(ty).visit_numeric_variables(|id| {
                roots.insert(id);
            });
        }
        // Inference allocates all kinds from one identity space, but only
        // type/representation variables connect numeric decisions. Dimensions,
        // extents and rows still travel in each clause's complete type schema.
        for id in &scheme.vars {
            self.resolve(&Type::Var(*id)).visit_numeric_variables(|id| {
                roots.insert(id);
            });
            if let Rep::Var(id) = self.resolve_rep(Rep::Var(*id)) {
                roots.insert(id);
            }
        }
        if roots.is_empty() {
            scheme.numeric.clear();
            return;
        }
        scheme.numeric = self.numeric_component(roots);
    }

    /// Validate a literal choice in its complete numeric component without
    /// cloning unrelated module signatures, levels and substitutions.
    fn probe_numeric_reading(
        &self,
        ty: &Type,
        reading: &Type,
    ) -> Result<(), UnifyError> {
        let mut contract = Scheme::mono(self.resolve(ty));
        self.attach_numeric_scheme(&mut contract);
        let mut trial = Self::new();
        let mut projection = crate::TypeSubstitution::default();
        projection.register_variables(&contract.ty, &mut trial);
        for constraint in &contract.numeric {
            for ty in constraint.types() {
                projection.register_variables(&ty, &mut trial);
            }
        }
        for constraint in &contract.numeric {
            trial.add_numeric_constraint(constraint.substitute(&projection))?;
        }
        trial.unify(&projection.apply(&contract.ty), &projection.apply(reading))
    }

    /// Captures the active field exclusions of a scheme's quantified rows.
    pub(crate) fn attach_row_lacks(
        &self,
        scheme: &mut Scheme,
    ) {
        if scheme.rows.is_empty() {
            scheme.row_lacks.clear();
            return;
        }
        let mut combined = BTreeMap::<u32, Vec<String>>::new();
        for (id, labels) in std::mem::take(&mut scheme.row_lacks) {
            if scheme.rows.contains(&id) {
                combined.entry(id).or_default().extend(labels);
            }
        }
        for id in &scheme.rows {
            if let Some(labels) = self.lacks.get(id) {
                combined
                    .entry(*id)
                    .or_default()
                    .extend(labels.iter().cloned());
            }
        }
        scheme.row_lacks = combined
            .into_iter()
            .filter_map(|(id, mut labels)| {
                labels.sort_unstable();
                labels.dedup();
                (!labels.is_empty()).then_some((id, labels))
            })
            .collect();
    }

    /// Normalize an interface without choosing new defaults or changing its
    /// ordered quantifier identities.
    #[must_use]
    pub fn resolve_scheme(
        &self,
        scheme: &Scheme,
    ) -> Scheme {
        let mut result = scheme.clone();
        result.ty = self.resolve(&scheme.ty);
        for constraint in &mut result.constraints {
            for ty in &mut constraint.args {
                *ty = self.resolve(ty);
            }
        }
        result.numeric = scheme
            .numeric
            .iter()
            .map(|clause| self.resolve_numeric(clause))
            .collect();
        result.materializations = scheme
            .materializations
            .iter()
            .map(|ty| self.resolve(ty))
            .collect();
        self.attach_row_lacks(&mut result);
        result
    }

    /// Resolves a type through all substitutions.
    #[must_use]
    pub fn resolve(
        &self,
        ty: &Type,
    ) -> Type {
        self.resolve_depth(ty, 0)
    }

    fn resolve_depth(
        &self,
        ty: &Type,
        depth: u32,
    ) -> Type {
        if depth > 128 {
            return ty.clone();
        }
        match ty {
            // A chain `a := b := c := …` is followed without spending depth:
            // a tensor literal of N elements unifies N fresh variables in a
            // line, and the depth bound is for *structural* nesting. Cutting
            // the chain at 128 left the variable unresolved, so a literal with
            // more than 128 elements failed at run time (`E0304`).
            | Type::Var(id) => {
                match self.follow_type(*id) {
                    | Some(Type::Var(unbound)) => Type::Var(*unbound),
                    | Some(bound) => self.resolve_depth(bound, depth),
                    | None => ty.clone(),
                }
            },
            | Type::Scalar(rep, dim) => {
                Type::Scalar(self.resolve_rep_depth(*rep, depth), self.dims.resolve(dim))
            },
            | Type::Tensor { shape, elem } => {
                Type::Tensor {
                    shape: self.resolve_shape_depth(shape, depth.saturating_add(1)),
                    elem: Box::new(self.resolve_depth(elem, depth.saturating_add(1))),
                }
            },
            | Type::Fn(params, ret) => {
                Type::Fn(
                    params
                        .iter()
                        .map(|param| self.resolve_depth(param, depth.saturating_add(1)))
                        .collect(),
                    Box::new(self.resolve_depth(ret, depth.saturating_add(1))),
                )
            },
            | Type::Record(row) => Type::Record(self.resolve_row_depth(row, depth)),
            | Type::Adt(id, args) => {
                Type::Adt(
                    *id,
                    args.iter()
                        .map(|arg| self.resolve_depth(arg, depth.saturating_add(1)))
                        .collect(),
                )
            },
            | Type::Io(inner) => {
                Type::Io(Box::new(self.resolve_depth(inner, depth.saturating_add(1))))
            },
            | Type::Dyn(rep) => Type::Dyn(self.resolve_rep_depth(*rep, depth)),
            | Type::Point(dim) => Type::Point(self.dims.resolve(dim)),
            | Type::UnitOf(dim) => Type::UnitOf(self.dims.resolve(dim)),
            | Type::TypeRepIntrinsic(inner) => {
                Type::TypeRepIntrinsic(
                    inner
                        .as_ref()
                        .map(|ty| Box::new(self.resolve_depth(ty, depth.saturating_add(1)))),
                )
            },
            | Type::FromValueIntrinsic(inner) => {
                Type::FromValueIntrinsic(
                    inner
                        .as_ref()
                        .map(|ty| Box::new(self.resolve_depth(ty, depth.saturating_add(1)))),
                )
            },
            | Type::TensorIntrinsic(intrinsic) => {
                let mut intrinsic = *intrinsic;
                intrinsic.element = intrinsic
                    .element
                    .map(|rep| self.resolve_rep_depth(rep, depth.saturating_add(1)));
                Type::TensorIntrinsic(intrinsic)
            },
            | _ => ty.clone(),
        }
    }

    /// Resolves a dimension through the dimension substitution.
    #[must_use]
    pub fn resolve_dim(
        &self,
        dim: &Dim,
    ) -> Dim {
        self.dims.resolve(dim)
    }

    /// Resolves a representation through substitutions.
    #[must_use]
    pub fn resolve_rep(
        &self,
        rep: Rep,
    ) -> Rep {
        self.resolve_rep_depth(rep, 0)
    }

    fn resolve_rep_depth(
        &self,
        rep: Rep,
        depth: u32,
    ) -> Rep {
        let _ = depth;
        // Iterative: a representation chain has no structure to bound, and a
        // cycle is impossible (each step is bounded by the table's size).
        let mut current = rep;
        for _ in 0..=self.rep_subst.len() {
            match current {
                | Rep::Var(id) => {
                    match self.rep_subst.get(&id) {
                        | Some(bound) => current = *bound,
                        | None => return current,
                    }
                },
                | other => return other,
            }
        }
        current
    }

    /// The first non-variable binding at the end of the chain from `id`, or
    /// the last variable when the chain ends unbound (`None` if `id` itself
    /// is unbound).
    fn follow_type(
        &self,
        id: u32,
    ) -> Option<&Type> {
        let mut bound = self.subst.get(&id)?.as_ref();
        for _ in 0..self.subst.len() {
            let Type::Var(next) = bound else {
                break;
            };
            let Some(further) = self.subst.get(next) else {
                break;
            };
            bound = further.as_ref();
        }
        Some(bound)
    }

    /// Follows a variable binding chain, compressing alias-only hops and
    /// returning the root variable id alongside any non-variable `Arc` binding.
    fn follow_type_arc(
        &mut self,
        id: u32,
    ) -> (u32, Option<Arc<Type>>) {
        let Some(first) = self.subst.get(&id) else {
            return (id, None);
        };
        let Type::Var(next1) = first.as_ref() else {
            return (id, Some(Arc::clone(first)));
        };
        let next1 = *next1;
        let Some(second) = self.subst.get(&next1) else {
            return (next1, None);
        };
        let Type::Var(next2) = second.as_ref() else {
            return (next1, Some(Arc::clone(second)));
        };
        let mut chain = vec![id, next1];
        let mut curr = *next2;
        let mut resolved = None;
        for _ in 0..=self.subst.len() {
            let Some(bound) = self.subst.get(&curr) else {
                break;
            };
            if let Type::Var(next) = bound.as_ref() {
                chain.push(curr);
                curr = *next;
            } else {
                resolved = Some(Arc::clone(bound));
                break;
            }
        }
        let target = Arc::new(Type::Var(curr));
        for alias in chain {
            self.subst.insert(alias, Arc::clone(&target));
        }
        (curr, resolved)
    }

    /// Returns the root `Type::Var` if `ty` still resolves to an unbound type variable.
    #[must_use]
    #[allow(
        clippy::single_call_fn,
        reason = "advances tensor literal element aliases without eager full-tree resolution"
    )]
    pub(crate) fn unbound_var_alias(
        &mut self,
        ty: &Type,
    ) -> Option<Type> {
        let Type::Var(id) = ty else {
            return None;
        };
        let (root, bound) = self.follow_type_arc(*id);
        bound.is_none().then_some(Type::Var(root))
    }

    /// Resolves only the outermost head to read a tensor's resolved shape without cloning element trees.
    #[must_use]
    pub(crate) fn outer_tensor_shape(
        &self,
        ty: &Type,
    ) -> Option<Shape> {
        let head = match ty {
            | Type::Var(id) => self.follow_type(*id)?,
            | other => other,
        };
        let Type::Tensor { shape, .. } = head else {
            return None;
        };
        Some(self.resolve_shape_depth(shape, 1))
    }

    /// Shortens alias-only paths before publishing a batch of inferred types.
    ///
    /// The final variable and its binding stay in place, preserving the chosen
    /// representation, lexical levels and all dependencies on that variable.
    pub(crate) fn compress_type_aliases(&mut self) {
        let aliases = self
            .subst
            .iter()
            .filter_map(|(&id, ty)| matches!(ty.as_ref(), Type::Var(_)).then_some(id))
            .collect::<Vec<_>>();
        let mut path = Vec::new();
        for alias in aliases {
            let mut root = alias;
            // Binding links distinct resolved variables, so alias paths are acyclic.
            while let Some(bound) = self.subst.get(&root)
                && let Type::Var(parent) = bound.as_ref()
            {
                path.push(root);
                root = *parent;
            }
            if !path.is_empty() {
                let compressed = Arc::new(Type::Var(root));
                while let Some(child) = path.pop() {
                    self.subst.insert(child, Arc::clone(&compressed));
                }
            }
        }
    }

    /// Resolves a row through substitutions.
    #[must_use]
    pub fn resolve_row(
        &self,
        row: &Row,
    ) -> Row {
        self.resolve_row_depth(row, 0)
    }

    fn resolve_row_depth(
        &self,
        row: &Row,
        depth: u32,
    ) -> Row {
        if depth > 128 {
            return row.clone();
        }
        let mut current = row;
        let mut bindings_left = self.row_subst.len();
        let mut fields = Vec::new();
        let tail = loop {
            match current {
                | Row::Empty => break Row::Empty,
                | Row::Var(id) => {
                    let Some((bound, remaining)) =
                        self.row_subst.get(id).zip(bindings_left.checked_sub(1))
                    else {
                        break Row::Var(*id);
                    };
                    current = bound;
                    bindings_left = remaining;
                },
                | Row::Extend { label, ty, tail } => {
                    fields.push((
                        label.clone(),
                        self.resolve_depth(ty, depth.saturating_add(1)),
                    ));
                    current = tail;
                },
            }
        };
        extend_row_fields(fields.into_iter().rev(), tail)
    }

    /// Resolves a shape's type-level extents through the Nat substitution.
    #[must_use]
    pub fn resolve_shape(
        &self,
        shape: &Shape,
    ) -> Shape {
        self.resolve_shape_depth(shape, 0)
    }

    fn resolve_shape_depth(
        &self,
        shape: &Shape,
        depth: u32,
    ) -> Shape {
        if depth > 128 {
            return shape.clone();
        }
        match shape {
            | Shape::Rank(rank) => Shape::Rank(*rank),
            | Shape::Refined(items) => {
                Shape::Refined(
                    items
                        .iter()
                        .map(|item| {
                            item.as_ref()
                                .map(|nat| self.resolve_nat_depth(nat, depth.saturating_add(1)))
                        })
                        .collect(),
                )
            },
        }
    }

    /// A fresh type-level natural-number variable at the current level.
    pub fn fresh_nat(&mut self) -> Nat {
        let id = self.fresh_id();
        self.nat_levels.insert(id, self.level);
        Nat::Var(id)
    }

    /// Resolves a Nat term through the substitution, folding literals.
    #[must_use]
    pub fn resolve_nat(
        &self,
        nat: &Nat,
    ) -> Nat {
        self.resolve_nat_depth(nat, 0)
    }

    fn resolve_nat_depth(
        &self,
        nat: &Nat,
        depth: u32,
    ) -> Nat {
        if depth > 128 {
            return nat.clone();
        }
        match nat {
            | Nat::Lit(value) => Nat::Lit(*value),
            | Nat::Var(id) => {
                let Some(mut bound) = self.nat_subst.get(id) else {
                    return nat.clone();
                };
                for _ in 0..=self.nat_subst.len() {
                    let Nat::Var(next) = bound else {
                        break;
                    };
                    let Some(further) = self.nat_subst.get(next) else {
                        return Nat::Var(*next);
                    };
                    bound = further;
                }
                self.resolve_nat_depth(bound, depth.saturating_add(1))
            },
            | Nat::Add(left, right) => {
                let left = self.resolve_nat_depth(left, depth.saturating_add(1));
                let right = self.resolve_nat_depth(right, depth.saturating_add(1));
                left.fold_add(right)
            },
            | Nat::Mul(left, right) => {
                let left = self.resolve_nat_depth(left, depth.saturating_add(1));
                let right = self.resolve_nat_depth(right, depth.saturating_add(1));
                left.fold_mul(right)
            },
        }
    }

    /// Records the source name of a type-level variable, for diagnostics.
    ///
    /// A `rigid` variable is one the user declared (`fn f<n>`): the solver may
    /// bind it to a literal or to an expression of other variables
    /// (`n + 1 ~ 3` => `n = 2`), but it never merges two distinct declared
    /// variables (`n ~ m` is an error: the two extents are not known to be
    /// equal). A variable that is not rigid is an inference variable (a fresh
    /// instantiation of a generic callee) and unifies with anything.
    pub fn name_nat(
        &mut self,
        nat: &Nat,
        name: &str,
        rigid: bool,
    ) {
        if let Nat::Var(id) = nat {
            self.nat_names.insert(*id, name.to_string());
            if rigid {
                self.nat_rigid.insert(*id);
            }
        }
    }

    /// A Nat term as the reader wrote it (`n + 1`, `n * m`), resolved through
    /// the current solution.
    #[must_use]
    pub fn render_nat(
        &self,
        nat: &Nat,
    ) -> String {
        self.render_nat_prec(&self.resolve_nat(nat), 0)
    }

    fn render_nat_prec(
        &self,
        nat: &Nat,
        precedence: u8,
    ) -> String {
        match nat {
            | Nat::Lit(value) => value.to_string(),
            | Nat::Var(id) => {
                self.nat_names
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| format!("?{id}"))
            },
            | Nat::Add(left, right) => {
                let text = format!(
                    "{} + {}",
                    self.render_nat_prec(left, 1),
                    self.render_nat_prec(right, 1)
                );
                if precedence > 1 {
                    format!("({text})")
                } else {
                    text
                }
            },
            | Nat::Mul(left, right) => {
                format!(
                    "{} * {}",
                    self.render_nat_prec(left, 2),
                    self.render_nat_prec(right, 2)
                )
            },
        }
    }

    /// A shape as the reader wrote it (`[n, m + 1]`), resolved.
    #[must_use]
    pub fn render_shape(
        &self,
        shape: &Shape,
    ) -> String {
        match self.resolve_shape(shape) {
            | Shape::Rank(rank) => format!("rank {rank}"),
            | Shape::Refined(items) => {
                let items: Vec<String> = items
                    .iter()
                    .map(|item| {
                        item.as_ref()
                            .map_or_else(|| "?".to_string(), |nat| self.render_nat(nat))
                    })
                    .collect();
                format!("[{}]", items.join(", "))
            },
        }
    }

    /// The polynomial of an already resolved Nat term.
    fn nat_poly(nat: &Nat) -> NatLinear {
        match nat {
            | Nat::Lit(value) => NatLinear::constant(BigInt::from(*value)),
            | Nat::Var(id) => {
                NatLinear {
                    terms: BTreeMap::from([(vec![*id], BigInt::one())]),
                }
            },
            | Nat::Add(left, right) => Self::nat_poly(left).add(Self::nat_poly(right)),
            | Nat::Mul(left, right) => Self::nat_poly(left).mul(&Self::nat_poly(right)),
        }
    }

    /// Substitutes the current solution into a stored polynomial.
    fn reduce_nat(
        &self,
        mut equation: NatLinear,
    ) -> NatLinear {
        // `resolve_nat` bounds the depth of one traversal. Repeat as needed so
        // a deferred fact is indexed by its unbound variables even when the
        // substitution chain is deeper than one traversal can resolve.
        while equation
            .terms
            .keys()
            .flatten()
            .any(|id| self.nat_subst.contains_key(id))
        {
            let mut out = NatLinear::constant(BigInt::ZERO);
            for (monomial, coefficient) in equation.terms {
                let mut term = NatLinear::constant(coefficient);
                for id in monomial {
                    term = term.mul(&Self::nat_poly(&self.resolve_nat(&Nat::Var(id))));
                }
                out = out.add(term);
            }
            equation = out;
        }
        equation
    }

    /// `lhs = rhs` for an equation `lhs - rhs = 0`, for diagnostics.
    fn describe_nat_equation(
        &self,
        equation: &NatLinear,
    ) -> (String, String) {
        let side = |negative: bool| {
            let mut parts = Vec::new();
            let mut constant = None;
            for (monomial, coefficient) in &equation.terms {
                if (coefficient.sign() == Sign::Minus) != negative {
                    continue;
                }
                let magnitude = coefficient.magnitude();
                if monomial.is_empty() {
                    constant = Some(magnitude.to_string());
                    continue;
                }
                let vars: Vec<String> = monomial
                    .iter()
                    .map(|id| self.render_nat(&Nat::Var(*id)))
                    .collect();
                let product = vars.join(" * ");
                parts.push(if magnitude.is_one() {
                    product
                } else {
                    format!("{magnitude} * {product}")
                });
            }
            parts.extend(constant);
            if parts.is_empty() {
                "0".to_string()
            } else {
                parts.join(" + ")
            }
        };
        (side(false), side(true))
    }

    /// Encodes a non-negative polynomial as a Nat term.
    #[cfg_attr(
        not(test),
        expect(
            clippy::single_call_fn,
            reason = "construct a representable Nat binding from an isolated equation"
        )
    )]
    fn poly_to_nat(equation: &NatLinear) -> Option<Nat> {
        if equation
            .terms
            .values()
            .any(|coefficient| coefficient.sign() == Sign::Minus)
        {
            return None;
        }
        let mut result: Option<Nat> = None;
        let mut push = |term: Nat| {
            result = Some(match result.take() {
                | Some(sum) => sum.add(term),
                | None => term,
            });
        };
        for (monomial, coefficient) in &equation.terms {
            if monomial.is_empty() {
                continue;
            }
            let mut product: Option<Nat> = None;
            for id in monomial {
                let var = Nat::Var(*id);
                product = Some(match product.take() {
                    | Some(factor) => factor.mul(var),
                    | None => var,
                });
            }
            let mut term = product?;
            if !coefficient.is_one() {
                term = Nat::Lit(coefficient.to_usize()?).mul(term);
            }
            push(term);
        }
        if let Some(constant) = equation.constant_part() {
            push(Nat::Lit(constant.to_usize()?));
        }
        result.or(Some(Nat::Lit(0)))
    }

    fn nat_is_flexible(
        &self,
        id: u32,
    ) -> bool {
        !self.nat_rigid.contains(&id) && !self.nat_subst.contains_key(&id)
    }

    /// Unifies two type-level natural numbers (§3.5.1).
    ///
    /// The difference is put in polynomial form. A single variable isolates
    /// (`n + 1 ~ 3` => `n = 2`, `2 * n ~ 6` => `n = 3`); an inference variable
    /// is eliminated against any expression of the others (`a ~ n + 1` =>
    /// `a = n + 1`, `a ~ b` => `a = b`); products of variables are opaque
    /// monomials that cancel when they match (`n * m ~ m * n`); what remains
    /// stays pending until one of its variables is bound. A form with no
    /// variable either holds or is inconsistent.
    ///
    /// # Errors
    ///
    /// [`UnifyError::NatMismatch`] when the equation has no non-negative
    /// integer solution and [`UnifyError::NatRigid`] when it equates two
    /// declared extents that are not known to be equal.
    pub fn unify_nat(
        &mut self,
        left: &Nat,
        right: &Nat,
    ) -> Result<(), UnifyError> {
        let left = self.resolve_nat(left);
        let right = self.resolve_nat(right);
        if left == right {
            return Ok(());
        }
        let equation = self.reduce_nat(Self::nat_poly(&left).sub(Self::nat_poly(&right)));
        let step = self.step_nat_equation(equation).map_err(|error| {
            match error {
                | UnifyError::NatMismatch { .. } => {
                    UnifyError::NatMismatch {
                        left: self.render_nat_prec(&left, 0),
                        right: self.render_nat_prec(&right, 0),
                    }
                },
                | other => other,
            }
        })?;
        if let NatStep::Residual(equation) = step {
            let variables = equation.variables();
            self.nat_pending.insert_waiting(equation, variables);
        }
        self.propagate_nat_equations()
    }

    /// Retries affected facts until no binding can trigger more work.
    fn propagate_nat_equations(&mut self) -> Result<(), UnifyError> {
        while let Some(id) = self.nat_pending.take_dirty_fact() {
            let Some(previous) = self.nat_pending.get(id) else {
                continue;
            };
            #[cfg(test)]
            {
                self.nat_retries += 1;
            }
            let equation = self.reduce_nat(previous.clone());
            match self.step_nat_equation(equation) {
                | Ok(NatStep::Consumed) => self.nat_pending.retire(id),
                | Ok(NatStep::Residual(equation)) => {
                    let variables = equation.variables();
                    self.nat_pending.replace_waiting(id, equation, variables);
                },
                | Err(error) => {
                    self.nat_pending.retire(id);
                    return Err(error);
                },
            }
        }
        Ok(())
    }

    fn bind_nat(
        &mut self,
        id: u32,
        expression: Nat,
    ) {
        self.nat_subst.insert(id, expression);
        self.nat_pending.wake_dependents(id);
    }

    fn nat_mismatch(
        &self,
        equation: &NatLinear,
    ) -> UnifyError {
        let (left, right) = self.describe_nat_equation(equation);
        UnifyError::NatMismatch { left, right }
    }

    fn step_nat_equation(
        &mut self,
        equation: NatLinear,
    ) -> Result<NatStep, UnifyError> {
        if equation.is_constant() {
            return if equation.terms.is_empty() {
                Ok(NatStep::Consumed)
            } else {
                Err(self.nat_mismatch(&equation))
            };
        }
        let linear = equation.linear_terms();
        // One variable and a constant: `c*v + k = 0`.
        if let [(id, coefficient)] = linear.as_slice()
            && equation.is_linear()
        {
            let constant = equation.constant_part().unwrap_or(&BigInt::ZERO);
            let value =
                exact_nat_quotient(constant, coefficient).and_then(|value| value.to_usize());
            let Some(value) = value else {
                return Err(self.nat_mismatch(&equation));
            };
            self.bind_nat(*id, Nat::Lit(value));
            return Ok(NatStep::Consumed);
        }
        // An inference variable that appears only linearly is eliminated
        // against the rest (`a - b = 0` => `a = b`; `a - n - 1 = 0` =>
        // `a = n + 1`), provided the rest is a non-negative expression.
        let mut candidates: Vec<(u32, &BigInt)> = linear
            .iter()
            .copied()
            .filter(|(id, _)| self.nat_is_flexible(*id) && !equation.in_product(*id))
            .collect();
        candidates.sort_by_key(|(id, _)| (self.nat_level(*id), *id));
        for (id, coefficient) in candidates.into_iter().rev() {
            let mut solution = equation.clone();
            solution.terms.remove([id].as_slice());
            if solution
                .terms
                .values_mut()
                .try_for_each(|value| {
                    *value = exact_nat_quotient(value, coefficient)?;
                    Some(())
                })
                .is_none()
            {
                continue;
            }
            let Some(expression) = Self::poly_to_nat(&solution) else {
                continue;
            };
            // The eliminated variable's level bounds its replacement's.
            let level = self.nat_level(id);
            for var in solution.variables() {
                let entry = self.nat_levels.entry(var).or_insert(level);
                *entry = (*entry).min(level);
            }
            self.bind_nat(id, expression);
            return Ok(NatStep::Consumed);
        }
        // Two declared extents that are not the same variable.
        if let [(left_id, left_coefficient), (right_id, right_coefficient)] = linear.as_slice()
            && equation.is_linear()
            && equation.constant_part().is_none()
            && right_coefficient.sign() != left_coefficient.sign()
            && right_coefficient.magnitude() == left_coefficient.magnitude()
            && self.nat_rigid.contains(left_id)
            && self.nat_rigid.contains(right_id)
        {
            let left = self.render_nat(&Nat::Var(*left_id));
            let right = self.render_nat(&Nat::Var(*right_id));
            return Err(UnifyError::NatRigid { left, right });
        }
        Ok(NatStep::Residual(equation))
    }

    /// How many equations are still unsolved.
    #[must_use]
    pub const fn unsolved_nat_equations(&self) -> usize {
        self.nat_pending.len()
    }

    /// Removes and returns the equations still pending, rendered as
    /// `lhs = rhs`; the checker reports each at the end of a declaration
    /// (§3.10.2 rule 6).
    pub fn take_unsolved_nat_equations(&mut self) -> Vec<UnsolvedNat> {
        self.nat_pending
            .drain()
            .map(|equation| {
                let (left, right) = self.describe_nat_equation(&equation);
                UnsolvedNat {
                    text: format!("{left} = {right}"),
                    linear: equation.is_linear(),
                }
            })
            .collect()
    }

    /// The level at which a Nat variable was created, for generalisation.
    #[must_use]
    pub fn nat_level(
        &self,
        id: u32,
    ) -> u32 {
        self.nat_levels.get(&id).copied().unwrap_or(0)
    }

    /// Unifies two types.
    ///
    /// # Errors
    ///
    /// Returns a [`UnifyError`] describing the first difference found.
    pub fn unify(
        &mut self,
        left: &Type,
        right: &Type,
    ) -> Result<(), UnifyError> {
        if std::ptr::eq(left, right) {
            return self.propagate_numeric();
        }
        if let (Type::Var(a), Type::Var(b)) = (left, right)
            && (a == b || self.unified_vars.contains(&canonical_pair(*a, *b)))
        {
            return self.propagate_numeric();
        }
        let mut memo = UnifyMemo::new();
        self.unify_pair_rec(&mut memo, left, right, 0)?;
        self.propagate_numeric()?;
        self.unified_vars.extend(memo.verified_var_pairs);
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "structural match across Type variants with iterative single-child loop"
    )]
    fn unify_pair_rec(
        &mut self,
        memo: &mut UnifyMemo,
        left: &Type,
        right: &Type,
        depth: u32,
    ) -> Result<(), UnifyError> {
        if std::ptr::eq(left, right) || left.is_error() || right.is_error() {
            return Ok(());
        }
        if depth >= 64 {
            return self.unify_trampoline(memo, left, right);
        }
        let mut cur_l = left;
        let mut cur_r = right;
        loop {
            if std::ptr::eq(cur_l, cur_r) || cur_l.is_error() || cur_r.is_error() {
                return Ok(());
            }
            if matches!(cur_l, Type::Var(_)) || matches!(cur_r, Type::Var(_)) {
                break;
            }
            match (cur_l, cur_r) {
                | (Type::Tensor { shape: sa, elem: ea }, Type::Tensor { shape: sb, elem: eb }) => {
                    let key_l = NodeKey::Ptr(std::ptr::from_ref(cur_l).addr());
                    let key_r = NodeKey::Ptr(std::ptr::from_ref(cur_r).addr());
                    if !memo.visited.insert((key_l, key_r)) {
                        return Ok(());
                    }
                    self.unify_shape_memo(memo, sa, sb)?;
                    cur_l = ea;
                    cur_r = eb;
                },
                | (Type::Io(a), Type::Io(b))
                | (Type::TypeRepIntrinsic(Some(a)), Type::TypeRepIntrinsic(Some(b)))
                | (Type::FromValueIntrinsic(Some(a)), Type::FromValueIntrinsic(Some(b))) => {
                    let key_l = NodeKey::Ptr(std::ptr::from_ref(cur_l).addr());
                    let key_r = NodeKey::Ptr(std::ptr::from_ref(cur_r).addr());
                    if !memo.visited.insert((key_l, key_r)) {
                        return Ok(());
                    }
                    cur_l = a;
                    cur_r = b;
                },
                | (Type::Fn(pa, ra), Type::Fn(pb, rb)) => {
                    let key_l = NodeKey::Ptr(std::ptr::from_ref(cur_l).addr());
                    let key_r = NodeKey::Ptr(std::ptr::from_ref(cur_r).addr());
                    if !memo.visited.insert((key_l, key_r)) {
                        return Ok(());
                    }
                    if pa.len() != pb.len() {
                        return Err(UnifyError::Mismatch {
                            left: self.resolve(cur_l),
                            right: self.resolve(cur_r),
                        });
                    }
                    for (a, b) in pa.iter().zip(pb.iter()) {
                        self.unify_pair_rec(memo, a, b, depth.saturating_add(1))?;
                    }
                    cur_l = ra;
                    cur_r = rb;
                },
                | (Type::Adt(ia, aa), Type::Adt(ib, ab)) => {
                    let key_l = NodeKey::Ptr(std::ptr::from_ref(cur_l).addr());
                    let key_r = NodeKey::Ptr(std::ptr::from_ref(cur_r).addr());
                    if !memo.visited.insert((key_l, key_r)) {
                        return Ok(());
                    }
                    if !crate::adt::same_type(*ia, *ib) || aa.len() != ab.len() {
                        return Err(UnifyError::Mismatch {
                            left: self.resolve(cur_l),
                            right: self.resolve(cur_r),
                        });
                    }
                    if aa.len() == 1
                        && let (Some(a), Some(b)) = (aa.first(), ab.first())
                    {
                        cur_l = a;
                        cur_r = b;
                        continue;
                    }
                    for (a, b) in aa.iter().zip(ab.iter()) {
                        self.unify_pair_rec(memo, a, b, depth.saturating_add(1))?;
                    }
                    return Ok(());
                },
                | (Type::Record(ra), Type::Record(rb)) => {
                    let key_l = NodeKey::Ptr(std::ptr::from_ref(cur_l).addr());
                    let key_r = NodeKey::Ptr(std::ptr::from_ref(cur_r).addr());
                    if !memo.visited.insert((key_l, key_r)) {
                        return Ok(());
                    }
                    return self.unify_row_memo(memo, ra, rb, depth.saturating_add(1));
                },
                | _ => return self.unify_leaf_types(cur_l, cur_r),
            }
        }
        match (cur_l, cur_r) {
            | (Type::Var(id_l), Type::Var(id_r)) => {
                if let Some((arc_l, arc_r)) = self.unify_var_pair(memo, *id_l, *id_r)? {
                    return self.unify_pair_rec(
                        memo,
                        arc_l.as_ref(),
                        arc_r.as_ref(),
                        depth.saturating_add(1),
                    );
                }
                Ok(())
            },
            | (Type::Var(id), other) => self.unify_var_with_non_var(memo, *id, other, true, depth),
            | (other, Type::Var(id)) => self.unify_var_with_non_var(memo, *id, other, false, depth),
            | _ => Ok(()),
        }
    }

    fn unify_var_pair(
        &mut self,
        memo: &mut UnifyMemo,
        id_l: u32,
        id_r: u32,
    ) -> Result<Option<(Arc<Type>, Arc<Type>)>, UnifyError> {
        let (root_l, opt_arc_l) = self.follow_type_arc(id_l);
        let (root_r, opt_arc_r) = self.follow_type_arc(id_r);
        if root_l == root_r {
            return Ok(None);
        }
        let var_pair = canonical_pair(root_l, root_r);
        if self.unified_vars.contains(&var_pair) {
            return Ok(None);
        }
        match (opt_arc_l, opt_arc_r) {
            | (None, None) => {
                self.bind_var_alias(root_l, root_r);
                Ok(None)
            },
            | (None, Some(arc_r)) => {
                if !arc_r.is_error() {
                    self.bind_var_non_var(root_l, BindTarget::Arc(arc_r))?;
                }
                Ok(None)
            },
            | (Some(arc_l), None) => {
                if !arc_l.is_error() {
                    self.bind_var_non_var(root_r, BindTarget::Arc(arc_l))?;
                }
                Ok(None)
            },
            | (Some(arc_l), Some(arc_r)) => {
                if Arc::ptr_eq(&arc_l, &arc_r) {
                    memo.verified_var_pairs.push(var_pair);
                    return Ok(None);
                }
                if !memo
                    .visited
                    .insert((NodeKey::Var(root_l), NodeKey::Var(root_r)))
                {
                    return Ok(None);
                }
                memo.verified_var_pairs.push(var_pair);
                Ok(Some((arc_l, arc_r)))
            },
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "shared variable-against-structure handler called from left and right match arms"
    )]
    fn unify_var_with_non_var(
        &mut self,
        memo: &mut UnifyMemo,
        id: u32,
        other: &Type,
        var_on_left: bool,
        depth: u32,
    ) -> Result<(), UnifyError> {
        if other.is_error() {
            return Ok(());
        }
        let (root, opt_arc) = self.follow_type_arc(id);
        let Some(arc) = opt_arc else {
            return self.bind_var_non_var(root, BindTarget::Borrowed(other));
        };
        if std::ptr::eq(arc.as_ref(), other) {
            return Ok(());
        }
        let other_ptr = std::ptr::from_ref(other).addr();
        let key = if var_on_left {
            (NodeKey::Var(root), NodeKey::Ptr(other_ptr))
        } else {
            (NodeKey::Ptr(other_ptr), NodeKey::Var(root))
        };
        if !memo.visited.insert(key) {
            return Ok(());
        }
        let (l, r) = if var_on_left {
            (arc.as_ref(), other)
        } else {
            (other, arc.as_ref())
        };
        self.unify_pair_rec(memo, l, r, depth.saturating_add(1))
    }

    fn unify_leaf_types(
        &mut self,
        left: &Type,
        right: &Type,
    ) -> Result<(), UnifyError> {
        match (left, right) {
            | (Type::Scalar(rep_a, dim_a), Type::Scalar(rep_b, dim_b)) => {
                self.unify_rep_inner(*rep_a, *rep_b)?;
                self.unify_dim(dim_a, dim_b)
            },
            | (Type::TensorIntrinsic(a), Type::TensorIntrinsic(b)) => {
                if a == b {
                    Ok(())
                } else if a.kind != crate::ty::TensorIntrinsic::Fill
                    && b.kind != crate::ty::TensorIntrinsic::Fill
                {
                    match (a.element, b.element) {
                        | (Some(l), Some(r)) => self.unify_rep_inner(l, r),
                        | (None, None) => Ok(()),
                        | _ => {
                            Err(UnifyError::Mismatch {
                                left: self.resolve(left),
                                right: self.resolve(right),
                            })
                        },
                    }
                } else {
                    Err(UnifyError::Mismatch {
                        left: self.resolve(left),
                        right: self.resolve(right),
                    })
                }
            },
            | (Type::Point(a), Type::Point(b)) | (Type::UnitOf(a), Type::UnitOf(b)) => {
                self.unify_dim(a, b)
            },
            | (Type::Dyn(a), Type::Dyn(b)) => self.unify_rep_inner(*a, *b),
            | (Type::Bool, Type::Bool)
            | (Type::Prop, Type::Prop)
            | (Type::Str, Type::Str)
            | (Type::Unit, Type::Unit)
            | (Type::WriteNpyIntrinsic, Type::WriteNpyIntrinsic)
            | (Type::ToCallbackIntrinsic, Type::ToCallbackIntrinsic)
            | (Type::TypeRepIntrinsic(None), Type::TypeRepIntrinsic(None))
            | (Type::FromValueIntrinsic(None), Type::FromValueIntrinsic(None)) => Ok(()),
            | (Type::ReadNpyIntrinsic(a), Type::ReadNpyIntrinsic(b)) if a == b => Ok(()),
            | (Type::Param(a), Type::Param(b)) if a == b => Ok(()),
            | _ => {
                Err(UnifyError::Mismatch {
                    left: self.resolve(left),
                    right: self.resolve(right),
                })
            },
        }
    }

    #[allow(
        clippy::cognitive_complexity,
        clippy::single_call_fn,
        clippy::too_many_lines,
        reason = "iterative fallback avoids stack overflow for arbitrarily deep or DAG-shaped types"
    )]
    fn unify_trampoline(
        &mut self,
        memo: &mut UnifyMemo,
        left: &Type,
        right: &Type,
    ) -> Result<(), UnifyError> {
        let mut work = vec![(memo.to_flat_side(left), memo.to_flat_side(right))];
        while let Some((raw_l, raw_r)) = work.pop() {
            let side_l = memo.unwrap_flat_var(raw_l);
            let side_r = memo.unwrap_flat_var(raw_r);
            match (side_l, side_r) {
                | (FlatSide::Var(id_l), FlatSide::Var(id_r)) => {
                    if let Some((arc_l, arc_r)) = self.unify_var_pair(memo, id_l, id_r)? {
                        work.push((
                            memo.to_flat_side(arc_l.as_ref()),
                            memo.to_flat_side(arc_r.as_ref()),
                        ));
                    }
                },
                | (FlatSide::Var(id), FlatSide::Node(tree, idx)) => {
                    if tree.is_error(idx) {
                        continue;
                    }
                    let (root, opt_arc) = self.follow_type_arc(id);
                    match opt_arc {
                        | Some(arc) => {
                            let ptr_r = tree.ptr(idx);
                            if Arc::as_ptr(&arc).addr() == ptr_r
                                || !memo
                                    .visited
                                    .insert((NodeKey::Var(root), NodeKey::Ptr(ptr_r)))
                            {
                                continue;
                            }
                            work.push((memo.to_flat_side(arc.as_ref()), FlatSide::Node(tree, idx)));
                        },
                        | None => {
                            let ty = unflatten_type(&tree, idx);
                            self.bind_var_non_var(root, BindTarget::Borrowed(&ty))?;
                        },
                    }
                },
                | (FlatSide::Node(tree, idx), FlatSide::Var(id)) => {
                    if tree.is_error(idx) {
                        continue;
                    }
                    let (root, opt_arc) = self.follow_type_arc(id);
                    match opt_arc {
                        | Some(arc) => {
                            let ptr_l = tree.ptr(idx);
                            if Arc::as_ptr(&arc).addr() == ptr_l
                                || !memo
                                    .visited
                                    .insert((NodeKey::Ptr(ptr_l), NodeKey::Var(root)))
                            {
                                continue;
                            }
                            work.push((FlatSide::Node(tree, idx), memo.to_flat_side(arc.as_ref())));
                        },
                        | None => {
                            let ty = unflatten_type(&tree, idx);
                            self.bind_var_non_var(root, BindTarget::Borrowed(&ty))?;
                        },
                    }
                },
                | (FlatSide::Node(tl, mut il), FlatSide::Node(tr, mut ir)) => {
                    loop {
                        let ptr_l = tl.ptr(il);
                        let ptr_r = tr.ptr(ir);
                        if ptr_l == ptr_r || tl.is_error(il) || tr.is_error(ir) {
                            break;
                        }
                        if tl.is_var(il) || tr.is_var(ir) {
                            work.push((FlatSide::Node(tl, il), FlatSide::Node(tr, ir)));
                            break;
                        }
                        let (Some(nl), Some(nr)) = (tl.nodes.get(il), tr.nodes.get(ir)) else {
                            break;
                        };
                        match (nl, nr) {
                            | (
                                FlatType::Tensor { shape: sa, elem: ea },
                                FlatType::Tensor { shape: sb, elem: eb },
                            ) => {
                                if !memo
                                    .visited
                                    .insert((NodeKey::Ptr(ptr_l), NodeKey::Ptr(ptr_r)))
                                {
                                    break;
                                }
                                self.unify_shape_memo(memo, sa, sb)?;
                                il = *ea;
                                ir = *eb;
                            },
                            | (FlatType::Io(a), FlatType::Io(b))
                            | (FlatType::TypeRep(a), FlatType::TypeRep(b))
                            | (FlatType::FromValue(a), FlatType::FromValue(b)) => {
                                if !memo
                                    .visited
                                    .insert((NodeKey::Ptr(ptr_l), NodeKey::Ptr(ptr_r)))
                                {
                                    break;
                                }
                                il = *a;
                                ir = *b;
                            },
                            | (
                                FlatType::Fn { params: pa, ret: ra },
                                FlatType::Fn { params: pb, ret: rb },
                            ) => {
                                if !memo
                                    .visited
                                    .insert((NodeKey::Ptr(ptr_l), NodeKey::Ptr(ptr_r)))
                                {
                                    break;
                                }
                                if pa.len() != pb.len() {
                                    return Err(UnifyError::Mismatch {
                                        left: self.resolve(&unflatten_type(&tl, il)),
                                        right: self.resolve(&unflatten_type(&tr, ir)),
                                    });
                                }
                                if pa.is_empty() {
                                    il = *ra;
                                    ir = *rb;
                                    continue;
                                }
                                work.push((
                                    FlatSide::Node(Arc::clone(&tl), *ra),
                                    FlatSide::Node(Arc::clone(&tr), *rb),
                                ));
                                for (&a, &b) in pa.iter().zip(pb.iter()).rev() {
                                    work.push((
                                        FlatSide::Node(Arc::clone(&tl), a),
                                        FlatSide::Node(Arc::clone(&tr), b),
                                    ));
                                }
                                break;
                            },
                            | (
                                FlatType::Adt { id: ia, args: aa },
                                FlatType::Adt { id: ib, args: ab },
                            ) => {
                                if !memo
                                    .visited
                                    .insert((NodeKey::Ptr(ptr_l), NodeKey::Ptr(ptr_r)))
                                {
                                    break;
                                }
                                if !crate::adt::same_type(*ia, *ib) || aa.len() != ab.len() {
                                    return Err(UnifyError::Mismatch {
                                        left: self.resolve(&unflatten_type(&tl, il)),
                                        right: self.resolve(&unflatten_type(&tr, ir)),
                                    });
                                }
                                if aa.len() == 1
                                    && let (Some(&a), Some(&b)) = (aa.first(), ab.first())
                                {
                                    il = a;
                                    ir = b;
                                    continue;
                                }
                                for (&a, &b) in aa.iter().zip(ab.iter()).rev() {
                                    work.push((
                                        FlatSide::Node(Arc::clone(&tl), a),
                                        FlatSide::Node(Arc::clone(&tr), b),
                                    ));
                                }
                                break;
                            },
                            | (
                                FlatType::Direct(Type::Record(ra)),
                                FlatType::Direct(Type::Record(rb)),
                            ) => {
                                if !memo
                                    .visited
                                    .insert((NodeKey::Ptr(ptr_l), NodeKey::Ptr(ptr_r)))
                                {
                                    break;
                                }
                                self.unify_row_memo(memo, ra, rb, 64)?;
                                break;
                            },
                            | (FlatType::Direct(la), FlatType::Direct(rb)) => {
                                self.unify_leaf_types(la, rb)?;
                                break;
                            },
                            | _ => {
                                return Err(UnifyError::Mismatch {
                                    left: self.resolve(&unflatten_type(&tl, il)),
                                    right: self.resolve(&unflatten_type(&tr, ir)),
                                });
                            },
                        }
                    }
                },
            }
        }
        Ok(())
    }

    /// Unifies two dimensions.
    ///
    /// # Errors
    ///
    /// Returns the residual mismatch from [`DimUnifier::unify`].
    pub fn unify_dim(
        &mut self,
        left: &Dim,
        right: &Dim,
    ) -> Result<(), UnifyError> {
        let left = self.dims.resolve(left);
        let right = self.dims.resolve(right);
        if left == right {
            return Ok(());
        }
        // Record the levels of the variables that this unification may bind.
        let before = left.vars();
        for id in before {
            let level = self.level;
            self.dim_levels.entry(id).or_insert(level);
        }
        self.dims.unify(&left, &right).map_err(UnifyError::Dim)?;
        self.propagate_weak_dims();
        Ok(())
    }

    /// A weak variable that unified with another variable makes that variable
    /// weak as well: the solution of a weak variable may not be generalised.
    fn propagate_weak_dims(&mut self) {
        if self.weak.is_empty() {
            return;
        }
        loop {
            let mut grown = false;
            let weak: Vec<u32> = self.weak.iter().copied().collect();
            for id in weak {
                for var in self.dims.resolve(&Dim::var(id)).vars() {
                    if self.weak.insert(var) {
                        grown = true;
                    }
                }
            }
            if !grown {
                return;
            }
        }
    }

    /// Unifies two representations.
    ///
    /// # Errors
    ///
    /// Fails when both sides are concrete and different, or when a literal
    /// cannot take the requested representation.
    /// Records `variable ⊒ other`: a lower bound, not a binding (§3.4.1).
    ///
    /// `join(ρ, Int)` must not make `ρ` an `Int` — that would turn
    /// `fn f(x) = x + 1` into `Int -> Int` and silently accept `x + 2.0`.
    /// The bound is resolved when the variable is bound or defaulted.
    ///
    /// # Errors
    ///
    /// Fails when both sides are concrete and `variable` is below `other`, or
    /// when a literal variable has no reading above `other`.
    pub fn constrain_rep(
        &mut self,
        variable: Rep,
        other: Rep,
    ) -> Result<(), UnifyError> {
        self.constrain_rep_inner(variable, other)?;
        let constraint = NumericConstraint::Relation(RepRelation::AtLeast {
            upper: variable,
            lower: other,
        });
        if !self.contains_numeric(&constraint) {
            self.push_numeric(&constraint);
        }
        self.propagate_numeric()
    }

    fn constrain_rep_inner(
        &mut self,
        variable: Rep,
        other: Rep,
    ) -> Result<(), UnifyError> {
        let variable = self.resolve_rep(variable);
        let other = self.resolve_rep(other);
        if variable == other {
            return Ok(());
        }
        match (variable, other) {
            | (Rep::Var(id), lower) => {
                if let Some(kind) = self.rep_literals.get(&id).copied() {
                    if let Rep::Var(_) = lower {
                        // Deferred: the other variable carries a literal
                        // constraint or none at all.
                    } else if !kind.accepts_rep(lower) {
                        return Err(UnifyError::LiteralMismatch {
                            kind,
                            requested: Type::Scalar(lower, Dim::one()),
                        });
                    }
                }
                let bounds = self.rep_lower.entry(id).or_default();
                if !bounds.contains(&lower) {
                    bounds.push(lower);
                }
                Ok(())
            },
            | (concrete, lower) => {
                if concrete.at_least(lower) {
                    Ok(())
                } else {
                    Err(UnifyError::Mismatch {
                        left: Type::Scalar(concrete, Dim::one()),
                        right: Type::Scalar(lower, Dim::one()),
                    })
                }
            },
        }
    }

    /// The lower bounds recorded for a representation variable.
    #[must_use]
    pub fn rep_bounds(
        &self,
        rep: Rep,
    ) -> Vec<Rep> {
        match self.resolve_rep(rep) {
            | Rep::Var(id) => self.rep_lower.get(&id).cloned().unwrap_or_default(),
            | _ => Vec::new(),
        }
    }

    /// Equates two representations and propagates the resulting constraints.
    ///
    /// # Errors
    ///
    /// Returns a mismatch for different concrete representations or a numeric conflict.
    pub fn unify_rep(
        &mut self,
        left: Rep,
        right: Rep,
    ) -> Result<(), UnifyError> {
        self.unify_rep_inner(left, right)?;
        self.propagate_numeric()
    }

    fn unify_rep_inner(
        &mut self,
        left: Rep,
        right: Rep,
    ) -> Result<(), UnifyError> {
        let left = self.resolve_rep(left);
        let right = self.resolve_rep(right);
        if left == right {
            return Ok(());
        }
        match (left, right) {
            // Keep the *literal* variable alive when one side is literal-marked:
            // it carries the defaulting rule (`1.0` defaults to `Float`), and a
            // binding the other way round loses it — the merged variable then
            // generalises instead of defaulting, so `let g = sqrt(dyn(4.0 m^2))`
            // became `∀ρ. Dyn<ρ>` instead of `Dyn<Float>`.
            | (Rep::Var(a), Rep::Var(b)) => {
                let a_literal = self.rep_literals.contains_key(&a);
                let b_literal = self.rep_literals.contains_key(&b);
                if a_literal && !b_literal {
                    self.bind_rep(b, Rep::Var(a))
                } else {
                    self.bind_rep(a, Rep::Var(b))
                }
            },
            | (Rep::Var(id), other) | (other, Rep::Var(id)) => {
                if std::env::var_os("AEVIA_TRACE_REP").is_some() {
                    eprintln!(
                        "bind r{id} := {other:?}
{}",
                        std::backtrace::Backtrace::force_capture()
                    );
                }
                self.bind_rep(id, other)
            },
            | _ => {
                Err(UnifyError::Mismatch {
                    left: Type::Scalar(left, Dim::one()),
                    right: Type::Scalar(right, Dim::one()),
                })
            },
        }
    }

    fn bind_rep(
        &mut self,
        id: u32,
        value: Rep,
    ) -> Result<(), UnifyError> {
        if let Rep::Var(other) = value {
            if other == id {
                return Ok(());
            }
            // Do not copy a literal constraint onto the other variable: the
            // merged variable is also used by a non-literal operand, so it must
            // keep defaulting by the general rule (`Real`), not by `Int`.
            // `1 + 2` keeps its constraint because both sides are literals and
            // the target already carries it.
            for bound in self.rep_lower.get(&id).cloned().unwrap_or_default() {
                let bounds = self.rep_lower.entry(other).or_default();
                if !bounds.contains(&bound) {
                    bounds.push(bound);
                }
            }
            if self.weak.contains(&id) {
                self.weak.insert(other);
            }
            if self.composite_literals.contains(&id) {
                self.composite_literals.insert(other);
            }
            let level = self.rep_levels.get(&id).copied().unwrap_or(0);
            self.rep_subst.insert(id, Rep::Var(other));
            self.numeric_changed(id);
            self.rep_levels.insert(
                other,
                level.min(self.rep_levels.get(&other).copied().unwrap_or(u32::MAX)),
            );
            return Ok(());
        }
        // A concrete value must be above every concrete bound.
        for bound in self.rep_bounds(Rep::Var(id)) {
            let bound = self.resolve_rep(bound);
            if let Rep::Var(_) = bound {
                // Unresolved: verified once both ends are concrete.
                continue;
            }
            if !value.at_least(bound) {
                return Err(UnifyError::Mismatch {
                    left: Type::Scalar(value, Dim::one()),
                    right: Type::Scalar(bound, Dim::one()),
                });
            }
        }
        if let Some(kind) = self.rep_literals.get(&id).copied()
            && !kind.accepts_rep(value)
        {
            return Err(UnifyError::LiteralMismatch {
                kind,
                requested: Type::Scalar(value, Dim::one()),
            });
        }
        let level = self.rep_levels.get(&id).copied().unwrap_or(0);
        self.rep_subst.insert(id, value);
        self.numeric_changed(id);
        self.lower_rep_levels(value, level);
        Ok(())
    }

    #[allow(
        clippy::single_call_fn,
        reason = "binds two unbound root type variables while preserving literal/weak invariants"
    )]
    /// Lowers the level recorded for a representation variable.
    fn lower_rep_levels(
        &mut self,
        rep: Rep,
        level: u32,
    ) {
        if let Rep::Var(id) = rep {
            let entry = self.rep_levels.entry(id).or_insert(level);
            *entry = (*entry).min(level);
        }
    }

    fn bind_var_alias(
        &mut self,
        id: u32,
        other: u32,
    ) {
        if other == id {
            return;
        }
        if let Some(kind) = self.literals.get(&id).copied() {
            self.literals.entry(other).or_insert(kind);
        }
        if self.weak.contains(&id) {
            self.weak.insert(other);
        }
        let level = self.levels.get(&id).copied().unwrap_or(0);
        let entry = self.levels.entry(other).or_insert(level);
        *entry = (*entry).min(level);
        self.subst.insert(id, Arc::new(Type::Var(other)));
        self.numeric_changed(id);
    }

    fn bind_var_non_var(
        &mut self,
        id: u32,
        target: BindTarget<'_>,
    ) -> Result<(), UnifyError> {
        let ty = target.as_type();
        if self.occurs(id, Occurrence::Type(ty)) {
            return Err(UnifyError::Occurs(id));
        }
        if let Some(kind) = self.literals.get(&id).copied() {
            match ty {
                | Type::Scalar(rep, dim) => {
                    if !kind.is_quantity() && !dim.is_one() {
                        let resolved_dim = self.dims.resolve(dim);
                        // A bare literal is dimensionless: an unknown dimension
                        // is forced to 1, a concrete one is an error.
                        if !resolved_dim.is_one()
                            && (resolved_dim.is_concrete()
                                || self.dims.unify(&resolved_dim, &Dim::one()).is_err())
                        {
                            return Err(UnifyError::LiteralMismatch {
                                kind,
                                requested: self.resolve(ty),
                            });
                        }
                    }
                    let rep = self.resolve_rep(*rep);
                    if let Rep::Var(rep_id) = rep {
                        self.rep_literals.entry(rep_id).or_insert(kind);
                    } else if !kind.accepts_rep(rep) {
                        return Err(UnifyError::LiteralMismatch {
                            kind,
                            requested: self.resolve(ty),
                        });
                    }
                },
                | _ => {
                    return Err(UnifyError::LiteralMismatch {
                        kind,
                        requested: self.resolve(ty),
                    });
                },
            }
        }
        let level = self.levels.get(&id).copied().unwrap_or(0);
        let mut numeric_roots = HashSet::new();
        self.lower_levels_iter(Occurrence::Type(ty), level, Some(&mut numeric_roots));
        if !numeric_roots.is_empty() {
            let constraints = self.numeric_component(numeric_roots);
            for constraint in constraints {
                for related in constraint.types() {
                    self.lower_levels_iter(Occurrence::Type(&related), level, None);
                }
            }
        }
        self.subst.insert(id, target.into_arc());
        self.numeric_changed(id);
        Ok(())
    }

    /// Checks both type and row bindings without resolving or cloning entire
    /// subtrees. The work stack also covers rows wider than the resolution cap.
    fn occurs(
        &self,
        id: u32,
        root: Occurrence<'_>,
    ) -> bool {
        let mut pending = vec![root];
        let mut visited = HashSet::new();
        while let Some(node) = pending.pop() {
            match node {
                | Occurrence::Type(Type::Var(other)) => {
                    if *other == id {
                        return true;
                    }
                    if visited.insert((false, *other))
                        && let Some(bound) = self.subst.get(other)
                    {
                        pending.push(Occurrence::Type(bound.as_ref()));
                    }
                },
                | Occurrence::Row(Row::Var(other)) => {
                    if *other == id {
                        return true;
                    }
                    if visited.insert((true, *other))
                        && let Some(bound) = self.row_subst.get(other)
                    {
                        pending.push(Occurrence::Row(bound));
                    }
                },
                | Occurrence::Type(
                    Type::Tensor { elem: inner, .. }
                    | Type::Io(inner)
                    | Type::TypeRepIntrinsic(Some(inner))
                    | Type::FromValueIntrinsic(Some(inner)),
                ) => pending.push(Occurrence::Type(inner)),
                | Occurrence::Type(Type::Fn(params, ret)) => {
                    pending.push(Occurrence::Type(ret));
                    pending.extend(params.iter().map(Occurrence::Type));
                },
                | Occurrence::Type(Type::Adt(_, args)) => {
                    pending.extend(args.iter().map(Occurrence::Type));
                },
                | Occurrence::Type(Type::Record(row)) => pending.push(Occurrence::Row(row)),
                | Occurrence::Type(Type::Scalar(_, dim) | Type::Point(dim) | Type::UnitOf(dim))
                    if !dim.is_concrete() && self.resolve_dim(dim).contains_var(id) =>
                {
                    return true;
                },
                | Occurrence::Row(Row::Extend { ty, tail, .. }) => {
                    pending.push(Occurrence::Row(tail));
                    pending.push(Occurrence::Type(ty));
                },
                | _ => {},
            }
        }
        false
    }

    #[allow(
        clippy::cognitive_complexity,
        reason = "iterative DAG-safe level lowering across Type, Row, Rep, Dim and Nat tables"
    )]
    fn lower_levels_iter(
        &mut self,
        root: Occurrence<'_>,
        level: u32,
        mut collect_numeric: Option<&mut HashSet<u32>>,
    ) {
        let subst = &self.subst;
        let row_subst = &self.row_subst;
        let rep_subst = &self.rep_subst;
        let dims = &self.dims;
        let nat_subst = &self.nat_subst;
        let levels = &mut self.levels;
        let rep_levels = &mut self.rep_levels;
        let dim_levels = &mut self.dim_levels;
        let nat_levels = &mut self.nat_levels;
        let row_levels = &mut self.row_levels;

        let mut pending = vec![root];
        let mut visited_vars = HashSet::new();
        let mut visited_ptrs = HashSet::new();
        let mut visited_nats = HashSet::new();
        let mut pending_nats = Vec::new();

        while let Some(node) = pending.pop() {
            match node {
                | Occurrence::Type(ty) => {
                    let mut current = ty;
                    loop {
                        match current {
                            | Type::Var(id) => {
                                let entry = levels.entry(*id).or_insert(level);
                                *entry = (*entry).min(level);
                                if let Some(roots) = collect_numeric.as_mut() {
                                    roots.insert(*id);
                                }
                                if visited_vars.insert((false, *id))
                                    && let Some(bound) = subst.get(id)
                                {
                                    current = bound.as_ref();
                                    continue;
                                }
                            },
                            | Type::Scalar(rep, dim) => {
                                let resolved_rep = resolve_rep_table(rep_subst, *rep);
                                if let Rep::Var(rep_id) = resolved_rep {
                                    let entry = rep_levels.entry(rep_id).or_insert(level);
                                    *entry = (*entry).min(level);
                                    if let Some(roots) = collect_numeric.as_mut() {
                                        roots.insert(rep_id);
                                    }
                                }
                                if !dim.is_concrete() {
                                    for id in dims.resolve(dim).vars() {
                                        let entry = dim_levels.entry(id).or_insert(level);
                                        *entry = (*entry).min(level);
                                    }
                                }
                            },
                            | Type::Tensor { shape, elem } => {
                                if !visited_ptrs.insert(std::ptr::from_ref(current).addr()) {
                                    break;
                                }
                                if let Shape::Refined(extents) = shape {
                                    for extent in extents.iter().flatten() {
                                        if nat_subst.is_empty() {
                                            for id in extent.vars() {
                                                let entry = nat_levels.entry(id).or_insert(level);
                                                *entry = (*entry).min(level);
                                            }
                                        } else {
                                            pending_nats.push(extent);
                                        }
                                    }
                                    while let Some(nat) = pending_nats.pop() {
                                        match nat {
                                            | Nat::Lit(_) => {},
                                            | Nat::Var(id) => {
                                                let entry = nat_levels.entry(*id).or_insert(level);
                                                *entry = (*entry).min(level);
                                                if visited_nats.insert(*id)
                                                    && let Some(bound) = nat_subst.get(id)
                                                {
                                                    pending_nats.push(bound);
                                                }
                                            },
                                            | Nat::Add(a, b) | Nat::Mul(a, b) => {
                                                pending_nats.push(a);
                                                pending_nats.push(b);
                                            },
                                        }
                                    }
                                }
                                current = elem;
                                continue;
                            },
                            | Type::Fn(params, ret) => {
                                if !visited_ptrs.insert(std::ptr::from_ref(current).addr()) {
                                    break;
                                }
                                for param in params {
                                    pending.push(Occurrence::Type(param));
                                }
                                current = ret;
                                continue;
                            },
                            | Type::Record(row) => {
                                pending.push(Occurrence::Row(row));
                            },
                            | Type::Adt(_, args) => {
                                if !visited_ptrs.insert(std::ptr::from_ref(current).addr()) {
                                    break;
                                }
                                for arg in args {
                                    pending.push(Occurrence::Type(arg));
                                }
                            },
                            | Type::Io(inner)
                            | Type::TypeRepIntrinsic(Some(inner))
                            | Type::FromValueIntrinsic(Some(inner)) => {
                                if !visited_ptrs.insert(std::ptr::from_ref(current).addr()) {
                                    break;
                                }
                                current = inner;
                                continue;
                            },
                            | Type::TensorIntrinsic(intrinsic) => {
                                if let Some(rep) = intrinsic.element
                                    && let Rep::Var(rep_id) = resolve_rep_table(rep_subst, rep)
                                {
                                    let entry = rep_levels.entry(rep_id).or_insert(level);
                                    *entry = (*entry).min(level);
                                    if let Some(roots) = collect_numeric.as_mut() {
                                        roots.insert(rep_id);
                                    }
                                }
                            },
                            | Type::Dyn(rep) => {
                                if let Rep::Var(rep_id) = resolve_rep_table(rep_subst, *rep) {
                                    let entry = rep_levels.entry(rep_id).or_insert(level);
                                    *entry = (*entry).min(level);
                                    if let Some(roots) = collect_numeric.as_mut() {
                                        roots.insert(rep_id);
                                    }
                                }
                            },
                            | Type::Point(dim) | Type::UnitOf(dim) => {
                                if !dim.is_concrete() {
                                    for id in dims.resolve(dim).vars() {
                                        let entry = dim_levels.entry(id).or_insert(level);
                                        *entry = (*entry).min(level);
                                    }
                                }
                            },
                            | _ => {},
                        }
                        break;
                    }
                },
                | Occurrence::Row(row) => {
                    let mut current = row;
                    loop {
                        match current {
                            | Row::Empty => break,
                            | Row::Var(id) => {
                                let entry = row_levels.entry(*id).or_insert(level);
                                *entry = (*entry).min(level);
                                if visited_vars.insert((true, *id))
                                    && let Some(bound) = row_subst.get(id)
                                {
                                    current = bound;
                                    continue;
                                }
                                break;
                            },
                            | Row::Extend { ty, tail, .. } => {
                                pending.push(Occurrence::Type(ty));
                                current = tail;
                            },
                        }
                    }
                },
            }
        }
    }

    fn unify_shape_memo(
        &mut self,
        memo: &mut UnifyMemo,
        left: &Shape,
        right: &Shape,
    ) -> Result<(), UnifyError> {
        if std::ptr::eq(left, right) {
            return Ok(());
        }
        if let (Shape::Rank(a), Shape::Rank(b)) = (left, right)
            && a == b
        {
            return Ok(());
        }
        let key = (
            std::ptr::from_ref(left).addr(),
            std::ptr::from_ref(right).addr(),
        );
        if !memo.visited_shapes.insert(key) {
            return Ok(());
        }
        self.unify_shape(left, right)
    }

    /// Unifies two shapes.
    ///
    /// A refined shape and a same-rank rank-only shape describe the same
    /// tensor (§3.5.1/§3.5.3): a value with a known extent is usable where
    /// only the rank is known, and unifying a `Tensor<[2], τ>` with a
    /// `Tensor<1, τ>` (or the reverse order — elementwise ops check
    /// left/right arbitrarily) must succeed. Rank mismatch and two different
    /// extents stay errors.
    ///
    /// # Errors
    ///
    /// Fails when the ranks differ or when two known extents differ.
    pub fn unify_shape(
        &mut self,
        left: &Shape,
        right: &Shape,
    ) -> Result<(), UnifyError> {
        if std::ptr::eq(left, right) {
            return Ok(());
        }
        match (left, right) {
            | (Shape::Rank(a), Shape::Rank(b)) if a == b => Ok(()),
            // Same rank, one side knows the extents — safe in either direction.
            | (Shape::Refined(items), Shape::Rank(rank))
            | (Shape::Rank(rank), Shape::Refined(items)) => {
                if usize::try_from(*rank).unwrap_or(usize::MAX) == items.len() {
                    Ok(())
                } else {
                    Err(UnifyError::ShapeMismatch {
                        left: left.clone(),
                        right: right.clone(),
                    })
                }
            },
            | (Shape::Refined(a), Shape::Refined(b)) => {
                if a.len() != b.len() {
                    return Err(UnifyError::ShapeMismatch {
                        left: left.clone(),
                        right: right.clone(),
                    });
                }
                for (x, y) in a.iter().zip(b.iter()) {
                    match (x, y) {
                        | (Some(p), Some(q)) if p == q => {},
                        // Two literal extents that differ are a shape error
                        // the reader can see (`[2]` against `[3]`), not the
                        // solver's normalised `0 = 1`.
                        | (Some(Nat::Lit(p)), Some(Nat::Lit(q))) if p != q => {
                            return Err(UnifyError::ShapeMismatch {
                                left: left.clone(),
                                right: right.clone(),
                            });
                        },
                        | (Some(x), Some(y)) => self.unify_nat(x, y)?,
                        // `?` stays compatible with any extent (§3.5.1).
                        | _ => {},
                    }
                }
                Ok(())
            },
            | _ => {
                Err(UnifyError::ShapeMismatch {
                    left: left.clone(),
                    right: right.clone(),
                })
            },
        }
    }

    /// The shape of an elementwise operation on two tensors of the same rank
    /// (§3.5.3): each axis must be equal or one side must be `1`, which
    /// broadcasts (`[3, 1] + [1, 4]` is `[3, 4]`).
    ///
    /// Two extents where neither is the literal `1` are unified (so a symbolic
    /// extent is assumed to equal the other side); `[n, 1] + [1, m]` is
    /// `[n, m]` with no constraint on `n` and `m`. A rank-only operand has
    /// unknown extents: they can only be `1` or equal to the other side, so the
    /// other side's extent wins unless that is `1` too.
    ///
    /// # Errors
    ///
    /// [`UnifyError::ShapeMismatch`] for different ranks or two different
    /// literal extents (neither `1`), and the Nat errors of [`Self::unify_nat`].
    pub fn broadcast_shape(
        &mut self,
        left: &Shape,
        right: &Shape,
    ) -> Result<Shape, UnifyError> {
        let left = self.resolve_shape(left);
        let right = self.resolve_shape(right);
        let rank = left.rank();
        if rank != right.rank() {
            return Err(UnifyError::ShapeMismatch { left, right });
        }
        let one = |nat: &Nat| matches!(nat, Nat::Lit(1));
        let out = match (left, right) {
            | (Shape::Rank(_), Shape::Rank(_)) => return Ok(Shape::Rank(rank)),
            | (Shape::Rank(_), Shape::Refined(mut axes))
            | (Shape::Refined(mut axes), Shape::Rank(_)) => {
                for axis in &mut axes {
                    if axis.as_ref().is_some_and(one) {
                        *axis = None;
                    }
                }
                axes
            },
            | (Shape::Refined(left), Shape::Refined(right)) => {
                let mut out = Vec::with_capacity(left.len());
                for (x, y) in left.iter().zip(&right) {
                    let extent = match (x, y) {
                        | (None, None) => None,
                        | (None, Some(known)) | (Some(known), None) => {
                            (!one(known)).then(|| known.clone())
                        },
                        | (Some(p), Some(q)) => {
                            let (p, q) = (self.resolve_nat(p), self.resolve_nat(q));
                            if p == q || one(&q) {
                                Some(p)
                            } else if one(&p) {
                                Some(q)
                            } else if matches!((&p, &q), (Nat::Lit(_), Nat::Lit(_))) {
                                return Err(UnifyError::ShapeMismatch {
                                    left: Shape::Refined(left),
                                    right: Shape::Refined(right),
                                });
                            } else {
                                self.unify_nat(&p, &q)?;
                                Some(self.resolve_nat(&p))
                            }
                        },
                    };
                    out.push(extent);
                }
                out
            },
        };
        if out.iter().all(Option::is_none) {
            Ok(Shape::Rank(rank))
        } else {
            Ok(Shape::Refined(out))
        }
    }

    fn unify_row_memo(
        &mut self,
        memo: &mut UnifyMemo,
        left: &Row,
        right: &Row,
        depth: u32,
    ) -> Result<(), UnifyError> {
        let left_tail = left.tail_var();
        let right_tail = right.tail_var();
        let right_fields: HashMap<_, _> = right.iter_fields().collect();
        let mut left_labels = HashSet::new();
        let mut left_only = Vec::new();
        for (label, ty) in left.iter_fields() {
            left_labels.insert(label);
            if let Some(other) = right_fields.get(label) {
                self.unify_pair_rec(memo, ty, other, depth.saturating_add(1))?;
            } else if right_tail.is_some() {
                left_only.push((label, ty));
            } else {
                return Err(UnifyError::MissingField {
                    field: label.to_string(),
                    row: right.clone(),
                });
            }
        }

        // Unifying field types can itself bind one of the row tails.
        if [left_tail, right_tail]
            .into_iter()
            .flatten()
            .any(|tail| self.row_subst.contains_key(&tail))
        {
            return self.unify_row_memo(
                memo,
                &self.resolve_row(left),
                &self.resolve_row(right),
                depth.saturating_add(1),
            );
        }

        let mut right_only = Vec::new();
        for (label, ty) in right.iter_fields() {
            if left_labels.contains(label) {
                continue;
            }
            if left_tail.is_none() {
                return Err(UnifyError::MissingField {
                    field: label.to_string(),
                    row: left.clone(),
                });
            }
            right_only.push((label, ty));
        }

        if left_tail.is_some()
            && left_tail == right_tail
            && let Some((label, _)) = left_only.first().or_else(|| right_only.first())
        {
            return Err(UnifyError::DuplicateField {
                field: (*label).to_string(),
            });
        }
        for (row, tail) in [(left, left_tail), (right, right_tail)] {
            if let Some(tail) = tail {
                self.lacks
                    .entry(tail)
                    .or_default()
                    .extend(row.iter_fields().map(|(label, _)| label.to_string()));
            }
        }

        let close_over = |fields: Vec<(&str, &Type)>, tail| {
            extend_row_fields(
                fields
                    .into_iter()
                    .rev()
                    .map(|(label, ty)| (label.to_string(), ty.clone())),
                tail,
            )
        };
        match (left_tail, right_tail) {
            | (Some(a), Some(b)) if a != b => {
                if left_only.is_empty() && right_only.is_empty() {
                    self.bind_row(a, &Row::Var(b))
                } else {
                    // The two residual rows share one fresh tail. Insert the
                    // unmatched fields in batches, retaining source order.
                    let shared = self.fresh_row();
                    self.bind_row(a, &close_over(right_only, shared.clone()))?;
                    self.bind_row(b, &close_over(left_only, shared))
                }
            },
            | (Some(tail), None) => self.bind_row(tail, &close_over(right_only, Row::Empty)),
            | (None, Some(tail)) => self.bind_row(tail, &close_over(left_only, Row::Empty)),
            | _ => Ok(()),
        }
    }

    /// Records that a row variable must not contain `label` (§3.6.1).
    ///
    /// # Errors
    ///
    /// Fails when the condition is already violated — the row would end up
    /// with two fields of the same name, which is `E0208`.
    pub fn require_lacks(
        &mut self,
        id: u32,
        label: &str,
    ) -> Result<(), UnifyError> {
        let row = self.resolve_row(&Row::Var(id));
        if row.field(label).is_some() {
            return Err(UnifyError::DuplicateField {
                field: label.to_string(),
            });
        }
        self.lacks.entry(id).or_default().insert(label.to_string());
        if let Some(tail) = row.tail_var()
            && tail != id
        {
            self.lacks
                .entry(tail)
                .or_default()
                .insert(label.to_string());
        }
        Ok(())
    }

    /// The labels a row variable must not contain, in insertion order.
    #[must_use]
    pub fn lacks_of(
        &self,
        id: u32,
    ) -> Vec<String> {
        let row = self.resolve_row(&Row::Var(id));
        let id = row_id(&row).unwrap_or(id);
        self.lacks
            .get(&id)
            .map_or_else(Vec::new, |labels| labels.iter().cloned().collect())
    }

    /// Extends an open row with a new field.
    ///
    /// # Errors
    ///
    /// Fails when the field already exists, the tail must lack the field, or
    /// the row is closed.
    pub fn extend_row(
        &mut self,
        tail: u32,
        label: &str,
        ty: Type,
    ) -> Result<(), UnifyError> {
        let row = self.resolve_row(&Row::Var(tail));
        if row.field(label).is_some() {
            return Err(UnifyError::DuplicateField {
                field: label.to_string(),
            });
        }
        let Some(tail) = row.tail_var() else {
            return Err(UnifyError::MissingField {
                field: label.to_string(),
                row,
            });
        };
        let fresh = self.fresh_row();
        self.bind_row(
            tail,
            &Row::Extend {
                label: label.to_string(),
                ty: Box::new(ty),
                tail: Box::new(fresh),
            },
        )
    }

    fn bind_row(
        &mut self,
        id: u32,
        row: &Row,
    ) -> Result<(), UnifyError> {
        let row = self.resolve_row(row);
        if row_id(&row) == Some(id) {
            return Ok(());
        }
        if self.occurs(id, Occurrence::Row(&row)) {
            return Err(UnifyError::Occurs(id));
        }

        let mut labels = self.lacks.get(&id).cloned().unwrap_or_default();
        for (label, _) in row.iter_fields() {
            if !labels.insert(label.to_string()) {
                return Err(UnifyError::DuplicateField {
                    field: label.to_string(),
                });
            }
        }
        if let Some(tail) = row.tail_var() {
            self.lacks.entry(tail).or_default().extend(labels);
            if self.weak.contains(&id) {
                self.weak.insert(tail);
            }
        }
        let level = self.row_levels.get(&id).copied().unwrap_or(0);
        self.lower_levels_iter(Occurrence::Row(&row), level, None);
        self.row_subst.insert(id, row);
        Ok(())
    }

    /// Instantiates a scheme with fresh variables at the current level.
    pub fn instantiate(
        &mut self,
        scheme: &Scheme,
    ) -> Type {
        self.instantiate_scheme(scheme).0
    }

    /// Instantiates a scheme, also instantiating its class constraints.
    ///
    /// The constraints come back with the same fresh variables, so the caller
    /// can solve them against the instance table (§3.8.2).
    pub fn instantiate_scheme(
        &mut self,
        scheme: &Scheme,
    ) -> (Type, Vec<Constraint>) {
        let (ty, constraints, _) = self.instantiate_scheme_with_bindings(scheme);
        (ty, constraints)
    }

    /// Instantiates a signature and exposes its ordered variable correspondence.
    pub fn instantiate_scheme_with_bindings(
        &mut self,
        scheme: &Scheme,
    ) -> (Type, Vec<Constraint>, crate::TypeSubstitution) {
        if scheme.is_closed() {
            return (
                scheme.ty.clone(),
                Vec::new(),
                crate::TypeSubstitution::default(),
            );
        }
        let mut type_map = HashMap::new();
        for id in &scheme.vars {
            let fresh = self.fresh_id();
            self.levels.insert(fresh, self.level);
            // A scheme's shared variable list also quantifies representation
            // variables. Keep their level so a first-class alias can be
            // generalized again instead of silently defaulting its Rep.
            self.rep_levels.insert(fresh, self.level);
            self.numeric_universals.insert(fresh);
            type_map.insert(*id, fresh);
        }
        let mut dim_map = HashMap::new();
        for id in &scheme.dims {
            let fresh = self.fresh_id();
            self.dim_levels.insert(fresh, self.level);
            dim_map.insert(*id, fresh);
        }
        let mut row_map = HashMap::new();
        for id in &scheme.rows {
            let fresh = self.fresh_id();
            self.row_levels.insert(fresh, self.level);
            row_map.insert(*id, fresh);
        }
        for (id, labels) in &scheme.row_lacks {
            if let Some(fresh) = row_map.get(id) {
                self.lacks
                    .entry(*fresh)
                    .or_default()
                    .extend(labels.iter().cloned());
            }
        }
        let mut nat_map = HashMap::new();
        for id in &scheme.nats {
            let fresh = self.fresh_id();
            self.nat_levels.insert(fresh, self.level);
            nat_map.insert(*id, Nat::Var(fresh));
        }
        let bindings = crate::TypeSubstitution {
            types: type_map
                .iter()
                .map(|(id, fresh)| (*id, Type::Var(*fresh)))
                .collect(),
            representations: type_map
                .iter()
                .map(|(id, fresh)| (*id, Rep::Var(*fresh)))
                .collect(),
            dimensions: dim_map
                .iter()
                .map(|(id, fresh)| (*id, Dim::var(*fresh)))
                .collect(),
            rows: row_map
                .iter()
                .map(|(id, fresh)| (*id, Row::Var(*fresh)))
                .collect(),
            naturals: nat_map,
        };
        let ty = bindings.apply(&scheme.ty);
        let constraints = scheme
            .constraints
            .iter()
            .map(|constraint| {
                Constraint {
                    class: constraint.class.clone(),
                    args: constraint
                        .args
                        .iter()
                        .map(|arg| bindings.apply(arg))
                        .collect(),
                }
            })
            .collect();
        for constraint in &scheme.numeric {
            if let Err(error) = self.add_numeric_constraint(constraint.substitute(&bindings)) {
                self.numeric_errors.push(error.to_string());
            }
        }
        (ty, constraints, bindings)
    }

    /// Generalises a type over the variables introduced above the current
    /// level.
    ///
    /// Weak variables and literal variables are *not* quantified: the first
    /// are weak by definition (§3.3.3) and the second must default at the end
    /// of the declaration (§3.10.2).
    #[must_use]
    pub fn generalize(
        &self,
        ty: &Type,
    ) -> Scheme {
        self.generalize_with(ty, Vec::new())
    }

    /// Generalises a type together with the class constraints that belong to
    /// it.
    #[must_use]
    pub fn generalize_with(
        &self,
        ty: &Type,
        constraints: Vec<Constraint>,
    ) -> Scheme {
        self.generalize_at(self.level, ty, constraints)
    }

    /// Generalises as if the binding sat at `level`.
    ///
    /// A module-level binding is generalised at level 0 even while the
    /// remaining declarations are still being checked at level 1, because a
    /// later declaration must see its polymorphic type.
    #[must_use]
    pub fn generalize_at(
        &self,
        level: u32,
        ty: &Type,
        constraints: Vec<Constraint>,
    ) -> Scheme {
        let mut vars = Vec::new();
        let mut dims = Vec::new();
        let mut rows = Vec::new();
        let mut nats = Vec::new();
        self.collect_generalizable(
            &self.resolve(ty),
            level,
            &mut vars,
            &mut dims,
            &mut rows,
            &mut nats,
        );
        // Representation variables are quantified through the type-variable
        // list as well: they are created from the same counter, so a scheme
        // that quantifies `ρ` must quantify its id.
        let mut scheme = Scheme {
            vars,
            dims,
            rows,
            row_lacks: Vec::new(),
            nats,
            constraints,
            numeric: Vec::new(),
            materializations: Vec::new(),
            ty: self.resolve(ty),
        };
        if let Type::Fn(inputs, _) = self.resolve(ty) {
            self.quantify_numeric_inputs(&mut scheme, &inputs, level);
        }
        self.attach_numeric_scheme(&mut scheme);
        if matches!(scheme.ty, Type::Fn(..)) {
            self.quantify_numeric_existentials(&mut scheme, level);
        }
        for constraint in &scheme.numeric {
            for ty in constraint.types() {
                self.collect_generalizable(
                    &ty,
                    level,
                    &mut scheme.vars,
                    &mut scheme.dims,
                    &mut scheme.rows,
                    &mut scheme.nats,
                );
            }
        }
        self.attach_row_lacks(&mut scheme);
        scheme
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_generalizable(
        &self,
        ty: &Type,
        level: u32,
        vars: &mut Vec<u32>,
        dims: &mut Vec<u32>,
        rows: &mut Vec<u32>,
        nats: &mut Vec<u32>,
    ) {
        let type_var = |id: u32| {
            !self.weak.contains(&id)
                && (!self.literals.contains_key(&id) || self.numeric_universals.contains(&id))
                && self.levels.get(&id).copied().unwrap_or(0) > level
        };
        let rep_var = |id: u32| {
            !self.weak.contains(&id)
                && (!self.rep_literals.contains_key(&id) || self.numeric_universals.contains(&id))
                && self.rep_levels.get(&id).copied().unwrap_or(0) > level
        };
        let dim_var = |id: u32| {
            !self.is_weak_dim(id) && self.dim_levels.get(&id).copied().unwrap_or(0) > level
        };
        let nat_var = |id: u32| {
            !self.weak.contains(&id) && self.nat_levels.get(&id).copied().unwrap_or(0) > level
        };
        match ty {
            | Type::Var(id) => {
                if type_var(*id) && !vars.contains(id) {
                    vars.push(*id);
                }
            },
            | Type::Scalar(rep, dim) => {
                if let Rep::Var(id) = rep
                    && rep_var(*id)
                    && !vars.contains(id)
                {
                    vars.push(*id);
                }
                for id in dim.vars() {
                    if dim_var(id) && !dims.contains(&id) {
                        dims.push(id);
                    }
                }
            },
            | Type::Tensor { shape, elem } => {
                // A shape extent variable that is still free generalises with
                // the binding, exactly like a dimension variable (§3.5.1).
                if let Shape::Refined(items) = shape {
                    for nat in items.iter().flatten() {
                        for id in nat.vars() {
                            let resolved = self.resolve_nat(&Nat::Var(id));
                            let Nat::Var(free) = resolved else {
                                continue;
                            };
                            if nat_var(free) && !nats.contains(&free) {
                                nats.push(free);
                            }
                        }
                    }
                }
                self.collect_generalizable(elem, level, vars, dims, rows, nats);
            },
            | Type::Fn(params, ret) => {
                for param in params {
                    self.collect_generalizable(param, level, vars, dims, rows, nats);
                }
                self.collect_generalizable(ret, level, vars, dims, rows, nats);
            },
            | Type::Record(row) => {
                self.collect_row(row, level, vars, dims, rows, nats);
            },
            | Type::Adt(_, args) => {
                for arg in args {
                    self.collect_generalizable(arg, level, vars, dims, rows, nats);
                }
            },
            | Type::Io(inner)
            | Type::TypeRepIntrinsic(Some(inner))
            | Type::FromValueIntrinsic(Some(inner)) => {
                self.collect_generalizable(inner, level, vars, dims, rows, nats);
            },
            | Type::TensorIntrinsic(intrinsic) => {
                if let Some(Rep::Var(id)) = intrinsic.element
                    && rep_var(id)
                    && !vars.contains(&id)
                {
                    vars.push(id);
                }
            },
            | Type::Point(dim) | Type::UnitOf(dim) => {
                for id in dim.vars() {
                    if dim_var(id) && !dims.contains(&id) {
                        dims.push(id);
                    }
                }
            },
            | Type::Dyn(Rep::Var(id)) => {
                if rep_var(*id) && !vars.contains(id) {
                    vars.push(*id);
                }
            },
            | _ => {},
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_row(
        &self,
        row: &Row,
        level: u32,
        vars: &mut Vec<u32>,
        dims: &mut Vec<u32>,
        rows: &mut Vec<u32>,
        nats: &mut Vec<u32>,
    ) {
        for (_, field) in row.iter_fields() {
            self.collect_generalizable(field, level, vars, dims, rows, nats);
        }
        if let Some(id) = row.tail_var()
            && !self.weak.contains(&id)
            && self.row_levels.get(&id).copied().unwrap_or(0) > level
            && !rows.contains(&id)
        {
            rows.push(id);
        }
    }

    /// Applies the defaulting rules of §3.10.2 to a resolved type.
    ///
    /// Returns the defaulted type and the ids of dimension variables that were
    /// defaulted to `1`, which the caller reports as `W0301`.
    pub fn default_type(
        &mut self,
        ty: &Type,
    ) -> (Type, Vec<u32>) {
        let mut scheme = Scheme::mono(ty.clone());
        self.attach_numeric_scheme(&mut scheme);
        let (defaulted, warnings) = self.default_scheme(&scheme);
        (defaulted.ty, warnings)
    }

    /// Defaults the variables a scheme did *not* quantify (§3.10.2).
    ///
    /// This is the module-end step: generalisation runs first, so
    /// `<d>`-polymorphic functions keep their dimension variables and only
    /// weak and literal variables are defaulted.
    pub fn default_scheme(
        &mut self,
        scheme: &Scheme,
    ) -> (Scheme, Vec<u32>) {
        self.default_scheme_preserving(scheme, &crate::TypeSubstitution::default())
    }

    /// Defaults unquantified variables while retaining the lexical parameters.
    pub fn default_scheme_preserving(
        &mut self,
        scheme: &Scheme,
        lexical: &crate::TypeSubstitution,
    ) -> (Scheme, Vec<u32>) {
        let mut protection = scheme.clone();
        for id in lexical.types.keys().chain(lexical.representations.keys()) {
            if !protection.vars.contains(id) {
                protection.vars.push(*id);
            }
        }
        for id in lexical.dimensions.keys() {
            if !protection.dims.contains(id) {
                protection.dims.push(*id);
            }
        }
        for id in lexical.rows.keys() {
            if !protection.rows.contains(id) {
                protection.rows.push(*id);
            }
        }
        for id in lexical.naturals.keys() {
            if !protection.nats.contains(id) {
                protection.nats.push(*id);
            }
        }
        let protected: HashSet<_> = protection
            .vars
            .iter()
            .chain(&protection.dims)
            .chain(&protection.rows)
            .chain(&protection.nats)
            .copied()
            .collect();
        if let Err(error) = self.default_numeric_literals(&scheme.numeric, &protected) {
            self.numeric_errors.push(error.to_string());
        }
        let mut warnings = Vec::new();
        let ty = self.default_inner(&scheme.ty, &protection, &mut warnings, 0);
        let constraints = scheme
            .constraints
            .iter()
            .map(|constraint| {
                Constraint {
                    class: constraint.class.clone(),
                    args: constraint
                        .args
                        .iter()
                        .map(|arg| self.default_inner(arg, &protection, &mut warnings, 0))
                        .collect(),
                }
            })
            .collect();
        if let Err(error) = self.propagate_numeric() {
            self.numeric_errors.push(error.to_string());
        }
        let mut result = Scheme {
            vars: scheme.vars.clone(),
            dims: scheme.dims.clone(),
            rows: scheme.rows.clone(),
            row_lacks: scheme.row_lacks.clone(),
            nats: scheme.nats.clone(),
            constraints,
            numeric: scheme
                .numeric
                .iter()
                .map(|constraint| self.resolve_numeric(constraint))
                .collect(),
            materializations: scheme
                .materializations
                .iter()
                .map(|ty| self.resolve(ty))
                .collect(),
            ty: self.resolve(&ty),
        };
        self.attach_row_lacks(&mut result);
        self.attach_numeric_scheme(&mut result);
        (result, warnings)
    }

    fn default_inner(
        &mut self,
        ty: &Type,
        scheme: &Scheme,
        warnings: &mut Vec<u32>,
        depth: u32,
    ) -> Type {
        if depth > 64 {
            return ty.clone();
        }
        let child_depth = depth.saturating_add(1);
        let quantified_type = |id: u32| scheme.vars.contains(&id);
        match self.resolve(ty) {
            | Type::Var(id) => {
                if quantified_type(id) || !self.above_floor(id) {
                    return Type::Var(id);
                }
                match self.literals.get(&id).copied() {
                    | Some(kind) => {
                        let rep = Rep::default_for_literal(kind);
                        let defaulted = Type::Scalar(rep, Dim::one());
                        // Record the default as a substitution: a deferred
                        // constraint recorded this variable, and it must see
                        // the defaulted type too.
                        self.subst.insert(id, Arc::new(defaulted.clone()));
                        self.numeric_changed(id);
                        defaulted
                    },
                    // A type variable with no literal constraint cannot be
                    // defaulted to `Real`: §3.10.2 only defaults representation,
                    // dimension, natural and row variables. It stays free so the
                    // caller can report `E0304`.
                    | None => Type::Var(id),
                }
            },
            | Type::Scalar(rep, dim) => {
                let rep = match self.resolve_rep(rep) {
                    | Rep::Var(id) if quantified_type(id) || !self.above_floor(id) => Rep::Var(id),
                    | Rep::Var(id) => self.default_rep(id),
                    | other => other,
                };
                let dim = self.default_dim(&dim, scheme, warnings);
                Type::Scalar(rep, dim)
            },
            | Type::Tensor { shape, elem } => {
                Type::Tensor {
                    // §3.10.2 rule 6: an unquantified Nat variable is *not*
                    // defaulted — the checker reports `E0304` for it. Resolving the
                    // shape here only applies the substitutions already known.
                    shape: self.resolve_shape(&shape),
                    elem: Box::new(self.default_inner(&elem, scheme, warnings, child_depth)),
                }
            },
            | Type::Fn(params, ret) => {
                Type::Fn(
                    params
                        .iter()
                        .map(|param| self.default_inner(param, scheme, warnings, child_depth))
                        .collect(),
                    Box::new(self.default_inner(&ret, scheme, warnings, child_depth)),
                )
            },
            | Type::Record(row) => Type::Record(self.default_row(&row, scheme, warnings, depth)),
            | Type::Adt(id, args) => {
                Type::Adt(
                    id,
                    args.iter()
                        .map(|arg| self.default_inner(arg, scheme, warnings, child_depth))
                        .collect(),
                )
            },
            | Type::Io(inner) => {
                Type::Io(Box::new(self.default_inner(
                    &inner,
                    scheme,
                    warnings,
                    child_depth,
                )))
            },
            | Type::Point(dim) => Type::Point(self.default_dim(&dim, scheme, warnings)),
            | Type::UnitOf(dim) => Type::UnitOf(self.default_dim(&dim, scheme, warnings)),
            | Type::Dyn(rep) => {
                match self.resolve_rep(rep) {
                    | Rep::Var(id) if quantified_type(id) => Type::Dyn(Rep::Var(id)),
                    | Rep::Var(id) => Type::Dyn(self.default_rep(id)),
                    | other => Type::Dyn(other),
                }
            },
            | Type::TypeRepIntrinsic(inner) => {
                Type::TypeRepIntrinsic(
                    inner
                        .as_ref()
                        .map(|ty| Box::new(self.default_inner(ty, scheme, warnings, child_depth))),
                )
            },
            | Type::FromValueIntrinsic(inner) => {
                Type::FromValueIntrinsic(
                    inner
                        .as_ref()
                        .map(|ty| Box::new(self.default_inner(ty, scheme, warnings, child_depth))),
                )
            },
            | Type::TensorIntrinsic(mut intrinsic) => {
                if let Some(rep) = intrinsic.element
                    && let Type::Scalar(rep, _) = self.default_inner(
                        &Type::Scalar(rep, Dim::one()),
                        scheme,
                        warnings,
                        child_depth,
                    )
                {
                    intrinsic.element = Some(rep);
                }
                Type::TensorIntrinsic(intrinsic)
            },
            | other => other,
        }
    }

    /// The representation a numeric literal's variable takes: its default, or
    /// the least reading above its recorded lower bounds.
    ///
    /// This is the static answer §3.10.1 gives a literal, exposed so the
    /// checker can publish it (`CheckedModule::literal_reps`). Unlike
    /// [`Self::default_rep`], it is defined for a variable that sits only in a
    /// body and so was never visited by scheme defaulting.
    #[must_use]
    pub fn literal_representation(
        &mut self,
        rep: Rep,
    ) -> Option<Rep> {
        let rep = self.resolve_rep(rep);
        let Rep::Var(id) = rep else {
            return Some(rep);
        };
        let kind = self.rep_literals.get(&id).copied()?;
        self.choose_representation(id, Some(kind))
    }

    fn choose_representation(
        &mut self,
        id: u32,
        literal: Option<LiteralKind>,
    ) -> Option<Rep> {
        let preferred = literal.map_or(Rep::Real, Rep::default_for_literal);
        for candidate in [preferred, Rep::Int, Rep::Float, Rep::Real, Rep::Complex] {
            if literal.is_some_and(|kind| !kind.accepts_rep(candidate)) {
                continue;
            }
            let ty = Type::Scalar(Rep::Var(id), Dim::one());
            let reading = Type::Scalar(candidate, Dim::one());
            if self.probe_numeric_reading(&ty, &reading).is_ok()
                && self.unify_rep(Rep::Var(id), candidate).is_ok()
            {
                return Some(candidate);
            }
        }
        self.numeric_errors
            .push("no numeric representation satisfies the inferred constraints".to_string());
        None
    }

    /// Default through the same relation graph used by explicit calls.
    fn default_rep(
        &mut self,
        id: u32,
    ) -> Rep {
        self.default_rep_at(id, 0)
    }

    /// §3.10.2: an arithmetic result is not an independent variable. Its
    /// default is the `join` (or `div_rep`) of its operands' defaults, so the
    /// operands are defaulted first — leaves by their literal rule — and the
    /// result follows. Defaulting the result first picked rule 4 (`Real`) for
    /// `1 + 1.0` and then let the `Float` leaf promote silently (`W0302`),
    /// which made the static type of a mixed literal tree disagree with the
    /// spec's `join(Int, Float) = Float`.
    fn default_rep_at(
        &mut self,
        id: u32,
        depth: u32,
    ) -> Rep {
        if let Rep::Var(current) = self.resolve_rep(Rep::Var(id)) {
            let derived = if depth < 4096 {
                self.derived_default(current, depth)
            } else {
                None
            };
            if let Rep::Var(current) = self.resolve_rep(Rep::Var(current)) {
                if let Some(rep) = derived {
                    let ty = Type::Scalar(Rep::Var(current), Dim::one());
                    let reading = Type::Scalar(rep, Dim::one());
                    if self.probe_numeric_reading(&ty, &reading).is_ok()
                        && self.unify_rep(Rep::Var(current), rep).is_ok()
                    {
                        return rep;
                    }
                }
                let literal = self.rep_literals.get(&current).copied();
                return self
                    .choose_representation(current, literal)
                    .unwrap_or_else(|| literal.map_or(Rep::Real, Rep::default_for_literal));
            }
        }
        self.resolve_rep(Rep::Var(id))
    }

    /// The default an arithmetic result inherits from its operands.
    fn derived_default(
        &mut self,
        id: u32,
        depth: u32,
    ) -> Option<Rep> {
        if let Some((left, right)) = self.join_inputs.get(&id).copied() {
            // Operands that are not direct literal leaves are defaulted
            // first. A literal leaf that then meets an exact `Real` is read
            // exactly (§3.10.1: `1/3 + 0.1` is `13/30`, not `1/3` plus the
            // binary expansion of `0.1`); otherwise it takes its own default.
            let is_leaf = |infer: &Self, rep: Rep| {
                matches!(infer.resolve_rep(rep), Rep::Var(input) if input != id
                    && infer.direct_literal_kind(Rep::Var(input)).is_some())
            };
            let order = if is_leaf(self, left) && !is_leaf(self, right) {
                [right, left]
            } else {
                [left, right]
            };
            let mut resolved_first = None;
            for operand in order {
                if let (Some(Rep::Real), Rep::Var(input)) =
                    (resolved_first, self.resolve_rep(operand))
                {
                    let kind = self.direct_literal_kind(Rep::Var(input));
                    if kind.is_some_and(|kind| kind.accepts_rep(Rep::Real)) && input != id {
                        let ty = Type::Scalar(Rep::Var(input), Dim::one());
                        let reading = Type::Scalar(Rep::Real, Dim::one());
                        if self.probe_numeric_reading(&ty, &reading).is_ok() {
                            let _ = self.unify_rep(Rep::Var(input), Rep::Real);
                        }
                    }
                }
                let resolved = match self.resolve_rep(operand) {
                    | Rep::Var(input) if input != id && self.above_floor(input) => {
                        self.default_rep_at(input, depth.saturating_add(1))
                    },
                    | other => other,
                };
                if resolved_first.is_none() {
                    resolved_first = Some(resolved);
                }
            }
            return match (self.resolve_rep(left), self.resolve_rep(right)) {
                | (Rep::Var(_), _) | (_, Rep::Var(_)) => None,
                | (a, b) => a.join_concrete(b),
            };
        }
        if let Some(input) = self.division_inputs.get(&id).copied() {
            let resolved = match self.resolve_rep(input) {
                | Rep::Var(operand) if operand != id && self.above_floor(operand) => {
                    self.default_rep_at(operand, depth.saturating_add(1))
                },
                | other => other,
            };
            return (!matches!(resolved, Rep::Var(_))).then(|| resolved.div_rep());
        }
        None
    }

    fn default_dim(
        &mut self,
        dim: &Dim,
        scheme: &Scheme,
        warnings: &mut Vec<u32>,
    ) -> Dim {
        for id in dim.vars() {
            if scheme.dims.contains(&id) || self.dim_is_bound(id) || !self.above_floor(id) {
                continue;
            }
            // §3.10.2 rule 5, narrowed by §3.3.3: only a *weak* variable is
            // worth a warning — a generalisable one was quantified above.
            if self.is_weak_dim(id) {
                warnings.push(id);
            }
            let _ = self.dims.bind(id, Dim::one());
        }
        self.dims.resolve(dim)
    }

    fn default_row(
        &mut self,
        row: &Row,
        scheme: &Scheme,
        warnings: &mut Vec<u32>,
        depth: u32,
    ) -> Row {
        let child_depth = depth.saturating_add(1);
        let fields: Vec<_> = row
            .iter_fields()
            .map(|(label, ty)| {
                (
                    label.to_string(),
                    self.default_inner(ty, scheme, warnings, child_depth),
                )
            })
            .collect();
        let tail = row
            .tail_var()
            .filter(|id| scheme.rows.contains(id) || !self.above_floor(*id))
            .map_or(Row::Empty, Row::Var);
        extend_row_fields(fields.into_iter().rev(), tail)
    }

    /// Whether a variable was created inside the declaration being closed.
    ///
    /// Defaulting an inner `let` must not touch a variable that belongs to the
    /// enclosing function: that one is generalised, or defaulted, at the
    /// enclosing declaration's end.
    fn above_floor(
        &self,
        id: u32,
    ) -> bool {
        let level = self
            .levels
            .get(&id)
            .or_else(|| self.rep_levels.get(&id))
            .or_else(|| self.dim_levels.get(&id))
            .or_else(|| self.row_levels.get(&id))
            .copied()
            .unwrap_or(0);
        level > self.level
    }

    fn dim_is_bound(
        &self,
        id: u32,
    ) -> bool {
        !self.dims.resolve(&Dim::var(id)).contains_var(id)
    }

    /// Reports representation bounds that were left violated once every
    /// variable resolved.
    ///
    /// `fn f(x: Int): Int = x + 2.0` binds the result to `Int` while the
    /// literal needs `Float`; the annotation is what must be rejected.
    #[must_use]
    pub fn rep_conflicts(&self) -> Vec<(Rep, Rep)> {
        let mut out = Vec::new();
        for (id, bounds) in &self.rep_lower {
            let variable = self.resolve_rep(Rep::Var(*id));
            if let Rep::Var(_) = variable {
                continue;
            }
            for bound in bounds {
                let bound = self.resolve_rep(*bound);
                if let Rep::Var(_) = bound {
                    continue;
                }
                if !variable.at_least(bound) {
                    out.push((variable, bound));
                }
            }
        }
        out
    }

    /// Restores a snapshot taken before a speculative unification.
    ///
    /// The variable counter is *not* rolled back: reusing ids that a previous
    /// attempt already bound would make a "fresh" variable arrive pre-bound,
    /// which is exactly the bug that made two `value_in` calls share a
    /// dimension.
    pub fn restore(
        &mut self,
        snapshot: &Self,
    ) {
        let next = self.next.max(snapshot.next);
        *self = snapshot.clone();
        self.next = next;
    }

    /// The number of allocated variables, for tests.
    #[must_use]
    pub const fn variable_count(&self) -> u32 {
        self.next
    }
}

enum Occurrence<'a> {
    Type(&'a Type),
    Row(&'a Row),
}

enum BindTarget<'a> {
    Arc(Arc<Type>),
    Borrowed(&'a Type),
}

impl BindTarget<'_> {
    #[allow(
        clippy::single_call_fn,
        reason = "uniformly borrows the target Type whether wrapped in Arc or stack-borrowed"
    )]
    fn as_type(&self) -> &Type {
        match self {
            | Self::Arc(arc) => arc.as_ref(),
            | Self::Borrowed(ty) => ty,
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "preserves O(1) Arc sharing when binding a variable to an existing Arc"
    )]
    fn into_arc(self) -> Arc<Type> {
        match self {
            | Self::Arc(arc) => arc,
            | Self::Borrowed(ty) => Arc::new(ty.clone()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum NodeKey {
    Var(u32),
    Ptr(usize),
}

#[derive(Default)]
struct UnifyMemo {
    visited: HashSet<(NodeKey, NodeKey)>,
    visited_shapes: HashSet<(usize, usize)>,
    verified_var_pairs: Vec<(u32, u32)>,
    flat_cache: HashMap<usize, Arc<FlatTree>>,
}

impl UnifyMemo {
    fn new() -> Self {
        Self::default()
    }

    fn to_flat_side(
        &mut self,
        ty: &Type,
    ) -> FlatSide {
        if let Type::Var(id) = ty {
            return FlatSide::Var(*id);
        }
        let addr = std::ptr::from_ref(ty).addr();
        if let Some(cached) = self.flat_cache.get(&addr) {
            return FlatSide::Node(Arc::clone(cached), 0);
        }
        let tree = Arc::new(flatten_type(ty));
        self.flat_cache.insert(addr, Arc::clone(&tree));
        FlatSide::Node(tree, 0)
    }

    #[allow(
        clippy::unused_self,
        reason = "method keeps trampoline side normalization consistent on UnifyMemo"
    )]
    fn unwrap_flat_var(
        &self,
        side: FlatSide,
    ) -> FlatSide {
        if let FlatSide::Node(ref tree, idx) = side
            && let Some(FlatType::Direct(Type::Var(id))) = tree.nodes.get(idx)
        {
            return FlatSide::Var(*id);
        }
        side
    }
}

enum FlatSide {
    Var(u32),
    Node(Arc<FlatTree>, usize),
}

struct FlatTree {
    nodes: Vec<FlatType>,
    ptrs: Vec<usize>,
}

impl FlatTree {
    fn is_error(
        &self,
        idx: usize,
    ) -> bool {
        matches!(self.nodes.get(idx), Some(FlatType::Direct(ty)) if ty.is_error())
    }

    fn is_var(
        &self,
        idx: usize,
    ) -> bool {
        matches!(self.nodes.get(idx), Some(FlatType::Direct(Type::Var(_))))
    }

    fn ptr(
        &self,
        idx: usize,
    ) -> usize {
        self.ptrs.get(idx).copied().unwrap_or(0)
    }
}

enum FlatType {
    Direct(Type),
    Tensor {
        shape: Shape,
        elem: usize,
    },
    TypeRep(usize),
    FromValue(usize),
    Fn {
        params: Vec<usize>,
        ret: usize,
    },
    Adt {
        id: crate::ty::AdtId,
        args: Vec<usize>,
    },
    Io(usize),
}

const fn canonical_pair(
    a: u32,
    b: u32,
) -> (u32, u32) {
    if a < b { (a, b) } else { (b, a) }
}

fn resolve_rep_table(
    rep_subst: &HashMap<u32, Rep>,
    rep: Rep,
) -> Rep {
    let mut current = rep;
    for _ in 0..=rep_subst.len() {
        let Rep::Var(id) = current else {
            return current;
        };
        let Some(bound) = rep_subst.get(&id) else {
            return current;
        };
        current = *bound;
    }
    current
}

#[allow(
    clippy::single_call_fn,
    reason = "flattens deep type trees iteratively once per Arc when trampoline activates"
)]
fn flatten_type(root: &Type) -> FlatTree {
    let mut nodes = vec![FlatType::Direct(Type::Error)];
    let mut ptrs = vec![0usize];
    let mut work = vec![(0usize, root)];
    while let Some((slot, ty)) = work.pop() {
        if let Some(ptr) = ptrs.get_mut(slot) {
            *ptr = std::ptr::from_ref(ty).addr();
        }
        let flat = match ty {
            | Type::Tensor { shape, elem } => {
                let child = nodes.len();
                nodes.push(FlatType::Direct(Type::Error));
                ptrs.push(0);
                work.push((child, elem));
                FlatType::Tensor {
                    shape: shape.clone(),
                    elem: child,
                }
            },
            | Type::Io(inner) => {
                let child = nodes.len();
                nodes.push(FlatType::Direct(Type::Error));
                ptrs.push(0);
                work.push((child, inner));
                FlatType::Io(child)
            },
            | Type::TypeRepIntrinsic(Some(inner)) => {
                let child = nodes.len();
                nodes.push(FlatType::Direct(Type::Error));
                ptrs.push(0);
                work.push((child, inner));
                FlatType::TypeRep(child)
            },
            | Type::FromValueIntrinsic(Some(inner)) => {
                let child = nodes.len();
                nodes.push(FlatType::Direct(Type::Error));
                ptrs.push(0);
                work.push((child, inner));
                FlatType::FromValue(child)
            },
            | Type::Fn(params, ret) => {
                let mut param_slots = Vec::with_capacity(params.len());
                for param in params {
                    let child = nodes.len();
                    nodes.push(FlatType::Direct(Type::Error));
                    ptrs.push(0);
                    work.push((child, param));
                    param_slots.push(child);
                }
                let ret_slot = nodes.len();
                nodes.push(FlatType::Direct(Type::Error));
                ptrs.push(0);
                work.push((ret_slot, ret));
                FlatType::Fn {
                    params: param_slots,
                    ret: ret_slot,
                }
            },
            | Type::Adt(id, args) => {
                let mut arg_slots = Vec::with_capacity(args.len());
                for arg in args {
                    let child = nodes.len();
                    nodes.push(FlatType::Direct(Type::Error));
                    ptrs.push(0);
                    work.push((child, arg));
                    arg_slots.push(child);
                }
                FlatType::Adt {
                    id: *id,
                    args: arg_slots,
                }
            },
            | _ => FlatType::Direct(ty.clone()),
        };
        if let Some(target) = nodes.get_mut(slot) {
            *target = flat;
        }
    }
    FlatTree { nodes, ptrs }
}

fn unflatten_type(
    tree: &FlatTree,
    root: usize,
) -> Type {
    let mut dfs = vec![(root, false)];
    let mut built: HashMap<usize, Type> = HashMap::new();
    while let Some((idx, ready)) = dfs.pop() {
        let Some(node) = tree.nodes.get(idx) else {
            continue;
        };
        if !ready {
            dfs.push((idx, true));
            match node {
                | FlatType::Tensor { elem, .. }
                | FlatType::Io(elem)
                | FlatType::TypeRep(elem)
                | FlatType::FromValue(elem) => {
                    dfs.push((*elem, false));
                },
                | FlatType::Fn { params, ret } => {
                    dfs.push((*ret, false));
                    for &param in params {
                        dfs.push((param, false));
                    }
                },
                | FlatType::Adt { args, .. } => {
                    for &arg in args {
                        dfs.push((arg, false));
                    }
                },
                | FlatType::Direct(_) => {},
            }
            continue;
        }
        let ty = match node {
            | FlatType::Direct(direct) => direct.clone(),
            | FlatType::Tensor { shape, elem } => {
                Type::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(built.remove(elem).unwrap_or(Type::Error)),
                }
            },
            | FlatType::Io(inner) => Type::Io(Box::new(built.remove(inner).unwrap_or(Type::Error))),
            | FlatType::TypeRep(inner) => {
                Type::TypeRepIntrinsic(Some(Box::new(built.remove(inner).unwrap_or(Type::Error))))
            },
            | FlatType::FromValue(inner) => {
                Type::FromValueIntrinsic(Some(Box::new(built.remove(inner).unwrap_or(Type::Error))))
            },
            | FlatType::Fn { params, ret } => {
                let param_tys = params
                    .iter()
                    .map(|id| built.remove(id).unwrap_or(Type::Error))
                    .collect();
                let ret_ty = built.remove(ret).unwrap_or(Type::Error);
                Type::Fn(param_tys, Box::new(ret_ty))
            },
            | FlatType::Adt { id, args } => {
                let arg_tys = args
                    .iter()
                    .map(|id| built.remove(id).unwrap_or(Type::Error))
                    .collect();
                Type::Adt(*id, arg_tys)
            },
        };
        built.insert(idx, ty);
    }
    built.remove(&root).unwrap_or(Type::Error)
}

/// Applies fields from back to front around a shared tail.
fn extend_row_fields(
    fields: impl IntoIterator<Item = (String, Type)>,
    tail: Row,
) -> Row {
    fields.into_iter().fold(tail, |tail, (label, ty)| {
        Row::Extend {
            label,
            ty: Box::new(ty),
            tail: Box::new(tail),
        }
    })
}

const fn row_id(row: &Row) -> Option<u32> {
    match row {
        | Row::Var(id) => Some(*id),
        | _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_dependencies_include_only_type_and_representation_variables() {
        let ty = Type::func(
            vec![
                Type::tensor(
                    Shape::Refined(vec![Some(Nat::Var(9))]),
                    Type::Scalar(Rep::Var(5), Dim::var(10)),
                ),
                Type::Record(Row::Extend {
                    label: "field".to_owned(),
                    ty: Box::new(Type::Io(Box::new(Type::Dyn(Rep::Var(4))))),
                    tail: Box::new(Row::Var(11)),
                }),
                Type::Point(Dim::var(12)),
            ],
            Type::Adt(
                0,
                vec![
                    Type::FromValueIntrinsic(Some(Box::new(Type::Var(2)))),
                    Type::TypeRepIntrinsic(Some(Box::new(Type::Var(2)))),
                    Type::TensorIntrinsic(crate::ty::TensorIntrinsicType {
                        kind: crate::ty::TensorIntrinsic::Fill,
                        element: Some(Rep::Var(3)),
                    }),
                    Type::Param(13),
                ],
            ),
        );
        let literal = NumericConstraint::Literal {
            ty: ty.clone(),
            kind: LiteralKind::Int,
        };
        let default = NumericConstraint::DefaultLiteral {
            ty,
            kind: LiteralKind::Decimal,
            inputs: vec![Rep::Var(6), Rep::Float, Rep::Var(4)],
        };
        let relation = NumericConstraint::Relation(RepRelation::Join {
            left: Rep::Var(7),
            right: Rep::Var(7),
            result: Rep::Var(8),
        });
        assert_eq!(Infer::numeric_variables(&literal), [2, 3, 4, 5]);
        assert_eq!(Infer::numeric_variables(&default), [2, 3, 4, 5, 6]);
        assert_eq!(Infer::numeric_variables(&relation), [7, 8]);
    }

    #[test]
    fn quantified_numeric_inputs_keep_stable_variable_order() {
        let mut infer = Infer::new();
        infer.enter_level();
        let mut inputs = Vec::new();
        for _ in 0..8 {
            inputs.push(infer.fresh_type());
            inputs.push(Type::Scalar(infer.fresh_rep(), Dim::one()));
        }
        let mut scheme = Scheme::mono(Type::Unit);
        infer.quantify_numeric_inputs(&mut scheme, &inputs, 0);
        assert_eq!(
            scheme.vars,
            [0, 2, 4, 6, 8, 10, 12, 14, 1, 3, 5, 7, 9, 11, 13, 15]
        );
    }

    #[test]
    fn an_inconsistent_nat_equation_keeps_other_pending_constraints() {
        let mut infer = Infer::new();
        let a = infer.fresh_nat();
        let b = infer.fresh_nat();
        infer.name_nat(&a, "a", true);
        infer.name_nat(&b, "b", true);
        infer.unify_nat(&a.add(b), &Nat::lit(2)).unwrap();
        assert_eq!(infer.unsolved_nat_equations(), 1);
        assert!(infer.unify_nat(&Nat::lit(1), &Nat::lit(2)).is_err());
        assert_eq!(infer.unsolved_nat_equations(), 1);
    }

    #[test]
    fn unrelated_nat_equations_are_not_retried() {
        let mut infer = Infer::new();
        let mut extents = Vec::new();
        for _ in 0..1024 {
            let a = infer.fresh_nat();
            let b = infer.fresh_nat();
            infer.name_nat(&a, "a", true);
            infer.name_nat(&b, "b", true);
            infer
                .unify_nat(&a.clone().add(b.clone()), &Nat::lit(7))
                .unwrap();
            extents.push((a, b));
        }
        let independent = infer.fresh_nat();
        infer.unify_nat(&independent, &Nat::lit(8)).unwrap();
        assert_eq!(infer.nat_retries, 0);

        let (a, b) = &extents[511];
        infer.unify_nat(a, &Nat::lit(1)).unwrap();
        assert_eq!(infer.nat_retries, 1);
        assert_eq!(infer.resolve_nat(b), Nat::lit(6));
        assert_eq!(infer.unsolved_nat_equations(), 1023);
    }

    #[test]
    fn pending_nat_dependencies_follow_aliases_and_trigger_later_bindings() {
        let mut infer = Infer::new();
        let alias = infer.fresh_nat();
        let x = infer.fresh_nat();
        let y = infer.fresh_nat();
        let z = infer.fresh_nat();
        infer.name_nat(&y, "y", true);
        infer.name_nat(&z, "z", true);
        let first = Nat::lit(2).mul(x.clone()).add(y.clone());
        infer.unify_nat(&first, &Nat::lit(7)).unwrap();
        infer
            .unify_nat(&y.clone().add(z.clone()), &Nat::lit(9))
            .unwrap();
        assert_eq!(infer.nat_retries, 0);

        infer.unify_nat(&x, &alias).unwrap();
        assert_eq!(infer.nat_retries, 1);
        infer.unify_nat(&alias, &Nat::lit(3)).unwrap();
        assert_eq!(infer.nat_retries, 3);
        assert_eq!(infer.resolve_nat(&y), Nat::lit(1));
        assert_eq!(infer.resolve_nat(&z), Nat::lit(8));
        assert_eq!(infer.unsolved_nat_equations(), 0);
    }

    #[test]
    fn a_deferred_nat_error_identifies_the_inconsistent_equation() {
        let mut infer = Infer::new();
        let a = infer.fresh_nat();
        let b = infer.fresh_nat();
        let c = infer.fresh_nat();
        infer.name_nat(&a, "a", true);
        infer.name_nat(&b, "b", true);
        infer.name_nat(&c, "c", true);
        let first = Nat::lit(2).mul(a).add(b.clone());
        infer.unify_nat(&first, &Nat::lit(3)).unwrap();
        infer
            .unify_nat(&b.clone().add(c.clone()), &Nat::lit(5))
            .unwrap();

        let error = infer.unify_nat(&b, &Nat::lit(2)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "type-level equation has no solution: 2 * a = 1"
        );
        assert_eq!(infer.unsolved_nat_equations(), 1);

        let independent = infer.fresh_nat();
        infer.unify_nat(&independent, &Nat::lit(8)).unwrap();
        assert_eq!(infer.resolve_nat(&c), Nat::lit(3));
        assert_eq!(infer.unsolved_nat_equations(), 0);
    }

    #[test]
    fn pending_nat_fact_indexes_leaves_beyond_the_resolution_depth() {
        let mut infer = Infer::new();
        let chain: Vec<_> = (0..=384).map(|_| infer.fresh_nat()).collect();
        for pair in chain.windows(2).rev() {
            infer
                .unify_nat(&pair[1], &pair[0].clone().add(Nat::lit(1)))
                .unwrap();
        }
        let other = infer.fresh_nat();
        let equation = chain.last().unwrap().clone().add(other.clone());
        infer.unify_nat(&equation, &Nat::lit(500)).unwrap();
        assert_eq!(infer.unsolved_nat_equations(), 1);
        infer.unify_nat(&chain[0], &Nat::lit(3)).unwrap();
        assert_eq!(infer.resolve_nat(&other), Nat::lit(113));
        assert_eq!(infer.unsolved_nat_equations(), 0);
    }

    #[test]
    fn nat_isolation_keeps_coefficients_wider_than_a_machine_integer() {
        let mut infer = Infer::new();
        let n = infer.fresh_nat();
        let max = Nat::lit(usize::MAX);
        let wide = max.clone().mul(max.clone()).mul(max);
        infer
            .unify_nat(&wide.clone().mul(n.clone()), &wide.clone().add(wide))
            .unwrap();
        assert_eq!(infer.resolve_nat(&n), Nat::lit(2));
    }

    #[test]
    fn distinct_large_nat_products_do_not_unify() {
        let mut infer = Infer::new();
        let max = Nat::lit(usize::MAX);
        let wide = max.clone().mul(max.clone());
        let lhs = wide.clone().mul(max);
        let rhs = wide.mul(Nat::lit(usize::MAX - 1));
        assert!(matches!(
            infer.unify_nat(&lhs, &rhs),
            Err(UnifyError::NatMismatch { .. })
        ));
    }

    #[test]
    fn broadcasting_keeps_large_rank_only_shapes_compact() {
        let mut infer = Infer::new();
        let large = Shape::Rank(u32::MAX);
        assert_eq!(infer.broadcast_shape(&large, &large).unwrap(), large);
        assert!(infer.broadcast_shape(&large, &Shape::Rank(1)).is_err());

        let rank = Shape::Rank(3);
        let refined = Shape::refined([Some(1), Some(4), None]);
        let expected = Shape::refined([None, Some(4), None]);
        assert_eq!(infer.broadcast_shape(&rank, &refined).unwrap(), expected);
        assert_eq!(infer.broadcast_shape(&refined, &rank).unwrap(), expected);
    }

    #[test]
    fn specialized_intrinsic_schemas_resolve_every_nested_kind() {
        let mut infer = Infer::new();
        infer.enter_level();
        let ty = infer.fresh_type();
        let rep = infer.fresh_rep();
        let dim = infer.fresh_dim();
        let nat = infer.fresh_nat();
        let row = infer.fresh_row();
        let intrinsic = |rep| {
            Type::TensorIntrinsic(crate::ty::TensorIntrinsicType {
                kind: crate::ty::TensorIntrinsic::Zeros,
                element: Some(rep),
            })
        };
        let source = Type::TypeRepIntrinsic(Some(Box::new(Type::func(
            vec![
                Type::Point(dim.clone()),
                Type::UnitOf(dim),
                intrinsic(rep),
                Type::tensor(Shape::Refined(vec![Some(nat)]), Type::Record(row)),
            ],
            Type::FromValueIntrinsic(Some(Box::new(ty))),
        ))));
        let length = Dim::base("Length");
        let target = Type::TypeRepIntrinsic(Some(Box::new(Type::func(
            vec![
                Type::Point(length.clone()),
                Type::UnitOf(length),
                intrinsic(Rep::Float),
                Type::tensor(
                    Shape::refined([Some(4)]),
                    Type::record([("x".to_string(), Type::Bool)]),
                ),
            ],
            Type::FromValueIntrinsic(Some(Box::new(Type::Str))),
        ))));
        let scheme = infer.generalize_at(0, &source, Vec::new());
        assert_eq!(
            (
                scheme.vars.len(),
                scheme.dims.len(),
                scheme.nats.len(),
                scheme.rows.len()
            ),
            (2, 1, 1, 1)
        );
        infer.unify(&source, &target).unwrap();
        assert_eq!(infer.resolve(&source), target);
    }

    #[test]
    fn specialized_intrinsic_schemas_reject_recursive_types() {
        for type_rep in [false, true] {
            let mut infer = Infer::new();
            let variable = infer.fresh_type();
            let wrapper = if type_rep {
                Type::TypeRepIntrinsic(Some(Box::new(variable.clone())))
            } else {
                Type::FromValueIntrinsic(Some(Box::new(variable.clone())))
            };
            assert!(matches!(
                infer.unify(&variable, &wrapper),
                Err(UnifyError::Occurs(_))
            ));
        }
    }

    #[test]
    fn a_specialized_intrinsic_defaults_its_unquantified_weak_dimension() {
        let mut infer = Infer::new();
        infer.enter_level();
        let dim = infer.fresh_weak_dim();
        let ty = Type::FromValueIntrinsic(Some(Box::new(Type::UnitOf(dim))));
        infer.leave_level();
        let (defaulted, warnings) = infer.default_scheme(&Scheme::mono(ty.clone()));
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            defaulted.ty,
            Type::FromValueIntrinsic(Some(Box::new(Type::UnitOf(Dim::one()))))
        );
        assert_eq!(infer.resolve(&ty), defaulted.ty);
    }

    #[test]
    fn an_escaping_intrinsic_schema_keeps_its_monomorphic_variables() {
        let mut infer = Infer::new();
        let outer = infer.fresh_type();
        infer.enter_level();
        let rep = infer.fresh_rep();
        let dim = infer.fresh_dim();
        let nat = infer.fresh_nat();
        let row = infer.fresh_row();
        let ty = infer.fresh_type();
        let inner = Type::TypeRepIntrinsic(Some(Box::new(Type::func(
            vec![
                Type::Point(dim),
                Type::TensorIntrinsic(crate::ty::TensorIntrinsicType {
                    kind: crate::ty::TensorIntrinsic::Zeros,
                    element: Some(rep),
                }),
                Type::tensor(Shape::Refined(vec![Some(nat)]), Type::Record(row)),
            ],
            Type::FromValueIntrinsic(Some(Box::new(ty))),
        ))));
        infer.unify(&outer, &inner).unwrap();
        infer.leave_level();
        let scheme = infer.generalize(&inner);
        assert!(
            scheme.vars.is_empty()
                && scheme.dims.is_empty()
                && scheme.nats.is_empty()
                && scheme.rows.is_empty(),
            "escaping monomorphic variables were generalized: {scheme:?}"
        );
    }

    #[test]
    fn unifies_dimension_variables() {
        let mut infer = Infer::new();
        let dim = infer.fresh_dim();
        infer
            .unify(&Type::real(dim.clone()), &Type::real(Dim::base("Length")))
            .expect("solvable");
        assert_eq!(
            infer.resolve(&Type::real(dim)),
            Type::real(Dim::base("Length"))
        );
    }

    #[test]
    fn rejects_dimension_mismatch() {
        let mut infer = Infer::new();
        let error = infer
            .unify(
                &Type::real(Dim::base("Length")),
                &Type::real(Dim::base("Time")),
            )
            .expect_err("incompatible");
        assert!(matches!(error, UnifyError::Dim(_)));
    }

    #[test]
    fn integer_literals_are_not_dimension_polymorphic() {
        let mut infer = Infer::new();
        let literal = infer.fresh_type();
        infer.mark_literal(&literal, LiteralKind::Int);
        let error = infer
            .unify(&literal, &Type::real(Dim::base("Length")))
            .expect_err("a bare literal is dimensionless");
        assert!(matches!(error, UnifyError::LiteralMismatch { .. }));
    }

    #[test]
    fn records_unify_with_open_rows() {
        let mut infer = Infer::new();
        let tail = infer.fresh_row();
        let open = Type::Record(Row::Extend {
            label: "x".to_string(),
            ty: Box::new(Type::int()),
            tail: Box::new(tail),
        });
        let closed = Type::record([
            ("x".to_string(), Type::int()),
            ("y".to_string(), Type::int()),
        ]);
        infer.unify(&open, &closed).expect("open row absorbs y");
    }

    #[test]
    fn missing_fields_are_reported() {
        let mut infer = Infer::new();
        let closed = Type::record([("x".to_string(), Type::int())]);
        let other = Type::record([("z".to_string(), Type::int())]);
        let error = infer.unify(&closed, &other).expect_err("no field z");
        assert!(matches!(error, UnifyError::MissingField { .. }));
    }

    #[test]
    fn generalisation_quantifies_fresh_variables() {
        let mut infer = Infer::new();
        infer.enter_level();
        let variable = infer.fresh_type();
        let ty = Type::func(vec![variable.clone()], variable.clone());
        infer.leave_level();
        let scheme = infer.generalize(&ty);
        assert_eq!(scheme.vars.len(), 1);
    }

    #[test]
    fn aliases_to_younger_variables_keep_the_outer_inference_level() {
        let mut infer = Infer::new();
        let outer_type = infer.fresh_type();
        let outer_row = Type::Record(infer.fresh_row());
        infer.enter_level();
        let inner_type = infer.fresh_type();
        let inner_row = Type::Record(infer.fresh_row());
        infer.unify(&outer_type, &inner_type).unwrap();
        infer.unify(&outer_row, &inner_row).unwrap();
        infer.leave_level();
        assert!(infer.generalize(&inner_type).vars.is_empty());
        assert!(infer.generalize(&inner_row).rows.is_empty());
    }

    #[test]
    fn resolving_a_wide_row_reaches_its_last_field() {
        let mut infer = Infer::new();
        let last = infer.fresh_type();
        let fields = (0..256)
            .map(|index| (format!("field{index:03}"), Type::Bool))
            .chain(std::iter::once(("last".to_string(), last.clone())));
        let row = Row::closed(fields);
        infer.unify(&last, &Type::int()).unwrap();
        assert_eq!(infer.resolve_row(&row).field("last"), Some(&Type::int()));
    }

    #[test]
    fn unifying_wide_open_rows_allocates_one_shared_tail() {
        let mut infer = Infer::new();
        let left_tail = infer.fresh_row();
        let right_tail = infer.fresh_row();
        let fields = |side: &str| {
            (0..128)
                .map(|index| (format!("{side}{index:03}"), Type::int()))
                .collect::<Vec<_>>()
        };
        let left = extend_row_fields(fields("left").into_iter().rev(), left_tail.clone());
        let right = extend_row_fields(fields("right").into_iter().rev(), right_tail.clone());
        let before = infer.variable_count();
        infer
            .unify(&Type::Record(left), &Type::Record(right))
            .unwrap();
        assert_eq!(infer.variable_count(), before + 1);
        let left = infer.resolve_row(&left_tail);
        let right = infer.resolve_row(&right_tail);
        assert_eq!(left.field("right127"), Some(&Type::int()));
        assert_eq!(right.field("left127"), Some(&Type::int()));
        assert_eq!(left.tail_var(), right.tail_var());
    }

    #[test]
    fn instantiation_produces_distinct_variables() {
        let mut infer = Infer::new();
        let scheme = Scheme::poly(vec![0], Type::func(vec![Type::Var(0)], Type::Var(0)));
        let first = infer.instantiate(&scheme);
        let second = infer.instantiate(&scheme);
        assert_ne!(first, second);
    }

    #[test]
    fn a_row_variable_cannot_gain_a_label_it_lacks() {
        // §3.6.1: absorbing `x` leaves a tail that must lack `x`.
        let mut infer = Infer::new();
        let tail = infer.fresh_row();
        let Row::Var(id) = tail else {
            panic!("fresh_row is a variable");
        };
        infer.require_lacks(id, "x").expect("a fresh row lacks x");
        let error = infer
            .extend_row(id, "x", Type::int())
            .expect_err("the label is already there");
        assert!(matches!(error, UnifyError::DuplicateField { .. }));
    }

    #[test]
    fn absorbing_a_field_records_a_lacks_condition() {
        let mut infer = Infer::new();
        let Row::Var(open) = infer.fresh_row() else {
            panic!("fresh_row is a variable");
        };
        let extension = Type::Record(Row::Extend {
            label: "y".to_string(),
            ty: Box::new(Type::int()),
            tail: Box::new(infer.fresh_row()),
        });
        infer
            .unify(&Type::Record(Row::Var(open)), &extension)
            .unwrap();
        let row = infer.resolve_row(&Row::Var(open));
        let tail = row.tail_var().unwrap();
        assert_eq!(row.field("y"), Some(&Type::int()));
        assert!(matches!(infer.extend_row(tail, "y", Type::int()),
            Err(UnifyError::DuplicateField { field }) if field == "y"));
    }

    #[test]
    fn lacks_conditions_follow_extensions_and_aliases() {
        let mut infer = Infer::new();
        let Row::Var(row) = infer.fresh_row() else {
            panic!("fresh_row is a variable");
        };
        let Row::Var(alias) = infer.fresh_row() else {
            panic!("fresh_row is a variable");
        };
        infer
            .unify(&Type::Record(Row::Var(alias)), &Type::Record(Row::Var(row)))
            .unwrap();
        infer.require_lacks(alias, "limited").unwrap();
        infer.extend_row(alias, "first", Type::int()).unwrap();
        infer.require_lacks(row, "later").unwrap();
        infer.extend_row(row, "second", Type::Bool).unwrap();
        let resolved = infer.resolve_row(&Row::Var(alias));
        assert_eq!(resolved.field("first"), Some(&Type::int()));
        assert_eq!(resolved.field("second"), Some(&Type::Bool));
        let residual = resolved.tail_var().unwrap();
        for label in ["limited", "later", "first", "second"] {
            assert!(matches!(infer.extend_row(residual, label, Type::int()),
                Err(UnifyError::DuplicateField { field }) if field == label));
        }
    }

    #[test]
    fn disjoint_prefixes_cannot_reuse_the_same_row_tail() {
        let mut infer = Infer::new();
        let tail = infer.fresh_row();
        let record = |label: &str| {
            Type::Record(Row::Extend {
                label: label.to_string(),
                ty: Box::new(Type::int()),
                tail: Box::new(tail.clone()),
            })
        };
        assert!(matches!(
            infer.unify(&record("left"), &record("right")),
            Err(UnifyError::DuplicateField { .. })
        ));
        assert_eq!(infer.resolve_row(&tail), tail);
    }

    #[test]
    fn a_row_unified_with_a_closed_record_cannot_acquire_another_field() {
        let mut infer = Infer::new();
        let open = Type::Record(infer.fresh_row());
        infer
            .unify(&open, &Type::record([("x".to_string(), Type::int())]))
            .unwrap();
        let error = infer
            .unify(
                &open,
                &Type::record([
                    ("x".to_string(), Type::int()),
                    ("y".to_string(), Type::Bool),
                ]),
            )
            .unwrap_err();
        assert!(matches!(error, UnifyError::MissingField { .. }));
    }

    #[test]
    fn a_weak_dimension_is_not_generalised() {
        let mut infer = Infer::new();
        infer.enter_level();
        let dim = infer.fresh_weak_dim();
        let ty = Type::real(dim.clone());
        infer.leave_level();
        let scheme = infer.generalize(&ty);
        assert!(scheme.dims.is_empty(), "a weak dimension is not quantified");
        let (defaulted, warnings) = infer.default_scheme(&scheme);
        assert_eq!(warnings.len(), 1);
        assert_eq!(defaulted.ty, Type::real(Dim::one()));
    }

    #[test]
    fn joining_an_int_records_a_bound_instead_of_binding() {
        // §3.4.1.
        let mut infer = Infer::new();
        let rep = infer.fresh_rep();
        infer
            .constrain_rep(rep, Rep::Int)
            .expect("Int is a lower bound");
        assert_eq!(infer.resolve_rep(rep), rep, "the variable stays free");
        assert_eq!(infer.rep_bounds(rep), vec![Rep::Int]);
    }

    #[test]
    fn a_bound_below_the_recorded_lower_bound_is_a_mismatch() {
        let mut infer = Infer::new();
        let rep = infer.fresh_rep();
        infer.constrain_rep(rep, Rep::Real).expect("bound");
        let error = infer
            .unify_rep(rep, Rep::Int)
            .expect_err("Int is below Real");
        assert!(matches!(error, UnifyError::Mismatch { .. }));
    }
    #[test]
    fn computed_results_do_not_choose_the_reading_of_their_inputs() {
        let mut infer = Infer::new();
        infer.enter_level();
        let input = infer.fresh_rep();
        infer.mark_literal(&Type::Scalar(input, Dim::one()), LiteralKind::Int);
        let output = infer.divide_representation(input).unwrap();
        // The array zero shares the quotient's element type, but must not
        // choose Float for the quotient before 1 and 3 default to Int.
        infer.mark_literal(&Type::Scalar(output, Dim::one()), LiteralKind::Int);
        let mut scheme = Scheme::mono(Type::Scalar(output, Dim::one()));
        infer.attach_numeric_scheme(&mut scheme);
        scheme.numeric.reverse();
        let (defaulted, _) = infer.default_scheme(&scheme);
        assert_eq!(infer.resolve_rep(input), Rep::Int);
        assert_eq!(defaulted.ty, Type::real(Dim::one()));
    }

    #[test]
    fn divide_absorption_keeps_one_truthful_transfer() {
        let mut infer = Infer::new();
        let input = infer.fresh_rep();
        let divided = infer.divide_representation(input).unwrap();
        assert_eq!(infer.divide_representation(divided).unwrap(), divided);
        assert_eq!(infer.join_representation(input, divided).unwrap(), divided);
        assert_eq!(infer.join_representation(divided, input).unwrap(), divided);
        for selected in [Rep::Int, Rep::Float, Rep::Real, Rep::Complex] {
            let mut trial = infer.clone();
            trial.unify_rep(input, selected).unwrap();
            assert_eq!(trial.resolve_rep(divided), selected.div_rep());
        }
    }
    #[test]
    fn repeated_join_operands_keep_the_existing_transfer() {
        let mut infer = Infer::new();
        let left = infer.fresh_rep();
        let right = infer.fresh_rep();
        let result = infer.join_representation(left, right).unwrap();
        for operand in [left, right] {
            assert_eq!(infer.join_representation(result, operand).unwrap(), result);
            assert_eq!(infer.join_representation(operand, result).unwrap(), result);
        }
        for a in [Rep::Int, Rep::Float, Rep::Real, Rep::Complex] {
            for b in [Rep::Int, Rep::Float, Rep::Real, Rep::Complex] {
                let mut trial = infer.clone();
                trial.unify_rep(left, a).unwrap();
                trial.unify_rep(right, b).unwrap();
                assert_eq!(trial.resolve_rep(result), a.join_concrete(b).unwrap());
            }
        }
    }
    #[test]
    fn alias_bindings_wake_all_connected_numeric_transfers() {
        let mut infer = Infer::new();
        let left = infer.fresh_rep();
        let right = infer.fresh_rep();
        let first = infer.divide_representation(left).unwrap();
        let second = infer.divide_representation(right).unwrap();
        infer.unify_rep(left, right).unwrap();
        let mut projected = Scheme::mono(Type::Scalar(left, Dim::one()));
        infer.attach_numeric_scheme(&mut projected);
        assert_eq!(
            projected.numeric.len(),
            2,
            "an old alias still projects both transfers"
        );
        infer.unify_rep(right, Rep::Float).unwrap();
        assert_eq!(infer.resolve_rep(first), Rep::Float);
        assert_eq!(infer.resolve_rep(second), Rep::Float);
        assert_eq!(infer.numeric_constraint_count(), 0);
    }

    #[test]
    fn type_alias_to_a_scalar_reindexes_the_literal_domain() {
        let mut infer = Infer::new();
        let literal = infer.fresh_type();
        let alias = infer.fresh_type();
        let rep = infer.fresh_rep();
        infer.mark_literal(&literal, LiteralKind::Decimal);
        infer.unify(&literal, &alias).unwrap();
        infer.unify(&alias, &Type::Scalar(rep, Dim::one())).unwrap();
        assert!(!infer.representation_domain(rep).unwrap().contains(Rep::Int));
        assert!(infer.unify_rep(rep, Rep::Int).is_err());
    }

    #[test]
    fn compressed_type_aliases_keep_the_same_live_root_and_numeric_domain() {
        let mut infer = Infer::new();
        let vars = (0..256).map(|_| infer.fresh_type()).collect::<Vec<_>>();
        infer.mark_literal(&vars[0], LiteralKind::Decimal);
        for pair in vars.windows(2) {
            infer.unify(&pair[0], &pair[1]).unwrap();
        }
        let root = vars.last().unwrap();
        infer.compress_type_aliases();
        for alias in &vars {
            assert_eq!(&infer.resolve(alias), root);
            if let Type::Var(id) = alias
                && alias != root
            {
                assert_eq!(infer.subst.get(id).map(Arc::as_ref), Some(root));
            }
        }
        let rep = infer.fresh_rep();
        infer.unify(root, &Type::Scalar(rep, Dim::one())).unwrap();
        assert!(!infer.representation_domain(rep).unwrap().contains(Rep::Int));
        infer.unify_rep(rep, Rep::Float).unwrap();
        assert!(
            vars.iter()
                .all(|ty| infer.resolve(ty) == Type::float(Dim::one()))
        );
    }

    #[test]
    fn shared_dimensions_preserve_schema_without_connecting_reading_decisions() {
        let mut infer = Infer::new();
        infer.enter_level();
        let dimension = infer.fresh_dim();
        let left = infer.fresh_rep();
        let right = infer.fresh_rep();
        let output = infer.divide_representation(left).unwrap();
        infer.mark_literal(
            &Type::Scalar(left, dimension.clone()),
            LiteralKind::QuantityInt,
        );
        infer.mark_literal(
            &Type::Scalar(right, dimension.clone()),
            LiteralKind::QuantityDecimal,
        );
        infer.leave_level();
        let scheme = infer.generalize(&Type::func(
            vec![Type::Scalar(left, dimension.clone())],
            Type::Scalar(output, dimension.clone()),
        ));
        assert!(!scheme.dims.is_empty());
        assert_eq!(scheme.numeric.len(), 2);
        assert!(scheme.numeric.iter().all(|fact| {
            !Infer::numeric_variables(fact).contains(&match right {
                | Rep::Var(id) => id,
                | _ => unreachable!(),
            })
        }));
        let mut bindings = crate::TypeSubstitution::default();
        let mut target = Infer::new();
        bindings.freshen(&scheme.ty, &mut target);
        let instantiated = bindings.apply(&scheme.ty);
        for clause in &scheme.numeric {
            for ty in clause.types() {
                bindings.freshen(&ty, &mut target);
            }
            target
                .add_numeric_constraint(clause.substitute(&bindings))
                .unwrap();
        }
        let Type::Fn(params, ret) = instantiated else {
            panic!("function")
        };
        target
            .unify(&params[0], &Type::float(Dim::base("Length")))
            .unwrap();
        assert_eq!(target.resolve(&ret), Type::float(Dim::base("Length")));
        assert!(matches!(infer.resolve_rep(right), Rep::Var(_)));
    }

    fn drain_deep_tensor_type(mut ty: Type) {
        while let Type::Tensor { elem, .. } = ty {
            ty = *elem;
        }
    }

    fn build_deep_tensor(
        depth: usize,
        leaf: Type,
    ) -> Type {
        let mut current = leaf;
        for step in 0..depth {
            let extent = (step % 4) + 1;
            current = Type::Tensor {
                shape: Shape::Refined(vec![Some(Nat::Lit(extent as u64))]),
                elem: Box::new(current),
            };
        }
        current
    }

    #[test]
    fn deep_nested_tensor_unification_accepts_equal_and_rejects_mismatch() {
        let mut infer = Infer::new();
        let deep_a = build_deep_tensor(4096, Type::float(Dim::one()));
        let deep_b = build_deep_tensor(4096, Type::float(Dim::one()));
        infer.unify(&deep_a, &deep_b).unwrap();

        let bound_a = build_deep_tensor(1024, Type::float(Dim::one()));
        let bound_b = build_deep_tensor(1024, Type::float(Dim::one()));
        let var_a = infer.fresh_type();
        let var_b = infer.fresh_type();
        infer.unify(&var_a, &bound_a).unwrap();
        infer.unify(&var_b, &bound_b).unwrap();
        infer.unify(&var_a, &var_b).unwrap();
        // Repeat hits `unified_vars` directly.
        infer.unify(&var_a, &var_b).unwrap();

        let bad_leaf = build_deep_tensor(4096, Type::int());
        assert!(infer.unify(&deep_a, &bad_leaf).is_err());

        let bad_shape = Type::Tensor {
            shape: Shape::Refined(vec![Some(Nat::Lit(99))]),
            elem: Box::new(build_deep_tensor(4095, Type::float(Dim::one()))),
        };
        assert!(infer.unify(&deep_a, &bad_shape).is_err());

        // Drop deep monolithic trees iteratively.
        drop(infer);
        drain_deep_tensor_type(deep_a);
        drain_deep_tensor_type(deep_b);
        drain_deep_tensor_type(bound_a);
        drain_deep_tensor_type(bound_b);
        drain_deep_tensor_type(bad_leaf);
        drain_deep_tensor_type(bad_shape);
    }

    #[test]
    fn deep_var_bound_tensor_chain_exercises_trampoline_without_stack_overflow() {
        let mut infer = Infer::new();
        let mut head_a = Type::float(Dim::one());
        let mut head_b = Type::float(Dim::one());
        for step in 0..4096 {
            let extent = Some(Nat::Lit(((step % 3) + 2) as u64));
            let va = infer.fresh_type();
            let vb = infer.fresh_type();
            infer
                .unify(
                    &va,
                    &Type::Tensor {
                        shape: Shape::Refined(vec![extent.clone()]),
                        elem: Box::new(head_a),
                    },
                )
                .unwrap();
            infer
                .unify(
                    &vb,
                    &Type::Tensor {
                        shape: Shape::Refined(vec![extent]),
                        elem: Box::new(head_b),
                    },
                )
                .unwrap();
            head_a = va;
            head_b = vb;
        }
        infer.unify(&head_a, &head_b).unwrap();

        // Also verify mixed Var-chain against monolithic deep tensor (triggers depth >= 64 trampoline).
        let monolithic = build_deep_tensor(4096, Type::float(Dim::one()));
        let mut mixed = Type::float(Dim::one());
        for step in 0..4096 {
            let extent = Some(Nat::Lit(((step % 4) + 1) as u64));
            let v = infer.fresh_type();
            infer
                .unify(
                    &v,
                    &Type::Tensor {
                        shape: Shape::Refined(vec![extent]),
                        elem: Box::new(mixed),
                    },
                )
                .unwrap();
            mixed = v;
        }
        infer.unify(&mixed, &monolithic).unwrap();
        drain_deep_tensor_type(monolithic);
    }

    #[test]
    fn shared_dag_tensor_unification_deduplicates_paths_and_respects_restore() {
        let mut infer = Infer::new();
        let build_dag = |infer: &mut Infer, leaf: Type| -> Type {
            let mut cur = infer.fresh_type();
            infer.unify(&cur, &leaf).unwrap();
            for depth in 0..64 {
                let next = infer.fresh_type();
                let tensor = Type::Tensor {
                    shape: Shape::Refined(vec![Some(Nat::Lit((depth % 5) + 1))]),
                    elem: Box::new(cur.clone()),
                };
                infer
                    .unify(
                        &next,
                        &Type::func(vec![tensor.clone(), tensor.clone()], tensor),
                    )
                    .unwrap();
                cur = next;
            }
            cur
        };

        let left = build_dag(&mut infer, Type::float(Dim::one()));
        let right = build_dag(&mut infer, Type::float(Dim::one()));
        let bad = build_dag(&mut infer, Type::int());

        let snapshot = infer.clone();
        infer.unify(&left, &right).unwrap();
        // Repeat after caching `unified_vars`.
        infer.unify(&left, &right).unwrap();
        // Mismatch deep at bottom of 2^64 paths still fails cleanly.
        assert!(infer.unify(&left, &bad).is_err());

        // Restoring snapshot rolls back `unified_vars` too, so re-unifying still validates cleanly.
        infer.restore(&snapshot);
        assert!(infer.unify(&left, &bad).is_err());
        infer.restore(&snapshot);
        infer.unify(&left, &right).unwrap();
    }
}
