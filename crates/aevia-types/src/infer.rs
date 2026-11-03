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

use crate::ty::{Constraint, LiteralKind, Rep, Row, Scheme, Shape, Type};
use aevia_dim::{Dim, DimMismatch, DimUnifier};
use std::collections::{HashMap, HashSet};

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
}

impl std::fmt::Display for UnifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mismatch { left, right } => write!(f, "expected {left}, found {right}"),
            Self::Dim(mismatch) => write!(
                f,
                "dimension mismatch: expected {}, found {}",
                mismatch.right.to_plain_string(),
                mismatch.left.to_plain_string()
            ),
            Self::Occurs(id) => write!(f, "infinite type: t{id} occurs in its own definition"),
            Self::MissingField { field, row } => {
                write!(f, "the record {{ {row} }} has no field `{field}`")
            }
            Self::DuplicateField { field } => {
                write!(f, "the record would have two fields named `{field}`")
            }
            Self::ShapeMismatch { left, right } => {
                write!(f, "shape mismatch: expected {left}, found {right}")
            }
            Self::LiteralMismatch { kind, requested } => match kind {
                LiteralKind::Int | LiteralKind::Decimal => write!(
                    f,
                    "a bare numeric literal cannot have type {requested}; write a unit (for example `1 m`)"
                ),
                _ => write!(f, "the literal cannot have type {requested}"),
            },
        }
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
    /// `ρ lacks l` side conditions on row variables (§3.6.1, PLAN D5).
    lacks: HashMap<u32, Vec<String>>,
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
            lacks: HashMap::new(),
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

    /// Whether a variable is weak.
    #[must_use]
    pub fn is_weak(&self, id: u32) -> bool {
        self.weak.contains(&id)
    }

    /// Whether a dimension variable is weak, directly or through a
    /// substitution: `let f = fn t => x * t` makes `t`'s dimension weak too.
    #[must_use]
    pub fn is_weak_dim(&self, id: u32) -> bool {
        if self.weak.contains(&id) {
            return true;
        }
        self.weak.iter().any(|weak| {
            self.dims
                .resolve(&Dim::var(*weak))
                .contains_var(id)
        })
    }

    /// Records that a type variable came from a literal.
    pub fn mark_literal(&mut self, ty: &Type, kind: LiteralKind) {
        match ty {
            Type::Var(id) => {
                self.literals.insert(*id, kind);
            }
            Type::Scalar(Rep::Var(id), _) => {
                self.rep_literals.insert(*id, kind);
            }
            _ => {}
        }
    }

    /// The literal kind attached to a representation variable, if any.
    #[must_use]
    pub fn rep_literal_kind(&self, rep: Rep) -> Option<LiteralKind> {
        match rep {
            Rep::Var(id) => self.rep_literals.get(&id).copied(),
            _ => None,
        }
    }

    /// The literal kind attached to a variable, if any.
    #[must_use]
    pub fn literal_kind(&self, ty: &Type) -> Option<LiteralKind> {
        match ty {
            Type::Var(id) => self.literals.get(id).copied(),
            Type::Scalar(Rep::Var(id), _) => self.rep_literals.get(id).copied(),
            _ => None,
        }
    }

    /// Resolves a type through all substitutions.
    #[must_use]
    pub fn resolve(&self, ty: &Type) -> Type {
        self.resolve_depth(ty, 0)
    }

    fn resolve_depth(&self, ty: &Type, depth: u32) -> Type {
        if depth > 128 {
            return ty.clone();
        }
        match ty {
            Type::Var(id) => match self.subst.get(id) {
                Some(bound) => self.resolve_depth(bound, depth + 1),
                None => ty.clone(),
            },
            Type::Scalar(rep, dim) => {
                Type::Scalar(self.resolve_rep_depth(*rep, depth), self.dims.resolve(dim))
            }
            Type::Tensor { shape, elem } => Type::Tensor {
                shape: shape.clone(),
                elem: Box::new(self.resolve_depth(elem, depth + 1)),
            },
            Type::Fn(params, ret) => Type::Fn(
                params
                    .iter()
                    .map(|param| self.resolve_depth(param, depth + 1))
                    .collect(),
                Box::new(self.resolve_depth(ret, depth + 1)),
            ),
            Type::Record(row) => Type::Record(self.resolve_row_depth(row, depth)),
            Type::Adt(id, args) => Type::Adt(
                *id,
                args.iter()
                    .map(|arg| self.resolve_depth(arg, depth + 1))
                    .collect(),
            ),
            Type::Io(inner) => Type::Io(Box::new(self.resolve_depth(inner, depth + 1))),
            Type::Dyn(rep) => Type::Dyn(self.resolve_rep_depth(*rep, depth)),
            _ => ty.clone(),
        }
    }

    /// Resolves a dimension through the dimension substitution.
    #[must_use]
    pub fn resolve_dim(&self, dim: &Dim) -> Dim {
        self.dims.resolve(dim)
    }

    /// Resolves a representation through substitutions.
    #[must_use]
    pub fn resolve_rep(&self, rep: Rep) -> Rep {
        self.resolve_rep_depth(rep, 0)
    }

    fn resolve_rep_depth(&self, rep: Rep, depth: u32) -> Rep {
        if depth > 128 {
            return rep;
        }
        match rep {
            Rep::Var(id) => match self.rep_subst.get(&id) {
                Some(bound) => self.resolve_rep_depth(*bound, depth + 1),
                None => rep,
            },
            other => other,
        }
    }

    /// Resolves a row through substitutions.
    #[must_use]
    pub fn resolve_row(&self, row: &Row) -> Row {
        self.resolve_row_depth(row, 0)
    }

    fn resolve_row_depth(&self, row: &Row, depth: u32) -> Row {
        if depth > 128 {
            return row.clone();
        }
        match row {
            Row::Var(id) => match self.row_subst.get(id) {
                Some(bound) => self.resolve_row_depth(bound, depth + 1),
                None => row.clone(),
            },
            Row::Extend { label, ty, tail } => Row::Extend {
                label: label.clone(),
                ty: Box::new(self.resolve_depth(ty, 0)),
                tail: Box::new(self.resolve_row_depth(tail, depth + 1)),
            },
            Row::Empty => Row::Empty,
        }
    }

    /// Unifies two types.
    ///
    /// # Errors
    ///
    /// Returns a [`UnifyError`] describing the first difference found.
    pub fn unify(&mut self, left: &Type, right: &Type) -> Result<(), UnifyError> {
        let left = self.resolve(left);
        let right = self.resolve(right);
        if left == right {
            return Ok(());
        }
        if left.is_error() || right.is_error() {
            return Ok(());
        }
        match (&left, &right) {
            (Type::Var(id), other) | (other, Type::Var(id)) => self.bind_var(*id, other),
            (Type::Scalar(rep_a, dim_a), Type::Scalar(rep_b, dim_b)) => {
                self.unify_rep(*rep_a, *rep_b)?;
                self.unify_dim(dim_a, dim_b)
            }
            (Type::Tensor { shape: sa, elem: ea }, Type::Tensor { shape: sb, elem: eb }) => {
                self.unify_shape(sa, sb)?;
                self.unify(ea, eb)
            }
            (Type::Fn(pa, ra), Type::Fn(pb, rb)) => {
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
            }
            (Type::Record(ra), Type::Record(rb)) => self.unify_row(ra, rb),
            (Type::Adt(ia, aa), Type::Adt(ib, ab)) => {
                if ia != ib || aa.len() != ab.len() {
                    return Err(UnifyError::Mismatch {
                        left: left.clone(),
                        right: right.clone(),
                    });
                }
                for (a, b) in aa.iter().zip(ab.iter()) {
                    self.unify(a, b)?;
                }
                Ok(())
            }
            (Type::Io(a), Type::Io(b)) => self.unify(a, b),
            (Type::Point(a), Type::Point(b)) => self.unify_dim(a, b),
            (Type::UnitOf(a), Type::UnitOf(b)) => self.unify_dim(a, b),
            (Type::Dyn(a), Type::Dyn(b)) => self.unify_rep(*a, *b),
            (Type::Param(a), Type::Param(b)) if a == b => Ok(()),
            _ => Err(UnifyError::Mismatch {
                left: left.clone(),
                right: right.clone(),
            }),
        }
    }

    /// Unifies two dimensions.
    ///
    /// # Errors
    ///
    /// Returns the residual mismatch from [`DimUnifier::unify`].
    pub fn unify_dim(&mut self, left: &Dim, right: &Dim) -> Result<(), UnifyError> {
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
        self.dims
            .unify(&left, &right)
            .map_err(UnifyError::Dim)?;
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
    pub fn constrain_rep(&mut self, variable: Rep, other: Rep) -> Result<(), UnifyError> {
        let variable = self.resolve_rep(variable);
        let other = self.resolve_rep(other);
        if variable == other {
            return Ok(());
        }
        match (variable, other) {
            (Rep::Var(id), lower) => {
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
            }
            (concrete, lower) => {
                if concrete.at_least(lower) {
                    Ok(())
                } else {
                    Err(UnifyError::Mismatch {
                        left: Type::Scalar(concrete, Dim::one()),
                        right: Type::Scalar(lower, Dim::one()),
                    })
                }
            }
        }
    }

    /// The lower bounds recorded for a representation variable.
    #[must_use]
    pub fn rep_bounds(&self, rep: Rep) -> Vec<Rep> {
        match self.resolve_rep(rep) {
            Rep::Var(id) => self.rep_lower.get(&id).cloned().unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    pub fn unify_rep(&mut self, left: Rep, right: Rep) -> Result<(), UnifyError> {
        let left = self.resolve_rep(left);
        let right = self.resolve_rep(right);
        if left == right {
            return Ok(());
        }
        match (left, right) {
            (Rep::Var(id), other) | (other, Rep::Var(id)) => self.bind_rep(id, other),
            _ => Err(UnifyError::Mismatch {
                left: Type::Scalar(left, Dim::one()),
                right: Type::Scalar(right, Dim::one()),
            }),
        }
    }

    fn bind_rep(&mut self, id: u32, value: Rep) -> Result<(), UnifyError> {
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
            let level = self.rep_levels.get(&id).copied().unwrap_or(0);
            self.rep_subst.insert(id, Rep::Var(other));
            self.rep_levels.insert(other, level.min(
                self.rep_levels.get(&other).copied().unwrap_or(u32::MAX),
            ));
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
        self.lower_rep_levels(value, level);
        Ok(())
    }

    fn bind_var(&mut self, id: u32, ty: &Type) -> Result<(), UnifyError> {
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
            return Ok(());
        }
        if self.occurs(id, &ty) {
            return Err(UnifyError::Occurs(id));
        }
        if let Some(kind) = self.literals.get(&id).copied() {
            match &ty {
                Type::Scalar(rep, dim) => {
                    if !kind.is_quantity() && !dim.is_one() {
                        // A bare literal is dimensionless: an unknown dimension
                        // is forced to 1, a concrete one is an error.
                        if dim.is_concrete()
                            || self.dims.unify(dim, &Dim::one()).is_err()
                        {
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
                }
                _ => {
                    return Err(UnifyError::LiteralMismatch {
                        kind,
                        requested: ty.clone(),
                    });
                }
            }
        }
        let level = self.levels.get(&id).copied().unwrap_or(0);
        self.lower_levels(&ty, level);
        self.subst.insert(id, ty);
        Ok(())
    }

    fn occurs(&self, id: u32, ty: &Type) -> bool {
        match self.resolve(ty) {
            Type::Var(other) => other == id,
            Type::Scalar(_, dim) => dim.contains_var(id),
            Type::Tensor { elem, .. } => self.occurs(id, &elem),
            Type::Fn(params, ret) => {
                params.iter().any(|param| self.occurs(id, param)) || self.occurs(id, &ret)
            }
            Type::Record(row) => self.occurs_row(id, &row),
            Type::Adt(_, args) => args.iter().any(|arg| self.occurs(id, arg)),
            Type::Io(inner) => self.occurs(id, &inner),
            Type::Point(dim) | Type::UnitOf(dim) => dim.contains_var(id),
            _ => false,
        }
    }

    fn occurs_row(&self, id: u32, row: &Row) -> bool {
        match self.resolve_row(row) {
            Row::Empty => false,
            Row::Var(other) => other == id,
            Row::Extend { ty, tail, .. } => {
                self.occurs(id, &ty) || self.occurs_row(id, &tail)
            }
        }
    }

    fn lower_levels(&mut self, ty: &Type, level: u32) {
        match ty {
            Type::Var(id) => {
                if let Some(existing) = self.levels.get_mut(id) {
                    *existing = (*existing).min(level);
                } else {
                    self.levels.insert(*id, level);
                }
            }
            Type::Scalar(rep, dim) => {
                self.lower_rep_levels(*rep, level);
                for id in dim.vars() {
                    let entry = self.dim_levels.entry(id).or_insert(level);
                    *entry = (*entry).min(level);
                }
            }
            Type::Tensor { elem, .. } => self.lower_levels(elem, level),
            Type::Fn(params, ret) => {
                for param in params {
                    self.lower_levels(param, level);
                }
                self.lower_levels(ret, level);
            }
            Type::Record(row) => self.lower_row_levels(row, level),
            Type::Adt(_, args) => {
                for arg in args {
                    self.lower_levels(arg, level);
                }
            }
            Type::Io(inner) => self.lower_levels(inner, level),
            Type::Point(dim) | Type::UnitOf(dim) => {
                for id in dim.vars() {
                    let entry = self.dim_levels.entry(id).or_insert(level);
                    *entry = (*entry).min(level);
                }
            }
            _ => {}
        }
    }

    fn lower_row_levels(&mut self, row: &Row, level: u32) {
        match row {
            Row::Empty => {}
            Row::Var(id) => {
                let entry = self.row_levels.entry(*id).or_insert(level);
                *entry = (*entry).min(level);
            }
            Row::Extend { ty, tail, .. } => {
                self.lower_levels(ty, level);
                self.lower_row_levels(tail, level);
            }
        }
    }

    fn lower_rep_levels(&mut self, rep: Rep, level: u32) {
        if let Rep::Var(id) = rep {
            let entry = self.rep_levels.entry(id).or_insert(level);
            *entry = (*entry).min(level);
        }
    }

    /// Unifies two shapes.
    ///
    /// Refined → rank-only is the safe implicit conversion (§3.5.1): a tensor
    /// whose extents are known is usable where only the rank is known. The
    /// other direction needs `refine_shape` and a runtime check, so a
    /// rank-only shape never satisfies a refined annotation.
    ///
    /// # Errors
    ///
    /// Fails when the ranks differ, when a rank-only shape would have to
    /// become refined, or when two known extents differ.
    pub fn unify_shape(&mut self, left: &Shape, right: &Shape) -> Result<(), UnifyError> {
        match (left, right) {
            (Shape::Rank(a), Shape::Rank(b)) if a == b => Ok(()),
            // Safe: the left side knows more than the right side needs.
            (Shape::Refined(items), Shape::Rank(rank)) => {
                if usize::try_from(*rank).unwrap_or(usize::MAX) == items.len() {
                    Ok(())
                } else {
                    Err(UnifyError::ShapeMismatch {
                        left: left.clone(),
                        right: right.clone(),
                    })
                }
            }
            (Shape::Rank(_), Shape::Refined(_)) => Err(UnifyError::ShapeMismatch {
                left: left.clone(),
                right: right.clone(),
            }),
            (Shape::Refined(a), Shape::Refined(b)) => {
                if a.len() != b.len() {
                    return Err(UnifyError::ShapeMismatch {
                        left: left.clone(),
                        right: right.clone(),
                    });
                }
                for (x, y) in a.iter().zip(b.iter()) {
                    if let (Some(x), Some(y)) = (x, y) {
                        if x != y {
                            return Err(UnifyError::ShapeMismatch {
                                left: left.clone(),
                                right: right.clone(),
                            });
                        }
                    }
                }
                Ok(())
            }
            _ => Err(UnifyError::ShapeMismatch {
                left: left.clone(),
                right: right.clone(),
            }),
        }
    }

    fn unify_row(&mut self, left: &Row, right: &Row) -> Result<(), UnifyError> {
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
                Some((_, other)) => self.unify(ty, other)?,
                None => {
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
                }
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
            (Some(a), Some(b)) if a != b => {
                // The merged tail keeps both sides' lacks conditions.
                for label in self.lacks.get(&b).cloned().unwrap_or_default() {
                    self.require_lacks(a, &label)?;
                }
                self.bind_row(a, Row::Var(b))
            }
            _ => Ok(()),
        }
    }

    /// Records that a row variable must not contain `label` (§3.6.1).
    ///
    /// # Errors
    ///
    /// Fails when the condition is already violated — the row would end up
    /// with two fields of the same name, which is `E0208`.
    pub fn require_lacks(&mut self, id: u32, label: &str) -> Result<(), UnifyError> {
        if self.resolve_row(&Row::Var(id)).fields().iter().any(|(name, _)| name == label) {
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
    pub fn lacks_of(&self, id: u32) -> Vec<String> {
        self.lacks.get(&id).cloned().unwrap_or_default()
    }

    /// Extends an open row with a new field, returning the field's type.
    ///
    /// # Errors
    ///
    /// Fails when the row cannot absorb the field (a duplicate label).
    pub fn extend_row(&mut self, tail: u32, label: &str, ty: Type) -> Result<(), UnifyError> {
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

    fn bind_row(&mut self, id: u32, row: Row) -> Result<(), UnifyError> {
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

    fn row_occurs(&self, id: u32, row: &Row) -> bool {
        match self.resolve_row(row) {
            Row::Empty => false,
            Row::Var(other) => other == id,
            Row::Extend { ty, tail, .. } => {
                self.occurs(id, &ty) || self.row_occurs(id, &tail)
            }
        }
    }

    /// Instantiates a scheme with fresh variables at the current level.
    pub fn instantiate(&mut self, scheme: &Scheme) -> Type {
        self.instantiate_scheme(scheme).0
    }

    /// Instantiates a scheme, also instantiating its class constraints.
    ///
    /// The constraints come back with the same fresh variables, so the caller
    /// can solve them against the instance table (§3.8.2).
    pub fn instantiate_scheme(&mut self, scheme: &Scheme) -> (Type, Vec<Constraint>) {
        if scheme.is_closed() {
            return (scheme.ty.clone(), Vec::new());
        }
        let mut type_map = HashMap::new();
        for id in &scheme.vars {
            let fresh = self.fresh_id();
            self.levels.insert(fresh, self.level);
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
        let ty = substitute(&scheme.ty, &type_map, &dim_map, &row_map);
        let constraints = scheme
            .constraints
            .iter()
            .map(|constraint| Constraint {
                class: constraint.class.clone(),
                args: constraint
                    .args
                    .iter()
                    .map(|arg| substitute(arg, &type_map, &dim_map, &row_map))
                    .collect(),
            })
            .collect();
        (ty, constraints)
    }

    /// Generalises a type over the variables introduced above the current
    /// level.
    ///
    /// Weak variables and literal variables are *not* quantified: the first
    /// are weak by definition (§3.3.3) and the second must default at the end
    /// of the declaration (§3.10.2).
    #[must_use]
    pub fn generalize(&self, ty: &Type) -> Scheme {
        self.generalize_with(ty, Vec::new())
    }

    /// Generalises a type together with the class constraints that belong to
    /// it.
    #[must_use]
    pub fn generalize_with(&self, ty: &Type, constraints: Vec<Constraint>) -> Scheme {
        self.generalize_at(self.level, ty, constraints)
    }

    /// Generalises as if the binding sat at `level`.
    ///
    /// A module-level binding is generalised at level 0 even while the
    /// remaining declarations are still being checked at level 1, because a
    /// later declaration must see its polymorphic type.
    #[must_use]
    pub fn generalize_at(&self, level: u32, ty: &Type, constraints: Vec<Constraint>) -> Scheme {
        let mut vars = Vec::new();
        let mut dims = Vec::new();
        let mut rows = Vec::new();
        self.collect_generalizable(&self.resolve(ty), level, &mut vars, &mut dims, &mut rows);
        // Representation variables are quantified through the type-variable
        // list as well: they are created from the same counter, so a scheme
        // that quantifies `ρ` must quantify its id.
        Scheme {
            vars,
            dims,
            rows,
            constraints,
            ty: self.resolve(ty),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_generalizable(
        &self,
        ty: &Type,
        level: u32,
        vars: &mut Vec<u32>,
        dims: &mut Vec<u32>,
        rows: &mut Vec<u32>,
    ) {
        let type_var = |id: u32| {
            !self.weak.contains(&id)
                && !self.literals.contains_key(&id)
                && self.levels.get(&id).copied().unwrap_or(0) > level
        };
        let rep_var = |id: u32| {
            !self.weak.contains(&id)
                && !self.rep_literals.contains_key(&id)
                && self.rep_levels.get(&id).copied().unwrap_or(0) > level
        };
        let dim_var = |id: u32| {
            !self.is_weak_dim(id) && self.dim_levels.get(&id).copied().unwrap_or(0) > level
        };
        match ty {
            Type::Var(id) => {
                if type_var(*id) && !vars.contains(id) {
                    vars.push(*id);
                }
            }
            Type::Scalar(rep, dim) => {
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
            }
            Type::Tensor { elem, .. } => {
                self.collect_generalizable(elem, level, vars, dims, rows);
            }
            Type::Fn(params, ret) => {
                for param in params {
                    self.collect_generalizable(param, level, vars, dims, rows);
                }
                self.collect_generalizable(ret, level, vars, dims, rows);
            }
            Type::Record(row) => {
                self.collect_row(row, level, vars, dims, rows);
            }
            Type::Adt(_, args) => {
                for arg in args {
                    self.collect_generalizable(arg, level, vars, dims, rows);
                }
            }
            Type::Io(inner) => {
                self.collect_generalizable(inner, level, vars, dims, rows);
            }
            Type::Point(dim) | Type::UnitOf(dim) => {
                for id in dim.vars() {
                    if dim_var(id) && !dims.contains(&id) {
                        dims.push(id);
                    }
                }
            }
            Type::Dyn(Rep::Var(id)) => {
                if rep_var(*id) && !vars.contains(id) {
                    vars.push(*id);
                }
            }
            _ => {}
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
    ) {
        match row {
            Row::Empty => {}
            Row::Var(id) => {
                if !self.weak.contains(id)
                    && self.row_levels.get(id).copied().unwrap_or(0) > level
                    && !rows.contains(id)
                {
                    rows.push(*id);
                }
            }
            Row::Extend { ty, tail, .. } => {
                self.collect_generalizable(ty, level, vars, dims, rows);
                self.collect_row(tail, level, vars, dims, rows);
            }
        }
    }

    /// Applies the defaulting rules of §3.10.2 to a resolved type.
    ///
    /// Returns the defaulted type and the ids of dimension variables that were
    /// defaulted to `1`, which the caller reports as `W0301`.
    pub fn default_type(&mut self, ty: &Type) -> (Type, Vec<u32>) {
        let mut warnings = Vec::new();
        let defaulted = self.default_inner(ty, &Scheme::mono(ty.clone()), &mut warnings, 0);
        (defaulted, warnings)
    }

    /// Defaults the variables a scheme did *not* quantify (§3.10.2).
    ///
    /// This is the module-end step of PLAN D4: generalisation runs first, so
    /// `<d>`-polymorphic functions keep their dimension variables and only
    /// weak and literal variables are defaulted.
    pub fn default_scheme(&mut self, scheme: &Scheme) -> (Scheme, Vec<u32>) {
        let mut warnings = Vec::new();
        let ty = self.default_inner(&scheme.ty, scheme, &mut warnings, 0);
        let constraints = scheme
            .constraints
            .iter()
            .map(|constraint| Constraint {
                class: constraint.class.clone(),
                args: constraint
                    .args
                    .iter()
                    .map(|arg| self.default_inner(arg, scheme, &mut warnings, 0))
                    .collect(),
            })
            .collect();
        (
            Scheme {
                vars: scheme.vars.clone(),
                dims: scheme.dims.clone(),
                rows: scheme.rows.clone(),
                constraints,
                ty,
            },
            warnings,
        )
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
            Type::Var(id) => {
                if quantified_type(id) || !self.above_floor(id) {
                    return Type::Var(id);
                }
                match self.literals.get(&id).copied() {
                    Some(kind) => {
                        let rep = Rep::default_for_literal(kind);
                        let defaulted = Type::Scalar(rep, Dim::one());
                        // Record the default as a substitution: a deferred
                        // constraint recorded this variable, and it must see
                        // the defaulted type too.
                        self.subst.insert(id, defaulted.clone());
                        defaulted
                    }
                    // A type variable with no literal constraint cannot be
                    // defaulted to `Real`: §3.10.2 only defaults representation,
                    // dimension, natural and row variables. It stays free so the
                    // caller can report `E0304`.
                    None => Type::Var(id),
                }
            }
            Type::Scalar(rep, dim) => {
                let rep = match self.resolve_rep(rep) {
                    Rep::Var(id) if quantified_type(id) || !self.above_floor(id) => Rep::Var(id),
                    Rep::Var(id) => self.default_rep(id),
                    other => other,
                };
                let dim = self.default_dim(&dim, scheme, warnings);
                Type::Scalar(rep, dim)
            }
            Type::Tensor { shape, elem } => Type::Tensor {
                shape,
                elem: Box::new(self.default_inner(&elem, scheme, warnings, depth + 1)),
            },
            Type::Fn(params, ret) => Type::Fn(
                params
                    .iter()
                    .map(|param| self.default_inner(param, scheme, warnings, depth + 1))
                    .collect(),
                Box::new(self.default_inner(&ret, scheme, warnings, depth + 1)),
            ),
            Type::Record(row) => {
                Type::Record(self.default_row(&row, scheme, warnings, depth))
            }
            Type::Adt(id, args) => Type::Adt(
                id,
                args.iter()
                    .map(|arg| self.default_inner(arg, scheme, warnings, depth + 1))
                    .collect(),
            ),
            Type::Io(inner) => Type::Io(Box::new(self.default_inner(
                &inner,
                scheme,
                warnings,
                depth + 1,
            ))),
            Type::Point(dim) => Type::Point(self.default_dim(&dim, scheme, warnings)),
            Type::UnitOf(dim) => Type::UnitOf(self.default_dim(&dim, scheme, warnings)),
            Type::Dyn(rep) => match self.resolve_rep(rep) {
                Rep::Var(id) if quantified_type(id) => Type::Dyn(Rep::Var(id)),
                Rep::Var(id) => Type::Dyn(self.default_rep(id)),
                other => Type::Dyn(other),
            },
            other => other,
        }
    }

    /// The representation a variable defaults to, honouring its lower bounds.
    fn default_rep(&mut self, id: u32) -> Rep {
        let mut lower: Vec<Rep> = Vec::new();
        for bound in self.rep_lower.get(&id).cloned().unwrap_or_default() {
            self.collect_rep_bounds(bound, &mut lower, 0);
        }
        let kind = self.rep_literals.get(&id).copied();
        let preferred = kind.map_or(Rep::Real, Rep::default_for_literal);
        // The default must be above every bound; when it is not, climb the
        // lattice to the least representation that is (§3.4.1).
        let mut chosen = if lower.iter().all(|bound| preferred.at_least(*bound)) {
            preferred
        } else {
            *Rep::upper_candidates(lower.iter().copied().fold(Rep::Int, |acc, bound| {
                acc.join_concrete(bound).unwrap_or(Rep::Complex)
            }))
            .last()
            .unwrap_or(&Rep::Real)
        };
        if let Some(kind) = kind {
            if !kind.accepts_rep(chosen) {
                chosen = Rep::Real;
            }
        }
        let _ = self.rep_subst.insert(id, chosen);
        chosen
    }

    fn collect_rep_bounds(&self, rep: Rep, out: &mut Vec<Rep>, depth: u32) {
        if depth > 16 {
            return;
        }
        match self.resolve_rep(rep) {
            Rep::Var(id) => {
                for bound in self.rep_lower.get(&id).cloned().unwrap_or_default() {
                    self.collect_rep_bounds(bound, out, depth + 1);
                }
            }
            concrete => {
                if !out.contains(&concrete) {
                    out.push(concrete);
                }
            }
        }
    }

    fn default_dim(&mut self, dim: &Dim, scheme: &Scheme, warnings: &mut Vec<u32>) -> Dim {
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

    fn default_row(&mut self, row: &Row, scheme: &Scheme, warnings: &mut Vec<u32>, depth: u32) -> Row {
        match self.resolve_row(row) {
            Row::Empty => Row::Empty,
            Row::Var(id) => {
                if scheme.rows.contains(&id) || !self.above_floor(id) {
                    Row::Var(id)
                } else {
                    Row::Empty
                }
            }
            Row::Extend { label, ty, tail } => Row::Extend {
                label,
                ty: Box::new(self.default_inner(&ty, scheme, warnings, depth + 1)),
                tail: Box::new(self.default_row(&tail, scheme, warnings, depth)),
            },
        }
    }

    /// Whether a variable was created inside the declaration being closed.
    ///
    /// Defaulting an inner `let` must not touch a variable that belongs to the
    /// enclosing function: that one is generalised, or defaulted, at the
    /// enclosing declaration's end.
    fn above_floor(&self, id: u32) -> bool {
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

    fn dim_is_bound(&self, id: u32) -> bool {
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
    pub fn restore(&mut self, snapshot: &Self) {
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


fn substitute(
    ty: &Type,
    types: &HashMap<u32, u32>,
    dims: &HashMap<u32, u32>,
    rows: &HashMap<u32, u32>,
) -> Type {
    match ty {
        Type::Var(id) => match types.get(id) {
            Some(fresh) => Type::Var(*fresh),
            None => ty.clone(),
        },
        Type::Scalar(rep, dim) => {
            let rep = match rep {
                Rep::Var(id) => match types.get(id) {
                    Some(fresh) => Rep::Var(*fresh),
                    None => *rep,
                },
                other => *other,
            };
            let dim = dim.substitute(|id| dims.get(&id).map(|fresh| Dim::var(*fresh)));
            Type::Scalar(rep, dim)
        }
        Type::Tensor { shape, elem } => Type::Tensor {
            shape: shape.clone(),
            elem: Box::new(substitute(elem, types, dims, rows)),
        },
        Type::Fn(params, ret) => Type::Fn(
            params
                .iter()
                .map(|param| substitute(param, types, dims, rows))
                .collect(),
            Box::new(substitute(ret, types, dims, rows)),
        ),
        Type::Record(row) => Type::Record(substitute_row(row, types, dims, rows)),
        Type::Adt(id, args) => Type::Adt(
            *id,
            args.iter()
                .map(|arg| substitute(arg, types, dims, rows))
                .collect(),
        ),
        Type::Io(inner) => Type::Io(Box::new(substitute(inner, types, dims, rows))),
        Type::Dyn(Rep::Var(id)) => match types.get(id) {
            Some(fresh) => Type::Dyn(Rep::Var(*fresh)),
            None => ty.clone(),
        },
        Type::Point(dim) => {
            Type::Point(dim.substitute(|id| dims.get(&id).map(|fresh| Dim::var(*fresh))))
        }
        Type::UnitOf(dim) => {
            Type::UnitOf(dim.substitute(|id| dims.get(&id).map(|fresh| Dim::var(*fresh))))
        }
        other => other.clone(),
    }
}

fn substitute_row(
    row: &Row,
    types: &HashMap<u32, u32>,
    dims: &HashMap<u32, u32>,
    rows: &HashMap<u32, u32>,
) -> Row {
    match row {
        Row::Empty => Row::Empty,
        Row::Var(id) => match rows.get(id) {
            Some(fresh) => Row::Var(*fresh),
            None => Row::Var(*id),
        },
        Row::Extend { label, ty, tail } => Row::Extend {
            label: label.clone(),
            ty: Box::new(substitute(ty, types, dims, rows)),
            tail: Box::new(substitute_row(tail, types, dims, rows)),
        },
    }
}

fn row_id(row: &Row) -> Option<u32> {
    match row {
        Row::Var(id) => Some(*id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unifies_dimension_variables() {
        let mut infer = Infer::new();
        let dim = infer.fresh_dim();
        infer
            .unify(&Type::real(dim.clone()), &Type::real(Dim::base("Length")))
            .expect("solvable");
        assert_eq!(infer.resolve(&Type::real(dim)), Type::real(Dim::base("Length")));
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
        let fresh = resolved.tail_var().expect("the row stays open");
        assert!(infer.lacks_of(fresh).contains(&"y".to_string()));
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
}
