//! Resolver tests: hand-made scenarios, then a randomised comparison against an
//! exhaustive oracle.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::index::Dependency;
use crate::index::Index;
use crate::index::IndexError;
use crate::index::MemoryIndex;
use crate::index::Summary;
use crate::req::VersionReq;
use crate::resolve::Cause;
use crate::resolve::Failure;
use crate::resolve::Node;
use crate::resolve::Options;
use crate::resolve::Reason;
use crate::resolve::Resolution;
use crate::resolve::ResolveError;
use crate::resolve::Root;
use crate::resolve::Unify;
use crate::resolve::resolve;
use crate::testing::Rng;
use crate::version::Version;

fn v(text: &str) -> Version {
    Version::parse(text).expect("version")
}

fn r(text: &str) -> VersionReq {
    VersionReq::parse(text).expect("requirement")
}

fn root(dependencies: &[(&str, &str)]) -> Root {
    Root {
        id: "app".to_string(),
        dependencies: dependencies
            .iter()
            .map(|(name, req)| Dependency::new(name, r(req)))
            .collect(),
    }
}

fn run(
    index: &MemoryIndex,
    dependencies: &[(&str, &str)],
) -> Result<Resolution, ResolveError> {
    resolve(&[root(dependencies)], index, &Options::default())
}

fn chosen(
    resolution: &Resolution,
    package: &str,
) -> Vec<String> {
    resolution
        .versions_of(package)
        .map(ToString::to_string)
        .collect()
}

#[test]
fn the_highest_compatible_version_wins() {
    let mut index = MemoryIndex::new();
    for version in ["1.0.0", "1.2.0", "1.9.3", "2.0.0", "0.9.0"] {
        index.add("o/foo", version, &[]);
    }
    let resolution = run(&index, &[("o/foo", "^1.0")]).expect("resolves");
    assert_eq!(chosen(&resolution, "o/foo"), ["1.9.3"]);
    let resolution = run(&index, &[("o/foo", "*")]).expect("resolves");
    assert_eq!(chosen(&resolution, "o/foo"), ["2.0.0"]);
}

#[test]
fn catalogue_prunes_and_orders_wide_numeric_prereleases_numerically() {
    let mut index = MemoryIndex::new();
    for release in [
        "1.0.0-99999999999999999999",
        "1.0.0",
        "1.0.0-100000000000000000000",
        "1.0.0-18446744073709551615",
    ] {
        index.add("o/foo", release, &[]);
    }
    let selected = run(&index, &[("o/foo", ">=1.0.0-99999999999999999999, <1.0.0")])
        .expect("wide prerelease range resolves");
    assert_eq!(chosen(&selected, "o/foo"), ["1.0.0-100000000000000000000"]);
}

#[test]
fn catalogue_bounds_keep_precedence_ties_and_apply_the_prerelease_gate() {
    let mut index = MemoryIndex::new();
    for release in [
        "0.9.9",
        "1.0.0-beta.1",
        "1.0.0-beta.2",
        "1.0.0",
        "1.0.0+first",
        "1.0.0+second",
        "1.0.1-beta.1",
        "1.1.0",
        "2.0.0",
    ] {
        index.add("o/foo", release, &[]);
    }
    let pre = run(&index, &[("o/foo", ">=1.0.0-beta.2, <1.0.0")]).expect("pre-release");
    assert_eq!(chosen(&pre, "o/foo"), ["1.0.0-beta.2"]);

    let release = run(&index, &[("o/foo", ">=1.0.0-beta.1, <1.1.0")]).expect("release");
    assert_eq!(chosen(&release, "o/foo"), ["1.0.0+second"]);

    let mut options = Options::default();
    options
        .locked
        .insert("o/foo".to_owned(), BTreeSet::from([v("1.0.0+first")]));
    let locked = resolve(&[root(&[("o/foo", "=1.0.0")])], &index, &options).expect("lock");
    assert_eq!(chosen(&locked, "o/foo"), ["1.0.0+first"]);

    let empty = run(&index, &[("o/foo", ">2.0.0, <1.0.0")]);
    assert!(matches!(empty, Err(ResolveError::Unsatisfiable(_))));
}

#[test]
fn an_empty_root_resolves_to_nothing() {
    let resolution = run(&MemoryIndex::new(), &[]).expect("resolves");
    assert!(resolution.packages.is_empty() && resolution.edges.is_empty());
}

