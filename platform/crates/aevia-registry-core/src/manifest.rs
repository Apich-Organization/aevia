//! `aevia.toml`, read with the toolchain's own rules.
//!
//! # Why this is a re-implementation
//!
//! `aevia_core::manifest` owns the real parser, but `aevia-core` links the RSSN
//! kernel: a registry that depended on it would build three million lines of
//! symbolic algebra to read four keys. So the subset is duplicated here — and
//! deliberately duplicated *exactly*, including its refusals. The toolchain
//! rejects an unknown key in `[package]` with `E0605`; this reader rejects it
//! too, which is what makes "the registry accepted it" mean "`aevia check`
//! accepts it". A registry that quietly tolerates metadata the toolchain
//! refuses would let an author publish a package nobody can build.
//!
//! The consequence for authors: `description`, `license` and `keywords` do
//! **not** go in `aevia.toml`. They are passed on the command line, read from
//! `README.md`/`LICENSE`, or recorded in the registry's own
//! `aevia-pkg.toml` (see [`RegistryMeta`]), which the toolchain never reads.

use std::fmt::Write as _;
use std::path::Path;
use std::path::PathBuf;

use crate::error::E_MANIFEST;
use crate::error::Error;
use crate::error::Result;
use crate::name::valid_crate_name;

/// A dependency as the manifest declares it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ManifestDependency {
    /// The module root it is imported under, e.g. `UnitsExtra`.
    pub import: String,
    /// The path as written, relative to the manifest's directory.
    pub path: String,
}

/// The parts of `aevia.toml` the registry cares about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageManifest {
    /// `[package] name`.
    pub name: String,
    /// `[package] version`, always `major.minor.patch`.
    pub version: String,
    /// `[package] entry`, relative to the package root.
    pub entry: String,
    /// `[dependencies]`, sorted by import root.
    pub dependencies: Vec<ManifestDependency>,
}

impl PackageManifest {
    /// Parses a manifest.
    ///
    /// # Errors
    ///
    /// `E_MANIFEST` with the file and line, exactly where the toolchain would
    /// report `E0605`.
    pub fn parse(
        text: &str,
        origin: &str,
    ) -> Result<Self> {
        let mut table = String::new();
        let mut name = None;
        let mut version = None;
        let mut entry = None;
        let mut dependencies = Vec::new();
        for (index, raw) in text.lines().enumerate() {
            let number = index + 1;
            let line = raw.trim_end_matches('\r').trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(header) = line.strip_prefix('[') {
                let Some(header) = header.strip_suffix(']') else {
                    return Err(fail(
                        origin,
                        number,
                        format!("unterminated table header `{line}`"),
                    ));
                };
                match header.trim() {
                    | "package" | "dependencies" => table = header.trim().to_string(),
                    | other => {
                        return Err(fail(
                            origin,
                            number,
                            format!(
                                "unknown table `[{other}]`; the toolchain reads `[package]` and \
                                 `[dependencies]` only, so registry metadata goes in \
                                 `aevia-pkg.toml`"
                            ),
                        ));
                    },
                }
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(fail(
                    origin,
                    number,
                    format!("expected `key = value`, found `{line}`"),
                ));
            };
            let key = key.trim();
            let value = value.trim();
            match table.as_str() {
                | "package" => {
                    let text = parse_string(value).ok_or_else(|| {
                        fail(origin, number, format!("`{key}` expects a quoted string"))
                    })?;
                    match key {
                        | "name" => name = Some(text),
                        | "version" => version = Some(text),
                        | "entry" => entry = Some(text),
                        | other => {
                            return Err(fail(
                                origin,
                                number,
                                format!(
                                    "unknown key `{other}` in `[package]`; the toolchain reads \
                                     `name`, `version` and `entry` — put the description in \
                                     `aevia-pkg.toml`"
                                ),
                            ));
                        },
                    }
                },
                | "dependencies" => {
                    let path = parse_dependency(value).ok_or_else(|| {
                        fail(
                            origin,
                            number,
                            format!("`{key}` expects `{{ path = \"…\" }}`"),
                        )
                    })?;
                    if !is_module_segment(key) {
                        return Err(fail(
                            origin,
                            number,
                            format!(
                                "`{key}` is not a module name; a dependency's key is the root \
                                 module it is imported under, so it must start with an upper-case \
                                 letter and contain no `-`"
                            ),
                        ));
                    }
                    dependencies.push(ManifestDependency {
                        import: key.to_string(),
                        path,
                    });
                },
                | _ => {
                    return Err(fail(
                        origin,
                        number,
                        format!("`{line}` appears before any table header"),
                    ));
                },
            }
        }
        let name = name.ok_or_else(|| {
            Error::new(E_MANIFEST, format!("{origin}: `[package] name` is missing"))
        })?;
        if !valid_crate_name(&name) {
            return Err(Error::new(
                E_MANIFEST,
                format!("{origin}: `{name}` is not a package name; use letters, digits and `_`"),
            ));
        }
        let version = version.ok_or_else(|| {
            Error::new(
                E_MANIFEST,
                format!("{origin}: `[package] version` is missing"),
            )
        })?;
        if !is_version(&version) {
            return Err(Error::new(
                E_MANIFEST,
                format!("{origin}: `{version}` is not a `major.minor.patch` version"),
            ));
        }
        dependencies.sort_by(|left, right| left.import.cmp(&right.import));
        Ok(Self {
            name,
            version,
            entry: entry.unwrap_or_else(|| "src/main.aev".to_string()),
            dependencies,
        })
    }

