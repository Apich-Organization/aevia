//! Checking a module: name resolution, inference, and the exported interface.
//!
//! The checker walks the AST in four passes:
//!
//! 1. dimensions, units and type declarations, in source order;
//! 2. top-level signatures, so bodies may refer forward (§7.1.2);
//! 3. bodies;
//! 4. defaulting and generalisation of the top-level bindings.
//!
//! The output is a [`CheckedModule`] whose export table the runtime consumes:
//! it records which names are values, constructors, types or units, and their
//! types after defaulting.

use crate::adt::{self, Adt, Ctor};
use crate::builtins::{self, dynamic_value_type, op_error_type};
use crate::class::{ClassTable, InstanceError, Origin, Resolution};
use crate::diagnostic_bridge::code;
use crate::infer::{Infer, UnifyError};
use crate::ty::{Constraint, LiteralKind, Rep, Row, Scheme, Shape, Type};
use aevia_dim::{Dim, DimEnv, UnitDef, UnitError, UnitExpr, UnitTable};
use aevia_syntax::ast::*;
use aevia_syntax::{Diagnostic, Diagnostics, FileId, Span};
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};
use std::collections::{HashMap, HashSet};

/// What a name exported by a module denotes.
#[derive(Clone, Debug)]
pub enum Export {
    /// A function or value binding.
    Value {
        /// Its generalised type.
        scheme: Scheme,
        /// Index into the module's declarations.
        decl: usize,
        /// Number of parameters for functions.
        arity: usize,
    },
    /// A variant constructor.
    Ctor {
        /// The type it belongs to.
        adt: adt::AdtId,
        /// Its position in the type.
        index: usize,
        /// Its type as a function or value.
        scheme: Scheme,
    },
    /// A nominal type whose single constructor has the same name: `type P2 =
    /// | P2(Float, Float)`. Both `A.P2` (in an annotation) and `A.P2(1, 2)`
    /// (as a value) must work, so the export carries both.
    TypeCtor {
        /// The type.
        adt: adt::AdtId,
        /// The constructor's position.
        index: usize,
        /// The constructor's type.
        scheme: Scheme,
    },
    /// A nominal type.
    Type {
        /// The registered type.
        adt: adt::AdtId,
    },
    /// A transparent type alias.
    Alias {
        /// Number of parameters.
        params: usize,
    },
    /// A unit value such as `m`.
    Unit {
        /// The definition, with dimension and scale.
        def: UnitDef,
    },
    /// A base dimension or alias.
    Dimension {
        /// The dimension, when it is an alias.
        dim: Option<Dim>,
    },
}

/// One statically resolved `Rssn.<op>` call (PLAN D1).
///
/// The checker picks the overload; the runtime must execute this key and never
/// resolve again, or a program would run a different overload than it was
/// typed against.
#[derive(Clone, Debug)]
pub struct RssnCall {
    /// Where the call is written.
    pub span: Span,
    /// The operator name.
    pub name: String,
    /// The resolved overload key.
    pub key: aevia_rssn::OpKey,
    /// The index of the overload in the manifest, for diagnostics.
    pub overload: usize,
}

/// One statically resolved class-method call.
///
/// `aevia-core` must dispatch `method` on `head` to the body of the instance
/// this record names, instead of to a global of the same name.
#[derive(Clone, Debug)]
pub struct DictionaryUse {
    /// Where the call is written.
    pub span: Span,
    /// The class.
    pub class: String,
    /// The instance head the checker proved.
    pub head: Type,
    /// The method that was called.
    pub method: String,
}

/// The interface of a checked module.
#[derive(Clone, Debug, Default)]
pub struct CheckedModule {
    /// The module path, when declared.
    pub name: Option<Vec<String>>,
    /// Exported names.
    pub exports: IndexMap<String, Export>,
    /// The unit table after user declarations.
    pub units: UnitTable,
    /// The dimension environment after user declarations.
    pub dims: DimEnv,
    /// Names of `test` declarations, in order.
    pub tests: Vec<String>,
    /// `Rssn.<op>` calls with the overload the checker resolved.
    pub rssn_calls: Vec<RssnCall>,
    /// Class-method calls with the instance the checker proved.
    pub dictionaries: Vec<DictionaryUse>,
    /// Transparent type aliases, by name.
    ///
    /// A record alias has no nominal tag at run time, so an instance head
    /// spelled `Vec2` and the checker's record `{ x: Float, y: Float }` are
    /// the same head written two ways. The runtime needs the definition to
    /// match them; without it an instance on an alias is unreachable.
    pub aliases: IndexMap<String, Type>,
    /// The resolved type of each expression, keyed by its span.
    ///
    /// Every expression the checker visits has an entry: the type `synth`
    /// produced for it, or the type `check` checked it against. A `let` or
    /// parameter binder has an entry too, at the binder's own span, because
    /// that is the same type its uses get and a hover on the binder is the
    /// same question. The entries are recorded with unification variables
    /// intact and resolved once, at the end of the module, after defaulting —
    /// so an entry never names a variable that a later step bound to
    /// something else.
    ///
    /// This is what lets a language server answer "what is the type of *this*
    /// expression". [`Self::exports`] answers the same question for a name.
    pub expr_types: HashMap<Span, Type>,
    /// Number of errors.
    pub errors: usize,
}

/// The result of checking a file.
pub struct CheckResult {
    /// Diagnostics, in source order.
    pub diagnostics: Diagnostics,
    /// The interface, even when there were errors (partial).
    pub module: CheckedModule,
}

/// Checks one parsed module.
#[must_use]
pub fn check_module(file: FileId, module: &Module) -> CheckResult {
    check_module_with_imports(file, module, &[])
}

/// An imported module: the prefix its names are reachable under, and its
/// exports.
pub type Import = (String, IndexMap<String, Export>);

/// Lexes nothing, checks one module with the names of its imports in scope.
///
/// `imports` holds one entry per `import`/`open` declaration that resolved. A
/// qualified import contributes `prefix.name`; an `open` contributes the bare
/// name as well, which is why the caller decides the prefix.
pub fn check_module_with_imports(
    file: FileId,
    module: &Module,
    imports: &[Import],
) -> CheckResult {
    let mut checker = Checker::new(file);
    checker.add_imports(imports);
    checker.run(module);
    checker.checked.aliases = checker
        .aliases
        .iter()
        .map(|(name, def)| (name.clone(), def.ty.clone()))
        .collect();
    CheckResult {
        diagnostics: checker.diagnostics,
        module: checker.checked,
    }
}

struct Checker {
    file: FileId,
    infer: Infer,
    globals: HashMap<String, Scheme>,
    fn_signatures: HashMap<String, Type>,
    scopes: Vec<HashMap<String, Scheme>>,
    units: UnitTable,
    dims: DimEnv,
    aliases: HashMap<String, AliasDef>,
    adts: HashMap<String, adt::AdtId>,
    type_params: Vec<(String, Type)>,
    dim_params: Vec<(String, Dim)>,
    /// The type a top-level `let` was given in pass B, so pass C checks
    /// against the same variable a forward reference sees (§7.1.2).
    top_lets: HashMap<String, Type>,
    /// Names declared by this compilation unit, for the orphan rule.
    local_adts: HashSet<adt::AdtId>,
    classes: ClassTable,
    /// Constraints whose type was still unknown when they were generated.
    deferred_constraints: Vec<(String, Type, Span)>,
    /// True while converting an `instance` head, where an unknown type or
    /// dimension name is an implicitly quantified variable (§3.8.1).
    implicit_params: bool,
    /// The alias being converted, so a self-reference is `E0312` rather than
    /// "unknown type".
    declaring_alias: Option<String>,
    /// Set when a `Real`/`Float` mix forced the exact path (`W0302`).
    mixed_rep: bool,
    /// Top-level names that some body actually read, for `W0303`.
    referenced: HashSet<String>,
    /// `(span, type)` for every expression and binder, in the order the
    /// checker visited them.
    ///
    /// Kept as a vector while checking — a push is cheaper than a hash insert
    /// and a speculative branch (`overloaded_call`, `det`) can truncate it —
    /// and collapsed into [`CheckedModule::expr_types`] at the end, resolved.
    /// A span may be recorded twice (a call argument is both synthesised and
    /// checked); the last record is the authoritative one.
    expr_types: Vec<(Span, Type)>,
    diagnostics: Diagnostics,
    checked: CheckedModule,
    return_type: Option<Type>,
}

#[derive(Clone)]
struct AliasDef {
    ty: Type,
}

impl Checker {
    fn new(file: FileId) -> Self {
        let mut checker = Self {
            file,
            infer: Infer::new(),
            globals: HashMap::new(),
            fn_signatures: HashMap::new(),
            scopes: Vec::new(),
            units: aevia_rssn::unit_table(),
            dims: DimEnv::si(),
            aliases: HashMap::new(),
            adts: HashMap::new(),
            type_params: Vec::new(),
            dim_params: Vec::new(),
            top_lets: HashMap::new(),
            local_adts: HashSet::new(),
            classes: ClassTable::prelude(),
            deferred_constraints: Vec::new(),
            implicit_params: false,
            declaring_alias: None,
            mixed_rep: false,
            referenced: HashSet::new(),
            expr_types: Vec::with_capacity(256),
            diagnostics: Diagnostics::new(),
            checked: CheckedModule::default(),
            return_type: None,
        };
        checker.register_prelude();
        checker
    }

    /// Records that an unqualified import brought `key` in, and reports a
    /// clash when a *different* module already brought a different type.
    ///
    /// Opening the same module twice is idempotent (§7.2.1), so an identical
    /// scheme is not a clash.
    fn note_opened(
        &mut self,
        opened: &mut Vec<(String, Type)>,
        prefix: &str,
        key: &str,
        scheme: &Scheme,
    ) {
        if !prefix.is_empty() {
            return;
        }
        if let Some((_, previous)) = opened.iter().find(|(name, _)| name == key) {
            if previous != &scheme.ty {
                self.error_help(
                    "E0603",
                    format!("`{key}` is opened by two modules"),
                    Span::default(),
                    "qualify the name, or import the modules without `open`",
                );
            }
            return;
        }
        opened.push((key.to_string(), scheme.ty.clone()));
    }

    /// Puts imported names in scope.
    ///
    /// An imported value is a global with a dotted name, which is exactly how
    /// `A.B.f` resolves: the qualified form is one key, not a nested lookup.
    fn add_imports(&mut self, imports: &[Import]) {
        // Names an earlier import in this call already brought in, with the
        // type it gave them: a second, different one is ambiguous (§7.2.2).
        let mut opened: Vec<(String, Type)> = Vec::new();
        for (prefix, exports) in imports {
            for (name, export) in exports {
                let key = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                match export {
                    Export::Value { scheme, .. } | Export::Ctor { scheme, .. } => {
                        self.note_opened(&mut opened, prefix, &key, scheme);
                        self.globals.insert(key.clone(), scheme.clone());
                    }
                    Export::TypeCtor { adt, scheme, .. } => {
                        self.note_opened(&mut opened, prefix, &key, scheme);
                        self.globals.insert(key.clone(), scheme.clone());
                        self.adts.insert(key.clone(), *adt);
                    }
                    Export::Unit { def } => {
                        // The unit is usable both qualified and, after
                        // `import { km }`, unqualified. A qualified spelling
                        // (`V.Imp2.smoot`) is a unit *name*, so it has to be
                        // in the table, not just in the value namespace.
                        let mut definition = def.clone();
                        if prefix.is_empty() {
                            if self.units.lookup(&definition.name).is_none() {
                                let _ = self.units.insert(definition.clone());
                            }
                        } else if self.units.lookup(&key).is_none() {
                            definition.name = key.clone();
                            definition.aliases.clear();
                            let _ = self.units.insert(definition);
                        }
                        self.globals
                            .insert(key.clone(), Scheme::mono(Type::UnitOf(def.dim.clone())));
                    }
                    Export::Type { adt } => {
                        self.adts.insert(key.clone(), *adt);
                    }
                    Export::Alias { .. } => {}
                    Export::Dimension { dim: Some(dim) } => {
                        self.dims.define_alias(&key, dim.clone());
                    }
                    Export::Dimension { dim: None } => {
                        let _ = self.dims.define_base(&key);
                    }
                }
            }
        }
    }

    fn register_prelude(&mut self) {
        // Unit values are visible unqualified, mirroring `open Units.SI`.
        for unit in self.units.list().to_vec() {
            let ty = Type::UnitOf(unit.dim.clone());
            self.globals
                .insert(unit.name.clone(), Scheme::mono(ty.clone()));
            for alias in &unit.aliases {
                self.globals.insert(alias.clone(), Scheme::mono(ty.clone()));
            }
        }
        // `None`, `Ok`, ... are in the prelude, so every module can see them
        // without an import. They are *not* part of any module's interface:
        // exporting them would make two `open`s look like a name clash.
        for name in ["None", "Some", "Ok", "Err", "Less", "Equal", "Greater"] {
            if let Some((adt, index, _)) = adt::find_prelude_ctor(name) {
                self.globals
                    .insert(name.to_string(), ctor_scheme(adt, index));
            }
        }
        // §3.8.4: `show`, `compare` and friends are class methods, not
        // unconstrained builtins. `show(f)` for a function is `E0310`.
        for (class, method, ty) in prelude_class_methods() {
            let scheme = crate::class::method_scheme(class, 1, ty);
            self.globals.insert(method.to_string(), scheme);
        }
    }

    fn run(&mut self, module: &Module) {
        self.checked.name = module.name.clone();

        // Pass A: types, dimensions and units.
        //
        // Units are collected before anything else resolves a unit literal, so
        // a declaration may be written after its first use (§7.1.2).
        for decl in &module.decls {
            match &decl.kind {
                DeclKind::Dimension { name, value } => {
                    self.declare_dimension(name, value.as_ref(), decl.span);
                }
                DeclKind::Unit { .. } => self.declare_unit(decl),
                DeclKind::Type { name, params, body } => {
                    self.declare_type(name, params, body, decl.private);
                }
                DeclKind::Class {
                    name,
                    params,
                    methods,
                } => self.declare_class(name, params, methods, decl.span),
                _ => {}
            }
        }

        // Pass B: signatures at level 1, so they can be generalised at level 0.
        self.infer.enter_level();
        let mut pending: Vec<(String, Span)> = Vec::new();
        let mut seen_fns: HashSet<String> = HashSet::new();
        for (index, decl) in module.decls.iter().enumerate() {
            match &decl.kind {
                DeclKind::Fn {
                    name, params, args, ret, ..
                } => {
                    if !seen_fns.insert(name.clone()) {
                        self.error_help(
                            "E0316",
                            format!("`{name}` is defined more than once"),
                            decl.span,
                            "rename one of the two functions",
                        );
                    }
                    let ty = self.fn_signature(params, args, ret.as_ref());
                    self.fn_signatures.insert(name.clone(), ty.clone());
                    let scheme = Scheme::mono(ty);
                    self.globals.insert(name.clone(), scheme.clone());
                    // §7.1.3: `private` keeps a name out of the interface.
                    if !decl.private {
                        self.checked.exports.insert(
                            name.clone(),
                            Export::Value {
                                scheme,
                                decl: index,
                                arity: args.len(),
                            },
                        );
                    }
                    pending.push((name.clone(), decl.span));
                }
                DeclKind::Let { pattern, ty, .. } => {
                    let PatKind::Var(name) = &pattern.kind else {
                        continue;
                    };
                    // One variable for both passes: a forward reference and the
                    // definition must see the same type (§7.1.2).
                    let value_ty = match ty {
                        Some(ty) => self.convert_type(ty),
                        None => self.infer.fresh_type(),
                    };
                    self.top_lets.insert(name.clone(), value_ty.clone());
                    let scheme = Scheme::mono(value_ty);
                    self.globals.insert(name.clone(), scheme.clone());
                    if !decl.private {
                        self.checked.exports.insert(
                            name.clone(),
                            Export::Value {
                                scheme,
                                decl: index,
                                arity: 0,
                            },
                        );
                    }
                    pending.push((name.clone(), decl.span));
                }
                _ => {}
            }
        }

        // Pass C: bodies.
        for decl in &module.decls {
            match &decl.kind {
                DeclKind::Fn {
                    name, args, body, ..
                } => self.check_fn_body(name, args, body),
                DeclKind::Let {
                    pattern,
                    ty,
                    value,
                    rec,
                } => self.check_top_let_body(pattern, ty.as_ref(), value, *rec, decl.span),
                DeclKind::Test { name, body } => {
                    let ty = self.synth(body);
                    self.expect_bool(&ty, body.span);
                    self.checked.tests.push(name.clone());
                }
                DeclKind::Instance { .. } => self.check_instance(decl),
                DeclKind::Derive { .. } => self.check_derive(decl),
                _ => {}
            }
        }

        // Pass D: generalise every top-level binding, then default what is
        // left. Defaulting *after* generalisation is what keeps `<d>`
        // polymorphic functions polymorphic (PLAN D4, §3.10.2).
        self.infer.leave_level();
        for (name, _) in &pending {
            let Some(scheme) = self.globals.get(name).cloned() else {
                continue;
            };
            let generalised = self.generalize_scheme(&scheme);
            self.globals.insert(name.clone(), generalised.clone());
            if let Some(Export::Value { scheme, .. }) = self.checked.exports.get_mut(name) {
                *scheme = generalised;
            }
        }
        // A constraint whose type is one of this binding's quantified
        // variables belongs to the binding's scheme (§3.2.5): that is how
        // `fn f(x) = show(x)` becomes `Show a => a -> String`.
        for (name, _) in &pending {
            let Some(scheme) = self.globals.get(name).cloned() else {
                continue;
            };
            if scheme.vars.is_empty() {
                continue;
            }
            let mut attached: Vec<Constraint> = Vec::new();
            self.deferred_constraints.retain(|(class, ty, _)| {
                let resolved = self.infer.resolve(ty);
                if mentions_any(&resolved, &scheme.vars) {
                    attached.push(Constraint::new(class.clone(), vec![resolved]));
                    false
                } else {
                    true
                }
            });
            if attached.is_empty() {
                continue;
            }
            let mut combined = scheme.constraints.clone();
            for constraint in attached {
                if !combined.contains(&constraint) {
                    combined.push(constraint);
                }
            }
            let constrained = scheme.constrained(combined);
            self.globals.insert(name.clone(), constrained.clone());
            if let Some(Export::Value { scheme, .. }) = self.checked.exports.get_mut(name) {
                *scheme = constrained;
            }
        }
        for (name, span) in &pending {
            let Some(scheme) = self.globals.get(name).cloned() else {
                continue;
            };
            let (defaulted, warnings) = self.infer.default_scheme(&scheme);
            for _ in warnings {
                self.warn(
                    "W0301",
                    format!("the dimension of `{name}` is unresolved; it defaults to 1"),
                    *span,
                );
            }
            self.globals.insert(name.clone(), defaulted.clone());
            if let Some(Export::Value { scheme, .. }) = self.checked.exports.get_mut(name) {
                *scheme = defaulted;
            }
        }
        // A constraint that only ever appeared in a discarded expression still
        // has a literal type to default: `show(1)` is `Show Int`.
        for index in 0..self.deferred_constraints.len() {
            let ty = self.deferred_constraints[index].1.clone();
            let (defaulted, _) = self.infer.default_type(&ty);
            self.deferred_constraints[index].1 = defaulted;
        }

        // §B.12: a symbol nobody reads is dead weight.
        for decl in &module.decls {
            if let DeclKind::Let {
                pattern, value, ..
            } = &decl.kind
            {
                let PatKind::Var(name) = &pattern.kind else {
                    continue;
                };
                if self.referenced.contains(name) || decl.private {
                    continue;
                }
                if is_symbol_factory(value) {
                    self.warn(
                        "W0303",
                        format!("the symbol `{name}` is never used"),
                        decl.span,
                    );
                }
            }
        }

        // Every constraint whose type became known is solved; the rest is
        // `E0310` (§3.8.2 rule 3).
        self.flush_deferred_constraints();
        // A bound that the final substitution violates is a type error: the
        // annotation, not the literal, is what is wrong.
        for (got, bound) in self.infer.rep_conflicts() {
            self.diagnostics.push(
                Diagnostic::error(
                    "E0302",
                    "type mismatch",
                    Span::new(self.file, 0, 0),
                )
                .with_primary_label(format!(
                    "this value is {}, but the context needs at least {}",
                    got.name(),
                    bound.name()
                ))
                .with_help("annotate the binding with the wider representation"),
            );
        }

        if self.mixed_rep {
            self.warn(
                "W0302",
                "mixing `Real` and `Float` uses the exact `Real` path",
                Span::new(self.file, 0, 0),
            );
        }

        // The runtime resolves unit literals through this table, so it has to
        // travel with the checked module — *after* the user's declarations.
        self.checked.units = self.units.clone();
        self.checked.dims = self.dims.clone();
        for dictionary in &mut self.checked.dictionaries {
            // The head is recorded when the call is checked; a structural
            // match may already have found the instance, so its literal
            // arguments are only defaulted here.
            let (defaulted, _) = self.infer.default_type(&dictionary.head);
            dictionary.head = self.infer.resolve(&defaulted);
        }
        // The per-expression table is resolved *last*, after every unification
        // and every defaulting step: an entry recorded while checking holds
        // unification variables, and this is the only point at which their
        // final bindings are known. Resolving earlier would freeze the
        // instantiation a call happened to have at the moment it was checked.
        let recorded = std::mem::take(&mut self.expr_types);
        self.checked.expr_types = recorded
            .into_iter()
            .map(|(span, ty)| (span, self.infer.resolve(&ty)))
            .collect();
        self.checked.errors = self.diagnostics.error_count();
    }

