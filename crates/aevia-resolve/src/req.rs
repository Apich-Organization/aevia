//! Version requirements: `^1.2`, `~1.2.3`, `>=1.2, <2`, `1.*`, `=1.2.3`, `*`.
//!
//! A requirement is a comma-separated conjunction using Cargo-style comparator
//! notation. Aevia applies the intervals below and then checks pre-release
//! eligibility:
//!
//! | written      | means                                        |
//! |--------------|----------------------------------------------|
//! | `1.2.3`      | `^1.2.3` (a bare version is a caret)         |
//! | `^1.2.3`     | `>=1.2.3, <2.0.0`                            |
//! | `^0.2.3`     | `>=0.2.3, <0.3.0`                            |
//! | `^0.0.3`     | `=0.0.3`                                     |
//! | `~1.2.3`     | `>=1.2.3, <1.3.0`                            |
//! | `~1`         | `>=1.0.0, <2.0.0`                            |
//! | `1.*`, `1.x` | `>=1.0.0, <2.0.0`                            |
//! | `=1.2`       | `>=1.2.0, <1.3.0`                            |
//! | `>1.2`       | `>=1.3.0`                                    |
//! | `<=1.2`      | `<1.3.0`                                     |
//! | `*`          | any release                                  |
//!
//! # Pre-releases
//!
//! A pre-release version satisfies a requirement only when a comparator of that
//! requirement names a pre-release of the *same* `major.minor.patch`
//! (`>=1.0.0-rc.1` accepts `1.0.0-rc.2`; `^1` never accepts `2.0.0-rc.1`, and
//! `*` never accepts any pre-release). This is the rule that stops
//! `aevia update` from silently adopting a release candidate. The interval
//! endpoints apply before this rule, so `^1, >=1.0.0-alpha` excludes
//! `1.0.0-beta`, which precedes the `^1` floor of `1.0.0`.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use crate::version::Identifier;
use crate::version::ParseError;
use crate::version::Version;
use crate::version::parse_prerelease;

/// A comparison operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    /// `=`
    Exact,
    /// `>`
    Greater,
    /// `>=`
    GreaterEq,
    /// `<`
    Less,
    /// `<=`
    LessEq,
    /// `~`
    Tilde,
    /// `^`, also what a bare version means.
    Caret,
    /// `1.*`, `1.2.x`
    Wildcard,
}

/// One comparator: an operator and a possibly partial version.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Comparator {
    /// How the version is compared.
    pub op: Op,
    /// The major number.
    pub major: u64,
    /// The minor number, absent in `1` and `1.*`.
    pub minor: Option<u64>,
    /// The patch number, absent in `1.2` and `1.2.*`.
    pub patch: Option<u64>,
    /// Pre-release identifiers; only legal with a full `major.minor.patch`.
    pub pre: Vec<Identifier>,
}

/// One end of an [`Interval`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bound {
    /// No limit.
    Unbounded,
    /// The version itself is inside.
    Included(Version),
    /// The version itself is outside.
    Excluded(Version),
}

/// A contiguous range of versions: what a requirement reduces to before the
/// pre-release rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interval {
    /// The lower end.
    pub lower: Bound,
    /// The upper end.
    pub upper: Bound,
}

impl Interval {
    /// Every version.
    #[must_use]
    pub const fn full() -> Self {
        Self {
            lower: Bound::Unbounded,
            upper: Bound::Unbounded,
        }
    }

    /// Whether `version` lies inside, ignoring the pre-release rule.
    #[must_use]
    pub fn contains(
        &self,
        version: &Version,
    ) -> bool {
        self.meets_lower(version) && self.meets_upper(version)
    }

    pub(crate) fn meets_lower(
        &self,
        version: &Version,
    ) -> bool {
        match &self.lower {
            | Bound::Unbounded => true,
            | Bound::Included(bound) => !version.cmp_precedence(bound).is_lt(),
            | Bound::Excluded(bound) => version.cmp_precedence(bound).is_gt(),
        }
    }