    /// Reads `dir/aevia.toml`.
    ///
    /// # Errors
    ///
    /// `E_MANIFEST` when the file is missing or does not parse.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join("aevia.toml");
        let text = std::fs::read_to_string(&path)
            .map_err(|error| Error::io(format!("cannot read `{}`", path.display()), &error))?;
        Self::parse(&text, &path.display().to_string())
    }

    /// The entry file's path inside the package.
    #[must_use]
    pub fn entry_path(
        &self,
        dir: &Path,
    ) -> PathBuf {
        dir.join(&self.entry)
    }
}

/// Metadata the registry stores but `aevia.toml` cannot carry.
///
/// It lives in an optional `aevia-pkg.toml` at the package root. The toolchain
/// never reads that file, so adding it cannot break `aevia check`.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RegistryMeta {
    /// One sentence describing the package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// An SPDX identifier, `Apache-2.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Up to five lower-case keywords.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// Where the source lives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// The file shown on the package page, relative to the package root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readme: Option<String>,
}

impl RegistryMeta {
    /// Reads `dir/aevia-pkg.toml` when it exists.
    ///
    /// # Errors
    ///
    /// `E_MANIFEST` when the file exists but is not a flat `key = value` table.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join("aevia-pkg.toml");
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|error| Error::io(format!("cannot read `{}`", path.display()), &error))?;
        Self::parse(&text, &path.display().to_string())
    }

    /// Parses the flat table this file is allowed to be.
    ///
    /// # Errors
    ///
    /// `E_MANIFEST` on any line outside `key = "value"`, `key = ["a", "b"]`.
    pub fn parse(
        text: &str,
        origin: &str,
    ) -> Result<Self> {
        let mut meta = Self::default();
        for (index, raw) in text.lines().enumerate() {
            let number = index + 1;
            let line = raw.trim_end_matches('\r').trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(fail(
                    origin,
                    number,
                    format!("expected `key = value`, found `{line}`"),
                ));
            };
            let key = key.trim();
            let value = value.trim();
            match key {
                | "keywords" => {
                    meta.keywords = parse_array(value).ok_or_else(|| {
                        fail(
                            origin,
                            number,
                            "`keywords` expects an array of quoted strings",
                        )
                    })?;
                },
                | "description" | "license" | "repository" | "readme" => {
                    let text = parse_string(value).ok_or_else(|| {
                        fail(origin, number, format!("`{key}` expects a quoted string"))
                    })?;
                    match key {
                        | "description" => meta.description = Some(text),
                        | "license" => meta.license = Some(text),
                        | "repository" => meta.repository = Some(text),
                        | _ => meta.readme = Some(text),
                    }
                },
                | other => {
                    return Err(fail(
                        origin,
                        number,
                        format!(
                            "unknown key `{other}`; `aevia-pkg.toml` reads `description`, \
                             `license`, `keywords`, `repository` and `readme`"
                        ),
                    ));
                },
            }
        }
        meta.keywords.truncate(5);
        for keyword in &mut meta.keywords {
            *keyword = keyword.trim().to_ascii_lowercase();
        }
        meta.keywords.retain(|keyword| !keyword.is_empty());
        Ok(meta)
    }
}

