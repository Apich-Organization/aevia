//! Error reporting with `miette`.

use miette::{Diagnostic, Report, SourceSpan};
use std::fmt;
use std::path::PathBuf;
/// Result type for Aevia operations.
pub type AeviaResult<T> = Result<T, AeviaError>;

/// Top-level compiler / CLI error.
#[derive(Debug)]
pub enum AeviaError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Message(String),
    NotImplemented {
        feature: String,
        phase: String,
    },
    Manifest(String),
    ProjectExists(PathBuf),
}

impl fmt::Display for AeviaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "I/O error at {}: {source}", path.display()),
            Self::Message(msg) => f.write_str(msg),
            Self::NotImplemented { feature, phase } => {
                write!(f, "{feature} is not implemented yet (planned for {phase})")
            }
            Self::Manifest(msg) => write!(f, "invalid Aevia.toml: {msg}"),
            Self::ProjectExists(path) => {
                write!(f, "project directory already exists: {}", path.display())
            }
        }
    }
}

impl std::error::Error for AeviaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl Diagnostic for AeviaError {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new(match self {
            Self::Io { .. } => "aevia::io",
            Self::Message(_) => "aevia::error",
            Self::NotImplemented { .. } => "aevia::not_implemented",
            Self::Manifest(_) => "aevia::manifest",
            Self::ProjectExists(_) => "aevia::project_exists",
        }))
    }
}

impl AeviaError {
    pub fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }

    pub fn not_implemented(feature: impl Into<String>, phase: impl Into<String>) -> Self {
        Self::NotImplemented {
            feature: feature.into(),
            phase: phase.into(),
        }
    }

    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// Print a diagnostic report to stderr.
pub fn emit(error: AeviaError) {
    let report = Report::new(error);
    eprintln!("{report:?}");
}

/// Placeholder for future span-backed errors.
#[derive(Debug, Clone)]
pub struct SpannedError {
    pub message: String,
    pub offset: usize,
    pub len: usize,
}

impl fmt::Display for SpannedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SpannedError {}

impl Diagnostic for SpannedError {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new("aevia::parse"))
    }
}

impl SpannedError {
    pub fn label(&self) -> (SourceSpan, String) {
        (SourceSpan::new(self.offset.into(), self.len), self.message.clone())
    }
}
