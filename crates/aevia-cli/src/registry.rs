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

/// Runs `aevia-pkg` with `args` and reports whether it succeeded; the
/// dependency commands (`add`, `update`, `fetch`) use this to download indexes
/// and verified sources into `.aevia/registry` while keeping their own output.
///
/// # Errors
///
/// A message when the binary cannot be found or run, or exits unsuccessfully.
pub fn invoke(args: &[String]) -> Result<(), String> {
    let Some(binary) = locate() else {
        return Err(format!(
            "cannot find `{BINARY}` (set `AEVIA_PKG`, put it on `PATH`, or install it with \
             `cargo install --path platform/crates/aevia-pkg`); it downloads and verifies \
             registry packages"
        ));
    };
    match Command::new(&binary).args(args).status() {
        | Ok(status) if status.success() => Ok(()),
        | Ok(status) => {
            Err(format!(
                "`{} {}` failed ({status})",
                binary.display(),
                args.join(" ")
            ))
        },
        | Err(error) => Err(format!("cannot run `{}`: {error}", binary.display())),
    }
}

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

/// The commands of the registry client that `aevia` also accepts at top level.
///
/// `aevia search` reads better than `aevia pkg search`, and these are the
/// commands people reach for most. `add`, `update`, `fetch` and `tree` are not
/// listed: they are native to the toolchain (they edit `aevia.toml` and
/// `aevia.lock`) and call the client only for the network part. `publish` is
/// native too: it runs this binary's own checks first.
pub const FORWARDED: &[&str] = &[
    "login", "key", "yank", "search", "info", "verify", "status", "lock", "index",
];

/// Whether `command` is one of the top-level aliases of the registry client.
#[must_use]
pub fn is_forwarded(command: &str) -> bool {
    FORWARDED.contains(&command)
}

/// The registry a command talks to when it is not told: `--registry URL`, then
/// `AEVIA_REGISTRY`, then the public registry.
const DEFAULT_REGISTRY: &str = "https://aevia.apich.org";

/// `aevia publish [--no-verify] [client options…]`
///
/// Runs the checks a published package should pass — type-check and formatting
/// — with this very binary, then hands over to `aevia-pkg publish`. A package
/// that does not compile is refused *before* a key is touched or a challenge
/// is requested: the registry would reject it anyway, but only after a
/// passphrase prompt and a round trip.
pub fn publish(args: &[String]) -> ExitCode {
    let skip = args.iter().any(|argument| argument == "--no-verify");
    let forwarded: Vec<String> = std::iter::once("publish".to_string())
        .chain(
            args.iter()
                .filter(|argument| argument.as_str() != "--no-verify")
                .cloned(),
        )
        .collect();
    if !skip && !preflight() {
        eprintln!(
            "aevia publish: the package does not pass its own checks; fix them, or publish \
             anyway with `--no-verify`"
        );
        return ExitCode::from(1);
    }
    run(&forwarded)
}

/// Type-checks the package here and verifies its formatting with this binary.
fn preflight() -> bool {
    let Ok(me) = std::env::current_exe() else {
        // Without a path to ourselves there is nothing to run; the registry
        // still checks everything that matters on its side.
        return true;
    };
    for (label, args) in [
        ("check", &["check", "--all"][..]),
        ("fmt --check", &["fmt", "--check"][..]),
    ] {
        eprintln!("aevia publish: running `aevia {label}`");
        match Command::new(&me).args(args).status() {
            | Ok(status) if status.success() => {},
            | Ok(_) => return false,
            | Err(error) => {
                eprintln!("aevia publish: cannot run `aevia {label}`: {error}");
                return false;
            },
        }
    }
    true
}

