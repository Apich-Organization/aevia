//! `aevia pkg` — the package registry client.
//!
//! # Why this is a forwarder and not an implementation
//!
//! The registry client is `aevia-pkg`, a binary in the platform workspace
//! (`platform/crates/aevia-pkg`). It is deliberately not linked into this
//! binary: it carries an OpenPGP verifier, an HTTP client and a tarball codec
//! that a compiler run never needs, and the toolchain's dependency graph is
//! already the heaviest thing in the project.
//!
//! Forwarding gives the ergonomics without the coupling: `aevia pkg publish`
//! works, and the exit status, the standard streams and the terminal prompts
//! belong to the child, so a passphrase prompt from `gpg-agent` still works.
//!
//! # Where the binary is looked for
//!
//! 1. `AEVIA_PKG`, for a checkout that is not installed.
//! 2. `aevia-pkg` on `PATH`, which is what `cargo install --path
//!    platform/crates/aevia-pkg` produces.
//! 3. `aevia-pkg` next to this executable, so an unpacked release works without
//!    installing anything.
//!
//! When none of them exists the message names all three, because "command not
//! found" on its own does not tell a user that a second binary is involved.

use std::path::PathBuf;
use std::process::Command;
use std::process::ExitCode;

/// The name of the client binary.
const BINARY: &str = if cfg!(windows) {
    "aevia-pkg.exe"
} else {
    "aevia-pkg"
};

/// Runs `aevia-pkg` with `args`.
pub fn run(args: &[String]) -> ExitCode {
    let Some(binary) = locate() else {
        eprintln!(
            "aevia pkg: cannot find `{BINARY}`.\n\
             aevia pkg: install it with `cargo install --path platform/crates/aevia-pkg`,\n\
             aevia pkg: or point `AEVIA_PKG` at a built binary."
        );
        return ExitCode::from(2);
    };
    match Command::new(&binary).args(args).status() {
        | Ok(status) => {
            match status.code() {
                | Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
                | None => {
                    // Killed by a signal: report failure rather than success.
                    eprintln!("aevia pkg: the client did not exit normally");
                    ExitCode::from(1)
                },
            }
        },
        | Err(error) => {
            eprintln!("aevia pkg: cannot run `{}`: {error}", binary.display());
            ExitCode::from(1)
        },
    }
}

/// Finds the client binary.
fn locate() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("AEVIA_PKG") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Some(found) = on_path() {
        return Some(found);
    }
    let beside = std::env::current_exe().ok()?.parent()?.join(BINARY);
    beside.is_file().then_some(beside)
}

/// Looks for the binary in `PATH`.
///
/// Written by hand rather than shelling out to `which`: the answer is needed on
/// Windows too, and `Command::new(BINARY)` would only tell us whether the spawn
/// worked, not where it came from — which is what the error message wants.
fn on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(BINARY))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_binary_name_has_the_platform_suffix() {
        if cfg!(windows) {
            assert!(BINARY.ends_with(".exe"));
        } else {
            assert!(!BINARY.contains('.'));
        }
    }

    #[test]
    fn an_empty_path_finds_nothing() {
        // `on_path` reads the process environment; this only asserts that the
        // helper is total, not that a particular machine has the binary.
        let found = on_path();
        assert!(found.is_none() || found.is_some_and(|path| path.is_file()));
    }
}
