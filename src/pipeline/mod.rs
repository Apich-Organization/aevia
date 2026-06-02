//! JIT pipeline passes between lowering and code generation. Phase 6–8.

use crate::fusion::{self, FusionConfig, FusionPlan};
use rssn_advanced::custom::descriptor::CustomOpRegistry;
use rssn_advanced::dag::builder::DagBuilder;
use rssn_advanced::dag::node::DagNodeId;
use rssn_advanced::egraph::egraph::{EGraph, EGraphConfig};
use rssn_advanced::heuristic::{HeuristicConfig, HeuristicEngine, SearchStrategy};
use std::sync::Arc;

/// Statistics from the e-graph saturation pass (for logging / diagnostics).
#[derive(Debug, Clone, Copy, Default)]
pub struct EgraphStats {
    pub merges_performed: usize,
    pub rounds_completed: usize,
    pub converged: bool,
}

/// Configuration for the full optimize pipeline (heuristic + e-graph).
#[derive(Debug, Clone, Copy)]
pub struct OptimizeConfig {
    pub egraph: EGraphConfig,
    /// When false, only heuristic simplification runs (custom-op simplify rules).
    pub enable_egraph: bool,
}

impl Default for OptimizeConfig {
    fn default() -> Self {
        Self {
            egraph: EGraphConfig::default(),
            enable_egraph: true,
        }
    }
}

/// Run heuristic simplification using custom-op simplify rules.
pub fn simplify(
    builder: &mut DagBuilder,
    root: DagNodeId,
    op_reg: &CustomOpRegistry,
) -> DagNodeId {
    let rule_registry = op_reg.build_rule_registry();
    let mut engine = HeuristicEngine::new(HeuristicConfig::default(), SearchStrategy::Greedy)
        .with_rule_registry(Arc::new(rule_registry));
    engine.simplify(builder, root)
}

/// Equality saturation with built-in RSSN rules plus custom-op e-graph rules.
pub fn saturate_egraph(
    builder: &mut DagBuilder,
    root: DagNodeId,
    op_reg: &CustomOpRegistry,
    cfg: EGraphConfig,
) -> (DagNodeId, EgraphStats) {
    let mut eg = EGraph::new(builder, cfg);
    op_reg.apply_to_egraph(&mut eg);
    eg.saturate(root);
    let best = eg.extract(root);
    let stats = EgraphStats {
        merges_performed: eg.merges_performed,
        rounds_completed: eg.rounds_completed,
        converged: eg.converged,
    };
    (best, stats)
}

/// Heuristic simplify followed by optional e-graph saturation + extraction.
pub fn optimize(
    builder: &mut DagBuilder,
    root: DagNodeId,
    op_reg: &CustomOpRegistry,
    config: OptimizeConfig,
) -> (DagNodeId, Option<EgraphStats>) {
    let root = simplify(builder, root, op_reg);
    if config.enable_egraph {
        let (root, stats) = saturate_egraph(builder, root, op_reg, config.egraph);
        (root, Some(stats))
    } else {
        (root, None)
    }
}

/// Result of the full optimize + kernel-fusion pipeline.
#[derive(Debug, Clone)]
pub struct PipelineResult {
    pub root: DagNodeId,
    pub egraph: Option<EgraphStats>,
    pub fusion: FusionPlan,
}

/// Simplify, optional e-graph saturation, then kernel fusion partition.
pub fn optimize_with_fusion(
    builder: &mut DagBuilder,
    root: DagNodeId,
    op_reg: &CustomOpRegistry,
    opt_config: OptimizeConfig,
    fusion_config: FusionConfig,
) -> PipelineResult {
    let (root, egraph) = optimize(builder, root, op_reg, opt_config);
    let (root, fusion) = fusion::apply(builder, root, fusion_config);
    PipelineResult {
        root,
        egraph,
        fusion,
    }
}