    pub(crate) fn meets_upper(
        &self,
        version: &Version,
    ) -> bool {
        match &self.upper {
            | Bound::Unbounded => true,
            | Bound::Included(bound) => !version.cmp_precedence(bound).is_gt(),
            | Bound::Excluded(bound) => version.cmp_precedence(bound).is_lt(),
        }
    }

    /// The versions inside both.
    #[must_use]
    pub fn intersect(
        &self,
        other: &Self,
    ) -> Self {
        Self {
            lower: tighter_lower(&self.lower, &other.lower),
            upper: tighter_upper(&self.upper, &other.upper),
        }
    }

    /// Whether no version can lie inside.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let (low, low_open) = match &self.lower {
            | Bound::Unbounded => return false,
            | Bound::Included(version) => (version, false),
            | Bound::Excluded(version) => (version, true),
        };
        if low_open && !low.is_prerelease() && low.triple() == (u64::MAX, u64::MAX, u64::MAX) {
            return true;
        }
        let (high, high_open) = match &self.upper {
            | Bound::Unbounded => return false,
            | Bound::Included(version) => (version, false),
            | Bound::Excluded(version) => (version, true),
        };
        match low.cmp_precedence(high) {
            | Ordering::Greater => true,
            | Ordering::Equal => low_open || high_open,
            | Ordering::Less => false,
        }
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "lower bound selection mirrors the separate upper bound rule"
)]
fn tighter_lower(
    left: &Bound,
    right: &Bound,
) -> Bound {
    match (left, right) {
        | (Bound::Unbounded, other) | (other, Bound::Unbounded) => other.clone(),
        | (Bound::Included(a) | Bound::Excluded(a), Bound::Included(b) | Bound::Excluded(b)) => {
            match a.cmp_precedence(b) {
                | Ordering::Greater => left.clone(),
                | Ordering::Less => right.clone(),
                // Equal versions: the open end is the tighter one.
                | Ordering::Equal => {
                    if matches!(left, Bound::Excluded(_)) {
                        left.clone()
                    } else {
                        right.clone()
                    }
                },
            }
        },
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "upper bound selection mirrors the separate lower bound rule"
)]
fn tighter_upper(
    left: &Bound,
    right: &Bound,
) -> Bound {
    match (left, right) {
        | (Bound::Unbounded, other) | (other, Bound::Unbounded) => other.clone(),
        | (Bound::Included(a) | Bound::Excluded(a), Bound::Included(b) | Bound::Excluded(b)) => {
            match a.cmp_precedence(b) {
                | Ordering::Less => left.clone(),
                | Ordering::Greater => right.clone(),
                | Ordering::Equal => {
                    if matches!(left, Bound::Excluded(_)) {
                        left.clone()
                    } else {
                        right.clone()
                    }
                },
            }
        },
    }
}

impl Comparator {
    fn floor(&self) -> Version {
        Version {
            major: self.major,
            minor: self.minor.unwrap_or(0),
            patch: self.patch.unwrap_or(0),
            pre: self.pre.clone(),
            build: Vec::new(),
        }
    }

    /// The first release after this major or major/minor prefix.
    fn next_after_partial(&self) -> Option<Version> {
        self.minor
            .and_then(|minor| minor.checked_add(1))
            .map(|minor| Version::new(self.major, minor, 0))
            .or_else(|| {
                self.major
                    .checked_add(1)
                    .map(|major| Version::new(major, 0, 0))
            })
    }

