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
    Attribute, DimExpr, EGraphRule, Field, FunctionBody, Item, MacroRule, OpProperties,
    Param, SimplifyRule, SourceFile, Spanned, Stmt, Visibility,
};
use rust_decimal::Decimal;
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

/// Public re-export of `kw` for use by `expr.rs`.
pub fn kw_pub<'a>(word: &'static str) -> impl Parser<'a, &'a str, (), extra::Err<Simple<'a, char>>> + Clone {
    kw(word)
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
/// `let [mut] name [: DimType] = expr;`
fn let_stmt_with_expr<'a, P>(expr: P) -> impl Parser<'a, &'a str, Spanned<Stmt>, extra::Err<Simple<'a, char>>> + Clone
where
    P: Parser<'a, &'a str, Spanned<crate::ast::Expr>, extra::Err<Simple<'a, char>>> + Clone + 'a
{
    let is_mut = kw("mut").padded().or_not().map(|m| m.is_some());

    kw("let")
        .padded()
        .ignore_then(is_mut)
        .then(ident().padded())
        .then(just(':').padded().ignore_then(dim()).or_not())
        .then_ignore(just('=').padded())
        .then(expr.padded())
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

fn let_stmt<'a>() -> impl Parser<'a, &'a str, Spanned<Stmt>, extra::Err<Simple<'a, char>>> + Clone {
    let_stmt_with_expr(expr_parser())
}

/// `name = expr;` (re-assignment)
fn assign_stmt_with_expr<'a, P>(expr: P) -> impl Parser<'a, &'a str, Spanned<Stmt>, extra::Err<Simple<'a, char>>> + Clone
where
    P: Parser<'a, &'a str, Spanned<crate::ast::Expr>, extra::Err<Simple<'a, char>>> + Clone + 'a
{
    ident()
        .padded()
        .then_ignore(just('=').padded())
        .then(expr.padded())
        .then_ignore(just(';').padded())
        .map_with(|(name, value), e| Spanned::new(Stmt::Assign { target: name, value }, to_src(e.span())))
}

fn assign_stmt<'a>() -> impl Parser<'a, &'a str, Spanned<Stmt>, extra::Err<Simple<'a, char>>> + Clone {
    assign_stmt_with_expr(expr_parser())
}

/// `break;`
fn break_stmt<'a>() -> impl Parser<'a, &'a str, Spanned<Stmt>, extra::Err<Simple<'a, char>>> + Clone {
    kw("break")
        .padded()
        .then_ignore(just(';').padded())
        .map_with(|_, e| Spanned::new(Stmt::Break, to_src(e.span())))
}

/// `continue;`
fn continue_stmt<'a>() -> impl Parser<'a, &'a str, Spanned<Stmt>, extra::Err<Simple<'a, char>>> + Clone {
    kw("continue")
        .padded()
        .then_ignore(just(';').padded())
        .map_with(|_, e| Spanned::new(Stmt::Continue, to_src(e.span())))
}

/// Any statement inside a block body.
fn stmt_with_expr<'a, P>(expr: P) -> impl Parser<'a, &'a str, Spanned<Stmt>, extra::Err<Simple<'a, char>>> + Clone
where
    P: Parser<'a, &'a str, Spanned<crate::ast::Expr>, extra::Err<Simple<'a, char>>> + Clone + 'a
{
    choice((
        let_stmt_with_expr(expr.clone()),
        break_stmt(),
        continue_stmt(),
        assign_stmt_with_expr(expr.clone()),
        // Bare expression statement (trailing `;`)
        expr.padded().then_ignore(just(';').padded()).map_with(|expr, e| {
            Spanned::new(Stmt::Expr(expr), to_src(e.span()))
        }),
    ))
}

fn stmt<'a>() -> impl Parser<'a, &'a str, Spanned<Stmt>, extra::Err<Simple<'a, char>>> + Clone {
    stmt_with_expr(expr_parser())
}

/// A `{ stmt* }` block body.
pub fn block_body_with_expr<'a, P>(expr: P) -> impl Parser<'a, &'a str, Vec<Spanned<Stmt>>, extra::Err<Simple<'a, char>>> + Clone
where
    P: Parser<'a, &'a str, Spanned<crate::ast::Expr>, extra::Err<Simple<'a, char>>> + Clone + 'a
{
    just('{')
        .padded()
        .ignore_then(stmt_with_expr(expr).repeated().collect::<Vec<_>>())
        .then_ignore(just('}').padded())
}

fn block_body<'a>() -> impl Parser<'a, &'a str, Vec<Spanned<Stmt>>, extra::Err<Simple<'a, char>>> + Clone {
    block_body_with_expr(expr_parser())
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

fn bool_lit<'a>() -> impl Parser<'a, &'a str, bool, extra::Err<Simple<'a, char>>> + Clone {
    choice((just("true").to(true), just("false").to(false)))
}

fn property_flag<'a>(
    name: &'static str,
) -> impl Parser<'a, &'a str, bool, extra::Err<Simple<'a, char>>> + Clone {
    just(name)
        .padded()
        .then(just(':').padded().ignore_then(bool_lit()).or_not())
        .map(|(_, v)| v.unwrap_or(true))
}

