//! Aevia source parser (Chumsky 0.13). Phase 1.

#![allow(dead_code)]

pub mod expr;
pub mod items;

use crate::diagnostics::AeviaError;

/// Parse an `.ae` source file into an AST (currently returns the raw `SourceFile`).
pub fn parse_source(source: &str, path: &str) -> Result<(), AeviaError> {
    items::parse_source(source)
        .map(|_| ())
        .map_err(|e| AeviaError::message(format!("parse error in {path}: {e}")))
}
