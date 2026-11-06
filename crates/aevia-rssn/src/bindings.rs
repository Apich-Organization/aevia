//! Manifest-driven binding inventory.  The catalog deliberately separates an
//! exact source signature from a dynamic route and from an unavailable bridge.
//! Consumers must not interpret a derived kernel signature as an exact API.

use crate::registry::OpEntry;
use crate::registry::Registry;
use crate::registry::Source;
use rssn::kernel::signature::DimExpr;
use rssn::kernel::signature::Exp;
use rssn::kernel::signature::NatExpr;
use rssn::kernel::signature::TypeExpr;
use std::collections::BTreeSet;

/// How much of an overload can be expressed and called from Aevia.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Coverage {
    /// Exact manifest signature, representable and supported by the bridge.
    Typed,
    /// A call by name remains available, but no exact source signature is promised.
    Dynamic(Vec<String>),
    /// Even the dynamic route cannot carry the required value or backend.
    Unavailable(Vec<String>),
}

/// One overload (or one unsigned operator) in deterministic manifest order.
#[derive(Clone, Debug)]
pub struct Binding<'a> {
    /// Canonical manifest operator and its metadata.
    pub op: &'a OpEntry,
    /// Overload index; `None` denotes an unsigned registration.
    pub overload: Option<usize>,
    /// Unique source identifier for this overload in the complete facade.
    pub name: String,
    /// Coverage and explanations, ordered deterministically.
    pub coverage: Coverage,
}

/// Build a complete, deterministic catalog directly from the live manifest.
/// Every operator contributes at least one row, including unsigned operators.
#[must_use]
pub fn catalog(registry: &Registry) -> Vec<Binding<'_>> {
    let mut used = BTreeSet::new();
    let mut result = Vec::new();
    for op in registry.ops() {
        let count = op.raw.len().max(1);
        for index in 0..count {
            let overload = (index < op.raw.len()).then_some(index);
            let base = crate::codegen::binding_base_name(
                &op.name,
                index,
                overload
                    .is_some_and(|i| op.raw[i].params.is_empty() && op.raw[i].variadic.is_none()),
            );
            let mut name = base.clone();
            let mut suffix = 2;
            while !used.insert(name.clone()) {
                name = format!("{base}_{suffix}");
                suffix += 1;
            }
            result.push(Binding {
                op,
                overload,
                name,
                coverage: classify(registry, op, overload),
            });
        }
    }
    result
}

