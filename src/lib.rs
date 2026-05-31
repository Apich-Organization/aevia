//! Aevia compiler library.

pub mod ast;
pub mod cli;
pub mod commands;
pub mod diagnostics;
pub mod logging;
pub mod lowering;
pub mod manifest;
pub mod modules;
pub mod ops;
pub mod parser;
pub mod project;
pub mod types;

pub use diagnostics::{AeviaError, AeviaResult};

/// Initialize logging and run the CLI.
pub fn run() -> AeviaResult<()> {
    logging::init();
    cli::execute()
}
