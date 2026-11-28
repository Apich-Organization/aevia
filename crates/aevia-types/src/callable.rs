//! Exact checked callable witnesses and origin-ordered generic projections.
//! These identify a checked declaration revision, not a language nominal type.

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
use crate::NumericConstraint;
use crate::Row;
use crate::Scheme;
use crate::Type;
use crate::TypeSubstitution;
use crate::check::FunctionEvidence;
use crate::check::GenericBinder;
use crate::keys::key_part;
use std::sync::Arc;

/// Owned declaration and checked creation-contract identity.
///
/// Source owners and span-free syntax survive append-only rechecks. The
/// resolved contract distinguishes semantic changes hidden behind aliases.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
    bincode_next::Fingerprint,
)]
pub struct CallableId(Arc<str>);
impl CallableId {
    pub(crate) fn new(
        module: &crate::ModulePath,
        site: aevia_syntax::Span,
        syntax: &str,
    ) -> Self {
        let mut text = "checked-callable-v1:".to_string();
        key_part(&mut text, &module.identity_key());
        key_part(&mut text, &site.start.to_string());
        key_part(&mut text, &site.end.to_string());
        key_part(&mut text, syntax);
        Self(text.into())
    }

    pub(crate) fn with_contract(
        &self,
        contract: &str,
    ) -> Self {
        Self(format!("{}{}:{contract}", self.0, contract.len()).into())
    }
}

/// The complete selection of an actual source declaration's binders, in its
/// original order, expressed in the current scheme/activation universe.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
    bincode_next::Fingerprint,
)]
pub struct CallableProjection {
    /// Defining source declaration and immutable checked contract.
    pub origin: CallableId,
    /// Original declaration order, independent of this alias's visible order.
    pub arguments: Vec<GenericBinder>,
    /// True when source syntax explicitly selected this generic view.
    pub selected: bool,
    /// A checked body returning this parameter unchanged transports the actual
    /// callable argument, including phantom binders absent from its Fn shape.
    pub forwarded_parameter: Option<usize>,
}
impl CallableProjection {
    /// Instantiate the projection with the same substitution as its scheme.
    #[must_use]
    pub fn substituted(
        &self,
        values: &TypeSubstitution,
    ) -> Self {
        Self {
            arguments: self
                .arguments
                .iter()
                .map(|arg| arg.substituted(values))
                .collect(),
            ..self.clone()
        }
    }

    /// Freeze solved variables without changing their defining origin.
    #[must_use]
    pub fn resolved(
        &self,
        infer: &Infer,
    ) -> Self {
        Self {
            arguments: self
                .arguments
                .iter()
                .map(|arg| arg.resolved(infer))
                .collect(),
            ..self.clone()
        }
    }
}
struct ContractEncoder {
    text: String,
    names: TypeSubstitution,
    infer: Infer,
}
impl ContractEncoder {
    fn part(
        &mut self,
        value: impl AsRef<str>,
    ) {
        key_part(&mut self.text, value.as_ref());
    }

    fn ty(
        &mut self,
        value: &Type,
    ) {
        let value = self.names.freshen(value, &mut self.infer);
        self.ty_resolved(&value);
    }

    fn ty_resolved(
        &mut self,
        value: &Type,
    ) {
        match value {
            | Type::Scalar(rep, dim) => {
                self.part("scalar");
                self.part(format!("{rep:?}"));
                self.part(dim.to_transport_string());
            },
            | Type::Point(dim) => {
                self.part("point");
                self.part(dim.to_transport_string());
            },
            | Type::UnitOf(dim) => {
                self.part("unit-of");
                self.part(dim.to_transport_string());
            },
            | Type::Dyn(rep) => {
                self.part("dyn");
                self.part(format!("{rep:?}"));
            },
            | Type::Tensor { shape, elem } => {
                self.part("tensor");
                self.part(format!("{shape:?}"));
                self.ty_resolved(elem);
            },
            | Type::Fn(params, ret) => {
                self.part("fn");
                self.part(params.len().to_string());
                for param in params {
                    self.ty_resolved(param);
                }
                self.ty_resolved(ret);
            },
            | Type::Record(row) => {
                self.part("record");
                self.row(row);
            },
            | Type::Adt(id, args) => {
                self.part("adt");
                self.part(format!("{:?}", crate::adt::key_of(*id)));
                self.part(args.len().to_string());
                for arg in args {
                    self.ty_resolved(arg);
                }
            },
            | Type::Io(value) => {
                self.part("io");
                self.ty_resolved(value);
            },
            | Type::TypeRepIntrinsic(inner) | Type::FromValueIntrinsic(inner) => {
                self.part(if matches!(value, Type::TypeRepIntrinsic(_)) {
                    "type-rep"
                } else {
                    "from-value"
                });
                if let Some(value) = inner {
                    self.part("selected");
                    self.ty_resolved(value);
                } else {
                    self.part("unselected");
                }
            },
            | _ => self.part(format!("{value:?}")),
        }
    }

