//! Startup phase profile (§9.13 startup budget, round-2 performance work).
//!
//! The question is where a cold `aevia run empty.aev` spends its time:
//! process spawn, lex/parse, type check, `Interp::new` (which seeds the kernel
//! constants), module load, or the program itself. This test measures the
//! in-process phases and prints them; spawn cost is measured by the caller (the
//! CLI) because a test cannot fork itself honestly.
//!
//! Debug numbers are reported as well — they are what the test suite sees, and
//! they bound the work — but the §9.13 target is a release-mode claim.

use aevia_core::{compile, Interp};
use std::time::{Duration, Instant};

const ROUNDS: usize = 25;

fn median(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn measure(mut body: impl FnMut()) -> Duration {
    let mut samples = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let start = Instant::now();
        body();
        samples.push(start.elapsed());
    }
    median(&mut samples)
}

#[test]
fn startup_phases_are_profiled() {
    let source = "fn main(): IO Unit = println(\"\")\n";
    let path = std::env::temp_dir().join(format!(
        "aevia-startup-{}.aev",
        std::process::id()
    ));
    std::fs::write(&path, source).expect("write the empty program");

    let read = measure(|| {
        let _ = std::fs::read_to_string(&path).expect("read");
    });

    let mut source_map = aevia_syntax::SourceMap::new();
    let parse = measure(|| {
        source_map = aevia_syntax::SourceMap::new();
        let file = source_map.add(path.display().to_string(), source);
        let lexed = aevia_syntax::lex(file, source).expect("lex");
        let _ = aevia_syntax::parse_module(file, source, &lexed);
    });

    let compilation = compile("empty.aev", source);
    assert!(!compilation.diagnostics.has_errors());
    let checked = compilation.checked;
    let module = compilation.module.expect("a module");

    let check = measure(|| {
        let _ = aevia_types::check::check_module_with_imports(
            source_map.add("checked.aev", source),
            &module,
            &[],
        );
    });

    let construct = measure(|| {
        let _ = Interp::new(&checked);
    });

    let load = measure(|| {
        let mut interp = Interp::new(&checked).with_buffer();
        let diagnostics = interp.load(&module);
        assert!(!diagnostics.has_errors());
    });

    let run = {
        // Construction and loading are outside the timed region: this phase is
        // *only* `run_main` on a prepared interpreter.
        let mut samples = Vec::with_capacity(ROUNDS);
        for _ in 0..ROUNDS {
            let mut interp = Interp::new(&checked).with_buffer();
            let diagnostics = interp.load(&module);
            assert!(!diagnostics.has_errors());
            let start = Instant::now();
            interp.run_main().expect("main runs");
            samples.push(start.elapsed());
        }
        median(&mut samples)
    };

    let total = read + parse + check + construct + load + run;
    eprintln!(
        "startup profile (debug, median of {ROUNDS}): read {read:?}, lex+parse {parse:?}, \
         check {check:?}, Interp::new {construct:?}, load {load:?}, run {run:?} — total {total:?}"
    );
    // A conservative gate so an accidental quadratic/pathological phase fails
    // the test; the actual budget claim is made from release numbers.
    assert!(
        total < Duration::from_millis(500),
        "empty-program startup is pathologically slow: {total:?}"
    );

    let _ = std::fs::remove_file(&path);
}
