//! `~/.aevia/config.toml` — the only state `aevia-pkg` keeps.
//!
//! # What is *not* in here
//!
//! No key material, no session token, no password. The whole file is a
//! registry URL, a fingerprint and the kind of key that fingerprint belongs to.
//! That is the entire point of a signature-based registry: the credential is
//! the private key, it stays in `gpg-agent` or `ssh-agent`, and the worst a
//! stolen config file can do is tell someone which key you sign with — which
//! is already public on the registry.
//!
//! # Why it is parsed by hand
//!
//! The accepted format is exactly what [`Config::render`] writes: `key =
//! "value"` lines with `#` comments. A real TOML parser would be a dependency
//! for four keys and would accept documents this tool then cannot round-trip.
//! Reading only the subset it writes means a config file either means what it
//! says or is rejected with the line number.
//!
//! `AEVIA_PKG_HOME` overrides the directory, which is how the tests exercise
//! the file without touching a developer's real `~/.aevia`.

use std::path::Path;
use std::path::PathBuf;

use aevia_registry_core::signature::KeyKind;

use crate::error::CliError;

/// Where the client goes when neither `--registry` nor the config says.
///
/// Hard-coded rather than discovered: a client that silently talked to a
/// different registry depending on the machine's environment would make "I
/// published it" and "you cannot download it" impossible to debug.
pub const DEFAULT_REGISTRY: &str = "https://aevia.apich.org";

/// The environment variable that relocates the config directory.
pub const HOME_ENV: &str = "AEVIA_PKG_HOME";

/// The client's persistent settings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    /// The registry base URL, without a trailing slash.
    pub registry: Option<String>,
    /// The fingerprint of the key `publish`/`yank`/`login` use by default.
    pub default_key: Option<String>,
    /// Which signature system [`Config::default_key`] belongs to.
    pub default_key_kind: Option<KeyKind>,
}

impl Config {
    /// The registry to talk to, honouring `--registry` over the file.
    #[must_use]
    pub fn registry(
        &self,
        override_url: Option<&str>,
    ) -> String {
        override_url
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .or(self.registry.as_deref())
            .unwrap_or(DEFAULT_REGISTRY)
            .trim_end_matches('/')
            .to_string()
    }

    /// Parses a config file.
    ///
    /// # Errors
    ///
    /// `E_CONFIG` for an unquoted value, an unknown key, or a key kind that is
    /// neither `openpgp` nor `ssh`. An unknown key is refused rather than
    /// ignored: a publisher who wrote `default-key` (hyphen) would otherwise
    /// silently get the old key and publish under the wrong identity.
    pub fn parse(
        text: &str,
        origin: &str,
    ) -> Result<Self, CliError> {
        let mut config = Self::default();
        for (index, raw) in text.lines().enumerate() {
            let number = index + 1;
            let line = raw.trim_end_matches('\r').trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(fail(
                    origin,
                    number,
                    format!("expected `key = \"value\"`, found `{line}`"),
                ));
            };
            let key = key.trim();
            let value = unquote(value.trim())
                .ok_or_else(|| fail(origin, number, format!("`{key}` expects a quoted string")))?;
            match key {
                | "registry" => config.registry = Some(value.trim_end_matches('/').to_string()),
                | "default_key" => config.default_key = Some(value),
                | "default_key_kind" => {
                    let kind = KeyKind::parse(&value).ok_or_else(|| {
                        fail(
                            origin,
                            number,
                            format!("`{value}` is not a key kind; write `openpgp` or `ssh`"),
                        )
                    })?;
                    config.default_key_kind = Some(kind);
                },
                | other => {
                    return Err(fail(
                        origin,
                        number,
                        format!(
                            "unknown key `{other}`; this file holds `registry`, `default_key` \
                             and `default_key_kind`"
                        ),
                    ));
                },
            }
        }
        Ok(config)
    }

    /// Renders the file in the one form [`Config::parse`] accepts.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("# aevia-pkg — written by `aevia-pkg login`.\n");
        out.push_str("# Holds no secrets: a fingerprint is public, and the private key stays\n");
        out.push_str("# in gpg-agent or ssh-agent.\n");
        out.push_str(&format!(
            "registry = \"{}\"\n",
            self.registry.as_deref().unwrap_or(DEFAULT_REGISTRY)
        ));
        if let Some(fingerprint) = &self.default_key {
            out.push_str(&format!("default_key = \"{fingerprint}\"\n"));
        }
        if let Some(kind) = self.default_key_kind {
            out.push_str(&format!("default_key_kind = \"{}\"\n", kind.as_str()));
        }
        out
    }

    /// Reads the config, treating "no file yet" as "all defaults".
    ///
    /// # Errors
    ///
    /// `E_CONFIG` when the file exists but does not parse, or cannot be read.
    pub fn load(home: &Path) -> Result<Self, CliError> {
        let path = home.join("config.toml");
        match std::fs::read_to_string(&path) {
            | Ok(text) => Self::parse(&text, &path.display().to_string()),
            | Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            | Err(error) => Err(CliError::new(format!(
                "cannot read `{}`: {error}",
                path.display()
            ))),
        }
    }

    /// Writes the config, creating `home` and restricting the file to its owner.
    ///
    /// The mode is `0600` even though nothing secret is stored: the file names
    /// the key that speaks for the account, and on a shared machine another
    /// user's ability to rewrite it is a way to redirect a publish.
    ///
    /// # Errors
    ///
    /// `E_CONFIG` when the directory or file cannot be written.
    pub fn save(
        &self,
        home: &Path,
    ) -> Result<PathBuf, CliError> {
        std::fs::create_dir_all(home).map_err(|error| {
            CliError::new(format!("cannot create `{}`: {error}", home.display()))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(home)
                .map_err(|error| {
                    CliError::new(format!("cannot stat `{}`: {error}", home.display()))
                })?
                .permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(home, permissions).map_err(|error| {
                CliError::new(format!("cannot restrict `{}`: {error}", home.display()))
            })?;
        }
        let path = home.join("config.toml");
        std::fs::write(&path, self.render()).map_err(|error| {
            CliError::new(format!("cannot write `{}`: {error}", path.display()))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&path)
                .map_err(|error| {
                    CliError::new(format!("cannot stat `{}`: {error}", path.display()))
                })?
                .permissions();
            permissions.set_mode(0o600);
            std::fs::set_permissions(&path, permissions).map_err(|error| {
                CliError::new(format!("cannot restrict `{}`: {error}", path.display()))
            })?;
        }
        Ok(path)
    }
}