#[test]
fn a_diamond_shares_one_version() {
    let mut index = MemoryIndex::new();
    index
        .add("o/a", "1.0.0", &[("o/c", "^1.1")])
        .add("o/b", "1.0.0", &[("o/c", "^1.0")]);
    for version in ["1.0.0", "1.1.0", "1.3.0", "2.0.0"] {
        index.add("o/c", version, &[]);
    }
    let resolution = run(&index, &[("o/a", "^1"), ("o/b", "^1")]).expect("resolves");
    assert_eq!(chosen(&resolution, "o/c"), ["1.3.0"]);
    // Both dependents point at the same node.
    let target = resolution
        .packages
        .iter()
        .find(|p| p.package == "o/c")
        .expect("c");
    assert_eq!(resolution.dependents_of(target).len(), 2);
}

#[test]
fn incompatible_majors_coexist_under_the_series_rule() {
    let mut index = MemoryIndex::new();
    index
        .add("o/a", "1.0.0", &[("o/c", "^1")])
        .add("o/b", "1.0.0", &[("o/c", "^2")])
        .add("o/c", "1.4.0", &[])
        .add("o/c", "2.1.0", &[]);
    let resolution = run(&index, &[("o/a", "^1"), ("o/b", "^1")]).expect("resolves");
    assert_eq!(chosen(&resolution, "o/c"), ["1.4.0", "2.1.0"]);
    // Each dependent is wired to the version it asked for.
    let a = Node::Package(crate::PackageVersion {
        package: "o/a".into(),
        version: v("1.0.0"),
    });
    let edge = resolution.edges_from(&a).first().copied().expect("edge");
    assert_eq!(edge.to.version, v("1.4.0"));
}

#[test]
fn zero_dot_x_series_are_incompatible_with_each_other() {
    let mut index = MemoryIndex::new();
    index
        .add("o/a", "1.0.0", &[("o/c", "^0.1")])
        .add("o/b", "1.0.0", &[("o/c", "^0.2")])
        .add("o/c", "0.1.5", &[])
        .add("o/c", "0.1.9", &[])
        .add("o/c", "0.2.0", &[]);
    let resolution = run(&index, &[("o/a", "1"), ("o/b", "1")]).expect("resolves");
    assert_eq!(chosen(&resolution, "o/c"), ["0.1.9", "0.2.0"]);
}

#[test]
fn the_package_rule_turns_that_into_a_conflict_with_an_explanation() {
    let mut index = MemoryIndex::new();
    index
        .add("o/a", "1.0.0", &[("o/c", "^1")])
        .add("o/b", "1.0.0", &[("o/c", "^2")])
        .add("o/c", "1.4.0", &[])
        .add("o/c", "2.1.0", &[]);
    let options = Options {
        unify: Unify::Package,
        ..Options::default()
    };
    let error =
        resolve(&[root(&[("o/a", "^1"), ("o/b", "^1")])], &index, &options).expect_err("conflict");
    let text = error.to_string();
    assert!(matches!(error, ResolveError::Unsatisfiable(_)));
    assert!(text.contains("o/c ^1") && text.contains("o/c ^2"), "{text}");
    assert!(text.contains("already chosen"), "{text}");
    assert!(
        text.contains("o/a 1.0.0") && text.contains("o/b 1.0.0"),
        "{text}"
    );
}

#[test]
fn backtracking_changes_an_earlier_choice_to_make_a_later_one_fit() {
    // The newest `a` needs `c ^2`, but `b` pins `c ^1`: only an older `a` fits.
    let mut index = MemoryIndex::new();
    index
        .add("o/a", "1.0.0", &[("o/c", "^1")])
        .add("o/a", "1.1.0", &[("o/c", "^1")])
        .add("o/a", "2.0.0", &[("o/c", "=2.0.0")])
        .add("o/b", "1.0.0", &[("o/c", "=1.0.0")])
        .add("o/c", "1.0.0", &[])
        .add("o/c", "1.1.0", &[])
        .add("o/c", "2.0.0", &[]);
    let options = Options {
        unify: Unify::Package,
        ..Options::default()
    };
    let resolution =
        resolve(&[root(&[("o/a", "*"), ("o/b", "^1")])], &index, &options).expect("resolves");
    assert_eq!(chosen(&resolution, "o/a"), ["1.1.0"]);
    assert_eq!(chosen(&resolution, "o/c"), ["1.0.0"]);
}

