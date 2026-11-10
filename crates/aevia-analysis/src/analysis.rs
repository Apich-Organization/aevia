//! The semantic queries.
//!
//! [`Analysis`] compiles one document once and answers every question about
//! it: diagnostics, hover, completion, definition, outline and formatting. It
//! is the whole public surface a non-editor consumer needs — a playground
//! calls [`Analysis::new`] on the text in the editor and then the same queries
//! an editor would.
//!
//! Two paths produce the answers. [`Analysis::new`] compiles the document's
//! text alone — the right choice for a buffer that is not on disk. For a saved
//! file, [`Analysis::from_path`] loads the file **and its import graph**
//! through `aevia_core::driver`, so imported qualified names, `open`ed names
//! and their definitions resolve instead of being reported as unbound. The
//! multi-module answer is only used while the editor's text still matches the
//! file on disk: a stale disk answer is worse than the single-file fallback.
//!
//! Nothing here knows what a client is. There is no request vocabulary, no
//! numeric severity, no JSON: a query takes a byte offset and returns a Rust
//! value from [`crate::types`].

use crate::actions;
use crate::diagnostic::Diagnostic;
use crate::diagnostic::{
    self,
};
use crate::document::Document;
use crate::occurrence::Occurrence;
use crate::occurrence::Role;
use crate::occurrence::{
    self,
};
use crate::position::LineIndex;
use crate::position::Range;
use crate::prelude;
use crate::tokens;
use crate::types::CodeAction;
use crate::types::Completion;
use crate::types::CompletionKind;
use crate::types::FileEdit;
use crate::types::Hover;
use crate::types::Location;
use crate::types::Origin;
use crate::types::RenameError;
use crate::types::RenamePlaceholder;
use crate::types::SemanticToken;
use crate::types::SignatureHelp;
use crate::types::SignatureInfo;
use crate::types::Symbol;
use crate::types::SymbolKind;
use crate::types::TextEdit;
use crate::types::WorkspaceSymbol;
use crate::walk;
use aevia_core::compile;
use aevia_core::driver::LoadedModule;
use aevia_core::driver::Program;
use aevia_syntax::Diagnostics;
use aevia_syntax::FileId;
use aevia_syntax::SourceMap;
use aevia_syntax::Span;
use aevia_syntax::ast::*;
use aevia_types::Scheme;
use aevia_types::Type;
use aevia_types::builtins;
use aevia_types::check::CheckedModule;
use aevia_types::check::Export;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use unicode_ident::is_xid_continue;

/// Where the answers come from.
#[allow(clippy::large_enum_variant)] // two long-lived values; boxing would only add indirection
enum Source {
    /// The document's text alone — the unsaved-buffer fallback.
    Single(aevia_core::Compilation),
    /// The file and everything it imports, loaded from disk.
    ///
    /// Shared with [`crate::AnalysisCache`] so a cached program can be answered
    /// from without copying it.
    Program(Rc<Program>),
}

/// One compiled document, ready to answer questions.
pub struct Analysis<'a> {
    document: &'a Document,
    source: Source,
    /// The file the compiler gave this document. A diagnostic in any other
    /// file is not this document's to report.
    file: FileId,
    occurrences: occurrence::Occurrences,
    /// Where each name declared by the module is written.
    definitions: HashMap<String, Span>,
    /// In program mode, one declaration table per loaded module (in the
    /// program's dependency order), for jumping into an imported file.
    module_definitions: Vec<HashMap<String, Span>>,
    /// In program mode, every prefix a module's names are reachable under
    /// (`A`, an alias, or the empty string for an `open`) to its index.
    prefixes: HashMap<String, usize>,
}

impl<'a> Analysis<'a> {
    /// Compiles `document` alone: the unsaved-buffer path.
    ///
    /// The compilation is the checker's, not a second implementation of it:
    /// every type this crate reports is the one `check_module` produced. A
    /// saved file should use [`Analysis::from_path`] so its imports resolve.
    #[must_use]
    pub fn new(document: &'a Document) -> Self {
        let compilation = compile(document.name(), document.text());
        let file = compilation.module.as_ref().map_or(0, |module| module.file);
        let occurrences = compilation
            .module
            .as_ref()
            .map_or_else(occurrence::Occurrences::default, |module| {
                occurrence::collect(module, document.text())
            });
        let definitions = compilation
            .module
            .as_ref()
            .map_or_else(HashMap::new, |module| declarations(module, document.text()));
        let module_definitions = vec![definitions.clone()];
        Self {
            document,
            source: Source::Single(compilation),
            file,
            occurrences,
            definitions,
            module_definitions,
            prefixes: HashMap::new(),
        }
    }

    /// Loads a document and its import graph.
    ///
    /// The file is loaded through `aevia_core::driver`, with the package roots
    /// from `aevia.toml` when there is a manifest — exactly the resolution
    /// `aevia check` uses. Imports resolve from disk in **both** cases:
    ///
    /// * the buffer still matches the file on disk: the file is loaded as it
    ///   is (§9.10's saved-file path);
    /// * the buffer has unsaved edits: its text is loaded as an **overlay** for
    ///   the entry module (`driver::load_with_roots_and_source`), so an unsaved
    ///   file keeps its imports and its diagnostics point at the buffer text.
    ///
    /// [`Analysis::new`] remains the fallback when there is no path to load
    /// from (a scratch buffer), and [`crate::AnalysisCache`] is the right entry
    /// point for a caller that asks repeatedly.
    #[must_use]
    pub fn from_path(
        document: &'a Document,
        path: &Path,
    ) -> Self {
        if !crate::cache::analysable(path) {
            return Self::new(document);
        }
        let dirty = std::fs::read_to_string(path)
            .map(|on_disk| on_disk != document.text())
            .unwrap_or(true);
        let program = if dirty {
            crate::cache::load_dirty_program(path, document.text())
        } else {
            crate::cache::load_program(path)
        };
        Self::from_program(document, Rc::new(program))
    }

    /// Answers from an already-loaded program, when it describes this document.
    ///
    /// [`crate::AnalysisCache`] calls this with a program it kept from an
    /// earlier request. The check is the same one [`Analysis::from_path`] makes
    /// against the filesystem: if the loaded entry text is not exactly the
    /// document's text (an unsaved edit, a file that changed on disk), the
    /// answer falls back to single-file analysis rather than describing source
    /// that is not there.
    #[must_use]
    pub fn from_program(
        document: &'a Document,
        program: Rc<Program>,
    ) -> Self {
        let Some(entry) = program.entry() else {
            return Self::new(document);
        };
        let entry_text = program
            .sources
            .file(entry.file)
            .map_or("", |source| source.text.as_str());
        if entry_text != document.text() {
            return Self::new(document);
        }
        let file = entry.file;
        let occurrences = occurrence::collect(&entry.module, document.text());
        let definitions = declarations(&entry.module, document.text());
        let module_definitions = program
            .modules
            .iter()
            .map(|module| {
                let text = program
                    .sources
                    .file(module.file)
                    .map_or("", |file| file.text.as_str());
                declarations(&module.module, text)
            })
            .collect();
        let prefixes = prefixes(&program, file);
        Self {
            document,
            source: Source::Program(program),
            file,
            occurrences,
            definitions,
            module_definitions,
            prefixes,
        }
    }

    /// True when the answers come from a loaded import graph rather than from
    /// the document alone.
    #[must_use]
    pub fn is_multi_module(&self) -> bool {
        matches!(self.source, Source::Program(_))
    }

    /// The document being analysed.
    #[must_use]
    pub fn document(&self) -> &Document {
        self.document
    }

    /// The checker's interface of the document's module, including when there
    /// were type errors.
    #[must_use]
    pub fn checked(&self) -> &CheckedModule {
        self.checked_module()
    }

    /// The document's parsed module, when parsing got far enough.
    fn module(&self) -> Option<&Module> {
        match &self.source {
            | Source::Single(compilation) => compilation.module.as_ref(),
            | Source::Program(program) => program.entry().map(|entry| &entry.module),
        }
    }

    /// The document module's checked interface.
    fn checked_module(&self) -> &CheckedModule {
        match &self.source {
            | Source::Single(compilation) => &compilation.checked,
            // `from_path` only builds the program variant when it has an
            // entry module, and the loader keeps the entry last.
            | Source::Program(program) => {
                &program
                    .modules
                    .last()
                    .expect("a loaded program has an entry")
                    .checked
            },
        }
    }

    /// The source map of the answer set: one file, or the whole program.
    fn sources(&self) -> &SourceMap {
        match &self.source {
            | Source::Single(compilation) => &compilation.sources,
            | Source::Program(program) => &program.sources,
        }
    }

    /// Every diagnostic of the answer set, before filtering by file.
    fn compiler_diagnostics(&self) -> &Diagnostics {
        match &self.source {
            | Source::Single(compilation) => &compilation.diagnostics,
            | Source::Program(program) => &program.diagnostics,
        }
    }

    /// The loaded program, when the document was saved and loaded from disk.
    fn program(&self) -> Option<&Program> {
        match &self.source {
            | Source::Single(_) => None,
            | Source::Program(program) => Some(program),
        }
    }

    /// The declaration table of loaded module `index`.
    fn definitions_of(
        &self,
        index: usize,
    ) -> Option<&HashMap<String, Span>> {
        self.module_definitions.get(index)
    }

    /// Resolves a dotted name such as `Math.sqrt` to the module that exports
    /// its last segment, trying the longest module prefix first.
    fn resolve_qualified(
        &self,
        path: &[String],
    ) -> Option<(usize, &Export)> {
        let program = self.program()?;
        let (member, prefix) = path.split_last()?;
        for length in (1..=prefix.len()).rev() {
            let key = prefix[..length].join(".");
            let Some(&index) = self.prefixes.get(&key) else {
                continue;
            };
            let module = program.modules.get(index)?;
            if let Some(export) = module.checked.exports.get(member) {
                return Some((index, export));
            }
        }
        None
    }

    /// Resolves an unqualified name to a module whose `open` brings it in.
    fn resolve_opened(
        &self,
        name: &str,
    ) -> Option<(usize, &Export)> {
        let program = self.program()?;
        let entry = program
            .modules
            .iter()
            .find(|module| module.file == self.file)?;
        for edge in entry.imports.iter().rev() {
            if !edge.prefix.is_empty()
                || edge
                    .names
                    .as_ref()
                    .is_some_and(|names| !names.iter().any(|selected| selected == name))
            {
                continue;
            }
            let (index, module) = program
                .modules
                .iter()
                .enumerate()
                .find(|(_, module)| module.file == edge.file)?;
            if let Some(export) = module.checked.exports.get(name) {
                return Some((index, export));
            }
        }
        None
    }

    /// The location of a declaration in loaded module `index`.
    fn declaration_in(
        &self,
        index: usize,
        name: &str,
    ) -> Option<Location> {
        let program = self.program()?;
        let module: &LoadedModule = program.modules.get(index)?;
        let span = *self.definitions_of(index)?.get(name)?;
        let file = program.sources.file(module.file)?;
        Some(Location {
            document: file.name.clone(),
            range: range_in_file(&file.text, span),
        })
    }

    /// The location of `span` in `file`, using that file's own text.
    fn location_in_file(
        &self,
        file: FileId,
        span: Span,
    ) -> Option<Location> {
        let source = self.sources().file(file)?;
        Some(Location {
            document: source.name.clone(),
            range: range_in_file(&source.text, span),
        })
    }

