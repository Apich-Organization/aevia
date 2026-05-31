//! `Aevia.toml` project manifest.

use crate::diagnostics::{AeviaError, AeviaResult};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

const MANIFEST_NAME: &str = "Aevia.toml";

#[derive(Debug, Clone)]
pub struct Manifest {
    pub package: PackageSection,
    pub profile: ProfileSection,
    pub dependencies: IndexMap<String, DependencySpec>,
}

#[derive(Debug, Clone)]
pub struct PackageSection {
    pub name: String,
    pub version: String,
    pub authors: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProfileSection {
    pub release: ReleaseProfile,
}

#[derive(Debug, Clone, Default)]
pub struct ReleaseProfile {
    pub opt_level: Option<u8>,
    pub backend: Option<String>,
    pub dimension_checking: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DependencySpec {
    pub version: Option<String>,
    pub registry: Option<String>,
}

impl Manifest {
    pub fn load_from_dir(project_root: &Path) -> AeviaResult<Self> {
        Self::load(&project_root.join(MANIFEST_NAME))
    }

    pub fn load(path: &Path) -> AeviaResult<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| AeviaError::io(path, e))?;
        Self::parse(&text).map_err(AeviaError::Manifest)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let root: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;

        let package_tbl = root
            .get("package")
            .and_then(|v| v.as_table())
            .ok_or_else(|| "missing [package] section".to_string())?;

        let name = package_tbl
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "package.name is required".to_string())?
            .to_string();

        let version = package_tbl
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("0.1.0")
            .to_string();

        let authors = package_tbl
            .get("authors")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let profile = parse_profile(root.get("profile"));
        let dependencies = parse_dependencies(root.get("dependencies"));

        Ok(Self {
            package: PackageSection {
                name,
                version,
                authors,
            },
            profile,
            dependencies,
        })
    }
}

fn parse_profile(value: Option<&toml::Value>) -> ProfileSection {
    let mut profile = ProfileSection::default();
    let Some(tbl) = value.and_then(|v| v.as_table()) else {
        return profile;
    };
    let Some(release) = tbl.get("release").and_then(|v| v.as_table()) else {
        return profile;
    };

    profile.release.opt_level = release
        .get("opt-level")
        .and_then(|v| v.as_integer())
        .and_then(|i| u8::try_from(i).ok());

    profile.release.backend = release
        .get("backend")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    profile.release.dimension_checking = release
        .get("dimension-checking")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    profile
}

fn parse_dependencies(value: Option<&toml::Value>) -> IndexMap<String, DependencySpec> {
    let mut deps = IndexMap::new();
    let Some(tbl) = value.and_then(|v| v.as_table()) else {
        return deps;
    };

    for (name, spec) in tbl {
        if let Some(version) = spec.as_str() {
            deps.insert(
                name.clone(),
                DependencySpec {
                    version: Some(version.to_string()),
                    registry: None,
                },
            );
        } else if let Some(spec_tbl) = spec.as_table() {
            deps.insert(
                name.clone(),
                DependencySpec {
                    version: spec_tbl
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    registry: spec_tbl
                        .get("registry")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                },
            );
        }
    }

    deps
}

/// Locate `Aevia.toml` by walking up from `start`.
pub fn find_project_root(start: &Path) -> AeviaResult<PathBuf> {
    let start = if start.is_file() {
        start.parent().unwrap_or(Path::new("."))
    } else {
        start
    };

    let mut dir = start.canonicalize().map_err(|e| AeviaError::io(start, e))?;

    loop {
        if dir.join(MANIFEST_NAME).is_file() {
            return Ok(dir);
        }
        if !dir.pop() {
            break;
        }
    }

    Err(AeviaError::message(format!(
        "no {MANIFEST_NAME} found in {} or any parent directory",
        start.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_manifest() {
        let text = r#"
[package]
name = "demo"
version = "1.0.0"
authors = ["test"]

[profile.release]
opt-level = 3
backend = "rssn-gpu"
dimension-checking = "strict"

[dependencies]
linear_algebra = { version = "1.4", registry = "aevia-central" }
"#;
        let m = Manifest::parse(text).unwrap();
        assert_eq!(m.package.name, "demo");
        assert_eq!(m.profile.release.opt_level, Some(3));
        assert_eq!(
            m.profile.release.backend.as_deref(),
            Some("rssn-gpu")
        );
        assert_eq!(m.dependencies.len(), 1);
    }
}
