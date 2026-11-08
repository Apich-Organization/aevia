//! Publishing: the challenge, the signature, the archive.
//!
//! The order of checks is the design. Nothing is written to the catalogue until
//! the bytes have been hashed, the statement has been re-read, the nonce has
//! been spent, the key has been found, the signature has verified, the scope
//! has been authorised and the manifest inside the archive has been compared
//! with the statement. A publish that fails any of those leaves no trace except
//! a spent nonce, which is exactly what a replay attempt should cost.

use axum::Json;
use axum::extract::Multipart;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use aevia_registry_core::PackageId;
use aevia_registry_core::manifest::PackageManifest;
use aevia_registry_core::manifest::RegistryMeta;
use aevia_registry_core::manifest::first_paragraph;
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
    let expires = Utc::now().timestamp() + STATEMENT_LIFETIME_SECONDS;
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
    /// An override for the description discovered from the README.
    pub description: Option<String>,
    /// An override for the detected licence.
    pub license: Option<String>,
    /// Up to five keywords.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Where the source lives.
    pub repository: Option<String>,
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
    let mut envelope: Option<Envelope> = None;
    let mut archive: Option<Vec<u8>> = None;
    while let Some(field) = multipart.next_field().await.map_err(|error| {
        ApiError::bad_request("E_MULTIPART", format!("malformed upload: {error}"))
    })? {
        match field.name() {
            | Some("envelope") => {
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
            | Some("package") => {
                let bytes = field.bytes().await.map_err(|error| {
                    ApiError::bad_request(
                        "E_MULTIPART",
                        format!("cannot read the archive: {error}"),
                    )
                })?;
                archive = Some(bytes.to_vec());
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
    let checksum = tarball::checksum(&archive);
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
    let files = tarball::text_files(&archive).map_err(ApiError::from_core)?;
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

    // Metadata: the envelope wins, then `aevia-pkg.toml`, then the README.
    let declared = files
        .iter()
        .find(|(name, _)| name == "aevia-pkg.toml")
        .and_then(|(_, text)| RegistryMeta::parse(text, "aevia-pkg.toml").ok())
        .unwrap_or_default();
    let readme = files
        .iter()
        .find(|(name, _)| {
            name.eq_ignore_ascii_case("readme.md") || name.eq_ignore_ascii_case("readme")
        })
        .map(|(_, text)| text.clone());
    let description = envelope
        .description
        .clone()
        .or(declared.description.clone())
        .or_else(|| readme.as_deref().and_then(first_paragraph));
    let license = envelope.license.clone().or(declared.license.clone());
    let repository = envelope.repository.clone().or(declared.repository.clone());
    let keywords: Vec<String> = if envelope.keywords.is_empty() {
        declared.keywords.clone()
    } else {
        envelope.keywords.clone()
    };

    // The catalogue rows.
    let package =
        match repo::package_by_identity(&state.pool, owner_id, &statement.name.name_key()).await? {
            | Some(package) => package,
            | None => {
                repo::create_package(
                    &state.pool,
                    owner_id,
                    statement.name.name(),
                    &statement.name.name_key(),
                )
                .await?
            },
        };
    let dependencies = serde_json::to_value(&manifest.dependencies).map_err(|error| {
        ApiError::internal(format!("cannot serialise the dependency list: {error}"))
    })?;
    let version = repo::insert_version(
        &state.pool,
        &NewVersion {
            package_id: package.id,
            version: &statement.version,
            checksum: &checksum,
            size_bytes: i64::try_from(archive.len()).unwrap_or(i64::MAX),
            manifest: serde_json::json!({
                "name": manifest.name,
                "version": manifest.version,
                "entry": manifest.entry,
            }),
            dependencies,
            readme: readme.as_deref(),
            published_by: publisher.id,
            key_kind: &key_kind,
            fingerprint: &fingerprint,
            signature: &envelope.signature,
            statement: &envelope.statement,
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
        &state.pool,
        package.id,
        description.as_deref(),
        license.as_deref(),
        repository.as_deref(),
        &keywords,
    )
    .await?;
    repo::touch_package(&state.pool, package.id).await?;

    // The bytes. Written after the row so a failed write leaves a row whose
    // archive is missing — a loud inconsistency — rather than an orphan file.
    state
        .storage
        .write(&statement.name, &statement.version, &archive)?;

    let entry = repo::append_log(
        &state.pool,
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

    // Documentation, off the request path.
    if state.config.docs_enabled {
        spawn_docs_build(&state, version.id, statement.name.clone(), files);
    } else {
        repo::set_docs_status(&state.pool, version.id, "skipped", None).await?;
    }

    let docs_url = if state.config.docs_enabled {
        Some(format!("/docs/{}/{}", statement.name, statement.version))
    } else {
        None
    };
    Ok((
        StatusCode::CREATED,
        Json(PublishResponse {
            name: statement.name.to_string(),
            version: statement.version,
            checksum,
            docs_url,
            log: LogRef {
                seq: entry.seq,
                hash: entry.entry_hash,
            },
        }),
    ))
}

/// Builds documentation in the background.
///
/// `spawn_blocking` because the compiler is synchronous and CPU-bound; a
/// registry that rendered docs on the request thread would make a publish wait
/// for a full parse of the package.
fn spawn_docs_build(
    state: &AppState,
    version_id: i64,
    name: PackageId,
    files: Vec<(String, String)>,
) {
    let pool = state.pool.clone();
    let package = name.to_string();
    tokio::spawn(async move {
        let built =
            tokio::task::spawn_blocking(move || aevia_registry_docs::build(&files, &package)).await;
        match built {
            | Ok(Ok(docs)) => {
                let pages: Vec<aevia_registry_db::models::DocsPage> = docs
                    .pages
                    .iter()
                    .map(|page| aevia_registry_db::models::DocsPage {
                        version_id,
                        path: page.path.clone(),
                        title: page.title.clone(),
                        html: page.html.clone(),
                        decls: i32::try_from(page.decls).unwrap_or(i32::MAX),
                        undocumented: i32::try_from(page.undocumented).unwrap_or(i32::MAX),
                        source_file: page.source_file.clone(),
                    })
                    .collect();
                if let Err(error) = repo::replace_docs(&pool, version_id, &pages).await {
                    tracing::error!(%error, version_id, "cannot store the documentation");
                    let _ =
                        repo::set_docs_status(&pool, version_id, "failed", Some(&error.message))
                            .await;
                    return;
                }
                if let Err(error) = repo::set_docs_status(&pool, version_id, "built", None).await {
                    tracing::error!(%error, version_id, "cannot record the documentation state");
                }
            },
            | Ok(Err(error)) => {
                tracing::warn!(%error, version_id, "documentation failed to build");
                let _ =
                    repo::set_docs_status(&pool, version_id, "failed", Some(&error.message)).await;
            },
            | Err(error) => {
                tracing::error!(%error, version_id, "the documentation task panicked");
                let _ = repo::set_docs_status(
                    &pool,
                    version_id,
                    "failed",
                    Some("the documentation task did not finish"),
                )
                .await;
            },
        }
    });
}

fn check_version(version: &str) -> ApiResult<()> {
    let parsed = semver::Version::parse(version).map_err(|error| {
        ApiError::bad_request(
            "E_VERSION",
            format!("`{version}` is not a version: {error}"),
        )
    })?;
    if !parsed.pre.is_empty() || !parsed.build.is_empty() {
        return Err(ApiError::bad_request(
            "E_VERSION",
            format!(
                "`{version}` carries a pre-release or build suffix; `aevia.toml` writes \
                 `major.minor.patch` only"
            ),
        ));
    }
    Ok(())
}

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