fn classify(
    registry: &Registry,
    op: &OpEntry,
    index: Option<usize>,
) -> Coverage {
    let mut dynamic = BTreeSet::new();
    let mut unavailable = BTreeSet::new();
    if !registry.capabilities.dynamic_call {
        unavailable.insert("core.dynamic_call is unavailable".to_string());
    }
    if !op.backend_support.interpreter {
        unavailable.insert("kernel interpreter backend is unavailable".to_string());
    }
    if let Some(capability) = registry.capabilities.missing_for(op) {
        unavailable.insert(format!("missing capability {capability}"));
    }
    if op.higher_order && !registry.capabilities.has("core.callback") {
        unavailable.insert(
            "missing capability core.callback (Aevia closures cannot cross the kernel ABI)"
                .to_string(),
        );
    }
    if op.higher_order && registry.capabilities.has("core.callback") {
        dynamic.insert("higher-order calls use the checked value-list callback ABI".to_string());
    }
    if !op.source.is_exact() {
        dynamic.insert(match op.source {
            | Source::Derived => "signature is derived, not exact".to_string(),
            | Source::None => {
                op.dynamic_reason
                    .clone()
                    .unwrap_or_else(|| "kernel supplies no signature".to_string())
            },
            | _ => unreachable!(),
        });
    }
    if let Some(index) = index {
        let signature = &op.raw[index];
        let mirrored = &op.signatures[index];
        let mut input_dims = BTreeSet::new();
        for param in &mirrored.params {
            collect_dim_vars(param, &mut input_dims);
        }
        if let Some((_, _, elem)) = &mirrored.variadic {
            collect_dim_vars(elem, &mut input_dims);
        }
        let mut result_dims = BTreeSet::new();
        collect_dim_vars(&mirrored.ret, &mut result_dims);
        if !result_dims.is_subset(&input_dims) {
            dynamic.insert("result dimension is not determined by any input".to_string());
        }
        if op.key(index).is_none() {
            dynamic.insert("manifest has no key for overload".to_string());
        }
        if signature.variadic.is_some() {
            dynamic.insert("variadic arguments use the dynamic value-list ABI".to_string());
        }
        for ty in signature
            .params
            .iter()
            .chain(std::iter::once(&signature.ret))
            .chain(signature.variadic.iter().map(|v| &v.elem))
        {
            inspect_type(ty, &mut dynamic, &mut unavailable);
        }
        if registry.capabilities.has("core.callback") {
            if op.higher_order && !mirrored.params.iter().any(|ty| matches!(ty, crate::registry::SigType::Func(..))) {
                unavailable.insert("higher-order signature has no supported trailing callback".to_string());
            }
            unavailable.remove(
                "missing capability core.callback (function values cannot cross the kernel ABI)",
            );
            if mirrored.params.iter().any(|ty| {
                match ty {
                    | crate::registry::SigType::Func(params, ret) => {
                        params.iter().any(contains_function) || contains_function(ret)
                    },
                    | _ => contains_function(ty),
                }
            }) || contains_function(&mirrored.ret)
                || mirrored
                    .variadic
                    .as_ref()
                    .is_some_and(|(_, _, ty)| contains_function(ty))
            {
                unavailable.insert(
                    "nested or returned function values cannot cross the kernel ABI".to_string(),
                );
            }
            if mirrored
                .params
                .iter()
                .any(|ty| matches!(ty, crate::registry::SigType::Func(..)))
            {
                if !op.source.is_exact() || op.signatures.len() != 1 || mirrored.variadic.is_some()
                {
                    unavailable.insert(
                        "callback bridge requires one exact non-variadic overload".to_string(),
                    );
                }
                if !callback_parameters_supported(&mirrored.params)
                    || !callback_raw_parameters_supported(&signature.params) {
                    unavailable.insert(
                        "callback bridge requires supported recursive trailing callback schemas"
                            .to_string(),
                    );
                }
                if !op.higher_order {
                    unavailable.insert(
                        "function parameter requires a higher-order kernel entry".to_string(),
                    );
                }
                dynamic.insert("callbacks use the checked value-list callback ABI; typed generic wrappers require reification".to_string());
            }
        }
    } else {
        dynamic.insert("kernel supplies no overload".to_string());
        if op.higher_order {
            unavailable.insert("higher-order callback schema is unavailable".to_string());
        }
    }
    if !unavailable.is_empty() {
        Coverage::Unavailable(unavailable.into_iter().chain(dynamic).collect())
    } else if !dynamic.is_empty() {
        Coverage::Dynamic(dynamic.into_iter().collect())
    } else {
        Coverage::Typed
    }
}

// Callback declarations describe an operator's accepted family; runtime
// callbacks still carry a separately authenticated, fully concrete schema.
fn callback_parameters_supported(params: &[crate::registry::SigType]) -> bool {
    use crate::registry::SigType as S;
    fn schema(ty: &S) -> bool {
        match ty {
            S::Int | S::Real | S::Float | S::Complex | S::Bool | S::Str => true,
            S::Dim(inner, _) => matches!(inner.as_ref(), S::Int | S::Real | S::Float | S::Complex),
            S::Tensor { rank: Some(rank), shape, elem } => shape.as_ref().is_none_or(|shape| shape.len() == *rank) && schema(elem),
            S::Record(fields) => {
                let mut names = BTreeSet::new();
                fields.iter().all(|(name, ty)| names.insert(name) && schema(ty))
            },
            _ => false,
        }
    }
    let mut seen_callback = false;
    for param in params {
        match param {
            S::Func(args, ret) => { seen_callback = true; if !args.iter().all(schema) || !schema(ret) { return false; } },
            _ if seen_callback => return false,
            _ => {},
        }
    }
    seen_callback
}

// Preserve facts that the mirrored shape cannot express, especially Any,
// malformed extents and unknown ranks; only named operator variables are
// existential. This check runs before choosing the generic dynamic facade.
fn callback_raw_parameters_supported(params: &[TypeExpr]) -> bool {
    fn dim(d: &DimExpr) -> bool {
        match d {
            DimExpr::Dimless | DimExpr::Base(_) | DimExpr::Var(_) => true,
            DimExpr::Mul(parts) => parts.iter().all(dim),
            DimExpr::Inverse(inner) => dim(inner),
            DimExpr::Pow(inner, Exp::Rat(power)) => power.denom != 0 && dim(inner),
            _ => false,
        }
    }
    fn schema(ty: &TypeExpr) -> bool {
        match ty {
            TypeExpr::Int | TypeExpr::Real | TypeExpr::Float | TypeExpr::Complex | TypeExpr::Bool | TypeExpr::String => true,
            TypeExpr::Dim(inner, d) => matches!(inner.as_ref(), TypeExpr::Int | TypeExpr::Real | TypeExpr::Float | TypeExpr::Complex) && dim(d),
            TypeExpr::Tensor { rank: NatExpr::Lit(rank), shape, elem } => {
                usize::try_from(*rank).ok().is_some_and(|rank| shape.as_ref().is_none_or(|shape| shape.len() == rank && shape.iter().all(|extent| match extent { NatExpr::Lit(value) => usize::try_from(*value).is_ok(), NatExpr::Var(_) => true, _ => false }))) && schema(elem)
            },
            TypeExpr::Record(fields) => { let mut names = BTreeSet::new(); fields.iter().all(|(name, ty)| names.insert(name) && schema(ty)) },
            _ => false,
        }
    }
    let mut seen_callback = false;
    for param in params {
        match param {
            TypeExpr::Function(args, ret) => { seen_callback = true; if !args.iter().all(schema) || !schema(ret) { return false; } },
            _ if seen_callback => return false,
            _ => {},
        }
    }
    seen_callback
}

