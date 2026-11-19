//! The dependency resolver.
//!
//! # Problem
//!
//! Given the requirements of one or more *roots* (the application and every
//! path package it pulls in, whose own dependencies are fixed by their
//! manifests) choose a set of registry package versions such that
//!
//! 1. every requirement of every root is met by a chosen version;
//! 2. every chosen version has each of its own dependencies met by a chosen
//!    version;
//! 3. at most one version is chosen per *compatibility series* of a package
//!    (see [`Unify`]);
//! 4. the chosen versions form no dependency cycle;
//! 5. nothing yanked is chosen unless the caller's lock already names it.
//!
//! and, among the solutions, prefer the one that keeps locked versions and
//! otherwise takes the highest compatible version of everything.
//!
//! # Why one version per series, and not `PubGrub`
//!
//! `PubGrub` (Dart, uv) is the best algorithm when a package has at most one
//! version in the solution: its learned incompatibilities give excellent
//! explanations. Aevia deliberately allows two *incompatible* majors of one
//! package to coexist (`units 1.x` for one dependent, `units 2.x` for another,
//! each imported under its own name), as Cargo does. A requirement such as
//! `>=1, <3` can then be met by either of two different "package slots", which a
//! `PubGrub` term cannot express without a disjunction over slots. So this is a
//! backtracking solver of the Cargo family, with the two properties that matter
//! in practice:
//!
//! * **Conflict-directed backjumping.** Every failure carries the set of
//!   *decisions* it depends on. When a failing subtree does not depend on the
//!   candidate that was just tried, the remaining candidates of that decision
//!   are skipped, so an unrelated choice made earlier does not multiply the
//!   work. (A test pins this: twelve independent two-version packages plus an
//!   unsatisfiable pair resolve in a handful of steps, not `2^12`.)
//! * **Fewest-candidates-first.** The next requirement to satisfy is the one
//!   with the fewest selectable versions, so dead ends surface early and
//!   forced choices are made before free ones. A binary search in the sorted
//!   catalogue narrows each scan to the requirement's version interval before
//!   applying pre-release, yank and occupied-slot rules.
//!
//! The explanation of a failure is the derivation tree of the failed search:
//! which requirement could not be met, which candidate versions were tried for
//! it and why each one failed, recursively. Consecutive candidates that fail
//! identically are folded into a range.
//!
//! # Determinism
//!
//! The result depends only on the requirements, the *set* of index entries and
//! the options: candidates are ordered by (locked first, then highest version),
//! ties between requirements are broken by their order in the roots and the
//! summaries (which the caller sorts), and the output is sorted. Shuffling the
//! index changes nothing; a test pins it.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::fmt::Write as _;
use std::rc::Rc;

use crate::index::Dependency;
use crate::index::Index;
use crate::index::IndexError;
use crate::index::IndexErrorKind;
use crate::index::Summary;
use crate::req::VersionReq;
use crate::version::Series;
use crate::version::Version;

/// How many versions of one package may be chosen together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unify {
    /// At most one per compatibility series (`1.x`, `0.3.x`, `0.0.4`): two
    /// incompatible majors coexist, two compatible releases never do. Aevia's
    /// default.
    Series,
    /// At most one version of a package, whatever its number.
    Package,
}

/// Knobs of a resolution.
#[derive(Clone, Debug)]
pub struct Options {
    /// Versions a lock already chose, by package. They are tried first, and a
    /// yanked one among them stays selectable (a yank must not break a build
    /// that already depends on it).
    pub locked: BTreeMap<String, BTreeSet<Version>>,
    /// The unification rule.
    pub unify: Unify,
    /// Give up after this many candidate activations (a guard against
    /// adversarial indexes, not a limit real graphs approach).
    pub max_activations: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            locked: BTreeMap::new(),
            unify: Unify::Series,
            max_activations: 200_000,
        }
    }
}

/// A fixed node of the graph: the application, or a package whose dependency
/// list comes from a manifest rather than from the index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    /// A display name, also the `from` of its edges (`kinematics`, `path+../lib`).
    pub id: String,
    /// What it requires.
    pub dependencies: Vec<Dependency>,
}

/// A chosen version of a package.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageVersion {
    /// The index key.
    pub package: String,
    /// The chosen version.
    pub version: Version,
}

impl fmt::Display for PackageVersion {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        write!(formatter, "{} {}", self.package, self.version)
    }
}

/// An end of an edge.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Node {
    /// A fixed root.
    Root(String),
    /// A chosen package version.
    Package(PackageVersion),
}

