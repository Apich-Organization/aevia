//! Kernel fusion partition and fusion-friendly DAG rewrites (plan §10).
//!
//! After e-graph optimization, pure arithmetic subtrees are grouped into
//! [`FusionKernel`] regions. Control flow and function calls form boundaries.
//! Expression-bodied functions and `#[simplify_fusion]` enable associative
//! flattening so mul/add chains fuse into shallower trees for the RSSN JIT.

use crate::ast::{Attribute, FunctionBody};
use rssn_advanced::dag::arena::DagArena;
use rssn_advanced::dag::builder::DagBuilder;
use rssn_advanced::dag::node::DagNodeId;
use rssn_advanced::dag::symbol::{CtrlKind, OpKind, SymbolKind};
use std::collections::{HashMap, HashSet};

/// Per-function fusion hints from attributes and body shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FusionConfig {
    /// `:=` body — eligible for full-expression kernel grouping.
    pub expression_body: bool,
    /// `#[jit_kernel]` — prefer batch / kernel JIT when possible.
    pub jit_kernel: bool,
    /// `#[simplify_fusion]` — aggressive associative flattening.
    pub simplify_fusion: bool,
}

impl Default for FusionConfig {
    fn default() -> Self {
        Self {
            expression_body: false,
            jit_kernel: false,
            simplify_fusion: false,
        }
    }
}

impl FusionConfig {
    /// Build fusion settings from a function's attributes and body.
    #[must_use]
    pub fn from_function(attributes: &[Attribute], body: &FunctionBody) -> Self {
        let expression_body = matches!(body, FunctionBody::Expression(_));
        let jit_kernel = has_attr(attributes, "jit_kernel");
        let simplify_fusion = has_attr(attributes, "simplify_fusion");
        Self {
            expression_body,
            jit_kernel,
            simplify_fusion,
        }
    }

    /// Whether to run associative chain flattening before partitioning.
    #[must_use]
    pub fn flatten_chains(&self) -> bool {
        self.expression_body || self.simplify_fusion
    }
}

fn has_attr(attributes: &[Attribute], name: &str) -> bool {
    attributes.iter().any(|a| a.name == name)
}

/// Kind of fused region in the DAG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelKind {
    /// Maximal subtree of operators with constant/variable leaves.
    PureArithmetic,
    /// Custom or builtin function application.
    Function,
    /// `if_else`, `select`, or `for_loop`.
    ControlFlow,
    /// Standalone constant or variable (only when not inside a larger kernel).
    Leaf,
}

/// One fused kernel: a contiguous region of the DAG rooted at `root`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FusionKernel {
    pub index: usize,
    pub root: DagNodeId,
    pub kind: KernelKind,
    /// All DAG nodes belonging to this kernel (post-order, duplicates omitted).
    pub nodes: Vec<DagNodeId>,
}

/// Partition of a function body DAG into kernels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FusionPlan {
    pub kernels: Vec<FusionKernel>,
    pub config: FusionConfig,
}

impl FusionPlan {
    #[must_use]
    pub fn arithmetic_kernel_count(&self) -> usize {
        self.kernels
            .iter()
            .filter(|k| k.kind == KernelKind::PureArithmetic)
            .count()
    }

    #[must_use]
    pub fn total_nodes(&self) -> usize {
        self.kernels.iter().map(|k| k.nodes.len()).sum()
    }
}

/// Partition `root` into fusion kernels without mutating the DAG.
#[must_use]
pub fn partition(arena: &DagArena, root: DagNodeId, config: FusionConfig) -> FusionPlan {
    let mut kernels = Vec::new();
    let mut visited = HashSet::new();
    partition_walk(arena, root, &mut kernels, &mut visited);
    for (i, k) in kernels.iter_mut().enumerate() {
        k.index = i;
    }
    FusionPlan { kernels, config }
}