    /// Records the type of the expression or binder at `span`.
    ///
    /// The type is stored unresolved; [`Self::run`] resolves the whole table
    /// once checking is over. A zero-width span is a synthetic node the parser
    /// invented, and `Error` is the checker's "inference already failed" — an
    /// entry for either would look like information without being any.
    ///
    /// `check` records the type it checked a node against right after `synth`
    /// recorded the type it produced for the same node (they have just been
    /// unified, so either is the answer). Replacing the previous entry instead
    /// of pushing a second one keeps the table one entry per span and saves a
    /// clone on every checked expression.
    fn note_type(&mut self, span: Span, ty: &Type) {
        if span.start >= span.end || matches!(ty, Type::Error) {
            return;
        }
        if let Some((last, recorded)) = self.expr_types.last_mut() {
            if *last == span {
                *recorded = ty.clone();
                return;
            }
        }
        self.expr_types.push((span, ty.clone()));
    }

    /// Generalises a scheme, keeping the class constraints of the declaration
    /// it came from.
    fn generalize_scheme(&mut self, scheme: &Scheme) -> Scheme {
        self.infer
            .generalize_with(&scheme.ty, scheme.constraints.clone())
    }

    // ----- declarations --------------------------------------------------

    fn declare_dimension(&mut self, name: &str, value: Option<&DimAst>, span: Span) {
        match value {
            Some(ast) => {
                let dim = self.convert_dim(ast, span);
                self.dims.define_alias(name, dim.clone());
                self.checked
                    .exports
                    .insert(name.to_string(), Export::Dimension { dim: Some(dim) });
            }
            None => {
                if !self.dims.define_base(name) {
                    self.error(
                        "E0316",
                        format!("dimension `{name}` is already defined"),
                        span,
                    );
                    return;
                }
                self.checked
                    .exports
                    .insert(name.to_string(), Export::Dimension { dim: None });
            }
        }
    }

    fn declare_unit(&mut self, decl: &Decl) {
        let DeclKind::Unit {
            name,
            dim,
            scale,
            base,
            offset,
            prefixable,
        } = &decl.kind
        else {
            return;
        };
        let dimension = self.convert_dim(dim, decl.span);
        let mut scale_value = scale.clone();
        if let Some(base) = base {
            match self.units.resolve(base) {
                Ok(resolved) => scale_value = scale_value.mul(&resolved.scale),
                Err(error) => {
                    self.unit_error(error, decl.span);
                    return;
                }
            }
        }
        let offset_value = match offset {
            Some((offset_scale, offset_unit)) => {
                let mut value = offset_scale.clone();
                if let Some(unit) = offset_unit {
                    match self.units.resolve(unit) {
                        Ok(resolved) => {
                            // §5.1.1: the offset must measure the same
                            // dimension as the unit it is attached to.
                            if resolved.dim.is_concrete()
                                && dimension.is_concrete()
                                && resolved.dim != dimension
                            {
                                let offset_dim = self.dims.format(&resolved.dim);
                                let unit_dim = self.dims.format(&dimension);
                                self.error_help(
                                    "E0504",
                                    format!(
                                        "the offset measures {offset_dim} but the unit measures {unit_dim}"
                                    ),
                                    decl.span,
                                    "an affine offset must have the unit's own dimension",
                                );
                                return;
                            }
                            value = value.mul(&resolved.scale);
                        }
                        Err(error) => {
                            self.unit_error(error, decl.span);
                            return;
                        }
                    }
                }
                Some(value)
            }
            None => None,
        };
        let def = UnitDef {
            name: name.clone(),
            aliases: Vec::new(),
            dim: dimension.clone(),
            scale: scale_value,
            offset: offset_value.clone(),
            prefixable: *prefixable,
            affine: offset_value.is_some(),
            display: name.clone(),
            category: "user".to_string(),
        };
        if let Err(error) = self.units.insert(def.clone()) {
            self.unit_error(error, decl.span);
            return;
        }
        self.globals
            .insert(name.clone(), Scheme::mono(Type::UnitOf(dimension)));
        if !decl.private {
            self.checked
                .exports
                .insert(name.clone(), Export::Unit { def });
        }
    }

    fn declare_type(
        &mut self,
        name: &str,
        params: &[TypeParam],
        body: &TypeBody,
        private: bool,
    ) {
        let saved_types = std::mem::take(&mut self.type_params);
        let saved_dims = std::mem::take(&mut self.dim_params);
        for (index, param) in params.iter().enumerate() {
            let param_ty = Type::Param(u32::try_from(index).unwrap_or(0));
            self.type_params.push((param.name.clone(), param_ty));
        }
        if self.adts.contains_key(name) {
            self.error(
                "E0316",
                format!("type `{name}` is already defined in this module"),
                Span::new(self.file, 0, 0),
            );
        }
        match body {
            TypeBody::Alias(ty) => {
                // An alias is transparent, so referring to itself is an
                // infinite type, not a recursive one (§3.6.4).
                let saved = self.declaring_alias.replace(name.to_string());
                let converted = self.convert_type(ty);
                self.declaring_alias = saved;
                self.aliases
                    .insert(name.to_string(), AliasDef { ty: converted });
                self.checked
                    .exports
                    .insert(name.to_string(), Export::Alias { params: params.len() });
            }
            TypeBody::Variants(ctors) => {
                // Allocate the id *before* converting the fields, so a field
                // that mentions the type being declared resolves (§3.6.4).
                let id = adt::allocate(
                    name,
                    params.iter().map(|p| p.name.clone()).collect(),
                );
                self.adts.insert(name.to_string(), id);
                self.local_adts.insert(id);
                let mut converted = Vec::new();
                for ctor in ctors {
                    let fields: Vec<Type> = ctor
                        .args
                        .iter()
                        .map(|field| self.convert_type(field))
                        .collect();
                    for (field_ty, written) in fields.iter().zip(ctor.args.iter()) {
                        self.check_positivity(id, field_ty, true, written.span);
                    }
                    converted.push(Ctor {
                        name: ctor.name.clone(),
                        fields,
                    });
                }
                adt::set_ctors(id, converted);
                if !private {
                    self.checked
                        .exports
                        .insert(name.to_string(), Export::Type { adt: id });
                }
                if let Some(registered) = adt::get(id) {
                    for (index, ctor) in registered.ctors.iter().enumerate() {
                        let scheme = ctor_scheme(id, index);
                        self.globals.insert(ctor.name.clone(), scheme.clone());
                        // `type P2 = | P2(...)` names the type and the
                        // constructor the same; keep both, or one of the two
                        // meanings disappears for importers.
                        let export = if ctor.name == *name {
                            Export::TypeCtor {
                                adt: id,
                                index,
                                scheme,
                            }
                        } else {
                            Export::Ctor {
                                adt: id,
                                index,
                                scheme,
                            }
                        };
                        self.checked.exports.insert(ctor.name.clone(), export);
                    }
                }
            }
            TypeBody::Abstract => {
                let id = adt::allocate(
                    name,
                    params.iter().map(|p| p.name.clone()).collect(),
                );
                self.adts.insert(name.to_string(), id);
                self.local_adts.insert(id);
                if !private {
                    self.checked
                        .exports
                        .insert(name.to_string(), Export::Type { adt: id });
                }
            }
        }
        self.type_params = saved_types;
        self.dim_params = saved_dims;
    }

    /// Rejects a recursive occurrence on the left of an arrow (§3.6.4).
    ///
    /// `type Bad = Bad -> Int` has no well-founded value: checking it is
    /// `E0312`. `type Tree<a> = | Node(Tree<a>, a, Tree<a>)` is positive and
    /// therefore legal.
    fn check_positivity(&mut self, id: adt::AdtId, ty: &Type, positive: bool, span: Span) {
        match ty {
            Type::Adt(other, args) => {
                if *other == id && !positive {
                    self.error_help(
                        "E0312",
                        "non-positive recursive type",
                        span,
                        "the type may only appear to the right of an arrow",
                    );
                }
                for arg in args {
                    self.check_positivity(id, arg, positive, span);
                }
            }
            Type::Tensor { elem, .. } => self.check_positivity(id, elem, positive, span),
            Type::Fn(params, ret) => {
                for param in params {
                    self.check_positivity(id, param, false, span);
                }
                self.check_positivity(id, ret, positive, span);
            }
            Type::Record(row) => {
                for (_, field) in row.fields() {
                    self.check_positivity(id, &field, positive, span);
                }
            }
            Type::Io(inner) => self.check_positivity(id, inner, positive, span),
            _ => {}
        }
    }

    fn declare_class(
        &mut self,
        name: &str,
        params: &[TypeParam],
        methods: &[(String, Ty)],
        span: Span,
    ) {
        let saved_types = std::mem::take(&mut self.type_params);
        let saved_dims = std::mem::take(&mut self.dim_params);
        for (index, param) in params.iter().enumerate() {
            // `Param`, not `Var`: a class parameter is a binder, and an
            // instance substitutes the head for it with `substitute_params`.
            // `method_scheme` converts it to a quantified `Var` when the
            // method becomes a scheme.
            self.type_params.push((
                param.name.clone(),
                Type::Param(u32::try_from(index).unwrap_or(0)),
            ));
        }
        let mut converted = Vec::new();
        for (method, ty) in methods {
            let method_ty = self.convert_type(ty);
            converted.push((method.clone(), method_ty.clone()));
            let scheme = crate::class::method_scheme(name, params.len().max(1), method_ty);
            self.globals.insert(method.clone(), scheme);
        }
        self.type_params = saved_types;
        self.dim_params = saved_dims;
        if let Err(code) = self.classes.declare_class(name, params.len().max(1), converted) {
            self.error(code, format!("class `{name}` is already declared"), span);
        }
    }

    /// Checks an `instance` and registers it (§3.8.1).
    ///
    /// The methods are *not* installed as globals of their own name: that is
    /// how the old checker let one instance shadow `show` for the whole module.
    /// Each method gets a mangled global, and the call site records which
    /// instance the checker proved.
    fn check_instance(&mut self, decl: &Decl) {
        let DeclKind::Instance {
            class,
            ty: head_ast,
            methods,
        } = &decl.kind
        else {
            return;
        };
        let head = self.convert_instance_head(head_ast);
        let Some(definition) = self.classes.class(class).cloned() else {
            self.error_help(
                "E0310",
                format!("no class `{class}` is in scope"),
                decl.span,
                "declare it with `class ... where`, or import the module that does",
            );
            return;
        };
        let mut specialised: HashMap<String, Type> = HashMap::new();
        let mut missing: Vec<String> = Vec::new();
        for (method, declared) in &definition.methods {
            let Some(instance_method) = methods.iter().find(|m| m.name == *method) else {
                missing.push(method.clone());
                continue;
            };
            // The instance's method must have the class method's type with the
            // head substituted for the class parameter.
            let expected = declared.substitute_params(&[head.clone()]);
            self.scopes.push(HashMap::new());
            let mut params = Vec::new();
            let Type::Fn(expected_params, expected_ret) = self.infer.resolve(&expected) else {
                self.scopes.pop();
                continue;
            };
            for (index, param) in instance_method.params.iter().enumerate() {
                let ty = match (&param.ty, expected_params.get(index)) {
                    (Some(annotation), _) => {
                        let annotated = self.convert_type(annotation);
                        if let Some(want) = expected_params.get(index) {
                            if let Err(error) = self.infer.unify(&annotated, want) {
                                self.report_unify(error, param.span);
                            }
                        }
                        annotated
                    }
                    (None, Some(want)) => want.clone(),
                    (None, None) => self.infer.fresh_type(),
                };
                self.bind_pattern(&param.pattern, &ty);
                params.push(ty);
            }
            let body_ty = self.synth(&instance_method.body);
            if let Err(error) = self.infer.unify(&body_ty, &expected_ret) {
                self.report_unify(error, instance_method.span);
            }
            self.scopes.pop();
            let method_ty = Type::func(params, (*expected_ret).clone());
            specialised.insert(method.clone(), method_ty.clone());
            // The body is reachable under a name no program can write, so it
            // cannot shadow the class method.
            let mangled = format!("{class}@{}::{method}", head);
            self.globals.insert(mangled, Scheme::mono(method_ty));
        }
        if !missing.is_empty() {
            self.error_help(
                "E0317",
                format!(
                    "this `{class}` instance does not define {}",
                    missing.join(", ")
                ),
                decl.span,
                "every method of the class must be defined",
            );
        }
        let head_is_local = self.head_is_local_ast(head_ast);
        match self
            .classes
            .add_instance(class, head, specialised, Origin::User, head_is_local)
        {
            Ok(()) => {}
            Err(InstanceError::Overlapping) => self.error_help(
                "E0311",
                format!("overlapping `{class}` instances"),
                decl.span,
                "two instances match the same type; merge or delete one",
            ),
            Err(InstanceError::Orphan) => self.error_help(
                "E0604",
                format!("orphan instance: neither `{class}` nor this type is local"),
                decl.span,
                "define the instance in the module that declares the type or the class",
            ),
            Err(InstanceError::UnknownClass) => {}
        }
    }

    /// Whether an instance head names a type this module declares.
    ///
    /// Aliases are transparent, so the written name is what decides: `Vec2`
    /// declared here is local even though its expansion is a record type.
    fn head_is_local_ast(&self, ty: &Ty) -> bool {
        match &ty.kind {
            TyKind::Named { path, .. } => {
                let name = path.join(".");
                self.adts
                    .get(&name)
                    .is_some_and(|id| self.local_adts.contains(id))
                    || self.aliases.contains_key(&name)
            }
            TyKind::Record { .. } => true,
            _ => false,
        }
    }

    /// Converts an instance head, treating unknown type and dimension names as
    /// implicitly quantified variables (§3.8.1).
    fn convert_instance_head(&mut self, ty: &Ty) -> Type {
        let saved_types = std::mem::take(&mut self.type_params);
        let saved_dims = std::mem::take(&mut self.dim_params);
        let saved_flag = self.implicit_params;
        self.implicit_params = true;
        let converted = self.convert_type(ty);
        self.implicit_params = saved_flag;
        self.type_params = saved_types;
        self.dim_params = saved_dims;
        converted
    }

    /// Checks a `derive` declaration (§3.8.5).
    fn check_derive(&mut self, decl: &Decl) {
        let DeclKind::Derive { classes, ty } = &decl.kind else {
            return;
        };
        let head = self.convert_type(ty);
        // A `derive` works on a nominal type or on a record alias: both are
        // structurally known here.
        let fields: Vec<Type> = match &head {
            Type::Adt(id, _) => match adt::get(*id) {
                Some(registered) => registered
                    .ctors
                    .iter()
                    .flat_map(|ctor| ctor.fields.clone())
                    .collect(),
                None => Vec::new(),
            },
            Type::Record(row) => row.fields().into_iter().map(|(_, ty)| ty).collect(),
            _ => {
                self.error_help(
                    "E0317",
                    "`derive` needs a nominal type or a record type",
                    decl.span,
                    "deriving works on `type` declarations",
                );
                return;
            }
        };
        for class in classes {
            if let Err(code) = self.classes.add_derived(class, head.clone(), &fields) {
                let message = if code == "E0310" {
                    format!("no class `{class}` is in scope")
                } else if code == "E0311" {
                    format!("an instance of `{class}` for this type already exists")
                } else {
                    format!("cannot derive `{class}` for this type")
                };
                self.error_help(
                    code,
                    message,
                    decl.span,
                    "write the instance by hand if a field has no instance of its own",
                );
            }
        }
    }

