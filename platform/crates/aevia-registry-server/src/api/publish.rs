//! Publishing: the challenge, the signature, the archive.
//!
//! The order of checks is the design. Nothing is written to the catalogue until
//! the bytes have been hashed, the statement has been re-read, the nonce has
//! been spent, the key has been found, the signature has verified, the scope
//! has been authorised and the manifest inside the archive has been compared
//! with the statement. A publish that fails any of those leaves no trace except
//! a spent nonce, which is exactly what a replay attempt should cost.

use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::Multipart;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::OwnedSemaphorePermit;

use aevia_registry_core::PackageId;
use aevia_registry_core::manifest::PackageManifest;
use aevia_registry_core::manifest::RegistryMeta;
use aevia_registry_core::manifest::non_empty_trimmed;
use aevia_registry_core::manifest::normalize_tag_list;
use aevia_registry_core::manifest::validate_http_url;
use aevia_registry_core::manifest::validate_relative_path;
use aevia_registry_core::manifest::validate_required_publish_metadata;
use aevia_registry_core::name::valid_crate_name;
use aevia_registry_core::signature::SignerKey;
use aevia_registry_core::statement::PublishStatement;
use aevia_registry_core::statement::STATEMENT_LIFETIME_SECONDS;
use aevia_registry_core::statement::Statement;
use aevia_registry_core::tarball;
use aevia_registry_db::models::NewVersion;
use aevia_registry_db::repo;

use crate::api::ApiError;
use crate::api::ApiResult;
use crate::api::authorise;
use crate::http;
use crate::state::AppState;

/// The body of a challenge request.
#[derive(Deserialize)]
pub struct ChallengeRequest {
    /// `owner/name`.
    pub name: String,
    /// `major.minor.patch`.
    pub version: String,
    /// `sha256:<hex>` of the archive about to be uploaded.
    pub checksum: String,
}

/// The challenge the client signs.
#[derive(Serialize)]
pub struct ChallengeResponse {
    /// The single-use nonce.
    pub nonce: String,
    /// The exact bytes to sign.
    pub statement: String,
    /// When they stop being valid, RFC 3339.
    pub expires_at: String,
}

/// `POST /api/v1/publish/challenge`
///
/// # Errors
///
/// `400` for a malformed name, version or checksum.
pub async fn challenge(
    State(state): State<AppState>,
    Json(request): Json<ChallengeRequest>,
) -> ApiResult<Json<ChallengeResponse>> {
    let id = request
        .name
        .parse::<PackageId>()
        .map_err(ApiError::from_core)?;
    if !valid_crate_name(id.name()) {
        return Err(ApiError::bad_request(
            "E_NAME",
            format!("`{}` is not a package name", id.name()),
        ));
    }
    check_version(&request.version)?;
    check_checksum(&request.checksum)?;
    let nonce = http::random_hex(16);
    let expires = Utc::now()
        .timestamp()
        .saturating_add(STATEMENT_LIFETIME_SECONDS);
    let statement = PublishStatement::new(
        id,
        request.version,
        request.checksum.to_ascii_lowercase(),
        nonce.clone(),
        expires,
    )
    .render();
    let expires_at = chrono::DateTime::from_timestamp(expires, 0).unwrap_or_else(Utc::now);
    repo::create_challenge(&state.pool, &nonce, "publish", &statement, expires_at).await?;
    Ok(Json(ChallengeResponse {
        nonce,
        statement,
        expires_at: expires_at.to_rfc3339(),
    }))
}

/// The JSON part of the upload.
#[derive(Deserialize)]
pub struct Envelope {
    /// The exact statement the server issued.
    pub statement: String,
    /// The ASCII-armoured detached signature over it.
    pub signature: String,
    /// The public key that made the signature.
    pub key: SignerKey,
    /// An override for the language edition.
    pub edition: Option<String>,
    /// An override for the package authors list.
    #[serde(default)]
    pub authors: Vec<String>,
    /// An override for the description declared in `aevia.toml`.
    pub description: Option<String>,
    /// An override for the license expression declared in `aevia.toml`.
    pub license: Option<String>,
    /// An override for `license-file`.
    pub license_file: Option<String>,
    /// Up to five keywords.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Up to five registry categories.
    #[serde(default)]
    pub categories: Vec<String>,
    /// Where the source lives.
    pub repository: Option<String>,
    /// Project homepage URL.
    pub homepage: Option<String>,
    /// Documentation URL.
    pub documentation: Option<String>,
}

