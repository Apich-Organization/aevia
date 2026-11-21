//! Converts written type syntax into inferred types and dimensions.

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
use super::builtin_type;
use crate::adt::Adt;
use crate::adt::{
    self,
};
use crate::builtins;
use crate::ty::LiteralKind;
use crate::ty::Nat;
use crate::ty::Rep;
use crate::ty::Row;
use crate::ty::Shape;
use crate::ty::Type;
use aevia_dim::Dim;
use aevia_syntax::Diagnostic;
use aevia_syntax::Span;
use aevia_syntax::ast::DimAst;
use aevia_syntax::ast::NatAst;
use aevia_syntax::ast::ShapeAst;
use aevia_syntax::ast::ShapeItem;
use aevia_syntax::ast::Ty;
use aevia_syntax::ast::TyKind;

impl Checker {
    pub(super) fn convert_type(
        &mut self,
        ty: &Ty,
    ) -> Type {
        // An instance head may append implicit parameters while converting.
        // Named lookup uses the lexical prefix present when conversion began.
        self.convert_scoped_type(ty, self.type_params.len())
    }

    fn convert_scoped_type(
        &mut self,
        ty: &Ty,
        prefix_len: usize,
    ) -> Type {
        match &ty.kind {
            | TyKind::Int | TyKind::NatArg(_) | TyKind::ShapeArg(_) => Type::int(),
            | TyKind::Bool => Type::Bool,
            | TyKind::Prop => Type::Prop,
            | TyKind::Str => Type::string(),
            | TyKind::Unit => Type::Unit,
            | TyKind::Real(dim) | TyKind::DimArg(dim) => Type::real(self.convert_dim(dim, ty.span)),
            | TyKind::Float(dim) => Type::float(self.convert_dim(dim, ty.span)),
            | TyKind::Complex(dim) => Type::complex(self.convert_dim(dim, ty.span)),
            | TyKind::Point(dim) => Type::Point(self.convert_dim(dim, ty.span)),
            | TyKind::UnitOf(dim) => Type::UnitOf(self.convert_dim(dim, ty.span)),
            | TyKind::Tensor { shape, elem } => {
                self.convert_tensor_type(shape, elem, prefix_len, ty.span)
            },
            | TyKind::Matrix { rows, cols, elem } => {
                let shape = Shape::Refined(vec![
                    Some(self.convert_nat(rows, ty.span)),
                    Some(self.convert_nat(cols, ty.span)),
                ]);
                Type::tensor(shape, self.convert_scoped_type(elem, prefix_len))
            },
            | TyKind::Sparse { format, shape, elem } => {
                self.convert_sparse_type(format, shape, elem, prefix_len, ty.span)
            },
            | TyKind::Row(items) => self.convert_row_sum(items, prefix_len),
            | TyKind::Option(inner) => Type::option(self.convert_scoped_type(inner, prefix_len)),
            | TyKind::Result(ok, err) => {
                Type::result(
                    self.convert_scoped_type(ok, prefix_len),
                    self.convert_scoped_type(err, prefix_len),
                )
            },
            | TyKind::Set(inner) => {
                let inner = self.convert_scoped_type(inner, prefix_len);
                // §3.8.4: `Set<a>` needs `TotalOrd a`; `Set<Float>` is not
                // legal because `Float` has no total order (NaN).
                self.require_instance("TotalOrd", &inner, ty.span);
                Type::Adt(adt::set_id(), vec![inner])
            },
            | TyKind::Table(inner) => {
                Type::Adt(
                    adt::intern(Adt {
                        name: "Table".to_string(),
                        params: vec!["r".to_string()],
                        ctors: Vec::new(),
                    }),
                    vec![self.convert_scoped_type(inner, prefix_len)],
                )
            },
            | TyKind::Dyn(rep) => {
                Type::Dyn(match rep.as_str() {
                    | "Int" => Rep::Int,
                    | "Float" => Rep::Float,
                    | "Complex" => Rep::Complex,
                    | _ => Rep::Real,
                })
            },
            | TyKind::RateOf(inner) => {
                let inner = self.convert_scoped_type(inner, prefix_len);
                self.rate_of(&inner, ty.span)
            },
            | TyKind::DiffusionOf(inner) => {
                let inner = self.convert_scoped_type(inner, prefix_len);
                self.diffusion_of(&inner, ty.span)
            },
            | TyKind::SymFun(domain, codomain) => {
                let domain = self.convert_dim(domain, ty.span);
                let codomain = self.convert_dim(codomain, ty.span);
                Type::func(vec![Type::real(domain)], Type::real(codomain))
            },
            | TyKind::Io(inner) => Type::io(self.convert_scoped_type(inner, prefix_len)),
            | TyKind::Fn(args, ret) => {
                Type::func(
                    args.iter()
                        .map(|arg| self.convert_scoped_type(arg, prefix_len))
                        .collect(),
                    self.convert_scoped_type(ret, prefix_len),
                )
            },
            | TyKind::Record { fields, rest } => {
                self.convert_record_type(fields, rest.as_deref(), prefix_len)
            },
            | TyKind::Tuple(items) => {
                Type::record(
                    items
                        .iter()
                        .enumerate()
                        .map(|(index, ty)| {
                            (index.to_string(), self.convert_scoped_type(ty, prefix_len))
                        })
                        .collect::<Vec<_>>(),
                )
            },
            | TyKind::Named { path, args } => self.convert_named(path, args, prefix_len, ty.span),
            | TyKind::Var(name) => self.convert_type_variable(name, prefix_len, ty.span),
            | TyKind::Forall(_, body) => self.convert_scoped_type(body, prefix_len),
        }
    }

