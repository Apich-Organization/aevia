//! The language specification, read from the repository that owns it.
//!
//! # Why it is loaded at start-up rather than baked in
//!
//! `include_str!` would freeze the specification into the binary, and the
//! binary is built in a container whose build context does not contain `spec/`.
//! Reading it at start-up from `AEVIA_SPEC_DIR` keeps one source of truth: the
//! operator mounts the language repository's `spec/` directory read-only, and
//! the site shows exactly what is in it. A missing directory is not a crash —
//! `/spec` then says so in both languages, which is a deployment problem the
//! reader can report.
//!
//! # Why the HTML is built once
//!
//! The chapters total about half a megabyte of Markdown. Rendering it per
//! request would spend a CPU-second of parsing to produce bytes that never
//! change between deploys, so the whole book is rendered once and held in
//! memory. A reload means a restart, which is the right granularity for a
//! document that ships with a release.

use std::path::Path;
use std::path::PathBuf;

use aevia_registry_core::Error;
use aevia_registry_core::vm::SpecBook;
use aevia_registry_core::vm::SpecChapter;
use aevia_registry_core::vm::SpecHeading;

/// `E_SPEC`: the specification directory exists but cannot be read.
pub const E_SPEC: &str = "E_SPEC";

/// The chapters, in the order `tools/bundle_spec.py` stitches them.
///
/// Kept identical to the bundler's list on purpose: the site and the
/// single-file edition must present the same document, and the order is the
/// document's argument.
const ORDER: &[&str] = &[
    "README.md",
    "00-overview.md",
    "01-lexical.md",
    "02-syntax.md",
    "03-types.md",
    "04-dimensions.md",
    "05-units.md",
    "06-semantics.md",
    "07-modules.md",
    "08-stdlib.md",
    "09-runtime.md",
    "10-backend.md",
    "11-examples.md",
    "12-rationale.md",
    "13-extensibility.md",
    "appendix-a-grammar.md",
    "appendix-b-diagnostics.md",
    "appendix-c-changelog.md",
];

/// The English title of each chapter, for the bilingual navigation.
///
/// A Chinese heading is unreadable to an English reader who is looking for the
/// chapter on, say, row polymorphism. These titles are the ones the language
/// repository's own English index uses, so the site and the repository agree.
fn english_title(slug: &str) -> &'static str {
    match slug {
        | "index" => "Aevia Language Specification",
        | "00-overview" => "Overview",
        | "01-lexical" => "Lexical structure",
        | "02-syntax" => "Syntax",
        | "03-types" => "Type system",
        | "04-dimensions" => "Dimensions",
        | "05-units" => "Units",
        | "06-semantics" => "Semantics",
        | "07-modules" => "Modules and packages",
        | "08-stdlib" => "Standard library",
        | "09-runtime" => "Runtime",
        | "10-backend" => "The rssn backend",
        | "11-examples" => "Worked examples",
        | "12-rationale" => "Rationale and limits",
        | "13-extensibility" => "Extensibility and RSSN integration",
        | "appendix-a-grammar" => "Appendix A — Grammar",
        | "appendix-b-diagnostics" => "Appendix B — Diagnostics",
        | "appendix-c-changelog" => "Appendix C — Changelog",
        | _ => "",
    }
}