    /// The versions this comparator admits, before the pre-release rule.
    #[must_use]
    pub fn interval(&self) -> Interval {
        let floor = self.floor();
        let full = self.patch.is_some();
        match self.op {
            | Op::Exact => {
                if full {
                    Interval {
                        lower: Bound::Included(floor.clone()),
                        upper: Bound::Included(floor),
                    }
                } else {
                    Interval {
                        lower: Bound::Included(floor),
                        upper: self
                            .next_after_partial()
                            .map_or(Bound::Unbounded, Bound::Excluded),
                    }
                }
            },
            | Op::Greater => {
                if full {
                    Interval {
                        lower: Bound::Excluded(floor),
                        upper: Bound::Unbounded,
                    }
                } else {
                    Interval {
                        lower: self.next_after_partial().map_or_else(
                            || Bound::Excluded(Version::new(u64::MAX, u64::MAX, u64::MAX)),
                            Bound::Included,
                        ),
                        upper: Bound::Unbounded,
                    }
                }
            },
            | Op::GreaterEq => {
                Interval {
                    lower: Bound::Included(floor),
                    upper: Bound::Unbounded,
                }
            },
            | Op::Less => {
                Interval {
                    lower: Bound::Unbounded,
                    upper: Bound::Excluded(floor),
                }
            },
            | Op::LessEq => {
                if full {
                    Interval {
                        lower: Bound::Unbounded,
                        upper: Bound::Included(floor),
                    }
                } else {
                    Interval {
                        lower: Bound::Unbounded,
                        upper: self
                            .next_after_partial()
                            .map_or(Bound::Unbounded, Bound::Excluded),
                    }
                }
            },
            | Op::Tilde | Op::Wildcard => {
                Interval {
                    lower: Bound::Included(floor),
                    upper: self
                        .next_after_partial()
                        .map_or(Bound::Unbounded, Bound::Excluded),
                }
            },
            | Op::Caret => {
                let upper = if self.major > 0 {
                    self.major
                        .checked_add(1)
                        .map(|major| Version::new(major, 0, 0))
                } else {
                    Some(match (self.minor, self.patch) {
                        | (None, _) => Version::new(1, 0, 0),
                        | (Some(minor), _) if minor > 0 => {
                            minor
                                .checked_add(1)
                                .map_or(Version::new(1, 0, 0), |minor| Version::new(0, minor, 0))
                        },
                        | (Some(_), Some(patch)) => {
                            patch
                                .checked_add(1)
                                .map_or(Version::new(0, 1, 0), |patch| Version::new(0, 0, patch))
                        },
                        | (Some(_), None) => Version::new(0, 1, 0),
                    })
                };
                Interval {
                    lower: Bound::Included(floor),
                    upper: upper.map_or(Bound::Unbounded, Bound::Excluded),
                }
            },
        }
    }
}

/// A conjunction of comparators.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VersionReq {
    /// All of these must hold; empty means "any release".
    pub comparators: Vec<Comparator>,
}

impl VersionReq {
    /// `*`: any release (never a pre-release).
    #[must_use]
    pub const fn any() -> Self {
        Self {
            comparators: Vec::new(),
        }
    }

    /// `=version`, exactly this version (pre-release allowed when it is one).
    #[must_use]
    pub fn exact(version: &Version) -> Self {
        Self {
            comparators: vec![Comparator {
                op: Op::Exact,
                major: version.major,
                minor: Some(version.minor),
                patch: Some(version.patch),
                pre: version.pre.clone(),
            }],
        }
    }

    /// `^version`, the requirement `aevia add` writes for the newest version.
    #[must_use]
    pub fn caret(version: &Version) -> Self {
        Self {
            comparators: vec![Comparator {
                op: Op::Caret,
                major: version.major,
                minor: Some(version.minor),
                patch: Some(version.patch),
                pre: version.pre.clone(),
            }],
        }
    }

