//! Package identity: `owner/name`.
//!
//! # Why the owner is part of the name
//!
//! crates.io hands out a single flat namespace, which turns a registry into a
//! land grab: the first person to type a word owns it forever, and the only
//! defence is a ten-minute wait and a support ticket. The Aevia registry scopes
//! every package to a **verified identity** — a GitHub login or an ORCID iD,
//! both proven through an OAuth flow — so `apich/units-extra` is unambiguously
//! the one published by the `apich` account. Squatting a scope requires
//! squatting the identity, and the identity is checked by GitHub or ORCID, not
//! by us.
//!
//! Two consequences worth stating: names are compared case-insensitively (the
//! same `aevia` package cannot exist as `Aevia` and `aevia`), and the owner
//! segment is normalised — GitHub logins are lower-case, ORCID check digits are
//! upper-case `X` — so a single identity has a single spelling.

use std::fmt;
use std::str::FromStr;

use crate::error::E_NAME;
use crate::error::Error;
use crate::error::Result;

/// A package's full identity, `owner/name`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageId {
    owner: String,
    name: String,
}

impl PackageId {
    /// Builds an identity from an owner and a name, validating both.
    ///
    /// # Errors
    ///
    /// `E_NAME` when either segment breaks the rules in [`valid_owner`] or
    /// [`valid_crate_name`].
    pub fn new(
        owner: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<Self> {
        let owner = normalize_owner(&owner.into());
        let name = name.into();
        if !valid_owner(&owner) {
            return Err(Error::new(
                E_NAME,
                format!(
                    "`{owner}` is not a publisher handle; it must be a GitHub login or an \
                     ORCID iD (`0000-0002-1825-0097`)"
                ),
            ));
        }
        if !valid_crate_name(&name) {
            return Err(Error::new(
                E_NAME,
                format!(
                    "`{name}` is not a package name; it must start with a letter and contain \
                     only ASCII letters, digits, `_` and `-` (at most 64 characters)"
                ),
            ));
        }
        Ok(Self { owner, name })
    }

    /// The owner segment: a GitHub login or an ORCID iD.
    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// The package name, as the author spells it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The name folded to lower case, for uniqueness.
    ///
    /// The registry stores this in a unique index so that `Aevia` and `aevia`
    /// are one package. The *display* name keeps the author's spelling.
    #[must_use]
    pub fn name_key(&self) -> String {
        self.name.to_ascii_lowercase()
    }

    /// `owner/name`, the canonical spelling.
    #[must_use]
    pub fn to_path(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    /// `owner/name` with the name folded, used for cache keys and index paths.
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}/{}", self.owner, self.name_key())
    }
}

impl fmt::Display for PackageId {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        write!(formatter, "{}/{}", self.owner, self.name)
    }
}

impl FromStr for PackageId {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        let Some((owner, name)) = text.split_once('/') else {
            return Err(Error::new(
                E_NAME,
                format!("`{text}` is not a package identity; write it as `owner/name`"),
            ));
        };
        if name.contains('/') {
            return Err(Error::new(
                E_NAME,
                format!("`{text}` has more than one `/`; an identity is `owner/name`"),
            ));
        }
        Self::new(owner, name)
    }
}

impl serde::Serialize for PackageId {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_path())
    }
}

impl<'de> serde::Deserialize<'de> for PackageId {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D
    ) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_str(&text).map_err(serde::de::Error::custom)
    }
}

/// Folds an owner to its canonical spelling.
///
/// An ORCID iD's check character is `X` in upper case; a GitHub login is
/// lower-case. Everything else is left as typed and rejected by
/// [`valid_owner`] if it is not one of those two shapes.
#[must_use]
pub fn normalize_owner(owner: &str) -> String {
    let trimmed = owner.trim();
    if looks_like_orcid(trimmed) {
        trimmed.to_ascii_uppercase()
    } else {
        trimmed.to_ascii_lowercase()
    }
}

/// Whether `owner` is a GitHub login or an ORCID iD.
#[must_use]
pub fn valid_owner(owner: &str) -> bool {
    valid_github_login(owner) || valid_orcid(owner)
}

/// `0000-0002-1825-0097`, with the check character `X` in either case.
#[must_use]
pub fn looks_like_orcid(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 19 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        match index {
            | 4 | 9 | 14 => {
                if *byte != b'-' {
                    return false;
                }
            },
            | 18 => {
                if !byte.is_ascii_digit() && *byte != b'X' && *byte != b'x' {
                    return false;
                }
            },
            | _ => {
                if !byte.is_ascii_digit() {
                    return false;
                }
            },
        }
    }
    true
}

