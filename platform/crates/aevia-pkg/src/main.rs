//! `aevia-pkg` — the command-line client for the Aevia package registry.
//!
//! This is the counterpart to `cargo publish`, with one structural difference:
//! **there are no API tokens**. Every authenticated action is a detached
//! signature over a short, server-issued statement, made with the publisher's
//! own OpenPGP or SSH key. The private key never leaves the machine, nothing
//! long-lived is stored, and a captured signature is worthless within five
//! minutes because the statement it covers names one archive and carries a
//! single-use nonce.
//!
//! The commands, in the order a new publisher meets them:
//!
//! * `login` binds a key to a GitHub or ORCID identity through a device code;
//! * `key list` / `key show` report which keys this machine can sign with;
//! * `publish` packs the current package, gets a statement, checks it, signs it
//!   and uploads the signed archive;
//! * `yank` withdraws a version with the same signature machinery;
//! * `search` / `info` read the registry;
//! * `add` / `verify` download a version and check the archive against the
//!   signature and the registered key before trusting it;
//! * `status` says what this machine can do and as whom.
//!
//! Two conventions hold throughout: **answers on stdout, diagnostics on
//! stderr**, so `--json` pipes cleanly; and **exit `2` for a wrong invocation
//! or a missing tool, `1` for a failed attempt**, so a script can tell "fix your
//! command line" from "try again".

#![forbid(unsafe_code)]

mod api;
mod auth;
mod config;
mod error;
mod fetch;
mod keys;
mod package;
mod publish;
mod query;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use clap::Subcommand;

use crate::api::Client;
use crate::config::Config;
use crate::error::CliError;

/// Everything a command needs, assembled once.
///
/// The config is loaded and the HTTP client built before dispatch, so a bad
/// `--registry` or an unreadable config fails in one place instead of in every
/// command. `json` and `quiet` live here because they are global flags that
/// change how a command reports, not what it does.
pub struct App {
    /// The registry this invocation talks to.
    pub client: Client,
    /// The settings read from `~/.aevia/config.toml`.
    pub config: Config,
    /// The directory the config lives in, for `login` and `status`.
    pub home: PathBuf,
    /// `--json`: emit machine-readable output where a command supports it.
    pub json: bool,
    /// `--quiet`: suppress progress and warnings.
    pub quiet: bool,
}

#[derive(Debug, Parser)]
#[command(
    name = "aevia-pkg",
    version,
    about = "Publish, fetch and verify signed Aevia packages",
    long_about = "Publish, fetch and verify signed Aevia packages.\n\n\
                  There are no API tokens: every authenticated action is a signature made \
                  with your own OpenPGP or SSH key.",
    disable_help_subcommand = true
)]
struct Cli {
    /// The registry root, `https://aevia.apich.org` by default.
    #[arg(long, global = true, value_name = "URL")]
    registry: Option<String>,

    /// Machine-readable output for `search`, `info` and `status`.
    #[arg(long, global = true)]
    json: bool,

    /// Suppress progress and warnings.
    #[arg(long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Bind a signing key to your account through a browser.
    Login {
        /// The fingerprint of the key to bind.
        #[arg(long, value_name = "FINGERPRINT")]
        key: Option<String>,
        /// An SSH key file, either the public or the private half.
        #[arg(long, value_name = "PATH")]
        ssh: Option<PathBuf>,
        /// A label for this machine, shown on your account page.
        #[arg(long, value_name = "TEXT")]
        label: Option<String>,
    },

    /// Inspect the signing keys on this machine.
    Key {
        #[command(subcommand)]
        command: KeyCommand,
    },

    /// Pack the package here, sign it and upload it.
    Publish {
        /// The fingerprint of the key to sign with.
        #[arg(long, value_name = "FINGERPRINT")]
        key: Option<String>,
        /// One sentence describing the package.
        #[arg(long, value_name = "TEXT")]
        description: Option<String>,
        /// An SPDX identifier, `Apache-2.0`.
        #[arg(long, value_name = "SPDX")]
        license: Option<String>,
        /// A keyword; repeat for up to five.
        #[arg(long = "keyword", value_name = "K")]
        keyword: Vec<String>,
        /// Where the source lives.
        #[arg(long, value_name = "URL")]
        repository: Option<String>,
        /// Print the statement and the checksum, sign nothing.
        #[arg(long)]
        dry_run: bool,
        /// Publish even though the git tree is not clean.
        #[arg(long)]
        allow_dirty: bool,
    },

