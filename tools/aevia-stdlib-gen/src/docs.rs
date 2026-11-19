//! `##` documentation for the generated standard-library modules.
//!
//! A chapter-8 name is documented from, in order of preference:
//!
//! 1. the rssn operator of the same name in the live Kernel Manifest: its
//!    description (written by the rule set, completed at rssn build time from
//!    the rustdoc of the function behind it), the signatures aevia's codegen
//!    gives it, its dimension rule and its examples;
//! 2. the rustdoc of the one public `aevia-std` function of that name;
//! 3. the signature chapter 8 itself states for the name.
//!
//! The chapter-8 signature is always shown when there is one: it is the
//! contract, the kernel's is what the operator happens to accept.
//!
//! Shared with the standard-library tests by path, like `support.rs`, so the
//! "generated sources are what the generator says" test sees the same text.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

/// Longest description kept per name; the sources are embedded in the binary
/// and parsed on import.
const MAX_DESCRIPTION: usize = 1400;

/// The §8.20 module sources with documentation.
///
/// `crates_dir` is the workspace's `crates` directory, where `aevia-std/src`
/// is read for rustdoc.
///
/// # Errors
/// Returns an error when a standard-library declaration is missing or undocumented.
#[allow(dead_code)]
#[allow(
    clippy::single_call_fn,
    reason = "shared by the generator and standard-library tests"
)]
pub fn module_sources(
    spec: &str,
    crates_dir: &Path,
) -> Result<Vec<(String, String)>, String> {
    let rust = scan_rustdoc(&crates_dir.join("aevia-std/src"));
    let registry = aevia_rssn::registry();
    aevia_stdlib_model::stdlib::try_spec_module_sources_with_docs(
        spec,
        &|module, name, specified| entry(registry, &rust, module, name, specified),
    )
}

/// Chapter-8 entries that share a name with a kernel operator that is a
/// different function: the kernel's `div` is a divergence, the Prelude's is
/// integer division. Checked by reading both descriptions; a name is only here
/// when its meaning differs, so the kernel's text must not describe it.
const NOT_THE_KERNEL_OPERATOR: &[(&str, &str)] = &[
    ("Prelude", "apply"),
    ("Prelude", "div"),
    ("Prelude", "rem"),
    ("Prelude", "rank"),
    ("LinearAlgebra", "at"),
    ("LinearAlgebra/Sparse", "contract"),
    ("Math", "log"),
    ("Measurement", "uncertainty"),
    ("Test", "forall"),
];