/// Whether `text` is an ORCID iD whose check digit is correct.
///
/// The registry does not require the checksum — ORCID itself is the authority
/// on which iDs exist — but `aevia-pkg login` uses it to catch a typo before a
/// round trip.
#[must_use]
pub fn orcid_checksum_ok(text: &str) -> bool {
    if !looks_like_orcid(text) {
        return false;
    }
    // The first eighteen characters are the fifteen digits of the iD body and
    // the three dashes between them; the nineteenth is the check character,
    // which may be `X` for ten and so is not a digit.
    let mut total = 0u32;
    for character in text[..18].chars().filter(char::is_ascii_digit) {
        total = (total + character.to_digit(10).unwrap_or(0)) * 2;
    }
    let expected = (12 - total % 11) % 11;
    let actual = match text.chars().nth(18) {
        | Some('X' | 'x') => 10,
        | Some(character) => character.to_digit(10).unwrap_or(u32::MAX),
        | None => return false,
    };
    actual == expected
}

fn valid_orcid(text: &str) -> bool {
    looks_like_orcid(text)
}

fn valid_github_login(text: &str) -> bool {
    let length = text.len();
    if length == 0 || length > 39 {
        return false;
    }
    let bytes = text.as_bytes();
    if bytes[0] == b'-' || bytes[length - 1] == b'-' {
        return false;
    }
    let mut previous_dash = false;
    for byte in bytes {
        match byte {
            | b'-' => {
                if previous_dash {
                    return false;
                }
                previous_dash = true;
            },
            | b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => previous_dash = false,
            | _ => return false,
        }
    }
    true
}

/// Whether `name` is a package name.
///
/// The rule is the toolchain's own (`aevia_core::manifest`): a leading letter,
/// then letters, digits, `_` and `-`. The registry adds a length bound, because
/// a name becomes a path segment in the sparse index.
#[must_use]
pub fn valid_crate_name(name: &str) -> bool {
    let mut characters = name.chars();
    match characters.next() {
        | Some(first) if first.is_ascii_alphabetic() => {},
        | _ => return false,
    }
    if name.len() > 64 {
        return false;
    }
    characters
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_github_identity_round_trips() {
        let id: PackageId = "Apich/units-extra".parse().expect("valid");
        assert_eq!(id.owner(), "apich", "GitHub logins fold to lower case");
        assert_eq!(id.name(), "units-extra", "the display name keeps its case");
        assert_eq!(id.to_string(), "apich/units-extra");
        assert_eq!(id.key(), "apich/units-extra");
    }

    #[test]
    fn an_orcid_check_digit_folds_to_upper_case() {
        let id: PackageId = "0000-0002-1825-009x/linalg".parse().expect("valid");
        assert_eq!(id.owner(), "0000-0002-1825-009X");
    }

    #[test]
    fn orcid_checksums_are_checked() {
        // The example iD from ORCID's own documentation.
        assert!(orcid_checksum_ok("0000-0002-1825-0097"));
        assert!(!orcid_checksum_ok("0000-0002-1825-0098"));
        // The check character can be `X`, which stands for ten; this iD is
        // computed with the ISO 7064 11,2 rule the standard specifies.
        assert!(orcid_checksum_ok("0000-0002-1825-109X"));
        assert!(!orcid_checksum_ok("0000-0002-1825-1090"));
        assert!(!orcid_checksum_ok("0000-0002-1825-009"));
    }

    #[test]
    fn names_are_compared_without_case() {
        let a: PackageId = "apich/Units".parse().expect("valid");
        let b: PackageId = "apich/units".parse().expect("valid");
        assert_ne!(a, b);
        assert_eq!(a.key(), b.key(), "the unique key folds case");
    }

    #[test]
    fn bad_names_are_refused_with_a_reason() {
        for text in [
            "no-slash",
            "apich/",
            "/units",
            "apich/a/b",
            "apich/9lives",
            "apich/with space",
            "not a login/units",
            "apich/units!",
        ] {
            let error = text.parse::<PackageId>().expect_err(text);
            assert_eq!(error.code, E_NAME, "{text}");
            assert!(!error.message.is_empty());
        }
    }

    #[test]
    fn a_64_character_name_is_the_limit() {
        let longest = format!("a{}", "b".repeat(63));
        assert!(valid_crate_name(&longest));
        assert!(!valid_crate_name(&format!("{longest}c")));
    }

    #[test]
    fn github_logins_follow_githubs_rules() {
        assert!(valid_github_login("apich"));
        assert!(valid_github_login("apich-org"));
        assert!(!valid_github_login("-apich"));
        assert!(!valid_github_login("apich-"));
        assert!(!valid_github_login("apich--org"));
        assert!(!valid_github_login("apich_org"));
    }
}
