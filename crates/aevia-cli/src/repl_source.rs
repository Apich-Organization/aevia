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
use aevia_syntax::FileId;
use aevia_syntax::Severity;
use aevia_syntax::SourceFile;
use aevia_syntax::SourceMap;
use aevia_syntax::Span;
use aevia_syntax::diagnostic::Style;
use aevia_syntax::span::display_path;

struct Document {
    id: u64,
    source: SourceFile,
    loaded: bool,
}

#[derive(Clone)]
pub struct Input {
    document: Rc<Document>,
    selection: Range<usize>,
}
impl Input {
    pub(crate) fn text(&self) -> &str {
        &self.document.source.text[self.selection.clone()]
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

    fn project(
        &self,
        span: Span,
        file: FileId,
    ) -> Span {
        let offset = |byte: u32| {
            self.input.selection.start.saturating_add(
                (byte as usize)
                    .saturating_sub(self.body.start)
                    .min(self.body.len())
                    .min(self.input.selection.len()),
            )
        };
        Span::new(file, offset(span.start), offset(span.end.max(span.start)))
    }
}

type WarningSet = Rc<RefCell<HashSet<String>>>;

#[derive(Default)]
struct Accepted {
    // Views remember how many regions were committed when they were created.
    // Appending to the log keeps that prefix intact while a view is in use.
    regions: Vec<Region>,
    seen: HashSet<(u64, Range<usize>)>,
    ordered_prefix: usize,
}

#[derive(Default)]
pub struct History {
    next: Cell<u64>,
    accepted: Rc<RefCell<Accepted>>,
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
        let name = path.map_or_else(
            || format!("<repl #{id}>"),
            |path| path.to_string_lossy().into_owned(),
        );
        Input {
            document: Rc::new(Document {
                id,
                source: SourceFile::new(name, text),
                loaded: path.is_some(),
            }),
            selection: 0..text.len(),
        }
    }

    pub(crate) fn accept(
        &self,
        view: &View,
    ) {
        let Some(region) = view.current.as_ref().filter(|region| region.declaration) else {
            return;
        };
        let mut accepted = self.accepted.borrow_mut();
        if !accepted
            .seen
            .insert((region.input.document.id, region.cover.clone()))
        {
            return;
        }
        let extends_ordered_prefix = accepted.ordered_prefix == accepted.regions.len()
            && region.cover.start <= region.cover.end
            && accepted
                .regions
                .last()
                .is_none_or(|last| last.cover.end <= region.cover.start);
        accepted.regions.push(region.clone());
        if extends_ordered_prefix {
            accepted.ordered_prefix = accepted.regions.len();
        }
    }

    pub(crate) fn source(
        &self,
        requested: &str,
    ) -> Result<String, String> {
        let mut documents = std::collections::BTreeMap::new();
        let accepted = self.accepted.borrow();
        for region in &accepted.regions {
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
                    .source
                    .text
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("");
                let preview: String = first.chars().take(80).collect();
                let name = if doc.loaded {
                    doc.source.name.as_str()
                } else {
                    "<repl>"
                };
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
            Ok(format!("{}\n{}", doc.source.name, doc.source.text))
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
            accepted_len: self.accepted.borrow().regions.len(),
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
    accepted: Rc<RefCell<Accepted>>,
    accepted_len: usize,
    current: Option<Region>,
    active: Option<Input>,
    warnings: WarningSet,
}
impl View {
    fn region(
        &self,
        span: Span,
        original: &SourceFile,
    ) -> Option<Region> {
        if original.name != "<repl>" {
            return None;
        }
        if let Some(region) = self.current.as_ref().filter(|region| region.contains(span)) {
            return Some(region.clone());
        }
        let accepted = self.accepted.borrow();
        let visible = accepted.regions.get(..self.accepted_len)?;
        let region = if self.accepted_len <= accepted.ordered_prefix {
            visible
                .partition_point(|region| region.cover.start <= span.start as usize)
                .checked_sub(1)
                .and_then(|index| visible.get(index))
                .filter(|region| region.contains(span))
        } else {
            // Preserve most-recent precedence for overlapping or out-of-order submissions.
            visible.iter().rev().find(|region| region.contains(span))
        };
        region.or(self.current.as_ref()).cloned()
    }

    pub(crate) const fn origins<'a>(
        &'a self,
        original: &'a SourceMap,
    ) -> Origins<'a> {
        Origins { view: self, original }
    }