#[test]
fn yanked_versions_are_skipped_unless_locked() {
    let mut index = MemoryIndex::new();
    index
        .add("o/foo", "1.0.0", &[])
        .add_yanked("o/foo", "1.1.0", &[]);
    let resolution = run(&index, &[("o/foo", "^1")]).expect("resolves");
    assert_eq!(chosen(&resolution, "o/foo"), ["1.0.0"]);

    let mut options = Options::default();
    options
        .locked
        .entry("o/foo".to_string())
        .or_default()
        .insert(v("1.1.0"));
    let resolution = resolve(&[root(&[("o/foo", "^1")])], &index, &options).expect("resolves");
    assert_eq!(
        chosen(&resolution, "o/foo"),
        ["1.1.0"],
        "a lock keeps a yanked version"
    );
}

#[test]
fn occupying_a_series_updates_the_pending_requirement_priority() {
    let mut index = MemoryIndex::new();
    index
        .add("o/x", "1.0.0", &[])
        .add("o/x", "1.1.0", &[])
        .add("o/y", "1.0.0", &[]);
    let roots = [root(&[("o/x", "=1.1.0"), ("o/y", "*"), ("o/x", "=1.0.0")])];
    let options = Options {
        max_activations: 1,
        ..Options::default()
    };

    // All three start with one choice, so the first one occupies x's 1.x
    // slot. The third must now outrank y and report its conflict without
    // spending a second activation.
    let error = resolve(&roots, &index, &options).expect_err("x versions conflict");
    assert!(matches!(error, ResolveError::Unsatisfiable(_)), "{error}");
    let explanation = error.to_string();
    assert!(explanation.contains("1.0.0"), "{explanation}");
    assert!(explanation.contains("1.1.0"), "{explanation}");
}

#[test]
fn rolling_back_a_candidate_restores_requirements_it_had_satisfied() {
    let mut index = MemoryIndex::new();
    index
        .add("o/x", "1.0.0", &[])
        .add("o/x", "2.0.0", &[("o/missing", "*")])
        .add("o/x", "3.0.0", &[])
        .add("o/x", "4.0.0", &[]);
    let resolution = run(&index, &[("o/x", ">=1, <3"), ("o/x", ">=2, <5")])
        .expect("the older choice restores the second root requirement");
    assert_eq!(chosen(&resolution, "o/x"), ["1.0.0", "4.0.0"]);
    assert_eq!(resolution.edges.len(), 2);
}

#[test]
fn rollback_reuses_pending_positions_after_the_first_branch_of_a_chain() {
    let mut index = MemoryIndex::new();
    index
        .add("o/start", "1.0.0", &[("o/next", "*")])
        .add("o/next", "1.0.0", &[("o/branch", "*")])
        .add("o/branch", "1.1.0", &[("o/left", "*"), ("o/right", "*")])
        .add("o/left", "1.0.0", &[("o/shared", "=1.0.0")])
        .add("o/right", "1.0.0", &[("o/shared", "=1.1.0")])
        .add(
            "o/branch",
            "1.0.0",
            &[("o/replacement", "*"), ("o/shared", "=1.1.0")],
        )
        .add("o/replacement", "1.0.0", &[("o/shared", "^1")])
        .add("o/shared", "1.0.0", &[])
        .add("o/shared", "1.1.0", &[]);
    let resolution = run(&index, &[("o/start", "*")]).expect("the earlier branch works");
    assert_eq!(chosen(&resolution, "o/branch"), ["1.0.0"]);
    assert_eq!(chosen(&resolution, "o/shared"), ["1.1.0"]);
    assert_eq!(resolution.packages.len(), 5);
    assert_eq!(resolution.edges.len(), 6);
}

#[test]
fn only_yanked_matches_are_explained_as_such() {
    let mut index = MemoryIndex::new();
    index
        .add("o/foo", "1.0.0", &[])
        .add_yanked("o/foo", "2.0.0", &[]);
    let error = run(&index, &[("o/foo", "^2")]).expect_err("only a yanked match");
    let text = error.to_string();
    assert!(text.contains("yanked"), "{text}");
    assert!(text.contains("2.0.0") && text.contains("1.0.0"), "{text}");
}