    /// Every diagnostic the compiler produced for this document.
    ///
    /// Parse, type and lint diagnostics all, in source order, with spans in
    /// other files dropped rather than misplaced. This crate converts and
    /// places them; it does not invent them — `W0101` (unused binding) and
    /// `E0309` (non-exhaustive match) are the checker's own, which is what
    /// keeps the editor's squiggles identical to `aevia check`'s report.
    #[must_use]
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics: Vec<Diagnostic> = self
            .compiler_diagnostics()
            .items
            .iter()
            .filter_map(|item| diagnostic::convert(item, self.file, self.document))
            .collect();
        // Source order, so two runs over the same text produce the same list.
        diagnostics
            .sort_by_key(|diagnostic| (diagnostic.range.start.line, diagnostic.range.start.column));
        diagnostics
    }

    /// What the cursor is on, when the compiler knows something about it.
    ///
    /// Two answers, in that order. First the *name* under the cursor: the
    /// exported scheme of a binding, the pinned overload of a `Rssn.<op>`
    /// call, the class of a method. Then, when the cursor is not on a name the
    /// checker declared — a local, a literal, an operator, a whole call — the
    /// type the checker recorded for the innermost expression containing it.
    ///
    /// A name whose declaration the checker knows keeps the declared answer,
    /// because a scheme is what a reader wants (`∀a. a -> a`); when the
    /// occurrence is more specific than the declaration (a call at `String`),
    /// the occurrence's type is added underneath. Everything else is answered
    /// from [`CheckedModule::expr_types`].
    ///
    /// Returns `None` rather than a guess: no entry, a type that still names a
    /// unification variable, or a cursor on whitespace.
    #[must_use]
    pub fn hover(
        &self,
        offset: u32,
    ) -> Option<Hover> {
        if let Some(occurrence) = occurrence::at(&self.occurrences, offset) {
            if let Some(contents) = self.describe(occurrence) {
                return Some(Hover {
                    contents,
                    range: Some(self.range(occurrence.span)),
                });
            }
            // A local binding or use: the checker never exported it, but it
            // did record the expression's type at exactly this span.
            if let Some(contents) = self.describe_expression_type(occurrence) {
                return Some(Hover {
                    contents,
                    range: Some(self.range(occurrence.span)),
                });
            }
        }
        let (span, ty) = self.expression_at(offset)?;
        if let Some(contents) = self.describe_field(span) {
            return Some(Hover {
                contents,
                range: Some(self.range(span)),
            });
        }
        Some(Hover {
            contents: format!("```aevia\n{ty}\n```"),
            range: Some(self.range(span)),
        })
    }

    /// The names in scope at `offset`: this module's exports and the prelude.
    ///
    /// A name the module declares shadows the prelude, which is what the
    /// checker does. The offset is validated against the document but does not
    /// narrow the list: a local binding is deliberately absent, because this
    /// list is the module's interface, not the lexical scope at a position —
    /// the walk in [`occurrence`] answers that for [`Self::definition`]. See
    /// `.workbuddy-ai/audit/lsp.md`.
    #[must_use]
    pub fn completion(
        &self,
        offset: u32,
    ) -> Vec<Completion> {
        if usize::try_from(offset).unwrap_or(usize::MAX) > self.document.len() {
            return Vec::new();
        }
        // After `.` only the receiver's record fields make sense; the general
        // list would be noise, and the client filters the prefix.
        if let Some(fields) = self.field_completions(offset) {
            return fields;
        }
        let mut items = Vec::new();
        let mut declared = std::collections::HashSet::new();
        for (name, export) in &self.checked_module().exports {
            items.push(Completion {
                label: name.clone(),
                kind: completion_kind(export),
                detail: export_detail(export),
                origin: Origin::Module,
            });
            declared.insert(name.clone());
        }
        // Imported names: `A.f` for a module import, and the bare name for an
        // `open`. A module name shadows the prelude like any declared name.
        if let Some(program) = self.program()
            && let Some(entry) = program
                .modules
                .iter()
                .find(|module| module.file == self.file)
        {
            for edge in &entry.imports {
                let Some(module) = program
                    .modules
                    .iter()
                    .find(|module| module.file == edge.file)
                else {
                    continue;
                };
                if !edge.prefix.is_empty() && declared.insert(edge.prefix.clone()) {
                    items.push(Completion {
                        label: edge.prefix.clone(),
                        kind: CompletionKind::Type,
                        detail: Some(format!("module `{}`", edge.prefix)),
                        origin: Origin::Imported,
                    });
                }
                for (name, export) in &module.checked.exports {
                    if edge
                        .names
                        .as_ref()
                        .is_some_and(|names| !names.contains(name))
                    {
                        continue;
                    }
                    let label = if edge.prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{}.{name}", edge.prefix)
                    };
                    if declared.insert(label.clone()) {
                        items.push(Completion {
                            label,
                            kind: completion_kind(export),
                            detail: export_detail(export),
                            origin: Origin::Imported,
                        });
                    }
                }
            }
        }
        for name in prelude::names() {
            if declared.contains(name) {
                continue;
            }
            let scheme = builtins::schemes(name).and_then(|mut schemes| schemes.drain(..).next());
            items.push(Completion {
                label: name.to_string(),
                kind: match &scheme {
                    | Some(scheme) if matches!(scheme.ty, aevia_types::Type::Fn(_, _)) => {
                        CompletionKind::Function
                    },
                    | _ if builtins::is_special(name)
                        || matches!(name, "type_rep" | "from_value") =>
                    {
                        CompletionKind::Function
                    },
                    | _ => CompletionKind::Value,
                },
                detail: scheme.map(|scheme| scheme.to_string()),
                origin: Origin::Prelude,
            });
        }
        // Keywords are offered where an expression can start, so a statement
        // beginning is helped without polluting a type or an import line.
        for keyword in expression_keywords(self.document.text(), offset) {
            items.push(Completion {
                label: keyword.to_string(),
                kind: CompletionKind::Keyword,
                detail: Some("keyword".to_string()),
                origin: Origin::Language,
            });
        }
        items.sort_by(|left, right| {
            left.origin
                .cmp(&right.origin)
                .then_with(|| left.label.cmp(&right.label))
        });
        items
    }

    /// Where the name under the cursor is declared, when it can be found.
    ///
    /// A local name resolves through the walk's scope stack, which knows which
    /// parameter or `let` is in view at that offset; a module-level name
    /// resolves through the module's declaration table. In program mode an
    /// imported name resolves through the loaded module that exports it, and
    /// the returned [`Location`] names *that* file. `Rssn.<op>` has no
    /// definition to go to: the kernel is not source.
    #[must_use]
    pub fn definition(
        &self,
        offset: u32,
    ) -> Option<Location> {
        let occurrence = occurrence::at(&self.occurrences, offset)?;
        match &occurrence.role {
            | Role::Qualified(path) => {
                if path.first().map(String::as_str) == Some("Rssn") {
                    return None;
                }
                let (index, _) = self.resolve_qualified(path)?;
                self.declaration_in(index, path.last()?)
            },
            | Role::Name(name)
            | Role::Constructor(name)
            | Role::TypeName(name)
            | Role::Unit(name) => {
                if let Some(binding) = occurrence.binding {
                    return Some(Location {
                        document: self.document.name().to_string(),
                        range: self.range(binding),
                    });
                }
                if let Some(span) = self.definitions.get(name).copied() {
                    return Some(Location {
                        document: self.document.name().to_string(),
                        range: self.range(span),
                    });
                }
                let (index, _) = self.resolve_opened(name)?;
                self.declaration_in(index, name)
            },
            | Role::Method { name, .. } => {
                let span = occurrence
                    .binding
                    .or_else(|| self.definitions.get(name).copied())?;
                Some(Location {
                    document: self.document.name().to_string(),
                    range: self.range(span),
                })
            },
        }
    }

    /// The document's outline.
    #[must_use]
    pub fn symbols(&self) -> Vec<Symbol> {
        let Some(module) = self.module() else {
            return Vec::new();
        };
        SymbolContext::new(self.document.text(), self.file, self.checked_module())
            .module_symbols(module)
    }

    /// The declarations of every module in the loaded program, for a
    /// workspace-wide symbol query.
    ///
    /// Each entry names the file it is declared in and carries a range in that
    /// file's own coordinates. In single-file mode this is the current
    /// document's outline. A query is matched case-insensitively against the
    /// declaration's name; an empty query lists everything.
    #[must_use]
    pub fn workspace_symbols(
        &self,
        query: &str,
    ) -> Vec<WorkspaceSymbol> {
        let needle = query.trim().to_lowercase();
        let mut symbols = Vec::new();
        match &self.source {
            | Source::Single(compilation) => {
                let Some(module) = &compilation.module else {
                    return symbols;
                };
                let context =
                    SymbolContext::new(self.document.text(), self.file, &compilation.checked);
                collect_workspace(
                    &context.module_symbols(module),
                    self.document.name(),
                    &needle,
                    &mut symbols,
                );
            },
            | Source::Program(program) => {
                for module in &program.modules {
                    let Some(source) = program.sources.file(module.file) else {
                        continue;
                    };
                    let context = SymbolContext::new(&source.text, module.file, &module.checked);
                    collect_workspace(
                        &context.module_symbols(&module.module),
                        &source.name,
                        &needle,
                        &mut symbols,
                    );
                }
            },
        }
        symbols
    }

    /// The edit that formats the document, when it is not already formatted.
    ///
    /// A file that does not parse is left alone: [`aevia_fmt`] refuses it, and
    /// the diagnostics already say why.
    #[must_use]
    pub fn formatting(&self) -> Option<TextEdit> {
        let formatted = aevia_fmt::format_source(self.document.text()).ok()?;
        if formatted == self.document.text() {
            return None;
        }
        Some(TextEdit {
            range: self.document.full_range(),
            new_text: formatted,
        })
    }

    /// The fixes and refactors available at `request`.
    ///
    /// Each action carries edits that have been **compiled**: `aevia-analysis`
    /// offers nothing whose replacement text it has not checked. `request` is
    /// the client's selection; a cursor is a zero-width range.
    #[must_use]
    pub fn code_actions(
        &self,
        request: Range,
    ) -> Vec<CodeAction> {
        let Some(module) = self.module() else {
            return Vec::new();
        };
        // Only this document's diagnostics can call for an action; a
        // diagnostic in an imported file belongs to that editor buffer.
        let mut diagnostics = Diagnostics::new();
        for item in &self.compiler_diagnostics().items {
            if item.span.file == self.file {
                diagnostics.push(item.clone());
            }
        }
        actions::actions(
            self.document,
            module,
            self.checked_module(),
            &diagnostics,
            request,
        )
    }

    /// The highlighted words of the document: types, dimensions, units and
    /// resolved operators (§9.10).
    ///
    /// The list is ordered and non-overlapping, so a client only has to encode
    /// it.
    #[must_use]
    pub fn semantic_tokens(&self) -> Vec<SemanticToken> {
        tokens::tokens(
            self.document,
            self.file,
            &self.occurrences,
            self.checked_module(),
        )
    }

    /// The declaration of the *type* of what the cursor is on (§9.10).
    ///
    /// `definition` answers "where is this name declared"; this answers "where
    /// is the type of this value declared". Only a nominal type has a
    /// declaration to jump to: a primitive or a function type has none, so the
    /// answer is `None` rather than a nearby declaration. In program mode the
    /// type may be declared in an imported module, and the location names that
    /// file.
    #[must_use]
    pub fn type_definition(
        &self,
        offset: u32,
    ) -> Option<Location> {
        let aevia_types::Type::Adt(id, _) = self.cursor_type(offset)? else {
            return None;
        };
        let name = aevia_types::adt::name_of(*id);
        if let Some(span) = self.definitions.get(&name).copied() {
            return Some(Location {
                document: self.document.name().to_string(),
                range: self.range(span),
            });
        }
        let program = self.program()?;
        for (index, module) in program.modules.iter().enumerate().rev() {
            if let Some(span) = self
                .definitions_of(index)
                .and_then(|table| table.get(&name))
                .copied()
            {
                return self.location_in_file(module.file, span);
            }
        }
        None
    }

    /// Help for the call the cursor is inside (§9.10).
    ///
    /// The call is found in the module's parsed tree; the argument index comes
    /// from the argument spans, so a nested call's commas belong to the inner
    /// call, a tuple or record literal is one argument, and a trailing comma
    /// means the next parameter. Signatures come from what the callee names: a
    /// user function's scheme (type and dimension variables rendered as
    /// letters, constraints in the label), a constructor's scheme, a prelude
    /// builtin's overload set, a class method's dictionary with the proved
    /// instance head, or a `Rssn.<op>` operator's manifest overloads with the
    /// checker's pinned overload active. A local binding or lambda is rendered
    /// from the checker's recorded function type.
    ///
    /// `None` outside a call, on the callee name itself, and for a callee that
    /// names nothing callable.
    #[must_use]
    pub fn signature_help(
        &self,
        offset: u32,
    ) -> Option<SignatureHelp> {
        let (views, entry) = self.views();
        let view = views.get(entry)?;
        let (callee, call) = innermost_call(view.module, offset)?;
        let ExprKind::Call { args, .. } = &call.kind else {
            return None;
        };
        let text = view.text;
        let call_end = usize::try_from(call.span.end).ok()?.min(text.len());
        let callee_end = usize::try_from(callee.span.end).ok()?;
        let open = text.get(callee_end..call_end)?.find('(')? + callee_end;
        // The matching `)` is found by depth from the call's own `(`, so a
        // nested call or a tuple argument cannot close the outer call early.
        let close = matching_paren(text, open)?;
        let offset = usize::try_from(offset).ok()?;
        // On the callee name or after the closing parenthesis is not "inside
        // the call", so an editor's `(` trigger does not pop up a stale list.
        if offset <= open || offset > close {
            return None;
        }
        let argument = active_argument(args, offset);
        let (signatures, active_signature) =
            self.call_signatures(&views, view, callee, call.span, args.len())?;
        if signatures.is_empty() {
            return None;
        }
        let active_signature = active_signature.min(signatures.len() - 1);
        let parameter_count = signatures[active_signature].parameters.len();
        let active_parameter = if parameter_count == 0 {
            0
        } else {
            argument.min(parameter_count - 1)
        };
        Some(SignatureHelp {
            signatures,
            active_signature,
            active_parameter: active_parameter as u32,
        })
    }

    /// The signatures for one call's callee, and the active overload.
    fn call_signatures(
        &self,
        views: &[ModuleView<'_>],
        view: &ModuleView<'_>,
        callee: &Expr,
        call_span: Span,
        argument_count: usize,
    ) -> Option<(Vec<SignatureInfo>, usize)> {
        // A class method's call site: the checker recorded the dictionary it
        // dispatches to, so the class and the proved instance head are known.
        if let Some(dictionary) = view
            .checked
            .dictionaries
            .iter()
            .find(|dictionary| dictionary.span == call_span)
            && let Some(signature) = dictionary_signature(views, dictionary)
        {
            return Some((vec![signature], 0));
        }
        if matches!(
            view.checked.expr_types.get(&callee.span),
            Some(aevia_types::Type::ToCallbackIntrinsic)
        ) {
            return Some((vec![SignatureInfo {
                label: format!("{}: ((…) -> …) -> Callback", callee_text(view.text, callee)),
                parameters: vec!["function with a concrete checked signature".to_owned()],
                documentation: Some("Captures a callable with its concrete argument, result and dimension schema for a synchronous kernel call.".to_owned()),
            }], 0));
        }
        match view.checked.expr_types.get(&callee.span) {
            | Some(aevia_types::Type::ReadNpyIntrinsic(rank)) => {
                let tensor_rank = rank.map_or_else(|| "rank".to_string(), |rank| rank.to_string());
                let documentation = if rank.is_some() {
                    "Reads a dimensionless Float tensor. The file rank is checked when the IO action runs; decoding failures return DataError."
                } else {
                    "Select a concrete rank with read_npy<rank>(path) before calling. Decoding and rank failures return DataError."
                };
                return Some((
                    vec![SignatureInfo {
                        label: format!(
                            "{}: Str -> IO<Result<Tensor<{tensor_rank}, Float>, DataError>>",
                            callee_text(view.text, callee)
                        ),
                        parameters: vec!["path: Str".to_owned()],
                        documentation: Some(documentation.to_owned()),
                    }],
                    0,
                ));
            },
            | Some(aevia_types::Type::WriteNpyIntrinsic) => {
                return Some((vec![SignatureInfo {
                    label: format!("{}: (Str, Tensor<n, Float>) -> IO<Result<(), DataError>>", callee_text(view.text, callee)),
                    parameters: vec!["path: Str".to_owned(), "data: Tensor<n, Float> (dimensionless)".to_owned()],
                    documentation: Some("Writes a dimensionless Float tensor of any rank. Its storage must match its shape; encoding failures return DataError.".to_owned()),
                }], 0));
            },
            | _ => {},
        }
        if let Some(parts) = callee_path(callee) {
            if parts.first().map(String::as_str) == Some("Rssn")
                && parts.len() > 1
                && let Some(signatures) = rssn_signatures(view, &parts[1..], call_span)
            {
                return Some(signatures);
            }
            let resolved = if parts.len() == 1 {
                resolve_unqualified_in(views, view, &parts[0])
            } else {
                resolve_qualified_in(views, view, &parts)
            };
            if let Some((module, _)) = resolved
                && let Some(signatures) = export_signatures(views, module, &parts)
            {
                return Some(signatures);
            }
            if parts.len() == 1
                && let Some(schemes) = aevia_types::builtins::schemes(&parts[0])
            {
                let name = parts[0].as_str();
                let signatures: Vec<SignatureInfo> = schemes
                    .iter()
                    .map(|scheme| scheme_signature(name, scheme, None))
                    .collect();
                if !signatures.is_empty() {
                    let active = active_by_arity(&signatures, argument_count);
                    return Some((signatures, active));
                }
            }
        }
        // A local binding or a lambda: the declaration tables do not name it,
        // but the checker recorded the callee expression's type.
        let label = callee_text(view.text, callee);
        let ty = self.recorded_type(callee.span)?;
        let signature = type_signature(&label, ty, None)?;
        Some((vec![signature], 0))
    }

    /// Every use of the name at `offset`, across every loaded module (§9.10).
    ///
    /// The search is over the **loaded program**: each module's parsed tree is
    /// walked again in its own text, so a use in an importer is found without
    /// assuming it looks like the declaration. `include_declaration` adds the
    /// declaration site itself, the way editors ask for it. The order is stable
    /// (file, then line) and duplicates are removed.
    ///
    /// A record-field label is answered document-locally: the field is
    /// renamable only when this document declares it, and only one record type
    /// may declare it, because the source alone cannot say which type a bare
    /// `{ x = … }` or `p.x` names.
    #[must_use]
    pub fn references(
        &self,
        offset: u32,
        include_declaration: bool,
    ) -> Vec<Location> {
        let (views, entry) = self.views();
        if let Some(target) = resolve_target(&views, entry, offset) {
            return collect_references(&views, &target, include_declaration);
        }
        field_references(&views, entry, offset).unwrap_or_default()
    }

    /// Whether the name at `offset` can be renamed, and the range to preselect.
    ///
    /// Local bindings and module-level values, functions, types, aliases and
    /// constructors are renamable. A prelude builtin has no declaration this
    /// program may edit; a unit or dimension name would change the unit or type
    /// system rather than an identifier; a module path segment is not a
    /// declaration. Each of those refuses with a sentence, not a silent `None`.
    pub fn prepare_rename(
        &self,
        offset: u32,
    ) -> Result<RenamePlaceholder, RenameError> {
        let (views, entry) = self.views();
        if let Some(target) = resolve_target(&views, entry, offset) {
            check_renamable(&target)?;
            check_physical_rename(&views, &target, &target.name)?;
            let view = views.get(entry).ok_or_else(|| {
                RenameError::NotRenamable("the document has no module to rename in".to_string())
            })?;
            let occurrence = occurrence::at(&view.occurrences, offset).ok_or_else(|| {
                RenameError::NotRenamable("there is no name here to rename".to_string())
            })?;
            let Some(name) = occurrence_name(&occurrence.role) else {
                return Err(RenameError::NotRenamable(
                    "there is no name here to rename".to_string(),
                ));
            };
            let span = occurrence_edit_span(view.text, occurrence, name);
            return Ok(RenamePlaceholder {
                range: range_in_file(view.text, span),
                placeholder: target.name.clone(),
            });
        }
        let field = field_or_refusal(&views, entry, offset)?;
        let view = views.get(entry).ok_or_else(|| {
            RenameError::NotRenamable("the document has no module to rename in".to_string())
        })?;
        Ok(RenamePlaceholder {
            range: range_in_file(view.text, field.span),
            placeholder: field.name.clone(),
        })
    }

    /// The edits that rename the name at `offset` to `new_name`.
    ///
    /// The result is per file: a declaration in one module and its qualified or
    /// `open`ed uses in importers are all updated. The new name must be a valid
    /// identifier of the same shape (upper-case for types and constructors,
    /// lower-case for values) and must not collide in the scope being renamed.
    pub fn rename(
        &self,
        offset: u32,
        new_name: &str,
    ) -> Result<Vec<FileEdit>, RenameError> {
        let (views, entry) = self.views();
        if let Some(target) = resolve_target(&views, entry, offset) {
            check_renamable(&target)?;
            validate_new_name(&target.name, new_name)?;
            check_physical_rename(&views, &target, new_name)?;
            return Ok(file_edits(
                collect_references(&views, &target, true),
                new_name,
            ));
        }
        let field = field_or_refusal(&views, entry, offset)?;
        validate_new_name(&field.name, new_name)?;
        check_field_collision(&views, entry, new_name)?;
        Ok(file_edits(
            field_references(&views, entry, offset)?,
            new_name,
        ))
    }

    /// One view per loaded module, and the index of the entry (open) module.
    fn views(&self) -> (Vec<ModuleView<'_>>, usize) {
        match &self.source {
            | Source::Single(compilation) => {
                let Some(module) = &compilation.module else {
                    return (Vec::new(), 0);
                };
                let text = self.document.text();
                let view = ModuleView {
                    index: 0,
                    file: self.file,
                    document: self.document.name(),
                    text,
                    module,
                    checked: &compilation.checked,
                    definitions: &self.definitions,
                    occurrences: occurrence::collect(module, text),
                    imports: Vec::new(),
                };
                (vec![view], 0)
            },
            | Source::Program(program) => {
                let mut views = Vec::new();
                for (index, module) in program.modules.iter().enumerate() {
                    let Some(source) = program.sources.file(module.file) else {
                        continue;
                    };
                    let Some(definitions) = self.module_definitions.get(index) else {
                        continue;
                    };
                    views.push(ModuleView {
                        index,
                        file: module.file,
                        // The open buffer keeps the name its client gave it
                        // (the loader names a physical file canonically).
                        document: if module.file == self.file {
                            self.document.name()
                        } else {
                            source.name.as_str()
                        },
                        text: source.text.as_str(),
                        module: &module.module,
                        checked: &module.checked,
                        definitions,
                        occurrences: occurrence::collect(&module.module, &source.text),
                        imports: module_imports(program, module),
                    });
                }
                let entry = views
                    .iter()
                    .position(|view| view.file == self.file)
                    .unwrap_or_else(|| views.len().saturating_sub(1));
                (views, entry)
            },
        }
    }

    /// The decided type of the name or expression under the cursor.
    ///
    /// The occurrence's recorded type wins, then the declared scheme of an
    /// exported or imported name, then the innermost expression's type: the
    /// same order the hover answer uses, because both questions are "what is
    /// this".
    fn cursor_type(
        &self,
        offset: u32,
    ) -> Option<&aevia_types::Type> {
        if let Some(occurrence) = occurrence::at(&self.occurrences, offset) {
            if let Some(ty) = self.recorded_type(occurrence.span) {
                return Some(ty);
            }
            let export = match &occurrence.role {
                | Role::Name(name) | Role::Constructor(name) | Role::TypeName(name) => {
                    self.checked_module()
                        .exports
                        .get(name)
                        .or_else(|| self.resolve_opened(name).map(|(_, export)| export))
                },
                | Role::Qualified(path) => self.resolve_qualified(path).map(|(_, export)| export),
                | _ => None,
            };
            if let Some(scheme) = export.and_then(export_scheme) {
                return Some(scheme);
            }
        }
        self.expression_at(offset).map(|(_, ty)| ty)
    }

    /// The range of a compiler span in this document.
    fn range(
        &self,
        span: Span,
    ) -> Range {
        self.document.range_of(span.start, span.end)
    }

    /// The hover text for an occurrence, or `None` when nothing is known.
    fn describe(
        &self,
        occurrence: &Occurrence,
    ) -> Option<String> {
        let checked = self.checked_module();
        match &occurrence.role {
            | Role::Name(name) => {
                // A class method call the checker resolved: the interesting
                // fact is *which instance*, not the scheme, because the scheme
                // is the class's and the instance is the proof.
                if let Some(call) = occurrence.call
                    && let Some(use_) = checked
                        .dictionaries
                        .iter()
                        .find(|use_| use_.span == call && &use_.method == name)
                {
                    return Some(format!(
                        "**class method** `{name}`\n\nclass `{}` · instance for `{}`",
                        use_.class, use_.head
                    ));
                }
                if let Some(export) = checked.exports.get(name) {
                    let text = describe_export(name, export)?;
                    let polymorphic = export_is_polymorphic(export);
                    return Some(self.with_use_type(text, occurrence.span, polymorphic));
                }
                // A name an `open` brought in from another module.
                if let Some((_, export)) = self.resolve_opened(name) {
                    let text = describe_export(name, export)?;
                    let polymorphic = export_is_polymorphic(export);
                    let text = format!("{text}\n\nimported");
                    return Some(self.with_use_type(text, occurrence.span, polymorphic));
                }
                let mut schemes = builtins::schemes(name)?;
                let scheme = schemes.drain(..).next()?;
                let polymorphic = !scheme.vars.is_empty() || !scheme.dims.is_empty();
                let text = format!("```aevia\n{name}: {scheme}\n```\n\nprelude");
                Some(self.with_use_type(text, occurrence.span, polymorphic))
            },
            | Role::Constructor(name) => {
                if let Some(export) = checked.exports.get(name) {
                    return describe_export(name, export);
                }
                let (_, export) = self.resolve_opened(name)?;
                describe_export(name, export)
            },
            | Role::TypeName(name) => {
                let export = checked
                    .exports
                    .get(name)
                    .or_else(|| self.resolve_opened(name).map(|(_, export)| export))?;
                match export {
                    | Export::Type { .. } => Some(format!("type `{name}`")),
                    | Export::Alias { params } => {
                        Some(format!("type alias `{name}` with {params} parameter(s)"))
                    },
                    | Export::TypeCtor { adt, .. } => {
                        Some(format!(
                            "type `{name}`\n\nconstructor `{name}` of `{}`",
                            aevia_types::adt::name_of(*adt)
                        ))
                    },
                    | other => describe_export(name, other),
                }
            },
            | Role::Method { name, class } => {
                Some(format!("**method** `{name}`\n\nclass `{class}`"))
            },
            | Role::Unit(name) => {
                // A unit literal's unit is not an exported name; the checker's
                // unit table is the record of what it resolved to.
                let dim = self
                    .checked_module()
                    .units
                    .lookup(name)
                    .map(|unit| unit.dim.to_plain_string());
                Some(match dim {
                    | Some(dim) => format!("**unit** `{name}`\n\ndimension `{dim}`"),
                    | None => format!("**unit** `{name}`"),
                })
            },
            | Role::Qualified(path) => self.describe_qualified(path, occurrence.span),
        }
    }

    /// The type the checker recorded for the expression or binder at `span`.
    ///
    /// A type that still names a unification variable is the checker saying
    /// "not decided here" — `t7` would look like information — and `Error` is
    /// its "inference already failed". Neither is an answer.
    fn recorded_type(
        &self,
        span: Span,
    ) -> Option<&aevia_types::Type> {
        let ty = self.checked_module().expr_types.get(&span)?;
        if ty.is_error() || aevia_types::class::has_variables(ty) {
            return None;
        }
        Some(ty)
    }

    /// Hover text for an occurrence answered from the expression table.
    ///
    /// A name keeps its name (`x: Int`); anything else is the bare type, which
    /// is what the expression *is*.
    fn describe_expression_type(
        &self,
        occurrence: &Occurrence,
    ) -> Option<String> {
        let ty = self.recorded_type(occurrence.span)?;
        Some(match &occurrence.role {
            | Role::Name(name) => format!("```aevia\n{name}: {ty}\n```"),
            | _ => format!("```aevia\n{ty}\n```"),
        })
    }

    /// The hover text for a record field access, when the base's type is a
    /// record whose row declares the field.
    ///
    /// This is how a field of a type imported from another module is named:
    /// the checker resolved the record in this module's own type table, and the
    /// row says which field the access is.
    fn describe_field(
        &self,
        span: Span,
    ) -> Option<String> {
        let module = self.module()?;
        let ExprKind::Field { base, name } = &walk::exactly(module, span)?.kind else {
            return None;
        };
        let base_type = self.recorded_type(base.span)?;
        let aevia_types::Type::Record(row) = base_type else {
            return None;
        };
        let field_type = row.field(name)?;
        Some(format!("```aevia\nfield {name}: {field_type}\n```"))
    }

    /// The innermost expression whose recorded span contains `offset`.
    ///
    /// "Innermost" is the shortest span: in `1.0 + 2` the binary expression
    /// covers the whole text, but a cursor on `1.0` is inside the literal's
    /// own span, and that is the expression the question is about.
    fn expression_at(
        &self,
        offset: u32,
    ) -> Option<(Span, &aevia_types::Type)> {
        // Whitespace is not an expression. Without this, a cursor on the
        // indent before a block's tail would hover the whole block.
        let text = self.document.text().as_bytes();
        let at = usize::try_from(offset).ok()?;
        if at >= text.len() || text[at].is_ascii_whitespace() {
            return None;
        }
        let mut best: Option<(&Span, &aevia_types::Type)> = None;
        for (span, ty) in &self.checked_module().expr_types {
            if span.start > offset || offset >= span.end {
                continue;
            }
            let shorter = best.is_none_or(|(best_span, _)| {
                span.end - span.start < best_span.end - best_span.start
            });
            if shorter {
                best = Some((span, ty));
            }
        }
        let (span, ty) = best?;
        // The innermost expression is the one the question is about. If *its*
        // type is not decided, the answer is silence — falling back to an
        // enclosing expression would describe a different piece of source.
        if ty.is_error() || aevia_types::class::has_variables(ty) {
            return None;
        }
        Some((*span, ty))
    }

    /// Adds the type this occurrence actually has, when the declaration's
    /// scheme does not already say it.
    ///
    /// `id` is `∀a. a -> a` wherever it is written; at `id("s")` the useful
    /// extra fact is that *this* use is `String -> String`. A monomorphic
    /// declaration has nothing to add.
    fn with_use_type(
        &self,
        text: String,
        span: Span,
        polymorphic: bool,
    ) -> String {
        if !polymorphic {
            return text;
        }
        match self.recorded_type(span) {
            | Some(ty) => format!("{text}\n\nas used here: `{ty}`"),
            | None => text,
        }
    }

    /// The hover text for a dotted name.
    ///
    /// A dotted name from an imported module (`Math.sqrt`, `A.Type`) is
    /// resolved against the loaded program's exports, so hover answers with the
    /// same scheme `aevia check` used. `Rssn.<op>` is the one dotted name the
    /// *checker* records: it pins the overload at the call site, and that pin is
    /// the whole point of pinning, so it is what the hover reports.
    fn describe_qualified(
        &self,
        path: &[String],
        span: Span,
    ) -> Option<String> {
        if let Some((index, export)) = self.resolve_qualified(path) {
            let full = path.join(".");
            let text = describe_export(&full, export)?;
            let polymorphic = export_is_polymorphic(export);
            let mut text = self.with_use_type(text, span, polymorphic);
            if let Some(program) = self.program()
                && let Some(module) = program.modules.get(index)
            {
                text.push_str(&format!("\n\nimported from `{}`", module.path.join(".")));
            }
            return Some(text);
        }
        let [root, op] = path else {
            return None;
        };
        if root != "Rssn" {
            return None;
        }
        let call = self.checked_module().rssn_calls.iter().find(|call| {
            call.name.as_str() == op.as_str()
                && call.span.start <= span.start
                && span.end <= call.span.end
        })?;
        let entry = aevia_rssn::registry().op(op);
        let overloads = entry.map_or(0, |entry| entry.signatures.len());
        let mut text = format!(
            "**kernel operator** `Rssn.{op}`\n\npinned overload {} of {overloads} · `{}`",
            call.overload + 1,
            call.key
        );
        if let Some(signature) = entry.and_then(|entry| entry.signatures.get(call.overload)) {
            text.push_str(&format!("\n\n```aevia\n{}\n```", signature.render()));
        }
        if let Some(entry) = entry
            && !entry.doc.is_empty()
        {
            text.push_str(&format!("\n\n{}", entry.doc));
        }
        Some(text)
    }
}

