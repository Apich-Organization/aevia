//! Package identities and the names derived from them.
//!
//! The toolchain (`aevia.toml`), the lock file and the registry all need to
//! agree on what `Units = "^1"` refers to and on how `apich/Units-Extra` is
//! spelled; this module is the one place that decides, so the registry cannot
//! accept a spelling the toolchain reads differently.

/// The scope a bare version string (`Units = "^1.2"`) names packages in.
///
/// A bare string has no owner, and a package's identity is `owner/name`; the
/// registry's reserved scope for the standard distribution fills it in.
/// Anything outside that scope writes `package = "owner/name"`.
pub const OFFICIAL_SCOPE: &str = "aevia";

/// `UnitsExtra` -> `aevia/units_extra`: the package a bare version string
/// names for the dependency key `key`.
///
/// Each upper-case letter after the first starts a new `_`-separated word, so
/// an acronym key (`FFT`) maps to `f_f_t`; a package with such a name is
/// written with an explicit `package = "…"`.
#[must_use]
pub fn official_package(key: &str) -> String {
    let mut name = String::new();
    for (position, character) in key.chars().enumerate() {
        if character.is_uppercase() {
            if position > 0 {
                name.push('_');
            }
            name.extend(character.to_lowercase());
        } else {
            name.push(character);
        }
    }
    format!("{OFFICIAL_SCOPE}/{name}")
}

/// The canonical spelling of a registry identity, or `None` when it is not one.
///
/// `owner/name`: the owner is a GitHub login or an ORCID iD, the name starts
/// with a letter and uses letters, digits, `_` and `-` (at most 64). Names are
/// compared case-insensitively, so the canonical form lower-cases them; an
/// ORCID iD keeps its upper-case check character. This is the key the
/// resolver, the lock and the index agree on.
#[must_use]
pub fn canonical_package(id: &str) -> Option<String> {
    let (owner, name) = id.trim().split_once('/')?;
    if owner.is_empty() || name.is_empty() || name.contains('/') || name.len() > 64 {
        return None;
    }
    if !owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        || owner.starts_with('-')
        || owner.ends_with('-')
    {
        return None;
    }
    let mut chars = name.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return None;
    }
    let orcid = owner.len() == 19
        && owner.char_indices().all(|(i, c)| {
            if matches!(i, 4 | 9 | 14) {
                c == '-'
            } else {
                c.is_ascii_digit() || (i == 18 && matches!(c, 'x' | 'X'))
            }
        });
    let owner = if orcid {
        owner.to_ascii_uppercase()
    } else {
        owner.to_ascii_lowercase()
    };
    Some(format!("{owner}/{}", name.to_ascii_lowercase()))
}

/// The import root a package name maps to: `units-extra` -> `UnitsExtra`.
///
/// Takes `owner/name` or a bare name. This is the mapping the toolchain uses
/// for the dependency key `aevia add` writes, and the one the registry applies
/// to decide whether an index dependency needs an explicit `import`.
#[must_use]
pub fn default_import(package: &str) -> String {
    let name = package.rsplit('/').next().unwrap_or(package);
    let mut out = String::new();
    for part in name.split(['_', '-']).filter(|part| !part.is_empty()) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() {
        name.to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_names_follow_the_key() {
        assert_eq!(official_package("UnitsExtra"), "aevia/units_extra");
        assert_eq!(official_package("Linalg"), "aevia/linalg");
        assert_eq!(official_package("Base64"), "aevia/base64");
    }

    #[test]
    fn identities_are_canonicalised_or_refused() {
        assert_eq!(
            canonical_package("ApIch/Units-Extra").as_deref(),
            Some("apich/units-extra")
        );
        assert_eq!(
            canonical_package("0000-0002-1825-009x/lin").as_deref(),
            Some("0000-0002-1825-009X/lin")
        );
        for bad in [
            "a", "/a", "a/", "a/b/c", "a/9b", "-a/b", "a-/b", "a_/b", "a/b c", "",
        ] {
            assert_eq!(canonical_package(bad), None, "{bad}");
        }
        assert_eq!(canonical_package(&format!("o/{}", "a".repeat(65))), None);
    }

    #[test]
    fn default_import_matches_the_toolchains_spelling() {
        assert_eq!(default_import("apich/units-extra"), "UnitsExtra");
        assert_eq!(default_import("units_extra"), "UnitsExtra");
        assert_eq!(default_import("o/a_b-c"), "ABC");
        assert_eq!(default_import("o/linalg"), "Linalg");
    }
}
