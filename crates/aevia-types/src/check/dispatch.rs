//! Call dispatch through RSSN signatures and Aevia type-class dictionaries.

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

use super::BuiltinDispatch;
use super::Checker;
use super::DictTarget;
use super::DictionaryUse;
use super::RssnCall;
use super::is_structural_show;
use crate::adt::Adt;
use crate::adt::{
    self,
};
use crate::builtins::dynamic_value_type;
use crate::builtins::op_error_type;
use crate::builtins::{
    self,
};
use crate::class::Origin;
use crate::infer::Infer;
use crate::ty::Rep;
use crate::ty::Scheme;
use crate::ty::Shape;
use crate::ty::Type;
use aevia_dim::Dim;
use aevia_syntax::Diagnostic;
use aevia_syntax::Span;
use aevia_syntax::ast::Expr;
use aevia_syntax::ast::ExprKind;

/// The flat key a qualified field access names, when the base is a path or an
/// import alias.
#[allow(
    clippy::single_call_fn,
    reason = "qualified names and record field lookup have distinct rules"
)]
pub(super) fn qualified_name(
    base: &Expr,
    field: &str,
) -> Option<String> {
    match &base.kind {
        | ExprKind::QName(path) => Some(format!("{}.{field}", path.join("."))),
        | ExprKind::Var(name) if name.chars().next().is_some_and(char::is_uppercase) => {
            Some(format!("{name}.{field}"))
        },
        | _ => None,
    }
}

/// The member of `Rssn` named by a direct access.
pub(super) fn rssn_member_name(expr: &Expr) -> Option<&str> {
    match &expr.kind {
        | ExprKind::Field { base, name } if is_rssn_root(base) => Some(name),
        | ExprKind::QName(path) => {
            match path.as_slice() {
                | [namespace, name] if namespace == "Rssn" => Some(name),
                | _ => None,
            }
        },
        | _ => None,
    }
}

/// The operator named by a direct access to `Rssn`.
#[allow(
    clippy::single_call_fn,
    reason = "operator lookup excludes the fixed members of the dynamic ABI"
)]
pub(super) fn rssn_op_name(expr: &Expr) -> Option<&str> {
    rssn_member_name(expr).filter(|name| rssn_fixed_member(name).is_none())
}

/// Whether an expression names the `Rssn` module itself.
pub(super) fn is_rssn_root(expr: &Expr) -> bool {
    match &expr.kind {
        | ExprKind::Var(name) => name == "Rssn",
        | ExprKind::QName(path) => matches!(path.as_slice(), [name] if name == "Rssn"),
        | _ => false,
    }
}

/// `Tensor<1, Value>`: the argument list of a dynamic call.
pub(super) fn value_list_type() -> Type {
    Type::tensor(Shape::Rank(1), dynamic_value_type())
}

/// An opaque host type of the dynamic ABI (`TypeRep`, `Plugin`, …).
///
/// It is interned by name, so this and `builtin_type`'s `OPAQUE` table resolve
/// to the same `AdtId` and a source annotation such as `fn f(p: Plugin)` names
/// the type the checker's signatures use.
pub(super) fn opaque_type(name: &str) -> Type {
    let id = adt::intern(Adt {
        name: name.to_string(),
        params: Vec::new(),
        ctors: Vec::new(),
    });
    Type::Adt(id, Vec::new())
}

fn dynamic_call_result() -> Type {
    Type::result(dynamic_value_type(), op_error_type())
}

/// Names the fixed ABI members and defers construction until a member is used.
fn rssn_fixed_member(name: &str) -> Option<fn() -> Type> {
    let factory: fn() -> Type = match name {
        | "Version" | "ManifestHash" => || Type::Str,
        | "Capabilities" | "Units" | "Constants" | "ValueKinds" => {
            || Type::tensor(Shape::Rank(1), Type::Str)
        },
        | "call" | "dynamic" => {
            || Type::func(vec![Type::Str, value_list_type()], dynamic_call_result())
        },
        | "call_key" => {
            || {
                Type::func(
                    vec![Type::Str, Type::Str, value_list_type()],
                    dynamic_call_result(),
                )
            }
        },
        // The result from `invoke` can be known only at an individual call site.
        | "invoke" => {
            || {
                Type::func(
                    vec![Type::Str, Type::Str, value_list_type()],
                    dynamic_value_type(),
                )
            }
        },
        | "resolve_overload" => {
            || {
                Type::func(
                    vec![Type::Str, value_list_type()],
                    Type::result(Type::Str, op_error_type()),
                )
            }
        },
        | "overloads" => || Type::func(vec![Type::Str], Type::tensor(Shape::Rank(1), Type::Str)),
        | "typed" => {
            || {
                Type::func(
                    vec![Type::Str, value_list_type(), opaque_type("TypeRep")],
                    dynamic_call_result(),
                )
            }
        },
        | "call_ho" => {
            || {
                Type::func(
                    vec![Type::Str, value_list_type(), callback_list_type()],
                    dynamic_call_result(),
                )
            }
        },
        | "call_ho_key" => {
            || {
                Type::func(
                    vec![
                        Type::Str,
                        Type::Str,
                        value_list_type(),
                        callback_list_type(),
                    ],
                    dynamic_call_result(),
                )
            }
        },
        | "register" | "register_plugin" => {
            || {
                Type::func(
                    vec![opaque_type("Plugin")],
                    Type::result(opaque_type("PluginId"), opaque_type("PluginError")),
                )
            }
        },
        | _ => return None,
    };
    Some(factory)
}