#[test]
fn a_lock_is_preferred_over_the_newest_version_but_never_over_the_requirement() {
    let mut index = MemoryIndex::new();
    for version in ["1.0.0", "1.1.0", "1.2.0"] {
        index.add("o/foo", version, &[]);
    }
    let mut options = Options::default();
    options
        .locked
        .entry("o/foo".to_string())
        .or_default()
        .insert(v("1.1.0"));
    let resolution = resolve(&[root(&[("o/foo", "^1")])], &index, &options).expect("resolves");
    assert_eq!(chosen(&resolution, "o/foo"), ["1.1.0"]);
    // A requirement the lock no longer satisfies is re-resolved.
    let resolution = resolve(&[root(&[("o/foo", "^1.2")])], &index, &options).expect("resolves");
    assert_eq!(chosen(&resolution, "o/foo"), ["1.2.0"]);
}

#[test]
fn pre_releases_are_chosen_only_when_asked_for() {
    let mut index = MemoryIndex::new();
    index
        .add("o/foo", "1.0.0", &[])
        .add("o/foo", "1.1.0-rc.1", &[])
        .add("o/foo", "2.0.0-rc.1", &[]);
    assert_eq!(
        chosen(&run(&index, &[("o/foo", "^1")]).expect("ok"), "o/foo"),
        ["1.0.0"]
    );
    assert_eq!(
        chosen(&run(&index, &[("o/foo", "*")]).expect("ok"), "o/foo"),
        ["1.0.0"]
    );
    assert_eq!(
        chosen(
            &run(&index, &[("o/foo", ">=1.1.0-rc.1")]).expect("ok"),
            "o/foo"
        ),
        ["1.1.0-rc.1"]
    );
}

#[test]
fn an_unknown_package_is_named() {
    let error = run(&MemoryIndex::new(), &[("o/ghost", "^1")]).expect_err("unknown");
    assert!(error.to_string().contains("not in the index"), "{error}");
}

#[test]
fn no_matching_version_lists_what_exists() {
    let mut index = MemoryIndex::new();
    index.add("o/foo", "1.0.0", &[]).add("o/foo", "1.1.0", &[]);
    let text = run(&index, &[("o/foo", "^3")])
        .expect_err("none")
        .to_string();
    assert!(text.contains("available: 1.0.0, 1.1.0"), "{text}");
}

#[test]
fn a_dependency_ring_is_an_error_when_it_cannot_be_avoided() {
    let mut index = MemoryIndex::new();
    index
        .add("o/a", "1.0.0", &[("o/b", "^1")])
        .add("o/b", "1.0.0", &[("o/a", "^1")]);
    let text = run(&index, &[("o/a", "^1")]).expect_err("ring").to_string();
    assert!(text.contains("ring"), "{text}");
    assert!(
        text.contains("o/a 1.0.0 -> o/b 1.0.0 -> o/a 1.0.0"),
        "{text}"
    );
}

#[test]
fn a_dependency_ring_is_avoided_by_choosing_another_version() {
    let mut index = MemoryIndex::new();
    index
        .add("o/a", "1.0.0", &[("o/b", "^1")])
        .add("o/b", "1.1.0", &[("o/a", "^1")])
        .add("o/b", "1.0.0", &[]);
    let resolution = run(&index, &[("o/a", "^1")]).expect("resolves");
    assert_eq!(chosen(&resolution, "o/b"), ["1.0.0"]);
}

#[test]
fn a_package_depending_on_itself_is_a_ring() {
    let mut index = MemoryIndex::new();
    index.add("o/a", "1.0.0", &[("o/a", "^1")]);
    assert!(run(&index, &[("o/a", "^1")]).is_err());
}

#[test]
fn the_result_does_not_depend_on_index_order() {
    let mut entries = [
        ("o/a", "1.0.0", vec![("o/c", "^1")]),
        ("o/a", "1.2.0", vec![("o/c", "^1.1")]),
        ("o/b", "1.0.0", vec![("o/c", "^1.0")]),
        ("o/c", "1.0.0", vec![]),
        ("o/c", "1.1.0", vec![]),
        ("o/c", "1.5.0", vec![("o/d", "*")]),
        ("o/d", "0.1.0", vec![]),
        ("o/d", "0.2.0", vec![]),
    ];
    let mut reference = None;
    let mut rng = Rng::new(31);
    for _ in 0..8 {
        for position in (1..entries.len()).rev() {
            let other = rng.below(position as u64 + 1) as usize;
            entries.swap(position, other);
        }
        let mut index = MemoryIndex::new();
        for (name, version, deps) in &entries {
            index.add(name, version, deps);
        }
        let resolution = run(&index, &[("o/a", "^1"), ("o/b", "^1")]).expect("resolves");
        match &reference {
            | None => reference = Some(resolution),
            | Some(first) => assert_eq!(first, &resolution),
        }
    }
}

