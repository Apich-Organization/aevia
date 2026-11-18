//! Runtime for Aevia: values, the symbolic layer, the interpreter and the
//! built-in functions.
//!
//! ```
//! use aevia_core::Interp;
//! use aevia_core::compile;
//!
//! let source =
//!     "let g: Real[L/T^2] = 9.80665 m/s^2\nlet h: Real[L] = 20.0 m\nlet t = sqrt(2 * h / g)\n";
//! let compilation = compile("test.aev", source);
//! assert!(!compilation.diagnostics.has_errors());
//! let module = compilation.module.as_ref().expect("parsed");
//! let mut interp = Interp::new(&compilation.checked);
//! interp.load(module);
//! let value = interp.eval_toplevel("t").expect("evaluates");
//! // `h` and `g` are annotated `Real`, so their decimal literals are read
//! // exactly (§3.10.1); without the annotation they would be `Float`.
//! // The square root of an exact value has no exact value and stays
//! // symbolic (§6.3.1), its square factor pulled out; `approx` is the
//! // explicit step to a number.
//! assert!(aevia_core::value::repr(&value).contains("sqrt("));
//! ```

#![forbid(unsafe_code)]
#![allow(
    clippy::needless_range_loop,
    clippy::should_implement_trait,
    clippy::type_complexity,
    clippy::result_large_err,
    // Runtime and evidence enums are unboxed plain data; the builtin dispatcher threads explicit context.
    clippy::large_enum_variant,
    clippy::too_many_arguments
)]

pub mod builtins;
mod callback;
pub mod cancellation;
pub mod disk_cache;
pub mod display;
pub mod driver;
pub mod eval;
pub mod eval_ir;
mod evidence;
pub mod fxhash;
mod generics;
mod host_witness;
pub mod int;
pub mod io_do;
#[cfg(feature = "jit")]
mod jit;
pub mod kernel;
pub mod manifest;
pub mod parallel;
mod pattern;
mod plugin_source;
#[cfg(test)]
mod release_tests;
pub mod rssn_module;
pub mod source;
pub mod sparse;
pub mod sym;
mod systems;
mod try_control;
pub mod value;

use aevia_syntax::Diagnostic;
use aevia_syntax::Diagnostics;
use aevia_syntax::Module;
use aevia_syntax::SourceMap;
use aevia_syntax::lex;
use aevia_syntax::parse_module;
use aevia_types::CheckedModule;
use aevia_types::check::check_module_with_identity;
use std::rc::Rc;
use value::Scope;
use value::Value;

pub use cancellation::CancellationToken;
pub use eval::EvalError;
pub use eval::Interp;
pub use eval::Mode;
pub use eval::Output;
pub use eval::install_stack_budget;
pub use value::show;

/// The result of compiling one file.
pub struct Compilation {
    /// The parsed module, when parsing succeeded far enough to produce one.
    pub module: Option<Module>,
    /// The checked interface.
    pub checked: CheckedModule,
    /// Parse and type diagnostics.
    pub diagnostics: Diagnostics,
    /// The sources, for rendering diagnostics.
    pub sources: SourceMap,
}

/// Lexes, parses and checks a source file.
#[must_use]
pub fn compile(
    name: &str,
    source: &str,
) -> Compilation {
    compile_with_source_identity(name, source, &source::SourceIdentity::from_name(name))
}

/// Compiles a revision of a host-owned source while retaining its stable
/// nominal identity. `name` remains the display name in diagnostics.
#[must_use]
pub fn compile_with_source_identity(
    name: &str,
    source: &str,
    identity: &source::SourceIdentity,
) -> Compilation {
    let mut sources = SourceMap::new();
    let file = sources.add(name, source);
    let mut diagnostics = Diagnostics::new();
    let lexed = match lex(file, source) {
        | Ok(lexed) => lexed,
        | Err(errors) => {
            return Compilation {
                module: None,
                checked: CheckedModule::default(),
                diagnostics: errors,
                sources,
            };
        },
    };
    let (module, parse_diagnostics) = parse_module(file, source, &lexed);
    diagnostics.extend(parse_diagnostics);
    if diagnostics.has_errors() {
        return Compilation {
            module: Some(module),
            checked: CheckedModule::default(),
            diagnostics,
            sources,
        };
    }
    let checked = check_module_with_identity(
        file,
        &module,
        &[],
        &[],
        &aevia_types::check::CheckOptions::default(),
        Some(identity.module_path(module.name.as_deref())),
        &[],
    );
    diagnostics.extend(checked.diagnostics.clone());
    Compilation {
        module: Some(module),
        checked: checked.module,
        diagnostics,
        sources,
    }
}