fn callback_list_type() -> Type {
    Type::tensor(Shape::Rank(1), opaque_type("Callback"))
}

fn rssn_rank(rank: Option<usize>) -> Option<u32> {
    match rank {
        | Some(rank) => u32::try_from(rank).ok(),
        | None => Some(1),
    }
}

/// Maps a manifest signature type to the checker's type.
fn rssn_type(
    signature: &aevia_rssn::SigType,
    infer: &mut Infer,
) -> Type {
    use aevia_rssn::SigType as S;
    match signature {
        | S::Int => Type::int(),
        | S::Real => Type::real(Dim::one()),
        | S::Float => Type::float(Dim::one()),
        | S::Complex => Type::complex(Dim::one()),
        | S::Bool => Type::Bool,
        | S::Str => Type::Str,
        | S::Unit(dim) => Type::UnitOf(dim.clone()),
        | S::Point(dim) => Type::Point(dim.clone()),
        | S::Dim(element, dim) => {
            match **element {
                | S::Int => Type::Scalar(Rep::Int, dim.clone()),
                | S::Float => Type::Scalar(Rep::Float, dim.clone()),
                | S::Complex => Type::Scalar(Rep::Complex, dim.clone()),
                | _ => Type::Scalar(Rep::Real, dim.clone()),
            }
        },
        | S::Tensor { rank, shape, elem } => {
            let shape = if let Some(extents) = shape {
                if u32::try_from(extents.len()).is_err()
                    || rank.is_some_and(|rank| rank != extents.len())
                {
                    return Type::Error;
                }
                Shape::refined(extents.iter().copied().map(Some))
            } else {
                let Some(rank) = rssn_rank(*rank) else {
                    return Type::Error;
                };
                Shape::Rank(rank)
            };
            Type::tensor(shape, rssn_type(elem, infer))
        },
        // The layout of a kernel sparse result is known only at run time.
        | S::Sparse { rank, elem } => {
            let Some(rank) = rssn_rank(*rank) else {
                return Type::Error;
            };
            let format = infer.fresh_type();
            builtins::sparse_type(
                format,
                Type::tensor(Shape::Rank(rank), rssn_type(elem, infer)),
            )
        },
        | S::Record(fields) => {
            Type::record(
                fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), rssn_type(ty, infer))),
            )
        },
        | S::Func(params, ret) => {
            Type::Fn(
                params.iter().map(|param| rssn_type(param, infer)).collect(),
                Box::new(rssn_type(ret, infer)),
            )
        },
        | S::Variant(cases) => {
            aevia_rssn::registry::option_payload(cases).map_or_else(dynamic_value_type, |payload| {
                Type::option(rssn_type(payload, infer))
            })
        },
        | S::Opaque(name) if name == aevia_rssn::registry::NATIVE_AD_VAR_KIND => {
            adt::native_ad_var_type()
        },
        | S::Table | S::Device | S::Opaque(_) | S::Var(_) => dynamic_value_type(),
        | S::Symbol | S::Dyn | S::Never => Type::Dyn(Rep::Real),
    }
}

