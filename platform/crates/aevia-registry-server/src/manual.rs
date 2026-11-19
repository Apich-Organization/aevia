//! Repository-owned bilingual manuals and their HTML editions.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use axum::extract::Path as RoutePath;
use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Response;

use aevia_registry_core::Error;
use aevia_registry_core::Lang;
use aevia_registry_core::vm::ManualChapter;
use aevia_registry_core::vm::ManualKind;

use crate::error::PageError;
use crate::error::PageResult;
use crate::http::Language;
use crate::state::AppState;

const E_MANUAL: &str = "E_MANUAL";

/// Both editions of a manual, with validated matching sequences.
#[derive(Debug, Default)]
struct Manual {
    en: Vec<ManualChapter>,
    zh: Vec<ManualChapter>,
}

impl Manual {
    fn load(
        root: &Path,
        kind: ManualKind,
        base_url: &str,
    ) -> Result<Self, Error> {
        let dir = root.join(&kind.route()[1..]);
        if !dir.try_exists().map_err(|error| file_error(&dir, &error))? {
            return Ok(Self::default());
        }
        let en = read_edition(&dir.join("en"))?;
        let zh = read_edition(&dir.join("zh"))?;
        if en.is_empty() || en.keys().ne(zh.keys()) {
            return Err(Error::new(
                E_MANUAL,
                format!(
                    "the English and Chinese editions in `{}` must have the same nonempty chapter list",
                    dir.display()
                ),
            ));
        }
        Ok(Self {
            en: render_edition(&en, kind, Lang::En, base_url),
            zh: render_edition(&zh, kind, Lang::Zh, base_url),
        })
    }

    fn edition(
        &self,
        language: Lang,
    ) -> &[ManualChapter] {
        match language {
            | Lang::En => &self.en,
            | Lang::Zh => &self.zh,
        }
    }
}

/// An immutable snapshot of the manuals used by requests.
#[derive(Debug, Default)]
pub struct ManualStore {
    book: Manual,
    architecture: Manual,
}

impl ManualStore {
    /// Loads and renders both manuals.
    ///
    /// # Errors
    ///
    /// Returns an error on unreadable chapters or incomplete bilingual editions.
    pub fn load(
        root: &Path,
        base_url: &str,
    ) -> Result<Self, Error> {
        Ok(Self {
            book: Manual::load(root, ManualKind::Book, base_url)?,
            architecture: Manual::load(root, ManualKind::Architecture, base_url)?,
        })
    }

    /// Whether a reload would remove an entire previously available manual.
    pub(crate) const fn loses_manual(
        &self,
        previous: &Self,
    ) -> bool {
        (self.book.en.is_empty() && !previous.book.en.is_empty())
            || (self.architecture.en.is_empty() && !previous.architecture.en.is_empty())
    }

    const fn manual(
        &self,
        kind: ManualKind,
    ) -> &Manual {
        match kind {
            | ManualKind::Book => &self.book,
            | ManualKind::Architecture => &self.architecture,
        }
    }
}

fn file_error(
    path: &Path,
    error: &std::io::Error,
) -> Error {
    Error::new(
        E_MANUAL,
        format!("cannot read `{}`: {error}", path.display()),
    )
}

struct ChapterSource {
    title: String,
    body: String,
}

fn read_edition(directory: &Path) -> Result<BTreeMap<String, ChapterSource>, Error> {
    let files = fs::read_dir(directory).map_err(|error| file_error(directory, &error))?;
    let mut chapters = BTreeMap::new();
    for file in files {
        let file = file.map_err(|error| file_error(directory, &error))?;
        let path = file.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let invalid = |reason: &str| Error::new(E_MANUAL, format!("`{}` {reason}", path.display()));
        let slug = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|slug| {
                !slug.is_empty()
                    && !matches!(*slug, "all" | "download")
                    && slug.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
            })
            .ok_or_else(|| invalid("must have a lowercase URL slug as the filename"))?;
        let raw = fs::read_to_string(&path).map_err(|error| file_error(&path, &error))?;
        let (heading, body) = raw.trim_start().split_once('\n').unwrap_or((&raw, ""));
        let title = heading
            .strip_prefix("# ")
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .ok_or_else(|| invalid("must start with a level-one heading"))?;
        if body.trim().is_empty() {
            return Err(invalid("has no chapter body"));
        }
        chapters.insert(
            slug.to_string(),
            ChapterSource {
                title: title.to_string(),
                body: body.to_string(),
            },
        );
    }
    Ok(chapters)
}

fn render_edition(
    sources: &BTreeMap<String, ChapterSource>,
    kind: ManualKind,
    language: Lang,
    base_url: &str,
) -> Vec<ManualChapter> {
    sources
        .iter()
        .map(|(slug, source)| {
            let route = kind.route();
            let lang = language.code();
            let html = aevia_registry_docs::render_manual_markdown(&source.body, &|href| {
                match local_chapter(sources, href, lang) {
                    | Some((target, fragment)) => format!("{route}/{target}?lang={lang}{fragment}"),
                    | None => href.to_string(),
                }
            });
            let (html, toc) = crate::spec::heading_ids(&html, "");
            let combined = aevia_registry_docs::render_manual_markdown(&source.body, &|href| {
                if let Some(fragment) = href.strip_prefix('#') {
                    return format!("#section-{slug}-{fragment}");
                }
                if let Some((target, fragment)) = local_chapter(sources, href, lang) {
                    match fragment.strip_prefix('#') {
                        | Some(fragment) => format!("#section-{target}-{fragment}"),
                        | None => format!("#section-{target}"),
                    }
                } else if href.starts_with('/') && !href.starts_with("//") {
                    format!("{base_url}{href}")
                } else {
                    href.to_string()
                }
            });
            let (combined_html, _) =
                crate::spec::heading_ids(&combined, &format!("section-{slug}-"));
            ManualChapter {
                slug: slug.clone(),
                title: source.title.clone(),
                html,
                toc,
                combined_html,
            }
        })
        .collect()
}

