//! Phase 8: kernel fusion partition and pipeline integration.

use aevia::ast::{FunctionBody, Item};
use aevia::fusion::{FusionConfig, FusionKernel, KernelKind};
use rssn_advanced::custom::descriptor::CustomOpRegistry;
use rssn_advanced::dag::builder::DagBuilder;
use std::collections::HashMap;

#[test]
fn pipeline_optimize_with_fusion_expression_body() {
    let src = "pub fn ke(m: kg, v: m/s) -> J := 0.5 * m * v^2;";
    let file = aevia::parser::items::parse_source(src).unwrap();
    let Item::Function { body, attributes, .. } = &file.items[0].node else {
        panic!("expected function");
    };

    let mut lowerer = aevia::lowering::Lowerer::new();
    let mut env = HashMap::new();
    for name in ["m", "v"] {
        let id = lowerer.builder.variable(name);
        env.insert(name.to_string(), id);
    }
    let FunctionBody::Expression(expr) = body else {
        panic!("expression body");
    };
    let root = lowerer.lower_expr(expr, &mut env).unwrap();

    let op_reg = CustomOpRegistry::new();
    let fusion_cfg = FusionConfig::from_function(attributes, body);
    assert!(fusion_cfg.expression_body);

    let result = aevia::pipeline::optimize_with_fusion(
        &mut lowerer.builder,
        root,
        &op_reg,
        aevia::pipeline::OptimizeConfig {
            enable_egraph: false,
            ..Default::default()
        },
        fusion_cfg,
    );

    assert_eq!(result.fusion.arithmetic_kernel_count(), 1);
    assert!(
        result.fusion.kernels[0].nodes.len() >= 5,
        "kinetic energy should fuse into one arithmetic kernel"
    );
}

#[test]
fn simplify_fusion_flattens_mul_chain() {
    let mut b = DagBuilder::new();
    let a = b.variable("a");
    let c = b.variable("c");
    let d = b.variable("d");
    let ac = b.mul(a, c);
    let nested = b.mul(ac, d);

    let cfg = FusionConfig {
        simplify_fusion: true,
        ..FusionConfig::default()
    };
    let (flat, plan) = aevia::fusion::apply(&mut b, nested, cfg);
    let k = plan
        .kernels
        .iter()
        .find(|k| k.root == flat)
        .or_else(|| {
            plan.kernels
                .iter()
                .find(|k| k.kind == KernelKind::PureArithmetic)
        })
        .expect("arithmetic kernel");
    assert_eq!(k.kind, KernelKind::PureArithmetic);
    assert!(k.nodes.len() >= 4);
}

#[test]
fn control_flow_splits_kernels() {
    let mut b = DagBuilder::new();
    let cond = b.constant(1.0);
    let then_v = b.variable("t");
    let else_v = b.constant(0.0);
    let root = b.if_else(cond, then_v, else_v);

    let plan = aevia::fusion::partition(b.arena(), root, FusionConfig::default());
    assert!(
        plan.kernels
            .iter()
            .any(|k: &FusionKernel| k.kind == KernelKind::ControlFlow)
    );
}
