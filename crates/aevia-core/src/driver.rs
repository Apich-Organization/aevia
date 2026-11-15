//! Loading a program: one file plus everything it imports.
//!
//! A module is a file, and its name is its path under the package's `src/`
//! directory: `import Physics.Ising` reads `src/Physics/Ising.aev`. A file with
//! no manifest keeps the older behaviour — the root is the entry file's
//! directory — so a standalone program resolves exactly as it always has.
//! Loading is a post-order walk, so a module is always checked after the
//! modules it imports, and a cycle is reported rather than followed.

use crate::disk_cache::Counters;
use crate::disk_cache::Digest;
use crate::disk_cache::KeyInputs;
use crate::manifest::ResolvedModule;
use crate::manifest::Roots;
use crate::source::SourceDependency;
use crate::source::SourceOverlays;
use aevia_syntax::Diagnostic;
use aevia_syntax::Diagnostics;
use aevia_syntax::FileId;
use aevia_syntax::SourceMap;
use aevia_syntax::Span;
use aevia_syntax::ast::DeclKind;
use aevia_syntax::ast::Module;
use aevia_syntax::lex;
use aevia_syntax::parse_module;
use aevia_types::check::CheckOptions;
use aevia_types::check::CheckedModule;
use aevia_types::check::Import;
use aevia_types::check::InterfaceBinding;
use aevia_types::check::ModuleInterface;
use aevia_types::check::check_module_with_identity;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

/// A module that was loaded and checked.
pub struct LoadedModule {
    /// Its path, empty for the entry file when it declares no name.
    pub path: Vec<String>,
    /// Where it came from.
    pub file: FileId,
    /// Logical source path and the package owner that selected this instance.
    pub source: ResolvedModule,
    /// The parsed module.
    pub module: Module,
    /// Its checked interface.
    pub checked: CheckedModule,
    /// Every other prefix its names are reachable under: an alias, or the
    /// empty string for an `open` or a name list.
    pub aliases: Vec<String>,
    /// Exact resolved import edges, independent of declared-name aliases.
    pub imports: Vec<ResolvedImport>,
}

/// A checked import edge retained by the loader for runtime and editor queries.
pub struct ResolvedImport {
    /// Target source instance, including its package owner.
    pub file: FileId,
    /// Prefix introduced by this import, empty for open/selective imports.
    pub prefix: String,
    /// Selected export names, or every exported name when absent.
    pub names: Option<Vec<String>>,
}

/// A whole program.
pub struct Program {
    /// The sources, for rendering diagnostics.
    pub sources: SourceMap,
    /// The modules, in dependency order (imports first).
    pub modules: Vec<LoadedModule>,
    /// Every diagnostic from every module.
    pub diagnostics: Diagnostics,
    /// Physical source reads and import candidates observed by this load.
    /// Includes absent candidates whose creation can change resolution.
    pub dependencies: Vec<SourceDependency>,
}

impl Program {
    /// The module that defines `main`.
    pub fn entry(&self) -> Option<&LoadedModule> {
        self.modules.last()
    }

    /// The interface of the module called `path`.
    #[must_use]
    pub fn exports(
        &self,
        path: &[String],
    ) -> Option<&CheckedModule> {
        self.modules
            .iter()
            .find(|module| module.path == path)
            .map(|module| &module.checked)
    }
}

/// A package-aware checked revision of an interactive source buffer.
/// The ordinary compilation facade keeps expression inspection APIs reusable.
pub struct ReplCompilation {
    pub compilation: crate::Compilation,
    pub program: Program,
    contexts: HashMap<(u32, u32, String), (Roots, ResolvedModule)>,
    overlays: SourceOverlays,
    roots: Option<Roots>,
}

impl std::ops::Deref for ReplCompilation {
    type Target = crate::Compilation;

    fn deref(&self) -> &Self::Target {
        &self.compilation
    }
}

/// A stable virtual entry with real package, registry and embedded-stdlib imports.
/// Imported source snapshots are committed only after a successful submission;
/// reset constructs a new compiler to observe changed files explicitly.
pub struct ReplCompiler {
    entry: PathBuf,
    identity: crate::source::SourceIdentity,
    sources: SourceMap,
    source_ids: HashMap<ModuleKey, FileId>,
    source_identities: HashMap<ModuleKey, aevia_types::keys::ModulePath>,
    overlays: SourceOverlays,
    contexts: HashMap<(u32, u32, String), (Roots, ResolvedModule)>,
    roots: Option<Roots>,
    resolutions: HashMap<Span, ModuleKey>,
    cache: std::rc::Rc<std::cell::RefCell<CheckCache>>,
    imports: Vec<String>,
    imported_names: Vec<String>,
}

impl ReplCompiler {
    pub fn new(
        directory: &Path,
        identity: crate::source::SourceIdentity,
    ) -> Self {
        Self {
            entry: crate::source::canonical_path(directory).join(".aevia-repl.aev"),
            identity,
            sources: SourceMap::new(),
            source_ids: HashMap::new(),
            source_identities: HashMap::new(),
            overlays: SourceOverlays::new(),
            contexts: HashMap::new(),
            roots: None,
            resolutions: HashMap::new(),
            cache: std::rc::Rc::new(std::cell::RefCell::new(CheckCache::new())),
            imports: Vec::new(),
            imported_names: Vec::new(),
        }
    }

    pub fn imports(&self) -> &[String] {
        &self.imports
    }

    pub fn imported_names(&self) -> &[String] {
        &self.imported_names
    }

    pub fn compile(
        &mut self,
        source: &str,
        origin: Option<&Path>,
    ) -> ReplCompilation {
        let roots = match self
            .roots
            .clone()
            .map(Ok)
            .unwrap_or_else(|| repl_roots(&self.entry))
        {
            | Ok(roots) => roots,
            | Err(error) => return self.failure(source, error),
        };
        let explicit_origin = origin;
        let origin = origin.unwrap_or(&self.entry);
        let origin_roots = match explicit_origin
            .map(repl_roots)
            .unwrap_or_else(|| Ok(roots.clone()))
        {
            | Ok(roots) => roots,
            | Err(error) => return self.failure(source, error),
        };
        let mut contexts = self.contexts.clone();
        let entry_file = self
            .source_ids
            .get(&ModuleKey::physical(&roots.entry_module(&self.entry)))
            .copied()
            .unwrap_or(0);
        let mut entry_import_roots = HashMap::new();
        if let Ok(tokens) = lex(entry_file, source) {
            let (module, _) = parse_module(entry_file, source, &tokens);
            for declaration in &module.decls {
                if matches!(declaration.kind, DeclKind::Import { .. }) {
                    let key = (
                        declaration.span.start,
                        declaration.span.end,
                        source
                            .get(declaration.span.start as usize..declaration.span.end as usize)
                            .unwrap_or("")
                            .to_string(),
                    );
                    let context = contexts
                        .entry(key)
                        .or_insert_with(|| {
                            (origin_roots.clone(), origin_roots.entry_module(origin))
                        })
                        .clone();
                    entry_import_roots.insert(declaration.span, context);
                }
            }
        }
        let mut overlays = self.overlays.clone();
        overlays.insert(&self.entry, source);
        let entry_key = ModuleKey::physical(&roots.entry_module(&self.entry));
        let mut loader = Loader {
            roots: Some(roots.clone()),
            sources: self.sources.clone(),
            source_ids: self.source_ids.clone(),
            source_identities: self.source_identities.clone(),
            entry_identity: Some((entry_key, self.identity.clone())),
            entry_import_roots,
            skip_malformed_checks: true,
            options: CheckOptions::default(),
            overlay: overlays.clone(),
            cache: Some(self.cache.clone()),
            ..Loader::default()
        };
        let root = loader.load_entry(&self.entry);
        self.source_ids = loader.source_ids.clone();
        let (modules, sources, mut diagnostics, dependencies) = loader.finish(root);
        // Text snapshots also retain the selected import instance. A retargeted
        // symlink or a new higher-priority candidate must not silently pair old
        // runtime values with a newly resolved nominal module.
        for (site, target) in repl_resolutions(&modules, &self.source_ids) {
            if self
                .resolutions
                .get(&site)
                .is_some_and(|previous| previous != &target)
            {
                diagnostics.push(Diagnostic::error("E0607", "an imported module resolves to a different source than this session initialized", site)
                    .with_help("use :reset to reload imports from the current package/filesystem"));
            }
        }
        self.sources = sources.clone();
        let compilation = crate::Compilation {
            module: modules.last().map(|module| module.module.clone()),
            checked: modules
                .last()
                .map(|module| module.checked.clone())
                .unwrap_or_default(),
            diagnostics: diagnostics.clone(),
            sources: sources.clone(),
        };
        ReplCompilation {
            compilation,
            program: Program {
                modules,
                sources,
                diagnostics,
                dependencies,
            },
            contexts,
            overlays,
            roots: Some(roots),
        }
    }

