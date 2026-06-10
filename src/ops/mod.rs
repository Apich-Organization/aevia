//! `op` DSL lowering to RSSN [`CustomOpRegistry`]. Phase 4.

pub mod egraph;
mod simplify;

use crate::ast::{Item, OpProperties, SourceFile};
use crate::diagnostics::AeviaResult;
use rssn_advanced::custom::descriptor::{CustomOpDescriptor, CustomOpRegistry, EvalFn};
use rssn_advanced::dag::builder::DagBuilder;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;

/// Identity stub for custom ops until the source language gains runtime bodies.
extern "C" fn unary_identity(x: f64) -> f64 {
    x
}

/// Build a [`CustomOpRegistry`] from all `pub op` items in a source file.
pub fn registry_from_file(file: &SourceFile, builder: &mut DagBuilder) -> CustomOpRegistry {
    let mut reg = CustomOpRegistry::new();
    for item in &file.items {
        if let Item::CustomOp {
            name,
            properties,
            ..
        } = &item.node
        {
            let _ = register_op(builder, &mut reg, name, properties);
        }
    }
    reg
}

/// Register every custom op across a multi-module program (entry file only by default).
pub fn registry_from_items(
    items: &[crate::ast::Spanned<Item>],
    builder: &mut DagBuilder,
) -> AeviaResult<Arc<CustomOpRegistry>> {
    let mut reg = CustomOpRegistry::new();
    for item in items {
        if let Item::CustomOp {
            name,
            properties,
            ..
        } = &item.node
        {
            register_op(builder, &mut reg, name, properties)?;
        }
    }
    Ok(Arc::new(reg))
}

fn register_op(
    builder: &mut DagBuilder,
    reg: &mut CustomOpRegistry,
    name: &str,
    props: &OpProperties,
) -> AeviaResult<()> {
    let fn_id = builder.intern_function(name);
    let mut desc = CustomOpDescriptor::builder(fn_id, name, EvalFn::Arity1(unary_identity));

    if props.vectorizable {
        desc = desc.vectorizable();
    }
    if let Some(cost) = props.cost {
        if let Some(c) = cost.to_f64() {
            desc = desc.cost(c);
        }
    }

    desc = simplify::attach_simplify_rules(name, fn_id, &props.simplify_rules, desc);
    desc = egraph::attach_egraph_rules(name, fn_id, &props.egraph_rules, desc);

    if props.commutative {
        desc = egraph::attach_commutativity_rule(fn_id, desc);
    }
    if props.associative {
        desc = egraph::attach_associativity_rule(fn_id, desc);
    }

    reg.register(desc.build())
        .map_err(|e| crate::diagnostics::AeviaError::message(format!("custom op `{name}`: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::items::parse_source;

    #[test]
    fn builds_registry_from_op_item() {
        let src = r#"
            pub op scale(x: m -> m) {
                properties {
                    vectorizable: true,
                    cost: 2.0,
                }
            }
        "#;
        let file = parse_source(src).unwrap();
        let mut builder = DagBuilder::new();
        let reg = registry_from_file(&file, &mut builder);
        assert!(reg.get_by_name("scale").is_some());
        assert!(reg.get_by_name("scale").unwrap().vectorizable);
    }
}
