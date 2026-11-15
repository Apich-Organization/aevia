//! `aevia-pkg publish` and `aevia-pkg yank`: the two signed actions.
//!
//! # The order of operations is the security property
//!
//! A publish asks the server for a statement, **checks that statement against
//! what it asked for**, and only then signs it. That order matters:
//!
//! 1. the archive is packed locally and its digest computed;
//! 2. the challenge is requested for that exact name, version and digest;
//! 3. the returned statement is parsed with
//!    [`aevia_registry_core::statement::PublishStatement::parse`] and compared
//!    field by field with the request;
//! 4. only a statement that survived step 3 is handed to `gpg`/`ssh-keygen`.
//!
//! Without step 3 a hostile or broken server could answer a challenge for
//! `apich/units-extra 0.2.0` with a statement naming `apich/aevia 1.0.0` and a
//! different digest, and the client would sign an authorisation for something
//! it never built. The signature is the only credential there is, so the text
//! it covers is the one thing that must be checked before the key touches it.
//!
//! # The owner comes from the key, not from the command line
//!
//! There is no `--owner`. The owner segment is the handle the signing key is
//! bound to, which the client asks the registry for
//! (`GET /api/v1/keys?fingerprint=…`) *before* packing. A key that is not
//! registered fails immediately with "run `aevia-pkg login`", rather than after
//! a 16 MiB upload.

use aevia_registry_core::PackageId;
use aevia_registry_core::manifest::PackageMetadata;
use aevia_registry_core::manifest::non_empty_trimmed;
use aevia_registry_core::manifest::normalize_tag_list;
use aevia_registry_core::manifest::validate_relative_path;
use aevia_registry_core::manifest::validate_required_publish_metadata;
use aevia_registry_core::signature::SignerKey;
use aevia_registry_core::statement::PublishStatement;
use aevia_registry_core::statement::YankStatement;
use aevia_registry_core::tarball;
use aevia_registry_core::vm::human_bytes;

use crate::App;
use crate::error::CliError;
use crate::keys;
use crate::package;

/// The most keywords the registry records for a package.
const MAX_KEYWORDS: usize = 5;

/// The flags `aevia-pkg publish` accepts.
#[derive(Clone, Debug, Default)]
pub struct Options<'a> {
    /// `--key`: the fingerprint to sign with, overriding the config.
    pub key: Option<&'a str>,
    /// `--description`: one sentence, overriding `aevia-pkg.toml`.
    pub description: Option<&'a str>,
    /// `--license`: an SPDX identifier, overriding detection.
    pub license: Option<&'a str>,
    /// `--keyword`, repeated.
    pub keywords: &'a [String],
    /// `--repository`: where the source lives.
    pub repository: Option<&'a str>,
    /// `--dry-run`: print the statement and the checksum, upload nothing.
    pub dry_run: bool,
    /// `--allow-dirty`: publish even though `git status` is not empty.
    pub allow_dirty: bool,
}

