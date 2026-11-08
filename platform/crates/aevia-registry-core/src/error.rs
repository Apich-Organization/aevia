//! One error type for the whole crate.
//!
//! A registry's errors end up in three places — a terminal, a JSON body and a
//! web page — so the shape is a stable machine code plus a sentence written for
//! a human. The code is a `&'static str` because every caller in the workspace
//! matches on it; inventing codes at run time would make them un-greppable.

use std::fmt;

/// A registry failure.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Error {
    /// A stable machine-readable code, `E_NAME`, `E_SIG`, …
    pub code: &'static str,
    /// The sentence shown to the user.
    pub message: String,
}

impl Error {
    /// Builds an error from a code and a message.
    #[must_use]
    pub fn new(
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// The code and the message on one line, for a terminal.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}: {}", self.code, self.message)
    }

    /// Wraps an I/O failure with the path that caused it.
    #[must_use]
    pub fn io(
        context: impl fmt::Display,
        error: &std::io::Error,
    ) -> Self {
        Self::new("E_IO", format!("{context}: {error}"))
    }

    /// Wraps a JSON failure.
    #[must_use]
    pub fn json(
        context: impl fmt::Display,
        error: &serde_json::Error,
    ) -> Self {
        Self::new("E_JSON", format!("{context}: {error}"))
    }
}

impl fmt::Display for Error {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}

/// The result type every function in this crate returns.
pub type Result<T> = std::result::Result<T, Error>;

/// `E_NAME`: a package name or owner does not satisfy the naming rules.
pub const E_NAME: &str = "E_NAME";
/// `E_VERSION`: a version is not a version this toolchain can express.
pub const E_VERSION: &str = "E_VERSION";
/// `E_MANIFEST`: `aevia.toml` does not parse under the toolchain's own rules.
pub const E_MANIFEST: &str = "E_MANIFEST";
/// `E_TARBALL`: the archive is not a package, or unpacks unsafely.
pub const E_TARBALL: &str = "E_TARBALL";
/// `E_SIGNATURE`: a signature did not verify.
pub const E_SIGNATURE: &str = "E_SIGNATURE";
/// `E_KEY`: a public key is malformed or unsupported.
pub const E_KEY: &str = "E_KEY";
/// `E_STATEMENT`: a signed statement is malformed, mismatched or expired.
pub const E_STATEMENT: &str = "E_STATEMENT";
/// `E_IO`: the filesystem refused.
pub const E_IO: &str = "E_IO";
/// `E_JSON`: a JSON document did not parse.
pub const E_JSON: &str = "E_JSON";
