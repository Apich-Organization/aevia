//! Type productions of appendix A.5: `Matrix`, `Sparse`, `row_type` and
//! `type_postfix`, plus the kind of a `type_arg`.

mod support;

use aevia_syntax::ast::{DeclKind, ExprKind, NatAst, ShapeAst, Ty, TyKind};
use support::expect_clean;

/// The annotation of `let x: <ty> = ...`.
fn annotation(source: &str) -> Ty {
    let module = expect_clean(source);
    let DeclKind::Let { ty, .. } = &module.decls[0].kind else {
        panic!("expected a let");
    };
    ty.clone().expect("an annotation")
}

#[test]
fn matrix_type() {
    let ty = annotation("let x: Matrix<3, 4, Float> = 0.0\n");
    let TyKind::Matrix { rows, cols, elem } = &ty.kind else {
        panic!("expected Matrix, got {:?}", ty.kind);
    };
    assert!(matches!(rows, NatAst::Lit(3)));
    assert!(matches!(cols, NatAst::Lit(4)));
    assert!(matches!(elem.kind, TyKind::Float(_)));
}

#[test]
fn sparse_type() {
    let ty = annotation("let x: Sparse<CSR, 2, Float> = 0.0\n");
    let TyKind::Sparse {
        format,
        shape,
        elem,
    } = &ty.kind
    else {
        panic!("expected Sparse, got {:?}", ty.kind);
    };
    assert_eq!(format, "CSR");
    assert!(matches!(shape, ShapeAst::Rank(NatAst::Lit(2))));
    assert!(matches!(elem.kind, TyKind::Float(_)));
}

#[test]
fn sparse_with_a_refined_shape() {
    let ty = annotation("let x: Sparse<csc, [100, 100], Float> = 0.0\n");
    let TyKind::Sparse { format, shape, .. } = &ty.kind else {
        panic!("expected Sparse, got {:?}", ty.kind);
    };
    assert_eq!(format, "csc");
    let ShapeAst::Refined(items) = shape else {
        panic!("expected a refined shape");
    };
    assert_eq!(items.len(), 2);
}

#[test]
fn table_with_a_row_sum() {
    let ty = annotation("let x: Table<{ a: Int } + { b: String }> = 0\n");
    let TyKind::Table(row) = &ty.kind else {
        panic!("expected Table, got {:?}", ty.kind);
    };
    let TyKind::Row(items) = &row.kind else {
        panic!("expected a row sum, got {:?}", row.kind);
    };
    assert_eq!(items.len(), 2);
    assert!(matches!(items[0].kind, TyKind::Record { .. }));
    assert!(matches!(items[1].kind, TyKind::Record { .. }));
}

#[test]
fn table_with_a_single_row_is_not_a_sum() {
    let ty = annotation("let x: Table<{ a: Int }> = 0\n");
    let TyKind::Table(row) = &ty.kind else {
        panic!("expected Table, got {:?}", ty.kind);
    };
    assert!(
        matches!(row.kind, TyKind::Record { .. }),
        "a one-summand row type stays the type itself, got {:?}",
        row.kind
    );
}

#[test]
fn a_dimension_postfix_argument_is_carried_as_a_dimension() {
    // `T[d]` is §A.5's `type_postfix`, and `d` is a dimension, not a type.
    let ty = annotation("let x: Vec[L] = 0\n");
    let TyKind::Named { path, args } = &ty.kind else {
        panic!("expected a named type, got {:?}", ty.kind);
    };
    assert_eq!(path, &["Vec".to_string()]);
    assert_eq!(args.len(), 1);
    let TyKind::DimArg(dim) = &args[0].kind else {
        panic!("expected a dimension argument, got {:?}", args[0].kind);
    };
    assert_eq!(dim.to_source(), "L");
}

#[test]
fn two_dimension_postfix_arguments() {
    let ty = annotation("let x: Foo[L][T] = 0\n");
    let TyKind::Named { args, .. } = &ty.kind else {
        panic!("expected a named type, got {:?}", ty.kind);
    };
    assert_eq!(args.len(), 2);
    assert!(matches!(args[0].kind, TyKind::DimArg(_)));
    assert!(matches!(args[1].kind, TyKind::DimArg(_)));
}

#[test]
fn a_dimension_postfix_takes_a_dimension_expression() {
    let ty = annotation("let x: Foo[L/T^2] = 0\n");
    let TyKind::Named { args, .. } = &ty.kind else {
        panic!("expected a named type, got {:?}", ty.kind);
    };
    let TyKind::DimArg(dim) = &args[0].kind else {
        panic!("expected a dimension argument");
    };
    assert_eq!(dim.to_source(), "L/T^2");
}

