//! Lower AST e-graph rules into RSSN equality-saturation rewrites. Phase 7.

use crate::ast::{EGraphRule, Expr};
use rssn_advanced::custom::descriptor::CustomOpDescriptorBuilder;
use rssn_advanced::dag::builder::DagBuilder;
use rssn_advanced::dag::node::DagNodeId;
use rssn_advanced::dag::symbol::{FnId, SymbolKind};
use rust_decimal::prelude::ToPrimitive;

const EPS: f64 = 1e-12;

/// Attach e-graph rules from the `op` body when patterns are recognized.
pub fn attach_egraph_rules(
    op_name: &str,
    fn_id: FnId,
    rules: &[EGraphRule],
    mut builder: CustomOpDescriptorBuilder,
) -> CustomOpDescriptorBuilder {
    for (i, rule) in rules.iter().enumerate() {
        if let Some((arg_value, repl_value)) =
            match_call_zero_literal(&rule.pattern, &rule.replacement, op_name)
        {
            builder = builder.egraph_rule(rule.after_builtins, move |b, kind, children| {
                merge_call_with_literal_arg(b, kind, children, fn_id, arg_value, repl_value)
            });
            continue;
        }
        if match_call_var_passthrough(&rule.pattern, &rule.replacement, op_name).is_some() {
            builder = builder.egraph_rule(rule.after_builtins, move |b, kind, children| {
                merge_call_with_var_arg(b, kind, children, fn_id)
            });
            continue;
        }
        let _ = i;
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

fn match_call_var_passthrough(
    pattern: &crate::ast::Spanned<Expr>,
    replacement: &crate::ast::Spanned<Expr>,
    op_name: &str,
) -> Option<()> {
    let Expr::Call { func, args } = &pattern.node else {
        return None;
    };
    let Expr::Variable(var) = &replacement.node else {
        return None;
    };
    if func != op_name || args.len() != 1 {
        return None;
    }
    let Expr::Variable(arg_var) = &args[0].node else {
        return None;
    };
    if var == arg_var {
        Some(())
    } else {
        None
    }
}

fn literal_f64(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Literal { value, suffix: None } => value.to_f64(),
        _ => None,
    }
}

/// Declare `op(expected_literal)` equivalent to constant `repl_value`.
fn merge_call_with_literal_arg(
    b: &mut DagBuilder,
    kind: &SymbolKind,
    children: &[DagNodeId],
    fn_id: FnId,
    expected_arg: f64,
    repl_value: f64,
) -> Option<DagNodeId> {
    let SymbolKind::Function(call_id) = *kind else {
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

/// Declare `op(x)` equivalent to its argument `x`.
fn merge_call_with_var_arg(
    _b: &mut DagBuilder,
    kind: &SymbolKind,
    children: &[DagNodeId],
    fn_id: FnId,
) -> Option<DagNodeId> {
    let SymbolKind::Function(call_id) = *kind else {
        return None;
    };
    if call_id != fn_id || children.len() != 1 {
        return None;
    }
    Some(children[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::items::parse_source;
    use rssn_advanced::custom::descriptor::{CustomOpDescriptor, CustomOpRegistry, EvalFn};
    use rssn_advanced::dag::builder::DagBuilder;
    use rssn_advanced::egraph::egraph::{EGraph, EGraphConfig};

    extern "C" fn id(x: f64) -> f64 {
        x
    }

    #[test]
    fn egraph_zero_literal_merge() {
        let src = r#"
            pub op double(x: m -> m) {
                egraph { rewrite double(0) => 0 }
            }
        "#;
        let file = parse_source(src).unwrap();
        let crate::ast::Item::CustomOp { name, properties, .. } = &file.items[0].node else {
            panic!("expected op");
        };
        let mut dag = DagBuilder::new();
        let fn_id = dag.intern_function(name);
        let desc = attach_egraph_rules(
            name,
            fn_id,
            &properties.egraph_rules,
            CustomOpDescriptor::builder(fn_id, name, EvalFn::Arity1(id)),
        )
        .build();
        let mut reg = CustomOpRegistry::new();
        reg.register(desc).unwrap();

        let arg = dag.constant(0.0);
        let call = dag.function_call(fn_id, &[arg]);

        let mut eg = EGraph::new(&mut dag, EGraphConfig::default());
        reg.apply_to_egraph(&mut eg);
        eg.saturate(call);
        let best = eg.extract(call);
        let node = dag.arena().get(best).unwrap();
        assert!(matches!(node.kind, SymbolKind::Constant(v) if v.abs() < EPS));
    }

    #[test]
    fn egraph_var_passthrough() {
        let src = r#"
            pub op id(x: m -> m) {
                egraph { rewrite id(y) => y }
            }
        "#;
        let file = parse_source(src).unwrap();
        let crate::ast::Item::CustomOp { name, properties, .. } = &file.items[0].node else {
            panic!("expected op");
        };
        let mut dag = DagBuilder::new();
        let fn_id = dag.intern_function(name);
        let desc = attach_egraph_rules(
            name,
            fn_id,
            &properties.egraph_rules,
            CustomOpDescriptor::builder(fn_id, name, EvalFn::Arity1(id)),
        )
        .build();
        let mut reg = CustomOpRegistry::new();
        reg.register(desc).unwrap();

        let x = dag.variable("y");
        let call = dag.function_call(fn_id, &[x]);

        let mut eg = EGraph::new(&mut dag, EGraphConfig::default());
        reg.apply_to_egraph(&mut eg);
        eg.saturate(call);
        let best = eg.extract(call);
        assert_eq!(best, x);
    }
}