impl fmt::Display for Node {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            | Self::Root(id) => formatter.write_str(id),
            | Self::Package(package) => write!(formatter, "{package}"),
        }
    }
}

/// One resolved dependency edge.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    /// The dependent.
    pub from: Node,
    /// The label the dependent imports it under.
    pub label: String,
    /// The version that satisfies the requirement (the highest chosen one that
    /// matches, when several chosen versions do).
    pub to: PackageVersion,
}

/// A successful resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    /// Every chosen package version, sorted.
    pub packages: Vec<PackageVersion>,
    /// Every dependency edge, sorted.
    pub edges: Vec<Edge>,
}

impl Resolution {
    /// The chosen versions of one package, ascending.
    pub fn versions_of<'a>(
        &'a self,
        package: &'a str,
    ) -> impl Iterator<Item = &'a Version> + 'a {
        self.packages
            .iter()
            .filter(move |chosen| chosen.package == package)
            .map(|chosen| &chosen.version)
    }

    /// The edges leaving `node`.
    #[must_use]
    pub fn edges_from(
        &self,
        node: &Node,
    ) -> Vec<&Edge> {
        self.edges
            .iter()
            .filter(|edge| &edge.from == node)
            .collect()
    }

    /// The nodes that depend on `target`.
    #[must_use]
    pub fn dependents_of(
        &self,
        target: &PackageVersion,
    ) -> Vec<&Node> {
        let mut out: Vec<&Node> = self
            .edges
            .iter()
            .filter(|edge| &edge.to == target)
            .map(|edge| &edge.from)
            .collect();
        out.dedup();
        out
    }

    /// The chosen packages with dependencies before dependents (a build order).
    #[must_use]
    pub fn build_order(&self) -> Vec<PackageVersion> {
        let mut targets = HashMap::<&PackageVersion, Vec<&PackageVersion>>::new();
        for edge in &self.edges {
            if let Node::Package(package) = &edge.from {
                targets.entry(package).or_default().push(&edge.to);
            }
        }
        let mut order = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = Vec::new();
        for package in &self.packages {
            stack.push((package, false));
            while let Some((package, expanded)) = stack.pop() {
                if expanded {
                    order.push(package.clone());
                } else if seen.insert(package) {
                    stack.push((package, true));
                    stack.extend(
                        targets
                            .get(package)
                            .into_iter()
                            .flatten()
                            .rev()
                            .map(|target| (*target, false)),
                    );
                }
            }
        }
        order
    }
}

// ---------------------------------------------------------------------------
// Explanations

/// Why a requirement could not be met.
#[derive(Clone, Debug)]
pub enum Failure {
    /// A requirement none of whose candidates work.
    Unsatisfied {
        /// Who asked, as displayed (`kinematics`, `plot 1.4.2`).
        requirer: String,
        /// The package key.
        package: String,
        /// What was asked.
        req: VersionReq,
        /// Why nothing worked.
        reason: Reason,
    },
    /// The chosen versions depend on each other in a ring.
    Cycle {
        /// The ring, first element repeated at the end.
        path: Vec<String>,
    },
}

/// The reason a requirement had no solution.
#[derive(Clone, Debug)]
pub enum Reason {
    /// The index has no such package.
    Unknown,
    /// The package exists but has no versions.
    NoVersions,
    /// Versions exist, none matches.
    NoMatch {
        /// Non-yanked versions that exist, ascending.
        available: Vec<Version>,
        /// Yanked versions that would match.
        yanked: Vec<Version>,
    },
    /// Matching versions exist and each was rejected.
    Candidates {
        /// Newest first.
        entries: Vec<Entry>,
        /// Candidates outside the retained diagnostic detail.
        omitted: usize,
    },
}

/// One rejected candidate.
#[derive(Clone, Debug)]
pub struct Entry {
    /// The candidate.
    pub version: Version,
    /// Why it was rejected.
    pub cause: Cause,
}

/// Why a candidate was rejected.
#[derive(Clone, Debug)]
pub enum Cause {
    /// Another version of the same series is already selected.
    Taken {
        /// The selected version.
        selected: Version,
        /// Who required the selected version.
        wanted_by: Vec<String>,
    },
    /// Selecting it led to this failure.
    Failed(Box<Failure>),
}

/// An unsatisfiable resolution, with a bounded derivation for diagnostics.
#[derive(Clone, Debug)]
pub struct Conflict {
    /// The failed search.
    pub failure: Failure,
}