/// Publishes the package in the current directory.
///
/// # Errors
///
/// A refusal (dirty tree, name mismatch, unregistered key), a packing failure,
/// a challenge that does not match, a signing failure, or the server's own
/// error.
pub async fn run(
    app: &App,
    options: &Options<'_>,
) -> Result<(), CliError> {
    let cwd = std::env::current_dir()
        .map_err(|error| CliError::new(format!("cannot read the current directory: {error}")))?;
    let package = package::load_from(&cwd)?;

    // The dirty check comes first: it is the cheapest refusal, and a publisher
    // who is about to upload the wrong tree should learn that before a 16 MiB
    // archive is built.
    match package::dirty_files(&package.root) {
        | Some(files) if !options.allow_dirty => {
            let listed: Vec<&str> = files.iter().take(5).map(String::as_str).collect();
            return Err(CliError::new(format!(
                "the working tree has {} uncommitted change(s), so the archive would not match \
                 any commit:\n  {}",
                files.len(),
                listed.join("\n  ")
            ))
            .with_next_step("commit or stash them, or pass `--allow-dirty` to publish anyway"));
        },
        | Some(files) => {
            if !app.quiet {
                eprintln!(
                    "aevia-pkg: warning: publishing a dirty tree ({} changed file(s))",
                    files.len()
                );
            }
        },
        | None => {},
    }

    let local = keys::resolve(&keys::discover(None)?, options.key, &app.config)?;
    if let Some(reason) = local.unsupported_reason() {
        return Err(CliError::new(format!(
            "`{}` cannot be used to publish: {reason}",
            local.fingerprint
        ))
        .with_next_step(
            "prefer an OpenPGP (GPG) key (hardware-backed recommended), or generate an \
             `ssh-ed25519` SSH key",
        ));
    }
    keys::warn_key_preference(local.kind, app.quiet, &[]);

    // Ask who this key speaks for *before* building anything. The owner of a
    // package is the account the key is bound to, so this is also how the
    // client learns the name it is publishing under.
    let status = app.client.key_status(&local.fingerprint).await?;
    if !status.registered {
        return Err(CliError::new(format!(
            "the key `{}` is not bound to a publisher account, so it cannot publish anything",
            local.fingerprint
        ))
        .with_next_step(format!(
            "run `aevia-pkg login --key {}` and approve the code in a browser",
            local.fingerprint
        )));
    }
    let Some(publisher) = status.publisher else {
        return Err(CliError::new(format!(
            "the registry reports `{}` as registered but names no publisher",
            local.fingerprint
        ))
        .with_next_step("run `aevia-pkg login` again to re-bind the key"));
    };

    let id = PackageId::new(&publisher.handle, &package.manifest.name).map_err(CliError::from)?;
    let version = package.manifest.version.clone();
    if !package::valid_version(&version) {
        return Err(CliError::new(format!(
            "E_VERSION: `{version}` is not a `major.minor.patch` version"
        ))
        .with_next_step("fix `[package] version` in `aevia.toml`"));
    }

    let metadata = PackageMetadata::discover(&package.root);
    let manifest = &package.manifest;
    let edition = non_empty_trimmed(manifest.edition.as_deref());
    let authors = normalize_tag_list(&manifest.authors, false, 64, "authors")?;
    let description = non_empty_trimmed(options.description)
        .or_else(|| non_empty_trimmed(manifest.description.as_deref()))
        .or_else(|| non_empty_trimmed(metadata.declared.description.as_deref()));
    let license = non_empty_trimmed(options.license)
        .or_else(|| non_empty_trimmed(manifest.license.as_deref()))
        .or_else(|| non_empty_trimmed(metadata.declared.license.as_deref()));
    let license_file = non_empty_trimmed(manifest.license_file.as_deref())
        .map(|path| validate_relative_path(&path, "license-file"))
        .transpose()?;
    let license_file_present = license_file.as_deref().is_some_and(|rel| {
        std::fs::read_to_string(package.root.join(rel))
            .is_ok_and(|contents| !contents.trim().is_empty())
    });
    validate_required_publish_metadata(
        description.as_deref(),
        license.as_deref(),
        license_file_present,
    )
    .map_err(|error| {
        CliError::from(error)
            .with_next_step("fill `[package] description` and `license` (or `license-file`) in `aevia.toml`, or pass `--description` and `--license`")
    })?;
    let repository = non_empty_trimmed(options.repository)
        .or_else(|| non_empty_trimmed(manifest.repository.as_deref()))
        .or(metadata.repository);
    let homepage = non_empty_trimmed(manifest.homepage.as_deref());
    let documentation = non_empty_trimmed(manifest.documentation.as_deref());
    let keywords = if !options.keywords.is_empty() {
        normalize_keywords(options.keywords)
    } else if !manifest.keywords.is_empty() {
        normalize_tag_list(&manifest.keywords, true, MAX_KEYWORDS, "keywords")?
    } else {
        metadata.declared.keywords
    };
    let categories = normalize_tag_list(&manifest.categories, true, MAX_KEYWORDS, "categories")?;

    let archive = package::pack(&package)?;
    let checksum = tarball::checksum(&archive);
    if !app.quiet {
        eprintln!(
            "aevia-pkg: packed {} ({}), {}",
            id,
            human_bytes(archive.len() as u64),
            checksum
        );
    }

    let challenge = app
        .client
        .publish_challenge(&id.to_path(), &version, &checksum)
        .await?;
    let statement = check_statement(
        &challenge.statement,
        &id,
        &version,
        &checksum,
        chrono::Utc::now().timestamp(),
    )?;

    if options.dry_run {
        println!("package   {id} {version}");
        println!(
            "archive   {} ({checksum})",
            human_bytes(archive.len() as u64)
        );
        println!("signer    {}", local.label());
        println!("statement");
        for line in statement.render().lines() {
            println!("  {line}");
        }
        println!("dry run: nothing was signed and nothing was uploaded");
        return Ok(());
    }

    // The bytes signed are the ones the server sent, not a re-render of the
    // parsed statement: if the two ever differed, the server would verify
    // against its own text and the signature would fail — but only after the
    // upload. Signing what was received is also what makes the check above
    // meaningful: the text checked and the text signed are the same string.
    let signature = keys::sign(&local, &challenge.statement)?;
    let armored = keys::public_key(&local)?;
    let signer = SignerKey {
        kind: local.kind,
        armored,
    };
    let envelope = Envelope {
        statement: &challenge.statement,
        signature: &signature,
        key: &signer,
        edition: edition.as_deref(),
        authors: &authors,
        description: description.as_deref(),
        license: license.as_deref(),
        license_file: license_file.as_deref(),
        keywords: &keywords,
        categories: &categories,
        repository: repository.as_deref(),
        homepage: homepage.as_deref(),
        documentation: documentation.as_deref(),
    };
    let envelope = serde_json::to_string(&envelope).map_err(|error| {
        CliError::new(format!("cannot serialise the publish envelope: {error}"))
    })?;

    if !app.quiet {
        eprintln!("aevia-pkg: uploading {} bytes", archive.len());
    }
    let accepted = app.client.publish(&envelope, &archive).await?;
    let extra_warnings: Vec<String> = accepted
        .warnings
        .iter()
        .filter(|warning| local.kind.preference_warning() != Some(warning.as_str()))
        .cloned()
        .collect();
    if !app.quiet {
        for warning in &extra_warnings {
            eprintln!("aevia-pkg: warning: {warning}");
        }
    }

    println!(
        "published {} {} ({})",
        accepted.name, accepted.version, accepted.checksum
    );
    if let Some(docs) = &accepted.docs_url {
        println!("docs    {}{}", app.client.base(), docs);
    }
    if let Some(log) = &accepted.log {
        println!("log     seq {} hash {}", log.seq, log.hash);
    }
    if !accepted.checksum.eq_ignore_ascii_case(&checksum) {
        // The archive the server stored is not the one that was signed. The
        // publish is already committed, so this is a loud warning rather than
        // an error: the publisher needs to know, and needs to know now.
        eprintln!(
            "aevia-pkg: warning: the registry stored {checksum} as {}",
            accepted.checksum
        );
    }
    Ok(())
}

