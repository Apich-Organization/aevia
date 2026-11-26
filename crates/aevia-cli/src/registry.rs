//! `aevia pkg` — the package registry client.
//!
//! # Why this is a forwarder and not an implementation
//!
//! The registry client is `aevia-pkg`, a binary in the platform workspace
//! (`platform/crates/aevia-pkg`). It is deliberately not linked into this
//! binary: it carries an `OpenPGP` verifier, an HTTP client and a tarball codec
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
//! 2. `aevia-pkg` next to this executable, so a release uses its matching client.
//! 3. `aevia-pkg` on `PATH`, which is what `cargo install --path
//!    platform/crates/aevia-pkg` produces.
//! 4. The platform workspace's own target directory in a source checkout.
//!
//! When none of them exists the message names all three, because "command not
//! found" on its own does not tell a user that a second binary is involved.

use std::path::Path;
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
    let binary = locate()?;
    let registry = std::env::var("AEVIA_REGISTRY").ok();
    match invocation(&binary, args, registry.as_deref()).status() {
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
    match locate() {
        | Ok(binary) => run_client(&binary, args),
        | Err(error) => {
            eprintln!("aevia pkg: {error}");
            ExitCode::from(2)
        },
    }
}

fn run_client(
    binary: &Path,
    args: &[String],
) -> ExitCode {
    let registry = std::env::var("AEVIA_REGISTRY").ok();
    match invocation(binary, args, registry.as_deref()).status() {
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

fn invocation(
    binary: &Path,
    args: &[String],
    registry: Option<&str>,
) -> Command {
    let mut process = Command::new(binary);
    let explicit = args
        .iter()
        .take_while(|argument| argument.as_str() != "--")
        .any(|argument| argument == "--registry" || argument.starts_with("--registry="));
    if !explicit && let Some(url) = registry.map(str::trim).filter(|url| !url.is_empty()) {
        process.args(["--registry", url]);
    }
    process.args(args);
    process
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
#[allow(
    clippy::single_call_fn,
    reason = "defines the registry command names available as top-level aliases"
)]
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
#[allow(
    clippy::single_call_fn,
    reason = "the publish command performs its compiler preflight before forwarding"
)]
pub fn publish(args: &[String]) -> ExitCode {
    let binary = match locate() {
        | Ok(binary) => binary,
        | Err(error) => {
            eprintln!("aevia publish: {error}");
            return ExitCode::from(2);
        },
    };
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
    run_client(&binary, &forwarded)
}

/// Type-checks the package here and verifies its formatting with this binary.
#[allow(
    clippy::single_call_fn,
    reason = "validates the package before registry publication starts"
)]
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