    /// Withdraw a version, or restore one.
    Yank {
        /// `owner/name@version`.
        target: String,
        /// Restore a yanked version instead of yanking it.
        #[arg(long)]
        undo: bool,
    },

    /// Search the registry.
    Search {
        /// What to look for.
        query: String,
        /// How to order the results.
        #[arg(long, value_enum, default_value = "relevance")]
        sort: query::Sort,
        /// The page, one-based.
        #[arg(long, default_value_t = 1)]
        page: u32,
    },

    /// Show one package's page.
    Info {
        /// `owner/name`.
        target: String,
    },

    /// Download, verify and vendor a version, and print the manifest line.
    Add {
        /// `owner/name[@version]`; the newest version when none is given.
        target: String,
        /// The package directory to vendor into, the current one by default.
        #[arg(long, value_name = "DIR")]
        dir: Option<PathBuf>,
    },

    /// Download one version and check it against its signature.
    Verify {
        /// `owner/name@version`.
        target: String,
    },

    /// Report the registry, the key and the package in this directory.
    Status,
}

#[derive(Debug, Subcommand)]
enum KeyCommand {
    /// List the secret keys this machine holds.
    List,
    /// Show one key, and whether the registry has it bound.
    Show {
        /// The fingerprint to show, the default key by default.
        #[arg(long, value_name = "FINGERPRINT")]
        key: Option<String>,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.quiet);
    match run(cli).await {
        | Ok(()) => ExitCode::SUCCESS,
        | Err(error) => {
            error.report();
            error.exit_code()
        },
    }
}

/// Builds the context and dispatches one command.
async fn run(cli: Cli) -> Result<(), CliError> {
    let home = config::home()?;
    let (config, warning) = query::load_config(&home);
    if let Some(warning) = warning {
        // A config file that does not parse is reported and then ignored: every
        // command still has a registry (the default) and a key (whatever the
        // machine holds), so the tool stays usable while the file is fixed.
        eprintln!("aevia-pkg: {warning}");
    }
    let registry = config.registry(cli.registry.as_deref());
    let client = Client::new(&registry)?;
    let app = App {
        client,
        config,
        home,
        json: cli.json,
        quiet: cli.quiet,
    };

    match cli.command {
        | Command::Login { key, ssh, label } => {
            auth::login(&app, key.as_deref(), ssh.as_deref(), label.as_deref()).await
        },
        | Command::Key { command } => {
            match command {
                | KeyCommand::List => query::key_list(&app),
                | KeyCommand::Show { key } => query::key_show(&app, key.as_deref()).await,
            }
        },
        | Command::Publish {
            key,
            description,
            license,
            keyword,
            repository,
            dry_run,
            allow_dirty,
        } => {
            let options = publish::Options {
                key: key.as_deref(),
                description: description.as_deref(),
                license: license.as_deref(),
                keywords: &keyword,
                repository: repository.as_deref(),
                dry_run,
                allow_dirty,
            };
            publish::run(&app, &options).await
        },
        | Command::Yank { target, undo } => publish::yank(&app, &target, undo).await,
        | Command::Search {
            query: text,
            sort,
            page,
        } => query::search(&app, &text, sort, page).await,
        | Command::Info { target } => {
            let (id, version) = package::parse_target(&target)?;
            if version.is_some() {
                return Err(CliError::usage(format!(
                    "`{target}` names a version; `aevia-pkg info` describes the whole package"
                ))
                .with_next_step(
                    "run `aevia-pkg verify <owner/name>@<version>` to check one version",
                ));
            }
            query::info(&app, &id).await
        },
        | Command::Add { target, dir } => fetch::add(&app, &target, dir.as_deref()).await,
        | Command::Verify { target } => fetch::verify_command(&app, &target).await,
        | Command::Status => query::status(&app).await,
    }
}

