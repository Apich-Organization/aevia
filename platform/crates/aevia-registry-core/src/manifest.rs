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

use aevia_resolve::Version;
use aevia_resolve::VersionReq;
use aevia_resolve::names::canonical_package;
use aevia_resolve::names::default_import;
use aevia_resolve::names::official_package;

use crate::error::E_MANIFEST;
use crate::error::Error;
use crate::error::Result;
use crate::name::valid_crate_name;

/// A dependency as the manifest declares it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ManifestDependency {
    /// The module root it is imported under, e.g. `UnitsExtra`.
    pub import: String,
    /// The local checkout, when the manifest names one. A package that is
    /// published cannot rely on it: consumers do not have that directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The registry identity, `owner/name` (canonical spelling; a bare version
    /// string names the official scope).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// The version requirement, in its canonical rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub req: Option<String>,
}

impl ManifestDependency {
    /// The index record of this dependency; `None` for a path-only one.
    #[must_use]
    pub fn to_index(&self) -> Option<crate::index::IndexDependency> {
        let package = self.package.clone()?;
        let req = self.req.clone()?;
        let import = (self.import != default_import(&package)).then(|| self.import.clone());
        Some(crate::index::IndexDependency {
            package,
            req,
            import,
        })
    }
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
            let line = strip_comment(raw.trim_end_matches('\r')).trim();
            if line.is_empty() {
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
                    if !is_module_segment(key) {
                        return Err(fail(
                            origin,
                            number,
                            format!(
                                "`{key}` is not a module name; a dependency's key is the root                                  module it is imported under, so it must start with an                                  upper-case letter and contain no `-`"
                            ),
                        ));
                    }
                    if dependencies
                        .iter()
                        .any(|held: &ManifestDependency| held.import == key)
                    {
                        return Err(fail(origin, number, format!("`{key}` is declared twice")));
                    }
                    dependencies.push(parse_dependency(key, value, origin, number)?);
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
        if let Err(error) = Version::parse(&version) {
            return Err(Error::new(
                E_MANIFEST,
                format!(
                    "{origin}: `{version}` is not a SemVer version ({}); write                      `major.minor.patch`, optionally with a `-pre-release`",
                    error.reason
                ),
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

    /// The registry dependencies as the index records them (path-only
    /// dependencies have no registry identity and are left out).
    #[must_use]
    pub fn index_dependencies(&self) -> Vec<crate::index::IndexDependency> {
        self.dependencies
            .iter()
            .filter_map(ManifestDependency::to_index)
            .collect()
    }

    /// Whether the manifest can be published: every dependency must be a
    /// registry package, because a consumer of the archive has no `../local`
    /// directory. A dependency may keep a `path` for local development as long
    /// as it also names its `version`.
    ///
    /// # Errors
    ///
    /// `E_MANIFEST` naming the path-only dependencies.
    pub fn check_publishable(&self) -> Result<()> {
        self.check_publishable_as("")
    }

    /// [`PackageManifest::check_publishable`] for the package published as
    /// `id` (`owner/name`), which additionally may not depend on itself.
    ///
    /// # Errors
    ///
    /// `E_MANIFEST` for a path-only dependency or a self-dependency.
    pub fn check_publishable_as(
        &self,
        id: &str,
    ) -> Result<()> {
        let path_only: Vec<&str> = self
            .dependencies
            .iter()
            .filter(|dependency| dependency.package.is_none())
            .map(|dependency| dependency.import.as_str())
            .collect();
        if !path_only.is_empty() {
            let listed = path_only
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(Error::new(
                E_MANIFEST,
                format!(
                    "{listed} {} only a `path`, which does not exist for anyone who downloads                      this package; give each a registry version, as in `Name = {{ path =                      \"../name\", package = \"owner/name\", version = \"^1.0\" }}`",
                    if path_only.len() == 1 { "names" } else { "name" }
                ),
            ));
        }
        if !id.is_empty()
            && let Some(own) = canonical_package(id)
            && self
                .dependencies
                .iter()
                .any(|dependency| dependency.package.as_deref() == Some(own.as_str()))
        {
            return Err(Error::new(
                E_MANIFEST,
                format!("`{own}` depends on itself"),
            ));
        }
        Ok(())
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

/// Cuts a trailing `# comment` that is not inside a quoted string.
fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (position, character) in line.char_indices() {
        match character {
            | '"' => quoted = !quoted,
            | '#' if !quoted => return &line[..position],
            | _ => {},
        }
    }
    line
}

/// Splits the inside of an inline table on the commas outside quoted strings.
fn split_fields(inner: &str) -> Option<Vec<&str>> {
    let mut fields = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (position, character) in inner.char_indices() {
        match character {
            | '"' => quoted = !quoted,
            | ',' if !quoted => {
                fields.push(&inner[start..position]);
                start = position + 1;
            },
            | _ => {},
        }
    }
    if quoted {
        return None;
    }
    fields.push(&inner[start..]);
    Some(fields)
}

/// Parses the value of one `[dependencies]` entry, exactly as the toolchain
/// does (`aevia_core::manifest`): a version requirement string, or an inline
/// table with `path`, `package` and `version`.
fn parse_dependency(
    key: &str,
    value: &str,
    origin: &str,
    number: usize,
) -> Result<ManifestDependency> {
    let value = value.trim();
    let requirement = |text: &str| -> Result<String> {
        VersionReq::parse(text)
            .map(|req| req.to_string())
            .map_err(|error| {
                fail(
                    origin,
                    number,
                    format!(
                        "{error}; a requirement is `^1.2`, `~1.2.3`, `>=1.2, <2`, `1.*`, \
                         `=1.2.3` or `*`"
                    ),
                )
            })
    };
    if value.starts_with('"') {
        let text = parse_string(value).ok_or_else(|| {
            fail(origin, number, "a version requirement is one quoted string")
        })?;
        return Ok(ManifestDependency {
            import: key.to_string(),
            path: None,
            package: Some(official_package(key)),
            req: Some(requirement(&text)?),
        });
    }
    let Some(inner) = value.strip_prefix('{').and_then(|rest| rest.strip_suffix('}')) else {
        return Err(fail(
            origin,
            number,
            format!(
                "expected a version requirement (`{key} = \"^1.2\"`) or an inline table such \
                 as `{{ path = \"…\" }}` or `{{ package = \"owner/name\", version = \"^1.2\" }}`"
            ),
        ));
    };
    let Some(fields) = split_fields(inner) else {
        return Err(fail(origin, number, "an unterminated string in the inline table"));
    };
    let mut path = None;
    let mut package = None;
    let mut version = None;
    for field in fields {
        let field = field.trim();
        if field.is_empty() {
            continue;
        }
        let Some((name, field_value)) = field.split_once('=') else {
            return Err(fail(
                origin,
                number,
                format!("expected `key = value` in `{field}`"),
            ));
        };
        let name = name.trim();
        let text = parse_string(field_value.trim())
            .ok_or_else(|| fail(origin, number, format!("`{name}` expects a quoted string")))?;
        let slot = match name {
            | "path" => &mut path,
            | "package" => &mut package,
            | "version" => &mut version,
            | other => {
                return Err(fail(
                    origin,
                    number,
                    format!(
                        "`{other}` is not supported; a dependency is a version requirement, \
                         `{{ package = \"owner/name\", version = \"…\" }}`, `{{ path = \"…\" }}`, \
                         or a path with a version (there is no git dependency)"
                    ),
                ));
            },
        };
        if slot.replace(text).is_some() {
            return Err(fail(origin, number, format!("`{name}` is given twice")));
        }
    }
    let package = match package {
        | Some(text) => {
            Some(canonical_package(&text).ok_or_else(|| {
                fail(
                    origin,
                    number,
                    format!("`{text}` is not a package identity; write `owner/name`"),
                )
            })?)
        },
        | None => None,
    };
    let req = match version {
        | Some(text) => Some(requirement(&text)?),
        | None => None,
    };
    let import = key.to_string();
    match (path, package, req) {
        | (Some(path), None, None) => {
            Ok(ManifestDependency {
                import,
                path: Some(path),
                package: None,
                req: None,
            })
        },
        | (path, package, Some(req)) => {
            Ok(ManifestDependency {
                import,
                path,
                package: Some(package.unwrap_or_else(|| official_package(key))),
                req: Some(req),
            })
        },
        | (None, Some(package), None) => {
            Err(fail(
                origin,
                number,
                format!("`package = \"{package}\"` needs a `version` (`*` accepts any)"),
            ))
        },
        | (Some(_), Some(_), None) => {
            Err(fail(
                origin,
                number,
                "`package` needs a `version` as well; without one this is a path dependency",
            ))
        },
        | (None, None, None) => Err(fail(origin, number, "a dependency needs a `path` or a `version`")),
    }
}

fn is_module_segment(text: &str) -> bool {
    let mut characters = text.chars();
    match characters.next() {
        | Some(first) if first.is_ascii_uppercase() => {},
        | _ => return false,
    }
    characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
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
            let key = &dependency.import;
            let official = official_package(key);
            let package = dependency.package.as_deref().unwrap_or("");
            match (&dependency.path, &dependency.req) {
                | (Some(path), None) => {
                    let _ = writeln!(out, "{key} = {{ path = \"{path}\" }}");
                },
                | (None, Some(req)) if package == official => {
                    let _ = writeln!(out, "{key} = \"{req}\"");
                },
                | (None, Some(req)) => {
                    let _ = writeln!(
                        out,
                        "{key} = {{ package = \"{package}\", version = \"{req}\" }}"
                    );
                },
                | (Some(path), Some(req)) if package == official => {
                    let _ = writeln!(out, "{key} = {{ path = \"{path}\", version = \"{req}\" }}");
                },
                | (Some(path), Some(req)) => {
                    let _ = writeln!(
                        out,
                        "{key} = {{ path = \"{path}\", package = \"{package}\", version = \
                         \"{req}\" }}"
                    );
                },
                | (None, None) => {},
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "# a package\n[package]\nname = \"units-extra\"\nversion = \"0.2.0\"\nentry = \"src/main.aev\"\n\n[dependencies]\nUnitsExtra = { path = \"../units\" }\n";

    fn with_dependencies(lines: &str) -> String {
        format!("[package]\nname = \"a\"\nversion = \"1.0.0\"\n[dependencies]\n{lines}")
    }

    #[test]
    fn a_manifest_parses() {
        let manifest = PackageManifest::parse(MANIFEST, "aevia.toml").expect("parses");
        assert_eq!(manifest.name, "units-extra");
        assert_eq!(manifest.version, "0.2.0");
        assert_eq!(manifest.entry, "src/main.aev");
        assert_eq!(manifest.dependencies.len(), 1);
        assert_eq!(manifest.dependencies[0].import, "UnitsExtra");
        assert_eq!(manifest.dependencies[0].path.as_deref(), Some("../units"));
        assert!(manifest.dependencies[0].package.is_none());
    }

    #[test]
    fn every_dependency_form_matches_what_the_toolchain_reads() {
        let text = with_dependencies(
            "Local = { path = \"../local\" }\n\
             Units = \"^1.2\"  # bare: the official scope\n\
             Range = \">=1.2, <2\"\n\
             Plot = { package = \"Apich/Plot\", version = \"~2.1.0\" }\n\
             Dev = { path = \"../dev\", package = \"apich/dev\", version = \"^0.3\" }\n",
        );
        let manifest = PackageManifest::parse(&text, "aevia.toml").expect("parses");
        let by_import = |name: &str| {
            manifest
                .dependencies
                .iter()
                .find(|d| d.import == name)
                .expect(name)
        };
        assert_eq!(by_import("Local").package, None);
        assert_eq!(by_import("Units").package.as_deref(), Some("aevia/units"));
        assert_eq!(by_import("Units").req.as_deref(), Some("^1.2"));
        assert_eq!(by_import("Range").req.as_deref(), Some(">=1.2, <2"));
        assert_eq!(by_import("Plot").package.as_deref(), Some("apich/plot"));
        assert_eq!(by_import("Dev").path.as_deref(), Some("../dev"));
        assert_eq!(by_import("Dev").req.as_deref(), Some("^0.3"));
        // The dependency list the index records: package + req, and the import
        // only when it is not the package's default.
        let index = by_import("Plot").to_index().expect("registry");
        assert_eq!(index.package, "apich/plot");
        assert_eq!(index.import, None, "Plot is the default import of apich/plot");
        let index = by_import("Units").to_index().expect("registry");
        assert_eq!(index.import, None, "Units is aevia/units' default import");
        assert!(by_import("Local").to_index().is_none());
    }

    #[test]
    fn a_published_package_may_not_need_a_path() {
        let text = with_dependencies(
            "Local = { path = \"../local\" }\nUnits = \"^1\"\nDev = { path = \"../dev\", version = \"1\" }\n",
        );
        let manifest = PackageManifest::parse(&text, "aevia.toml").expect("parses");
        let error = manifest.check_publishable().expect_err("path-only");
        assert_eq!(error.code, E_MANIFEST);
        assert!(error.message.contains("`Local`"), "{}", error.message);
        assert!(!error.message.contains("`Dev`"), "{}", error.message);
        let text = with_dependencies("Units = \"^1\"\nDev = { path = \"../dev\", version = \"1\" }\n");
        PackageManifest::parse(&text, "aevia.toml")
            .expect("parses")
            .check_publishable()
            .expect("path plus version is publishable");
    }

    #[test]
    fn a_package_may_not_depend_on_itself() {
        let text = "[package]\nname = \"a\"\nversion = \"1.0.0\"\n[dependencies]\nA = { package = \"o/a\", version = \"1\" }\n";
        let manifest = PackageManifest::parse(text, "aevia.toml").expect("parses");
        let error = manifest.check_publishable_as("o/a").expect_err("self dependency");
        assert!(error.message.contains("itself"), "{}", error.message);
        manifest
            .check_publishable_as("o/other")
            .expect("another package of the same name");
    }

    #[test]
    fn unsupported_dependency_syntax_is_refused_like_the_toolchain_refuses_it() {
        for (lines, needle) in [
            ("A = { git = \"x\" }\n", "not supported"),
            ("A = \"nonsense\"\n", "is not a number"),
            ("A = { package = \"apich/a\" }\n", "needs a `version`"),
            ("A = { package = \"nope\", version = \"1\" }\n", "owner/name"),
            ("A = 7\n", "expected a version requirement"),
            ("A = \"^1\"\nA = \"^2\"\n", "declared twice"),
        ] {
            let error =
                PackageManifest::parse(&with_dependencies(lines), "aevia.toml").expect_err(lines);
            assert_eq!(error.code, E_MANIFEST, "{lines}");
            assert!(error.message.contains(needle), "{lines}: {}", error.message);
        }
    }

    #[test]
    fn a_pre_release_version_is_a_version() {
        let manifest = PackageManifest::parse(
            "[package]\nname = \"a\"\nversion = \"1.0.0-rc.1\"\n",
            "m",
        )
        .expect("parses");
        assert_eq!(manifest.version, "1.0.0-rc.1");
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
        assert!(error.message.contains("SemVer"), "{}", error.message);
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
    fn the_render_helper_writes_what_the_parser_reads() {
        let dependencies = PackageManifest::parse(
            &with_dependencies(
                "Local = { path = \"../local\" }\nUnits = \"^1.2\"\nPlot = { package = \"apich/plot\", version = \"~2.1.0\" }\nDev = { path = \"../dev\", package = \"apich/dev\", version = \"^0.3\" }\n",
            ),
            "m",
        )
        .expect("parses")
        .dependencies;
        let text = render("a", "1.0.0", "src/main.aev", &dependencies);
        let again = PackageManifest::parse(&text, "m").expect("re-parses");
        assert_eq!(again.dependencies, dependencies);
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
