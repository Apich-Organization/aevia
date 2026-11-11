//! Semantic versions (SemVer 2.0.0) with a total order.
//!
//! # The order
//!
//! Precedence is the specification's: `major`, `minor`, `patch`, then the
//! pre-release (a version *with* a pre-release sorts before the same version
//! without one; identifiers compare numerically when both are numeric,
//! lexically otherwise, numeric before alphanumeric, a shorter list before a
//! longer one it prefixes). The specification says build metadata does not
//! affect precedence, but a total order must agree with `Eq`, and `1.0.0+a` and
//! `1.0.0+b` are different values, so build metadata is compared as the final
//! tie-break with the same identifier rules. Nothing in the resolver depends on
//! that tie-break: the registry refuses to publish two versions that differ
//! only in build metadata (see [`Version::precedence_key`]).
//!
//! # What is refused
//!
//! The parser is strict where lenient parsers cause trouble later: no leading
//! zeros in numeric fields, no `v` prefix, no missing minor/patch, no empty
//! identifiers, ASCII alphanumerics and `-` only.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// One dot-separated identifier of a pre-release or of build metadata.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Identifier {
    /// All digits, no leading zero (except `0` itself).
    Numeric(u64),
    /// Anything else: ASCII alphanumerics and `-`.
    Alpha(String),
}

impl Ord for Identifier {
    fn cmp(
        &self,
        other: &Self,
    ) -> Ordering {
        match (self, other) {
            | (Self::Numeric(left), Self::Numeric(right)) => left.cmp(right),
            | (Self::Numeric(_), Self::Alpha(_)) => Ordering::Less,
            | (Self::Alpha(_), Self::Numeric(_)) => Ordering::Greater,
            | (Self::Alpha(left), Self::Alpha(right)) => left.cmp(right),
        }
    }
}

impl PartialOrd for Identifier {
    fn partial_cmp(
        &self,
        other: &Self,
    ) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Identifier {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            | Self::Numeric(number) => write!(formatter, "{number}"),
            | Self::Alpha(text) => formatter.write_str(text),
        }
    }
}

/// Why a version or requirement string was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// What was being parsed, as written.
    pub input: String,
    /// What is wrong with it.
    pub reason: String,
}

impl ParseError {
    pub(crate) fn new(
        input: &str,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            input: input.to_string(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        write!(formatter, "`{}`: {}", self.input, self.reason)
    }
}

impl std::error::Error for ParseError {}

/// A semantic version.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Version {
    /// Incompatible API changes.
    pub major: u64,
    /// Backwards-compatible additions.
    pub minor: u64,
    /// Backwards-compatible fixes.
    pub patch: u64,
    /// Pre-release identifiers, empty for a release.
    pub pre: Vec<Identifier>,
    /// Build metadata identifiers (ignored by the specification's precedence).
    pub build: Vec<Identifier>,
}

/// The compatibility series of a version: versions in one series are assumed
/// interchangeable, and the resolver activates at most one version per series.
///
/// `>= 1.0.0` -> the major (`1`); `0.y.z` with `y > 0` -> `0.y`; `0.0.z` ->
/// `0.0.z`. This is the Cargo rule, and it is also what `^` means.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Series {
    /// The major component.
    pub major: u64,
    /// Present when `major == 0`.
    pub minor: Option<u64>,
    /// Present when `major == 0 && minor == 0`.
    pub patch: Option<u64>,
}

impl fmt::Display for Series {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        write!(formatter, "{}", self.major)?;
        if let Some(minor) = self.minor {
            write!(formatter, ".{minor}")?;
        }
        if let Some(patch) = self.patch {
            write!(formatter, ".{patch}")?;
        }
        Ok(())
    }
}

impl Version {
    /// A release version.
    #[must_use]
    pub const fn new(
        major: u64,
        minor: u64,
        patch: u64,
    ) -> Self {
        Self {
            major,
            minor,
            patch,
            pre: Vec::new(),
            build: Vec::new(),
        }
    }

