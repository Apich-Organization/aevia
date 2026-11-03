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
use aevia_types::check::{check_module_with_imports, CheckedModule, Import};
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
}

struct Parsed {
    file: FileId,
    module: Module,
    path: Vec<String>,
    imports: Vec<(PathBuf, Vec<String>, Option<String>, bool, Option<Vec<String>>)>,
}

impl Loader {
    fn load_file(&mut self, path: &Path, _importer: &[String]) -> Option<PathBuf> {
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
        let source = match std::fs::read_to_string(&canonical) {
            Ok(source) => source,
            Err(error) => {
                self.diagnostics.push(Diagnostic::error(
                    "E0607",
                    format!("cannot read `{}`: {error}", canonical.display()),
                    Span::default(),
                ));
                return None;
            }
        };
        let file = self.sources.add(canonical.display().to_string(), &source);
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
            let Some(target) = resolved else {
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
                    .with_note(format!("imported from `{}`", module_label(&path))),
                );
                continue;
            };
            if let Some(resolved) = self.load_file(&target, &path) {
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
            let checked = check_module_with_imports(parsed.file, &parsed.module, &imports);
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
        // The entry module keeps its names unqualified: `main` is `main`.
        if index == last {
            diagnostics.extend(interp.load(&module.module));
            continue;
        }
        diagnostics.extend(interp.load_qualified(&module.module, &module.path));
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