    /// Parses a requirement.
    ///
    /// # Errors
    ///
    /// [`ParseError`] naming the offending comparator.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let input = text;
        let text = text.trim();
        if text.is_empty() {
            return Err(ParseError::new(
                input,
                "a requirement is empty; write `*` to accept any version",
            ));
        }
        if text == "*" {
            return Ok(Self::any());
        }
        let mut comparators = Vec::new();
        for part in text.split(',') {
            let part = part.trim();
            if part.is_empty() {
                return Err(ParseError::new(input, "empty comparator between commas"));
            }
            comparators.push(parse_comparator(input, part).map_err(|mut error| {
                // `parse_comparator` names the comparator it was reading; when
                // the whole requirement is that one comparator, the prefix
                // only repeats the input.
                if part == text {
                    error.reason = error.reason.replace(&format!("`{part}`: "), "");
                }
                error
            })?);
        }
        Ok(Self { comparators })
    }

    /// Whether this is `*`.
    #[must_use]
    pub const fn is_any(&self) -> bool {
        self.comparators.is_empty()
    }

    /// The versions every comparator admits, before the pre-release rule.
    #[must_use]
    pub fn interval(&self) -> Interval {
        self.comparators
            .iter()
            .fold(Interval::full(), |acc, comparator| {
                acc.intersect(&comparator.interval())
            })
    }

    /// Whether `version` satisfies the requirement, pre-release rule included.
    #[must_use]
    pub fn matches(
        &self,
        version: &Version,
    ) -> bool {
        // Membership is conjunctive. Only callers needing the bounds have
        // to construct their intersection.
        self.permits_prerelease(version)
            && self
                .comparators
                .iter()
                .all(|comparator| comparator.interval().contains(version))
    }

    /// Applies the pre-release rule after the version interval has matched.
    pub(crate) fn permits_prerelease(
        &self,
        version: &Version,
    ) -> bool {
        !version.is_prerelease()
            || self.comparators.iter().any(|comparator| {
                !comparator.pre.is_empty()
                    && comparator.major == version.major
                    && comparator.minor == Some(version.minor)
                    && comparator.patch == Some(version.patch)
            })
    }

    /// Whether two requirements can be satisfied by one version (by interval;
    /// the pre-release rule can only make the answer stricter).
    #[must_use]
    pub fn overlaps(
        &self,
        other: &Self,
    ) -> bool {
        !self.interval().intersect(&other.interval()).is_empty()
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "comparators are the units of the requirement grammar"
)]
fn parse_comparator(
    input: &str,
    part: &str,
) -> Result<Comparator, ParseError> {
    let (op, rest) = split_op(part);
    let rest = rest.trim();
    if rest.is_empty() {
        return Err(ParseError::new(
            input,
            format!("`{part}` has an operator but no version"),
        ));
    }
    if rest.contains('+') {
        return Err(ParseError::new(
            input,
            format!("`{part}`: build metadata has no meaning in a requirement"),
        ));
    }
    let (numbers, pre) = match rest.split_once('-') {
        | Some((numbers, pre)) => (numbers, Some(pre)),
        | None => (rest, None),
    };
    let mut major = None;
    let mut minor = None;
    let mut patch = None;
    let mut wildcard = false;
    // Read one component beyond the maximum so excess fields have the
    // same diagnostic precedence without collecting the whole input.
    let mut components = numbers.split('.');
    let fields = [components.next(), components.next(), components.next()];
    if components.next().is_some() {
        return Err(ParseError::new(
            input,
            format!("`{part}` has more than three numbers"),
        ));
    }
    for (index, field) in fields.into_iter().flatten().enumerate() {
        let field = field.trim();
        if matches!(field, "*" | "x" | "X") {
            if index == 0 {
                if op.is_some() {
                    return Err(ParseError::new(
                        input,
                        format!("`{part}`: an operator needs at least a major number"),
                    ));
                }
                return Err(ParseError::new(
                    input,
                    format!("`{part}`: write `*` alone to accept any version"),
                ));
            }
            wildcard = true;
            continue;
        }
        if wildcard {
            return Err(ParseError::new(
                input,
                format!("`{part}`: a number cannot follow a wildcard"),
            ));
        }
        let number = parse_component(input, part, field)?;
        match index {
            | 0 => major = Some(number),
            | 1 => minor = Some(number),
            | _ => patch = Some(number),
        }
    }
    let Some(major) = major else {
        return Err(ParseError::new(
            input,
            format!("`{part}` has no major number"),
        ));
    };
    let pre = match pre {
        | None => Vec::new(),
        | Some(pre) => {
            if patch.is_none() {
                return Err(ParseError::new(
                    input,
                    format!("`{part}`: a pre-release needs a full `major.minor.patch`"),
                ));
            }
            parse_prerelease(input, pre).map_err(|mut error| {
                error.reason = format!("`{part}`: {}", error.reason);
                error
            })?
        },
    };
    let op = match (op, wildcard) {
        | (None | Some(Op::Exact), true) => Op::Wildcard,
        | (Some(op), _) => op,
        | (None, false) => Op::Caret,
    };
    Ok(Comparator {
        op,
        major,
        minor,
        patch,
        pre,
    })
}

