//! Small HTTP helpers: cookies, client addresses, random identifiers.
//!
//! They are here rather than pulled from a crate because each is a few lines
//! and each has one security-relevant decision that is better made explicitly:
//! the cookie flags, the trust boundary for `X-Forwarded-For`, and the source
//! of randomness for session ids and nonces.

use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::header::SET_COOKIE;
use rand::RngCore as _;

/// The session cookie's name.
pub const SESSION_COOKIE: &str = "aevia_session";

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
            && key.trim() == name {
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