/// Builds outline entries for one module, in that module's own coordinates.
///
/// The same builder serves the open document and every other module of a loaded
/// program: a symbol's range must be computed against the text it came from, and
/// its detail against the checks of the module that declares it. Sharing the
/// code is what keeps the document outline and the workspace-wide symbol list
/// from disagreeing about a declaration.
struct SymbolContext<'a> {
    text: &'a str,
    file: FileId,
    checked: &'a CheckedModule,
}

impl<'a> SymbolContext<'a> {
    fn new(
        text: &'a str,
        file: FileId,
        checked: &'a CheckedModule,
    ) -> Self {
        Self { text, file, checked }
    }

    /// The range of a span in this context's file.
    fn range(
        &self,
        span: Span,
    ) -> Range {
        if span.file != self.file {
            return Range::default();
        }
        range_in_file(self.text, span)
    }

    /// The source text a span covers, when it is in this file.
    fn text_of(
        &self,
        span: Span,
    ) -> String {
        if span.file != self.file {
            return String::new();
        }
        let start = usize::try_from(span.start).unwrap_or(usize::MAX);
        let end = usize::try_from(span.end).unwrap_or(usize::MAX);
        self.text.get(start..end).unwrap_or_default().to_string()
    }

    /// The range of a declaration's name.
    fn name_range(
        &self,
        within: Span,
        name: &str,
    ) -> Range {
        self.range(occurrence::name_span(self.text, within, name))
    }

