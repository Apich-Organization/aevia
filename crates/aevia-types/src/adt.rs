//! The nominal algebraic data types known to a compilation.
//!
//! Variants are nominal (§3.2.4), so a type is identified by an [`AdtId`], not
//! by its shape. The registry also holds the prelude types `Option`, `Result`
//! and `Ordering`, whose ids are fixed constants so that `Type::option` can be
//! built without touching the registry.

use crate::keys::AdtKey;
use crate::keys::ModulePath;
pub use crate::ty::AdtId;
use crate::ty::Type;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::RwLock;

/// `Option<a>`.
pub const OPTION_ID: AdtId = 0;
/// `Result<a, e>`.
pub const RESULT_ID: AdtId = 1;
/// `Ordering`.
pub const ORDERING_ID: AdtId = 2;

/// The defining module of each user-allocated type.
///
/// The module is kept beside the registry rather than inside [`Adt`] because
/// `Adt` is constructed structurally by consumers (the runtime interns the
/// opaque value types) and those constructors have no module. An entry exists
/// only for a type allocated by a compilation unit ([`allocate_in`]); a
/// built-in/opaque type is in the prelude scope.
fn modules() -> &'static RwLock<HashMap<AdtId, ModulePath>> {
    static MODULES: OnceLock<RwLock<HashMap<AdtId, ModulePath>>> = OnceLock::new();
    MODULES.get_or_init(|| RwLock::new(HashMap::new()))
}

/// The symbolic key of a registered type: `(defining module, name)`.
///
/// This is the identity that may leave the process (an interface file, a
/// diagnostic); the [`AdtId`] never does.
#[must_use]
pub fn key_of(id: AdtId) -> AdtKey {
    let module = modules()
        .read()
        .ok()
        .and_then(|table| table.get(&id).cloned())
        .unwrap_or_default();
    AdtKey::new(module, name_of(id))
}

/// Records the module that declares an allocated type.
pub fn set_module(
    id: AdtId,
    module: ModulePath,
) {
    if let Ok(mut table) = modules().write() {
        table.insert(id, module);
    }
}

/// The kind of one declared parameter of a variant type (§2: the kind may be
/// omitted and inferred from the constructor fields, or written `<d: Dim>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamKind {
    /// A type parameter (`type Tree<a>`).
    Type,
    /// A dimension parameter; the id is the placeholder dimension variable.
    Dim(u32),
}

fn param_kinds() -> &'static RwLock<HashMap<AdtId, Vec<ParamKind>>> {
    static KINDS: OnceLock<RwLock<HashMap<AdtId, Vec<ParamKind>>>> = OnceLock::new();
    KINDS.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Records the parameter kinds of a variant type.
pub fn set_param_kinds(
    id: AdtId,
    kinds: Vec<ParamKind>,
) {
    if let Ok(mut table) = param_kinds().write() {
        table.insert(id, kinds);
    }
}

/// The parameter kinds of a variant type; every parameter is a type unless
/// [`set_param_kinds`] recorded otherwise.
#[must_use]
pub fn param_kinds_of(id: AdtId) -> Vec<ParamKind> {
    if let Ok(table) = param_kinds().read() {
        if let Some(kinds) = table.get(&id) {
            return kinds.clone();
        }
    }
    let count = get(id).map_or(0, |adt| adt.params.len());
    vec![ParamKind::Type; count]
}

/// One constructor of a variant type.
#[derive(Clone, Debug)]
pub struct Ctor {
    /// The constructor name.
    pub name: String,
    /// The field types, written in terms of [`Type::Param`].
    pub fields: Vec<Type>,
}

/// A nominal variant type.
#[derive(Clone, Debug)]
pub struct Adt {
    /// The type name.
    pub name: String,
    /// The parameter names, for display and diagnostics.
    pub params: Vec<String>,
    /// The constructors, in declaration order.
    pub ctors: Vec<Ctor>,
}

fn registry() -> &'static RwLock<Vec<Adt>> {
    static REGISTRY: OnceLock<RwLock<Vec<Adt>>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        RwLock::new(vec![
            Adt {
                name: "Option".to_string(),
                params: vec!["a".to_string()],
                ctors: vec![
                    Ctor {
                        name: "None".to_string(),
                        fields: Vec::new(),
                    },
                    Ctor {
                        name: "Some".to_string(),
                        fields: vec![Type::Param(0)],
                    },
                ],
            },
            Adt {
                name: "Result".to_string(),
                params: vec!["a".to_string(), "e".to_string()],
                ctors: vec![
                    Ctor {
                        name: "Ok".to_string(),
                        fields: vec![Type::Param(0)],
                    },
                    Ctor {
                        name: "Err".to_string(),
                        fields: vec![Type::Param(1)],
                    },
                ],
            },
            Adt {
                name: "Ordering".to_string(),
                params: Vec::new(),
                ctors: vec![
                    Ctor {
                        name: "Less".to_string(),
                        fields: Vec::new(),
                    },
                    Ctor {
                        name: "Equal".to_string(),
                        fields: Vec::new(),
                    },
                    Ctor {
                        name: "Greater".to_string(),
                        fields: Vec::new(),
                    },
                ],
            },
        ])
    })
}