    /// Solves a class constraint, or defers it until the type is known.
    fn solve_constraint(&mut self, constraint: &Constraint, span: Span) {
        let Some(arg) = constraint.args.first() else {
            return;
        };
        let resolved = self.infer.resolve(arg);
        match self.classes.resolve(&constraint.class, &resolved) {
            Resolution::Found { .. } => {}
            Resolution::Deferred => self
                .deferred_constraints
                .push((constraint.class.clone(), resolved, span)),
            Resolution::Missing => {
                let heads = self.classes.instance_heads(&constraint.class);
                let known = heads
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                self.diagnostics.push(
                    Diagnostic::error(
                        "E0310",
                        format!(
                            "no instance of `{}` for `{resolved}`",
                            constraint.class
                        ),
                        span,
                    )
                    .with_primary_label("this type has no instance")
                    .with_note(if known.is_empty() {
                        "the class has no instances".to_string()
                    } else {
                        format!("instances: {known}")
                    })
                    .with_help("add an `instance` declaration, or use a type that has one"),
                );
            }
        }
    }

    /// `to` and `value_in` with a literal unit argument.
    ///
    /// Returns `None` when the call does not have the shape this rule covers,
    /// so the builtin signature still applies.
    fn unit_display_call(&mut self, name: &str, args: &[Expr], span: Span) -> Option<Type> {
        let [unit_expr, value] = args else {
            return None;
        };
        let unit_name = unit_name_of(unit_expr)?;
        let unit = self.units.lookup(&unit_name)?;
        let synth_value = self.synth(value);
        let value_ty = self.infer.resolve(&synth_value);
        match value_ty {
            Type::Point(dim) => {
                if dim.is_concrete() && unit.dim.is_concrete() && dim != unit.dim {
                    self.error_help(
                        "E0509",
                        format!("`{unit_name}` measures a different dimension"),
                        span,
                        "convert the value to the unit's dimension first",
                    );
                    return Some(Type::Error);
                }
                Some(if name == "to" {
                    Type::Point(dim)
                } else {
                    Type::float(Dim::one())
                })
            }
            Type::Scalar(rep, dim) => {
                if unit.affine {
                    self.error_help(
                        "E0510",
                        format!("`{unit_name}` is an affine unit, but this is a temperature difference"),
                        value.span,
                        "use `delta_to` to display a difference, or a `Point` for an absolute temperature",
                    );
                    return Some(Type::Error);
                }
                if dim.is_concrete() && unit.dim.is_concrete() && dim != unit.dim {
                    self.error_help(
                        "E0509",
                        format!("`{unit_name}` measures a different dimension"),
                        span,
                        "convert the value to the unit's dimension first",
                    );
                    return Some(Type::Error);
                }
                Some(if name == "to" {
                    Type::Scalar(rep, dim)
                } else {
                    Type::Scalar(rep, Dim::one())
                })
            }
            _ => None,
        }
    }

    /// Reports `E0310` when a concrete type has no instance of `class`.
    ///
    /// A type that is still unknown is re-checked at the end of the module,
    /// once defaulting has made it concrete: `sort([1.0, 2.0])` is
    /// `sort : Tensor<1, Float>` and `Float` has no `TotalOrd`.
    fn require_instance(&mut self, class: &str, ty: &Type, span: Span) {
        let resolved = self.infer.resolve(ty);
        match self.classes.resolve(class, &resolved) {
            Resolution::Missing => {
                self.error_help(
                    "E0310",
                    format!("no instance of `{class}` for `{resolved}`"),
                    span,
                    "this type has no total order; use a type that has one",
                );
            }
            Resolution::Deferred => self
                .deferred_constraints
                .push((class.to_string(), resolved, span)),
            Resolution::Found { .. } => {}
        }
    }

    /// Re-solves the constraints that were deferred, reporting `E0310` for the
    /// ones that never became concrete (§3.8.2 rule 3).
    fn flush_deferred_constraints(&mut self) {
        let deferred = std::mem::take(&mut self.deferred_constraints);
        for (class, ty, span) in deferred {
            let resolved = self.infer.resolve(&ty);
            match self.classes.resolve(&class, &resolved) {
                Resolution::Found { .. } => {}
                Resolution::Deferred | Resolution::Missing => {
                    self.error_help(
                        "E0310",
                        format!("no instance of `{class}` for `{resolved}`"),
                        span,
                        "add a type annotation that fixes the type, or an `instance` declaration",
                    );
                }
            }
        }
    }

    fn fn_signature(&mut self, params: &[TypeParam], args: &[Param], ret: Option<&Ty>) -> Type {
        let saved_types = self.type_params.clone();
        let saved_dims = self.dim_params.clone();
        for param in params {
            match param.kind {
                Some(Kind::Dim) => {
                    let dim = self.infer.fresh_dim();
                    self.dim_params.push((param.name.clone(), dim));
                }
                Some(Kind::Type) | None => {
                    let ty = self.infer.fresh_type();
                    self.type_params.push((param.name.clone(), ty));
                    // An unkinded parameter may also be used as a dimension
                    // (`fn mean<d>(xs: Real[d])`); give it both readings.
                    if param.kind.is_none() {
                        let dim = self.infer.fresh_dim();
                        self.dim_params.push((param.name.clone(), dim));
                    }
                }
                _ => {}
            }
        }
        let mut arg_types = Vec::new();
        for arg in args {
            let ty = match &arg.ty {
                Some(ty) => self.convert_type(ty),
                None => self.infer.fresh_type(),
            };
            arg_types.push(ty);
        }
        let ret_ty = match ret {
            Some(ty) => self.convert_type(ty),
            None => self.infer.fresh_type(),
        };
        self.type_params = saved_types;
        self.dim_params = saved_dims;
        Type::func(arg_types, ret_ty)
    }

    fn check_fn_body(&mut self, name: &str, args: &[Param], body: &Expr) {
        let Some(signature) = self.fn_signatures.get(name).cloned() else {
            return;
        };
        let Type::Fn(params, ret) = signature else {
            return;
        };
        self.scopes.push(HashMap::new());
        for (arg, param) in args.iter().zip(params.iter()) {
            self.bind_pattern(&arg.pattern, param);
        }
        self.return_type = Some((*ret).clone());
        self.check(body, &ret);
        self.return_type = None;
        self.scopes.pop();
        // Generalise as soon as the body is checked, for the same reason the
        // `let` path does: a later declaration must see the polymorphic type.
        // Waiting for the end of the module is too late — a call site in
        // between has already unified the shared variable, so `fn ident(x) = x`
        // came out monomorphic and `ident("s")` after `ident(1)` was an error.
        let generalised = self.infer.generalize_at(0, &Type::func(params, (*ret).clone()), Vec::new());
        self.globals.insert(name.to_string(), generalised.clone());
        if let Some(Export::Value { scheme, .. }) = self.checked.exports.get_mut(name) {
            *scheme = generalised;
        }
    }

    /// Checks the body of a top-level `let`.
    ///
    /// The type is the one pass B published, so a forward reference from
    /// another declaration constrains the *same* variable (§7.1.2). A
    /// non-`rec` binding is hidden while its own value is checked: a top-level
    /// `let` may refer to its neighbours, not to itself (§3.3.2 rule 5).
    fn check_top_let_body(
        &mut self,
        pattern: &Pat,
        annotation: Option<&Ty>,
        value: &Expr,
        rec: bool,
        span: Span,
    ) {
        let PatKind::Var(name) = &pattern.kind else {
            self.check_let_body(pattern, annotation, value, rec, span);
            return;
        };
        let target = match self.top_lets.get(name).cloned() {
            Some(target) => target,
            None => self.infer.fresh_type(),
        };
        let target = match annotation {
            Some(ty) => {
                let annotated = self.convert_type(ty);
                if let Err(error) = self.infer.unify(&target, &annotated) {
                    self.report_unify(error, ty.span);
                }
                annotated
            }
            None => target,
        };
        self.note_type(pattern.span, &target);
        if rec {
            self.check(value, &target);
        } else {
            let saved = self.globals.remove(name);
            self.check(value, &target);
            if let Some(scheme) = saved {
                self.globals.insert(name.clone(), scheme);
            }
        }
        // Generalise as soon as the body is checked, so a *later* declaration
        // that uses this one sees the polymorphic type. Defaulting still waits
        // for the end of the module (PLAN D4).
        let scheme = self.infer.generalize_at(0, &target, Vec::new());
        self.globals.insert(name.clone(), scheme.clone());
        if let Some(Export::Value {
            scheme: export_scheme,
            ..
        }) = self.checked.exports.get_mut(name)
        {
            *export_scheme = scheme;
        }
    }

    /// Checks a `let` whose name is not published by pass B.
    fn check_let_body(
        &mut self,
        pattern: &Pat,
        annotation: Option<&Ty>,
        value: &Expr,
        rec: bool,
        span: Span,
    ) {
        self.infer.enter_level();
        let target = self.infer.fresh_type();
        if let (PatKind::Var(name), true) = (&pattern.kind, rec) {
            self.globals.insert(name.clone(), Scheme::mono(target.clone()));
        }
        let target = match annotation {
            Some(ty) => {
                let annotated = self.convert_type(ty);
                if let Err(error) = self.infer.unify(&target, &annotated) {
                    self.report_unify(error, ty.span);
                }
                annotated
            }
            None => target,
        };
        self.check(value, &target);
        self.infer.leave_level();
        self.note_type(pattern.span, &target);
        let scheme = self.infer.generalize(&target);
        let (scheme, warnings) = self.infer.default_scheme(&scheme);
        for _ in warnings {
            self.warn("W0301", "unresolved dimension defaulted to 1", span);
        }
        if let PatKind::Var(name) = &pattern.kind {
            self.globals.insert(name.clone(), scheme.clone());
            if let Some(Export::Value {
                scheme: export_scheme,
                ..
            }) = self.checked.exports.get_mut(name)
            {
                *export_scheme = scheme;
            }
        } else {
            self.bind_pattern(pattern, &scheme.ty);
        }
    }

    /// Generalises and defaults a local `let` and binds its pattern.
    fn bind_local(&mut self, pattern: &Pat, value_ty: &Type, span: Span) {
        self.note_type(pattern.span, value_ty);
        let scheme = self.infer.generalize(value_ty);
        let (scheme, warnings) = self.infer.default_scheme(&scheme);
        for _ in warnings {
            self.warn("W0301", "unresolved dimension defaulted to 1", span);
        }
        if let PatKind::Var(name) = &pattern.kind {
            self.scopes
                .last_mut()
                .expect("a local binding has a scope")
                .insert(name.clone(), scheme);
        } else {
            self.bind_pattern(pattern, &scheme.ty);
        }
    }

    // ----- expressions ---------------------------------------------------

    fn synth(&mut self, expr: &Expr) -> Type {
        let ty = self.synth_inner(expr);
        self.note_type(expr.span, &ty);
        ty
    }

    fn synth_inner(&mut self, expr: &Expr) -> Type {
        match &expr.kind {
            // A parser recovery node: the syntax error was already reported, so
            // synthesise an unconstrained type instead of a second diagnostic.
            ExprKind::Error => self.infer.fresh_type(),
            ExprKind::UnitLit => Type::Unit,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Int(_) => {
                let ty = self.infer.fresh_type();
                self.infer.mark_literal(&ty, LiteralKind::Int);
                ty
            }
            ExprKind::Float(_) => {
                let ty = self.infer.fresh_type();
                self.infer.mark_literal(&ty, LiteralKind::Decimal);
                ty
            }
            ExprKind::Str(pieces) => {
                for piece in pieces {
                    if let StrPiece::Expr(inner) = piece {
                        self.synth(inner);
                    }
                }
                Type::string()
            }
            ExprKind::Quantity { value, unit } => self.quantity_type(value, unit, expr.span),
            ExprKind::Var(name) => self.lookup_value(name, expr.span),
            ExprKind::QName(path) => {
                // `Rssn.Version` and friends are kernel members, not names in
                // scope; the manifest decides what exists.
                if path.len() == 2 && path[0] == "Rssn" {
                    return self.rssn_member(&path[1], expr.span);
                }
                self.lookup_value(&path.join("."), expr.span)
            }
            ExprKind::Unary { op, operand } => self.synth_unary(*op, operand, expr.span),
            ExprKind::Binary { op, left, right } => {
                self.synth_binary(*op, left, right, expr.span)
            }
            ExprKind::Call { callee, args } => self.synth_call(callee, args, expr.span),
            ExprKind::TypeApp { callee, args } => self.synth_type_app(callee, args, expr.span),
            ExprKind::Lambda { params, ret, body } => {
                self.infer.enter_level();
                self.scopes.push(HashMap::new());
                let mut arg_types = Vec::new();
                for param in params {
                    let ty = match &param.ty {
                        Some(ty) => self.convert_type(ty),
                        None => self.infer.fresh_type(),
                    };
                    self.bind_pattern(&param.pattern, &ty);
                    arg_types.push(ty);
                }
                let ret_ty = match ret {
                    Some(ty) => self.convert_type(ty),
                    None => self.infer.fresh_type(),
                };
                self.check(body, &ret_ty);
                self.scopes.pop();
                self.infer.leave_level();
                Type::func(arg_types, ret_ty)
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_ty = self.synth(cond);
                self.expect_bool(&cond_ty, cond.span);
                let branch = self.infer.fresh_type();
                self.check(then_branch, &branch);
                self.check(else_branch, &branch);
                branch
            }
            ExprKind::Match { scrutinee, arms } => {
                let subject = self.synth(scrutinee);
                let result = self.infer.fresh_type();
                let mut covered: Vec<(adt::AdtId, usize)> = Vec::new();
                let mut covered_bool = (false, false);
                let mut has_wildcard = false;
                for arm in arms {
                    self.scopes.push(HashMap::new());
                    self.bind_pattern(&arm.pattern, &subject);
                    // §3.6.3: an arm after a `_` arm, or a second arm for the
                    // same constructor, can never be reached.
                    if arm.guard.is_none() {
                        if has_wildcard {
                            self.warn(
                                "W0201",
                                "this match arm is redundant: an earlier arm covers it",
                                arm.span,
                            );
                        }
                        match &arm.pattern.kind {
                            PatKind::Wildcard | PatKind::Var(_) => has_wildcard = true,
                            PatKind::Ctor { name, .. } => {
                                if let Type::Adt(id, _) = self.infer.resolve(&subject) {
                                    if let Some(index) = adt::ctor_index(id, name) {
                                        if covered.contains(&(id, index)) {
                                            self.warn(
                                                "W0201",
                                                format!("`{name}` is already covered"),
                                                arm.span,
                                            );
                                        }
                                        covered.push((id, index));
                                    }
                                }
                            }
                            PatKind::Lit(PatLit::Bool(true)) => {
                                if covered_bool.1 {
                                    self.warn("W0201", "`true` is already covered", arm.span);
                                }
                                covered_bool.1 = true;
                            }
                            PatKind::Lit(PatLit::Bool(false)) => {
                                if covered_bool.0 {
                                    self.warn("W0201", "`false` is already covered", arm.span);
                                }
                                covered_bool.0 = true;
                            }
                            _ => {}
                        }
                    }
                    if let Some(guard) = &arm.guard {
                        let guard_ty = self.synth(guard);
                        self.expect_bool(&guard_ty, guard.span);
                    }
                    self.check(&arm.body, &result);
                    self.scopes.pop();
                }
                self.check_exhaustive(&subject, &covered, covered_bool, has_wildcard, expr.span);
                result
            }
            ExprKind::LetIn {
                pattern,
                ty,
                value,
                body,
            } => {
                self.infer.enter_level();
                let value_ty = match ty {
                    Some(ty) => {
                        let ty = self.convert_type(ty);
                        self.check(value, &ty);
                        ty
                    }
                    None => self.synth(value),
                };
                self.infer.leave_level();
                self.scopes.push(HashMap::new());
                self.bind_local(pattern, &value_ty, expr.span);
                let body_ty = self.synth(body);
                self.scopes.pop();
                body_ty
            }
            ExprKind::Block { stmts, tail } => {
                self.scopes.push(HashMap::new());
                for stmt in stmts {
                    match stmt {
                        BlockStmt::Let {
                            pattern,
                            ty,
                            value,
                            rec,
                            span,
                        } => {
                            self.infer.enter_level();
                            let target = self.infer.fresh_type();
                            if let (PatKind::Var(name), true) = (&pattern.kind, *rec) {
                                self.scopes
                                    .last_mut()
                                    .expect("scope")
                                    .insert(name.clone(), Scheme::mono(target.clone()));
                            }
                            let value_ty = match ty {
                                Some(annotation) => {
                                    let annotated = self.convert_type(annotation);
                                    if let Err(error) = self.infer.unify(&target, &annotated) {
                                        self.report_unify(error, annotation.span);
                                    }
                                    self.check(value, &annotated);
                                    annotated
                                }
                                None => {
                                    self.check(value, &target);
                                    target
                                }
                            };
                            self.infer.leave_level();
                            self.bind_local(pattern, &value_ty, *span);
                        }
                        BlockStmt::Fn(decl) => {
                            if let DeclKind::Fn {
                                name, params, args, ret, body, ..
                            } = &decl.kind
                            {
                                let ty = self.fn_signature(params, args, ret.as_ref());
                                self.fn_signatures.insert(name.clone(), ty.clone());
                                self.scopes
                                    .last_mut()
                                    .expect("scope")
                                    .insert(name.clone(), Scheme::mono(ty));
                                self.check_fn_body(name, args, body);
                            }
                        }
                    }
                }
                let tail_ty = match tail {
                    Some(tail) => self.synth(tail),
                    None => {
                        self.error_help(
                            "E0201",
                            "block has no final expression",
                            expr.span,
                            "add a value after the `let` statements",
                        );
                        Type::Unit
                    }
                };
                self.scopes.pop();
                tail_ty
            }
            ExprKind::Do { stmts } => self.synth_do(stmts),
            ExprKind::Record { base, fields } => self.synth_record(base.as_deref(), fields),
            ExprKind::Field { base, name } => {
                if is_rssn_root(base) {
                    return self.rssn_member(name, expr.span);
                }
                // `A.B.f` parses as a field access on a qualified name; the
                // imported module puts `A.B.f` in scope as one key.
                if let Some(qualified) = qualified_name(base, name) {
                    if self.globals.contains_key(&qualified) {
                        return self.lookup_value(&qualified, expr.span);
                    }
                    // The module exists but does not export that name: say so
                    // rather than calling the whole path unbound.
                    if let Some(prefix) = qualified.rsplit_once('.').map(|(prefix, _)| prefix) {
                        let marker = format!("{prefix}.");
                        if self.globals.keys().any(|key| key.starts_with(&marker)) {
                            self.error_help(
                                "E0301",
                                format!("module `{prefix}` has no exported name `{name}`"),
                                expr.span,
                                "a `private` declaration is not part of the interface (§7.1.3)",
                            );
                            return Type::Error;
                        }
                    }
                }
                let base_ty = self.synth(base);
                self.field_type(&base_ty, name, expr.span)
            }
            ExprKind::Tuple(items) => {
                let types: Vec<Type> = items.iter().map(|item| self.synth(item)).collect();
                Type::record(
                    types
                        .into_iter()
                        .enumerate()
                        .map(|(index, ty)| (index.to_string(), ty)),
                )
            }
            ExprKind::Tensor(items) => self.synth_tensor(items, expr.span),
            ExprKind::Index { base, indices } => self.synth_index(base, indices, expr.span),
            ExprKind::Annot { expr: inner, ty } => {
                let ty = self.convert_type(ty);
                self.check(inner, &ty);
                ty
            }
            ExprKind::Placeholder => {
                self.error("E0202", "`_` is only allowed as a call argument", expr.span);
                Type::Error
            }
            ExprKind::Try(inner) => self.synth_try(inner, expr.span),
            ExprKind::Factorial { expr: inner, .. } => {
                let ty = self.synth(inner);
                let _ = self.infer.unify(&ty, &Type::int());
                Type::int()
            }
        }
    }