/// A one-paragraph English abstract of each chapter.
///
/// These are reading aids, not translations: each states what the chapter
/// decides, so a reader can tell whether it is the chapter they need. The
/// normative text stays the author's.
fn english_summary(slug: &str) -> Option<&'static str> {
    let summary = match slug {
        | "index" => {
            "The index of the design standard: what Aevia is, the reading order, \
             and the conformance language (RFC 2119 keywords) the rest of the \
             document is written in."
        },
        | "00-overview" => {
            "The one-sentence definition, the seven goals in priority order, the \
             non-goals, the design principles they follow from, a ten-minute tour, \
             and how the language layers over the rssn backend."
        },
        | "01-lexical" => {
            "Source encoding, identifiers (including Unicode), numeric and unit \
             literals, comments and doc comments, and the layout rule that makes \
             indentation significant without making whitespace ambiguous."
        },
        | "02-syntax" => {
            "Every declaration and expression form: modules, functions, `let`, \
             types, classes and instances, patterns, `do` blocks, pipes and \
             operators, with the precedence table and the full grammar's shape."
        },
        | "03-types" => {
            "The type language: kinds, HM inference with dimension unification, \
             row polymorphism for records, type classes, effects, defaulting, and \
             exhaustiveness checking. There are no subtypes and no dependent types."
        },
        | "04-dimensions" => {
            "Dimension algebra as exact rational exponents, the typing rules for \
             arithmetic and transcendental functions, dimension polymorphism, and \
             the explicit `Dyn` escape hatch for values whose dimension is only \
             known at run time."
        },
        | "05-units" => {
            "Unit declarations, SI prefixes, affine scales for temperatures, unit \
             systems (natural, atomic, geometrized) that reduce the dimensions \
             written in a module, and the exact conversion rules between them."
        },
        | "06-semantics" => {
            "Evaluation: values, the symbolic and numeric phases of one expression, \
             tensors and their shapes, pattern matching, and the rules that decide \
             when a symbolic result stays exact and when `approx` is required."
        },
        | "07-modules" => {
            "Namespaces, visibility, the package format (`aevia.toml`, `src/`, \
             lock files), import roots, tests, and how a package's modules are \
             resolved and cached."
        },
        | "08-stdlib" => {
            "The standard library's signatures: prelude, linear algebra, \
             statistics, special functions, distributions, FFT, interpolation, \
             ODE/SDE/PDE, tables and simulation."
        },
        | "09-runtime" => {
            "The pipeline from source to value, the interpreter's architecture, \
             the REPL, the command-line interface, the language server, parallel \
             evaluation, and the performance boundaries of each."
        },
        | "10-backend" => {
            "How Aevia maps onto rssn: the Kernel Manifest, the value-list ABI, \
             dimension handling at the boundary, callback kernels, and the gaps \
             that remain between the language and the engine."
        },
        | "11-examples" => {
            "Nine complete programs, from kinematics and symbolic calculus to \
             sparse linear algebra and stochastic simulation, each type-checked \
             against the rules in the preceding chapters."
        },
        | "12-rationale" => {
            "The alternatives that were considered and rejected, the trade-offs \
             that were accepted, the limits that are known, and the roadmap — the \
             non-normative chapter that explains why the normative ones say what \
             they say."
        },
        | "13-extensibility" => {
            "The extension contract: RSSN reflection, the generated `Rssn` module, \
             source plugins, the package registry's role, and the shape, device \
             and sparse-matrix decisions. This chapter takes precedence over any \
             earlier \"not yet supported\" remark."
        },
        | "appendix-a-grammar" => {
            "The complete EBNF grammar: lexical productions, declarations, \
             expressions, patterns and types, in one place."
        },
        | "appendix-b-diagnostics" => {
            "The diagnostic catalogue: error codes, severities, the message format, \
             and the requirement that every error carry at least one actionable fix."
        },
        | "appendix-c-changelog" => {
            "The audit history: what each revision changed, which review round \
             produced it, and the implementation boundaries that are still open."
        },
        | _ => return None,
    };
    Some(summary)
}

/// Whether a chapter is normative.
fn normative(slug: &str) -> bool {
    // The index states the rules; the examples, the rationale and the changelog
    // are explanation. Everything else is the standard.
    !matches!(
        slug,
        "index" | "11-examples" | "12-rationale" | "appendix-c-changelog"
    )
}

/// The specification, rendered once.
#[derive(Clone, Debug)]
pub struct SpecStore {
    book: SpecBook,
    directory: PathBuf,
}

