//! Span-independent declaration identity includes syntax that Fn shape erases.
use aevia_syntax::fingerprint;
use aevia_syntax::lex;
use aevia_syntax::parse_module;
fn fingerprint_at(
    file: u32,
    source: &str,
) -> String {
    let tokens = lex(file, source).unwrap();
    let (module, diagnostics) = parse_module(file, source, &tokens);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    fingerprint::declaration(&module.decls[0])
}
#[test]
fn declaration_encoding_ignores_file_ids_but_keeps_body_and_ordered_headers() {
    let source = "fn choose<a, b>(x: Int) = type_rep<a>()\n";
    assert_eq!(fingerprint_at(0, source), fingerprint_at(237, source));
    for changed in [
        "fn choose<b, a>(x: Int) = type_rep<a>()\n",
        "fn choose<a, b>(x: Int) = type_rep<b>()\n",
        "fn choose<a, b>(x: Int) = type_rep<Int>()\n",
    ] {
        assert_ne!(fingerprint_at(0, source), fingerprint_at(0, changed));
    }
    assert_ne!(
        fingerprint_at(0, "fn literal<r: Rep>(): r = 1\n"),
        fingerprint_at(0, "fn literal<r: Rep>(): r = 1.0\n")
    );
}
#[test]
fn a_deep_expression_is_encoded_iteratively_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(4 * 1024 * 1024)
        .spawn(|| {
            let source = format!("fn long() = 1{}\n", " + 1".repeat(4096));
            let encoded = fingerprint_at(3, &source);
            assert_eq!(encoded.matches("binary(Add,").count(), 4096);
            assert_eq!(encoded.matches("int:1").count(), 4097);
        })
        .unwrap()
        .join()
        .unwrap();
}
