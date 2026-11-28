//! Typing of sparse tensors (§8.4.1, §13.9.5).
//!
//! `Sparse<F, shape, τ>` is its own type constructor. Every operator below
//! reduces to the dense rule on the sparse operands' dense counterparts
//! (`Tensor<shape, τ>`), so element dimensions propagate exactly as they do
//! for dense tensors: sums need equal dimensions, products multiply them. What
//! changes is the *layout* of the result:
//!
//! | operation               | result                                        |
//! |-------------------------|-----------------------------------------------|
//! | `sparse ± sparse`       | `Sparse` in the left operand's format         |
//! | `sparse ± dense`        | dense                                         |
//! | `sparse * scalar`, `sparse / scalar` | `Sparse`                         |
//! | `sparse * sparse` (∘)   | `Sparse`                                      |
//! | `sparse * dense` (∘)    | `Sparse`                                      |
//! | `sparse @ dense`        | dense                                         |
//! | `dense @ sparse`        | dense                                         |
//! | `sparse @ sparse`       | `Sparse<Csr, …>`                              |
//!
//! There is no implicit conversion between `Sparse` and `Tensor`: a function
//! that wants one cannot be given the other (`to_dense` / `from_dense` are
//! explicit, §13.9.5).

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
use super::records::literal_index;
use crate::builtins;
use crate::ty::Row;
use crate::ty::Shape;
use crate::ty::Type;
use aevia_syntax::Span;
use aevia_syntax::ast::BinOp;
use aevia_syntax::ast::Expr;
use aevia_syntax::ast::ExprKind;
use aevia_syntax::ast::SliceAst;
use aevia_syntax::ast::Ty;
use aevia_syntax::ast::TyKind;
use std::collections::HashSet;

impl Checker {
    fn wrap_sparse(
        format: &Type,
        dense: Type,
    ) -> Type {
        if matches!(dense, Type::Tensor { .. }) {
            builtins::sparse_type(format.clone(), dense)
        } else {
            Type::Error
        }
    }

