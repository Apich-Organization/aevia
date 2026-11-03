//! The semantic queries.
//!
//! [`Analysis`] compiles one document once and answers every question about
//! it: diagnostics, hover, completion, definition, outline and formatting. It
//! is the whole public surface a non-editor consumer needs — a playground
//! calls [`Analysis::new`] on the text in the editor and then the same six
//! methods an editor would.
//!
//! Nothing here knows what a client is. There is no request vocabulary, no
//! numeric severity, no JSON: a query takes a byte offset and returns a Rust
//! value from [`crate::types`].

use crate::diagnostic::{self, Diagnostic};
use crate::document::Document;
use crate::occurrence::{self, Occurrence, Role};
use crate::position::Range;
use crate::prelude;
use crate::types::{
    Completion, CompletionKind, Hover, Location, Origin, Symbol, SymbolKind, TextEdit,
};
use aevia_core::compile;
use aevia_syntax::ast::*;
use aevia_syntax::{FileId, Span};
use aevia_types::check::{CheckedModule, Export};
use aevia_types::builtins;
use std::collections::HashMap;

/// One compiled document, ready to answer questions.
pub struct Analysis<'a> {
    document: &'a Document,
    compilation: aevia_core::Compilation,
    /// The file the compiler gave this document. A diagnostic in any other
    /// file is not this document's to report.
    file: FileId,
    occurrences: Vec<Occurrence>,
    /// Where each name declared by the module is written.
    definitions: HashMap<String, Span>,
}

impl<'a> Analysis<'a> {
    /// Compiles `document`.
    ///
    /// The compilation is the checker's, not a second implementation of it:
    /// every type this crate reports is the one `check_module` produced.
    #[must_use]
    pub fn new(document: &'a Document) -> Self {
        let compilation = compile(document.name(), document.text());
        let file = compilation
            .module
            .as_ref()
            .map_or(0, |module| module.file);
        let occurrences = compilation
            .module
            .as_ref()
            .map_or_else(Vec::new, |module| {
                occurrence::collect(module, document.text())
            });
        let definitions = compilation
            .module
            .as_ref()
            .map_or_else(HashMap::new, |module| declarations(module, document.text()));
        Self {
            document,
            compilation,
            file,
            occurrences,
            definitions,
        }
    }

    /// The document being analysed.
    #[must_use]
    pub fn document(&self) -> &Document {
        self.document
    }

    /// The checker's interface, including when there were type errors.
    #[must_use]
    pub fn checked(&self) -> &CheckedModule {
        &self.compilation.checked
    }