    /// The checker's rendering of an exported name's scheme, when it has one.
    fn scheme_of(
        &self,
        name: &str,
    ) -> Option<String> {
        match self.checked.exports.get(name)? {
            | Export::Value { scheme, .. } => scheme_is_known(scheme).then(|| scheme.to_string()),
            | Export::Ctor { scheme, .. } | Export::TypeCtor { scheme, .. } => {
                scheme_is_known(scheme).then(|| scheme.to_string())
            },
            | Export::Type { .. } | Export::Alias { .. } => None,
            | Export::Unit { def } => Some(format!("unit: {}", def.dim.to_plain_string())),
            | Export::Dimension { dim, .. } => {
                Some(format!("dimension: {}", dim.to_plain_string()))
            },
        }
    }

    /// Every top-level declaration of `module` as an outline entry.
    fn module_symbols(
        &self,
        module: &Module,
    ) -> Vec<Symbol> {
        module
            .decls
            .iter()
            .filter_map(|decl| self.declaration_symbol(decl))
            .collect()
    }

    /// One declaration as an outline entry.
    fn declaration_symbol(
        &self,
        decl: &Decl,
    ) -> Option<Symbol> {
        let name_range = |name: &str| self.name_range(decl.span, name);
        let whole = self.range(decl.span);
        let detail = |name: &str| self.scheme_of(name);
        let symbol = match &decl.kind {
            | DeclKind::Import { path, .. } => {
                Symbol {
                    name: path.join("."),
                    kind: SymbolKind::Namespace,
                    range: whole,
                    selection: whole,
                    detail: None,
                    children: Vec::new(),
                }
            },
            | DeclKind::Dimension { name, .. } => {
                Symbol {
                    name: name.clone(),
                    kind: SymbolKind::TypeParameter,
                    range: whole,
                    selection: name_range(name),
                    detail: None,
                    children: Vec::new(),
                }
            },
            | DeclKind::Unit { name, dim, .. } => {
                Symbol {
                    name: name.clone(),
                    kind: SymbolKind::Constant,
                    range: whole,
                    selection: name_range(name),
                    detail: Some(format!("unit: {}", dim.to_source())),
                    children: Vec::new(),
                }
            },
            | DeclKind::Type { name, body, .. } => {
                let (kind, children) = match body {
                    | TypeBody::Alias(_) => (SymbolKind::Struct, Vec::new()),
                    | TypeBody::Abstract => (SymbolKind::Interface, Vec::new()),
                    | TypeBody::Variants(ctors) => {
                        (
                            SymbolKind::Enum,
                            ctors
                                .iter()
                                .map(|ctor| {
                                    Symbol {
                                        name: ctor.name.clone(),
                                        kind: SymbolKind::EnumMember,
                                        range: self.range(ctor.span),
                                        selection: self.range(ctor.span),
                                        detail: None,
                                        children: Vec::new(),
                                    }
                                })
                                .collect(),
                        )
                    },
                };
                Symbol {
                    name: name.clone(),
                    kind,
                    range: whole,
                    selection: name_range(name),
                    detail: None,
                    children,
                }
            },
            | DeclKind::Class { name, methods, .. } => {
                Symbol {
                    name: name.clone(),
                    kind: SymbolKind::Interface,
                    range: whole,
                    selection: name_range(name),
                    detail: None,
                    children: methods
                        .iter()
                        .map(|(method, ty)| {
                            Symbol {
                                name: method.clone(),
                                kind: SymbolKind::Method,
                                range: whole,
                                selection: self.name_range(decl.span, method),
                                detail: Some(self.text_of(ty.span)),
                                children: Vec::new(),
                            }
                        })
                        .collect(),
                }
            },
            | DeclKind::Instance { class, ty, methods } => {
                Symbol {
                    name: format!("instance {class} for {}", self.text_of(ty.span)),
                    kind: SymbolKind::Object,
                    range: whole,
                    selection: whole,
                    detail: None,
                    children: methods
                        .iter()
                        .map(|method| {
                            Symbol {
                                name: method.name.clone(),
                                kind: SymbolKind::Method,
                                range: self.range(method.span),
                                selection: self.name_range(method.span, &method.name),
                                detail: None,
                                children: Vec::new(),
                            }
                        })
                        .collect(),
                }
            },
            | DeclKind::Derive { .. } => return None,
            | DeclKind::Fn { name, body, .. } => {
                Symbol {
                    name: name.clone(),
                    kind: SymbolKind::Function,
                    range: whole,
                    selection: name_range(name),
                    detail: detail(name),
                    children: self.nested(body),
                }
            },
            | DeclKind::Let { pattern, .. } => {
                let PatKind::Var(name) = &pattern.kind else {
                    return None;
                };
                Symbol {
                    name: name.clone(),
                    kind: SymbolKind::Variable,
                    range: whole,
                    selection: self.range(pattern.span),
                    detail: detail(name),
                    children: Vec::new(),
                }
            },
            | DeclKind::Test { name, .. } => {
                Symbol {
                    name: name.clone(),
                    kind: SymbolKind::Constant,
                    range: whole,
                    selection: whole,
                    detail: None,
                    children: Vec::new(),
                }
            },
        };
        Some(symbol)
    }

    /// The declarations nested directly in a function body.
    fn nested(
        &self,
        body: &Expr,
    ) -> Vec<Symbol> {
        let ExprKind::Block { stmts, .. } = &body.kind else {
            return Vec::new();
        };
        stmts
            .iter()
            .filter_map(|stmt| {
                match stmt {
                    | BlockStmt::Fn(decl) => self.declaration_symbol(decl),
                    | BlockStmt::Let { .. } => None,
                }
            })
            .collect()
    }
}

/// Adds `symbols` and their children, flattened, to `out` when the query
/// matches.
fn collect_workspace(
    symbols: &[Symbol],
    document: &str,
    needle: &str,
    out: &mut Vec<WorkspaceSymbol>,
) {
    let mut flattened = Vec::new();
    for symbol in symbols {
        flatten(symbol, &mut flattened);
    }
    for mut symbol in flattened {
        symbol.children.clear();
        if needle.is_empty() || symbol.name.to_lowercase().contains(needle) {
            out.push(WorkspaceSymbol {
                document: document.to_string(),
                symbol,
            });
        }
    }
}