/// Sets up diagnostics on stderr, off unless asked for.
///
/// `aevia-pkg` prints everything a publisher needs on stdout or stderr
/// directly; tracing is for the questions only a bug report raises — which
/// request went where — and is therefore silent unless `AEVIA_PKG_LOG` (or
/// `--quiet`, which turns it off) says otherwise.
fn init_tracing(quiet: bool) {
    let filter = if quiet {
        "off".to_string()
    } else {
        std::env::var("AEVIA_PKG_LOG").unwrap_or_else(|_| "warn".to_string())
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(std::io::stderr)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(args)
    }

    #[test]
    fn the_command_tree_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn a_bare_invocation_needs_a_command() {
        assert!(parse(&["aevia-pkg"]).is_err());
    }

    #[test]
    fn global_flags_work_before_and_after_the_subcommand() {
        let cli = parse(&["aevia-pkg", "--json", "status"]).expect("parses");
        assert!(cli.json);
        let cli = parse(&["aevia-pkg", "status", "--json"]).expect("parses");
        assert!(cli.json);
    }

    #[test]
    fn search_takes_a_sort_and_a_page() {
        let cli = parse(&[
            "aevia-pkg",
            "search",
            "linear algebra",
            "--sort",
            "downloads",
            "--page",
            "2",
        ])
        .expect("parses");
        match cli.command {
            | Command::Search { query, sort, page } => {
                assert_eq!(query, "linear algebra");
                assert_eq!(sort, query::Sort::Downloads);
                assert_eq!(page, 2);
            },
            | other => panic!("expected search, got {other:?}"),
        }
    }

    #[test]
    fn search_defaults_to_relevance_on_page_one() {
        let cli = parse(&["aevia-pkg", "search", "linear"]).expect("parses");
        match cli.command {
            | Command::Search { sort, page, .. } => {
                assert_eq!(sort, query::Sort::Relevance);
                assert_eq!(page, 1);
            },
            | other => panic!("expected search, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_sort_is_a_usage_error() {
        let error = parse(&["aevia-pkg", "search", "x", "--sort", "popular"]).expect_err("refused");
        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
    }

    #[test]
    fn publish_collects_repeated_keywords() {
        let cli = parse(&[
            "aevia-pkg",
            "publish",
            "--keyword",
            "linear",
            "--keyword",
            "math",
            "--dry-run",
        ])
        .expect("parses");
        match cli.command {
            | Command::Publish { keyword, dry_run, .. } => {
                assert_eq!(keyword, ["linear", "math"]);
                assert!(dry_run);
            },
            | other => panic!("expected publish, got {other:?}"),
        }
    }

    #[test]
    fn yank_takes_a_target_and_an_undo_flag() {
        let cli =
            parse(&["aevia-pkg", "yank", "apich/units-extra@0.2.0", "--undo"]).expect("parses");
        match cli.command {
            | Command::Yank { target, undo } => {
                assert_eq!(target, "apich/units-extra@0.2.0");
                assert!(undo);
            },
            | other => panic!("expected yank, got {other:?}"),
        }
    }

    #[test]
    fn login_takes_a_key_an_ssh_file_and_a_label() {
        let cli = parse(&[
            "aevia-pkg",
            "login",
            "--ssh",
            "/home/apich/.ssh/id_ed25519",
            "--label",
            "work laptop",
        ])
        .expect("parses");
        match cli.command {
            | Command::Login { key, ssh, label } => {
                assert!(key.is_none());
                assert_eq!(
                    ssh.as_deref(),
                    Some(std::path::Path::new("/home/apich/.ssh/id_ed25519"))
                );
                assert_eq!(label.as_deref(), Some("work laptop"));
            },
            | other => panic!("expected login, got {other:?}"),
        }
    }

    #[test]
    fn add_takes_an_optional_directory() {
        let cli = parse(&[
            "aevia-pkg",
            "add",
            "apich/units-extra@0.2.0",
            "--dir",
            "pkg",
        ])
        .expect("parses");
        match cli.command {
            | Command::Add { target, dir } => {
                assert_eq!(target, "apich/units-extra@0.2.0");
                assert_eq!(dir.as_deref(), Some(std::path::Path::new("pkg")));
            },
            | other => panic!("expected add, got {other:?}"),
        }
    }

    #[test]
    fn a_registry_override_is_kept_verbatim() {
        let cli =
            parse(&["aevia-pkg", "--registry", "https://example.test", "status"]).expect("parses");
        assert_eq!(cli.registry.as_deref(), Some("https://example.test"));
    }

    #[test]
    fn the_key_subcommand_has_two_verbs() {
        assert!(matches!(
            parse(&["aevia-pkg", "key", "list"]).map(|cli| cli.command),
            Ok(Command::Key {
                command: KeyCommand::List
            })
        ));
        assert!(matches!(
            parse(&["aevia-pkg", "key", "show"]).map(|cli| cli.command),
            Ok(Command::Key {
                command: KeyCommand::Show { key: None }
            })
        ));
    }
}
