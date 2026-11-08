//! §13.9.4: `@jit(target = …)` names a device; the CPU is the default target,
//! and a device this build has no backend for is a capability gap (`E1001`),
//! not a syntax error.

use aevia_syntax::lex;
use aevia_syntax::parse_module;
use aevia_types::Capabilities;
use aevia_types::CheckOptions;
use aevia_types::check::check_module_with_options;

fn codes(
    source: &str,
    capabilities: &[&str],
) -> Vec<String> {
    let lexed = lex(0, source).expect("lexes");
    let (module, parse) = parse_module(0, source, &lexed);
    assert!(!parse.has_errors(), "{:?}", parse.items);
    let options = CheckOptions {
        capabilities: Capabilities::from_names(capabilities.iter().copied()),
    };
    let result = check_module_with_options(0, &module, &[], &options);
    result
        .diagnostics
        .items
        .iter()
        .filter(|item| item.severity == aevia_syntax::Severity::Error)
        .map(|item| item.code.clone())
        .collect()
}

#[test]
fn the_cpu_target_is_the_default_jit() {
    let source = "@jit(target = Cpu)\nfn f(x: Float): Float = x * x\n";
    assert!(codes(source, &["core.jit"]).is_empty());
}

#[test]
fn a_gpu_target_is_a_capability_gap() {
    let source = "@jit(target = Gpu(0))\nfn f(x: Float): Float = x * x\n";
    assert_eq!(codes(source, &["core.jit"]), ["E1001"]);
    assert!(codes(source, &["core.jit", "core.gpu"]).is_empty());
}

#[test]
fn other_jit_arguments_are_rejected() {
    let source = "@jit(fast = true)\nfn f(x: Float): Float = x * x\n";
    assert_eq!(codes(source, &["core.jit"]), ["E0210"]);
    let source = "@jit(target = Tpu)\nfn f(x: Float): Float = x * x\n";
    assert_eq!(codes(source, &["core.jit"]), ["E0210"]);
}