    pub fn interrupted(
        &self,
        source: &str,
    ) -> ReplCompilation {
        self.failure_with_code(source, "E0812", "evaluation interrupted".to_string())
    }

    fn failure(
        &self,
        source: &str,
        message: String,
    ) -> ReplCompilation {
        self.failure_with_code(source, "E0605", message)
    }

    fn failure_with_code(
        &self,
        source: &str,
        code: &str,
        message: String,
    ) -> ReplCompilation {
        let mut sources = SourceMap::new();
        let file = sources.add("<repl>", source);
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(Diagnostic::error(code, message, Span::new(file, 0, 0)));
        ReplCompilation {
            compilation: crate::Compilation {
                module: None,
                checked: CheckedModule::default(),
                diagnostics: diagnostics.clone(),
                sources: sources.clone(),
            },
            program: Program {
                modules: Vec::new(),
                sources,
                diagnostics,
                dependencies: Vec::new(),
            },
            contexts: self.contexts.clone(),
            overlays: self.overlays.clone(),
            roots: self.roots.clone(),
        }
    }

    pub fn commit(
        &mut self,
        compilation: &ReplCompilation,
    ) {
        self.contexts = compilation.contexts.clone();
        self.overlays = compilation.overlays.clone();
        self.roots = compilation.roots.clone();
        self.resolutions = repl_resolutions(&compilation.program.modules, &self.source_ids);
        self.imports.clear();
        self.imported_names.clear();
        let entry_file = compilation.program.entry().map(|module| module.file);
        for module in &compilation.program.modules {
            if Some(module.file) == entry_file {
                continue;
            }
            if let Some((key, _)) = self
                .source_ids
                .iter()
                .find(|(_, file)| **file == module.file)
            {
                self.source_identities
                    .insert(key.clone(), module.checked.module_path());
            }
            if let Some(source) = compilation.sources.file(module.file)
                && !source.name.starts_with("stdlib://")
                && !source.name.starts_with("rssn://")
            {
                self.overlays
                    .insert(&module.source.path, source.text.clone());
            }
        }
        if let Some(entry) = compilation.program.entry() {
            for import in &entry.imports {
                let Some(module) = compilation
                    .program
                    .modules
                    .iter()
                    .find(|module| module.file == import.file)
                else {
                    continue;
                };
                let selection = import
                    .names
                    .as_ref()
                    .map(|names| format!(" {{ {} }}", names.join(", ")))
                    .unwrap_or_default();
                self.imports.push(format!(
                    "{}{}{}",
                    module.path.join("."),
                    if import.prefix.is_empty() {
                        " (open)".to_string()
                    } else {
                        format!(" as {}", import.prefix)
                    },
                    selection
                ));
                for name in module.checked.exports.keys() {
                    if import
                        .names
                        .as_ref()
                        .is_none_or(|names| names.contains(name))
                    {
                        self.imported_names.push(if import.prefix.is_empty() {
                            name.clone()
                        } else {
                            format!("{}.{name}", import.prefix)
                        });
                    }
                }
            }
        }
        self.imports.sort();
        self.imports.dedup();
        self.imported_names.sort();
        self.imported_names.dedup();
    }
}

fn repl_resolutions(
    modules: &[LoadedModule],
    ids: &HashMap<ModuleKey, FileId>,
) -> HashMap<Span, ModuleKey> {
    let mut resolutions = HashMap::new();
    for module in modules {
        for (declaration, edge) in module
            .module
            .decls
            .iter()
            .filter(|decl| matches!(decl.kind, DeclKind::Import { .. }))
            .zip(&module.imports)
        {
            if let Some((key, _)) = ids.iter().find(|(_, file)| **file == edge.file) {
                resolutions.insert(declaration.span, key.clone());
            }
        }
    }
    resolutions
}

fn repl_local_names(module: &Module) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let mut patterns = Vec::new();
    for declaration in &module.decls {
        match &declaration.kind {
            | DeclKind::Fn { name, .. } => {
                names.insert(name.clone());
            },
            | DeclKind::Let { pattern, .. } => patterns.push(pattern),
            | _ => {},
        }
    }
    while let Some(pattern) = patterns.pop() {
        use aevia_syntax::PatKind;
        match &pattern.kind {
            | PatKind::Var(name) => {
                names.insert(name.clone());
            },
            | PatKind::Ctor { args, .. } | PatKind::Tuple(args) => patterns.extend(args),
            | PatKind::Record(fields) => {
                for (name, value) in fields {
                    if let Some(value) = value {
                        patterns.push(value);
                    } else {
                        names.insert(name.clone());
                    }
                }
            },
            | PatKind::Tensor { items, rest } => {
                patterns.extend(items);
                if let Some(name) = rest {
                    names.insert(name.clone());
                }
            },
            | PatKind::As { pattern, name } => {
                patterns.push(pattern);
                names.insert(name.clone());
            },
            | PatKind::Annot { pattern, .. } => patterns.push(pattern),
            | _ => {},
        }
    }
    names
}

fn repl_roots(origin: &Path) -> Result<Roots, String> {
    let parent = origin.parent().unwrap_or_else(|| Path::new("."));
    match crate::manifest::Package::find(parent).map_err(|error| error.render())? {
        | Some(package) => {
            package
                .roots()
                .map(|roots| roots.with_local(parent))
                .map_err(|error| error.render())
        },
        | None => Ok(Roots::adjacent(origin)),
    }
}

/// Runtime import instances retained for the lifetime of one REPL interpreter.
#[derive(Default)]
pub struct ReplRuntime {
    scopes: HashMap<FileId, std::rc::Rc<crate::value::Scope>>,
    exports: HashMap<FileId, HashMap<String, crate::Value>>,
}