    fn check(&mut self, expr: &Expr, expected: &Type) {
        self.check_inner(expr, expected);
        // Whatever path `check_inner` took — a direct comparison, or a
        // bidirectional rule that never synthesised the node — the type of the
        // expression is the type it was checked against. Recorded raw and
        // resolved at the end, like every other entry.
        self.note_type(expr.span, expected);
    }

    fn check_inner(&mut self, expr: &Expr, expected: &Type) {
        let resolved_expected = self.infer.resolve(expected);
        // An empty list has no element to learn its type from; the context
        // supplies it, which is what `let e: Tensor<1, Float> = []` means.
        if matches!(&expr.kind, ExprKind::Tensor(items) if items.is_empty())
            && matches!(resolved_expected, Type::Tensor { .. })
        {
            return;
        }
        match (&expr.kind, &resolved_expected) {
            (ExprKind::Lambda { params, ret, body }, Type::Fn(expected_params, expected_ret))
                if params.len() == expected_params.len() =>
            {
                self.infer.enter_level();
                self.scopes.push(HashMap::new());
                for (param, expected_ty) in params.iter().zip(expected_params.iter()) {
                    if let Some(annotation) = &param.ty {
                        let annotated = self.convert_type(annotation);
                        if let Err(error) = self.infer.unify(&annotated, expected_ty) {
                            self.report_unify(error, param.span);
                        }
                    }
                    self.bind_pattern(&param.pattern, expected_ty);
                }
                if let Some(annotation) = ret {
                    let annotated = self.convert_type(annotation);
                    if let Err(error) = self.infer.unify(&annotated, expected_ret) {
                        self.report_unify(error, annotation_span(annotation));
                    }
                }
                self.check(body, expected_ret);
                self.scopes.pop();
                self.infer.leave_level();
                return;
            }
            (
                ExprKind::If {
                    cond,
                    then_branch,
                    else_branch,
                },
                _,
            ) => {
                let cond_ty = self.synth(cond);
                self.expect_bool(&cond_ty, cond.span);
                self.check(then_branch, expected);
                self.check(else_branch, expected);
                return;
            }
            (ExprKind::Block { stmts, tail: Some(tail) }, _) => {
                self.scopes.push(HashMap::new());
                for stmt in stmts {
                    if let BlockStmt::Let {
                        pattern,
                        ty,
                        value,
                        ..
                    } = stmt
                    {
                        let value_ty = match ty {
                            Some(ty) => {
                                let ty = self.convert_type(ty);
                                self.check(value, &ty);
                                ty
                            }
                            None => self.synth(value),
                        };
                        self.bind_pattern(pattern, &value_ty);
                    }
                }
                self.check(tail, expected);
                self.scopes.pop();
                return;
            }
            _ => {}
        }
        let actual = self.synth(expr);
        let expected_resolved = self.infer.resolve(expected);
        // §3.9.4: a pure context cannot run an action. Saying `E0313` instead
        // of a plain mismatch is what tells the user to return `IO`.
        if matches!(actual, Type::Io(_))
            && !matches!(
                expected_resolved,
                Type::Io(_) | Type::Var(_) | Type::Error
            )
        {
            self.error_help(
                "E0313",
                "this is an `IO` action in a pure context",
                expr.span,
                "return `IO` from the enclosing function, or `run_io` at the entry point",
            );
            return;
        }
        if let Err(error) = self.infer.unify(&actual, expected) {
            self.report_unify(error, expr.span);
        }
    }

    fn synth_unary(&mut self, op: UnOp, operand: &Expr, span: Span) -> Type {
        let ty = self.synth(operand);
        let resolved = match op {
            UnOp::Neg | UnOp::Pos => self.force_scalar(&ty),
            _ => self.infer.resolve(&ty),
        };
        match op {
            UnOp::Neg | UnOp::Pos => match resolved {
                Type::Scalar(..) | Type::Tensor { .. } => resolved,
                other => {
                    self.error("E0302", format!("`-` is not defined for {other}"), span);
                    Type::Error
                }
            },
            UnOp::Not => match resolved {
                Type::Bool => Type::Bool,
                Type::Prop => Type::Prop,
                other => {
                    self.error("E0302", format!("`!` expects Bool, found {other}"), span);
                    Type::Error
                }
            },
            UnOp::Sqrt => match resolved {
                Type::Scalar(rep, dim) => {
                    // §3.4.2: a root divides, so an integer becomes `Real`.
                    Type::Scalar(self.div_rep(rep), dim.pow(&BigRational::new(1.into(), 2.into())))
                }
                other => {
                    self.error("E0302", format!("`√` expects a scalar, found {other}"), span);
                    Type::Error
                }
            },
        }
    }

    fn synth_binary(&mut self, op: BinOp, left: &Expr, right: &Expr, span: Span) -> Type {
        let left_ty = self.synth(left);
        let right_ty = self.synth(right);
        self.binary_type(op, &left_ty, &right_ty, left, right, span)
    }

    /// Commits an unresolved operand to being a scalar.
    ///
    /// Arithmetic is the only place where a bare variable has an obvious
    /// reading, and leaving it unconstrained would make `2 * h / g` fail before
    /// `g`'s declaration is checked. Literals keep their dimension rule: a
    /// bare `2` may only become a dimensionless scalar.
    fn force_scalar(&mut self, ty: &Type) -> Type {
        let resolved = self.infer.resolve(ty);
        if !matches!(resolved, Type::Var(_)) {
            return resolved;
        }
        let dim = match self.infer.literal_kind(&resolved) {
            Some(kind) if !kind.is_quantity() => Dim::one(),
            _ => self.infer.fresh_dim(),
        };
        let scalar = Type::Scalar(self.infer.fresh_rep(), dim);
        let _ = self.infer.unify(&resolved, &scalar);
        self.infer.resolve(&scalar)
    }

