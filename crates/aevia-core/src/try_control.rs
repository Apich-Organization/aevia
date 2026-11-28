//! Checked non-local returns are control flow, never ordinary error payloads.
#![deny(
    missing_docs,
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::nursery,
    clippy::panic,
    clippy::pedantic,
    clippy::single_call_fn,
    clippy::unwrap_used
)]

use crate::EvalError;
use crate::Interp;
use crate::value::Value;
use crate::value::{
    self,
};
use aevia_syntax::Span;
use aevia_types::Type;
use aevia_types::adt;
use aevia_types::check::TryBoundary;
use std::rc::Rc;
impl Interp {
    pub(crate) fn try_value(
        &self,
        value: &mut Value,
        span: Span,
    ) -> Result<Value, EvalError> {
        let Value::Variant(variant) = value else {
            return Err(EvalError::new("E0702", "`?` expects Result or Option").at(span));
        };
        let (success, name, arity) = match (variant.adt, variant.index) {
            | (adt::RESULT_ID, 0) => (true, "Ok", 1),
            | (adt::RESULT_ID, 1) => (false, "Err", 1),
            | (adt::OPTION_ID, 0) => (false, "None", 0),
            | (adt::OPTION_ID, 1) => (true, "Some", 1),
            | _ => return Err(EvalError::new("E0702", "malformed Result/Option in `?`").at(span)),
        };
        if variant.name != name || variant.fields.len() != arity {
            return Err(EvalError::new("E0702", "malformed Result/Option in `?`").at(span));
        }
        if success {
            return variant
                .fields
                .first()
                .cloned()
                .ok_or_else(|| EvalError::new("E0702", "malformed Result/Option in `?`").at(span));
        }
        let Some(target) = self.context.evidence.try_targets.get(&span) else {
            return Err(EvalError::new("E0702", relaxed_try_error(variant))
                .at(span)
                .with_stack(self.frames.clone()));
        };
        let expected = self
            .bindings
            .as_ref()
            .filter(|bindings| {
                bindings.verified
                    && bindings
                        .context
                        .upgrade()
                        .is_some_and(|context| Rc::ptr_eq(&context, &self.context))
            })
            .map_or_else(
                || target.result.clone(),
                |bindings| bindings.values.apply(&target.result),
            );
        let Type::Adt(id, _) = &expected else {
            return Err(EvalError::new("E0702", relaxed_try_error(variant))
                .at(span)
                .with_stack(self.frames.clone()));
        };
        if *id != adt::RESULT_ID && *id != adt::OPTION_ID {
            return Err(EvalError::new("E0702", relaxed_try_error(variant))
                .at(span)
                .with_stack(self.frames.clone()));
        }
        if *id != variant.adt {
            return Err(
                EvalError::new("E0702", "`?` cannot cross Result/Option families").at(span),
            );
        }
        // Propagation replaces the source's phantom type. A unique operand
        // can keep its payload allocation after discarding the old evidence.
        let mut propagated = if let Some(owned) = Rc::get_mut(variant) {
            owned.type_args = None;
            owned.verified_type = None;
            std::mem::replace(value, Value::Unit)
        } else {
            value::make_variant(*id, variant.index, name, variant.fields.clone())
        };
        if let Value::Variant(variant) = &mut propagated {
            Rc::make_mut(variant).constructor_origin = true;
        }
        // The payload is not freshly built from checked source: it is whatever
        // value reached `?`, possibly from a host. That is a trust boundary,
        // so the payload is checked against the destination type in full.
        if crate::host_witness::capture(&propagated, &expected, true).is_some() {
            if let Value::Variant(variant) = &mut propagated {
                let variant = Rc::make_mut(variant);
                variant.verified_type = Some(expected.clone());
                if let Type::Adt(_, args) = expected {
                    variant.type_args = Some(args);
                }
            }
        } else if !aevia_types::class::has_variables(&expected)
            && !legacy_text_error(&propagated, &expected)
        {
            return Err(EvalError::new(
                "E0302",
                "propagated error does not satisfy its checked return type",
            )
            .at(span));
        }
        let mut exit = EvalError::new("E0702", "source `?` return").at(span);
        exit.payload = Some(propagated);
        exit.try_target = Some(target.boundary);
        Err(exit)
    }
}
/// Handles an early return when it reaches the function that owns it.
pub fn finish_function(
    outcome: Result<Value, EvalError>,
    site: Option<Span>,
) -> Result<Value, EvalError> {
    match outcome {
        | Err(mut error)
            if site.is_some_and(|site| error.try_target == Some(TryBoundary::Function(site))) =>
        {
            error
                .payload
                .take()
                .ok_or_else(|| EvalError::new("E0809", "try return is missing its payload"))
        },
        | outcome => outcome,
    }
}

fn relaxed_try_error(variant: &value::VariantValue) -> String {
    match (variant.adt, variant.index, variant.fields.as_slice()) {
        | (adt::RESULT_ID, 1, [error]) => format!("`?` failed with `Err({})`", value::show(error)),
        | _ => "`?` failed on `None`".to_string(),
    }
}

// Existing builtin error families carry text before crossing the opaque Value
// boundary. Preserve that language ABI without minting a nominal witness. The
// exact prelude origin, empty schema and finite family list exclude source ADTs
// (including a source type with the same spelling) and ordinary concrete errors.
#[allow(
    clippy::single_call_fn,
    reason = "isolate the prelude error compatibility boundary"
)]
fn legacy_text_error(
    value: &Value,
    expected: &Type,
) -> bool {
    let (Value::Variant(value), Type::Adt(id, args)) = (value, expected) else {
        return false;
    };
    if *id != adt::RESULT_ID
        || value.index != 1
        || !matches!(value.fields.as_slice(), [Value::Str(_)])
    {
        return false;
    }
    let [_, Type::Adt(error, parameters)] = args.as_slice() else {
        return false;
    };
    let key = adt::key_of(*error);
    parameters.is_empty()
        && key.module.is_prelude()
        && matches!(
            key.name.as_str(),
            "ParseError"
                | "IOError"
                | "ShapeError"
                | "DimError"
                | "MathError"
                | "EvalError"
                | "LinAlgError"
                | "DataError"
                | "SimError"
                | "PdeError"
        )
        && adt::get(*error)
            .is_some_and(|definition| definition.params.is_empty() && definition.ctors.is_empty())
}