impl ReplRuntime {
    /// Atomically initialize new imports and the new entry declarations. A
    /// failed or interrupted submission neither consumes dependency initializers
    /// nor leaves a runtime module checked against a different source snapshot.
    pub fn synchronize(
        &mut self,
        interp: &mut crate::Interp,
        compilation: &ReplCompilation,
        loaded: usize,
        limit: usize,
    ) -> Diagnostics {
        if compilation.diagnostics.has_errors() {
            return compilation.diagnostics.clone();
        }
        let checkpoint = (
            interp.globals.clone(),
            interp.symbol_counts.clone(),
            interp.seed,
            interp.constants_seeded,
            interp.instance_methods.clone(),
            interp.method_classes.clone(),
            interp.class_keys.clone(),
            interp.aliases.clone(),
            interp.adts.clone(),
            interp.module_scopes.clone(),
            interp.file_scopes.clone(),
            interp.file_keys.clone(),
            interp.units.clone(),
            interp.dims.clone(),
            interp.context.clone(),
            interp.active_scope.clone(),
            interp.bindings.clone(),
            interp.next_instance_export,
            interp.current_module.clone(),
            interp.current_constructors.clone(),
            interp.instance_exports.clone(),
            interp.instance_sites.clone(),
            interp.module_units.clone(),
            interp.expr_types.clone(),
            interp.literal_reps.clone(),
            interp.constant_regions.clone(),
            interp.rssn_calls.clone(),
            interp.rssn_call_sites.clone(),
            interp.dictionaries.clone(),
            interp.dictionary_sites.clone(),
            interp.dyn_calls.clone(),
            interp.registrations.clone(),
            interp.method_refs.clone(),
            interp.exports.clone(),
        );
        let old_instances = interp.context.instances.borrow().clone();
        let old_scopes = self.scopes.clone();
        let old_exports = self.exports.clone();
        let mut diagnostics = self.prepare_imports(interp, &compilation.program);
        if !diagnostics.has_errors()
            && let Some(entry) = compilation.program.entry()
        {
            let limit = limit.min(entry.module.decls.len());
            let loaded = loaded.min(limit);
            if loaded < limit {
                if let Err((_, error)) = interp.load_incremental_declarations(
                    &entry.checked,
                    &entry.module.decls[..loaded],
                    &entry.module.decls[loaded..limit],
                ) {
                    diagnostics.push(error.diagnostic());
                }
            } else {
                interp.prepare_incremental_module(&entry.checked, &entry.module.decls[..loaded]);
            }
        }
        if !diagnostics.has_errors()
            && let Err(error) = interp.check_cancelled()
        {
            diagnostics.push(error.diagnostic());
        }
        if diagnostics.has_errors() {
            (
                interp.globals,
                interp.symbol_counts,
                interp.seed,
                interp.constants_seeded,
                interp.instance_methods,
                interp.method_classes,
                interp.class_keys,
                interp.aliases,
                interp.adts,
                interp.module_scopes,
                interp.file_scopes,
                interp.file_keys,
                interp.units,
                interp.dims,
                interp.context,
                interp.active_scope,
                interp.bindings,
                interp.next_instance_export,
                interp.current_module,
                interp.current_constructors,
                interp.instance_exports,
                interp.instance_sites,
                interp.module_units,
                interp.expr_types,
                interp.literal_reps,
                interp.constant_regions,
                interp.rssn_calls,
                interp.rssn_call_sites,
                interp.dictionaries,
                interp.dictionary_sites,
                interp.dyn_calls,
                interp.registrations,
                interp.method_refs,
                interp.exports,
            ) = checkpoint;
            *interp.context.instances.borrow_mut() = old_instances;
            self.scopes = old_scopes;
            self.exports = old_exports;
        }
        diagnostics
    }

    /// Initialize dependencies once and publish only the entry's explicit imports.
    pub fn prepare_imports(
        &mut self,
        interp: &mut crate::Interp,
        program: &Program,
    ) -> Diagnostics {
        let mut diagnostics = Diagnostics::new();
        let Some(entry) = program.entry() else {
            return diagnostics;
        };
        for module in &program.modules {
            if module.file == entry.file {
                break;
            }
            if let Err(error) = interp.check_cancelled() {
                diagnostics.push(error.diagnostic());
                return diagnostics;
            }
            if self.scopes.contains_key(&module.file) {
                continue;
            }
            interp.prepare_module(&module.checked);
            let imported = module
                .imports
                .iter()
                .filter_map(|import| self.scopes.get(&import.file).cloned())
                .collect();
            let scope = interp.module_scope(imported);
            self.bind_imports(module, &scope);
            diagnostics.extend(interp.load_into(&module.module, &scope));
            if diagnostics.has_errors() {
                return diagnostics;
            }
            let values = module
                .checked
                .exports
                .keys()
                .filter_map(|name| scope.lookup(name).map(|value| (name.clone(), value)))
                .collect();
            self.exports.insert(module.file, values);
            self.scopes.insert(module.file, scope.clone());
            interp.file_scopes.insert(module.file, scope);
            interp.file_keys.insert(
                module.file,
                std::sync::Arc::from(format!(
                    "{}|{:?}|{:?}",
                    module.path.join("."),
                    module.source.path,
                    module.source.owner
                )),
            );
        }
        if let Err(error) = interp.check_cancelled() {
            diagnostics.push(error.diagnostic());
            return diagnostics;
        }
        let locals = repl_local_names(&entry.module);
        for import in &entry.imports {
            if let Some(values) = self.exports.get(&import.file) {
                for (name, value) in values {
                    if import
                        .names
                        .as_ref()
                        .is_none_or(|names| names.contains(name))
                    {
                        let key = if import.prefix.is_empty() {
                            name.clone()
                        } else {
                            format!("{}.{name}", import.prefix)
                        };
                        if !(locals.contains(&key) && interp.globals.contains_key(&key)) {
                            interp.globals.insert(key, value.clone());
                        }
                    }
                }
            }
        }
        diagnostics
    }

    fn bind_imports(
        &self,
        module: &LoadedModule,
        scope: &std::rc::Rc<crate::value::Scope>,
    ) {
        for import in &module.imports {
            if let Some(values) = self.exports.get(&import.file) {
                for (name, value) in values {
                    if import
                        .names
                        .as_ref()
                        .is_none_or(|names| names.contains(name))
                    {
                        let key = if import.prefix.is_empty() {
                            name.clone()
                        } else {
                            format!("{}.{name}", import.prefix)
                        };
                        scope.vars.borrow_mut().insert(key, value.clone());
                    }
                }
            }
        }
    }
}

/// What a single-pass load of many entry files did.
///
/// `checks` is the number of modules the checker ran on: a package-wide
/// `check --all` loads every entry through one loader, so a shared dependency is
/// parsed and checked **once**, not once per entry. A quadratic implementation
/// shows up here as `checks` growing with entries × modules.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadStats {
    /// How many entry files were asked for.
    pub entries: usize,
    /// How many distinct source/owner pairs were parsed (entries plus imports).
    pub files: usize,
    /// How many modules were checked.
    pub modules: usize,
    /// The number of check invocations (`== modules` for a single pass).
    pub checks: usize,
    /// How many diagnostics the pass produced, after deduplication.
    pub diagnostics: usize,
    /// How many modules were restored from `.aevi` files instead of checked.
    pub disk_hits: usize,
    /// How many `.aevi` files this load wrote.
    pub disk_writes: usize,
}

/// Loads and checks several entry files in **one** pass.
///
/// A package-wide `check --all` has N entry files that often import the same
/// modules; loading each entry separately parses and checks the shared modules N
/// times. This shares one [`SourceMap`] and one [`Loader`], so each file is
/// parsed once and each module checked once in post-order, and the caller can
/// assert the work is linear with [`LoadStats::checks`].
///
/// `Program::entry` is the *last* module in dependency order; for a multi-entry
/// load the caller iterates `modules` instead.
#[must_use]
pub fn load_all_with_roots(
    entries: &[PathBuf],
    roots: Roots,
    options: &CheckOptions,
) -> (Program, LoadStats) {
    let mut roots = roots;
    for entry in entries {
        if let Some(dir) = entry.parent() {
            roots = roots.with_local(dir);
        }
    }
    let mut loader = Loader {
        roots: Some(roots),
        options: options.clone(),
        ..Loader::default()
    };
    let mut root = None;
    for entry in entries {
        let loaded = loader.load_entry(entry);
        if root.is_none() {
            root = loaded;
        }
    }
    let files = loader.parsed.len();
    let (modules, sources, mut diagnostics, dependencies) = loader.finish(root);
    dedupe_diagnostics(&mut diagnostics);
    let stats = LoadStats {
        entries: entries.len(),
        files,
        modules: modules.len(),
        checks: modules.len(),
        diagnostics: diagnostics.items.len(),
        disk_hits: 0,
        disk_writes: 0,
    };
    (
        Program {
            sources,
            modules,
            diagnostics,
            dependencies,
        },
        stats,
    )
}

/// Removes diagnostics two entries produced for the same shared module.
fn dedupe_diagnostics(diagnostics: &mut Diagnostics) {
    let mut seen = std::collections::HashSet::new();
    diagnostics.items.retain(|item| {
        seen.insert((
            item.code.clone(),
            item.message.clone(),
            item.span.start,
            item.span.end,
        ))
    });
}

/// Loads and checks `entry` and its imports.
///
/// A missing or unparsable import is reported and skipped: one broken file
/// should not hide the diagnostics of the rest.
#[must_use]
pub fn load(entry: &Path) -> Program {
    load_with_roots(entry, Roots::adjacent(entry))
}