fn local_chapter<'a>(
    sources: &BTreeMap<String, ChapterSource>,
    href: &'a str,
    lang: &str,
) -> Option<(&'a str, &'a str)> {
    let split = href.find('#').unwrap_or(href.len());
    let (path, fragment) = href.split_at(split);
    let path = match path.split_once('?') {
        | Some((path, query)) if query.strip_prefix("lang=") == Some(lang) => path,
        | Some(_) => return None,
        | None => path,
    };
    let path = path.strip_prefix("./").unwrap_or(path);
    let slug = path.strip_suffix(".md").unwrap_or(path);
    sources.contains_key(slug).then_some((slug, fragment))
}

/// Routes for a manual, nested under its public URL prefix.
pub fn router(kind: ManualKind) -> axum::Router<AppState> {
    use axum::routing::get;
    axum::Router::new()
        .route(
            "/",
            get(
                move |State(state): State<AppState>, Language(lang), headers| {
                    async move {
                        let snapshot = snapshot(&state).await;
                        let edition = snapshot.manual(kind).edition(lang);
                        let viewer = state.viewer(&headers).await;
                        Html(aevia_registry_web::manual_index(
                            kind,
                            edition,
                            lang,
                            viewer.as_ref(),
                        ))
                    }
                },
            ),
        )
        .route(
            "/all",
            get(
                move |State(state): State<AppState>, Language(lang), headers| {
                    async move {
                        let snapshot = snapshot(&state).await;
                        let edition = snapshot.manual(kind).edition(lang);
                        let viewer = state.viewer(&headers).await;
                        Html(aevia_registry_web::manual_all(
                            kind,
                            edition,
                            lang,
                            viewer.as_ref(),
                        ))
                    }
                },
            ),
        )
        .route(
            "/download",
            get(move |State(state): State<AppState>, Language(lang)| {
                async move {
                    let snapshot = snapshot(&state).await;
                    let edition = snapshot.manual(kind).edition(lang);
                    let filename = format!(
                        "aevia{}-{}.html",
                        kind.route().replace('/', "-"),
                        lang.code()
                    );
                    let disposition = format!("attachment; filename=\"{filename}\"");
                    (
                        [(axum::http::header::CONTENT_DISPOSITION, disposition)],
                        Html(aevia_registry_web::manual_download(kind, edition, lang)),
                    )
                }
            }),
        )
        .route(
            "/{slug}",
            get(
                move |State(state): State<AppState>, Language(lang), RoutePath(slug), headers| {
                    manual_chapter(state, kind, lang, slug, headers)
                },
            ),
        )
}

async fn snapshot(state: &AppState) -> Arc<ManualStore> {
    state.manuals.read().await.clone()
}

#[allow(
    clippy::single_call_fn,
    reason = "the chapter response resolves translation and canonical navigation together"
)]
async fn manual_chapter(
    state: AppState,
    kind: ManualKind,
    lang: Lang,
    slug: String,
    headers: axum::http::HeaderMap,
) -> PageResult<Response> {
    let snapshot = snapshot(&state).await;
    let edition = snapshot.manual(kind).edition(lang);
    let index = edition
        .iter()
        .position(|chapter| chapter.slug == slug)
        .ok_or_else(|| {
            PageError::not_found(match lang {
                | Lang::En => format!("This manual has no chapter `{slug}`."),
                | Lang::Zh => format!("本手册中没有 `{slug}` 一章。"),
            })
            .in_lang(lang)
        })?;
    let viewer = state.viewer(&headers).await;
    Ok(Html(aevia_registry_web::manual_chapter(
        kind,
        edition,
        index,
        lang,
        viewer.as_ref(),
        state.config.playground.is_some(),
    ))
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_edition_has_resolving_links_and_scoped_combined_headings() {
        let root = tempfile::tempdir().unwrap();
        for (lang, title) in [("en", "First"), ("zh", "第一章")] {
            let dir = root.path().join("book").join(lang);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("01-first.md"), format!(
                "# {title}\n\n## Help\n\n[here](#help), [next](02-next.md#help), [the next chapter](02-next?lang={lang}) and [next heading](./02-next.md?lang={lang}#help)\n\n```aevia,run\nlet x = 1\n```\n"
            )).unwrap();
            fs::write(dir.join("02-next.md"), "# Next\n\n## Help\n\nNext.").unwrap();
        }
        let store = ManualStore::load(root.path(), "https://example.com").unwrap();
        let chapter = &store.book.zh[0];
        assert_eq!(chapter.title, "第一章");
        assert_eq!(
            chapter.html.matches("/book/02-next?lang=zh#help").count(),
            2
        );
        assert!(chapter.html.contains("href=\"/book/02-next?lang=zh\""));
        assert!(chapter.html.contains("id=\"help\""));
        assert!(
            chapter
                .html
                .contains("class=\"language-aevia interactive-example\"")
        );
        assert!(
            chapter
                .combined_html
                .contains("id=\"section-01-first-help\"")
        );
        assert_eq!(
            chapter
                .combined_html
                .matches("href=\"#section-02-next-help\"")
                .count(),
            2
        );
        assert!(chapter.combined_html.contains("href=\"#section-02-next\""));
        fs::remove_file(root.path().join("book/zh/02-next.md")).unwrap();
        assert!(ManualStore::load(root.path(), "https://example.com").is_err());
    }
}
