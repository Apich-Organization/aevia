//! The rank-generic built-ins are templates over the rank: a call at a rank
//! above the enumerated ones is instantiated at the rank of its arguments.

use aevia_syntax::lex;
use aevia_syntax::parse_module;
use aevia_types::builtins;
use aevia_types::check::check_module;

/// The error codes `source` produces.
fn codes_of(source: &str) -> Vec<String> {
    let lexed = lex(0, source).expect("lexes");
    let (module, parsed) = parse_module(0, source, &lexed);
    assert!(!parsed.has_errors(), "{source}");
    check_module(0, &module)
        .diagnostics
        .items
        .iter()
        .filter(|diagnostic| diagnostic.severity == aevia_syntax::Severity::Error)
        .map(|diagnostic| diagnostic.code.to_string())
        .collect()
}

fn assert_clean(source: &str) {
    assert_eq!(codes_of(source), Vec::<String>::new(), "{source}");
}

const T6: &str = "let t = zeros(2, 2, 2, 2, 2, 2)\n";

#[test]
fn reductions_and_maps_work_at_rank_six() {
    for call in [
        "let r: Float = sum(t)",
        "let r: Float = mean(t)",
        "let r: Float = norm(t)",
        "let r: Float = var(t)",
        "let r: Tensor<6, Float> = sum(t, 5)",
        "let r: Tensor<6, Float> = map(fn (x) => x + 1.0, t)",
        "let r: Tensor<6, Float> = map2(fn (x, y) => x + y, t, t)",
        "let r: Tensor<1, Float> = flatten(t)",
        "let r: Tensor<6, Float> = reverse_axis(t, 2)",
        "let r: Tensor<6, Float> = normalize(t)",
        "let r: Tensor<5, Float> = at(t, 0)",
    ] {
        assert_clean(&format!("{T6}{call}\n"));
    }
}

#[test]
fn the_fft_family_works_at_rank_six() {
    for call in [
        "let r: Tensor<6, Complex> = fftn(t)",
        "let r: Tensor<6, Complex> = ifftn(t)",
        "let r: Tensor<6, Complex> = fft2(t)",
        "let r: Tensor<6, Complex> = fft_axis(t, 3)",
        "let r: Tensor<6, Complex> = fftn_axes(t, [0, 5])",
        "let r: Tensor<6, Complex> = rfftn(t)",
        "let r: Tensor<6, Complex> = rfftn_axes(t, [1, 4])",
        "let r: Tensor<6, Float> = irfftn_axes(rfftn_axes(t, [1, 4]), [1, 4], 2)",
        "let r: Tensor<6, Float> = irfftn(rfftn(t), 2)",
        "let r: Tensor<6, Float> = fftshift(t)",
        "let r: Tensor<6, Float> = ifftshift_axis(t, 1)",
    ] {
        assert_clean(&format!("{T6}{call}\n"));
    }
}

#[test]
fn the_sequence_family_sees_rank_five_rows_of_a_rank_six_tensor() {
    for call in [
        "let r: Tensor<6, Float> = reverse(t)",
        "let r: Tensor<6, Float> = filter(fn (row) => sum(row) > 0.0, t)",
        "let r: Tensor<6, Float> = take(1, t)",
        "let r: Tensor<6, Float> = concat(t, t)",
        "let r: Bool = any(fn (row) => sum(row) > 0.0, t)",
        "let r: Option<Tensor<5, Float>> = first(t)",
        "let r: Float = fold(fn (acc, row) => acc + sum(row), 0.0, t)",
        "let r: Tensor<6, Float> = sort_float(t, true)",
        "let r: Tensor<6, Float> = sort_by(fn (a, b) => total_compare(1, 2), t)",
        "let r: Int = length(t)",
    ] {
        assert_clean(&format!("{T6}{call}\n"));
    }
}

#[test]
fn zip_and_enumerate_yield_rows() {
    assert_clean(&format!(
        "{T6}let p: Tensor<1, {{ index: Int, value: Tensor<5, Float> }}> = enumerate(t)\n"
    ));
    assert_clean(
        "let a = [[1, 2], [3, 4]]\nlet z: Tensor<1, (Tensor<1, Int>, String)> = zip(a, [\"x\", \"y\"])\n",
    );
    assert_clean(
        "let a = [[1, 2], [3, 4]]\nlet z: Tensor<1, (Tensor<1, Int>, Tensor<1, Int>)> = zip(a, a)\n",
    );
}

#[test]
fn rows_must_be_ordered_to_be_sorted() {
    // `Float` has no `TotalOrd`; `sort_float` is the float sort.
    assert_eq!(
        codes_of("let a = sort([[1.5, 2.0], [0.5, 1.0]])\n"),
        vec!["E0310".to_string()]
    );
    assert_clean("let a = sort([[2, 1], [1, 5]])\nlet b = sort_axis([[2, 1], [1, 5]], 1)\n");
    assert_eq!(
        codes_of("let a = sort_by_key(fn (row) => 1.5, [[2, 1], [1, 5]])\n"),
        vec!["E0310".to_string()]
    );
}

#[test]
fn a_rank_mismatch_is_still_e0302_at_any_rank() {
    assert_eq!(
        codes_of(&format!("{T6}let r = fft2(zeros(3))\n")),
        vec!["E0302".to_string()]
    );
    assert_eq!(
        codes_of(&format!("{T6}let r: Tensor<5, Float> = sum(t, 5)\n")),
        vec!["E0302".to_string()]
    );
}

#[test]
fn the_table_enumerates_up_to_the_static_rank_and_templates_above() {
    assert_eq!(builtins::STATIC_RANK, 4);
    assert!(builtins::rank_schemes_for("fftn", &[3]).is_empty());
    assert_eq!(builtins::rank_schemes_for("fftn", &[6]).len(), 3);
    assert_eq!(builtins::rank_schemes_for("sum", &[7]).len(), 2);
    assert!(builtins::rank_schemes_for("not_a_function", &[7]).is_empty());
    assert_eq!(builtins::rank_schemes_for("zip", &[5, 1]).len(), 3);
}

#[test]
fn argsort_orders_rows_of_an_ordered_element_type() {
    assert_clean("let r: Tensor<1, Int> = argsort([[2, 1], [1, 5]])\n");
    assert_eq!(
        codes_of("let t = zeros(2, 2, 2, 2, 2, 2)\nlet r = argsort(t)\n"),
        vec!["E0310".to_string()]
    );
}
