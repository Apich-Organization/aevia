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
//! # Why one version per series, and not PubGrub
//!
//! PubGrub (Dart, uv) is the best algorithm when a package has at most one
//! version in the solution: its learned incompatibilities give excellent
//! explanations. Aevia deliberately allows two *incompatible* majors of one
//! package to coexist (`units 1.x` for one dependent, `units 2.x` for another,
//! each imported under its own name), as Cargo does. A requirement such as
//! `>=1, <3` can then be met by either of two different "package slots", which a
//! PubGrub term cannot express without a disjunction over slots. So this is a
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
//!   forced choices are made before free ones.
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
        let mut order = Vec::new();
        let mut seen: HashSet<&PackageVersion> = HashSet::new();
        for package in &self.packages {
            self.visit(package, &mut seen, &mut order);
        }
        order
    }

    fn visit<'a>(
        &'a self,
        package: &'a PackageVersion,
        seen: &mut HashSet<&'a PackageVersion>,
        order: &mut Vec<PackageVersion>,
    ) {
        if !seen.insert(package) {
            return;
        }
        let node = Node::Package(package.clone());
        let targets: Vec<&PackageVersion> = self
            .edges_from(&node)
            .into_iter()
            .map(|edge| &edge.to)
            .collect();
        for target in targets {
            self.visit(target, seen, order);
        }
        order.push(package.clone());
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
        /// Older candidates not listed.
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

/// An unsatisfiable resolution, with its derivation.
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
        text.push(format!("... {} more", versions.len() - SHOWN));
    }
    text.join(", ")
}

/// The part of an entry that does not name its own version, so that
/// consecutive candidates failing the same way fold into one line.
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
                render(inner, left - 1, &mut nested);
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

fn list_names(names: &[String]) -> String {
    let mut shown: Vec<String> = names
        .iter()
        .take(4)
        .map(|name| format!("`{name}`"))
        .collect();
    if names.len() > 4 {
        shown.push(format!("{} more", names.len() - 4));
    }
    shown.join(", ")
}

