//! Lower AST simplify rules into RSSN heuristic rules. Phase 6.

use crate::ast::{Expr, SimplifyRule};
use rssn_advanced::custom::descriptor::CustomOpDescriptorBuilder;
use rssn_advanced::dag::builder::DagBuilder;
use rssn_advanced::dag::node::DagNodeId;
use rssn_advanced::dag::symbol::{FnId, SymbolKind};
use rust_decimal::prelude::ToPrimitive;

const EPS: f64 = 1e-12;

/// Attach simplify rules parsed from the `op` body when patterns are recognized.
pub fn attach_simplify_rules(
    op_name: &str,
    fn_id: FnId,
    rules: &[SimplifyRule],
    mut builder: CustomOpDescriptorBuilder,
) -> CustomOpDescriptorBuilder {
    for (i, rule) in rules.iter().enumerate() {
        if let Some((arg_value, repl_value)) =
            match_call_zero_literal(&rule.pattern, &rule.replacement, op_name)
        {
            builder = builder.simplify_rule(
                format!("{op_name}-zero-literal-{i}"),
                10,
                move |b, kind, children| {
                    rewrite_call_with_literal_arg(b, kind, children, fn_id, arg_value, repl_value)
                },
            );
        }
    }
    builder
}

fn match_call_zero_literal(
    pattern: &crate::ast::Spanned<Expr>,
    replacement: &crate::ast::Spanned<Expr>,
    op_name: &str,
) -> Option<(f64, f64)> {
    let Expr::Call { func, args } = &pattern.node else {
        return None;
    };
    if func != op_name || args.len() != 1 {
        return None;
    }
    let arg = literal_f64(&args[0].node)?;
    let repl = literal_f64(&replacement.node)?;
    Some((arg, repl))
}

fn literal_f64(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Literal { value, suffix: None } => value.to_f64(),
        _ => None,
    }
}

fn rewrite_call_with_literal_arg(
    b: &mut DagBuilder,
    kind: SymbolKind,
    children: &[DagNodeId],
    fn_id: FnId,
    expected_arg: f64,
    repl_value: f64,
) -> Option<DagNodeId> {
    let SymbolKind::Function(call_id) = kind else {
        return None;
    };
    if call_id != fn_id || children.len() != 1 {
        return None;
    }
    let child = b.arena().get(children[0])?;
    let SymbolKind::Constant(v) = child.kind else {
        return None;
    };
    if (v - expected_arg).abs() > EPS {
        return None;
    }
    Some(b.constant(repl_value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::items::parse_source;
    use rssn_advanced::custom::descriptor::{CustomOpDescriptor, CustomOpRegistry, EvalFn};
    use rssn_advanced::dag::builder::DagBuilder;

    extern "C" fn id(x: f64) -> f64 {
        x
    }

    #[test]
    fn zero_arg_simplify_rule_fires() {
        let src = r#"
            pub op double(x: m -> m) {
                simplify { double(0) => 0 }
            }
        "#;
        let file = parse_source(src).unwrap();
        let item = &file.items[0].node;
        let crate::ast::Item::CustomOp { name, properties, .. } = item else {
            panic!("expected op");
        };
        let mut dag = DagBuilder::new();
        let fn_id = dag.intern_function(name);
        let mut desc = CustomOpDescriptor::builder(fn_id, name, EvalFn::Arity1(id));
        desc = attach_simplify_rules(name, fn_id, &properties.simplify_rules, desc);
        let mut reg = CustomOpRegistry::new();
        reg.register(desc.build()).unwrap();
        let rules = reg.build_rule_registry();
        let mut engine = rssn_advanced::heuristic::engine::HeuristicEngine::new(
            rssn_advanced::heuristic::HeuristicConfig::default(),
            rssn_advanced::heuristic::SearchStrategy::Greedy,
        )
        .with_rule_registry(std::sync::Arc::new(rules));
        let arg = dag.constant(0.0);
        let call = dag.function_call(fn_id, &[arg]);
        let out = engine.simplify(&mut dag, call);
        let node = dag.arena().get(out).unwrap();
        assert!(matches!(node.kind, SymbolKind::Constant(v) if v.abs() < EPS));
    }
}