/// Appends an outline entry and its descendants, parents first.
fn flatten(
    symbol: &Symbol,
    out: &mut Vec<Symbol>,
) {
    out.push(symbol.clone());
    for child in &symbol.children {
        flatten(child, out);
    }
}

/// Every prefix a loaded module's names are reachable under.
///
/// A module path is always a prefix; an `import … as A` adds `A`; an `open`
/// adds the empty prefix, which is how a bare name is found.
fn prefixes(
    program: &Program,
    file: FileId,
) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    if let Some((index, module)) = program
        .modules
        .iter()
        .enumerate()
        .find(|(_, module)| module.file == file)
    {
        if !module.path.is_empty() {
            map.insert(module.path.join("."), index);
        }
        for edge in &module.imports {
            if !edge.prefix.is_empty()
                && let Some(target) = program
                    .modules
                    .iter()
                    .position(|target| target.file == edge.file)
            {
                map.insert(edge.prefix.clone(), target);
            }
        }
    }
    map
}

/// The range of `span` in a file whose text is `text`.
fn range_in_file(
    text: &str,
    span: Span,
) -> Range {
    let index = LineIndex::new(text);
    Range::new(
        index.position(text, span.start),
        index.position(text, span.end),
    )
}

/// The scheme of an export, when the checker decided one.
fn export_scheme(export: &Export) -> Option<&aevia_types::Type> {
    let scheme = match export {
        | Export::Value { scheme, .. }
        | Export::Ctor { scheme, .. }
        | Export::TypeCtor { scheme, .. } => scheme,
        | Export::Type { .. }
        | Export::Alias { .. }
        | Export::Unit { .. }
        | Export::Dimension { .. } => {
            return None;
        },
    };
    if scheme.ty.is_error() || aevia_types::class::has_variables(&scheme.ty) {
        return None;
    }
    Some(&scheme.ty)
}

/// Where every name the module declares is written.
///
/// An `instance` method is only recorded when exactly one instance declares
/// that name: with two, the source alone cannot say which one a call reaches —
/// the checker's `DictionaryUse` is what knows — and pointing at a coin flip
/// would be worse than pointing at nothing.
fn declarations(
    module: &Module,
    text: &str,
) -> HashMap<String, Span> {
    let mut map: HashMap<String, Span> = HashMap::new();
    let mut methods: HashMap<&str, Vec<Span>> = HashMap::new();
    for decl in &module.decls {
        match &decl.kind {
            | DeclKind::Fn { name, .. }
            | DeclKind::Type { name, .. }
            | DeclKind::Class { name, .. }
            | DeclKind::Dimension { name, .. }
            | DeclKind::Unit { name, .. } => {
                add(&mut map, text, name, decl.span);
            },
            | DeclKind::Let { pattern, .. } => {
                if let PatKind::Var(name) = &pattern.kind {
                    add(&mut map, text, name, pattern.span);
                }
            },
            | DeclKind::Instance {
                methods: declared, ..
            } => {
                for method in declared {
                    methods
                        .entry(method.name.as_str())
                        .or_default()
                        .push(occurrence::name_span(text, method.span, &method.name));
                }
            },
            | DeclKind::Import { .. } | DeclKind::Derive { .. } | DeclKind::Test { .. } => {},
        }
        if let DeclKind::Type {
            body: TypeBody::Variants(ctors),
            ..
        } = &decl.kind
        {
            for ctor in ctors {
                map.entry(ctor.name.clone()).or_insert(ctor.span);
            }
        }
    }
    for (name, spans) in methods {
        if let [span] = spans.as_slice() {
            map.entry(name.to_string()).or_insert(*span);
        }
    }
    map
}

/// Records where `name` is written inside `within`, keeping the first
/// declaration of a name.
fn add(
    map: &mut HashMap<String, Span>,
    text: &str,
    name: &str,
    within: Span,
) {
    map.entry(name.to_string())
        .or_insert_with(|| occurrence::name_span(text, within, name));
}

/// True when an export's scheme has variables the occurrence could instantiate.
fn export_is_polymorphic(export: &Export) -> bool {
    match export {
        | Export::Value { scheme, .. }
        | Export::Ctor { scheme, .. }
        | Export::TypeCtor { scheme, .. } => !scheme.vars.is_empty() || !scheme.dims.is_empty(),
        | Export::Type { .. }
        | Export::Alias { .. }
        | Export::Unit { .. }
        | Export::Dimension { .. } => false,
    }
}

/// The completion kind of an exported name.
fn completion_kind(export: &Export) -> CompletionKind {
    match export {
        | Export::Value { arity, .. } if *arity > 0 => CompletionKind::Function,
        | Export::Value { .. } => CompletionKind::Value,
        | Export::Ctor { .. } | Export::TypeCtor { .. } => CompletionKind::Constructor,
        | Export::Type { .. } | Export::Alias { .. } => CompletionKind::Type,
        | Export::Unit { .. } => CompletionKind::Unit,
        | Export::Dimension { .. } => CompletionKind::Dimension,
    }
}

/// True when a scheme says something about its type.
///
/// A binding whose inference failed is exported with a bare type variable, and
/// showing `t0` as a type would be a worse answer than showing nothing: it
/// looks like information.
fn scheme_is_known(scheme: &aevia_types::Scheme) -> bool {
    !matches!(
        scheme.ty,
        aevia_types::Type::Var(_) | aevia_types::Type::Error
    )
}

/// The detail line for an exported name.
fn export_detail(export: &Export) -> Option<String> {
    match export {
        | Export::Value { scheme, .. } | Export::Ctor { scheme, .. } => {
            scheme_is_known(scheme).then(|| scheme.to_string())
        },
        | Export::TypeCtor { scheme, adt, .. } => {
            scheme_is_known(scheme)
                .then(|| format!("{} · {}", aevia_types::adt::name_of(*adt), scheme))
        },
        | Export::Type { adt } => Some(aevia_types::adt::name_of(*adt)),
        | Export::Alias { params } => Some(format!("{params} parameter(s)")),
        | Export::Unit { def } => Some(format!("unit: {}", def.dim.to_plain_string())),
        | Export::Dimension { dim, .. } => Some(format!("dimension: {}", dim.to_plain_string())),
    }
}

/// The hover text for an exported name, when the checker knows its type.
fn describe_export(
    name: &str,
    export: &Export,
) -> Option<String> {
    Some(match export {
        | Export::Value { scheme, arity, .. } => {
            if !scheme_is_known(scheme) {
                return None;
            }
            let mut text = format!("```aevia\n{name}: {scheme}\n```");
            if *arity == 0 {
                text.push_str("\n\nvalue");
            }
            text
        },
        | Export::Ctor { adt, scheme, .. } | Export::TypeCtor { adt, scheme, .. } => {
            if !scheme_is_known(scheme) {
                return None;
            }
            format!(
                "```aevia\n{name}: {scheme}\n```\n\nconstructor of `{}`",
                aevia_types::adt::name_of(*adt)
            )
        },
        | Export::Type { .. } => format!("type `{name}`"),
        | Export::Alias { params } => format!("type alias `{name}` with {params} parameter(s)"),
        | Export::Unit { def } => {
            format!(
                "**unit** `{name}`\n\ndimension `{}`",
                def.dim.to_plain_string()
            )
        },
        | Export::Dimension { dim, is_base } => {
            if *is_base {
                format!("**base dimension** `{name}`")
            } else {
                format!("**dimension** `{name}`\n\n= `{}`", dim.to_plain_string())
            }
        },
    })
}

// --- References and rename -------------------------------------------------

/// What kind of declaration an export names, for rename refusals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExportKind {
    Value,
    Ctor,
    TypeCtor,
    Type,
    Alias,
    Unit,
    Dimension,
}

fn export_kind(export: &Export) -> ExportKind {
    match export {
        | Export::Value { .. } => ExportKind::Value,
        | Export::Ctor { .. } => ExportKind::Ctor,
        | Export::TypeCtor { .. } => ExportKind::TypeCtor,
        | Export::Type { .. } => ExportKind::Type,
        | Export::Alias { .. } => ExportKind::Alias,
        | Export::Unit { .. } => ExportKind::Unit,
        | Export::Dimension { .. } => ExportKind::Dimension,
    }
}

/// One loaded module, re-walked in its own text for a reference search.
struct ModuleView<'a> {
    /// Its position in the program's module list, the index the declaration
    /// tables use.
    index: usize,
    file: FileId,
    /// What a location in this module names its file.
    document: &'a str,
    text: &'a str,
    /// The parsed module, for the field-label walk.
    module: &'a Module,
    checked: &'a CheckedModule,
    definitions: &'a HashMap<String, Span>,
    occurrences: occurrence::Occurrences,
    /// The prefixes this module's imports bring other modules in under.
    imports: Vec<(String, usize, Option<Vec<String>>)>,
}

fn view_of<'v, 'a>(
    views: &'v [ModuleView<'a>],
    module: usize,
) -> Option<&'v ModuleView<'a>> {
    views.iter().find(|view| view.index == module)
}

fn view_of_file<'v, 'a>(
    views: &'v [ModuleView<'a>],
    file: FileId,
) -> Option<&'v ModuleView<'a>> {
    views.iter().find(|view| view.file == file)
}

/// The prefixes `module`'s imports bring other modules in under.
fn module_imports(
    program: &Program,
    module: &LoadedModule,
) -> Vec<(String, usize, Option<Vec<String>>)> {
    module
        .imports
        .iter()
        .filter_map(|edge| {
            let index = program
                .modules
                .iter()
                .position(|target| target.file == edge.file)?;
            Some((edge.prefix.clone(), index, edge.names.clone()))
        })
        .collect()
}

/// The name a role carries, when it names a declaration or use.
fn occurrence_name(role: &Role) -> Option<&str> {
    match role {
        | Role::Name(name) | Role::Constructor(name) | Role::TypeName(name) | Role::Unit(name) => {
            Some(name)
        },
        | Role::Qualified(path) => path.last().map(String::as_str),
        | Role::Method { .. } => None,
    }
}

/// The span of the name to replace: the last segment of a dotted name, so
/// `A.sqrt` renames `sqrt` and not the module prefix.
fn occurrence_edit_span(
    text: &str,
    occurrence: &Occurrence,
    name: &str,
) -> Span {
    match &occurrence.role {
        | Role::Qualified(_) => last_name_span(text, occurrence.span, name),
        | _ => occurrence::name_span(text, occurrence.span, name),
    }
}

/// The last whole-word occurrence of `name` inside `within`.
fn last_name_span(
    text: &str,
    within: Span,
    name: &str,
) -> Span {
    occurrence::identifier_span(text, within, name, true)
        .unwrap_or_else(|| occurrence::name_span(text, within, name))
}

fn identifier_continue(character: char) -> bool {
    is_xid_continue(character) || character == '\'' || ('₀'..='₉').contains(&character)
}