#[allow(
    clippy::single_call_fn,
    reason = "operator recognition keeps the longest-first spelling table together"
)]
fn split_op(part: &str) -> (Option<Op>, &str) {
    for (prefix, op) in [
        (">=", Op::GreaterEq),
        ("<=", Op::LessEq),
        (">", Op::Greater),
        ("<", Op::Less),
        ("=", Op::Exact),
        ("^", Op::Caret),
        ("~", Op::Tilde),
    ] {
        if let Some(rest) = part.strip_prefix(prefix) {
            return (Some(op), rest);
        }
    }
    (None, part)
}

#[allow(
    clippy::single_call_fn,
    reason = "all numeric comparator components share one validation rule"
)]
fn parse_component(
    input: &str,
    part: &str,
    field: &str,
) -> Result<u64, ParseError> {
    if field.is_empty() || !field.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ParseError::new(
            input,
            format!("`{part}`: `{field}` is not a number"),
        ));
    }
    if field.len() > 1 && field.starts_with('0') {
        return Err(ParseError::new(
            input,
            format!("`{part}`: `{field}` has a leading zero"),
        ));
    }
    field
        .parse::<u64>()
        .map_err(|_| ParseError::new(input, format!("`{part}`: `{field}` is too large")))
}

impl fmt::Display for Comparator {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        let prefix = match self.op {
            | Op::Exact => "=",
            | Op::Greater => ">",
            | Op::GreaterEq => ">=",
            | Op::Less => "<",
            | Op::LessEq => "<=",
            | Op::Tilde => "~",
            | Op::Caret => "^",
            | Op::Wildcard => "",
        };
        write!(formatter, "{prefix}{}", self.major)?;
        match (self.minor, self.patch) {
            | (None, _) => {
                if self.op == Op::Wildcard {
                    formatter.write_str(".*")?;
                }
            },
            | (Some(minor), None) => {
                write!(formatter, ".{minor}")?;
                if self.op == Op::Wildcard {
                    formatter.write_str(".*")?;
                }
            },
            | (Some(minor), Some(patch)) => write!(formatter, ".{minor}.{patch}")?,
        }
        if !self.pre.is_empty() {
            formatter.write_str("-")?;
            for (index, identifier) in self.pre.iter().enumerate() {
                if index > 0 {
                    formatter.write_str(".")?;
                }
                write!(formatter, "{identifier}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for VersionReq {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        if self.comparators.is_empty() {
            return formatter.write_str("*");
        }
        for (index, comparator) in self.comparators.iter().enumerate() {
            if index > 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "{comparator}")?;
        }
        Ok(())
    }
}

impl FromStr for VersionReq {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, ParseError> {
        Self::parse(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap_or_else(|error| panic!("{error}"))
    }

    fn r(text: &str) -> VersionReq {
        VersionReq::parse(text).unwrap_or_else(|error| panic!("{error}"))
    }

    fn accepts(
        req: &str,
        yes: &[&str],
        no: &[&str],
    ) {
        let req = r(req);
        for version in yes {
            assert!(req.matches(&v(version)), "`{req}` should accept {version}");
        }
        for version in no {
            assert!(!req.matches(&v(version)), "`{req}` should refuse {version}");
        }
    }