    fn convert_tensor_type(
        &mut self,
        shape: &ShapeAst,
        elem: &Ty,
        prefix_len: usize,
        span: Span,
    ) -> Type {
        let element = self.convert_scoped_type(elem, prefix_len);
        self.convert_shape(shape, span)
            .map_or(Type::Error, |shape| Type::tensor(shape, element))
    }

    #[allow(
        clippy::single_call_fn,
        reason = "sparse annotations validate a storage marker before combining it with the tensor type"
    )]
    fn convert_sparse_type(
        &mut self,
        format: &str,
        shape: &ShapeAst,
        elem: &Ty,
        prefix_len: usize,
        span: Span,
    ) -> Type {
        let dense = self.convert_tensor_type(shape, elem, prefix_len, span);
        if dense.is_error() {
            return dense;
        }
        let Some(marker) = builtins::SPARSE_FORMATS
            .into_iter()
            .find(|name| name.eq_ignore_ascii_case(format))
            .and_then(builtins::format_type)
        else {
            self.error_help(
                "E0310",
                format!("`{format}` is not a sparse format"),
                span,
                "the formats are Csr, Csc, Coo, Block and Bcsr",
            );
            return Type::Error;
        };
        builtins::sparse_type(marker, dense)
    }

    #[allow(
        clippy::single_call_fn,
        reason = "keeps the type-level record union rules local to the row syntax"
    )]
    fn convert_row_sum(
        &mut self,
        items: &[Ty],
        prefix_len: usize,
    ) -> Type {
        let mut merged = Vec::new();
        for item in items {
            match self.convert_scoped_type(item, prefix_len) {
                | Type::Record(row) => merged.extend(row.fields()),
                | other => return other,
            }
        }
        Type::record(merged)
    }

    #[allow(
        clippy::single_call_fn,
        reason = "builds a record type while resolving the written row tail"
    )]
    fn convert_record_type(
        &mut self,
        fields: &[(String, Ty)],
        rest: Option<&str>,
        prefix_len: usize,
    ) -> Type {
        let mut fields: Vec<_> = fields
            .iter()
            .map(|(name, ty)| (name.clone(), self.convert_scoped_type(ty, prefix_len)))
            .collect();
        let Some(rest) = rest else {
            return Type::record(fields);
        };
        let mut row = match self
            .row_params
            .iter()
            .rev()
            .find(|(param, _)| param == rest)
        {
            | Some((_, row)) => row.clone(),
            | None => self.infer.fresh_row(),
        };
        fields.sort_by(|left, right| left.0.cmp(&right.0));
        for (name, ty) in fields.into_iter().rev() {
            row = Row::Extend {
                label: name,
                ty: Box::new(ty),
                tail: Box::new(row),
            };
        }
        Type::Record(row)
    }

    #[allow(
        clippy::single_call_fn,
        reason = "preserves the namespace search order for alias, nominal, and built-in types"
    )]
    fn convert_named(
        &mut self,
        path: &[String],
        args: &[Ty],
        prefix_len: usize,
        span: Span,
    ) -> Type {
        let name = path.join(".");
        if args.is_empty()
            && let Some(parameter) = self.lexical_type_parameter(&name, prefix_len)
        {
            return parameter;
        }
        if self.declaring_alias.as_deref() == Some(name.as_str()) {
            self.error_help(
                "E0312",
                "non-positive recursive type",
                span,
                "a type alias cannot refer to itself; declare a variant type instead",
            );
            return Type::Error;
        }
        if let Some(alias) = self.aliases.get(&name).cloned() {
            return self.instantiate_alias(&alias, args, span);
        }
        if let Some(id) = self.adts.get(&name).copied() {
            // §2/§8.9.1: an ADT parameter may be dimension-kinded; a
            // dimension argument is encoded as `UnitOf(d)`, so the
            // argument vector stays a vector of types.
            let kinds = adt::param_kinds_of(id);
            if args.len() > kinds.len() {
                self.error(
                    "E0315",
                    format!(
                        "`{name}` takes {} type argument(s), found {}",
                        kinds.len(),
                        args.len()
                    ),
                    span,
                );
            }
            let mut converted = Vec::with_capacity(kinds.len());
            for (index, kind) in kinds.iter().enumerate() {
                match kind {
                    | adt::ParamKind::Type => {
                        let value = match args.get(index) {
                            | Some(arg) => self.convert_scoped_type(arg, prefix_len),
                            | None => self.infer.fresh_type(),
                        };
                        converted.push(value);
                    },
                    | adt::ParamKind::Dim(_) => {
                        let dim = match args.get(index) {
                            | Some(arg) => self.convert_type_as_dim(arg),
                            | None => self.infer.fresh_dim(),
                        };
                        converted.push(Type::UnitOf(dim));
                    },
                }
            }
            return Type::Adt(id, converted);
        }
        if let Some(builtin) = builtin_type(&name) {
            return builtin;
        }
        self.error("E0301", format!("unknown type `{name}`"), span);
        Type::Error
    }

    fn lexical_type_parameter(
        &self,
        name: &str,
        prefix_len: usize,
    ) -> Option<Type> {
        self.type_params
            .iter()
            .take(prefix_len)
            .rfind(|(parameter, _)| parameter == name)
            .map(|(_, ty)| ty.clone())
    }

    #[allow(
        clippy::single_call_fn,
        reason = "resolves a written variable through its lexical or implicit binder"
    )]
    fn convert_type_variable(
        &mut self,
        name: &str,
        prefix_len: usize,
        span: Span,
    ) -> Type {
        if let Some(ty) = self.lexical_type_parameter(name, prefix_len) {
            return ty;
        }
        if self.implicit_params {
            // Reuse an implicit variable introduced earlier in this head.
            if let Some((_, ty)) = self
                .type_params
                .iter()
                .rev()
                .find(|(param, _)| param == name)
            {
                return ty.clone();
            }
            // §3.8.1: a variable in an instance head is implicitly
            // universally quantified.
            let fresh = self.infer.fresh_type();
            self.type_params.push((name.to_string(), fresh.clone()));
            return fresh;
        }
        self.error("E0301", format!("unknown type variable `{name}`"), span);
        Type::Error
    }

    /// §8.9: `RateOf<s>` is a built-in type function — the derivative of a
    /// state has its dimension divided by `T`, recursively through tensors and
    /// records.
    ///
    /// A state field that cannot be flattened into the numeric kernel
    /// (`Int`, `Bool`, a function, a variant) is `E0302`: the ODE runtime
    /// flattens `Float` scalars and tensors into `Vec<f64>`, so anything else
    /// has no representation on that path. An open record row cannot be
    /// flattened either.
    pub(super) fn rate_of(
        &mut self,
        ty: &Type,
        span: Span,
    ) -> Type {
        let resolved = self.infer.resolve(ty);
        match resolved {
            | Type::Scalar(rep, dim) => {
                let rep = self.infer.resolve_rep(rep);
                let is_int = rep == Rep::Int
                    || matches!(self.infer.rep_literal_kind(rep), Some(LiteralKind::Int));
                if is_int {
                    self.error_help(
                        "E0302",
                        "an ODE state cannot contain an `Int` field",
                        span,
                        "make the state field a `Float` (the solver works on `f64`)",
                    );
                    return Type::Error;
                }
                Type::Scalar(rep, dim.div(&Dim::base("Time")))
            },
            | Type::Tensor { shape, elem } => {
                let elem = self.rate_of(&elem, span);
                Type::Tensor {
                    shape,
                    elem: Box::new(elem),
                }
            },
            | Type::Record(row) => {
                if row.tail_var().is_some() {
                    self.error_help(
                        "E0302",
                        "an ODE state cannot be an open record",
                        span,
                        "write the state as a closed record type",
                    );
                    return Type::Error;
                }
                let fields = row
                    .fields()
                    .into_iter()
                    .map(|(name, ty)| (name, self.rate_of(&ty, span)))
                    .collect::<Vec<_>>();
                Type::Record(Row::closed(fields))
            },
            | Type::Var(_) => {
                // An unannotated parameter of the right-hand side: commit it to
                // a scalar and use its literal kind.
                let scalar = self.force_scalar(&resolved);
                if matches!(self.infer.resolve(&scalar), Type::Error) {
                    Type::Error
                } else {
                    self.rate_of(&scalar, span)
                }
            },
            | other => {
                self.error_help(
                    "E0302",
                    format!("an ODE state cannot contain a value of type {other}"),
                    span,
                    "an `odeint` state is `Float` scalars, tensors and closed records of them",
                );
                Type::Error
            },
        }
    }

    /// §8.9.1: `DiffusionOf<s>` is the built-in type function for the diffusion
    /// of an SDE: a scalar's dimension is divided by `T^(1/2)` (the Brownian
    /// scaling `dW ~ √dt`), recursively through tensors and closed records.
    ///
    /// ```text
    /// DiffusionOf<Scalar<ρ,d>>        = Scalar<ρ, d / T^(1/2)>
    /// DiffusionOf<Tensor<n, τ>>       = Tensor<n, DiffusionOf<τ>>
    /// DiffusionOf<{ l1: τ1, … }>      = { l1: DiffusionOf<τ1>, … }
    /// ```
    ///
    /// `Int`/`Bool`/function fields and open rows are `E0302`, exactly as for
    /// `RateOf`: the SDE path flattens the state into `Vec<f64>`.
    pub(super) fn diffusion_of(
        &mut self,
        ty: &Type,
        span: Span,
    ) -> Type {
        let resolved = self.infer.resolve(ty);
        match resolved {
            | Type::Scalar(rep, dim) => {
                let rep = self.infer.resolve_rep(rep);
                let is_int = rep == Rep::Int
                    || matches!(self.infer.rep_literal_kind(rep), Some(LiteralKind::Int));
                if is_int {
                    self.error_help(
                        "E0302",
                        "an SDE state cannot contain an `Int` field",
                        span,
                        "make the state field a `Float` (the integrator works on `f64`)",
                    );
                    return Type::Error;
                }
                let half = num_rational::BigRational::new(1.into(), 2.into());
                let sqrt_time = Dim::base("Time").pow(&half);
                // `dW ~ √dt`, so the diffusion's dimension is the state's
                // *divided* by `T^(1/2)` (Brownian scaling).
                Type::Scalar(rep, dim.div(&sqrt_time))
            },
            | Type::Tensor { shape, elem } => {
                let elem = self.diffusion_of(&elem, span);
                Type::Tensor {
                    shape,
                    elem: Box::new(elem),
                }
            },
            | Type::Record(row) => {
                if row.tail_var().is_some() {
                    self.error_help(
                        "E0302",
                        "an SDE state cannot be an open record",
                        span,
                        "write the state as a closed record type",
                    );
                    return Type::Error;
                }
                let fields = row
                    .fields()
                    .into_iter()
                    .map(|(name, ty)| (name, self.diffusion_of(&ty, span)))
                    .collect::<Vec<_>>();
                Type::Record(Row::closed(fields))
            },
            | Type::Var(_) => {
                let scalar = self.force_scalar(&resolved);
                if matches!(self.infer.resolve(&scalar), Type::Error) {
                    Type::Error
                } else {
                    self.diffusion_of(&scalar, span)
                }
            },
            | other => {
                self.error_help(
                    "E0302",
                    format!("an SDE state cannot contain a value of type {other}"),
                    span,
                    "an SDE state is `Float` scalars, tensors and closed records of them",
                );
                Type::Error
            },
        }
    }

    /// §8.9.1: the diffusion of `sdeint_general` for a state `s` with `m` noise
    /// channels (the channel count is a run-time check, like the matrix width of
    /// a rank-1 state): `GeneralDiffusionOf<s>` is `DiffusionOf<s>` with a
    /// trailing noise axis on every leaf.
    ///
    /// ```text
    /// GeneralDiffusionOf<Scalar<ρ,d>>   = Tensor<1, Scalar<ρ, d / T^(1/2)>>
    /// GeneralDiffusionOf<Tensor<n, τ>>  = Tensor<n + 1, DiffusionOf<τ>>
    /// GeneralDiffusionOf<{ l: τ, … }>   = { l: GeneralDiffusionOf<τ>, … }
    /// ```
    ///
    /// Flattening it field by field, row-major, is the kernel's `n x m` diffusion
    /// matrix (a scalar field is one row of `m` coefficients).
    pub(super) fn general_diffusion_of(
        &mut self,
        ty: &Type,
        span: Span,
    ) -> Type {
        let resolved = self.infer.resolve(ty);
        match resolved {
            | Type::Scalar(..) => {
                let leaf = self.diffusion_of(&resolved, span);
                if leaf.is_error() {
                    Type::Error
                } else {
                    Type::tensor(Shape::Rank(1), leaf)
                }
            },
            | Type::Tensor { shape, elem } => {
                let leaf = self.diffusion_of(&elem, span);
                if leaf.is_error() {
                    return Type::Error;
                }
                let shape = match shape {
                    | Shape::Rank(rank) => {
                        let Some(rank) = rank.checked_add(1) else {
                            self.error(
                                "E0302",
                                "SDE diffusion rank exceeds the supported range",
                                span,
                            );
                            return Type::Error;
                        };
                        Shape::Rank(rank)
                    },
                    | Shape::Refined(mut items) => {
                        items.push(None);
                        Shape::Refined(items)
                    },
                };
                Type::Tensor {
                    shape,
                    elem: Box::new(leaf),
                }
            },
            | Type::Record(row) => {
                if row.tail_var().is_some() {
                    self.error_help(
                        "E0302",
                        "an SDE state cannot be an open record",
                        span,
                        "write the state as a closed record type",
                    );
                    return Type::Error;
                }
                let fields = row
                    .fields()
                    .into_iter()
                    .map(|(name, ty)| (name, self.general_diffusion_of(&ty, span)))
                    .collect::<Vec<_>>();
                Type::Record(Row::closed(fields))
            },
            | Type::Var(_) => {
                let scalar = self.force_scalar(&resolved);
                if matches!(self.infer.resolve(&scalar), Type::Error) {
                    Type::Error
                } else {
                    self.general_diffusion_of(&scalar, span)
                }
            },
            // `Int`/`Bool`/function fields: the same `E0302` as `DiffusionOf`.
            | other => self.diffusion_of(&other, span),
        }
    }

    pub(super) fn convert_shape(
        &mut self,
        shape: &ShapeAst,
        span: Span,
    ) -> Option<Shape> {
        match shape {
            | ShapeAst::Rank(nat) => {
                let converted = self.convert_nat_term(nat, span);
                if let Some(value) = converted.as_const() {
                    if let Ok(rank) = u32::try_from(value) {
                        return Some(Shape::Rank(rank));
                    }
                } else if !converted.vars().is_empty() {
                    self.error_help(
                        "E0304",
                        "the rank of a tensor must be a concrete natural number",
                        span,
                        "write a concrete rank, for example `Tensor<1, Float>`, or use a refined shape `Tensor<[n], Float>`",
                    );
                    return None;
                }
                self.error("E0304", "tensor rank exceeds the supported range", span);
                None
            },
            | ShapeAst::Refined(items) => {
                Some(Shape::Refined(
                    items
                        .iter()
                        .map(|item| {
                            match item {
                                | ShapeItem::Known(nat) => Some(self.convert_nat(nat, span)),
                                | ShapeItem::Unknown => None,
                            }
                        })
                        .collect(),
                ))
            },
        }
    }

    /// Converts a type-level natural number (§3.5.1).
    ///
    /// A variable resolves through the enclosing declaration's `<n: Nat>` (or
    /// an unkinded parameter, which gets a Nat reading too). An unknown name is
    /// `E0304`: unlike an unkinded type parameter, an undeclared Nat variable
    /// has no binder to quantify over. Inside an instance head the name is an
    /// implicit parameter, like a dimension there.
    pub(super) fn convert_nat(
        &mut self,
        nat: &NatAst,
        span: Span,
    ) -> Nat {
        let converted = self.convert_nat_term(nat, span);
        if converted.as_const().is_none() && converted.vars().is_empty() {
            self.error("E0304", "shape extent exceeds the supported range", span);
        }
        converted
    }

    fn convert_nat_term(
        &mut self,
        nat: &NatAst,
        span: Span,
    ) -> Nat {
        match nat {
            | NatAst::Lit(value) => Nat::Lit(*value),
            | NatAst::Var(name) => {
                if let Some((_, nat)) = self
                    .nat_params
                    .iter()
                    .rev()
                    .find(|(param, _)| param == name)
                {
                    return nat.clone();
                }
                if self.implicit_params {
                    return self.infer.fresh_nat();
                }
                self.error_help(
                    "E0304",
                    format!("cannot infer the shape extent `{name}`"),
                    span,
                    "declare it, for example `fn f<n>(xs: Tensor<[n], Float>)`; only literals, `+` and `*` by a literal are supported in a shape",
                );
                Nat::Lit(0)
            },
            | NatAst::Add(left, right) => {
                let left = self.convert_nat_term(left, span);
                let right = self.convert_nat_term(right, span);
                left.fold_add(right)
            },
            | NatAst::Mul(left, right) => {
                let left = self.convert_nat_term(left, span);
                let right = self.convert_nat_term(right, span);
                left.fold_mul(right)
            },
        }
    }

    pub(super) fn convert_dim(
        &mut self,
        dim: &DimAst,
        span: Span,
    ) -> Dim {
        match dim {
            | DimAst::One => Dim::one(),
            | DimAst::Name(name) => {
                if let Some((_, dim)) = self
                    .dim_params
                    .iter()
                    .rev()
                    .find(|(param, _)| param == name)
                {
                    return dim.clone();
                }
                match self.dims.lookup(name) {
                    // §5.11.2: a dimension written in a module with a unit
                    // system denotes its representative (`L` is `Energy^-1`
                    // in natural units). The reduction is a homomorphism, so
                    // reducing each named factor reduces the whole product.
                    | Some(dim) if !self.si_dimensions => self.units.reduce_dim(&dim),
                    | Some(dim) => dim,
                    | None => {
                        if self.implicit_params {
                            // §3.8.1: `instance Show for Real[d]` quantifies
                            // `d` implicitly.
                            let fresh = self.infer.fresh_dim();
                            self.dim_params.push((name.clone(), fresh.clone()));
                            return fresh;
                        }
                        let help = self.unknown_dimension_help(name);
                        let mut diagnostic =
                            Diagnostic::error("E0407", format!("unknown dimension `{name}`"), span);
                        if let Some(help) = help {
                            diagnostic = diagnostic.with_help(help);
                        }
                        self.diagnostics.push(diagnostic);
                        Dim::one()
                    },
                }
            },
            | DimAst::Mul(left, right) => {
                self.convert_dim(left, span)
                    .mul(&self.convert_dim(right, span))
            },
            | DimAst::Div(left, right) => {
                self.convert_dim(left, span)
                    .div(&self.convert_dim(right, span))
            },
            | DimAst::Pow(base, exponent) => self.convert_dim(base, span).pow(exponent),
            | DimAst::Paren(inner) => self.convert_dim(inner, span),
        }
    }
}
