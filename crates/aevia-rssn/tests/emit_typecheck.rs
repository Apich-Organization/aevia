//! Acceptance: the emitted `Rssn`/`Units`/`Constants` modules type-check.
//!
//! This is a **separate test target on purpose**. Checking a generated module
//! leaves global checker state that a second `check_module` call in the same
//! process can see (the module's own opaque `type Value`/`Symbol`/`OpError`
//! declarations are interned), so the acceptance is measured in a clean
//! process: one test target, one check.
//!
//! `Units.aev` and `Constants.aev` must be clean — that is where the duplicate
//! `m_u` and the folded `hbar` lived. `Rssn.aev` must be clean too: every
//! declaration pins a manifest `OpKey`, and the checker types a literal
//! `Rssn.invoke(op, key, …)` from that pinned overload (`rssn_resolve::
//! invoke_result`, task-34). A duplicate `fn`, an unknown unit, a dimension the
//! language cannot write, a malformed pinned hash, or a `Value`-where-`Real`
//! lie all show up as a diagnostic here.

use aevia_rssn::{codegen, kernel, registry};
use aevia_syntax::{SourceMap, lex, parse_module};

fn parse(name: &str, text: &str) -> aevia_syntax::Module {
    let lexed = lex(0, text).unwrap_or_else(|errors| {
        panic!("{name} does not lex: {:?}", errors.items[0]);
    });
    let (module, diagnostics) = parse_module(0, text, &lexed);
    if diagnostics.has_errors() {
        panic!(
            "{name} does not parse: {}",
            diagnostics.render(&SourceMap::new())
        );
    }
    module
}

#[test]
fn the_emitted_modules_type_check_with_no_diagnostics() {
    // The live manifest is the source; nothing here is pinned to a kernel
    // version, operator count or hash.
    let manifest = kernel::kernel().manifest();
    let files = codegen::files(registry());
    assert_eq!(files.len(), 3, "one file per module");
    let mut failures = Vec::new();
    for (name, text) in &files {
        let module = parse(name, text);
        let checked = aevia_types::check_module(0, &module);
        if checked.diagnostics.has_errors() {
            let sample: Vec<String> = checked
                .diagnostics
                .items
                .iter()
                .filter(|diagnostic| diagnostic.code.starts_with('E'))
                .take(8)
                .map(|diagnostic| {
                    format!(
                        "  {} {:?} @ {}..{} label={:?}",
                        diagnostic.code,
                        diagnostic.message,
                        diagnostic.span.start,
                        diagnostic.span.end,
                        diagnostic.label
                    )
                })
                .collect();
            failures.push(format!(
                "{name} does not type-check ({} diagnostic(s)), first ones:\n{}",
                checked
                    .diagnostics
                    .items
                    .iter()
                    .filter(|diagnostic| diagnostic.code.starts_with('E'))
                    .count(),
                sample.join("\n")
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} generated module(s) with diagnostics (manifest {}):\n{}",
        failures.len(),
        manifest.rssn_version,
        failures.join("\n")
    );
}