impl Conflict {
    /// A multi-line, human-readable explanation.
    #[must_use]
    pub fn explain(&self) -> String {
        let mut out = String::new();
        render(&self.failure, EXPLAIN_DEPTH, &mut out);
        out
    }
}

/// Why resolution did not produce a solution.
#[derive(Clone, Debug)]
pub enum ResolveError {
    /// No set of versions satisfies the requirements.
    Unsatisfiable(Conflict),
    /// The index could not be read (typically: a package is not fetched yet).
    Index(IndexError),
    /// The search exceeded [`Options::max_activations`].
    TooComplex {
        /// The limit that was hit.
        limit: usize,
    },
}

impl ResolveError {
    /// The package the index could not provide, when that is the reason.
    #[must_use]
    pub fn unavailable_package(&self) -> Option<&str> {
        match self {
            | Self::Index(error) if error.kind == IndexErrorKind::Unavailable => {
                Some(&error.package)
            },
            | _ => None,
        }
    }
}

impl fmt::Display for ResolveError {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            | Self::Unsatisfiable(conflict) => {
                write!(
                    formatter,
                    "the dependency requirements cannot be satisfied together:\n{}",
                    conflict.explain().trim_end()
                )
            },
            | Self::Index(error) => write!(formatter, "{error}"),
            | Self::TooComplex { limit } => {
                write!(
                    formatter,
                    "dependency resolution gave up after {limit} attempts; narrow a version \
                     requirement to help it"
                )
            },
        }
    }
}

impl std::error::Error for ResolveError {}

fn quoted(
    package: &str,
    version: &Version,
) -> String {
    format!("`{package} {version}`")
}

const EXPLAIN_DEPTH: usize = 10;

fn render(
    failure: &Failure,
    left: usize,
    out: &mut String,
) {
    match failure {
        | Failure::Cycle { path } => {
            let _ = writeln!(
                out,
                "these versions depend on each other in a ring: {}",
                path.join(" -> ")
            );
        },
        | Failure::Unsatisfied {
            requirer,
            package,
            req,
            reason,
        } => {
            let _ = write!(out, "`{requirer}` requires `{package} {req}`");
            match reason {
                | Reason::Unknown => {
                    let _ = writeln!(out, ", but `{package}` is not in the index");
                },
                | Reason::NoVersions => {
                    let _ = writeln!(out, ", but `{package}` has no published versions");
                },
                | Reason::NoMatch { available, yanked } => {
                    let _ = write!(out, ", but no version of `{package}` matches");
                    if available.is_empty() {
                        let _ = write!(out, " (it has none that is not yanked)");
                    } else {
                        let _ = write!(out, " (available: {})", join_versions(available));
                    }
                    if !yanked.is_empty() {
                        let _ = write!(
                            out,
                            "; {} would match but is yanked, and only a lock file can keep a \
                             yanked version",
                            join_versions(yanked)
                        );
                    }
                    out.push('\n');
                },
                | Reason::Candidates { entries, omitted } => {
                    let _ = writeln!(out, ", and every candidate fails:");
                    render_entries(package, entries, *omitted, left, out);
                },
            }
        },
    }
}

fn join_versions(versions: &[Version]) -> String {
    const SHOWN: usize = 8;
    let mut text: Vec<String> = versions
        .iter()
        .take(SHOWN)
        .map(Version::to_string)
        .collect();
    if versions.len() > SHOWN {
        text.push(format!("... {} more", versions.len().saturating_sub(SHOWN)));
    }
    text.join(", ")
}

/// The part of an entry that does not name its own version, so that
/// consecutive candidates failing the same way fold into one line.
#[allow(
    clippy::single_call_fn,
    reason = "one candidate is rendered independently of grouping"
)]
fn entry_body(
    package: &str,
    entry: &Entry,
    left: usize,
) -> String {
    let mut body = String::new();
    match &entry.cause {
        | Cause::Taken { selected, wanted_by } => {
            let who = if wanted_by.is_empty() {
                String::new()
            } else {
                format!(" (required by {})", list_names(wanted_by))
            };
            let _ = writeln!(
                body,
                "cannot be chosen: {} is already chosen{who}, and only one version of a \
                 compatibility series can be",
                quoted(package, selected)
            );
        },
        | Cause::Failed(inner) => {
            if left == 0 {
                body.push_str("fails (the explanation is cut here)\n");
            } else {
                body.push_str("was tried, but:\n");
                let mut nested = String::new();
                render(inner, left.saturating_sub(1), &mut nested);
                // The candidate's own name inside the nested text becomes a
                // placeholder; the caller puts the right name back.
                let own = format!("{package} {}", entry.version);
                body.push_str(&nested.replace(&own, PLACEHOLDER));
            }
        },
    }
    body
}