    #[allow(clippy::too_many_arguments)]
    fn binary_type(
        &mut self,
        op: BinOp,
        left_ty: &Type,
        right_ty: &Type,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) -> Type {
        let mut left_resolved = self.infer.resolve(left_ty);
        let mut right_resolved = self.infer.resolve(right_ty);
        if matches!(
            op,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow
        ) {
            left_resolved = self.force_scalar(&left_resolved);
            if op != BinOp::Pow {
                right_resolved = self.force_scalar(&right_resolved);
            }
        }
        match op {
            BinOp::Add | BinOp::Sub => {
                self.add_type(op, &left_resolved, &right_resolved, left, right, span)
            }
            BinOp::Mul => self.mul_type(&left_resolved, &right_resolved, span),
            BinOp::Div => self.div_type(&left_resolved, &right_resolved, span),
            BinOp::MatMul => self.matmul_type(&left_resolved, &right_resolved, span),
            BinOp::Pow => self.pow_type(&left_resolved, right, span),
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                if let Err(error) = self.infer.unify(&left_resolved, &right_resolved) {
                    self.report_unify(error, span);
                    return Type::Error;
                }
                let compared = self.infer.resolve(&left_resolved);
                // §3.8.4/§3.7.1: a function has no `Eq` instance — equality on
                // functions is undecidable.
                if contains_function(&compared) {
                    self.error_help(
                        "E0310",
                        format!("functions have no `Eq` instance, found {compared}"),
                        span,
                        "compare the values a function returns, not the function",
                    );
                    return Type::Error;
                }
                match compared {
                    Type::Scalar(rep, _) if matches!(self.infer.resolve_rep(rep), Rep::Real) => {
                        // §3.8.6: comparing exact reals yields a proposition.
                        Type::Prop
                    }
                    Type::Prop => Type::Prop,
                    _ => Type::Bool,
                }
            }
            BinOp::And | BinOp::Or => match (&left_resolved, &right_resolved) {
                (Type::Bool, Type::Bool) => Type::Bool,
                (Type::Prop, Type::Prop) => Type::Prop,
                _ => {
                    self.error(
                        "E0302",
                        format!(
                            "`{}` expects Bool or Prop operands, found {left_resolved} and {right_resolved}",
                            op.as_str()
                        ),
                        span,
                    );
                    Type::Error
                }
            },
            BinOp::Cons => match &right_resolved {
                Type::Tensor { elem, .. } => {
                    let elem = (**elem).clone();
                    let _ = self.infer.unify(&left_resolved, &elem);
                    Type::tensor(Shape::Rank(1), elem)
                }
                _ => {
                    self.error("E0302", "`::` expects a tensor on the right", span);
                    Type::Error
                }
            },
            BinOp::Pipe | BinOp::PipeBack => {
                self.error("E0202", "pipes are desugared by the parser", span);
                Type::Error
            }
        }
    }

    fn add_type(
        &mut self,
        op: BinOp,
        left: &Type,
        right: &Type,
        left_expr: &Expr,
        right_expr: &Expr,
        span: Span,
    ) -> Type {
        match (left, right) {
            (Type::Scalar(a_rep, a_dim), Type::Scalar(b_rep, b_dim)) => {
                if let Err(error) = self.infer.unify_dim(a_dim, b_dim) {
                    // §4.7: a bare `0` is not dimension-polymorphic, and the
                    // spec gives it its own code so the fix (`0 m`) is obvious.
                    let left_zero = is_bare_zero(left_expr);
                    let right_zero = is_bare_zero(right_expr);
                    let left_dim = self.infer.resolve_dim(a_dim);
                    let right_dim = self.infer.resolve_dim(b_dim);
                    if (left_zero && !right_dim.is_one()) || (right_zero && !left_dim.is_one()) {
                        self.error_help(
                            "E0406",
                            "a bare `0` is `Int`, not a dimensioned zero",
                            if left_zero { left_expr.span } else { right_expr.span },
                            "write `0 m` (or `0.0 m`) if you meant a zero quantity",
                        );
                        return Type::Error;
                    }
                    self.report_dim_error(error, span, left_expr.span, right_expr.span);
                    return Type::Error;
                }
                let rep = self.join_rep(*a_rep, *b_rep);
                Type::Scalar(rep, self.infer.resolve_dim(a_dim))
            }
            (Type::Tensor { shape, elem: a }, Type::Tensor { shape: b_shape, elem: b }) => {
                if let Err(error) = self.infer.unify_shape(shape, b_shape) {
                    self.report_unify(error, span);
                }
                let a = self.force_scalar(&self.infer.resolve(a));
                let b = self.force_scalar(&self.infer.resolve(b));
                let elem = self.add_type(op, &a, &b, left_expr, right_expr, span);
                Type::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(elem),
                }
            }
            (Type::Tensor { shape, elem }, Type::Scalar(_, dim))
            | (Type::Scalar(_, dim), Type::Tensor { shape, elem }) => {
                let elem = self.force_scalar(&self.infer.resolve(elem));
                if let Type::Scalar(_, elem_dim) = self.infer.resolve(&elem) {
                    if let Err(error) = self.infer.unify_dim(&elem_dim, dim) {
                        self.report_dim_error(error, span, left_expr.span, right_expr.span);
                        return Type::Error;
                    }
                }
                Type::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(elem),
                }
            }
            (Type::Point(a), Type::Point(b)) => {
                if op == BinOp::Sub {
                    if let Err(error) = self.infer.unify_dim(a, b) {
                        self.report_unify(error, span);
                        return Type::Error;
                    }
                    Type::float(self.infer.resolve_dim(a))
                } else {
                    self.error_help(
                        "E0405",
                        "two affine points cannot be added",
                        span,
                        "subtract them to get a difference, or use `lerp`",
                    );
                    Type::Error
                }
            }
            (Type::Point(a), Type::Scalar(rep, b)) if op == BinOp::Add => {
                if let Err(error) = self.infer.unify_dim(a, b) {
                    self.report_dim_error(error, span, left_expr.span, right_expr.span);
                    return Type::Error;
                }
                if let Err(error) = self.infer.unify_rep(*rep, Rep::Float) {
                    self.report_unify(error, right_expr.span);
                    return Type::Error;
                }
                Type::Point(self.infer.resolve_dim(a))
            }
            (Type::Scalar(rep, b), Type::Point(a)) if op == BinOp::Add => {
                if let Err(error) = self.infer.unify_dim(a, b) {
                    self.report_dim_error(error, span, left_expr.span, right_expr.span);
                    return Type::Error;
                }
                if let Err(error) = self.infer.unify_rep(*rep, Rep::Float) {
                    self.report_unify(error, left_expr.span);
                    return Type::Error;
                }
                Type::Point(self.infer.resolve_dim(a))
            }
            _ => {
                self.error(
                    "E0302",
                    format!(
                        "`{}` is not defined for {left} and {right}",
                        op.as_str()
                    ),
                    span,
                );
                Type::Error
            }
        }
    }

    fn mul_type(&mut self, left: &Type, right: &Type, span: Span) -> Type {
        match (left, right) {
            (Type::Scalar(a_rep, a_dim), Type::Scalar(b_rep, b_dim)) => {
                Type::Scalar(self.join_rep(*a_rep, *b_rep), a_dim.mul(b_dim))
            }
            (Type::Tensor { shape, elem }, Type::Scalar(rep, dim))
            | (Type::Scalar(rep, dim), Type::Tensor { shape, elem }) => {
                let elem = self.scale_elem(elem, *rep, dim, false, span);
                Type::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(elem),
                }
            }
            (Type::UnitOf(a), Type::UnitOf(b)) => Type::UnitOf(a.mul(b)),
            (Type::UnitOf(dim), Type::Scalar(rep, other))
            | (Type::Scalar(rep, other), Type::UnitOf(dim)) => {
                Type::Scalar(*rep, other.mul(dim))
            }
            (Type::UnitOf(dim), Type::Tensor { shape, elem })
            | (Type::Tensor { shape, elem }, Type::UnitOf(dim)) => {
                // §4.5.2: the representation does not change.
                let elem = self.scale_elem(elem, Rep::Real, dim, false, span);
                Type::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(elem),
                }
            }
            (Type::Point(_), _) | (_, Type::Point(_)) => {
                self.error("E0405", "affine points cannot be multiplied", span);
                Type::Error
            }
            _ => {
                self.error(
                    "E0302",
                    format!("`*` is not defined for {left} and {right}"),
                    span,
                );
                Type::Error
            }
        }
    }

    fn div_type(&mut self, left: &Type, right: &Type, span: Span) -> Type {
        match (left, right) {
            (Type::Scalar(a_rep, a_dim), Type::Scalar(b_rep, b_dim)) => {
                let joined = self.join_rep(*a_rep, *b_rep);
                Type::Scalar(self.div_rep(joined), a_dim.div(b_dim))
            }
            (Type::Tensor { shape, elem }, Type::Scalar(rep, dim)) => {
                let elem = self.scale_elem(elem, *rep, dim, true, span);
                Type::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(elem),
                }
            }
            (Type::UnitOf(a), Type::UnitOf(b)) => Type::UnitOf(a.div(b)),
            (Type::Scalar(rep, other), Type::UnitOf(dim)) => {
                Type::Scalar(*rep, other.div(dim))
            }
            (Type::Tensor { shape, elem }, Type::UnitOf(dim)) => {
                // §4.5.2: `Tensor<shape,Scalar<ρ,d>> / Unit[e]` keeps `ρ`.
                let elem = self.scale_elem(elem, Rep::Real, &dim.inverse(), false, span);
                Type::Tensor {
                    shape: shape.clone(),
                    elem: Box::new(elem),
                }
            }
            // §5.6.3: `p / k` and `p / K` are `E0405`, not a generic mismatch.
            (Type::Point(_), _) | (_, Type::Point(_)) => {
                self.error_help(
                    "E0405",
                    "an affine point cannot be divided",
                    span,
                    "divide a temperature *difference*, or use `lerp`",
                );
                Type::Error
            }
            _ => {
                self.error(
                    "E0302",
                    format!("`/` is not defined for {left} and {right}"),
                    span,
                );
                Type::Error
            }
        }
    }

    /// Scales or divides a tensor's element type.
    ///
    /// Division promotes an integer element to `Real` (`div_rep`, §3.4.2) but
    /// keeps a `Float` element on the IEEE path: `Tensor<Float> / 2` stays
    /// `Tensor<Float>`.
    fn scale_elem(&mut self, elem: &Type, rep: Rep, dim: &Dim, divide: bool, span: Span) -> Type {
        match self.infer.resolve(elem) {
            Type::Scalar(elem_rep, elem_dim) => {
                let joined = self.join_rep(elem_rep, rep);
                if divide {
                    Type::Scalar(self.div_rep(joined), elem_dim.div(dim))
                } else {
                    Type::Scalar(joined, elem_dim.mul(dim))
                }
            }
            other => {
                self.error(
                    "E0302",
                    format!("cannot scale a tensor of {other} by a unit"),
                    span,
                );
                Type::Error
            }
        }
    }

    /// `div_rep` of §3.4.2: `Int / Int` and `2^-1` are exact rationals.
    ///
    /// A literal that could still be an `Int` is committed to `Real` here,
    /// because the operation itself decides the representation.
    fn div_rep(&mut self, rep: Rep) -> Rep {
        match self.infer.resolve_rep(rep) {
            Rep::Int => Rep::Real,
            Rep::Var(id) => match self.infer.rep_literal_kind(Rep::Var(id)) {
                Some(LiteralKind::Int | LiteralKind::QuantityInt) => {
                    self.constrain_or_report(Rep::Var(id), Rep::Real);
                    Rep::Real
                }
                Some(LiteralKind::Decimal | LiteralKind::QuantityDecimal) => {
                    self.constrain_or_report(Rep::Var(id), Rep::Float);
                    Rep::Float
                }
                None => Rep::Var(id),
            },
            other => other,
        }
    }

    fn matmul_type(&mut self, left: &Type, right: &Type, span: Span) -> Type {
        let (Type::Tensor { shape: ls, elem: le }, Type::Tensor { shape: rs, elem: re }) =
            (left, right)
        else {
            self.error(
                "E0302",
                format!("`@` expects two tensors, found {left} and {right}"),
                span,
            );
            return Type::Error;
        };
        let elem = self.multiply_elems(le, re, span);
        if let (Shape::Refined(left_items), Shape::Refined(right_items)) = (ls, rs) {
            if let (Some(k), Some(k2)) = (
                left_items.get(1).copied().flatten(),
                right_items.first().copied().flatten(),
            ) {
                if k != k2 {
                    self.error(
                        "E0802",
                        format!("matmul requires left columns ({k}) = right rows ({k2})"),
                        span,
                    );
                    return Type::Error;
                }
            }
        }
        match (ls.rank(), rs.rank()) {
            (2, 2) => Type::Tensor {
                shape: match (ls, rs) {
                    (Shape::Refined(a), Shape::Refined(b)) => Shape::Refined(vec![
                        a.first().copied().flatten(),
                        b.get(1).copied().flatten(),
                    ]),
                    _ => Shape::Rank(2),
                },
                elem: Box::new(elem),
            },
            (2, 1) | (1, 2) => Type::Tensor {
                shape: Shape::Rank(1),
                elem: Box::new(elem),
            },
            (1, 1) => elem,
            _ => {
                self.error(
                    "E0302",
                    format!(
                        "`@` is not defined for tensors of rank {} and {}",
                        ls.rank(),
                        rs.rank()
                    ),
                    span,
                );
                Type::Error
            }
        }
    }

    fn multiply_elems(&mut self, left: &Type, right: &Type, span: Span) -> Type {
        let left = self.force_scalar(&self.infer.resolve(left));
        let right = self.force_scalar(&self.infer.resolve(right));
        match (&left, &right) {
            (Type::Scalar(a_rep, a_dim), Type::Scalar(b_rep, b_dim)) => {
                Type::Scalar(self.join_rep(*a_rep, *b_rep), a_dim.mul(b_dim))
            }
            _ => {
                self.error(
                    "E0411",
                    format!("incompatible tensor element types {left} and {right}"),
                    span,
                );
                Type::Error
            }
        }
    }

    fn pow_type(&mut self, base: &Type, exponent: &Expr, span: Span) -> Type {
        // §4.11: the exponent is a number, never a quantity — `2^(3 m)` is
        // `E0402`, not a silent mismatch.
        let exponent_synth = self.synth(exponent);
        let exponent_ty = self.infer.resolve(&exponent_synth);
        match &exponent_ty {
            Type::Scalar(_, dim) => {
                let resolved = self.infer.resolve_dim(dim);
                if !resolved.is_one()
                    && (resolved.is_concrete()
                        || self.infer.unify_dim(&resolved, &Dim::one()).is_err())
                {
                    let formatted = self.dims.format(&resolved);
                    self.error_help(
                        "E0402",
                        format!("the exponent of `^` must be dimensionless, found {formatted}"),
                        exponent.span,
                        "divide the exponent by its unit first",
                    );
                    return Type::Error;
                }
            }
            Type::Var(_) | Type::Error => {}
            other => {
                self.error(
                    "E0402",
                    format!("the exponent of `^` must be a number, found {other}"),
                    exponent.span,
                );
                return Type::Error;
            }
        }
        let exponent_value = literal_exponent(exponent);
        let Some(exponent_value) = exponent_value else {
            return match base {
                Type::Scalar(rep, dim) if dim.is_one() => Type::Scalar(*rep, Dim::one()),
                Type::Scalar(_, _) => {
                    self.error_help(
                        "E0404",
                        "a dimensioned value cannot be raised to a runtime exponent",
                        span,
                        "use a literal exponent, or `pow_dyn` for a checked dynamic power",
                    );
                    Type::Error
                }
                other => other.clone(),
            };
        };
        match base {
            Type::Scalar(rep, dim) => {
                // §3.4.2: a negative or fractional exponent divides, so an
                // integer base becomes an exact rational: `2^-1` is `Real`.
                let rep = if exponent_value.is_integer() && !exponent_value.is_negative() {
                    *rep
                } else {
                    self.div_rep(*rep)
                };
                Type::Scalar(rep, dim.pow(&exponent_value))
            }
            Type::Tensor { shape, elem } => Type::Tensor {
                shape: shape.clone(),
                elem: Box::new(self.pow_type(elem, exponent, span)),
            },
            other => {
                self.error("E0302", format!("`^` is not defined for {other}"), span);
                Type::Error
            }
        }
    }

    /// `join(ρ1, ρ2)` of §3.4.1.
    ///
    /// Two concrete representations take their least upper bound — `join(Int,
    /// Float) = Float`, so `fn f(a: Int): Int = a + 2.0` is a type error
    /// instead of silently typing as `Int`. When one side is a variable the
    /// other side becomes a *lower bound* (PLAN D4): the variable stays free,
    /// so `fn f(x) = x + 1` is polymorphic rather than `Int -> Int`.
    fn join_rep(&mut self, left: Rep, right: Rep) -> Rep {
        let left = self.infer.resolve_rep(left);
        let right = self.infer.resolve_rep(right);
        if left == right {
            return left;
        }
        match (left, right) {
            (Rep::Var(a), Rep::Var(b)) => {
                // The result must be above both. Keep one variable and record
                // the other as its lower bound, preferring the non-literal
                // side: a literal only says what its *default* is.
                let a_kind = self.infer.rep_literal_kind(Rep::Var(a));
                let b_kind = self.infer.rep_literal_kind(Rep::Var(b));
                let a_literal = a_kind.is_some();
                let b_literal = b_kind.is_some();
                let is_decimal = |kind: LiteralKind| {
                    matches!(kind, LiteralKind::Decimal | LiteralKind::QuantityDecimal)
                };
                // `1 m + 1.0 m`: an exact literal and a decimal literal.
                let mixed = match (a_kind, b_kind) {
                    (Some(a_kind), Some(b_kind)) => is_decimal(a_kind) != is_decimal(b_kind),
                    _ => false,
                };
                if mixed {
                    self.mixed_rep = true;
                }
                if a_literal && b_literal {
                    // §6.3.2: an exact operand and a decimal one meet on the
                    // exact `Real` path, whichever side each is written on.
                    // Keeping the *right* operand's literal kind — the old
                    // behaviour — typed `1.5 + 1` as `Int` and `1 + 1.5` as
                    // `Float`: the same sum typed two ways, one of which
                    // cannot hold the value. The result is the decimal side's
                    // variable, forced to `Real`, so the literal itself
                    // resolves through it.
                    if mixed {
                        let exact_path = if a_kind.is_some_and(is_decimal) {
                            Rep::Var(a)
                        } else {
                            Rep::Var(b)
                        };
                        // Mark the result as an *exact quantity*: that is the
                        // literal kind whose default is `Real`, and the one
                        // `div_rep` keeps on the exact path. Forcing `Rep::Real`
                        // outright would make `1.0 / 2` a `Float` again — the
                        // decimal literal's own kind wins in `div_rep`.
                        self.infer.mark_literal(
                            &Type::Scalar(exact_path, Dim::one()),
                            LiteralKind::QuantityInt,
                        );
                        self.constrain_or_report(exact_path, Rep::Var(a));
                        self.constrain_or_report(exact_path, Rep::Var(b));
                        return exact_path;
                    }
                    // `1 + 2`: the sum of two literals is still a literal, so
                    // it defaults rather than generalising.
                    //
                    // The joined kind must be the *stricter* of the two, not
                    // the right-hand one. `Int` is the only kind that admits
                    // an `Int` reading, so a value that came from an exact or
                    // decimal literal must not be re-marked `Int` when it meets
                    // one: `let x = 1.0 + 2` then `x * 3` used to narrow `x`
                    // to `Int`, which cannot hold the sum.
                    let joined_kind = match (a_kind, b_kind) {
                        (Some(LiteralKind::Int), Some(kind)) => kind,
                        (Some(kind), Some(LiteralKind::Int)) => kind,
                        (Some(kind), _) => kind,
                        (_, Some(kind)) => kind,
                        (None, None) => LiteralKind::Int,
                    };
                    self.infer.mark_literal(
                        &Type::Scalar(Rep::Var(a), Dim::one()),
                        joined_kind,
                    );
                    self.constrain_or_report(Rep::Var(a), Rep::Var(b));
                    Rep::Var(a)
                } else if a_literal {
                    self.constrain_or_report(Rep::Var(b), Rep::Var(a));
                    Rep::Var(b)
                } else {
                    self.constrain_or_report(Rep::Var(a), Rep::Var(b));
                    Rep::Var(a)
                }
            }
            (Rep::Var(variable), concrete) => {
                if let Some(kind) = self.infer.rep_literal_kind(Rep::Var(variable)) {
                    // A literal has a finite set of readings: pick the least
                    // one that is above `concrete`.
                    if let Some(chosen) = literal_reading_above(kind, concrete) {
                        self.constrain_or_report(Rep::Var(variable), concrete);
                        return chosen;
                    }
                }
                self.constrain_or_report(Rep::Var(variable), concrete);
                Rep::Var(variable)
            }
            (concrete, Rep::Var(variable)) => {
                if let Some(kind) = self.infer.rep_literal_kind(Rep::Var(variable)) {
                    if let Some(chosen) = literal_reading_above(kind, concrete) {
                        self.constrain_or_report(Rep::Var(variable), concrete);
                        return chosen;
                    }
                }
                self.constrain_or_report(Rep::Var(variable), concrete);
                Rep::Var(variable)
            }
            (a, b) => {
                if matches!((a, b), (Rep::Real, Rep::Float) | (Rep::Float, Rep::Real)) {
                    // §3.4.1: the exact path wins; `W0302` is the performance
                    // hint the appendix documents.
                    self.mixed_rep = true;
                }
                a.join_concrete(b).unwrap_or(Rep::Real)
            }
        }
    }

    /// Records a lower bound, reporting the failure instead of dropping it.
    fn constrain_or_report(&mut self, variable: Rep, lower: Rep) {
        if let Err(error) = self.infer.constrain_rep(variable, lower) {
            self.report_unify(error, Span::new(self.file, 0, 0));
        }
    }

    fn synth_call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> Type {
        if args
            .iter()
            .any(|arg| matches!(arg.kind, ExprKind::Placeholder))
        {
            return self.partial_application(callee, args, span);
        }
        // `Rssn.<op>(...)`: resolve the overload *here* and pin it, so the
        // runtime cannot execute a different one (PLAN D1).
        if let Some(op) = rssn_op_name(callee) {
            return self.rssn_call(&op, args, span);
        }
        // `Rssn.invoke` names an operator by *string*, so its result type is
        // not known statically. Typing it as a fresh variable would be
        // `∀a. a` — every call site could claim a different type. It returns
        // the dynamic value type instead, which the caller must `check`.
        if is_rssn_member(callee, "invoke") {
            let expected = [Type::Str, Type::Str, value_list_type()];
            for (arg, ty) in args.iter().zip(expected.iter()) {
                self.check(arg, ty);
            }
            if args.len() != expected.len() {
                self.error_help(
                    "E0315",
                    format!("`Rssn.invoke` takes 3 arguments, found {}", args.len()),
                    span,
                    "the generated declaration always passes a name, a signature hash and a list",
                );
            }
            return dynamic_value_type();
        }
        // A class method is solved against the instance table after the
        // arguments are checked, so the constraint sees the argument's type.
        if let ExprKind::Var(name) = &callee.kind {
            if let Some(scheme) = self.globals.get(name).cloned() {
                if !scheme.constraints.is_empty() {
                    return self.class_method_call(name, &scheme, args, span);
                }
            }
        }
        // `zeros<Float>(3, 4)`: the type argument fixes the element type.
        if let ExprKind::TypeApp {
            callee: inner,
            args: type_args,
        } = &callee.kind
        {
            if let ExprKind::Var(name) = &inner.kind {
                if matches!(name.as_str(), "zeros" | "ones" | "fill") {
                    return self.tensor_constructor(name, type_args, args, span);
                }
            }
        }
        if let ExprKind::Var(name) = &callee.kind {
            let shadowed = self.globals.contains_key(name)
                || self
                    .scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains_key(name));
            if shadowed {
                let callee_ty = self.synth(callee);
                return self.call_with_type(callee_ty, args, span);
            }
            if is_transcendental(name) {
                if let Some(arg) = args.first() {
                    let ty = self.synth(arg);
                    if let Type::Scalar(rep, dim) = self.infer.resolve(&ty) {
                        // An unresolved dimension is *committed* to 1: `exp(t)`
                        // in an unannotated lambda is dimensionless until
                        // something else says otherwise. A concrete dimension
                        // other than 1 is the error §3.4.4 describes.
                        let resolved = self.infer.resolve_dim(&dim);
                        if !resolved.is_one()
                            && (resolved.is_concrete()
                                || self.infer.unify_dim(&resolved, &Dim::one()).is_err())
                        {
                            let formatted = self.dims.format(&resolved);
                            self.error_help(
                                "E0403",
                                format!(
                                    "`{name}` requires a dimensionless argument, found {formatted}"
                                ),
                                arg.span,
                                "divide by a unit first, for example `x / 1 m`",
                            );
                            return Type::Error;
                        }
                        return Type::Scalar(rep, Dim::one());
                    }
                }
            }
            if builtins::is_special(name) {
                return self.special_call(name, args, span);
            }
            // §5.4.1: an affine unit displays a `Point`, never a temperature
            // difference — `value_in(°C, 10 K)` is `E0510`, not a conversion.
            if matches!(name.as_str(), "value_in" | "to") {
                if let Some(ty) = self.unit_display_call(name, args, span) {
                    return ty;
                }
            }
            // §4.11: `det`/`prod` of a dimensioned tensor cannot be expressed
            // statically — the exponent is the runtime order.
            if matches!(name.as_str(), "det" | "prod") {
                let saved = self.infer.clone();
                let before = self.diagnostics.items.len();
                let types_before = self.expr_types.len();
                if let Some(arg) = args.first() {
                    let ty = self.synth(arg);
                    if let Type::Tensor { elem, .. } = self.infer.resolve(&ty) {
                        if let Type::Scalar(_, dim) = self.infer.resolve(&elem) {
                            let resolved = self.infer.resolve_dim(&dim);
                            if !resolved.is_one() && resolved.is_concrete() {
                                let formatted = self.dims.format(&resolved);
                                self.error_help(
                                    "E0410",
                                    format!(
                                        "`{name}` of a {formatted} tensor needs a runtime dimension"
                                    ),
                                    arg.span,
                                    "divide by a unit first, or use `det_unit`",
                                );
                                return Type::Error;
                            }
                        }
                    }
                }
                self.infer.restore(&saved);
                self.diagnostics.items.truncate(before);
                // The probe was speculative: its substitutions were thrown
                // away, so the types it recorded would name variables nobody
                // can resolve. Drop them with the substitutions.
                self.expr_types.truncate(types_before);
            }
            // §3.8.4: `sort` needs a total order.
            if name == "sort" {
                if let Some(arg) = args.first() {
                    let ty = self.synth(arg);
                    if let Type::Tensor { elem, .. } = self.infer.resolve(&ty) {
                        self.require_instance("TotalOrd", &elem, span);
                    }
                    return ty;
                }
            }
            if let Some(candidates) = builtins::schemes(name) {
                if candidates.len() > 1 {
                    return self.overloaded_call(name, &candidates, args, span);
                }
            }
        }
        let callee_ty = self.synth(callee);
        self.call_with_type(callee_ty, args, span)
    }

    fn call_with_type(&mut self, callee_ty: Type, args: &[Expr], span: Span) -> Type {
        let callee_ty = self.infer.resolve(&callee_ty);
        match callee_ty {
            Type::Fn(params, ret) => {
                if params.len() != args.len() {
                    self.error(
                        "E0315",
                        format!(
                            "this function takes {} argument(s) but {} were given",
                            params.len(),
                            args.len()
                        ),
                        span,
                    );
                    for arg in args {
                        self.synth(arg);
                    }
                    return *ret;
                }
                for (arg, param) in args.iter().zip(params.iter()) {
                    self.check(arg, param);
                }
                *ret
            }
            Type::Var(_) => {
                let params: Vec<Type> = args.iter().map(|arg| self.synth(arg)).collect();
                let ret = self.infer.fresh_type();
                let function = Type::func(params, ret.clone());
                let _ = self.infer.unify(&callee_ty, &function);
                ret
            }
            other => {
                self.error("E0305", format!("{other} is not a function"), span);
                for arg in args {
                    self.synth(arg);
                }
                Type::Error
            }
        }
    }

    fn partial_application(&mut self, callee: &Expr, args: &[Expr], span: Span) -> Type {
        let mut params = Vec::new();
        let mut call_args = Vec::new();
        let mut scope = HashMap::new();
        let mut param_types = Vec::new();
        for (index, arg) in args.iter().enumerate() {
            if matches!(arg.kind, ExprKind::Placeholder) {
                let name = format!("__arg{index}");
                let ty = self.infer.fresh_type();
                scope.insert(name.clone(), Scheme::mono(ty.clone()));
                param_types.push(ty);
                params.push(Param {
                    pattern: Pat::new(PatKind::Var(name.clone()), arg.span),
                    ty: None,
                    span: arg.span,
                });
                call_args.push(Expr::new(ExprKind::Var(name), arg.span));
            } else {
                call_args.push(arg.clone());
            }
        }
        self.scopes.push(scope);
        let body_ty = self.synth_call(callee, &call_args, span);
        self.scopes.pop();
        let _ = params;
        Type::func(param_types, body_ty)
    }

    fn tensor_constructor(
        &mut self,
        name: &str,
        type_args: &[Ty],
        args: &[Expr],
        _span: Span,
    ) -> Type {
        let elem = type_args
            .first()
            .map_or_else(|| Type::float(Dim::one()), |ty| self.convert_type(ty));
        let (rank, value_args): (u32, &[Expr]) = if name == "fill" {
            (u32::try_from(args.len().saturating_sub(1)).unwrap_or(0), &args[1..])
        } else {
            (u32::try_from(args.len()).unwrap_or(0), args)
        };
        for arg in value_args {
            let ty = self.synth(arg);
            let _ = self.infer.unify(&ty, &Type::int());
        }
        if name == "fill" {
            if let Some(first) = args.first() {
                let ty = self.synth(first);
                return Type::tensor(Shape::Rank(rank), ty);
            }
        }
        Type::tensor(Shape::Rank(rank), elem)
    }

    fn overloaded_call(
        &mut self,
        name: &str,
        candidates: &[Scheme],
        args: &[Expr],
        span: Span,
    ) -> Type {
        let saved = self.infer.clone();
        let diagnostics_before = self.diagnostics.items.len();
        let types_before = self.expr_types.len();
        let mut first_error: Option<UnifyError> = None;
        for candidate in candidates {
            self.infer.restore(&saved);
            self.diagnostics.items.truncate(diagnostics_before);
            // Each candidate is a guess that was rolled back; its recorded
            // types were rolled back with it.
            self.expr_types.truncate(types_before);
            let callee = self.infer.instantiate(candidate);
            let Type::Fn(params, ret) = self.infer.resolve(&callee) else {
                continue;
            };
            if params.len() != args.len() {
                continue;
            }
            let mut ok = true;
            for (arg, param) in args.iter().zip(params.iter()) {
                let arg_ty = self.synth(arg);
                if let Err(error) = self.infer.unify(&arg_ty, param) {
                    first_error.get_or_insert(error);
                    ok = false;
                    break;
                }
            }
            if ok {
                return *ret;
            }
        }
        self.infer.restore(&saved);
        self.expr_types.truncate(types_before);
        for arg in args {
            self.synth(arg);
        }
        if let Some(error) = first_error {
            self.report_unify(error, span);
        } else {
            self.error(
                "E1003",
                format!("no overload of `{name}` accepts {} argument(s)", args.len()),
                span,
            );
        }
        Type::Error
    }

    fn special_call(&mut self, name: &str, args: &[Expr], span: Span) -> Type {
        match name {
            "zeros" | "ones" => {
                let rank = u32::try_from(args.len()).unwrap_or(0);
                for arg in args {
                    let ty = self.synth(arg);
                    let _ = self.infer.unify(&ty, &Type::int());
                }
                Type::tensor(Shape::Rank(rank), Type::float(Dim::one()))
            }
            "fill" => {
                if args.is_empty() {
                    self.error("E0315", "`fill` needs a value", span);
                    return Type::Error;
                }
                let elem = self.synth(&args[0]);
                let rank = u32::try_from(args.len().saturating_sub(1)).unwrap_or(0);
                for arg in &args[1..] {
                    let ty = self.synth(arg);
                    let _ = self.infer.unify(&ty, &Type::int());
                }
                Type::tensor(Shape::Rank(rank), elem)
            }
            "symbol" => {
                if let Some(arg) = args.first() {
                    let ty = self.synth(arg);
                    let _ = self.infer.unify(&ty, &Type::string());
                }
                // §3.3.3: a symbol keeps one dimension for the whole program,
                // so its dimension variable is weak and never generalised.
                Type::real(self.infer.fresh_weak_dim())
            }
            "symfun" => {
                if let Some(arg) = args.first() {
                    let ty = self.synth(arg);
                    let _ = self.infer.unify(&ty, &Type::string());
                }
                let domain = self.infer.fresh_weak_dim();
                let codomain = self.infer.fresh_weak_dim();
                Type::func(vec![Type::real(domain)], Type::real(codomain))
            }
            _ => Type::Error,
        }
    }

    fn synth_type_app(&mut self, callee: &Expr, args: &[Ty], span: Span) -> Type {
        let _ = (args, span);
        self.synth(callee)
    }

    fn synth_record(&mut self, base: Option<&Expr>, fields: &[(String, Expr)]) -> Type {
        // §3.6.1/§2: two fields with the same name in one literal is `E0208`,
        // not a silent last-write-wins.
        let mut seen: HashSet<&str> = HashSet::new();
        for (name, _) in fields {
            if !seen.insert(name.as_str()) {
                self.error_help(
                    "E0208",
                    format!("duplicate field `{name}`"),
                    Span::new(self.file, 0, 0),
                    "delete one of the two fields",
                );
            }
        }
        let mut row = match base {
            Some(base) => {
                let base_ty = self.synth(base);
                match self.infer.resolve(&base_ty) {
                    // The base row — including its open tail — is the result
                    // row, so `{ p with z = … }` keeps `p`'s unknown fields.
                    Type::Record(row) => row,
                    other => {
                        self.error(
                            "E0302",
                            format!("record update expects a record, found {other}"),
                            base.span,
                        );
                        Row::Empty
                    }
                }
            }
            None => Row::Empty,
        };
        for (name, value) in fields {
            let ty = self.synth(value);
            if base.is_some() {
                // §3.6.2: adding a field requires `row(p) lacks z`.
                if row.field(name).is_none() {
                    if let Some(tail) = row.tail_var() {
                        if let Err(error) = self.infer.require_lacks(tail, name) {
                            self.report_unify(error, value.span);
                        }
                    }
                }
                row = row.update(name, ty);
            } else {
                row = Row::Extend {
                    label: name.clone(),
                    ty: Box::new(ty),
                    tail: Box::new(row),
                };
            }
        }
        Type::Record(if base.is_some() {
            row
        } else {
            Row::closed(row.fields())
        })
    }

    fn field_type(&mut self, base: &Type, name: &str, span: Span) -> Type {
        let base = self.infer.resolve(base);
        if matches!(base, Type::Var(_)) {
            // An unannotated parameter used as `p.x` is an open record with at
            // least that field (§3.6.1).
            let field = self.infer.fresh_type();
            let tail = self.infer.fresh_row();
            let record = Type::Record(Row::Extend {
                label: name.to_string(),
                ty: Box::new(field.clone()),
                tail: Box::new(tail),
            });
            let _ = self.infer.unify(&base, &record);
            return field;
        }
        match self.infer.resolve(&base) {
            Type::Record(row) => match row.field(name) {
                Some(ty) => ty.clone(),
                None => match row.tail_var() {
                    Some(tail) => {
                        // The row is still open: absorb the field (§3.6.1).
                        let field = self.infer.fresh_type();
                        let _ = self.infer.extend_row(tail, name, field.clone());
                        field
                    }
                    None => {
                        self.error(
                            "E0306",
                            format!("the record {{ {row} }} has no field `{name}`"),
                            span,
                        );
                        Type::Error
                    }
                },
            },
            other => {
                self.error(
                    "E0302",
                    format!("field access expects a record, found {other}"),
                    span,
                );
                Type::Error
            }
        }
    }

    fn synth_tensor(&mut self, items: &[Expr], span: Span) -> Type {
        if items.is_empty() {
            self.error_help(
                "E0314",
                "the element type of an empty tensor cannot be inferred",
                span,
                "annotate it, for example `let e: Tensor<1, Float> = []`",
            );
            return Type::Error;
        }
        let mut elem: Option<Type> = None;
        for item in items {
            let ty = self.synth(item);
            match &elem {
                None => elem = Some(ty),
                Some(existing) => {
                    if let Err(error) = self.infer.unify(existing, &ty) {
                        self.report_unify(error, item.span);
                    }
                }
            }
        }
        let elem = elem.unwrap_or(Type::Error);
        let resolved = self.infer.resolve(&elem);
        match resolved {
            Type::Tensor {
                shape: inner,
                elem: inner_elem,
            } => {
                let mut shape = vec![Some(items.len())];
                shape.extend(shape_items(&inner));
                Type::Tensor {
                    shape: Shape::Refined(shape),
                    elem: inner_elem,
                }
            }
            other => Type::Tensor {
                shape: Shape::Refined(vec![Some(items.len())]),
                elem: Box::new(other),
            },
        }
    }

    fn synth_index(&mut self, base: &Expr, indices: &[SliceAst], span: Span) -> Type {
        let base_ty = self.synth(base);
        let mut resolved = self.infer.resolve(&base_ty);
        if matches!(resolved, Type::Var(_)) {
            let elem = self.infer.fresh_type();
            let tensor = Type::tensor(Shape::Rank(1), elem);
            let _ = self.infer.unify(&resolved, &tensor);
            resolved = self.infer.resolve(&tensor);
        }
        let Type::Tensor { shape, elem } = resolved else {
            self.error(
                "E0302",
                format!("indexing expects a tensor, found {resolved}"),
                span,
            );
            return Type::Error;
        };
        let mut sliced = false;
        let mut consumed: Vec<usize> = Vec::new();
        for (position, index) in indices.iter().enumerate() {
            match index {
                SliceAst::Index(expr) => {
                    // §3.5.4: the index is an `Int`. A `String` index used to
                    // be silently accepted because the error was discarded.
                    let ty = self.synth(expr);
                    if let Err(error) = self.infer.unify(&ty, &Type::int()) {
                        self.report_unify(error, expr.span);
                    }
                    // A literal index into a refined shape is checked here;
                    // a dynamic one is checked at run time (§3.5.4).
                    if let Some(value) = literal_index(expr) {
                        if let Shape::Refined(items) = &shape {
                            if let Some(Some(extent)) = items.get(position) {
                                let extent = i64::try_from(*extent).unwrap_or(i64::MAX);
                                if value < -extent || value >= extent {
                                    self.error_help(
                                        "E0802",
                                        format!(
                                            "index {value} is out of bounds for a dimension of extent {extent}"
                                        ),
                                        expr.span,
                                        "a literal index is checked at compile time",
                                    );
                                }
                            }
                        }
                    }
                    consumed.push(position);
                }
                SliceAst::Range {
                    start, end, step, ..
                } => {
                    sliced = true;
                    for part in [start, end].into_iter().flatten() {
                        let ty = self.synth(part);
                        if let Err(error) = self.infer.unify(&ty, &Type::int()) {
                            self.report_unify(error, part.span);
                        }
                    }
                    // §6.6.4: the step is an `Int`, not a `Float` and not a
                    // quantity — a step is a number of positions, so it has no
                    // dimension and no fractional part. A known zero is
                    // rejected here; a computed one is `E0802` at run time.
                    if let Some(step) = step {
                        let ty = self.synth(step);
                        if self.infer.unify(&ty, &Type::int()).is_err() {
                            self.error(
                                "E0302",
                                "a slice step must be an Int".to_string(),
                                step.span,
                            );
                        } else if literal_index(step) == Some(0) {
                            self.error_help(
                                "E0802",
                                "a slice step of zero never advances".to_string(),
                                step.span,
                                "omit the step to walk by one",
                            );
                        }
                    }
                }
            }
        }
        let rank = shape.rank();
        let removed = u32::try_from(consumed.len()).unwrap_or(0);
        let result_rank = if sliced {
            rank.saturating_sub(removed).max(1)
        } else {
            rank.saturating_sub(removed)
        };
        if result_rank == 0 {
            return *elem;
        }
        // The refinement survives indexing: `[3]` indexed by `0` is a scalar,
        // and `[3, 4]` indexed by `i` is `[4]`.
        let result_shape = match &shape {
            Shape::Refined(items) => {
                let mut kept = items.clone();
                for position in consumed.iter().rev() {
                    if *position < kept.len() {
                        kept.remove(*position);
                    }
                }
                Shape::Refined(kept)
            }
            Shape::Rank(_) => Shape::Rank(result_rank),
        };
        Type::Tensor {
            shape: result_shape,
            elem,
        }
    }

    fn synth_try(&mut self, inner: &Expr, span: Span) -> Type {
        let inner_ty = self.synth(inner);
        let inner_ty = self.infer.resolve(&inner_ty);
        let result_arity = match &inner_ty {
            Type::Adt(id, args) if *id == adt::RESULT_ID && args.len() == 2 => Some(2),
            Type::Adt(id, args) if *id == adt::OPTION_ID && !args.is_empty() => Some(1),
            _ => None,
        };
        let Some(arity) = result_arity else {
            self.error(
                "E0702",
                "`?` requires a `Result` or `Option` value",
                span,
            );
            return Type::Error;
        };
        // §3.9.3: `?` returns from the *enclosing function*, so that function
        // must return the same container.
        let expected = self.return_type.clone();
        match expected.map(|ty| self.infer.resolve(&ty)) {
            Some(Type::Adt(id, args)) if id == adt::RESULT_ID && args.len() == 2 && arity == 2 => {
                if let Type::Adt(_, inner_args) = &inner_ty {
                    // The error types only need to agree when one of them is
                    // still open. Two different concrete error types are a
                    // *conversion* question the specification does not settle,
                    // so the enclosing function's type wins.
                    let left = self.infer.resolve(&inner_args[1]);
                    let right = self.infer.resolve(&args[1]);
                    if matches!(left, Type::Var(_)) || matches!(right, Type::Var(_)) {
                        let _ = self.infer.unify(&inner_args[1], &args[1]);
                    }
                }
            }
            Some(Type::Adt(id, args))
                if id == adt::OPTION_ID && !args.is_empty() && arity == 1 => {}
            // An unannotated function's return type is not known yet.
            Some(Type::Var(_) | Type::Error) => {}
            Some(other) => {
                self.error_help(
                    "E0702",
                    format!("`?` needs the enclosing function to return `Result`, but it returns {other}"),
                    span,
                    "return `Result<T, E>`, or handle the error with `match`",
                );
                return Type::Error;
            }
            None => {}
        }
        match inner_ty {
            Type::Adt(_, args) => args[0].clone(),
            other => other,
        }
    }

    fn synth_do(&mut self, stmts: &[DoStmt]) -> Type {
        self.scopes.push(HashMap::new());
        let mut last_ty = Type::Unit;
        for stmt in stmts {
            match stmt {
                DoStmt::Let {
                    pattern,
                    ty,
                    value,
                    ..
                } => {
                    let value_ty = match ty {
                        Some(ty) => {
                            let ty = self.convert_type(ty);
                            self.check(value, &ty);
                            ty
                        }
                        None => self.synth(value),
                    };
                    self.bind_pattern(pattern, &value_ty);
                }
                DoStmt::Bind { pattern, value, .. } => {
                    let value_ty = self.synth(value);
                    let inner = match self.infer.resolve(&value_ty) {
                        Type::Io(inner) => *inner,
                        other => {
                            self.error(
                                "E0701",
                                format!("`<-` expects an IO action, found {other}"),
                                value.span,
                            );
                            Type::Error
                        }
                    };
                    self.bind_pattern(pattern, &inner);
                }
                DoStmt::Expr(expr) => {
                    let ty = self.synth(expr);
                    match self.infer.resolve(&ty) {
                        Type::Io(inner) => {
                            if !inner.is_unit() {
                                self.warn(
                                    "W0202",
                                    "this IO action's result is discarded",
                                    expr.span,
                                );
                            }
                            last_ty = *inner;
                        }
                        other => {
                            self.error(
                                "E0701",
                                format!("a `do` statement must be an IO action, found {other}"),
                                expr.span,
                            );
                        }
                    }
                }
            }
        }
        self.scopes.pop();
        Type::io(last_ty)
    }

    fn quantity_type(&mut self, value: &NumberLit, unit: &UnitExpr, span: Span) -> Type {
        let resolved = match self.units.resolve(unit) {
            Ok(resolved) => resolved,
            Err(error) => {
                self.unit_error(error, span);
                return Type::Error;
            }
        };
        if resolved.affine {
            return Type::Point(resolved.dim);
        }
        let rep = self.infer.fresh_rep();
        let kind = if value.is_decimal() {
            LiteralKind::QuantityDecimal
        } else {
            LiteralKind::QuantityInt
        };
        self.infer
            .mark_literal(&Type::Scalar(rep, resolved.dim.clone()), kind);
        Type::Scalar(rep, resolved.dim)
    }

    fn check_exhaustive(
        &mut self,
        subject: &Type,
        covered: &[(adt::AdtId, usize)],
        covered_bool: (bool, bool),
        has_wildcard: bool,
        span: Span,
    ) {
        if has_wildcard {
            return;
        }
        match self.infer.resolve(subject) {
            Type::Adt(id, _) => {
                if let Some(adt) = adt::get(id) {
                    if adt.ctors.is_empty() {
                        return;
                    }
                    let missing: Vec<String> = adt
                        .ctors
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| !covered.contains(&(id, *index)))
                        .map(|(_, ctor)| ctor.name.clone())
                        .collect();
                    if !missing.is_empty() {
                        self.error_help(
                            "E0309",
                            format!("non-exhaustive match: missing {}", missing.join(", ")),
                            span,
                            "add the missing arms or a `_` arm",
                        );
                    }
                }
            }
            Type::Bool if covered_bool != (true, true) => {
                let missing = match covered_bool {
                    (false, false) => "true and false",
                    (true, false) => "false",
                    (false, true) => "true",
                    _ => "",
                };
                self.error_help(
                    "E0309",
                    format!("non-exhaustive match on Bool: missing {missing}"),
                    span,
                    "add the missing arms or a `_` arm",
                );
            }
            _ => {}
        }
    }

    // ----- patterns ------------------------------------------------------

    fn bind_pattern(&mut self, pattern: &Pat, ty: &Type) {
        match &pattern.kind {
            // A parser recovery node: the syntax error was already reported.
            PatKind::Error => {}
            PatKind::Wildcard => {}
            PatKind::Var(name) => {
                // The binder is a place the editor asks about, and this is the
                // type its uses get. Recorded at the pattern's own span, so it
                // never competes with an expression that happens to share it.
                self.note_type(pattern.span, ty);
                if let Some(scope) = self.scopes.last_mut() {
                    scope.insert(name.clone(), Scheme::mono(ty.clone()));
                } else {
                    self.globals.insert(name.clone(), Scheme::mono(ty.clone()));
                }
            }
            PatKind::Ctor { name, args } => {
                let resolved = self.infer.resolve(ty);
                let (adt_id, adt_args) = match &resolved {
                    Type::Adt(id, args) => (*id, args.clone()),
                    _ => {
                        self.error(
                            "E0308",
                            format!("pattern `{name}` does not match {resolved}"),
                            pattern.span,
                        );
                        return;
                    }
                };
                let Some(registered) = adt::get(adt_id) else {
                    return;
                };
                let Some(index) = registered.ctors.iter().position(|ctor| ctor.name == *name) else {
                    self.error(
                        "E0307",
                        format!("`{}` has no constructor `{name}`", registered.name),
                        pattern.span,
                    );
                    return;
                };
                let ctor = &registered.ctors[index];
                if ctor.fields.len() != args.len() {
                    self.error(
                        "E0307",
                        format!(
                            "constructor `{name}` takes {} argument(s), found {}",
                            ctor.fields.len(),
                            args.len()
                        ),
                        pattern.span,
                    );
                    return;
                }
                for (sub, field) in args.iter().zip(ctor.fields.iter()) {
                    let field_ty = field.substitute_params(&adt_args);
                    self.bind_pattern(sub, &field_ty);
                }
            }
            PatKind::Record(fields) => {
                let resolved = self.infer.resolve(ty);
                let Type::Record(row) = resolved else {
                    self.error(
                        "E0308",
                        format!("a record pattern does not match {resolved}"),
                        pattern.span,
                    );
                    return;
                };
                for (name, sub) in fields {
                    let Some(field_ty) = row.field(name).cloned() else {
                        self.error(
                            "E0306",
                            format!("the record {{ {row} }} has no field `{name}`"),
                            pattern.span,
                        );
                        continue;
                    };
                    match sub {
                        Some(sub) => self.bind_pattern(sub, &field_ty),
                        None => {
                            if let Some(scope) = self.scopes.last_mut() {
                                scope.insert(name.clone(), Scheme::mono(field_ty));
                            }
                        }
                    }
                }
            }
            PatKind::Tuple(items) => {
                let resolved = self.infer.resolve(ty);
                let Type::Record(row) = resolved else {
                    self.error(
                        "E0308",
                        format!("a tuple pattern does not match {resolved}"),
                        pattern.span,
                    );
                    return;
                };
                for (index, item) in items.iter().enumerate() {
                    let field_ty = row
                        .field(&index.to_string())
                        .cloned()
                        .unwrap_or(Type::Error);
                    self.bind_pattern(item, &field_ty);
                }
            }
            PatKind::Tensor { items, rest } => {
                let resolved = self.infer.resolve(ty);
                let Type::Tensor { elem, .. } = resolved else {
                    self.error(
                        "E0308",
                        format!("a tensor pattern does not match {resolved}"),
                        pattern.span,
                    );
                    return;
                };
                for item in items {
                    self.bind_pattern(item, &elem);
                }
                if let Some(name) = rest {
                    if let Some(scope) = self.scopes.last_mut() {
                        scope.insert(
                            name.clone(),
                            Scheme::mono(Type::tensor(Shape::Rank(1), (*elem).clone())),
                        );
                    }
                }
            }
            PatKind::Lit(lit) => {
                let lit_ty = match lit {
                    PatLit::Int(_) => Type::int(),
                    PatLit::Str(_) => Type::string(),
                    PatLit::Bool(_) => Type::bool(),
                    PatLit::Unit => Type::Unit,
                };
                if let Err(error) = self.infer.unify(ty, &lit_ty) {
                    self.report_unify(error, pattern.span);
                }
            }
            PatKind::As { pattern: inner, name } => {
                self.bind_pattern(inner, ty);
                if let Some(scope) = self.scopes.last_mut() {
                    scope.insert(name.clone(), Scheme::mono(ty.clone()));
                }
            }
            PatKind::Annot {
                pattern: inner,
                ty: annotation,
            } => {
                let annotated = self.convert_type(annotation);
                if let Err(error) = self.infer.unify(&annotated, ty) {
                    self.report_unify(error, pattern.span);
                }
                self.bind_pattern(inner, &annotated);
            }
        }
    }

    // ----- types ---------------------------------------------------------

    fn convert_type(&mut self, ty: &Ty) -> Type {
        let params = self.type_params.clone();
        self.convert_type_with_params(ty, &params)
    }

    fn convert_type_with_params(&mut self, ty: &Ty, params: &[(String, Type)]) -> Type {
        match &ty.kind {
            TyKind::Int => Type::int(),
            TyKind::Bool => Type::Bool,
            TyKind::Prop => Type::Prop,
            TyKind::Str => Type::string(),
            TyKind::Unit => Type::Unit,
            TyKind::Real(dim) => Type::real(self.convert_dim(dim, ty.span)),
            TyKind::Float(dim) => Type::float(self.convert_dim(dim, ty.span)),
            TyKind::Complex(dim) => Type::complex(self.convert_dim(dim, ty.span)),
            TyKind::Point(dim) => Type::Point(self.convert_dim(dim, ty.span)),
            TyKind::UnitOf(dim) => Type::UnitOf(self.convert_dim(dim, ty.span)),
            TyKind::Tensor { shape, elem } => Type::Tensor {
                shape: self.convert_shape(shape),
                elem: Box::new(self.convert_type_with_params(elem, params)),
            },
            // `Matrix<n, m, τ>` is a rank-2 tensor with a static shape.
            TyKind::Matrix { rows, cols, elem } => Type::Tensor {
                shape: Shape::Refined(vec![Some(self.convert_nat(rows)), Some(self.convert_nat(cols))]),
                elem: Box::new(self.convert_type_with_params(elem, params)),
            },
            // `Sparse<fmt, shape, τ>` has the shape of its dense counterpart;
            // the storage format is a runtime property.
            TyKind::Sparse { shape, elem, .. } => Type::Tensor {
                shape: self.convert_shape(shape),
                elem: Box::new(self.convert_type_with_params(elem, params)),
            },
            // A row sum is the union of its summands' fields.
            TyKind::Row(items) => {
                let mut merged: Vec<(String, Type)> = Vec::new();
                for item in items {
                    let converted = self.convert_type_with_params(item, params);
                    match converted {
                        Type::Record(row) => merged.extend(row.fields()),
                        other => return other,
                    }
                }
                Type::record(merged)
            }
            // A dimension argument is carried as `Real[d]`, the encoding this
            // checker already uses for a dimension in argument position.
            TyKind::DimArg(dim) => Type::real(self.convert_dim(dim, ty.span)),
            // A natural number or a shape is a value-level argument: it only
            // appears where a shape is expected, so it contributes no type.
            TyKind::NatArg(_) | TyKind::ShapeArg(_) => Type::int(),
            TyKind::Option(inner) => Type::option(self.convert_type_with_params(inner, params)),
            TyKind::Result(ok, err) => Type::result(
                self.convert_type_with_params(ok, params),
                self.convert_type_with_params(err, params),
            ),
            TyKind::Set(inner) => {
                let inner = self.convert_type_with_params(inner, params);
                // §3.8.4: `Set<a>` needs `TotalOrd a`; `Set<Float>` is not
                // legal because `Float` has no total order (NaN).
                self.require_instance("TotalOrd", &inner, ty.span);
                Type::Adt(
                    adt::intern(Adt {
                        name: "Set".to_string(),
                        params: vec!["a".to_string()],
                        ctors: Vec::new(),
                    }),
                    vec![inner],
                )
            }
            TyKind::Table(inner) => Type::Adt(
                adt::intern(Adt {
                    name: "Table".to_string(),
                    params: vec!["r".to_string()],
                    ctors: Vec::new(),
                }),
                vec![self.convert_type_with_params(inner, params)],
            ),
            TyKind::Dyn(rep) => Type::Dyn(match rep.as_str() {
                "Int" => Rep::Int,
                "Float" => Rep::Float,
                "Complex" => Rep::Complex,
                _ => Rep::Real,
            }),
            TyKind::RateOf(inner) => {
                let inner = self.convert_type_with_params(inner, params);
                self.rate_of(&inner)
            }
            TyKind::SymFun(domain, codomain) => {
                let domain = self.convert_dim(domain, ty.span);
                let codomain = self.convert_dim(codomain, ty.span);
                Type::func(vec![Type::real(domain)], Type::real(codomain))
            }
            TyKind::Io(inner) => Type::io(self.convert_type_with_params(inner, params)),
            TyKind::Fn(args, ret) => Type::func(
                args.iter()
                    .map(|arg| self.convert_type_with_params(arg, params))
                    .collect(),
                self.convert_type_with_params(ret, params),
            ),
            TyKind::Record { fields, rest } => {
                let mut collected: Vec<(String, Type)> = fields
                    .iter()
                    .map(|(name, ty)| {
                        (name.clone(), self.convert_type_with_params(ty, params))
                    })
                    .collect();
                if rest.is_some() {
                    // An open row: the fields are known, the tail is a fresh
                    // row variable that absorbs whatever the caller passes.
                    collected.sort_by(|a, b| a.0.cmp(&b.0));
                    let mut row = self.infer.fresh_row();
                    for (name, ty) in collected.into_iter().rev() {
                        row = Row::Extend {
                            label: name,
                            ty: Box::new(ty),
                            tail: Box::new(row),
                        };
                    }
                    Type::Record(row)
                } else {
                    Type::record(collected)
                }
            }
            TyKind::Tuple(items) => Type::record(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, ty)| {
                        (
                            index.to_string(),
                            self.convert_type_with_params(ty, params),
                        )
                    })
                    .collect::<Vec<_>>(),
            ),
            TyKind::Named { path, args } => {
                let name = path.join(".");
                if self.declaring_alias.as_deref() == Some(name.as_str()) {
                    self.error_help(
                        "E0312",
                        "non-positive recursive type",
                        ty.span,
                        "a type alias cannot refer to itself; declare a variant type instead",
                    );
                    return Type::Error;
                }
                if let Some(alias) = self.aliases.get(&name).cloned() {
                    let converted: Vec<Type> = args
                        .iter()
                        .map(|arg| self.convert_type_with_params(arg, params))
                        .collect();
                    return alias.ty.substitute_params(&converted);
                }
                if let Some(id) = self.adts.get(&name).copied() {
                    let converted: Vec<Type> = args
                        .iter()
                        .map(|arg| self.convert_type_with_params(arg, params))
                        .collect();
                    return Type::Adt(id, converted);
                }
                if let Some(builtin) = builtin_type(&name) {
                    return builtin;
                }
                self.error("E0301", format!("unknown type `{name}`"), ty.span);
                Type::Error
            }
            TyKind::Var(name) => {
                if let Some((_, ty)) = params.iter().find(|(param, _)| param == name) {
                    return ty.clone();
                }
                if self.implicit_params {
                    // §3.8.1: a variable in an instance head is implicitly
                    // universally quantified.
                    let fresh = self.infer.fresh_type();
                    self.type_params.push((name.clone(), fresh.clone()));
                    return fresh;
                }
                self.error(
                    "E0301",
                    format!("unknown type variable `{name}`"),
                    ty.span,
                );
                Type::Error
            }
            TyKind::Forall(_, body) => self.convert_type_with_params(body, params),
        }
    }

    fn rate_of(&mut self, ty: &Type) -> Type {
        match ty {
            Type::Scalar(rep, dim) => Type::Scalar(*rep, dim.div(&Dim::base("Time"))),
            Type::Tensor { shape, elem } => Type::Tensor {
                shape: shape.clone(),
                elem: Box::new(self.rate_of(elem)),
            },
            Type::Record(row) => Type::Record(Row::closed(
                row.fields()
                    .into_iter()
                    .map(|(name, ty)| (name, self.rate_of(&ty)))
                    .collect::<Vec<_>>(),
            )),
            other => other.clone(),
        }
    }

    fn convert_shape(&mut self, shape: &ShapeAst) -> Shape {
        match shape {
            ShapeAst::Rank(nat) => Shape::Rank(u32::try_from(self.convert_nat(nat)).unwrap_or(0)),
            ShapeAst::Refined(items) => Shape::Refined(
                items
                    .iter()
                    .map(|item| match item {
                        ShapeItem::Known(nat) => Some(self.convert_nat(nat)),
                        ShapeItem::Unknown => None,
                    })
                    .collect(),
            ),
        }
    }

    fn convert_nat(&mut self, nat: &NatAst) -> usize {
        match nat {
            NatAst::Lit(value) => *value,
            NatAst::Var(name) => {
                self.error_help(
                    "E0304",
                    format!("cannot infer the rank `{name}`"),
                    Span::new(self.file, 0, 0),
                    "write a concrete rank, for example `Tensor<1, Float>`",
                );
                0
            }
            NatAst::Add(left, right) => self.convert_nat(left) + self.convert_nat(right),
        }
    }

    fn convert_dim(&mut self, dim: &DimAst, span: Span) -> Dim {
        match dim {
            DimAst::One => Dim::one(),
            DimAst::Name(name) => {
                if let Some((_, dim)) = self.dim_params.iter().find(|(param, _)| param == name) {
                    return dim.clone();
                }
                match self.dims.lookup(name) {
                    Some(dim) => dim,
                    None => {
                        if self.implicit_params {
                            // §3.8.1: `instance Show for Real[d]` quantifies
                            // `d` implicitly.
                            let fresh = self.infer.fresh_dim();
                            self.dim_params.push((name.clone(), fresh.clone()));
                            return fresh;
                        }
                        self.error("E0407", format!("unknown dimension `{name}`"), span);
                        Dim::one()
                    }
                }
            }
            DimAst::Mul(left, right) => self
                .convert_dim(left, span)
                .mul(&self.convert_dim(right, span)),
            DimAst::Div(left, right) => self
                .convert_dim(left, span)
                .div(&self.convert_dim(right, span)),
            DimAst::Pow(base, exponent) => self.convert_dim(base, span).pow(exponent),
            DimAst::Paren(inner) => self.convert_dim(inner, span),
        }
    }

    // ----- helpers -------------------------------------------------------

    fn lookup_value(&mut self, name: &str, span: Span) -> Type {
        self.referenced.insert(name.to_string());
        for scope in self.scopes.iter().rev() {
            if let Some(scheme) = scope.get(name) {
                let scheme = scheme.clone();
                return self.instantiate_value(&scheme, span);
            }
        }
        if let Some(scheme) = self.globals.get(name).cloned() {
            return self.instantiate_value(&scheme, span);
        }
        if let Some(unit) = self.units.lookup(name) {
            return Type::UnitOf(unit.dim);
        }
        if let Some(candidates) = builtins::schemes(name) {
            return self.infer.instantiate(&candidates[0]);
        }
        if let Some((adt, index, _)) = adt::find_prelude_ctor(name) {
            let scheme = ctor_scheme(adt, index);
            return self.infer.instantiate(&scheme);
        }
        self.error("E0301", format!("unbound name `{name}`"), span);
        Type::Error
    }

    /// Instantiates a scheme, solving whatever class constraints come with it.
    fn instantiate_value(&mut self, scheme: &Scheme, span: Span) -> Type {
        let (ty, constraints) = self.infer.instantiate_scheme(scheme);
        for constraint in &constraints {
            self.solve_constraint(constraint, span);
        }
        ty
    }

    fn expect_bool(&mut self, ty: &Type, span: Span) {
        if matches!(self.infer.resolve(ty), Type::Var(_)) {
            let _ = self.infer.unify(ty, &Type::Bool);
            return;
        }
        match self.infer.resolve(ty) {
            Type::Bool | Type::Error => {}
            Type::Prop => {
                self.error_help(
                    "E0302",
                    "a symbolic proposition is not a `Bool`",
                    span,
                    "use `decide(p)` or `assuming(p, ...)`",
                );
            }
            other => self.error("E0302", format!("expected Bool, found {other}"), span),
        }
    }

    fn report_unify(&mut self, error: UnifyError, span: Span) {
        match error {
            UnifyError::Dim(mismatch) => {
                let left = self.dims.format(&mismatch.left);
                let right = self.dims.format(&mismatch.right);
                let difference = self.dims.format(&mismatch.difference);
                self.diagnostics.push(
                    Diagnostic::error("E0401", "dimension mismatch", span)
                        .with_primary_label(format!("expected {right}, found {left}"))
                        .with_note(format!("left:  {left}"))
                        .with_note(format!("right: {right}"))
                        .with_note(format!("difference: {difference}")),
                );
            }
            UnifyError::Mismatch { left, right } => {
                self.diagnostics.push(
                    Diagnostic::error("E0302", "type mismatch", span)
                        .with_primary_label(format!("expected {right}, found {left}")),
                );
            }
            UnifyError::LiteralMismatch { kind, requested } => {
                let label = match kind {
                    LiteralKind::Int | LiteralKind::Decimal => {
                        format!("a bare numeric literal cannot have type {requested}")
                    }
                    _ => format!("this literal cannot have type {requested}"),
                };
                self.diagnostics.push(
                    Diagnostic::error("E0302", "type mismatch", span)
                        .with_primary_label(label)
                        .with_help("write a unit, for example `1 m`"),
                );
            }
            other => {
                self.diagnostics
                    .push(Diagnostic::error(code(&other), other.to_string(), span));
            }
        }
    }

    fn report_dim_error(
        &mut self,
        error: UnifyError,
        span: Span,
        left_span: Span,
        right_span: Span,
    ) {
        let UnifyError::Dim(mismatch) = error else {
            self.report_unify(error, span);
            return;
        };
        let left = self.dims.format(&mismatch.left);
        let right = self.dims.format(&mismatch.right);
        let difference = self.dims.format(&mismatch.difference);
        self.diagnostics.push(
            Diagnostic::error("E0401", "dimension mismatch", span)
                .with_primary_label(format!("expected {right}, found {left}"))
                .with_label(left_span, format!("this is {left}"))
                .with_label(right_span, format!("this is {right}"))
                .with_note(format!("difference: {difference}")),
        );
        if mismatch.right.is_one() {
            self.diagnostics
                .items
                .last_mut()
                .expect("diagnostic was just pushed")
                .helps
                .push("a bare number has no unit; write `2 m` if you meant a length".to_string());
        }
    }

    fn unit_error(&mut self, error: UnitError, span: Span) {
        self.diagnostics
            .push(Diagnostic::error(error.code(), error.to_string(), span));
    }

    fn error(&mut self, code: &str, message: impl Into<String>, span: Span) {
        self.diagnostics
            .push(Diagnostic::error(code, message, span));
    }

    fn error_help(&mut self, code: &str, message: impl Into<String>, span: Span, help: &str) {
        self.diagnostics.push(
            Diagnostic::error(code, message, span).with_help(help),
        );
    }

    fn warn(&mut self, code: &str, message: impl Into<String>, span: Span) {
        self.diagnostics
            .push(Diagnostic::warning(code, message, span));
    }
}