/// Checks a server-rendered publish statement against the request it answers.
///
/// This is the pure heart of the publish path, kept separate so it can be
/// tested without a server, a key or a network: given the text the registry
/// sent and the name, version and digest the client asked about, either the
/// statement authorises exactly that archive or the client refuses to sign.
///
/// # Errors
///
/// `E_STATEMENT` when the text does not parse, names another package or
/// version, covers another digest, or has already expired.
pub fn check_statement(
    text: &str,
    expected_name: &PackageId,
    expected_version: &str,
    expected_checksum: &str,
    now: i64,
) -> Result<PublishStatement, CliError> {
    let statement = PublishStatement::parse(text).map_err(|error| {
        CliError::new(format!(
            "the registry's statement does not parse: {}",
            error.message
        ))
        .with_next_step("this is a registry bug or a proxy rewriting the response")
    })?;
    if statement.name != *expected_name {
        return Err(CliError::new(format!(
            "the registry asked for `{}` to be signed, but `{expected_name}` was requested",
            statement.name
        ))
        .with_next_step("nothing was signed; report this to the registry"));
    }
    if statement.version != expected_version {
        return Err(CliError::new(format!(
            "the registry asked for version `{}` to be signed, but `{expected_version}` was \
             requested",
            statement.version
        ))
        .with_next_step("nothing was signed; report this to the registry"));
    }
    if !statement.checksum.eq_ignore_ascii_case(expected_checksum) {
        return Err(CliError::new(format!(
            "the registry asked for {} to be signed, but the archive packed here is \
             {expected_checksum}",
            statement.checksum
        ))
        .with_next_step(
            "nothing was signed; the archive the registry would store is not the one built here",
        ));
    }
    if statement.is_expired(now) {
        return Err(CliError::new(format!(
            "the statement expired at {} before it could be signed",
            statement.expires
        ))
        .with_next_step("check the machine's clock, then run the command again"));
    }
    Ok(statement)
}

