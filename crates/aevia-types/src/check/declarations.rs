//! Dimension, unit, alias and nominal type declarations.

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

use super::AliasExport;
use super::AliasParamKind;
use super::Checker;
use super::Export;
use super::ParamUsage;
use super::bare_type_name;
use super::ctor_scheme;
use crate::adt::Ctor;
use crate::adt::{
    self,
};
use crate::ty::Nat;
use crate::ty::Row;
use crate::ty::Scheme;
use crate::ty::Type;
use aevia_dim::Atom;
use aevia_dim::Dim;
use aevia_dim::Scale;
use aevia_dim::UnitDef;
use aevia_dim::UnitError;
use aevia_dim::UnitExpr;
use aevia_syntax::Span;
use aevia_syntax::ast::Constructor;
use aevia_syntax::ast::Decl;
use aevia_syntax::ast::DeclKind;
use aevia_syntax::ast::DimAst;
use aevia_syntax::ast::Kind;
use aevia_syntax::ast::Ty;
use aevia_syntax::ast::TyKind;
use aevia_syntax::ast::TypeBody;
use aevia_syntax::ast::TypeParam;
use std::collections::HashMap;

#[derive(Clone)]
pub(super) struct AliasDef {
    /// Expanded body in this checker's inference environment.
    pub(super) ty: Type,
    params: Vec<AliasParameter>,
}

/// A type placeholder uses its ordinal in the declaration. The other kinds
/// use fresh inference identities while the alias is in scope.
#[derive(Clone, Copy)]
enum AliasParameter {
    Type,
    Dim(u32),
    Nat(u32),
    Row(u32),
}

impl AliasParameter {
    const fn kind(self) -> AliasParamKind {
        match self {
            | Self::Type => AliasParamKind::Type,
            | Self::Dim(_) => AliasParamKind::Dim,
            | Self::Nat(_) => AliasParamKind::Nat,
            | Self::Row(_) => AliasParamKind::Row,
        }
    }
}

/// Moves the kinded placeholders between declaration positions and inference
/// identities, while preserving the ordinal of every ordinary type parameter.
fn remap_alias_parameters(
    body: &Type,
    params: &[AliasParameter],
    coordinates: impl Fn(u32, u32) -> (u32, u32),
) -> Type {
    let mut types = Vec::with_capacity(params.len());
    let mut dims = HashMap::new();
    let mut nats = HashMap::new();
    let mut rows = HashMap::new();
    for (position, param) in (0..=u32::MAX).zip(params) {
        let ty = match *param {
            | AliasParameter::Type => Some(Type::Param(position)),
            | AliasParameter::Dim(local) => {
                let (from, to) = coordinates(position, local);
                dims.insert(from, Dim::var(to));
                None
            },
            | AliasParameter::Nat(local) => {
                let (from, to) = coordinates(position, local);
                nats.insert(from, Nat::Var(to));
                None
            },
            | AliasParameter::Row(local) => {
                let (from, to) = coordinates(position, local);
                rows.insert(from, Row::Var(to));
                None
            },
        };
        types.push(ty);
    }
    body.substitute_kinded(&types, &dims, &nats, &rows)
}

impl Checker {
    pub(super) fn declare_dimension(
        &mut self,
        name: &str,
        value: Option<&DimAst>,
        span: Span,
        private: bool,
    ) {
        let (dim, is_base) = match value {
            | Some(ast) => {
                let dim = self.convert_dim(ast, span);
                self.dims.define_alias(name, dim.clone());
                (dim, false)
            },
            | None => {
                if !self
                    .dims
                    .define_nominal_base(name, &self.module_path.identity_key())
                {
                    self.error(
                        "E0316",
                        format!("dimension `{name}` is already defined"),
                        span,
                    );
                    return;
                }
                let Some(dim) = self.dims.lookup(name) else {
                    self.error("E0407", format!("could not bind dimension `{name}`"), span);
                    return;
                };
                (dim, true)
            },
        };
        if !private {
            self.checked
                .exports
                .insert(name.to_string(), Export::Dimension { dim, is_base });
        }
    }