    #[test]
    fn caret_follows_the_cargo_table() {
        accepts("^1.2.3", &["1.2.3", "1.9.9"], &["1.2.2", "2.0.0", "0.9.0"]);
        accepts("^1.2", &["1.2.0", "1.99.0"], &["1.1.9", "2.0.0"]);
        accepts("^1", &["1.0.0", "1.5.5"], &["0.9.9", "2.0.0"]);
        accepts("^0.2.3", &["0.2.3", "0.2.9"], &["0.2.2", "0.3.0", "1.0.0"]);
        accepts("^0.2", &["0.2.0", "0.2.7"], &["0.1.9", "0.3.0"]);
        accepts("^0.0.3", &["0.0.3"], &["0.0.2", "0.0.4", "0.1.0"]);
        accepts("^0.0", &["0.0.0", "0.0.9"], &["0.1.0"]);
        accepts("^0", &["0.0.1", "0.9.0"], &["1.0.0"]);
    }

    #[test]
    fn a_bare_version_is_a_caret() {
        assert_eq!(r("1.2.3"), r("^1.2.3"));
        accepts("1.2", &["1.2.0", "1.8.0"], &["2.0.0"]);
    }

    #[test]
    fn tilde_allows_patch_changes_only() {
        accepts("~1.2.3", &["1.2.3", "1.2.9"], &["1.3.0", "1.2.2"]);
        accepts("~1.2", &["1.2.0", "1.2.9"], &["1.3.0"]);
        accepts("~1", &["1.0.0", "1.9.9"], &["2.0.0"]);
    }

    #[test]
    fn wildcards_and_the_star() {
        accepts("1.*", &["1.0.0", "1.9.9"], &["2.0.0", "0.9.0"]);
        accepts("1.2.x", &["1.2.0", "1.2.9"], &["1.3.0"]);
        accepts("*", &["0.0.1", "7.7.7"], &["1.0.0-rc.1"]);
        assert!(r("*").is_any());
        assert_eq!(r("1.X"), r("1.*"));
    }

    #[test]
    fn comparators_with_partial_versions() {
        accepts(">1.2", &["1.3.0", "2.0.0"], &["1.2.9", "1.2.0"]);
        accepts(">=1.2", &["1.2.0", "3.0.0"], &["1.1.9"]);
        accepts("<1.2", &["1.1.9", "0.1.0"], &["1.2.0"]);
        accepts("<=1.2", &["1.2.9", "1.0.0"], &["1.3.0"]);
        accepts("=1.2.3", &["1.2.3"], &["1.2.4", "1.2.2"]);
        accepts("=1.2", &["1.2.0", "1.2.9"], &["1.3.0"]);
        accepts(">1.2.3", &["1.2.4"], &["1.2.3"]);
    }

    #[test]
    fn conjunctions_intersect() {
        accepts(">=1.2, <2", &["1.2.0", "1.9.9"], &["1.1.9", "2.0.0"]);
        accepts(">= 1.2 , < 1.5.0", &["1.4.9"], &["1.5.0"]);
        accepts("^1.2, <1.4", &["1.3.9"], &["1.4.0"]);
        assert!(!r(">=2, <1").interval().contains(&v("1.5.0")));
        assert!(r(">=2, <1").interval().is_empty());
        assert!(r(">1.0.0, <1.0.0").interval().is_empty());
        assert!(!r(">=1.0.0, <=1.0.0").interval().is_empty());
    }

    #[test]
    fn pre_releases_need_an_explicit_comparator_for_the_same_triple() {
        accepts(
            ">=1.0.0-rc.1",
            &["1.0.0-rc.1", "1.0.0-rc.2", "1.0.0", "1.2.0"],
            &["1.0.0-beta", "1.2.0-rc.1"],
        );
        accepts("^1.0.0-rc.1", &["1.0.0-rc.5", "1.3.0"], &["2.0.0-rc.1"]);
        accepts("=1.0.0-rc.1", &["1.0.0-rc.1"], &["1.0.0-rc.2", "1.0.0"]);
        accepts("^1", &["1.4.0"], &["1.0.0-rc.1", "2.0.0-alpha"]);
        accepts(">=1.0.0, <2.0.0", &["1.5.0"], &["2.0.0-rc.1", "1.5.0-rc.1"]);
    }