    /// The typing of an operator with a sparse operand, or `None` when
    /// neither operand is sparse.
    #[allow(
        clippy::too_many_arguments,
        reason = "the dense operator rules use the two source expressions as well as their types"
    )]
    pub(super) fn sparse_binary(
        &mut self,
        op: BinOp,
        left_ty: &Type,
        right_ty: &Type,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) -> Option<Type> {
        let left_sparse = builtins::sparse_parts(left_ty);
        let right_sparse = builtins::sparse_parts(right_ty);
        if left_sparse.is_none() && right_sparse.is_none() {
            return None;
        }
        let is_tensor = |ty: &Type| matches!(ty, Type::Tensor { .. });
        let is_scalar = |ty: &Type| matches!(ty, Type::Scalar(..) | Type::UnitOf(_));
        let result = match (op, left_sparse, right_sparse) {
            | (BinOp::Eq | BinOp::Ne, Some((_, a)), Some((_, b))) => {
                // Equality is logical (format-independent): only the shape and
                // the element type have to agree.
                if let Err(error) = self.infer.unify(a, b) {
                    self.report_unify(error, span);
                    return Some(Type::Error);
                }
                Type::Bool
            },
            | (BinOp::Add | BinOp::Sub, Some((f, a)), Some((_, b))) => {
                let t = self.add_type(op, a, b, left, right, span);
                Self::wrap_sparse(f, t)
            },
            | (BinOp::Add | BinOp::Sub, Some((_, a)), None) if is_tensor(right_ty) => {
                self.add_type(op, a, right_ty, left, right, span)
            },
            | (BinOp::Add | BinOp::Sub, None, Some((_, b))) if is_tensor(left_ty) => {
                self.add_type(op, left_ty, b, left, right, span)
            },
            | (BinOp::Mul, Some((f, a)), Some((_, b))) => {
                let t = self.mul_type(a, b, span);
                Self::wrap_sparse(f, t)
            },
            | (BinOp::Mul, Some((f, a)), None) if is_scalar(right_ty) || is_tensor(right_ty) => {
                let t = self.mul_type(a, right_ty, span);
                Self::wrap_sparse(f, t)
            },
            | (BinOp::Mul, None, Some((f, b))) if is_scalar(left_ty) || is_tensor(left_ty) => {
                let t = self.mul_type(left_ty, b, span);
                Self::wrap_sparse(f, t)
            },
            | (BinOp::Div, Some((f, a)), None) if is_scalar(right_ty) => {
                let t = self.div_type(a, right_ty, span);
                Self::wrap_sparse(f, t)
            },
            | (BinOp::MatMul, Some((_, a)), Some((_, b))) => {
                let t = self.matmul_type(a, b, span);
                let csr = builtins::format_type("Csr").unwrap_or(Type::Error);
                Self::wrap_sparse(&csr, t)
            },
            | (BinOp::MatMul, Some((_, a)), None) => self.matmul_type(a, right_ty, span),
            | (BinOp::MatMul, None, Some((_, b))) => self.matmul_type(left_ty, b, span),
            | _ => {
                self.error_help(
                    "E0302",
                    format!(
                        "`{}` is not defined for {left_ty} and {right_ty}",
                        op.as_str()
                    ),
                    span,
                    "`Sparse` and `Tensor` never convert implicitly: use `to_dense` or `from_dense`",
                );
                Type::Error
            },
        };
        Some(result)
    }

    /// The format a type argument names (`from_dense<Coo>`).
    fn sparse_format_argument(
        &mut self,
        ty: &Ty,
    ) -> Option<Type> {
        let name = match &ty.kind {
            | TyKind::Named { path, args } if args.is_empty() => path.last(),
            | TyKind::Var(name) => Some(name),
            | _ => None,
        };
        let canonical = name.and_then(|name| {
            ["Csr", "Csc", "Coo", "Block", "Bcsr"]
                .into_iter()
                .find(|format| format.eq_ignore_ascii_case(name))
        });
        match canonical.and_then(builtins::format_type) {
            | Some(format) => Some(format),
            | None => {
                self.error_help(
                    "E0310",
                    "expected a sparse storage format",
                    ty.span,
                    "the formats are Csr, Csc, Coo, Block and Bcsr",
                );
                None
            },
        }
    }

    /// `from_dense<F>(A)`, `convert<F>(A)`, `speye<F>(n)`, `spdiag<F>(v)`.
    pub(super) fn sparse_format_call(
        &mut self,
        name: &str,
        type_args: &[Ty],
        args: &[Expr],
        span: Span,
    ) -> Type {
        let [format] = type_args else {
            self.error(
                "E0315",
                format!(
                    "`{name}` takes one format type argument, found {}",
                    type_args.len()
                ),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        };
        let Some(format) = self.sparse_format_argument(format) else {
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        };
        let candidates = builtins::sparse_schemes(name, Some(format)).unwrap_or_default();
        self.overloaded_call(name, &candidates, args, span)
    }

    /// The items of a list literal such as `[0, 1]`.
    const fn literal_list(expr: &Expr) -> Option<&[Expr]> {
        match &expr.kind {
            | ExprKind::Tensor(items) => Some(items.as_slice()),
            | _ => None,
        }
    }

    fn int_list_type(
        &mut self,
        expr: &Expr,
    ) -> Option<Type> {
        let expected = Type::tensor(Shape::Rank(1), Type::int());
        if Self::literal_list(expr).is_some_and(<[Expr]>::is_empty) {
            self.check(expr, &expected);
            return Some(expected);
        }
        let actual = self.synth(expr);
        if let Err(error) = self.infer.unify(&actual, &expected) {
            self.report_unify(error, expr.span);
            return None;
        }
        Some(self.infer.resolve(&actual))
    }

    /// `sparse_coo(shape, indices, values)` and `contract(A, B, axes_a, axes_b)`,
    /// whose result rank depends on a literal the schemes cannot see.
    pub(super) fn sparse_special_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let wanted = match (name, args) {
            | ("sparse_coo", [shape, indices, values]) => {
                return self.sparse_coo_call(shape, indices, values, span);
            },
            | ("contract", [left, right, left_axes, right_axes]) => {
                return self.contract_call(left, right, left_axes, right_axes, span);
            },
            | ("sparse_coo", _) => 3,
            | _ => 4,
        };
        self.error(
            "E0315",
            format!("`{name}` takes {wanted} arguments, found {}", args.len()),
            span,
        );
        for arg in args {
            self.synth(arg);
        }
        Type::Error
    }

    #[allow(
        clippy::single_call_fn,
        reason = "keeps COO construction separate from contraction and arity dispatch"
    )]
    fn sparse_coo_call(
        &mut self,
        shape: &Expr,
        indices: &Expr,
        values: &Expr,
        span: Span,
    ) -> Type {
        let shape_ty = self.int_list_type(shape);
        let index_ty = self.synth(indices);
        let value_ty = self.synth(values);
        let Some(shape_ty) = shape_ty else {
            return Type::Error;
        };
        let shape_rank = Self::literal_list(shape).map(<[Expr]>::len).or_else(|| {
            match shape_ty {
                | Type::Tensor { shape, .. } => shape.extent(0),
                | _ => None,
            }
        });
        let Some(rank) = self.coo_rank(shape_rank, &index_ty, indices.span, span) else {
            return Type::Error;
        };

        let element = self.infer.fresh_type();
        if let Err(error) = self
            .infer
            .unify(&value_ty, &Type::tensor(Shape::Rank(1), element.clone()))
        {
            self.report_unify(error, values.span);
            return Type::Error;
        }
        let element = self.force_scalar(&self.infer.resolve(&element));
        builtins::sparse_type(
            builtins::format_type("Coo").unwrap_or(Type::Error),
            Type::tensor(Shape::Rank(rank), element),
        )
    }

    #[allow(
        clippy::single_call_fn,
        reason = "bounds the coordinate layout and rank checks independently of element inference"
    )]
    fn coo_rank(
        &mut self,
        shape_rank: Option<usize>,
        indices: &Type,
        index_span: Span,
        call_span: Span,
    ) -> Option<u32> {
        let int = Type::int();
        let (coordinate_rank, description) = match self.infer.resolve(indices) {
            | Type::Tensor { shape, elem } if shape.rank() == 2 => {
                if let Err(error) = self.infer.unify(&elem, &int) {
                    self.report_unify(error, index_span);
                    return None;
                }
                (shape.extent(1), "an index matrix row")
            },
            | Type::Tensor { shape, elem } if shape.rank() == 1 => {
                match *elem {
                    | Type::Record(row) => {
                        let mut fields = &row;
                        let mut arity = 0usize;
                        while let Row::Extend { ty, tail, .. } = fields {
                            if let Err(error) = self.infer.unify(ty, &int) {
                                self.report_unify(error, index_span);
                                return None;
                            }
                            arity = arity.checked_add(1)?;
                            fields = tail;
                        }
                        (Some(arity), "an index tuple")
                    },
                    | other => {
                        if let Some(rank) = shape_rank
                            && rank > 1
                        {
                            self.error(
                                "E0802",
                                format!("a rank-{rank} tensor needs index tuples, found {other}"),
                                index_span,
                            );
                            return None;
                        }
                        if let Err(error) = self.infer.unify(&other, &int) {
                            self.report_unify(error, index_span);
                            return None;
                        }
                        (Some(1), "an index")
                    },
                }
            },
            | other => {
                self.error_help(
                    "E0302",
                    format!("`sparse_coo` indices must be an `Int` matrix or a list of tuples, found {other}"),
                    index_span,
                    "pass `[(0, 0), (1, 1)]` or a `Tensor<[nnz, rank], Int>`",
                );
                return None;
            },
        };
        let Some(rank) = shape_rank.or(coordinate_rank) else {
            self.error_help(
                "E0310",
                "the rank of a sparse tensor must be known when it is built",
                call_span,
                "write the shape as a list literal, as in `sparse_coo([n, n], …)`",
            );
            return None;
        };
        if rank == 0 {
            self.error("E0802", "a sparse tensor has at least one axis", call_span);
            return None;
        }
        if let (Some(rank), Some(arity)) = (shape_rank, coordinate_rank)
            && rank != arity
        {
            self.error(
                "E0802",
                format!("{description} has {arity} component(s), the tensor has rank {rank}"),
                index_span,
            );
            return None;
        }
        match u32::try_from(rank) {
            | Ok(rank) => Some(rank),
            | Err(_) => {
                self.error(
                    "E0802",
                    "sparse tensor rank exceeds the supported range",
                    call_span,
                );
                None
            },
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "contraction determines result rank from paired axes instead of using a fixed scheme"
    )]
    fn contract_call(
        &mut self,
        left: &Expr,
        right: &Expr,
        left_axes: &Expr,
        right_axes: &Expr,
        span: Span,
    ) -> Type {
        let a_ty = self.synth(left);
        let b_ty = self.synth(right);
        if self.int_list_type(left_axes).is_none() || self.int_list_type(right_axes).is_none() {
            return Type::Error;
        }
        let (Some(axes_a), Some(axes_b)) = (
            Self::literal_list(left_axes),
            Self::literal_list(right_axes),
        ) else {
            self.error_help(
                "E0310",
                "the contracted axes of `contract` must be list literals",
                span,
                "write `contract(A, B, [1], [0])`: the result rank depends on how many axes are contracted",
            );
            return Type::Error;
        };
        if axes_a.len() != axes_b.len() {
            self.error(
                "E0802",
                format!(
                    "`contract` pairs {} axes of the first operand with {} of the second",
                    axes_a.len(),
                    axes_b.len()
                ),
                span,
            );
            return Type::Error;
        }
        let operand = |this: &Self, ty: &Type| -> Option<(bool, Shape, Type)> {
            let resolved = this.infer.resolve(ty);
            if let Some((_, dense)) = builtins::sparse_parts(&resolved)
                && let Type::Tensor { shape, elem } = dense
            {
                return Some((true, shape.clone(), (**elem).clone()));
            }
            match resolved {
                | Type::Tensor { shape, elem } => Some((false, shape, *elem)),
                | _ => None,
            }
        };
        let (Some((a_sparse, a_shape, a_elem)), Some((b_sparse, b_shape, b_elem))) =
            (operand(self, &a_ty), operand(self, &b_ty))
        else {
            self.error(
                "E0302",
                format!("`contract` expects tensors or sparse tensors, found {a_ty} and {b_ty}"),
                span,
            );
            return Type::Error;
        };
        if !a_sparse && !b_sparse {
            self.error_help(
                "E0302",
                "`contract` needs at least one sparse operand",
                span,
                "dense tensors contract with `tensordot`",
            );
            return Type::Error;
        }
        let (rank_a, rank_b) = (a_shape.rank(), b_shape.rank());
        let Ok(contracted) = u32::try_from(axes_a.len()) else {
            self.error("E0802", "more contracted axes than the operands have", span);
            return Type::Error;
        };
        let (Some(free_a), Some(free_b)) = (
            rank_a.checked_sub(contracted),
            rank_b.checked_sub(contracted),
        ) else {
            self.error("E0802", "more contracted axes than the operands have", span);
            return Type::Error;
        };
        if !self.validate_contract_axes(axes_a, rank_a)
            || !self.validate_contract_axes(axes_b, rank_b)
        {
            return Type::Error;
        }
        let Some(rank) = free_a.checked_add(free_b) else {
            self.error(
                "E0302",
                "sparse contraction result rank exceeds the supported range",
                span,
            );
            return Type::Error;
        };
        let elem = self.multiply_elems(&a_elem, &b_elem, span);
        if rank == 0 {
            return elem;
        }
        let dense = Type::tensor(Shape::Rank(rank), elem);
        if a_sparse && b_sparse {
            builtins::sparse_type(builtins::format_type("Coo").unwrap_or(Type::Error), dense)
        } else {
            dense
        }
    }

    fn validate_contract_axes(
        &mut self,
        axes: &[Expr],
        rank: u32,
    ) -> bool {
        let mut seen = HashSet::new();
        for expr in axes {
            let Some(value) = literal_index(expr) else {
                continue;
            };
            if !u32::try_from(value).is_ok_and(|axis| axis < rank) {
                self.error(
                    "E0802",
                    format!("contraction axis {value} is out of range for rank {rank}"),
                    expr.span,
                );
                return false;
            }
            if !seen.insert(value) {
                self.error(
                    "E0802",
                    format!("contraction axis {value} appears more than once"),
                    expr.span,
                );
                return false;
            }
        }
        true
    }

    /// `A[i, j, …]` on a sparse tensor: one `Int` per axis, giving the
    /// element type. With a range on some axis it is a slice instead: the
    /// integer axes disappear, the ranged axes stay, and the result is a
    /// sparse tensor (rank 2 keeps the format, any other rank is `Coo`).
    pub(super) fn sparse_index(
        &mut self,
        format: &Type,
        dense: &Type,
        indices: &[SliceAst],
        span: Span,
    ) -> Type {
        let Type::Tensor { shape, elem } = dense else {
            return Type::Error;
        };
        let rank = shape.rank();
        if usize::try_from(rank) != Ok(indices.len()) {
            self.error(
                "E0302",
                format!(
                    "a rank-{rank} sparse tensor is indexed by {rank} Int(s), found {}",
                    indices.len()
                ),
                span,
            );
            return Type::Error;
        }
        let kept = indices
            .iter()
            .filter(|index| matches!(index, SliceAst::Range { .. }))
            .count();
        for index in indices {
            match index {
                | SliceAst::Index(expr) => {
                    let ty = self.synth(expr);
                    if let Err(error) = self.infer.unify(&ty, &Type::int()) {
                        self.report_unify(error, expr.span);
                    }
                },
                | SliceAst::Range { start, end, step, .. } => {
                    for part in [start, end, step].into_iter().flatten() {
                        let ty = self.synth(part);
                        if let Err(error) = self.infer.unify(&ty, &Type::int()) {
                            self.report_unify(error, part.span);
                        }
                    }
                },
            }
        }
        if kept == 0 {
            return self.force_scalar(&self.infer.resolve(elem));
        }
        let Ok(kept) = u32::try_from(kept) else {
            return Type::Error;
        };
        let marker = if kept == 2 {
            format.clone()
        } else {
            builtins::format_type("Coo").unwrap_or(Type::Error)
        };
        builtins::sparse_type(
            marker,
            Type::tensor(Shape::Rank(kept), self.infer.resolve(elem)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::check_module;
    use aevia_syntax::Severity;
    use aevia_syntax::lex;
    use aevia_syntax::parse_module;

    fn errors(source: &str) -> Vec<(String, String)> {
        let lexed = lex(0, source).expect("test source lexes");
        let (module, parsed) = parse_module(0, source, &lexed);
        assert!(!parsed.has_errors(), "{:?}", parsed.items);
        check_module(0, &module)
            .diagnostics
            .items
            .into_iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .map(|diagnostic| (diagnostic.code, diagnostic.message))
            .collect()
    }

    #[test]
    fn coo_rank_can_come_from_a_refined_index_matrix() {
        let errors = errors(
            "fn make(shape: Tensor<1, Int>, indices: Tensor<[3, 2], Int>, values: Tensor<1, Float>) = sparse_coo(shape, indices, values)",
        );
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn coo_rejects_a_matrix_width_that_disagrees_with_the_shape() {
        let errors = errors("let a = sparse_coo([2, 2], [[0, 0, 0]], [1.0])");
        assert_eq!(
            errors,
            [(
                "E0802".into(),
                "an index matrix row has 3 component(s), the tensor has rank 2".into()
            )]
        );
    }

    #[test]
    fn contraction_diagnoses_invalid_literal_axes() {
        for (axes, message) in [
            ("[0, 0]", "appears more than once"),
            ("[0, 2]", "out of range for rank 2"),
            ("[-1, 0]", "out of range for rank 2"),
        ] {
            let errors = errors(&format!(
                "let a = from_dense<Coo>([[1.0]])\nlet result = contract(a, a, {axes}, [0, 1])"
            ));
            assert!(
                errors
                    .iter()
                    .any(|(code, text)| code == "E0802" && text.contains(message)),
                "{errors:?}"
            );
        }
    }
}
