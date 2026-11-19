//! Structural record and tensor expression checking.

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

use super::Checker;
use super::closest_name;
use crate::builtins;
use crate::ty::Nat;
use crate::ty::Row;
use crate::ty::Shape;
use crate::ty::Type;
use aevia_syntax::Span;
use aevia_syntax::ast::Expr;
use aevia_syntax::ast::ExprKind;
use aevia_syntax::ast::SliceAst;
use aevia_syntax::ast::UnOp;
use num_bigint::Sign;
use num_traits::ToPrimitive;
use std::collections::HashSet;

impl Checker {
    pub(super) fn synth_record(
        &mut self,
        base: Option<&Expr>,
        fields: &[(String, Expr)],
    ) -> Type {
        let mut seen = HashSet::with_capacity(fields.len());
        for (name, value) in fields {
            if !seen.insert(name.as_str()) {
                self.error_help(
                    "E0208",
                    format!("duplicate field `{name}`"),
                    value.span,
                    "delete one of the two fields",
                );
            }
        }
        let Some(base) = base else {
            return Type::record(
                fields
                    .iter()
                    .map(|(name, value)| (name.clone(), self.synth(value))),
            );
        };
        let base_ty = self.synth(base);
        let mut row = match self.infer.resolve(&base_ty) {
            | Type::Record(row) => row,
            | other => {
                self.error(
                    "E0302",
                    format!("record update expects a record, found {other}"),
                    base.span,
                );
                Row::Empty
            },
        };
        for (name, value) in fields {
            let ty = self.synth(value);
            // §3.6.2: adding a field requires its absence from the base tail.
            if row.field(name).is_none()
                && let Some(tail) = row.tail_var()
                && let Err(error) = self.infer.require_lacks(tail, name)
            {
                self.report_unify(error, value.span);
            }
            row = row.update(name, ty);
        }
        Type::Record(row)
    }

    pub(super) fn field_type(
        &mut self,
        base: &Type,
        name: &str,
        span: Span,
    ) -> Type {
        let resolved = self.infer.resolve(base);
        match resolved {
            | Type::Var(_) => {
                let field = self.infer.fresh_type();
                let tail = self.infer.fresh_row();
                let record = Type::Record(Row::Extend {
                    label: name.to_string(),
                    ty: Box::new(field.clone()),
                    tail: Box::new(tail),
                });
                if let Err(error) = self.infer.unify(&resolved, &record) {
                    self.report_unify(error, span);
                    return Type::Error;
                }
                field
            },
            | Type::Record(row) => self.record_field(&row, name, span),
            | Type::Error => Type::Error,
            | other => {
                self.error(
                    "E0302",
                    format!("field access expects a record, found {other}"),
                    span,
                );
                Type::Error
            },
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "keeps record projection and its field-specific diagnostics together"
    )]
    fn record_field(
        &mut self,
        row: &Row,
        name: &str,
        span: Span,
    ) -> Type {
        if let Some(ty) = row.field(name) {
            return ty.clone();
        }
        if let Some(tail) = row.tail_var() {
            let field = self.infer.fresh_type();
            if let Err(error) = self.infer.extend_row(tail, name, field.clone()) {
                self.report_unify(error, span);
                return Type::Error;
            }
            return field;
        }
        let message = format!("the record {{ {row} }} has no field `{name}`");
        let names = row
            .iter_fields()
            .map(|(field, _)| field)
            .collect::<Vec<_>>();
        if let Some(close) = closest_name(name, names.iter().copied()) {
            self.error_help("E0306", message, span, &format!("did you mean `{close}`?"));
        } else if names.is_empty() {
            self.error("E0306", message, span);
        } else {
            self.error_help(
                "E0306",
                message,
                span,
                &format!("the fields are {}", names.join(", ")),
            );
        }
        Type::Error
    }