/// Loads and checks `entry`, resolving imports against `roots`.
///
/// `roots` is what a package adds: `import A.B` is `<roots.source>/A/B.aev`,
/// and `import Dep.A.B` is the dependency's `src/A/B.aev`. Everything else is
/// unchanged, so the caller that passes [`Roots::adjacent`] gets exactly the
/// behaviour of [`load`].
#[must_use]
pub fn load_with_roots(
    entry: &Path,
    roots: Roots,
) -> Program {
    // The entry's own directory is tried after the package's `src/`, so a file
    // in `tests/` or `examples/` reaches both the package's modules and its own
    // neighbours.
    let roots = match entry.parent() {
        | Some(dir) => roots.with_local(dir),
        | None => roots,
    };
    let mut loader = Loader {
        roots: Some(roots),
        ..Loader::default()
    };
    let root = loader.load_entry(entry);
    let (modules, sources, diagnostics, dependencies) = loader.finish(root);
    Program {
        sources,
        modules,
        diagnostics,
        dependencies,
    }
}

/// Loads `entry` from `source` — a dirty editor buffer — while its imports
/// resolve from disk and `roots` exactly as [`load_with_roots`] does.
///
/// The `SourceMap` entry carries `source`, so diagnostics point at the text the
/// editor holds; an unsaved file that imports other modules keeps its imports.
#[must_use]
pub fn load_with_roots_and_source(
    entry: &Path,
    roots: Roots,
    source: &str,
    options: &CheckOptions,
) -> Program {
    let mut overlays = SourceOverlays::new();
    overlays.insert(entry, source);
    load_with_roots_and_overlays(entry, roots, &overlays, options)
}

/// Loads an entry and all its imports against one snapshot of unsaved buffers.
/// Physical buffers can be reached through aliases and multiple package owners;
/// source identity is shared while nominal ownership/import context is retained.
/// An overlaid import need not exist on disk yet.
#[must_use]
pub fn load_with_roots_and_overlays(
    entry: &Path,
    roots: Roots,
    overlays: &SourceOverlays,
    options: &CheckOptions,
) -> Program {
    let roots = match entry.parent() {
        | Some(dir) => roots.with_local(dir),
        | None => roots,
    };
    let mut loader = Loader {
        roots: Some(roots),
        options: options.clone(),
        overlay: overlays.clone(),
        ..Loader::default()
    };
    let root = loader.load_entry(entry);
    let (modules, sources, diagnostics, dependencies) = loader.finish(root);
    Program {
        sources,
        modules,
        diagnostics,
        dependencies,
    }
}

/// [`load_with_roots_and_source`] with the adjacent roots and default options.
#[must_use]
pub fn load_with_source(
    entry: &Path,
    source: &str,
) -> Program {
    load_with_roots_and_source(
        entry,
        Roots::adjacent(entry),
        source,
        &CheckOptions::default(),
    )
}

#[derive(Default)]
struct Loader {
    /// Where module paths are resolved: the package's `src/` plus its
    /// dependencies, or the entry file's directory for a standalone file.
    roots: Option<Roots>,
    sources: SourceMap,
    /// Interactive hosts reserve source ids across changes to the import graph.
    source_ids: HashMap<ModuleKey, FileId>,
    source_identities: HashMap<ModuleKey, aevia_types::keys::ModulePath>,
    /// A virtual entry keeps its host identity independently of its disk anchor.
    entry_identity: Option<(ModuleKey, crate::source::SourceIdentity)>,
    /// A loaded REPL fragment can resolve imports from another package/directory.
    entry_import_roots: HashMap<Span, (Roots, ResolvedModule)>,
    context_roots: HashMap<ModuleKey, Roots>,
    /// Interactive diagnostics stop at syntax errors; editor loads retain
    /// their historical recovery-AST analysis by leaving this disabled.
    skip_malformed_checks: bool,
    diagnostics: Diagnostics,
    /// Parsed modules by owning package and canonical source, in post-order.
    order: Vec<ModuleKey>,
    parsed: HashMap<ModuleKey, Parsed>,
    /// The import stack, for cycle detection.
    stack: Vec<ModuleKey>,
    /// The capabilities the checker gates static gaps on.
    options: CheckOptions,
    /// Unsaved source buffers by canonical physical file. All owning instances
    /// see the same buffer contents while retaining their own import context.
    overlay: SourceOverlays,
    dependencies: BTreeMap<PathBuf, SourceDependency>,
    /// One source text per physical file, shared by all owning instantiations.
    physical_sources: HashMap<PathBuf, String>,
    /// Standard-library imports: synthetic path key → module components.
    ///
    /// The embedded stdlib (§8.20) is not on disk, so an `import Math` that no
    /// package root resolves is keyed as `stdlib/Math.aev` and its source comes
    /// from [`aevia_std::stdlib::source`]; post-order, cycle detection and
    /// `run_modules` then treat it exactly like a file module.
    stdlib: HashMap<PathBuf, Vec<String>>,
    /// RSSN namespaces generated lazily from the live kernel manifest.
    kernel_modules: HashMap<PathBuf, (String, String)>,
    /// Checked modules kept from earlier loads (§9.11); see [`CheckCache`].
    cache: Option<std::rc::Rc<std::cell::RefCell<CheckCache>>>,
    /// The per-module on-disk cache (`.aevi`, §9.11), when configured.
    disk: Option<crate::disk_cache::DiskCache>,
    /// What this load did, readable by the entry point after `finish`.
    counters: std::sync::Arc<Counters>,
}

/// Incremental checking (§9.11): checked modules kept between loads.
///
/// Checking a module is a function of its source text, its `FileId`, the
/// checked interfaces of its imports, the bindings they are imported under,
/// its identity and the check options. A module whose inputs all hash the
/// same as on the previous load is not checked again — its `CheckedModule`
/// and diagnostics are reused — and because the fingerprint includes the
/// imports' *checked interfaces*, an edit that leaves a dependency's interface
/// unchanged does not recheck the modules that import it (early cutoff). The
/// standard library and unchanged package modules are therefore checked once
/// per session, and an edit to the entry file rechecks only the entry.
///
/// `FileId`s are positions in the load's `SourceMap`, so they are the same
/// on every load of a program whose import graph has not changed; an entry
/// is reused only under the same `FileId`, which keeps every span in the
/// reused interface pointing into the text it was checked against.
///
/// The fingerprint is a 64-bit hash of the inputs' structural rendering;
/// the cache is per process (a `watch` session, an editor, a REPL).
#[derive(Default)]
pub struct CheckCache {
    entries: HashMap<String, CacheEntry>,
    /// Modules reused from the cache on the most recent load.
    pub reused: usize,
    /// Modules checked on the most recent load.
    pub checked: usize,
}

struct CacheEntry {
    file: FileId,
    fingerprint: u64,
    diagnostics: Diagnostics,
    module: aevia_types::check::CheckedModule,
}

impl CheckCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many modules the cache holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Loads `entry` reusing the modules `cache` kept from earlier loads.
///
/// The result is exactly the program [`load_with_roots_and_overlays`] would
/// produce; `LoadStats::checks` counts only the modules checked this time and
/// `cache.reused` the ones that were not.
#[must_use]
pub fn load_incremental(
    entry: &Path,
    roots: Roots,
    overlays: &SourceOverlays,
    options: &CheckOptions,
    cache: &std::rc::Rc<std::cell::RefCell<CheckCache>>,
) -> (Program, LoadStats) {
    load_configured(entry, roots, overlays, options, Some(cache), None)
}

/// Loads `entry` through the per-module on-disk cache (`.aevi`, spec §9.11).
///
/// A module whose key — source text, `FileId`, identity, import bindings, the
/// interface digests of its dependencies, the capability set and the compiler
/// identity — matches a file in `disk` is restored from it instead of being
/// checked; a module that is checked is written back. The program is exactly
/// the one [`load_with_roots_and_overlays`] builds; `LoadStats::checks` counts
/// only the modules the checker ran on, `disk_hits` and `disk_writes` the
/// cache traffic.
#[must_use]
pub fn load_with_disk_cache(
    entry: &Path,
    roots: Roots,
    overlays: &SourceOverlays,
    options: &CheckOptions,
    disk: &crate::disk_cache::DiskCache,
) -> (Program, LoadStats) {
    load_configured(entry, roots, overlays, options, None, Some(disk))
}

