//! One iterative, transactional matcher for source and Core IR patterns.
//! A failed match never commits bindings to a lexical environment or slot frame.
use crate::value::NumKind;
use crate::value::TensorData;
use crate::value::Value;
use aevia_syntax::ast::Pat;
use aevia_syntax::ast::PatKind;
use aevia_syntax::ast::PatLit;
use aevia_types::adt::AdtId;
use aevia_types::adt::{
    self,
};
use num_rational::BigRational;
use std::rc::Rc;

#[derive(Clone, Copy)]
enum Node<'a> {
    Ast(&'a Pat),
    Ir(&'a aevia_ir::Pattern),
}
enum Target<'a> {
    Name(&'a str),
    Slot(u32),
}
enum Work<'a> {
    Match(Node<'a>, Value),
    Bind(Target<'a>, Value),
}
pub(crate) enum Binding {
    Name(String, Value),
    Slot(u32, Value),
}

pub(crate) fn ast(
    pattern: &Pat,
    value: &Value,
    constructors: &crate::evidence::SpanMap<(AdtId, usize)>,
) -> Option<Vec<Binding>> {
    execute(Node::Ast(pattern), value, constructors)
}
pub(crate) fn ir(
    pattern: &aevia_ir::Pattern,
    value: &Value,
) -> Option<Vec<Binding>> {
    execute(Node::Ir(pattern), value, &Default::default())
}
fn constructor(
    value: &Value,
    name: &str,
    expected: Option<(AdtId, usize)>,
    arity: usize,
) -> bool {
    let (Value::Variant(value), Some((id, index))) = (value, expected) else {
        return false;
    };
    adt::same_type(value.adt, id)
        && value.index == index
        && value.name == name
        && value.fields.len() == arity
}
fn literal(
    literal: &PatLit,
    value: &Value,
) -> bool {
    match (literal, value) {
        | (PatLit::Int(expected), Value::Num(number)) => {
            match &number.kind {
                | NumKind::Int(actual) => actual == expected,
                | NumKind::Exact(actual) => actual == &BigRational::from_integer(expected.clone()),
                | NumKind::Float(actual) => {
                    BigRational::from_float(*actual)
                        == Some(BigRational::from_integer(expected.clone()))
                },
                | _ => false,
            }
        },
        | (PatLit::Str(expected), Value::Str(actual)) => actual.as_ref() == expected,
        | (PatLit::Bool(expected), Value::Bool(actual)) => expected == actual,
        | (PatLit::Unit, Value::Unit) => true,
        | _ => false,
    }
}
fn tensor(
    value: &Value,
    count: usize,
    rest: bool,
) -> Option<&Rc<TensorData>> {
    let Value::Tensor(tensor) = value else {
        return None;
    };
    if tensor.rank() != 1
        || tensor.shape[0] != tensor.storage.len()
        || if rest {
            tensor.shape[0] < count
        } else {
            tensor.shape[0] != count
        }
    {
        return None;
    }
    Some(tensor)
}
fn execute<'a>(
    node: Node<'a>,
    value: &Value,
    constructors: &crate::evidence::SpanMap<(AdtId, usize)>,
) -> Option<Vec<Binding>> {
    let mut pending = vec![Work::Match(node, value.clone())];
    let mut bindings = Vec::new();
    while let Some(work) = pending.pop() {
        let (node, value) = match work {
            | Work::Bind(target, value) => {
                bindings.push(match target {
                    | Target::Name(name) => Binding::Name(name.to_string(), value),
                    | Target::Slot(slot) => Binding::Slot(slot, value),
                });
                continue;
            },
            | Work::Match(node, value) => (node, value),
        };
        match node {
            | Node::Ast(pattern) => {
                match &pattern.kind {
                    | PatKind::Error => return None,
                    | PatKind::Wildcard => {},
                    | PatKind::Var(name) => pending.push(Work::Bind(Target::Name(name), value)),
                    | PatKind::Lit(expected) => {
                        if !literal(expected, &value) {
                            return None;
                        }
                    },
                    | PatKind::Ctor { name, args } => {
                        if !constructor(
                            &value,
                            name,
                            constructors.get(&pattern.span).copied(),
                            args.len(),
                        ) {
                            return None;
                        }
                        let Value::Variant(value) = value else {
                            unreachable!()
                        };
                        for (pattern, field) in args.iter().zip(&value.fields).rev() {
                            pending.push(Work::Match(Node::Ast(pattern), field.clone()));
                        }
                    },
                    | PatKind::Record(fields) => {
                        let Value::Record(value) = value else {
                            return None;
                        };
                        for (name, pattern) in fields.iter().rev() {
                            let field = value.fields.get(name)?.clone();
                            pending.push(match pattern {
                                | Some(pattern) => Work::Match(Node::Ast(pattern), field),
                                | None => Work::Bind(Target::Name(name), field),
                            });
                        }
                    },
                    | PatKind::Tuple(items) => {
                        let Value::Record(value) = value else {
                            return None;
                        };
                        if value.fields.len() != items.len() {
                            return None;
                        }
                        for (index, pattern) in items.iter().enumerate().rev() {
                            pending.push(Work::Match(
                                Node::Ast(pattern),
                                value.fields.get(&index.to_string())?.clone(),
                            ));
                        }
                    },
                    | PatKind::Tensor { items, rest } => {
                        let tensor = tensor(&value, items.len(), rest.is_some())?;
                        if let Some(name) = rest {
                            pending.push(Work::Bind(
                                Target::Name(name),
                                Value::Tensor(tensor.tail_view(items.len())?),
                            ));
                        }
                        for (index, pattern) in items.iter().enumerate().rev() {
                            pending.push(Work::Match(Node::Ast(pattern), tensor.get_flat(index)?));
                        }
                    },
                    | PatKind::As { pattern, name } => {
                        pending.push(Work::Bind(Target::Name(name), value.clone()));
                        pending.push(Work::Match(Node::Ast(pattern), value));
                    },
                    | PatKind::Annot { pattern, .. } => {
                        pending.push(Work::Match(Node::Ast(pattern), value))
                    },
                }
            },
            | Node::Ir(pattern) => {
                match pattern {
                    | aevia_ir::Pattern::Error => return None,
                    | aevia_ir::Pattern::Wildcard => {},
                    | aevia_ir::Pattern::Bind { slot, inner } => {
                        pending.push(Work::Bind(Target::Slot(*slot), value.clone()));
                        if let Some(inner) = inner {
                            pending.push(Work::Match(Node::Ir(inner), value));
                        }
                    },
                    | aevia_ir::Pattern::Lit(expected) => {
                        if !literal(expected, &value) {
                            return None;
                        }
                    },
                    | aevia_ir::Pattern::Ctor { name, identity, args } => {
                        if !constructor(&value, name, *identity, args.len()) {
                            return None;
                        }
                        let Value::Variant(value) = value else {
                            unreachable!()
                        };
                        for (pattern, field) in args.iter().zip(&value.fields).rev() {
                            pending.push(Work::Match(Node::Ir(pattern), field.clone()));
                        }
                    },
                    | aevia_ir::Pattern::Record(fields) => {
                        let Value::Record(value) = value else {
                            return None;
                        };
                        for (name, pattern) in fields.iter().rev() {
                            pending.push(Work::Match(
                                Node::Ir(pattern),
                                value.fields.get(name)?.clone(),
                            ));
                        }
                    },
                    | aevia_ir::Pattern::Tuple(items) => {
                        let Value::Record(value) = value else {
                            return None;
                        };
                        if value.fields.len() != items.len() {
                            return None;
                        }
                        for (index, pattern) in items.iter().enumerate().rev() {
                            pending.push(Work::Match(
                                Node::Ir(pattern),
                                value.fields.get(&index.to_string())?.clone(),
                            ));
                        }
                    },
                    | aevia_ir::Pattern::Tensor { items, rest } => {
                        let tensor = tensor(&value, items.len(), rest.is_some())?;
                        if let Some(slot) = rest {
                            pending.push(Work::Bind(
                                Target::Slot(*slot),
                                Value::Tensor(tensor.tail_view(items.len())?),
                            ));
                        }
                        for (index, pattern) in items.iter().enumerate().rev() {
                            pending.push(Work::Match(Node::Ir(pattern), tensor.get_flat(index)?));
                        }
                    },
                }
            },
        }
    }
    Some(bindings)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deeply_nested_bind_patterns_use_an_explicit_work_stack() {
        let mut pattern = aevia_ir::Pattern::Wildcard;
        for slot in 0..10_000 {
            pattern = aevia_ir::Pattern::Bind {
                slot,
                inner: Some(Box::new(pattern)),
            };
        }
        let bindings = ir(&pattern, &Value::Unit).unwrap();
        assert_eq!(bindings.len(), 10_000);
        // The matcher is iterative. Drop the synthetic host-created pattern
        // iteratively too; source syntax has a separate parser depth contract.
        while let aevia_ir::Pattern::Bind {
            inner: Some(inner), ..
        } = pattern
        {
            pattern = *inner;
        }
    }
}
