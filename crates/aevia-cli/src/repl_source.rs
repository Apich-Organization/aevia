//! Presentation-only projections from the persistent REPL buffer to entered code.
//! Checker/runtime spans and module identities are never rewritten.
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;

use aevia_syntax::Diagnostic;
use aevia_syntax::Diagnostics;
use aevia_syntax::Severity;
use aevia_syntax::SourceMap;
use aevia_syntax::Span;
use aevia_syntax::diagnostic::Style;

#[derive(Clone)]
struct Document {
    id: u64,
    name: Option<String>,
    text: String,
}

#[derive(Clone)]
pub struct Input {
    document: Rc<Document>,
    selection: Range<usize>,
}
impl Input {
    pub(crate) fn text(&self) -> &str {
        &self.document.text[self.selection.clone()]
    }

    pub(crate) fn select(
        &self,
        range: Range<usize>,
    ) -> Self {
        assert!(range.start <= range.end && range.end <= self.selection.len());
        Self {
            document: self.document.clone(),
            selection: self.selection.start.saturating_add(range.start)
                ..self.selection.start.saturating_add(range.end),
        }
    }

    pub(crate) fn argument(
        &self,
        text: &str,
    ) -> Option<Self> {
        self.text()
            .rfind(text)
            .map(|start| self.select(start..start.saturating_add(text.len())))
    }
}

#[derive(Clone)]
pub struct Region {
    cover: Range<usize>,
    body: Range<usize>,
    input: Input,
    declaration: bool,
}
impl Region {
    #[allow(
        clippy::single_call_fn,
        reason = "constructs a reusable mapping from generated to entered declaration source"
    )]
    pub(crate) const fn declaration(
        start: usize,
        generated_end: usize,
        body_len: usize,
        input: Input,
    ) -> Self {
        Self {
            cover: start..generated_end,
            body: start..start.saturating_add(body_len),
            input,
            declaration: true,
        }
    }

    pub(crate) fn expression(
        start: usize,
        prefix_len: usize,
        generated_end: usize,
        input: Input,
    ) -> Self {
        let begin = start.saturating_add(prefix_len);
        let body = begin..begin.saturating_add(input.selection.len());
        Self {
            cover: start..generated_end,
            body,
            input,
            declaration: false,
        }
    }

    const fn contains(
        &self,
        span: Span,
    ) -> bool {
        let start = span.start as usize;
        // EOF belongs to the current submission, not to the preceding one.
        start >= self.cover.start && start <= self.cover.end
    }
}

type WarningSet = Rc<RefCell<HashSet<String>>>;
#[derive(Default)]
pub struct History {
    next: Cell<u64>,
    accepted: Vec<Region>,
    warnings: WarningSet,
}
impl History {
    pub(crate) fn input(
        &self,
        text: &str,
        path: Option<&Path>,
    ) -> Input {
        let id = self.next.get().saturating_add(1);
        self.next.set(id);
        Input {
            document: Rc::new(Document {
                id,
                name: path.map(|path| path.to_string_lossy().into_owned()),
                text: text.into(),
            }),
            selection: 0..text.len(),
        }
    }

    pub(crate) fn accept(
        &mut self,
        view: &View,
    ) {
        if let Some(region) = view.current.as_ref().filter(|region| region.declaration)
            && !self.accepted.iter().any(|old| {
                old.cover == region.cover && old.input.document.id == region.input.document.id
            })
        {
            self.accepted.push(region.clone());
        }
    }

    pub(crate) fn source(
        &self,
        requested: &str,
    ) -> Result<String, String> {
        let mut documents = std::collections::BTreeMap::new();
        for region in &self.accepted {
            documents
                .entry(region.input.document.id)
                .or_insert(&region.input.document);
        }
        if requested.is_empty() {
            if documents.is_empty() {
                return Ok("no accepted source; enter a definition or use :load FILE".into());
            }
            let mut text = String::from("Accepted source (use :source N to inspect an input):\n");
            for (id, doc) in documents {
                let first = doc
                    .text
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("");
                let preview: String = first.chars().take(80).collect();
                let name = doc.name.as_deref().unwrap_or("<repl>");
                let _ = writeln!(text, "  {id}  {name}  {preview}");
            }
            Ok(text.trim_end().into())
        } else {
            let id = requested.parse::<u64>().map_err(|_| {
                "usage: :source [N] — inspect accepted source by its diagnostic number".to_string()
            })?;
            let doc = documents
                .get(&id)
                .ok_or_else(|| format!("no accepted source #{id}; use :source to list inputs"))?;
            let name = doc.name.clone().unwrap_or_else(|| format!("<repl #{id}>"));
            Ok(format!("{name}\n{}", doc.text))
        }
    }

    pub(crate) fn view(
        &self,
        current: Option<Region>,
        active: Option<&Input>,
    ) -> View {
        let active = active
            .cloned()
            .or_else(|| current.as_ref().map(|region| region.input.clone()));
        View {
            accepted: self.accepted.clone(),
            current,
            active,
            warnings: self.warnings.clone(),
        }
    }
}

pub struct Compilation {
    pub(crate) raw: aevia_core::driver::ReplCompilation,
    pub(crate) view: View,
}
impl std::ops::Deref for Compilation {
    type Target = aevia_core::driver::ReplCompilation;

    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}
impl Compilation {
    pub(crate) fn render(
        &self,
        diagnostics: &Diagnostics,
        style: Style,
    ) -> String {
        self.view.render(diagnostics, &self.sources, style)
    }
}

