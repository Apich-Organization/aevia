//! Unification, instantiation, generalisation and defaulting.
//!
//! Types, dimensions and rows are solved by one engine because they are
//! entangled: `Real[L/T]` can only be unified once its dimension variable is
//! solved, and a record field type can itself be a dimensioned scalar. The
//! dimension half delegates to [`aevia_dim::DimUnifier`], which implements the
//! abelian-group unification of §4.3.
//!
//! Generalisation uses levels rather than a free-variable scan over the
//! environment: a variable created at level `l` may be quantified when the
//! binding sits at a level lower than `l`. This is the standard efficient
//! formulation and keeps `let` inference linear in practice.

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
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;

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
        /// The left term.
        left: Nat,
        /// The right term.
        right: Nat,
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
            | Self::NatNonLinear { left, right } => {
                write!(
                    f,
                    "type-level equation is not linear: {left} = {right}; only literals, `+` and `*` by a literal are supported"
                )
            },
        }
    }
}

/// A linear form over type-level `Nat` variables: `Σ cᵢ·vᵢ + c`.
///
/// Equations are solved by turning them into `left - right = 0` and isolating
/// the single remaining variable. Two or more variables leave the equation
/// pending; a zero-variable form either holds or is inconsistent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NatLinear {
    coeffs: BTreeMap<u32, i128>,
    constant: i128,
}

impl NatLinear {
    fn constant(value: i128) -> Self {
        Self {
            coeffs: BTreeMap::new(),
            constant: value,
        }
    }

    fn variable(id: u32) -> Self {
        let mut coeffs = BTreeMap::new();
        coeffs.insert(id, 1);
        Self { coeffs, constant: 0 }
    }

    fn add(
        mut self,
        other: Self,
    ) -> Self {
        for (id, coefficient) in other.coeffs {
            *self.coeffs.entry(id).or_insert(0) += coefficient;
        }
        self.constant += other.constant;
        self
    }

    fn sub(
        self,
        other: Self,
    ) -> Self {
        self.add(other.negate())
    }

    fn negate(mut self) -> Self {
        self.constant = -self.constant;
        for coefficient in self.coeffs.values_mut() {
            *coefficient = -*coefficient;
        }
        self
    }

    fn scale(
        mut self,
        factor: i128,
    ) -> Self {
        self.constant *= factor;
        for coefficient in self.coeffs.values_mut() {
            *coefficient *= factor;
        }
        self
    }

    fn normalised(mut self) -> Self {
        self.coeffs.retain(|_, coefficient| *coefficient != 0);
        self
    }
}

