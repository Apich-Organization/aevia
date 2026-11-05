//! The specification's own programs, compiled against the shipped module tree.
//!
//! `spec/11-examples.md` says these are "not fragments, but complete modules
//! that can be put in a `.aev` file and run". This test extracts every complete
//! `module …` program from the chapter, materialises the §8.20 standard library
//! next to it, and type-checks the program with the real loader and checker.
//!
//! It deliberately does **not** paper over language gaps: the pass/fail table
//! is asserted against [`EXPECTED`], which records the current outcome and the
//! root cause of each failure. A program that starts passing, or starts failing
//! with a different diagnostic, fails this test so the table is refreshed with
//! evidence instead of drifting. Failures owned by another work stream are
//! listed with the stream that owns them.

mod common;

use common::{errors, rendered, spec_programs_path, Package};

/// One complete program from §11.
struct Program {
    /// Section number (`11.1`).
    section: String,
    /// The declared module name.
    module: String,
    /// The program source.
    source: String,
}

/// The first error of a failed program, as its stable code.
fn first_error(program: &aevia_core::driver::Program) -> String {
    program
        .diagnostics
        .items
        .iter()
        .find(|item| item.code.starts_with('E'))
        .map(|item| item.code.clone())
        .unwrap_or_else(|| "<none>".to_string())
}

fn extracted() -> Vec<Program> {
    let source = std::fs::read_to_string(spec_programs_path()).expect("spec/11-examples.md");
    let mut out = Vec::new();
    let mut section = String::new();
    let mut language = String::new();
    let mut in_block = false;
    let mut block = String::new();
    for raw in source.lines() {
        if !in_block {
            if let Some(rest) = raw.strip_prefix("## 11.") {
                let number = rest
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('.');
                section = format!("11.{number}");
                continue;
            }
            if let Some(fence) = raw.trim_start().strip_prefix("```") {
                in_block = true;
                language = fence.trim().to_string();
                block.clear();
            }
            continue;
        }
        if raw.trim_start().starts_with("```") {
            in_block = false;
            if language == "aevia" && !block.contains("...") {
                let module = block
                    .lines()
                    .find_map(|line| line.trim().strip_prefix("module "))
                    .map(str::trim)
                    .unwrap_or("");
                if !module.is_empty() {
                    out.push(Program {
                        section: section.clone(),
                        module: module.to_string(),
                        source: block.clone(),
                    });
                }
            }
            continue;
        }
        block.push_str(raw);
        block.push('\n');
    }
    out
}

#[test]
fn the_specification_programs_type_check_or_have_a_recorded_cause() {
    let package = Package::with_stdlib("spec-programs");
    let programs = extracted();
    assert!(!programs.is_empty());

    let mut table = Vec::new();
    let mut import_failures = Vec::new();
    let mut outcomes = Vec::new();
    for program in &programs {
        let loaded = package.compile_program(&format!("spec_{}", program.module), &program.source);
        let failures = errors(&loaded);
        for failure in &failures {
            if failure.contains("E0607") {
                import_failures.push(format!("{}: {failure}", program.module));
            }
        }
        let result = if failures.is_empty() {
            "ok".to_string()
        } else {
            first_error(&loaded)
        };
        table.push(format!(
            "{:<6} {:<12} {}",
            program.section,
            program.module,
            if failures.is_empty() {
                "PASS".to_string()
            } else {
                format!("FAIL ({} diagnostic(s))", failures.len())
            }
        ));
        if !failures.is_empty() {
            // The span is the evidence for the root cause in EXPECTED. Printed
            // once per failing program, never asserted as text.
            eprintln!("--- {} ({}) ---", program.module, program.section);
            eprintln!("{}", rendered(&loaded));
        }
        outcomes.push((program.module.as_str(), result, failures.join("; ")));
    }

    eprintln!("spec/11 complete programs ({}):", programs.len());
    for line in &table {
        eprintln!("  {line}");
    }
    for (module, result, failures) in &outcomes {
        if result == "ok" {
            eprintln!("  {module}: ok");
        } else {
            eprintln!("  {module}: {result}");
            eprintln!("    {failures}");
        }
    }

    // The §8.20 module tree exists to make the spec's `import`/`open` lines
    // resolve. Any `E0607` here is a missing module, which is this task's own
    // contract — so it is an assertion, not a table entry.
    assert!(
        import_failures.is_empty(),
        "the standard-library module tree did not resolve an import: {import_failures:#?}"
    );

    // Every failure must be one of the documented, owned language gaps below.
    // The table is asserted exactly: a program that starts passing or starts
    // failing differently fails this test so the table is refreshed with
    // evidence rather than drifting.
    for (module, result, failures) in &outcomes {
        let expected = EXPECTED
            .iter()
            .find(|(name, _, _)| name == module)
            .unwrap_or_else(|| panic!("no expectation recorded for `{module}`"));
        assert_eq!(
            result, &expected.1,
            "`{module}` now reports `{result}`, expected `{}` ({}); diagnostics: {failures}",
            expected.1, expected.2
        );
    }
    assert_eq!(
        outcomes.len(),
        EXPECTED.len(),
        "the spec-program table is incomplete"
    );
}