    pub(crate) fn render(
        &self,
        diagnostics: &Diagnostics,
        original: &SourceMap,
        style: Style,
    ) -> String {
        if diagnostics.items.is_empty() {
            return String::new();
        }
        let mut projection = Projection {
            view: self,
            original,
            sources: SourceMap::new(),
            files: HashMap::new(),
        };
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
            let (span, identity) = projection.span(item.span);
            item.span = span;
            for label in &mut item.labels {
                label.span = projection.span(label.span).0;
            }
            let sources = &projection.sources;
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
            rendered.push(item.render_with(sources, style));
        }
        rendered.join("\n")
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum ProjectedFile {
    Submitted(u64),
    Original(FileId),
}

// Only files named by a diagnostic or a label are needed by the renderer.
struct Projection<'a> {
    view: &'a View,
    original: &'a SourceMap,
    sources: SourceMap,
    files: HashMap<ProjectedFile, FileId>,
}

impl Projection<'_> {
    fn span(
        &mut self,
        span: Span,
    ) -> (Span, Option<u64>) {
        let Some(original) = self.original.file(span.file) else {
            return (
                Span {
                    file: FileId::MAX,
                    ..span
                },
                None,
            );
        };
        if let Some(region) = self.view.region(span, original) {
            let document = &region.input.document;
            let file = *self
                .files
                .entry(ProjectedFile::Submitted(document.id))
                .or_insert_with(|| {
                    let active = self
                        .view
                        .active
                        .as_ref()
                        .is_some_and(|input| input.document.id == document.id);
                    let name = if active && !document.loaded {
                        "<repl>"
                    } else {
                        &document.source.name
                    };
                    self.sources.add(name, &document.source.text)
                });
            return (region.project(span, file), Some(document.id));
        }
        let file = *self
            .files
            .entry(ProjectedFile::Original(span.file))
            .or_insert_with(|| self.sources.add(&original.name, &original.text));
        (Span { file, ..span }, None)
    }
}