struct Offline;

impl Index for Offline {
    fn summaries(
        &self,
        package: &str,
    ) -> Result<Vec<Summary>, IndexError> {
        Err(IndexError::unavailable(package, "not fetched yet"))
    }
}

#[test]
fn an_unavailable_index_stops_the_search_and_names_the_package() {
    let error =
        resolve(&[root(&[("o/foo", "^1")])], &Offline, &Options::default()).expect_err("offline");
    assert_eq!(error.unavailable_package(), Some("o/foo"));
}

#[test]
fn an_earlier_impossible_requirement_leaves_later_index_entries_unread() {
    struct PartialIndex;

    impl Index for PartialIndex {
        fn summaries(
            &self,
            package: &str,
        ) -> Result<Vec<Summary>, IndexError> {
            if package == "o/missing" {
                Err(IndexError::missing(package))
            } else {
                Err(IndexError::unavailable(package, "not fetched yet"))
            }
        }
    }

    let options = Options::default();
    let missing_first = root(&[("o/missing", "*"), ("o/offline", "*")]);
    let missing = resolve(&[missing_first], &PartialIndex, &options).expect_err("missing");
    assert!(matches!(missing, ResolveError::Unsatisfiable(_)));

    let offline_first = root(&[("o/offline", "*"), ("o/missing", "*")]);
    let offline = resolve(&[offline_first], &PartialIndex, &options).expect_err("offline");
    assert_eq!(offline.unavailable_package(), Some("o/offline"));
}

#[test]
fn the_search_budget_is_enforced() {
    let mut index = MemoryIndex::new();
    index
        .add("o/a", "1.0.0", &[("o/b", "^1")])
        .add("o/b", "1.0.0", &[]);
    let options = Options {
        max_activations: 1,
        ..Options::default()
    };
    let error = resolve(&[root(&[("o/a", "^1")])], &index, &options).expect_err("budget");
    assert!(
        matches!(error, ResolveError::TooComplex { limit: 1 }),
        "{error}"
    );
}

#[test]
fn a_failure_that_does_not_involve_earlier_choices_does_not_retry_them() {
    // Twelve independent packages with two versions each, then an
    // unsatisfiable pair. Chronological backtracking would try 2^12 = 4096
    // combinations of the independent ones; backjumping gives up on them as
    // soon as it sees the failure does not depend on them.
    let mut index = MemoryIndex::new();
    let mut roots = Vec::new();
    for number in 0..12 {
        let name = format!("o/p{number}");
        index.add(&name, "1.0.0", &[]).add(&name, "1.1.0", &[]);
        roots.push((name, "^1".to_string()));
    }
    for version in ["1.0.0", "1.1.0", "1.2.0"] {
        index
            .add("o/x", version, &[("o/z", "=1.0.0")])
            .add("o/y", version, &[("o/z", "=1.1.0")]);
    }
    index.add("o/z", "1.0.0", &[]).add("o/z", "1.1.0", &[]);
    roots.push(("o/x".to_string(), "^1".to_string()));
    roots.push(("o/y".to_string(), "^1".to_string()));
    let pairs: Vec<(&str, &str)> = roots
        .iter()
        .map(|(name, req)| (name.as_str(), req.as_str()))
        .collect();
    let options = Options {
        max_activations: 200,
        ..Options::default()
    };
    let error = resolve(&[root(&pairs)], &index, &options).expect_err("unsatisfiable");
    assert!(
        matches!(error, ResolveError::Unsatisfiable(_)),
        "must fail fast, not exhaust the budget: {error}"
    );
}

#[test]
fn identical_failures_fold_into_a_range_in_the_explanation() {
    let mut index = MemoryIndex::new();
    for minor in 0..6 {
        index.add("o/a", &format!("1.{minor}.0"), &[("o/ghost", "^1")]);
    }
    let text = run(&index, &[("o/a", "^1")])
        .expect_err("fails")
        .to_string();
    assert!(text.contains("(6 versions)"), "{text}");
    assert!(text.contains("not in the index"), "{text}");
    assert!(text.lines().count() < 12, "stays short:\n{text}");
}