#[allow(
    clippy::single_call_fn,
    clippy::too_many_lines,
    reason = "keeps documentation precedence and its canonical output order together"
)]
fn entry(
    registry: &aevia_rssn::registry::Registry,
    rust: &BTreeMap<String, Vec<String>>,
    module: &str,
    name: &str,
    specified: &[String],
) -> Option<String> {
    let mut summary = String::new();
    let mut body = String::new();
    let mut footer = String::new();
    let mut kernel = String::new();
    let mut dims = String::new();
    let mut examples = String::new();
    let op = registry
        .op(name)
        .filter(|op| !op.doc.trim().is_empty())
        .filter(|_| !NOT_THE_KERNEL_OPERATOR.contains(&(module, name)));
    if let Some(op) = op {
        (summary, body) = split_summary(&op.doc, name);
        let mut seen = Vec::new();
        for signature in op.signatures.iter().take(4) {
            let text = signature.render();
            if !seen.contains(&text) {
                seen.push(text);
            }
        }
        if !seen.is_empty() {
            let _ = write!(kernel, "**Kernel signature** ({}):", op.source.as_str());
            for text in &seen {
                let _ = write!(kernel, "\n- `{name} : {text}`");
            }
            if op.signatures.len() > 4 {
                let _ = write!(
                    kernel,
                    "\n- ... and {} more overloads",
                    op.signatures.len().saturating_sub(4)
                );
            }
        }
        if let Some(rule) = &op.dim_rule {
            dims = format!("**Dimensions:** {}", dim_rule(rule));
        }
        for example in op.examples.iter().take(3) {
            let _ = write!(examples, "\n- `{}` => `{}`", example.input, example.output);
        }
        if examples.is_empty() {
            // Recorded calls: the output is the *sort* of the answer, not a value.
            for (input, output) in op.witnesses.iter().take(3) {
                let _ = write!(examples, "\n- `{input}` returns a `{output}`");
            }
            if !examples.is_empty() {
                examples = format!(
                    "{examples}\n  (recorded kernel calls; the result is shown as its sort)"
                );
            }
        }
        let how = if format!("{:?}", op.doc_source) == "RustName" {
            "by name"
        } else {
            "by its declared path"
        };
        let origin = match &op.doc_path {
            | Some(path) => {
                format!(
                    "rssn operator `{}.{name}`; rustdoc of `{path}` ({how})",
                    op.rule_set
                )
            },
            | None => format!("rssn operator `{}.{name}`", op.rule_set),
        };
        footer = format!("*Source: {origin}.*");
    } else if let Some([text]) = rust.get(name).map(Vec::as_slice) {
        (summary, body) = split_summary(text, name);
        footer = String::from("*Source: Rust documentation of the `aevia-std` function.*");
    }
    // The guide is written for the Aevia caller, so it replaces whatever
    // prose the kernel manifest or the Rust sources have for the same name.
    // The kernel signatures and the dimension rule stay: they are facts about
    // the operator, not prose.
    if let Some(guide) = aevia_stdlib_model::guides::guide(name) {
        (summary, body) = split_summary(guide.description, name);
        examples = String::new();
        footer = String::from("*Source: Aevia standard library guide.*");
        if !guide.example.trim().is_empty() {
            let _ = write!(examples, "\n```text\n{}\n```", guide.example.trim_end());
        }
    }
    let mut out = String::new();
    if !summary.is_empty() {
        out.push_str(&summary);
    }
    for text in specified {
        if !out.is_empty() {
            out.push('\n');
        }
        let _ = write!(out, "\n**Signature:** `{name} : {text}`");
    }
    for part in [kernel, dims] {
        if !part.is_empty() {
            let _ = write!(out, "\n\n{part}");
        }
    }
    if !body.is_empty() {
        let _ = write!(out, "\n\n{}", cap(&body));
    }
    if !examples.is_empty() {
        let _ = write!(out, "\n\n**Examples:**{examples}");
    }
    if !footer.is_empty() {
        let _ = write!(out, "\n\n{footer}");
    }
    let out = out.trim().to_owned();
    (!out.is_empty()).then_some(out)
}

/// Cuts at a paragraph boundary near [`MAX_DESCRIPTION`].
#[allow(
    clippy::single_call_fn,
    reason = "isolates the prose length and UTF-8 policy from rendering"
)]
fn cap(text: &str) -> String {
    if text.len() <= MAX_DESCRIPTION {
        return text.to_owned();
    }
    let head = text
        .get(..text.floor_char_boundary(MAX_DESCRIPTION))
        .unwrap_or(text);
    let head = head.rsplit_once("\n\n").map_or(head, |(before, _)| before);
    format!("{}\n\n(truncated)", head.trim_end())
}