/// Checks a server-rendered yank statement against the request it answers.
///
/// # Errors
///
/// `E_STATEMENT` when the text does not parse, names another package, version
/// or direction, or has expired.
pub fn check_yank_statement(
    text: &str,
    expected_name: &PackageId,
    expected_version: &str,
    expected_yanked: bool,
    now: i64,
) -> Result<YankStatement, CliError> {
    let statement = YankStatement::parse(text).map_err(|error| {
        CliError::new(format!(
            "the registry's statement does not parse: {}",
            error.message
        ))
        .with_next_step("this is a registry bug or a proxy rewriting the response")
    })?;
    if statement.name != *expected_name {
        return Err(CliError::new(format!(
            "the registry asked for `{}` to be signed, but `{expected_name}` was requested",
            statement.name
        ))
        .with_next_step("nothing was signed; report this to the registry"));
    }
    if statement.version != expected_version {
        return Err(CliError::new(format!(
            "the registry asked for version `{}` to be signed, but `{expected_version}` was \
             requested",
            statement.version
        ))
        .with_next_step("nothing was signed; report this to the registry"));
    }
    if statement.yanked != expected_yanked {
        return Err(CliError::new(format!(
            "the registry asked for `yanked: {}` to be signed, but `yanked: {expected_yanked}` \
             was requested",
            statement.yanked
        ))
        .with_next_step("nothing was signed; report this to the registry"));
    }
    if statement.is_expired(now) {
        return Err(CliError::new(format!(
            "the statement expired at {} before it could be signed",
            statement.expires
        ))
        .with_next_step("check the machine's clock, then run the command again"));
    }
    Ok(statement)
}

/// Yanks or restores one version.
///
/// # Errors
///
/// A malformed target, an unregistered key, a mismatched statement, a signing
/// failure, or the server's own error.
pub async fn yank(
    app: &App,
    spec: &str,
    undo: bool,
) -> Result<(), CliError> {
    let (id, version) = package::parse_target(spec)?;
    let Some(version) = version else {
        return Err(CliError::usage(format!(
            "`{spec}` names no version; write it as `{id}@0.2.0`"
        )));
    };
    let yanked = !undo;

    let local = keys::resolve(&keys::discover(None)?, None, &app.config)?;
    keys::warn_key_preference(local.kind, app.quiet, &[]);
    let status = app.client.key_status(&local.fingerprint).await?;
    if !status.registered {
        return Err(CliError::new(format!(
            "the key `{}` is not bound to a publisher account",
            local.fingerprint
        ))
        .with_next_step(format!(
            "run `aevia-pkg login --key {}` first",
            local.fingerprint
        )));
    }
    // The server checks this too, but a local check names the two accounts and
    // saves a round trip on the mistake people actually make: two keys, one
    // account each.
    if let Some(publisher) = &status.publisher
        && publisher.handle != id.owner()
    {
        return Err(CliError::new(format!(
            "the key `{}` belongs to `{}`, but `{id}` is owned by `{}`",
            local.fingerprint,
            publisher.handle,
            id.owner()
        ))
        .with_next_step("pass `--key` with a key bound to the owning account"));
    }

    let challenge = app
        .client
        .yank_challenge(&id.to_path(), &version, yanked)
        .await?;
    check_yank_statement(
        &challenge.statement,
        &id,
        &version,
        yanked,
        chrono::Utc::now().timestamp(),
    )?;
    let signature = keys::sign(&local, &challenge.statement)?;
    let armored = keys::public_key(&local)?;
    let body = serde_json::json!({
        "statement": challenge.statement,
        "signature": signature,
        "key": SignerKey {
            kind: local.kind,
            armored,
        },
    });
    app.client.yank(&body).await?;
    if yanked {
        println!("yanked {id} {version}");
    } else {
        println!("restored {id} {version}");
    }
    Ok(())
}

/// Normalises `--keyword` values the way the registry stores them.
fn normalize_keywords(keywords: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for keyword in keywords {
        let keyword = keyword.trim().to_ascii_lowercase();
        if keyword.is_empty() || out.contains(&keyword) {
            continue;
        }
        out.push(keyword);
        if out.len() == MAX_KEYWORDS {
            break;
        }
    }
    out
}