impl Interp {
    /// Kept for hosts that called it before loading a module: registration
    /// now happens after the module's functions are bound
    /// ([`Interp::publish_registrations_in`]), so there is nothing to report
    /// beforehand.
    pub fn publish_registrations(&mut self) -> Diagnostics {
        Diagnostics::new()
    }

    /// Evaluates a module's top-level declarations.
    ///
    /// Returns the diagnostics raised while initialising the module; they are
    /// returned rather than printed so the caller can render them with source
    /// context.
    pub fn load(
        &mut self,
        module: &Module,
    ) -> Diagnostics {
        let mut diagnostics = self.publish_registrations();
        let root = self.module_scope(Vec::new());
        self.file_scopes.insert(module.file, root.clone());
        self.file_keys
            .insert(module.file, std::sync::Arc::from("<single module>"));
        diagnostics.extend(self.load_into(module, &root));
        // A single-file program keeps its names unqualified, which is what
        // `eval_toplevel` and the REPL expect.
        for name in top_level_names(module) {
            if let Some(value) = root.lookup(&name) {
                crate::value::bump_environment();
                self.globals.insert(name, value);
            }
        }
        diagnostics
    }

    /// Evaluates a module into `scope` and exports its names as `path.name`.
    ///
    /// Each module gets its own scope, so two modules may both define a
    /// private helper without one shadowing the other.
    pub fn load_qualified(
        &mut self,
        module: &Module,
        path: &[String],
    ) -> Diagnostics {
        let mut diagnostics = self.publish_registrations();
        let scope = self.module_scope(Vec::new());
        self.file_scopes.insert(module.file, scope.clone());
        self.file_keys
            .insert(module.file, std::sync::Arc::from(path.join(".").as_str()));
        diagnostics.extend(self.load_into(module, &scope));
        let names = top_level_names(module);
        if path.is_empty() {
            for name in names {
                if let Some(value) = scope.lookup(&name) {
                    crate::value::bump_environment();
                    self.globals.insert(name, value);
                }
            }
            return diagnostics;
        }
        let prefix = path.join(".");
        for name in names {
            if let Some(value) = scope.lookup(&name) {
                crate::value::bump_environment();
                self.globals.insert(format!("{prefix}.{name}"), value);
            }
        }
        diagnostics
    }

    /// Evaluates a module's declarations into one scope.
    fn load_into(
        &mut self,
        module: &Module,
        root: &Rc<Scope>,
    ) -> Diagnostics {
        reserve_source_declarations(root, &module.decls);
        match self.mode {
            | Mode::Ast => self.load_into_ast(module, root),
            | Mode::Ir => self.load_into_ir(module, root),
        }
    }

