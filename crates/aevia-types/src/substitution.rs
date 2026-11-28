//! Explicit substitutions shared by checked call instantiation and execution.
//!
//! Values on the right belong to the target variable universe. Applying a
//! substitution is deliberately one pass, so independently checked modules may
//! reuse numeric variable ids without recursively capturing each other's ids.

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

use crate::Infer;
use crate::Nat;
use crate::Rep;
use crate::Row;
use crate::Type;
use crate::ty::TypeVariableRef;
use aevia_dim::Dim;
use std::collections::HashMap;

/// Maps each kind of source variable into its target universe.
#[derive(Clone, Debug, Default)]
pub struct TypeSubstitution {
    /// Ordinary type variables and their target types.
    pub types: HashMap<u32, Type>,
    /// Numeric representation variables.
    pub representations: HashMap<u32, Rep>,
    /// Dimension variables.
    pub dimensions: HashMap<u32, Dim>,
    /// Open record row variables.
    pub rows: HashMap<u32, Row>,
    /// Natural number variables in refined tensor extents.
    pub naturals: HashMap<u32, Nat>,
}
impl TypeSubstitution {
    /// Rewrites source variables once throughout `ty`.
    #[must_use]
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

    /// Rewrites a representation variable, preserving a concrete representation.
    #[must_use]
    pub fn rep(
        &self,
        rep: Rep,
    ) -> Rep {
        match rep {
            | Rep::Var(id) => self.representations.get(&id).copied().unwrap_or(rep),
            | _ => rep,
        }
    }

    /// Rewrites the dimension's symbolic factors once.
    #[must_use]
    pub fn dim(
        &self,
        dim: &Dim,
    ) -> Dim {
        dim.substitute(|id| self.dimensions.get(&id).cloned())
    }

    /// Rewrites row tails and the types of extended fields.
    /// The field chain is walked iteratively, including any mapped tail.
    #[must_use]
    pub fn row(
        &self,
        row: &Row,
    ) -> Row {
        if let Row::Var(id) = row
            && self.rows.is_empty()
        {
            return Row::Var(*id);
        }
        let mut result = Row::Empty;
        let (tail, id) = append_row_fields(&mut result, row, |ty| self.apply(ty));
        if let Some(id) = id {
            if let Some(replacement) = self.rows.get(&id) {
                // A replacement already belongs to the target universe.
                let (tail, id) = append_row_fields(tail, replacement, Type::clone);
                if let Some(id) = id {
                    *tail = Row::Var(id);
                }
            } else {
                *tail = Row::Var(id);
            }
        }
        result
    }

    /// Freshen every kind, retaining a map back to the original body variables.
    pub fn freshen(
        &mut self,
        ty: &Type,
        infer: &mut Infer,
    ) -> Type {
        self.register_variables(ty, infer);
        self.apply(ty)
    }

    /// Records fresh target variables for the free variables in `ty`.
    pub(crate) fn register_variables(
        &mut self,
        ty: &Type,
        infer: &mut Infer,
    ) {
        ty.visit_variables(&mut |variable| {
            match variable {
                | TypeVariableRef::Type(id) => {
                    self.types.entry(id).or_insert_with(|| infer.fresh_type());
                },
                | TypeVariableRef::Representation(Rep::Var(id)) => {
                    self.representations
                        .entry(id)
                        .or_insert_with(|| infer.fresh_rep());
                },
                | TypeVariableRef::Representation(_) => {},
                | TypeVariableRef::Dimension(dim) => {
                    for id in dim.vars() {
                        self.dimensions
                            .entry(id)
                            .or_insert_with(|| infer.fresh_dim());
                    }
                },
                | TypeVariableRef::Extents(items) => {
                    for nat in items.iter().flatten() {
                        for id in nat.vars() {
                            self.naturals.entry(id).or_insert_with(|| infer.fresh_nat());
                        }
                    }
                },
                | TypeVariableRef::Row(id) => {
                    self.rows.entry(id).or_insert_with(|| infer.fresh_row());
                },
            }
        });
    }

    /// Resolves each target value against the current inference state.
    #[must_use]
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