#[test]
fn conflict_retains_the_newest_attempts_and_taken_versions_even_with_a_lock() {
    let mut index = MemoryIndex::new();
    index.add("o/x", "2.0.0-rc.1", &[]);
    for minor in 0..20 {
        index.add("o/x", &format!("1.{minor}.0"), &[("o/ghost", "*")]);
    }
    for minor in 0..8 {
        index.add("o/x", &format!("2.{minor}.0"), &[]);
    }
    let mut options = Options::default();
    options
        .locked
        .insert("o/x".into(), BTreeSet::from([v("1.0.0")]));
    let error = resolve(
        &[root(&[("o/x", "=2.0.0-rc.1"), ("o/x", "*")])],
        &index,
        &options,
    )
    .expect_err("every candidate is taken or has a missing dependency");
    let ResolveError::Unsatisfiable(conflict) = error else {
        panic!("unexpected resolver error: {error}");
    };
    let Failure::Unsatisfied {
        reason: Reason::Candidates { entries, .. },
        ..
    } = conflict.failure
    else {
        panic!("expected candidate details for the exact prerelease");
    };
    let Cause::Failed(inner) = &entries.first().expect("prerelease candidate").cause else {
        panic!("expected prerelease candidate to have a nested conflict");
    };
    let Failure::Unsatisfied {
        reason: Reason::Candidates { entries, omitted },
        ..
    } = &**inner
    else {
        panic!("expected explanation for the wildcard");
    };
    assert_eq!(*omitted, 16);
    let expected: Vec<_> = (0..8)
        .rev()
        .map(|minor| Version::new(2, minor, 0))
        .chain((16..20).rev().map(|minor| Version::new(1, minor, 0)))
        .collect();
    assert_eq!(
        entries
            .iter()
            .map(|entry| &entry.version)
            .collect::<Vec<_>>(),
        expected.iter().collect::<Vec<_>>()
    );
}

#[test]
fn a_taken_version_omitted_from_diagnostics_still_guides_backtracking() {
    let mut index = MemoryIndex::new();
    index
        .add("o/x", "1.0.0", &[])
        .add("o/x", "1.1.0", &[])
        .add("o/x", "3.0.0-rc.1", &[]);
    for minor in 0..16 {
        index.add("o/x", &format!("3.{minor}.0"), &[]);
    }
    let mut options = Options::default();
    options
        .locked
        .insert("o/x".into(), BTreeSet::from([v("1.0.0")]));
    let requirements = [
        ("o/x", "=3.0.0-rc.1"),
        ("o/x", "^1"),
        ("o/x", ">=1.1.0, <4"),
    ];
    let resolution = resolve(&[root(&requirements)], &index, &options)
        .expect("the older blocked candidate points to the choice that can change");
    assert_eq!(chosen(&resolution, "o/x"), ["1.1.0", "3.0.0-rc.1"]);
}

#[test]
fn edges_carry_the_import_label_and_roots_are_nodes() {
    let mut index = MemoryIndex::new();
    index.add("apich/units-extra", "1.0.0", &[]);
    let roots = [Root {
        id: "app".to_string(),
        dependencies: vec![Dependency {
            label: "Units".to_string(),
            package: "apich/units-extra".to_string(),
            req: r("^1"),
        }],
    }];
    let resolution = resolve(&roots, &index, &Options::default()).expect("resolves");
    assert_eq!(resolution.edges.len(), 1);
    assert_eq!(resolution.edges[0].label, "Units");
    assert_eq!(resolution.edges[0].from, Node::Root("app".to_string()));
}

#[test]
fn several_roots_share_one_solution() {
    // A path package and the application both depend on `c`.
    let mut index = MemoryIndex::new();
    for version in ["1.0.0", "1.2.0"] {
        index.add("o/c", version, &[]);
    }
    let roots = [
        root(&[("o/c", "^1")]),
        Root {
            id: "path+../lib".to_string(),
            dependencies: vec![Dependency::new("o/c", r("^1.1"))],
        },
    ];
    let resolution = resolve(&roots, &index, &Options::default()).expect("resolves");
    assert_eq!(chosen(&resolution, "o/c"), ["1.2.0"]);
    assert_eq!(resolution.edges.len(), 2);
}

#[test]
fn build_order_puts_dependencies_first() {
    let mut index = MemoryIndex::new();
    index
        .add("o/a", "1.0.0", &[("o/b", "^1")])
        .add("o/b", "1.0.0", &[("o/c", "^1")])
        .add("o/c", "1.0.0", &[]);
    let resolution = run(&index, &[("o/a", "^1")]).expect("resolves");
    let order: Vec<String> = resolution
        .build_order()
        .into_iter()
        .map(|p| p.package)
        .collect();
    assert_eq!(order, ["o/c", "o/b", "o/a"]);
}

