//! The one error type the client reports with.
//!
//! `aevia-pkg` is a command-line tool before it is a library, so an error is
//! not just a value that propagates: it is a paragraph printed to stderr and an
//! exit status a script branches on. Two things follow from that.
//!
//! * Every failure carries a **next step** when there is one. `cargo` says
//!   `error: failed to parse manifest`; a publisher then has to guess. Saying
//!   `run `aevia-pkg login --key …` first` costs one sentence and saves a
//!   round trip through the documentation.
//! * The exit status is part of the contract. `2` means "you invoked me
//!   wrongly, or the tool I need is not installed" and `1` means "I tried and
//!   the attempt failed", so a script can tell "fix your command line" from
//!   "retry later" without parsing prose.
//!
//! Registry failures are not re-invented here: [`aevia_registry_core::Error`]
//! already carries a stable code (`E_SIGNATURE`, `E_STATEMENT`, …) and a
//! sentence, and it is converted by [`CliError::from`] so the code a publisher
//! sees on the terminal is the code the registry logged.

use std::fmt;

use aevia_registry_core::Error as CoreError;

/// How a failure should be reported to the shell.
///
/// The split exists because a missing `gpg` is not a failed publish — it is a
/// publish that never started, and the fix is to install a program, not to
/// retry. Collapsing both into exit 1 would make `aevia-pkg publish || retry`
/// loops spin forever on a machine that will never succeed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// A command was attempted and failed: exit `1`.
    Failure,
    /// The invocation, or the environment it needs, is wrong: exit `2`.
    Usage,
}

/// A failure as the terminal should see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliError {
    /// What failed and why, written as `<what failed>: <why>`.
    pub message: String,
    /// What the publisher should do next, when there is something to do.
    pub next_step: Option<String>,
    /// Which exit status this failure maps to.
    pub exit: Exit,
}

impl CliError {
    /// A failed attempt.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            next_step: None,
            exit: Exit::Failure,
        }
    }

    /// A wrongly formed invocation, or a tool the client cannot run without.
    ///
    /// This is used for a missing `gpg`/`ssh-keygen` as well as for a bad
    /// argument: both mean "the client cannot even begin", and both are fixed
    /// by changing the machine or the command line rather than by retrying.
    #[must_use]
    pub fn usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            next_step: None,
            exit: Exit::Usage,
        }
    }

    /// Attaches the next step to take.
    #[must_use]
    pub fn with_next_step(
        mut self,
        next_step: impl Into<String>,
    ) -> Self {
        self.next_step = Some(next_step.into());
        self
    }

    /// The exit status this failure maps to.
    #[must_use]
    pub fn exit_code(&self) -> std::process::ExitCode {
        match self.exit {
            | Exit::Usage => std::process::ExitCode::from(2),
            | Exit::Failure => std::process::ExitCode::from(1),
        }
    }

    /// Prints the failure to stderr in the form every command shares.
    ///
    /// stderr, not stdout, because `aevia-pkg search --json | jq` must not see
    /// a diagnostic in the middle of its document.
    pub fn report(&self) {
        eprintln!("aevia-pkg: {}", self.message);
        if let Some(next_step) = &self.next_step {
            eprintln!("aevia-pkg: {next_step}");
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for CliError {}

impl From<CoreError> for CliError {
    /// Keeps the registry's own code in front of the sentence.
    ///
    /// `E_STATEMENT: the nonce is not …` is worth more to a publisher than the
    /// same words without the code, because the code is what the registry's
    /// documentation and the server log are indexed by.
    fn from(error: CoreError) -> Self {
        Self::new(error.render())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_registry_error_keeps_its_code() {
        let error = CliError::from(CoreError::new("E_STATEMENT", "the nonce is short"));
        assert_eq!(error.message, "E_STATEMENT: the nonce is short");
        assert_eq!(error.exit, Exit::Failure);
    }

    #[test]
    fn usage_failures_exit_two_and_attempts_exit_one() {
        assert_eq!(
            CliError::usage("x").exit_code(),
            std::process::ExitCode::from(2)
        );
        assert_eq!(
            CliError::new("x").exit_code(),
            std::process::ExitCode::from(1)
        );
    }

    #[test]
    fn a_next_step_is_optional_and_kept() {
        assert!(CliError::new("x").next_step.is_none());
        let error = CliError::new("x").with_next_step("run `aevia-pkg login`");
        assert_eq!(error.next_step.as_deref(), Some("run `aevia-pkg login`"));
    }
}