    pub(super) fn declare_unit(
        &mut self,
        decl: &Decl,
    ) {
        let DeclKind::Unit {
            name,
            dim,
            scale,
            base,
            offset,
            prefixable,
        } = &decl.kind
        else {
            return;
        };
        // §5.11.4: a unit is a definition in SI, wherever it is declared; a
        // module with a unit system sees it through the system like any other
        // unit, and an SI importer sees the unit it wrote.
        let si_dimensions = std::mem::replace(&mut self.si_dimensions, true);
        let dimension = self.convert_dim(dim, decl.span);
        self.si_dimensions = si_dimensions;
        let Some(scale) = self.declared_unit_scale(name, scale, base.as_ref(), None, decl.span)
        else {
            return;
        };
        let offset_value = if let Some((offset, base)) = offset {
            let Some(value) =
                self.declared_unit_scale(name, offset, base.as_ref(), Some(&dimension), decl.span)
            else {
                return;
            };
            Some(value)
        } else {
            None
        };
        let affine = offset_value.is_some();
        let def = UnitDef {
            name: name.clone(),
            aliases: Vec::new(),
            dim: dimension.clone(),
            scale,
            offset: offset_value,
            prefixable: *prefixable,
            affine,
            display: name.clone(),
            category: "user".to_string(),
        };
        if let Err(error) = self.units.insert(def.clone()) {
            self.unit_error(error, decl.span);
            return;
        }
        let local = self.units.reduce_dim(&dimension);
        self.globals
            .insert(name.clone(), Scheme::mono(Type::UnitOf(local)));
        if !decl.private {
            self.checked
                .exports
                .insert(name.clone(), Export::Unit { def });
        }
    }

    /// Resolves a scale or an affine offset in SI, the unit definition's basis.
    fn declared_unit_scale(
        &mut self,
        name: &str,
        scale: &Scale,
        base: Option<&UnitExpr>,
        offset_dim: Option<&Dim>,
        span: Span,
    ) -> Option<Scale> {
        let Some(base) = base else {
            return Some(scale.clone());
        };
        let resolved = match self.units.resolve_si(base) {
            | Ok(resolved) => resolved,
            | Err(error) => {
                self.unit_error(error, span);
                return None;
            },
        };
        if let Some(dimension) = offset_dim
            && resolved.dim.is_concrete()
            && dimension.is_concrete()
            && resolved.dim != *dimension
        {
            let offset_dim = self.dims.format(&resolved.dim);
            let unit_dim = self.dims.format(dimension);
            self.error_help(
                "E0504",
                format!("the offset measures {offset_dim} but the unit measures {unit_dim}"),
                span,
                "an affine offset must have the unit's own dimension",
            );
            return None;
        }
        scale.checked_mul(&resolved.scale).or_else(|| {
            self.unit_error(UnitError::InvalidScale(name.to_string()), span);
            None
        })
    }

    /// The declared parameter kinds of a generic alias or variant type, when it
    /// is already in scope. Used to propagate a kind through a nested
    /// application (`type Opts<d> = | Fixed(FixedOpts<d>) | …`).
    fn declared_param_kinds(
        &self,
        name: &str,
    ) -> Option<Vec<AliasParamKind>> {
        if let Some(alias) = self.aliases.get(name) {
            return Some(alias.params.iter().map(|param| param.kind()).collect());
        }
        self.adts.get(name).map(|id| {
            adt::param_kinds_of(*id)
                .into_iter()
                .map(|kind| {
                    match kind {
                        | adt::ParamKind::Type => AliasParamKind::Type,
                        | adt::ParamKind::Dim(_) => AliasParamKind::Dim,
                    }
                })
                .collect()
        })
    }

