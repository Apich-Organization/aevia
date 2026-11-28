//! The read-only commands: `search`, `info`, `status`, `key list`, `key show`.
//!
//! # Two audiences, one code path
//!
//! Every command here can be read by a person or piped into `jq`. The rule that
//! makes both work is the same everywhere: the answer goes to stdout, in either
//! prose or JSON, and *everything else* — warnings, "asking the registry…",
//! notices about a missing tool — goes to stderr. A `--json` document is
//! therefore never polluted by a diagnostic, and a human never loses one.
//!
//! # Why `status` is defensive
//!
//! `status` is what a publisher runs when something is already wrong, so it
//! must not fail because one probe failed. A missing `gpg`, an unreachable
//! registry or a directory that is not a package each degrade one line of the
//! report instead of aborting it.

use std::path::Path;

use aevia_registry_core::vm::CratePage;
use aevia_registry_core::vm::human_bytes;

use crate::App;
use crate::api;
use crate::config::Config;
use crate::error::CliError;
use crate::keys;
use crate::package;

/// The orderings `/api/v1/search` accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Sort {
    /// Best textual match first.
    Relevance,
    /// Most-downloaded first.
    Downloads,
    /// Newest release first.
    Recent,
    /// Alphabetical by `owner/name`.
    Name,
}

impl Sort {
    /// The wire name, which is the spelling the server documents.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            | Self::Relevance => "relevance",
            | Self::Downloads => "downloads",
            | Self::Recent => "recent",
            | Self::Name => "name",
        }
    }
}

/// `aevia-pkg search <query>`.
///
/// # Errors
///
/// The registry's error, or a JSON failure when serialising the answer.
#[allow(
    clippy::single_call_fn,
    reason = "the search command owns its pagination parameters and output schema"
)]
pub async fn search(
    app: &App,
    query: &str,
    sort: Sort,
    page: u32,
) -> Result<(), CliError> {
    let list = app.client.search(query, sort.as_str(), page).await?;
    if app.json {
        print_json(&list)?;
        return Ok(());
    }
    if list.items.is_empty() {
        println!("no packages match `{query}`");
        return Ok(());
    }
    println!(
        "{} result{} for `{query}`, page {} of {} (sorted by {})",
        list.total,
        if list.total == 1 { "" } else { "s" },
        list.page.max(1),
        list.last_page(),
        list.sort
    );
    for item in &list.items {
        println!();
        println!("{} {}", item.id, item.version);
        if let Some(description) = &item.description {
            println!("  {description}");
        }
        println!(
            "  {} download{}, updated {}",
            item.downloads,
            if item.downloads == 1 {
                ""
            } else {
                "s"
            },
            item.updated_at
        );
    }
    Ok(())
}

/// `aevia-pkg info <owner/name>`.
///
/// # Errors
///
/// The registry's error, or a JSON failure when serialising the answer.
#[allow(
    clippy::single_call_fn,
    reason = "the info command selects human-readable or JSON package metadata"
)]
pub async fn info(
    app: &App,
    id: &aevia_registry_core::PackageId,
) -> Result<(), CliError> {
    let page = app.client.crate_page(id.owner(), id.name()).await?;
    if app.json {
        print_json(&page)?;
        return Ok(());
    }
    print_page(&page);
    Ok(())
}

/// Prints a package page for a person.
#[allow(
    clippy::single_call_fn,
    reason = "human package details have their own stable field labels and ordering"
)]
fn print_page(page: &CratePage) {
    println!("{} {}", page.id, page.selected.version);
    if let Some(description) = &page.description {
        println!("{description}");
    }
    println!();
    if let Some(edition) = &page.edition {
        println!("edition     {edition}");
    }
    if !page.authors.is_empty() {
        println!("authors     {}", page.authors.join(", "));
    }
    if let Some(license) = &page.license {
        println!("license     {license}");
    } else if let Some(license_file) = &page.license_file {
        println!("license     file:{license_file}");
    }
    if let Some(repository) = &page.repository {
        println!("repository  {repository}");
    }
    if let Some(homepage) = &page.homepage {
        println!("homepage    {homepage}");
    }
    if let Some(documentation) = &page.documentation {
        println!("docs        {documentation}");
    }
    if !page.keywords.is_empty() {
        println!("keywords    {}", page.keywords.join(", "));
    }
    if !page.categories.is_empty() {
        println!("categories  {}", page.categories.join(", "));
    }
    println!(
        "downloads   {} across {} version{}",
        page.downloads,
        page.versions.len(),
        if page.versions.len() == 1 {
            ""
        } else {
            "s"
        }
    );
    println!("created     {}", page.created_at);
    println!("updated     {}", page.updated_at);
    if !page.owners.is_empty() {
        let owners: Vec<String> = page
            .owners
            .iter()
            .map(|owner| format!("{} ({})", owner.handle, owner.kind))
            .collect();
        println!("owners      {}", owners.join(", "));
    }
    println!();
    println!("versions");
    for version in &page.versions {
        let flags = match (version.yanked, version.docs) {
            | (true, true) => "  yanked, documented",
            | (true, false) => "  yanked",
            | (false, true) => "  documented",
            | (false, false) => "",
        };
        println!(
            "  {:<10} {:<10} {} published {}{flags}",
            version.version,
            human_bytes(version.size),
            version.checksum,
            version.published_at
        );
    }
    if !page.dependencies.is_empty() {
        println!();
        println!("dependencies");
        for dependency in &page.dependencies {
            println!(
                "  {} = {} {}",
                dependency.import, dependency.package, dependency.req
            );
        }
    }
}

