//! Aevia mathematical and physical expression parser using Chumsky.

use crate::ast::{BinOp, DimExpr, Expr, MatchArm, Pattern, Spanned, Stmt, UnOp};
use chumsky::prelude::*;
use miette::SourceSpan;
use rust_decimal::Decimal;
use std::str::FromStr;

/// Convert a Chumsky `SimpleSpan` into a `miette::SourceSpan`.
fn to_src_span(span: SimpleSpan) -> SourceSpan {
    SourceSpan::new(span.start.into(), span.end - span.start)
}

/// Merge two `miette::SourceSpan`s into one covering both.
fn merge_spans(a: SourceSpan, b: SourceSpan) -> SourceSpan {
    let start = a.offset();
    let end = b.offset() + b.len();
    SourceSpan::new(start.into(), end - start)
}

/// Create a parser for physical dimensional expressions (e.g. `m / s^2`, `kg * m / s^2`).
pub fn dim_expr_parser<'a>() -> impl Parser<'a, &'a str, Spanned<DimExpr>, extra::Err<Simple<'a, char>>> + Clone {
    // Base unit identifier (e.g. "m", "kg") or the constant "1".
    let unit_id = any()
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
        .or(just('1').to("1".to_string()));

    // Base unit with its source span.
    let base = unit_id.map_with(|name, e| Spanned::new(DimExpr::Base(name), to_src_span(e.span())));

    // Optional integer exponent (e.g. `^2`, `^-3`).
    let exponent = just('^')
        .ignore_then(just('-').or_not())
        .then(
            any()
                .filter(|c: &char| c.is_ascii_digit())
                .repeated()
                .at_least(1)
                .collect::<Vec<char>>(),
        )
        .map(|(neg, digits): (Option<char>, Vec<char>)| {
            let mut s = String::new();
            if neg.is_some() {
                s.push('-');
            }
            s.extend(digits);
            s.parse::<i32>().unwrap_or(1)
        });

    // unit^exp (optional)
    let unit_with_pow = base
        .then(exponent.or_not())
        .map_with(|(base_unit, pow_opt), e| {
            if let Some(exp) = pow_opt {
                Spanned::new(DimExpr::Power(Box::new(base_unit), exp), to_src_span(e.span()))
            } else {
                base_unit
            }
        });

    // Left-associative * and / chain.
    let chain = unit_with_pow.clone().then(
        choice((just('*').to(true), just('/').to(false)))
            .padded()
            .then(unit_with_pow)
            .repeated()
            .collect::<Vec<_>>(),
    )
    .map(|(first, rest): (Spanned<DimExpr>, Vec<(bool, Spanned<DimExpr>)>)| {
        rest.into_iter().fold(first, |lhs, (is_mul, rhs)| {
            let span = merge_spans(lhs.span, rhs.span);
            if is_mul {
                Spanned::new(DimExpr::Mul(Box::new(lhs), Box::new(rhs)), span)
            } else {
                Spanned::new(DimExpr::Div(Box::new(lhs), Box::new(rhs)), span)
            }
        })
    });

    let shape_suffix = just('[')
        .ignore_then(
            any()
                .filter(|c: &char| c.is_ascii_digit())
                .repeated()
                .at_least(1)
                .collect::<Vec<char>>()
                .map(|chars| {
                    let s: String = chars.into_iter().collect();
                    s.parse::<usize>().unwrap_or(0)
                })
                .padded()
                .separated_by(just(',').padded())
                .collect::<Vec<usize>>()
        )
        .then_ignore(just(']'));

    chain.then(shape_suffix.or_not()).map_with(|(base_dim, shape_opt), e| {
        if let Some(shape) = shape_opt {
            Spanned::new(DimExpr::Tensor {
                base: Box::new(base_dim),
                shape,
            }, to_src_span(e.span()))
        } else {
            base_dim
        }
    })
}

