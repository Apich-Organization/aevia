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

use crate::ty::{Rep, Row, Scheme, Shape, Type};
use std::collections::HashMap;

/// A class declaration.
#[derive(Clone, Debug)]
pub struct ClassDef {
    /// Number of parameters.
    pub params: usize,
    /// Methods, with their types written in terms of [`Type::Param`].
    pub methods: Vec<(String, Type)>,
    /// Whether the class is part of the prelude.
    pub builtin: bool,
}

/// Where an instance came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    /// A prelude instance.
    Prelude,
    /// A user `instance` declaration.
    User,
    /// A `derive` declaration.
    Derived,
}

/// One instance.
#[derive(Clone, Debug)]
pub struct Instance {
    /// The class name.
    pub class: String,
    /// The instance head.
    pub head: Type,
    /// The specialised type of each method.
    pub methods: HashMap<String, Type>,
    /// Where it came from.
    pub origin: Origin,
}

/// Why an instance could not be registered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstanceError {
    /// No such class.
    UnknownClass,
    /// Two instances have overlapping heads (`E0311`).
    Overlapping,
    /// The instance is for a foreign type and a foreign class (`E0604`).
    Orphan,
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
        ] {
            table.classes.insert(
                name.to_string(),
                ClassDef {
                    params: arity,
                    methods: Vec::new(),
                    builtin: true,
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
        for head in [int.clone(), Type::Bool, Type::Str, Type::Unit, float.clone()] {
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
        for head in [Type::Str, Type::tensor(Shape::Rank(1), Type::Param(0))] {
            table.add_prelude("Sized", head);
        }
        table
    }

    fn add_prelude(&mut self, class: &str, head: Type) {
        let instances = self.instances.entry(class.to_string()).or_default();
        instances.push(Instance {
            class: class.to_string(),
            head,
            methods: HashMap::new(),
            origin: Origin::Prelude,
        });
    }

    /// Declares a class.
    ///
    /// # Errors
    ///
    /// `E0316` when the name is already taken by another class.
    pub fn declare_class(
        &mut self,
        name: &str,
        params: usize,
        methods: Vec<(String, Type)>,
    ) -> Result<(), &'static str> {
        if self.classes.contains_key(name) {
            return Err("E0316");
        }
        self.classes.insert(
            name.to_string(),
            ClassDef {
                params,
                methods,
                builtin: false,
            },
        );
        Ok(())
    }

    /// Whether a class exists.
    #[must_use]
    pub fn has_class(&self, name: &str) -> bool {
        self.classes.contains_key(name)
    }

    /// The class declaration.
    #[must_use]
    pub fn class(&self, name: &str) -> Option<&ClassDef> {
        self.classes.get(name)
    }

    /// The class that declares a method, if any.
    #[must_use]
    pub fn class_of_method(&self, method: &str) -> Option<(&str, &Type)> {
        for (name, class) in &self.classes {
            if let Some((_, ty)) = class.methods.iter().find(|(m, _)| m == method) {
                return Some((name, ty));
            }
        }
        None
    }

    /// Registers an instance.
    ///
    /// # Errors
    ///
    /// `UnknownClass` for a class that does not exist, `Overlapping` when the
    /// head overlaps an existing instance (`E0311`), and `Orphan` when neither
    /// the head nor the class is local (`E0604`).
    pub fn add_instance(
        &mut self,
        class: &str,
        head: Type,
        methods: HashMap<String, Type>,
        origin: Origin,
        head_is_local: bool,
    ) -> Result<(), InstanceError> {
        let Some(definition) = self.classes.get(class) else {
            return Err(InstanceError::UnknownClass);
        };
        // §3.8.3: an instance belongs either to the module that declares the
        // type or to the module that declares the class. A builtin class is
        // foreign, so a user instance for it needs a local type.
        if origin == Origin::User && !head_is_local && definition.builtin {
            return Err(InstanceError::Orphan);
        }
        let instances = self.instances.entry(class.to_string()).or_default();
        if instances
            .iter()
            .any(|existing| heads_overlap(&existing.head, &head))
        {
            return Err(InstanceError::Overlapping);
        }
        let index = instances.len();
        instances.push(Instance {
            class: class.to_string(),
            head,
            methods,
            origin,
        });
        let _ = index;
        Ok(())
    }

    /// Solves `class τ` against the instance set (§3.8.2).
    #[must_use]
    pub fn resolve(&self, class: &str, ty: &Type) -> Resolution {
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
            Some(index) => Resolution::Found { index },
            None if has_variables(ty) => Resolution::Deferred,
            None => Resolution::Missing,
        }
    }

    /// The type of `method` for the instance that matches `ty`.
    #[must_use]
    pub fn method_type(&self, class: &str, ty: &Type, method: &str) -> Option<Type> {
        let Resolution::Found { index } = self.resolve(class, ty) else {
            return None;
        };
        let instance = self.instances.get(class)?.get(index)?;
        instance.methods.get(method).cloned()
    }

    /// Every instance of a class, for diagnostics.
    #[must_use]
    pub fn instance_heads(&self, class: &str) -> Vec<Type> {
        self.instances
            .get(class)
            .map(|instances| instances.iter().map(|instance| instance.head.clone()).collect())
            .unwrap_or_default()
    }

    /// The number of registered instances, for tests.
    #[must_use]
    pub fn instance_count(&self) -> usize {
        self.instances.values().map(Vec::len).sum()
    }

    /// Adds a `derive`d instance for a type, given the types of its fields.
    ///
    /// Returns `E0317` when a field cannot be derived over (§3.8.5).
    ///
    /// # Errors
    ///
    /// `UnknownClass` for an unknown class, `Overlapping` for a second
    /// instance, and `E0317` when a field has no instance.
    pub fn add_derived(
        &mut self,
        class: &str,
        head: Type,
        fields: &[Type],
    ) -> Result<(), &'static str> {
        let Some(definition) = self.classes.get(class) else {
            return Err("E0310");
        };
        if definition.params != 1 {
            return Err("E0317");
        }
        // §3.8.5: a function field has no `Eq`/`Show`/`Hash` instance.
        for field in fields {
            if field_has_function(field) {
                return Err("E0317");
            }
            if requires_instance(class) && !self.instance_for_field(class, field) {
                return Err("E0317");
            }
        }
        let methods = HashMap::new();
        match self.add_instance(class, head, methods, Origin::Derived, true) {
            Ok(()) => Ok(()),
            Err(InstanceError::Overlapping) => Err("E0311"),
            Err(_) => Err("E0317"),
        }
    }

    /// Whether a field type has an instance of `class`.
    fn instance_for_field(&self, class: &str, field: &Type) -> bool {
        let resolved = match field {
            Type::Param(_) => return true,
            other => other.clone(),
        };
        // `Show`/`Eq` of a container needs the element instance, which a
        // derived instance does not check transitively (§3.8.5 is structural).
        matches!(
            self.resolve(class, &resolved),
            Resolution::Found { .. } | Resolution::Deferred
        )
    }
}