/// `aevia-pkg key list`.
///
/// # Errors
///
/// `E_KEY` when a key file cannot be read. A missing `gpg` is a warning, not a
/// failure: an SSH-only publisher still gets their list.
#[allow(
    clippy::single_call_fn,
    reason = "the key list command owns local discovery and output formatting"
)]
pub fn key_list(app: &App) -> Result<(), CliError> {
    let keys = keys::discover(None)?;
    if keys.is_empty() {
        println!("no signing keys found");
        println!(
            "create one with `gpg --quick-generate-key \"You <you@example.com>\" ed25519 sign 2y`"
        );
        return Ok(());
    }
    for key in &keys {
        let default = app
            .config
            .default_key
            .as_deref()
            .is_some_and(|fingerprint| {
                keys::find(std::slice::from_ref(key), fingerprint).is_some()
            });
        print!("{}  {}", key.kind.as_str(), key.fingerprint);
        if default {
            print!("  (default)");
        }
        println!();
        if !key.identity.is_empty() {
            println!("    {}", key.identity);
        }
        if let Some(reason) = key.unsupported_reason() {
            println!("    unusable: {reason}");
        } else if let Some(warning) = key.kind.preference_warning() {
            println!("    note: {warning}");
        }
    }
    Ok(())
}

/// `aevia-pkg key show [--key FINGERPRINT]`.
///
/// # Errors
///
/// A usage error when no key can be chosen, `E_KEY` when it cannot be read.
#[allow(
    clippy::single_call_fn,
    reason = "the key show command combines a local key and its registry status"
)]
pub async fn key_show(
    app: &App,
    fingerprint: Option<&str>,
) -> Result<(), CliError> {
    let local = keys::resolve(&keys::discover(None)?, fingerprint, &app.config)?;
    println!("key         {}", local.fingerprint);
    println!("kind        {}", local.kind.as_str());
    if !local.identity.is_empty() {
        println!("identity    {}", local.identity);
    }
    println!(
        "source      {}",
        match &local.origin {
            | keys::KeyOrigin::Gpg => "gpg-agent".to_string(),
            | keys::KeyOrigin::Ssh { private_path, .. } => private_path.display().to_string(),
        }
    );
    let is_default = app
        .config
        .default_key
        .as_deref()
        .is_some_and(|known| known.eq_ignore_ascii_case(&local.fingerprint));
    println!("default     {}", if is_default { "yes" } else { "no" });
    if let Some(reason) = local.unsupported_reason() {
        println!("usable      no: {reason}");
    } else if let Some(warning) = local.kind.preference_warning() {
        println!("warning     {warning}");
    }

    // Registration is a network question, so a failure here is reported as an
    // unknown rather than as an error: the key is still the key.
    match app.client.key_status(&local.fingerprint).await {
        | Ok(status) if status.registered => {
            println!(
                "registered  yes{}",
                status
                    .publisher
                    .map_or_default(|publisher| format!(" — {}", publisher.label()))
            );
        },
        | Ok(_) => {
            println!("registered  no");
            println!(
                "next        run `aevia-pkg login --key {}` to bind it",
                local.fingerprint
            );
        },
        | Err(error) => println!("registered  unknown ({})", error.message),
    }
    Ok(())
}