fn decimal_lit<'a>() -> impl Parser<'a, &'a str, Decimal, extra::Err<Simple<'a, char>>> + Clone {
    any()
        .filter(|c: &char| c.is_ascii_digit() || *c == '.')
        .repeated()
        .at_least(1)
        .collect::<String>()
        .map(|s| s.parse::<Decimal>().unwrap_or(Decimal::ONE))
}

fn rewrite_rule<'a>() -> impl Parser<'a, &'a str, (Spanned<crate::ast::Expr>, Spanned<crate::ast::Expr>), extra::Err<Simple<'a, char>>> + Clone {
    expr_parser()
        .padded()
        .then_ignore(just("=>").padded().or(just("->").padded()))
        .then(expr_parser().padded())
}

/// ` [after] rewrite pattern => replacement` inside an `egraph { }` block.
fn egraph_rewrite_rule<'a>() -> impl Parser<'a, &'a str, EGraphRule, extra::Err<Simple<'a, char>>> + Clone {
    kw("after")
        .padded()
        .or_not()
        .map(|a| a.is_some())
        .then_ignore(kw("rewrite").padded())
        .then(rewrite_rule())
        .map(|(after_builtins, (pattern, replacement))| EGraphRule {
            pattern,
            replacement,
            after_builtins,
        })
}

fn properties_inner<'a>() -> impl Parser<'a, &'a str, OpProperties, extra::Err<Simple<'a, char>>> + Clone {
    let flag = choice((
        property_flag("vectorizable").map(OpPatch::Vectorizable),
        property_flag("commutative").map(OpPatch::Commutative),
        property_flag("associative").map(OpPatch::Associative),
        just("cost")
            .padded()
            .then_ignore(just(':').padded())
            .ignore_then(decimal_lit())
            .map(OpPatch::Cost),
    ));

    just('{')
        .padded()
        .ignore_then(
            flag.padded()
                .then_ignore(just(',').padded().or_not())
                .repeated()
                .collect::<Vec<_>>(),
        )
        .then_ignore(just('}').padded())
        .map(apply_op_patches)
}

#[derive(Clone)]
enum OpPatch {
    Vectorizable(bool),
    Commutative(bool),
    Associative(bool),
    Cost(Decimal),
}

fn empty_op_properties() -> OpProperties {
    OpProperties {
        vectorizable: false,
        commutative: false,
        associative: false,
        cost: None,
        simplify_rules: Vec::new(),
        egraph_rules: Vec::new(),
    }
}

fn apply_op_patches(patches: Vec<OpPatch>) -> OpProperties {
    let mut props = empty_op_properties();
    for patch in patches {
        match patch {
            OpPatch::Vectorizable(v) => props.vectorizable = v,
            OpPatch::Commutative(v) => props.commutative = v,
            OpPatch::Associative(v) => props.associative = v,
            OpPatch::Cost(c) => props.cost = Some(c),
        }
    }
    props
}

