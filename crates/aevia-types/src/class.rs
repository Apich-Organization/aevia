//! Type classes: declarations, instances, and head-matching resolution (§3.8).
//!
//! Resolution is static (PLAN D3): a constraint `C τ` is solved against the
//! instance set when `τ` becomes concrete, and the chosen instance is recorded
//! in [`crate::check::CheckedModule::dictionaries`] so the runtime can execute
//! exactly the instance the checker proved.
//!
//! There is no backtracking (§3.8.2): the head of an instance is matched
//! structurally, and two instances whose heads overlap are rejected when they
//! are declared (`E0311`).

use crate::keys::ClassKey;
use crate::keys::InstanceKey;
use crate::keys::ModulePath;
use crate::keys::head_key;
use crate::keys::witness_of;
use crate::ty::Rep;
use crate::ty::Row;
use crate::ty::Scheme;
use crate::ty::Shape;
use crate::ty::Type;
use std::collections::HashMap;

/// A class declaration.
#[derive(Clone, Debug)]
pub struct ClassDef {
    /// Number of parameters. The language has exactly one (§3.8.1, round-2
    /// decision 1f); the field stays so a reader of an old interface can be
    /// rejected instead of misread.
    pub params: usize,
    /// Methods, with their types written in terms of [`Type::Param`].
    pub methods: Vec<(String, Type)>,
    /// Whether the class is part of the prelude.
    pub builtin: bool,
    /// The symbolic identity: `(defining module, name)`.
    pub key: ClassKey,
}

/// Where an instance came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    /// A prelude instance.
    Prelude,
    /// A user `instance` declaration in this module.
    User,
    /// An instance that arrived with an imported module interface. The orphan
    /// rule is a declaration rule, so it is not re-applied here.
    Imported,
    /// A `derive` declaration.
    Derived,
}

/// One instance.
#[derive(Clone, Debug)]
pub struct Instance {
    /// The class name, local to the defining module.
    pub class: String,
    /// The instance head.
    pub head: Type,
    /// The specialised type of each method.
    pub methods: HashMap<String, Type>,
    /// Where it came from.
    pub origin: Origin,
    /// The symbolic identity.
    pub key: InstanceKey,
}

/// Why an instance could not be registered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstanceError {
    /// No such class.
    UnknownClass,
    /// Two instances have overlapping heads (`E0311`). Both keys are carried so
    /// the diagnostic can name the modules that disagree.
    Overlapping {
        /// The instance being registered.
        incoming: InstanceKey,
        /// The instance already in the table.
        existing: InstanceKey,
    },
    /// The instance is for a foreign type and a foreign class (`E0604`).
    Orphan,
    /// A `class` declared more than one parameter (§3.8.1, round-2 decision 1f).
    MultiParameter,
    /// Two statically distinct instances cannot be told apart by the
    /// information preserved by the runtime (`E0311`).
    WitnessCollision {
        /// The instance being registered.
        incoming: InstanceKey,
        /// The instance already in the table.
        existing: InstanceKey,
    },
}

/// Why a `derive` failed (§3.8.5, round-2 decision 1e).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeriveError {
    /// No such class in scope.
    UnknownClass(String),
    /// The class is not in the closed structural set
    /// `{Eq, Ord, TotalOrd, Show, Hash}`.
    UnsupportedClass(String),
    /// A field has no instance of the class, so a structural derive would be a
    /// lie. Both the field and the class are carried for the message.
    Field {
        /// The class being derived.
        class: String,
        /// The field type that cannot support it.
        field: Type,
    },
    /// A function field can never have the structural instances.
    FunctionField {
        /// The class being derived.
        class: String,
        /// The field type (containing the function).
        field: Type,
    },
    /// An instance for the same class and head is already registered.
    Overlapping(ClassKey),
    /// The derived type is foreign and the class is a builtin class, so the
    /// `derive` is an orphan instance (`E0604`).
    Orphan,
}

/// Why a class declaration failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClassError {
    /// The name is already taken in this module (`E0316`).
    Duplicate,
    /// More than one parameter was written (§3.8.1: the language has exactly
    /// one).
    MultiParameter,
}

/// The methods a prelude class declares, with their types in terms of
/// [`Type::Param`] (the class parameter).
///
/// A prelude class allows a *partial* override: a user instance may define some
/// methods and let the built-in structural implementation cover the rest
/// (round-2 decision 1e). A user class requires every method.
fn prelude_methods(class: &str) -> Vec<(String, Type)> {
    let param = Type::Param(0);
    let ordering = Type::Adt(crate::adt::ORDERING_ID, Vec::new());
    let func = |args: Vec<Type>, ret: Type| Type::func(args, ret);
    match class {
        | "Show" => {
            vec![
                ("show".to_string(), func(vec![param.clone()], Type::Str)),
                ("repr".to_string(), func(vec![param.clone()], Type::Str)),
                ("to_latex".to_string(), func(vec![param.clone()], Type::Str)),
                ("to_typst".to_string(), func(vec![param], Type::Str)),
            ]
        },
        | "Ord" => {
            vec![(
                "compare".to_string(),
                func(vec![Type::Param(0), Type::Param(0)], Type::option(ordering)),
            )]
        },
        | "TotalOrd" => {
            vec![(
                "total_compare".to_string(),
                func(
                    vec![Type::Param(0), Type::Param(0)],
                    Type::Adt(crate::adt::ORDERING_ID, Vec::new()),
                ),
            )]
        },
        | "Hash" => vec![("hash".to_string(), func(vec![Type::Param(0)], Type::int()))],
        | _ => Vec::new(),
    }
}