/// Create a parser for computational expressions with precedence climbing.
pub fn expr_parser<'a>() -> impl Parser<'a, &'a str, Spanned<Expr>, extra::Err<Simple<'a, char>>> + Clone {
    recursive(|expr| {
        // Identifiers.
        let ident = any()
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
            });

        // Integer / float digits.
        let int_digits = any()
            .filter(|c: &char| c.is_ascii_digit())
            .repeated()
            .at_least(1)
            .collect::<Vec<char>>();

        let number_str = int_digits
            .clone()
            .then(just('.').then(int_digits).or_not())
            .map(|(int_part, frac_part): (Vec<char>, Option<(char, Vec<char>)>)| {
                let mut s = String::new();
                s.extend(int_part);
                if let Some((_, frac)) = frac_part {
                    s.push('.');
                    s.extend(frac);
                }
                s
            });

        // Physical literal with optional unit suffix, e.g. `9.8_m/s^2`.
        let literal = number_str
            .then(just('_').ignore_then(dim_expr_parser()).or_not())
            .map_with(|(num_str, suffix_opt), e| {
                let dec = Decimal::from_str(&num_str).unwrap_or(Decimal::ZERO);
                Spanned::new(
                    Expr::Literal { value: dec, suffix: suffix_opt },
                    to_src_span(e.span()),
                )
            });

        // Function call or bare variable.
        let call_or_var = ident
            .then(
                just('(')
                    .padded()
                    .ignore_then(
                        expr.clone()
                            .padded()
                            .separated_by(just(',').padded())
                            .collect::<Vec<_>>(),
                    )
                    .then_ignore(just(')'))
                    .or_not(),
            )
            .map_with(|(name, args_opt), e| {
                let span = to_src_span(e.span());
                if let Some(args) = args_opt {
                    Spanned::new(Expr::Call { func: name, args }, span)
                } else {
                    Spanned::new(Expr::Variable(name), span)
                }
            });

        // Parenthesised sub-expression.
        let parens = just('(')
            .padded()
            .ignore_then(expr.clone())
            .then_ignore(just(')').padded());

        // `loop { stmts }`
        let loop_kw = super::items::kw_pub("loop");
        let loop_expr = loop_kw
            .padded()
            .ignore_then(super::items::block_body_with_expr(expr.clone()))
            .map_with(|body, e| Spanned::new(Expr::Loop { body }, to_src_span(e.span())));

        // `while cond { stmts }`
        let while_expr = super::items::kw_pub("while")
            .padded()
            .ignore_then(expr.clone().padded())
            .then(super::items::block_body_with_expr(expr.clone()))
            .map_with(|(cond, body), e| {
                Spanned::new(
                    Expr::While { cond: Box::new(cond), body },
                    to_src_span(e.span()),
                )
            });

        // `for var in start..end { stmts }`
        let for_expr = super::items::kw_pub("for")
            .padded()
            .ignore_then(ident.clone().padded())
            .then_ignore(super::items::kw_pub("in").padded())
            .then(expr.clone().padded())
            .then_ignore(just("..").padded())
            .then(expr.clone().padded())
            .then(super::items::block_body_with_expr(expr.clone()))
            .map_with(|(((var, start), end), body), e| {
                Spanned::new(
                    Expr::For {
                        var,
                        start: Box::new(start),
                        end: Box::new(end),
                        body,
                    },
                    to_src_span(e.span()),
                )
            });

        // `if cond { then } [elseif cond { then }]* [else { else }]`
        // Parsed as right-recursive nested Expr::If nodes.
        let if_expr = {
            let block_expr = super::items::block_body_with_expr(expr.clone()).map_with(|stmts, e| {
                Spanned::new(Expr::Block(stmts), to_src_span(e.span()))
            });

            super::items::kw_pub("if")
                .padded()
                .ignore_then(expr.clone().padded())
                .then(block_expr.clone())
                .then(
                    // `elseif` chains
                    super::items::kw_pub("elseif")
                        .padded()
                        .ignore_then(expr.clone().padded())
                        .then(block_expr.clone())
                        .repeated()
                        .collect::<Vec<_>>(),
                )
                .then(
                    // optional `else { ... }`
                    super::items::kw_pub("else")
                        .padded()
                        .ignore_then(block_expr.clone())
                        .or_not(),
                )
                .map_with(|(((cond, then_branch), elseif_arms), else_branch), e| {
                    let span = to_src_span(e.span());
                    // Build the final else node first (innermost).
                    let mut current_else: Option<Box<Spanned<Expr>>> =
                        else_branch.map(Box::new);
                    // Fold elseif arms from right to left.
                    for (ei_cond, ei_then) in elseif_arms.into_iter().rev() {
                        let ei_span = merge_spans(ei_cond.span, ei_then.span);
                        current_else = Some(Box::new(Spanned::new(
                            Expr::If {
                                cond: Box::new(ei_cond),
                                then_branch: Box::new(ei_then),
                                else_branch: current_else,
                            },
                            ei_span,
                        )));
                    }
                    Spanned::new(
                        Expr::If {
                            cond: Box::new(cond),
                            then_branch: Box::new(then_branch),
                            else_branch: current_else,
                        },
                        span,
                    )
                })
        };

        // Pattern for a match arm.
        let pattern = choice((
            just('_').map(|_| Pattern::Wildcard),
            // numeric literal pattern
            any()
                .filter(|c: &char| c.is_ascii_digit())
                .repeated()
                .at_least(1)
                .collect::<String>()
                .then(
                    just('.')
                        .then(
                            any()
                                .filter(|c: &char| c.is_ascii_digit())
                                .repeated()
                                .at_least(1)
                                .collect::<String>(),
                        )
                        .or_not(),
                )
                .map(|(int, frac)| {
                    let s = if let Some((_, f)) = frac {
                        format!("{int}.{f}")
                    } else {
                        int
                    };
                    Pattern::Literal(Decimal::from_str(&s).unwrap_or(Decimal::ZERO))
                }),
            // binding with optional type guard
            ident
                .clone()
                .then(
                    just(':')
                        .padded()
                        .ignore_then(dim_expr_parser())
                        .or_not(),
                )
                .map(|(name, type_guard)| Pattern::Binding { name, type_guard }),
        ));

        // `match scrutinee { pattern => expr, ... }`
        let match_expr = super::items::kw_pub("match")
            .padded()
            .ignore_then(expr.clone().padded())
            .then(
                just('{')
                    .padded()
                    .ignore_then(
                        pattern
                            .padded()
                            .then_ignore(just("=>").padded())
                            .then(expr.clone().padded())
                            .then_ignore(just(',').padded().or_not())
                            .map(|(pat, body)| MatchArm { pattern: pat, body })
                            .repeated()
                            .collect::<Vec<_>>(),
                    )
                    .then_ignore(just('}').padded()),
            )
            .map_with(|(scrutinee, arms), e| {
                Spanned::new(
                    Expr::Match { scrutinee: Box::new(scrutinee), arms },
                    to_src_span(e.span()),
                )
            });

        // `continue` (no-op in lowering, signals loop continuation)
        let continue_expr = super::items::kw_pub("continue")
            .map_with(|_, e| {
                // Desugar continue to a literal 0.0 (acts as a no-result placeholder)
                Spanned::new(
                    Expr::Literal { value: Decimal::ZERO, suffix: None },
                    to_src_span(e.span()),
                )
            });

        let atom = choice((
            loop_expr,
            while_expr,
            for_expr,
            if_expr,
            match_expr,
            continue_expr,
            literal,
            call_or_var,
            parens,
        ));

        // Unary negation (zero or more leading `-`).
        let unary = just('-')
            .padded()
            .repeated()
            .collect::<Vec<char>>()
            .then(atom)
            .map_with(|(negs, mut inner), e| {
                let span = to_src_span(e.span());
                for _ in negs {
                    inner = Spanned::new(
                        Expr::UnaryOp { op: UnOp::Neg, expr: Box::new(inner) },
                        span,
                    );
                }
                inner
            });

        // Right-associative exponentiation (^).
        let power = unary.clone().then(
            just('^').padded().ignore_then(unary).repeated().collect::<Vec<_>>(),
        )
        .map(|(base, exps): (Spanned<Expr>, Vec<Spanned<Expr>>)| {
            // Fold right by reversing.
            let mut all = vec![base];
            all.extend(exps);
            let mut iter = all.into_iter().rev();
            let mut acc = iter.next().unwrap();
            for lhs in iter {
                let span = merge_spans(lhs.span, acc.span);
                acc = Spanned::new(
                    Expr::BinaryOp { op: BinOp::Pow, lhs: Box::new(lhs), rhs: Box::new(acc) },
                    span,
                );
            }
            acc
        });

        // Left-associative * / %.
        let mult_op = choice((
            just('*').to(BinOp::Mul),
            just('/').to(BinOp::Div),
            just('%').to(BinOp::Mod),
        ));

        let multiplicative = power.clone()
            .then(mult_op.padded().then(power).repeated().collect::<Vec<_>>())
            .map(|(first, rest): (Spanned<Expr>, Vec<(BinOp, Spanned<Expr>)>)| {
                rest.into_iter().fold(first, |lhs, (op, rhs)| {
                    let span = merge_spans(lhs.span, rhs.span);
                    Spanned::new(Expr::BinaryOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }, span)
                })
            });

        // Left-associative + -.
        let add_op = choice((just('+').to(BinOp::Add), just('-').to(BinOp::Sub)));

        let additive = multiplicative.clone()
            .then(add_op.padded().then(multiplicative).repeated().collect::<Vec<_>>())
            .map(|(first, rest): (Spanned<Expr>, Vec<(BinOp, Spanned<Expr>)>)| {
                rest.into_iter().fold(first, |lhs, (op, rhs)| {
                    let span = merge_spans(lhs.span, rhs.span);
                    Spanned::new(Expr::BinaryOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }, span)
                })
            });

        // Comparisons < > ==.
        let comp_op = choice((
            just("==").to(BinOp::Eq),
            just('<').to(BinOp::Lt),
            just('>').to(BinOp::Gt),
        ));

        additive.clone()
            .then(comp_op.padded().then(additive).repeated().collect::<Vec<_>>())
            .map(|(first, rest): (Spanned<Expr>, Vec<(BinOp, Spanned<Expr>)>)| {
                rest.into_iter().fold(first, |lhs, (op, rhs)| {
                    let span = merge_spans(lhs.span, rhs.span);
                    Spanned::new(Expr::BinaryOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }, span)
                })
            })
    })
}