#[derive(Clone)]
enum OpSection {
    Properties(OpProperties),
    Simplify(Vec<SimplifyRule>),
    Egraph(Vec<EGraphRule>),
}

/// Parses the body of a `pub op` declaration: `properties`, `simplify`, `egraph` blocks.
fn op_body<'a>() -> impl Parser<'a, &'a str, OpProperties, extra::Err<Simple<'a, char>>> + Clone {
    let section = choice((
        kw("properties")
            .padded()
            .ignore_then(properties_inner())
            .map(OpSection::Properties),
        kw("simplify")
            .padded()
            .ignore_then(
                just('{')
                    .padded()
                    .ignore_then(
                        rewrite_rule()
                            .map_with(|(pattern, replacement), _e| {
                                SimplifyRule {
                                    pattern,
                                    replacement,
                                }
                            })
                            .padded()
                            .repeated()
                            .collect::<Vec<_>>(),
                    )
                    .then_ignore(just('}').padded()),
            )
            .map(OpSection::Simplify),
        kw("egraph")
            .padded()
            .ignore_then(
                just('{')
                    .padded()
                    .ignore_then(
                        egraph_rewrite_rule()
                            .padded()
                            .repeated()
                            .collect::<Vec<_>>(),
                    )
                    .then_ignore(just('}').padded()),
            )
            .map(OpSection::Egraph),
    ));

    just('{')
        .padded()
        .ignore_then(section.padded().repeated().collect::<Vec<_>>())
        .then_ignore(just('}').padded())
        .map(|sections| {
            let mut props = empty_op_properties();
            for section in sections {
                match section {
                    OpSection::Properties(p) => {
                        props.vectorizable = p.vectorizable;
                        props.commutative = p.commutative;
                        props.associative = p.associative;
                        props.cost = p.cost;
                    }
                    OpSection::Simplify(rules) => props.simplify_rules = rules,
                    OpSection::Egraph(rules) => props.egraph_rules = rules,
                }
            }
            props
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
        .map_with(|(path, alias), e| {
            Spanned::new(Item::Use { path, alias }, to_src(e.span()))
        })
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
            Spanned::new(
                Item::TypeAlias {
                    name,
                    dimension_expr,
                    doc: None,
                },
                to_src(e.span()),
            )
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
            Spanned::new(
                Item::Struct {
                    name,
                    fields,
                    visibility: vis,
                    doc: None,
                },
                to_src(e.span()),
            )
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
                    doc: None,
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
        .then(op_body())
        .map_with(|(((vis, name), ((param_name, input_dim), output_dim)), properties), e| {
            Spanned::new(
                Item::CustomOp {
                    name,
                    param_name,
                    input_dim,
                    output_dim,
                    visibility: vis,
                    properties,
                    doc: None,
                },
                to_src(e.span()),
            )
        })
}

// ── Source file ───────────────────────────────────────────────────────────────

/// Parse a complete `.ae` source file into a `SourceFile` AST node.
pub fn source_file_parser<'a>() -> impl Parser<'a, &'a str, SourceFile, extra::Err<Simple<'a, char>>> + Clone {
    recursive(|item| {
        let mod_item = visibility()
            .then_ignore(kw("mod").padded())
            .then(ident().padded())
            .then(
                just(';')
                    .padded()
                    .to(None)
                    .or(
                        just('{')
                            .padded()
                            .ignore_then(item.repeated().collect::<Vec<_>>())
                            .then_ignore(just('}').padded())
                            .map(Some),
                    ),
            )
            .map_with(|((vis, name), body), e| {
                Spanned::new(
                    Item::Module {
                        name,
                        body,
                        visibility: vis,
                        doc: None,
                    },
                    to_src(e.span()),
                )
            });

        choice((
            op_item(),
            fn_item(),
            mod_item,
            struct_item(),
            type_alias_item(),
            use_item(),
            macro_rules_item(),
        ))
        .padded()
    })
    .repeated()
    .collect::<Vec<_>>()
    .map(|items| SourceFile { doc: None, items })
    .padded()
}

/// Parses `macro_rules! name { (pattern) => { replacement } ... }`
///
/// Patterns and replacements are captured as raw strings — actual token-tree
/// matching happens in the expansion pre-pass.
fn macro_rules_item<'a>() -> impl Parser<'a, &'a str, Spanned<Item>, extra::Err<Simple<'a, char>>> + Clone {
    // Captures all characters between balanced `{...}` (including nested braces).
    fn balanced_braces<'a>() -> impl Parser<'a, &'a str, String, extra::Err<Simple<'a, char>>> + Clone {
        just('{').then(
            recursive(|inner| {
                choice((
                    just('{').then(inner).then(just('}')).map(|((open, mid), close)| {
                        let mut s = String::from(open);
                        s.push_str(&mid);
                        s.push(close);
                        s
                    }),
                    none_of("}").map(|c: char| c.to_string()),
                ))
                .repeated()
                .collect::<Vec<String>>()
                .map(|parts| parts.concat())
            })
        )
        .then(just('}'))
        .map(|((_, mid), _)| mid)
    }

    // Captures all characters between balanced `(...)`
    fn balanced_parens<'a>() -> impl Parser<'a, &'a str, String, extra::Err<Simple<'a, char>>> + Clone {
        just('(').then(
            none_of(")")
                .repeated()
                .collect::<String>()
        )
        .then(just(')'))
        .map(|((_, mid), _)| mid)
    }

    let single_rule = balanced_parens()
        .padded()
        .then_ignore(just("=>").padded())
        .then(balanced_braces().padded())
        .map(|(pattern, replacement)| crate::ast::MacroRule { pattern, replacement });

    kw("macro_rules!")
        .padded()
        .ignore_then(ident().padded())
        .then(
            single_rule
                .padded()
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just('{').padded(), just('}').padded()),
        )
        .map_with(|(name, rules), e| {
            Spanned::new(Item::MacroDef { name, rules }, to_src(e.span()))
        })
}

/// Parse a full `.ae` source string into a `SourceFile`, returning
/// a human-readable error message on failure.
pub fn parse_source(input: &str) -> Result<SourceFile, String> {
    let prepared = crate::parser::docs::prepare_source(input);
    let mut file = source_file_parser()
        .parse(&prepared.code)
        .into_result()
        .map_err(|errs| {
            errs.into_iter()
                .map(|e| format!("{e:?}"))
                .collect::<Vec<_>>()
                .join("\n")
        })?;
    file.doc = prepared.module_doc;
    crate::parser::docs::attach_item_docs(&mut file, prepared.item_docs);
    Ok(file)
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
        if let Item::Struct { name, fields, visibility, .. } = &f.items[0].node {
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
    fn test_egraph_after_builtins() {
        let src = r#"
            pub op f(x: m -> m) {
                egraph { after rewrite f(y) => y }
            }
        "#;
        let f = parse(src);
        if let Item::CustomOp { properties, .. } = &f.items[0].node {
            assert_eq!(properties.egraph_rules.len(), 1);
            assert!(properties.egraph_rules[0].after_builtins);
        } else {
            panic!("expected CustomOp");
        }
    }

    #[test]
    fn test_op_properties_parsed() {
        let f = parse(
            r#"pub op scale(x: m -> m) {
                properties { vectorizable: true, cost: 2.5, }
            }"#,
        );
        if let Item::CustomOp { properties, .. } = &f.items[0].node {
            assert!(properties.vectorizable);
            assert_eq!(properties.cost.map(|c| c.to_string()), Some("2.5".to_string()));
        } else {
            panic!("expected CustomOp");
        }
    }

    #[test]
    fn test_mod_inline() {
        let f = parse("pub mod inner { fn helper(x: m) -> m := x; }");
        assert_eq!(f.items.len(), 1);
        if let Item::Module { name, body, visibility, .. } = &f.items[0].node {
            assert_eq!(name, "inner");
            assert_eq!(*visibility, Visibility::Public);
            assert!(body.as_ref().is_some_and(|b| !b.is_empty()));
        } else {
            panic!("expected Module");
        }
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