/// The recorded outcome of every complete §11 program.
///
/// `(module, result, root cause)`, where `result` is `"ok"` or the first error
/// code. The root cause names the feature and, when it is not this crate's, the
/// work stream that owns it. This is the evidence-backed table: update it only
/// together with the diagnostics printed by
/// [`the_specification_programs_type_check_or_have_a_recorded_cause`].
///
/// Every program passes as of the round-2 frontend/runtime fixes; the two that
/// used to be recorded gaps (§11.2 `RateOf` record unification, §11.4
/// `Refined`/`Rank` unification) are now asserted as passing instead.
const EXPECTED: &[(&str, &str, &str)] = &[
    (
        "Kinematics",
        "ok",
        "§11.1: units, `open Units.SI`, `import Math { sqrt }`, dimensioned sqrt",
    ),
    (
        "Projectile",
        "ok",
        "§11.2: records + `RateOf<State>` — an *imported* `odeint` keeps the \
         §8.9 type-function typing (`Ode.aev` re-exports the loose HM scheme; \
         imports are not local shadows)",
    ),
    ("Symbolic", "ok", "§11.3: `import Math/Calculus/Solve`, higher-order diff/solve"),
    (
        "Circuits",
        "ok",
        "§11.4: dimensioned linear algebra; a refined shape unifies with a \
         same-rank rank-only shape for elementwise ops (`Tensor<[2], V>` vs \
         `Tensor<1, A>` in `v * i`)",
    ),
    ("Thermo", "ok", "§11.5: affine points, `point(300.0, K)`, `to(°C, …)`"),
    (
        "Analysis",
        "ok",
        "§11.6: `import Data.Csv { read_csv, column, Table }` + Stats/Plot signatures",
    ),
    (
        "IsingDemo",
        "ok",
        "§11.7: `import Physics.Ising`, typed Params record (executes separately)",
    ),
    ("Regression", "ok", "§11.8: dimension-polymorphic statistics"),
    (
        "Full",
        "ok",
        "§11.10: units + calculus + linear algebra + Plot in one program",
    ),
];

#[test]
fn the_extraction_is_the_chapters_complete_programs() {
    let programs = extracted();
    let names: Vec<&str> = programs.iter().map(|program| program.module.as_str()).collect();
    assert_eq!(
        names,
        [
            "Kinematics",
            "Projectile",
            "Symbolic",
            "Circuits",
            "Thermo",
            "Analysis",
            "IsingDemo",
            "Regression",
            "Full",
        ],
        "the complete-program extraction drifted from spec/11-examples.md"
    );
    for program in &programs {
        assert!(
            program.source.starts_with(&format!("module {}", program.module)),
            "`{}` does not start with its module declaration",
            program.section
        );
    }
}
