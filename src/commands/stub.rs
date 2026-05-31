//! Shared stub for not-yet-implemented commands.

use crate::diagnostics::{AeviaError, AeviaResult};
use colored::Colorize;

pub fn not_implemented(feature: &str, phase: impl Into<String>) -> AeviaResult<()> {
    let phase = phase.into();
    let err = AeviaError::not_implemented(feature, phase.clone());
    eprintln!("{} {}", "note:".yellow().bold(), err);
    Err(err)
}