impl SpecStore {
    /// Loads and renders every chapter in `directory`.
    ///
    /// # Errors
    ///
    /// `E_SPEC` when the directory exists but a chapter cannot be read.
    pub fn load(directory: &Path) -> Result<Self, Error> {
        if !directory.is_dir() {
            tracing::warn!(
                path = %directory.display(),
                "no specification directory; /spec will explain how to mount one"
            );
            return Ok(Self {
                book: SpecBook::default(),
                directory: directory.to_path_buf(),
            });
        }
        let mut chapters = Vec::new();
        let mut revision = String::new();
        for name in ORDER {
            let path = directory.join(name);
            if !path.is_file() {
                continue;
            }
            let text = std::fs::read_to_string(&path).map_err(|error| {
                Error::new(E_SPEC, format!("cannot read `{}`: {error}", path.display()))
            })?;
            let text = text.replace("\r\n", "\n");
            if let Some(found) = highest_revision(&text)
                && found > revision
            {
                revision = found;
            }
            let slug = slug_of(name);
            chapters.push(render_chapter(&slug, &text));
        }
        tracing::info!(
            chapters = chapters.len(),
            words = chapters.iter().map(|chapter| chapter.words).sum::<usize>(),
            revision = %revision,
            "specification loaded"
        );
        Ok(Self {
            book: SpecBook { revision, chapters },
            directory: directory.to_path_buf(),
        })
    }

    /// The rendered book.
    #[must_use]
    pub fn book(&self) -> &SpecBook {
        &self.book
    }

    /// Whether anything was loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.book.chapters.is_empty()
    }

    /// Where it was read from, for the log line and the "not mounted" notice.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

/// `04-dimensions.md` → `04-dimensions`; `README.md` → `index`.
fn slug_of(name: &str) -> String {
    let stem = name.strip_suffix(".md").unwrap_or(name);
    if stem.eq_ignore_ascii_case("readme") {
        "index".to_string()
    } else {
        stem.to_string()
    }
}

/// `04-dimensions` → `04`; `appendix-a-grammar` → `A`.
fn number_of(slug: &str) -> String {
    if let Some((head, _)) = slug.split_once('-')
        && head.chars().all(|character| character.is_ascii_digit())
    {
        return head.to_string();
    }
    if let Some(rest) = slug.strip_prefix("appendix-")
        && let Some(letter) = rest.chars().next()
    {
        return letter.to_ascii_uppercase().to_string();
    }
    "—".to_string()
}

/// Renders one chapter: title, body, table of contents.
fn render_chapter(
    slug: &str,
    text: &str,
) -> SpecChapter {
    let title = first_heading(text).unwrap_or_else(|| slug.to_string());
    let body = strip_first_heading(text);
    let rewritten = rewrite_links(&body);
    let html = aevia_registry_docs::render_markdown(&rewritten);
    let (html, toc) = inject_heading_ids(&html);
    SpecChapter {
        slug: slug.to_string(),
        number: number_of(slug),
        short_title: strip_number(&title),
        title: title.clone(),
        title_en: {
            let english = english_title(slug);
            if english.is_empty() {
                title
            } else {
                english.to_string()
            }
        },
        summary_en: english_summary(slug).map(str::to_string),
        words: word_count(text),
        html,
        toc,
        normative: normative(slug),
    }
}

/// Drops a leading `00 — ` from a chapter heading.
///
/// The headings carry their own number, and every list on the site also has a
/// number column; without this the sidebar reads `04  04 — 量纲`.
fn strip_number(title: &str) -> String {
    let trimmed = title.trim_start();
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return trimmed.to_string();
    }
    let rest = trimmed[digits.len()..].trim_start();
    let rest = rest
        .strip_prefix('—')
        .or_else(|| rest.strip_prefix('-'))
        .or_else(|| rest.strip_prefix('–'))
        .map(str::trim_start)
        .unwrap_or(rest);
    if rest.is_empty() {
        trimmed.to_string()
    } else {
        rest.to_string()
    }
}

/// The first `# ` heading.
fn first_heading(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        line.strip_prefix("# ")
            .map(|title| title.trim().to_string())
    })
}

/// The body without its `# ` heading, which the page renders itself.
fn strip_first_heading(text: &str) -> String {
    let mut lines = text.lines();
    for line in lines.by_ref() {
        if line.starts_with("# ") {
            break;
        }
    }
    lines.collect::<Vec<_>>().join("\n")
}