/// Everything the registry records about a package, discovered from its tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackageMetadata {
    /// The `aevia-pkg.toml` table, when the author wrote one.
    pub declared: RegistryMeta,
    /// The description to show: declared, else the first paragraph of README.
    pub description: Option<String>,
    /// The license: declared, else detected from `LICENSE`.
    pub license: Option<String>,
    /// The README's markdown, as stored.
    pub readme: Option<String>,
    /// The repository link, declared or read from `.git/config`.
    pub repository: Option<String>,
}

impl PackageMetadata {
    /// Discovers metadata from a package directory.
    ///
    /// Nothing here fails: a package without a README or a LICENSE is a
    /// package with less metadata, not an invalid one.
    #[must_use]
    pub fn discover(dir: &Path) -> Self {
        let declared = RegistryMeta::load(dir).unwrap_or_default();
        let readme_path = declared
            .readme
            .clone()
            .map(|name| dir.join(name))
            .filter(|path| path.is_file())
            .or_else(|| find_file(dir, &["README.md", "README", "readme.md"]));
        let readme = readme_path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok());
        let description = declared
            .description
            .clone()
            .or_else(|| readme.as_deref().and_then(first_paragraph));
        let license = declared
            .license
            .clone()
            .or_else(|| detect_license(dir))
            .map(|license| license.trim().to_string());
        let repository = declared.repository.clone().or_else(|| git_remote(dir));
        Self {
            declared,
            description,
            license,
            readme,
            repository,
        }
    }
}

/// The first paragraph of a README that reads as a description.
///
/// Headings, badges, block quotes and fenced code are skipped: a description
/// that begins with `# aevia` or `[![CI](…)]` tells a reader nothing.
#[must_use]
pub fn first_paragraph(readme: &str) -> Option<String> {
    let mut collected: Vec<&str> = Vec::new();
    let mut fenced = false;
    for line in readme.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if trimmed.is_empty() {
            if collected.is_empty() {
                continue;
            }
            break;
        }
        if trimmed.starts_with('#') || trimmed.starts_with("![") || trimmed.starts_with("[![") {
            continue;
        }
        if trimmed.starts_with('>') || trimmed.starts_with('<') || trimmed.starts_with("---") {
            continue;
        }
        collected.push(trimmed);
    }
    if collected.is_empty() {
        return None;
    }
    let mut text = collected.join(" ");
    for mark in ["**", "__", "`", "*"] {
        text = text.replace(mark, "");
    }
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(truncate_words(text, 200))
}

/// Cuts `text` to at most `limit` characters on a word boundary.
#[must_use]
pub fn truncate_words(
    text: &str,
    limit: usize,
) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out = String::new();
    for word in text.split_whitespace() {
        if out.chars().count() + word.chars().count() + 1 > limit {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    if out.is_empty() {
        text.chars().take(limit).collect()
    } else {
        out.push('…');
        out
    }
}

fn find_file(
    dir: &Path,
    names: &[&str],
) -> Option<PathBuf> {
    for name in names {
        let path = dir.join(name);
        if path.is_file() {
            return Some(path);
        }
    }
    // A case-insensitive fallback, because Windows and macOS checkouts disagree
    // with Linux about the spelling of `LICENSE`.
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if names
            .iter()
            .any(|candidate| candidate.to_ascii_lowercase() == name)
        {
            return Some(entry.path());
        }
    }
    None
}

/// Detects the license from the repository's licence files.
///
/// The registry never guesses on the author's behalf — a wrong licence is worse
/// than an empty one — so only unambiguous phrases are matched.
#[must_use]
pub fn detect_license(dir: &Path) -> Option<String> {
    let path = find_file(
        dir,
        &["LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING", "LICENCE"],
    )?;
    let text = std::fs::read_to_string(path).ok()?;
    if let Some(identifier) = spdx_identifier(&text) {
        return Some(identifier);
    }
    let lower = text.to_ascii_lowercase();
    let found = if lower.contains("apache license") && lower.contains("version 2.0") {
        "Apache-2.0"
    } else if lower.contains("gnu affero general public license") {
        // Every published version of the AGPL is 3.0; the licence text does not
        // distinguish, so neither does this.
        "AGPL-3.0-only"
    } else if lower.contains("gnu lesser general public license") {
        "LGPL-3.0-only"
    } else if lower.contains("gnu general public license") {
        if lower.contains("version 2") {
            "GPL-2.0-only"
        } else {
            "GPL-3.0-only"
        }
    } else if lower.contains("mozilla public license") {
        "MPL-2.0"
    } else if lower.contains("permission is hereby granted, free of charge") {
        "MIT"
    } else if lower.contains("redistribution and use in source and binary forms")
        && lower.contains("neither the name")
    {
        "BSD-3-Clause"
    } else if lower.contains("redistribution and use in source and binary forms") {
        "BSD-2-Clause"
    } else if lower
        .contains("this is free and unencumbered software released into the public domain")
    {
        "Unlicense"
    } else if lower.contains("creative commons") && lower.contains("attribution 4.0") {
        "CC-BY-4.0"
    } else {
        return None;
    };
    Some(found.to_string())
}