// ---------------------------------------------------------------------------
// Property test: the solver against an exhaustive search.

struct Item {
    package: usize,
    version: Version,
    yanked: bool,
    deps: Vec<(usize, VersionReq)>,
}

const NAMES: [&str; 4] = ["p0", "p1", "p2", "p3"];

fn universe(rng: &mut Rng) -> (Vec<Item>, Vec<(usize, VersionReq)>) {
    let versions = [
        "0.1.0", "0.1.5", "0.2.0", "1.0.0", "1.1.0", "1.2.0", "2.0.0",
    ];
    let reqs = [
        "^0.1",
        "^0.2",
        "^1",
        "^1.1",
        "^2",
        "*",
        ">=0.1.5, <1",
        "=1.0.0",
        "~1.1.0",
        ">=1, <3",
    ];
    let mut items = Vec::new();
    for package in 0..NAMES.len() {
        let mut taken = BTreeSet::new();
        for _ in 0..(1 + rng.below(3)) {
            taken.insert(rng.below(versions.len() as u64) as usize);
        }
        for position in taken {
            let mut deps = Vec::new();
            for _ in 0..rng.below(3) {
                deps.push((
                    rng.below(NAMES.len() as u64) as usize,
                    r(reqs[rng.below(reqs.len() as u64) as usize]),
                ));
            }
            items.push(Item {
                package,
                version: v(versions[position]),
                yanked: rng.chance(10),
                deps,
            });
        }
    }
    let mut roots = Vec::new();
    for _ in 0..(1 + rng.below(3)) {
        roots.push((
            rng.below(NAMES.len() as u64) as usize,
            r(reqs[rng.below(reqs.len() as u64) as usize]),
        ));
    }
    (items, roots)
}

fn valid(
    items: &[Item],
    roots: &[(usize, VersionReq)],
    chosen: &[usize],
    unify: Unify,
) -> bool {
    if chosen.iter().any(|i| items[*i].yanked) {
        return false;
    }
    for (a, left) in chosen.iter().enumerate() {
        for right in &chosen[a + 1..] {
            let (x, y) = (&items[*left], &items[*right]);
            if x.package == y.package {
                let same = match unify {
                    | Unify::Package => true,
                    | Unify::Series => x.version.series() == y.version.series(),
                };
                if same {
                    return false;
                }
            }
        }
    }
    let met = |package: usize, req: &VersionReq| {
        chosen
            .iter()
            .any(|i| items[*i].package == package && req.matches(&items[*i].version))
    };
    if !roots.iter().all(|(package, req)| met(*package, req)) {
        return false;
    }
    if !chosen.iter().all(|i| {
        items[*i]
            .deps
            .iter()
            .all(|(package, req)| met(*package, req))
    }) {
        return false;
    }
    // Acyclic, with an edge to every chosen version that could satisfy a
    // requirement (the solver's definition).
    let edges = |from: usize| -> Vec<usize> {
        chosen
            .iter()
            .copied()
            .filter(|to| {
                items[from].deps.iter().any(|(package, req)| {
                    items[*to].package == *package && req.matches(&items[*to].version)
                })
            })
            .collect()
    };
    let mut state: BTreeMap<usize, u8> = BTreeMap::new();
    fn dfs(
        node: usize,
        edges: &dyn Fn(usize) -> Vec<usize>,
        state: &mut BTreeMap<usize, u8>,
    ) -> bool {
        match state.get(&node) {
            | Some(1) => return false,
            | Some(_) => return true,
            | None => {},
        }
        state.insert(node, 1);
        for next in edges(node) {
            if !dfs(next, edges, state) {
                return false;
            }
        }
        state.insert(node, 2);
        true
    }
    chosen.iter().all(|i| dfs(*i, &edges, &mut state))
}

fn exists(
    items: &[Item],
    roots: &[(usize, VersionReq)],
    unify: Unify,
) -> bool {
    (0u32..(1 << items.len())).any(|mask| {
        let chosen: Vec<usize> = (0..items.len()).filter(|i| mask >> i & 1 == 1).collect();
        valid(items, roots, &chosen, unify)
    })
}

