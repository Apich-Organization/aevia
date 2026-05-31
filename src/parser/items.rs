//! Aevia top-level item and statement parser (Milestone 1b).
//!
//! Parses the full Aevia grammar:
//!   - `use` imports
//!   - `type` aliases
//!   - `struct` declarations
//!   - `fn` / expression-bodied (`:=`) functions
//!   - `pub op` custom operator declarations
//!   - `let` / `let mut` bindings
//!   - Block-bodied function statements
//!
//! Every node is annotated with a `miette::SourceSpan` via `Spanned<T>`.

use crate::ast::{
    Attribute, DimExpr, Field, FunctionBody, Item, OpProperties,
    Param, SourceFile, Spanned, Stmt, Visibility,
};
use crate::parser::expr::{dim_expr_parser, expr_parser};
use chumsky::prelude::*;
use miette::SourceSpan;

// ── Span helpers ──────────────────────────────────────────────────────────────

fn to_src(span: SimpleSpan) -> SourceSpan {
    SourceSpan::new(span.start.into(), span.end - span.start)
}

// ── Shared primitives ─────────────────────────────────────────────────────────

/// Any run of whitespace (spaces + newlines).
fn ws<'a>() -> impl Parser<'a, &'a str, (), extra::Err<Simple<'a, char>>> + Clone {
    any().filter(|c: &char| c.is_whitespace()).repeated().ignored()
}

/// A Rust-style identifier.
fn ident<'a>() -> impl Parser<'a, &'a str, String, extra::Err<Simple<'a, char>>> + Clone {
    any()
        .filter(|c: &char| c.is_alphabetic() || *c == '_')
        .then(
            any()
                .filter(|c: &char| c.is_alphanumeric() || *c == '_')
                .repeated()
                .collect::<Vec<char>>(),
        )
        .map(|(first, rest): (char, Vec<char>)| {
            let mut s = String::new();
            s.push(first);
            s.extend(rest);
            s
        })
}

/// Keyword: exact string followed by a non-alphanumeric boundary.
fn kw<'a>(word: &'static str) -> impl Parser<'a, &'a str, (), extra::Err<Simple<'a, char>>> + Clone {
    just(word)
        .then_ignore(
            any()
                .filter(|c: &char| c.is_alphanumeric() || *c == '_')
                .not(),
        )
        .ignored()
}

/// Visibility prefix: `pub(crate)`, `pub`, or nothing (Private).
fn visibility<'a>() -> impl Parser<'a, &'a str, Visibility, extra::Err<Simple<'a, char>>> + Clone {
    choice((
        just("pub(crate)").padded().to(Visibility::Crate),
        kw("pub").padded().to(Visibility::Public),
    ))
    .or_not()
    .map(|v| v.unwrap_or(Visibility::Private))
}

/// `#[attr_name]` or `#[attr_name(arg1, arg2)]`.
fn attribute<'a>() -> impl Parser<'a, &'a str, Attribute, extra::Err<Simple<'a, char>>> + Clone {
    just('#')
        .ignore_then(just('[').padded())
        .ignore_then(ident())
        .then(
            just('(')
                .padded()
                .ignore_then(
                    ident()
                        .padded()
                        .separated_by(just(',').padded())
                        .collect::<Vec<_>>(),
                )
                .then_ignore(just(')').padded())
                .or_not(),
        )
        .then_ignore(just(']').padded())
        .map(|(name, args)| Attribute {
            name,
            args: args.unwrap_or_default(),
        })
}

// ── Dimension expressions ─────────────────────────────────────────────────────

/// Wraps the expression-level dim parser for use in top-level grammar.
fn dim<'a>() -> impl Parser<'a, &'a str, Spanned<DimExpr>, extra::Err<Simple<'a, char>>> + Clone {
    dim_expr_parser()
}

// ── Statements ────────────────────────────────────────────────────────────────