fn partition_walk(
    arena: &DagArena,
    id: DagNodeId,
    kernels: &mut Vec<FusionKernel>,
    visited: &mut HashSet<u32>,
) {
    if id.is_none() || visited.contains(&id.value()) {
        return;
    }
    let Some(node) = arena.get(id) else {
        return;
    };

    match node.kind {
        SymbolKind::ControlFlow(ctrl) => {
            visited.insert(id.value());
            kernels.push(FusionKernel {
                index: kernels.len(),
                root: id,
                kind: KernelKind::ControlFlow,
                nodes: vec![id],
            });
            for &child in node.children.as_slice() {
                partition_walk(arena, child, kernels, visited);
            }
            let _ = ctrl;
        }
        SymbolKind::Function(_) => {
            visited.insert(id.value());
            let mut nodes = vec![id];
            for &child in node.children.as_slice() {
                collect_if_leaf_or_boundary(arena, child, &mut nodes, visited);
            }
            kernels.push(FusionKernel {
                index: kernels.len(),
                root: id,
                kind: KernelKind::Function,
                nodes,
            });
            for &child in node.children.as_slice() {
                partition_walk(arena, child, kernels, visited);
            }
        }
        SymbolKind::Operator(_) => {
            let nodes = collect_pure_arithmetic_nodes(arena, id);
            for n in &nodes {
                visited.insert(n.value());
            }
            kernels.push(FusionKernel {
                index: kernels.len(),
                root: id,
                kind: KernelKind::PureArithmetic,
                nodes,
            });
        }
        SymbolKind::Constant(_) | SymbolKind::Variable(_) => {
            if visited.insert(id.value()) {
                kernels.push(FusionKernel {
                    index: kernels.len(),
                    root: id,
                    kind: KernelKind::Leaf,
                    nodes: vec![id],
                });
            }
        }
    }
}

fn collect_if_leaf_or_boundary(
    arena: &DagArena,
    id: DagNodeId,
    nodes: &mut Vec<DagNodeId>,
    visited: &mut HashSet<u32>,
) {
    if id.is_none() || visited.contains(&id.value()) {
        return;
    }
    let Some(node) = arena.get(id) else {
        return;
    };
    match node.kind {
        SymbolKind::Constant(_) | SymbolKind::Variable(_) => {
            if visited.insert(id.value()) {
                nodes.push(id);
            }
        }
        _ => {}
    }
}

/// Collect all nodes in the maximal pure-arithmetic subtree at `root`.
fn collect_pure_arithmetic_nodes(arena: &DagArena, root: DagNodeId) -> Vec<DagNodeId> {
    let mut nodes = Vec::new();
    let mut stack = vec![root];
    let mut seen = HashSet::new();
    while let Some(id) = stack.pop() {
        if id.is_none() || !seen.insert(id.value()) {
            continue;
        }
        let Some(node) = arena.get(id) else {
            continue;
        };
        match node.kind {
            SymbolKind::Operator(_) | SymbolKind::Constant(_) | SymbolKind::Variable(_) => {
                nodes.push(id);
                for &child in node.children.as_slice() {
                    stack.push(child);
                }
            }
            _ => {}
        }
    }
    nodes
}

/// Flatten nested `+` and `*` chains so the JIT can fuse mul-add patterns.
///
/// Rebuilds the subgraph at `root` in `builder` (structural sharing preserved
/// where subtrees are unchanged).
pub fn flatten_associative_chains(
    builder: &mut DagBuilder,
    root: DagNodeId,
    aggressive: bool,
) -> DagNodeId {
    if root.is_none() {
        return root;
    }
    let arena = builder.arena().clone();
    let mut cache = HashMap::new();
    rebuild_flattened(builder, &arena, root, aggressive, &mut cache);
    cache
        .get(&root.value())
        .copied()
        .unwrap_or(root)
}

fn rebuild_flattened(
    builder: &mut DagBuilder,
    arena: &DagArena,
    id: DagNodeId,
    aggressive: bool,
    cache: &mut HashMap<u32, DagNodeId>,
) -> DagNodeId {
    if let Some(&cached) = cache.get(&id.value()) {
        return cached;
    }
    let Some(node) = arena.get(id) else {
        return id;
    };

    let new_id = match node.kind {
        SymbolKind::Constant(v) => builder.constant(v),
        SymbolKind::Variable(sym) => {
            let name = builder.registry().name(sym).map(str::to_owned);
            match name {
                Some(n) => builder.variable(&n),
                None => id,
            }
        }
        SymbolKind::Operator(OpKind::Add) => {
            let operands = collect_bin_operands(arena, id, OpKind::Add);
            let rebuilt: Vec<DagNodeId> = operands
                .iter()
                .map(|&o| rebuild_flattened(builder, arena, o, aggressive, cache))
                .collect();
            builder.add_many(&rebuilt).unwrap_or(id)
        }
        SymbolKind::Operator(OpKind::Mul) if aggressive => {
            let operands = collect_bin_operands(arena, id, OpKind::Mul);
            let rebuilt: Vec<DagNodeId> = operands
                .iter()
                .map(|&o| rebuild_flattened(builder, arena, o, aggressive, cache))
                .collect();
            builder.mul_many(&rebuilt).unwrap_or(id)
        }
        SymbolKind::Operator(op) => {
            let children: Vec<DagNodeId> = node
                .children
                .as_slice()
                .iter()
                .map(|&c| rebuild_flattened(builder, arena, c, aggressive, cache))
                .collect();
            match (op, children.as_slice()) {
                (OpKind::Neg, [a]) => builder.neg(*a),
                (OpKind::Add, [a, b]) => builder.add(*a, *b),
                (OpKind::Sub, [a, b]) => builder.sub(*a, *b),
                (OpKind::Mul, [a, b]) => builder.mul(*a, *b),
                (OpKind::Div, [a, b]) => builder.div(*a, *b),
                (OpKind::Pow, [a, b]) => builder.pow(*a, *b),
                (OpKind::Mod, [a, b]) => builder.modulo(*a, *b),
                _ => id,
            }
        }
        SymbolKind::Function(fn_id) => {
            let children: Vec<DagNodeId> = node
                .children
                .as_slice()
                .iter()
                .map(|&c| rebuild_flattened(builder, arena, c, aggressive, cache))
                .collect();
            builder.function_call(fn_id, &children)
        }
        SymbolKind::ControlFlow(ctrl) => {
            let children: Vec<DagNodeId> = node
                .children
                .as_slice()
                .iter()
                .map(|&c| rebuild_flattened(builder, arena, c, aggressive, cache))
                .collect();
            match ctrl {
                CtrlKind::Select => builder.select(children[0], children[1], children[2]),
                CtrlKind::IfElse => builder.if_else(children[0], children[1], children[2]),
                CtrlKind::ForLoop => {
                    builder.for_loop(children[0], children[1], children[2], children[3])
                }
            }
        }
    };

    cache.insert(id.value(), new_id);
    new_id
}