    /// Propagates a written argument's kind from the callee's declaration into
    /// the usage sets, so an unannotated parameter used only as a *dimension
    /// argument* (`FixedOpts<d>`) is inferred as a dimension.
    fn scan_kind_args(
        &self,
        ty: &Ty,
        usage: &mut ParamUsage,
    ) {
        match &ty.kind {
            | TyKind::Named { path, args } => {
                let kinds = self.declared_param_kinds(&path.join("."));
                for (index, arg) in args.iter().enumerate() {
                    let declared = kinds.as_ref().and_then(|kinds| kinds.get(index)).copied();
                    let names = match declared {
                        | Some(AliasParamKind::Dim) => Some(&mut usage.dims),
                        | Some(AliasParamKind::Nat) => Some(&mut usage.nats),
                        | Some(AliasParamKind::Row) => Some(&mut usage.rows),
                        | Some(AliasParamKind::Type) | None => None,
                    };
                    if let Some((names, name)) = names.zip(bare_type_name(arg)) {
                        names.insert(name.clone());
                    } else {
                        self.scan_kind_args(arg, usage);
                    }
                }
            },
            | TyKind::Tensor { elem, .. }
            | TyKind::Sparse { elem, .. }
            | TyKind::Matrix { elem, .. }
            | TyKind::Option(elem)
            | TyKind::Set(elem)
            | TyKind::Table(elem)
            | TyKind::RateOf(elem)
            | TyKind::DiffusionOf(elem)
            | TyKind::Io(elem) => self.scan_kind_args(elem, usage),
            | TyKind::Result(ok, error) => {
                self.scan_kind_args(ok, usage);
                self.scan_kind_args(error, usage);
            },
            | TyKind::Fn(params, ret) => {
                for param in params {
                    self.scan_kind_args(param, usage);
                }
                self.scan_kind_args(ret, usage);
            },
            | TyKind::Record { fields, .. } => {
                for (_, field) in fields {
                    self.scan_kind_args(field, usage);
                }
            },
            | TyKind::Tuple(items) | TyKind::Row(items) => {
                for item in items {
                    self.scan_kind_args(item, usage);
                }
            },
            | TyKind::Forall(_, body) => self.scan_kind_args(body, usage),
            | _ => {},
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "collects declaration kind requirements before installing its inference bindings"
    )]
    fn declaration_param_kinds(
        &mut self,
        params: &[TypeParam],
        body: &TypeBody,
        span: Span,
    ) -> (Vec<AliasParamKind>, Vec<(usize, AliasParamKind)>) {
        let mut usage = ParamUsage::default();
        let mut scan_field = |ty| {
            usage.scan(ty);
            self.scan_kind_args(ty, &mut usage);
        };
        match body {
            | TypeBody::Alias(ty) => scan_field(ty),
            | TypeBody::Variants(ctors) => {
                ctors
                    .iter()
                    .flat_map(|ctor| &ctor.args)
                    .for_each(scan_field);
            },
            | TypeBody::Abstract => {},
        }
        let mut kinds = Vec::with_capacity(params.len());
        let mut extra: Vec<(usize, AliasParamKind)> = Vec::new();
        for (index, param) in params.iter().enumerate() {
            let name = &param.name;
            let explicit = match param.kind {
                | Some(Kind::Type) => Some(AliasParamKind::Type),
                | Some(Kind::Dim) => Some(AliasParamKind::Dim),
                | Some(Kind::Nat) => Some(AliasParamKind::Nat),
                | Some(Kind::Row) => Some(AliasParamKind::Row),
                | Some(other) => {
                    self.error_help(
                        "E0317",
                        format!(
                            "the parameter `{name}` has a kind that a type declaration does not support: {other:?}"
                        ),
                        span,
                        "use `Type`, `Dim`, `Nat` or `Row`, or omit the kind so it is inferred from the body",
                    );
                    Some(AliasParamKind::Type)
                },
                | None => None,
            };
            let dim_use = usage.dims.contains(name);
            let nat_use = usage.nats.contains(name);
            let row_use = usage.rows.contains(name);
            let mut candidates = [
                (row_use, AliasParamKind::Row),
                (dim_use, AliasParamKind::Dim),
                (nat_use, AliasParamKind::Nat),
            ]
            .into_iter()
            .filter_map(|(used, kind)| used.then_some(kind));
            let inferred = candidates.next();
            if explicit.is_none()
                && let Some(conflict) = candidates.next()
            {
                self.error_help(
                    "E0317",
                    format!("cannot infer the kind of `{name}`: the body uses it in incompatible positions"),
                    span,
                    "annotate it, for example `type Foo<d: Dim>`",
                );
                // Give the non-primary readings a placeholder too, so the
                // declaration does not cascade into bogus E0407/E0304 errors.
                extra.extend(
                    std::iter::once(conflict)
                        .chain(candidates)
                        .map(|kind| (index, kind)),
                );
            }
            kinds.push(explicit.or(inferred).unwrap_or(AliasParamKind::Type));
        }
        (kinds, extra)
    }

