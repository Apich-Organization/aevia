//! Runs the Aevia-side test suites of the generated domain modules.
//!
//! `tests/domain_*.aev` at the workspace root are ordinary `aevia test` files:
//! they import `Rssn_NumberTheory`, `Rssn_Combinatorics`, ... (rendered from the
//! live manifest and `domains.overlay` at module resolution) and assert with
//! `test "..." = <Bool>` blocks. This target loads each one through the real
//! driver, so a wrapper that does not type-check, a conversion that does not
//! decode, or a kernel behaviour change all fail here, not only under
//! `cargo run -p aevia-cli -- test`.

use aevia_core::Interp;
use aevia_core::driver;
use aevia_core::value::Value;
use aevia_syntax::DeclKind;
use std::path::Path;
use std::path::PathBuf;

fn suites() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("the workspace tests directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("domain_") && name.ends_with(".aev"))
        })
        .collect();
    files.sort();
    files
}

/// Runs every `test` block of one file; returns `(passed, failures)`.
fn run(path: &Path) -> (usize, Vec<String>) {
    let program = driver::load(path);
    assert!(
        !program.diagnostics.has_errors(),
        "{} does not compile: {:?}",
        path.display(),
        program
            .diagnostics
            .items
            .iter()
            .map(|d| format!("{} {}", d.code, d.message))
            .collect::<Vec<_>>()
    );
    let entry = program.entry().expect("an entry module");
    let mut interp = Interp::new(&entry.checked);
    let load = driver::run_modules(&mut interp, &program);
    assert!(
        !load.has_errors(),
        "{} does not load: {:?}",
        path.display(),
        load.items
            .iter()
            .map(|d| format!("{} {}", d.code, d.message))
            .collect::<Vec<_>>()
    );
    let root = interp.root();
    let mut passed = 0;
    let mut failures = Vec::new();
    for decl in &entry.module.decls {
        if let DeclKind::Test { name, body } = &decl.kind {
            match interp.eval(body, &root) {
                | Ok(Value::Bool(true)) => passed += 1,
                | Ok(other) => {
                    failures.push(format!(
                        "{name}: returned {}",
                        aevia_core::value::show(&other)
                    ));
                },
                | Err(error) => failures.push(format!("{name}: {}", error.message)),
            }
        }
    }
    (passed, failures)
}

#[test]
fn the_rssn_domain_suites_pass() {
    // The interpreter recurses; run on a thread with a stack like the CLI's.
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| {
            let files = suites();
            assert!(
                files.len() >= 3,
                "expected tests/domain_*.aev suites, found {files:?}"
            );
            let mut report = Vec::new();
            let mut total = 0;
            for path in &files {
                let (passed, failures) = run(path);
                total += passed;
                assert!(
                    passed + failures.len() > 0,
                    "{} has no tests",
                    path.display()
                );
                for failure in failures {
                    report.push(format!("{}: {failure}", path.display()));
                }
            }
            assert!(
                report.is_empty(),
                "failing Aevia tests:\n{}",
                report.join("\n")
            );
            assert!(total >= 30, "suspiciously few Aevia tests ran: {total}");
        })
        .expect("spawn");
    handle.join().expect("the suite thread panicked");
}