/// The `envelope` part of the publish upload.
#[derive(serde::Serialize)]
struct Envelope<'a> {
    statement: &'a str,
    signature: &'a str,
    key: &'a SignerKey,
    #[serde(skip_serializing_if = "Option::is_none")]
    edition: Option<&'a str>,
    #[serde(skip_serializing_if = "slice_is_empty")]
    authors: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    license_file: Option<&'a str>,
    keywords: &'a [String],
    #[serde(skip_serializing_if = "slice_is_empty")]
    categories: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    repository: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    homepage: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    documentation: Option<&'a str>,
}

const fn slice_is_empty(items: &&[String]) -> bool {
    items.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHECKSUM: &str =
        "sha256:abababababababababababababababababababababababababababababababab";

    fn id() -> PackageId {
        "apich/units-extra".parse().expect("name")
    }

    fn statement(
        name: &str,
        version: &str,
        checksum: &str,
    ) -> String {
        PublishStatement::new(
            name.parse().expect("name"),
            version,
            checksum,
            "0123456789abcdef0123456789abcdef",
            1_794_000_300,
        )
        .render()
    }

    #[test]
    fn a_matching_statement_is_accepted() {
        let text = statement("apich/units-extra", "0.2.0", CHECKSUM);
        let checked =
            check_statement(&text, &id(), "0.2.0", CHECKSUM, 1_794_000_000).expect("accepted");
        assert_eq!(checked.name, id());
        assert_eq!(checked.version, "0.2.0");
    }

    #[test]
    fn a_statement_for_another_package_is_refused() {
        let text = statement("apich/aevia", "0.2.0", CHECKSUM);
        let error =
            check_statement(&text, &id(), "0.2.0", CHECKSUM, 1_794_000_000).expect_err("refused");
        assert!(error.message.contains("apich/aevia"), "{}", error.message);
        assert!(
            error
                .next_step
                .as_deref()
                .is_some_and(|step| step.contains("nothing was signed")),
            "{:?}",
            error.next_step
        );
    }

    #[test]
    fn a_statement_for_another_version_is_refused() {
        let text = statement("apich/units-extra", "9.9.9", CHECKSUM);
        let error =
            check_statement(&text, &id(), "0.2.0", CHECKSUM, 1_794_000_000).expect_err("refused");
        assert!(error.message.contains("9.9.9"), "{}", error.message);
    }

    #[test]
    fn a_statement_over_another_archive_is_refused() {
        let other = format!("sha256:{}", "cd".repeat(32));
        let text = statement("apich/units-extra", "0.2.0", &other);
        let error =
            check_statement(&text, &id(), "0.2.0", CHECKSUM, 1_794_000_000).expect_err("refused");
        assert!(error.message.contains(&other), "{}", error.message);
    }

    #[test]
    fn an_expired_statement_is_refused_before_it_is_signed() {
        let text = statement("apich/units-extra", "0.2.0", CHECKSUM);
        let error =
            check_statement(&text, &id(), "0.2.0", CHECKSUM, 1_794_000_400).expect_err("refused");
        assert!(error.message.contains("expired"), "{}", error.message);
    }

    #[test]
    fn a_statement_that_is_not_a_statement_is_refused() {
        let error = check_statement("hello", &id(), "0.2.0", CHECKSUM, 0).expect_err("refused");
        assert!(
            error.message.contains("does not parse"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_yank_statement_must_agree_about_the_direction() {
        let text =
            YankStatement::new(id(), "0.2.0", false, "0123456789abcdef", 1_794_000_300).render();
        let error =
            check_yank_statement(&text, &id(), "0.2.0", true, 1_794_000_000).expect_err("refused");
        assert!(error.message.contains("yanked"), "{}", error.message);
        assert!(check_yank_statement(&text, &id(), "0.2.0", false, 1_794_000_000).is_ok());
    }

    #[test]
    fn keywords_are_lower_cased_deduplicated_and_capped() {
        let keywords: Vec<String> = [" Linear ", "linear", "MATH", "", "a", "b", "c"]
            .iter()
            .map(|value| (*value).to_string())
            .collect();
        assert_eq!(
            normalize_keywords(&keywords),
            ["linear", "math", "a", "b", "c"]
        );
    }
}