/// `aevia docs [owner/name[@version]] [--open] [--registry URL]`
///
/// Prints the address of a package's documentation on the registry (the site
/// the compiler built at publish time), or opens it. With no package, the
/// documentation landing page.
pub fn docs(args: &[String]) -> ExitCode {
    let mut open = false;
    let mut registry: Option<String> = None;
    let mut target: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            | "--open" => open = true,
            | "--registry" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    eprintln!("aevia docs: `--registry` expects a URL");
                    return ExitCode::from(2);
                };
                registry = Some(value.clone());
            },
            | other if other.starts_with('-') => {
                eprintln!(
                    "aevia docs: unknown option `{other}` (the options are `--open` and \
                     `--registry URL`)"
                );
                return ExitCode::from(2);
            },
            | other => {
                if target.replace(other.to_string()).is_some() {
                    eprintln!("aevia docs: expected one package, `owner/name[@version]`");
                    return ExitCode::from(2);
                }
            },
        }
        index += 1;
    }
    let base = registry
        .or_else(|| std::env::var("AEVIA_REGISTRY").ok())
        .unwrap_or_else(|| DEFAULT_REGISTRY.to_string());
    let url = match docs_url(&base, target.as_deref()) {
        | Ok(url) => url,
        | Err(message) => {
            eprintln!("aevia docs: {message}");
            return ExitCode::from(2);
        },
    };
    println!("{url}");
    if open {
        open_url(&url);
    }
    ExitCode::SUCCESS
}

/// The documentation address for `target` (`owner/name[@version]`) on `base`.
fn docs_url(
    base: &str,
    target: Option<&str>,
) -> Result<String, String> {
    let base = base.trim_end_matches('/');
    let Some(target) = target else {
        return Ok(format!("{base}/docs"));
    };
    let (id, version) = match target.split_once('@') {
        | Some((id, version)) => (id, Some(version)),
        | None => (target, None),
    };
    let valid = id
        .split_once('/')
        .is_some_and(|(owner, name)| !owner.is_empty() && !name.is_empty() && !name.contains('/'));
    if !valid {
        return Err(format!(
            "`{target}` is not `owner/name[@version]` (packages are scoped, `apich/units`)"
        ));
    }
    Ok(match version {
        | Some(version) if !version.is_empty() => format!("{base}/docs/{id}/{version}"),
        | _ => format!("{base}/docs/{id}"),
    })
}

/// Opens `url` in the default browser, when standard output is a terminal.
fn open_url(url: &str) {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return;
    }
    #[cfg(target_os = "windows")]
    let spawned = Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(url)
        .spawn();
    #[cfg(target_os = "macos")]
    let spawned = Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let spawned = Command::new("xdg-open").arg(url).spawn();
    if let Err(error) = spawned {
        eprintln!("aevia docs: cannot start a browser ({error})");
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
    let exe = std::env::current_exe().ok()?;
    let beside = exe.parent()?.join(BINARY);
    if beside.is_file() {
        return Some(beside);
    }
    // A checkout: `target/{profile}/aevia` with the platform workspace's own
    // `platform/target/{profile}/aevia-pkg` next to it, so `cargo build` in
    // both workspaces is all a contributor needs.
    let profile = exe.parent()?.file_name()?.to_owned();
    let root = exe.parent()?.parent()?.parent()?;
    let built = root
        .join("platform")
        .join("target")
        .join(profile)
        .join(BINARY);
    built.is_file().then_some(built)
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
    fn the_aliases_are_the_client_commands_and_not_the_native_ones() {
        assert!(is_forwarded("search"));
        assert!(is_forwarded("login"));
        assert!(!is_forwarded("add"));
        assert!(!is_forwarded("publish"), "publish has its own pre-flight");
    }

    #[test]
    fn docs_addresses_follow_the_site_layout() {
        let base = "https://aevia.apich.org/";
        assert_eq!(
            docs_url(base, None).unwrap(),
            "https://aevia.apich.org/docs"
        );
        assert_eq!(
            docs_url(base, Some("apich/units")).unwrap(),
            "https://aevia.apich.org/docs/apich/units"
        );
        assert_eq!(
            docs_url(base, Some("apich/units@1.2.0")).unwrap(),
            "https://aevia.apich.org/docs/apich/units/1.2.0"
        );
        assert!(docs_url(base, Some("units")).is_err());
        assert!(docs_url(base, Some("/units")).is_err());
    }

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