pub struct View {
    accepted: Vec<Region>,
    current: Option<Region>,
    active: Option<Input>,
    warnings: WarningSet,
}
impl View {
    fn region(
        &self,
        span: Span,
    ) -> Option<&Region> {
        self.current
            .as_ref()
            .filter(|region| region.contains(span))
            .or_else(|| {
                self.accepted
                    .iter()
                    .rev()
                    .find(|region| region.contains(span))
            })
            .or(self.current.as_ref())
    }

    pub(crate) fn origins<'a>(
        &'a self,
        original: &'a SourceMap,
    ) -> Origins<'a> {
        let (sources, ids) = self.project_sources(original, true);
        Origins {
            view: self,
            original,
            sources,
            ids,
        }
    }

    fn project_sources(
        &self,
        original: &SourceMap,
        archived: bool,
    ) -> (SourceMap, HashMap<u64, u32>) {
        let mut sources = original.clone();
        let mut ids = HashMap::new();
        for region in self.accepted.iter().chain(self.current.iter()) {
            let document = &region.input.document;
            ids.entry(document.id).or_insert_with(|| {
                let name = document.name.clone().unwrap_or_else(|| {
                    if !archived
                        && self
                            .active
                            .as_ref()
                            .is_some_and(|input| input.document.id == document.id)
                    {
                        "<repl>".into()
                    } else {
                        format!("<repl #{}>", document.id)
                    }
                });
                sources.add(name, &document.text)
            });
        }
        (sources, ids)
    }

    fn project_span(
        &self,
        sources: &SourceMap,
        ids: &HashMap<u64, u32>,
        span: Span,
    ) -> Span {
        if sources
            .file(span.file)
            .is_none_or(|file| file.name != "<repl>")
        {
            return span;
        }
        let Some(region) = self.region(span) else {
            return span;
        };
        let Some(file) = ids.get(&region.input.document.id) else {
            return span;
        };
        let offset = |byte: u32| {
            region.input.selection.start.saturating_add(
                (byte as usize)
                    .saturating_sub(region.body.start)
                    .min(region.body.len())
                    .min(region.input.selection.len()),
            )
        };
        Span::new(*file, offset(span.start), offset(span.end.max(span.start)))
    }

    pub(crate) fn render(
        &self,
        diagnostics: &Diagnostics,
        original: &SourceMap,
        style: Style,
    ) -> String {
        let (sources, ids) = self.project_sources(original, false);
        let mut rendered = Vec::new();
        let mut emitted = HashSet::new();
        for item in &diagnostics.items {
            if item.code == aevia_core::cancellation::INTERRUPTED
                && item.severity == Severity::Error
            {
                if emitted.insert("interrupted".to_string()) {
                    rendered.push(match style {
                        | Style::Ansi => "\x1b[33minterrupted (E0812)\x1b[0m\n".to_string(),
                        | Style::Plain => "interrupted (E0812)\n".to_string(),
                    });
                }
                continue;
            }
            let mut item: Diagnostic = item.clone();
            if let Some(region) = self.current.as_ref().filter(|region| {
                !region.declaration
                    && region.contains(item.span)
                    && region.body.start > region.cover.start
            }) && let Some(binding) = original
                .file(item.span.file)
                .filter(|file| file.name == "<repl>")
                .and_then(|file| file.text.get(region.cover.start..region.body.start))
                .and_then(|prefix| prefix.strip_prefix("let "))
                .and_then(|prefix| prefix.strip_suffix(" = "))
            {
                // A displayed expression's result is used by the host;
                // an unused-temporary warning has no user meaning.
                if item.severity == Severity::Warning && item.code == "W0303" {
                    continue;
                }
                let name = format!("`{binding}`");
                if !region.input.text().contains(binding) {
                    item.message = item.message.replace(&name, "this expression");
                    item.label = item.label.replace(&name, "this expression");
                    for text in item.notes.iter_mut().chain(item.helps.iter_mut()) {
                        *text = text.replace(&name, "this expression");
                    }
                }
            }
            item.span = self.project_span(original, &ids, item.span);
            for label in &mut item.labels {
                label.span = self.project_span(original, &ids, label.span);
            }
            let key = format!(
                "{:?}|{}|{}|{}|{}|{}|{:?}|{:?}",
                item.severity,
                item.code,
                item.message,
                sources.location(item.span),
                sources.text(item.span),
                item.label,
                item.notes,
                item.helps
            );
            if !emitted.insert(key) {
                continue;
            }
            if item.severity == Severity::Warning {
                // Include document identity as well as source spelling: a new
                // submission can legitimately repeat an earlier warning.
                let identity = ids
                    .iter()
                    .find(|(_, file)| **file == item.span.file)
                    .map(|(id, _)| *id);
                let source = if identity.is_some() {
                    String::new()
                } else {
                    sources
                        .file(item.span.file)
                        .map_or(String::new(), |file| file.name.clone())
                };
                let key = format!(
                    "{identity:?}|{source}|{}..{}|{}|{}|{}|{:?}|{:?}",
                    item.span.start,
                    item.span.end,
                    item.code,
                    item.message,
                    sources.text(item.span),
                    item.notes,
                    item.helps
                );
                if !self.warnings.borrow_mut().insert(key) {
                    continue;
                }
            }
            rendered.push(item.render_with(&sources, style));
        }
        rendered.join("\n")
    }
}

/// A precomputed projection reused for all item-help origins in a checked revision.
pub struct Origins<'a> {
    view: &'a View,
    original: &'a SourceMap,
    sources: SourceMap,
    ids: HashMap<u64, u32>,
}
impl Origins<'_> {
    pub(crate) fn location(
        &self,
        span: Span,
    ) -> String {
        self.sources
            .location(self.view.project_span(self.original, &self.ids, span))
    }
}