/// Points the specification's relative links at the right place.
///
/// Inside `spec/`, a chapter links to its siblings as `01-lexical.md` and to
/// files outside it as `../ARCHITECTURE.md`. On the site the
/// first must become `/spec/01-lexical` and the second has no page at all, so it
/// is sent to the repository where the file does exist.
fn rewrite_links(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(index) = rest.find("](") {
        out.push_str(&rest[..index + 2]);
        rest = &rest[index + 2..];
        let Some(close) = rest.find(')') else {
            break;
        };
        let target = &rest[..close];
        let rewritten = rewrite_target(target);
        out.push_str(&rewritten);
        rest = &rest[close..];
    }
    out.push_str(rest);
    out
}

fn rewrite_target(target: &str) -> String {
    const REPO: &str = "https://github.com/Apich-Organization/aevia/blob/main";
    if let Some(rest) = target.strip_prefix("../") {
        return format!("{REPO}/{rest}");
    }
    if target.starts_with("http://") || target.starts_with("https://") || target.starts_with('#') {
        return target.to_string();
    }
    // `04-dimensions.md#4.2` → `/spec/04-dimensions#4-2`
    if let Some((path, anchor)) = target.split_once('#')
        && let Some(stem) = path.strip_suffix(".md")
    {
        let slug = slug_of(stem);
        return format!("/spec/{slug}#{anchor}");
    }
    if let Some(stem) = target.strip_suffix(".md") {
        return format!("/spec/{}", slug_of(stem));
    }
    target.to_string()
}

/// Adds stable ids to `h2`–`h4` and collects the table of contents.
///
/// The ids have to be added here rather than by the Markdown renderer because
/// the renderer is shared with package READMEs, where an id on every heading
/// would be noise. The slug keeps CJK characters, so a Chinese heading gets a
/// readable, stable anchor instead of a numbered one.
fn inject_heading_ids(html: &str) -> (String, Vec<SpecHeading>) {
    let mut out = String::with_capacity(html.len() + 256);
    let mut toc = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut rest = html;
    loop {
        let Some(start) = find_heading(rest) else {
            out.push_str(rest);
            break;
        };
        let (level, tag_len) = start;
        let open_end = match rest[start.1..].find('>') {
            | Some(offset) => start.1 + offset,
            | None => {
                out.push_str(rest);
                break;
            },
        };
        let close_tag = format!("</h{level}>");
        let Some(close) = rest[open_end..].find(&close_tag) else {
            out.push_str(rest);
            break;
        };
        let inner = &rest[open_end + 1..open_end + close];
        let text = strip_tags(inner);
        let base = slugify(&text);
        let count = seen.entry(base.clone()).or_insert(0);
        *count += 1;
        let id = if *count == 1 {
            base
        } else {
            format!("{base}-{}", *count)
        };
        // `close` is the offset of `</hN>` from `open_end`, so the element
        // runs to `open_end + close + close_tag.len()`. Getting this off by one
        // truncates the closing tag and swallows the next paragraph's `<`.
        let end = open_end + close + close_tag.len();
        out.push_str(&rest[..open_end]);
        out.push_str(&format!(" id=\"{id}\""));
        out.push_str(&rest[open_end..end]);
        toc.push(SpecHeading { level, text, id });
        rest = &rest[end..];
        let _ = tag_len;
    }
    (out, toc)
}

/// Finds the earliest `<h2>`–`<h4>` opening tag: `(level, offset of '<')`.
///
/// The earliest, not the outermost: a chapter whose first heading is an `h3`
/// must not have its `h2` further down processed first, or the ids land in the
/// wrong order and the `h3` is skipped.
fn find_heading(text: &str) -> Option<(u8, usize)> {
    let mut best: Option<(u8, usize)> = None;
    for level in [2u8, 3, 4] {
        let needle = format!("<h{level}>");
        if let Some(index) = text.find(&needle)
            && best.is_none_or(|(_, offset)| index < offset)
        {
            best = Some((level, index));
        }
    }
    best
}