/// `let [mut] name [: DimType] = expr;`
fn let_stmt<'a>() -> impl Parser<'a, &'a str, Spanned<Stmt>, extra::Err<Simple<'a, char>>> + Clone {
    let is_mut = kw("mut").padded().or_not().map(|m| m.is_some());

    kw("let")
        .padded()
        .ignore_then(is_mut)
        .then(ident().padded())
        .then(just(':').padded().ignore_then(dim()).or_not())
        .then_ignore(just('=').padded())
        .then(expr_parser().padded())
        .then_ignore(just(';').padded())
        .map_with(|(((is_mut, name), declared_type), init), e| {
            Spanned::new(
                Stmt::Let {
                    name,
                    declared_type,
                    init,
                    is_mut,
                },
                to_src(e.span()),
            )
        })
}

/// `name = expr;` (re-assignment)
fn assign_stmt<'a>() -> impl Parser<'a, &'a str, Spanned<Stmt>, extra::Err<Simple<'a, char>>> + Clone {
    ident()
        .padded()
        .then_ignore(just('=').padded())
        .then(expr_parser().padded())
        .then_ignore(just(';').padded())
        .map_with(|(name, value), e| Spanned::new(Stmt::Assign { target: name, value }, to_src(e.span())))
}

/// `break;`
fn break_stmt<'a>() -> impl Parser<'a, &'a str, Spanned<Stmt>, extra::Err<Simple<'a, char>>> + Clone {
    kw("break")
        .padded()
        .then_ignore(just(';').padded())
        .map_with(|_, e| Spanned::new(Stmt::Break, to_src(e.span())))
}

/// Any statement inside a block body.
fn stmt<'a>() -> impl Parser<'a, &'a str, Spanned<Stmt>, extra::Err<Simple<'a, char>>> + Clone {
    choice((
        let_stmt(),
        break_stmt(),
        assign_stmt(),
        // Bare expression statement (trailing `;`)
        expr_parser().padded().then_ignore(just(';').padded()).map_with(|expr, e| {
            Spanned::new(Stmt::Expr(expr), to_src(e.span()))
        }),
    ))
}

/// A `{ stmt* }` block body.
fn block_body<'a>() -> impl Parser<'a, &'a str, Vec<Spanned<Stmt>>, extra::Err<Simple<'a, char>>> + Clone {
    just('{')
        .padded()
        .ignore_then(stmt().repeated().collect::<Vec<_>>())
        .then_ignore(just('}').padded())
}

// ── Function parameter / field ────────────────────────────────────────────────

/// `name: DimType`
fn param<'a>() -> impl Parser<'a, &'a str, Param, extra::Err<Simple<'a, char>>> + Clone {
    ident()
        .padded()
        .then_ignore(just(':').padded())
        .then(dim())
        .map(|(name, dim)| Param { name, dim })
}

/// `[pub] name: DimType`
fn field<'a>() -> impl Parser<'a, &'a str, Field, extra::Err<Simple<'a, char>>> + Clone {
    visibility()
        .then(ident().padded())
        .then_ignore(just(':').padded())
        .then(dim())
        .map(|((vis, name), dim)| Field {
            name,
            dim,
            visibility: vis,
        })
}

// ── Op properties block ───────────────────────────────────────────────────────

/// Parses the `properties { ... }` block for `pub op` declarations.
/// Supports: `vectorizable = true/false`, `commutative`, `associative`,
/// `cost = <float>`, `simplify { ... }`, `egraph { ... }`.
fn op_properties<'a>() -> impl Parser<'a, &'a str, OpProperties, extra::Err<Simple<'a, char>>> + Clone {
    just('{')
        .padded()
        // We accept any non-brace content for now and return defaults.
        // TODO(Phase 2): parse individual property lines.
        .ignore_then(
            any()
                .filter(|c: &char| *c != '}')
                .repeated()
                .ignored(),
        )
        .then_ignore(just('}').padded())
        .map(|_| OpProperties {
            vectorizable: false,
            commutative: false,
            associative: false,
            cost: None,
            simplify_rules: Vec::new(),
            egraph_rules: Vec::new(),
        })
}