struct ResolvedMetadata {
    edition: Option<String>,
    authors: Vec<String>,
    description: Option<String>,
    license: Option<String>,
    license_file: Option<String>,
    readme_path: Option<String>,
    readme_text: Option<String>,
    repository: Option<String>,
    homepage: Option<String>,
    documentation: Option<String>,
    keywords: Vec<String>,
    categories: Vec<String>,
}

fn first_non_empty(candidates: &[Option<&str>]) -> Option<String> {
    candidates
        .iter()
        .find_map(|candidate| non_empty_trimmed(*candidate))
}

fn validate_optional_url(
    raw: Option<String>,
    field: &str,
) -> ApiResult<Option<String>> {
    raw.map(|url| validate_http_url(&url, field).map_err(ApiError::from_core))
        .transpose()
}

#[allow(
    clippy::single_call_fn,
    reason = "publication metadata reconciles the manifest and the signed statement"
)]
fn resolve_publish_metadata(
    envelope: &Envelope,
    manifest: &PackageManifest,
    declared: &RegistryMeta,
    files: &[(String, String)],
) -> ApiResult<ResolvedMetadata> {
    let edition = first_non_empty(&[envelope.edition.as_deref(), manifest.edition.as_deref()]);
    let raw_authors = if envelope.authors.is_empty() {
        &manifest.authors
    } else {
        &envelope.authors
    };
    let authors =
        normalize_tag_list(raw_authors, false, 64, "authors").map_err(ApiError::from_core)?;
    let description = first_non_empty(&[
        envelope.description.as_deref(),
        manifest.description.as_deref(),
        declared.description.as_deref(),
    ]);
    let license = first_non_empty(&[
        envelope.license.as_deref(),
        manifest.license.as_deref(),
        declared.license.as_deref(),
    ]);
    let license_file = first_non_empty(&[
        envelope.license_file.as_deref(),
        manifest.license_file.as_deref(),
    ])
    .map(|path| validate_relative_path(&path, "license-file").map_err(ApiError::from_core))
    .transpose()?;
    let license_file_in_archive = license_file.as_deref().is_some_and(|rel| {
        files
            .iter()
            .any(|(name, text)| name == rel && !text.trim().is_empty())
    });
    if let Some(rel) = license_file.as_deref()
        && !license_file_in_archive
    {
        return Err(ApiError::bad_request(
            "E_MANIFEST",
            format!(
                "`[package] license-file` names `{rel}`, which is missing or empty in the archive"
            ),
        ));
    }
    validate_required_publish_metadata(
        description.as_deref(),
        license.as_deref(),
        license_file_in_archive,
    )
    .map_err(ApiError::from_core)?;

    let readme_path = first_non_empty(&[manifest.readme.as_deref(), declared.readme.as_deref()])
        .map(|path| validate_relative_path(&path, "readme").map_err(ApiError::from_core))
        .transpose()?;
    let readme_text = readme_path
        .as_deref()
        .and_then(|rel| {
            files
                .iter()
                .find(|(name, _)| name == rel || name.eq_ignore_ascii_case(rel))
                .map(|(_, text)| text.clone())
        })
        .or_else(|| {
            files
                .iter()
                .find(|(name, _)| {
                    name.eq_ignore_ascii_case("readme.md") || name.eq_ignore_ascii_case("readme")
                })
                .map(|(_, text)| text.clone())
        });

    let repository = validate_optional_url(
        first_non_empty(&[
            envelope.repository.as_deref(),
            manifest.repository.as_deref(),
            declared.repository.as_deref(),
        ]),
        "repository",
    )?;
    let homepage = validate_optional_url(
        first_non_empty(&[envelope.homepage.as_deref(), manifest.homepage.as_deref()]),
        "homepage",
    )?;
    let documentation = validate_optional_url(
        first_non_empty(&[
            envelope.documentation.as_deref(),
            manifest.documentation.as_deref(),
        ]),
        "documentation",
    )?;

    let raw_keywords = if !envelope.keywords.is_empty() {
        &envelope.keywords
    } else if !manifest.keywords.is_empty() {
        &manifest.keywords
    } else {
        &declared.keywords
    };
    let keywords =
        normalize_tag_list(raw_keywords, true, 5, "keywords").map_err(ApiError::from_core)?;
    let raw_categories = if envelope.categories.is_empty() {
        &manifest.categories
    } else {
        &envelope.categories
    };
    let categories =
        normalize_tag_list(raw_categories, true, 5, "categories").map_err(ApiError::from_core)?;

    Ok(ResolvedMetadata {
        edition,
        authors,
        description,
        license,
        license_file,
        readme_path,
        readme_text,
        repository,
        homepage,
        documentation,
        keywords,
        categories,
    })
}

