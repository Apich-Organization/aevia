//! Small HTTP helpers: cookies, client addresses, random identifiers.
//!
//! They are here rather than pulled from a crate because each is a few lines
//! and each has one security-relevant decision that is better made explicitly:
//! the cookie flags, the trust boundary for `X-Forwarded-For`, and the source
//! of randomness for session ids and nonces.

use axum::extract::FromRequestParts;
use axum::extract::Request;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::Method;
use axum::http::StatusCode;
use axum::http::header;
use axum::http::header::SET_COOKIE;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::IntoResponse as _;
use axum::response::Response;

use aevia_registry_core::Lang;
use rand::RngCore as _;
use rand::seq::IndexedRandom as _;

use crate::error::ApiError;
use crate::error::PageError;
use crate::state::AppState;

/// The session cookie's name.
pub const SESSION_COOKIE: &str = "aevia_session";

/// Host-only session cookies from HTTPS deployments.
pub const SECURE_SESSION_COOKIE: &str = "__Host-aevia_session";

/// The cookie that remembers the reader's language.
pub const LANG_COOKIE: &str = "aevia_lang";

/// The path every cookie is scoped to.
const COOKIE_PATH: &str = "/";

/// Reads one cookie from the request.
#[must_use]
pub fn cookie(
    headers: &HeaderMap,
    name: &str,
) -> Option<String> {
    for part in headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
    {
        let part = part.trim();
        if let Some((key, value)) = part.split_once('=')
            && key.trim() == name
        {
            return Some(value.trim().to_string());
        }
    }
    None
}

/// Builds the `Set-Cookie` header for a session.
///
/// `HttpOnly` keeps the cookie away from scripts, `SameSite=Lax` stops a
/// cross-site form from acting as the user while still allowing a top-level
/// navigation back from GitHub or ORCID, and `Secure` is set whenever the
/// public origin is HTTPS — which it always is in production, and cannot be in
/// local development.
#[must_use]
pub fn session_cookie(
    id: &str,
    max_age_seconds: i64,
    secure: bool,
) -> HeaderValue {
    let name = if secure {
        SECURE_SESSION_COOKIE
    } else {
        SESSION_COOKIE
    };
    session_cookie_named(name, id, max_age_seconds, secure)
}