    /// Initialises a module by walking the typed AST.
    fn load_into_ast(
        &mut self,
        module: &Module,
        root: &Rc<Scope>,
    ) -> Diagnostics {
        let mut diagnostics = Diagnostics::new();
        let root = root.clone();
        // Functions first: they are values, and §7.1.2 lets them refer to any
        // top-level name of their module.
        for decl in &module.decls {
            if let aevia_syntax::DeclKind::Fn { name, args, body, .. } = &decl.kind {
                let closure = self.checked_ast_closure(
                    args.iter().map(|arg| arg.pattern.clone()).collect(),
                    body.clone(),
                    root.clone(),
                    Some(name.clone()),
                    decl.span,
                );
                root.bind_source(name, closure, source_visibility(decl));
            }
        }
        // `instance` methods are values too, and a call site may reach them
        // before the declaration is seen (§7.1.2). They are bound under the
        // mangled global the checker names them with (§3.8.1).
        for decl in &module.decls {
            if let aevia_syntax::DeclKind::Instance { class, ty, methods } = &decl.kind {
                let export = self.instance_export_at(decl.span);
                for method in methods {
                    self.register_instance(class, ty, export.as_ref(), method, &root);
                }
            }
        }
        // Constructors are values too, and a construction inside this module
        // must use *this module's* nominal identity (D6), not the first
        // same-named type in the process registry.
        bind_module_constructors(self, module, &root);
        // §13.5.2: functions are values now and no `let` has run yet, so a
        // top-level binding may already call a published operator.
        diagnostics.extend(self.publish_module_registrations(module, &root));
        // Then the bindings, in passes: `let a = b + 1` before `let b = 41` is
        // legal (§7.1.2), so a pass that hits an unbound name retries once the
        // names after it are bound.
        let mut pending: Vec<usize> = module
            .decls
            .iter()
            .enumerate()
            .filter(|(_, decl)| matches!(decl.kind, aevia_syntax::DeclKind::Let { .. }))
            .map(|(index, _)| index)
            .collect();
        while !pending.is_empty() {
            let mut deferred = Vec::new();
            let mut progress = false;
            for index in pending {
                let decl = &module.decls[index];
                let aevia_syntax::DeclKind::Let { pattern, value, .. } = &decl.kind else {
                    continue;
                };
                match self.eval(value, &root) {
                    | Ok(bound) => {
                        progress = true;
                        if let Err(error) = self.bind_source_pattern(
                            pattern,
                            &bound,
                            &root,
                            source_visibility(decl),
                        ) {
                            diagnostics.push(error.diagnostic());
                        }
                    },
                    // A name that is not bound yet may be bound by a later
                    // declaration; anything else is a real failure.
                    | Err(error) if error.code == "E0301" => deferred.push((index, error)),
                    | Err(error) => {
                        progress = true;
                        diagnostics.push(error.diagnostic());
                    },
                }
            }
            if deferred.is_empty() {
                break;
            }
            if !progress {
                for (_, error) in deferred {
                    diagnostics.push(error.diagnostic());
                }
                break;
            }
            pending = deferred.into_iter().map(|(index, _)| index).collect();
        }
        diagnostics
    }

    /// Initialises a module by running the Core IR.
    ///
    /// The order is the one `load_into_ast` uses, because the language requires
    /// it (§7.1.2): `fn`s first so they can refer to each other and to any
    /// top-level name, then instance methods, then the `let`s in passes, so a
    /// forward reference is deferred rather than reported.
    fn load_into_ir(
        &mut self,
        module: &Module,
        root: &Rc<Scope>,
    ) -> Diagnostics {
        let mut diagnostics = Diagnostics::new();
        let mut lowered = aevia_ir::lower::lower_module_with_runtime_evidence(
            module,
            &self.rssn_call_sites,
            &self.dictionary_sites,
            &self.literal_reps,
            &self.method_refs,
            &self.context.evidence.constructor_refs,
            &self.context.evidence.expr_types,
            &self.constant_regions,
        );
        if self.optimize_ir {
            aevia_ir::optimize::module(&mut lowered);
        }
        let sites: std::collections::HashMap<_, _> = module
            .decls
            .iter()
            .map(|decl| (decl.span, source_visibility(decl)))
            .collect();
        let root = root.clone();
        for decl in &lowered.fns {
            let closure = crate::eval_ir::module_closure(&decl.function, &root);
            root.bind_source(&decl.name, closure, sites[&decl.span]);
        }
        for instance in &lowered.instances {
            let export = self.instance_export_at(instance.span);
            for method in &instance.methods {
                self.register_instance_function(
                    &instance.class,
                    &instance.head,
                    export.as_ref(),
                    &method.name,
                    &method.function,
                    &root,
                );
            }
        }
        bind_module_constructors(self, module, &root);
        diagnostics.extend(self.publish_module_registrations(module, &root));
        let mut pending: Vec<usize> = (0..lowered.lets.len()).collect();
        while !pending.is_empty() {
            let mut deferred = Vec::new();
            let mut progress = false;
            for index in pending {
                let decl = &lowered.lets[index];
                match crate::eval_ir::eval_top(self, &decl.block, decl.nslots, &root) {
                    | Ok(bound) => {
                        progress = true;
                        if let Err(error) = self.bind_source_pattern(
                            &decl.pattern,
                            &bound,
                            &root,
                            sites[&decl.span],
                        ) {
                            diagnostics.push(error.diagnostic());
                        }
                    },
                    | Err(error) if error.code == "E0301" => deferred.push((index, error)),
                    | Err(error) => {
                        progress = true;
                        diagnostics.push(error.diagnostic());
                    },
                }
            }
            if deferred.is_empty() {
                break;
            }
            if !progress {
                for (_, error) in deferred {
                    diagnostics.push(error.diagnostic());
                }
                break;
            }
            pending = deferred.into_iter().map(|(index, _)| index).collect();
        }
        diagnostics
    }

