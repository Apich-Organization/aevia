//! Project scaffolding and templates.

use crate::diagnostics::{AeviaError, AeviaResult};
use crate::manifest::Manifest;
use std::fs;
use std::path::{Path, PathBuf};

const SAMPLE_MAIN_AE: &str = r#"//! Sample Aevia simulation entry point.

type Acceleration = m / s^2;
type Force = kg * Acceleration;

/// Kinetic energy: 0.5 * m * v^2
/// # Physical Context
/// - domain: mass > 0.0, velocity >= 0.0
/// - unit_output: Joule
pub fn kinetic_energy(m: kg, v: m/s) -> Joule := 0.5 * m * v^2;

pub fn main() {
    let mass: kg = 10.0;
    let acc: Acceleration = 9.8;
    let force: Force = mass * acc;
    let _ = force;
}
"#;

const SAMPLE_LIB_AE: &str = r#"//! Library root for shared simulation utilities.

pub mod physics;
"#;

const SAMPLE_PHYSICS_AE: &str = r#"//! Physics helpers.

pub struct Particle {
    pub position: m,
    pub velocity: m / s,
    pub mass: kg,
}
"#;

const SAMPLE_AEVIA_TOML: &str = r#"[package]
name = "{name}"
version = "0.1.0"
authors = ["Aevia Developer"]

[profile.release]
opt-level = 3
backend = "rssn"
dimension-checking = "strict"
"#;

/// Create a new Aevia project at `root/name`.
pub fn create_new(name: &str, parent: &Path) -> AeviaResult<PathBuf> {
    let project_dir = parent.join(name);
    if project_dir.exists() {
        return Err(AeviaError::ProjectExists(project_dir));
    }

    fs::create_dir_all(project_dir.join("src/modules"))
        .map_err(|e| AeviaError::io(&project_dir, e))?;
    fs::create_dir_all(project_dir.join("tests"))
        .map_err(|e| AeviaError::io(&project_dir, e))?;
    fs::create_dir_all(project_dir.join("kernels"))
        .map_err(|e| AeviaError::io(&project_dir, e))?;

    let manifest = SAMPLE_AEVIA_TOML.replace("{name}", name);
    write_file(project_dir.join("Aevia.toml"), &manifest)?;
    write_file(project_dir.join("src/main.ae"), SAMPLE_MAIN_AE)?;
    write_file(project_dir.join("src/lib.ae"), SAMPLE_LIB_AE)?;
    write_file(
        project_dir.join("src/modules/physics.ae"),
        SAMPLE_PHYSICS_AE,
    )?;
    write_file(
        project_dir.join("tests/smoke.ae"),
        "// @aevia-test: check\n\nfn smoke() -> m := 1.0;\n",
    )?;
    write_file(
        project_dir.join("kernels/.gitkeep"),
        "",
    )?;

    Ok(project_dir)
}

fn write_file(path: PathBuf, contents: &str) -> AeviaResult<()> {
    fs::write(&path, contents).map_err(|e| AeviaError::io(path, e))
}

/// Load manifest for a path inside a project.
pub fn load_manifest_for(path: &Path) -> AeviaResult<Manifest> {
    let root = crate::manifest::find_project_root(path)?;
    Manifest::load_from_dir(&root)
}
