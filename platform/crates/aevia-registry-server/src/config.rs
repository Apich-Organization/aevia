//! Configuration, read once at start-up.
//!
//! Every value has a default that works in a container, and the two that cannot
//! have one — the database URL and the session secret — are refused rather than
//! guessed. A server that boots with a random session secret would silently log
//! everyone out on every restart; a server that boots without a database would
//! serve 500s until someone noticed.

use std::net::SocketAddr;
use std::path::PathBuf;

/// A configuration problem, with the variable that caused it.
#[derive(Debug)]
pub struct ConfigError {
    /// What is wrong.
    pub message: String,
}

impl std::fmt::Display for ConfigError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ConfigError {}

/// The GitHub App, when one is configured.
#[derive(Clone)]
pub struct GithubConfig {
    /// The App's client id, used for the user-authorisation flow.
    pub client_id: String,
    /// The App's client secret.
    pub client_secret: String,
}

/// The ORCID OAuth client, when one is configured.
#[derive(Clone)]
pub struct OrcidConfig {
    /// The client id.
    pub client_id: String,
    /// The client secret.
    pub client_secret: String,
    /// `https://orcid.org` or the sandbox.
    pub base: String,
}

/// Everything the server needs that is not a database row.
#[derive(Clone)]
pub struct Config {
    /// Where to listen.
    pub bind: SocketAddr,
    /// The public origin, without a trailing slash.
    pub base_url: String,
    /// The PostgreSQL connection string.
    pub database_url: String,
    /// Where archives are stored.
    pub storage_dir: PathBuf,
    /// Where the language specification's Markdown chapters live.
    pub spec_dir: PathBuf,
    /// The HMAC key for session cookies.
    pub session_secret: Vec<u8>,
    /// The name the site shows.
    pub registry_name: String,
    /// Whether documentation is built at publish time.
    pub docs_enabled: bool,
    /// Whether `X-Forwarded-For` is trusted.
    pub trust_proxy: bool,
    /// The GitHub App.
    pub github: Option<GithubConfig>,
    /// The ORCID client.
    pub orcid: Option<OrcidConfig>,
}

impl Config {
    /// Reads the environment.
    ///
    /// # Errors
    ///
    /// A [`ConfigError`] naming the variable that is missing or malformed.
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = required("DATABASE_URL")?;
        let session_secret = {
            let raw = required("AEVIA_SESSION_SECRET")?;
            let bytes = hex::decode(raw.trim()).map_err(|_| ConfigError {
                message:
                    "AEVIA_SESSION_SECRET must be hex; generate it with `openssl rand -hex 32`"
                        .to_string(),
            })?;
            if bytes.len() < 32 {
                return Err(ConfigError {
                    message: "AEVIA_SESSION_SECRET must be at least 32 bytes (64 hex characters); \
                              generate it with `openssl rand -hex 32`"
                        .to_string(),
                });
            }
            bytes
        };
        let bind = optional("AEVIA_BIND", "0.0.0.0:8080")
            .parse::<SocketAddr>()
            .map_err(|error| ConfigError {
                message: format!("AEVIA_BIND is not an address: {error}"),
            })?;
        let base_url = optional("AEVIA_BASE_URL", "https://aevia.apich.org")
            .trim_end_matches('/')
            .to_string();
        let storage_dir = PathBuf::from(optional("AEVIA_STORAGE_DIR", "/var/lib/aevia/packages"));
        // Defaults to the language repository checked out next to this one,
        // which is where a developer runs the server from.
        let spec_dir = PathBuf::from(optional("AEVIA_SPEC_DIR", "../spec"));
        let github = match (
            var("AEVIA_GITHUB_CLIENT_ID"),
            var("AEVIA_GITHUB_CLIENT_SECRET"),
        ) {
            | (Some(client_id), Some(client_secret)) => Some(GithubConfig {
                client_id,
                client_secret,
            }),
            | _ => None,
        };
        let orcid = match (
            var("AEVIA_ORCID_CLIENT_ID"),
            var("AEVIA_ORCID_CLIENT_SECRET"),
        ) {
            | (Some(client_id), Some(client_secret)) => Some(OrcidConfig {
                client_id,
                client_secret,
                base: optional("AEVIA_ORCID_BASE", "https://orcid.org")
                    .trim_end_matches('/')
                    .to_string(),
            }),
            | _ => None,
        };
        Ok(Self {
            bind,
            base_url,
            database_url,
            storage_dir,
            spec_dir,
            session_secret,
            registry_name: optional("AEVIA_REGISTRY_NAME", "aevia"),
            docs_enabled: flag("AEVIA_DOCS_ENABLED", true),
            trust_proxy: flag("AEVIA_TRUST_PROXY", false),
            github,
            orcid,
        })
    }

    /// The absolute URL of a site path.
    #[must_use]
    pub fn url(
        &self,
        path: &str,
    ) -> String {
        format!("{}{}", self.base_url, path)
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn required(name: &str) -> Result<String, ConfigError> {
    var(name).ok_or_else(|| ConfigError {
        message: format!("{name} is required; see infra/.env.example"),
    })
}

fn optional(
    name: &str,
    default: &str,
) -> String {
    var(name).unwrap_or_else(|| default.to_string())
}

fn flag(
    name: &str,
    default: bool,
) -> bool {
    match var(name).as_deref() {
        | Some("1" | "true" | "yes" | "on") => true,
        | Some("0" | "false" | "no" | "off") => false,
        | _ => default,
    }
}