/// The directory the config lives in: `$AEVIA_PKG_HOME` or `~/.aevia`.
///
/// # Errors
///
/// `E_CONFIG` when neither the override nor a home directory is known, which
/// happens in a bare container with no `HOME` and no `USERPROFILE`.
pub fn home() -> Result<PathBuf, CliError> {
    home_from(
        std::env::var_os(HOME_ENV),
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")),
    )
}

/// [`home`] with its inputs passed in, so the decision is testable.
///
/// # Errors
///
/// `E_CONFIG` when there is nothing to build a path from.
pub fn home_from(
    override_dir: Option<std::ffi::OsString>,
    home_dir: Option<std::ffi::OsString>,
) -> Result<PathBuf, CliError> {
    if let Some(dir) = override_dir.filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let Some(home_dir) = home_dir.filter(|dir| !dir.is_empty()) else {
        return Err(CliError::new(
            "cannot find a home directory; set AEVIA_PKG_HOME",
        ));
    };
    Ok(PathBuf::from(home_dir).join(".aevia"))
}

fn unquote(value: &str) -> Option<String> {
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    if inner.contains('"') {
        return None;
    }
    Some(inner.to_string())
}

fn fail(
    origin: &str,
    number: usize,
    message: impl std::fmt::Display,
) -> CliError {
    CliError::new(format!("E_CONFIG: {origin}:{number}: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_config_round_trips_through_its_own_render() {
        let config = Config {
            registry: Some("https://example.test".to_string()),
            default_key: Some("0123456789ABCDEF".to_string()),
            default_key_kind: Some(KeyKind::OpenPgp),
        };
        let text = config.render();
        assert_eq!(Config::parse(&text, "config.toml").expect("parses"), config);
    }

    #[test]
    fn an_empty_config_file_is_all_defaults() {
        let config = Config::parse("# nothing yet\n", "config.toml").expect("parses");
        assert_eq!(config, Config::default());
        assert_eq!(config.registry(None), DEFAULT_REGISTRY);
    }

    #[test]
    fn the_command_line_beats_the_file() {
        let config = Config {
            registry: Some("https://file.test".to_string()),
            ..Config::default()
        };
        assert_eq!(config.registry(None), "https://file.test");
        assert_eq!(
            config.registry(Some("https://flag.test/")),
            "https://flag.test"
        );
    }

    #[test]
    fn a_misspelled_key_is_refused_rather_than_ignored() {
        let error = Config::parse("default-key = \"x\"\n", "config.toml").expect_err("refused");
        assert!(error.message.contains("unknown key"), "{}", error.message);
    }

    #[test]
    fn an_unknown_key_kind_is_refused() {
        let error = Config::parse("default_key_kind = \"pgp2\"\n", "c").expect_err("refused");
        assert!(error.message.contains("openpgp"), "{}", error.message);
    }

    #[test]
    fn the_home_directory_honours_the_override() {
        let path = home_from(
            Some(std::ffi::OsString::from("/tmp/aevia-test")),
            Some(std::ffi::OsString::from("/home/apich")),
        )
        .expect("home");
        assert_eq!(path, PathBuf::from("/tmp/aevia-test"));
        let path = home_from(None, Some(std::ffi::OsString::from("/home/apich"))).expect("home");
        assert_eq!(path, PathBuf::from("/home/apich").join(".aevia"));
    }

    #[test]
    fn a_missing_home_is_an_error_not_a_panic() {
        let error = home_from(None, None).expect_err("no home");
        assert!(
            error.message.contains("AEVIA_PKG_HOME"),
            "{}",
            error.message
        );
    }

    #[test]
    fn saving_and_loading_agree() {
        let dir = std::env::temp_dir().join(format!("aevia-pkg-config-{}", std::process::id()));
        let config = Config {
            registry: Some("https://example.test".to_string()),
            default_key: Some("ABCD".to_string()),
            default_key_kind: Some(KeyKind::Ssh),
        };
        let path = config.save(&dir).expect("saved");
        assert!(path.is_file());
        assert_eq!(Config::load(&dir).expect("loaded"), config);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