/// Whether deriving `class` over this field needs an instance of the field's
/// own type. A derived instance is structural, so it does.
fn requires_instance(class: &str) -> bool {
    matches!(class, "Eq" | "Ord" | "TotalOrd" | "Show" | "Hash")
}

fn field_has_function(ty: &Type) -> bool {
    match ty {
        Type::Fn(..) => true,
        Type::Tensor { elem, .. } => field_has_function(elem),
        Type::Record(row) => row.fields().iter().any(|(_, ty)| field_has_function(ty)),
        Type::Adt(_, args) => args.iter().any(field_has_function),
        Type::Io(inner) => field_has_function(inner),
        _ => false,
    }
}

/// Whether two instance heads can match the same type (§3.8.3).
#[must_use]
pub fn heads_overlap(left: &Type, right: &Type) -> bool {
    match (left, right) {
        (Type::Param(_) | Type::Var(_), _) | (_, Type::Param(_) | Type::Var(_)) => true,
        (Type::Scalar(a, _), Type::Scalar(b, _)) => a == b || is_rep_var(*a) || is_rep_var(*b),
        (Type::Bool, Type::Bool)
        | (Type::Prop, Type::Prop)
        | (Type::Str, Type::Str)
        | (Type::Unit, Type::Unit) => true,
        (Type::Tensor { elem: a, .. }, Type::Tensor { elem: b, .. }) => heads_overlap(a, b),
        (Type::Point(_), Type::Point(_)) | (Type::UnitOf(_), Type::UnitOf(_)) => true,
        (Type::Fn(pa, ra), Type::Fn(pb, rb)) => {
            pa.len() == pb.len()
                && pa.iter().zip(pb.iter()).all(|(a, b)| heads_overlap(a, b))
                && heads_overlap(ra, rb)
        }
        (Type::Record(a), Type::Record(b)) => rows_overlap(a, b),
        (Type::Adt(a, aa), Type::Adt(b, ab)) => {
            a == b && aa.len() == ab.len() && aa.iter().zip(ab.iter()).all(|(x, y)| heads_overlap(x, y))
        }
        (Type::Io(a), Type::Io(b)) => heads_overlap(a, b),
        (Type::Dyn(_), Type::Dyn(_)) => true,
        _ => false,
    }
}