/// Stands for "this candidate" while consecutive entries are compared.
const PLACEHOLDER: &str = "\u{a7}";

#[allow(
    clippy::single_call_fn,
    reason = "limits the required-by list independently of candidate rendering"
)]
fn list_names(names: &[String]) -> String {
    let mut shown: Vec<String> = names
        .iter()
        .take(4)
        .map(|name| format!("`{name}`"))
        .collect();
    if names.len() > 4 {
        shown.push(format!("{} more", names.len().saturating_sub(4)));
    }
    shown.join(", ")
}

#[allow(
    clippy::single_call_fn,
    reason = "the presentation pass groups equivalent failure explanations"
)]
fn render_entries(
    package: &str,
    entries: &[Entry],
    omitted: usize,
    left: usize,
    out: &mut String,
) {
    let mut entries = entries
        .iter()
        .map(|entry| (entry, entry_body(package, entry, left)))
        .peekable();
    while let Some((entry, body)) = entries.next() {
        let first = &entry.version;
        let mut last = first;
        let mut count = 1usize;
        while let Some((next, _)) = entries.next_if(|(_, next_body)| *next_body == body) {
            last = &next.version;
            count = count.saturating_add(1);
        }
        let heading = match count {
            | 1 => quoted(package, first),
            | 2 => format!("`{package} {first}` and `{package} {last}`"),
            | _ => format!("`{package} {first}` down to `{package} {last}` ({count} versions)"),
        };
        let replacement = if count == 1 {
            format!("{package} {first}")
        } else {
            format!("{package} (each of these)")
        };
        let body = body.replace(PLACEHOLDER, &replacement);
        let mut lines = body.lines();
        if let Some(head) = lines.next() {
            let _ = writeln!(out, "  - {heading} {head}");
        }
        for line in lines {
            let _ = writeln!(out, "    {line}");
        }
    }
    if omitted > 0 {
        let _ = writeln!(out, "  - ... and {omitted} older candidates fail as well");
    }
}

// ---------------------------------------------------------------------------
// The search

#[derive(Clone)]
enum Origin {
    Root(usize),
    Active(usize),
}

#[derive(Clone)]
struct Pending {
    from: Origin,
    dep: Rc<Dependency>,
    /// Counts depend only on the immutable index and lock until a slot for this
    /// package is occupied. Cloning the pending queue preserves this work.
    unoccupied_choices: Option<usize>,
}

impl Pending {
    fn new(
        from: Origin,
        dependency: &Dependency,
    ) -> Self {
        Self {
            from,
            dep: Rc::new(dependency.clone()),
            unoccupied_choices: None,
        }
    }
}

struct Active {
    package: String,
    summary: Rc<Summary>,
}

impl Active {
    fn dependencies(&self) -> &[Dependency] {
        &self.summary.dependencies
    }
}

struct Fail {
    /// Indices into `Solver::active` this failure depends on.
    blame: BTreeSet<usize>,
    failure: Failure,
}

type Summaries = Rc<[Rc<Summary>]>;

/// Restricts the ordered catalogue to the requirement's precedence interval,
/// then applies the pre-release rule to just the versions inside that interval.
fn matching_summaries<'a>(
    summaries: &'a [Rc<Summary>],
    requirement: &'a VersionReq,
) -> impl DoubleEndedIterator<Item = &'a Rc<Summary>> {
    let interval = requirement.interval();
    let first = summaries.partition_point(|summary| !interval.meets_lower(&summary.version));
    let tail = summaries.get(first..).unwrap_or_default();
    let count = tail.partition_point(|summary| interval.meets_upper(&summary.version));
    tail.get(..count)
        .unwrap_or_default()
        .iter()
        .filter(|summary| requirement.permits_prerelease(&summary.version))
}

enum Availability {
    Free,
    Taken(usize),
    Yanked,
}

struct Classified {
    /// Selectable candidates, in preference order.
    free: Vec<Rc<Summary>>,
    /// Matching candidates whose series is already taken: (candidate, occupant).
    taken: Vec<(Rc<Summary>, usize)>,
    /// Matching yanked versions that are not locked.
    yanked: Vec<Version>,
}

/// A candidate's place in the active set and its slot for rollback.
struct Activation {
    summary: Rc<Summary>,
    key: Series,
    level: usize,
}