/// The reply to a successful publish.
#[derive(Serialize)]
pub struct PublishResponse {
    /// `owner/name`.
    pub name: String,
    /// The version.
    pub version: String,
    /// `sha256:<hex>`.
    pub checksum: String,
    /// Where the documentation will appear, when documentation is enabled.
    pub docs_url: Option<String>,
    /// The log entry this publish created.
    pub log: LogRef,
    /// Policy recommendations (for example preferring `OpenPGP` + hardware keys
    /// when publishing with an SSH key).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// A reference to one transparency-log entry.
#[derive(Serialize)]
pub struct LogRef {
    /// Position in the chain.
    pub seq: i64,
    /// The entry's hash.
    pub hash: String,
}

/// `POST /api/v1/publish`
///
/// # Errors
///
/// `400` for a malformed envelope, `403` for a signature or scope failure,
/// `409` when the version already exists, `413` when the archive is too large.
pub async fn publish(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> ApiResult<(StatusCode, Json<PublishResponse>)> {
    let slot = Arc::new(
        state
            .publish_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                ApiError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "E_PUBLISH_BUSY",
                    "The registry is processing other releases. Try again shortly.",
                )
            })?,
    );
    let mut envelope: Option<Envelope> = None;
    let mut archive: Option<Bytes> = None;
    while let Some(field) = multipart.next_field().await.map_err(|error| {
        ApiError::bad_request("E_MULTIPART", format!("malformed upload: {error}"))
    })? {
        match field.name() {
            | Some("envelope") if envelope.is_none() => {
                let text = field.text().await.map_err(|error| {
                    ApiError::bad_request(
                        "E_MULTIPART",
                        format!("cannot read the envelope: {error}"),
                    )
                })?;
                envelope = Some(serde_json::from_str(&text).map_err(|error| {
                    ApiError::bad_request("E_JSON", format!("the envelope is not JSON: {error}"))
                })?);
            },
            | Some("package") if archive.is_none() => {
                let bytes = field.bytes().await.map_err(|error| {
                    ApiError::bad_request(
                        "E_MULTIPART",
                        format!("cannot read the archive: {error}"),
                    )
                })?;
                if bytes.len() as u64 > tarball::MAX_ARCHIVE_BYTES {
                    return Err(ApiError::new(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "E_TARBALL",
                        "A package archive can be at most 16 MiB",
                    ));
                }
                archive = Some(bytes);
            },
            | Some("envelope" | "package") => {
                return Err(ApiError::bad_request(
                    "E_MULTIPART",
                    "An upload part was repeated",
                ));
            },
            | _ => {},
        }
    }
    let Some(envelope) = envelope else {
        return Err(ApiError::bad_request(
            "E_MULTIPART",
            "the upload has no `envelope` part",
        ));
    };
    let Some(archive) = archive else {
        return Err(ApiError::bad_request(
            "E_MULTIPART",
            "the upload has no `package` part",
        ));
    };

    // The signature, the nonce, the key and the clock, in one place.
    let authorised = authorise(
        &state,
        "publish",
        &envelope.statement,
        &envelope.signature,
        &envelope.key,
    )
    .await?;
    let Statement::Publish(statement) = authorised.statement else {
        return Err(ApiError::bad_request(
            "E_STATEMENT",
            "not a publish statement",
        ));
    };
    let publisher = &authorised.publisher;
    let fingerprint = authorised.fingerprint.clone();
    let key_kind = authorised.key_kind.clone();

    // The archive must be the one that was signed.
    let bytes = archive.clone();
    let checksum = publication_task(&slot, move || tarball::checksum(&bytes)).await?;
    if !checksum.eq_ignore_ascii_case(&statement.checksum) {
        return Err(ApiError::bad_request(
            "E_TARBALL",
            format!(
                "the archive is {checksum}, but the statement signs {}",
                statement.checksum
            ),
        ));
    }

    // The key's holder must be allowed to publish under this scope.
    let Some(owner_id) =
        repo::resolve_publishing_owner(&state.pool, publisher.id, statement.name.owner()).await?
    else {
        return Err(ApiError::forbidden(
            "E_FORBIDDEN",
            format!(
                "{} may not publish as `{}`; a package is published under your own handle, or \
                 under an organisation your GitHub App installation covers",
                publisher.handle,
                statement.name.owner()
            ),
        ));
    };

    // The archive must be a package, and it must be *this* package.
    let bytes = archive.clone();
    let files = publication_task(&slot, move || tarball::text_files(&bytes)).await??;
    let manifest_text = files
        .iter()
        .find(|(name, _)| name == "aevia.toml")
        .map(|(_, text)| text.clone())
        .ok_or_else(|| {
            ApiError::bad_request("E_TARBALL", "the archive has no `aevia.toml` at its root")
        })?;
    let manifest =
        PackageManifest::parse(&manifest_text, "aevia.toml").map_err(ApiError::from_core)?;
    if manifest.name != statement.name.name() {
        return Err(ApiError::bad_request(
            "E_MANIFEST",
            format!(
                "the archive's `aevia.toml` declares `{}`, but the statement publishes `{}`",
                manifest.name,
                statement.name.name()
            ),
        ));
    }
    if manifest.version != statement.version {
        return Err(ApiError::bad_request(
            "E_MANIFEST",
            format!(
                "the archive's `aevia.toml` declares {}, but the statement publishes {}",
                manifest.version, statement.version
            ),
        ));
    }

    // Dependencies: a published package can only depend on registry packages
    // that exist and can be resolved. Checked before anything is written, so a
    // refused publish costs a nonce and nothing else.
    manifest
        .check_publishable_as(&statement.name.to_string())
        .map_err(ApiError::from_core)?;
    check_dependencies(&state, &manifest).await?;

    // Metadata: the envelope wins, then `aevia.toml`, then `aevia-pkg.toml`.
    // Required publish fields (`description` and `license` or `license-file`)
    // are enforced before any database row is written.
    let declared = files
        .iter()
        .find(|(name, _)| name == "aevia-pkg.toml")
        .and_then(|(_, text)| RegistryMeta::parse(text, "aevia-pkg.toml").ok())
        .unwrap_or_default();
    let meta = resolve_publish_metadata(&envelope, &manifest, &declared, &files)?;
    drop(files);

    // The catalogue rows, the archive file and the log entry in one
    // transaction: either the version exists everywhere (row, dependency rows,
    // bytes, log link) or nowhere. The archive is written after the rows so a
    // conflicting version never overwrites the file of the one that won, and
    // before the commit so a failed write rolls the rows back instead of
    // leaving a version whose bytes are missing.
    let mut transaction = state.pool.begin().await.map_err(|error| {
        ApiError::from_core(aevia_registry_db::sql_error(
            "cannot start the publish transaction",
            error,
        ))
    })?;
    let package =
        repo::publication_package(&mut transaction, owner_id, statement.name.name()).await?;
    let package_name = PackageId::new(statement.name.owner(), &package.name)?;
    let dependencies = serde_json::to_value(manifest.index_dependencies()).map_err(|error| {
        ApiError::internal(format!("cannot serialise the dependency list: {error}"))
    })?;
    repo::insert_version_in(
        &mut transaction,
        &NewVersion {
            package_id: package.id,
            version: &statement.version,
            checksum: &checksum,
            size_bytes: i64::try_from(archive.len()).unwrap_or(i64::MAX),
            manifest: serde_json::json!({
                "name": manifest.name,
                "version": manifest.version,
                "edition": meta.edition,
                "entry": manifest.entry,
                "authors": meta.authors,
                "description": meta.description,
                "license": meta.license,
                "license_file": meta.license_file,
                "readme": meta.readme_path,
                "repository": meta.repository,
                "homepage": meta.homepage,
                "documentation": meta.documentation,
                "keywords": meta.keywords,
                "categories": meta.categories,
            }),
            dependencies,
            readme: meta.readme_text.as_deref(),
            published_by: publisher.id,
            key_kind: &key_kind,
            fingerprint: &fingerprint,
            signature: &envelope.signature,
            statement: &envelope.statement,
            build_docs: state.config.docs_enabled,
        },
    )
    .await
    .map_err(|error| {
        if error.code == aevia_registry_db::E_CONFLICT {
            ApiError::conflict(format!(
                "{} {} is already published; versions are immutable — publish a new one",
                statement.name, statement.version
            ))
        } else {
            ApiError::from_core(error)
        }
    })?;

    repo::update_package_meta(
        &mut *transaction,
        package.id,
        &repo::PackageMetaUpdate {
            edition: meta.edition.as_deref(),
            authors: &meta.authors,
            description: meta.description.as_deref(),
            license: meta.license.as_deref(),
            license_file: meta.license_file.as_deref(),
            repository: meta.repository.as_deref(),
            homepage: meta.homepage.as_deref(),
            documentation: meta.documentation.as_deref(),
            keywords: &meta.keywords,
            categories: &meta.categories,
        },
    )
    .await?;
    repo::touch_package(&mut *transaction, package.id).await?;

    let storage = state.storage.clone();
    let digest = checksum.clone();
    publication_task(&slot, move || storage.write(&digest, &archive)).await??;

    let entry = repo::append_log_in(
        &mut transaction,
        "publish",
        &format!(
            "{} published {} {}",
            publisher.handle, statement.name, statement.version
        ),
        serde_json::json!({
            "name": statement.name.to_string(),
            "version": statement.version,
            "checksum": checksum,
            "fingerprint": fingerprint,
            "kind": key_kind,
            "publisher": publisher.handle,
        }),
    )
    .await?;
    transaction.commit().await.map_err(|error| {
        ApiError::from_core(aevia_registry_db::sql_error(
            "cannot commit the publish",
            error,
        ))
    })?;

    drop(slot);
    let docs_url = if state.config.docs_enabled {
        state.docs_wake.notify_one();
        Some(format!("/docs/{package_name}/{}", statement.version))
    } else {
        None
    };
    let warnings: Vec<String> = envelope
        .key
        .kind
        .preference_warning()
        .into_iter()
        .map(str::to_string)
        .collect();
    Ok((
        StatusCode::CREATED,
        Json(PublishResponse {
            name: package_name.to_string(),
            version: statement.version,
            checksum,
            docs_url,
            log: LogRef {
                seq: entry.seq,
                hash: entry.entry_hash,
            },
            warnings,
        }),
    ))
}