/// Parse a source string into a spanned expression, returning parse errors on failure.
pub fn parse_expr(input: &str) -> Result<Spanned<Expr>, Vec<Simple<'_, char>>> {
    expr_parser().padded().parse(input).into_result()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_literal() {
        let parsed = parse_expr("10.0").unwrap();
        if let Expr::Literal { value, suffix } = parsed.node {
            assert_eq!(value, Decimal::from_str("10.0").unwrap());
            assert!(suffix.is_none());
        } else {
            panic!("expected Literal");
        }
    }

    #[test]
    fn test_suffixed_literal() {
        let parsed = parse_expr("9.8_m/s^2").unwrap();
        if let Expr::Literal { value, suffix } = parsed.node {
            assert_eq!(value, Decimal::from_str("9.8").unwrap());
            let suff = suffix.expect("expected unit suffix").node;
            assert!(matches!(suff, DimExpr::Div(..)), "expected Div dim, got {:?}", suff);
        } else {
            panic!("expected Literal");
        }
    }

    #[test]
    fn test_variable() {
        let parsed = parse_expr("mass").unwrap();
        assert_eq!(parsed.node, Expr::Variable("mass".to_string()));
    }

    #[test]
    fn test_precedence_mul_over_add() {
        // 1 + 2 * 3  =>  Add(1, Mul(2, 3))
        let parsed = parse_expr("1 + 2 * 3").unwrap();
        if let Expr::BinaryOp { op, lhs, rhs } = parsed.node {
            assert_eq!(op, BinOp::Add);
            assert!(matches!(lhs.node, Expr::Literal { .. }));
            assert!(matches!(rhs.node, Expr::BinaryOp { op: BinOp::Mul, .. }));
        } else {
            panic!("expected BinaryOp");
        }
    }

    #[test]
    fn test_function_call() {
        let parsed = parse_expr("kinetic_energy(m, v)").unwrap();
        if let Expr::Call { func, args } = parsed.node {
            assert_eq!(func, "kinetic_energy");
            assert_eq!(args.len(), 2);
        } else {
            panic!("expected Call");
        }
    }

    #[test]
    fn test_comparison() {
        let parsed = parse_expr("x < 10.0_s").unwrap();
        if let Expr::BinaryOp { op, .. } = parsed.node {
            assert_eq!(op, BinOp::Lt);
        } else {
            panic!("expected BinaryOp");
        }
    }

    #[test]
    fn test_unary_neg() {
        let parsed = parse_expr("-x").unwrap();
        assert!(matches!(parsed.node, Expr::UnaryOp { op: UnOp::Neg, .. }));
    }

    #[test]
    fn test_exponentiation() {
        // 2 ^ 3 ^ 2 is right-associative => 2 ^ (3 ^ 2)
        let parsed = parse_expr("2 ^ 3").unwrap();
        assert!(matches!(parsed.node, Expr::BinaryOp { op: BinOp::Pow, .. }));
    }
}