fn decl_span_in(
    views: &[ModuleView<'_>],
    module: usize,
    name: &str,
) -> Option<(FileId, Span)> {
    let view = view_of(views, module)?;
    let span = *view.definitions.get(name)?;
    Some((view.file, occurrence::name_span(view.text, span, name)))
}

fn resolve_export(
    views: &[ModuleView<'_>],
    module: usize,
    name: &str,
) -> Option<ExportKind> {
    view_of(views, module)?
        .checked
        .exports
        .get(name)
        .map(export_kind)
}

fn resolve_unqualified_in(
    views: &[ModuleView<'_>],
    view: &ModuleView<'_>,
    name: &str,
) -> Option<(usize, ExportKind)> {
    if let Some(kind) = view.checked.exports.get(name).map(export_kind) {
        return Some((view.index, kind));
    }
    for (prefix, target, names) in view.imports.iter().rev() {
        if !prefix.is_empty()
            || names
                .as_ref()
                .is_some_and(|names| !names.iter().any(|selected| selected == name))
        {
            continue;
        }
        if let Some(kind) = resolve_export(views, *target, name) {
            return Some((*target, kind));
        }
    }
    None
}

fn resolve_qualified_in(
    views: &[ModuleView<'_>],
    view: &ModuleView<'_>,
    path: &[String],
) -> Option<(usize, ExportKind)> {
    let (member, prefix) = path.split_last()?;
    for length in (1..=prefix.len()).rev() {
        let key = prefix[..length].join(".");
        if let Some((_, target, _)) = view
            .imports
            .iter()
            .rev()
            .find(|(import, _, _)| *import == key)
            && let Some(kind) = resolve_export(views, *target, member)
        {
            return Some((*target, kind));
        }
    }
    None
}

/// The declaration an occurrence names, as `(file, name span)`.
///
/// A binding is already a declaration location; a bare or dotted name resolves
/// through the module's exports and its imports to the module that declares it.
fn canonical_decl_of(
    views: &[ModuleView<'_>],
    view: &ModuleView<'_>,
    occurrence: &Occurrence,
) -> Option<(FileId, Span)> {
    let name = occurrence_name(&occurrence.role)?;
    if let Some(binding) = occurrence.binding {
        return Some((view.file, binding));
    }
    let (module, _) = match &occurrence.role {
        | Role::Qualified(path) => resolve_qualified_in(views, view, path)?,
        | Role::Name(_) | Role::Constructor(_) | Role::TypeName(_) | Role::Unit(_) => {
            resolve_unqualified_in(views, view, name)?
        },
        | Role::Method { .. } => return None,
    };
    decl_span_in(views, module, name)
}

/// What the cursor's name is, canonically.
struct Target {
    /// The file and span of the declaration.
    file: FileId,
    span: Span,
    name: String,
    reach: Reach,
}

/// Where the name is declared, which decides what a rename may refuse.
enum Reach {
    /// A binding inside a function: renamable in its file.
    Local,
    /// A module-level declaration: renamable across the loaded program.
    Module { module: usize, kind: ExportKind },
}

fn resolve_target(
    views: &[ModuleView<'_>],
    entry: usize,
    offset: u32,
) -> Option<Target> {
    let view = views.get(entry)?;
    let occurrence = occurrence::at(&view.occurrences, offset)?;
    let name = occurrence_name(&occurrence.role)?.to_string();
    if let Role::Qualified(path) = &occurrence.role
        && path.first().map(String::as_str) == Some("Rssn")
    {
        return None;
    }
    if matches!(occurrence.role, Role::Method { .. }) {
        return None;
    }
    if let Some(binding) = occurrence.binding {
        // The walk binds a module-level declaration in its own scope too, so
        // binding equality with the declaration table is what tells the two
        // apart. A module declaration renames program-wide; a parameter or
        // `let` renames in its file.
        let declaration = view
            .definitions
            .get(&name)
            .map(|span| occurrence::name_span(view.text, *span, &name));
        if declaration == Some(binding)
            && let Some(kind) = view.checked.exports.get(&name).map(export_kind)
        {
            return Some(Target {
                file: view.file,
                span: binding,
                name,
                reach: Reach::Module {
                    module: view.index,
                    kind,
                },
            });
        }
        return Some(Target {
            file: view.file,
            span: binding,
            name,
            reach: Reach::Local,
        });
    }
    let (module, kind) = match &occurrence.role {
        | Role::Qualified(path) => resolve_qualified_in(views, view, path)?,
        | Role::Name(_) | Role::Constructor(_) | Role::TypeName(_) | Role::Unit(_) => {
            resolve_unqualified_in(views, view, &name)?
        },
        | Role::Method { .. } => return None,
    };
    let (file, span) = decl_span_in(views, module, &name)?;
    Some(Target {
        file,
        span,
        name,
        reach: Reach::Module { module, kind },
    })
}

/// Dispatch identities remain owner-specific; an edit names one physical text
/// declaration even when it was checked under several package owners.
fn physical_declaration<'v>(
    views: &'v [ModuleView<'_>],
    file: FileId,
    span: Span,
) -> Option<(&'v str, u32, u32)> {
    Some((view_of_file(views, file)?.document, span.start, span.end))
}

fn check_physical_rename(
    views: &[ModuleView<'_>],
    target: &Target,
    new_name: &str,
) -> Result<(), RenameError> {
    let target_key = physical_declaration(views, target.file, target.span).ok_or_else(|| {
        RenameError::NotRenamable("the declaring source is not loaded".to_owned())
    })?;
    if target_key.0.starts_with("stdlib://") || target_key.0.starts_with("rssn://") {
        return Err(RenameError::NotRenamable(
            "generated library sources cannot be renamed by a workspace edit".to_owned(),
        ));
    }
    check_collision(views, target, new_name)?;
    let mut edited = std::collections::HashSet::new();
    for view in views {
        for occurrence in &view.occurrences {
            let Some(name) = occurrence_name(&occurrence.role) else {
                continue;
            };
            let Some((file, span)) = canonical_decl_of(views, view, occurrence) else {
                continue;
            };
            if physical_declaration(views, file, span) != Some(target_key) {
                continue;
            }
            if new_name != target.name
                && matches!(occurrence.role, Role::Name(_))
                && let Some(binding) = view.occurrences.lookup(occurrence, new_name)
                && physical_declaration(views, view.file, binding) != Some(target_key)
            {
                return Err(RenameError::Collision(format!(
                    "`{new_name}` would capture a reference in `{}`",
                    view.document
                )));
            }
            let occurrence_span = occurrence_edit_span(view.text, occurrence, name);
            edited.insert((view.document, occurrence_span.start, occurrence_span.end));
            // Check every owner's scope of the shared declaration.
            if view.document == target_key.0
                && occurrence_span.start == target_key.1
                && occurrence_span.end == target_key.2
            {
                let reach = match &target.reach {
                    | Reach::Local => Reach::Local,
                    | Reach::Module { kind, .. } => {
                        Reach::Module {
                            module: view.index,
                            kind: *kind,
                        }
                    },
                };
                check_collision(
                    views,
                    &Target {
                        file: view.file,
                        span: occurrence_span,
                        name: target.name.clone(),
                        reach,
                    },
                    new_name,
                )?;
            }
        }
    }
    // Renaming a binding can also capture uses which are not edited: e.g. a
    // parameter renamed to an opened function already used inside its body.
    if new_name != target.name {
        for view in views {
            for occurrence in &view.occurrences {
                if !matches!(&occurrence.role, Role::Name(name) if name == new_name)
                    || view.occurrences.lookup(occurrence, new_name).is_some()
                {
                    continue;
                }
                let mut old_name = occurrence.clone();
                old_name.role = Role::Name(target.name.clone());
                old_name.binding = view.occurrences.lookup(occurrence, &target.name);
                let reachable = canonical_decl_of(views, view, &old_name)
                    .and_then(|(file, span)| physical_declaration(views, file, span));
                if reachable == Some(target_key) {
                    return Err(RenameError::Collision(format!(
                        "renaming to `{new_name}` would capture an existing reference in `{}`",
                        view.document
                    )));
                }
            }
        }
    }
    // The same bytes can be interpreted differently by another owner. Editing
    // that use for one interpretation would silently break the other one.
    for view in views {
        for occurrence in &view.occurrences {
            let Some(name) = occurrence_name(&occurrence.role) else {
                continue;
            };
            let span = occurrence_edit_span(view.text, occurrence, name);
            if !edited.contains(&(view.document, span.start, span.end)) {
                continue;
            }
            let key = canonical_decl_of(views, view, occurrence)
                .and_then(|(file, span)| physical_declaration(views, file, span));
            if key != Some(target_key) {
                return Err(RenameError::NotRenamable(format!(
                    "shared source `{}` resolves this use to different declarations under different package owners",
                    view.document
                )));
            }
        }
    }
    Ok(())
}

/// Every location whose declaration is `target`'s.
fn collect_references(
    views: &[ModuleView<'_>],
    target: &Target,
    include_declaration: bool,
) -> Vec<Location> {
    let mut locations: Vec<Location> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for view in views {
        for occurrence in &view.occurrences {
            let Some(name) = occurrence_name(&occurrence.role) else {
                continue;
            };
            let Some(canonical) = canonical_decl_of(views, view, occurrence) else {
                continue;
            };
            if physical_declaration(views, canonical.0, canonical.1)
                != physical_declaration(views, target.file, target.span)
            {
                continue;
            }
            let span = occurrence_edit_span(view.text, occurrence, name);
            if !include_declaration
                && physical_declaration(views, view.file, span)
                    == physical_declaration(views, target.file, target.span)
            {
                continue;
            }
            if !seen.insert((view.document.to_string(), span.start, span.end)) {
                continue;
            }
            locations.push(Location {
                document: view.document.to_string(),
                range: range_in_file(view.text, span),
            });
        }
    }
    locations.sort_by(|left, right| {
        left.document
            .cmp(&right.document)
            .then(left.range.start.line.cmp(&right.range.start.line))
            .then(left.range.start.column.cmp(&right.range.start.column))
    });
    locations
}

fn check_renamable(target: &Target) -> Result<(), RenameError> {
    match &target.reach {
        | Reach::Local => Ok(()),
        | Reach::Module { kind, .. } => {
            match kind {
                | ExportKind::Unit => {
                    Err(RenameError::NotRenamable(format!(
                        "`{}` is a unit; renaming it would change the unit system, not an identifier",
                        target.name
                    )))
                },
                | ExportKind::Dimension => {
                    Err(RenameError::NotRenamable(format!(
                        "`{}` is a dimension; renaming it would change the type system, not an identifier",
                        target.name
                    )))
                },
                | ExportKind::Value
                | ExportKind::Ctor
                | ExportKind::TypeCtor
                | ExportKind::Type
                | ExportKind::Alias => Ok(()),
            }
        },
    }
}

fn validate_new_name(
    old: &str,
    new: &str,
) -> Result<(), RenameError> {
    if aevia_syntax::token::Keyword::from_word(new).is_some() {
        return Err(RenameError::InvalidName(format!("`{new}` is a keyword")));
    }
    if new.is_empty() || !is_identifier(new) {
        return Err(RenameError::InvalidName(format!(
            "`{new}` is not an Aevia identifier"
        )));
    }
    let upper = |text: &str| text.chars().next().is_some_and(char::is_uppercase);
    if upper(old) != upper(new) {
        return Err(RenameError::InvalidName(
            "a type, constructor or module name keeps an upper-case initial; a value keeps a \
             lower-case one"
                .to_string(),
        ));
    }
    Ok(())
}

fn is_identifier(text: &str) -> bool {
    if text == "_" {
        return false;
    }
    let lexed = aevia_syntax::lexer::lex_inline(0, text, 0);
    matches!(lexed.tokens.as_slice(), [first, end]
        if matches!(&first.kind, aevia_syntax::token::TokenKind::Ident(name)
            | aevia_syntax::token::TokenKind::UpperIdent(name) if name == text)
            && matches!(end.kind, aevia_syntax::token::TokenKind::Eof))
}

fn check_collision(
    views: &[ModuleView<'_>],
    target: &Target,
    new_name: &str,
) -> Result<(), RenameError> {
    if new_name == target.name {
        return Ok(());
    }
    match &target.reach {
        | Reach::Module { module, .. } => {
            let view = view_of(views, *module).ok_or_else(|| {
                RenameError::NotRenamable("the declaring module is not loaded".to_string())
            })?;
            if view.checked.exports.contains_key(new_name) {
                return Err(RenameError::Collision(format!(
                    "`{new_name}` is already declared in `{}`",
                    view.document
                )));
            }
            Ok(())
        },
        | Reach::Local => {
            let Some(view) = view_of_file(views, target.file) else {
                return Ok(());
            };
            if view.definitions.contains_key(new_name) {
                return Err(RenameError::Collision(format!(
                    "`{new_name}` is already declared in `{}`",
                    view.document
                )));
            }
            let taken = view.occurrences.iter().any(|occurrence| {
                let (Role::Name(name) | Role::TypeName(name) | Role::Constructor(name)) =
                    &occurrence.role
                else {
                    return false;
                };
                name == new_name && occurrence.binding == Some(occurrence.span)
            });
            if taken {
                return Err(RenameError::Collision(format!(
                    "`{new_name}` is already bound in `{}`",
                    view.document
                )));
            }
            Ok(())
        },
    }
}

// --- Record-field labels ---------------------------------------------------

/// A field label written somewhere in one module.
struct FieldLabel {
    name: String,
    span: Span,
    /// The record type that declares it, when the label is in a type body.
    owner: Option<String>,
}

/// A field label the user asked to rename.
struct FieldTarget {
    name: String,
    #[allow(dead_code)]
    owner: String,
    span: Span,
}

/// The field labels declared by record types in `module`, as
/// `(type, field, label span)`.
fn declared_record_fields(
    module: &Module,
    text: &str,
) -> Vec<(String, String, Span)> {
    let mut fields = Vec::new();
    for decl in &module.decls {
        let DeclKind::Type {
            name: owner,
            body: TypeBody::Alias(alias),
            ..
        } = &decl.kind
        else {
            continue;
        };
        let TyKind::Record { fields: declared, .. } = &alias.kind else {
            continue;
        };
        for (field, field_ty) in declared {
            fields.push((
                owner.clone(),
                field.clone(),
                label_before(text, alias.span.file, field_ty.span.start, field),
            ));
        }
    }
    fields
}

/// Every field label written in `module`: type bodies, record expressions,
/// field accesses and patterns in declarations.
fn field_labels(
    module: &Module,
    text: &str,
) -> Vec<FieldLabel> {
    let mut labels = Vec::new();
    for (owner, name, span) in declared_record_fields(module, text) {
        labels.push(FieldLabel {
            name,
            span,
            owner: Some(owner),
        });
    }
    walk::module_exprs(module, |expr| {
        match &expr.kind {
            | ExprKind::Record { fields, .. } => {
                for (field, value) in fields {
                    labels.push(FieldLabel {
                        name: field.clone(),
                        span: label_before(text, expr.span.file, value.span.start, field),
                        owner: None,
                    });
                }
            },
            | ExprKind::Field { name, .. } => {
                labels.push(FieldLabel {
                    name: name.clone(),
                    span: last_name_span(text, expr.span, name),
                    owner: None,
                })
            },
            | _ => {},
        }
    });
    for decl in &module.decls {
        match &decl.kind {
            | DeclKind::Fn { args, .. } => {
                for param in args {
                    labels_in_pattern(text, &param.pattern, &mut labels);
                }
            },
            | DeclKind::Instance { methods, .. } => {
                for method in methods {
                    for param in &method.params {
                        labels_in_pattern(text, &param.pattern, &mut labels);
                    }
                }
            },
            | DeclKind::Let { pattern, .. } => labels_in_pattern(text, pattern, &mut labels),
            | _ => {},
        }
    }
    labels
}

/// Record-field labels in a pattern.
fn labels_in_pattern(
    text: &str,
    pattern: &Pat,
    out: &mut Vec<FieldLabel>,
) {
    match &pattern.kind {
        | PatKind::Record(fields) => {
            for (field, sub) in fields {
                let before = sub.as_ref().map_or(pattern.span.end, |sub| sub.span.start);
                out.push(FieldLabel {
                    name: field.clone(),
                    span: label_before(text, pattern.span.file, before, field),
                    owner: None,
                });
                if let Some(sub) = sub {
                    labels_in_pattern(text, sub, out);
                }
            }
        },
        | PatKind::Ctor { args, .. } => {
            for arg in args {
                labels_in_pattern(text, arg, out);
            }
        },
        | PatKind::Tuple(items) => {
            for item in items {
                labels_in_pattern(text, item, out);
            }
        },
        | PatKind::Tensor { items, .. } => {
            for item in items {
                labels_in_pattern(text, item, out);
            }
        },
        | PatKind::As { pattern, .. } => labels_in_pattern(text, pattern, out),
        | PatKind::Annot { pattern, .. } => labels_in_pattern(text, pattern, out),
        | PatKind::Wildcard | PatKind::Error | PatKind::Var(_) | PatKind::Lit(_) => {},
    }
}

/// The label `name` immediately before byte `before`, in a small window.
///
/// A record type is written `{ x: τ }` and a record expression `{ x = e }`, so
/// the label is the last whole-word `name` before its value or type. That is
/// enough because a label and its value are adjacent; the window keeps it from
/// matching an unrelated earlier name.
fn label_before(
    text: &str,
    file: FileId,
    before: u32,
    name: &str,
) -> Span {
    let end = usize::try_from(before)
        .unwrap_or(usize::MAX)
        .min(text.len());
    let window_start = end.saturating_sub(96);
    let Some(slice) = text.get(window_start..end) else {
        return Span::new(file, end, end);
    };
    for (at, _) in slice.rmatch_indices(name) {
        let before_ok = at == 0
            || !slice[..at]
                .chars()
                .next_back()
                .is_some_and(identifier_continue);
        let after = at + name.len();
        let after_ok = after >= slice.len()
            || !slice[after..]
                .chars()
                .next()
                .is_some_and(identifier_continue);
        if before_ok && after_ok {
            let start = window_start + at;
            return Span::new(file, start, start + name.len());
        }
    }
    Span::new(file, end, end)
}

/// Resolve the record field under the cursor, or say why it is refused.
fn field_target(
    views: &[ModuleView<'_>],
    entry: usize,
    offset: u32,
) -> Result<FieldTarget, RenameError> {
    let Some(view) = views.get(entry) else {
        return Err(RenameError::NotRenamable(
            "the document has no module to rename in".to_string(),
        ));
    };
    let labels = field_labels(view.module, view.text);
    let Some(cursor) = labels
        .iter()
        .filter(|label| label.span.start <= offset && offset <= label.span.end)
        .max_by_key(|label| label.span.end.saturating_sub(label.span.start))
    else {
        return Err(RenameError::NotRenamable(
            "there is no declaration here that a rename could change".to_string(),
        ));
    };
    let declared = declared_record_fields(view.module, view.text);
    let owners: Vec<&String> = declared
        .iter()
        .filter(|(_, name, _)| name == &cursor.name)
        .map(|(owner, _, _)| owner)
        .collect();
    match owners.as_slice() {
        | [owner] => {
            if let Some(declared_in) = &cursor.owner
                && declared_in != *owner
            {
                return Err(RenameError::NotRenamable(format!(
                    "`{}` is a field of `{declared_in}`, which is declared in this document, \
                         but the field's own type cannot be renamed from here",
                    cursor.name
                )));
            }
            Ok(FieldTarget {
                name: cursor.name.clone(),
                owner: (*owner).clone(),
                span: cursor.span,
            })
        },
        | [] => {
            Err(RenameError::NotRenamable(format!(
                "`{}` is a record field of a type this document does not declare",
                cursor.name
            )))
        },
        | _ => {
            Err(RenameError::NotRenamable(format!(
                "`{}` is a field of several record types in this document; renaming it would be \
             ambiguous",
                cursor.name
            )))
        },
    }
}

/// The field under the cursor, or the sentence that best explains the refusal.
///
/// When the cursor is on a name the occurrence walk knows but no declaration
/// table resolves (a prelude builtin, a kernel operation, a unit literal, an
/// instance method), the refusal is about *that* name, not about record fields.
fn field_or_refusal(
    views: &[ModuleView<'_>],
    entry: usize,
    offset: u32,
) -> Result<FieldTarget, RenameError> {
    match field_target(views, entry, offset) {
        | Ok(field) => Ok(field),
        | Err(error) => {
            match views.get(entry) {
                | Some(view) if occurrence::at(&view.occurrences, offset).is_some() => {
                    Err(undeclarable_error(view, offset))
                },
                | _ => Err(error),
            }
        },
    }
}

/// Why a name the occurrence walk found cannot be renamed.
fn undeclarable_error(
    view: &ModuleView<'_>,
    offset: u32,
) -> RenameError {
    let Some(occurrence) = occurrence::at(&view.occurrences, offset) else {
        return RenameError::NotRenamable(
            "there is no declaration here that a rename could change".to_string(),
        );
    };
    match &occurrence.role {
        | Role::Qualified(path) if path.first().map(String::as_str) == Some("Rssn") => {
            RenameError::NotRenamable(format!(
                "`{}` is a kernel operation; the kernel is not source",
                path.join(".")
            ))
        },
        | Role::Unit(name) => {
            RenameError::NotRenamable(format!(
                "`{name}` is a unit; renaming it would change the unit system, not an identifier"
            ))
        },
        | Role::Method { name, .. } => {
            RenameError::NotRenamable(format!(
                "instance method `{name}` is dispatched by the checker; methods are not renamable yet"
            ))
        },
        | Role::Name(name) | Role::Constructor(name) | Role::TypeName(name) => {
            RenameError::NotRenamable(format!(
                "`{name}` has no declaration in this program that a rename could change (a \
                 prelude builtin is not source)"
            ))
        },
        | Role::Qualified(path) => {
            RenameError::NotRenamable(format!(
                "`{}` does not resolve to a declaration in this program",
                path.join(".")
            ))
        },
    }
}

/// The labels that rename with the field under the cursor.
fn field_references(
    views: &[ModuleView<'_>],
    entry: usize,
    offset: u32,
) -> Result<Vec<Location>, RenameError> {
    let field = field_target(views, entry, offset)?;
    let Some(view) = views.get(entry) else {
        return Err(RenameError::NotRenamable(
            "the document has no module to rename in".to_string(),
        ));
    };
    let mut locations = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for label in field_labels(view.module, view.text) {
        if label.name != field.name {
            continue;
        }
        if !seen.insert((label.span.start, label.span.end)) {
            continue;
        }
        locations.push(Location {
            document: view.document.to_string(),
            range: range_in_file(view.text, label.span),
        });
    }
    locations.sort_by(|left, right| {
        left.range
            .start
            .line
            .cmp(&right.range.start.line)
            .then(left.range.start.column.cmp(&right.range.start.column))
    });
    Ok(locations)
}

fn check_field_collision(
    views: &[ModuleView<'_>],
    entry: usize,
    new_name: &str,
) -> Result<(), RenameError> {
    let Some(view) = views.get(entry) else {
        return Ok(());
    };
    let declared = declared_record_fields(view.module, view.text);
    if declared.iter().any(|(_, name, _)| name == new_name) {
        return Err(RenameError::Collision(format!(
            "`{new_name}` is already a record field in `{}`",
            view.document
        )));
    }
    Ok(())
}

/// Rename edits from locations, in their stable order.
fn file_edits(
    locations: Vec<Location>,
    new_name: &str,
) -> Vec<FileEdit> {
    locations
        .into_iter()
        .map(|location| {
            FileEdit {
                document: location.document,
                edit: TextEdit {
                    range: location.range,
                    new_text: new_name.to_string(),
                },
            }
        })
        .collect()
}

// --- Signature help --------------------------------------------------------

/// The innermost call expression containing `offset`, as `(callee, call)`.
fn innermost_call(
    module: &Module,
    offset: u32,
) -> Option<(&Expr, &Expr)> {
    let mut best: Option<(&Expr, &Expr)> = None;
    walk::module_exprs(module, |expr| {
        let ExprKind::Call { callee, .. } = &expr.kind else {
            return;
        };
        if !(expr.span.start <= offset && offset <= expr.span.end) {
            return;
        }
        let length = expr.span.end.saturating_sub(expr.span.start);
        let replace = match best {
            | None => true,
            | Some((_, current)) => length < current.span.end.saturating_sub(current.span.start),
        };
        if replace {
            best = Some((callee.as_ref(), expr));
        }
    });
    best
}

/// The index of the `)` matching the `(` at `open`, by nesting depth.
fn matching_paren(
    text: &str,
    open: usize,
) -> Option<usize> {
    let mut depth = 0usize;
    for (index, character) in text.char_indices().skip_while(|(index, _)| *index <= open) {
        match character {
            | '(' => depth += 1,
            | ')' => {
                if depth == 0 {
                    return Some(index);
                }
                depth -= 1;
            },
            | _ => {},
        }
    }
    None
}

/// Which argument the cursor is in.
///
/// The spans are the AST's, so a comma inside a nested call, a tuple or a
/// record does not split an argument; a cursor between arguments is on the next
/// one, which is what a trailing comma means.
fn active_argument(
    args: &[Expr],
    offset: usize,
) -> usize {
    let offset = u32::try_from(offset).unwrap_or(u32::MAX);
    let mut active = 0;
    for (index, argument) in args.iter().enumerate() {
        if offset <= argument.span.end {
            return index;
        }
        active = index + 1;
    }
    active
}

/// The dotted path a callee expression spells, when it spells one.
fn callee_path(expr: &Expr) -> Option<Vec<String>> {
    match &expr.kind {
        | ExprKind::QName(path) => Some(path.clone()),
        | ExprKind::Var(name) => Some(vec![name.clone()]),
        | ExprKind::Field { base, name } => {
            let mut path = callee_path(base)?;
            path.push(name.clone());
            Some(path)
        },
        | ExprKind::TypeApp { callee, .. } => callee_path(callee),
        | _ => None,
    }
}

/// The source text of a callee expression.
fn callee_text(
    text: &str,
    callee: &Expr,
) -> String {
    let start = usize::try_from(callee.span.start).unwrap_or(usize::MAX);
    let end = usize::try_from(callee.span.end)
        .unwrap_or(usize::MAX)
        .min(text.len());
    text.get(start..end).unwrap_or("call").trim().to_string()
}

/// Which overload an argument count selects; the first when none is exact.
fn active_by_arity(
    signatures: &[SignatureInfo],
    argument_count: usize,
) -> usize {
    signatures
        .iter()
        .position(|signature| signature.parameters.len() == argument_count)
        .unwrap_or(0)
}

/// Readable names for quantified variables in one rendered signature.
#[derive(Default)]
struct VarNames {
    names: HashMap<String, String>,
    next: usize,
}

impl VarNames {
    /// Replaces the checker's `t0`/`p1`/`r2` spellings with `a`, `b`, … so a
    /// signature is readable. Dimension variables keep their `d0` spelling: the
    /// surface language never invents names for them either.
    fn readable(
        &mut self,
        text: &str,
    ) -> String {
        let characters: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut index = 0;
        while index < characters.len() {
            let character = characters[index];
            if matches!(character, 't' | 'p' | 'r') {
                let mut end = index + 1;
                while end < characters.len() && characters[end].is_ascii_digit() {
                    end += 1;
                }
                if end > index + 1 {
                    let token: String = characters[index..end].iter().collect();
                    let name = self.names.entry(token).or_insert_with(|| {
                        let name = variable_name(self.next);
                        self.next += 1;
                        name
                    });
                    out.push_str(name);
                    index = end;
                    continue;
                }
            }
            out.push(character);
            index += 1;
        }
        out
    }
}

fn variable_name(index: usize) -> String {
    if index < 26 {
        ((b'a' + u8::try_from(index).unwrap_or(0)) as char).to_string()
    } else {
        format!("v{index}")
    }
}

/// One signature rendered from a declaration's scheme.
fn scheme_signature(
    name: &str,
    scheme: &Scheme,
    documentation: Option<String>,
) -> SignatureInfo {
    let mut names = VarNames::default();
    if let Type::Fn(parameters, ret) = &scheme.ty {
        let parameters: Vec<String> = parameters
            .iter()
            .map(|parameter| names.readable(&parameter.to_string()))
            .collect();
        let ret = names.readable(&ret.to_string());
        let constraints: Vec<String> = scheme
            .constraints
            .iter()
            .map(|constraint| names.readable(&constraint.to_string()))
            .collect();
        let prefix = if constraints.is_empty() {
            String::new()
        } else {
            format!("{} => ", constraints.join(", "))
        };
        SignatureInfo {
            label: format!("{name}: {prefix}({}) -> {ret}", parameters.join(", ")),
            parameters,
            documentation,
        }
    } else {
        SignatureInfo {
            label: format!("{name}: {}", names.readable(&scheme.ty.to_string())),
            parameters: Vec::new(),
            documentation,
        }
    }
}

/// One signature rendered from a decided function type.
fn type_signature(
    name: &str,
    ty: &Type,
    documentation: Option<String>,
) -> Option<SignatureInfo> {
    let Type::Fn(parameters, ret) = ty else {
        return None;
    };
    let mut names = VarNames::default();
    let parameters: Vec<String> = parameters
        .iter()
        .map(|parameter| names.readable(&parameter.to_string()))
        .collect();
    let ret = names.readable(&ret.to_string());
    Some(SignatureInfo {
        label: format!("{name}: ({}) -> {ret}", parameters.join(", ")),
        parameters,
        documentation,
    })
}

/// The signatures of a callable declaration in `module`.
fn export_signatures(
    views: &[ModuleView<'_>],
    module: usize,
    parts: &[String],
) -> Option<(Vec<SignatureInfo>, usize)> {
    let name = parts.last()?.as_str();
    let display = parts.join(".");
    let view = view_of(views, module)?;
    let export = view.checked.exports.get(name)?;
    let signature = match export {
        | Export::Value { scheme, .. } => {
            scheme_signature(&display, scheme, declaration_doc(view, name))
        },
        | Export::Ctor { adt, scheme, .. } | Export::TypeCtor { adt, scheme, .. } => {
            scheme_signature(
                &display,
                scheme,
                type_declaration_doc(view, &aevia_types::adt::name_of(*adt)),
            )
        },
        | Export::Type { .. }
        | Export::Alias { .. }
        | Export::Unit { .. }
        | Export::Dimension { .. } => return None,
    };
    Some((vec![signature], 0))
}

/// The manifest overloads of `Rssn.<op>`, with the checker's pinned one active.
fn rssn_signatures(
    view: &ModuleView<'_>,
    parts: &[String],
    call_span: Span,
) -> Option<(Vec<SignatureInfo>, usize)> {
    let [operator] = parts else {
        return None;
    };
    let entry = aevia_rssn::registry::registry().op(operator)?;
    let signatures: Vec<SignatureInfo> = entry
        .signatures
        .iter()
        .map(|signature| {
            SignatureInfo {
                label: format!("Rssn.{operator}: {}", signature.render()),
                parameters: signature
                    .params
                    .iter()
                    .map(aevia_rssn::registry::render_type)
                    .collect(),
                documentation: (!entry.doc.is_empty()).then(|| entry.doc.clone()),
            }
        })
        .collect();
    if signatures.is_empty() {
        return None;
    }
    let pinned = view
        .checked
        .rssn_calls
        .iter()
        .find(|call| call.span == call_span)
        .and_then(|call| entry.key_index(&call.key))
        .unwrap_or(0);
    let count = signatures.len();
    Some((signatures, pinned.min(count - 1)))
}

/// One class method's signature, with the class and proved instance head.
fn dictionary_signature(
    views: &[ModuleView<'_>],
    dictionary: &aevia_types::check::DictionaryUse,
) -> Option<SignatureInfo> {
    let class = dictionary.class.as_str();
    let method = dictionary.method.as_str();
    let mut method_ty = None;
    for view in views {
        for export in view.checked.classes.values() {
            if let Some((_, ty)) = export.methods.iter().find(|(name, _)| name == method) {
                method_ty = Some(ty);
                break;
            }
        }
        if method_ty.is_some() {
            break;
        }
    }
    let Type::Fn(parameters, ret) = method_ty? else {
        return None;
    };
    let mut names = VarNames::default();
    let parameters: Vec<String> = parameters
        .iter()
        .map(|parameter| names.readable(&parameter.to_string()))
        .collect();
    let ret = names.readable(&ret.to_string());
    let head = names.readable(&dictionary.head.to_string());
    Some(SignatureInfo {
        label: format!(
            "{method}: {class} {head} => ({}) -> {ret}",
            parameters.join(", ")
        ),
        parameters,
        documentation: Some(format!(
            "class `{class}` method; the checker proved the `{head}` instance at this call"
        )),
    })
}

/// The `##` documentation immediately above a declaration.
fn declaration_doc(
    view: &ModuleView<'_>,
    name: &str,
) -> Option<String> {
    for decl in &view.module.decls {
        let matches = match &decl.kind {
            | DeclKind::Fn { name: declared, .. } => declared == name,
            | DeclKind::Let { pattern, .. } => {
                matches!(&pattern.kind, PatKind::Var(bound) if bound == name)
            },
            | _ => false,
        };
        if matches {
            return doc_before(view.text, decl.span.start);
        }
    }
    None
}

/// The `##` documentation immediately above a type declaration.
fn type_declaration_doc(
    view: &ModuleView<'_>,
    name: &str,
) -> Option<String> {
    for decl in &view.module.decls {
        if let DeclKind::Type { name: declared, .. } = &decl.kind
            && declared == name
        {
            return doc_before(view.text, decl.span.start);
        }
    }
    None
}

/// The `##` comment lines immediately before byte `start`.
fn doc_before(
    text: &str,
    start: u32,
) -> Option<String> {
    let start = usize::try_from(start).ok()?;
    let before = text.get(..start)?;
    let mut lines: Vec<&str> = before.split('\n').collect();
    if !lines.last().is_some_and(|line| line.trim().is_empty()) {
        return None;
    }
    lines.pop();
    let mut documented = Vec::new();
    while let Some(line) = lines.last() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("##") else {
            break;
        };
        documented.push(rest.trim().to_string());
        lines.pop();
    }
    documented.reverse();
    if documented.is_empty() {
        None
    } else {
        Some(documented.join("\n"))
    }
}

// --- Completion details ----------------------------------------------------

impl Analysis<'_> {
    /// The record fields of the expression before a `.`.
    ///
    /// The receiver is the innermost expression whose span ends where the dot
    /// begins. Its type comes from the checker when it recorded one (calls and
    /// field accesses are); a plain local variable's *use* has no entry, so the
    /// binder's annotation is read from the tree and resolved through the
    /// module's alias tables instead.
    fn field_completions(
        &self,
        offset: u32,
    ) -> Option<Vec<Completion>> {
        let offset = usize::try_from(offset).ok()?;
        let text = self.document.text();
        let mut start = offset;
        while start > 0 {
            let character = text[..start].chars().next_back()?;
            if character.is_alphanumeric() || character == '_' {
                start -= character.len_utf8();
            } else {
                break;
            }
        }
        let dot = start.checked_sub(1)?;
        if text.as_bytes().get(dot) != Some(&b'.') {
            return None;
        }
        let receiver_end = dot;
        let (views, entry) = self.views();
        let view = views.get(entry)?;
        let mut receiver: Option<&Expr> = None;
        walk::module_exprs(view.module, |expr| {
            if usize::try_from(expr.span.end).ok() == Some(receiver_end) {
                let replace = match receiver {
                    | None => true,
                    | Some(current) => expr.span.start >= current.span.start,
                };
                if replace {
                    receiver = Some(expr);
                }
            }
        });
        let fields = self.receiver_fields(&views, view, receiver?)?;
        let mut items: Vec<Completion> = fields
            .into_iter()
            .map(|(label, detail)| {
                Completion {
                    label,
                    kind: CompletionKind::Field,
                    detail,
                    origin: Origin::Module,
                }
            })
            .collect();
        items.sort_by(|left, right| left.label.cmp(&right.label));
        Some(items)
    }

    /// Field labels and rendered field types for a receiver expression.
    fn receiver_fields(
        &self,
        views: &[ModuleView<'_>],
        view: &ModuleView<'_>,
        receiver: &Expr,
    ) -> Option<Vec<(String, Option<String>)>> {
        if let Some(Type::Record(row)) = self.recorded_type(receiver.span) {
            return Some(
                row.fields()
                    .into_iter()
                    .map(|(label, ty)| (label, Some(ty.to_string())))
                    .collect(),
            );
        }
        let ExprKind::Var(_) = &receiver.kind else {
            return None;
        };
        let binding = occurrence::at(&self.occurrences, receiver.span.start)?.binding?;
        let annotation = binding_annotation(view.module, binding)?;
        annotation_fields(views, view, annotation)
    }
}

/// The type annotation written on the binder at `binding`.
fn binding_annotation(
    module: &Module,
    binding: Span,
) -> Option<&Ty> {
    for decl in &module.decls {
        match &decl.kind {
            | DeclKind::Fn { args, .. } => {
                for param in args {
                    if param.pattern.span == binding {
                        return param.ty.as_ref();
                    }
                }
            },
            | DeclKind::Let { pattern, ty, .. } => {
                if pattern.span == binding {
                    return ty.as_ref();
                }
            },
            | DeclKind::Instance { methods, .. } => {
                for method in methods {
                    for param in &method.params {
                        if param.pattern.span == binding {
                            return param.ty.as_ref();
                        }
                    }
                }
            },
            | _ => {},
        }
    }
    let mut found = None;
    walk::module_exprs(module, |expr| {
        match &expr.kind {
            | ExprKind::Block { stmts, .. } => {
                for statement in stmts {
                    if let BlockStmt::Let { pattern, ty, .. } = statement
                        && pattern.span == binding
                    {
                        found = ty.as_ref();
                    }
                }
            },
            | ExprKind::Do { stmts } => {
                for statement in stmts {
                    if let DoStmt::Let { pattern, ty, .. } = statement
                        && pattern.span == binding
                    {
                        found = ty.as_ref();
                    }
                }
            },
            | _ => {},
        }
    });
    found
}

/// Field labels of a type annotation, resolving a named alias to its record.
fn annotation_fields(
    views: &[ModuleView<'_>],
    view: &ModuleView<'_>,
    annotation: &Ty,
) -> Option<Vec<(String, Option<String>)>> {
    match &annotation.kind {
        | TyKind::Record { fields, .. } => {
            Some(
                fields
                    .iter()
                    .map(|(name, field_ty)| (name.clone(), source_slice(view.text, field_ty.span)))
                    .collect(),
            )
        },
        | TyKind::Named { path, .. } => {
            let name = path.last()?;
            let qualified = path.join(".");
            for candidate in views {
                for key in [name.as_str(), qualified.as_str()] {
                    if let Some(Type::Record(row)) = candidate.checked.aliases.get(key) {
                        return Some(
                            row.fields()
                                .into_iter()
                                .map(|(label, ty)| (label, Some(ty.to_string())))
                                .collect(),
                        );
                    }
                    if let Some(alias) = candidate.checked.alias_defs.get(key)
                        && let Type::Record(row) = &alias.body
                    {
                        return Some(
                            row.fields()
                                .into_iter()
                                .map(|(label, ty)| (label, Some(ty.to_string())))
                                .collect(),
                        );
                    }
                }
            }
            None
        },
        | _ => None,
    }
}

/// The source text a span covers, trimmed.
fn source_slice(
    text: &str,
    span: Span,
) -> Option<String> {
    let start = usize::try_from(span.start).ok()?;
    let end = usize::try_from(span.end).ok()?.min(text.len());
    text.get(start..end).map(|slice| slice.trim().to_string())
}

/// The keywords that can start an expression at `offset`.
///
/// The test is deliberately syntactic and conservative: after `=`, `(`, `,`,
/// `|`, `->`, `do`, `then`, `else` or the pipe operator an expression is
/// expected; at the top of a file, a declaration is. Loading the world for a
/// keyword list would cost more than it is worth.
fn expression_keywords(
    text: &str,
    offset: u32,
) -> Vec<&'static str> {
    const KEYWORDS: [&str; 4] = ["do", "let", "match", "with"];
    if !expression_position(text, offset) {
        return Vec::new();
    }
    KEYWORDS.to_vec()
}

fn expression_position(
    text: &str,
    offset: u32,
) -> bool {
    let offset = usize::try_from(offset).unwrap_or(0).min(text.len());
    let mut start = offset;
    while start > 0 {
        let Some(character) = text[..start].chars().next_back() else {
            break;
        };
        if character.is_alphanumeric() || character == '_' {
            start -= character.len_utf8();
        } else {
            break;
        }
    }
    let before = text[..start].trim_end();
    if before.is_empty() {
        return false;
    }
    let previous = before.chars().next_back().unwrap_or(' ');
    if matches!(previous, '=' | '(' | ',' | '|') {
        return true;
    }
    before.ends_with("->")
        || before.ends_with('>')
        || before.ends_with("do")
        || before.ends_with("then")
        || before.ends_with("else")
}