/// Item-help origins borrow the source indexes of their submissions.
pub struct Origins<'a> {
    view: &'a View,
    original: &'a SourceMap,
}
impl Origins<'_> {
    pub(crate) fn location(
        &self,
        span: Span,
    ) -> String {
        let Some(region) = self
            .original
            .file(span.file)
            .and_then(|file| self.view.region(span, file))
        else {
            return self.original.location(span);
        };
        let document = &region.input.document;
        let (line, column) = document
            .source
            .line_col(region.project(span, span.file).start);
        let display;
        let name = if document.loaded {
            display = display_path(&document.source.name);
            &display
        } else {
            &document.source.name
        };
        format!("{name}:{line}:{column}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aevia_syntax::FileId;

    fn commit(
        history: &History,
        text: &str,
        start: usize,
        end: usize,
    ) {
        let input = history.input(text, None);
        let region = Region::declaration(start, end, text.len(), input);
        history.accept(&history.view(Some(region), None));
    }

    #[test]
    fn earlier_views_keep_their_snapshot_and_shared_boundaries_choose_the_latest_region() {
        let history = History::default();
        let mut original = SourceMap::new();
        let file = original.add("<repl>", "old\nnew\r\n");
        let boundary = Span::new(file, 4, 4);

        commit(&history, "old\n", 0, 4);
        let first = history.view(None, None);
        commit(&history, "new\r\n", 4, 9);
        let second = history.view(None, None);
        assert_eq!(first.origins(&original).location(boundary), "<repl #1>:2:1");
        assert_eq!(
            second.origins(&original).location(boundary),
            "<repl #2>:1:1"
        );

        commit(&history, "!!\r\n", 2, 6);
        let overlap = history.view(None, None);
        assert_eq!(
            overlap.origins(&original).location(boundary),
            "<repl #3>:1:3"
        );
        assert_eq!(
            second.origins(&original).location(boundary),
            "<repl #2>:1:1"
        );

        let input = history.input("last", None);
        let current = Region::declaration(4, 8, 4, input);
        let view = history.view(Some(current), None);
        assert_eq!(view.origins(&original).location(boundary), "<repl #4>:1:1");
        history.accept(&view);
        history.accept(&view);
        assert_eq!(
            history
                .source("")
                .unwrap()
                .lines()
                .filter(|line| line.starts_with("  4 "))
                .count(),
            1
        );
    }

    #[test]
    fn diagnostic_projection_keeps_selected_unicode_crlf_and_cross_file_labels() {
        let history = History::default();
        let previous = "old\r\n";
        commit(&history, previous, 0, previous.len());
        let entered = "préambule\r\n:type α\r\n  😀bad\r\n";
        let selection = entered.find('α').unwrap();
        let input = history.input(entered, None);
        let selected = input.select(selection..entered.len());
        let prefix = "let __repl_type = ";
        let generated = format!("{previous}{prefix}{}\n", selected.text());
        let current = Region::expression(previous.len(), prefix.len(), generated.len(), selected);
        let mut original = SourceMap::new();
        let file = original.add("<repl>", generated);
        let imported = original.add("dependency.aev", "éx\r\n");
        let emoji = previous.len() + prefix.len() + entered.find('😀').unwrap() - selection;
        let inside = Span::new(file, emoji + 1, emoji + 3);
        let cr = Span::new(file, emoji + "😀bad".len(), emoji + "😀bad".len());
        let lf = Span::new(file, cr.start as usize + 1, cr.start as usize + 1);
        let view = history.view(Some(current), Some(&input));
        let origins = view.origins(&original);
        assert_eq!(origins.location(inside), "<repl #2>:3:3");
        assert_eq!(origins.location(cr), "<repl #2>:3:7");
        assert_eq!(origins.location(lf), origins.location(cr));
        assert_eq!(
            origins.location(Span::new(imported, 2, 3)),
            "dependency.aev:1:2"
        );

        let mut diagnostics = Diagnostics::new();
        diagnostics.push(
            Diagnostic::error("E0302", "bad value in `__repl_type`", inside)
                .with_primary_label("selected here")
                .with_label(Span::new(file, 0, 3), "earlier declaration")
                .with_label(Span::new(imported, 2, 3), "imported source")
                .with_label(Span::new(FileId::MAX, 0, 0), "unknown file"),
        );
        let rendered = view.render(&diagnostics, &original, Style::Plain);
        for expected in [
            "bad value in this expression",
            "<repl>:3:3",
            "3 |   😀bad",
            "<repl #1>:1:1",
            "dependency.aev:1:2",
            "<unknown>",
        ] {
            assert!(rendered.contains(expected), "{expected}: {rendered}");
        }
        assert!(!rendered.contains("__repl_type"), "{rendered}");
    }

    #[test]
    fn repeated_warnings_are_scoped_to_the_entered_document() {
        let history = History::default();
        let source = "let unused = 1";
        let mut original = SourceMap::new();
        let file = original.add("<repl>", source);
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(Diagnostic::warning("W0001", "demo", Span::new(file, 4, 10)));
        for _ in 0..2 {
            let input = history.input(source, None);
            let region = Region::declaration(0, source.len(), source.len(), input);
            let view = history.view(Some(region), None);
            assert!(
                view.render(&diagnostics, &original, Style::Plain)
                    .contains("warning[W0001]")
            );
            assert!(
                view.render(&diagnostics, &original, Style::Plain)
                    .is_empty()
            );
        }
    }
}
