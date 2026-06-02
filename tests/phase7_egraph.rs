//! Phase 7: e-graph integration with custom-op rules.

use rssn_advanced::custom::descriptor::{CustomOpDescriptor, CustomOpRegistry, EvalFn};
use rssn_advanced::dag::builder::DagBuilder;
use rssn_advanced::dag::symbol::SymbolKind;
use rssn_advanced::egraph::egraph::EGraphConfig;

extern "C" fn id(x: f64) -> f64 {
    x
}

#[test]
fn pipeline_optimize_applies_egraph() {
    let src = r#"
        pub op wrap(x: m -> m) {
            egraph { rewrite wrap(y) => y }
        }
    "#;
    let file = aevia::parser::items::parse_source(src).unwrap();
    let aevia::ast::Item::CustomOp { name, properties, .. } = &file.items[0].node else {
        panic!("expected op");
    };

    let mut dag = DagBuilder::new();
    let fn_id = dag.intern_function(name);
    let desc = aevia::ops::egraph::attach_egraph_rules(
        name,
        fn_id,
        &properties.egraph_rules,
        CustomOpDescriptor::builder(fn_id, name, EvalFn::Arity1(id)),
    )
    .build();
    let mut reg = CustomOpRegistry::new();
    reg.register(desc).unwrap();

    let x = dag.variable("t");
    let call = dag.function_call(fn_id, &[x]);

    let (best, stats) = aevia::pipeline::optimize(
        &mut dag,
        call,
        &reg,
        aevia::pipeline::OptimizeConfig {
            enable_egraph: true,
            egraph: EGraphConfig {
                max_rounds: 8,
                ..Default::default()
            },
        },
    );
    assert!(stats.is_some());
    assert_eq!(best, x);
}

#[test]
fn integration_physics_op_has_egraph_rules() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/integration/src/modules/physics.ae");
    let file = aevia::parser::items::parse_source(
        &std::fs::read_to_string(&path).unwrap(),
    )
    .unwrap();
    let aevia::ast::Item::CustomOp { properties, .. } = file
        .items
        .iter()
        .find(|i| matches!(i.node, aevia::ast::Item::CustomOp { .. }))
        .map(|i| &i.node)
        .expect("scale_velocity op") else {
        panic!();
    };
    assert!(!properties.egraph_rules.is_empty());
}