/// The name of a unit when an expression is a bare unit value.
fn unit_name_of(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Var(name) => Some(name.clone()),
        ExprKind::QName(path) => Some(path.join(".")),
        _ => None,
    }
}

/// Whether a type mentions any of `ids`.
fn mentions_any(ty: &Type, ids: &[u32]) -> bool {
    match ty {
        Type::Var(id) => ids.contains(id),
        Type::Scalar(rep, _) => match rep {
            Rep::Var(id) => ids.contains(id),
            _ => false,
        },
        Type::Tensor { elem, .. } => mentions_any(elem, ids),
        Type::Fn(params, ret) => {
            params.iter().any(|param| mentions_any(param, ids)) || mentions_any(ret, ids)
        }
        Type::Record(row) => row
            .fields()
            .iter()
            .any(|(_, field)| mentions_any(field, ids)),
        Type::Adt(_, args) => args.iter().any(|arg| mentions_any(arg, ids)),
        Type::Io(inner) => mentions_any(inner, ids),
        _ => false,
    }
}

/// Whether an expression calls `symbol` or `symfun`.
fn is_symbol_factory(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Call { callee, .. } => match &callee.kind {
            ExprKind::Var(name) => matches!(name.as_str(), "symbol" | "symfun"),
            _ => false,
        },
        _ => false,
    }
}