/// Interns a *built-in* type, keyed by name.
///
/// This is for the opaque types the checker itself needs (`Value`, `OpError`,
/// the error types, `Set`, `Table`): they are not nominal declarations, so one
/// id per name is right. A user `type` never goes through here — see
/// [`allocate`].
///
/// # Panics
///
/// Panics when the registry lock is poisoned, which only happens if another
/// thread panicked while holding it.
#[must_use]
pub fn intern(adt: Adt) -> AdtId {
    let mut guard = registry().write().expect("adt registry is not poisoned");
    if let Some(index) = guard.iter().position(|existing| existing.name == adt.name) {
        return u32::try_from(index).unwrap_or(0);
    }
    let id = u32::try_from(guard.len()).unwrap_or(u32::MAX);
    guard.push(adt);
    id
}

/// Allocates a *fresh* id for a nominal type declared by a compilation unit
/// (PLAN D6).
///
/// Ids are never reused and never keyed by name, so two modules that both
/// declare `type Shape` get distinct types, and a user `type Result` cannot
/// replace the prelude's.
///
/// # Panics
///
/// Panics when the registry lock is poisoned.
#[must_use]
pub fn allocate(
    name: &str,
    params: Vec<String>,
) -> AdtId {
    allocate_in(ModulePath::prelude(), name, params)
}

/// Allocates a fresh id for a type declared in `module`, recording the defining
/// module so [`key_of`] can derive the symbolic identity.
///
/// # Panics
///
/// Panics when the registry lock is poisoned.
#[must_use]
pub fn allocate_in(
    module: ModulePath,
    name: &str,
    params: Vec<String>,
) -> AdtId {
    let mut guard = registry().write().expect("adt registry is not poisoned");
    let id = u32::try_from(guard.len()).unwrap_or(u32::MAX);
    guard.push(Adt {
        name: name.to_string(),
        params,
        ctors: Vec::new(),
    });
    set_module(id, module);
    id
}

/// Fills in the constructors of a type allocated by [`allocate`].
///
/// Declaring a recursive type requires the name to exist before its fields are
/// converted (§3.6.4), which is why allocation and construction are two steps.
pub fn set_ctors(
    id: AdtId,
    ctors: Vec<Ctor>,
) {
    if let Ok(mut guard) = registry().write() {
        if let Some(entry) = usize::try_from(id)
            .ok()
            .and_then(|index| guard.get_mut(index))
        {
            entry.ctors = ctors;
        }
    }
}

/// Returns a copy of a registered type.
#[must_use]
pub fn get(id: AdtId) -> Option<Adt> {
    let guard = registry().read().ok()?;
    guard.get(usize::try_from(id).ok()?).cloned()
}

/// The name of a registered type, or `Adt#n` when the id is unknown.
#[must_use]
pub fn name_of(id: AdtId) -> String {
    get(id).map_or_else(|| format!("Adt#{id}"), |adt| adt.name)
}

/// The index of a constructor inside its type.
#[must_use]
pub fn ctor_index(
    id: AdtId,
    name: &str,
) -> Option<usize> {
    let adt = get(id)?;
    adt.ctors.iter().position(|ctor| ctor.name == name)
}

/// Finds a constructor of one of the prelude types by name.
///
/// Only the prelude is searched: a user constructor is reachable through the
/// module's own tables, and searching every compiled module would make name
/// resolution depend on what else was checked in this process.
#[must_use]
pub fn find_prelude_ctor(name: &str) -> Option<(AdtId, usize, Adt)> {
    let guard = registry().read().ok()?;
    for id in [OPTION_ID, RESULT_ID, ORDERING_ID] {
        let adt = guard.get(usize::try_from(id).ok()?)?;
        if let Some(position) = adt.ctors.iter().position(|ctor| ctor.name == name) {
            return Some((id, position, adt.clone()));
        }
    }
    None
}

/// Finds a constructor by name across all registered types.
#[must_use]
pub fn find_ctor(name: &str) -> Option<(AdtId, usize, Adt)> {
    let guard = registry().read().ok()?;
    for (index, adt) in guard.iter().enumerate() {
        if let Some(position) = adt.ctors.iter().position(|ctor| ctor.name == name) {
            return Some((u32::try_from(index).ok()?, position, adt.clone()));
        }
    }
    None
}

/// The number of registered types, for tests.
#[must_use]
pub fn count() -> usize {
    registry().read().map_or(0, |guard| guard.len())
}

/// Resets the registry to the prelude types. Used by tests.
pub fn reset() {
    if let Ok(mut guard) = registry().write() {
        guard.truncate(3);
    }
    if let Ok(mut table) = modules().write() {
        table.clear();
    }
    if let Ok(mut table) = param_kinds().write() {
        table.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prelude_types_have_fixed_ids() {
        assert_eq!(name_of(OPTION_ID), "Option");
        assert_eq!(name_of(RESULT_ID), "Result");
        assert_eq!(ctor_index(OPTION_ID, "Some"), Some(1));
        assert_eq!(ctor_index(RESULT_ID, "Err"), Some(1));
    }

    #[test]
    fn interning_is_idempotent() {
        let adt = Adt {
            name: "TestOnly".to_string(),
            params: Vec::new(),
            ctors: vec![Ctor {
                name: "Only".to_string(),
                fields: Vec::new(),
            }],
        };
        let first = intern(adt.clone());
        let second = intern(adt);
        assert_eq!(first, second);
    }

    #[test]
    fn allocation_never_reuses_an_id() {
        // §3.2.4 / PLAN D6: two compilation units that declare the same name
        // must not share one nominal type.
        let first = allocate("Shape", Vec::new());
        let second = allocate("Shape", Vec::new());
        assert_ne!(first, second);
        set_ctors(
            first,
            vec![Ctor {
                name: "Circle".to_string(),
                fields: Vec::new(),
            }],
        );
        assert!(get(second).is_some_and(|adt| adt.ctors.is_empty()));
    }
}