async fn publication_task<T: Send + 'static>(
    slot: &Arc<OwnedSemaphorePermit>,
    operation: impl FnOnce() -> T + Send + 'static,
) -> ApiResult<T> {
    let slot = slot.clone();
    tokio::task::spawn_blocking(move || {
        let _slot = slot;
        operation()
    })
    .await
    .map_err(|error| ApiError::internal(format!("a publication background task failed: {error}")))
}

pub(crate) fn check_version(version: &str) -> ApiResult<()> {
    let parsed = aevia_resolve::Version::parse(version).map_err(|error| {
        ApiError::bad_request(
            "E_VERSION",
            format!("`{version}` is not a version: {}", error.reason),
        )
    })?;
    if !parsed.build.is_empty() {
        return Err(ApiError::bad_request(
            "E_VERSION",
            format!(
                "`{version}` carries build metadata; versions that differ only in build \
                 metadata have the same precedence, so the registry cannot tell them apart — \
                 publish `major.minor.patch` or `major.minor.patch-pre-release`"
            ),
        ));
    }
    Ok(())
}

/// The most dependencies one version may declare. Resolution reads every one of
/// them, so an unbounded list is a way to make every consumer's `aevia update`
/// slow.
pub const MAX_DEPENDENCIES: usize = 256;