    /// Binds one `instance` method body that was lowered to the Core IR.
    ///
    /// The mangled global and the instance index are the ones
    /// [`Interp::register_instance`] builds, so a call site resolved by the
    /// checker finds either form.
    pub fn register_instance_function(
        &mut self,
        class: &str,
        head: &aevia_syntax::ast::Ty,
        export: Option<&aevia_types::check::InstanceExport>,
        method: &str,
        function: &Rc<aevia_ir::Function>,
        env: &Rc<Scope>,
    ) {
        let spelled = crate::eval::instance_head_name(head);
        let key = export.map(|export| export.key.clone());
        let name = match &key {
            | Some(key) => Interp::instance_global(key, method),
            | None => format!("{class}@{}::{method}", spelled.as_deref().unwrap_or("?")),
        };
        let closure = crate::eval_ir::module_closure(function, env);
        self.context
            .instances
            .borrow_mut()
            .push(crate::evidence::InstanceTemplate {
                entry: crate::eval::InstanceEntry {
                    key: key.clone(),
                    checked_head: export.map(|export| export.head.clone()),
                    shape: crate::eval::head_shape(head, &self.aliases),
                    global: name.clone(),
                },
                method: method.to_string(),
                code: Rc::new(crate::evidence::MethodCode::Ir(function.clone())),
                cached: Rc::new(std::cell::RefCell::new(match &closure {
                    | Value::Closure(closure) => Rc::downgrade(closure),
                    | _ => unreachable!(),
                })),
                scope: Rc::downgrade(env),
            });
        crate::value::bump_environment();
        self.globals.insert(name.clone(), closure);
        crate::value::bump_environment();
        self.method_classes
            .insert(method.to_string(), class.to_string());
        self.instance_methods
            .entry(format!("{class}::{method}"))
            .or_default()
            .push(crate::eval::InstanceEntry {
                key,
                checked_head: export.map(|export| export.head.clone()),
                shape: crate::eval::head_shape(head, &self.aliases),
                global: name,
            });
    }

    /// Evaluates a named top-level binding, for tests and the REPL.
    ///
    /// # Errors
    ///
    /// Returns an error when the name is unknown.
    pub fn eval_toplevel(
        &mut self,
        name: &str,
    ) -> Result<Value, EvalError> {
        self.globals
            .get(name)
            .cloned()
            .ok_or_else(|| EvalError::new("E0301", format!("unbound name `{name}`")))
    }

    /// Installs checked evidence for an append-only module revision while
    /// retaining the declaration cursor of already initialized instances.
    pub fn prepare_incremental_module(
        &mut self,
        checked: &CheckedModule,
        loaded: &[aevia_syntax::Decl],
    ) {
        self.prepare_module(checked);
        if self.instance_sites.is_empty() {
            self.next_instance_export = loaded
                .iter()
                .filter(|decl| matches!(decl.kind, aevia_syntax::DeclKind::Instance { .. }))
                .count()
                .min(self.instance_exports.len());
        }
        self.units = checked.units.clone();
        self.dims = checked.dims.clone();
    }

