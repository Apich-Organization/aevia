//! Chapter 8 is the contract: every name it declares must resolve.
//!
//! The list this test walks is *derived from `spec/08-stdlib.md`*, checked in,
//! and re-derived on every run — a hand-written list could be shortened until
//! the test passed, which is the failure mode this file exists to prevent.
//!
//! For each name the test asserts one of two things:
//!
//! * the name resolves — it is typed by the checker *and* the runtime has a
//!   dispatch arm for it (`call` returns something other than "no
//!   implementation"); or
//! * the name is a documented capability gap: `E1001` whose message names a
//!   capability from §10/§13.8.
//!
//! A name that is typed but has no arm fails the test, and so does a name whose
//! `E1001` does not name a capability: a missing *implementation* is a bug,
//! not a gap (PLAN §3 rule 1).

#![allow(clippy::result_large_err)]

use aevia_core::builtins;
use aevia_core::value::Value;
use aevia_types::adt;

/// The names, one per line, sorted. Generated from the specification.
const NAMES: &str = include_str!("stdlib_names.txt");

/// Built-in names that chapter 8 does not declare but the runtime still owns.
///
/// The coverage test walks these too, so a name that is typed and forgotten by
/// the dispatcher fails even when it is not in the chapter. Everything here is
/// older than this round of work; the list is the audit's "typed but not
/// implemented" set, kept explicit so that removing an arm is caught.
const EXTRA_NAMES: &[&str] = &[
    "and_then",
    "approx",
    "assert",
    "assert_eq",
    "base",
    "by_cases",
    "chars",
    "decide",
    "delta_to",
    "double_factorial",
    "eval",
    "from_value",
    "is_none",
    "is_some",
    "map_err",
    "max_all",
    "min_all",
    "point",
    "print",
    "println",
    "read_file",
    "read_line",
    "repr",
    "show",
    "simplify",
    "subst",
    "symfun",
    "symbol",
    "time_now",
    "to",
    "to_latex",
    "to_typst",
    "to_value",
    "unwrap_or",
    "value_in",
    "write_file",
];

/// The capabilities a documented `E1001` gap may name.
///
/// This is the union of the two things that can be missing in this build:
///
/// * a **kernel** capability — the rows derived from the Kernel Manifest's
///   `Features` (`aevia-rssn`'s one table) plus the capability names §10/§13.8
///   declare that the manifest has no feature for (`io.binary`, `core.gpu`,
///   `core.distributed`);
/// * a **host-bridge** capability — `core.callback` and `core.plugin.source`
///   have separate rows: scoped Float callbacks are supported, while Aevia
///   source-level plugin construction remains unavailable (§8.19/§13.5).
///
/// Reading the enabled rows from the live manifest instead of typing the list
/// is what keeps this test honest when the kernel gains a capability.
fn capabilities() -> Vec<String> {
    let mut names: Vec<String> = aevia_rssn::registry()
        .capabilities
        .table()
        .into_iter()
        .map(|row| row.name)
        .collect();
    // Names §10/§13.8 declare that the manifest has no `Features` field for.
    names.extend(
        ["core.gpu", "core.distributed", "io.binary"]
            .into_iter()
            .map(str::to_string),
    );
    names.sort();
    names.dedup();
    names
}

fn declared() -> Vec<&'static str> {
    NAMES
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

fn spec_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/08-stdlib.md")
}

/// One argument of the right shape to reach a dispatch arm.
///
/// The value does not need to be the right *type*: a name with an arm answers
/// with a type error (`E0302`) or an arity error (`E0315`), and a name without
/// one answers `E1001` "no implementation". That difference is the signal.
fn probe() -> Vec<Value> {
    vec![Value::int(1)]
}

/// Whether an `E1001` message names one of the capabilities this build may
/// genuinely lack.
fn names_a_capability(message: &str) -> bool {
    capabilities()
        .iter()
        .any(|capability| message.contains(capability))
}

#[test]
fn the_checked_in_list_is_the_one_the_specification_declares() {
    let source = std::fs::read_to_string(spec_path())
        .expect("spec/08-stdlib.md must be readable from the test");
    let derived = aevia_std::spec::names(&source);
    let checked_in: Vec<String> = declared().iter().map(|name| (*name).to_string()).collect();
    if derived != checked_in {
        let missing: Vec<&String> = derived.iter().filter(|n| !checked_in.contains(n)).collect();
        let extra: Vec<&String> = checked_in.iter().filter(|n| !derived.contains(n)).collect();
        panic!(
            "the checked-in list has drifted from spec/08-stdlib.md: {missing:?} is \
             declared but not listed, {extra:?} is listed but not declared"
        );
    }
}

#[test]
fn every_declared_name_is_typed() {
    let mut missing = Vec::new();
    for name in declared() {
        if let Some(member) = name.strip_prefix("Rssn.") {
            if aevia_core::rssn_module::is_entry_point(member)
                || aevia_core::rssn_module::is_data_member(member)
            {
                continue;
            }
            missing.push(format!("{name} (not an Rssn member)"));
            continue;
        }
        if adt::find_prelude_ctor(name).is_some() {
            continue;
        }
        if aevia_types::builtins::is_builtin(name) || aevia_types::builtins::is_special(name) {
            continue;
        }
        missing.push(format!("{name} (not typed)"));
    }
    assert!(
        missing.is_empty(),
        "{} name(s) chapter 8 declares are not typed by the checker: {:?}",
        missing.len(),
        missing
    );
}