fn contains_function(ty: &crate::registry::SigType) -> bool {
    use crate::registry::SigType;
    match ty {
        | SigType::Func(..) => true,
        | SigType::Dim(inner, _)
        | SigType::Tensor { elem: inner, .. }
        | SigType::Sparse { elem: inner, .. } => contains_function(inner),
        | SigType::Record(fields) => fields.iter().any(|(_, ty)| contains_function(ty)),
        | SigType::Variant(cases) => {
            cases
                .iter()
                .any(|(_, fields)| fields.iter().any(contains_function))
        },
        | _ => false,
    }
}

fn collect_dim_vars(
    ty: &crate::registry::SigType,
    vars: &mut BTreeSet<u32>,
) {
    use crate::registry::SigType as S;
    match ty {
        | S::Dim(inner, dim) => {
            vars.extend(dim.vars());
            collect_dim_vars(inner, vars);
        },
        | S::Unit(dim) | S::Point(dim) => vars.extend(dim.vars()),
        | S::Tensor { elem, .. } | S::Sparse { elem, .. } => collect_dim_vars(elem, vars),
        | S::Record(fields) => {
            for (_, field) in fields {
                collect_dim_vars(field, vars);
            }
        },
        | S::Variant(cases) => {
            for (_, fields) in cases {
                for field in fields {
                    collect_dim_vars(field, vars);
                }
            }
        },
        | S::Func(args, ret) => {
            for arg in args {
                collect_dim_vars(arg, vars);
            }
            collect_dim_vars(ret, vars);
        },
        | _ => {},
    }
}

fn inspect_dim(
    dim: &DimExpr,
    reasons: &mut BTreeSet<String>,
) {
    match dim {
        | DimExpr::NonMetric | DimExpr::Container => {
            reasons
                .insert("non-scalar dimension cannot be represented in a source signature".into());
        },
        | DimExpr::Pow(_, Exp::Symbol(_)) => {
            reasons.insert(
                "symbolic dimension exponent cannot be represented in a source signature".into(),
            );
        },
        | DimExpr::Pow(base, _) | DimExpr::Inverse(base) => inspect_dim(base, reasons),
        | DimExpr::Mul(parts) => {
            for part in parts {
                inspect_dim(part, reasons);
            }
        },
        | _ => {},
    }
}

fn inspect_type(
    ty: &TypeExpr,
    dynamic: &mut BTreeSet<String>,
    unavailable: &mut BTreeSet<String>,
) {
    match ty {
        | TypeExpr::Unit(dim) | TypeExpr::Point(dim) => inspect_dim(dim, dynamic),
        | TypeExpr::Dim(inner, dim) => {
            inspect_dim(dim, dynamic);
            inspect_type(inner, dynamic, unavailable);
        },
        | TypeExpr::Tensor { rank, shape, elem } => {
            if !matches!(rank, NatExpr::Lit(_)) {
                dynamic.insert("tensor rank is not fixed".into());
            }
            if shape.as_ref().is_some_and(|extents| !matches!(rank, NatExpr::Lit(rank) if usize::try_from(*rank).ok() == Some(extents.len())) || extents.iter().any(|extent| !matches!(extent, NatExpr::Lit(_)))) {
                dynamic.insert("tensor extents are not fully concrete in the source facade".into());
            }
            inspect_type(elem, dynamic, unavailable);
        },
        | TypeExpr::Sparse { elem, .. } => {
            dynamic.insert("sparse representation is not preserved by the source facade".into());
            inspect_type(elem, dynamic, unavailable);
        },
        | TypeExpr::Record(fields) => {
            for (_, ty) in fields {
                inspect_type(ty, dynamic, unavailable);
            }
        },
        | TypeExpr::Variant(variants) => {
            dynamic.insert("variant type has no source facade spelling".into());
            for (_, types) in variants {
                for ty in types {
                    inspect_type(ty, dynamic, unavailable);
                }
            }
        },
        | TypeExpr::Function(params, ret) => {
            unavailable.insert(
                "missing capability core.callback (function values cannot cross the kernel ABI)"
                    .into(),
            );
            for ty in params {
                inspect_type(ty, dynamic, unavailable);
            }
            inspect_type(ret, dynamic, unavailable);
        },
        | TypeExpr::Table(_) => {
            unavailable.insert("table values cannot cross the kernel ABI".into());
        },
        | TypeExpr::Device => {
            unavailable.insert("device values cannot cross the kernel ABI".into());
        },
        | TypeExpr::Opaque(_) => {
            dynamic.insert("host opaque type has no source facade spelling".into());
        },
        | TypeExpr::Var(_) => {
            dynamic.insert("type parameter has no source facade spelling".into());
        },
        | TypeExpr::Never => {
            dynamic.insert("never type has no source facade spelling".into());
        },
        | TypeExpr::Dyn(_) => {
            dynamic.insert("kernel signature is dynamically represented".into());
        },
        | _ => {},
    }
}

