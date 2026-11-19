//! Canonical links and machine-readable registry and documentation resources.

use axum::extract::Path;
use axum::extract::State;
use axum::http::HeaderValue;
use axum::http::StatusCode;
use axum::http::Uri;
use axum::http::header;
use axum::response::IntoResponse;
use axum::response::Response;
use std::fmt::Write as _;

use aevia_registry_core::Lang;
use aevia_registry_core::PackageId;
use aevia_registry_core::vm::ManualKind;
use aevia_registry_db::repo;

use crate::error::PageError;
use crate::error::PageResult;
use crate::http::Language;
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
         Disallow: /api/v1/auth/\nSitemap: {base}/sitemap.xml\n"
    );
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

const LANDING: [&str; 3] = ["/", "/cli", "/docs"];

/// Routes that serve first-party text and have reproducible language URLs.
fn handbook_path(path: &str) -> bool {
    ["/book", "/architecture", "/spec", "/stdlib"]
        .into_iter()
        .any(|base| {
            path == base
                || path
                    .strip_prefix(base)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        })
}

/// Assigns canonical and alternate links only to public HTML representations.
#[allow(
    clippy::single_call_fn,
    reason = "canonical links and indexing policy are shared across HTML routes"
)]
pub(crate) async fn site_links(
    state: &AppState,
    uri: &Uri,
    lang: Lang,
    response: &mut Response,
) {
    let path = uri.path();
    let private = matches!(path, "/account" | "/login" | "/logout" | "/activate")
        || path.starts_with("/auth/")
        || path.starts_with("/account/");
    let duplicate = handbook_path(path) && (path.ends_with("/all") || path.ends_with("/download"));
    if private
        || duplicate
        || (path == "/crates"
            && uri.query().is_some_and(|query| {
                url::form_urlencoded::parse(query.as_bytes()).any(|(name, _)| name != "lang")
            }))
    {
        response
            .headers_mut()
            .insert("x-robots-tag", HeaderValue::from_static("noindex"));
        return;
    }
    if !response.status().is_success()
        || !path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/.-_+%~".contains(&byte))
    {
        return;
    }

    let origin = &state.config.base_url;
    let (base, edition, bilingual, json_alternate) = if let Some(slug) = path.strip_prefix("/spec/")
    {
        let translated = state.spec.read().await.markdown(slug, Lang::En).is_some();
        let language = if translated { lang } else { Lang::Zh };
        (format!("{origin}{path}"), Some(language), translated, false)
    } else if path == "/stdlib" || path.starts_with("/stdlib/") {
        let stdlib = state.stdlib_docs.read().await.clone();
        let translated = stdlib.is_bilingual();
        let language = if translated { lang } else { Lang::En };
        let base = path
            .strip_prefix("/stdlib/")
            .and_then(|slug| stdlib.book().module(slug))
            .map_or_else(
                || format!("{origin}{path}"),
                |module| format!("{origin}/stdlib/{}", module.slug),
            );
        (
            base,
            Some(language),
            translated,
            path == "/stdlib" && stdlib.catalog().is_some(),
        )
    } else if LANDING.contains(&path) || handbook_path(path) {
        (format!("{origin}{path}"), Some(lang), true, false)
    } else {
        (format!("{origin}{path}"), None, false, false)
    };
    let canonical = edition.map_or_else(
        || base.clone(),
        |edition| format!("{base}?lang={}", edition.code()),
    );
    let headers = response.headers_mut();
    if let Ok(link) = HeaderValue::from_str(&format!("<{canonical}>; rel=canonical")) {
        headers.append(header::LINK, link);
    }
    if bilingual {
        for (locale, edition) in [("en", "en"), ("zh", "zh"), ("x-default", "en")] {
            let alternate = format!("<{base}?lang={edition}>; rel=alternate; hreflang={locale}");
            if let Ok(link) = HeaderValue::from_str(&alternate) {
                headers.append(header::LINK, link);
            }
        }
    }
    let alternate = if json_alternate {
        Some(format!(
            "<{base}/catalog.json>; rel=alternate; type=\"application/json\""
        ))
    } else if (path.starts_with("/book/")
        || path.starts_with("/architecture/")
        || path.starts_with("/spec/") && (bilingual || lang == Lang::Zh))
        && !path.ends_with('/')
    {
        Some(format!(
            "<{base}.md?lang={}>; rel=alternate; type=\"text/markdown\"",
            lang.code()
        ))
    } else {
        None
    };
    if let Some(alternate) = alternate
        && let Ok(link) = HeaderValue::from_str(&alternate)
    {
        headers.append(header::LINK, link);
    }
}