fn render_entries(
    package: &str,
    entries: &[Entry],
    omitted: usize,
    left: usize,
    out: &mut String,
) {
    let mut index = 0;
    while index < entries.len() {
        let body = entry_body(package, &entries[index], left);
        let mut end = index + 1;
        while end < entries.len() && entry_body(package, &entries[end], left) == body {
            end += 1;
        }
        let group = &entries[index..end];
        let first = &group[0].version;
        let last = &group[group.len() - 1].version;
        let heading = if group.len() == 1 {
            quoted(package, first)
        } else if group.len() == 2 {
            format!("`{package} {first}` and `{package} {last}`")
        } else {
            format!(
                "`{package} {first}` down to `{package} {last}` ({} versions)",
                group.len()
            )
        };
        let replacement = if group.len() == 1 {
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
        index = end;
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
}

struct Active {
    package: String,
    version: Version,
    summaries: Rc<Vec<Summary>>,
    at: usize,
}

impl Active {
    fn dependencies(&self) -> &[Dependency] {
        &self.summaries[self.at].dependencies
    }
}

struct Fail {
    /// Indices into `Solver::active` this failure depends on.
    blame: BTreeSet<usize>,
    failure: Failure,
}

struct Classified {
    /// Selectable candidates (indices into the summaries), in preference order.
    free: Vec<usize>,
    /// Matching candidates whose series is already taken: (candidate, occupant).
    taken: Vec<(usize, usize)>,
    /// Matching yanked versions that are not locked.
    yanked: Vec<Version>,
}

struct Solver<'a> {
    index: &'a dyn Index,
    options: &'a Options,
    roots: &'a [Root],
    cache: HashMap<String, Rc<Vec<Summary>>>,
    unknown: HashSet<String>,
    active: Vec<Active>,
    slots: HashMap<String, BTreeMap<Series, usize>>,
    steps: usize,
    abort: Option<ResolveError>,
}

impl<'a> Solver<'a> {
    fn slot_key(
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
    ) -> Rc<Vec<Summary>> {
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
        let list = Rc::new(list);
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
                .any(|index| dep.req.matches(&self.active[*index].version))
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

    fn classify(
        &mut self,
        dep: &Dependency,
    ) -> (Rc<Vec<Summary>>, Classified) {
        let summaries = self.summaries(&dep.package);
        let mut free = Vec::new();
        let mut taken = Vec::new();
        let mut yanked = Vec::new();
        for (position, summary) in summaries.iter().enumerate() {
            if !dep.req.matches(&summary.version) {
                continue;
            }
            let locked = self.is_locked(&dep.package, &summary.version);
            if summary.yanked && !locked {
                yanked.push(summary.version.clone());
                continue;
            }
            let key = self.slot_key(&summary.version);
            match self
                .slots
                .get(&dep.package)
                .and_then(|slots| slots.get(&key))
            {
                | Some(occupant) => taken.push((position, *occupant)),
                | None => free.push(position),
            }
        }
        // Locked first, then newest first. `summaries` is ascending, so a
        // reverse gives the newest; the stable sort keeps that within a group.
        free.reverse();
        free.sort_by_key(|position| !self.is_locked(&dep.package, &summaries[*position].version));
        taken.reverse();
        yanked.reverse();
        (summaries, Classified { free, taken, yanked })
    }

    fn name_of(
        &self,
        from: &Origin,
    ) -> String {
        match from {
            | Origin::Root(index) => self.roots[*index].id.clone(),
            | Origin::Active(index) => {
                let active = &self.active[*index];
                format!("{} {}", active.package, active.version)
            },
        }
    }

    /// Who required the version `occupant`: roots and chosen packages whose
    /// requirement on that package it satisfies.
    fn wanted_by(
        &self,
        occupant: usize,
    ) -> Vec<String> {
        let target = &self.active[occupant];
        let mut out = Vec::new();
        for root in self.roots {
            if root
                .dependencies
                .iter()
                .any(|dep| dep.package == target.package && dep.req.matches(&target.version))
            {
                out.push(root.id.clone());
            }
        }
        for active in &self.active {
            if active
                .dependencies()
                .iter()
                .any(|dep| dep.package == target.package && dep.req.matches(&target.version))
            {
                out.push(format!("{} {}", active.package, active.version));
            }
        }
        out
    }

    fn aborted() -> Fail {
        Fail {
            blame: BTreeSet::new(),
            failure: Failure::Cycle { path: Vec::new() },
        }
    }

    // Cold failures retain structured blame; boxing would change the internal error contract.
    #[allow(clippy::result_large_err)]
    fn solve(
        &mut self,
        pending: Vec<Pending>,
    ) -> Result<(), Fail> {
        let mut remaining: Vec<Pending> = pending
            .into_iter()
            .filter(|pending| !self.satisfied(&pending.dep))
            .collect();
        if remaining.is_empty() {
            return self.finish();
        }
        // Fewest selectable candidates first; the earliest wins a tie.
        let mut best: Option<(usize, Rc<Vec<Summary>>, Classified)> = None;
        for (position, pending) in remaining.iter().enumerate() {
            let dep = Rc::clone(&pending.dep);
            let (summaries, class) = self.classify(&dep);
            if self.abort.is_some() {
                return Err(Self::aborted());
            }
            let better = best
                .as_ref()
                .is_none_or(|(_, _, held)| class.free.len() < held.free.len());
            let stop = class.free.is_empty();
            if better {
                best = Some((position, summaries, class));
            }
            if stop {
                break;
            }
        }
        let Some((position, summaries, class)) = best else {
            return self.finish();
        };
        let chosen = remaining.remove(position);
        let mut accumulated: BTreeSet<usize> = BTreeSet::new();
        let mut tried: Vec<(usize, Failure)> = Vec::new();
        for &candidate in &class.free {
            self.steps += 1;
            if self.steps > self.options.max_activations {
                self.abort = Some(ResolveError::TooComplex {
                    limit: self.options.max_activations,
                });
                return Err(Self::aborted());
            }
            let level = self.active.len();
            let summary = &summaries[candidate];
            let key = self.slot_key(&summary.version);
            self.active.push(Active {
                package: chosen.dep.package.clone(),
                version: summary.version.clone(),
                summaries: Rc::clone(&summaries),
                at: candidate,
            });
            self.slots
                .entry(chosen.dep.package.clone())
                .or_default()
                .insert(key.clone(), level);
            let mut next = remaining.clone();
            for dependency in &summary.dependencies {
                next.push(Pending {
                    from: Origin::Active(level),
                    dep: Rc::new(dependency.clone()),
                });
            }
            let outcome = self.solve(next);
            match outcome {
                | Ok(()) => return Ok(()),
                | Err(fail) => {
                    self.active.pop();
                    if let Some(slots) = self.slots.get_mut(&chosen.dep.package) {
                        slots.remove(&key);
                    }
                    if self.abort.is_some() {
                        return Err(fail);
                    }
                    if !fail.blame.contains(&level) {
                        // The failure does not depend on this candidate, so no
                        // other candidate of this requirement can repair it.
                        return Err(fail);
                    }
                    accumulated.extend(fail.blame.iter().copied().filter(|l| *l != level));
                    tried.push((candidate, fail.failure));
                },
            }
        }
        Err(self.exhausted(&chosen, &summaries, &class, accumulated, tried))
    }

    fn exhausted(
        &self,
        chosen: &Pending,
        summaries: &Rc<Vec<Summary>>,
        class: &Classified,
        mut blame: BTreeSet<usize>,
        tried: Vec<(usize, Failure)>,
    ) -> Fail {
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
        } else if class.free.is_empty() && class.taken.is_empty() {
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
            for (candidate, failure) in tried {
                entries.push(Entry {
                    version: summaries[candidate].version.clone(),
                    cause: Cause::Failed(Box::new(failure)),
                });
            }
            for (candidate, occupant) in &class.taken {
                entries.push(Entry {
                    version: summaries[*candidate].version.clone(),
                    cause: Cause::Taken {
                        selected: self.active[*occupant].version.clone(),
                        wanted_by: self.wanted_by(*occupant),
                    },
                });
            }
            entries.sort_by(|left, right| right.version.cmp(&left.version));
            const KEEP: usize = 12;
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
    #[allow(clippy::result_large_err)] // Same structured cold failure as `solve`.
    fn finish(&self) -> Result<(), Fail> {
        let count = self.active.len();
        let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); count];
        for (from, active) in self.active.iter().enumerate() {
            for dependency in active.dependencies() {
                for (to, other) in self.active.iter().enumerate() {
                    if other.package == dependency.package && dependency.req.matches(&other.version)
                    {
                        adjacency[from].push(to);
                    }
                }
            }
        }
        // 0 = unvisited, 1 = on the stack, 2 = done.
        let mut state = vec![0u8; count];
        for start in 0..count {
            if state[start] != 0 {
                continue;
            }
            let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
            let mut path: Vec<usize> = vec![start];
            state[start] = 1;
            while let Some((node, next)) = stack.pop() {
                if next < adjacency[node].len() {
                    stack.push((node, next + 1));
                    let target = adjacency[node][next];
                    match state[target] {
                        | 0 => {
                            state[target] = 1;
                            stack.push((target, 0));
                            path.push(target);
                        },
                        | 1 => {
                            let from = path.iter().position(|n| *n == target).unwrap_or(0);
                            let ring: Vec<usize> = path[from..].to_vec();
                            let mut names: Vec<String> = ring
                                .iter()
                                .map(|n| {
                                    format!(
                                        "{} {}",
                                        self.active[*n].package, self.active[*n].version
                                    )
                                })
                                .collect();
                            names.push(names[0].clone());
                            return Err(Fail {
                                blame: ring.into_iter().collect(),
                                failure: Failure::Cycle { path: names },
                            });
                        },
                        | _ => {},
                    }
                } else {
                    state[node] = 2;
                    path.pop();
                }
            }
        }
        Ok(())
    }

    fn resolution(&self) -> Resolution {
        let target_of = |dependency: &Dependency| -> Option<PackageVersion> {
            self.slots.get(&dependency.package).and_then(|slots| {
                slots
                    .values()
                    .map(|index| &self.active[*index])
                    .filter(|active| dependency.req.matches(&active.version))
                    .max_by(|left, right| left.version.cmp(&right.version))
                    .map(|active| {
                        PackageVersion {
                            package: active.package.clone(),
                            version: active.version.clone(),
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
                version: active.version.clone(),
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
            pending.push(Pending {
                from: Origin::Root(position),
                dep: Rc::new(dependency.clone()),
            });
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
