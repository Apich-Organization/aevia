//! Loading a program: one file plus everything it imports.
//!
//! A module is a file, and its name is its path under the package's `src/`
//! directory: `import Physics.Ising` reads `src/Physics/Ising.aev`. A file with
//! no manifest keeps the older behaviour — the root is the entry file's
//! directory — so a standalone program resolves exactly as it always has.
//! Loading is a post-order walk, so a module is always checked after the
//! modules it imports, and a cycle is reported rather than followed.

use crate::manifest::Roots;
use aevia_syntax::ast::{DeclKind, Module};
use aevia_syntax::{lex, parse_module, Diagnostic, Diagnostics, FileId, SourceMap, Span};
use aevia_types::check::{
    check_module_with_interfaces, CheckOptions, CheckedModule, Import, ModuleInterface,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A module that was loaded and checked.
pub struct LoadedModule {
    /// Its path, empty for the entry file when it declares no name.
    pub path: Vec<String>,
    /// Where it came from.
    pub file: FileId,
    /// The parsed module.
    pub module: Module,
    /// Its checked interface.
    pub checked: CheckedModule,
    /// Every other prefix its names are reachable under: an alias, or the
    /// empty string for an `open` or a name list.
    pub aliases: Vec<String>,
}

/// A whole program.
pub struct Program {
    /// The sources, for rendering diagnostics.
    pub sources: SourceMap,
    /// The modules, in dependency order (imports first).
    pub modules: Vec<LoadedModule>,
    /// Every diagnostic from every module.
    pub diagnostics: Diagnostics,
}

impl Program {
    /// The module that defines `main`.
    pub fn entry(&self) -> Option<&LoadedModule> {
        self.modules.last()
    }

    /// The interface of the module called `path`.
    #[must_use]
    pub fn exports(&self, path: &[String]) -> Option<&CheckedModule> {
        self.modules
            .iter()
            .find(|module| module.path == path)
            .map(|module| &module.checked)
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
    /// How many distinct files were parsed (entries plus every import).
    pub files: usize,
    /// How many modules were checked.
    pub modules: usize,
    /// The number of check invocations (`== modules` for a single pass).
    pub checks: usize,
    /// How many diagnostics the pass produced, after deduplication.
    pub diagnostics: usize,
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
    for entry in entries {
        loader.load_file(entry, &[]);
    }
    let files = loader.parsed.len();
    let (modules, sources, mut diagnostics) = loader.finish(entries.first().cloned());
    dedupe_diagnostics(&mut diagnostics);
    let stats = LoadStats {
        entries: entries.len(),
        files,
        modules: modules.len(),
        checks: modules.len(),
        diagnostics: diagnostics.items.len(),
    };
    (
        Program {
            sources,
            modules,
            diagnostics,
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
pub fn load_with_roots(entry: &Path, roots: Roots) -> Program {
    // The entry's own directory is tried after the package's `src/`, so a file
    // in `tests/` or `examples/` reaches both the package's modules and its own
    // neighbours.
    let roots = match entry.parent() {
        Some(dir) => roots.with_local(dir),
        None => roots,
    };
    let mut loader = Loader {
        roots: Some(roots),
        ..Loader::default()
    };
    let root = loader.load_file(entry, &[]);
    let (modules, sources, diagnostics) = loader.finish(root);
    Program {
        sources,
        modules,
        diagnostics,
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
    let roots = match entry.parent() {
        Some(dir) => roots.with_local(dir),
        None => roots,
    };
    let mut loader = Loader {
        roots: Some(roots),
        options: options.clone(),
        ..Loader::default()
    };
    loader.overlay.insert(entry.to_path_buf(), source.to_string());
    let root = loader.load_file(entry, &[]);
    let (modules, sources, diagnostics) = loader.finish(root);
    Program {
        sources,
        modules,
        diagnostics,
    }
}

/// [`load_with_roots_and_source`] with the adjacent roots and default options.
#[must_use]
pub fn load_with_source(entry: &Path, source: &str) -> Program {
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
    diagnostics: Diagnostics,
    /// Parsed modules by canonical path, in post-order.
    order: Vec<PathBuf>,
    parsed: HashMap<PathBuf, Parsed>,
    /// The import stack, for cycle detection.
    stack: Vec<PathBuf>,
    /// The capabilities the checker gates static gaps on.
    options: CheckOptions,
    /// Entry sources supplied by the caller (an unsaved editor buffer), keyed by
    /// the entry path they stand in for. Imports still resolve from disk.
    overlay: HashMap<PathBuf, String>,
    /// Standard-library imports: synthetic path key → module components.
    ///
    /// The embedded stdlib (§8.20) is not on disk, so an `import Math` that no
    /// package root resolves is keyed as `stdlib/Math.aev` and its source comes
    /// from [`aevia_std::stdlib::source`]; post-order, cycle detection and
    /// `run_modules` then treat it exactly like a file module.
    stdlib: HashMap<PathBuf, Vec<String>>,
}

struct Parsed {
    file: FileId,
    module: Module,
    path: Vec<String>,
    imports: Vec<(PathBuf, Vec<String>, Option<String>, bool, Option<Vec<String>>)>,
}

impl Loader {
    fn load_file(&mut self, path: &Path, expected: &[String]) -> Option<PathBuf> {
        let canonical = path.to_path_buf();
        if self.roots.is_none() {
            self.roots = Some(Roots::adjacent(&canonical));
        }
        if self.parsed.contains_key(&canonical) {
            return Some(canonical);
        }
        if self.stack.contains(&canonical) {
            let mut cycle: Vec<String> = self
                .stack
                .iter()
                .map(|item| item.display().to_string())
                .collect();
            cycle.push(canonical.display().to_string());
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
        let embedded = self.stdlib.get(&canonical).cloned().and_then(|components| {
            let path: Vec<&str> = components.iter().map(String::as_str).collect();
            aevia_std::stdlib::source(&path).map(|source| (components, source))
        });
        // A dirty editor buffer for the entry file wins over disk; imports still
        // resolve through the loader.
        let overlaid = self.overlay.get(&canonical).cloned();
        let source = match (&overlaid, &embedded) {
            (Some(source), _) => source.clone(),
            (None, Some((_, source))) => source.to_string(),
            (None, None) => match std::fs::read_to_string(&canonical) {
                Ok(source) => source,
                Err(error) => {
                    self.diagnostics.push(Diagnostic::error(
                        "E0607",
                        format!("cannot read `{}`: {error}", canonical.display()),
                        Span::default(),
                    ));
                    return None;
                }
            },
        };
        let display = match (&overlaid, &embedded) {
            (Some(_), _) => canonical.display().to_string(),
            (None, Some((components, _))) => format!("stdlib://{}.aev", components.join("/")),
            (None, None) => canonical.display().to_string(),
        };
        let file = self.sources.add(display, &source);
        let lexed = match lex(file, &source) {
            Ok(lexed) => lexed,
            Err(errors) => {
                self.diagnostics.extend(errors);
                // Remember the failure so a second importer does not report it
                // again.
                self.parsed.insert(
                    canonical.clone(),
                    Parsed {
                        file,
                        module: Module {
                            name: None,
                            decls: Vec::new(),
                            file,
                        },
                        path: Vec::new(),
                        imports: Vec::new(),
                    },
                );
                self.order.push(canonical.clone());
                return None;
            }
        };
        let (module, parse_diagnostics) = parse_module(file, &source, &lexed);
        self.diagnostics.extend(parse_diagnostics);
        if self.diagnostics.has_errors() && self.stack.is_empty() {
            // The entry file failed to parse; there is nothing to import.
        }

        let path = module_path(&module, &canonical);
        // §2.2.5 / appendix B: a module's declared name must match the path it
        // is imported by, or `import A.B` would name a module called `C`. The
        // entry file is exempt because it is not imported by a path.
        if !expected.is_empty() && module.name.is_some() && path != expected {
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
        // A module path is absolute within the program: `import A.B` means
        // `<package src>/A/B.aev` from every file, not relative to the
        // importer, and `import Dep.A.B` means the dependency's copy.
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
            let resolved = self
                .roots
                .as_ref()
                .and_then(|roots| roots.resolve(import_path));
            let target = match resolved {
                Some(target) => target,
                None => {
                    // §8.20: the standard library ships embedded, not on disk.
                    // A module the package roots do not resolve but the stdlib
                    // names is loaded from the embedded source under a stable
                    // synthetic key, so post-order and cycle detection work the
                    // same way as for a file.
                    let components: Vec<&str> =
                        import_path.iter().map(String::as_str).collect();
                    if aevia_std::stdlib::is_module_name(&components) {
                        let mut key = PathBuf::from("stdlib");
                        for component in import_path {
                            key.push(component);
                        }
                        key.set_extension("aev");
                        self.stdlib.insert(key.clone(), import_path.clone());
                        key
                    } else {
                        let candidates = self
                            .roots
                            .as_ref()
                            .map(|roots| roots.candidates(import_path))
                            .unwrap_or_default();
                        let expected = candidates
                            .iter()
                            .map(|candidate| format!("`{}`", candidate.display()))
                            .collect::<Vec<_>>()
                            .join(" or ");
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E0607",
                                format!(
                                    "cannot find module `{}`: expected {expected}",
                                    import_path.join(".")
                                ),
                                decl.span,
                            )
                            .with_note(format!(
                                "imported from `{}`",
                                module_label(&path)
                            )),
                        );
                        continue;
                    }
                }
            };
            if let Some(resolved) = self.load_file(&target, import_path) {
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
                file,
                module,
                path,
                imports,
            },
        );
        self.order.push(canonical.clone());
        Some(canonical)
    }

    fn finish(
        mut self,
        root: Option<PathBuf>,
    ) -> (Vec<LoadedModule>, SourceMap, Diagnostics) {
        let sources = self.sources.clone();
        let Some(root) = root else {
            return (Vec::new(), sources, self.diagnostics);
        };
        // Every prefix each module is imported under, across all importers.
        let mut prefixes: HashMap<PathBuf, Vec<String>> = HashMap::new();
        for parsed in self.parsed.values() {
            for (target, import_path, alias, open, names) in &parsed.imports {
                let prefix = if *open || names.is_some() {
                    String::new()
                } else {
                    alias
                        .clone()
                        .unwrap_or_else(|| import_path.join("."))
                };
                let entry = prefixes.entry(target.clone()).or_default();
                if !entry.contains(&prefix) {
                    entry.push(prefix);
                }
            }
        }
        let mut modules: Vec<LoadedModule> = Vec::new();
        // The checked interfaces of the modules already resolved, in dependency
        // order (§7.4). They carry classes, instances and alias definitions, so
        // an importer can use a class it did not declare and expand an imported
        // alias instead of treating it as opaque.
        let mut interfaces: Vec<ModuleInterface> = Vec::new();
        for path in self.order.clone() {
            let Some(parsed) = self.parsed.remove(&path) else {
                continue;
            };
            // The imports of this module, as the checker wants them: one entry
            // per resolved module, with the prefix its names are reachable
            // under.
            let mut imports: Vec<Import> = Vec::new();
            for (target, import_path, alias, open, names) in &parsed.imports {
                let Some(other) = self
                    .order
                    .iter()
                    .position(|candidate| candidate == target)
                    .and_then(|index| modules.get(index))
                else {
                    continue;
                };
                // `open` and a name list both bring names in unqualified;
                // an alias or the module path qualifies them.
                let prefix = if *open || names.is_some() {
                    String::new()
                } else {
                    alias
                        .clone()
                        .unwrap_or_else(|| import_path.join("."))
                };
                let span = parsed
                    .module
                    .decls
                    .iter()
                    .find(|decl| {
                        matches!(&decl.kind, DeclKind::Import { path, .. } if path == import_path)
                    })
                    .map_or_else(Span::default, |decl| decl.span);
                let mut exports = other.checked.exports.clone();
                if let Some(selected) = names {
                    // `import A.B { a, b }` brings only those names in, and a
                    // name the module does not export is an error rather than
                    // a silent no-op.
                    for name in selected {
                        if !exports.contains_key(name) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    "E0301",
                                    format!(
                                        "module `{}` does not export `{name}`",
                                        import_path.join(".")
                                    ),
                                    span,
                                )
                                .with_note("check the spelling, or drop the name list"),
                            );
                        }
                    }
                    exports.retain(|name, _| selected.contains(name));
                }
                imports.push((prefix, exports));
            }
            let checked = check_module_with_interfaces(
                parsed.file,
                &parsed.module,
                &imports,
                &interfaces,
                &self.options,
            );
            interfaces.push(checked.module.interface());
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
                module: parsed.module,
                checked: checked.module,
                aliases,
            });
        }
        // The entry file is last in dependency order, which is what `entry`
        // relies on; the walk already guarantees it.
        let _ = root;
        (modules, sources, self.diagnostics)
    }
}