    /// Every diagnostic the compiler produced for this document.
    ///
    /// Parse and type diagnostics both, in the compiler's order, with spans in
    /// other files dropped rather than misplaced.
    #[must_use]
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.compilation
            .diagnostics
            .items
            .iter()
            .filter_map(|item| diagnostic::convert(item, self.file, self.document))
            .collect()
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
    pub fn hover(&self, offset: u32) -> Option<Hover> {
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
    pub fn completion(&self, offset: u32) -> Vec<Completion> {
        if usize::try_from(offset).unwrap_or(usize::MAX) > self.document.len() {
            return Vec::new();
        }
        let mut items = Vec::new();
        let mut declared = std::collections::HashSet::new();
        for (name, export) in &self.compilation.checked.exports {
            items.push(Completion {
                label: name.clone(),
                kind: completion_kind(export),
                detail: export_detail(export),
                origin: Origin::Module,
            });
            declared.insert(name.as_str());
        }
        for name in prelude::names() {
            if declared.contains(name) {
                continue;
            }
            let scheme = builtins::schemes(name).and_then(|mut schemes| schemes.drain(..).next());
            items.push(Completion {
                label: name.to_string(),
                kind: match &scheme {
                    Some(scheme) if matches!(scheme.ty, aevia_types::Type::Fn(_, _)) => {
                        CompletionKind::Function
                    }
                    _ => CompletionKind::Value,
                },
                detail: scheme.map(|scheme| scheme.to_string()),
                origin: Origin::Prelude,
            });
        }
        items.sort_by(|left, right| {
            left.origin
                .cmp(&right.origin)
                .then_with(|| left.label.cmp(&right.label))
        });
        items
    }

    /// Where the name under the cursor is declared, when it is declared here.
    ///
    /// A local name resolves through the walk's scope stack, which knows which
    /// parameter or `let` is in view at that offset; a module-level name
    /// resolves through the module's declaration table. `Rssn.<op>` has no
    /// definition to go to: the kernel is not source.
    #[must_use]
    pub fn definition(&self, offset: u32) -> Option<Location> {
        let occurrence = occurrence::at(&self.occurrences, offset)?;
        let name = match &occurrence.role {
            Role::Name(name) | Role::Constructor(name) | Role::TypeName(name) => name,
            Role::Method { name, .. } => name,
            Role::Qualified(_) => return None,
        };
        let span = occurrence
            .binding
            .or_else(|| self.definitions.get(name).copied())?;
        Some(Location {
            document: self.document.name().to_string(),
            range: self.range(span),
        })
    }

    /// The document's outline.
    #[must_use]
    pub fn symbols(&self) -> Vec<Symbol> {
        let Some(module) = &self.compilation.module else {
            return Vec::new();
        };
        let mut symbols = Vec::new();
        for decl in &module.decls {
            if let Some(symbol) = self.declaration_symbol(decl) {
                symbols.push(symbol);
            }
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

    /// The range of a compiler span in this document.
    fn range(&self, span: Span) -> Range {
        self.document.range_of(span.start, span.end)
    }

    /// The source text a span covers, which is how a type is shown in the
    /// outline: the user's own spelling, not a re-rendering of the AST.
    fn text_of(&self, span: Span) -> String {
        let start = usize::try_from(span.start).unwrap_or(usize::MAX);
        let end = usize::try_from(span.end).unwrap_or(usize::MAX);
        self.document
            .text()
            .get(start..end)
            .unwrap_or_default()
            .to_string()
    }

    /// The hover text for an occurrence, or `None` when nothing is known.
    fn describe(&self, occurrence: &Occurrence) -> Option<String> {
        let checked = &self.compilation.checked;
        match &occurrence.role {
            Role::Name(name) => {
                // A class method call the checker resolved: the interesting
                // fact is *which instance*, not the scheme, because the scheme
                // is the class's and the instance is the proof.
                if let Some(call) = occurrence.call {
                    if let Some(use_) = checked
                        .dictionaries
                        .iter()
                        .find(|use_| use_.span == call && &use_.method == name)
                    {
                        return Some(format!(
                            "**class method** `{name}`\n\nclass `{}` · instance for `{}`",
                            use_.class, use_.head
                        ));
                    }
                }
                if let Some(export) = checked.exports.get(name) {
                    let text = describe_export(name, export)?;
                    let polymorphic = export_is_polymorphic(export);
                    return Some(self.with_use_type(text, occurrence.span, polymorphic));
                }
                let mut schemes = builtins::schemes(name)?;
                let scheme = schemes.drain(..).next()?;
                let polymorphic = !scheme.vars.is_empty() || !scheme.dims.is_empty();
                let text = format!("```aevia\n{name}: {scheme}\n```\n\nprelude");
                Some(self.with_use_type(text, occurrence.span, polymorphic))
            }
            Role::Constructor(name) => {
                let export = checked.exports.get(name)?;
                describe_export(name, export)
            }
            Role::TypeName(name) => match checked.exports.get(name)? {
                Export::Type { .. } => Some(format!("type `{name}`")),
                Export::Alias { params } => {
                    Some(format!("type alias `{name}` with {params} parameter(s)"))
                }
                Export::TypeCtor { adt, .. } => Some(format!(
                    "type `{name}`\n\nconstructor `{name}` of `{}`",
                    aevia_types::adt::name_of(*adt)
                )),
                other => describe_export(name, other),
            },
            Role::Method { name, class } => Some(format!(
                "**method** `{name}`\n\nclass `{class}`"
            )),
            Role::Qualified(path) => self.describe_qualified(path, occurrence.span),
        }
    }

    /// The type the checker recorded for the expression or binder at `span`.
    ///
    /// A type that still names a unification variable is the checker saying
    /// "not decided here" — `t7` would look like information — and `Error` is
    /// its "inference already failed". Neither is an answer.
    fn recorded_type(&self, span: Span) -> Option<&aevia_types::Type> {
        let ty = self.compilation.checked.expr_types.get(&span)?;
        if ty.is_error() || aevia_types::class::has_variables(ty) {
            return None;
        }
        Some(ty)
    }

    /// Hover text for an occurrence answered from the expression table.
    ///
    /// A name keeps its name (`x: Int`); anything else is the bare type, which
    /// is what the expression *is*.
    fn describe_expression_type(&self, occurrence: &Occurrence) -> Option<String> {
        let ty = self.recorded_type(occurrence.span)?;
        Some(match &occurrence.role {
            Role::Name(name) => format!("```aevia\n{name}: {ty}\n```"),
            _ => format!("```aevia\n{ty}\n```"),
        })
    }

    /// The innermost expression whose recorded span contains `offset`.
    ///
    /// "Innermost" is the shortest span: in `1.0 + 2` the binary expression
    /// covers the whole text, but a cursor on `1.0` is inside the literal's
    /// own span, and that is the expression the question is about.
    fn expression_at(&self, offset: u32) -> Option<(Span, &aevia_types::Type)> {
        // Whitespace is not an expression. Without this, a cursor on the
        // indent before a block's tail would hover the whole block.
        let text = self.document.text().as_bytes();
        let at = usize::try_from(offset).ok()?;
        if at >= text.len() || text[at].is_ascii_whitespace() {
            return None;
        }
        let mut best: Option<(&Span, &aevia_types::Type)> = None;
        for (span, ty) in &self.compilation.checked.expr_types {
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
    fn with_use_type(&self, text: String, span: Span, polymorphic: bool) -> String {
        if !polymorphic {
            return text;
        }
        match self.recorded_type(span) {
            Some(ty) => format!("{text}\n\nas used here: `{ty}`"),
            None => text,
        }
    }

    /// The hover text for a dotted name.
    ///
    /// `Rssn.<op>` is the only dotted name whose meaning the checker records:
    /// it pins the overload at the call site, and that pin is the whole point
    /// of PLAN D1, so it is what the hover reports. Any other dotted name is a
    /// name from an imported module, whose exports this compilation does not
    /// hold.
    fn describe_qualified(&self, path: &[String], span: Span) -> Option<String> {
        let [root, op] = path else {
            return None;
        };
        if root != "Rssn" {
            return None;
        }
        let call = self
            .compilation
            .checked
            .rssn_calls
            .iter()
            .find(|call| {
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
        if let Some(entry) = entry {
            if !entry.doc.is_empty() {
                text.push_str(&format!("\n\n{}", entry.doc));
            }
        }
        Some(text)
    }

    /// One declaration as an outline entry.
    fn declaration_symbol(&self, decl: &Decl) -> Option<Symbol> {
        let name_range = |name: &str| self.range(occurrence::name_span(self.document.text(), decl.span, name));
        let whole = self.range(decl.span);
        let detail = |name: &str| self.scheme_of(name);
        let symbol = match &decl.kind {
            DeclKind::Import { path, .. } => Symbol {
                name: path.join("."),
                kind: SymbolKind::Namespace,
                range: whole,
                selection: whole,
                detail: None,
                children: Vec::new(),
            },
            DeclKind::Dimension { name, .. } => Symbol {
                name: name.clone(),
                kind: SymbolKind::TypeParameter,
                range: whole,
                selection: name_range(name),
                detail: None,
                children: Vec::new(),
            },
            DeclKind::Unit { name, dim, .. } => Symbol {
                name: name.clone(),
                kind: SymbolKind::Constant,
                range: whole,
                selection: name_range(name),
                detail: Some(format!("unit: {}", dim.to_source())),
                children: Vec::new(),
            },
            DeclKind::Type { name, body, .. } => {
                let (kind, children) = match body {
                    TypeBody::Alias(_) => (SymbolKind::Struct, Vec::new()),
                    TypeBody::Abstract => (SymbolKind::Interface, Vec::new()),
                    TypeBody::Variants(ctors) => (
                        SymbolKind::Enum,
                        ctors
                            .iter()
                            .map(|ctor| Symbol {
                                name: ctor.name.clone(),
                                kind: SymbolKind::EnumMember,
                                range: self.range(ctor.span),
                                selection: self.range(ctor.span),
                                detail: None,
                                children: Vec::new(),
                            })
                            .collect(),
                    ),
                };
                Symbol {
                    name: name.clone(),
                    kind,
                    range: whole,
                    selection: name_range(name),
                    detail: None,
                    children,
                }
            }
            DeclKind::Class {
                name,
                methods,
                ..
            } => Symbol {
                name: name.clone(),
                kind: SymbolKind::Interface,
                range: whole,
                selection: name_range(name),
                detail: None,
                children: methods
                    .iter()
                    .map(|(method, ty)| Symbol {
                        name: method.clone(),
                        kind: SymbolKind::Method,
                        range: whole,
                        selection: self.range(occurrence::name_span(
                            self.document.text(),
                            decl.span,
                            method,
                        )),
                        detail: Some(self.text_of(ty.span)),
                        children: Vec::new(),
                    })
                    .collect(),
            },
            DeclKind::Instance {
                class,
                ty,
                methods,
            } => Symbol {
                name: format!("instance {class} for {}", self.text_of(ty.span)),
                kind: SymbolKind::Object,
                range: whole,
                selection: whole,
                detail: None,
                children: methods
                    .iter()
                    .map(|method| Symbol {
                        name: method.name.clone(),
                        kind: SymbolKind::Method,
                        range: self.range(method.span),
                        selection: self.range(occurrence::name_span(
                            self.document.text(),
                            method.span,
                            &method.name,
                        )),
                        detail: None,
                        children: Vec::new(),
                    })
                    .collect(),
            },
            DeclKind::Derive { .. } => return None,
            DeclKind::Fn { name, body, .. } => Symbol {
                name: name.clone(),
                kind: SymbolKind::Function,
                range: whole,
                selection: name_range(name),
                detail: detail(name),
                children: self.nested(body),
            },
            DeclKind::Let { pattern, .. } => {
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
            }
            DeclKind::Test { name, .. } => Symbol {
                name: name.clone(),
                kind: SymbolKind::Constant,
                range: whole,
                selection: whole,
                detail: None,
                children: Vec::new(),
            },
        };
        Some(symbol)
    }

    /// The declarations nested directly in a function body.
    fn nested(&self, body: &Expr) -> Vec<Symbol> {
        let ExprKind::Block { stmts, .. } = &body.kind else {
            return Vec::new();
        };
        stmts
            .iter()
            .filter_map(|stmt| match stmt {
                BlockStmt::Fn(decl) => self.declaration_symbol(decl),
                BlockStmt::Let { .. } => None,
            })
            .collect()
    }

    /// The checker's rendering of an exported name's scheme, when it has one.
    fn scheme_of(&self, name: &str) -> Option<String> {
        match self.compilation.checked.exports.get(name)? {
            Export::Value { scheme, .. } => scheme_is_known(scheme).then(|| scheme.to_string()),
            Export::Ctor { scheme, .. } | Export::TypeCtor { scheme, .. } => {
                scheme_is_known(scheme).then(|| scheme.to_string())
            }
            Export::Type { .. } | Export::Alias { .. } => None,
            Export::Unit { def } => Some(format!("unit: {}", def.dim.to_plain_string())),
            Export::Dimension { dim } => dim
                .as_ref()
                .map(|dim| format!("dimension: {}", dim.to_plain_string())),
        }
    }
}

/// Where every name the module declares is written.
///
/// An `instance` method is only recorded when exactly one instance declares
/// that name: with two, the source alone cannot say which one a call reaches —
/// the checker's `DictionaryUse` is what knows — and pointing at a coin flip
/// would be worse than pointing at nothing.
fn declarations(module: &Module, text: &str) -> HashMap<String, Span> {
    let mut map: HashMap<String, Span> = HashMap::new();
    let mut methods: HashMap<&str, Vec<Span>> = HashMap::new();
    for decl in &module.decls {
        match &decl.kind {
            DeclKind::Fn { name, .. }
            | DeclKind::Type { name, .. }
            | DeclKind::Class { name, .. }
            | DeclKind::Dimension { name, .. }
            | DeclKind::Unit { name, .. } => {
                add(&mut map, text, name, decl.span);
            }
            DeclKind::Let { pattern, .. } => {
                if let PatKind::Var(name) = &pattern.kind {
                    add(&mut map, text, name, pattern.span);
                }
            }
            DeclKind::Instance { methods: declared, .. } => {
                for method in declared {
                    methods
                        .entry(method.name.as_str())
                        .or_default()
                        .push(occurrence::name_span(text, method.span, &method.name));
                }
            }
            DeclKind::Import { .. } | DeclKind::Derive { .. } | DeclKind::Test { .. } => {}
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
fn add(map: &mut HashMap<String, Span>, text: &str, name: &str, within: Span) {
    map.entry(name.to_string())
        .or_insert_with(|| occurrence::name_span(text, within, name));
}

/// True when an export's scheme has variables the occurrence could instantiate.
fn export_is_polymorphic(export: &Export) -> bool {
    match export {
        Export::Value { scheme, .. }
        | Export::Ctor { scheme, .. }
        | Export::TypeCtor { scheme, .. } => {
            !scheme.vars.is_empty() || !scheme.dims.is_empty()
        }
        Export::Type { .. }
        | Export::Alias { .. }
        | Export::Unit { .. }
        | Export::Dimension { .. } => false,
    }
}

/// The completion kind of an exported name.
fn completion_kind(export: &Export) -> CompletionKind {
    match export {
        Export::Value { arity, .. } if *arity > 0 => CompletionKind::Function,
        Export::Value { .. } => CompletionKind::Value,
        Export::Ctor { .. } | Export::TypeCtor { .. } => CompletionKind::Constructor,
        Export::Type { .. } | Export::Alias { .. } => CompletionKind::Type,
        Export::Unit { .. } => CompletionKind::Unit,
        Export::Dimension { .. } => CompletionKind::Dimension,
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
        Export::Value { scheme, .. } | Export::Ctor { scheme, .. } => {
            scheme_is_known(scheme).then(|| scheme.to_string())
        }
        Export::TypeCtor { scheme, adt, .. } => scheme_is_known(scheme)
            .then(|| format!("{} · {}", aevia_types::adt::name_of(*adt), scheme)),
        Export::Type { adt } => Some(aevia_types::adt::name_of(*adt)),
        Export::Alias { params } => Some(format!("{params} parameter(s)")),
        Export::Unit { def } => Some(format!("unit: {}", def.dim.to_plain_string())),
        Export::Dimension { dim } => dim
            .as_ref()
            .map(|dim| format!("dimension: {}", dim.to_plain_string())),
    }
}

/// The hover text for an exported name, when the checker knows its type.
fn describe_export(name: &str, export: &Export) -> Option<String> {
    Some(match export {
        Export::Value { scheme, arity, .. } => {
            if !scheme_is_known(scheme) {
                return None;
            }
            let mut text = format!("```aevia\n{name}: {scheme}\n```");
            if *arity == 0 {
                text.push_str("\n\nvalue");
            }
            text
        }
        Export::Ctor { adt, scheme, .. } | Export::TypeCtor { adt, scheme, .. } => {
            if !scheme_is_known(scheme) {
                return None;
            }
            format!(
                "```aevia\n{name}: {scheme}\n```\n\nconstructor of `{}`",
                aevia_types::adt::name_of(*adt)
            )
        }
        Export::Type { .. } => format!("type `{name}`"),
        Export::Alias { params } => format!("type alias `{name}` with {params} parameter(s)"),
        Export::Unit { def } => format!(
            "**unit** `{name}`\n\ndimension `{}`",
            def.dim.to_plain_string()
        ),
        Export::Dimension { dim } => match dim {
            Some(dim) => format!(
                "**dimension** `{name}`\n\n= `{}`",
                dim.to_plain_string()
            ),
            None => format!("**base dimension** `{name}`"),
        },
    })
}
