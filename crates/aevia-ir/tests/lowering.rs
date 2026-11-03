//! The shape of the IR: ANF naming, closure capture order, spans, tail calls.
//!
//! These tests inspect the lowered form rather than a result, because the
//! properties they check — one name per intermediate, a deterministic capture
//! vector, a span on every node, a call in tail position — are what the
//! interpreter and a future JIT rely on.

#![allow(clippy::result_large_err)]

use aevia_ir::lower::lower_module;
use aevia_ir::{Atom, CallKind, Function, Pattern, Rhs, Stmt, Tail};
use aevia_syntax::ast::BinOp;

fn lower(source: &str) -> aevia_ir::Module {
    let mut sources = aevia_syntax::SourceMap::new();
    let file = sources.add("test.aev", source);
    let lexed = aevia_syntax::lex(file, source).expect("the source lexes");
    let (module, parse) = aevia_syntax::parse_module(file, source, &lexed);
    assert!(
        !parse.has_errors(),
        "parse errors: {:?}",
        parse
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let checked = aevia_types::check::check_module(file, &module);
    assert!(
        !checked.diagnostics.has_errors(),
        "check errors: {:?}",
        checked
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    lower_module(
        &module,
        &checked.module.rssn_calls,
        &checked.module.dictionaries,
    )
}

fn top_function<'a>(module: &'a aevia_ir::Module, name: &str) -> &'a std::rc::Rc<Function> {
    &module
        .fns
        .iter()
        .find(|decl| decl.name == name)
        .unwrap_or_else(|| panic!("no `fn {name}`"))
        .function
}

#[test]
fn every_intermediate_is_named() {
    // `1 + 2 * 3` becomes two statements: the product into one slot, the sum
    // into the next. A call would take the atoms, not the expressions.
    let module = lower("let r = 1 + 2 * 3\n");
    let decl = &module.lets[0];
    assert_eq!(decl.block.stmts.len(), 2, "{:#?}", decl.block);
    let Stmt::Let(first) = &decl.block.stmts[0] else {
        panic!("expected a let");
    };
    let Rhs::Binary {
        op: BinOp::Mul,
        left: Atom::Int(left),
        right: Atom::Int(right),
    } = &first.rhs
    else {
        panic!("expected `2 * 3`, found {:?}", first.rhs);
    };
    assert_eq!((left.to_string(), right.to_string()), ("2".into(), "3".into()));
    let Stmt::Let(second) = &decl.block.stmts[1] else {
        panic!("expected a let");
    };
    let Rhs::Binary {
        op: BinOp::Add,
        left: Atom::Int(_),
        right: Atom::Local(slot),
    } = &second.rhs
    else {
        panic!("expected `1 + slot`, found {:?}", second.rhs);
    };
    // The right operand of the sum is the atom the product produced, not a
    // second copy of the product.
    assert_eq!(*slot, first.slot);
    match &decl.block.tail {
        Tail::Return { atom, .. } => assert_eq!(*atom, Atom::Local(second.slot)),
        other => panic!("expected a return, found {other:?}"),
    }
}

#[test]
fn a_call_takes_atoms_and_keeps_its_site() {
    // A top-level value is in tail position, so the call is the block's tail;
    // its arguments are atoms, not expressions.
    let module = lower("fn add(a: Int, b: Int): Int = a + b\nlet r = add(1, 2)\n");
    let decl = &module.lets[0];
    let Tail::Call(call) = &decl.block.tail else {
        panic!("expected a tail call, found {:?}", decl.block.tail);
    };
    assert!(matches!(call.kind, CallKind::Normal));
    assert!(matches!(call.callee, Atom::Global(ref name) if name == "add"));
    assert_eq!(call.args.len(), 2);
    assert!(call.args.iter().all(|arg| matches!(arg, Atom::Int(_))));
    // The call site keeps the span of the call expression, which is what a
    // stack frame reports.
    assert!(!call.span.is_empty());
}

#[test]
fn captures_are_ordered_by_first_use() {
    // `b + x + a` mentions `b` first and `a` last, so the environment is
    // [b, a] — deterministic, and independent of how many times each is used.
    let module = lower(
        "fn outer(a: Int, b: Int): Int =\n    let f = fn (x: Int) => b + x + a\n    f(1)\n",
    );
    let outer = top_function(&module, "outer");
    assert_eq!(outer.params.len(), 2);
    // The body binds `f` first; find the closure in the body's statements.
    let mut found = None;
    for stmt in &outer.body.stmts {
        if let Stmt::Let(item) = stmt {
            if let Rhs::Closure(function) = &item.rhs {
                found = Some(function.clone());
            }
        }
    }
    let inner = found.expect("the body builds a closure");
    assert_eq!(
        inner.captures,
        vec![
            aevia_ir::Capture::Local(1),
            aevia_ir::Capture::Local(0),
        ],
        "capture order must be first use, found {:?}",
        inner.captures
    );
    // `x` is the inner parameter, so it is a slot of the inner frame, not a
    // capture.
    assert!(matches!(inner.params[0], Pattern::Bind { .. }));
}

