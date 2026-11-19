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
        /// Uniform physical dimension, if attested by the kernel.
        dimension: Option<Dim>,
        /// Evidence shared by all elements.
        element: Option<Box<Self>>,
        /// Uniform element representation at the ABI boundary.
        element_repr: Option<SigType>,
        /// Evidence for individual elements when the shared evidence is insufficient.
        elements: Vec<Self>,
    },
    /// Named field witnesses of a structured result.
    Record(Vec<(String, Self)>),
    /// Actual tagged payload witnesses; tags do not establish nominal ADT identity.
    Variant {
        /// Actual payload tag.
        tag: String,
        /// One witness for every actual payload field.
        fields: Vec<Self>,
    },
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
        | TypeExpr::Opaque(kind) if crate::registry::native_object_kind(kind) => {
            Some(SigType::Opaque(kind.clone()))
        },
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
            let elements = dimension.as_ref().map_or_else(
                || {
                    t.data.iter().map(|value| {
                        extract(value, trusted && matches!(value, Value::Tensor(_) | Value::Record(_)))
                    }).collect()
                },
                |expected| {
                    t.data.iter().filter_map(|value| {
                        if !matches!(value, Value::Dyn(_) | Value::Quantity(_)) {
                            return None;
                        }
                        let witness = extract(value, false);
                        (!matches!(&witness, DimensionWitness::Scalar(actual) if actual == expected)).then_some(witness)
                    }).collect()
                },
            );
            DimensionWitness::Tensor {
                dimension,
                element: Some(Box::new(schema(&t.elem, trusted))),
                element_repr: representation(&t.elem),
                elements,
            }
        },
        | Value::Variant(v) => {
            DimensionWitness::Variant {
                tag: v.tag.clone(),
                fields: v
                    .payload
                    .iter()
                    .map(|field| extract(field, trusted))
                    .collect(),
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
/// Empty tensor element representation comes from actual structural evidence.
///
/// # Errors
///
/// Returns an error when the value, witness and expected signature disagree.
pub fn attach(
    value: &mut Value,
    witness: &DimensionWitness,
    signature: &SigType,
) -> Result<(), KernelError> {
    if matches!(signature, SigType::Dyn) || matches!(witness, DimensionWitness::Scalar(_)) {
        return attach_dynamic(value, witness);
    }

    match (value, witness, signature) {
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
                        representation(&t.elem)
                            .as_ref()
                            .and_then(scalar_type)
                            .ok_or_else(|| KernelError::capability("type.tensor_element_witness"))?
                    };
                    t.elem = if dim.is_one() {
                        base
                    } else {
                        TypeExpr::Dim(Box::new(base), crate::registry::to_dim_expr(dim))
                    };
                }
            }
            // Preserve actual metadata for an empty native-handle tensor. The
            // declared signature alone never supplies this evidence.
            if t.data.is_empty()
                && let Some(SigType::Opaque(actual)) = element_repr
                && crate::registry::native_object_kind(actual)
                && matches!(elem.as_ref(), SigType::Opaque(expected) if expected == actual)
            {
                t.elem = TypeExpr::Opaque(actual.clone());
            }
            for (value, witness) in t.data.iter_mut().zip(elements) {
                // Scalar leaves inherit the tensor's uniform metadata; do not
                // wrap each one and duplicate units.
                if let (Some(uniform), DimensionWitness::Scalar(actual)) = (dimension, witness)
                    && scalar_type(elem).is_some()
                {
                    if uniform != actual {
                        return Err(KernelError::capability("dim.input_witness"));
                    }
                    if matches!(value, Value::Quantity(_) | Value::Dyn(_)) {
                        attach_dynamic(value, witness)?;
                    }
                } else {
                    attach(value, witness, elem)?;
                }
            }
        },
        | (
            Value::Variant(value),
            DimensionWitness::Variant { tag, fields },
            SigType::Variant(cases),
        ) => {
            if crate::registry::option_payload(cases).is_none()
                || value.tag != *tag
                || value.payload.len() != fields.len()
            {
                return Err(KernelError::capability("type.option_witness"));
            }
            let (_, schema) = cases
                .iter()
                .find(|(name, _)| name == tag)
                .ok_or_else(|| KernelError::capability("type.option_witness"))?;
            if schema.len() != value.payload.len() {
                return Err(KernelError::capability("type.option_witness"));
            }
            for ((value, proof), sig) in value.payload.iter_mut().zip(fields).zip(schema) {
                attach(value, proof, sig)?;
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

// Reconstruct only types supplied by actual structural/physical evidence.
// The dynamic signature itself supplies no representation or unit facts.
fn evidenced_type(
    repr: &SigType,
    proof: &DimensionWitness,
) -> Option<TypeExpr> {
    use DimensionWitness as W;
    match (repr, proof) {
        | (SigType::Int | SigType::Real | SigType::Float | SigType::Complex, W::Scalar(dim))
            if dim.is_concrete() =>
        {
            let base = scalar_type(repr)?;
            Some(if dim.is_one() {
                base
            } else {
                TypeExpr::Dim(Box::new(base), crate::registry::to_dim_expr(dim))
            })
        },
        | (SigType::Dim(inner, declared), W::Scalar(actual))
            if declared.is_concrete() && declared == actual =>
        {
            evidenced_type(inner, proof)
        },
        | (SigType::Bool, W::Unknown) => Some(TypeExpr::Bool),
        | (SigType::Bool, W::Scalar(dim)) if dim.is_one() => Some(TypeExpr::Bool),
        | (SigType::Str, W::Unknown) => Some(TypeExpr::String),
        | (SigType::Str, W::Scalar(dim)) if dim.is_one() => Some(TypeExpr::String),
        | (SigType::Opaque(kind), W::Unknown) if crate::registry::native_object_kind(kind) => {
            Some(TypeExpr::Opaque(kind.clone()))
        },
        | (SigType::Record(schema), W::Record(fields)) if schema.len() == fields.len() => {
            let mut names = std::collections::BTreeSet::new();
            let mut result = Vec::new();
            for (name, repr) in schema {
                if !names.insert(name) {
                    return None;
                }
                let mut found = fields.iter().filter(|(n, _)| n == name);
                let (_, proof) = found.next()?;
                if found.next().is_some() {
                    return None;
                }
                result.push((name.clone(), evidenced_type(repr, proof)?));
            }
            Some(TypeExpr::Record(result))
        },
        | (
            SigType::Tensor {
                rank: Some(rank),
                shape,
                elem,
            },
            W::Tensor {
                dimension,
                element,
                element_repr: Some(actual),
                ..
            },
        ) if elem.as_ref() == actual && shape.as_ref().is_none_or(|s| s.len() == *rank) => {
            let fallback = dimension
                .as_ref()
                .map_or(W::Unknown, |d| W::Scalar(d.clone()));
            let proof = element.as_deref().unwrap_or(&fallback);
            let shape = match shape {
                | Some(s) => {
                    Some(
                        s.iter()
                            .map(|n| {
                                u64::try_from(*n)
                                    .ok()
                                    .map(rssn::kernel::signature::NatExpr::Lit)
                            })
                            .collect::<Option<Vec<_>>>()?,
                    )
                },
                | None => None,
            };
            Some(TypeExpr::Tensor {
                rank: rssn::kernel::signature::NatExpr::Lit(u64::try_from(*rank).ok()?),
                shape,
                elem: Box::new(evidenced_type(elem, proof)?),
            })
        },
        | _ => None,
    }
}
#[allow(
    clippy::too_many_lines,
    reason = "Validates evidence for each recursive dynamic value variant"
)]
fn attach_dynamic(
    value: &mut Value,
    witness: &DimensionWitness,
) -> Result<(), KernelError> {
    use DimensionWitness as W;
    let missing = || KernelError::capability("dim.dynamic_input_witness");
    match (value, witness) {
        | (value, W::Scalar(dim)) => {
            if !dim.is_concrete() {
                return Err(missing());
            }
            let explicit = match &*value {
                | Value::Quantity(q) => Some(&q.unit.dimension),
                | Value::Dyn(d) => Some(&d.dim),
                | _ => None,
            };
            if let Some(actual) = explicit {
                if concrete(actual).as_ref() != Some(dim) {
                    return Err(missing());
                }
                return Ok(());
            }
            if !matches!(
                &*value,
                Value::Int(_) | Value::Real(_) | Value::Float(_) | Value::Complex(_)
            ) {
                return Err(missing());
            }
            if !dim.is_one() {
                let payload = std::mem::replace(value, Value::Bool(false));
                *value = Value::Quantity(QuantityValue {
                    value: Box::new(payload),
                    unit: crate::UnitVal::linear("coherent", dim.clone(), Scale::one()).to_rssn(),
                });
            }
        },
        | (
            Value::Tensor(t),
            W::Tensor {
                dimension,
                element,
                element_repr,
                elements,
            },
        ) => {
            if t.shape.iter().try_fold(1usize, |n, d| n.checked_mul(*d)) != Some(t.data.len())
                || (!elements.is_empty() && elements.len() != t.data.len())
            {
                return Err(missing());
            }
            if dimension.as_ref().is_some_and(|d| !d.is_concrete()) {
                return Err(missing());
            }
            let fallback = dimension
                .as_ref()
                .map_or(W::Unknown, |d| W::Scalar(d.clone()));
            let uniform = element_repr.as_ref().is_some_and(|r| {
                matches!(
                    scalar_type(r),
                    Some(TypeExpr::Int | TypeExpr::Real | TypeExpr::Float | TypeExpr::Complex)
                )
            }) && dimension.is_some();
            if let Some(repr) = element_repr {
                let proof = element.as_deref().unwrap_or(&fallback);
                if let Some(ty) = evidenced_type(repr, proof) {
                    t.elem = ty;
                } else if t.data.is_empty() {
                    return Err(missing());
                }
            }
            for (i, v) in t.data.iter_mut().enumerate() {
                let unknown = W::Unknown;
                let inherited = if uniform {
                    &fallback
                } else {
                    &unknown
                };
                let proof = elements.get(i).or(element.as_deref()).unwrap_or(inherited);
                if uniform {
                    let W::Scalar(actual) = proof else {
                        return Err(missing());
                    };
                    if dimension.as_ref() != Some(actual) {
                        return Err(missing());
                    }
                    if matches!(v, Value::Quantity(_) | Value::Dyn(_)) {
                        attach_dynamic(v, proof)?;
                    }
                    let repr = element_repr.as_ref().ok_or_else(missing)?;
                    let mut payload = &*v;
                    loop {
                        match payload {
                            | Value::Quantity(q) => payload = &q.value,
                            | Value::Dyn(d) => payload = &d.value,
                            | _ => break,
                        }
                    }
                    let repr = if let SigType::Dim(inner, _) = repr {
                        inner.as_ref()
                    } else {
                        repr
                    };
                    if !matches!(
                        (payload, repr),
                        (Value::Int(_), SigType::Int | SigType::Real)
                            | (Value::Real(_), SigType::Real)
                            | (Value::Float(_), SigType::Float)
                            | (Value::Complex(_), SigType::Complex)
                    ) {
                        return Err(missing());
                    }
                } else if !matches!(proof, W::Unknown) {
                    attach_dynamic(v, proof)?;
                }
            }
            if !uniform && let Some((first, rest)) = t.data.split_first() {
                let first = first.type_expr();
                t.elem = if rest.iter().all(|v| v.type_expr() == first) {
                    first
                } else {
                    TypeExpr::Dyn(rssn::kernel::signature::RepExpr {
                        repr: "term".into(),
                        dim: None,
                    })
                };
            }
        },
        | (Value::Record(r), W::Record(fields)) => {
            if r.fields.len() != fields.len() {
                return Err(missing());
            }
            let mut names = std::collections::BTreeSet::new();
            for (name, v) in &mut r.fields {
                if !names.insert(name.clone()) {
                    return Err(missing());
                }
                let mut found = fields.iter().filter(|(n, _)| n == name);
                let Some((_, proof)) = found.next() else {
                    return Err(missing());
                };
                if found.next().is_some() {
                    return Err(missing());
                }
                attach_dynamic(v, proof)?;
            }
        },
        | (Value::Variant(v), W::Variant { tag, fields }) => {
            if &v.tag != tag || v.payload.len() != fields.len() {
                return Err(missing());
            }
            for (v, w) in v.payload.iter_mut().zip(fields) {
                attach_dynamic(v, w)?;
            }
        },
        | (_, W::Unknown) => {},
        | _ => return Err(missing()),
    }
    Ok(())
}

