//! Pattern matrices retain product correlations and rank-one list structure.
use aevia_syntax::SourceMap;
use aevia_syntax::lex;
use aevia_syntax::parse_module;
use aevia_types::check::check_module;
fn diagnostics(source: &str) -> aevia_syntax::Diagnostics {
    let mut sources = SourceMap::new();
    let file = sources.add("patterns.aev", source);
    let tokens = lex(file, source).expect("valid tokens");
    let (module, parsed) = parse_module(file, source, &tokens);
    assert!(!parsed.has_errors(), "{:?}", parsed.items);
    check_module(file, &module).diagnostics
}
fn accepted(source: &str) {
    let diagnostics = diagnostics(source);
    assert!(
        !diagnostics.has_errors(),
        "{source}\n{:?}",
        diagnostics.items
    );
}
fn rejected(
    source: &str,
    code: &str,
) {
    let diagnostics = diagnostics(source);
    assert!(
        diagnostics.items.iter().any(|item| item.code == code),
        "{source}\n{:?}",
        diagnostics.items
    );
}
#[test]
fn diagonal_products_do_not_cover_the_crossed_values() {
    rejected(
        "fn f(p: (Bool, Bool)): Int = match p with\n| (true, true) -> 1\n| (false, false) -> 2\n",
        "E0309",
    );
    rejected(
        "fn f(p: { x: Bool, y: Bool }): Int = match p with\n| { x = true, y = true } -> 1\n| { x = false, y = false } -> 2\n",
        "E0309",
    );
    rejected(
        "type Pair = | Pair(Bool, Bool)\nfn f(p: Pair): Int = match p with\n| Pair(true, true) -> 1\n| Pair(false, false) -> 2\n",
        "E0309",
    );
    accepted(
        "fn f(p: (Bool, Bool)): Int = match p with\n| (true, _) -> 1\n| (false, true) -> 2\n| (false, false) -> 3\n",
    );
}

#[test]
fn irrelevant_columns_preserve_wide_match_coverage_and_usefulness() {
    let fields = (0..48)
        .map(|index| format!("unused{index:02}: Bool"))
        .collect::<Vec<_>>()
        .join(", ");
    let header =
        format!("fn choose(p: {{ {fields}, zstate: Bool, ztag: Bool }}): Int = match p with\n");
    let covering = format!("{header}| {{ zstate = true }} -> 1\n| {{ zstate = false }} -> 0\n");
    accepted(&covering);

    let redundant = diagnostics(&format!("{covering}| _ -> 2\n"));
    assert!(!redundant.has_errors(), "{:?}", redundant.items);
    assert_eq!(
        redundant
            .items
            .iter()
            .filter(|item| item.code == "W0201")
            .count(),
        1,
        "{:?}",
        redundant.items
    );

    rejected(
        &format!("{header}| {{ zstate = true }} -> 1\n| {{ zstate = false }} if true -> 0\n"),
        "E0309",
    );
    rejected(
        &format!(
            "{header}| {{ zstate = true, ztag = true }} -> 1\n\
             | {{ zstate = false, ztag = false }} -> 0\n"
        ),
        "E0309",
    );
}

#[test]
fn unconstrained_recursive_fields_do_not_expand_into_infinite_constructors() {
    accepted(
        "type Chain = | ChainEnd | ChainNext(Chain)\n\
         fn choose(p: { chain: Chain, items: Tensor<1, Int>, state: Bool }): Int = match p with\n\
         | { state = true } -> 1\n\
         | { state = false } -> 0\n",
    );
}
#[test]
fn vector_nil_and_cons_cover_every_length_without_a_wildcard() {
    accepted(
        "fn length(xs: Tensor<1, Int>): Int = match xs with\n| [] -> 0\n| [head, ..tail] -> 1 + length(tail)\n",
    );
    rejected(
        "fn f(xs: Tensor<1, Int>): Int = match xs with\n| [] -> 0\n| [head] -> 1\n",
        "E0309",
    );
    rejected(
        "fn f(xs: Tensor<1, Int>): Int = match xs with\n| [head, ..tail] -> head\n",
        "E0309",
    );
    accepted("fn f(xs: Tensor<[2], Int>): Int = match xs with\n| [a, b] -> a + b\n");
}
#[test]
fn repeated_binders_and_wrong_tuple_or_tensor_shapes_are_diagnosed() {
    for source in [
        "let (x, x) = (1, true)\n",
        "let { left = x, right = x } = { left = 1, right = true }\n",
        "let [x, ..x] = [1, 2]\n",
        "let x as x = 1\n",
        "let { left = x, left = y } = { left = 1 }\n",
        "let (a, b) = (1, 2, 3)\n",
        "let [a, b] = [[1, 2], [3, 4]]\n",
    ] {
        rejected(source, "E0308");
    }
}
#[test]
fn constructor_and_vector_patterns_infer_unannotated_inputs() {
    accepted(
        "fn f(x) = match x with\n| None -> 0\n| Some(value) -> value + 1\nlet result = f(Some(2))\n",
    );
    accepted(
        "fn length(xs): Int = match xs with\n| [] -> 0\n| [head, ..tail] -> 1 + length(tail)\nlet result = length([1, 2, 3])\n",
    );
}
#[test]
fn guards_do_not_count_towards_vector_coverage() {
    rejected(
        "fn f(xs: Tensor<1, Int>): Int = match xs with\n| [] -> 0\n| [head, ..tail] if head > 0 -> 1\n",
        "E0309",
    );
}
#[test]
fn opaque_builtin_errors_are_not_treated_as_empty_enums() {
    rejected(
        "fn f(error: ParseError): Int = match error with\n| 0 -> 1\n",
        "E0309",
    );
}
#[test]
fn deep_matrix_work_is_bounded_and_wildcard_recovers_coverage() {
    let patterns = std::iter::repeat_n("_", 256).collect::<Vec<_>>().join(", ");
    let source =
        format!("fn f(xs: Tensor<[256], Int>): Int = match xs with\n| [{patterns}] -> 1\n");
    rejected(&source, "E0309");
    accepted(&(source + "| _ -> 0\n"));
}
