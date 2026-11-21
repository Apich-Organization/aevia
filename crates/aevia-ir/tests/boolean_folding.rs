//! Release IR folding of required, statement-free Boolean operands.

use aevia_ir::Atom;
use aevia_ir::Rhs;
use aevia_ir::Stmt;
use aevia_ir::Tail;
use std::fmt::Write;

#[test]
fn literal_operands_remove_dead_source_branches_and_keep_selected_spans() {
    for condition in ["true && true", "false || true"] {
        let mut source =
            format!("fn choose(): Int =\n    if {condition} then\n        42\n    else\n");
        for index in 0..128 {
            writeln!(source, "        let dead{index} = !false").unwrap();
        }
        source.push_str("        0\n");
        let lexed = aevia_syntax::lex(0, &source).unwrap();
        let (ast, parsed) = aevia_syntax::parse_module(0, &source, &lexed);
        assert!(!parsed.has_errors(), "{:?}", parsed.items);
        let checked = aevia_types::check::check_module(0, &ast);
        assert!(
            !checked.diagnostics.has_errors(),
            "{:?}",
            checked.diagnostics.items
        );
        let mut ir = aevia_ir::lower::lower_module(
            &ast,
            &checked.module.rssn_calls,
            &checked.module.dictionaries,
        );
        let body = &ir.fns[0].function.body;
        let [Stmt::Let(test)] = body.stmts.as_slice() else {
            panic!("conditional binding")
        };
        assert!(matches!(test.rhs, Rhs::AndOr { .. }));
        let test_span = test.span;
        let Tail::If {
            then_block,
            else_block,
            ..
        } = &body.tail
        else {
            panic!("condition before release optimization")
        };
        assert_eq!(else_block.stmts.len(), 128);
        let Tail::Return {
            span: selected_span, ..
        } = then_block.tail
        else {
            panic!("return from the selected source arm")
        };

        aevia_ir::optimize::module(&mut ir);
        let optimized = &ir.fns[0].function.body;
        assert!(
            matches!(optimized.stmts.as_slice(), [Stmt::Let(test)]
                if matches!(test.rhs, Rhs::Atom(Atom::Bool(true))) && test.span == test_span),
            "{condition}: {:?}",
            optimized.stmts
        );
        assert!(matches!(optimized.tail, Tail::Return { span, .. } if span == selected_span));
    }
}