    fn row(
        &mut self,
        row: &Row,
    ) {
        match row {
            | Row::Empty => self.part("closed"),
            | Row::Var(id) => {
                self.part("row-variable");
                self.part(id.to_string());
            },
            | Row::Extend { label, ty, tail } => {
                self.part("field");
                self.part(label);
                self.ty_resolved(ty);
                self.row(tail);
            },
        }
    }

    fn numeric(
        &mut self,
        clauses: &[NumericConstraint],
    ) {
        self.part(clauses.len().to_string());
        for clause in clauses {
            match clause {
                | NumericConstraint::Literal { ty, kind } => {
                    self.part("literal");
                    self.part(format!("{kind:?}"));
                    self.ty(ty);
                },
                | NumericConstraint::DefaultLiteral { ty, kind, inputs } => {
                    self.part("default-literal");
                    self.part(format!("{kind:?}"));
                    self.ty(ty);
                    self.part(inputs.len().to_string());
                    for input in inputs {
                        self.ty(&Type::Scalar(*input, aevia_dim::Dim::one()));
                    }
                },
                | NumericConstraint::Relation(relation) => {
                    for ty in clause.types() {
                        self.names.register_variables(&ty, &mut self.infer);
                    }
                    self.part(format!("{:?}", relation.map(|rep| self.names.rep(rep))));
                },
            }
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "keeps canonical row-exclusion encoding with the other function requirements"
    )]
    fn row_lacks(
        &mut self,
        requirements: &[(u32, Vec<String>)],
    ) {
        let mut normalized = Vec::new();
        for (id, labels) in requirements {
            self.names
                .register_variables(&Type::Record(Row::Var(*id)), &mut self.infer);
            if let Some(Row::Var(fresh)) = self.names.rows.get(id) {
                normalized.extend(labels.iter().map(|label| (*fresh, label)));
            }
        }
        if !normalized.is_empty() {
            normalized.sort_unstable();
            normalized.dedup();
            self.part("row-lacks");
            self.part(normalized.len().to_string());
            for (id, label) in normalized {
                self.part(id.to_string());
                self.part(label);
            }
        }
    }

    fn quantifiers(
        &mut self,
        scheme: &Scheme,
    ) {
        // Quantification membership is a contract property too. Sort the canonical
        // ids rather than transporting module-local allocation order.
        for (kind, ids) in [
            ("type", &scheme.vars),
            ("dimension", &scheme.dims),
            ("row", &scheme.rows),
            ("natural", &scheme.nats),
        ] {
            let mut normalized: Vec<_> = ids
                .iter()
                .filter_map(|id| {
                    match kind {
                        | "type" => {
                            self.names
                                .types
                                .get(id)
                                .map(|value| format!("{value:?}"))
                                .or_else(|| {
                                    self.names
                                        .representations
                                        .get(id)
                                        .map(|value| format!("{value:?}"))
                                })
                        },
                        | "dimension" => {
                            self.names
                                .dimensions
                                .get(id)
                                .map(aevia_dim::Dim::to_transport_string)
                        },
                        | "row" => self.names.rows.get(id).map(|value| format!("{value:?}")),
                        | "natural" => {
                            self.names
                                .naturals
                                .get(id)
                                .map(|value| format!("{value:?}"))
                        },
                        | _ => None,
                    }
                })
                .collect();
            normalized.sort();
            self.part(kind);
            self.part(format!("{normalized:?}"));
        }
    }
}

/// The alpha-normalized creation contract of a checked callable.
///
/// Phantom reifications, numeric facts and captured materializations still
/// participate when they are absent from the function type.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "encode the complete checked callable contract"
    )
)]
pub(crate) fn contract(function: &FunctionEvidence) -> String {
    let mut encoder = ContractEncoder {
        text: "callable-contract-v1:".into(),
        names: TypeSubstitution::default(),
        infer: Infer::new(),
    };
    encoder.part(function.binders.len().to_string());
    for binder in &function.binders {
        encoder.part(format!("{:?}", std::mem::discriminant(binder)));
        encoder.ty(&binder.as_type());
    }
    encoder.ty(&function.scheme.ty);
    encoder.part(function.scheme.constraints.len().to_string());
    for requirement in &function.scheme.constraints {
        encoder.part(&requirement.class);
        encoder.part(requirement.args.len().to_string());
        for argument in &requirement.args {
            encoder.ty(argument);
        }
    }
    encoder.row_lacks(&function.scheme.row_lacks);
    for (label, values) in [
        ("materializations", &function.scheme.materializations),
        ("reifications", &function.reifications),
    ] {
        encoder.part(label);
        encoder.part(values.len().to_string());
        for value in values {
            encoder.ty(value);
        }
    }
    encoder.numeric(&function.scheme.numeric);
    encoder.part(function.numeric_literals.len().to_string());
    for (_, kind, ty) in &function.numeric_literals {
        encoder.part(format!("{kind:?}"));
        encoder.ty(ty);
    }
    encoder.quantifiers(&function.scheme);
    encoder.text
}