    /// Installs a checked incremental batch in one lexical scope.
    ///
    /// Checked evidence is installed after taking a rollback snapshot. Functions,
    /// constructors and instance methods are bound before initializers, so a pasted module
    /// can use forward function references and mutual recursion. Each batch
    /// captures a fresh scope: later submissions cannot rewrite bindings seen
    /// by closures from earlier submissions.
    ///
    /// # Errors
    ///
    /// Returns the number of declarations reached before the first runtime
    /// error and that error. Failure is atomic: no new batch bindings remain,
    /// and symbol counters, random state and instance registrations are restored.
    /// Later initializers are not executed. The caller must also reinstall the
    /// previous checked revision with [`Self::prepare_incremental_module`].
    pub fn load_incremental_declarations(
        &mut self,
        checked: &CheckedModule,
        loaded: &[aevia_syntax::Decl],
        declarations: &[aevia_syntax::Decl],
    ) -> Result<(), (usize, EvalError)> {
        // Each batch creates its own scope, so these snapshots only clone
        // binding tables: no scope captured by an earlier closure is mutated.
        let globals = self.globals.clone();
        let symbol_counts = self.symbol_counts.clone();
        let seed = self.seed;
        let constants_seeded = self.constants_seeded;
        let instance_methods = self.instance_methods.clone();
        let method_classes = self.method_classes.clone();
        let class_keys = self.class_keys.clone();
        let aliases = self.aliases.clone();
        let adts = self.adts.clone();
        let module_scopes = self.module_scopes.clone();
        let next_instance_export = self.next_instance_export;
        let context = self.context.clone();
        let instances = context.instances.borrow().clone();
        let active_scope = self.active_scope.clone();
        let bindings = self.bindings.clone();
        self.prepare_incremental_module(checked, loaded);
        let result = self.load_incremental_declarations_in(declarations);
        if result.is_err() {
            crate::value::bump_environment();
            self.globals = globals;
            self.symbol_counts = symbol_counts;
            self.seed = seed;
            self.constants_seeded = constants_seeded;
            self.instance_methods = instance_methods;
            crate::value::bump_environment();
            self.method_classes = method_classes;
            self.class_keys = class_keys;
            self.aliases = aliases;
            self.adts = adts;
            self.module_scopes = module_scopes;
            self.next_instance_export = next_instance_export;
            *context.instances.borrow_mut() = instances;
            self.context = context;
            self.active_scope = active_scope;
            self.bindings = bindings;
        }
        result
    }

    fn load_incremental_declarations_in(
        &mut self,
        declarations: &[aevia_syntax::Decl],
    ) -> Result<(), (usize, EvalError)> {
        let root = self.root();
        self.remember_scope(&root);
        reserve_source_declarations(&root, declarations);
        let prebound = |decl: &aevia_syntax::Decl| {
            matches!(
                decl.kind,
                aevia_syntax::DeclKind::Fn { .. }
                    | aevia_syntax::DeclKind::Type { .. }
                    | aevia_syntax::DeclKind::Instance { .. }
            )
        };
        for declaration in declarations.iter().filter(|decl| prebound(decl)) {
            self.load_decl_in(declaration, &root)
                .map_err(|error| (0, error))?;
        }
        // Instance registration also publishes mangled implementation
        // globals. Preserve these in this batch's lexical snapshot.
        crate::value::bump_environment();
        root.vars.borrow_mut().extend(self.globals.clone());
        let mut pending: Vec<usize> = declarations
            .iter()
            .enumerate()
            .filter(|(_, declaration)| !prebound(declaration))
            .map(|(index, _)| index)
            .collect();
        while !pending.is_empty() {
            let mut deferred = Vec::new();
            let mut progress = false;
            for index in pending {
                // A deferred initializer must not consume random/symbol state
                // twice when its forward dependency becomes available.
                let symbol_counts = self.symbol_counts.clone();
                let seed = self.seed;
                let constants_seeded = self.constants_seeded;
                let globals = self.globals.clone();
                match self.load_decl_in(&declarations[index], &root) {
                    | Ok(()) => progress = true,
                    | Err(error) if error.code == "E0301" => {
                        self.symbol_counts = symbol_counts;
                        self.seed = seed;
                        self.constants_seeded = constants_seeded;
                        crate::value::bump_environment();
                        self.globals = globals;
                        deferred.push((index, error));
                    },
                    | Err(error) => return Err((index, error)),
                }
            }
            if deferred.is_empty() {
                break;
            }
            if !progress {
                return Err(deferred.remove(0));
            }
            pending = deferred.into_iter().map(|(index, _)| index).collect();
        }
        Ok(())
    }