struct Decision {
    chosen: Pending,
    summaries: Summaries,
    remaining: Vec<Pending>,
    options: std::vec::IntoIter<Rc<Summary>>,
    class: Classified,
    accumulated: BTreeSet<usize>,
    tried: Vec<(Rc<Summary>, Failure)>,
    activation: Option<Activation>,
}

/// Bound retained conflict trees at the depth the formatter can display.
/// Child trees already obey the bound, so truncation and destruction stay shallow.
fn retain_conflict_depth(
    failure: &mut Failure,
    depth: usize,
) {
    if let Failure::Unsatisfied {
        reason: Reason::Candidates { entries, omitted },
        ..
    } = failure
    {
        if depth == 0 {
            *omitted = omitted.saturating_add(entries.len());
            entries.clear();
        } else {
            for entry in entries {
                if let Cause::Failed(inner) = &mut entry.cause {
                    retain_conflict_depth(inner, depth.saturating_sub(1));
                }
            }
        }
    }
}

struct Solver<'a> {
    index: &'a dyn Index,
    options: &'a Options,
    roots: &'a [Root],
    cache: HashMap<String, Summaries>,
    unknown: HashSet<String>,
    active: Vec<Active>,
    slots: HashMap<String, BTreeMap<Series, usize>>,
    steps: usize,
    abort: Option<ResolveError>,
}