#[cfg(test)]
mod callback_strategy_tests {
    use super::callback_parameters_supported;
    use crate::registry::SigType as S;
    #[test]
    fn callback_facades_match_recursive_transport_and_trailing_order() {
        let scalar = S::Func(vec![S::Int, S::Bool], Box::new(S::Real));
        assert!(callback_parameters_supported(&[S::Float, scalar.clone()]));
        assert!(!callback_parameters_supported(&[scalar, S::Float]));
        let tensor = S::Tensor { rank: Some(1), shape: Some(vec![2]), elem: Box::new(S::Float) };
        let record = S::Record(vec![("y".into(), tensor.clone()), ("ok".into(), S::Bool)]);
        assert!(callback_parameters_supported(&[S::Func(vec![record.clone()], Box::new(tensor))]));
        assert!(!callback_parameters_supported(&[S::Func(vec![S::Var("T".into())], Box::new(S::Float))]));
        assert!(!callback_parameters_supported(&[S::Func(vec![S::Tensor { rank: None, shape: None, elem: Box::new(S::Float) }], Box::new(S::Float))]));
        assert!(!callback_parameters_supported(&[S::Func(vec![S::Record(vec![("x".into(), S::Int), ("x".into(), S::Int)])], Box::new(S::Float))]));
        assert!(!callback_parameters_supported(&[S::Func(vec![S::Func(vec![],Box::new(S::Float))], Box::new(record))]));
        assert!(!callback_parameters_supported(&[S::Float]));
    }
    #[test]
    fn raw_callback_schema_rejects_unknown_and_malformed_promises() {
        use super::callback_raw_parameters_supported as supports;
        use rssn::kernel::signature::{DimExpr, NatExpr, TypeExpr as T};
        let callback = |arg| T::Function(vec![arg],Box::new(T::Float));
        let generic = T::Tensor { rank: NatExpr::Lit(1), shape: Some(vec![NatExpr::Var("N".into())]), elem: Box::new(T::Float) };
        assert!(supports(&[callback(generic)]));
        assert!(!supports(&[callback(T::Var("T".into()))]));
        assert!(!supports(&[callback(T::Float.with_dim(DimExpr::Any))]));
        assert!(!supports(&[callback(T::Tensor { rank: NatExpr::Lit(1), shape: Some(vec![NatExpr::Lit(1),NatExpr::Lit(2)]), elem: Box::new(T::Float) })]));
        assert!(!supports(&[callback(T::Float), T::Var("T".into())]));
    }
    #[test]
    fn generic_dynamic_fallback_cannot_bypass_callback_guard() {
        use super::{classify, Coverage};
        use rssn::kernel::signature::TypeExpr as T;
        let registry = crate::registry::registry();
        let mut op = registry.op("num_integrate").expect("integration callback").clone();
        let callback = T::Function(vec![T::Var("T".into())], Box::new(T::Float));
        op.raw[0].params = vec![callback, T::Var("T".into())];
        op.signatures[0].params = vec![S::Func(vec![S::Var("T".into())],Box::new(S::Float)), S::Var("T".into())];
        assert!(matches!(classify(registry, &op, Some(0)), Coverage::Unavailable(_)));
        op.raw[0].params = vec![T::Var("T".into())];
        op.signatures[0].params = vec![S::Var("T".into())];
        assert!(matches!(classify(registry, &op, Some(0)), Coverage::Unavailable(_)));
    }

}