#[allow(
    clippy::single_call_fn,
    reason = "session cookies share attributes independently of their names"
)]
fn session_cookie_named(
    name: &str,
    id: &str,
    max_age_seconds: i64,
    secure: bool,
) -> HeaderValue {
    let mut value = format!(
        "{name}={id}; Path={COOKIE_PATH}; HttpOnly; SameSite=Lax; Max-Age={max_age_seconds}"
    );
    if secure {
        value.push_str("; Secure");
    }
    HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// Builds the `Set-Cookie` header that remembers a language.
///
/// A year, because a language preference is not a session: a reader who chose
/// Chinese should not be asked again next month. No `HttpOnly` — the value is
/// not a secret, and letting the page read it would allow a client-side switch
/// later without a round trip.
#[must_use]
pub fn language_cookie(
    code: &str,
    secure: bool,
) -> HeaderValue {
    let mut value =
        format!("{LANG_COOKIE}={code}; Path={COOKIE_PATH}; SameSite=Lax; Max-Age=31536000");
    if secure {
        value.push_str("; Secure");
    }
    HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// Builds the `Set-Cookie` header that ends a session.
#[must_use]
pub fn clear_session_cookie(secure: bool) -> HeaderValue {
    session_cookie("", 0, secure)
}

/// The `Set-Cookie` header name.
#[must_use]
pub const fn set_cookie_header() -> axum::http::HeaderName {
    SET_COOKIE
}

/// The language a request asks for, from its headers.
///
/// The same rule as the [`Language`] extractor, exposed as a function for the
/// places that hold a `HeaderMap` but cannot take an extractor — the 404
/// fallback and the error page.
#[must_use]
pub fn language_from(headers: &HeaderMap) -> Lang {
    cookie(headers, LANG_COOKIE)
        .and_then(|value| Lang::parse(&value))
        .or_else(|| {
            headers
                .get(axum::http::header::ACCEPT_LANGUAGE)
                .and_then(|value| value.to_str().ok())
                .and_then(Lang::from_accept_language)
        })
        .unwrap_or(Lang::En)
}

fn language_for(
    headers: &HeaderMap,
    query: Option<&str>,
) -> Lang {
    query
        .and_then(query_language)
        .unwrap_or_else(|| language_from(headers))
}

pub(crate) fn query_language(query: &str) -> Option<Lang> {
    url::form_urlencoded::parse(query.as_bytes())
        .find_map(|(name, value)| (name == "lang").then(|| Lang::parse(&value)).flatten())
}

/// The reader's language, from the cookie or the `Accept-Language` header.
///
/// The cookie wins: a reader who has explicitly chosen a language should not be
/// overruled by their browser's header on the next request. `Accept-Language`
/// is the first-visit guess, and English is the fallback because the API and
/// the code are English.
pub struct Language(pub Lang);

impl std::ops::Deref for Language {
    type Target = Lang;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Language {
    type Rejection = std::convert::Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        // `?lang=zh` beats the cookie: a link that says which language it wants
        // must win over a preference set months ago, or the link is a lie.
        std::future::ready(Ok(Self(language_for(&parts.headers, parts.uri.query()))))
    }
}

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data: https://avatars.githubusercontent.com; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'";

/// Rejects cross-origin browser writes and attaches the registry's response policies.
#[allow(
    clippy::single_call_fn,
    reason = "the middleware applies a single security-header policy to all routes"
)]
pub(crate) async fn site_policy(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let uri = request.uri().clone();
    let path = uri.path();
    let auth = path.starts_with("/auth/")
        || path.starts_with("/api/v1/auth/")
        || matches!(path, "/login" | "/logout" | "/activate");
    let browser_write = request.method() == Method::POST
        && (matches!(path, "/logout" | "/activate")
            || path.starts_with("/account/")
            || (path.starts_with("/crates/") && path.ends_with("/yank")));
    let lang = language_for(request.headers(), request.uri().query());
    let mut response = if !state.limits.allow(&request, state.config.trust_proxy) {
        let mut response = ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "E_RATE_LIMIT",
            "Too many requests. Try again in a minute.",
        )
        .into_response();
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        response
    } else if browser_write && !same_origin(request.headers(), &state.config.base_url) {
        PageError::forbidden(match lang {
            | Lang::En => "Send this form from the registry page.",
            | Lang::Zh => "请从注册中心页面提交此表单。",
        })
        .in_lang(lang)
        .into_response()
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    if is_secure(&state.config.base_url) {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static(if auth {
            "no-referrer"
        } else {
            "strict-origin-when-cross-origin"
        }),
    );
    let html = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html"));
    if html {
        headers.insert(
            header::CONTENT_LANGUAGE,
            HeaderValue::from_static(lang.code()),
        );
    }
    if html || auth || headers.contains_key(header::SET_COOKIE) {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-store"),
        );
    }
    if html {
        crate::seo::site_links(&state, &uri, lang, &mut response).await;
    }
    response
}

#[allow(
    clippy::single_call_fn,
    reason = "browser writes share an origin comparison policy"
)]
fn same_origin(
    headers: &HeaderMap,
    public_origin: &str,
) -> bool {
    if headers
        .get("sec-fetch-site")
        .is_some_and(|value| value != "same-origin")
    {
        return false;
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        return origin == public_origin
            || origin == "null"
                && headers
                    .get("sec-fetch-site")
                    .is_some_and(|value| value == "same-origin");
    }
    if let Some(referer) = headers.get(header::REFERER) {
        return referer
            .to_str()
            .ok()
            .and_then(|value| url::Url::parse(value).ok())
            .is_some_and(|url| url.origin().ascii_serialization() == public_origin);
    }
    headers
        .get("sec-fetch-site")
        .is_some_and(|value| value == "same-origin")
}