    /// Applies `next` to each target value while retaining this map's source keys.
    #[must_use]
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
fn append_row_fields<'a>(
    mut output: &'a mut Row,
    mut input: &Row,
    map: impl Fn(&Type) -> Type,
) -> (&'a mut Row, Option<u32>) {
    while let Row::Extend { label, ty, tail } = input {
        *output = Row::Extend {
            label: label.clone(),
            ty: Box::new(map(ty)),
            tail: Box::new(Row::Empty),
        };
        let Row::Extend { tail: written, .. } = output else {
            break;
        };
        output = written;
        input = tail;
    }
    let id = if let Row::Var(id) = input {
        Some(*id)
    } else {
        None
    };
    (output, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Shape;

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

    fn extend(
        label: &str,
        ty: Type,
        tail: Row,
    ) -> Row {
        Row::Extend {
            label: label.to_owned(),
            ty: Box::new(ty),
            tail: Box::new(tail),
        }
    }

    #[test]
    fn appending_a_replacement_preserves_order_and_the_target_universe() {
        let mut substitution = TypeSubstitution::default();
        substitution.types.insert(0, Type::Str);
        substitution.rows.insert(1, Row::Empty);
        let target = extend("same", Type::Var(0), Row::Var(1));
        substitution.rows.insert(0, target.clone());

        let source = extend("same", Type::Var(0), Row::Var(0));
        assert_eq!(substitution.row(&source), extend("same", Type::Str, target));
        assert_eq!(substitution.row(&Row::Empty), Row::Empty);
        assert_eq!(substitution.row(&Row::Var(9)), Row::Var(9));
        assert_eq!(TypeSubstitution::default().row(&Row::Var(9)), Row::Var(9));
    }

    #[test]
    fn fresh_variables_keep_their_first_occurrence_order() {
        let row = extend(
            "first",
            Type::Var(41),
            extend(
                "second",
                Type::Point(Dim::var(12)),
                extend(
                    "third",
                    Type::tensor(Shape::Refined(vec![Some(Nat::Var(77))]), Type::Var(41)),
                    Row::Var(55),
                ),
            ),
        );
        let mut actual = Infer::new();
        let mut substitution = TypeSubstitution::default();
        let _ = substitution.freshen(&Type::Record(row), &mut actual);

        let mut expected = Infer::new();
        assert_eq!(substitution.types[&41], expected.fresh_type());
        assert_eq!(substitution.dimensions[&12], expected.fresh_dim());
        assert_eq!(substitution.naturals[&77], expected.fresh_nat());
        assert_eq!(substitution.rows[&55], expected.fresh_row());
        assert_eq!(actual.fresh_type(), expected.fresh_type());
    }

    #[test]
    fn wide_source_rows_and_mapped_tails_only_need_a_small_stack() {
        fn wide(
            width: usize,
            tail: Row,
        ) -> Row {
            (0..width).fold(tail, |tail, _| extend("field", Type::Var(10), tail))
        }
        // Release each node explicitly to isolate the stack used by substitution.
        fn consume(mut row: Row) -> (usize, Option<u32>) {
            let mut count = 0;
            while let Row::Extend { tail, .. } = row {
                count += 1;
                row = *tail;
            }
            (
                count,
                if let Row::Var(id) = row {
                    Some(id)
                } else {
                    None
                },
            )
        }

        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let width = 8192;
                let mut substitution = TypeSubstitution::default();
                let input = Type::Record(wide(width, Row::Var(99)));
                let Type::Record(fresh) = substitution.freshen(&input, &mut Infer::new()) else {
                    panic!("expected a record");
                };
                let Type::Record(source) = input else {
                    panic!("expected a record");
                };
                assert_eq!(consume(source), (width, Some(99)));
                assert_eq!(consume(fresh).0, width);

                substitution.rows.insert(7, wide(width, Row::Var(7)));
                let replaced = substitution.row(&Row::Var(7));
                assert_eq!(consume(replaced), (width, Some(7)));
                assert_eq!(
                    consume(substitution.rows.remove(&7).unwrap()),
                    (width, Some(7))
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