    pub(super) fn synth_tensor(
        &mut self,
        items: &[Expr],
        span: Span,
    ) -> Type {
        if items.is_empty() {
            self.error_help(
                "E0314",
                "the element type of an empty tensor cannot be inferred",
                span,
                "annotate it, for example `let e: Tensor<1, Float> = []`",
            );
            return Type::Error;
        }
        let mut elem: Option<Type> = None;
        for item in items {
            let ty = self.synth(item);
            match &elem {
                | None => elem = Some(ty),
                | Some(existing) => {
                    let existing_resolved = self.infer.resolve(existing);
                    let this_resolved = self.infer.resolve(&ty);
                    if let Err(error) = self.infer.unify(existing, &ty) {
                        // §3.5.2: same-level sub-sequences must have equal
                        // length, and that has its own code (E0203) instead of
                        // a generic mismatch — `[[1.0, 2.0], [3.0]]` is ragged.
                        if shapes_are_ragged(&existing_resolved, &this_resolved) {
                            self.error(
                                "E0203",
                                "ragged tensor literal: every row must have the same length",
                                item.span,
                            );
                        } else {
                            self.report_unify(error, item.span);
                        }
                    }
                },
            }
        }
        let elem = elem.unwrap_or(Type::Error);
        let resolved = self.infer.resolve(&elem);
        match resolved {
            | Type::Tensor {
                shape: inner,
                elem: inner_elem,
            } => {
                // A rank-only annotation can be arbitrarily larger than the source.
                // Keep small refinements and preserve just the rank for larger values.
                const MAX_IMPLICIT_REFINEMENT_RANK: u32 = 64;
                let shape = match inner {
                    | Shape::Rank(rank) => {
                        let Some(rank) = rank.checked_add(1) else {
                            self.error(
                                "E0302",
                                "tensor literal rank exceeds the supported range",
                                span,
                            );
                            return Type::Error;
                        };
                        if rank <= MAX_IMPLICIT_REFINEMENT_RANK {
                            let mut extents = vec![Some(Nat::lit(items.len()))];
                            extents.resize(rank as usize, None);
                            Shape::Refined(extents)
                        } else {
                            Shape::Rank(rank)
                        }
                    },
                    | Shape::Refined(mut extents) => {
                        extents.insert(0, Some(Nat::lit(items.len())));
                        Shape::Refined(extents)
                    },
                };
                Type::Tensor {
                    shape,
                    elem: inner_elem,
                }
            },
            | other => {
                Type::Tensor {
                    shape: Shape::Refined(vec![Some(Nat::lit(items.len()))]),
                    elem: Box::new(other),
                }
            },
        }
    }

    pub(super) fn synth_index(
        &mut self,
        base: &Expr,
        indices: &[SliceAst],
        span: Span,
    ) -> Type {
        let base_ty = self.synth(base);
        let mut resolved = self.infer.resolve(&base_ty);
        if matches!(resolved, Type::Var(_)) {
            let tensor = Type::tensor(Shape::Rank(1), self.infer.fresh_type());
            if let Err(error) = self.infer.unify(&resolved, &tensor) {
                self.report_unify(error, span);
                return Type::Error;
            }
            resolved = self.infer.resolve(&tensor);
        }
        if matches!(resolved, Type::Str) {
            let [index] = indices else {
                self.error(
                    "E0302",
                    format!("a string has one axis, found {} indices", indices.len()),
                    span,
                );
                return Type::Error;
            };
            self.check_index_terms(index);
            return Type::Str;
        }
        if let Some((format, dense)) = builtins::sparse_parts(&resolved) {
            return self.sparse_index(format, dense, indices, span);
        }
        if let Type::Tensor { shape, elem } = resolved {
            return self.tensor_index(shape, *elem, indices, span);
        }
        if !resolved.is_error() {
            self.error(
                "E0302",
                format!("indexing expects a tensor or a string, found {resolved}"),
                span,
            );
        }
        Type::Error
    }