/// The four `Rssn` entry points §8.19 declares are *reachable from source*, not
/// merely present in a runtime list: a program has to be able to name them and
/// the checker has to give them a type. `is_entry_point` alone is a claim about
/// the runtime and would pass even while `rssn_member` reported `E0301`.
#[test]
fn the_fixed_entry_points_are_reachable_from_source() {
    for (member, source) in [
        ("typed", "let f = Rssn.typed\n"),
        ("call_ho", "let f = Rssn.call_ho\n"),
        ("register", "let f = Rssn.register\n"),
        ("register_plugin", "let f = Rssn.register_plugin\n"),
    ] {
        assert!(
            aevia_core::rssn_module::is_entry_point(member),
            "`Rssn.{member}` is not an entry point"
        );
        let compilation = aevia_core::compile("rssn.aev", source);
        let errors: Vec<String> = compilation
            .diagnostics
            .items
            .iter()
            .filter(|item| item.severity == aevia_syntax::Severity::Error)
            .map(|item| format!("{}: {}", item.code, item.message))
            .collect();
        assert!(
            errors.is_empty(),
            "`Rssn.{member}` is not reachable from source: {errors:?}"
        );
    }
}

#[test]
fn every_declared_name_is_implemented_or_a_named_capability_gap() {
    let mut missing = Vec::new();
    for name in declared() {
        if let Some(member) = name.strip_prefix("Rssn.") {
            // A `Rssn` member is implemented by the bridge, not by `call`.
            // Reachability from source is asserted separately, above.
            if aevia_core::rssn_module::is_entry_point(member)
                || aevia_core::rssn_module::is_data_member(member)
            {
                continue;
            }
            missing.push(format!("{name} (not an Rssn member)"));
            continue;
        }
        if adt::find_prelude_ctor(name).is_some() {
            continue;
        }
        match builtins::call_simple(name, probe()) {
            | Ok(_) => {},
            | Err(error) if error.code == "E1001" => {
                if !names_a_capability(&error.message) {
                    missing.push(format!(
                        "{name} (E1001 without a capability: {})",
                        error.message
                    ));
                }
            },
            | Err(_) => {},
        }
    }
    assert!(
        missing.is_empty(),
        "{} name(s) chapter 8 declares do not resolve: {:#?}",
        missing.len(),
        missing
    );
}

#[test]
fn every_other_builtin_is_implemented() {
    let mut missing = Vec::new();
    for name in EXTRA_NAMES {
        match builtins::call_simple(name, probe()) {
            | Ok(_) => {},
            | Err(error) if error.code == "E1001" => {
                if !names_a_capability(&error.message) {
                    missing.push(format!("{name} (E1001 without a capability)"));
                }
            },
            | Err(_) => {},
        }
    }
    assert!(
        missing.is_empty(),
        "built-ins typed but not dispatched: {missing:?}"
    );
}

/// Implementing NPY locally (§8.11) closed the last two documented gaps: no
/// chapter-8 name may be an `E1001` capability gap any more.
#[test]
fn chapter_eight_has_no_capability_gaps() {
    let mut gaps = Vec::new();
    for name in declared() {
        if name.starts_with("Rssn.") || adt::find_prelude_ctor(name).is_some() {
            continue;
        }
        if let Err(error) = builtins::call_simple(name, probe()) {
            if error.code == "E1001" {
                gaps.push((name.to_string(), error.message));
            }
        }
    }
    gaps.sort();
    assert!(
        gaps.is_empty(),
        "chapter 8 still has capability gap(s): {gaps:?}"
    );
}

#[test]
fn the_coverage_is_reported() {
    let total = declared().len();
    let mut implemented = 0usize;
    let mut gaps = Vec::new();
    for name in declared() {
        if let Some(member) = name.strip_prefix("Rssn.") {
            if aevia_core::rssn_module::is_entry_point(member)
                || aevia_core::rssn_module::is_data_member(member)
            {
                implemented += 1;
            }
            continue;
        }
        if adt::find_prelude_ctor(name).is_some() {
            implemented += 1;
            continue;
        }
        match builtins::call_simple(name, probe()) {
            | Ok(_) => implemented += 1,
            | Err(error) if error.code == "E1001" => gaps.push((name, error.message)),
            | Err(_) => implemented += 1,
        }
    }
    let accounted = implemented + gaps.len();
    let percent = 100.0 * accounted as f64 / total as f64;
    eprintln!(
        "chapter 8: {implemented}/{total} names implemented, {} documented capability \
         gap(s), 0 blocked — {percent:.1}% accounted for",
        gaps.len()
    );
    for (name, message) in &gaps {
        eprintln!("  gap: {name} — {message}");
    }
    assert_eq!(
        accounted, total,
        "every name is implemented or a named capability gap"
    );
    assert_eq!(percent, 100.0);
}
