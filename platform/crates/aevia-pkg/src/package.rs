//! The package in the current directory: finding it, packing it, naming it.
//!
//! # Why the manifest is read through `aevia-registry-core`
//!
//! `aevia_core::manifest` owns the real parser, but that crate links the RSSN
//! kernel. `aevia-registry-core` re-implements the same subset — same keys,
//! same refusals — precisely so a registry client can read `aevia.toml` without
//! building a computer-algebra system. This module uses that reader, which
//! means `aevia-pkg` accepts exactly the manifests `aevia check` accepts: a
//! package that publishes here is a package that builds there.
//!
//! # Why the archive is built by `aevia-registry-core` too
//!
//! The checksum is what gets signed, and the server recomputes it from the
//! bytes it receives. If the client packed with different rules — different
//! ordering, a timestamp, a `target/` directory — the digest would differ from
//! what a re-pack produces and every signature would look stale. Sharing
//! [`aevia_registry_core::tarball::pack_dir`] makes "the same tree packs to the
//! same bytes" true by construction rather than by agreement.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use aevia_registry_core::PackageId;
use aevia_registry_core::manifest::PackageManifest;
use aevia_registry_core::tarball;

use crate::error::CliError;

/// A package directory and its parsed manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    /// The directory holding `aevia.toml`.
    pub root: PathBuf,
    /// The manifest, as the toolchain reads it.
    pub manifest: PackageManifest,
}

/// The nearest directory at or above `start` that holds an `aevia.toml`.
///
/// The search walks upwards because `aevia add` is run from anywhere inside a
/// package — `src/`, a submodule, a test fixture — and the answer should be the
/// same in all of them.
#[must_use]
pub fn find_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|directory| directory.join("aevia.toml").is_file())
        .map(Path::to_path_buf)
}

/// Finds and parses the package containing `start`.
///
/// # Errors
///
/// `E_MANIFEST` when there is no `aevia.toml` above `start`, or it does not
/// parse.
pub fn load_from(start: &Path) -> Result<Package, CliError> {
    let Some(root) = find_root(start) else {
        return Err(CliError::new(format!(
            "no `aevia.toml` in `{}` or any parent directory",
            start.display()
        ))
        .with_next_step(
            "run the command inside a package, or create one with `aevia init <name>`",
        ));
    };
    let manifest = PackageManifest::load(&root).map_err(CliError::from)?;
    Ok(Package { root, manifest })
}

/// Packs the package into the deterministic `.tar.gz` the registry stores.
///
/// # Errors
///
/// `E_TARBALL` when the tree cannot be packed — a missing entry file, an
/// archive over the size limit, an unreadable file.
pub fn pack(package: &Package) -> Result<Vec<u8>, CliError> {
    tarball::pack_dir(&package.root, &package.manifest).map_err(CliError::from)
}

/// The uncommitted changes in the package's git tree, when it has one.
///
/// `None` means "cannot tell, so do not object": `git` may not be installed, and
/// the directory may not be a repository at all. The publish path treats `None`
/// as clean because refusing to publish an unpacked tarball of a source release
/// would be worse than publishing one that happens to sit in a git checkout.
#[must_use]
pub fn dirty_files(root: &Path) -> Option<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain", "--", "."])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let files: Vec<String> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    if files.is_empty() {
        None
    } else {
        Some(files)
    }
}

/// Splits `owner/name[@version]` into its parts.
///
/// `@` rather than `/version` because an ORCID iD is full of dashes and a
/// GitHub login may not contain `@`, so the separator is unambiguous from the
/// right.
///
/// # Errors
///
/// `E_NAME` for an identity that does not parse, `E_VERSION` for a version that
/// is not `major.minor.patch`.
pub fn parse_target(spec: &str) -> Result<(PackageId, Option<String>), CliError> {
    let spec = spec.trim();
    let (identity, version) = match spec.rsplit_once('@') {
        | Some((identity, version)) => (identity, Some(version)),
        | None => (spec, None),
    };
    let id: PackageId = identity.parse().map_err(CliError::from)?;
    if let Some(version) = version {
        if !valid_version(version) {
            return Err(CliError::new(format!(
                "E_VERSION: `{version}` is not a `major.minor.patch` version"
            ))
            .with_next_step("write the version as `0.2.0`, as `aevia.toml` does"));
        }
        return Ok((id, Some(version.to_string())));
    }
    Ok((id, None))
}

