//! Bounded pattern-matrix coverage. Rows stay together so field correlations
//! cannot be mistaken for independent coverage. Exhausted work is conservative.
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
    fn types(&self) -> Vec<Type> {
        match self {
            | Self::Ctor(_, fields) => fields.clone(),
            | Self::Record(fields) => fields.iter().map(|(_, ty)| ty.clone()).collect(),
            | Self::Cons(head, tail) => vec![head.clone(), tail.clone()],
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
                        node = Node::Pattern(pattern)
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
fn fields<'a>(
    node: Node<'a>,
    case: &Case,
) -> Option<Vec<Node<'a>>> {
    match normalize(node) {
        | Node::Any => Some(vec![Node::Any; case.types().len()]),
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
                    if items.len() == expected.len()
                        && expected.iter().all(|(name, _)| {
                            name.parse::<usize>().is_ok_and(|index| index < items.len())
                        }) =>
                {
                    Some(
                        expected
                            .iter()
                            .map(|(name, _)| {
                                Node::Pattern(
                                    &items[name.parse::<usize>().expect("checked tuple label")],
                                )
                            })
                            .collect(),
                    )
                },
                | _ => None,
            }
        },
        | Node::Tensor([], false) if matches!(case, Case::Nil) => Some(Vec::new()),
        | Node::Tensor(items, rest) if !items.is_empty() && matches!(case, Case::Cons(..)) => {
            Some(vec![
                Node::Pattern(&items[0]),
                Node::Tensor(&items[1..], rest),
            ])
        },
        | _ => None,
    }
}
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
                    length.map_or(Shape::Rank(1), |n| Shape::refined(vec![Some(n - 1)])),
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
    if rows
        .iter()
        .any(|row| row.iter().all(|node| matches!(normalize(*node), Node::Any)))
    {
        return true;
    }
    if types.is_empty() {
        return true;
    }
    if *budget == 0 || depth >= DEPTH {
        return false;
    }
    *budget -= 1;
    let alternatives = cases(&types[0], infer);
    if alternatives.is_empty() {
        // Finite literal arms cannot cover an infinite or opaque domain.
        let defaults = rows
            .iter()
            .filter(|row| matches!(normalize(row[0]), Node::Any))
            .map(|row| row[1..].to_vec())
            .collect::<Vec<_>>();
        return matrix(&defaults, &types[1..], infer, budget, depth + 1);
    }
    for case in alternatives {
        let mut specialized = Vec::new();
        for row in rows {
            if let Some(mut fields) = fields(row[0], &case) {
                fields.extend_from_slice(&row[1..]);
                specialized.push(fields);
            }
        }
        let mut fields = case.types();
        fields.extend_from_slice(&types[1..]);
        if !matrix(&specialized, &fields, infer, budget, depth + 1) {
            return false;
        }
    }
    true
}
pub(crate) fn covers(
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