    /// Parses `major.minor.patch[-pre][+build]`.
    ///
    /// # Errors
    ///
    /// [`ParseError`] naming what is wrong.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let input = text;
        let text = text.trim();
        if text.is_empty() {
            return Err(ParseError::new(input, "a version is empty"));
        }
        let (rest, build) = match text.split_once('+') {
            | Some((rest, build)) => (rest, Some(build)),
            | None => (text, None),
        };
        let (core, pre) = match rest.split_once('-') {
            | Some((core, pre)) => (core, Some(pre)),
            | None => (rest, None),
        };
        let mut numbers = core.split('.');
        let mut next = |what: &str| -> Result<u64, ParseError> {
            let part = numbers.next().ok_or_else(|| {
                ParseError::new(
                    input,
                    format!("missing the {what} number; a version is `major.minor.patch`"),
                )
            })?;
            parse_number(input, part, what)
        };
        let major = next("major")?;
        let minor = next("minor")?;
        let patch = next("patch")?;
        if numbers.next().is_some() {
            return Err(ParseError::new(
                input,
                "more than three numbers; a version is `major.minor.patch`",
            ));
        }
        let pre = match pre {
            | Some(pre) => parse_identifiers(input, pre, "pre-release", true)?,
            | None => Vec::new(),
        };
        let build = match build {
            | Some(build) => parse_identifiers(input, build, "build metadata", false)?,
            | None => Vec::new(),
        };
        Ok(Self {
            major,
            minor,
            patch,
            pre,
            build,
        })
    }

    /// Whether this is a pre-release (`1.0.0-rc.1`).
    #[must_use]
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }

    /// The compatibility series (see [`Series`]).
    #[must_use]
    pub fn series(&self) -> Series {
        if self.major > 0 {
            Series {
                major: self.major,
                minor: None,
                patch: None,
            }
        } else if self.minor > 0 {
            Series {
                major: 0,
                minor: Some(self.minor),
                patch: None,
            }
        } else {
            Series {
                major: 0,
                minor: Some(0),
                patch: Some(self.patch),
            }
        }
    }

    /// The version without build metadata: two versions with equal keys have
    /// equal precedence, and a registry must treat them as the same release.
    #[must_use]
    pub fn precedence_key(&self) -> Self {
        Self {
            build: Vec::new(),
            ..self.clone()
        }
    }

    /// The three-number part as a tuple.
    #[must_use]
    pub fn triple(&self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }
}

fn parse_number(
    input: &str,
    part: &str,
    what: &str,
) -> Result<u64, ParseError> {
    if part.is_empty() {
        return Err(ParseError::new(
            input,
            format!("the {what} number is empty"),
        ));
    }
    if !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ParseError::new(
            input,
            format!("the {what} number `{part}` is not a number"),
        ));
    }
    if part.len() > 1 && part.starts_with('0') {
        return Err(ParseError::new(
            input,
            format!("the {what} number `{part}` has a leading zero"),
        ));
    }
    part.parse::<u64>()
        .map_err(|_| ParseError::new(input, format!("the {what} number `{part}` is too large")))
}

fn parse_identifiers(
    input: &str,
    text: &str,
    what: &str,
    leading_zero_is_error: bool,
) -> Result<Vec<Identifier>, ParseError> {
    if text.is_empty() {
        return Err(ParseError::new(input, format!("the {what} is empty")));
    }
    let mut out = Vec::new();
    for part in text.split('.') {
        if part.is_empty() {
            return Err(ParseError::new(
                input,
                format!("the {what} has an empty identifier"),
            ));
        }
        if !part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(ParseError::new(
                input,
                format!("the {what} identifier `{part}` may use only letters, digits and `-`"),
            ));
        }
        if part.bytes().all(|byte| byte.is_ascii_digit()) {
            let padded = part.len() > 1 && part.starts_with('0');
            if padded && leading_zero_is_error {
                return Err(ParseError::new(
                    input,
                    format!("the numeric {what} identifier `{part}` has a leading zero"),
                ));
            }
            match part.parse::<u64>() {
                | Ok(number) if !padded => out.push(Identifier::Numeric(number)),
                // A build identifier such as `007` (or one too large for a
                // `u64`) is legal and stays text, so it renders back exactly as
                // written.
                | _ => out.push(Identifier::Alpha(part.to_string())),
            }
        } else {
            out.push(Identifier::Alpha(part.to_string()));
        }
    }
    Ok(out)
}

fn compare_identifiers(
    left: &[Identifier],
    right: &[Identifier],
) -> Ordering {
    for (a, b) in left.iter().zip(right) {
        let order = a.cmp(b);
        if order != Ordering::Equal {
            return order;
        }
    }
    left.len().cmp(&right.len())
}

impl Ord for Version {
    fn cmp(
        &self,
        other: &Self,
    ) -> Ordering {
        self.triple()
            .cmp(&other.triple())
            .then_with(|| {
                match (self.pre.is_empty(), other.pre.is_empty()) {
                    | (true, true) => Ordering::Equal,
                    // A release is greater than any pre-release of it.
                    | (true, false) => Ordering::Greater,
                    | (false, true) => Ordering::Less,
                    | (false, false) => compare_identifiers(&self.pre, &other.pre),
                }
            })
            .then_with(|| compare_identifiers(&self.build, &other.build))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(
        &self,
        other: &Self,
    ) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            formatter.write_str("-")?;
            write_identifiers(formatter, &self.pre)?;
        }
        if !self.build.is_empty() {
            formatter.write_str("+")?;
            write_identifiers(formatter, &self.build)?;
        }
        Ok(())
    }
}