/// [`load_incremental`] that also consults and fills an on-disk cache: a
/// `watch` session keeps its in-memory [`CheckCache`] and starts warm from the
/// previous process's `.aevi` files.
#[must_use]
pub fn load_incremental_with_disk_cache(
    entry: &Path,
    roots: Roots,
    overlays: &SourceOverlays,
    options: &CheckOptions,
    cache: &std::rc::Rc<std::cell::RefCell<CheckCache>>,
    disk: &crate::disk_cache::DiskCache,
) -> (Program, LoadStats) {
    load_configured(entry, roots, overlays, options, Some(cache), Some(disk))
}

fn load_configured(
    entry: &Path,
    roots: Roots,
    overlays: &SourceOverlays,
    options: &CheckOptions,
    cache: Option<&std::rc::Rc<std::cell::RefCell<CheckCache>>>,
    disk: Option<&crate::disk_cache::DiskCache>,
) -> (Program, LoadStats) {
    let roots = match entry.parent() {
        | Some(dir) => roots.with_local(dir),
        | None => roots,
    };
    if let Some(cache) = cache {
        let mut cache = cache.borrow_mut();
        cache.reused = 0;
        cache.checked = 0;
    }
    let mut loader = Loader {
        roots: Some(roots),
        options: options.clone(),
        overlay: overlays.clone(),
        cache: cache.cloned(),
        disk: disk.cloned(),
        ..Loader::default()
    };
    let counters = loader.counters.clone();
    let root = loader.load_entry(entry);
    let files = loader.parsed.len();
    let (modules, sources, mut diagnostics, dependencies) = loader.finish(root);
    dedupe_diagnostics(&mut diagnostics);
    let stats = LoadStats {
        entries: 1,
        files,
        modules: modules.len(),
        checks: Counters::get(&counters.checked),
        diagnostics: diagnostics.items.len(),
        disk_hits: Counters::get(&counters.disk_hits),
        disk_writes: Counters::get(&counters.disk_writes),
    };
    (
        Program {
            sources,
            modules,
            diagnostics,
            dependencies,
        },
        stats,
    )
}

fn fingerprint(parts: &[&dyn std::fmt::Debug]) -> u64 {
    use std::hash::Hash as _;
    use std::hash::Hasher as _;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for part in parts {
        format!("{part:?}").hash(&mut hasher);
    }
    hasher.finish()
}

struct Parsed {
    context: ResolvedModule,
    file: FileId,
    module: Module,
    syntax_failed: bool,
    path: Vec<String>,
    imports: Vec<(
        ModuleKey,
        Vec<String>,
        Option<String>,
        bool,
        Option<Vec<String>>,
    )>,
}

// A physical file can be instantiated by distinct owning packages. Dependency
// aliases of the same owner share this key; a package boundary never vanishes
// merely because src/ or one module is a symlink to shared external sources.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ModuleKey {
    path: PathBuf,
    owner: Option<PathBuf>,
}
impl ModuleKey {
    fn physical(context: &ResolvedModule) -> Self {
        Self {
            path: crate::source::canonical_path(&context.path),
            owner: context.owner.clone(),
        }
    }

    fn label(&self) -> String {
        match &self.owner {
            | Some(owner) => format!("{} [package {}]", self.path.display(), owner.display()),
            | None => self.path.display().to_string(),
        }
    }
}
impl std::ops::Deref for ModuleKey {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}
impl AsRef<Path> for ModuleKey {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Loader {
    fn roots_for(
        &self,
        key: &ModuleKey,
    ) -> Option<&Roots> {
        self.context_roots.get(key).or(self.roots.as_ref())
    }

    fn watch(
        &mut self,
        path: &Path,
    ) -> bool {
        self.dependencies
            .entry(path.to_path_buf())
            .or_insert_with(|| SourceDependency::observe(path, &self.overlay))
            .available()
    }

    fn entry_context(
        &self,
        path: &Path,
    ) -> ResolvedModule {
        self.roots.as_ref().map_or_else(
            || {
                ResolvedModule {
                    path: path.to_path_buf(),
                    owner: None,
                }
            },
            |roots| roots.entry_module(path),
        )
    }

    fn load_entry(
        &mut self,
        path: &Path,
    ) -> Option<ModuleKey> {
        self.watch(path);
        self.load_file(&self.entry_context(path), &[])
    }