// ── Top-level items ───────────────────────────────────────────────────────────

/// `use path::to::item [as Alias];`
fn use_item<'a>() -> impl Parser<'a, &'a str, Spanned<Item>, extra::Err<Simple<'a, char>>> + Clone {
    kw("use")
        .padded()
        .ignore_then(
            ident()
                .separated_by(just("::"))
                .at_least(1)
                .collect::<Vec<_>>(),
        )
        .then(kw("as").padded().ignore_then(ident()).or_not())
        .then_ignore(just(';').padded())
        .map_with(|(path, alias), e| Spanned::new(Item::Use { path, alias }, to_src(e.span())))
}

/// `type Name = DimType;`
fn type_alias_item<'a>() -> impl Parser<'a, &'a str, Spanned<Item>, extra::Err<Simple<'a, char>>> + Clone {
    kw("type")
        .padded()
        .ignore_then(ident().padded())
        .then_ignore(just('=').padded())
        .then(dim())
        .then_ignore(just(';').padded())
        .map_with(|(name, dimension_expr), e| {
            Spanned::new(Item::TypeAlias { name, dimension_expr }, to_src(e.span()))
        })
}

/// `[pub] struct Name { fields }`
fn struct_item<'a>() -> impl Parser<'a, &'a str, Spanned<Item>, extra::Err<Simple<'a, char>>> + Clone {
    visibility()
        .then_ignore(kw("struct").padded())
        .then(ident().padded())
        .then(
            just('{')
                .padded()
                .ignore_then(
                    field()
                        .padded()
                        .separated_by(just(',').padded())
                        .allow_trailing()
                        .collect::<Vec<_>>(),
                )
                .then_ignore(just('}').padded()),
        )
        .map_with(|((vis, name), fields), e| {
            Spanned::new(Item::Struct { name, fields, visibility: vis }, to_src(e.span()))
        })
}

/// `[#[attr]]* [pub] fn name(params) [-> DimType] := expr;`
/// or
/// `[#[attr]]* [pub] fn name(params) [-> DimType] { stmts }`
fn fn_item<'a>() -> impl Parser<'a, &'a str, Spanned<Item>, extra::Err<Simple<'a, char>>> + Clone {
    let params = param()
        .padded()
        .separated_by(just(',').padded())
        .allow_trailing()
        .collect::<Vec<_>>();

    let return_type = just("->").padded().ignore_then(dim());

    let expr_body = just(":=")
        .padded()
        .ignore_then(expr_parser().padded())
        .then_ignore(just(';').padded())
        .map(FunctionBody::Expression);

    let block_body_variant = block_body().map(FunctionBody::Block);

    attribute()
        .padded()
        .repeated()
        .collect::<Vec<_>>()
        .then(visibility())
        .then_ignore(kw("fn").padded())
        .then(ident().padded())
        .then(
            just('(')
                .padded()
                .ignore_then(params)
                .then_ignore(just(')').padded()),
        )
        .then(return_type.or_not())
        .then(choice((expr_body, block_body_variant)))
        .map_with(|(((((attrs, vis), name), params), return_type), body), e| {
            Spanned::new(
                Item::Function {
                    name,
                    params,
                    return_type,
                    body,
                    visibility: vis,
                    attributes: attrs,
                },
                to_src(e.span()),
            )
        })
}

/// `[pub] op name(param: InputDim -> OutputDim) { properties }`
fn op_item<'a>() -> impl Parser<'a, &'a str, Spanned<Item>, extra::Err<Simple<'a, char>>> + Clone {
    visibility()
        .then_ignore(kw("op").padded())
        .then(ident().padded())
        .then(
            just('(')
                .padded()
                .ignore_then(ident().padded())
                .then_ignore(just(':').padded())
                .then(dim())
                .then_ignore(just("->").padded())
                .then(dim())
                .then_ignore(just(')').padded()),
        )
        .then(op_properties())
        .map_with(|(((_vis, name), ((param_name, input_dim), output_dim)), properties), e| {
            Spanned::new(
                Item::CustomOp {
                    name,
                    param_name,
                    input_dim,
                    output_dim,
                    properties,
                },
                to_src(e.span()),
            )
        })
}