/// Whether an expression is the bare literal `0` (§4.7).
fn is_bare_zero(expr: &Expr) -> bool {
    matches!(&expr.kind, ExprKind::Int(value) if value.is_zero())
}

/// The value of a literal index, when it is one (§3.5.4).
fn literal_index(expr: &Expr) -> Option<i64> {
    match &expr.kind {
        ExprKind::Int(value) => value.to_i64(),
        ExprKind::Unary {
            op: UnOp::Neg,
            operand,
        } => literal_index(operand).map(|value| -value),
        _ => None,
    }
}

/// Whether a type mentions a function anywhere.
fn contains_function(ty: &Type) -> bool {
    match ty {
        Type::Fn(..) => true,
        Type::Tensor { elem, .. } => contains_function(elem),
        Type::Record(row) => row
            .fields()
            .iter()
            .any(|(_, field)| contains_function(field)),
        Type::Adt(_, args) => args.iter().any(contains_function),
        Type::Io(inner) => contains_function(inner),
        _ => false,
    }
}

/// The least reading of a literal that is above `concrete` (§3.4.1).
///
/// A literal has a finite set of representations; the default is preferred
/// when it is high enough, otherwise the first accepted reading above the
/// bound.
fn literal_reading_above(kind: LiteralKind, concrete: Rep) -> Option<Rep> {
    let default = Rep::default_for_literal(kind);
    if kind.accepts_rep(default) && default.at_least(concrete) {
        return Some(default);
    }
    Rep::upper_candidates(concrete)
        .iter()
        .copied()
        .find(|candidate| kind.accepts_rep(*candidate))
}