/// Splits `owner/name[@requirement]` into the identity and the requirement.
///
/// The requirement is anything `aevia.toml` accepts after `version =`: `1.2`
/// (a caret), `^1.2`, `~1.2.3`, `>=1.2, <2`, `=1.2.3`, `*`.
///
/// # Errors
///
/// `E_NAME` for an identity that does not parse, `E_VERSION` for a
/// requirement that does not.
pub fn parse_requirement_target(
    spec: &str
) -> Result<(PackageId, Option<aevia_resolve::VersionReq>), CliError> {
    let spec = spec.trim();
    let (identity, requirement) = match spec.split_once('@') {
        | Some((identity, requirement)) => (identity, Some(requirement)),
        | None => (spec, None),
    };
    let id: PackageId = identity.parse().map_err(CliError::from)?;
    let req = match requirement {
        | Some(text) => {
            Some(aevia_resolve::VersionReq::parse(text).map_err(|error| {
                CliError::new(format!("E_VERSION: {error}")).with_next_step(
                    "write the requirement as in aevia.toml: `1.2`, `^1.2`, `~1.2.3`, `>=1.2, <2`",
                )
            })?)
        },
        | None => None,
    };
    Ok((id, req))
}

/// Whether `version` is the `major.minor.patch` shape the toolchain writes.
///
/// Pre-release and build suffixes are refused here for the same reason
/// [`aevia_registry_core::statement`] refuses them: `aevia.toml` cannot express
/// them, so a version with `-rc.1` could never be reproduced by the toolchain.
#[must_use]
pub fn valid_version(version: &str) -> bool {
    aevia_resolve::Version::parse(version).is_ok_and(|parsed| parsed.build.is_empty())
}

/// The `[dependencies]` key a package is imported under, `UnitsExtra`.
///
/// This is `aevia_core::manifest::import_root`, re-implemented because that
/// crate cannot be linked here (it pulls in the RSSN kernel). The rule is the
/// toolchain's: split on `_` and `-`, capitalise each segment, concatenate. It
/// is used only to print the line a publisher pastes into `aevia.toml`, so the
/// worst a divergence could do is print a key the toolchain then refuses.
#[must_use]
pub fn import_root(name: &str) -> String {
    let mut out = String::new();
    for part in name.split(['_', '-']).filter(|part| !part.is_empty()) {
        let mut characters = part.chars();
        if let Some(first) = characters.next() {
            out.extend(first.to_uppercase());
            out.push_str(characters.as_str());
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

    fn package_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aevia-pkg-package-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).expect("src");
        std::fs::write(
            dir.join("aevia.toml"),
            "[package]\nname = \"units-extra\"\nversion = \"0.2.0\"\n",
        )
        .expect("manifest");
        std::fs::write(dir.join("src/main.aev"), "fn main(): Unit = ()\n").expect("entry");
        dir
    }

    #[test]
    fn the_root_is_found_from_a_subdirectory() {
        let dir = package_dir();
        let found = find_root(&dir.join("src")).expect("found");
        assert_eq!(found, dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_manifest_says_so() {
        let dir = std::env::temp_dir().join(format!(
            "aevia-pkg-empty-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("dir");
        let error = load_from(&dir).expect_err("no manifest");
        assert!(error.message.contains("aevia.toml"), "{}", error.message);
        assert!(error.next_step.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_target_splits_on_the_last_at_sign() {
        let (id, version) = parse_target("apich/units-extra@0.2.0").expect("parses");
        assert_eq!(id.to_path(), "apich/units-extra");
        assert_eq!(version.as_deref(), Some("0.2.0"));

        let (id, version) = parse_target("0000-0002-1825-0097/linalg").expect("parses");
        assert_eq!(id.owner(), "0000-0002-1825-0097");
        assert!(version.is_none());
    }

    #[test]
    fn a_bad_target_is_refused_with_the_reason() {
        for spec in [
            "units-extra",
            "apich/units@1.0",
            "apich/9lives",
        ] {
            let error = parse_target(spec).expect_err(spec);
            assert!(!error.message.is_empty(), "{spec}");
        }
    }

    #[test]
    fn versions_are_three_numbers() {
        assert!(valid_version("0.2.0"));
        assert!(valid_version("10.20.30"));
        assert!(!valid_version("1.0"));
        assert!(!valid_version("1.0.0.0"));
        assert!(!valid_version("v1.0.0"));
        assert!(valid_version("1.0.0-rc.1"));
        assert!(!valid_version("1.0.0+build"));
    }

    #[test]
    fn a_requirement_target_splits_on_the_at_sign() {
        let (id, req) = parse_requirement_target("apich/units-extra@^1.2").expect("parses");
        assert_eq!(id.to_path(), "apich/units-extra");
        assert_eq!(req.expect("req").to_string(), "^1.2");
        let (_, req) = parse_requirement_target("apich/units-extra@>=1.2, <2").expect("parses");
        assert_eq!(req.expect("req").to_string(), ">=1.2, <2");
        let (_, req) = parse_requirement_target("apich/units-extra").expect("parses");
        assert!(req.is_none());
        assert!(parse_requirement_target("apich/units@^x").is_err());
        assert!(parse_requirement_target("units@1").is_err());
    }

    #[test]
    fn the_import_root_matches_the_toolchains_spelling() {
        assert_eq!(import_root("units-extra"), "UnitsExtra");
        assert_eq!(import_root("units_extra"), "UnitsExtra");
        assert_eq!(import_root("linalg"), "Linalg");
        assert_eq!(import_root("a_b-c"), "ABC");
    }
}