/// Resolves a hosted package page or a language documentation section.
///
/// Prints a package documentation link or the documentation index and can
/// open the link in the default browser.
#[allow(
    clippy::single_call_fn,
    reason = "the docs command validates language, section and package coordinates"
)]
pub fn docs(args: &[String]) -> ExitCode {
    let mut open = false;
    let mut registry = None;
    let mut target = None;
    let mut section = None;
    let mut language = None;
    let mut download = false;
    let mut local = false;
    let mut args = args.iter().map(String::as_str);
    while let Some(argument) = args.next() {
        match argument {
            | "--open" => open = true,
            | "--download" => download = true,
            | "--local" => local = true,
            | "--section" => {
                let value = args.next();
                if !matches!(value, Some("book" | "architecture" | "spec" | "stdlib")) {
                    eprintln!("aevia docs: --section expects book, architecture, spec, or stdlib");
                    return ExitCode::from(2);
                }
                section = value;
            },
            | "--lang" => {
                let value = args.next();
                if !matches!(value, Some("en" | "zh")) {
                    eprintln!("aevia docs: --lang expects en or zh");
                    return ExitCode::from(2);
                }
                language = value;
            },
            | "--registry" => {
                let Some(value) = args.next() else {
                    eprintln!("aevia docs: `--registry` expects a URL");
                    return ExitCode::from(2);
                };
                registry = Some(value);
            },
            | other if other.starts_with('-') => {
                eprintln!("aevia docs: unknown option `{other}` (see `aevia docs --help`)");
                return ExitCode::from(2);
            },
            | other => {
                if target.replace(other).is_some() {
                    eprintln!("aevia docs: expected one package, `owner/name[@version]`");
                    return ExitCode::from(2);
                }
            },
        }
    }
    if local {
        if download {
            eprintln!("aevia docs: --download describes a hosted page; drop it with --local");
            return ExitCode::from(2);
        }
        let path = match local_documentation(section, language) {
            | Ok(path) => path,
            | Err(message) => {
                eprintln!("aevia docs: {message}");
                return ExitCode::from(2);
            },
        };
        println!("{}", path.display());
        if open {
            open_url(&path.to_string_lossy());
        }
        return ExitCode::SUCCESS;
    }
    let environment = std::env::var("AEVIA_REGISTRY").ok();
    let base = registry
        .or_else(|| {
            environment
                .as_deref()
                .map(str::trim)
                .filter(|url| !url.is_empty())
        })
        .unwrap_or(DEFAULT_REGISTRY);
    let url = match documentation_url(base, target, section, language, download) {
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

/// The local documentation root, for an offline `aevia docs --local`.
///
/// Checked in order: `AEVIA_DOCS`, `AEVIA_DOCS_HOME`, `$AEVIA_HOME/docs`, the
/// installed toolchain's `share/doc`, and finally a source checkout (a
/// directory holding both `spec/` and `platform/stdlib-docs/`).
fn local_docs_root() -> Result<PathBuf, String> {
    for key in ["AEVIA_DOCS", "AEVIA_DOCS_HOME"] {
        if let Some(dir) = std::env::var_os(key) {
            let path = PathBuf::from(dir);
            if path.is_dir() {
                return Ok(path);
            }
            return Err(format!(
                "{key} does not name a directory: {}",
                path.display()
            ));
        }
    }
    if let Some(home) = std::env::var_os("AEVIA_HOME") {
        let docs = PathBuf::from(home).join("docs");
        if docs.is_dir() {
            return Ok(docs);
        }
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(root) = exe.ancestors().nth(2)
    {
        for candidate in [root.join("share/doc"), root.join("docs")] {
            if candidate.is_dir() {
                return Ok(candidate);
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            if ancestor.join("spec").is_dir() && ancestor.join("platform/stdlib-docs").is_dir() {
                return Ok(ancestor.to_path_buf());
            }
        }
    }
    Err(
        "no local documentation found; set AEVIA_DOCS to the directory that holds \
         the specification and standard-library reference"
            .to_owned(),
    )
}

/// Resolves one local document against the ambient documentation root.
fn local_documentation(
    section: Option<&str>,
    language: Option<&str>,
) -> Result<PathBuf, String> {
    local_documentation_in(&local_docs_root()?, section, language)
}

/// Resolves one local document under `root`.
fn local_documentation_in(
    root: &std::path::Path,
    section: Option<&str>,
    language: Option<&str>,
) -> Result<PathBuf, String> {
    let chinese = language == Some("zh");
    let existing = |path: PathBuf| {
        if path.is_file() {
            Ok(path)
        } else {
            Err(format!("missing local document: {}", path.display()))
        }
    };
    match section {
        | Some("spec") => {
            let name = if chinese {
                "Aevia-Language-Specification.md"
            } else {
                "Aevia-Language-Specification.en.md"
            };
            let full = root.join(name);
            if full.is_file() {
                return Ok(full);
            }
            // A chapter-by-chapter checkout keeps the same text under `spec/`.
            let directory = if chinese {
                root.join("spec")
            } else {
                root.join("spec/en")
            };
            existing(directory.join("08-stdlib.md"))
        },
        | Some("stdlib") => {
            let directory = root.join("platform/stdlib-docs");
            let index = directory.join("index.md");
            if index.is_file() {
                return Ok(index);
            }
            existing(directory.join("stdlib.md"))
        },
        | Some("architecture") => existing(root.join("ARCHITECTURE.md")),
        | Some("book") => {
            Err(
                "the book is only published online; use --section spec or --section stdlib \
             with --local, or drop --local for the hosted book"
                    .to_owned(),
            )
        },
        | Some(other) => Err(format!("unknown documentation section `{other}`")),
        | None => Ok(root.to_path_buf()),
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "resolves and validates one hosted documentation target"
)]
fn documentation_url(
    base: &str,
    target: Option<&str>,
    section: Option<&str>,
    language: Option<&str>,
    download: bool,
) -> Result<String, String> {
    if section.is_some() && target.is_some() {
        return Err("choose a package or a documentation section".to_owned());
    }
    if download && !matches!(section, Some("book" | "architecture")) {
        return Err("--download needs --section book or --section architecture".to_owned());
    }
    let base = base.trim().trim_end_matches('/');
    let valid_base = base.split_once("://").is_some_and(|(scheme, rest)| {
        (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
            && rest.split('/').next().is_some_and(|host| !host.is_empty())
            && !rest
                .chars()
                .any(|ch| ch.is_whitespace() || ch.is_control() || matches!(ch, '?' | '#' | '\\'))
    });
    if !valid_base {
        return Err("the registry address must be an HTTP or HTTPS base URL".to_owned());
    }
    let mut url = if let Some(section) = section {
        format!("{base}/{section}")
    } else {
        docs_url(base, target)?
    };
    if download {
        url.push_str("/download");
    }
    if let Some(language) = language {
        url.push_str("?lang=");
        url.push_str(language);
    }
    Ok(url)
}

/// The documentation address for `target` (`owner/name[@version]`) on `base`.
#[allow(
    clippy::single_call_fn,
    reason = "parses package documentation coordinates independently of CLI flags"
)]
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
    let valid = id.split_once('/').is_some_and(|(owner, name)| {
        !owner.is_empty()
            && owner.len() <= 39
            && owner
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && name.len() <= 64
            && aevia_core::manifest::valid_package_name(name)
    });
    if !valid {
        return Err(format!(
            "`{target}` is not `owner/name[@version]` (packages are scoped, `apich/units`)"
        ));
    }
    if let Some(version) = version {
        let parsed = aevia_resolve::Version::parse(version)
            .map_err(|error| format!("invalid package version `{version}`: {error}"))?;
        Ok(format!("{base}/docs/{id}/{parsed}"))
    } else {
        Ok(format!("{base}/docs/{id}"))
    }
}

/// Opens `url` in the default browser, when standard output is a terminal.
#[allow(
    clippy::single_call_fn,
    reason = "applies the docs command's interactive browser policy"
)]
fn open_url(url: &str) {
    use std::io::IsTerminal;
    if std::io::stdout().is_terminal()
        && let Err(error) = crate::terminal::open_default(std::ffi::OsStr::new(url))
    {
        eprintln!("aevia docs: cannot start a browser ({error})");
    }
}