    fn load_file(
        &mut self,
        context: &ResolvedModule,
        expected: &[String],
    ) -> Option<ModuleKey> {
        let canonical = if self.stdlib.contains_key(&context.path)
            || self.kernel_modules.contains_key(&context.path)
        {
            ModuleKey {
                path: context.path.clone(),
                owner: None,
            }
        } else {
            ModuleKey::physical(context)
        };
        if self.roots.is_none() {
            self.roots = Some(Roots::adjacent(&canonical));
        }
        if let Some(parsed) = self.parsed.get(&canonical) {
            if !expected.is_empty()
                && parsed.module.name.is_some()
                && self.roots_for(&canonical).is_some_and(|roots| {
                    !roots.resolved_name_matches(context, &parsed.path, expected)
                })
            {
                self.diagnostics.push(Diagnostic::error(
                    "E0601",
                    format!(
                        "module `{}` declares name `{}` but is imported as `{}`",
                        canonical.display(),
                        parsed.path.join("."),
                        expected.join(".")
                    ),
                    Span::default(),
                ));
            }
            return Some(canonical);
        }
        if self.stack.contains(&canonical) {
            let mut cycle: Vec<String> = self.stack.iter().map(ModuleKey::label).collect();
            cycle.push(canonical.label());
            self.diagnostics.push(
                Diagnostic::error(
                    "E0602",
                    format!("import cycle: {}", cycle.join(" -> ")),
                    Span::default(),
                )
                .with_note("a module cannot depend on itself, directly or through another"),
            );
            return None;
        }
        let kernel = self.kernel_modules.get(&canonical.path).cloned();
        let embedded = self
            .stdlib
            .get(&canonical.path)
            .cloned()
            .and_then(|components| {
                let path: Vec<&str> = components.iter().map(String::as_str).collect();
                aevia_std::stdlib::source(&path).map(|source| (components, source))
            });
        // One immutable overlay snapshot applies to entry and imported sources.
        let physical = embedded.is_none() && kernel.is_none();
        let overlaid = physical
            .then(|| self.overlay.get(&canonical.path))
            .flatten()
            .map(str::to_owned);
        if physical {
            self.watch(&context.path);
        }
        let source = match (&overlaid, &embedded) {
            | (Some(source), _) => source.clone(),
            | (None, Some((_, source))) => source.to_string(),
            | (None, None) if kernel.is_some() => kernel.as_ref().unwrap().1.clone(),
            | (None, None) => {
                if let Some(source) = self.physical_sources.get(&canonical.path) {
                    source.clone()
                } else {
                    match std::fs::read_to_string(&canonical) {
                        | Ok(source) => {
                            self.physical_sources
                                .insert(canonical.path.clone(), source.clone());
                            source
                        },
                        | Err(error) => {
                            if let Some(input) = self.dependencies.get_mut(&context.path) {
                                input.failed(&error);
                            }
                            self.diagnostics.push(Diagnostic::error(
                                "E0607",
                                format!("cannot read `{}`: {error}", canonical.display()),
                                Span::default(),
                            ));
                            return None;
                        },
                    }
                }
            },
        };
        if physical && let Some(input) = self.dependencies.get_mut(&context.path) {
            input.loaded(&source);
        }
        let display = match (&overlaid, &embedded) {
            | (Some(_), _) => canonical.display().to_string(),
            | (None, Some((components, _))) => format!("stdlib://{}.aev", components.join("/")),
            | (None, None) => {
                kernel
                    .as_ref()
                    .map(|(name, _)| format!("rssn://{name}.aev"))
                    .unwrap_or_else(|| canonical.display().to_string())
            },
        };
        let display = if self
            .entry_identity
            .as_ref()
            .is_some_and(|(entry, _)| entry == &canonical)
        {
            "<repl>".to_string()
        } else {
            display
        };
        let file = if let Some(file) = self.source_ids.get(&canonical).copied() {
            self.sources.replace(file, display, &source);
            file
        } else {
            let file = self.sources.add(display, &source);
            self.source_ids.insert(canonical.clone(), file);
            file
        };
        let lexed = match lex(file, &source) {
            | Ok(lexed) => lexed,
            | Err(errors) => {
                self.diagnostics.extend(errors);
                // Remember the failure so a second importer does not report it
                // again.
                self.parsed.insert(
                    canonical.clone(),
                    Parsed {
                        context: context.clone(),
                        file,
                        module: Module {
                            name: None,
                            unit_system: None,
                            decls: Vec::new(),
                            file,
                        },
                        syntax_failed: true,
                        path: Vec::new(),
                        imports: Vec::new(),
                    },
                );
                self.order.push(canonical.clone());
                return None;
            },
        };
        let (module, parse_diagnostics) = parse_module(file, &source, &lexed);
        let syntax_failed = parse_diagnostics.has_errors();
        self.diagnostics.extend(parse_diagnostics);
        if self.diagnostics.has_errors() && self.stack.is_empty() {
            // The entry file failed to parse; there is nothing to import.
        }

        let path = if let Some((entry, identity)) = &self.entry_identity
            && entry == &canonical
        {
            identity.module_path(module.name.as_deref()).0
        } else {
            module_path(&module, &canonical)
        };
        // §2.2.5 / appendix B: a module's declared name must match the path it
        // is imported by, or `import A.B` would name a module called `C`. The
        // entry file is exempt because it is not imported by a path.
        let declared_matches = self
            .roots_for(&canonical)
            .map_or(path == expected, |roots| {
                roots.resolved_name_matches(context, &path, expected)
            });
        if !expected.is_empty() && module.name.is_some() && !declared_matches {
            self.diagnostics.push(
                Diagnostic::error(
                    "E0601",
                    format!(
                        "module `{}` declares name `{}` but is imported as `{}`",
                        canonical.display(),
                        path.join("."),
                        expected.join(".")
                    ),
                    Span::default(),
                )
                .with_note("the declared `module` name and the import path must agree"),
            );
        }
        self.stack.push(canonical.clone());

        let mut imports = Vec::new();
        // A source module inherits the owner that selected it. `import A.B`
        // uses that owner's src tree; a dependency import switches to the
        // dependency owner before its source is canonicalized.
        for decl in &module.decls {
            let DeclKind::Import {
                path: import_path,
                alias,
                names,
                open,
            } = &decl.kind
            else {
                continue;
            };
            let (import_roots, importer) = self
                .entry_import_roots
                .get(&decl.span)
                .cloned()
                .or_else(|| {
                    self.context_roots
                        .get(&canonical)
                        .cloned()
                        .map(|roots| (roots, context.clone()))
                })
                .unwrap_or_else(|| {
                    (
                        self.roots.as_ref().expect("loader roots").clone(),
                        context.clone(),
                    )
                });
            let candidates = import_roots.module_candidates(&importer, import_path);
            let resolved = candidates
                .into_iter()
                .find(|candidate| self.watch(&candidate.path));
            let target = match resolved {
                | Some(target) => target,
                | None => {
                    // §8.20: the standard library ships embedded, not on disk.
                    // A module the package roots do not resolve but the stdlib
                    // names is loaded from the embedded source under a stable
                    // synthetic key, so post-order and cycle detection work the
                    // same way as for a file.
                    let components: Vec<&str> = import_path.iter().map(String::as_str).collect();
                    if aevia_std::stdlib::is_module_name(&components) {
                        let mut key = PathBuf::from("stdlib");
                        for component in import_path {
                            key.push(component);
                        }
                        key.set_extension("aev");
                        self.stdlib.insert(key.clone(), import_path.clone());
                        ResolvedModule {
                            path: key,
                            owner: None,
                        }
                    } else if import_path.len() == 1 && import_path[0].starts_with("Rssn_") {
                        let name = &import_path[0];
                        let Some(source) =
                            aevia_rssn::codegen::namespace_source(aevia_rssn::registry(), name)
                        else {
                            self.diagnostics.push(Diagnostic::error(
                                "E0607",
                                format!("cannot find kernel namespace `{name}`"),
                                decl.span,
                            ));
                            continue;
                        };
                        let key = PathBuf::from("rssn").join(format!("{name}.aev"));
                        self.kernel_modules
                            .entry(key.clone())
                            .or_insert_with(|| (name.clone(), source));
                        ResolvedModule {
                            path: key,
                            owner: None,
                        }
                    } else {
                        let candidates = import_roots.module_candidates(&importer, import_path);
                        let expected = candidates
                            .iter()
                            .map(|candidate| format!("`{}`", candidate.path.display()))
                            .collect::<Vec<_>>()
                            .join(" or ");
                        let wanted = import_path.join(".");
                        let dotted: Vec<String> = aevia_std::stdlib::module_paths()
                            .map(|module| module.replace('/', "."))
                            .collect();
                        // Module names are case-sensitive (`FFT`, not `Fft`),
                        // so a case-insensitive match is the first suggestion.
                        let close = dotted
                            .iter()
                            .find(|module| module.eq_ignore_ascii_case(&wanted))
                            .map(String::as_str)
                            .or_else(|| {
                                aevia_types::check::closest_name(
                                    &wanted,
                                    dotted.iter().map(String::as_str),
                                )
                            });
                        let mut diagnostic = Diagnostic::error(
                            "E0607",
                            format!("cannot find module `{wanted}`: expected {expected}"),
                            decl.span,
                        )
                        .with_note(format!("imported from `{}`", module_label(&path)));
                        if let Some(close) = close {
                            diagnostic = diagnostic
                                .with_help(format!("the standard library has a module `{close}`"));
                        }
                        self.diagnostics.push(diagnostic);
                        continue;
                    }
                },
            };
            let target_key = if self.stdlib.contains_key(&target.path)
                || self.kernel_modules.contains_key(&target.path)
            {
                ModuleKey {
                    path: target.path.clone(),
                    owner: None,
                }
            } else {
                ModuleKey::physical(&target)
            };
            self.context_roots
                .entry(target_key.clone())
                .or_insert(import_roots);
            let resolved = self.load_file(&target, import_path).or_else(|| {
                // Keep a failed lexical dependency edge so only its importers
                // are stage-blocked; independent modules remain checkable.
                (self.skip_malformed_checks
                    && self
                        .parsed
                        .get(&target_key)
                        .is_some_and(|parsed| parsed.syntax_failed))
                .then_some(target_key)
            });
            if let Some(resolved) = resolved {
                imports.push((
                    resolved,
                    import_path.clone(),
                    alias.clone(),
                    *open,
                    names.clone(),
                ));
            }
        }

        self.stack.pop();
        self.parsed.insert(
            canonical.clone(),
            Parsed {
                context: context.clone(),
                file,
                module,
                syntax_failed,
                path,
                imports,
            },
        );
        self.order.push(canonical.clone());
        Some(canonical)
    }