fn write_identifiers(
    formatter: &mut fmt::Formatter<'_>,
    identifiers: &[Identifier],
) -> fmt::Result {
    for (index, identifier) in identifiers.iter().enumerate() {
        if index > 0 {
            formatter.write_str(".")?;
        }
        write!(formatter, "{identifier}")?;
    }
    Ok(())
}

impl FromStr for Version {
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

    #[test]
    fn versions_round_trip_through_display() {
        for text in [
            "0.0.0",
            "1.2.3",
            "10.20.30",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-0.3.7",
            "1.0.0-x.7.z.92",
            "1.0.0-x-y-z.-",
            "1.0.0+20130313144700",
            "1.0.0-beta+exp.sha.5114f85",
            "1.0.0+007",
        ] {
            assert_eq!(v(text).to_string(), text, "{text}");
        }
    }

    #[test]
    fn the_specifications_example_chain_is_ordered() {
        // semver.org section 11: the canonical chain of precedence.
        let chain = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "2.0.0",
            "2.1.0",
            "2.1.1",
        ];
        for pair in chain.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn numeric_identifiers_compare_as_numbers_not_text() {
        assert!(v("1.0.0-2") < v("1.0.0-10"));
        assert!(v("1.0.0-9") < v("1.0.0-a"));
        assert!(v("1.0.0-1.2") > v("1.0.0-1"));
    }

    #[test]
    fn build_metadata_is_the_last_tie_break_and_agrees_with_eq() {
        assert_ne!(v("1.0.0+a"), v("1.0.0+b"));
        assert_ne!(v("1.0.0+a").cmp(&v("1.0.0+b")), Ordering::Equal);
        assert_eq!(v("1.0.0+a").cmp(&v("1.0.0+a")), Ordering::Equal);
        assert_eq!(v("1.0.0+a").precedence_key(), v("1.0.0+b").precedence_key());
        // Precedence still dominates build metadata.
        assert!(v("1.0.0+z") < v("1.0.1+a"));
    }

    #[test]
    fn malformed_versions_are_refused_with_a_reason() {
        for (text, reason) in [
            ("", "empty"),
            ("1", "missing the minor"),
            ("1.2", "missing the patch"),
            ("1.2.3.4", "more than three"),
            ("01.2.3", "leading zero"),
            ("1.02.3", "leading zero"),
            ("v1.2.3", "not a number"),
            ("1.2.x", "not a number"),
            ("1.2.3-", "empty"),
            ("1.2.3-a..b", "empty identifier"),
            ("1.2.3-01", "leading zero"),
            ("1.2.3-a_b", "letters, digits"),
            ("1.2.3+", "empty"),
            ("99999999999999999999.0.0", "too large"),
        ] {
            let error = Version::parse(text).expect_err(text);
            assert!(
                error.reason.contains(reason),
                "`{text}`: `{}` should mention `{reason}`",
                error.reason
            );
        }
    }

    #[test]
    fn series_follow_the_caret_rule() {
        assert_eq!(v("1.2.3").series(), v("1.9.0").series());
        assert_ne!(v("1.2.3").series(), v("2.0.0").series());
        assert_eq!(v("0.3.1").series(), v("0.3.9").series());
        assert_ne!(v("0.3.1").series(), v("0.4.0").series());
        assert_ne!(v("0.0.3").series(), v("0.0.4").series());
        assert_eq!(v("1.0.0-rc.1").series(), v("1.0.0").series());
        assert_eq!(v("0.0.3").series().to_string(), "0.0.3");
        assert_eq!(v("0.3.1").series().to_string(), "0.3");
        assert_eq!(v("4.3.1").series().to_string(), "4");
    }

    #[test]
    fn ordering_is_total_antisymmetric_and_transitive_on_a_random_sample() {
        let mut rng = crate::testing::Rng::new(0xA11CE);
        let pool: Vec<Version> = (0..40).map(|_| rng.version()).collect();
        for a in &pool {
            assert_eq!(a.cmp(a), Ordering::Equal);
            for b in &pool {
                assert_eq!(a.cmp(b), b.cmp(a).reverse(), "{a} vs {b}");
                assert_eq!(a == b, a.cmp(b) == Ordering::Equal, "{a} vs {b}");
                for c in &pool {
                    if a <= b && b <= c {
                        assert!(a <= c, "{a} <= {b} <= {c}");
                    }
                }
            }
        }
    }

    #[test]
    fn display_then_parse_is_the_identity_on_random_versions() {
        let mut rng = crate::testing::Rng::new(7);
        for _ in 0..500 {
            let version = rng.version();
            assert_eq!(v(&version.to_string()), version, "{version}");
        }
    }
}