fn rows_overlap(left: &Row, right: &Row) -> bool {
    let left_fields = left.fields();
    let right_fields = right.fields();
    for (label, ty) in &left_fields {
        if let Some((_, other)) = right_fields.iter().find(|(name, _)| name == label) {
            if !heads_overlap(ty, other) {
                return false;
            }
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
pub fn matches_head(pattern: &Type, concrete: &Type) -> bool {
    match (pattern, concrete) {
        (Type::Param(_) | Type::Var(_), _) => true,
        (Type::Scalar(a, _), Type::Scalar(b, _)) => {
            if is_rep_var(*a) {
                true
            } else {
                *a == *b
            }
        }
        (Type::Bool, Type::Bool)
        | (Type::Prop, Type::Prop)
        | (Type::Str, Type::Str)
        | (Type::Unit, Type::Unit) => true,
        (Type::Tensor { elem: a, .. }, Type::Tensor { elem: b, .. }) => matches_head(a, b),
        (Type::Point(_), Type::Point(_)) | (Type::UnitOf(_), Type::UnitOf(_)) => true,
        (Type::Fn(pa, ra), Type::Fn(pb, rb)) => {
            pa.len() == pb.len()
                && pa.iter().zip(pb.iter()).all(|(a, b)| matches_head(a, b))
                && matches_head(ra, rb)
        }
        (Type::Record(a), Type::Record(b)) => {
            let a_fields = a.fields();
            let b_fields = b.fields();
            if a_fields.len() != b_fields.len() {
                return false;
            }
            a_fields.iter().all(|(label, ty)| {
                b_fields
                    .iter()
                    .find(|(name, _)| name == label)
                    .is_some_and(|(_, other)| matches_head(ty, other))
            })
        }
        (Type::Adt(a, aa), Type::Adt(b, ab)) => {
            a == b && aa.len() == ab.len() && aa.iter().zip(ab.iter()).all(|(x, y)| matches_head(x, y))
        }
        (Type::Io(a), Type::Io(b)) => matches_head(a, b),
        (Type::Dyn(_), Type::Dyn(_)) => true,
        _ => false,
    }
}

/// Whether a type still contains a type, dimension or row variable.
#[must_use]
pub fn has_variables(ty: &Type) -> bool {
    match ty {
        Type::Var(_) | Type::Param(_) => true,
        Type::Scalar(rep, dim) => matches!(rep, Rep::Var(_)) || !dim.vars().is_empty(),
        Type::Tensor { elem, .. } => has_variables(elem),
        Type::Fn(params, ret) => params.iter().any(has_variables) || has_variables(ret),
        Type::Record(row) => row_has_variables(row),
        Type::Adt(_, args) => args.iter().any(has_variables),
        Type::Io(inner) => has_variables(inner),
        Type::Point(dim) | Type::UnitOf(dim) => !dim.vars().is_empty(),
        Type::Dyn(rep) => matches!(rep, Rep::Var(_)),
        Type::Bool | Type::Prop | Type::Str | Type::Unit | Type::Error => false,
    }
}

fn row_has_variables(row: &Row) -> bool {
    match row {
        Row::Empty => false,
        Row::Var(_) => true,
        Row::Extend { ty, tail, .. } => has_variables(ty) || row_has_variables(tail),
    }
}

/// The scheme of a class method: `∀a. C a => σ`.
///
/// `ty` is written in terms of `Type::Var(0..params)`; instantiation replaces
/// them, so the small ids never escape.
#[must_use]
pub fn method_scheme(class: &str, params: usize, ty: Type) -> Scheme {
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
                "Show",
                Type::real(length),
                HashMap::new(),
                Origin::User,
                true,
            )
            .expect_err("Show Real[d] already exists");
        assert_eq!(error, InstanceError::Overlapping);
    }

    #[test]
    fn a_foreign_instance_is_an_orphan() {
        let mut table = ClassTable::prelude();
        let error = table
            .add_instance(
                "Show",
                Type::int(),
                HashMap::new(),
                Origin::User,
                false,
            )
            .expect_err("the user cannot add Show Int");
        assert_eq!(error, InstanceError::Orphan);
    }
}