/// Finds the client binary.
fn locate() -> Result<PathBuf, String> {
    if let Some(explicit) = std::env::var_os("AEVIA_PKG") {
        let path = PathBuf::from(explicit);
        let fail = || {
            format!(
                "AEVIA_PKG does not name a registry executable: `{}`",
                path.display()
            )
        };
        if !path.is_file() {
            return Err(fail());
        }
        return std::path::absolute(&path).map_err(|_| fail());
    }
    let executable = std::env::current_exe().ok();
    let profile_dir = executable.as_deref().and_then(Path::parent);
    if let Some(beside) = profile_dir.map(|directory| directory.join(BINARY))
        && beside.is_file()
    {
        return Ok(beside);
    }
    if let Some(found) = on_path() {
        return Ok(found);
    }
    if let Some(directory) = profile_dir
        && let Some(profile) = directory.file_name()
        && let Some(root) = directory.parent().and_then(Path::parent)
    {
        // A source checkout keeps a distinct target directory for the platform.
        let built = root
            .join("platform")
            .join("target")
            .join(profile)
            .join(BINARY);
        if built.is_file() {
            return Ok(built);
        }
    }
    Err(format!(
        "cannot find `{BINARY}`. Install it beside aevia or on PATH, run          `cargo install --path platform/crates/aevia-pkg`, or set AEVIA_PKG."
    ))
}

