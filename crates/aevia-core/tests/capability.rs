//! §9.6.1 — `@jit` is gated by the backend's `core.jit` capability.
//!
//! The checker owns the gate (a capability is static), and `CheckOptions`'
//! default reads the capability set of the `aevia-rssn` registry this build is
//! linked against. The default build rejects `@jit` with `E1001`; the optional
//! JIT build accepts it and advertises the capability.

#![allow(clippy::result_large_err)]

use aevia_core::compile;

fn diagnostics(source: &str) -> Vec<(String, String)> {
    compile("capability.aev", source)
        .diagnostics
        .items
        .iter()
        .map(|item| (item.code.clone(), item.message.clone()))
        .collect()
}

#[test]
#[cfg(not(feature = "jit"))]
fn jit_is_a_named_capability_gap_in_this_build() {
    // The registry is the single capability table (§13.8.2); the gate reads it.
    let capabilities = aevia_rssn::registry().capabilities.clone();
    assert!(
        !capabilities.jit,
        "this build is expected to have features.jit = false"
    );

    let errors = diagnostics(
        r#"@jit
fn hot(x: Float): Float = x * 2.0

fn main(): IO Unit = do
    println(show(hot(2.0)))
"#,
    );
    assert_eq!(
        errors
            .iter()
            .map(|(code, _)| code.as_str())
            .collect::<Vec<_>>(),
        vec!["E1001"],
        "the only diagnostic is the capability gap: {errors:?}"
    );
    let (_, message) = &errors[0];
    assert!(
        message.contains("core.jit"),
        "the gap names the capability: {message}"
    );

    // The same function without `@jit` is legal: the language feature exists,
    // only the backend capability is missing (§13.1 rule 1).
    let errors = diagnostics(
        r#"fn hot(x: Float): Float = x * 2.0

fn main(): IO Unit = do
    println(show(hot(2.0)))
"#,
    );
    assert!(errors.is_empty(), "no @jit, no gap: {errors:?}");
}

#[test]
#[cfg(feature = "jit")]
fn jit_capability_is_available_when_enabled() {
    assert!(aevia_rssn::registry().capabilities.jit);
    assert!(diagnostics("@jit\nfn hot(x: Float): Float = x * 2.0\n").is_empty());
}

#[test]
fn jit_is_only_allowed_on_a_function() {
    let errors = diagnostics(
        r#"@jit
let x = 1
"#,
    );
    assert_eq!(
        errors
            .iter()
            .map(|(code, _)| code.as_str())
            .collect::<Vec<_>>(),
        vec!["E0210"],
        "@jit on a non-fn is a malformed attribute: {errors:?}"
    );
}

#[test]
fn a_caller_owned_capability_set_can_grant_jit() {
    // §13.8.1: a consumer that owns the manifest registry can check against the
    // exact backend it will run on. This is the entry point the CLI/driver uses
    // when it has a capability set of its own; with `core.jit` granted the
    // attribute is accepted independently of the linked backend.
    let mut sources = aevia_syntax::SourceMap::new();
    let file = sources.add("capability.aev", "@jit\nfn hot(x: Float): Float = x\n");
    let source = "@jit\nfn hot(x: Float): Float = x\n";
    let lexed = aevia_syntax::lex(file, source).expect("lex");
    let (module, parsed) = aevia_syntax::parse_module(file, source, &lexed);
    assert!(!parsed.has_errors(), "parse: {parsed:?}");

    let options = aevia_types::check::CheckOptions {
        capabilities: aevia_types::check::Capabilities::from_names(["core.jit"]),
    };
    let checked = aevia_types::check::check_module_with_options(file, &module, &[], &options);
    assert!(
        !checked.diagnostics.has_errors(),
        "a granted capability accepts @jit: {:?}",
        checked
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
}
