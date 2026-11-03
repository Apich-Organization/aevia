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
//!   capability from §10's table.
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

/// The capabilities §10 declares. An `E1001` that names one of these is a
/// documented gap; an `E1001` that names nothing is a missing implementation.
const CAPABILITIES: &[&str] = &[
    "core.dynamic_call",
    "core.gpu",
    "core.jit",
    "core.distributed",
    "core.plugin",
    "core.symbolic",
    "core.tensor",
    "core.ode",
    "io.binary",
];

/// Names §8 declares that this build cannot resolve, with the reason and the
/// crate that must change.
///
/// Each entry is asserted to *still be blocked*, so a name cannot quietly join
/// this list: if it starts resolving, the test fails and the entry must be
/// deleted.
const BLOCKED: &[(&str, &str)] = &[
    (
        "Rssn.typed",
        "the runtime arm exists (`rssn_module::call` handles `typed`), but \
         `check.rs::rssn_member` has no signature for it, so a program cannot \
         name it; the checker needs a `TypeRep` type",
    ),
    (
        "Rssn.call_ho",
        "as `Rssn.typed`: the checker has no signature for a higher-order \
         dynamic call",
    ),
    (
        "Rssn.register",
        "as `Rssn.typed`: plugin registration needs a `Plugin` type the \
         checker does not have",
    ),
    (
        "Rssn.register_plugin",
        "as `Rssn.typed`: alias of `Rssn.register`, blocked for the same reason",
    ),
];

fn declared() -> Vec<&'static str> {
    NAMES
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

fn spec_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/08-stdlib.md")
}

/// One argument of the right shape to reach a dispatch arm.
///
/// The value does not need to be the right *type*: a name with an arm answers
/// with a type error (`E0302`) or an arity error (`E0315`), and a name without
/// one answers `E1001` "no implementation". That difference is the signal.
fn probe() -> Vec<Value> {
    vec![Value::int(1)]
}

fn is_blocked(name: &str) -> bool {
    BLOCKED.iter().any(|(blocked, _)| *blocked == name)
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
        if is_blocked(name) {
            continue;
        }
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

#[test]
fn every_declared_name_is_implemented_or_a_named_capability_gap() {
    let mut missing = Vec::new();
    for name in declared() {
        if is_blocked(name) {
            continue;
        }
        if let Some(member) = name.strip_prefix("Rssn.") {
            // A `Rssn` member is implemented by the bridge, not by `call`.
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
            Ok(_) => {}
            Err(error) if error.code == "E1001" => {
                let named = CAPABILITIES
                    .iter()
                    .any(|capability| error.message.contains(capability));
                if !named {
                    missing.push(format!("{name} (E1001 without a capability: {})", error.message));
                }
            }
            Err(_) => {}
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
            Ok(_) => {}
            Err(error) if error.code == "E1001" => {
                let named = CAPABILITIES
                    .iter()
                    .any(|capability| error.message.contains(capability));
                if !named {
                    missing.push(format!("{name} (E1001 without a capability)"));
                }
            }
            Err(_) => {}
        }
    }
    assert!(
        missing.is_empty(),
        "built-ins typed but not dispatched: {missing:?}"
    );
}

#[test]
fn a_blocked_name_is_still_blocked() {
    for (name, reason) in BLOCKED {
        // The blocker is the *checker*: a program that names the member gets a
        // diagnostic. A runtime arm existing (as `Rssn.typed` now does) does
        // not make the name reachable, so the test asks the checker rather
        // than the bridge.
        let source = format!("let x = {name}\n");
        let compilation = aevia_core::compile("blocked.aev", &source);
        assert!(
            compilation.diagnostics.has_errors(),
            "`{name}` now resolves from source; delete it from BLOCKED and from \
             the report ({reason})"
        );
    }
}

#[test]
fn the_coverage_is_reported() {
    let total = declared().len();
    let mut implemented = 0usize;
    let mut gaps = Vec::new();
    for name in declared() {
        if is_blocked(name) {
            continue;
        }
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
            Ok(_) => implemented += 1,
            Err(error) if error.code == "E1001" => gaps.push((name, error.message)),
            Err(_) => implemented += 1,
        }
    }
    let blocked = BLOCKED.len();
    let percent = 100.0 * implemented as f64 / total as f64;
    eprintln!(
        "chapter 8: {implemented}/{total} names implemented ({percent:.1}%), \
         {} documented capability gap(s), {blocked} blocked",
        gaps.len()
    );
    for (name, message) in &gaps {
        eprintln!("  gap: {name} — {message}");
    }
    assert_eq!(
        implemented + gaps.len() + blocked,
        total,
        "every name is implemented, a named gap, or blocked"
    );
}