fn collect_bin_operands(arena: &DagArena, id: DagNodeId, op: OpKind) -> Vec<DagNodeId> {
    let mut out = Vec::new();
    let mut stack = vec![id];
    while let Some(cur) = stack.pop() {
        let Some(node) = arena.get(cur) else {
            continue;
        };
        if node.kind == SymbolKind::Operator(op) && node.children.len() == 2 {
            let [a, b] = node.children.as_slice() else {
                out.push(cur);
                continue;
            };
            stack.push(*b);
            stack.push(*a);
        } else {
            out.push(cur);
        }
    }
    out
}

/// Apply fusion flattening (when configured) and return the partition plan.
pub fn apply(
    builder: &mut DagBuilder,
    root: DagNodeId,
    config: FusionConfig,
) -> (DagNodeId, FusionPlan) {
    let root = if config.flatten_chains() {
        flatten_associative_chains(builder, root, config.simplify_fusion)
    } else {
        root
    };
    let plan = partition(builder.arena(), root, config);
    (root, plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rssn_advanced::dag::builder::DagBuilder;

    #[test]
    fn partition_single_arithmetic_kernel() {
        let mut b = DagBuilder::new();
        let x = b.variable("x");
        let y = b.variable("y");
        let prod = b.mul(x, y);
        let one = b.constant(1.0);
        let root = b.add(prod, one);

        let plan = partition(b.arena(), root, FusionConfig::default());
        assert_eq!(plan.arithmetic_kernel_count(), 1);
        assert!(plan.kernels[0].nodes.len() >= 4);
    }

    #[test]
    fn flatten_add_chain() {
        let mut b = DagBuilder::new();
        let a = b.variable("a");
        let c = b.variable("c");
        let d = b.variable("d");
        let inner = b.add(a, c);
        let root = b.add(inner, d);

        let flat = flatten_associative_chains(&mut b, root, false);
        let plan = partition(b.arena(), flat, FusionConfig::default());
        let k = plan
            .kernels
            .iter()
            .find(|k| k.kind == KernelKind::PureArithmetic)
            .unwrap();
        assert!(k.nodes.len() >= 4);
    }

    #[test]
    fn partition_splits_control_flow() {
        let mut b = DagBuilder::new();
        let cond = b.constant(1.0);
        let then_v = b.variable("t");
        let else_v = b.constant(0.0);
        let root = b.if_else(cond, then_v, else_v);

        let plan = partition(b.arena(), root, FusionConfig::default());
        assert!(plan.kernels.iter().any(|k| k.kind == KernelKind::ControlFlow));
        assert!(plan.kernels.iter().any(|k| k.kind == KernelKind::Leaf));
    }

    #[test]
    fn config_from_attributes() {
        use crate::ast::Attribute;
        let cfg = FusionConfig::from_function(
            &[Attribute {
                name: "jit_kernel".into(),
                args: vec![],
            }],
            &FunctionBody::Expression(crate::ast::Spanned::new(
                crate::ast::Expr::Literal {
                    value: rust_decimal::Decimal::ONE,
                    suffix: None,
                },
                miette::SourceSpan::new(0.into(), 1),
            )),
        );
        assert!(cfg.jit_kernel);
        assert!(cfg.expression_body);
        assert!(cfg.flatten_chains());
    }
}