/// The first sentence and the rest, with the markdown made safe for a `##`
/// block: headings become bold labels and every fence is plain text, so
/// `aevia test --doc` does not mistake Rust snippets for Aevia programs.
fn split_summary(
    doc: &str,
    name: &str,
) -> (String, String) {
    let mut lines: Vec<String> = Vec::new();
    let mut fenced = false;
    for line in doc.trim().lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            // An untagged or `rust` fence is Rust, never an Aevia program.
            lines.push(if fenced { "```" } else { "```text" }.to_owned());
            fenced = !fenced;
        } else if !fenced && trimmed.starts_with('#') {
            lines.push(format!("**{}**", trimmed.trim_start_matches('#').trim()));
        } else {
            lines.push(line.trim_end().to_owned());
        }
    }
    let text = lines.join("\n");
    let paragraph = text
        .split_once("\n\n")
        .map_or(text.as_str(), |(first, _)| first);
    let sentence_end = paragraph
        .match_indices(". ")
        .map(|(at, _)| at.saturating_add(1))
        .next()
        .unwrap_or(paragraph.len());
    let mut summary = paragraph
        .get(..sentence_end)
        .unwrap_or(paragraph)
        .replace('\n', " ");
    // `name(a, b)`: ... -> keep the call form out of the summary line.
    let call = format!("`{name}(");
    if summary.starts_with(&call)
        && let Some((_, description)) = summary.split_once("`: ")
    {
        summary = description.to_owned();
    }
    let mut summary = summary.trim().to_owned();
    // A sentence that opens with a call, `defint(f, a, b) is ...`, keeps its
    // spelling: upper-casing would misname the function.
    let word_end = summary
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(summary.len());
    let opens_with_call = summary
        .get(word_end..)
        .is_some_and(|tail| tail.starts_with('('));
    if !opens_with_call && let Some((first, suffix)) = summary.split_at_checked(1) {
        summary = format!("{}{suffix}", first.to_uppercase());
    }
    let rest = text
        .get(sentence_end..)
        .unwrap_or_default()
        .trim()
        .to_owned();
    (summary, rest)
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps the notation for dimension rules separate from markup"
)]
fn dim_rule(rule: &impl std::fmt::Debug) -> String {
    let text = format!("{rule:?}");
    match text.as_str() {
        | "Dimensionless" => "the result is dimensionless".to_owned(),
        | "Sum" => "arguments must agree; the result has their dimension".to_owned(),
        | "Product" => "the result is the product of the argument dimensions".to_owned(),
        | "Quotient" => "the result is argument 1 divided by argument 2".to_owned(),
        | "Power" => "`a^b` with `b` dimensionless".to_owned(),
        | "Inverse" => "the inverse of the argument's dimension".to_owned(),
        | "Square" => "the square of the argument's dimension".to_owned(),
        | "Any" => "dimensionally polymorphic".to_owned(),
        | "Compare" => "arguments must agree; the result is a truth value".to_owned(),
        | "Angle" => "the result is an angle (dimensionless)".to_owned(),
        | other if other.starts_with("Diff {") || other.starts_with("Integral {") => {
            let digits: Vec<usize> = other
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|part| part.parse().ok())
                .collect();
            let (f, x) = (
                digits.first().copied().unwrap_or(0).saturating_add(1),
                digits.get(1).copied().unwrap_or(1).saturating_add(1),
            );
            if other.starts_with("Diff") {
                format!("argument {f} divided by argument {x}")
            } else {
                format!("argument {f} multiplied by argument {x}")
            }
        },
        | other => {
            other
                .strip_prefix("Same(")
                .and_then(|rest| rest.strip_suffix(')'))
                .and_then(|digits| digits.parse::<usize>().ok())
                .map_or_else(
                    || format!("kernel rule `{other}`"),
                    |index| {
                        format!(
                            "the result has the dimension of argument {}",
                            index.saturating_add(1)
                        )
                    },
                )
        },
    }
}

/// Every documented public free function of the crate sources under `src`,
/// `name -> [doc]`. Only `pub fn` items with a `///` block count; a name
/// documented twice is ambiguous and the caller will skip it.
#[allow(
    clippy::single_call_fn,
    reason = "shared source scanner is directly exercised by tests"
)]
pub fn scan_rustdoc(src: &Path) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut files = Vec::new();
    collect(src, &mut files);
    files.sort();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let mut pending: Vec<String> = Vec::new();
        for raw in text.lines() {
            let line = raw.trim_start();
            if let Some(rest) = line.strip_prefix("///") {
                if !rest.starts_with('/') {
                    pending.push(rest.strip_prefix(' ').unwrap_or(rest).to_owned());
                }
                continue;
            }
            if line.starts_with("#[") || line.starts_with("//") {
                continue;
            }
            // Free functions only: no indentation, so no methods.
            if raw.len() == line.len()
                && let Some(rest) = line.strip_prefix("pub fn ")
                && !pending.is_empty()
            {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let mut kept: Vec<&str> = Vec::new();
                let mut fenced = false;
                for doc_line in &pending {
                    if doc_line.trim_start().starts_with("```") {
                        fenced = !fenced;
                    } else if fenced && (doc_line.trim() == "#" || doc_line.starts_with("# ")) {
                        continue;
                    }
                    kept.push(doc_line);
                }
                let doc = kept.join("\n").trim().to_owned();
                if !name.is_empty() && !doc.is_empty() {
                    out.entry(name).or_default().push(doc);
                }
            }
            pending.clear();
        }
    }
    out
}

fn collect(
    dir: &Path,
    files: &mut Vec<std::path::PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}