/// The inference engine state for one module.
#[derive(Clone, Debug)]
pub struct Infer {
    subst: HashMap<u32, Type>,
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
    /// `ρ ⊒ σ` lower bounds recorded by `join` (§3.4.1, PLAN D4).
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
    /// `ρ lacks l` side conditions on row variables (§3.6.1, PLAN D5).
    lacks: HashMap<u32, Vec<String>>,
    /// Type-level natural-number substitution (§3.5.1).
    nat_subst: HashMap<u32, Nat>,
    /// Level of each type-level natural-number variable, for generalisation.
    nat_levels: HashMap<u32, u32>,
    /// Linear equations that mention two or more Nat variables. They cannot be
    /// solved yet; after every new binding they are retried. This is what lets
    /// `n + 1 ~ 3` and `2 * n ~ 6` work without a full decision procedure.
    nat_pending: Vec<NatLinear>,
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
            nat_pending: Vec::new(),
            next: 0,
            level: 0,
        }
    }

    fn fresh_id(&mut self) -> u32 {
        let id = self.next;
        self.next += 1;
        id
    }

    /// The current generalisation level.
    #[must_use]
    pub fn level(&self) -> u32 {
        self.level
    }

    /// Enters a nested binding level.
    pub fn enter_level(&mut self) {
        self.level += 1;
    }

    #[must_use]
    pub fn current_level(&self) -> u32 {
        self.level
    }

    /// Leaves a binding level.
    pub fn leave_level(&mut self) {
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

    /// A fresh *weak* dimension variable (§3.3.3, PLAN D7).
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
        free.freshen(&self.resolve(ty), &mut Infer::new());
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
        let mut dependencies: Vec<_> = Self::numeric_variables(&constraint).into_iter().collect();
        dependencies.sort_unstable();
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
        constraint: NumericConstraint,
    ) {
        let constraint = self.resolve_numeric(&constraint);
        let dependencies = Self::numeric_variables(&constraint).into_iter().collect();
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
            let dependencies = Self::numeric_variables(&constraint).into_iter().collect();
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
                self.push_numeric(constraint);
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

    /// Register transported numeric semantics in this inference universe.
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
            if let Type::Scalar(_, dim) = self.resolve(ty) {
                if !kind.is_quantity() {
                    self.unify_dim(&dim, &Dim::one())?;
                }
            }
        } else if !self.contains_numeric(&constraint) {
            self.push_numeric(constraint);
        }
        self.propagate_numeric()
    }

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
                    if let Some(rep) = domain.unique() {
                        if matches!(self.resolve_rep(Rep::Var(id)), Rep::Var(_)) {
                            self.rep_subst.insert(id, rep);
                            self.numeric_changed(id);
                            changed = true;
                        }
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
        let mut names = Infer::new();
        for ty in inputs {
            free.freshen(&self.resolve(ty), &mut names);
        }
        for id in free.types.keys() {
            if !self.weak.contains(id)
                && self.levels.get(id).copied().unwrap_or(0) > level
                && !scheme.vars.contains(id)
            {
                scheme.vars.push(*id);
            }
        }
        for id in free.representations.keys() {
            if !self.weak.contains(id)
                && self.rep_levels.get(id).copied().unwrap_or(0) > level
                && !scheme.vars.contains(id)
            {
                scheme.vars.push(*id);
            }
        }
        self.attach_numeric_scheme(scheme);
    }

    pub fn quantify_numeric_existentials(
        &self,
        scheme: &mut Scheme,
        level: u32,
    ) {
        let mut free = crate::TypeSubstitution::default();
        let mut names = Infer::new();
        for constraint in &scheme.numeric {
            // Private defaulted literal variables are existential, rather
            // than selectable parameters, but each activation needs a fresh
            // copy of them. Other intermediates generalize normally.
            if let NumericConstraint::DefaultLiteral { ty, .. } = constraint {
                free.freshen(ty, &mut names);
            }
        }
        for id in free.representations.keys() {
            if !self.weak.contains(id)
                && self.rep_levels.get(id).copied().unwrap_or(0) > level
                && !scheme.vars.contains(id)
            {
                scheme.vars.push(*id);
            }
        }
    }

    pub fn default_unprotected_numeric(
        &mut self,
        protected: &HashSet<u32>,
    ) -> Result<(), UnifyError> {
        let constraints: Vec<_> = self.numeric.iter().cloned().collect();
        self.default_numeric_literals(&constraints, protected)?;
        self.propagate_numeric()
    }

    pub fn take_numeric_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.numeric_errors)
    }

    pub fn default_numeric_literals(
        &mut self,
        constraints: &[NumericConstraint],
        protected: &HashSet<u32>,
    ) -> Result<bool, UnifyError> {
        self.propagate_numeric()?;
        let mut changed = false;
        let defaults: HashSet<u32> = constraints
            .iter()
            .filter_map(|constraint| {
                match self.resolve_numeric(constraint) {
                    | NumericConstraint::DefaultLiteral { ty, .. } => {
                        Some(Self::numeric_variables(&NumericConstraint::Literal {
                            ty,
                            kind: LiteralKind::Int,
                        }))
                    },
                    | _ => None,
                }
            })
            .flatten()
            .collect();
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
            let free = Self::numeric_variables(&NumericConstraint::Literal {
                ty: ty.clone(),
                kind: *kind,
            });
            if free.is_empty()
                || free.iter().any(|id| {
                    protected.contains(id) || defaults.contains(id) || computed.contains(id)
                })
            {
                continue;
            }
            let preferred = self.literal_kind(&ty).unwrap_or(*kind);
            for rep in [
                Rep::default_for_literal(preferred),
                Rep::Int,
                Rep::Float,
                Rep::Real,
                Rep::Complex,
            ] {
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

    fn numeric_variables(constraint: &NumericConstraint) -> HashSet<u32> {
        let mut free = crate::TypeSubstitution::default();
        let mut names = Infer::new();
        for ty in constraint.types() {
            free.freshen(&ty, &mut names);
        }
        free.types
            .keys()
            .chain(free.representations.keys())
            .copied()
            .collect()
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
            roots.extend(Self::numeric_variables(&NumericConstraint::Literal {
                ty: self.resolve(ty),
                kind: LiteralKind::Int,
            }));
        }
        // Inference allocates all kinds from one identity space, but only
        // type/representation variables connect numeric decisions. Dimensions,
        // extents and rows still travel in each clause's complete type schema.
        for id in &scheme.vars {
            roots.extend(Self::numeric_variables(&NumericConstraint::Literal {
                ty: self.resolve(&Type::Var(*id)),
                kind: LiteralKind::Int,
            }));
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
        let mut trial = Infer::new();
        let mut projection = crate::TypeSubstitution::default();
        projection.freshen(&contract.ty, &mut trial);
        for constraint in &contract.numeric {
            for ty in constraint.types() {
                projection.freshen(&ty, &mut trial);
            }
        }
        for constraint in &contract.numeric {
            trial.add_numeric_constraint(constraint.substitute(&projection))?;
        }
        trial.unify(&projection.apply(&contract.ty), &projection.apply(reading))
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
            | Type::Var(id) => {
                match self.subst.get(id) {
                    | Some(bound) => self.resolve_depth(bound, depth + 1),
                    | None => ty.clone(),
                }
            },
            | Type::Scalar(rep, dim) => {
                Type::Scalar(self.resolve_rep_depth(*rep, depth), self.dims.resolve(dim))
            },
            | Type::Tensor { shape, elem } => {
                Type::Tensor {
                    shape: self.resolve_shape_depth(shape, depth + 1),
                    elem: Box::new(self.resolve_depth(elem, depth + 1)),
                }
            },
            | Type::Fn(params, ret) => {
                Type::Fn(
                    params
                        .iter()
                        .map(|param| self.resolve_depth(param, depth + 1))
                        .collect(),
                    Box::new(self.resolve_depth(ret, depth + 1)),
                )
            },
            | Type::Record(row) => Type::Record(self.resolve_row_depth(row, depth)),
            | Type::Adt(id, args) => {
                Type::Adt(
                    *id,
                    args.iter()
                        .map(|arg| self.resolve_depth(arg, depth + 1))
                        .collect(),
                )
            },
            | Type::Io(inner) => Type::Io(Box::new(self.resolve_depth(inner, depth + 1))),
            | Type::Dyn(rep) => Type::Dyn(self.resolve_rep_depth(*rep, depth)),
            | Type::Point(dim) => Type::Point(self.dims.resolve(dim)),
            | Type::UnitOf(dim) => Type::UnitOf(self.dims.resolve(dim)),
            | Type::TypeRepIntrinsic(inner) => {
                Type::TypeRepIntrinsic(
                    inner
                        .as_ref()
                        .map(|ty| Box::new(self.resolve_depth(ty, depth + 1))),
                )
            },
            | Type::FromValueIntrinsic(inner) => {
                Type::FromValueIntrinsic(
                    inner
                        .as_ref()
                        .map(|ty| Box::new(self.resolve_depth(ty, depth + 1))),
                )
            },
            | Type::TensorIntrinsic(intrinsic) => {
                let mut intrinsic = *intrinsic;
                intrinsic.element = intrinsic
                    .element
                    .map(|rep| self.resolve_rep_depth(rep, depth + 1));
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
        if depth > 128 {
            return rep;
        }
        match rep {
            | Rep::Var(id) => {
                match self.rep_subst.get(&id) {
                    | Some(bound) => self.resolve_rep_depth(*bound, depth + 1),
                    | None => rep,
                }
            },
            | other => other,
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
        match row {
            | Row::Var(id) => {
                match self.row_subst.get(id) {
                    | Some(bound) => self.resolve_row_depth(bound, depth + 1),
                    | None => row.clone(),
                }
            },
            | Row::Extend { label, ty, tail } => {
                Row::Extend {
                    label: label.clone(),
                    ty: Box::new(self.resolve_depth(ty, 0)),
                    tail: Box::new(self.resolve_row_depth(tail, depth + 1)),
                }
            },
            | Row::Empty => Row::Empty,
        }
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
                                .map(|nat| self.resolve_nat_depth(nat, depth + 1))
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
                match self.nat_subst.get(id) {
                    | Some(bound) => self.resolve_nat_depth(&bound.clone(), depth + 1),
                    | None => nat.clone(),
                }
            },
            | Nat::Add(left, right) => {
                let left = self.resolve_nat_depth(left, depth + 1);
                let right = self.resolve_nat_depth(right, depth + 1);
                match (left.as_const(), right.as_const()) {
                    | (Some(left), Some(right)) => Nat::Lit(left.saturating_add(right)),
                    | _ => Nat::Add(Box::new(left), Box::new(right)),
                }
            },
            | Nat::Mul(left, right) => {
                let left = self.resolve_nat_depth(left, depth + 1);
                let right = self.resolve_nat_depth(right, depth + 1);
                match (left.as_const(), right.as_const()) {
                    | (Some(left), Some(right)) => Nat::Lit(left.saturating_mul(right)),
                    | _ => Nat::Mul(Box::new(left), Box::new(right)),
                }
            },
        }
    }

    /// The linear form of a Nat term, or `None` when it is not linear (a
    /// product of two variables, §3.5.1).
    fn nat_linear(
        &self,
        nat: &Nat,
    ) -> Option<NatLinear> {
        match self.resolve_nat(nat) {
            | Nat::Lit(value) => Some(NatLinear::constant(i128::try_from(value).ok()?)),
            | Nat::Var(id) => Some(NatLinear::variable(id)),
            | Nat::Add(left, right) => {
                Some(
                    self.nat_linear(&left)?
                        .add(self.nat_linear(&right)?)
                        .normalised(),
                )
            },
            | Nat::Mul(left, right) => {
                let left = self.nat_linear(&left)?;
                let right = self.nat_linear(&right)?;
                match (left.coeffs.is_empty(), right.coeffs.is_empty()) {
                    | (true, true) => Some(NatLinear::constant(left.constant * right.constant)),
                    | (true, false) => Some(right.scale(left.constant).normalised()),
                    | (false, true) => Some(left.scale(right.constant).normalised()),
                    | (false, false) => None,
                }
            },
        }
    }

    /// Unifies two type-level natural numbers (§3.5.1).
    ///
    /// The difference is put in linear form. One variable isolates (`n + 1 ~ 3`
    /// ⇒ `n = 2`, `2 * n ~ 6` ⇒ `n = 3`); two or more variables keep the
    /// equation pending and it is retried after every later binding; a
    /// variable-free form either holds or is inconsistent.
    ///
    /// # Errors
    ///
    /// [`UnifyError::NatMismatch`] when the equation has no non-negative
    /// integer solution, [`UnifyError::NatNonLinear`] when a product of two
    /// variables appears (the checker maps it to `E0304`).
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
        let (Some(left_form), Some(right_form)) = (self.nat_linear(&left), self.nat_linear(&right))
        else {
            return Err(UnifyError::NatNonLinear { left, right });
        };
        let equation = left_form.sub(right_form).normalised();
        self.apply_nat_equation(equation)
    }

    /// Solves one linear equation, or keeps it pending when it has several
    /// variables.
    fn apply_nat_equation(
        &mut self,
        equation: NatLinear,
    ) -> Result<(), UnifyError> {
        if equation.coeffs.is_empty() {
            return if equation.constant == 0 {
                Ok(())
            } else {
                Err(UnifyError::NatMismatch {
                    left: Nat::Lit(0),
                    right: Nat::Lit(
                        usize::try_from(equation.constant.unsigned_abs()).unwrap_or(usize::MAX),
                    ),
                })
            };
        }
        if equation.coeffs.len() == 1 {
            self.solve_isolated(equation)?;
            return Ok(());
        }
        self.nat_pending.push(equation);
        Ok(())
    }

    /// Solves an equation with exactly one variable, if the solution is a
    /// non-negative integer; binds it and retries the pending equations.
    fn solve_isolated(
        &mut self,
        equation: NatLinear,
    ) -> Result<(), UnifyError> {
        let (&id, &coefficient) = equation
            .coeffs
            .iter()
            .next()
            .expect("the caller checked there is exactly one variable");
        let rhs = -equation.constant;
        if coefficient == 0 || rhs % coefficient != 0 {
            return Err(UnifyError::NatMismatch {
                left: Nat::Var(id),
                right: Nat::Lit(usize::try_from(rhs.unsigned_abs()).unwrap_or(usize::MAX)),
            });
        }
        let value = rhs / coefficient;
        if value < 0 {
            return Err(UnifyError::NatMismatch {
                left: Nat::Var(id),
                right: Nat::Lit(usize::try_from(value.unsigned_abs()).unwrap_or(usize::MAX)),
            });
        }
        self.nat_subst
            .insert(id, Nat::Lit(usize::try_from(value).unwrap_or(usize::MAX)));
        self.solve_pending_nat()
    }

    /// Retries the multi-variable equations; a later binding may have made one
    /// solvable (or inconsistent).
    fn solve_pending_nat(&mut self) -> Result<(), UnifyError> {
        loop {
            let pending = std::mem::take(&mut self.nat_pending);
            let mut progressed = false;
            let mut kept = Vec::new();
            for equation in pending {
                // Substitute the current solution into the stored form.
                let mut reduced = NatLinear::constant(equation.constant);
                for (id, coefficient) in &equation.coeffs {
                    let resolved = self.resolve_nat(&Nat::Var(*id));
                    let Some(form) = self.nat_linear(&resolved) else {
                        return Err(UnifyError::NatNonLinear {
                            left: Nat::Var(*id),
                            right: Nat::Var(*id),
                        });
                    };
                    reduced = reduced.add(form.scale(*coefficient));
                }
                let reduced = reduced.normalised();
                if reduced.coeffs.is_empty() {
                    if reduced.constant != 0 {
                        return Err(UnifyError::NatMismatch {
                            left: Nat::Lit(0),
                            right: Nat::Lit(
                                usize::try_from(reduced.constant.unsigned_abs())
                                    .unwrap_or(usize::MAX),
                            ),
                        });
                    }
                    continue;
                }
                if reduced.coeffs.len() == 1 {
                    self.solve_isolated_without_retry(reduced)?;
                    progressed = true;
                    continue;
                }
                kept.push(reduced);
            }
            self.nat_pending = kept;
            if !progressed {
                return Ok(());
            }
        }
    }

    /// [`Self::solve_isolated`] without re-entering [`Self::solve_pending_nat`]
    /// (the caller is already inside it).
    fn solve_isolated_without_retry(
        &mut self,
        equation: NatLinear,
    ) -> Result<(), UnifyError> {
        let (&id, &coefficient) = equation.coeffs.iter().next().expect("one variable");
        let rhs = -equation.constant;
        if coefficient == 0 || rhs % coefficient != 0 {
            return Err(UnifyError::NatMismatch {
                left: Nat::Var(id),
                right: Nat::Lit(usize::try_from(rhs.unsigned_abs()).unwrap_or(usize::MAX)),
            });
        }
        let value = rhs / coefficient;
        if value < 0 {
            return Err(UnifyError::NatMismatch {
                left: Nat::Var(id),
                right: Nat::Lit(usize::try_from(value.unsigned_abs()).unwrap_or(usize::MAX)),
            });
        }
        self.nat_subst
            .insert(id, Nat::Lit(usize::try_from(value).unwrap_or(usize::MAX)));
        Ok(())
    }

    /// How many multi-variable Nat equations are still unsolved. The checker
    /// reports `E0304` for them at the end of the module (§3.10.2 rule 6).
    #[must_use]
    pub fn unsolved_nat_equations(&self) -> usize {
        self.nat_pending.len()
    }

    /// Removes and returns the number of multi-variable equations still
    /// pending; the checker reports `E0304` for them at the end of a
    /// declaration (§3.10.2 rule 6).
    pub fn take_unsolved_nat_equations(&mut self) -> usize {
        let count = self.nat_pending.len();
        self.nat_pending.clear();
        count
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
        self.unify_inner(left, right)?;
        self.propagate_numeric()
    }

    fn unify_inner(
        &mut self,
        left: &Type,
        right: &Type,
    ) -> Result<(), UnifyError> {
        let left = self.resolve(left);
        let right = self.resolve(right);
        if left == right {
            return Ok(());
        }
        if left.is_error() || right.is_error() {
            return Ok(());
        }
        match (&left, &right) {
            | (Type::Var(id), other) | (other, Type::Var(id)) => self.bind_var(*id, other),
            | (Type::Scalar(rep_a, dim_a), Type::Scalar(rep_b, dim_b)) => {
                self.unify_rep(*rep_a, *rep_b)?;
                self.unify_dim(dim_a, dim_b)
            },
            | (Type::Tensor { shape: sa, elem: ea }, Type::Tensor { shape: sb, elem: eb }) => {
                self.unify_shape(sa, sb)?;
                self.unify(ea, eb)
            },
            | (Type::TensorIntrinsic(a), Type::TensorIntrinsic(b))
                if a.kind != crate::ty::TensorIntrinsic::Fill
                    && b.kind != crate::ty::TensorIntrinsic::Fill =>
            {
                match (a.element, b.element) {
                    | (Some(left), Some(right)) => self.unify_rep(left, right),
                    | (None, None) => Ok(()),
                    | _ => {
                        Err(UnifyError::Mismatch {
                            left: left.clone(),
                            right: right.clone(),
                        })
                    },
                }
            },
            | (Type::TypeRepIntrinsic(Some(a)), Type::TypeRepIntrinsic(Some(b)))
            | (Type::FromValueIntrinsic(Some(a)), Type::FromValueIntrinsic(Some(b))) => {
                self.unify(a, b)
            },
            | (Type::Fn(pa, ra), Type::Fn(pb, rb)) => {
                if pa.len() != pb.len() {
                    return Err(UnifyError::Mismatch {
                        left: left.clone(),
                        right: right.clone(),
                    });
                }
                for (a, b) in pa.iter().zip(pb.iter()) {
                    self.unify(a, b)?;
                }
                self.unify(ra, rb)
            },
            | (Type::Record(ra), Type::Record(rb)) => self.unify_row(ra, rb),
            | (Type::Adt(ia, aa), Type::Adt(ib, ab)) => {
                if !crate::adt::same_type(*ia, *ib) || aa.len() != ab.len() {
                    return Err(UnifyError::Mismatch {
                        left: left.clone(),
                        right: right.clone(),
                    });
                }
                for (a, b) in aa.iter().zip(ab.iter()) {
                    self.unify(a, b)?;
                }
                Ok(())
            },
            | (Type::Io(a), Type::Io(b)) => self.unify(a, b),
            | (Type::Point(a), Type::Point(b)) => self.unify_dim(a, b),
            | (Type::UnitOf(a), Type::UnitOf(b)) => self.unify_dim(a, b),
            | (Type::Dyn(a), Type::Dyn(b)) => self.unify_rep(*a, *b),
            | (Type::Param(a), Type::Param(b)) if a == b => Ok(()),
            | _ => {
                Err(UnifyError::Mismatch {
                    left: left.clone(),
                    right: right.clone(),
                })
            },
        }
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
            self.push_numeric(constraint);
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
            | (Rep::Var(id), other) | (other, Rep::Var(id)) => self.bind_rep(id, other),
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
        if let Some(kind) = self.rep_literals.get(&id).copied() {
            if !kind.accepts_rep(value) {
                return Err(UnifyError::LiteralMismatch {
                    kind,
                    requested: Type::Scalar(value, Dim::one()),
                });
            }
        }
        let level = self.rep_levels.get(&id).copied().unwrap_or(0);
        self.rep_subst.insert(id, value);
        self.numeric_changed(id);
        self.lower_rep_levels(value, level);
        Ok(())
    }

    fn bind_var(
        &mut self,
        id: u32,
        ty: &Type,
    ) -> Result<(), UnifyError> {
        let ty = self.resolve(ty);
        if let Type::Var(other) = ty {
            if other == id {
                return Ok(());
            }
            if let Some(kind) = self.literals.get(&id).copied() {
                self.literals.entry(other).or_insert(kind);
            }
            if self.weak.contains(&id) {
                self.weak.insert(other);
            }
            self.subst.insert(id, Type::Var(other));
            self.numeric_changed(id);
            return Ok(());
        }
        if self.occurs(id, &ty) {
            return Err(UnifyError::Occurs(id));
        }
        if let Some(kind) = self.literals.get(&id).copied() {
            match &ty {
                | Type::Scalar(rep, dim) => {
                    if !kind.is_quantity() && !dim.is_one() {
                        // A bare literal is dimensionless: an unknown dimension
                        // is forced to 1, a concrete one is an error.
                        if dim.is_concrete() || self.dims.unify(dim, &Dim::one()).is_err() {
                            return Err(UnifyError::LiteralMismatch {
                                kind,
                                requested: ty.clone(),
                            });
                        }
                    }
                    let rep = self.resolve_rep(*rep);
                    if let Rep::Var(rep_id) = rep {
                        self.rep_literals.entry(rep_id).or_insert(kind);
                    } else if !kind.accepts_rep(rep) {
                        return Err(UnifyError::LiteralMismatch {
                            kind,
                            requested: ty.clone(),
                        });
                    }
                },
                | _ => {
                    return Err(UnifyError::LiteralMismatch {
                        kind,
                        requested: ty.clone(),
                    });
                },
            }
        }
        let level = self.levels.get(&id).copied().unwrap_or(0);
        self.lower_levels(&ty, level);
        self.lower_numeric_levels(&ty, level);
        self.subst.insert(id, ty);
        self.numeric_changed(id);
        Ok(())
    }

    fn occurs(
        &self,
        id: u32,
        ty: &Type,
    ) -> bool {
        match self.resolve(ty) {
            | Type::Var(other) => other == id,
            | Type::Scalar(_, dim) => dim.contains_var(id),
            | Type::Tensor { elem, .. } => self.occurs(id, &elem),
            | Type::Fn(params, ret) => {
                params.iter().any(|param| self.occurs(id, param)) || self.occurs(id, &ret)
            },
            | Type::Record(row) => self.occurs_row(id, &row),
            | Type::Adt(_, args) => args.iter().any(|arg| self.occurs(id, arg)),
            | Type::Io(inner) => self.occurs(id, &inner),
            | Type::TypeRepIntrinsic(Some(inner)) | Type::FromValueIntrinsic(Some(inner)) => {
                self.occurs(id, &inner)
            },
            | Type::Point(dim) | Type::UnitOf(dim) => dim.contains_var(id),
            | _ => false,
        }
    }

    fn occurs_row(
        &self,
        id: u32,
        row: &Row,
    ) -> bool {
        match self.resolve_row(row) {
            | Row::Empty => false,
            | Row::Var(other) => other == id,
            | Row::Extend { ty, tail, .. } => self.occurs(id, &ty) || self.occurs_row(id, &tail),
        }
    }

    fn lower_levels(
        &mut self,
        ty: &Type,
        level: u32,
    ) {
        match ty {
            | Type::Var(id) => {
                if let Some(existing) = self.levels.get_mut(id) {
                    *existing = (*existing).min(level);
                } else {
                    self.levels.insert(*id, level);
                }
            },
            | Type::Scalar(rep, dim) => {
                self.lower_rep_levels(*rep, level);
                for id in dim.vars() {
                    let entry = self.dim_levels.entry(id).or_insert(level);
                    *entry = (*entry).min(level);
                }
            },
            | Type::Tensor { shape, elem } => {
                if let Shape::Refined(extents) = shape {
                    for extent in extents.iter().flatten() {
                        for id in extent.vars() {
                            let entry = self.nat_levels.entry(id).or_insert(level);
                            *entry = (*entry).min(level);
                        }
                    }
                }
                self.lower_levels(elem, level);
            },
            | Type::Fn(params, ret) => {
                for param in params {
                    self.lower_levels(param, level);
                }
                self.lower_levels(ret, level);
            },
            | Type::Record(row) => self.lower_row_levels(row, level),
            | Type::Adt(_, args) => {
                for arg in args {
                    self.lower_levels(arg, level);
                }
            },
            | Type::Io(inner) => self.lower_levels(inner, level),
            | Type::TypeRepIntrinsic(Some(inner)) | Type::FromValueIntrinsic(Some(inner)) => {
                self.lower_levels(inner, level)
            },
            | Type::TensorIntrinsic(intrinsic) => {
                if let Some(rep) = intrinsic.element {
                    self.lower_rep_levels(rep, level);
                }
            },
            | Type::Dyn(rep) => self.lower_rep_levels(*rep, level),
            | Type::Point(dim) | Type::UnitOf(dim) => {
                for id in dim.vars() {
                    let entry = self.dim_levels.entry(id).or_insert(level);
                    *entry = (*entry).min(level);
                }
            },
            | _ => {},
        }
    }

    fn lower_row_levels(
        &mut self,
        row: &Row,
        level: u32,
    ) {
        match row {
            | Row::Empty => {},
            | Row::Var(id) => {
                let entry = self.row_levels.entry(*id).or_insert(level);
                *entry = (*entry).min(level);
            },
            | Row::Extend { ty, tail, .. } => {
                self.lower_levels(ty, level);
                self.lower_row_levels(tail, level);
            },
        }
    }

    fn lower_numeric_levels(
        &mut self,
        ty: &Type,
        level: u32,
    ) {
        let mut schema = Scheme::mono(ty.clone());
        self.attach_numeric_scheme(&mut schema);
        for constraint in schema.numeric {
            for ty in constraint.types() {
                self.lower_levels(&ty, level);
            }
        }
    }

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

    fn unify_row(
        &mut self,
        left: &Row,
        right: &Row,
    ) -> Result<(), UnifyError> {
        let left = self.resolve_row(left);
        let right = self.resolve_row(right);
        if left == right {
            return Ok(());
        }
        let left_fields = left.fields();
        let right_fields = right.fields();
        let left_tail = left.tail_var();
        let right_tail = right.tail_var();

        for (label, ty) in &left_fields {
            match right_fields.iter().find(|(name, _)| name == label) {
                | Some((_, other)) => self.unify(ty, other)?,
                | None => {
                    let Some(tail) = right_tail else {
                        return Err(UnifyError::MissingField {
                            field: label.clone(),
                            row: right.clone(),
                        });
                    };
                    // Rémy: `tail` becomes `{ l | tail' }`, and `tail'` must
                    // lack `l` — otherwise a later unification could add `l`
                    // a second time (§3.6.1).
                    let fresh = self.fresh_row();
                    if let Some(fresh_id) = row_id(&fresh) {
                        self.require_lacks(fresh_id, label)?;
                    }
                    let extension = Row::Extend {
                        label: label.clone(),
                        ty: Box::new(ty.clone()),
                        tail: Box::new(fresh),
                    };
                    self.bind_row(tail, extension)?;
                    return self.unify_row(&left, &self.resolve_row(&right));
                },
            }
        }
        for (label, ty) in &right_fields {
            if left_fields.iter().any(|(name, _)| name == label) {
                continue;
            }
            let Some(tail) = left_tail else {
                return Err(UnifyError::MissingField {
                    field: label.clone(),
                    row: left.clone(),
                });
            };
            let fresh = self.fresh_row();
            if let Some(fresh_id) = row_id(&fresh) {
                self.require_lacks(fresh_id, label)?;
            }
            let extension = Row::Extend {
                label: label.clone(),
                ty: Box::new(ty.clone()),
                tail: Box::new(fresh),
            };
            self.bind_row(tail, extension)?;
            return self.unify_row(&self.resolve_row(&left), &right);
        }
        match (left_tail, right_tail) {
            | (Some(a), Some(b)) if a != b => {
                // The merged tail keeps both sides' lacks conditions.
                for label in self.lacks.get(&b).cloned().unwrap_or_default() {
                    self.require_lacks(a, &label)?;
                }
                self.bind_row(a, Row::Var(b))
            },
            | (Some(tail), None) | (None, Some(tail)) => self.bind_row(tail, Row::Empty),
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
        if self
            .resolve_row(&Row::Var(id))
            .fields()
            .iter()
            .any(|(name, _)| name == label)
        {
            return Err(UnifyError::DuplicateField {
                field: label.to_string(),
            });
        }
        let labels = self.lacks.entry(id).or_default();
        if !labels.iter().any(|existing| existing == label) {
            labels.push(label.to_string());
        }
        Ok(())
    }

    /// The labels a row variable must not contain.
    #[must_use]
    pub fn lacks_of(
        &self,
        id: u32,
    ) -> Vec<String> {
        self.lacks.get(&id).cloned().unwrap_or_default()
    }

    /// Extends an open row with a new field, returning the field's type.
    ///
    /// # Errors
    ///
    /// Fails when the row cannot absorb the field (a duplicate label).
    pub fn extend_row(
        &mut self,
        tail: u32,
        label: &str,
        ty: Type,
    ) -> Result<(), UnifyError> {
        let fresh = self.fresh_row();
        if let Some(fresh_id) = row_id(&fresh) {
            self.require_lacks(fresh_id, label)?;
        }
        self.bind_row(
            tail,
            Row::Extend {
                label: label.to_string(),
                ty: Box::new(ty),
                tail: Box::new(fresh),
            },
        )
    }

    fn bind_row(
        &mut self,
        id: u32,
        row: Row,
    ) -> Result<(), UnifyError> {
        let row = self.resolve_row(&row);
        if let Row::Var(other) = row {
            if other == id {
                return Ok(());
            }
            for label in self.lacks.get(&id).cloned().unwrap_or_default() {
                let labels = self.lacks.entry(other).or_default();
                if !labels.iter().any(|existing| existing == &label) {
                    labels.push(label);
                }
            }
            if self.weak.contains(&id) {
                self.weak.insert(other);
            }
            self.row_subst.insert(id, Row::Var(other));
            return Ok(());
        }
        for label in self.lacks.get(&id).cloned().unwrap_or_default() {
            if row.fields().iter().any(|(name, _)| name == &label) {
                return Err(UnifyError::DuplicateField { field: label });
            }
        }
        if self.row_occurs(id, &row) {
            return Err(UnifyError::Occurs(id));
        }
        let level = self.row_levels.get(&id).copied().unwrap_or(0);
        self.lower_row_levels(&row, level);
        self.row_subst.insert(id, row);
        Ok(())
    }

    fn row_occurs(
        &self,
        id: u32,
        row: &Row,
    ) -> bool {
        match self.resolve_row(row) {
            | Row::Empty => false,
            | Row::Var(other) => other == id,
            | Row::Extend { ty, tail, .. } => self.occurs(id, &ty) || self.row_occurs(id, &tail),
        }
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
                if let Rep::Var(id) = rep {
                    if rep_var(*id) && !vars.contains(id) {
                        vars.push(*id);
                    }
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
                            let free = match resolved {
                                | Nat::Var(free) => free,
                                | _ => continue,
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
            | Type::Io(inner) => {
                self.collect_generalizable(inner, level, vars, dims, rows, nats);
            },
            | Type::TypeRepIntrinsic(Some(inner)) | Type::FromValueIntrinsic(Some(inner)) => {
                self.collect_generalizable(inner, level, vars, dims, rows, nats);
            },
            | Type::TensorIntrinsic(intrinsic) => {
                if let Some(Rep::Var(id)) = intrinsic.element {
                    if rep_var(id) && !vars.contains(&id) {
                        vars.push(id);
                    }
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
        match row {
            | Row::Empty => {},
            | Row::Var(id) => {
                if !self.weak.contains(id)
                    && self.row_levels.get(id).copied().unwrap_or(0) > level
                    && !rows.contains(id)
                {
                    rows.push(*id);
                }
            },
            | Row::Extend { ty, tail, .. } => {
                self.collect_generalizable(ty, level, vars, dims, rows, nats);
                self.collect_row(tail, level, vars, dims, rows, nats);
            },
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
    /// This is the module-end step of PLAN D4: generalisation runs first, so
    /// `<d>`-polymorphic functions keep their dimension variables and only
    /// weak and literal variables are defaulted.
    pub fn default_scheme(
        &mut self,
        scheme: &Scheme,
    ) -> (Scheme, Vec<u32>) {
        self.default_scheme_preserving(scheme, &crate::TypeSubstitution::default())
    }

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
                        self.subst.insert(id, defaulted.clone());
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
                    elem: Box::new(self.default_inner(&elem, scheme, warnings, depth + 1)),
                }
            },
            | Type::Fn(params, ret) => {
                Type::Fn(
                    params
                        .iter()
                        .map(|param| self.default_inner(param, scheme, warnings, depth + 1))
                        .collect(),
                    Box::new(self.default_inner(&ret, scheme, warnings, depth + 1)),
                )
            },
            | Type::Record(row) => Type::Record(self.default_row(&row, scheme, warnings, depth)),
            | Type::Adt(id, args) => {
                Type::Adt(
                    id,
                    args.iter()
                        .map(|arg| self.default_inner(arg, scheme, warnings, depth + 1))
                        .collect(),
                )
            },
            | Type::Io(inner) => {
                Type::Io(Box::new(self.default_inner(
                    &inner,
                    scheme,
                    warnings,
                    depth + 1,
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
                        .map(|ty| Box::new(self.default_inner(ty, scheme, warnings, depth + 1))),
                )
            },
            | Type::FromValueIntrinsic(inner) => {
                Type::FromValueIntrinsic(
                    inner
                        .as_ref()
                        .map(|ty| Box::new(self.default_inner(ty, scheme, warnings, depth + 1))),
                )
            },
            | Type::TensorIntrinsic(mut intrinsic) => {
                if let Some(rep) = intrinsic.element {
                    if let Type::Scalar(rep, _) = self.default_inner(
                        &Type::Scalar(rep, Dim::one()),
                        scheme,
                        warnings,
                        depth + 1,
                    ) {
                        intrinsic.element = Some(rep);
                    }
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
        let literal = self.rep_literals.get(&id).copied();
        self.choose_representation(id, literal)
            .unwrap_or_else(|| literal.map_or(Rep::Real, Rep::default_for_literal))
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
        match self.resolve_row(row) {
            | Row::Empty => Row::Empty,
            | Row::Var(id) => {
                if scheme.rows.contains(&id) || !self.above_floor(id) {
                    Row::Var(id)
                } else {
                    Row::Empty
                }
            },
            | Row::Extend { label, ty, tail } => {
                Row::Extend {
                    label,
                    ty: Box::new(self.default_inner(&ty, scheme, warnings, depth + 1)),
                    tail: Box::new(self.default_row(&tail, scheme, warnings, depth)),
                }
            },
        }
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
    pub fn variable_count(&self) -> u32 {
        self.next
    }
}

fn row_id(row: &Row) -> Option<u32> {
    match row {
        | Row::Var(id) => Some(*id),
        | _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let tail = infer.fresh_row();
        let open = Type::Record(Row::Extend {
            label: "x".to_string(),
            ty: Box::new(Type::int()),
            tail: Box::new(tail.clone()),
        });
        let closed = Type::record([
            ("x".to_string(), Type::int()),
            ("y".to_string(), Type::int()),
        ]);
        infer.unify(&open, &closed).expect("open row absorbs y");
        let Row::Var(id) = tail else {
            panic!("fresh_row is a variable");
        };
        let resolved = infer.resolve_row(&Row::Var(id));
        assert_eq!(
            resolved.tail_var(),
            None,
            "equality with a closed row closes its tail"
        );
        assert!(
            infer
                .lacks
                .values()
                .any(|labels| labels.contains(&"y".to_string())),
            "the consumed field still records its lacks condition"
        );
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
        // §3.4.1 / PLAN D4.
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
}