    #[test]
    fn displaying_a_requirement_parses_back_to_itself() {
        for text in [
            "*",
            "^1.2.3",
            "~1.2",
            "~1",
            ">=1.2, <2",
            ">1.2.3",
            "<=1.2",
            "=0.0.3",
            "1.*",
            "1.2.*",
            ">=1.0.0-rc.1",
            "^0.2",
        ] {
            let req = r(text);
            let shown = req.to_string();
            assert_eq!(r(&shown), req, "{text} -> {shown}");
        }
        assert_eq!(r("1.2.3").to_string(), "^1.2.3");
        assert_eq!(r(">=1.2,<2").to_string(), ">=1.2, <2");
    }

    #[test]
    fn malformed_requirements_are_refused_with_a_reason() {
        for (text, reason) in [
            ("", "empty"),
            (",", "empty comparator"),
            ("^", "no version"),
            ("^1.2.3.4", "more than three"),
            ("^1.a", "not a number"),
            (">=*", "at least a major"),
            ("1.*.3", "cannot follow a wildcard"),
            ("^1.2-beta", "full `major.minor.patch`"),
            ("^1.2.3+build", "build metadata"),
            ("^01.2.3", "leading zero"),
            ("x", "alone"),
            ("1.2.3 || 2", "not a number"),
        ] {
            let error = VersionReq::parse(text).expect_err(text);
            assert!(
                error.reason.contains(reason),
                "`{text}`: `{}` should mention `{reason}`",
                error.reason
            );
        }
    }

    #[test]
    fn excess_components_report_before_individual_field_errors() {
        let text = format!(">=1, ^01.{}.7", "two.".repeat(2048));
        let error = VersionReq::parse(&text).unwrap_err();
        assert_eq!(error.input, text);
        assert!(error.reason.ends_with("has more than three numbers"));
        for text in ["^01.two.3.4", "^.2.3.", "^*.1.2.3"] {
            assert_eq!(
                VersionReq::parse(text).unwrap_err().reason,
                format!("`{text}` has more than three numbers")
            );
        }
    }

    #[test]
    fn prerelease_identifiers_share_version_parsing_and_comparator_errors() {
        let text = "  >= 1 . 2 . 3-alpha.10.X-7 , <2  ";
        let parsed = r(text);
        assert_eq!(parsed.comparators[0].pre, v("1.2.3-alpha.10.X-7").pre);
        assert_eq!(parsed.to_string(), ">=1.2.3-alpha.10.X-7, <2");
        assert!(parsed.matches(&v("1.2.3-alpha.10.X-7+pkg.001")));
        for suffix in ["", "alpha..7", "alpha.007", "alpha_1", " alpha"] {
            let version = format!("1.2.3-{suffix}");
            let reason = Version::parse(&version).unwrap_err().reason;
            let single = format!(">={version}");
            let conjunction = format!("{single}, <2");
            assert_eq!(VersionReq::parse(&single).unwrap_err().reason, reason);
            assert_eq!(
                VersionReq::parse(&conjunction).unwrap_err().reason,
                format!("`{single}`: {reason}")
            );
        }
    }

    #[test]
    fn matches_agrees_with_the_interval_on_releases_for_random_requirements() {
        let mut rng = crate::testing::Rng::new(99);
        for _ in 0..300 {
            let req = rng.requirement();
            let interval = req.interval();
            for _ in 0..30 {
                let mut version = rng.version();
                version.pre.clear();
                version.build.clear();
                assert_eq!(
                    req.matches(&version),
                    interval.contains(&version),
                    "`{req}` vs {version}"
                );
            }
        }
    }