/// Why a dependency requirement can never be resolved, or `None` when it can.
///
/// `versions` is every published version of the target as `(version, yanked)`.
/// A requirement is resolvable when some non-yanked version satisfies it (a
/// yanked one is selectable only by a lock file, which a new publish cannot
/// have).
#[must_use]
pub fn dependency_problem(
    package: &str,
    req: &aevia_resolve::VersionReq,
    versions: &[(String, bool)],
) -> Option<String> {
    let mut available: Vec<aevia_resolve::Version> = Vec::new();
    let mut yanked_match = false;
    for (text, yanked) in versions {
        let Ok(version) = aevia_resolve::Version::parse(text) else {
            continue;
        };
        if req.matches(&version) {
            if *yanked {
                yanked_match = true;
            } else {
                return None;
            }
        }
        if !yanked {
            available.push(version);
        }
    }
    available.sort();
    let listed = if available.is_empty() {
        "it has no version that is not yanked".to_string()
    } else {
        format!(
            "available: {}",
            available
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    Some(format!(
        "no published version of `{package}` satisfies `{req}` ({listed}{})",
        if yanked_match {
            "; a yanked version would match, and nobody can resolve to a yanked version"
        } else {
            ""
        }
    ))
}

/// Refuses a manifest whose dependencies cannot be resolved.
#[allow(
    clippy::single_call_fn,
    reason = "publication checks dependency identity against the catalog before opening a transaction"
)]
async fn check_dependencies(
    state: &AppState,
    manifest: &PackageManifest,
) -> ApiResult<()> {
    let dependencies = manifest.index_dependencies();
    if dependencies.len() > MAX_DEPENDENCIES {
        return Err(ApiError::bad_request(
            "E_DEPENDENCY",
            format!(
                "{} dependencies; a version may declare at most {MAX_DEPENDENCIES}",
                dependencies.len()
            ),
        ));
    }
    for dependency in dependencies {
        let id = dependency.package.parse::<PackageId>().map_err(|error| {
            ApiError::bad_request(
                "E_DEPENDENCY",
                format!("dependency `{}`: {}", dependency.package, error.message),
            )
        })?;
        let req = aevia_resolve::VersionReq::parse(&dependency.req).map_err(|error| {
            ApiError::bad_request("E_DEPENDENCY", format!("dependency `{id}`: {error}"))
        })?;
        let Some((package, _)) =
            repo::package_by_handle(&state.pool, id.owner(), &id.name_key()).await?
        else {
            return Err(ApiError::bad_request(
                "E_DEPENDENCY",
                format!(
                    "`{id}` is not a package on this registry, so `{}` cannot resolve",
                    dependency.label()
                ),
            ));
        };
        let versions: Vec<(String, bool)> = repo::versions(&state.pool, package.id)
            .await?
            .into_iter()
            .map(|row| (row.version, row.yanked))
            .collect();
        if let Some(problem) = dependency_problem(&id.to_string(), &req, &versions) {
            return Err(ApiError::bad_request("E_DEPENDENCY", problem));
        }
    }
    Ok(())
}

