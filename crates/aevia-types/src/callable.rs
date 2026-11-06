//! Exact checked callable witnesses and origin-ordered generic projections.
//! These identify a checked declaration revision, not a language nominal type.
use crate::Infer;
use crate::TypeSubstitution;
use crate::check::GenericBinder;
use std::sync::Arc;

/// Owned declaration and checked creation-contract identity. Source owners and
/// complete span-free syntax remain stable during append-only rechecks; the
/// resolved contract distinguishes semantic changes hidden behind aliases.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CallableId(Arc<str>);
impl CallableId {
    pub(crate) fn new(
        module: &crate::ModulePath,
        site: aevia_syntax::Span,
        syntax: &str,
    ) -> Self {
        fn part(
            text: &mut String,
            value: &str,
        ) {
            text.push_str(&value.len().to_string());
            text.push(':');
            text.push_str(value);
        }
        let mut text = "checked-callable-v1:".to_string();
        part(&mut text, &module.identity_key());
        part(&mut text, &site.start.to_string());
        part(&mut text, &site.end.to_string());
        part(&mut text, syntax);
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
/// The fully resolved creation contract, alpha-normalized independently of the
/// checker's module-wide allocation order. Phantom reifications, numeric facts
/// and captured materializations participate even when absent from Fn shape.
pub(crate) fn contract(function: &crate::check::FunctionEvidence) -> String {
    use crate::NumericConstraint;
    use crate::Row;
    use crate::Type;
    struct Encoder {
        text: String,
        names: TypeSubstitution,
        infer: Infer,
    }
    impl Encoder {
        fn part(
            &mut self,
            value: impl AsRef<str>,
        ) {
            let value = value.as_ref();
            self.text.push_str(&value.len().to_string());
            self.text.push(':');
            self.text.push_str(value);
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
    }
    let mut encoder = Encoder {
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
    encoder.part(function.scheme.numeric.len().to_string());
    for clause in &function.scheme.numeric {
        match clause {
            | NumericConstraint::Literal { ty, kind } => {
                encoder.part("literal");
                encoder.part(format!("{kind:?}"));
                encoder.ty(ty);
            },
            | NumericConstraint::DefaultLiteral { ty, kind, inputs } => {
                encoder.part("default-literal");
                encoder.part(format!("{kind:?}"));
                encoder.ty(ty);
                encoder.part(inputs.len().to_string());
                for input in inputs {
                    encoder.ty(&Type::Scalar(*input, aevia_dim::Dim::one()));
                }
            },
            | NumericConstraint::Relation(relation) => {
                for ty in clause.types() {
                    encoder.names.freshen(&ty, &mut encoder.infer);
                }
                encoder.part(format!("{:?}", relation.map(|rep| encoder.names.rep(rep))));
            },
        }
    }
    encoder.part(function.numeric_literals.len().to_string());
    for (_, kind, ty) in &function.numeric_literals {
        encoder.part(format!("{kind:?}"));
        encoder.ty(ty);
    }
    // Quantification membership is a contract property too. Sort the canonical
    // ids rather than transporting module-local allocation order.
    for (kind, ids) in [
        ("type", &function.scheme.vars),
        ("dimension", &function.scheme.dims),
        ("row", &function.scheme.rows),
        ("natural", &function.scheme.nats),
    ] {
        let mut normalized: Vec<_> = ids
            .iter()
            .filter_map(|id| {
                match kind {
                    | "type" => {
                        encoder
                            .names
                            .types
                            .get(id)
                            .map(|value| format!("{value:?}"))
                            .or_else(|| {
                                encoder
                                    .names
                                    .representations
                                    .get(id)
                                    .map(|value| format!("{value:?}"))
                            })
                    },
                    | "dimension" => {
                        encoder
                            .names
                            .dimensions
                            .get(id)
                            .map(|value| value.to_transport_string())
                    },
                    | "row" => encoder.names.rows.get(id).map(|value| format!("{value:?}")),
                    | "natural" => {
                        encoder
                            .names
                            .naturals
                            .get(id)
                            .map(|value| format!("{value:?}"))
                    },
                    | _ => None,
                }
            })
            .collect();
        normalized.sort();
        encoder.part(kind);
        encoder.part(format!("{normalized:?}"));
    }
    encoder.text
}