impl Checker {
    /// Types a reference to an RSSN member outside a call.
    ///
    /// All overloads must have the same Aevia type for a bare name to
    /// represent a callable.
    pub(super) fn rssn_member(
        &mut self,
        name: &str,
        span: Span,
    ) -> Type {
        if let Some(factory) = rssn_fixed_member(name) {
            return factory();
        }
        let Some(entry) = aevia_rssn::registry().op(name) else {
            self.error_help(
                "E0301",
                format!("rssn has no operator `{name}`"),
                span,
                "run `aevia rssn --audit` for the operators this build provides",
            );
            return Type::Error;
        };
        let Some((signature, rest)) = entry.signatures.split_first() else {
            return Type::func(vec![value_list_type()], dynamic_value_type());
        };
        let first = self.rssn_signature_type(signature);
        if rest
            .iter()
            .all(|signature| self.rssn_signature_type(signature) == first)
        {
            return first;
        }
        self.error_help(
            "E0303",
            format!("`Rssn.{name}` is overloaded and cannot be used as a value"),
            span,
            "call it, or bind the result of a call to a name",
        );
        Type::Error
    }

    /// Types a call to `Rssn.<op>` by resolving the overload at the call site.
    pub(super) fn rssn_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let Some(entry) = aevia_rssn::registry().op(name) else {
            for arg in args {
                self.synth(arg);
            }
            self.error_help(
                "E0301",
                format!("rssn has no operator `{name}`"),
                span,
                "run `aevia rssn --audit` for the operators this build provides",
            );
            return Type::Error;
        };
        let mut arg_types = Vec::with_capacity(args.len());
        let mut classified = Vec::with_capacity(args.len());
        for arg in args {
            let ty = self.synth(arg);
            let resolved = self.infer.resolve(&ty);
            classified.push(self.classify_arg(&resolved));
            arg_types.push(resolved);
        }
        match crate::rssn_resolve::resolve(entry, &classified) {
            | crate::rssn_resolve::Resolution::Unique { index, .. } => {
                self.rssn_selected_call(name, entry, index, &arg_types, span)
            },
            | crate::rssn_resolve::Resolution::Ambiguous { candidates } => {
                let rendered: Vec<String> = candidates
                    .iter()
                    .filter_map(|index| entry.signatures.get(*index))
                    .map(aevia_rssn::Sig::render)
                    .collect();
                let help = if args.is_empty() {
                    "use `Rssn.dynamic` with an empty argument list to request the kernel result"
                } else {
                    "annotate an argument so one overload wins"
                };
                self.diagnostics.push(
                    Diagnostic::error(
                        "E1002",
                        format!("`Rssn.{name}` is ambiguous for these arguments"),
                        span,
                    )
                    .with_primary_label("several overloads match equally well")
                    .with_note(format!("candidates: {}", rendered.join("; ")))
                    .with_help(help),
                );
                Type::Error
            },
            | crate::rssn_resolve::Resolution::None => {
                let rendered: Vec<String> = entry
                    .signatures
                    .iter()
                    .map(aevia_rssn::Sig::render)
                    .collect();
                let diagnostic = Diagnostic::error(
                    "E1003",
                    format!("no overload of `Rssn.{name}` accepts these arguments"),
                    span,
                )
                .with_primary_label("no candidate accepts this argument list")
                .with_note(format!("overloads: {}", rendered.join("; ")));
                let argument = crate::rssn_resolve::symbol_argument_mismatch(entry, &classified)
                    .and_then(|index| args.get(index));
                self.diagnostics.push(if let Some(argument) = argument {
                    diagnostic
                        .with_label(argument.span, "the kernel requires a free symbol here")
                        .with_help("create a free symbol with `symbol`, pass it through `to_value`, and use `Rssn.typed` with a result type witness")
                } else {
                    diagnostic.with_help("use `Rssn.dynamic` for a signature-free call")
                });
                Type::Error
            },
            | crate::rssn_resolve::Resolution::Unknown => {
                self.error("E0301", format!("rssn has no operator `{name}`"), span);
                Type::Error
            },
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "keeps manifest signature instantiation and publication of the chosen key together"
    )]
    fn rssn_selected_call(
        &mut self,
        name: &str,
        entry: &aevia_rssn::OpEntry,
        index: usize,
        arg_types: &[Type],
        span: Span,
    ) -> Type {
        let Some(signature) = entry.signatures.get(index) else {
            return Type::Error;
        };
        let result = if entry.source.is_exact() {
            if entry
                .raw
                .get(index)
                .is_some_and(aevia_rssn::registry::signature_has_unrepresentable_dimensions)
            {
                self.error_help(
                    "E1001",
                    format!("`Rssn.{name}` has dimensions this checker cannot represent"),
                    span,
                    "use `Rssn.dynamic` and inspect the kernel result",
                );
                return Type::Error;
            }
            match crate::rssn_resolve::instantiate_dimensions(signature, arg_types, &mut self.infer)
            {
                | Ok(result) => result.unwrap_or(Type::Dyn(Rep::Real)),
                | Err(error) => {
                    self.report_unify(error, span);
                    return Type::Error;
                },
            }
        } else {
            // A derived signature selects an overload. Its result dimensions
            // come from the evaluated kernel value.
            Type::Dyn(Rep::Real)
        };
        if let Some(key) = entry.key(index) {
            self.checked.rssn_calls.push(RssnCall {
                span,
                name: name.to_string(),
                key: key.clone(),
                overload: index,
            });
        }
        result
    }

    /// Classifies an argument for overload resolution.
    ///
    /// A literal that has not been committed yet is classified by the
    /// representation it defaults to: `sin(0.5)` must pick the numeric
    /// overload, not tie with the symbolic one.
    fn classify_arg(
        &self,
        ty: &Type,
    ) -> aevia_rssn::SigType {
        match ty {
            | Type::Var(_) => {
                match self.infer.literal_kind(ty) {
                    | Some(kind) => {
                        crate::rssn_resolve::classify(&Type::Scalar(
                            Rep::default_for_literal(kind),
                            Dim::one(),
                        ))
                    },
                    | None => aevia_rssn::SigType::Dyn,
                }
            },
            | Type::Scalar(Rep::Var(id), dim) => {
                match self.infer.rep_literal_kind(Rep::Var(*id)) {
                    | Some(kind) => {
                        crate::rssn_resolve::classify(&Type::Scalar(
                            Rep::default_for_literal(kind),
                            dim.clone(),
                        ))
                    },
                    | None => aevia_rssn::SigType::Dyn,
                }
            },
            | other => crate::rssn_resolve::classify(other),
        }
    }

    /// The Aevia type of one manifest signature.
    fn rssn_signature_type(
        &mut self,
        signature: &aevia_rssn::Sig,
    ) -> Type {
        let mut params: Vec<Type> = Vec::with_capacity(
            signature
                .params
                .len()
                .saturating_add(usize::from(signature.variadic.is_some())),
        );
        for param in &signature.params {
            params.push(rssn_type(param, &mut self.infer));
        }
        if signature.variadic.is_some() {
            params.push(Type::tensor(Shape::Rank(1), dynamic_value_type()));
        }
        Type::Fn(params, Box::new(rssn_type(&signature.ret, &mut self.infer)))
    }

    /// Types a call to a class method and solves its constraints.
    pub(super) fn class_method_call(
        &mut self,
        name: &str,
        scheme: &Scheme,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let (ty, constraints) = self.infer.instantiate_scheme(scheme);
        let ret = self.call_with_type(ty, args, span);
        for constraint in constraints {
            self.solve_constraint(&constraint, span);
            if let Some(arg) = constraint.args.first() {
                let head = self.infer.resolve(arg);
                let target = self.dict_target(&constraint.class, &head);
                self.checked.dictionaries.push(DictionaryUse {
                    span,
                    class: constraint.class,
                    head,
                    method: name.to_string(),
                    target,
                });
            }
        }
        ret
    }

    /// Selects an instance, a built-in operation, or the registered instance
    /// candidates for a class-method call with an unresolved head.
    pub(super) fn dict_target(
        &self,
        class: &str,
        head: &Type,
    ) -> DictTarget {
        if let Some(instance) = self.classes.resolve_instance(class, head) {
            if instance.origin == Origin::Derived {
                return DictTarget::Builtin(BuiltinDispatch::Derived);
            }
            return DictTarget::Instance(instance.key.clone());
        }
        if is_structural_show(class, head) {
            return DictTarget::Builtin(BuiltinDispatch::ShowStructural);
        }
        let instances = self.classes.instances_of(class);
        let candidates = instances
            .iter()
            .map(|instance| instance.key.clone())
            .collect();
        let derived = instances
            .iter()
            .filter(|instance| instance.origin == Origin::Derived)
            .map(|instance| (instance.key.clone(), instance.head.clone()))
            .collect();
        DictTarget::Witness { candidates, derived }
    }
}

#[cfg(test)]
mod tests {
    use super::rssn_type;
    use crate::infer::Infer;
    use crate::ty::Type;
    use aevia_rssn::SigType;

    #[test]
    fn oversized_signature_ranks_are_rejected() {
        let Ok(rank) = usize::try_from(u64::from(u32::MAX) + 1) else {
            return;
        };
        let mut infer = Infer::new();
        for signature in [
            SigType::Tensor {
                rank: Some(rank),
                shape: None,
                elem: Box::new(SigType::Float),
            },
            SigType::Sparse {
                rank: Some(rank),
                elem: Box::new(SigType::Float),
            },
        ] {
            assert_eq!(rssn_type(&signature, &mut infer), Type::Error);
        }
    }
}