    /// Adds one top-level declaration to a running session (§9.8).
    ///
    /// The REPL appends declarations instead of re-evaluating the session: a
    /// fresh interpreter per line re-ran every `let` and every side effect,
    /// and reset `symbol_counts`, so `symbol("x")` on two lines produced the
    /// same symbol twice.
    ///
    /// # Errors
    ///
    /// Propagates the declaration's own runtime failure.
    pub fn load_decl(
        &mut self,
        decl: &aevia_syntax::Decl,
    ) -> Result<(), EvalError> {
        let root = self.root();
        self.remember_scope(&root);
        self.load_decl_in(decl, &root)
    }

    fn load_decl_in(
        &mut self,
        decl: &aevia_syntax::Decl,
        root: &Rc<Scope>,
    ) -> Result<(), EvalError> {
        use aevia_syntax::DeclKind;
        reserve_source_declarations(root, std::slice::from_ref(decl));
        if let DeclKind::Type {
            body: aevia_syntax::ast::TypeBody::Variants(ctors),
            ..
        } = &decl.kind
        {
            for ctor in ctors {
                if let Some(&(adt, index)) = self.current_constructors.get(&ctor.name)
                    && let Some(value) = crate::eval::source_ctor_value_at(adt, index)
                {
                    crate::value::bump_environment();
                    root.vars
                        .borrow_mut()
                        .insert(ctor.name.clone(), value.clone());
                    self.globals.insert(ctor.name.clone(), value);
                }
            }
            return Ok(());
        }
        if self.mode == Mode::Ir
            && let Some(mut lowered) = aevia_ir::lower::lower_decl_with_runtime_evidence(
                decl,
                &self.rssn_call_sites,
                &self.dictionary_sites,
                &self.literal_reps,
                &self.method_refs,
                &self.context.evidence.constructor_refs,
                &self.context.evidence.expr_types,
                &self.constant_regions,
            )
        {
            if self.optimize_ir {
                aevia_ir::optimize::declaration(&mut lowered);
            }
            return self.load_lowered_in(lowered, root, source_visibility(decl));
        }
        match &decl.kind {
            | DeclKind::Fn { name, args, body, .. } => {
                let closure = self.checked_ast_closure(
                    args.iter().map(|arg| arg.pattern.clone()).collect(),
                    body.clone(),
                    root.clone(),
                    Some(name.clone()),
                    decl.span,
                );
                root.bind_source(name, closure, source_visibility(decl));
                if let Some(latest) = root.lookup(name) {
                    crate::value::bump_environment();
                    self.globals.insert(name.clone(), latest);
                }
                Ok(())
            },
            | DeclKind::Let { pattern, value, .. } => {
                let bound = self.eval(value, root)?;
                self.bind_source_pattern(pattern, &bound, root, source_visibility(decl))?;
                self.export_source_pattern(pattern, root);
                Ok(())
            },
            | DeclKind::Instance { class, ty, methods } => {
                let export = self.instance_export_at(decl.span);
                for method in methods {
                    self.register_instance(class, ty, export.as_ref(), method, root);
                }
                Ok(())
            },
            | _ => Ok(()),
        }
    }