    fn fresh_alias_parameter(
        &mut self,
        kind: AliasParamKind,
    ) -> Option<AliasParameter> {
        match kind {
            | AliasParamKind::Type => Some(AliasParameter::Type),
            | AliasParamKind::Dim => {
                let fresh = self.infer.fresh_dim();
                let (Atom::Var(id), _) = fresh.factors().next()? else {
                    return None;
                };
                Some(AliasParameter::Dim(*id))
            },
            | AliasParamKind::Nat => {
                let Nat::Var(id) = self.infer.fresh_nat() else {
                    return None;
                };
                Some(AliasParameter::Nat(id))
            },
            | AliasParamKind::Row => {
                let Row::Var(id) = self.infer.fresh_row() else {
                    return None;
                };
                Some(AliasParameter::Row(id))
            },
        }
    }

    fn bind_declaration_parameter(
        &mut self,
        name: &str,
        position: u32,
        param: AliasParameter,
    ) {
        let name = name.to_string();
        match param {
            | AliasParameter::Type => self.type_params.push((name, Type::Param(position))),
            | AliasParameter::Dim(id) => self.dim_params.push((name, Dim::var(id))),
            | AliasParameter::Nat(id) => self.nat_params.push((name, Nat::Var(id))),
            | AliasParameter::Row(id) => self.row_params.push((name, Row::Var(id))),
        }
    }

    /// Converts local inference ids to the interface's declaration positions.
    pub(super) fn canonical_alias_export(def: &AliasDef) -> AliasExport {
        AliasExport {
            params: def.params.len(),
            body: remap_alias_parameters(&def.ty, &def.params, |position, local| (local, position)),
            kinds: def.params.iter().map(|param| param.kind()).collect(),
        }
    }

    /// Rebuilds an imported alias with this checker's own fresh placeholders.
    pub(super) fn imported_alias_def(
        &mut self,
        alias: &AliasExport,
    ) -> AliasDef {
        let params = if alias.params == alias.kinds.len() && u32::try_from(alias.params).is_ok() {
            alias
                .kinds
                .iter()
                .map(|kind| self.fresh_alias_parameter(*kind))
                .collect()
        } else {
            None
        };
        let Some(params): Option<Vec<_>> = params else {
            self.error(
                "E0317",
                "an imported alias has invalid parameter metadata",
                Span::new(self.file, 0, 0),
            );
            return AliasDef {
                ty: Type::Error,
                params: Vec::new(),
            };
        };
        AliasDef {
            ty: remap_alias_parameters(&alias.body, &params, |position, local| (position, local)),
            params,
        }
    }