    #[test]
    fn matching_agrees_with_the_intersection_at_prerelease_and_overflow_boundaries() {
        let max = u64::MAX;
        let comparators = [
            ">=1.0.0-rc.1".to_owned(),
            "<1.0.0".to_owned(),
            "<=1.0.0-rc.2".to_owned(),
            ">1.0.0-rc.2".to_owned(),
            "=1.0.0-rc.1".to_owned(),
            "^1.0".to_owned(),
            "1.2.*".to_owned(),
            "~1.2.0-alpha".to_owned(),
            format!(">{max}.{max}"),
            format!("<{max}.{max}.{max}"),
            format!("^{max}.{max}.{max}-preview.0"),
            format!("^0.0.{max}"),
        ];
        let versions = [
            v("0.0.0-alpha"),
            v("0.0.0"),
            v("1.0.0-beta.2"),
            v("1.0.0-rc.1+rebuild.9"),
            v("1.0.0-rc.2"),
            v("1.0.0-rc.10"),
            v("1.0.0+rebuild.9"),
            v("1.1.1-rc.1"),
            v("1.2.0-alpha"),
            v("1.2.0"),
            v("2.0.0"),
            v(&format!("0.0.{max}-preview.1")),
            v(&format!("0.0.{max}")),
            v(&format!("{max}.{max}.{max}-preview.1")),
            v(&format!("{max}.{max}.{max}+rebuild.1")),
        ];
        for first in &comparators {
            for second in &comparators {
                let req = r(&format!("{first}, {second}"));
                let interval = req.interval();
                for version in &versions {
                    assert_eq!(
                        req.matches(version),
                        interval.contains(version) && req.permits_prerelease(version),
                        "`{req}` vs {version}"
                    );
                }
            }
        }
    }

    #[test]
    fn overlap_is_symmetric_and_agrees_with_a_witness_search() {
        let mut rng = crate::testing::Rng::new(5);
        for _ in 0..200 {
            let a = rng.requirement();
            let b = rng.requirement();
            assert_eq!(a.overlaps(&b), b.overlaps(&a), "`{a}` / `{b}`");
            // If a witness exists among small releases, the intervals overlap.
            let witness = (0..4u64)
                .flat_map(|x| (0..4u64).flat_map(move |y| (0..4u64).map(move |z| (x, y, z))))
                .map(|(x, y, z)| Version::new(x, y, z))
                .any(|version| a.matches(&version) && b.matches(&version));
            if witness {
                assert!(a.overlaps(&b), "`{a}` / `{b}` share a version");
            }
        }
    }

    #[test]
    fn extreme_component_bounds_carry_or_remain_unbounded() {
        let max = u64::MAX;
        assert!(r(&format!("^{max}")).matches(&v(&format!("{max}.{max}.{max}"))));
        assert!(r(&format!("~1.{max}")).matches(&v(&format!("1.{max}.9"))));
        assert!(!r(&format!(">1.{max}")).matches(&v(&format!("1.{max}.9"))));
        assert!(r(&format!(">1.{max}")).matches(&v("2.0.0")));
        assert!(r(&format!(">{max}.{max}")).interval().is_empty());
        assert!(r(&format!(">{max}.{max}.{max}")).interval().is_empty());
        assert!(r(&format!("^0.0.{max}")).matches(&v(&format!("0.0.{max}"))));
        assert!(!r(&format!("^0.0.{max}")).matches(&v("0.1.0")));
    }

    #[test]
    fn requirements_compare_precedence_without_build_metadata() {
        let built = v("1.2.3+packaged.1");
        assert!(r("=1.2.3").matches(&built));
        assert!(r("<=1.2.3").matches(&built));
        assert!(!r(">1.2.3").matches(&built));
        assert!(r("=1.2.3-rc.1").matches(&v("1.2.3-rc.1+packaged.1")));
    }

    #[test]
    fn constructors_write_what_add_writes() {
        assert_eq!(VersionReq::caret(&v("1.2.3")).to_string(), "^1.2.3");
        assert_eq!(VersionReq::exact(&v("1.2.3")).to_string(), "=1.2.3");
        assert_eq!(
            VersionReq::exact(&v("1.2.3-rc.1")).to_string(),
            "=1.2.3-rc.1"
        );
        assert!(VersionReq::exact(&v("1.2.3-rc.1")).matches(&v("1.2.3-rc.1")));
        assert_eq!(VersionReq::any().to_string(), "*");
    }
}
