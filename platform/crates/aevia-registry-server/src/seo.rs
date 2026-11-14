//! Machine-facing pages: the release feed, the sitemap, `robots.txt` and the
//! package badge.
//!
//! None of these has a language or a viewer, so none goes through the page
//! layer; each is a small, escaped string.

use axum::extract::Path;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;
use axum::response::Response;

use aevia_registry_core::PackageId;
use aevia_registry_db::repo;

use crate::error::PageError;
use crate::error::PageResult;
use crate::state::AppState;

/// Escapes text for an XML element or attribute.
#[must_use]
pub fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            | '&' => out.push_str("&amp;"),
            | '<' => out.push_str("&lt;"),
            | '>' => out.push_str("&gt;"),
            | '"' => out.push_str("&quot;"),
            | '\'' => out.push_str("&apos;"),
            | other => out.push(other),
        }
    }
    out
}

/// `GET /robots.txt`
pub async fn robots(State(state): State<AppState>) -> Response {
    let base = state.config.base_url.trim_end_matches('/');
    let body = format!(
        "User-agent: *\nAllow: /\nDisallow: /account\nDisallow: /activate\nDisallow: /auth/\n\
         Disallow: /api/\nSitemap: {base}/sitemap.xml\n"
    );
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

/// `GET /sitemap.xml`
///
/// # Errors
///
/// `E_DB` when the packages cannot be listed.
pub async fn sitemap(State(state): State<AppState>) -> PageResult<Response> {
    let base = state.config.base_url.trim_end_matches('/').to_string();
    let (packages, _) = repo::list_packages(&state.pool, "", "name", 5000, 0).await?;
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for path in ["/", "/crates", "/docs", "/cli", "/spec", "/log"] {
        xml.push_str(&format!("  <url><loc>{}{path}</loc></url>\n", xml_escape(&base)));
    }
    for package in &packages {
        let id = format!("{}/{}", package.owner_handle, package.name);
        xml.push_str(&format!(
            "  <url><loc>{}/crates/{}</loc><lastmod>{}</lastmod></url>\n",
            xml_escape(&base),
            xml_escape(&id),
            package.updated_at.format("%Y-%m-%d"),
        ));
        if package.has_docs {
            xml.push_str(&format!(
                "  <url><loc>{}/docs/{}</loc></url>\n",
                xml_escape(&base),
                xml_escape(&id)
            ));
        }
    }
    xml.push_str("</urlset>\n");
    Ok(([(header::CONTENT_TYPE, "application/xml; charset=utf-8")], xml).into_response())
}

/// `GET /feed.xml` — an Atom feed of the newest releases.
///
/// # Errors
///
/// `E_DB` when the releases cannot be read.
pub async fn feed(State(state): State<AppState>) -> PageResult<Response> {
    let base = state.config.base_url.trim_end_matches('/').to_string();
    let releases = repo::recent_releases(&state.pool, 50).await?;
    let updated = releases
        .first()
        .map_or_else(|| "1970-01-01T00:00:00Z".to_string(), |release| release.created_at.to_rfc3339());
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <feed xmlns=\"http://www.w3.org/2005/Atom\">\n\
         \x20 <title>Aevia registry: new releases</title>\n\
         \x20 <id>{base}/feed.xml</id>\n\
         \x20 <link rel=\"self\" href=\"{base}/feed.xml\"/>\n\
         \x20 <link href=\"{base}/crates\"/>\n\
         \x20 <updated>{updated}</updated>\n",
        base = xml_escape(&base),
    );
    for release in &releases {
        let id = format!("{}/{}", release.owner_handle, release.name);
        let url = format!("{base}/crates/{id}/{}", release.version);
        xml.push_str(&format!(
            "  <entry>\n    <title>{title}</title>\n    <id>{url}</id>\n    \
             <link href=\"{url}\"/>\n    <updated>{when}</updated>\n    \
             <summary>{summary}</summary>\n  </entry>\n",
            title = xml_escape(&format!("{id} {}", release.version)),
            url = xml_escape(&url),
            when = release.created_at.to_rfc3339(),
            summary = xml_escape(release.description.as_deref().unwrap_or("")),
        ));
    }
    xml.push_str("</feed>\n");
    Ok(([(header::CONTENT_TYPE, "application/atom+xml; charset=utf-8")], xml).into_response())
}

/// A flat two-part badge.
///
/// The widths are estimated from the character count (about 6.5 px per
/// character at 11 px): exact text metrics would need a font, and a badge a
/// pixel or two too wide is still a badge.
#[must_use]
pub fn badge_svg(
    label: &str,
    value: &str,
    colour: &str,
) -> String {
    let width = |text: &str| u32::try_from(text.chars().count()).unwrap_or(0) * 7 + 12;
    let (left, right) = (width(label), width(value));
    let label = xml_escape(label);
    let value = xml_escape(value);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{total}\" height=\"20\" role=\"img\" \
         aria-label=\"{label}: {value}\"><rect width=\"{left}\" height=\"20\" fill=\"#555\"/>\
         <rect x=\"{left}\" width=\"{right}\" height=\"20\" fill=\"{colour}\"/>\
         <g fill=\"#fff\" font-family=\"Verdana,sans-serif\" font-size=\"11\" text-anchor=\"middle\">\
         <text x=\"{lx}\" y=\"14\">{label}</text><text x=\"{rx}\" y=\"14\">{value}</text></g></svg>",
        total = left + right,
        lx = left / 2,
        rx = left + right / 2,
    )
}

/// `GET /badge/{owner}/{name}` — `aevia | 1.2.0`, for a README.
///
/// # Errors
///
/// `404` when the package does not exist.
pub async fn badge(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
) -> PageResult<Response> {
    let name = name.strip_suffix(".svg").unwrap_or(&name).to_string();
    let id = PackageId::new(&owner, &name)?;
    let Some((package, _)) =
        repo::package_by_handle(&state.pool, id.owner(), &id.name_key()).await?
    else {
        return Err(PageError::not_found(format!(
            "There is no package called {id}."
        )));
    };
    let latest = repo::latest_version(&state.pool, package.id).await?;
    let svg = match latest {
        | Some(version) => badge_svg("aevia", &version.version, "#2d7d46"),
        | None => badge_svg("aevia", "no release", "#9f9f9f"),
    };
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        svg,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_text_is_escaped() {
        assert_eq!(xml_escape("a<b>&\"'"), "a&lt;b&gt;&amp;&quot;&apos;");
    }

    #[test]
    fn a_badge_names_the_version_and_escapes_it() {
        let svg = badge_svg("aevia", "1.2.0-rc.1<x>", "#2d7d46");
        assert!(svg.contains("aevia: 1.2.0-rc.1&lt;x&gt;"));
        assert!(svg.starts_with("<svg"));
        assert!(!svg.contains("<x>"));
    }
}
