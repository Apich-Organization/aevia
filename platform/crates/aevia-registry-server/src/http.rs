//! Small HTTP helpers: cookies, client addresses, random identifiers.
//!
//! They are here rather than pulled from a crate because each is a few lines
//! and each has one security-relevant decision that is better made explicitly:
//! the cookie flags, the trust boundary for `X-Forwarded-For`, and the source
//! of randomness for session ids and nonces.

use axum::extract::FromRequestParts;
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::header::SET_COOKIE;
use axum::http::request::Parts;

use aevia_registry_core::Lang;
use rand::RngCore as _;

/// The session cookie's name.
pub const SESSION_COOKIE: &str = "aevia_session";

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
    let header = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for part in header.split(';') {
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
    let mut value = format!(
        "{SESSION_COOKIE}={id}; Path={COOKIE_PATH}; HttpOnly; SameSite=Lax; Max-Age={max_age_seconds}"
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
pub fn clear_session_cookie() -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}=; Path={COOKIE_PATH}; HttpOnly; SameSite=Lax; Max-Age=0"
    ))
    .unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// The `Set-Cookie` header name.
#[must_use]
pub fn set_cookie_header() -> axum::http::HeaderName {
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

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        // `?lang=zh` beats the cookie: a link that says which language it wants
        // must win over a preference set months ago, or the link is a lie.
        let from_query = parts
            .uri
            .query()
            .and_then(|query| query.split('&').find_map(|pair| pair.strip_prefix("lang=")))
            .and_then(Lang::parse);
        Ok(Self(
            from_query.unwrap_or_else(|| language_from(&parts.headers)),
        ))
    }
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
    let mut bytes = [0u8; 8];
    rand::rng().fill_bytes(&mut bytes);
    let mut code = String::with_capacity(9);
    for (index, byte) in bytes.iter().enumerate() {
        if index == 4 {
            code.push('-');
        }
        code.push(char::from(ALPHABET[usize::from(*byte) % ALPHABET.len()]));
    }
    code
}

/// Normalises a user code typed in lower case or without its dash.
#[must_use]
pub fn normalize_user_code(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_uppercase())
        .collect();
    if cleaned.len() == 8 {
        format!("{}-{}", &cleaned[..4], &cleaned[4..])
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