/// The result of solving one constraint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Exactly one instance matched.
    Found {
        /// The index of the instance in its class's list.
        index: usize,
    },
    /// The type still contains variables; try again at the end of the module.
    Deferred,
    /// No instance matches a concrete type (`E0310`).
    Missing,
}

/// The instance table of one compilation unit.
#[derive(Clone, Debug, Default)]
pub struct ClassTable {
    classes: HashMap<String, ClassDef>,
    instances: HashMap<String, Vec<Instance>>,
}

impl ClassTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The prelude classes and their instances (§3.8.4).
    ///
    /// `Real` deliberately has no `Eq` instance: comparison on `Real` yields a
    /// `Prop` (§3.8.6). Functions have no `Eq` and no `Show`. `Float` has `Eq`
    /// but not `TotalOrd`, so `Set<Float>` is rejected.
    #[must_use]
    pub fn prelude() -> Self {
        let mut table = Self::new();
        for (name, arity) in [
            ("Eq", 1),
            ("Ord", 1),
            ("TotalOrd", 1),
            ("Show", 1),
            ("Hash", 1),
            ("Num", 1),
            ("Fractional", 1),
            ("Floating", 1),
            ("Container", 1),
            ("Sized", 1),
            // §3.8.4: `par_map`'s capability class. Symbolic `Real` is not
            // parallel, so it has no instance.
            ("Parallel", 1),
        ] {
            let methods = prelude_methods(name);
            table.classes.insert(
                name.to_string(),
                ClassDef {
                    params: arity,
                    methods,
                    builtin: true,
                    key: ClassKey::prelude(name),
                },
            );
        }

        let int = Type::int();
        let float = Type::float(aevia_dim::Dim::one());
        let real = Type::real(aevia_dim::Dim::one());
        let complex = Type::complex(aevia_dim::Dim::one());
        let point = Type::Point(aevia_dim::Dim::one());
        let tensor = Type::tensor(Shape::Rank(1), Type::Param(0));
        let option = Type::option(Type::Param(0));

        for head in [
            int.clone(),
            Type::Bool,
            Type::Str,
            Type::Unit,
            Type::Prop,
            float.clone(),
            complex.clone(),
            point.clone(),
            tensor.clone(),
            option.clone(),
            Type::Adt(crate::adt::ORDERING_ID, Vec::new()),
        ] {
            table.add_prelude("Eq", head);
        }
        for head in [
            int.clone(),
            Type::Bool,
            Type::Str,
            Type::Unit,
            float.clone(),
            Type::Adt(crate::adt::ORDERING_ID, Vec::new()),
        ] {
            table.add_prelude("Ord", head);
        }
        for head in [
            int.clone(),
            Type::Bool,
            Type::Str,
            Type::Unit,
            Type::Adt(crate::adt::ORDERING_ID, Vec::new()),
        ] {
            table.add_prelude("TotalOrd", head);
        }
        for head in [
            int.clone(),
            Type::Bool,
            Type::Str,
            Type::Unit,
            real.clone(),
            float.clone(),
            complex.clone(),
            point,
            tensor.clone(),
            option,
        ] {
            table.add_prelude("Show", head);
        }
        for head in [
            int.clone(),
            Type::Bool,
            Type::Str,
            Type::Unit,
            float.clone(),
        ] {
            table.add_prelude("Hash", head);
        }
        for head in [int.clone(), real.clone(), float.clone(), complex.clone()] {
            table.add_prelude("Num", head);
        }
        for head in [real.clone(), float.clone(), complex.clone()] {
            table.add_prelude("Fractional", head.clone());
            table.add_prelude("Floating", head);
        }
        for head in [tensor, Type::Str] {
            table.add_prelude("Container", head);
        }
        // §3.8.4: `Parallel a` — only `Int`, `Float` and tensors of them. The
        // symbolic `Real` path is session-bound and not parallel.
        for elem in [int.clone(), float.clone()] {
            table.add_prelude("Parallel", elem.clone());
            table.add_prelude("Parallel", Type::tensor(Shape::Rank(1), elem.clone()));
            table.add_prelude("Parallel", Type::tensor(Shape::Rank(2), elem));
        }
        for head in [Type::Str, Type::tensor(Shape::Rank(1), Type::Param(0))] {
            table.add_prelude("Sized", head);
        }
        table
    }

    fn add_prelude(
        &mut self,
        class: &str,
        head: Type,
    ) {
        let class_key = ClassKey::prelude(class);
        let key = InstanceKey::new(ModulePath::prelude(), class_key.clone(), head_key(&head));
        let instances = self.instances.entry(class.to_string()).or_default();
        instances.push(Instance {
            class: class.to_string(),
            head,
            methods: HashMap::new(),
            origin: Origin::Prelude,
            key,
        });
    }

    /// Declares a class.
    ///
    /// # Errors
    ///
    /// `E0316` when the name is already taken by another class, or
    /// `E0317`-worthy `InstanceError::MultiParameter` when more than one
    /// parameter is written (§3.8.1 — the language has exactly one).
    pub fn declare_class(
        &mut self,
        module: ModulePath,
        name: &str,
        params: usize,
        methods: Vec<(String, Type)>,
    ) -> Result<(), ClassError> {
        if self.classes.contains_key(name) {
            return Err(ClassError::Duplicate);
        }
        if params > 1 {
            return Err(ClassError::MultiParameter);
        }
        self.classes.insert(
            name.to_string(),
            ClassDef {
                params,
                methods,
                builtin: false,
                key: ClassKey::new(module, name),
            },
        );
        Ok(())
    }

    /// Whether a class exists.
    #[must_use]
    pub fn has_class(
        &self,
        name: &str,
    ) -> bool {
        self.classes.contains_key(name)
    }

    /// The class declaration.
    #[must_use]
    pub fn class(
        &self,
        name: &str,
    ) -> Option<&ClassDef> {
        self.classes.get(name)
    }

    /// The symbolic key of a declared class.
    #[must_use]
    pub fn class_key(
        &self,
        name: &str,
    ) -> Option<ClassKey> {
        self.classes.get(name).map(|class| class.key.clone())
    }

    /// Whether a class is declared by the module that owns this table (as
    /// opposed to being imported or built in).
    #[must_use]
    pub fn class_is_local(
        &self,
        module: &ModulePath,
        name: &str,
    ) -> bool {
        self.classes
            .get(name)
            .is_some_and(|class| &class.key.module == module)
    }

    /// The class that declares a method, if any.
    #[must_use]
    pub fn class_of_method(
        &self,
        method: &str,
    ) -> Option<(&str, &Type)> {
        for (name, class) in &self.classes {
            if let Some((_, ty)) = class.methods.iter().find(|(m, _)| m == method) {
                return Some((name, ty));
            }
        }
        None
    }

    /// Registers an instance.
    ///
    /// `module` is the module that declares it; `head_is_local` and
    /// `class_is_local` say whether the *type* and the *class* are declared by
    /// the module being checked — together they decide the orphan rule
    /// (§3.8.3). An imported instance is never re-checked for orphanhood: it
    /// was legal where it was declared.
    ///
    /// # Errors
    ///
    /// `UnknownClass` for a class that does not exist, `Overlapping` when the
    /// head overlaps an existing instance (`E0311`, carrying both keys so the
    /// diagnostic can name the modules), and `Orphan` when neither the head nor
    /// the class is local (`E0604`).
    pub fn add_instance(
        &mut self,
        module: &ModulePath,
        class: &str,
        head: Type,
        methods: HashMap<String, Type>,
        origin: Origin,
        head_is_local: bool,
        class_is_local: bool,
    ) -> Result<InstanceKey, InstanceError> {
        let Some(definition) = self.classes.get(class) else {
            return Err(InstanceError::UnknownClass);
        };
        // §3.8.3: an instance belongs either to the module that declares the
        // type or to the module that declares the class. The rule is a
        // *declaration* rule; an instance that came with an interface already
        // passed it where it was declared.
        if matches!(origin, Origin::User | Origin::Derived) && !head_is_local && !class_is_local {
            return Err(InstanceError::Orphan);
        }
        let class_key = definition.key.clone();
        let key = InstanceKey::new(module.clone(), class_key, head_key(&head));
        let instances = self.instances.entry(class.to_string()).or_default();
        if let Some(existing) = instances
            .iter()
            .find(|existing| heads_overlap(&existing.head, &head))
        {
            return Err(InstanceError::Overlapping {
                incoming: key,
                existing: existing.key.clone(),
            });
        }
        // Statically distinct heads can still be indistinguishable at runtime.
        // Reject such pairs instead of choosing an arbitrary instance.
        if let Some(existing) = instances
            .iter()
            .find(|existing| runtime_heads_overlap(&existing.head, &head))
        {
            return Err(InstanceError::WitnessCollision {
                incoming: key,
                existing: existing.key.clone(),
            });
        }
        instances.push(Instance {
            class: class.to_string(),
            head,
            methods,
            origin,
            key: key.clone(),
        });
        Ok(key)
    }

    /// Solves `class τ` against the instance set (§3.8.2).
    #[must_use]
    pub fn resolve(
        &self,
        class: &str,
        ty: &Type,
    ) -> Resolution {
        let Some(instances) = self.instances.get(class) else {
            return Resolution::Missing;
        };
        let mut found = None;
        for (index, instance) in instances.iter().enumerate() {
            if matches_head(&instance.head, ty) {
                if found.is_some() {
                    // Coherence is enforced at registration, so this can only
                    // happen for heads that were not compared there.
                    return Resolution::Missing;
                }
                found = Some(index);
            }
        }
        match found {
            | Some(index) => Resolution::Found { index },
            | None if has_variables(ty) => Resolution::Deferred,
            | None => Resolution::Missing,
        }
    }

    /// The type of `method` for the instance that matches `ty`.
    #[must_use]
    pub fn method_type(
        &self,
        class: &str,
        ty: &Type,
        method: &str,
    ) -> Option<Type> {
        let Resolution::Found { index } = self.resolve(class, ty) else {
            return None;
        };
        let instance = self.instances.get(class)?.get(index)?;
        instance.methods.get(method).cloned()
    }

    /// The symbolic key of the instance that matches `ty`, when one does.
    #[must_use]
    pub fn resolve_key(
        &self,
        class: &str,
        ty: &Type,
    ) -> Option<InstanceKey> {
        let Resolution::Found { index } = self.resolve(class, ty) else {
            return None;
        };
        self.instance_key(class, index)
    }

    /// The key of an instance by position.
    #[must_use]
    pub fn instance_key(
        &self,
        class: &str,
        index: usize,
    ) -> Option<InstanceKey> {
        self.instances
            .get(class)?
            .get(index)
            .map(|instance| instance.key.clone())
    }

    /// The instances of a class, with their keys, in registration order.
    #[must_use]
    pub fn instances_of(
        &self,
        class: &str,
    ) -> Vec<&Instance> {
        self.instances
            .get(class)
            .map(|instances| instances.iter().collect())
            .unwrap_or_default()
    }

    /// Every class declaration, for export and tests.
    #[must_use]
    pub fn classes(&self) -> Vec<&ClassDef> {
        self.classes.values().collect()
    }

    /// Every instance whose key's module is `module`, for export.
    ///
    /// A prelude instance is not exported by a user module; an imported one is
    /// not re-exported (imports are not transitive, §7.2.1).
    #[must_use]
    pub fn instances_declared_in(
        &self,
        module: &ModulePath,
    ) -> Vec<&Instance> {
        self.instances
            .values()
            .flatten()
            .filter(|instance| &instance.key.module == module)
            .collect()
    }

    /// Every instance of a class, for diagnostics.
    #[must_use]
    pub fn instance_heads(
        &self,
        class: &str,
    ) -> Vec<Type> {
        self.instances
            .get(class)
            .map(|instances| {
                instances
                    .iter()
                    .map(|instance| instance.head.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The symbolic keys of every instance of a class.
    #[must_use]
    pub fn instance_keys(
        &self,
        class: &str,
    ) -> Vec<InstanceKey> {
        self.instances
            .get(class)
            .map(|instances| {
                instances
                    .iter()
                    .map(|instance| instance.key.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The number of registered instances, for tests.
    #[must_use]
    pub fn instance_count(&self) -> usize {
        self.instances.values().map(Vec::len).sum()
    }

    /// The classes a module declares, with their methods, for the interface.
    #[must_use]
    pub fn classes_declared_in(
        &self,
        module: &ModulePath,
    ) -> Vec<&ClassDef> {
        self.classes
            .values()
            .filter(|class| &class.key.module == module)
            .collect()
    }

    /// Merges the classes and instances of an imported module interface.
    ///
    /// An imported instance is registered with [`Origin::Imported`], which
    /// skips the orphan rule (it was checked where it was declared) but keeps
    /// overlap (`E0311`): coherence is global. The returned error is the first
    /// rejection, with both keys so the caller can name the modules.
    ///
    /// # Errors
    ///
    /// `Overlapping` when an imported instance collides with an existing one.
    pub fn merge_imported(
        &mut self,
        module: &ModulePath,
        classes: &[(ClassKey, ClassDef)],
        instances: &[Instance],
    ) -> Result<(), InstanceError> {
        for (key, definition) in classes {
            self.classes
                .entry(key.name.clone())
                .or_insert_with(|| definition.clone());
        }
        for instance in instances {
            let class_locally_known = self.classes.contains_key(&instance.class);
            if !class_locally_known {
                // The class arrived with the instance's interface; keep the
                // instance name resolvable even when the class declaration was
                // not exported (a private class with a visible instance is not
                // possible, but an interface consumer must not drop data).
                continue;
            }
            let class_key = instance.key.class.clone();
            let key = InstanceKey::new(module.clone(), class_key, instance.key.head.clone());
            let entries = self.instances.entry(instance.class.clone()).or_default();
            if let Some(existing) = entries
                .iter()
                .find(|existing| heads_overlap(&existing.head, &instance.head))
            {
                return Err(InstanceError::Overlapping {
                    incoming: key,
                    existing: existing.key.clone(),
                });
            }
            if let Some(existing) = entries
                .iter()
                .find(|existing| runtime_heads_overlap(&existing.head, &instance.head))
            {
                return Err(InstanceError::WitnessCollision {
                    incoming: key,
                    existing: existing.key.clone(),
                });
            }
            let mut imported = instance.clone();
            imported.key = key;
            imported.origin = Origin::Imported;
            entries.push(imported);
        }
        Ok(())
    }

    /// Adds a `derive`d instance for a type, given the types of its fields.
    ///
    /// §3.8.5 and round-2 decision 1e: `derive` is closed to the structural set
    /// `{Eq, Ord, TotalOrd, Show, Hash}`. Any other class is `E0317`, and the
    /// error names the class or the field that cannot support the instance.
    ///
    /// `module` is the module the `derive` is written in and `head_is_local`
    /// whether the derived type is declared there: a `derive` on a foreign type
    /// is an orphan like any other instance (`E0604`), which the old hardcoded
    /// `true` bypassed.
    ///
    /// # Errors
    ///
    /// A [`DeriveError`] the caller maps to `E0317`/`E0310`/`E0311`/`E0604`.
    pub fn add_derived(
        &mut self,
        module: &ModulePath,
        class: &str,
        head: Type,
        fields: &[Type],
        head_is_local: bool,
        class_is_local: bool,
    ) -> Result<InstanceKey, DeriveError> {
        let Some(definition) = self.classes.get(class) else {
            return Err(DeriveError::UnknownClass(class.to_string()));
        };
        if !DERIVABLE.contains(&class) {
            return Err(DeriveError::UnsupportedClass(class.to_string()));
        }
        if definition.params != 1 {
            return Err(DeriveError::UnsupportedClass(class.to_string()));
        }
        let class_key = definition.key.clone();
        // §3.8.5: a function field has no `Eq`/`Show`/`Hash` instance.
        for field in fields {
            if field_has_function(field) {
                return Err(DeriveError::FunctionField {
                    class: class.to_string(),
                    field: field.clone(),
                });
            }
            if !self.instance_for_field(class, field) {
                return Err(DeriveError::Field {
                    class: class.to_string(),
                    field: field.clone(),
                });
            }
        }
        let methods = HashMap::new();
        match self.add_instance(
            module,
            class,
            head,
            methods,
            Origin::Derived,
            head_is_local,
            class_is_local,
        ) {
            | Ok(key) => Ok(key),
            | Err(InstanceError::Overlapping { .. }) => Err(DeriveError::Overlapping(class_key)),
            | Err(InstanceError::Orphan) => Err(DeriveError::Orphan),
            | Err(_) => Err(DeriveError::UnsupportedClass(class.to_string())),
        }
    }

    /// Whether a field type has an instance of `class`.
    fn instance_for_field(
        &self,
        class: &str,
        field: &Type,
    ) -> bool {
        let resolved = match field {
            | Type::Param(_) => return true,
            | other => other.clone(),
        };
        // `Show`/`Eq` of a container needs the element instance, which a
        // derived instance does not check transitively (§3.8.5 is structural).
        matches!(
            self.resolve(class, &resolved),
            Resolution::Found { .. } | Resolution::Deferred
        ) || (class == "Show" && is_structural_show_head(&resolved))
    }
}

/// The closed set of classes `derive` can instantiate (§3.8.5).
pub const DERIVABLE: &[&str] = &["Eq", "Ord", "TotalOrd", "Show", "Hash"];

fn field_has_function(ty: &Type) -> bool {
    match ty {
        | Type::Fn(..) => true,
        | Type::Tensor { elem, .. } => field_has_function(elem),
        | Type::Record(row) => row.fields().iter().any(|(_, ty)| field_has_function(ty)),
        | Type::Adt(_, args) => args.iter().any(field_has_function),
        | Type::Io(inner) => field_has_function(inner),
        | _ => false,
    }
}

/// Whether two instance heads can match the same type (§3.8.3).
///
/// Round-2 decision 1d: a tensor's rank and a record's label set are part of the
/// head, so heads that differ there are disjoint; treating them as overlapping
/// rejected legal instance sets, and treating them as matching would let an
/// instance silently capture a different type.
#[must_use]
pub fn heads_overlap(
    left: &Type,
    right: &Type,
) -> bool {
    match (left, right) {
        | (Type::Param(_) | Type::Var(_), _) | (_, Type::Param(_) | Type::Var(_)) => true,
        | (Type::Scalar(a, _), Type::Scalar(b, _)) => a == b || is_rep_var(*a) || is_rep_var(*b),
        | (Type::Bool, Type::Bool)
        | (Type::Prop, Type::Prop)
        | (Type::Str, Type::Str)
        | (Type::Unit, Type::Unit) => true,
        | (Type::Tensor { shape: sa, elem: a }, Type::Tensor { shape: sb, elem: b }) => {
            shape_heads_overlap(sa, sb) && heads_overlap(a, b)
        },
        | (Type::Point(_), Type::Point(_)) | (Type::UnitOf(_), Type::UnitOf(_)) => true,
        | (Type::Fn(pa, ra), Type::Fn(pb, rb)) => {
            pa.len() == pb.len()
                && pa.iter().zip(pb.iter()).all(|(a, b)| heads_overlap(a, b))
                && heads_overlap(ra, rb)
        },
        | (Type::Record(a), Type::Record(b)) => rows_overlap(a, b),
        | (Type::Adt(a, aa), Type::Adt(b, ab)) => {
            a == b
                && aa.len() == ab.len()
                && aa.iter().zip(ab.iter()).all(|(x, y)| heads_overlap(x, y))
        },
        | (Type::Io(a), Type::Io(b)) => heads_overlap(a, b),
        | (Type::Dyn(_), Type::Dyn(_)) => true,
        | _ => false,
    }
}

/// Whether runtime-observable values could satisfy both disjoint heads.
/// This is deliberately conservative for erased or unsupported types.
fn runtime_heads_overlap(left: &Type, right: &Type) -> bool {
    match (left, right) {
        | (Type::Tensor { shape: a, elem: ea }, Type::Tensor { shape: b, elem: eb }) => {
            if !shape_heads_overlap(a, b) {
                return false;
            }
            match (&**ea, &**eb) {
                | (Type::Scalar(ra, _), Type::Scalar(rb, _)) => {
                    // Typed exact storage preserves Int versus Real. Untagged
                    // buffers cannot dispatch either head and are never guessed.
                    *ra == *rb || is_rep_var(*ra) || is_rep_var(*rb)
                },
                | _ => true,
            }
        },
        | (Type::Record(a), Type::Record(b)) => {
            let af = a.fields();
            let bf = b.fields();
            if a.tail_var().is_none() && b.tail_var().is_none() && af.len() != bf.len() {
                return false;
            }
            for (name, ty) in &af {
                match bf.iter().find(|(other, _)| other == name) {
                    | Some((_, other)) if !runtime_heads_overlap(ty, other) => return false,
                    | None if b.tail_var().is_none() => return false,
                    | _ => {},
                }
            }
            if a.tail_var().is_none() && bf.iter().any(|(name, _)| !af.iter().any(|(n, _)| n == name)) {
                return false;
            }
            true
        },
        | (Type::Adt(a, aa), Type::Adt(b, ab)) => {
            crate::adt::key_of(*a) == crate::adt::key_of(*b)
                && aa.len() == ab.len()
                && aa.iter().zip(ab).all(|(a, b)| runtime_heads_overlap(a, b))
        },
        | _ => match (witness_of(left), witness_of(right)) {
            | (Some(a), Some(b)) => a == b,
            | _ => heads_overlap(left, right),
        },
    }
}

/// Whether `show`/`repr` of this type is guaranteed structurally (§3.8.4 and
/// round-2 decision 1e). The runtime's builtin printer is the fallback when no
/// user instance matches, so these heads do not need an `instance` declaration.
#[must_use]
pub fn is_structural_show_head(ty: &Type) -> bool {
    match ty {
        | Type::Scalar(..) | Type::Bool | Type::Prop | Type::Str | Type::Unit => true,
        | Type::Tensor { elem, .. } => is_structural_show_head(elem),
        // §4.9.4: a `Dyn` prints as the scalar it carries.
        | Type::Dyn(_) | Type::UnitOf(_) | Type::Io(_) => true,
        | Type::Record(row) => {
            row.tail_var().is_none()
                && row
                    .fields()
                    .iter()
                    .all(|(_, field)| is_structural_show_head(field))
        },
        // A nominal user type needs its own `Show` instance (or `derive`): the
        // printer cannot know a variant's field order or constructor names.
        | Type::Adt(..)
        | Type::Point(_)
        | Type::Fn(..)
        | Type::Var(_)
        | Type::Param(_)
        | Type::Error => false,
    }
}

/// Whether two tensor shapes can describe the same type.
fn shape_heads_overlap(
    left: &Shape,
    right: &Shape,
) -> bool {
    if left.rank() != right.rank() {
        return false;
    }
    match (left, right) {
        | (Shape::Refined(a), Shape::Refined(b)) => {
            a.iter().zip(b.iter()).all(|(left, right)| {
                match (left, right) {
                    | (Some(left), Some(right)) => match (left.as_const(), right.as_const()) {
                        | (Some(left), Some(right)) => left == right,
                        | _ => true,
                    },
                    | _ => true,
                }
            })
        },
        | _ => true,
    }
}

/// Whether a concrete tensor type can match a head shape.
fn shape_head_matches(
    pattern: &Shape,
    concrete: &Shape,
) -> bool {
    if pattern.rank() != concrete.rank() {
        return false;
    }
    match (pattern, concrete) {
        | (Shape::Refined(pattern), Shape::Refined(concrete)) => {
            pattern
                .iter()
                .zip(concrete.iter())
                .all(|(pattern, concrete)| {
                    match (pattern, concrete) {
                        | (Some(pattern), Some(concrete)) => match (pattern.as_const(), concrete.as_const()) {
                            | (Some(pattern), Some(concrete)) => pattern == concrete,
                            | _ => pattern == concrete,
                        },
                        | (Some(_), None) => false,
                        | (None, _) => true,
                    }
                })
        },
        // Unknown extents cannot prove a fixed extent instance.
        | (Shape::Refined(items), Shape::Rank(_)) => items.iter().all(Option::is_none),
        | _ => true,
    }
}

fn rows_overlap(
    left: &Row,
    right: &Row,
) -> bool {
    let left_fields = left.fields();
    let right_fields = right.fields();
    let left_open = left.tail_var().is_some();
    let right_open = right.tail_var().is_some();
    if !left_open && !right_open && left_fields.len() != right_fields.len() {
        // Both heads are closed records: a value's record type has exactly one
        // label set, so different sizes can never be the same type.
        return false;
    }
    for (label, ty) in &left_fields {
        if let Some((_, other)) = right_fields.iter().find(|(name, _)| name == label) {
            if !heads_overlap(ty, other) {
                return false;
            }
        } else if !right_open {
            // A closed right head lacks this label, so no value matches both.
            return false;
        }
    }
    for (label, _) in &right_fields {
        if !left_fields.iter().any(|(name, _)| name == label) && !left_open {
            return false;
        }
    }
    true
}

fn is_rep_var(rep: Rep) -> bool {
    matches!(rep, Rep::Var(_))
}

/// Structural head matching: `pattern` may contain wildcards, `concrete` may
/// not.
#[must_use]
pub fn matches_head(
    pattern: &Type,
    concrete: &Type,
) -> bool {
    matches_head_with_bindings(pattern, concrete, &mut HashMap::new())
}

fn matches_head_with_bindings(
    pattern: &Type,
    concrete: &Type,
    bindings: &mut HashMap<(bool, u32), Type>,
) -> bool {
    match (pattern, concrete) {
        | (Type::Param(id), _) | (Type::Var(id), _) => {
            let key = (matches!(pattern, Type::Param(_)), *id);
            match bindings.get(&key) {
                | Some(previous) => previous == concrete,
                | None => {
                    bindings.insert(key, concrete.clone());
                    true
                },
            }
        },
        | (Type::Scalar(a, _), Type::Scalar(b, _)) => {
            if is_rep_var(*a) {
                true
            } else {
                *a == *b
            }
        },
        | (Type::Bool, Type::Bool)
        | (Type::Prop, Type::Prop)
        | (Type::Str, Type::Str)
        | (Type::Unit, Type::Unit) => true,
        | (
            Type::Tensor {
                shape: pattern,
                elem: a,
            },
            Type::Tensor {
                shape: concrete,
                elem: b,
            },
        ) => shape_head_matches(pattern, concrete) && matches_head_with_bindings(a, b, bindings),
        | (Type::Point(_), Type::Point(_)) | (Type::UnitOf(_), Type::UnitOf(_)) => true,
        | (Type::Fn(pa, ra), Type::Fn(pb, rb)) => {
            pa.len() == pb.len()
                && pa
                    .iter()
                    .zip(pb.iter())
                    .all(|(a, b)| matches_head_with_bindings(a, b, bindings))
                && matches_head_with_bindings(ra, rb, bindings)
        },
        | (Type::Record(pattern), Type::Record(concrete)) => {
            let pattern_fields = pattern.fields();
            let concrete_fields = concrete.fields();
            let pattern_open = pattern.tail_var().is_some();
            if !pattern_open && (pattern_fields.len() != concrete_fields.len() || concrete.tail_var().is_some()) {
                return false;
            }
            // Every field the head states must be present in the concrete type
            // with a matching type; an open head allows extra fields.
            pattern_fields.iter().all(|(label, ty)| {
                concrete_fields
                    .iter()
                    .find(|(name, _)| name == label)
                    .is_some_and(|(_, other)| matches_head_with_bindings(ty, other, bindings))
            }) && (pattern_open || concrete_fields.len() == pattern_fields.len())
        },
        | (Type::Adt(a, aa), Type::Adt(b, ab)) => {
            a == b
                && aa.len() == ab.len()
                && aa
                    .iter()
                    .zip(ab.iter())
                    .all(|(x, y)| matches_head_with_bindings(x, y, bindings))
        },
        | (Type::Io(a), Type::Io(b)) => matches_head_with_bindings(a, b, bindings),
        | (Type::Dyn(_), Type::Dyn(_)) => true,
        | _ => false,
    }
}

/// Whether a type still contains a type, dimension or row variable.
#[must_use]
pub fn has_variables(ty: &Type) -> bool {
    match ty {
        | Type::Var(_) | Type::Param(_) => true,
        | Type::Scalar(rep, dim) => matches!(rep, Rep::Var(_)) || !dim.vars().is_empty(),
        | Type::Tensor { shape, elem } => shape.has_variables() || has_variables(elem),
        | Type::Fn(params, ret) => params.iter().any(has_variables) || has_variables(ret),
        | Type::Record(row) => row_has_variables(row),
        | Type::Adt(_, args) => args.iter().any(has_variables),
        | Type::Io(inner) => has_variables(inner),
        | Type::Point(dim) | Type::UnitOf(dim) => !dim.vars().is_empty(),
        | Type::Dyn(rep) => matches!(rep, Rep::Var(_)),
        | Type::Bool | Type::Prop | Type::Str | Type::Unit | Type::Error => false,
    }
}

fn row_has_variables(row: &Row) -> bool {
    match row {
        | Row::Empty => false,
        | Row::Var(_) => true,
        | Row::Extend { ty, tail, .. } => has_variables(ty) || row_has_variables(tail),
    }
}

/// The scheme of a class method: `∀a. C a => σ`.
///
/// `ty` is written in terms of `Type::Var(0..params)`; instantiation replaces
/// them, so the small ids never escape.
#[must_use]
pub fn method_scheme(
    class: &str,
    params: usize,
    ty: Type,
) -> Scheme {
    let count = u32::try_from(params).unwrap_or(0);
    // The class declaration stores its parameters as `Type::Param(i)` so that
    // an instance can substitute the head with `substitute_params`. A scheme
    // quantifies over `Type::Var(i)`, so convert here — the two spaces must not
    // be confused, or an instance head silently fails to replace the parameter.
    let quantified = ty.substitute_params(&(0..count).map(Type::Var).collect::<Vec<_>>());
    Scheme {
        vars: (0..count).collect(),
        dims: Vec::new(),
        rows: Vec::new(),
        nats: Vec::new(),
        constraints: vec![crate::ty::Constraint::new(
            class,
            (0..count).map(Type::Var).collect(),
        )],
        ty: quantified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adt::Ctor;

    #[test]
    fn the_prelude_has_the_specified_instances() {
        let table = ClassTable::prelude();
        assert!(matches!(
            table.resolve("Show", &Type::int()),
            Resolution::Found { .. }
        ));
        assert!(matches!(
            table.resolve("Eq", &Type::real(aevia_dim::Dim::one())),
            Resolution::Missing
        ));
        assert!(matches!(
            table.resolve("Eq", &Type::func(vec![Type::int()], Type::int())),
            Resolution::Missing
        ));
        assert!(matches!(
            table.resolve("TotalOrd", &Type::float(aevia_dim::Dim::one())),
            Resolution::Missing
        ));
    }

    #[test]
    fn a_dimensioned_real_has_show_but_not_eq() {
        let table = ClassTable::prelude();
        let length = aevia_dim::Dim::base("Length");
        assert!(matches!(
            table.resolve("Show", &Type::real(length.clone())),
            Resolution::Found { .. }
        ));
        assert!(matches!(
            table.resolve("Eq", &Type::real(length)),
            Resolution::Missing
        ));
    }

    #[test]
    fn overlapping_heads_are_rejected() {
        let mut table = ClassTable::prelude();
        let length = aevia_dim::Dim::base("Length");
        let error = table
            .add_instance(
                &ModulePath::new(["Test"]),
                "Show",
                Type::real(length),
                HashMap::new(),
                Origin::User,
                true,
                false,
            )
            .expect_err("Show Real[d] already exists");
        assert!(matches!(error, InstanceError::Overlapping { .. }));
    }

    #[test]
    fn a_foreign_instance_is_an_orphan() {
        let mut table = ClassTable::prelude();
        let error = table
            .add_instance(
                &ModulePath::new(["Test"]),
                "Show",
                Type::int(),
                HashMap::new(),
                Origin::User,
                false,
                false,
            )
            .expect_err("the user cannot add Show Int");
        assert_eq!(error, InstanceError::Orphan);
    }

    #[test]
    fn an_imported_instance_skips_the_orphan_rule() {
        let mut table = ClassTable::prelude();
        table
            .declare_class(ModulePath::new(["Test"]), "Pretty", 1, Vec::new())
            .expect("declare Pretty");
        // Imported from a module where the head was local: not re-checked for
        // orphanhood here (the head is foreign to this module).
        table
            .add_instance(
                &ModulePath::new(["Library"]),
                "Pretty",
                Type::record(vec![("x".to_string(), Type::int())]),
                HashMap::new(),
                Origin::Imported,
                false,
                false,
            )
            .expect("an imported instance is legal where it was declared");
        // A local class instance for a foreign prelude type is legal: the class
        // is local.
        table
            .add_instance(
                &ModulePath::new(["Test"]),
                "Pretty",
                Type::int(),
                HashMap::new(),
                Origin::User,
                false,
                true,
            )
            .expect("the class is local, so the instance is legal");
    }

    #[test]
    fn repeated_instance_variables_must_match_the_same_type() {
        let pair =
            |left, right| Type::record(vec![("first".into(), left), ("second".into(), right)]);
        let repeated = pair(Type::Var(42), Type::Var(42));
        assert!(matches_head(&repeated, &pair(Type::int(), Type::int())));
        assert!(!matches_head(&repeated, &pair(Type::int(), Type::Str)));
        assert!(matches_head(
            &pair(Type::Var(42), Type::Var(43)),
            &pair(Type::int(), Type::Str),
        ));

        let mut table = ClassTable::new();
        table
            .declare_class(ModulePath::new(["Test"]), "Same", 1, Vec::new())
            .unwrap();
        table
            .add_instance(
                &ModulePath::new(["Test"]),
                "Same",
                repeated,
                HashMap::new(),
                Origin::User,
                true,
                true,
            )
            .unwrap();
        assert!(matches!(
            table.resolve("Same", &pair(Type::int(), Type::int())),
            Resolution::Found { .. }
        ));
        assert_eq!(
            table.resolve("Same", &pair(Type::int(), Type::Str)),
            Resolution::Missing
        );
        assert_eq!(
            table.resolve("Same", &pair(Type::int(), Type::Var(99))),
            Resolution::Deferred
        );
    }

    #[test]
    fn a_tensor_instance_head_respects_the_rank() {
        // Round-2 decision 1d: `Tensor<1, a>` and `Tensor<2, a>` are disjoint
        // heads and must both be registrable.
        let mut table = ClassTable::prelude();
        table
            .declare_class(ModulePath::new(["Test"]), "Pretty", 1, Vec::new())
            .expect("declare Pretty");
        for rank in [1, 2] {
            table
                .add_instance(
                    &ModulePath::new(["Test"]),
                    "Pretty",
                    Type::tensor(Shape::Rank(rank), Type::int()),
                    HashMap::new(),
                    Origin::User,
                    false,
                    true,
                )
                .expect("a distinct rank is a distinct head");
        }
        // Exact tensor buffers carry their element representation, so these
        // two heads are distinguishable even though both store rationals.
        table
            .add_instance(
                &ModulePath::new(["Test"]),
                "Pretty",
                Type::tensor(Shape::Rank(1), Type::real(aevia_dim::Dim::one())),
                HashMap::new(),
                Origin::User,
                false,
                true,
            )
            .expect("typed exact tensors have distinct representations");
    }

    #[test]
    fn a_closed_record_head_respects_the_label_set() {
        let mut table = ClassTable::prelude();
        table
            .declare_class(ModulePath::new(["Test"]), "Pretty", 1, Vec::new())
            .expect("declare Pretty");
        let one = Type::record(vec![("x".to_string(), Type::int())]);
        let two = Type::record(vec![
            ("x".to_string(), Type::int()),
            ("y".to_string(), Type::int()),
        ]);
        table
            .add_instance(
                &ModulePath::new(["Test"]),
                "Pretty",
                one,
                HashMap::new(),
                Origin::User,
                true,
                true,
            )
            .expect("one-label head");
        table
            .add_instance(
                &ModulePath::new(["Test"]),
                "Pretty",
                two,
                HashMap::new(),
                Origin::User,
                true,
                true,
            )
            .expect("a different closed label set is a different head");
    }

    #[test]
    fn derive_is_closed_to_the_structural_set() {
        let mut table = ClassTable::prelude();
        table
            .declare_class(ModulePath::new(["Test"]), "Pretty", 1, Vec::new())
            .expect("declare Pretty");
        let id = crate::adt::allocate_in(ModulePath::new(["Test"]), "V", Vec::new());
        crate::adt::set_ctors(
            id,
            vec![Ctor {
                name: "V".to_string(),
                fields: vec![Type::int()],
            }],
        );
        let head = Type::Adt(id, Vec::new());
        let error = table
            .add_derived(
                &ModulePath::new(["Test"]),
                "Pretty",
                head.clone(),
                &[Type::int()],
                true,
                true,
            )
            .expect_err("Pretty is not derivable");
        assert_eq!(error, DeriveError::UnsupportedClass("Pretty".to_string()));
        table
            .add_derived(
                &ModulePath::new(["Test"]),
                "Eq",
                head.clone(),
                &[Type::int()],
                true,
                false,
            )
            .expect("Eq is derivable over an Int field");
        // ... but a foreign type cannot be derived over.
        let foreign = crate::adt::allocate_in(ModulePath::new(["Other"]), "W", Vec::new());
        crate::adt::set_ctors(
            foreign,
            vec![Ctor {
                name: "W".to_string(),
                fields: vec![Type::int()],
            }],
        );
        let error = table
            .add_derived(
                &ModulePath::new(["Test"]),
                "Show",
                Type::Adt(foreign, Vec::new()),
                &[Type::int()],
                false,
                false,
            )
            .expect_err("derive over a foreign type is an orphan");
        assert_eq!(error, DeriveError::Orphan);
    }
}