/// `n` random bytes, hex encoded.
///
/// `rand::rng()` is the operating system's generator, seeded per thread; the
/// values here are session ids, nonces and device codes, so anything weaker
/// would be a vulnerability rather than a style choice.
#[must_use]
pub fn random_hex(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// A short code a human can retype, `ABCD-EFGH`.
///
/// The alphabet omits `I`, `L`, `O`, `0` and `1`, which are the characters
/// people transcribe wrongly when reading a code off one screen and typing it
/// into another.
#[must_use]
pub fn user_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut rng = rand::rng();
    let mut code = String::with_capacity(9);
    for index in 0..8 {
        if index == 4 {
            code.push('-');
        }
        code.push(char::from(
            ALPHABET.choose(&mut rng).copied().unwrap_or(b'A'),
        ));
    }
    code
}

/// Normalises a user code typed in lower case or without its dash.
#[must_use]
pub fn normalize_user_code(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_uppercase())
        .collect();
    if let Some((left, right)) = cleaned.split_at_checked(4)
        && right.len() == 4
    {
        format!("{left}-{right}")
    } else {
        cleaned
    }
}

/// Whether the public origin is HTTPS, which decides the `Secure` flag.
#[must_use]
pub fn is_secure(base_url: &str) -> bool {
    base_url.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_language_cookie_is_a_preference() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_static("aevia_lang=zh"),
        );
        assert_eq!(
            cookie(&headers, LANG_COOKIE).and_then(|value| Lang::parse(&value)),
            Some(Lang::Zh)
        );
    }

    #[test]
    fn browser_form_origins_are_checked_against_the_public_origin() {
        let public_origin = "https://aevia.example";
        let mut headers = HeaderMap::new();
        headers.insert(header::ORIGIN, HeaderValue::from_static(public_origin));
        assert!(same_origin(&headers, public_origin));
        headers.insert("sec-fetch-site", HeaderValue::from_static("same-site"));
        assert!(!same_origin(&headers, public_origin));
        headers.remove("sec-fetch-site");
        headers.insert(header::ORIGIN, HeaderValue::from_static("null"));
        assert!(!same_origin(&headers, public_origin));
        headers.remove(header::ORIGIN);
        headers.insert(
            header::REFERER,
            HeaderValue::from_static("https://aevia.example/a?b=1"),
        );
        assert!(same_origin(&headers, public_origin));
    }

    #[test]
    fn a_cookie_is_found_among_others() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_static("other=1; aevia_session=abc123; last=2"),
        );
        assert_eq!(cookie(&headers, SESSION_COOKIE).as_deref(), Some("abc123"));
        assert_eq!(cookie(&headers, "missing"), None);
    }

    #[test]
    fn the_session_cookie_carries_its_flags() {
        let value = session_cookie("abc", 60, true);
        let text = value.to_str().expect("ascii");
        assert!(text.contains("HttpOnly"));
        assert!(text.contains("SameSite=Lax"));
        assert!(text.contains("Secure"));
        assert!(text.contains("Max-Age=60"));
    }

    #[test]
    fn a_local_session_is_not_marked_secure() {
        let value = session_cookie("abc", 60, false);
        assert!(!value.to_str().expect("ascii").contains("Secure"));
    }

    #[test]
    fn the_user_code_alphabet_has_no_confusable_characters() {
        for _ in 0..64 {
            let code = user_code();
            assert_eq!(code.len(), 9);
            assert_eq!(&code[4..5], "-");
            for character in code.chars().filter(|c| *c != '-') {
                assert!(
                    !matches!(character, 'I' | 'L' | 'O' | '0' | '1'),
                    "{code} contains {character}"
                );
            }
        }
    }

    #[test]
    fn a_user_code_is_forgiving_about_how_it_is_typed() {
        assert_eq!(normalize_user_code("abcd efgh"), "ABCD-EFGH");
        assert_eq!(normalize_user_code("ABCD-EFGH"), "ABCD-EFGH");
    }

    #[test]
    fn random_hex_has_the_requested_length() {
        assert_eq!(random_hex(16).len(), 32);
        assert_ne!(random_hex(16), random_hex(16));
    }
}