    /// Expands an alias by substituting each argument according to its kind.
    pub(super) fn instantiate_alias(
        &mut self,
        alias: &AliasDef,
        args: &[Ty],
        span: Span,
    ) -> Type {
        if alias.ty == Type::Error {
            return Type::Error;
        }
        if args.len() > alias.params.len() {
            self.error(
                "E0315",
                format!(
                    "the alias takes {} type argument(s), found {}",
                    alias.params.len(),
                    args.len()
                ),
                span,
            );
        }
        let mut types = Vec::with_capacity(alias.params.len());
        let mut dims = HashMap::new();
        let mut nats = HashMap::new();
        let mut rows = HashMap::new();
        for (index, param) in alias.params.iter().enumerate() {
            let arg = args.get(index);
            let ty = match *param {
                | AliasParameter::Type => {
                    Some(match arg {
                        | Some(arg) => self.convert_type(arg),
                        | None => self.infer.fresh_type(),
                    })
                },
                | AliasParameter::Dim(id) => {
                    let dim = match arg {
                        | Some(arg) => self.convert_type_as_dim(arg),
                        | None => self.infer.fresh_dim(),
                    };
                    dims.insert(id, dim);
                    None
                },
                | AliasParameter::Nat(id) => {
                    let nat = match arg {
                        | Some(arg) => self.convert_type_as_nat(arg),
                        | None => self.infer.fresh_nat(),
                    };
                    nats.insert(id, nat);
                    None
                },
                | AliasParameter::Row(id) => {
                    let row = match arg {
                        | Some(arg) => self.convert_type_as_row(arg),
                        | None => self.infer.fresh_row(),
                    };
                    rows.insert(id, row);
                    None
                },
            };
            types.push(ty);
        }
        alias.ty.substitute_kinded(&types, &dims, &nats, &rows)
    }

    /// Converts a type argument written for a dimension-kinded parameter.
    pub(super) fn convert_type_as_dim(
        &mut self,
        ty: &Ty,
    ) -> Dim {
        let name = match &ty.kind {
            | TyKind::Named { path, args } if args.is_empty() => Some(path.join(".")),
            | TyKind::Var(name) => Some(name.clone()),
            | _ => None,
        };
        if let Some(name) = name {
            if let Some((_, dim)) = self
                .dim_params
                .iter()
                .rev()
                .find(|(param, _)| *param == name)
            {
                return dim.clone();
            }
            if let Some(dim) = self.dims.lookup(&name) {
                return dim;
            }
            if self.implicit_params {
                return self.infer.fresh_dim();
            }
            self.error_help(
                "E0407",
                format!("unknown dimension `{name}` in a type argument"),
                ty.span,
                "the parameter is dimension-kinded; pass a dimension such as `L`",
            );
            return Dim::one();
        }
        match &ty.kind {
            | TyKind::DimArg(dim) => self.convert_dim(dim, ty.span),
            | _ => {
                self.error_help(
                    "E0407",
                    "this type argument must be a dimension",
                    ty.span,
                    "declare the parameter as `<d: Dim>` or pass a dimension such as `L`",
                );
                Dim::one()
            },
        }
    }

    /// Converts a type argument written for a natural-number-kinded parameter.
    pub(super) fn convert_type_as_nat(
        &mut self,
        ty: &Ty,
    ) -> Nat {
        match bare_type_name(ty) {
            | Some(name) => {
                if let Some((_, nat)) = self
                    .nat_params
                    .iter()
                    .rev()
                    .find(|(param, _)| param == name)
                {
                    return nat.clone();
                }
                self.error_help(
                    "E0304",
                    format!("cannot infer the shape extent `{name}`"),
                    ty.span,
                    "the parameter is a type-level natural; pass a number such as `3`",
                );
                Nat::Lit(0)
            },
            | None => {
                match &ty.kind {
                    | TyKind::NatArg(nat) => self.convert_nat(nat, ty.span),
                    | _ => {
                        self.error_help(
                            "E0304",
                            "this type argument must be a type-level natural number",
                            ty.span,
                            "declare the parameter as `<n: Nat>` or pass a number such as `3`",
                        );
                        Nat::Lit(0)
                    },
                }
            },
        }
    }