    #[allow(
        clippy::single_call_fn,
        reason = "declines invalid selectors and preserves the remaining axes of a tensor"
    )]
    fn tensor_index(
        &mut self,
        shape: Shape,
        elem: Type,
        indices: &[SliceAst],
        span: Span,
    ) -> Type {
        let rank = shape.rank();
        if u32::try_from(indices.len()).map_or(true, |count| count > rank) {
            self.error(
                "E0802",
                format!("a rank-{rank} tensor has {} selectors", indices.len()),
                span,
            );
            return Type::Error;
        }
        let mut extents = match shape {
            | Shape::Refined(items) => Some(items.into_iter()),
            | Shape::Rank(_) => None,
        };
        let mut refined = extents.as_ref().map(|_| Vec::new());
        for index in indices {
            let extent = extents.as_mut().and_then(Iterator::next).flatten();
            self.check_index_terms(index);
            match index {
                | SliceAst::Index(expr) => {
                    self.check_index_bounds(expr, extent.as_ref());
                },
                | SliceAst::Range {
                    start,
                    end,
                    step,
                    inclusive,
                } => {
                    if let Some(refined) = &mut refined {
                        refined.push(self.literal_slice_extent(
                            extent.as_ref(),
                            start.as_ref(),
                            end.as_ref(),
                            step.as_ref(),
                            *inclusive,
                        ));
                    }
                },
            }
        }
        let dropped = indices
            .iter()
            .filter(|index| matches!(index, SliceAst::Index(_)))
            .count();
        let result_rank = rank.saturating_sub(u32::try_from(dropped).unwrap_or(rank));
        if result_rank == 0 {
            return elem;
        }
        let result_shape = match (refined, extents) {
            | (Some(mut refined), Some(extents)) => {
                refined.extend(extents);
                Shape::Refined(refined)
            },
            | _ => Shape::Rank(result_rank),
        };
        Type::tensor(result_shape, elem)
    }

    fn check_index_terms(
        &mut self,
        index: &SliceAst,
    ) {
        let (terms, step) = match index {
            | SliceAst::Index(expr) => ([Some(expr), None], None),
            | SliceAst::Range { start, end, step, .. } => {
                ([start.as_ref(), end.as_ref()], step.as_ref())
            },
        };
        for term in terms.into_iter().flatten() {
            let ty = self.synth(term);
            if let Err(error) = self.infer.unify(&ty, &Type::int()) {
                self.report_unify(error, term.span);
            }
        }
        if let Some(step) = step {
            let ty = self.synth(step);
            if self.infer.unify(&ty, &Type::int()).is_err() {
                self.error("E0302", "a slice step must be an Int", step.span);
            } else if literal_index(step) == Some(0) {
                self.error_help(
                    "E0802",
                    "a slice step of zero never advances",
                    step.span,
                    "omit the step to walk by one",
                );
            }
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "pairs a statically known selector with its original tensor dimension"
    )]
    fn check_index_bounds(
        &mut self,
        expr: &Expr,
        extent: Option<&Nat>,
    ) {
        if let (Some(index), Some(extent)) = (
            literal_index(expr),
            extent.and_then(|extent| self.infer.resolve_nat(extent).as_const()),
        ) && !isize::try_from(extent)
            .is_ok_and(|end| (end.saturating_neg()..end).contains(&index))
        {
            self.error_help(
                "E0802",
                format!("index {index} is out of bounds for a dimension of extent {extent}"),
                expr.span,
                "a literal index is checked at compile time",
            );
        }
    }

    /// The known extent of a literal window or stepped walk.
    #[allow(
        clippy::single_call_fn,
        reason = "keeps literal selector normalization aligned with the runtime walk"
    )]
    fn literal_slice_extent(
        &self,
        source_extent: Option<&Nat>,
        start: Option<&Expr>,
        end: Option<&Expr>,
        stride_term: Option<&Expr>,
        inclusive: bool,
    ) -> Option<Nat> {
        let extent = isize::try_from(self.infer.resolve_nat(source_extent?).as_const()?).ok()?;
        let start = if let Some(expr) = start {
            Some(literal_index(expr)?)
        } else {
            None
        };
        let end = if let Some(expr) = end {
            Some(literal_index(expr)?)
        } else {
            None
        };
        let from_end = |index: isize| {
            if index < 0 {
                extent.saturating_add(index)
            } else {
                index
            }
        };
        if let Some(term) = stride_term {
            let stride = literal_index(term)?;
            if stride == 0 {
                return None;
            }
            if extent == 0 {
                return Some(Nat::lit(0));
            }
            let high = if stride > 0 {
                extent
            } else {
                extent.saturating_sub(1)
            };
            let first = start.map_or(if stride > 0 { 0 } else { high }, |raw| {
                from_end(raw).clamp(0, high)
            });
            let stop = end.map_or(if stride > 0 { extent } else { -1 }, |raw| {
                from_end(raw).clamp(-1, high)
            });
            let distance = if (stride > 0 && first < stop) || (stride < 0 && first > stop) {
                first.abs_diff(stop)
            } else {
                0
            };
            return Some(Nat::lit(distance.div_ceil(stride.unsigned_abs())));
        }
        let first = start.map_or(0, |raw| from_end(raw).clamp(0, extent));
        let stop = end
            .map_or(extent, |raw| from_end(raw).clamp(0, extent))
            .saturating_add(isize::from(inclusive))
            .min(extent);
        if stop < first {
            return None;
        }
        Some(Nat::lit(first.abs_diff(stop)))
    }
}

/// A literal selector as the runtime observes it, including Int saturation.
pub(super) fn literal_index(expr: &Expr) -> Option<isize> {
    let mut operand = expr;
    let mut negate = false;
    loop {
        match &operand.kind {
            | ExprKind::Int(value) => {
                let negative = (value.sign() == Sign::Minus) ^ negate;
                let magnitude = value.magnitude().to_isize();
                return Some(if negative {
                    magnitude.and_then(isize::checked_neg).unwrap_or(isize::MIN)
                } else {
                    magnitude.unwrap_or(isize::MAX)
                });
            },
            | ExprKind::Unary {
                op: UnOp::Neg,
                operand: inner,
            } => {
                operand = inner;
                negate = !negate;
            },
            | _ => return None,
        }
    }
}

/// Two same-rank tensors with unequal known extents make a ragged literal.
#[allow(
    clippy::single_call_fn,
    reason = "distinguishes nonrectangular literal structure from element-type errors"
)]
fn shapes_are_ragged(
    left: &Type,
    right: &Type,
) -> bool {
    let (
        Type::Tensor {
            shape: Shape::Refined(a),
            ..
        },
        Type::Tensor {
            shape: Shape::Refined(b),
            ..
        },
    ) = (left, right)
    else {
        return false;
    };
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).any(|(left, right)| {
        match (left, right) {
            | (Some(left), Some(right)) => left != right,
            | _ => false,
        }
    })
}