#[cfg(test)]
mod dynamic_input_evidence_tests {
    use super::DimensionWitness as W;
    use super::attach;
    use crate::SigType as S;
    use aevia_dim::Dim;
    use rssn::kernel::signature::TypeExpr;
    use rssn::kernel::value::RecordValue;
    use rssn::kernel::value::TensorValue;
    use rssn::kernel::value::Value;
    fn tensor(data: Vec<Value>) -> Value {
        Value::Tensor(TensorValue {
            shape: vec![data.len()],
            data,
            elem: TypeExpr::Float,
        })
    }
    fn dimension(value: &Value) -> Dim {
        match value {
            | Value::Quantity(q) => crate::registry::convert_dim(&q.unit.dimension),
            | Value::Dyn(d) => crate::registry::convert_dim(&d.dim),
            | _ => Dim::one(),
        }
    }
    #[test]
    fn dynamic_tensors_preserve_uniform_and_independent_physical_axes() {
        let length = Dim::base("Length");
        let time = Dim::base("Time");
        let mut uniform = tensor(vec![Value::Float(2.0), Value::Float(3.0)]);
        attach(
            &mut uniform,
            &W::Tensor {
                dimension: Some(length.clone()),
                element: None,
                element_repr: Some(S::Float),
                elements: vec![],
            },
            &S::Dyn,
        )
        .unwrap();
        let Value::Tensor(t) = &uniform else { panic!() };
        assert_eq!(
            t.elem,
            TypeExpr::Dim(
                Box::new(TypeExpr::Float),
                crate::registry::to_dim_expr(&length)
            )
        );
        let mut mixed = tensor(vec![Value::Float(2.0), Value::Float(3.0)]);
        attach(
            &mut mixed,
            &W::Tensor {
                dimension: Some(Dim::one()),
                element: None,
                element_repr: None,
                elements: vec![W::Scalar(length.clone()), W::Scalar(time.clone())],
            },
            &S::Dyn,
        )
        .unwrap();
        let Value::Tensor(t) = &mixed else { panic!() };
        assert_eq!(dimension(&t.data[0]), length);
        assert_eq!(dimension(&t.data[1]), time);
        assert!(matches!(t.elem, TypeExpr::Dyn(_)));
    }
    #[test]
    fn dynamic_records_and_empty_structures_use_actual_nested_evidence() {
        let length = Dim::base("Length");
        let time = Dim::base("Time");
        let fields = vec![
            ("x".into(), W::Scalar(length.clone())),
            ("t".into(), W::Scalar(time.clone())),
        ];
        let mut record = Value::Record(RecordValue {
            fields: vec![
                ("x".into(), Value::Float(2.0)),
                ("t".into(), Value::Float(3.0)),
            ],
        });
        attach(&mut record, &W::Record(fields.clone()), &S::Dyn).unwrap();
        let Value::Record(r) = record else { panic!() };
        assert_eq!(dimension(r.get("x").unwrap()), length);
        assert_eq!(dimension(r.get("t").unwrap()), time);
        let mut empty = tensor(vec![]);
        let proof = W::Tensor {
            dimension: None,
            element: Some(Box::new(W::Record(fields))),
            element_repr: Some(S::Record(vec![
                ("x".into(), S::Float),
                ("t".into(), S::Float),
            ])),
            elements: vec![],
        };
        attach(&mut empty, &proof, &S::Dyn).unwrap();
        let Value::Tensor(t) = empty else { panic!() };
        let TypeExpr::Record(fields) = t.elem else {
            panic!()
        };
        assert!(
            fields
                .iter()
                .all(|(_, ty)| matches!(ty, TypeExpr::Dim(_, _)))
        );
        let mut empty = tensor(vec![]);
        let bad = W::Tensor {
            dimension: None,
            element: Some(Box::new(W::Record(vec![]))),
            element_repr: Some(S::Record(vec![("x".into(), S::Float)])),
            elements: vec![],
        };
        assert!(attach(&mut empty, &bad, &S::Dyn).is_err());
    }
}