    /// Adds one declaration that was lowered to the Core IR (§9.8).
    fn load_lowered_in(
        &mut self,
        decl: aevia_ir::lower::LoweredDecl,
        root: &Rc<Scope>,
        visible: aevia_syntax::Span,
    ) -> Result<(), EvalError> {
        match decl {
            | aevia_ir::lower::LoweredDecl::Fn(decl) => {
                let closure = crate::eval_ir::module_closure(&decl.function, root);
                root.bind_source(&decl.name, closure, visible);
                if let Some(latest) = root.lookup(&decl.name) {
                    crate::value::bump_environment();
                    self.globals.insert(decl.name, latest);
                }
                Ok(())
            },
            | aevia_ir::lower::LoweredDecl::Let(decl) => {
                let bound = crate::eval_ir::eval_top(self, &decl.block, decl.nslots, root)?;
                self.bind_source_pattern(&decl.pattern, &bound, root, visible)?;
                self.export_source_pattern(&decl.pattern, root);
                Ok(())
            },
            | aevia_ir::lower::LoweredDecl::Instance(instance) => {
                let export = self.instance_export_at(instance.span);
                for method in &instance.methods {
                    self.register_instance_function(
                        &instance.class,
                        &instance.head,
                        export.as_ref(),
                        &method.name,
                        &method.function,
                        root,
                    );
                }
                Ok(())
            },
        }
    }

    /// Executes `main`, if it is an IO action.
    ///
    /// # Errors
    ///
    /// Propagates IO failures.
    pub fn run_main(&mut self) -> Result<Option<Value>, EvalError> {
        let Some(main) = self.globals.get("main").cloned() else {
            return Ok(None);
        };
        let value = match main {
            | Value::Io(action) => return self.run_io(&action).map(Some),
            | Value::Closure(closure) if closure.arity() == 0 => {
                self.apply(
                    Value::Closure(closure),
                    Vec::new(),
                    aevia_syntax::Span::default(),
                )?
            },
            | other => other,
        };
        match value {
            | Value::Io(action) => self.run_io(&action).map(Some),
            // §6.8.1: `main` describes an action. A `Result` payload that
            // escaped a `?` is not one, and reporting it as success made
            // `fn main(): IO Unit = do let x = parse_int("abc")? …` exit 0
            // without running a single statement.
            | Value::Unit => Ok(Some(Value::Unit)),
            | Value::Variant(_) => {
                Err(EvalError::new(
                    "E0702",
                    "`?` in `main` tried to return an error from a function that does not                  return a `Result`",
                ))
            },
            | other => Ok(Some(other)),
        }
    }

    fn bind_source_pattern(
        &mut self,
        pattern: &aevia_syntax::Pat,
        value: &Value,
        scope: &Rc<Scope>,
        visible: aevia_syntax::Span,
    ) -> Result<(), EvalError> {
        if let aevia_syntax::PatKind::Var(name) = &pattern.kind {
            scope.bind_source(name, value.clone(), visible);
            return Ok(());
        }
        // Match in a temporary lexical scope: failed destructuring must not
        // partially overwrite the module, and old revisions remain available.
        let bindings = Scope::child(scope);
        self.bind_pattern_entry(pattern, value, &bindings)?;
        for (name, value) in bindings.vars.borrow().iter() {
            scope.bind_source(name, value.clone(), visible);
        }
        Ok(())
    }

    fn export_source_pattern(
        &mut self,
        pattern: &aevia_syntax::Pat,
        scope: &Rc<Scope>,
    ) {
        for name in source_pattern_names(pattern) {
            if let Some(value) = scope.lookup(&name) {
                crate::value::bump_environment();
                self.globals.insert(name, value);
            }
        }
    }
}

/// Functions and recursive bindings see themselves; ordinary initializers
/// see the preceding declaration with that name.
fn source_visibility(declaration: &aevia_syntax::Decl) -> aevia_syntax::Span {
    let mut site = declaration.span;
    if matches!(
        declaration.kind,
        aevia_syntax::DeclKind::Fn { .. } | aevia_syntax::DeclKind::Let { rec: true, .. }
    ) {
        site.end = site.start;
    } else {
        site.start = site.end;
    }
    site
}