impl Solver<'_> {
    const fn slot_key(
        &self,
        version: &Version,
    ) -> Series {
        match self.options.unify {
            | Unify::Series => version.series(),
            | Unify::Package => {
                Series {
                    major: 0,
                    minor: None,
                    patch: None,
                }
            },
        }
    }

    fn summaries(
        &mut self,
        package: &str,
    ) -> Summaries {
        if let Some(found) = self.cache.get(package) {
            return Rc::clone(found);
        }
        let mut list = match self.index.summaries(package) {
            | Ok(list) => list,
            | Err(error) => {
                if error.kind == IndexErrorKind::Unavailable && self.abort.is_none() {
                    self.abort = Some(ResolveError::Index(error));
                } else if error.kind == IndexErrorKind::Missing {
                    self.unknown.insert(package.to_string());
                }
                Vec::new()
            },
        };
        // Order must not depend on the index's iteration order.
        list.sort_by(|left, right| left.version.cmp(&right.version));
        list.dedup_by(|left, right| left.version == right.version);
        let list = list.into_iter().map(Rc::new).collect::<Summaries>();
        self.cache.insert(package.to_string(), Rc::clone(&list));
        list
    }

    fn satisfied(
        &self,
        dep: &Dependency,
    ) -> bool {
        self.slots.get(&dep.package).is_some_and(|slots| {
            slots
                .values()
                .filter_map(|index| self.active.get(*index))
                .any(|active| dep.req.matches(&active.summary.version))
        })
    }

    fn is_locked(
        &self,
        package: &str,
        version: &Version,
    ) -> bool {
        self.options
            .locked
            .get(package)
            .is_some_and(|versions| versions.contains(version))
    }

    fn availability(
        &self,
        package: &str,
        summary: &Summary,
        slots: Option<&BTreeMap<Series, usize>>,
    ) -> Availability {
        if summary.yanked && !self.is_locked(package, &summary.version) {
            return Availability::Yanked;
        }
        slots
            .and_then(|slots| slots.get(&self.slot_key(&summary.version)))
            .map_or(Availability::Free, |occupant| {
                Availability::Taken(*occupant)
            })
    }

    #[allow(
        clippy::single_call_fn,
        reason = "counting for the decision heuristic does not allocate conflict evidence"
    )]
    fn choice_count(
        &mut self,
        pending: &mut Pending,
        limit: usize,
    ) -> usize {
        let dep = &pending.dep;
        let occupied = self
            .slots
            .get(&dep.package)
            .is_some_and(|slots| !slots.is_empty());
        if !occupied && let Some(count) = pending.unoccupied_choices {
            return count;
        }
        let summaries = self.summaries(&dep.package);
        let slots = self
            .slots
            .get(&dep.package)
            .filter(|slots| !slots.is_empty());
        let free = matching_summaries(&summaries, &dep.req).filter(|summary| {
            matches!(
                self.availability(&dep.package, summary, slots),
                Availability::Free
            )
        });
        if occupied {
            // A tie cannot displace the earlier requirement. Counts reaching
            // the current best only need to serve as a lower bound.
            free.take(limit).count()
        } else {
            let count = free.count();
            pending.unoccupied_choices = Some(count);
            count
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "only the selected requirement needs candidate order and conflict evidence"
    )]
    fn classify(
        &mut self,
        dep: &Dependency,
    ) -> (Summaries, Classified) {
        let summaries = self.summaries(&dep.package);
        let slots = self.slots.get(&dep.package);
        let mut free = Vec::new();
        let mut taken = Vec::new();
        let mut yanked = Vec::new();
        for summary in matching_summaries(&summaries, &dep.req).rev() {
            match self.availability(&dep.package, summary, slots) {
                | Availability::Free => free.push(Rc::clone(summary)),
                | Availability::Taken(occupant) => taken.push((Rc::clone(summary), occupant)),
                | Availability::Yanked => yanked.push(summary.version.clone()),
            }
        }
        // Newest first within each group, with locked versions tried first.
        if let Some(locked) = self.options.locked.get(&dep.package) {
            free.sort_by_key(|summary| !locked.contains(&summary.version));
        }
        (summaries, Classified { free, taken, yanked })
    }

    fn name_of(
        &self,
        from: &Origin,
    ) -> String {
        match from {
            | Origin::Root(index) => {
                self.roots
                    .get(*index)
                    .map_or_else(|| "<root>".into(), |root| root.id.clone())
            },
            | Origin::Active(index) => {
                self.active.get(*index).map_or_else(
                    || "<package>".into(),
                    |active| format!("{} {}", active.package, active.summary.version),
                )
            },
        }
    }

    /// Who required the version `occupant`: roots and chosen packages whose
    /// requirement on that package it satisfies.
    fn wanted_by(
        &self,
        target: &Active,
    ) -> Vec<String> {
        let mut out = Vec::new();
        for root in self.roots {
            if root.dependencies.iter().any(|dep| {
                dep.package == target.package && dep.req.matches(&target.summary.version)
            }) {
                out.push(root.id.clone());
            }
        }
        for active in &self.active {
            if active.dependencies().iter().any(|dep| {
                dep.package == target.package && dep.req.matches(&target.summary.version)
            }) {
                out.push(format!("{} {}", active.package, active.summary.version));
            }
        }
        out
    }

    const fn aborted() -> Fail {
        Fail {
            blame: BTreeSet::new(),
            failure: Failure::Cycle { path: Vec::new() },
        }
    }

    /// Chooses the next outstanding requirement. The pending order breaks ties.
    #[allow(
        clippy::single_call_fn,
        reason = "requirement selection is separate from candidate activation and rollback"
    )]
    fn decision(
        &mut self,
        pending: Vec<Pending>,
    ) -> Result<Option<Decision>, ()> {
        let mut remaining: Vec<Pending> = pending
            .into_iter()
            .filter(|pending| !self.satisfied(&pending.dep))
            .collect();
        let mut best: Option<(usize, usize)> = None;
        for (position, pending) in remaining.iter_mut().enumerate() {
            let limit = best.map_or(usize::MAX, |(_, count)| count);
            let count = self.choice_count(pending, limit);
            if self.abort.is_some() {
                return Err(());
            }
            if best.is_none() || count < limit {
                best = Some((position, count));
            }
            if count == 0 {
                break;
            }
        }
        // The position came from enumerating this same, unmodified vector.
        Ok(best.map(|(position, _)| {
            let chosen = remaining.remove(position);
            let (summaries, mut class) = self.classify(&chosen.dep);
            Decision {
                chosen,
                summaries,
                remaining,
                options: std::mem::take(&mut class.free).into_iter(),
                class,
                accumulated: BTreeSet::new(),
                tried: Vec::new(),
                activation: None,
            }
        }))
    }

    // Cold failures retain structured blame; boxing would change the internal error contract.
    #[allow(clippy::result_large_err)]
    #[allow(
        clippy::single_call_fn,
        reason = "explicit decision stack drives the separate conflict-directed search"
    )]
    fn solve(
        &mut self,
        mut pending: Vec<Pending>,
    ) -> Result<(), Fail> {
        let mut stack: Vec<Decision> = Vec::new();
        'requirements: loop {
            let mut failure = match self.decision(pending).map_err(|()| Self::aborted())? {
                | Some(decision) => {
                    stack.push(decision);
                    None
                },
                | None => {
                    match self.finish() {
                        | Ok(()) => return Ok(()),
                        | Err(failure) => Some(failure),
                    }
                },
            };
            while let Some(mut decision) = stack.pop() {
                if let Some(failed) = failure.take() {
                    // A parent decision has one live candidate while its child runs.
                    let Some(activation) = decision.activation.take() else {
                        failure = Some(failed);
                        continue;
                    };
                    self.active.pop();
                    if let Some(slots) = self.slots.get_mut(&decision.chosen.dep.package) {
                        slots.remove(&activation.key);
                    }
                    if !failed.blame.contains(&activation.level) {
                        // No alternative for this decision can address the failure.
                        failure = Some(failed);
                        continue;
                    }
                    decision.accumulated.extend(
                        failed
                            .blame
                            .into_iter()
                            .filter(|level| *level != activation.level),
                    );
                    decision.tried.push((activation.summary, failed.failure));
                }
                if let Some(summary) = decision.options.next() {
                    if self.steps >= self.options.max_activations {
                        self.abort = Some(ResolveError::TooComplex {
                            limit: self.options.max_activations,
                        });
                        return Err(Self::aborted());
                    }
                    self.steps = self.steps.saturating_add(1);
                    let level = self.active.len();
                    let key = self.slot_key(&summary.version);
                    let package = &decision.chosen.dep.package;
                    self.active.push(Active {
                        package: package.clone(),
                        summary: Rc::clone(&summary),
                    });
                    self.slots
                        .entry(package.clone())
                        .or_default()
                        .insert(key.clone(), level);
                    pending = if decision.options.len() == 0 {
                        std::mem::take(&mut decision.remaining)
                    } else {
                        decision.remaining.clone()
                    };
                    pending.extend(
                        summary
                            .dependencies
                            .iter()
                            .map(|dependency| Pending::new(Origin::Active(level), dependency)),
                    );
                    decision.activation = Some(Activation { summary, key, level });
                    stack.push(decision);
                    continue 'requirements;
                }
                failure = Some(self.exhausted(
                    &decision.chosen,
                    &decision.summaries,
                    &decision.class,
                    decision.accumulated,
                    decision.tried,
                ));
            }
            return Err(failure.unwrap_or_else(Self::aborted));
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "exhaustion constructs the public conflict derivation"
    )]
    fn exhausted(
        &self,
        chosen: &Pending,
        summaries: &[Rc<Summary>],
        class: &Classified,
        mut blame: BTreeSet<usize>,
        tried: Vec<(Rc<Summary>, Failure)>,
    ) -> Fail {
        const KEEP: usize = 12;
        if let Origin::Active(index) = chosen.from {
            blame.insert(index);
        }
        for (_, occupant) in &class.taken {
            blame.insert(*occupant);
        }
        let dep = &chosen.dep;
        let reason = if summaries.is_empty() {
            if self.unknown.contains(&dep.package) {
                Reason::Unknown
            } else {
                Reason::NoVersions
            }
        } else if tried.is_empty() && class.taken.is_empty() {
            Reason::NoMatch {
                available: summaries
                    .iter()
                    .filter(|summary| !summary.yanked)
                    .map(|summary| summary.version.clone())
                    .collect(),
                yanked: class.yanked.clone(),
            }
        } else {
            let mut entries: Vec<Entry> = Vec::new();
            for (candidate, mut failure) in tried {
                retain_conflict_depth(&mut failure, EXPLAIN_DEPTH);
                entries.push(Entry {
                    version: candidate.version.clone(),
                    cause: Cause::Failed(Box::new(failure)),
                });
            }
            for (candidate, occupant) in &class.taken {
                if let Some(active) = self.active.get(*occupant) {
                    entries.push(Entry {
                        version: candidate.version.clone(),
                        cause: Cause::Taken {
                            selected: active.summary.version.clone(),
                            wanted_by: self.wanted_by(active),
                        },
                    });
                }
            }
            entries.sort_by(|left, right| right.version.cmp(&left.version));
            let omitted = entries.len().saturating_sub(KEEP);
            entries.truncate(KEEP);
            Reason::Candidates { entries, omitted }
        };
        Fail {
            blame,
            failure: Failure::Unsatisfied {
                requirer: self.name_of(&chosen.from),
                package: dep.package.clone(),
                req: dep.req.clone(),
                reason,
            },
        }
    }

    /// Every requirement is met; reject a ring of dependencies.
    #[allow(
        clippy::result_large_err,
        reason = "cycle failures carry the same backjump information"
    )]
    #[allow(
        clippy::single_call_fn,
        reason = "the resolved graph is validated before emitting a solution"
    )]
    fn finish(&self) -> Result<(), Fail> {
        let adjacency: Vec<Vec<usize>> = self
            .active
            .iter()
            .map(|active| {
                let mut targets = Vec::new();
                for dependency in active.dependencies() {
                    let first = targets.len();
                    targets.extend(
                        self.slots
                            .get(&dependency.package)
                            .into_iter()
                            .flat_map(BTreeMap::values)
                            .filter(|index| {
                                self.active.get(**index).is_some_and(|target| {
                                    dependency.req.matches(&target.summary.version)
                                })
                            })
                            .copied(),
                    );
                    // Preserve the original ordering: declaration first, then activation.
                    if let Some(matching) = targets.get_mut(first..) {
                        matching.sort_unstable();
                    }
                }
                targets
            })
            .collect();
        let mut complete = HashSet::new();
        let mut visiting = HashMap::new();
        for (start, targets) in adjacency.iter().enumerate() {
            if complete.contains(&start) {
                continue;
            }
            let mut stack = vec![(start, targets.iter())];
            visiting.insert(start, 0);
            while let Some((node, targets)) = stack.last_mut() {
                if let Some(&target) = targets.next() {
                    if complete.contains(&target) {
                        continue;
                    }
                    if let Some(&from) = visiting.get(&target) {
                        let ring: BTreeSet<usize> =
                            stack.iter().skip(from).map(|(index, _)| *index).collect();
                        let mut path: Vec<String> = stack
                            .iter()
                            .skip(from)
                            .filter_map(|(index, _)| self.active.get(*index))
                            .map(|active| format!("{} {}", active.package, active.summary.version))
                            .collect();
                        if let Some(first) = path.first().cloned() {
                            path.push(first);
                        }
                        return Err(Fail {
                            blame: ring,
                            failure: Failure::Cycle { path },
                        });
                    }
                    if let Some(targets) = adjacency.get(target) {
                        visiting.insert(target, stack.len());
                        stack.push((target, targets.iter()));
                    }
                } else {
                    complete.insert(*node);
                    visiting.remove(node);
                    stack.pop();
                }
            }
        }
        Ok(())
    }

    #[allow(
        clippy::single_call_fn,
        reason = "converts search state to the public resolution graph"
    )]
    fn resolution(&self) -> Resolution {
        let target_of = |dependency: &Dependency| -> Option<PackageVersion> {
            self.slots.get(&dependency.package).and_then(|slots| {
                slots
                    .values()
                    .filter_map(|index| self.active.get(*index))
                    .filter(|active| dependency.req.matches(&active.summary.version))
                    .max_by(|left, right| left.summary.version.cmp(&right.summary.version))
                    .map(|active| {
                        PackageVersion {
                            package: active.package.clone(),
                            version: active.summary.version.clone(),
                        }
                    })
            })
        };
        let mut edges = Vec::new();
        for root in self.roots {
            for dependency in &root.dependencies {
                if let Some(to) = target_of(dependency) {
                    edges.push(Edge {
                        from: Node::Root(root.id.clone()),
                        label: dependency.label.clone(),
                        to,
                    });
                }
            }
        }
        let mut packages: Vec<PackageVersion> = Vec::new();
        for active in &self.active {
            let from = PackageVersion {
                package: active.package.clone(),
                version: active.summary.version.clone(),
            };
            for dependency in active.dependencies() {
                if let Some(to) = target_of(dependency) {
                    edges.push(Edge {
                        from: Node::Package(from.clone()),
                        label: dependency.label.clone(),
                        to,
                    });
                }
            }
            packages.push(from);
        }
        packages.sort();
        edges.sort();
        edges.dedup();
        Resolution { packages, edges }
    }
}

/// Resolves `roots` against `index`.
///
/// # Errors
///
/// [`ResolveError::Unsatisfiable`] with a derivation when no solution exists,
/// [`ResolveError::Index`] when the index cannot answer,
/// [`ResolveError::TooComplex`] when the search budget is exhausted.
pub fn resolve(
    roots: &[Root],
    index: &dyn Index,
    options: &Options,
) -> Result<Resolution, ResolveError> {
    let mut solver = Solver {
        index,
        options,
        roots,
        cache: HashMap::new(),
        unknown: HashSet::new(),
        active: Vec::new(),
        slots: HashMap::new(),
        steps: 0,
        abort: None,
    };
    let mut pending = Vec::new();
    for (position, root) in roots.iter().enumerate() {
        for dependency in &root.dependencies {
            pending.push(Pending::new(Origin::Root(position), dependency));
        }
    }
    match solver.solve(pending) {
        | Ok(()) => Ok(solver.resolution()),
        | Err(fail) => {
            match solver.abort.take() {
                | Some(error) => Err(error),
                | None => {
                    Err(ResolveError::Unsatisfiable(Conflict {
                        failure: fail.failure,
                    }))
                },
            }
        },
    }
}