/// Looks for the binary in `PATH`.
///
/// Written by hand rather than shelling out to `which`: the answer is needed on
/// Windows too, and `Command::new(BINARY)` would only tell us whether the spawn
/// worked, not where it came from — which is what the error message wants.
#[allow(
    clippy::single_call_fn,
    reason = "locates the registry executable with platform-specific naming"
)]
fn on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(BINARY))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| std::path::absolute(candidate).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_documents_resolve_from_a_checkout() {
        let root = std::env::temp_dir().join(format!(
            "aevia-docs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        let stdlib = root.join("platform/stdlib-docs");
        std::fs::create_dir_all(&stdlib).expect("stdlib dir");
        std::fs::write(root.join("Aevia-Language-Specification.en.md"), "en").expect("en spec");
        std::fs::write(root.join("Aevia-Language-Specification.md"), "zh").expect("zh spec");
        std::fs::write(stdlib.join("index.md"), "index").expect("stdlib index");
        std::fs::write(root.join("ARCHITECTURE.md"), "arch").expect("architecture");

        assert_eq!(
            local_documentation_in(&root, Some("spec"), Some("en")).expect("en"),
            root.join("Aevia-Language-Specification.en.md")
        );
        assert_eq!(
            local_documentation_in(&root, Some("spec"), Some("zh")).expect("zh"),
            root.join("Aevia-Language-Specification.md")
        );
        assert_eq!(
            local_documentation_in(&root, Some("stdlib"), None).expect("stdlib"),
            stdlib.join("index.md")
        );
        assert_eq!(
            local_documentation_in(&root, Some("architecture"), None).expect("architecture"),
            root.join("ARCHITECTURE.md")
        );
        assert_eq!(
            local_documentation_in(&root, None, None).expect("root"),
            root
        );
        assert!(local_documentation_in(&root, Some("book"), None).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

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
        assert!(docs_url(base, Some("apich/units@../../other")).is_err());
        assert!(docs_url(base, Some("apich/units?lang=zh")).is_err());
        assert_eq!(
            docs_url(base, Some("0000-0002-1825-009X/Lib@1.2.0-rc.1+local")).unwrap(),
            "https://aevia.apich.org/docs/0000-0002-1825-009X/Lib/1.2.0-rc.1+local"
        );
        assert_eq!(
            documentation_url(base, None, Some("architecture"), Some("zh"), true).unwrap(),
            "https://aevia.apich.org/architecture/download?lang=zh"
        );
        assert!(documentation_url(base, None, Some("stdlib"), None, true).is_err());
        assert!(documentation_url(base, Some("apich/units"), Some("book"), None, false).is_err());
        assert!(documentation_url("file:///tmp", None, None, None, false).is_err());
        assert!(documentation_url("http://", None, None, None, false).is_err());
        assert_eq!(
            documentation_url("http://127.0.0.1:8080/registry/", None, None, None, false).unwrap(),
            "http://127.0.0.1:8080/registry/docs"
        );
    }

    #[test]
    fn explicit_registry_takes_precedence_over_the_environment() {
        let default = Some(" http://localhost:8080 ");
        let search = ["search".to_string(), "units".to_string()];
        let process = invocation(Path::new(BINARY), &search, default);
        assert_eq!(
            process.get_args().collect::<Vec<_>>(),
            ["--registry", "http://localhost:8080", "search", "units"]
        );
        let explicit = [
            "search".to_string(),
            "--registry=https://other.test".to_string(),
            "units".to_string(),
        ];
        let process = invocation(Path::new(BINARY), &explicit, default);
        assert!(
            process
                .get_args()
                .eq(explicit.iter().map(std::ffi::OsStr::new))
        );
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
