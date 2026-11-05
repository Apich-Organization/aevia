//! Shape type arguments at run time (§8.4, item 4 of the round).
//!
//! `reshape`, `zeros_of` and `ones_of` name their target shape in a *type*
//! argument. The runtime reads that argument from the AST node, so these tests
//! go through a program wherever the parser accepts the spelling, and through
//! `call_typed` for the `[3, 2]` spelling the parser does not accept yet
//! (recorded in `.workbuddy-ai/audit/dicts.md`).

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::builtins::call_typed;
use aevia_core::compile;
use aevia_core::value::Value;
use aevia_syntax::Span;
use aevia_syntax::ast::NatAst;
use aevia_syntax::ast::ShapeAst;
use aevia_syntax::ast::ShapeItem;
use aevia_syntax::ast::Ty;
use aevia_syntax::ast::TyKind;

fn interp(source: &str) -> Interp {
    let compilation = compile("shapes.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "compile errors: {:?}",
        compilation
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let module = compilation.module.expect("module");
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = interp.load(&module);
    assert!(
        !diagnostics.has_errors(),
        "load errors: {:?}",
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    interp
}

fn tensor(
    source: &str,
    name: &str,
) -> std::rc::Rc<aevia_core::value::TensorData> {
    let mut interp = interp(source);
    match interp.eval_toplevel(name).expect("the name is bound") {
        | Value::Tensor(tensor) => tensor,
        | other => panic!("expected a tensor, found {}", other.type_name()),
    }
}

fn extents(shape: &[usize]) -> Vec<usize> {
    shape.to_vec()
}

#[test]
fn zeros_of_builds_the_shape_it_is_given() {
    let tensor = tensor("let z = zeros_of<Tensor<[2, 3], Float>>()\n", "z");
    assert_eq!(extents(&tensor.shape), vec![2, 3]);
    assert_eq!(tensor.storage.len(), 6);
}

#[test]
fn ones_of_builds_the_shape_it_is_given() {
    let tensor = tensor("let o = ones_of<Tensor<[4], Float>>()\n", "o");
    assert_eq!(extents(&tensor.shape), vec![4]);
    let values: Vec<f64> = tensor
        .elements()
        .iter()
        .map(|element| element.as_f64().expect("a number"))
        .collect();
    assert_eq!(values, vec![1.0, 1.0, 1.0, 1.0]);
}

#[test]
fn reshape_changes_the_shape_and_keeps_the_elements() {
    let tensor = tensor(
        "let r = reshape<[3, 2]>(zeros(2, 3))\nlet t = match r with\n    | Ok(t) -> t\n    | Err(_) -> zeros_of<Tensor<[3, 2], Float>>()\n",
        "t",
    );
    assert_eq!(extents(&tensor.shape), vec![3, 2]);
    assert_eq!(tensor.storage.len(), 6);
}

#[test]
fn reshape_refuses_a_different_element_count() {
    // §8.4: the element count is checked at run time. The declared type is a
    // `Result`, so the failure is the `Err` the type promises.
    let mut interp = interp("let r = reshape<Tensor<[4], Float>>(zeros(2, 3))\n");
    let Value::Variant(result) = interp.eval_toplevel("r").expect("r is bound") else {
        panic!("reshape answers with a Result");
    };
    assert_eq!(result.index, 1, "Err");
    let Value::Variant(error) = &result.fields[0] else {
        panic!("the payload is a ShapeError");
    };
    assert_eq!(error.name, "ElementCount");
}

#[test]
fn reshape_dyn_reports_the_same_failure_as_a_result() {
    let mut interp = interp("let r = reshape_dyn(zeros(2, 3), [4])\n");
    let Value::Variant(result) = interp.eval_toplevel("r").expect("r is bound") else {
        panic!("reshape_dyn answers with a Result");
    };
    assert_eq!(result.index, 1, "Err");
}

#[test]
fn a_shape_written_as_a_shape_argument_works() {
    // The specification spells the argument `reshape<[3, 2]>`; the parser does
    // not reach `type_arg` from an expression's `<…>` list yet, so the AST
    // node is built here. This is the spelling the runtime already handles.
    let ty = Ty::new(
        TyKind::ShapeArg(ShapeAst::Refined(vec![
            ShapeItem::Known(NatAst::Lit(3)),
            ShapeItem::Known(NatAst::Lit(2)),
        ])),
        Span::default(),
    );
    let mut interp = Interp::new(&aevia_types::CheckedModule::default());
    let matrix = Value::Tensor(std::rc::Rc::new(aevia_core::value::TensorData {
        shape: vec![2, 3],
        storage: aevia_core::value::Storage::Dense(std::rc::Rc::new(vec![
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0,
        ])),
        dim: aevia_dim::Dim::one(),
        display: None,
    }));
    let reshaped = call_typed(&mut interp, "reshape", &[ty], vec![matrix], Span::default())
        .expect("a 2×3 tensor has six elements");
    let Value::Variant(result) = reshaped else {
        panic!("reshape answers with a Result");
    };
    assert_eq!(result.index, 0, "Ok");
    let Value::Tensor(tensor) = &result.fields[0] else {
        panic!("Ok carries a tensor");
    };
    assert_eq!(tensor.shape, vec![3, 2]);
}

#[test]
fn reshape_without_a_type_argument_names_the_requirement() {
    // `reshape` is typed `∀a. a -> Result<a, ShapeError>`, so the call checks;
    // without the type argument there is no shape to build.
    let compilation = compile("shapes.aev", "let r = reshape(zeros(2, 3))\n");
    assert!(!compilation.diagnostics.has_errors());
    let module = compilation.module.expect("module");
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = interp.load(&module);
    assert_eq!(
        diagnostics
            .items
            .iter()
            .map(|item| item.code.as_str())
            .collect::<Vec<_>>(),
        vec!["E0310"]
    );
}