    /// Converts a type argument written for a row-kinded parameter.
    pub(super) fn convert_type_as_row(
        &mut self,
        ty: &Ty,
    ) -> Row {
        let converted = self.convert_type(ty);
        match self.infer.resolve(&converted) {
            | Type::Record(row) => row,
            | other => {
                self.error_help(
                    "E0302",
                    format!("a row argument must be a record type, found {other}"),
                    ty.span,
                    "declare the parameter as `<r: Row>` and pass a record such as `{ x: Int }`",
                );
                Row::Empty
            },
        }
    }

    pub(super) fn declare_type(
        &mut self,
        name: &str,
        params: &[TypeParam],
        body: &TypeBody,
        span: Span,
        private: bool,
    ) {
        if u32::try_from(params.len()).is_err() {
            self.error("E0317", "too many type declaration parameters", span);
            return;
        }
        let (kinds, extra) = self.declaration_param_kinds(params, body, span);
        let Some(bindings) = kinds
            .into_iter()
            .map(|kind| self.fresh_alias_parameter(kind))
            .collect::<Option<Vec<_>>>()
        else {
            self.error(
                "E0317",
                "could not allocate type declaration parameters",
                span,
            );
            return;
        };
        let outer_types = std::mem::take(&mut self.type_params);
        let outer_dims = std::mem::take(&mut self.dim_params);
        let outer_nats = std::mem::take(&mut self.nat_params);
        let outer_rows = std::mem::take(&mut self.row_params);
        for (position, (param, binding)) in (0..=u32::MAX).zip(params.iter().zip(&bindings)) {
            self.bind_declaration_parameter(&param.name, position, *binding);
        }
        // A diagnosed kind conflict still gets a placeholder for each reading
        // so checking the rest of the body can continue.
        for (index, kind) in extra {
            if let (Some(param), Ok(position), Some(binding)) = (
                params.get(index),
                u32::try_from(index),
                self.fresh_alias_parameter(kind),
            ) {
                self.bind_declaration_parameter(&param.name, position, binding);
            }
        }
        if self.adts.contains_key(name) || self.aliases.contains_key(name) {
            self.error(
                "E0316",
                format!("type `{name}` is already defined in this module"),
                span,
            );
        }
        match body {
            | TypeBody::Alias(ty) => {
                let outer = self.declaring_alias.replace(name.to_string());
                let converted = self.convert_type(ty);
                self.declaring_alias = outer;
                self.aliases.insert(
                    name.to_string(),
                    AliasDef {
                        ty: converted,
                        params: bindings,
                    },
                );
                if !private {
                    self.checked
                        .exports
                        .insert(name.to_string(), Export::Alias { params: params.len() });
                }
            },
            | TypeBody::Variants(ctors) => {
                let id = self.allocate_nominal_type(name, params);
                self.declare_variants(id, name, &bindings, ctors, span, private);
            },
            | TypeBody::Abstract => {
                if bindings
                    .iter()
                    .any(|kind| !matches!(kind, AliasParameter::Type))
                {
                    self.error_help(
                        "E0317",
                        format!("the abstract type `{name}` can only take type parameters"),
                        span,
                        "an opaque runtime type is parameterised by types",
                    );
                }
                let id = self.allocate_nominal_type(name, params);
                adt::set_param_kinds(id, vec![adt::ParamKind::Type; params.len()]);
                adt::set_ctors(id, Vec::new());
                if !private {
                    self.checked
                        .exports
                        .insert(name.to_string(), Export::Type { adt: id });
                }
            },
        }
        self.type_params = outer_types;
        self.dim_params = outer_dims;
        self.nat_params = outer_nats;
        self.row_params = outer_rows;
    }