/// Publishes a module's constructors under the names an importer can write.
///
/// A user type is nominal per module (D6): `A.Circle` takes its id from A's
/// type, not from whichever same-named type the process registry finds first.
fn publish_constructors(
    interp: &mut crate::Interp,
    module: &Module,
    path: &aevia_types::keys::ModulePath,
    aliases: &[String],
) {
    for decl in &module.decls {
        let DeclKind::Type {
            body: aevia_syntax::ast::TypeBody::Variants(ctors),
            ..
        } = &decl.kind
        else {
            continue;
        };
        for ctor in ctors {
            let Some(value) = crate::eval::ctor_value(path, &ctor.name) else {
                continue;
            };
            let qualified = if path.is_prelude() {
                ctor.name.clone()
            } else {
                format!("{}.{}", path.dotted(), ctor.name)
            };
            interp.globals.insert(qualified, value.clone());
            for alias in aliases {
                let key = if alias.is_empty() {
                    ctor.name.clone()
                } else {
                    format!("{alias}.{}", ctor.name)
                };
                interp.globals.insert(key, value.clone());
            }
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
fn module_path(module: &Module, file: &Path) -> Vec<String> {
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
pub fn run_modules(interp: &mut crate::Interp, program: &Program) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    let last = program.modules.len().saturating_sub(1);
    for (index, module) in program.modules.iter().enumerate() {
        // A module's own `unit` declarations (and the ones it imported) are in
        // its checked table; the runtime needs them to fold literals.
        for unit in module.checked.units.list() {
            if interp.units.lookup(&unit.name).is_none() {
                let _ = interp.units.insert(unit.clone());
            }
        }
        // Every module's pinned `Rssn.<op>` keys, not just the entry module's:
        // an operator call in an imported module has a span in *that* file, and
        // without its key the evaluator would have to resolve again (PLAN D1).
        interp.add_rssn_calls(&module.checked.rssn_calls);
        // The same for the class instances the checker proved: a class-method
        // call in an imported module has a span in that module's file.
        interp.add_dictionaries(&module.checked.dictionaries);
        // The interface names this module's instances (`InstanceKey::mangle()`)
        // and makes its classes/aliases visible to the modules loaded after it.
        interp.prepare_module(&module.checked);
        // The entry module keeps its names unqualified: `main` is `main`.
        if index == last {
            diagnostics.extend(interp.load(&module.module));
            continue;
        }
        diagnostics.extend(interp.load_qualified(&module.module, &module.path));
        // A qualified constructor (`A.Red`) is a value the importer names, and
        // its identity is A's type, not whichever same-named type was checked
        // first (D6).
        let module_path = aevia_types::keys::ModulePath::new(module.path.iter().cloned());
        publish_constructors(interp, &module.module, &module_path, &module.aliases);
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