#[test]
fn the_solver_agrees_with_exhaustive_search_on_random_universes() {
    let mut rng = Rng::new(0x5EED);
    let mut solved = 0;
    let mut unsolved = 0;
    for round in 0..2000 {
        let (items, roots) = universe(&mut rng);
        if items.len() > 13 {
            continue;
        }
        let mut index = MemoryIndex::new();
        for item in &items {
            let deps: Vec<Dependency> = item
                .deps
                .iter()
                .map(|(package, req)| Dependency::new(NAMES[*package], req.clone()))
                .collect();
            index.insert(
                NAMES[item.package],
                Summary {
                    version: item.version.clone(),
                    dependencies: deps,
                    yanked: item.yanked,
                },
            );
        }
        let root = Root {
            id: "app".to_string(),
            dependencies: roots
                .iter()
                .map(|(package, req)| Dependency::new(NAMES[*package], req.clone()))
                .collect(),
        };
        for unify in [Unify::Series, Unify::Package] {
            let options = Options {
                unify,
                ..Options::default()
            };
            let outcome = resolve(std::slice::from_ref(&root), &index, &options);
            let possible = exists(&items, &roots, unify);
            match outcome {
                | Ok(resolution) => {
                    let mask: Vec<usize> = resolution
                        .packages
                        .iter()
                        .map(|chosen| {
                            items
                                .iter()
                                .position(|item| {
                                    NAMES[item.package] == chosen.package
                                        && item.version == chosen.version
                                })
                                .expect("the solver chose something that exists")
                        })
                        .collect();
                    assert!(
                        valid(&items, &roots, &mask, unify),
                        "round {round} ({unify:?}): the solver returned an invalid solution \
                         {:?}",
                        resolution.packages
                    );
                    assert!(possible, "round {round}: oracle disagrees");
                    solved += 1;
                },
                | Err(ResolveError::Unsatisfiable(conflict)) => {
                    assert!(
                        !possible,
                        "round {round} ({unify:?}): the solver missed a solution; it said:\n{}",
                        conflict.explain()
                    );
                    assert!(!conflict.explain().is_empty());
                    unsolved += 1;
                },
                | Err(other) => panic!("round {round}: unexpected {other}"),
            }
        }
    }
    // The generator must exercise both outcomes, or the test proves little.
    assert!(solved > 150, "only {solved} solvable universes");
    assert!(unsolved > 150, "only {unsolved} unsolvable universes");
}

#[test]
fn newest_is_chosen_whenever_nothing_forces_otherwise_on_random_chains() {
    // A chain a -> b -> c with no upper pressure: everything is the newest.
    let mut rng = Rng::new(404);
    for _ in 0..50 {
        let mut index = MemoryIndex::new();
        let mut newest = BTreeMap::new();
        for name in ["o/a", "o/b", "o/c"] {
            let mut best = v("0.0.0");
            for _ in 0..(1 + rng.below(5)) {
                let version = Version::new(1, rng.below(10), rng.below(10));
                if version > best {
                    best = version.clone();
                }
                let deps: Vec<(&str, &str)> = match name {
                    | "o/a" => vec![("o/b", "^1")],
                    | "o/b" => vec![("o/c", "^1")],
                    | _ => vec![],
                };
                // Duplicate versions are dropped by the resolver.
                index.add(name, &version.to_string(), &deps);
            }
            newest.insert(name, best.to_string());
        }
        let resolution = run(&index, &[("o/a", "^1")]).expect("resolves");
        for (name, best) in newest {
            assert_eq!(chosen(&resolution, name), [best], "{name}");
        }
    }
}

#[test]
fn a_deep_index_chain_resolves_and_explains_failures_without_using_the_host_stack() {
    const PACKAGES: usize = 4_000;
    let mut index = MemoryIndex::new();
    for package in 1..PACKAGES {
        let name = format!("o/package-{package:04}");
        let next = format!("o/package-{:04}", package + 1);
        index.add(&name, "1.0.0", &[(&next, "^1")]);
    }
    let query = [("o/package-0001", "^1")];
    let error = run(&index, &query).expect_err("the last package is missing");
    let explanation = error.to_string();
    assert!(explanation.contains("package-0001"), "{explanation}");
    assert!(
        explanation.contains("explanation is cut here"),
        "{explanation}"
    );

    index.add(&format!("o/package-{PACKAGES:04}"), "1.0.0", &[]);
    let resolution = run(&index, &query).expect("the long chain resolves");
    let build_order = resolution.build_order();
    assert_eq!(build_order.len(), PACKAGES);
    assert_eq!(
        build_order.first().map(|package| package.package.as_str()),
        Some("o/package-4000"),
    );
    assert_eq!(
        build_order.last().map(|package| package.package.as_str()),
        Some("o/package-0001"),
    );
}