/// Any top-level item.
fn item<'a>() -> impl Parser<'a, &'a str, Spanned<Item>, extra::Err<Simple<'a, char>>> + Clone {
    choice((
        op_item(),
        fn_item(),
        struct_item(),
        type_alias_item(),
        use_item(),
    ))
    .padded()
}

// ── Source file ───────────────────────────────────────────────────────────────

/// Parse a complete `.ae` source file into a `SourceFile` AST node.
pub fn source_file_parser<'a>() -> impl Parser<'a, &'a str, SourceFile, extra::Err<Simple<'a, char>>> + Clone {
    item()
        .repeated()
        .collect::<Vec<_>>()
        .map(|items| SourceFile { doc: None, items })
        .padded()
}

/// Parse a full `.ae` source string into a `SourceFile`, returning
/// a human-readable error message on failure.
pub fn parse_source(input: &str) -> Result<SourceFile, String> {
    source_file_parser()
        .parse(input)
        .into_result()
        .map_err(|errs| {
            errs.into_iter()
                .map(|e| format!("{e:?}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> SourceFile {
        match parse_source(src) {
            Ok(f) => f,
            Err(e) => panic!("parse failed:\n{e}"),
        }
    }

    #[test]
    fn test_use_item() {
        let f = parse("use mechanics::kinematics;");
        assert_eq!(f.items.len(), 1);
        assert!(matches!(f.items[0].node, Item::Use { .. }));
    }

    #[test]
    fn test_type_alias() {
        let f = parse("type Acceleration = m / s^2;");
        assert_eq!(f.items.len(), 1);
        if let Item::TypeAlias { name, .. } = &f.items[0].node {
            assert_eq!(name, "Acceleration");
        } else {
            panic!("expected TypeAlias");
        }
    }

    #[test]
    fn test_struct_item() {
        let f = parse("pub struct Particle { mass: kg, velocity: m/s }");
        assert_eq!(f.items.len(), 1);
        if let Item::Struct { name, fields, visibility } = &f.items[0].node {
            assert_eq!(name, "Particle");
            assert_eq!(*visibility, Visibility::Public);
            assert_eq!(fields.len(), 2);
        } else {
            panic!("expected Struct");
        }
    }

    #[test]
    fn test_expr_fn() {
        let f = parse("pub fn force(m: kg, a: m/s^2) -> N := m * a;");
        assert_eq!(f.items.len(), 1);
        if let Item::Function { name, params, body, visibility, .. } = &f.items[0].node {
            assert_eq!(name, "force");
            assert_eq!(*visibility, Visibility::Public);
            assert_eq!(params.len(), 2);
            assert!(matches!(body, FunctionBody::Expression(_)));
        } else {
            panic!("expected Function");
        }
    }

    #[test]
    fn test_block_fn() {
        let f = parse("fn compute(x: m) { let y: m = x * 2.0; }");
        assert_eq!(f.items.len(), 1);
        if let Item::Function { name, body, .. } = &f.items[0].node {
            assert_eq!(name, "compute");
            assert!(matches!(body, FunctionBody::Block(_)));
            if let FunctionBody::Block(stmts) = body {
                assert_eq!(stmts.len(), 1);
            }
        } else {
            panic!("expected Function");
        }
    }

    #[test]
    fn test_op_item() {
        let f = parse("pub op differentiate(y: m -> m/s) {}");
        assert_eq!(f.items.len(), 1);
        assert!(matches!(f.items[0].node, Item::CustomOp { .. }));
    }

    #[test]
    fn test_multiple_items() {
        let src = r#"
            type Velocity = m / s;
            pub struct Body { mass: kg, vel: m/s }
            fn ke(m: kg, v: m/s) -> J := 0.5 * m * v^2;
        "#;
        let f = parse(src);
        assert_eq!(f.items.len(), 3);
    }
}