fn sitemap_entry(
    xml: &mut String,
    url: &str,
    lastmod: Option<&str>,
) {
    let _ = write!(xml, "  <url><loc>{}</loc>", xml_escape(url));
    if let Some(date) = lastmod {
        let _ = write!(xml, "<lastmod>{date}</lastmod>");
    }
    xml.push_str("</url>\n");
}

fn languages(
    xml: &mut String,
    url: &str,
    english: bool,
    chinese: bool,
) {
    if english {
        sitemap_entry(xml, &format!("{url}?lang=en"), None);
    }
    if chinese {
        sitemap_entry(xml, &format!("{url}?lang=zh"), None);
    }
}

/// `GET /sitemap.xml`
///
/// # Errors
///
/// `E_DB` when the packages cannot be listed.
pub async fn sitemap(State(state): State<AppState>) -> PageResult<Response> {
    let base = &state.config.base_url;
    let (packages, _) = repo::list_packages(&state.pool, "", "name", 5000, 0).await?;
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for path in LANDING {
        languages(&mut xml, &format!("{base}{path}"), true, true);
    }
    for path in ["/crates", "/log"] {
        sitemap_entry(&mut xml, &format!("{base}{path}"), None);
    }
    {
        let spec = state.spec.read().await.clone();
        if !spec.is_empty() {
            languages(&mut xml, &format!("{base}/spec"), true, true);
            for chapter in &spec.book().chapters {
                languages(
                    &mut xml,
                    &format!("{base}/spec/{}", chapter.slug),
                    chapter.html_en.is_some(),
                    true,
                );
            }
        }
        let stdlib = state.stdlib_docs.read().await.clone();
        if !stdlib.is_empty() {
            languages(
                &mut xml,
                &format!("{base}/stdlib"),
                true,
                stdlib.is_bilingual(),
            );
            for module in &stdlib.book().modules {
                languages(
                    &mut xml,
                    &format!("{base}/stdlib/{}", module.slug),
                    true,
                    stdlib.is_bilingual(),
                );
            }
        }
        let manuals = state.manuals.read().await.clone();
        for kind in [ManualKind::Book, ManualKind::Architecture] {
            let chapters = manuals.edition(kind, Lang::En);
            if !chapters.is_empty() {
                let url = format!("{base}{}", kind.route());
                languages(&mut xml, &url, true, true);
                for chapter in chapters {
                    languages(&mut xml, &format!("{url}/{}", chapter.slug), true, true);
                }
            }
        }
    }
    for package in &packages {
        let id = format!("{}/{}", package.owner_handle, package.name);
        let day = package.updated_at.format("%Y-%m-%d").to_string();
        sitemap_entry(&mut xml, &format!("{base}/crates/{id}"), Some(&day));
    }
    xml.push_str("</urlset>\n");
    Ok((
        [
            (header::CONTENT_TYPE, "application/xml; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=120"),
        ],
        xml,
    )
        .into_response())
}