#[test]
fn a_natural_number_type_argument_keeps_its_kind() {
    let ty = annotation("let x: Foo<3> = 0\n");
    let TyKind::Named { args, .. } = &ty.kind else {
        panic!("expected a named type, got {:?}", ty.kind);
    };
    assert!(
        matches!(args[0].kind, TyKind::NatArg(NatAst::Lit(3))),
        "expected a natural-number argument, got {:?}",
        args[0].kind
    );
}

#[test]
fn a_shape_type_argument_keeps_its_kind() {
    let ty = annotation("let x: Foo<[3, ?]> = 0\n");
    let TyKind::Named { args, .. } = &ty.kind else {
        panic!("expected a named type, got {:?}", ty.kind);
    };
    let TyKind::ShapeArg(ShapeAst::Refined(items)) = &args[0].kind else {
        panic!("expected a shape argument, got {:?}", args[0].kind);
    };
    assert_eq!(items.len(), 2);
}

#[test]
fn a_dimension_postfix_on_a_non_named_type_is_e0202() {
    // `Int` is not a type constructor, so `Int[L]` has no meaning (§A.5:
    // `type_postfix` applies to a `type_atom`, but only a named type has
    // parameters).
    let source = "let x: Int[L] = 0\n";
    let diagnostic = support::expect_error(source, "E0202");
    assert_eq!(support::span_text(source, &diagnostic), "Int[L]");
}

#[test]
fn a_type_argument_stays_a_type() {
    let ty = annotation("let x: Option<Vec2> = None\n");
    let TyKind::Option(inner) = &ty.kind else {
        panic!("expected Option, got {:?}", ty.kind);
    };
    assert!(matches!(inner.kind, TyKind::Named { .. }));
}

#[test]
fn the_normative_type_productions_still_parse() {
    // A sample of §A.5 that must keep working after the additions.
    for source in [
        "let x: Tensor<2, Float> = []\n",
        "let x: Tensor<[3, ?], Real[L]> = []\n",
        "let x: Point[Theta] = 0\n",
        "let x: Unit[L] = 0\n",
        "let x: Result<Int, String> = Ok(1)\n",
        "let x: Set<Int> = 0\n",
        "let x: Dyn<Float> = 0\n",
        "let x: RateOf<Real[L]> = 0\n",
        "let x: SymFun<L, T> = 0\n",
        "let x: IO Unit = 0\n",
        "let x: { a: Int, ...r } = 0\n",
        "let x: (Int, String) -> Bool = 0\n",
        "let x: forall d. Real[d] -> Real[d] = 0\n",
        "let x: Solutions<Real[L], T> = 0\n",
    ] {
        expect_clean(source);
    }
}

#[test]
fn an_expression_type_argument_can_be_a_shape() {
    // §2.5/§3.5.1: `reshape<[3, 2]>(a)` names its target shape in the
    // expression's type-argument list, so that list is `type_arg` (§A.5),
    // not merely `type` — the parser used `ty()` and rejected the `[`.
    let module = expect_clean("let r = reshape<[3, 2]>(a)\n");
    let DeclKind::Let { value, .. } = &module.decls[0].kind else {
        panic!("expected a let");
    };
    let ExprKind::Call { callee, args } = &value.kind else {
        panic!("expected a call, got {:?}", value.kind);
    };
    assert_eq!(args.len(), 1);
    let ExprKind::TypeApp {
        callee: inner,
        args: type_args,
    } = &callee.kind
    else {
        panic!("expected a type application, got {:?}", callee.kind);
    };
    assert!(matches!(inner.kind, ExprKind::Var(ref name) if name == "reshape"));
    let TyKind::ShapeArg(ShapeAst::Refined(items)) = &type_args[0].kind else {
        panic!("expected a shape argument, got {:?}", type_args[0].kind);
    };
    assert_eq!(items.len(), 2);
}

#[test]
fn an_expression_type_argument_can_be_a_natural_number() {
    // §A.5 `type_arg ::= type | nat | dim | shape`.
    let module = expect_clean("let z = zeros_of<3>()\n");
    let DeclKind::Let { value, .. } = &module.decls[0].kind else {
        panic!("expected a let");
    };
    let ExprKind::Call { callee, .. } = &value.kind else {
        panic!("expected a call");
    };
    let ExprKind::TypeApp { args, .. } = &callee.kind else {
        panic!("expected a type application");
    };
    assert!(matches!(args[0].kind, TyKind::NatArg(NatAst::Lit(3))));
}

#[test]
fn a_comparison_after_an_identifier_is_still_a_comparison() {
    // §2.7.2: `a < b` is a comparison; only `ident<` (adjacent) tries type
    // arguments, and a failure rolls back to `BinOp::Lt`.
    for source in [
        "let x = a < b\n",
        "let x = f(a) < 3\n",
        "let y = a < b && b < c\n",
    ] {
        expect_clean(source);
    }
}