fn annotation_span(ty: &Ty) -> Span {
    ty.span
}

fn literal_exponent(expr: &Expr) -> Option<BigRational> {
    match &expr.kind {
        ExprKind::Int(value) => value
            .to_i64()
            .map(|value| BigRational::from_integer(BigInt::from(value))),
        ExprKind::Unary {
            op: UnOp::Neg,
            operand,
        } => literal_exponent(operand).map(|value| -value),
        ExprKind::Binary {
            op: BinOp::Div,
            left,
            right,
        } => match (&left.kind, &right.kind) {
            (ExprKind::Int(a), ExprKind::Int(b)) if !b.is_zero() => {
                let a = BigRational::from_integer(a.clone());
                let b = BigRational::from_integer(b.clone());
                Some(a / b)
            }
            _ => None,
        },
        _ => None,
    }
}

fn ctor_scheme(adt: adt::AdtId, index: usize) -> Scheme {
    let Some(registered) = adt::get(adt) else {
        return Scheme::mono(Type::Error);
    };
    let Some(ctor) = registered.ctors.get(index) else {
        return Scheme::mono(Type::Error);
    };
    let params: Vec<Type> = (0..registered.params.len())
        .map(|index| Type::Var(u32::try_from(index).unwrap_or(0)))
        .collect();
    let result = Type::Adt(adt, params.clone());
    let fields: Vec<Type> = ctor
        .fields
        .iter()
        .map(|field| field.substitute_params(&params))
        .collect();
    let ty = if fields.is_empty() {
        result
    } else {
        Type::func(fields, result)
    };
    Scheme {
        vars: (0..u32::try_from(registered.params.len()).unwrap_or(0)).collect(),
        dims: Vec::new(),
        rows: Vec::new(),
        constraints: Vec::new(),
        ty,
    }
}

fn builtin_type(name: &str) -> Option<Type> {
    const OPAQUE: [&str; 12] = [
        "EvalError",
        "MathError",
        "DataError",
        "IOError",
        "LinAlgError",
        "ShapeError",
        "ParseError",
        "OpError",
        "DimError",
        "SimError",
        "PluginError",
        "Dynamic",
    ];
    if OPAQUE.contains(&name) {
        let id = adt::intern(Adt {
            name: name.to_string(),
            params: Vec::new(),
            ctors: Vec::new(),
        });
        return Some(Type::Adt(id, Vec::new()));
    }
    None
}

/// The class methods of §3.8.4 that the prelude declares.
///
/// Their type is `∀a. C a => σ`, which is what makes `show(f)` and `f == f`
/// errors: a function has no `Show`/`Eq` instance.
fn prelude_class_methods() -> Vec<(&'static str, &'static str, Type)> {
    let var = Type::Var(0);
    let ordering = Type::Adt(adt::ORDERING_ID, Vec::new());
    vec![
        ("Show", "show", Type::func(vec![var.clone()], Type::Str)),
        ("Show", "repr", Type::func(vec![var.clone()], Type::Str)),
        ("Show", "to_latex", Type::func(vec![var.clone()], Type::Str)),
        ("Show", "to_typst", Type::func(vec![var.clone()], Type::Str)),
        (
            "Ord",
            "compare",
            Type::func(vec![var.clone(), var.clone()], Type::option(ordering.clone())),
        ),
        (
            "TotalOrd",
            "total_compare",
            Type::func(vec![var.clone(), var.clone()], ordering),
        ),
        ("Hash", "hash", Type::func(vec![var], Type::int())),
    ]
}

/// Functions of §3.4.4 that only accept dimensionless arguments.
fn is_transcendental(name: &str) -> bool {
    matches!(
        name,
        "exp"
            | "ln"
            | "log"
            | "log2"
            | "log10"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "sinh"
            | "cosh"
            | "tanh"
            | "asinh"
            | "acosh"
            | "atanh"
            | "gamma"
            | "lgamma"
            | "erf"
            | "erfc"
            | "zeta"
    )
}


/// The flat key a qualified field access names, when the base is a path or an
/// import alias.
fn qualified_name(base: &Expr, field: &str) -> Option<String> {
    match &base.kind {
        ExprKind::QName(path) => Some(format!("{}.{field}", path.join("."))),
        ExprKind::Var(name) if name.chars().next().is_some_and(char::is_uppercase) => {
            Some(format!("{name}.{field}"))
        }
        _ => None,
    }
}

/// Whether an expression is `Rssn.<member>`.
fn is_rssn_member(expr: &Expr, member: &str) -> bool {
    match &expr.kind {
        ExprKind::Field { base, name } => name == member && is_rssn_root(base),
        ExprKind::QName(path) => path.len() == 2 && path[0] == "Rssn" && path[1] == member,
        _ => false,
    }
}

/// The operator name when an expression names `Rssn.<op>`.
///
/// The manifest's non-operator members (`Rssn.call`, `Rssn.Version`, …) are
/// excluded: they have a fixed type and are not resolved against overloads.
fn rssn_op_name(expr: &Expr) -> Option<String> {
    const MEMBERS: [&str; 11] = [
        "Version",
        "ManifestHash",
        "Capabilities",
        "Units",
        "Constants",
        "ValueKinds",
        "call",
        "dynamic",
        "call_key",
        "invoke",
        "resolve_overload",
    ];
    let name = match &expr.kind {
        ExprKind::Field { base, name } if is_rssn_root(base) => name.clone(),
        ExprKind::QName(path) if path.len() == 2 && path[0] == "Rssn" => path[1].clone(),
        _ => return None,
    };
    if MEMBERS.contains(&name.as_str()) || name == "overloads" {
        return None;
    }
    Some(name)
}

/// Whether an expression names the `Rssn` module itself.
fn is_rssn_root(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Var(name) => name == "Rssn",
        ExprKind::QName(path) => path.len() == 1 && path[0] == "Rssn",
        _ => false,
    }
}

/// `Tensor<1, Value>`: the argument list of a dynamic call.
fn value_list_type() -> Type {
    Type::tensor(Shape::Rank(1), dynamic_value_type())
}

/// Maps a manifest signature type to the checker's type.
fn rssn_type(sig: &aevia_rssn::SigType) -> Type {
    use aevia_rssn::SigType as S;
    match sig {
        S::Int => Type::int(),
        S::Real => Type::real(Dim::one()),
        S::Float => Type::float(Dim::one()),
        S::Complex => Type::complex(Dim::one()),
        S::Bool => Type::Bool,
        S::Str => Type::Str,
        S::Symbol => Type::Dyn(Rep::Real),
        S::Unit(dim) => Type::UnitOf(dim.clone()),
        S::Point(dim) => Type::Point(dim.clone()),
        S::Dim(inner, dim) => match **inner {
            S::Int => Type::Scalar(Rep::Int, dim.clone()),
            S::Float => Type::Scalar(Rep::Float, dim.clone()),
            S::Complex => Type::Scalar(Rep::Complex, dim.clone()),
            _ => Type::Scalar(Rep::Real, dim.clone()),
        },
        S::Tensor { rank, elem } => {
            let shape = Shape::Rank(rank.map_or(1, |rank| u32::try_from(rank).unwrap_or(1)));
            Type::tensor(shape, rssn_type(elem))
        }
        S::Sparse { rank, elem } => {
            let shape = Shape::Rank(rank.map_or(1, |rank| u32::try_from(rank).unwrap_or(1)));
            Type::tensor(shape, rssn_type(elem))
        }
        S::Record(fields) => Type::record(
            fields
                .iter()
                .map(|(name, ty)| (name.clone(), rssn_type(ty))),
        ),
        S::Func(params, ret) => Type::Fn(
            params.iter().map(rssn_type).collect(),
            Box::new(rssn_type(ret)),
        ),
        S::Table | S::Device | S::Opaque(_) | S::Var(_) | S::Variant(_) => dynamic_value_type(),
        S::Dyn | S::Never => Type::Dyn(Rep::Real),
    }
}

impl Checker {
    /// Types `Rssn.<name>` in *value* position.
    ///
    /// An overloaded name is not a first-class value (PLAN D1): unless every
    /// overload has the same type, using it without a call is `E0303`.
    fn rssn_member(&mut self, name: &str, span: Span) -> Type {
        match name {
            "Version" | "ManifestHash" => return Type::Str,
            "Capabilities" | "Units" | "Constants" | "ValueKinds" => {
                return Type::tensor(Shape::Rank(1), Type::Str);
            }
            "call" | "dynamic" => {
                return Type::Fn(
                    vec![Type::Str, value_list_type()],
                    Box::new(Type::Adt(
                        adt::RESULT_ID,
                        vec![dynamic_value_type(), op_error_type()],
                    )),
                );
            }
            "call_key" => {
                return Type::Fn(
                    vec![Type::Str, Type::Str, value_list_type()],
                    Box::new(Type::Adt(
                        adt::RESULT_ID,
                        vec![dynamic_value_type(), op_error_type()],
                    )),
                );
            }
            "invoke" => {
                // As a value `invoke` is a dynamic call: its result type is
                // only known per call site, never once for the module.
                return Type::Fn(
                    vec![Type::Str, Type::Str, value_list_type()],
                    Box::new(dynamic_value_type()),
                );
            }
            "resolve_overload" => {
                return Type::Fn(
                    vec![Type::Str, value_list_type()],
                    Box::new(Type::Adt(
                        adt::RESULT_ID,
                        vec![Type::Str, op_error_type()],
                    )),
                );
            }
            "overloads" => {
                return Type::Fn(
                    vec![Type::Str],
                    Box::new(Type::tensor(Shape::Rank(1), Type::Str)),
                );
            }
            _ => {}
        }
        let Some(entry) = aevia_rssn::registry().op(name) else {
            self.error_help(
                "E0301",
                format!("rssn has no operator `{name}`"),
                span,
                "run `aevia rssn --audit` for the operators this build provides",
            );
            return Type::Error;
        };
        if entry.signatures.is_empty() {
            return Type::Fn(vec![value_list_type()], Box::new(dynamic_value_type()));
        }
        let types: Vec<Type> = entry
            .signatures
            .iter()
            .map(|signature| self.rssn_signature_type(signature))
            .collect();
        let first = types.first().cloned().unwrap_or(Type::Error);
        if types.iter().all(|ty| *ty == first) {
            return first;
        }
        self.error_help(
            "E0303",
            format!("`Rssn.{name}` is overloaded and cannot be used as a value"),
            span,
            "call it, or bind the result of a call to a name",
        );
        Type::Error
    }

    /// Types a call to `Rssn.<op>` by resolving the overload at the call site.
    fn rssn_call(&mut self, name: &str, args: &[Expr], span: Span) -> Type {
        let Some(entry) = aevia_rssn::registry().op(name) else {
            for arg in args {
                self.synth(arg);
            }
            self.error_help(
                "E0301",
                format!("rssn has no operator `{name}`"),
                span,
                "run `aevia rssn --audit` for the operators this build provides",
            );
            return Type::Error;
        };
        let mut arg_types = Vec::new();
        let mut classified = Vec::new();
        for arg in args {
            let ty = self.synth(arg);
            let resolved = self.infer.resolve(&ty);
            classified.push(self.classify_arg(&resolved));
            arg_types.push(resolved);
        }
        match crate::rssn_resolve::resolve(entry, &classified) {
            crate::rssn_resolve::Resolution::Unique { index, .. } => {
                let Some(signature) = entry.signatures.get(index) else {
                    return Type::Error;
                };
                let ret = rssn_type(&signature.ret);
                if let Some(key) = entry.key(index) {
                    self.checked.rssn_calls.push(RssnCall {
                        span,
                        name: name.to_string(),
                        key: key.clone(),
                        overload: index,
                    });
                }
                ret
            }
            crate::rssn_resolve::Resolution::Ambiguous { candidates } => {
                let rendered: Vec<String> = candidates
                    .iter()
                    .filter_map(|index| entry.signatures.get(*index))
                    .map(aevia_rssn::Sig::render)
                    .collect();
                self.diagnostics.push(
                    Diagnostic::error(
                        "E1002",
                        format!("`Rssn.{name}` is ambiguous for these arguments"),
                        span,
                    )
                    .with_primary_label("several overloads match equally well")
                    .with_note(format!("candidates: {}", rendered.join("; ")))
                    .with_help("annotate an argument so one overload wins"),
                );
                Type::Error
            }
            crate::rssn_resolve::Resolution::None => {
                let rendered: Vec<String> = entry
                    .signatures
                    .iter()
                    .map(aevia_rssn::Sig::render)
                    .collect();
                self.diagnostics.push(
                    Diagnostic::error(
                        "E1003",
                        format!("no overload of `Rssn.{name}` accepts these arguments"),
                        span,
                    )
                    .with_primary_label("no candidate accepts this argument list")
                    .with_note(format!("overloads: {}", rendered.join("; ")))
                    .with_help("use `Rssn.dynamic` for a signature-free call"),
                );
                Type::Error
            }
            crate::rssn_resolve::Resolution::Unknown => {
                self.error("E0301", format!("rssn has no operator `{name}`"), span);
                Type::Error
            }
        }
    }

    /// Classifies an argument for overload resolution.
    ///
    /// A literal that has not been committed yet is classified by the
    /// representation it defaults to: `sin(0.5)` must pick the numeric
    /// overload, not tie with the symbolic one.
    fn classify_arg(&self, ty: &Type) -> aevia_rssn::SigType {
        match ty {
            Type::Var(_) => match self.infer.literal_kind(ty) {
                Some(kind) => crate::rssn_resolve::classify(&Type::Scalar(
                    Rep::default_for_literal(kind),
                    Dim::one(),
                )),
                None => aevia_rssn::SigType::Dyn,
            },
            Type::Scalar(Rep::Var(id), dim) => {
                match self.infer.rep_literal_kind(Rep::Var(*id)) {
                    Some(kind) => crate::rssn_resolve::classify(&Type::Scalar(
                        Rep::default_for_literal(kind),
                        dim.clone(),
                    )),
                    None => aevia_rssn::SigType::Dyn,
                }
            }
            other => crate::rssn_resolve::classify(other),
        }
    }

    /// The Aevia type of one manifest signature.
    fn rssn_signature_type(&mut self, signature: &aevia_rssn::Sig) -> Type {
        let mut params: Vec<Type> = signature.params.iter().map(rssn_type).collect();
        if signature.variadic.is_some() {
            params.push(Type::tensor(Shape::Rank(1), dynamic_value_type()));
        }
        Type::Fn(params, Box::new(rssn_type(&signature.ret)))
    }

    /// Types a call to a class method and solves its constraints.
    fn class_method_call(
        &mut self,
        name: &str,
        scheme: &Scheme,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let (ty, constraints) = self.infer.instantiate_scheme(scheme);
        let ret = self.call_with_type(ty, args, span);
        for constraint in &constraints {
            self.solve_constraint(constraint, span);
            if let Some(arg) = constraint.args.first() {
                self.checked.dictionaries.push(DictionaryUse {
                    span,
                    class: constraint.class.clone(),
                    head: self.infer.resolve(arg),
                    method: name.to_string(),
                });
            }
        }
        ret
    }
}

fn shape_items(shape: &Shape) -> Vec<Option<usize>> {
    match shape {
        Shape::Rank(rank) => vec![None; usize::try_from(*rank).unwrap_or(0)],
        Shape::Refined(items) => items.clone(),
    }
}