/// `GET /llms.txt` provides source links and API navigation in plain text.
pub async fn llms(
    State(state): State<AppState>,
    Language(lang): Language,
) -> Response {
    let base = &state.config.base_url;
    let code = lang.code();
    let (overview, documentation, specification, library, registry, data, search, feed, sitemap) =
        match lang {
            | Lang::En => {
                (
                    "A dimension-checked scientific language. Use the source links below for documentation and the registry API for versioned package data.",
                    "Documentation",
                    "Specification",
                    "Standard library",
                    "Package registry",
                    "Registry data",
                    "Package search",
                    "Release feed",
                    "Sitemap",
                )
            },
            | Lang::Zh => {
                (
                    "带量纲检查的科学计算语言。通过以下链接阅读文档源码，或通过注册中心 API 查询带版本的包数据。",
                    "文档",
                    "语言规范",
                    "标准库",
                    "包注册中心",
                    "注册中心数据",
                    "包搜索",
                    "发布订阅",
                    "站点地图",
                )
            },
        };
    let mut body = format!(
        "# Aevia\n\n> {overview}\n\n## {documentation}\n\n- [CLI]({base}/cli?lang={code})\n- [{specification}]({base}/spec?lang={code})\n- [{library}]({base}/stdlib?lang={code})\n- [{registry}]({base}/crates)\n"
    );
    let spec = state.spec.read().await.clone();
    for chapter in &spec.book().chapters {
        if spec.markdown(&chapter.slug, lang).is_some() {
            let _ = writeln!(
                body,
                "- [{specification} {}]({base}/spec/{}.md?lang={code})",
                chapter.slug, chapter.slug
            );
        }
    }
    drop(spec);
    let stdlib = state.stdlib_docs.read().await.clone();
    if stdlib.catalog().is_some() {
        let _ = writeln!(
            body,
            "- [{library} + RSSN (JSON)]({base}/stdlib/catalog.json)"
        );
    }
    drop(stdlib);
    let manuals = state.manuals.read().await.clone();
    for kind in [ManualKind::Book, ManualKind::Architecture] {
        for chapter in manuals.edition(kind, lang) {
            let _ = writeln!(
                body,
                "- [{} {}]({base}{}/{}.md?lang={code})",
                kind.title(lang),
                chapter.slug,
                kind.route(),
                chapter.slug
            );
        }
    }
    let _ = write!(
        body,
        "\n## {data}\n\n- [{search} (JSON)]({base}/api/v1/search)\n- [{feed} (Atom)]({base}/feed.xml)\n- [{sitemap}]({base}/sitemap.xml)\n"
    );
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CONTENT_LANGUAGE, code),
            (header::CACHE_CONTROL, "private, no-store"),
        ],
        body,
    )
        .into_response()
}

/// `GET /stdlib/catalog.json` serves the generator's validated wire format.
///
/// # Errors
///
/// Returns 404 when no generated catalog is mounted.
pub async fn catalog(State(state): State<AppState>) -> PageResult<Response> {
    let body = state
        .stdlib_docs
        .read()
        .await
        .catalog()
        .ok_or_else(|| PageError::not_found("The generated catalog is unavailable."))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "public, max-age=60"),
        ],
        body,
    )
        .into_response())
}

pub(crate) fn markdown_response(
    body: String,
    lang: Lang,
) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
            (header::CONTENT_LANGUAGE, lang.code()),
            (header::CACHE_CONTROL, "private, no-store"),
            (header::HeaderName::from_static("x-robots-tag"), "noindex"),
        ],
        body,
    )
        .into_response()
}

/// `GET /feed.xml` — an Atom feed of the newest releases.
///
/// # Errors
///
/// `E_DB` when the releases cannot be read.
pub async fn feed(State(state): State<AppState>) -> PageResult<Response> {
    let base = state.config.base_url.trim_end_matches('/').to_string();
    let releases = repo::recent_releases(&state.pool, 50).await?;
    let updated = releases.first().map_or_else(
        || "1970-01-01T00:00:00Z".to_string(),
        |release| release.created_at.to_rfc3339(),
    );
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
        let _ = write!(
            xml,
            "  <entry>\n    <title>{title}</title>\n    <id>{url}</id>\n    \
             <link href=\"{url}\"/>\n    <updated>{when}</updated>\n    \
             <summary>{summary}</summary>\n  </entry>\n",
            title = xml_escape(&format!("{id} {}", release.version)),
            url = xml_escape(&url),
            when = release.created_at.to_rfc3339(),
            summary = xml_escape(release.description.as_deref().unwrap_or(""))
        );
    }
    xml.push_str("</feed>\n");
    Ok((
        [(header::CONTENT_TYPE, "application/atom+xml; charset=utf-8")],
        xml,
    )
        .into_response())
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
    let width = |text: &str| text.chars().count().saturating_mul(7).saturating_add(12);
    let (left, right) = (width(label), width(value));
    let label = xml_escape(label);
    let value = xml_escape(value);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{total}\" height=\"20\" role=\"img\" \
         aria-label=\"{label}: {value}\"><rect width=\"{left}\" height=\"20\" fill=\"#555\"/>\
         <rect x=\"{left}\" width=\"{right}\" height=\"20\" fill=\"{colour}\"/>\
         <g fill=\"#fff\" font-family=\"Verdana,sans-serif\" font-size=\"11\" text-anchor=\"middle\">\
         <text x=\"{lx}\" y=\"14\">{label}</text><text x=\"{rx}\" y=\"14\">{value}</text></g></svg>",
        total = left.saturating_add(right),
        lx = left / 2,
        rx = left.saturating_add(right / 2),
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
