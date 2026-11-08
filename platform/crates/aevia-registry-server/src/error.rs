//! Two error shapes: JSON for the API, HTML for the pages.
//!
//! The API and the site serve the same failures to different audiences, so the
//! same `aevia_registry_core::Error` is wrapped twice. The mapping from code to
//! status is written once, here, so a new code cannot accidentally become a 500
//! just because nobody remembered to map it.

use axum::Json;
use axum::http::StatusCode;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Response;

use aevia_registry_core::Error;

/// An API failure, rendered as `{"error":{"code":…,"message":…}}`.
#[derive(Debug)]
pub struct ApiError {
    /// The status the client sees.
    pub status: StatusCode,
    /// The registry error.
    pub error: Error,
}

impl ApiError {
    /// Builds an error from its parts.
    #[must_use]
    pub fn new(
        status: StatusCode,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status,
            error: Error::new(code, message),
        }
    }

    /// `400`: the request is malformed or the statement is wrong.
    #[must_use]
    pub fn bad_request(
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    /// `403`: the signature or the key does not authorise this action.
    #[must_use]
    pub fn forbidden(
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, message)
    }

    /// `404`: no such package or version.
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "E_NOT_FOUND", message)
    }

    /// `409`: the version is already published.
    #[must_use]
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "E_CONFLICT", message)
    }

    /// `500`: something the client cannot fix.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "E_DB", message)
    }

    /// Maps a core error to the status its code implies.
    #[must_use]
    pub fn from_core(error: Error) -> Self {
        let status = match error.code {
            | "E_NOT_FOUND" => StatusCode::NOT_FOUND,
            | "E_CONFLICT" => StatusCode::CONFLICT,
            | "E_SIGNATURE" | "E_KEY" | "E_FORBIDDEN" => StatusCode::FORBIDDEN,
            | "E_DB" | "E_IO" | "E_DOCS" => StatusCode::INTERNAL_SERVER_ERROR,
            | _ => StatusCode::BAD_REQUEST,
        };
        Self { status, error }
    }
}

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        Self::from_core(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if self.status.is_server_error() {
            tracing::error!(code = self.error.code, message = %self.error.message, "request failed");
        }
        let body = serde_json::json!({
            "error": { "code": self.error.code, "message": self.error.message }
        });
        (self.status, Json(body)).into_response()
    }
}

/// A page failure, rendered as the site's error page.
#[derive(Debug)]
pub struct PageError {
    /// The status the browser sees.
    pub status: StatusCode,
    /// A machine code, shown in small text.
    pub code: String,
    /// The sentence shown to the reader.
    pub message: String,
}

impl PageError {
    /// `404`, with a sentence that says what was not found.
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "E_NOT_FOUND".to_string(),
            message: message.into(),
        }
    }

    /// `403`.
    #[must_use]
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "E_FORBIDDEN".to_string(),
            message: message.into(),
        }
    }

    /// `500`.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "E_DB".to_string(),
            message: message.into(),
        }
    }
}

impl From<Error> for PageError {
    fn from(error: Error) -> Self {
        let status = ApiError::from_core(error.clone()).status;
        Self {
            status,
            code: error.code.to_string(),
            message: error.message,
        }
    }
}

impl IntoResponse for PageError {
    fn into_response(self) -> Response {
        if self.status.is_server_error() {
            tracing::error!(code = %self.code, message = %self.message, "page failed");
        }
        let html =
            aevia_registry_web::error_page(self.status.as_u16(), &self.code, &self.message, None);
        (self.status, Html(html)).into_response()
    }
}

/// The API's result type.
pub type ApiResult<T> = Result<T, ApiError>;

/// The pages' result type.
pub type PageResult<T> = Result<T, PageError>;
