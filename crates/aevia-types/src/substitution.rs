//! Explicit substitutions shared by checked call instantiation and execution.
//! Values on the right belong to the target variable universe. Applying a
//! substitution is deliberately one pass, so independently checked modules may
//! reuse numeric variable ids without recursively capturing each other's ids.
use crate::Infer;
use crate::Nat;
use crate::Rep;
use crate::Row;
use crate::Shape;
use crate::Type;
use aevia_dim::Dim;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct TypeSubstitution {
    pub types: HashMap<u32, Type>,
    pub representations: HashMap<u32, Rep>,
    pub dimensions: HashMap<u32, Dim>,
    pub rows: HashMap<u32, Row>,
    pub naturals: HashMap<u32, Nat>,
}
impl TypeSubstitution {
    pub fn apply(
        &self,
        ty: &Type,
    ) -> Type {
        match ty {
            | Type::Var(id) => self.types.get(id).cloned().unwrap_or_else(|| ty.clone()),
            | Type::Scalar(rep, dim) => Type::Scalar(self.rep(*rep), self.dim(dim)),
            | Type::Dyn(rep) => Type::Dyn(self.rep(*rep)),
            | Type::Point(dim) => Type::Point(self.dim(dim)),
            | Type::UnitOf(dim) => Type::UnitOf(self.dim(dim)),
            | Type::Tensor { shape, elem } => {
                Type::tensor(shape.substitute_nats(&self.naturals), self.apply(elem))
            },
            | Type::Fn(args, ret) => {
                Type::func(
                    args.iter().map(|arg| self.apply(arg)).collect(),
                    self.apply(ret),
                )
            },
            | Type::Record(row) => Type::Record(self.row(row)),
            | Type::Adt(id, args) => {
                Type::Adt(*id, args.iter().map(|arg| self.apply(arg)).collect())
            },
            | Type::Io(inner) => Type::Io(Box::new(self.apply(inner))),
            | Type::TypeRepIntrinsic(inner) => {
                Type::TypeRepIntrinsic(inner.as_ref().map(|ty| Box::new(self.apply(ty))))
            },
            | Type::FromValueIntrinsic(inner) => {
                Type::FromValueIntrinsic(inner.as_ref().map(|ty| Box::new(self.apply(ty))))
            },
            | Type::TensorIntrinsic(intrinsic) => {
                let mut intrinsic = *intrinsic;
                intrinsic.element = intrinsic.element.map(|rep| self.rep(rep));
                Type::TensorIntrinsic(intrinsic)
            },
            | Type::Bool
            | Type::Prop
            | Type::Str
            | Type::Unit
            | Type::Param(_)
            | Type::Error
            | Type::WriteNpyIntrinsic
            | Type::ReadNpyIntrinsic(_)
            | Type::ToCallbackIntrinsic => ty.clone(),
        }
    }

    pub fn rep(
        &self,
        rep: Rep,
    ) -> Rep {
        match rep {
            | Rep::Var(id) => self.representations.get(&id).copied().unwrap_or(rep),
            | _ => rep,
        }
    }

    pub fn dim(
        &self,
        dim: &Dim,
    ) -> Dim {
        dim.substitute(|id| self.dimensions.get(&id).cloned())
    }

    pub fn row(
        &self,
        row: &Row,
    ) -> Row {
        match row {
            | Row::Empty => Row::Empty,
            | Row::Var(id) => self.rows.get(id).cloned().unwrap_or_else(|| row.clone()),
            | Row::Extend { label, ty, tail } => {
                Row::Extend {
                    label: label.clone(),
                    ty: Box::new(self.apply(ty)),
                    tail: Box::new(self.row(tail)),
                }
            },
        }
    }

    /// Freshen every kind, retaining a map back to the original body variables.
    pub fn freshen(
        &mut self,
        ty: &Type,
        infer: &mut Infer,
    ) -> Type {
        match ty {
            | Type::Var(id) => {
                self.types.entry(*id).or_insert_with(|| infer.fresh_type());
            },
            | Type::Scalar(rep, dim) => {
                self.fresh_rep(*rep, infer);
                self.fresh_dim(dim, infer);
            },
            | Type::Dyn(rep) => self.fresh_rep(*rep, infer),
            | Type::Point(dim) | Type::UnitOf(dim) => self.fresh_dim(dim, infer),
            | Type::Tensor { shape, elem } => {
                if let Shape::Refined(items) = shape {
                    for nat in items.iter().flatten() {
                        for id in nat.vars() {
                            self.naturals.entry(id).or_insert_with(|| infer.fresh_nat());
                        }
                    }
                }
                self.freshen(elem, infer);
            },
            | Type::Fn(args, ret) => {
                for arg in args {
                    self.freshen(arg, infer);
                }
                self.freshen(ret, infer);
            },
            | Type::Record(row) => self.fresh_row(row, infer),
            | Type::Adt(_, args) => {
                for arg in args {
                    self.freshen(arg, infer);
                }
            },
            | Type::Io(inner) => {
                self.freshen(inner, infer);
            },
            | Type::TypeRepIntrinsic(Some(inner)) | Type::FromValueIntrinsic(Some(inner)) => {
                self.freshen(inner, infer);
            },
            | Type::TensorIntrinsic(intrinsic) => {
                if let Some(rep) = intrinsic.element {
                    self.fresh_rep(rep, infer);
                }
            },
            | Type::TypeRepIntrinsic(None)
            | Type::FromValueIntrinsic(None)
            | Type::Bool
            | Type::Prop
            | Type::Str
            | Type::Unit
            | Type::Param(_)
            | Type::Error
            | Type::WriteNpyIntrinsic
            | Type::ReadNpyIntrinsic(_)
            | Type::ToCallbackIntrinsic => {},
        }
        self.apply(ty)
    }

