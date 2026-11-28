//! Yanking: the only mutation a published version allows.
//!
//! A version is never deleted. Deleting one would break every lock file that
//! names it, and it would make the transparency log a lie — the log says the
//! version existed. Yanking marks it as not-to-be-resolved, which keeps old
//! builds working while steering new resolutions away. `--undo` restores it,
//! because a yank made in error should be repairable without a new version.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use aevia_registry_core::PackageId;
use aevia_registry_core::signature::SignerKey;
use aevia_registry_core::statement::STATEMENT_LIFETIME_SECONDS;
use aevia_registry_core::statement::YankStatement;
use aevia_registry_db::repo;

use crate::api::ApiError;
use crate::api::ApiResult;
use crate::api::authorise;
use crate::http;
use crate::state::AppState;

/// The body of a yank challenge request.
#[derive(Deserialize)]
pub struct ChallengeRequest {
    /// `owner/name`.
    pub name: String,
    /// The version to yank or restore.
    pub version: String,
    /// `true` to yank, `false` to restore.
    pub yanked: bool,
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

/// The body of a yank.
#[derive(Deserialize)]
pub struct YankRequest {
    /// The statement the server issued.
    pub statement: String,
    /// The detached signature over it.
    pub signature: String,
    /// The key that signed.
    pub key: SignerKey,
}

/// `POST /api/v1/yank/challenge`
///
/// # Errors
///
/// `400` for a malformed name or version.
pub async fn challenge(
    State(state): State<AppState>,
    Json(request): Json<ChallengeRequest>,
) -> ApiResult<Json<ChallengeResponse>> {
    let id = request
        .name
        .parse::<PackageId>()
        .map_err(ApiError::from_core)?;
    super::publish::check_version(&request.version)?;
    let nonce = http::random_hex(16);
    let expires = Utc::now()
        .timestamp()
        .saturating_add(STATEMENT_LIFETIME_SECONDS);
    let statement =
        YankStatement::new(id, request.version, request.yanked, nonce.clone(), expires).render();
    let expires_at = chrono::DateTime::from_timestamp(expires, 0).unwrap_or_else(Utc::now);
    repo::create_challenge(&state.pool, &nonce, "yank", &statement, expires_at).await?;
    Ok(Json(ChallengeResponse {
        nonce,
        statement,
        expires_at: expires_at.to_rfc3339(),
    }))
}

/// `POST /api/v1/yank`
///
/// # Errors
///
/// `403` when the signer does not own the package, `404` when the version does
/// not exist.
pub async fn yank(
    State(state): State<AppState>,
    Json(request): Json<YankRequest>,
) -> ApiResult<StatusCode> {
    let authorised = authorise(
        &state,
        "yank",
        &request.statement,
        &request.signature,
        &request.key,
    )
    .await?;
    let aevia_registry_core::statement::Statement::Yank(statement) = authorised.statement else {
        return Err(ApiError::bad_request("E_STATEMENT", "not a yank statement"));
    };
    let Some((package, _)) = repo::package_by_handle(
        &state.pool,
        statement.name.owner(),
        &statement.name.name_key(),
    )
    .await?
    else {
        return Err(ApiError::not_found(format!(
            "There is no package called {}.",
            statement.name
        )));
    };
    // Only the owner may yank. `resolve_publishing_owner` already checks that
    // the signer may publish under this scope, which is the same authority.
    let Some(owner_id) = repo::resolve_publishing_owner(
        &state.pool,
        authorised.publisher.id,
        statement.name.owner(),
    )
    .await?
    else {
        return Err(ApiError::forbidden(
            "E_FORBIDDEN",
            format!(
                "{} may not change {}",
                authorised.publisher.handle, statement.name
            ),
        ));
    };
    if owner_id != package.owner_id {
        return Err(ApiError::forbidden(
            "E_FORBIDDEN",
            "the package belongs to a different publisher",
        ));
    }
    transition(
        &state,
        YankChange {
            package_id: package.id,
            name: &statement.name,
            version: &statement.version,
            yanked: statement.yanked,
            publisher: &authorised.publisher.handle,
            fingerprint: Some(&authorised.fingerprint),
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) struct YankChange<'a> {
    pub package_id: i64,
    pub name: &'a PackageId,
    pub version: &'a str,
    pub yanked: bool,
    pub publisher: &'a str,
    pub fingerprint: Option<&'a str>,
}

pub(crate) async fn transition(
    state: &AppState,
    change: YankChange<'_>,
) -> ApiResult<()> {
    // The flag, the package's `updated_at` and the log link change together:
    // the sparse index is derived from the row, and a yank the log does not
    // mention (or a log entry for a yank that did not happen) would make the
    // transparency log lie about what clients are being told.
    let mut transaction = state.pool.begin().await.map_err(|error| {
        ApiError::from_core(aevia_registry_db::sql_error(
            "cannot start the yank transaction",
            error,
        ))
    })?;
    repo::lock_package(&mut transaction, change.package_id).await?;
    let Some((row, changed)) = repo::set_yanked_with_changed(
        &mut *transaction,
        change.package_id,
        change.version,
        change.yanked,
    )
    .await?
    else {
        return Err(ApiError::not_found(format!(
            "{} has no version {}.",
            change.name, change.version
        )));
    };
    if changed {
        repo::touch_package(&mut *transaction, change.package_id).await?;
        let verb = if change.yanked {
            "yanked"
        } else {
            "restored"
        };
        let mut payload = serde_json::Map::from_iter([
            ("name".into(), serde_json::json!(change.name.to_string())),
            ("version".into(), serde_json::json!(row.version)),
            ("yanked".into(), serde_json::json!(change.yanked)),
            ("publisher".into(), serde_json::json!(change.publisher)),
        ]);
        if let Some(fingerprint) = change.fingerprint {
            payload.insert("fingerprint".into(), serde_json::json!(fingerprint));
        } else {
            payload.insert("via".into(), serde_json::json!("browser"));
        }
        repo::append_log_in(
            &mut transaction,
            "yank",
            &format!(
                "{} {verb} {} {}",
                change.publisher, change.name, row.version
            ),
            serde_json::Value::Object(payload),
        )
        .await?;
    }
    transaction.commit().await.map_err(|error| {
        ApiError::from_core(aevia_registry_db::sql_error(
            "cannot commit the yank",
            error,
        ))
    })?;
    Ok(())
}