    fn finish(
        mut self,
        root: Option<ModuleKey>,
    ) -> (
        Vec<LoadedModule>,
        SourceMap,
        Diagnostics,
        Vec<SourceDependency>,
    ) {
        let dependencies = std::mem::take(&mut self.dependencies)
            .into_values()
            .collect();
        let sources = self.sources.clone();
        let Some(root) = root else {
            return (Vec::new(), sources, self.diagnostics, dependencies);
        };
        // Every prefix each module is imported under, across all importers.
        let mut prefixes: HashMap<ModuleKey, Vec<String>> = HashMap::new();
        for parsed in self.parsed.values() {
            for (target, import_path, alias, open, names) in &parsed.imports {
                let prefix = if *open || names.is_some() {
                    String::new()
                } else {
                    alias.clone().unwrap_or_else(|| import_path.join("."))
                };
                let entry = prefixes.entry(target.clone()).or_default();
                if !entry.contains(&prefix) {
                    entry.push(prefix);
                }
            }
        }
        let mut modules: Vec<LoadedModule> = Vec::new();
        let mut syntax_blocked = std::collections::HashSet::new();
        // The interface digest of each module already resolved (`None` when it
        // could not be digested), for the on-disk cache keys.
        let mut digests: HashMap<FileId, Option<Digest>> = HashMap::new();
        // The checked interfaces of the modules already resolved, in dependency
        // order (§7.4). They carry classes, instances and alias definitions, so
        // an importer can use a class it did not declare and expand an imported
        // alias instead of treating it as opaque.
        for path in self.order.clone() {
            let Some(parsed) = self.parsed.remove(&path) else {
                continue;
            };
            if self.skip_malformed_checks
                && (parsed.syntax_failed
                    || parsed
                        .imports
                        .iter()
                        .any(|(target, ..)| syntax_blocked.contains(target)))
            {
                syntax_blocked.insert(path);
                let checked = CheckedModule {
                    name: parsed.module.name.clone(),
                    anonymous_file: parsed.file,
                    errors: 1,
                    ..CheckedModule::default()
                };
                // Retain the recovered AST and its slot in dependency order,
                // but do not cache or type-check a syntax-recovery program.
                modules.push(LoadedModule {
                    path: parsed.path,
                    file: parsed.file,
                    source: parsed.context,
                    module: parsed.module,
                    checked,
                    aliases: Vec::new(),
                    imports: Vec::new(),
                });
                continue;
            }
            // The imports of this module, as the checker wants them: one entry
            // per resolved module, with the prefix its names are reachable
            // under.
            // Materialised only when the checker (or the in-memory fingerprint)
            // needs them: a module restored from disk never clones its
            // dependencies' export tables.
            let mut import_specs: Vec<(String, usize, Option<Vec<String>>)> = Vec::new();
            let mut runtime_imports = Vec::new();
            let mut bindings = Vec::new();
            for (target, import_path, alias, open, names) in &parsed.imports {
                let Some((other_index, other)) = self
                    .order
                    .iter()
                    .position(|candidate| candidate == target)
                    .and_then(|index| modules.get(index).map(|module| (index, module)))
                else {
                    continue;
                };
                // `open` and a name list both bring names in unqualified;
                // an alias or the module path qualifies them.
                let prefix = if *open || names.is_some() {
                    String::new()
                } else {
                    alias.clone().unwrap_or_else(|| import_path.join("."))
                };
                let span = parsed
                    .module
                    .decls
                    .iter()
                    .find(|decl| {
                        matches!(&decl.kind, DeclKind::Import { path, .. } if path == import_path)
                    })
                    .map_or_else(Span::default, |decl| decl.span);
                let exports = &other.checked.exports;
                if let Some(selected) = names {
                    // `import A.B { a, b }` brings only those names in, and a
                    // name the module does not export is an error rather than
                    // a silent no-op.
                    for name in selected {
                        if !exports.contains_key(name) {
                            let note =
                                missing_export_note(name, exports.keys().map(String::as_str));
                            self.diagnostics.push(
                                Diagnostic::error(
                                    "E0301",
                                    format!(
                                        "module `{}` does not export `{name}`",
                                        import_path.join(".")
                                    ),
                                    span,
                                )
                                .with_note(note),
                            );
                        }
                    }
                }
                bindings.push(InterfaceBinding {
                    module: other.checked.module_path(),
                    prefix: prefix.clone(),
                    names: names.clone(),
                });
                let runtime_names = names.as_ref().map(|names| {
                    let selected: Vec<_> = names.iter().filter_map(|name| match other.checked.exports.get(name) {
                        Some(aevia_types::Export::Type { adt } | aevia_types::Export::TypeCtor { adt, .. }) => Some(*adt),
                        _ => None,
                    }).collect();
                    let mut expanded = names.clone();
                    for (name, export) in &other.checked.exports {
                        if matches!(export, aevia_types::Export::Ctor { adt, .. } | aevia_types::Export::TypeCtor { adt, .. } if selected.contains(adt)) && !expanded.contains(name) { expanded.push(name.clone()); }
                    }
                    expanded
                });
                runtime_imports.push(ResolvedImport {
                    file: other.file,
                    prefix: prefix.clone(),
                    names: runtime_names,
                });
                import_specs.push((prefix, other_index, names.clone()));
            }
            // A module sees its dependency closure, never unrelated modules
            // merely checked earlier in the package traversal.
            let mut reachable = std::collections::HashSet::new();
            let mut pending: Vec<_> = runtime_imports.iter().map(|import| import.file).collect();
            while let Some(file) = pending.pop() {
                if reachable.insert(file)
                    && let Some(module) = modules.iter().find(|module| module.file == file)
                {
                    pending.extend(module.imports.iter().map(|import| import.file));
                }
            }
            let interfaces = || -> Vec<ModuleInterface> {
                modules
                    .iter()
                    .filter(|module| reachable.contains(&module.file))
                    .map(|module| module.checked.interface())
                    .collect()
            };
            let imports = || -> Vec<Import> {
                import_specs
                    .iter()
                    .map(|(prefix, index, names)| {
                        let mut exports = modules[*index].checked.exports.clone();
                        if let Some(selected) = names {
                            exports.retain(|name, _| selected.contains(name));
                        }
                        (prefix.clone(), exports)
                    })
                    .collect()
            };
            let identity = if let Some(identity) = self.source_identities.get(&path) {
                Some(identity.clone())
            } else if let Some((entry, identity)) = &self.entry_identity
                && entry == &path
            {
                Some(identity.module_path(parsed.module.name.as_deref()))
            } else if self.stdlib.contains_key(&path.path)
                || self.kernel_modules.contains_key(&path.path)
            {
                None
            } else {
                self.roots_for(&path)
                    .map(|roots| roots.resolved_module_identity(&parsed.context, &parsed.path))
            };
            let cache_key = format!("{path:?}");
            let print = self.cache.as_ref().map(|_| {
                let text = self
                    .sources
                    .file(parsed.file)
                    .map(|file| file.text.clone())
                    .unwrap_or_default();
                fingerprint(&[
                    &text,
                    &parsed.file,
                    &imports(),
                    &interfaces(),
                    &identity,
                    &bindings,
                    &self.options,
                ])
            });
            let reused = match (&self.cache, print) {
                | (Some(cache), Some(print)) => {
                    let cache = cache.borrow();
                    cache.entries.get(&cache_key).and_then(|entry| {
                        (entry.file == parsed.file && entry.fingerprint == print).then(|| {
                            aevia_types::check::CheckResult {
                                diagnostics: entry.diagnostics.clone(),
                                module: entry.module.clone(),
                            }
                        })
                    })
                },
                | _ => None,
            };
            // The on-disk key (§9.11). Generated kernel namespaces depend on
            // the live manifest rather than on a source file, so they are
            // never cached; a module whose dependency interfaces cannot all
            // be digested is simply not cacheable.
            let disk_key = match &self.disk {
                | Some(disk) if !self.kernel_modules.contains_key(&path.path) => {
                    let closure: Option<Vec<Digest>> = modules
                        .iter()
                        .filter(|module| reachable.contains(&module.file))
                        .map(|module| digests.get(&module.file).copied().flatten())
                        .collect();
                    closure.and_then(|closure| {
                        let text = self
                            .sources
                            .file(parsed.file)
                            .map(|file| file.text.clone())
                            .unwrap_or_default();
                        let prefixes: Vec<String> = import_specs
                            .iter()
                            .map(|(prefix, _, _)| prefix.clone())
                            .collect();
                        disk.key(&KeyInputs {
                            text: &text,
                            file: parsed.file,
                            path: &parsed.path,
                            identity: &identity,
                            bindings: &bindings,
                            prefixes: &prefixes,
                            options: &self.options,
                            interfaces: &closure,
                        })
                    })
                },
                | _ => None,
            };
            let mut own_digest: Option<Digest> = None;
            let mut from_memory = false;
            let mut loaded_from_disk = false;
            let restored = match reused {
                | Some(checked) => {
                    from_memory = true;
                    Some(checked)
                },
                | None => {
                    match (&self.disk, &disk_key) {
                        | (Some(disk), Some(key)) => {
                            disk.load(key, parsed.file).map(|hit| {
                                own_digest = Some(hit.interface);
                                loaded_from_disk = true;
                                Counters::bump(&self.counters.disk_hits);
                                aevia_types::check::CheckResult {
                                    diagnostics: hit.diagnostics,
                                    module: hit.module,
                                }
                            })
                        },
                        | _ => None,
                    }
                },
            };
            let checked = match restored {
                | Some(checked) => {
                    if from_memory && let Some(cache) = &self.cache {
                        cache.borrow_mut().reused += 1;
                    }
                    checked
                },
                | None => {
                    let checked = check_module_with_identity(
                        parsed.file,
                        &parsed.module,
                        &imports(),
                        &interfaces(),
                        &self.options,
                        identity,
                        &bindings,
                    );
                    Counters::bump(&self.counters.checked);
                    if let Some(cache) = &self.cache {
                        cache.borrow_mut().checked += 1;
                    }
                    checked
                },
            };
            if self.disk.is_some() {
                if own_digest.is_none() {
                    own_digest = crate::disk_cache::interface_digest(&checked.module.interface());
                }
                digests.insert(parsed.file, own_digest);
                // Only an error-free check is stored: its diagnostics are
                // warnings, and every type it declares is complete.
                if !from_memory
                    && !loaded_from_disk
                    && !checked.diagnostics.has_errors()
                    && let (Some(disk), Some(key), Some(digest)) =
                        (&self.disk, &disk_key, own_digest)
                    && disk.store(
                        key,
                        parsed.file,
                        digest,
                        &checked.diagnostics,
                        &checked.module,
                    )
                {
                    Counters::bump(&self.counters.disk_writes);
                }
            }
            if !from_memory && let (Some(cache), Some(print)) = (&self.cache, print) {
                cache.borrow_mut().entries.insert(
                    cache_key,
                    CacheEntry {
                        file: parsed.file,
                        fingerprint: print,
                        diagnostics: checked.diagnostics.clone(),
                        module: checked.module.clone(),
                    },
                );
            }
            self.diagnostics.extend(checked.diagnostics.clone());
            let aliases = prefixes
                .get(&path)
                .map(|list| {
                    list.iter()
                        .filter(|prefix| **prefix != parsed.path.join("."))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            modules.push(LoadedModule {
                path: parsed.path,
                file: parsed.file,
                source: parsed.context,
                module: parsed.module,
                checked: checked.module,
                aliases,
                imports: runtime_imports,
            });
        }
        // The entry file is last in dependency order, which is what `entry`
        // relies on; the walk already guarantees it.
        let _ = root;
        (modules, sources, self.diagnostics, dependencies)
    }
}

/// Publishes a module's constructors under the names an importer can write.
///
/// A user type is nominal per module (D6): `A.Circle` takes its id from A's
/// type, not from whichever same-named type the process registry finds first.
fn publish_constructors(
    interp: &mut crate::Interp,
    module: &LoadedModule,
) {
    for (name, &(adt, index)) in &module.checked.constructors {
        if !module.checked.exports.contains_key(name) {
            continue;
        }
        let Some(value) = crate::eval::source_ctor_value_at(adt, index) else {
            continue;
        };
        let qualified = format!("{}.{name}", module.path.join("."));
        interp.globals.insert(qualified, value.clone());
        for alias in &module.aliases {
            let key = if alias.is_empty() {
                name.clone()
            } else {
                format!("{alias}.{name}")
            };
            interp.globals.insert(key, value.clone());
        }
    }
}

/// The name of a module in a diagnostic: its path, or `<entry>`.
fn module_label(path: &[String]) -> String {
    if path.is_empty() {
        "<entry>".to_string()
    } else {
        path.join(".")
    }
}

/// The name a module declares, or its file stem.
fn module_path(
    module: &Module,
    file: &Path,
) -> Vec<String> {
    if let Some(name) = &module.name {
        return name.clone();
    }
    file.file_stem()
        .map(|stem| vec![stem.to_string_lossy().to_string()])
        .unwrap_or_default()
}

/// Evaluates a program's modules in order into `interp`.
///
/// Each module gets its own scope so that two modules may both define a
/// private helper, and every top-level name is also exported under its
/// qualified name so that importers can reach it.
pub fn run_modules(
    interp: &mut crate::Interp,
    program: &Program,
) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    let last = program.modules.len().saturating_sub(1);
    let mut exports: HashMap<FileId, HashMap<String, crate::Value>> = HashMap::new();
    let mut scopes = HashMap::new();
    for (index, module) in program.modules.iter().enumerate() {
        // A module's own `unit` declarations (and the ones it imported) are in
        // its checked table; the runtime needs them to fold literals.
        for unit in module.checked.units.list() {
            if interp.units.lookup(&unit.name).is_none() {
                let _ = interp.units.insert(unit.clone());
            }
        }
        // The interface names this module's instances (`InstanceKey::mangle()`)
        // and makes its classes/aliases visible to the modules loaded after it.
        interp.prepare_module(&module.checked);
        // Capture each import's actual values in the module's lexical scope.
        // Global aliases are a convenience for REPL inspection, not a binding
        // environment: two packages may both import their own `Helper`.
        let imported_scopes = module
            .imports
            .iter()
            .filter_map(|import| scopes.get(&import.file).cloned())
            .collect();
        let scope = interp.module_scope(imported_scopes);
        for import in &module.imports {
            if let Some(values) = exports.get(&import.file) {
                for (name, value) in values {
                    if import
                        .names
                        .as_ref()
                        .is_some_and(|names| !names.contains(name))
                    {
                        continue;
                    }
                    let key = if import.prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{}.{name}", import.prefix)
                    };
                    scope.vars.borrow_mut().insert(key, value.clone());
                }
            }
        }
        interp.registrations = module.checked.registrations.clone();
        diagnostics.extend(interp.load_into(&module.module, &scope));
        let names = crate::top_level_names(&module.module);
        let mut values = HashMap::new();
        for name in module.checked.exports.keys() {
            if let Some(value) = scope.lookup(name) {
                values.insert(name.clone(), value);
            }
        }
        for name in &names {
            if let Some(value) = scope.lookup(name) {
                let key = if index == last {
                    name.clone()
                } else {
                    format!("{}.{name}", module.path.join("."))
                };
                interp.globals.insert(key, value);
            }
        }
        exports.insert(module.file, values);
        interp.file_scopes.insert(module.file, scope.clone());
        interp.file_keys.insert(
            module.file,
            std::sync::Arc::from(format!(
                "{}|{:?}|{:?}",
                module.path.join("."),
                module.source.path,
                module.source.owner
            )),
        );
        scopes.insert(module.file, scope);
        if index == last {
            continue;
        }
        // A qualified constructor (`A.Red`) is a value the importer names, and
        // its identity is A's type, not whichever same-named type was checked
        // first (D6).
        publish_constructors(interp, module);
        // The checker resolved `G.f` and the bare `f` of an `open`; the
        // runtime has to publish the same keys or the two disagree.
        let qualified = module.path.join(".");
        let names = crate::top_level_names(&module.module);
        for prefix in &module.aliases {
            for name in &names {
                let from = format!("{qualified}.{name}");
                let Some(value) = interp.globals.get(&from).cloned() else {
                    continue;
                };
                let to = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                interp.globals.insert(to, value);
            }
        }
    }
    diagnostics
}

/// The help for `import M { name }` when `M` does not export `name`.
///
/// A misspelling gets the closest export; a correctly spelt name that lives in
/// a different standard-library module (`normal` is in `Stats.Distributions`,
/// not `Stats`) gets the modules that do export it.
fn missing_export_note<'a>(
    name: &str,
    exports: impl IntoIterator<Item = &'a str>,
) -> String {
    if let Some(close) = aevia_types::check::closest_name(name, exports) {
        return format!("did you mean `{close}`?");
    }
    let homes: Vec<String> = aevia_std::stdlib::module_paths()
        .filter(|path| {
            let components: Vec<&str> = path.split('/').collect();
            aevia_std::stdlib::exports(&components).contains(&name)
        })
        .map(|path| path.replace('/', "."))
        .take(3)
        .collect();
    if homes.is_empty() {
        "check the spelling, or drop the name list".to_string()
    } else {
        let listed: Vec<String> = homes.iter().map(|home| format!("`{home}`")).collect();
        format!("`{name}` is exported by {}", listed.join(", "))
    }
}