    fn fresh_rep(
        &mut self,
        rep: Rep,
        infer: &mut Infer,
    ) {
        if let Rep::Var(id) = rep {
            self.representations
                .entry(id)
                .or_insert_with(|| infer.fresh_rep());
        }
    }

    fn fresh_dim(
        &mut self,
        dim: &Dim,
        infer: &mut Infer,
    ) {
        for id in dim.vars() {
            self.dimensions
                .entry(id)
                .or_insert_with(|| infer.fresh_dim());
        }
    }

    fn fresh_row(
        &mut self,
        row: &Row,
        infer: &mut Infer,
    ) {
        match row {
            | Row::Empty => {},
            | Row::Var(id) => {
                self.rows.entry(*id).or_insert_with(|| infer.fresh_row());
            },
            | Row::Extend { ty, tail, .. } => {
                self.freshen(ty, infer);
                self.fresh_row(tail, infer);
            },
        }
    }

    pub fn resolved(
        &self,
        infer: &Infer,
    ) -> Self {
        Self {
            types: self
                .types
                .iter()
                .map(|(id, ty)| (*id, infer.resolve(ty)))
                .collect(),
            representations: self
                .representations
                .iter()
                .map(|(id, rep)| (*id, infer.resolve_rep(*rep)))
                .collect(),
            dimensions: self
                .dimensions
                .iter()
                .map(|(id, dim)| (*id, infer.resolve_dim(dim)))
                .collect(),
            rows: self
                .rows
                .iter()
                .map(|(id, row)| (*id, infer.resolve_row(row)))
                .collect(),
            naturals: self
                .naturals
                .iter()
                .map(|(id, nat)| (*id, infer.resolve_nat(nat)))
                .collect(),
        }
    }

    pub fn then(
        &self,
        next: &Self,
    ) -> Self {
        Self {
            types: self
                .types
                .iter()
                .map(|(id, ty)| (*id, next.apply(ty)))
                .collect(),
            representations: self
                .representations
                .iter()
                .map(|(id, rep)| (*id, next.rep(*rep)))
                .collect(),
            dimensions: self
                .dimensions
                .iter()
                .map(|(id, dim)| (*id, next.dim(dim)))
                .collect(),
            rows: self
                .rows
                .iter()
                .map(|(id, row)| (*id, next.row(row)))
                .collect(),
            naturals: self
                .naturals
                .iter()
                .map(|(id, nat)| (*id, nat.substitute(&next.naturals)))
                .collect(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutions_cross_variable_universes_only_once() {
        let mut substitution = TypeSubstitution::default();
        substitution.types.insert(0, Type::Var(1));
        substitution.types.insert(1, Type::Str);
        substitution.representations.insert(0, Rep::Var(1));
        substitution.representations.insert(1, Rep::Float);
        substitution.dimensions.insert(0, Dim::var(1));
        substitution.dimensions.insert(1, Dim::base("Length"));
        substitution.naturals.insert(0, Nat::Var(1));
        substitution.naturals.insert(1, Nat::Lit(3));
        substitution.rows.insert(0, Row::Var(1));
        substitution.rows.insert(1, Row::Empty);
        assert_eq!(substitution.apply(&Type::Var(0)), Type::Var(1));
        assert_eq!(substitution.rep(Rep::Var(0)), Rep::Var(1));
        assert_eq!(substitution.dim(&Dim::var(0)), Dim::var(1));
        assert_eq!(Nat::Var(0).substitute(&substitution.naturals), Nat::Var(1));
        assert_eq!(substitution.row(&Row::Var(0)), Row::Var(1));
    }

    #[test]
    fn freshening_preserves_correlations_across_nested_types_and_constraints() {
        let source = Type::func(
            vec![
                Type::Var(10),
                Type::Scalar(Rep::Var(11), Dim::var(12)),
                Type::tensor(
                    Shape::Refined(vec![Some(Nat::Var(13))]),
                    Type::Record(Row::Var(14)),
                ),
            ],
            Type::Var(10),
        );
        let mut names = TypeSubstitution::default();
        let mut infer = Infer::new();
        let fresh = names.freshen(&source, &mut infer);
        let Type::Fn(arguments, result) = fresh else {
            panic!("function")
        };
        assert_eq!(arguments[0], *result);
        infer.unify(&arguments[0], &Type::Bool).unwrap();
        infer
            .unify(&arguments[1], &Type::float(Dim::base("Time")))
            .unwrap();
        infer
            .unify(
                &arguments[2],
                &Type::tensor(
                    Shape::refined([Some(5)]),
                    Type::record([("name".into(), Type::Str)]),
                ),
            )
            .unwrap();
        let resolved = names.resolved(&infer);
        let Type::Fn(arguments, result) = resolved.apply(&source) else {
            panic!("function")
        };
        assert_eq!(*result, Type::Bool);
        assert_eq!(arguments[1], Type::float(Dim::base("Time")));
        assert_eq!(
            arguments[2],
            Type::tensor(
                Shape::refined([Some(5)]),
                Type::record([("name".into(), Type::Str)])
            )
        );
    }
}