/// `aevia-pkg status`: what this machine can do, and as whom.
///
/// # Errors
///
/// Only a failure to read the current directory or a malformed config, both of
/// which mean nothing else could work either.
#[allow(
    clippy::single_call_fn,
    reason = "status reports configuration, local keys and registry reachability together"
)]
pub async fn status(app: &App) -> Result<(), CliError> {
    let cwd = std::env::current_dir()
        .map_err(|error| CliError::new(format!("cannot read the current directory: {error}")))?;
    let package = package::find_root(&cwd).and_then(|root| {
        aevia_registry_core::manifest::PackageManifest::load(&root)
            .ok()
            .map(|manifest| (root, manifest))
    });
    let keys = keys::discover(None).unwrap_or_default();
    let default_key = keys::resolve(&keys, None, &app.config).ok();
    let default = default_key.as_ref().map(|key| key.fingerprint.clone());
    let default_kind = default_key
        .as_ref()
        .map(|key| key.kind)
        .or(app.config.default_key_kind);

    // `None` means "no key was chosen at all", which is a different answer from
    // "the registry could not be asked", and a publisher reading `status` needs
    // to tell them apart.
    let registration: Option<Result<api::KeyStatus, String>> = match &default {
        | Some(fingerprint) => {
            Some(
                app.client
                    .key_status(fingerprint)
                    .await
                    .map_err(|error| error.message),
            )
        },
        | None => None,
    };

    if app.json {
        let body = serde_json::json!({
            "registry": app.client.base(),
            "config": app.home.join("config.toml").display().to_string(),
            "default_key": default,
            "default_key_kind": default_kind.map(aevia_registry_core::signature::KeyKind::as_str),
            "registered": match &registration {
                | Some(Ok(status)) => serde_json::Value::Bool(status.registered),
                | _ => serde_json::Value::Null,
            },
            "publisher": match &registration {
                | Some(Ok(status)) => serde_json::to_value(&status.publisher)
                    .unwrap_or(serde_json::Value::Null),
                | _ => serde_json::Value::Null,
            },
            "key_error": match &registration {
                | Some(Err(message)) => serde_json::Value::String(message.clone()),
                | _ => serde_json::Value::Null,
            },
            "gpg": keys::gpg_available(),
            "ssh_keygen": keys::ssh_keygen_available(),
            "keys": keys.len(),
            "package": package.as_ref().map(|(root, manifest)| serde_json::json!({
                "root": root.display().to_string(),
                "name": manifest.name,
                "version": manifest.version,
            })),
        });
        print_json(&body)?;
        return Ok(());
    }

    println!("registry     {}", app.client.base());
    println!("config       {}", app.home.join("config.toml").display());
    match &default {
        | Some(fingerprint) => {
            println!(
                "default key  {fingerprint}{}",
                default_kind.map_or_default(|kind| format!(" ({})", kind.as_str()))
            );
        },
        | None => println!("default key  none — run `aevia-pkg login`"),
    }
    if let Some(kind) = default_kind {
        keys::warn_key_preference(kind, app.quiet, &[]);
    }
    match &registration {
        | Some(Ok(status)) if status.registered => {
            println!(
                "registered   yes{}",
                status
                    .publisher
                    .as_ref()
                    .map_or_default(|publisher| format!(" — {}", publisher.label()))
            );
        },
        | Some(Ok(_)) => println!("registered   no — run `aevia-pkg login`"),
        | Some(Err(message)) => println!("registered   unknown ({message})"),
        | None => println!("registered   none — run `aevia-pkg login`"),
    }
    println!(
        "tools        gpg {}, ssh-keygen {}",
        if keys::gpg_available() {
            "yes"
        } else {
            "no"
        },
        if keys::ssh_keygen_available() {
            "yes"
        } else {
            "no"
        }
    );
    println!("keys         {} found on this machine", keys.len());
    match &package {
        | Some((root, manifest)) => {
            println!(
                "package      {} {} in {}",
                manifest.name,
                manifest.version,
                root.display()
            );
        },
        | None => println!("package      none in `{}` or above", cwd.display()),
    }
    Ok(())
}

/// Reads the config for `status`, so a broken file is a line, not a crash.
#[must_use]
#[allow(
    clippy::single_call_fn,
    reason = "status renders a configuration error alongside its remaining observations"
)]
pub fn load_config(home: &Path) -> (Config, Option<String>) {
    match Config::load(home) {
        | Ok(config) => (config, None),
        | Err(error) => (Config::default(), Some(error.message)),
    }
}

/// Prints a serialisable value as pretty JSON on stdout.
fn print_json<T: serde::Serialize>(value: &T) -> Result<(), CliError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|error| CliError::new(format!("cannot serialise the answer as JSON: {error}")))?;
    println!("{text}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sortings_have_the_wire_names_the_api_documents() {
        assert_eq!(Sort::Relevance.as_str(), "relevance");
        assert_eq!(Sort::Downloads.as_str(), "downloads");
        assert_eq!(Sort::Recent.as_str(), "recent");
        assert_eq!(Sort::Name.as_str(), "name");
    }

    #[test]
    fn a_broken_config_is_reported_as_a_line() {
        let dir = std::env::temp_dir().join(format!("aevia-pkg-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("config.toml"), "nonsense\n").expect("write");
        let (config, error) = load_config(&dir);
        assert_eq!(config, Config::default());
        assert!(error.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
