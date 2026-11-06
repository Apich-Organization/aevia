//! Explicit type arguments apply to names, including qualified names.
use aevia_syntax::ast::ExprKind;
use aevia_syntax::parser::parse_expression;

#[test]
fn qualified_names_accept_explicit_type_arguments() {
    for source in [
        "T.zeros<Int>",
        "Library.Tensor.zeros<Int>",
        "T.zeros<Int>(2)",
    ] {
        let expr =
            parse_expression(0, source).unwrap_or_else(|e| panic!("{source}: {:?}", e.items));
        let tyapp = match &expr.kind {
            | ExprKind::TypeApp { .. } => &expr,
            | ExprKind::Call { callee, .. } => callee,
            | _ => panic!("{source}: expected type application"),
        };
        assert!(matches!(tyapp.kind, ExprKind::TypeApp { .. }), "{source}");
    }
}

#[test]
fn computed_callees_remain_ambiguous() {
    for source in ["f()<Int>", "f().member<Int>"] {
        let errors = parse_expression(0, source).expect_err("computed callee must be rejected");
        assert!(
            errors
                .items
                .iter()
                .any(|diagnostic| diagnostic.code == "E0207"),
            "{source}: {:?}",
            errors.items
        );
    }
    // Parenthesized arithmetic can be parsed as a comparison first, but it
    // still must never be accepted as a type-applied callable.
    assert!(parse_expression(0, "(a + b)<Int>").is_err());
}