#[test]
fn a_nested_capture_is_threaded_through_every_function() {
    // `a` is bound two functions up: the middle function must capture it so
    // the innermost one can.
    let module = lower("fn outer(a: Int): Int =\n    let g = fn () =>\n        let h = fn () => a\n        h()\n    g()\n");
    let outer = top_function(&module, "outer");
    let middle = closure_in(&outer.body.stmts).expect("g");
    assert_eq!(middle.captures, vec![aevia_ir::Capture::Local(0)]);
    let inner = closure_in(&middle.body.stmts).expect("h");
    assert_eq!(inner.captures, vec![aevia_ir::Capture::Capture(0)]);
}

fn closure_in(stmts: &[Stmt]) -> Option<std::rc::Rc<Function>> {
    for stmt in stmts {
        if let Stmt::Let(item) = stmt {
            match &item.rhs {
                Rhs::Closure(function) | Rhs::RecClosure { function, .. } => {
                    return Some(function.clone())
                }
                _ => {}
            }
        }
    }
    None
}

#[test]
fn every_node_keeps_its_span() {
    let source = "fn f(a: Int): Int =\n    if a > 0 then a * 2 else 0 - a\n";
    let module = lower(source);
    let function = top_function(&module, "f");
    let mut spans = Vec::new();
    collect_spans(&function.body, &mut spans);
    assert!(!spans.is_empty());
    for span in spans {
        assert!(
            usize::try_from(span.end).unwrap_or(usize::MAX) <= source.len(),
            "span {span:?} is outside the source"
        );
        assert!(span.end > span.start, "a node lost its span: {span:?}");
    }
}

fn collect_spans(block: &aevia_ir::Block, out: &mut Vec<aevia_syntax::Span>) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let(item) => out.push(item.span),
            Stmt::Bind { span, .. } => out.push(*span),
        }
    }
    match &block.tail {
        Tail::Return { span, .. } => out.push(*span),
        Tail::Call(call) => out.push(call.span),
        Tail::If {
            then_block,
            else_block,
            span,
            ..
        } => {
            out.push(*span);
            collect_spans(then_block, out);
            collect_spans(else_block, out);
        }
        Tail::Match { arms, span, .. } => {
            out.push(*span);
            for arm in arms {
                out.push(arm.span);
                collect_spans(&arm.body, out);
            }
        }
    }
}

#[test]
fn a_self_call_in_tail_position_is_marked_as_one() {
    let module = lower("fn count(n: Int): Int = if n == 0 then 0 else count(n - 1)\n");
    let function = top_function(&module, "count");
    let Tail::If { else_block, .. } = &function.body.tail else {
        panic!("expected the body to be an `if`, found {:?}", function.body.tail);
    };
    match &else_block.tail {
        Tail::Call(call) => {
            assert!(matches!(call.kind, CallKind::Normal));
            assert!(matches!(call.callee, Atom::Global(ref name) if name == "count"));
        }
        other => panic!("a tail call must be a `Tail::Call`, found {other:?}"),
    }
}

#[test]
fn a_non_tail_call_is_not_marked_as_one() {
    // `1 + count(n - 1)` needs the result, so it is a statement, not a tail.
    let module = lower("fn count(n: Int): Int = if n == 0 then 0 else 1 + count(n - 1)\n");
    let function = top_function(&module, "count");
    let Tail::If { else_block, .. } = &function.body.tail else {
        panic!("expected an `if`");
    };
    assert!(matches!(else_block.tail, Tail::Return { .. }));
    assert!(else_block
        .stmts
        .iter()
        .any(|stmt| matches!(stmt, Stmt::Let(item) if matches!(item.rhs, Rhs::Call(_)))));
}

#[test]
fn a_pinned_rssn_call_carries_its_overload() {
    let module = lower("let y = Rssn.sin(0.0)\n");
    let decl = &module.lets[0];
    let Tail::Call(call) = &decl.block.tail else {
        panic!("expected a tail call, found {:?}", decl.block.tail);
    };
    assert!(
        matches!(call.kind, CallKind::Rssn { .. }),
        "the checker pinned `Rssn.sin`, found {:?}",
        call.kind
    );
}

#[test]
fn a_do_block_binds_its_pattern_inside_the_thunk() {
    // The `let` is deferred: its value is lowered in the thunk, and the rest of
    // the block reads the binding as a slot of that frame.
    let module = lower(
        "fn main(): IO Unit = do\n    let x = 1 + 2\n    println(\"{x}\")\n",
    );
    let main = top_function(&module, "main");
    let Stmt::Let(item) = &main.body.stmts[0] else {
        panic!("expected the `do` plan");
    };
    let Rhs::Do(plan) = &item.rhs else {
        panic!("expected a `do` plan, found {:?}", item.rhs);
    };
    assert!(plan.prefix.is_empty());
    let aevia_ir::DoTerminal::Let { thunk } = &plan.terminal else {
        panic!("expected a deferred `let`");
    };
    assert!(thunk.params.is_empty());
    assert!(thunk.nslots >= 1);
    // The thunk's tail is the rest of the block, which mentions `x` as a slot.
    let Tail::Return { atom, .. } = &thunk.body.tail else {
        panic!("expected a return");
    };
    let Atom::Local(slot) = atom else {
        panic!("expected a slot, found {atom:?}");
    };
    assert!(thunk
        .body
        .stmts
        .iter()
        .any(|stmt| matches!(stmt, Stmt::Let(item) if item.slot == *slot)));
}
