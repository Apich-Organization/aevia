//! Bounded pattern-matrix coverage. Rows stay together so field correlations
//! cannot be mistaken for independent coverage. Exhausted work is conservative.

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

use crate::Shape;
use crate::Type;
use crate::adt;
use crate::infer::Infer;
use aevia_syntax::ast::Pat;
use aevia_syntax::ast::PatKind;
use aevia_syntax::ast::PatLit;

const STATES: usize = 8192;
const DEPTH: usize = 128;
#[derive(Clone, Copy)]
enum Node<'a> {
    Any,
    Pattern(&'a Pat),
    Tensor(&'a [Pat], bool),
}
enum Case {
    Ctor(String, Vec<Type>),
    Bool(bool),
    Unit,
    Record(Vec<(String, Type)>),
    Nil,
    Cons(Type, Type),
}
impl Case {
    const fn arity(&self) -> usize {
        match self {
            | Self::Ctor(_, fields) => fields.len(),
            | Self::Record(fields) => fields.len(),
            | Self::Cons(..) => 2,
            | _ => 0,
        }
    }

    fn into_types(self) -> Vec<Type> {
        match self {
            | Self::Ctor(_, fields) => fields,
            | Self::Record(fields) => fields.into_iter().map(|(_, ty)| ty).collect(),
            | Self::Cons(head, tail) => vec![head, tail],
            | _ => Vec::new(),
        }
    }
}
fn normalize(mut node: Node<'_>) -> Node<'_> {
    loop {
        match node {
            | Node::Pattern(pattern) => {
                match &pattern.kind {
                    | PatKind::Var(_) | PatKind::Wildcard => return Node::Any,
                    | PatKind::As { pattern, .. } | PatKind::Annot { pattern, .. } => {
                        node = Node::Pattern(pattern);
                    },
                    | PatKind::Tensor { items, rest } => node = Node::Tensor(items, rest.is_some()),
                    | _ => return node,
                }
            },
            | Node::Tensor([], true) => return Node::Any,
            | _ => return node,
        }
    }
}
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "specialize one row against a constructor"
    )
)]
fn fields<'a>(
    node: Node<'a>,
    case: &Case,
) -> Option<Vec<Node<'a>>> {
    match normalize(node) {
        | Node::Any => Some(vec![Node::Any; case.arity()]),
        | Node::Pattern(pattern) => {
            match (&pattern.kind, case) {
                | (PatKind::Ctor { name, args }, Case::Ctor(expected, types))
                    if name == expected && args.len() == types.len() =>
                {
                    Some(args.iter().map(Node::Pattern).collect())
                },
                | (PatKind::Lit(PatLit::Bool(value)), Case::Bool(expected)) if value == expected => {
                    Some(Vec::new())
                },
                | (PatKind::Lit(PatLit::Unit), Case::Unit) => Some(Vec::new()),
                | (PatKind::Record(fields), Case::Record(expected)) => {
                    Some(
                        expected
                            .iter()
                            .map(|(label, _)| {
                                fields
                                    .iter()
                                    .find(|(name, _)| name == label)
                                    .and_then(|(_, p)| p.as_ref())
                                    .map_or(Node::Any, Node::Pattern)
                            })
                            .collect(),
                    )
                },
                | (PatKind::Tuple(items), Case::Record(expected))
                    if items.len() == expected.len() =>
                {
                    expected
                        .iter()
                        .map(|(name, _)| {
                            let index = name.parse::<usize>().ok()?;
                            items.get(index).map(Node::Pattern)
                        })
                        .collect()
                },
                | _ => None,
            }
        },
        | Node::Tensor([], false) if matches!(case, Case::Nil) => Some(Vec::new()),
        | Node::Tensor([first, tail @ ..], rest) if matches!(case, Case::Cons(..)) => {
            Some(vec![Node::Pattern(first), Node::Tensor(tail, rest)])
        },
        | _ => None,
    }
}
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "enumerate constructors of the first type column"
    )
)]
fn cases(
    ty: &Type,
    infer: &Infer,
) -> Vec<Case> {
    match infer.resolve(ty) {
        | Type::Bool => vec![Case::Bool(false), Case::Bool(true)],
        | Type::Unit => vec![Case::Unit],
        | Type::Adt(id, args) => {
            adt::get(id).map_or_else(Vec::new, |definition| {
                definition
                    .ctors
                    .iter()
                    .map(|ctor| {
                        Case::Ctor(
                            ctor.name.clone(),
                            ctor.fields
                                .iter()
                                .map(|field| {
                                    super::check::instantiate_pattern_field(id, field, &args)
                                })
                                .collect(),
                        )
                    })
                    .collect()
            })
        },
        | Type::Record(row) => vec![Case::Record(row.fields())],
        | Type::Tensor { shape, elem } if shape.rank() == 1 => {
            let length = shape.extent(0);
            let mut cases = Vec::new();
            if length.is_none_or(|n| n == 0) {
                cases.push(Case::Nil);
            }
            if length.is_none_or(|n| n > 0) {
                let tail = Type::tensor(
                    length.map_or(Shape::Rank(1), |n| {
                        Shape::refined([Some(n.saturating_sub(1))])
                    }),
                    (*elem).clone(),
                );
                cases.push(Case::Cons(*elem, tail));
            }
            cases
        },
        | _ => Vec::new(),
    }
}
fn matrix(
    rows: &[Vec<Node<'_>>],
    types: &[Type],
    infer: &Infer,
    budget: &mut usize,
    depth: usize,
) -> bool {
    if rows.is_empty() {
        return false;
    }
    if rows.iter().any(|row| {
        row.len() == types.len() && row.iter().all(|node| matches!(normalize(*node), Node::Any))
    }) {
        return true;
    }
    let Some((ty, tail_types)) = types.split_first() else {
        return true;
    };
    let Some(remaining) = budget.checked_sub(1).filter(|_| depth < DEPTH) else {
        return false;
    };
    *budget = remaining;
    let next_depth = depth.saturating_add(1);
    let alternatives = cases(ty, infer);
    if alternatives.is_empty() {
        // Finite literal arms cannot cover an infinite or opaque domain.
        let defaults = rows
            .iter()
            .filter_map(|row| {
                let (first, tail) = row.split_first()?;
                matches!(normalize(*first), Node::Any).then(|| tail.to_vec())
            })
            .collect::<Vec<_>>();
        return matrix(&defaults, tail_types, infer, budget, next_depth);
    }
    for case in alternatives {
        let mut specialized = Vec::new();
        for (first, tail) in rows.iter().filter_map(|row| row.split_first()) {
            if let Some(mut fields) = fields(*first, &case) {
                fields.extend_from_slice(tail);
                specialized.push(fields);
            }
        }
        let mut fields = case.into_types();
        fields.extend_from_slice(tail_types);
        if !matrix(&specialized, &fields, infer, budget, next_depth) {
            return false;
        }
    }
    true
}
/// Checks whether the patterns cover every value of the scrutinee type.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "perform bounded exhaustiveness checking"
    )
)]
pub fn covers(
    patterns: &[Pat],
    ty: &Type,
    infer: &Infer,
    depth: u32,
) -> bool {
    let rows = patterns
        .iter()
        .map(|pattern| vec![Node::Pattern(pattern)])
        .collect::<Vec<_>>();
    let mut budget = STATES;
    matrix(
        &rows,
        std::slice::from_ref(ty),
        infer,
        &mut budget,
        depth as usize,
    )
}
