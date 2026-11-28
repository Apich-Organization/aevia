//! Attributes (§2.2.7): a known attribute reaches the declaration, an unknown
//! one is `E0210`.

mod support;

use aevia_syntax::ast::AttrArg;
use aevia_syntax::ast::DeclKind;
use aevia_syntax::ast::ExprKind;
use support::expect_clean;
use support::expect_error;
use support::span_text;

#[test]
fn an_unknown_attribute_is_e0210() {
    for source in ["@nope\nfn f() = 1\n", "@compile_fail\nfn f() = 1\n"] {
        let diagnostic = expect_error(source, "E0210");
        assert_eq!(
            span_text(source, &diagnostic),
            source.split('\n').next().unwrap()
        );
    }
}

#[test]
fn a_known_attribute_is_accepted() {
    for name in [
        "stable",
        "experimental",
        "deprecated(\"use new_api\")",
        "jit",
        "op(\"mydomain/softplus\")",
        "rule(\"softplus(x) => ln(1 + exp(x))\")",
    ] {
        let source = format!("@{name}\nfn f() = 1\n");
        expect_clean(&source);
    }
}

#[test]
fn compile_fail_is_rejected_rather_than_ignored() {
    // `@compile_fail` used to parse and then be silently ignored by the
    // checker. The metaprogramming design replaces it with the
    // `compile_fail("…")` test feature, so until the standard settles it is an
    // unknown attribute instead of a no-op (§2.2.7 `E0210`).
    expect_error("@compile_fail\nfn f() = 1\n", "E0210");
}

#[test]
fn jit_is_visible_on_the_declaration() {
    // §9.6.1 needs `@jit` at the checker, so it has to survive parsing in a
    // form later phases can read.
    let module = expect_clean("@jit\nfn hot(x: Float[1]): Float[1] = x\n");
    let decl = &module.decls[0];
    assert!(decl.has_attribute("jit"));
    assert_eq!(decl.attribute("jit").map(|attr| attr.span.start), Some(0));
}

#[test]
fn op_keeps_its_argument() {
    let module = expect_clean("@op(\"mydomain/softplus\")\nfn softplus(x: Real[1]): Real[1] = x\n");
    let decl = &module.decls[0];
    let attr = decl.attribute("op").expect("@op");
    assert_eq!(attr.args.len(), 1);
    let AttrArg { name, value } = &attr.args[0];
    assert!(name.is_none());
    let ExprKind::Str(pieces) = &value.kind else {
        panic!("expected a string argument, got {:?}", value.kind);
    };
    let aevia_syntax::ast::StrPiece::Text(text) = &pieces[0] else {
        panic!("expected literal text");
    };
    assert_eq!(text, "mydomain/softplus");
}

#[test]
fn an_attribute_argument_may_be_named() {
    let module = expect_clean("@jit(target = Gpu(0))\nfn hot() = 1\n");
    let attr = module.decls[0].attribute("jit").expect("@jit");
    assert_eq!(attr.args[0].name.as_deref(), Some("target"));
}

#[test]
fn several_attributes_stack_on_one_declaration() {
    let module = expect_clean("@stable\n@deprecated(\"use g\")\nfn f() = 1\n");
    let decl = &module.decls[0];
    assert!(decl.has_attribute("stable"));
    assert!(decl.has_attribute("deprecated"));
    assert!(!decl.has_attribute("jit"));
}

#[test]
fn an_attribute_before_private_reaches_the_declaration() {
    let module = expect_clean("@jit\nprivate fn hot() = 1\n");
    let decl = &module.decls[0];
    assert!(decl.private);
    assert!(decl.has_attribute("jit"));
    assert!(matches!(decl.kind, DeclKind::Fn { .. }));
}
