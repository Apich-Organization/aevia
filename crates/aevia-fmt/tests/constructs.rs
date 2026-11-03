//! Round-trip tests for constructs the example corpus does not contain.
//!
//! The corpus proves the formatter handles the programs the specification is
//! written against; these snippets cover the rest of appendix A, one construct
//! at a time, so a gap names itself.

use aevia_fmt::{fingerprint, format_source};
use aevia_syntax::{lex, parse_module, Module};

fn parse(source: &str) -> Module {
    let lexed = lex(0, source).unwrap_or_else(|diagnostics| {
        panic!(
            "does not lex: {:?}",
            diagnostics
                .items
                .iter()
                .map(|item| (&item.code, &item.message))
                .collect::<Vec<_>>()
        )
    });
    let (module, diagnostics) = parse_module(0, source, &lexed);
    assert!(
        !diagnostics.has_errors(),
        "does not parse: {:?}",
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    module
}

/// Formats `source`, then checks the tree survived and the result is stable.
///
/// The exact layout is a choice, not a contract, so only snippets already
/// written in the canonical form are compared to their own text.
fn round_trip(source: &str) -> String {
    let formatted = format_source(source).unwrap_or_else(|error| panic!("{error}\n{source}"));
    assert_eq!(
        fingerprint::module(&parse(source)),
        fingerprint::module(&parse(&formatted)),
        "the tree changed:\n{source}\n--- became ---\n{formatted}"
    );
    assert_eq!(
        format_source(&formatted).expect("formats again"),
        formatted,
        "not idempotent"
    );
    formatted
}

#[test]
fn imports_and_opens() {
    for source in [
        "import A.B\n",
        "import A.B as C\n",
        "import A.B { x, y }\n",
        "open A.B\n",
        "open A.B { x }\n",
        "module A.B\n\nimport C.D\n",
    ] {
        assert_eq!(round_trip(source), source);
    }
}

#[test]
fn dimensions_and_units() {
    for source in [
        "dimension Currency\n",
        "dimension Area = L^2\n",
        "dimension Speed = L/T\n",
        "unit EUR : Currency = 27/25 USD\n",
        "unit deg : 1 = pi/180 rad prefixable\n",
        // A scale is a reduced rational, so `27315/100` is written `5463/20`:
        // the same number, and the only form the tree can reproduce.
        "unit K : Temp = 1 K offset 5463/20\n",
    ] {
        assert_eq!(round_trip(source), source);
    }
}

#[test]
fn types_and_classes() {
    for source in [
        "type Vec2 = { x: Real[L], y: Real[L] }\n",
        "type Shape = | Circle(Real[L]) | Rect(Real[L], Real[L])\n",
        "type Table2<r>\n",
        "type Tree<a> = | Leaf | Node(Tree<a>, a, Tree<a>)\n",
        "class Pretty a where\n    pretty: a -> String\n",
        "instance Pretty for Vec2 where\n    pretty(v: Vec2): String = \"v\"\n",
        "derive Eq, Show for Vec2\n",
        "fn area(s: Shape): Real[L^2] =\n    match s with\n    | Circle(r) -> r ^ 2\n    | Rect(w, h) -> w * h\n",
    ] {
        assert_eq!(round_trip(source), source);
    }
}

#[test]
fn every_expression_form() {
    for source in [
        "let a = if 1 < 2 then 1 else 2\n",
        // A `match` after `=` starts on the next line, like a block.
        "let a =\n    match b with\n    | 1 -> 0\n    | n if n > 2 -> n\n    | _ -> -1\n",
        "let a = let x = 1 in x + 1\n",
        "let a = fn (x: Int, y: Int): Int => x + y\n",
        "let a = fn x => x\n",
        "let a = { x = 1, y = 2 }\n",
        "let a = { b with x = 3 }\n",
        "let a = (1, 2.0, \"s\")\n",
        "let a = [1, 2, 3]\n",
        "let a = b[0]\n",
        "let a = b[0, 1]\n",
        "let a = b[0..2]\n",
        "let a = b[..]\n",
        "let a = b[1..=2]\n",
        "let a = b[..2]\n",
        "let a = c.x\n",
        "let a = f(1)(2)\n",
        "let a = zeros<Float>(2, 3)\n",
        "let a = x?\n",
        "let a = 5!\n",
        "let a = 5!!\n",
        "let a = -x\n",
        "let a = \u{221a}x\n",
        "let a = !true\n",
        "let a = 1 :: rest\n",
        "let a = 1 + 2 * 3 - 4 / 5 @ m\n",
        "let a = (1 + 2) * 3\n",
        "let a = 2 ^ (1 + 1)\n",
        "let a = (-2) ^ 2\n",
        "let a = 3 m/s^2\n",
        "let a = 50 %\n",
        "let a = ()\n",
        "let a = x : Real[L]\n",
        "let a = \"tab\\there {x + 1} and \\{literal\\}\"\n",
        "let a = 0.1\n",
        "let a = 1e-12\n",
        "let a = 255\n",
    ] {
        assert_eq!(round_trip(source), source);
    }
}

#[test]
fn the_pipe_operators_are_written_as_the_call_they_are() {
    // `x |> f` is sugar for `f(x)`: the parser builds the call, so the tree
    // cannot tell the two apart and the formatter writes the call. This is a
    // normalisation of the surface syntax, not a change of meaning.
    assert_eq!(round_trip("let a = x |> f\n"), "let a = f(x)\n");
    assert_eq!(round_trip("let a = f <| x\n"), "let a = f(x)\n");
}

#[test]
fn patterns() {
    // A lambda body follows `=>` on the same line, so the arms of the `match`
    // it contains are written at the enclosing indentation.
    for source in [
        "let f = fn p => match p with\n| _ -> 0\n",
        "let f = fn p => match p with\n| Some(x) -> x\n| None -> 0\n",
        "let f = fn p => match p with\n| { x, y = q } -> x\n",
        "let f = fn p => match p with\n| (a, b) -> a\n",
        "let f = fn p => match p with\n| [a, b, ..rest] -> a\n",
        "let f = fn p => match p with\n| 1 -> 1\n| \"s\" -> 2\n| true -> 3\n| () -> 4\n",
        "let f = fn p => match p with\n| x as y -> y\n",
        // An annotation pattern keeps its brackets; the space before `:` is
        // not part of the tree.
        "let f = fn p => match p with\n| (x: Int) -> x\n",
    ] {
        assert_eq!(round_trip(source), source);
    }
}

#[test]
fn blocks_do_and_tests() {
    for source in [
        "fn f(): Int =\n    let x = 1\n    x + 1\n",
        "fn f(): IO Unit = do\n    let x = 1\n    y <- read()\n    println(\"{x} {y}\")\n",
        "test \"adds\" = 1 + 1 == 2\n",
        // A block branch ends the line, so `else` starts the next one.
        "fn f(): Int = if true then\n    let x = 1\n    x\nelse 0\n",
        "fn f(): Int =\n    fn g(x: Int): Int = x + 1\n    g(1)\n",
    ] {
        assert_eq!(round_trip(source), source);
    }
}

#[test]
fn attributes_are_kept() {
    let source = "@bench\nfn f(): Int = 1\n\n@deprecated(\"use g\")\nfn h(): Int = 2\n";
    assert_eq!(round_trip(source), source);
}

#[test]
fn a_program_that_does_not_lex_is_refused() {
    let error = format_source("let s = \"unclosed\n").expect_err("refuses");
    assert_eq!(error.failure, aevia_fmt::Failure::Lex);
}

#[test]
fn a_comment_inside_an_expression_is_moved_but_kept() {
    // The AST has no place for a comment inside an expression, so it becomes
    // leading trivia of the next statement rather than being deleted.
    let source = "fn f(): Int =\n    let x = g(1, # why\n        2)\n    x\n";
    let formatted = round_trip(source);
    assert!(formatted.contains("# why"), "{formatted}");
}

#[test]
fn a_slice_step_round_trips() {
    // A step is written in the `a:b:s` spelling, which is the only one that
    // carries one; the fingerprint includes it, so a printer that dropped it
    // would fail the tree comparison rather than only the text comparison.
    for source in [
        "let a = b[1:5:2]
",
        "let a = b[::-1]
",
        "let a = b[::2]
",
        "let a = b[:3:2]
",
    ] {
        assert_eq!(round_trip(source), source);
    }
}

#[test]
fn a_colon_slice_without_a_step_is_the_range_it_means() {
    // `b[1:5]` and `b[1..5]` are the same node, so the formatter writes the
    // older spelling and the tree is unchanged.
    assert_eq!(round_trip("let a = b[1:5]
"), "let a = b[1..5]
");
    // The same rule on an axis of a matrix: a full slice with no step is the
    // `..` form, while the stepped axis keeps its `:` spelling.
    assert_eq!(
        round_trip("let a = m[::, ::-1]
"),
        "let a = m[.., ::-1]
"
    );
}
