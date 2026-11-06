//! Dimension evidence at the kernel ABI boundary.
//!
//! Unknown is distinct from dimensionless. A value-native kernel must return
//! explicit evidence for a dimensional result; graph-backed calls use the
//! host's separately checked dimension inference instead.
use crate::KernelError;
use crate::SigType;
use aevia_dim::Dim;
use aevia_dim::Scale;
use rssn::kernel::signature::TypeExpr;
use rssn::kernel::value::QuantityValue;
use rssn::kernel::value::Value;

/// A recursive dimension witness. Missing evidence is never dimensionless.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DimensionWitness {
    /// No dimensional statement is available.
    Unknown,
    /// A known scalar dimension, including dimensionless `1`.
    Scalar(Dim),
    /// A tensor's uniform element dimension and any nested element witnesses.
    Tensor {
        dimension: Option<Dim>,
        element: Option<Box<DimensionWitness>>,
        element_repr: Option<SigType>,
        elements: Vec<DimensionWitness>,
    },
    /// Named field witnesses of a structured result.
    Record(Vec<(String, DimensionWitness)>),
}
fn concrete(dim: &rssn::kernel::signature::DimExpr) -> Option<Dim> {
    let converted = crate::registry::convert_dim(dim);
    converted.is_concrete().then_some(converted)
}
fn representation(ty: &TypeExpr) -> Option<SigType> {
    match ty {
        | TypeExpr::Dim(inner, _) => representation(inner),
        | TypeExpr::Int => Some(SigType::Int),
        | TypeExpr::Real => Some(SigType::Real),
        | TypeExpr::Float => Some(SigType::Float),
        | TypeExpr::Complex => Some(SigType::Complex),
        | TypeExpr::Bool => Some(SigType::Bool),
        | TypeExpr::String => Some(SigType::Str),
        | TypeExpr::Tensor {
            rank: rssn::kernel::signature::NatExpr::Lit(rank),
            shape,
            elem,
        } => {
            Some(SigType::Tensor {
                rank: Some(usize::try_from(*rank).ok()?),
                shape: shape
                    .as_ref()
                    .and_then(|extents| {
                        extents
                            .iter()
                            .map(|extent| {
                                match extent {
                                    | rssn::kernel::signature::NatExpr::Lit(n) => {
                                        usize::try_from(*n).ok()
                                    },
                                    | _ => None,
                                }
                            })
                            .collect::<Option<Vec<_>>>()
                    })
                    .filter(|extents| usize::try_from(*rank).ok() == Some(extents.len())),
                elem: Box::new(representation(elem)?),
            })
        },
        | TypeExpr::Record(fields) => {
            let mut names = std::collections::BTreeSet::new();
            let fields = fields
                .iter()
                .map(|(name, ty)| {
                    if !names.insert(name) {
                        return None;
                    }
                    Some((name.clone(), representation(ty)?))
                })
                .collect::<Option<Vec<_>>>()?;
            Some(SigType::Record(fields))
        },
        | _ => None,
    }
}
fn schema(
    ty: &TypeExpr,
    trusted: bool,
) -> DimensionWitness {
    match ty {
        | TypeExpr::Dim(_, d) | TypeExpr::Point(d) => {
            concrete(d).map_or(DimensionWitness::Unknown, DimensionWitness::Scalar)
        },
        | TypeExpr::Int | TypeExpr::Real | TypeExpr::Float | TypeExpr::Complex if trusted => {
            DimensionWitness::Scalar(Dim::one())
        },
        | TypeExpr::Tensor { elem, .. } => {
            let element = schema(elem, trusted);
            let dimension = match &element {
                | DimensionWitness::Scalar(d) => Some(d.clone()),
                | _ => None,
            };
            DimensionWitness::Tensor {
                dimension,
                element: Some(Box::new(element)),
                element_repr: representation(elem),
                elements: Vec::new(),
            }
        },
        | TypeExpr::Record(fields) => {
            DimensionWitness::Record(
                fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), schema(ty, trusted)))
                    .collect(),
            )
        },
        | _ => DimensionWitness::Unknown,
    }
}
/// Extract explicit result witnesses. Bare numbers count as dimensionless only
/// when the caller already has an exact, trusted signature.
#[must_use]
pub fn extract(
    value: &Value,
    trusted: bool,
) -> DimensionWitness {
    match value {
        | Value::Dyn(v) => {
            concrete(&v.dim).map_or(DimensionWitness::Unknown, DimensionWitness::Scalar)
        },
        | Value::Quantity(v) => {
            concrete(&v.unit.dimension).map_or(DimensionWitness::Unknown, DimensionWitness::Scalar)
        },
        | Value::Int(_) | Value::Real(_) | Value::Float(_) | Value::Complex(_) if trusted => {
            DimensionWitness::Scalar(Dim::one())
        },
        | Value::Tensor(t) => {
            let dimension = match &t.elem {
                | TypeExpr::Dim(_, d) | TypeExpr::Point(d) => concrete(d),
                | TypeExpr::Int | TypeExpr::Real | TypeExpr::Float | TypeExpr::Complex if trusted => {
                    Some(Dim::one())
                },
                | _ => None,
            };
            // Numeric tensors have a uniform element schema; keep only
            // contradictory explicit witnesses rather than allocating one
            // witness per scalar. Nested structures still need each child.
            let elements = if let Some(expected) = &dimension {
                t.data.iter().filter_map(|value| {
                    if !matches!(value, Value::Dyn(_) | Value::Quantity(_)) { return None; }
                    let witness = extract(value, false);
                    if matches!(&witness, DimensionWitness::Scalar(actual) if actual == expected) { None } else { Some(witness) }
                }).collect()
            } else {
                t.data
                    .iter()
                    .map(|value| {
                        extract(
                            value,
                            trusted && matches!(value, Value::Tensor(_) | Value::Record(_)),
                        )
                    })
                    .collect()
            };
            DimensionWitness::Tensor {
                dimension,
                element: Some(Box::new(schema(&t.elem, trusted))),
                element_repr: representation(&t.elem),
                elements,
            }
        },
        | Value::Record(r) => {
            DimensionWitness::Record(
                r.fields
                    .iter()
                    .map(|(name, value)| (name.clone(), extract(value, trusted)))
                    .collect(),
            )
        },
        | _ => DimensionWitness::Unknown,
    }
}
fn scalar_type(sig: &SigType) -> Option<TypeExpr> {
    match sig {
        | SigType::Dim(inner, _) => scalar_type(inner),
        | SigType::Int => Some(TypeExpr::Int),
        | SigType::Real => Some(TypeExpr::Real),
        | SigType::Float => Some(TypeExpr::Float),
        | SigType::Complex => Some(TypeExpr::Complex),
        | SigType::Bool => Some(TypeExpr::Bool),
        | SigType::Str => Some(TypeExpr::String),
        | SigType::Symbol => Some(TypeExpr::Symbol),
        | _ => None,
    }
}
/// Attach known host dimensions to values before a value-native kernel call.
/// Empty tensor element representation comes from the checked exact signature.
pub fn attach(
    value: &mut Value,
    witness: &DimensionWitness,
    signature: &SigType,
) -> Result<(), KernelError> {
    match (value, witness, signature) {
        | (value, DimensionWitness::Scalar(dim), _)
            if !dim.is_one()
                && matches!(
                    &*value,
                    Value::Int(_)
                        | Value::Real(_)
                        | Value::Float(_)
                        | Value::Complex(_)
                        | Value::Dyn(_)
                ) =>
        {
            if !dim.is_concrete() {
                return Err(KernelError::capability("dim.input_witness"));
            }
            let payload = std::mem::replace(value, Value::Bool(false));
            let unit = crate::UnitVal::linear("coherent", dim.clone(), Scale::one()).to_rssn();
            *value = Value::Quantity(QuantityValue {
                value: Box::new(payload),
                unit,
            });
        },
        | (
            Value::Tensor(t),
            DimensionWitness::Tensor {
                dimension,
                element_repr,
                elements,
                ..
            },
            SigType::Tensor { elem, .. } | SigType::Sparse { elem, .. },
        ) => {
            if let Some(dim) = dimension {
                if !dim.is_concrete() {
                    return Err(KernelError::capability("dim.input_witness"));
                }
                if scalar_type(elem).is_some() {
                    // The selected signature is a requirement, not evidence of
                    // what an empty tensor contains. Use its actual stored
                    // representation; reject unknown instead of inventing it.
                    let base = if t.data.is_empty() {
                        let actual = element_repr.as_ref().ok_or_else(|| {
                            KernelError::capability("type.tensor_element_witness")
                        })?;
                        scalar_type(actual)
                            .ok_or_else(|| KernelError::capability("type.tensor_element_witness"))?
                    } else {
                        t.elem.clone()
                    };
                    t.elem = if dim.is_one() {
                        base
                    } else {
                        TypeExpr::Dim(Box::new(base), crate::registry::to_dim_expr(dim))
                    };
                }
            }
            for (value, witness) in t.data.iter_mut().zip(elements) {
                // Scalar leaves inherit the tensor's uniform metadata; do not
                // wrap each one and duplicate units.
                if !matches!(witness, DimensionWitness::Scalar(_)) {
                    attach(value, witness, elem)?;
                }
            }
        },
        | (Value::Record(r), DimensionWitness::Record(fields), SigType::Record(schema)) => {
            for (name, value) in &mut r.fields {
                if let (Some((_, witness)), Some((_, sig))) = (
                    fields.iter().find(|(n, _)| n == name),
                    schema.iter().find(|(n, _)| n == name),
                ) {
                    attach(value, witness, sig)?;
                }
            }
        },
        | _ => {},
    }
    Ok(())
}