fn spdx_identifier(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim().trim_start_matches(['/', '*', '#', ' ']);
        if let Some(rest) = trimmed.strip_prefix("SPDX-License-Identifier:") {
            let identifier = rest.trim().trim_end_matches("*/").trim();
            if !identifier.is_empty() && identifier.len() < 64 {
                return Some(identifier.to_string());
            }
        }
    }
    None
}

/// Reads `remote.origin.url` from `.git/config` and turns it into a web link.
#[must_use]
pub fn git_remote(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(".git/config")).ok()?;
    let mut in_origin = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_origin = trimmed == "[remote \"origin\"]";
            continue;
        }
        if !in_origin {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("url") {
            let url = rest.trim_start_matches([' ', '=']).trim();
            return Some(normalize_remote(url));
        }
    }
    None
}

fn normalize_remote(url: &str) -> String {
    let url = url.trim();
    // `git@github.com:owner/repo.git` → `https://github.com/owner/repo`
    if let Some(rest) = url.strip_prefix("git@")
        && let Some((host, path)) = rest.split_once(':')
    {
        let path = path.strip_suffix(".git").unwrap_or(path);
        return format!("https://{host}/{path}");
    }
    if let Some(rest) = url.strip_prefix("ssh://git@") {
        let rest = rest.strip_suffix(".git").unwrap_or(rest);
        return format!("https://{rest}");
    }
    url.strip_suffix(".git").unwrap_or(url).to_string()
}

fn fail(
    origin: &str,
    number: usize,
    message: impl std::fmt::Display,
) -> Error {
    Error::new(E_MANIFEST, format!("{origin}:{number}: {message}"))
}

fn parse_string(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
        return None;
    }
    let inner = &value[1..value.len() - 1];
    if inner.contains('"') {
        return None;
    }
    Some(inner.to_string())
}

fn parse_array(value: &str) -> Option<Vec<String>> {
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    let mut items = Vec::new();
    for item in inner.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        items.push(parse_string(item)?);
    }
    Some(items)
}

fn parse_dependency(value: &str) -> Option<String> {
    let inner = value.strip_prefix('{')?.strip_suffix('}')?;
    let mut path = None;
    for field in inner.split(',') {
        let field = field.trim();
        if field.is_empty() {
            continue;
        }
        let (key, value) = field.split_once('=')?;
        if key.trim() != "path" {
            return None;
        }
        path = Some(parse_string(value.trim())?);
    }
    path
}