    fn allocate_nominal_type(
        &mut self,
        name: &str,
        params: &[TypeParam],
    ) -> adt::AdtId {
        // Install the identity before converting any recursive fields.
        let id = adt::allocate_in(
            self.module_path.clone(),
            name,
            params.iter().map(|param| param.name.clone()).collect(),
        );
        self.adts.insert(name.to_string(), id);
        self.local_adts.insert(id);
        id
    }

    #[allow(
        clippy::single_call_fn,
        clippy::too_many_arguments,
        reason = "finalizes a nominal sum and publishes its constructors using source visibility and location"
    )]
    fn declare_variants(
        &mut self,
        id: adt::AdtId,
        name: &str,
        params: &[AliasParameter],
        ctors: &[Constructor],
        span: Span,
        private: bool,
    ) {
        let kinds = params
            .iter()
            .map(|parameter| {
                match *parameter {
                    | AliasParameter::Type => adt::ParamKind::Type,
                    | AliasParameter::Dim(id) => adt::ParamKind::Dim(id),
                    | AliasParameter::Nat(_) | AliasParameter::Row(_) => {
                        let kind = if matches!(parameter, AliasParameter::Nat(_)) {
                            "natural-number"
                        } else {
                            "row"
                        };
                        self.error_help(
                            "E0317",
                            format!("the variant type `{name}` cannot take a {kind} parameter"),
                            span,
                            "a variant type may take types and dimensions; use a record alias for a row or natural number",
                        );
                        adt::ParamKind::Type
                    },
                }
            })
            .collect();
        adt::set_param_kinds(id, kinds);
        let converted = ctors
            .iter()
            .map(|ctor| {
                let fields = ctor
                    .args
                    .iter()
                    .map(|field| {
                        let ty = self.convert_type(field);
                        self.check_positivity(id, &ty, true, field.span);
                        ty
                    })
                    .collect();
                Ctor {
                    name: ctor.name.clone(),
                    fields,
                }
            })
            .collect();
        adt::set_ctors(id, converted);
        if !private {
            self.checked
                .exports
                .insert(name.to_string(), Export::Type { adt: id });
        }
        let Some(registered) = adt::get(id) else {
            return;
        };
        for (index, (ctor, source)) in registered.ctors.iter().zip(ctors).enumerate() {
            let scheme = ctor_scheme(id, index);
            self.checked
                .constructors
                .insert(ctor.name.clone(), (id, index));
            self.constructor_bindings
                .insert(ctor.name.clone(), (id, index));
            self.globals.insert(ctor.name.clone(), scheme.clone());
            if private || source.private {
                continue;
            }
            let export = if ctor.name == name {
                Export::TypeCtor {
                    adt: id,
                    index,
                    scheme,
                }
            } else {
                Export::Ctor {
                    adt: id,
                    index,
                    scheme,
                }
            };
            self.checked.exports.insert(ctor.name.clone(), export);
        }
    }

    /// Rejects recursion inside function parameter types (§3.6.4).
    ///
    /// `type Bad = | Bad(Bad -> Int)` gives `E0312`, while
    /// `type Tree<a> = | Node(Tree<a>, a, Tree<a>)` is legal.
    fn check_positivity(
        &mut self,
        id: adt::AdtId,
        ty: &Type,
        positive: bool,
        span: Span,
    ) {
        match ty {
            | Type::Adt(other, args) => {
                if *other == id && !positive {
                    self.error_help(
                        "E0312",
                        "non-positive recursive type",
                        span,
                        "the type may only appear to the right of an arrow",
                    );
                }
                for arg in args {
                    self.check_positivity(id, arg, positive, span);
                }
            },
            | Type::Tensor { elem, .. } | Type::Io(elem) => {
                self.check_positivity(id, elem, positive, span);
            },
            | Type::Fn(params, ret) => {
                for param in params {
                    self.check_positivity(id, param, false, span);
                }
                self.check_positivity(id, ret, positive, span);
            },
            | Type::Record(row) => {
                let mut fields = row;
                while let Row::Extend { ty, tail, .. } = fields {
                    self.check_positivity(id, ty, positive, span);
                    fields = tail;
                }
            },
            | _ => {},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::CheckOptions;
    use super::super::check_module;
    use super::super::check_module_with_interfaces;
    use super::*;
    use aevia_syntax::ast::Module;
    use aevia_syntax::lex;
    use aevia_syntax::parse_module;

    fn parse(source: &str) -> Module {
        let tokens = lex(0, source).expect("test source lexes");
        let (module, diagnostics) = parse_module(0, source, &tokens);
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
        module
    }

    #[test]
    fn exported_aliases_preserve_type_and_dimension_parameters() {
        let library =
            parse("module Library\ntype Measured<a, d> = { value: a, measure: Float[d] }\n");
        let library = check_module(0, &library);
        assert!(
            !library.diagnostics.has_errors(),
            "{:?}",
            library.diagnostics.items
        );
        let interface = library.module.interface();
        let definition = interface.aliases.get("Measured").expect("exported alias");
        assert_eq!(
            definition.kinds,
            [AliasParamKind::Type, AliasParamKind::Dim]
        );
        let Type::Record(fields) = &definition.body else {
            panic!("the alias expands to a record");
        };
        assert_eq!(fields.field("value"), Some(&Type::Param(0)));

        let options = CheckOptions::default();
        let imports = [("Library".to_string(), library.module.exports)];
        let valid = parse("let x: Library.Measured<Int, L> = { value = 1, measure = 2.0 m }\n");
        let valid = check_module_with_interfaces(
            0,
            &valid,
            &imports,
            std::slice::from_ref(&interface),
            &options,
        );
        assert!(
            !valid.diagnostics.has_errors(),
            "{:?}",
            valid.diagnostics.items
        );
        let invalid =
            parse("let x: Library.Measured<Int, L> = { value = \"text\", measure = 2.0 m }\n");
        let invalid = check_module_with_interfaces(0, &invalid, &imports, &[interface], &options);
        assert!(
            invalid
                .diagnostics
                .items
                .iter()
                .any(|diagnostic| diagnostic.code == "E0302")
        );
    }

    #[test]
    fn omitted_alias_arguments_can_be_constrained_by_the_body() {
        let source = parse(
            "type Box<a> = { value: a }\nfn unwrap(box: Box): Int = box.value\nlet wrong = unwrap({ value = \"text\" })\n",
        );
        let checked = check_module(0, &source);
        assert!(
            checked
                .diagnostics
                .items
                .iter()
                .any(|diagnostic| diagnostic.code == "E0302"),
            "{:?}",
            checked.diagnostics.items
        );
    }

    #[test]
    fn an_imported_alias_with_a_mismatched_kind_count_reports_an_error() {
        let mut checker = Checker::new(0, CheckOptions::default());
        let definition = checker.imported_alias_def(&AliasExport {
            params: 0,
            body: Type::int(),
            kinds: vec![AliasParamKind::Dim],
        });
        assert_eq!(definition.ty, Type::Error);
        assert!(
            checker
                .diagnostics
                .items
                .iter()
                .any(|diagnostic| diagnostic.code == "E0317")
        );
    }

    #[test]
    fn unit_declaration_reports_scale_exponent_overflow() {
        let mut checker = Checker::new(0, CheckOptions::default());
        let scale = Scale {
            pi: i32::MAX,
            ..Scale::one()
        };
        let base = UnitExpr::Name("deg".to_string());
        assert!(
            checker
                .declared_unit_scale("huge", &scale, Some(&base), None, Span::new(0, 0, 0))
                .is_none()
        );
        assert!(
            checker
                .diagnostics
                .items
                .iter()
                .any(|diagnostic| diagnostic.code == "E0506")
        );
    }
}