/// Removes tags from a fragment, for a heading's text.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut depth = 0usize;
    for character in html.chars() {
        match character {
            | '<' => depth += 1,
            | '>' => depth = depth.saturating_sub(1),
            | other if depth == 0 => out.push(other),
            | _ => {},
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    decode_entities(&collapsed)
}

fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// A URL-safe anchor that keeps letters, digits and CJK characters.
fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut dash = false;
    for character in text.chars() {
        let keep = character.is_ascii_alphanumeric()
            || matches!(character, '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}');
        if keep {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(character.to_ascii_lowercase());
        } else {
            dash = true;
        }
    }
    if out.is_empty() {
        "section".to_string()
    } else {
        out
    }
}

/// Rough word count: CJK characters count one each, runs of Latin one each.
fn word_count(text: &str) -> usize {
    let mut count = 0usize;
    let mut in_word = false;
    for character in text.chars() {
        let cjk = matches!(character, '\u{4e00}'..='\u{9fff}');
        if cjk {
            count += 1;
            in_word = false;
        } else if character.is_alphanumeric() {
            if !in_word {
                count += 1;
                in_word = true;
            }
        } else {
            in_word = false;
        }
    }
    count
}

/// The highest `Revision N` mentioned in a chapter.
fn highest_revision(text: &str) -> Option<String> {
    let mut best: Option<u32> = None;
    let mut rest = text;
    while let Some(index) = rest.find("Revision ") {
        rest = &rest[index + 9..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(value) = digits.parse::<u32>()
            && best.is_none_or(|best| value > best)
        {
            best = Some(value);
        }
    }
    best.map(|value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_and_numbers_come_from_the_file_names() {
        assert_eq!(slug_of("README.md"), "index");
        assert_eq!(slug_of("04-dimensions.md"), "04-dimensions");
        assert_eq!(number_of("04-dimensions"), "04");
        assert_eq!(number_of("appendix-b-diagnostics"), "B");
        assert_eq!(number_of("index"), "—");
    }

    #[test]
    fn sibling_links_become_routes_and_outside_links_become_repository_links() {
        assert_eq!(rewrite_target("04-dimensions.md"), "/spec/04-dimensions");
        assert_eq!(rewrite_target("README.md"), "/spec/index");
        assert_eq!(
            rewrite_target("04-dimensions.md#4.2"),
            "/spec/04-dimensions#4.2"
        );
        assert_eq!(
            rewrite_target("../ARCHITECTURE.md"),
            "https://github.com/Apich-Organization/aevia/blob/main/ARCHITECTURE.md"
        );
        assert_eq!(rewrite_target("https://example.com"), "https://example.com");
    }

    #[test]
    fn headings_get_stable_cjk_aware_ids() {
        let (html, toc) = inject_heading_ids(
            "<h2>4.1 量纲代数</h2><p>x</p><h3>4.1.1 Why</h3><h2>4.1 量纲代数</h2>",
        );
        assert!(html.contains("id=\"4-1-量纲代数\""), "{html}");
        assert!(html.contains("id=\"4-1-1-why\""), "{html}");
        assert!(html.contains("id=\"4-1-量纲代数-2\""), "{html}");
        assert_eq!(toc.len(), 3);
        assert_eq!(toc[0].text, "4.1 量纲代数");
        assert_eq!(toc[1].level, 3);
    }

    #[test]
    fn a_chapter_loses_its_own_heading_and_keeps_the_rest() {
        let chapter = render_chapter("00-overview", "# 00 — 总览\n\n正文。\n");
        assert_eq!(chapter.title, "00 — 总览");
        assert!(!chapter.html.contains("00 — 总览"), "{}", chapter.html);
        assert!(chapter.html.contains("正文"));
        assert_eq!(chapter.title_en, "Overview");
        assert!(chapter.normative);
    }

    #[test]
    fn the_revision_is_the_highest_one_mentioned() {
        assert_eq!(
            highest_revision("Revision 9 … Revision 12").as_deref(),
            Some("12")
        );
        assert_eq!(highest_revision("no mention"), None);
    }

    #[test]
    fn word_counting_handles_both_scripts() {
        assert_eq!(word_count("量纲是类型"), 5);
        assert_eq!(word_count("two words"), 2);
        assert_eq!(word_count("量纲 is a type"), 5);
    }
}