fn is_module_segment(text: &str) -> bool {
    let mut characters = text.chars();
    match characters.next() {
        | Some(first) if first.is_ascii_uppercase() => {},
        | _ => return false,
    }
    characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn is_version(version: &str) -> bool {
    let mut parts = version.split('.');
    let mut count = 0;
    for part in &mut parts {
        if part.is_empty() || !part.chars().all(|character| character.is_ascii_digit()) {
            return false;
        }
        count += 1;
    }
    count == 3
}

/// Renders a manifest the way the toolchain writes it.
///
/// `aevia-pkg` uses this only for its own generated fixtures; publishing never
/// rewrites the author's file.
#[must_use]
pub fn render(
    name: &str,
    version: &str,
    entry: &str,
    dependencies: &[ManifestDependency],
) -> String {
    let mut out = String::new();
    out.push_str("[package]\n");
    let _ = writeln!(out, "name = \"{name}\"");
    let _ = writeln!(out, "version = \"{version}\"");
    let _ = writeln!(out, "entry = \"{entry}\"");
    if !dependencies.is_empty() {
        out.push_str("\n[dependencies]\n");
        for dependency in dependencies {
            let _ = writeln!(
                out,
                "{} = {{ path = \"{}\" }}",
                dependency.import, dependency.path
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "# a package\n[package]\nname = \"units-extra\"\nversion = \"0.2.0\"\nentry = \"src/main.aev\"\n\n[dependencies]\nUnitsExtra = { path = \"../units\" }\n";

    #[test]
    fn a_manifest_parses() {
        let manifest = PackageManifest::parse(MANIFEST, "aevia.toml").expect("parses");
        assert_eq!(manifest.name, "units-extra");
        assert_eq!(manifest.version, "0.2.0");
        assert_eq!(manifest.entry, "src/main.aev");
        assert_eq!(manifest.dependencies.len(), 1);
        assert_eq!(manifest.dependencies[0].import, "UnitsExtra");
        assert_eq!(manifest.dependencies[0].path, "../units");
    }

    #[test]
    fn the_entry_defaults_like_the_toolchain() {
        let manifest =
            PackageManifest::parse("[package]\nname = \"a\"\nversion = \"1.0.0\"\n", "x")
                .expect("parses");
        assert_eq!(manifest.entry, "src/main.aev");
    }

    #[test]
    fn metadata_in_package_is_refused_exactly_as_the_toolchain_refuses_it() {
        let text = "[package]\nname = \"a\"\nversion = \"1.0.0\"\ndescription = \"hi\"\n";
        let error = PackageManifest::parse(text, "aevia.toml").expect_err("refused");
        assert_eq!(error.code, E_MANIFEST);
        assert!(
            error.message.contains("aevia-pkg.toml"),
            "{}",
            error.message
        );
    }

    #[test]
    fn an_unknown_table_is_refused() {
        let error = PackageManifest::parse("[registry]\nurl = \"x\"\n", "m").expect_err("refused");
        assert!(error.message.contains("unknown table"), "{}", error.message);
    }

    #[test]
    fn a_two_part_version_is_refused() {
        let error = PackageManifest::parse("[package]\nname = \"a\"\nversion = \"1.0\"\n", "m")
            .expect_err("refused");
        assert!(
            error.message.contains("major.minor.patch"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_dependency_key_must_be_a_module_name() {
        let text = "[package]\nname = \"a\"\nversion = \"1.0.0\"\n[dependencies]\nunits = { path = \"../u\" }\n";
        let error = PackageManifest::parse(text, "m").expect_err("refused");
        assert!(
            error.message.contains("not a module name"),
            "{}",
            error.message
        );
    }

    #[test]
    fn registry_meta_is_read_from_its_own_file() {
        let meta = RegistryMeta::parse(
            "description = \"Linear algebra helpers\"\nlicense = \"Apache-2.0\"\nkeywords = [\"Linear\", \"math\"]\n",
            "aevia-pkg.toml",
        )
        .expect("parses");
        assert_eq!(meta.description.as_deref(), Some("Linear algebra helpers"));
        assert_eq!(meta.keywords, ["linear", "math"]);
    }

    #[test]
    fn a_description_skips_headings_and_badges() {
        let readme = "# aevia\n\n[![CI](x)](y)\n\nA pure functional language for\nscientific computing.\n\n## Install\n";
        assert_eq!(
            first_paragraph(readme).as_deref(),
            Some("A pure functional language for scientific computing.")
        );
    }

    #[test]
    fn a_long_description_is_cut_on_a_word() {
        let text = "word ".repeat(100);
        let cut = truncate_words(text.trim(), 20);
        assert!(cut.chars().count() <= 21, "{cut}");
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn licences_are_detected_by_their_own_words() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("LICENSE"),
            "Apache License\nVersion 2.0, January 2004\n",
        )
        .expect("write");
        assert_eq!(detect_license(dir.path()).as_deref(), Some("Apache-2.0"));

        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("LICENSE"),
            "// SPDX-License-Identifier: MPL-2.0\n",
        )
        .expect("write");
        assert_eq!(detect_license(dir.path()).as_deref(), Some("MPL-2.0"));
    }

    #[test]
    fn a_remote_becomes_a_web_link() {
        assert_eq!(
            normalize_remote("git@github.com:Apich-Organization/aevia.git"),
            "https://github.com/Apich-Organization/aevia"
        );
    }
}