fn source_pattern_names(pattern: &aevia_syntax::Pat) -> Vec<String> {
    use aevia_syntax::PatKind;
    let mut names = Vec::new();
    match &pattern.kind {
        | PatKind::Var(name) => names.push(name.clone()),
        | PatKind::Ctor { args, .. } | PatKind::Tuple(args) => {
            for pattern in args {
                names.extend(source_pattern_names(pattern));
            }
        },
        | PatKind::Record(fields) => {
            for (name, pattern) in fields {
                if let Some(pattern) = pattern {
                    names.extend(source_pattern_names(pattern));
                } else {
                    names.push(name.clone());
                }
            }
        },
        | PatKind::Tensor { items, rest } => {
            for pattern in items {
                names.extend(source_pattern_names(pattern));
            }
            names.extend(rest.iter().cloned());
        },
        | PatKind::As { pattern, name } => {
            names.extend(source_pattern_names(pattern));
            names.push(name.clone());
        },
        | PatKind::Annot { pattern, .. } => names.extend(source_pattern_names(pattern)),
        | _ => {},
    }
    names
}

fn reserve_source_declarations(
    scope: &Rc<Scope>,
    declarations: &[aevia_syntax::Decl],
) {
    for declaration in declarations {
        let names = match &declaration.kind {
            | aevia_syntax::DeclKind::Fn { name, .. } => vec![name.clone()],
            | aevia_syntax::DeclKind::Let { pattern, .. } => source_pattern_names(pattern),
            | _ => continue,
        };
        let visible = source_visibility(declaration);
        for name in names {
            scope.reserve_source(&name, visible);
            if matches!(
                declaration.kind,
                aevia_syntax::DeclKind::Let { rec: false, .. }
            ) {
                scope.reserve_initializer(&name, declaration.span);
            }
        }
    }
}

/// Binds a module's variant constructors into its scope.
///
/// A user type is nominal per module (D6), so `A.Circle` and `B.Circle` are
/// different constructors even though both are spelled `Circle`; the runtime
/// must take the id from the defining module rather than the name-keyed global
/// registry.
fn bind_module_constructors(
    interp: &Interp,
    module: &Module,
    root: &Rc<Scope>,
) {
    for decl in &module.decls {
        let aevia_syntax::DeclKind::Type {
            body: aevia_syntax::ast::TypeBody::Variants(ctors),
            ..
        } = &decl.kind
        else {
            continue;
        };
        for ctor in ctors {
            if let Some(&(adt, index)) = interp.current_constructors.get(&ctor.name)
                && let Some(value) = crate::eval::source_ctor_value_at(adt, index)
            {
                crate::value::bump_environment();
                root.vars.borrow_mut().insert(ctor.name.clone(), value);
            }
        }
    }
}

/// The names a module binds at the top level.
#[must_use]
pub fn top_level_names(module: &Module) -> Vec<String> {
    let mut names = Vec::new();
    for decl in &module.decls {
        match &decl.kind {
            | aevia_syntax::DeclKind::Fn { name, .. } => names.push(name.clone()),
            | aevia_syntax::DeclKind::Let { pattern, .. } => {
                names.extend(source_pattern_names(pattern));
            },
            | _ => {},
        }
    }
    names
}

/// Compiles and runs a program, returning its captured output.
///
/// # Errors
///
/// Returns the first diagnostic when the program does not compile, or the
/// runtime error when `main` fails.
pub fn run(
    source: &str,
    name: &str,
) -> Result<String, Diagnostic> {
    let compilation = compile(name, source);
    if compilation.diagnostics.has_errors() {
        return Err(compilation.diagnostics.items[0].clone());
    }
    let Some(module) = &compilation.module else {
        return Err(Diagnostic::error(
            "E0608",
            "no module",
            aevia_syntax::Span::default(),
        ));
    };
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    // §9.7: workers recompile the same source, so closures cross by span.
    let replica_source = source.to_string();
    let replica_name = name.to_string();
    interp.set_replica(std::sync::Arc::new(move || {
        let compilation = compile(&replica_name, &replica_source);
        let module = compilation.module.as_ref().ok_or("no module")?;
        let mut replica = Interp::new(&compilation.checked).with_buffer();
        if replica.load(module).has_errors() {
            return Err("the program's top level failed on a worker".to_string());
        }
        Ok(replica)
    }));
    let load_diagnostics = interp.load(module);
    if load_diagnostics.has_errors() {
        return Err(load_diagnostics.items[0].clone());
    }
    interp.run_main().map_err(|error| error.diagnostic())?;
    Ok(interp.captured())
}
