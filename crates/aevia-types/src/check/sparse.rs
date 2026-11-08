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
//! | `sparse * scalar`       | `Sparse`                                      |
//! | `sparse * sparse` (∘)   | `Sparse`                                      |
//! | `sparse * dense` (∘)    | `Sparse`                                      |
//! | `sparse @ dense`        | dense                                         |
//! | `dense @ sparse`        | dense                                         |
//! | `sparse @ sparse`       | `Sparse<Csr, …>`                              |
//!
//! There is no implicit conversion between `Sparse` and `Tensor`: a function
//! that wants one cannot be given the other (`to_dense` / `from_dense` are
//! explicit, §13.9.5).

use super::*;

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
    #[allow(clippy::too_many_arguments)]
    pub(super) fn sparse_binary(
        &mut self,
        op: BinOp,
        left_ty: &Type,
        right_ty: &Type,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) -> Option<Type> {
        let l = builtins::sparse_parts(left_ty).map(|(f, d)| (f.clone(), d.clone()));
        let r = builtins::sparse_parts(right_ty).map(|(f, d)| (f.clone(), d.clone()));
        if l.is_none() && r.is_none() {
            return None;
        }
        let csr = builtins::format_type("Csr").unwrap_or(Type::Error);
        let is_tensor = |ty: &Type| matches!(ty, Type::Tensor { .. });
        let is_scalar = |ty: &Type| matches!(ty, Type::Scalar(..) | Type::UnitOf(_));
        let result = match (op, &l, &r) {
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
            | TyKind::Named { path, args } if args.is_empty() => path.last().cloned(),
            | TyKind::Var(name) => Some(name.clone()),
            | _ => None,
        };
        let canonical = name.as_deref().map(|name| {
            match name.to_ascii_lowercase().as_str() {
                | "csr" => "Csr",
                | "csc" => "Csc",
                | "coo" => "Coo",
                | "block" => "Block",
                | "bcsr" => "Bcsr",
                | _ => "",
            }
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
        if type_args.len() != 1 {
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
        }
        let Some(format) = self.sparse_format_argument(&type_args[0]) else {
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        };
        let candidates = builtins::sparse_schemes(name, Some(format)).unwrap_or_default();
        self.overloaded_call(name, &candidates, args, span)
    }

    /// The literal length of a list expression (`[0, 1]`).
    fn literal_list_len(expr: &Expr) -> Option<usize> {
        match &expr.kind {
            | ExprKind::Tensor(items) => Some(items.len()),
            | _ => None,
        }
    }

    /// `sparse_coo(shape, indices, values)` and `contract(A, B, axes_a, axes_b)`,
    /// whose result rank depends on a literal the schemes cannot see.
    pub(super) fn sparse_special_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let wanted = if name == "sparse_coo" { 3 } else { 4 };
        if args.len() != wanted {
            self.error(
                "E0315",
                format!("`{name}` takes {wanted} arguments, found {}", args.len()),
                span,
            );
            for arg in args {
                self.synth(arg);
            }
            return Type::Error;
        }
        if name == "sparse_coo" {
            self.sparse_coo_call(args, span)
        } else {
            self.contract_call(args, span)
        }
    }

    fn sparse_coo_call(
        &mut self,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let shape_ty = self.synth(&args[0]);
        let index_ty = self.synth(&args[1]);
        let value_ty = self.synth(&args[2]);
        if let Err(error) = self
            .infer
            .unify(&shape_ty, &Type::tensor(Shape::Rank(1), Type::int()))
        {
            self.report_unify(error, args[0].span);
            return Type::Error;
        }
        let shape_resolved = self.infer.resolve(&shape_ty);
        let index_resolved = self.infer.resolve(&index_ty);
        // The rank: the literal shape's length, a refined shape's extent, or
        // the arity of the index tuples.
        let from_shape = Self::literal_list_len(&args[0]).or_else(|| {
            match &shape_resolved {
                | Type::Tensor {
                    shape: Shape::Refined(items),
                    ..
                } if items.len() == 1 => {
                    items[0]
                        .as_ref()
                        .and_then(|n| self.infer.resolve_nat(n).as_const())
                },
                | _ => None,
            }
        });
        let tuple_arity = match &index_resolved {
            | Type::Tensor { shape, elem } if shape.rank() == 1 => {
                match self.infer.resolve(elem) {
                    | Type::Record(row) => Some(row.fields().len()),
                    | Type::Scalar(..) => Some(1),
                    | _ => None,
                }
            },
            | _ => None,
        };
        let Some(rank) = from_shape.or(tuple_arity) else {
            self.error_help(
                "E0310",
                "the rank of a sparse tensor must be known when it is built",
                span,
                "write the shape as a list literal, as in `sparse_coo([n, n], …)`",
            );
            return Type::Error;
        };
        if rank == 0 {
            self.error("E0802", "a sparse tensor has at least one axis", span);
            return Type::Error;
        }
        // The indices: an `[nnz, rank]` Int matrix, or a list of Int tuples.
        let int = Type::int();
        match &index_resolved {
            | Type::Tensor { shape, elem } if shape.rank() == 2 => {
                if let Err(error) = self.infer.unify(elem, &int) {
                    self.report_unify(error, args[1].span);
                    return Type::Error;
                }
            },
            | Type::Tensor { shape, elem } if shape.rank() == 1 => {
                let element = self.infer.resolve(elem);
                match element {
                    | Type::Record(row) => {
                        let fields = row.fields();
                        if fields.len() != rank {
                            self.error(
                                "E0802",
                                format!(
                                    "index tuples have {} component(s), the tensor has rank {rank}",
                                    fields.len()
                                ),
                                args[1].span,
                            );
                            return Type::Error;
                        }
                        for (_, field) in &fields {
                            if let Err(error) = self.infer.unify(field, &int) {
                                self.report_unify(error, args[1].span);
                                return Type::Error;
                            }
                        }
                    },
                    | other => {
                        if rank != 1 {
                            self.error(
                                "E0802",
                                format!("a rank-{rank} tensor needs index tuples, found {other}"),
                                args[1].span,
                            );
                            return Type::Error;
                        }
                        if let Err(error) = self.infer.unify(&other, &int) {
                            self.report_unify(error, args[1].span);
                            return Type::Error;
                        }
                    },
                }
            },
            | other => {
                self.error_help(
                    "E0302",
                    format!("`sparse_coo` indices must be an `Int` matrix or a list of tuples, found {other}"),
                    args[1].span,
                    "pass `[(0, 0), (1, 1)]` or a `Tensor<[nnz, rank], Int>`",
                );
                return Type::Error;
            },
        }
        let element = self.infer.fresh_type();
        if let Err(error) = self
            .infer
            .unify(&value_ty, &Type::tensor(Shape::Rank(1), element.clone()))
        {
            self.report_unify(error, args[2].span);
            return Type::Error;
        }
        let element = self.force_scalar(&self.infer.resolve(&element));
        let rank = u32::try_from(rank).unwrap_or(u32::MAX);
        builtins::sparse_type(
            builtins::format_type("Coo").unwrap_or(Type::Error),
            Type::tensor(Shape::Rank(rank), element),
        )
    }

    fn contract_call(
        &mut self,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let a_ty = self.synth(&args[0]);
        let b_ty = self.synth(&args[1]);
        for axes in &args[2..] {
            let ty = self.synth(axes);
            if let Err(error) = self
                .infer
                .unify(&ty, &Type::tensor(Shape::Rank(1), Type::int()))
            {
                self.report_unify(error, axes.span);
                return Type::Error;
            }
        }
        let (Some(k_a), Some(k_b)) = (
            Self::literal_list_len(&args[2]),
            Self::literal_list_len(&args[3]),
        ) else {
            self.error_help(
                "E0310",
                "the contracted axes of `contract` must be list literals",
                span,
                "write `contract(A, B, [1], [0])`: the result rank depends on how many axes are contracted",
            );
            return Type::Error;
        };
        if k_a != k_b {
            self.error(
                "E0802",
                format!(
                    "`contract` pairs {k_a} axes of the first operand with {k_b} of the second"
                ),
                span,
            );
            return Type::Error;
        }
        let operand = |this: &Self, ty: &Type| -> Option<(bool, Shape, Type)> {
            let resolved = this.infer.resolve(ty);
            if let Some((_, dense)) = builtins::sparse_parts(&resolved) {
                if let Type::Tensor { shape, elem } = dense {
                    return Some((true, shape.clone(), (**elem).clone()));
                }
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
        let (rank_a, rank_b) = (a_shape.rank() as usize, b_shape.rank() as usize);
        if k_a > rank_a || k_a > rank_b {
            self.error("E0802", "more contracted axes than the operands have", span);
            return Type::Error;
        }
        let rank = (rank_a - k_a) + (rank_b - k_a);
        let elem = self.multiply_elems(&a_elem, &b_elem, span);
        if rank == 0 {
            return elem;
        }
        let dense = Type::tensor(Shape::Rank(u32::try_from(rank).unwrap_or(u32::MAX)), elem);
        if a_sparse && b_sparse {
            builtins::sparse_type(builtins::format_type("Coo").unwrap_or(Type::Error), dense)
        } else {
            dense
        }
    }

    /// `A[i, j, …]` on a sparse tensor: one `Int` per axis, giving the
    /// element type.
    pub(super) fn sparse_index(
        &mut self,
        dense: &Type,
        indices: &[SliceAst],
        span: Span,
    ) -> Type {
        let Type::Tensor { shape, elem } = dense else {
            return Type::Error;
        };
        let rank = shape.rank() as usize;
        if indices.len() != rank {
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
        for index in indices {
            match index {
                | SliceAst::Index(expr) => {
                    let ty = self.synth(expr);
                    if let Err(error) = self.infer.unify(&ty, &Type::int()) {
                        self.report_unify(error, expr.span);
                    }
                },
                | SliceAst::Range { .. } => {
                    self.error_help(
                        "E0302",
                        "a sparse tensor cannot be sliced",
                        span,
                        "index it with one `Int` per axis, or `to_dense` first",
                    );
                    return Type::Error;
                },
            }
        }
        self.force_scalar(&self.infer.resolve(elem))
    }
}