#[allow(
    clippy::single_call_fn,
    reason = "the signed checksum has an independent canonical wire format"
)]
fn check_checksum(checksum: &str) -> ApiResult<()> {
    let valid = checksum.strip_prefix("sha256:").is_some_and(|hex_part| {
        hex_part.len() == 64
            && hex_part
                .chars()
                .all(|character| character.is_ascii_hexdigit())
    });
    if valid {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "E_TARBALL",
            format!("`{checksum}` is not a `sha256:` digest"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn versions(list: &[(&str, bool)]) -> Vec<(String, bool)> {
        list.iter()
            .map(|(version, yanked)| ((*version).to_string(), *yanked))
            .collect()
    }

    fn req(text: &str) -> aevia_resolve::VersionReq {
        aevia_resolve::VersionReq::parse(text).expect("requirement")
    }

    #[test]
    fn a_requirement_with_a_live_match_is_resolvable() {
        let all = versions(&[("1.0.0", false), ("1.2.0", false), ("2.0.0", true)]);
        assert_eq!(dependency_problem("o/a", &req("^1"), &all), None);
        assert_eq!(dependency_problem("o/a", &req("*"), &all), None);
    }

    #[test]
    fn a_requirement_nothing_satisfies_is_explained_with_what_exists() {
        let all = versions(&[("1.0.0", false), ("1.2.0", false)]);
        let problem = dependency_problem("o/a", &req("^3"), &all).expect("unresolvable");
        assert!(
            problem.contains("`o/a`") && problem.contains("^3"),
            "{problem}"
        );
        assert!(problem.contains("available: 1.0.0, 1.2.0"), "{problem}");
    }

    #[test]
    fn a_requirement_only_a_yanked_version_satisfies_is_refused() {
        let all = versions(&[("1.0.0", false), ("2.0.0", true)]);
        let problem = dependency_problem("o/a", &req("^2"), &all).expect("unresolvable");
        assert!(problem.contains("yanked"), "{problem}");
        let none = versions(&[("1.0.0", true)]);
        let problem = dependency_problem("o/a", &req("*"), &none).expect("unresolvable");
        assert!(
            problem.contains("no version that is not yanked"),
            "{problem}"
        );
    }

    #[test]
    fn a_pre_release_satisfies_only_an_explicit_requirement() {
        let all = versions(&[("1.0.0-rc.1", false)]);
        assert!(dependency_problem("o/a", &req("^1"), &all).is_some());
        assert!(dependency_problem("o/a", &req(">=1.0.0-rc.1"), &all).is_none());
    }

    #[test]
    fn versions_may_be_pre_releases_but_not_carry_build_metadata() {
        assert!(check_version("1.2.3").is_ok());
        assert!(check_version("1.2.3-rc.1").is_ok());
        assert!(check_version("1.2.3+b1").is_err());
        assert!(check_version("1.2").is_err());
        assert!(check_version("v1.2.3").is_err());
    }
}
