//! The nominal algebraic data types known to a compilation.
//!
//! Variants are nominal (§3.2.4), so a type is identified by an [`AdtId`], not
//! by its shape. The registry also holds the prelude types `Option`, `Result`
//! and `Ordering`, whose ids are fixed constants so that `Type::option` can be
//! built without touching the registry.

#![cfg_attr(
    not(test),
    deny(
        missing_docs,
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::nursery,
        clippy::panic,
        clippy::pedantic,
        clippy::single_call_fn,
        clippy::unwrap_used
    )
)]

use crate::keys::AdtKey;
use crate::keys::ModulePath;
pub use crate::ty::AdtId;
use crate::ty::Type;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::RwLock;
use std::sync::RwLockWriteGuard;

/// `Option<a>`.
pub const OPTION_ID: AdtId = 0;
/// `Result<a, e>`.
pub const RESULT_ID: AdtId = 1;
/// `Ordering`.
pub const ORDERING_ID: AdtId = 2;

const PRELUDE_IDS: [AdtId; 3] = [OPTION_ID, RESULT_ID, ORDERING_ID];

/// The sealed host type of a session-owned native reverse-AD variable.
/// It has no source constructors; a same-named source declaration is distinct.
#[must_use]
pub fn native_ad_var_type() -> Type {
    Type::Adt(
        intern(Adt {
            name: "NativeAdVar".into(),
            params: Vec::new(),
            ctors: Vec::new(),
        }),
        Vec::new(),
    )
}
/// Whether a type is exactly the sealed native AD host type.
#[must_use]
pub fn is_native_ad_var_type(ty: &Type) -> bool {
    *ty == native_ad_var_type()
}

/// The symbolic key of a registered type: `(defining module, name)`.
/// This is the identity that may leave the process (an interface file, a
/// diagnostic); the [`AdtId`] never does.
#[must_use]
pub fn key_of(id: AdtId) -> AdtKey {
    let (module, name) = if let Ok(table) = registry().read()
        && let Some(adt) = table.definition(id)
    {
        (table.module(id), adt.name.clone())
    } else {
        (ModulePath::prelude(), format!("Adt#{id}"))
    };
    let mut key = AdtKey::new(module, name);
    if !key.module.is_prelude() {
        key.schema = schema_of(id);
    }
    key
}

/// The stable lexical origin of an allocated declaration.
#[must_use]
pub fn module_of(id: AdtId) -> ModulePath {
    registry()
        .read()
        .map_or_else(|_| ModulePath::prelude(), |table| table.module(id))
}
fn schemas() -> &'static RwLock<HashMap<AdtId, crate::DeclarationSchemaKey>> {
    static SCHEMAS: OnceLock<RwLock<HashMap<AdtId, crate::DeclarationSchemaKey>>> = OnceLock::new();
    SCHEMAS.get_or_init(|| RwLock::new(HashMap::new()))
}
/// Finds an already complete type by its symbolic key.
///
/// The cache decoder reuses types with the same module, name and declaration
/// schema.
#[must_use]
pub fn find_complete(key: &AdtKey) -> Option<AdtId> {
    let candidates = registry().read().ok()?.complete_candidates(key);
    candidates.into_iter().find(|id| key_of(*id) == *key)
}
#[allow(
    clippy::single_call_fn,
    reason = "the interface encoder persists finalization without accessing the registry directly"
)]
pub(crate) fn is_complete(id: AdtId) -> bool {
    registry()
        .read()
        .is_ok_and(|table| table.complete.contains(&id))
}

/// Captures the declaration and its defining module at the same point as
/// finalization so the schema builder cannot observe unfinished constructors.
#[allow(
    clippy::single_call_fn,
    reason = "the schema builder needs an atomic view of registry state"
)]
pub(crate) fn schema_snapshot(id: AdtId) -> Option<(Arc<Adt>, ModulePath)> {
    let table = registry().read().ok()?;
    let definition = table.definition(id)?;
    let module = table.module(id);
    if !module.is_prelude() && !table.complete.contains(&id) {
        return None;
    }
    let snapshot = (Arc::clone(definition), module);
    drop(table);
    Some(snapshot)
}
/// A collision-free declaration identity; incomplete declarations have no schema.
#[must_use]
pub fn schema_of(id: AdtId) -> Option<crate::DeclarationSchemaKey> {
    if let Some(schema) = schemas()
        .read()
        .ok()
        .and_then(|table| table.get(&id).cloned())
    {
        return Some(schema);
    }
    let schema = crate::declaration_schema::build(id)?;
    if let Ok(mut table) = schemas().write() {
        table.insert(id, schema.clone());
    }
    Some(schema)
}
/// Nominal equality across independent checks of an unchanged declaration.
/// Incomplete declarations and host types retain their exact allocated witness.
#[must_use]
pub fn same_type(
    left: AdtId,
    right: AdtId,
) -> bool {
    if left == right {
        return true;
    }
    let left = key_of(left);
    let right = key_of(right);
    left.schema.is_some() && right.schema.is_some() && left == right
}

/// Records the module that declares an allocated type before finalization.
/// Finalized declarations are immutable; later writes are ignored.
pub fn set_module(
    id: AdtId,
    module: ModulePath,
) {
    if let Ok(mut table) = registry().write()
        && !table.complete.contains(&id)
    {
        let table = &mut *table;
        if let Some(definition) = usize::try_from(id)
            .ok()
            .and_then(|index| table.definitions.get(index))
            && table.origins.get(&id).is_none_or(ModulePath::is_prelude) != module.is_prelude()
        {
            let name = &definition.name;
            if module.is_prelude() {
                add_witness(&mut table.hosts, name, id);
            } else if let Some(ids) = table.hosts.get_mut(name) {
                ids.remove(&id);
                if ids.is_empty() {
                    table.hosts.remove(name);
                }
            }
        }
        table.origins.insert(id, module);
    }
}

/// The kind of one declared parameter of a variant type (§2: the kind may be
/// omitted and inferred from the constructor fields, or written `<d: Dim>`).
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
    bincode_next::Fingerprint,
)]
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

/// Records the parameter kinds of an unfinished variant type.
/// Finalized declarations are immutable; later writes are ignored.
pub fn set_param_kinds(
    id: AdtId,
    kinds: Vec<ParamKind>,
) {
    if let Ok(table) = registry().read()
        && !table.complete.contains(&id)
        && let Ok(mut table) = param_kinds().write()
    {
        table.insert(id, kinds);
    }
}

/// The parameter kinds of a variant type; every parameter is a type unless
/// [`set_param_kinds`] recorded otherwise.
#[must_use]
pub fn param_kinds_of(id: AdtId) -> Vec<ParamKind> {
    if let Ok(table) = param_kinds().read()
        && let Some(kinds) = table.get(&id)
    {
        return kinds.clone();
    }
    let count = get(id).map_or(0, |adt| adt.params.len());
    vec![ParamKind::Type; count]
}

/// One constructor of a variant type.
#[derive(
    Clone,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
    bincode_next::Fingerprint,
)]
pub struct Ctor {
    /// The constructor name.
    pub name: String,
    /// The field types, written in terms of [`Type::Param`].
    pub fields: Vec<Type>,
}

/// A nominal variant type.
#[derive(
    Clone,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
    bincode_next::Fingerprint,
)]
pub struct Adt {
    /// The type name.
    pub name: String,
    /// The parameter names, for display and diagnostics.
    pub params: Vec<String>,
    /// The constructors, in declaration order.
    pub ctors: Vec<Ctor>,
}

#[derive(Default)]
struct CompleteCache {
    present: HashMap<ModulePath, HashMap<String, BTreeSet<AdtId>>>,
    // Repeated unresolved lookups benefit too, without retaining unrelated
    // missing names from the lifetime of the process.
    last_missing: Option<(ModulePath, String)>,
}

impl CompleteCache {
    #[allow(
        clippy::single_call_fn,
        reason = "provides a shared-read lookup of the memoized candidates"
    )]
    fn get(
        &self,
        key: &AdtKey,
    ) -> Option<Vec<AdtId>> {
        if let Some(ids) = self
            .present
            .get(&key.module)
            .and_then(|types| types.get(&key.name))
        {
            return Some(ids.iter().copied().collect());
        }
        self.last_missing
            .as_ref()
            .filter(|(module, name)| module == &key.module && name == &key.name)
            .map(|_| Vec::new())
    }

    #[allow(
        clippy::single_call_fn,
        reason = "records a discovered candidate group without keeping the registry read path exclusive"
    )]
    fn store(
        &mut self,
        key: &AdtKey,
        candidates: &[AdtId],
    ) {
        if candidates.is_empty() {
            self.last_missing = Some((key.module.clone(), key.name.clone()));
            return;
        }
        let ids = candidates.iter().copied().collect();
        if let Some(types) = self.present.get_mut(&key.module) {
            types.insert(key.name.clone(), ids);
            return;
        }
        self.present
            .insert(key.module.clone(), HashMap::from([(key.name.clone(), ids)]));
    }

    #[allow(
        clippy::single_call_fn,
        reason = "updates only lookup keys already known before the declaration finished"
    )]
    fn finalized(
        &mut self,
        module: &ModulePath,
        name: &str,
        id: AdtId,
    ) {
        if let Some(ids) = self
            .present
            .get_mut(module)
            .and_then(|types| types.get_mut(name))
        {
            ids.insert(id);
        }
        if self
            .last_missing
            .as_ref()
            .is_some_and(|(last_module, last_name)| last_module == module && last_name == name)
        {
            self.last_missing = None;
        }
    }
}

#[derive(Default)]
struct Registry {
    definitions: Vec<Arc<Adt>>,
    // Keep the structural public Adt independent of the source identity.
    origins: HashMap<AdtId, ModulePath>,
    complete: HashSet<AdtId>,
    hosts: HashMap<String, BTreeSet<AdtId>>,
    completed_names: RwLock<CompleteCache>,
}

impl Registry {
    fn with_prelude() -> Self {
        let mut registry = Self::default();
        for (id, adt) in PRELUDE_IDS.into_iter().zip(prelude_adts()) {
            registry.push(id, adt, true);
            registry.complete.insert(id);
        }
        registry
    }

    fn module(
        &self,
        id: AdtId,
    ) -> ModulePath {
        self.origins.get(&id).cloned().unwrap_or_default()
    }

    fn definition(
        &self,
        id: AdtId,
    ) -> Option<&Arc<Adt>> {
        self.definitions.get(usize::try_from(id).ok()?)
    }

    fn push(
        &mut self,
        id: AdtId,
        adt: Adt,
        host: bool,
    ) {
        if host {
            add_witness(&mut self.hosts, &adt.name, id);
        }
        self.definitions.push(Arc::new(adt));
    }

    fn host_type(
        &self,
        name: &str,
    ) -> Option<AdtId> {
        self.hosts.get(name)?.first().copied()
    }

    #[allow(
        clippy::single_call_fn,
        reason = "keeps the registry's lookup and activation policy together"
    )]
    fn complete_candidates(
        &self,
        key: &AdtKey,
    ) -> Vec<AdtId> {
        // Small registries are cheap to scan. A large registry remembers only
        // the requested declaration so isolated restores do no index build.
        const MIN_DEFINITIONS: usize = 128;
        let use_cache = self.definitions.len() >= MIN_DEFINITIONS;
        if use_cache
            && let Ok(index) = self.completed_names.read()
            && let Some(candidates) = index.get(key)
        {
            return candidates;
        }
        let matches: Vec<AdtId> = self
            .definitions
            .iter()
            .enumerate()
            .filter(|(_, adt)| adt.name == key.name)
            .filter_map(|(index, _)| AdtId::try_from(index).ok())
            .filter(|id| self.origins.get(id) == Some(&key.module) && self.complete.contains(id))
            .collect();
        if use_cache && let Ok(mut index) = self.completed_names.write() {
            index.store(key, &matches);
        }
        matches
    }
}

fn add_witness(
    table: &mut HashMap<String, BTreeSet<AdtId>>,
    name: &str,
    id: AdtId,
) {
    if let Some(ids) = table.get_mut(name) {
        ids.insert(id);
    } else {
        table.insert(name.into(), BTreeSet::from([id]));
    }
}

fn registry() -> &'static RwLock<Registry> {
    static REGISTRY: OnceLock<RwLock<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| RwLock::new(Registry::with_prelude()))
}

/// `Option`, `Result` and `Ordering`: the registry's first three entries.
#[allow(
    clippy::single_call_fn,
    reason = "records the canonical prelude declaration order independently of the registry lock"
)]
fn prelude_adts() -> [Adt; 3] {
    [
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
    ]
}

// Registry writes manipulate owned collections using concrete key types. A
// recovered lock still contains valid entries and can resume publication.
fn writable<T>(table: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    table.write().unwrap_or_else(|poisoned| {
        table.clear_poison();
        poisoned.into_inner()
    })
}

#[allow(
    clippy::panic,
    reason = "the infallible AdtId API cannot issue an alias when its 32-bit address space is exhausted"
)]
fn register(
    mut entries: RwLockWriteGuard<'_, Registry>,
    adt: Adt,
    module: Option<ModulePath>,
) -> AdtId {
    let Ok(id) = AdtId::try_from(entries.definitions.len()) else {
        drop(entries);
        panic!("the nominal type identifier space is exhausted");
    };
    let host = if let Some(module) = module {
        let is_prelude = module.is_prelude();
        entries.origins.insert(id, module);
        is_prelude
    } else {
        entries.origins.get(&id).is_none_or(ModulePath::is_prelude)
    };
    // Keep the registry lock across publication of the source identity.
    // Concurrent interning must never see a source type without its origin.
    entries.push(id, adt, host);
    drop(entries);
    id
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
/// Panics if the registry has exhausted the `AdtId` identifier space.
#[must_use]
pub fn intern(adt: Adt) -> AdtId {
    intern_with_status(adt).0
}

pub(crate) fn intern_with_status(adt: Adt) -> (AdtId, bool) {
    let registry = registry();
    if let Ok(entries) = registry.read()
        && let Some(id) = entries.host_type(&adt.name)
    {
        return (id, false);
    }
    let entries = writable(registry);
    // Another thread may have interned the type after the first lookup.
    if let Some(id) = entries.host_type(&adt.name) {
        return (id, false);
    }
    (register(entries, adt, None), true)
}

/// Allocates a fresh id for a nominal source type in the prelude scope.
///
/// Ids are never reused and never keyed by name, so two modules that both
/// declare `type Shape` get distinct types, and a user `type Result` cannot
/// replace the prelude's.
///
/// # Panics
///
/// Panics if the registry has exhausted the `AdtId` identifier space.
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
/// Panics if the registry has exhausted the `AdtId` identifier space.
#[must_use]
pub fn allocate_in(
    module: ModulePath,
    name: &str,
    params: Vec<String>,
) -> AdtId {
    let adt = Adt {
        name: name.to_string(),
        params,
        ctors: Vec::new(),
    };
    register(writable(registry()), adt, Some(module))
}

/// Fills in the constructors of a type allocated by [`allocate`].
///
/// Declaring a recursive type requires the name to exist before its fields are
/// converted (§3.6.4), which is why allocation and construction are two steps.
/// This finalizes the declaration, including an abstract type with no
/// constructors. Repeated calls cannot mutate a previously issued witness.
pub fn set_ctors(
    id: AdtId,
    ctors: Vec<Ctor>,
) {
    if let Ok(mut table) = registry().write()
        && !table.complete.contains(&id)
    {
        let table = &mut *table;
        if let Some(entry) = usize::try_from(id)
            .ok()
            .and_then(|index| table.definitions.get_mut(index))
        {
            Arc::make_mut(entry).ctors = ctors;
            table.complete.insert(id);
            if let Ok(index) = table.completed_names.get_mut()
                && (!index.present.is_empty() || index.last_missing.is_some())
                && let Some(module) = table.origins.get(&id)
            {
                index.finalized(module, &entry.name, id);
            }
        }
    }
}

/// The prelude's persistent ordered set, `Set<a>` (§6.6, §8.1.9).
///
/// It is opaque — no source constructor builds one — so it is interned by name
/// like the other host types; every compilation unit sees the same id.
#[must_use]
pub fn set_id() -> AdtId {
    intern(Adt {
        name: "Set".to_string(),
        params: vec!["a".to_string()],
        ctors: Vec::new(),
    })
}

/// Returns a registered type.
///
/// The entry is shared: a type is immutable once its constructors are set, and
/// this sits on the evaluator's constructor and pattern paths, where copying
/// the definition (names and every field type) per use dominated.
#[must_use]
pub fn get(id: AdtId) -> Option<Arc<Adt>> {
    let guard = registry().read().ok()?;
    guard.definition(id).cloned()
}

/// The name of a registered type, or `Adt#n` when the id is unknown.
#[must_use]
pub fn name_of(id: AdtId) -> String {
    get(id).map_or_else(|| format!("Adt#{id}"), |adt| adt.name.clone())
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
    let (id, position, adt) = {
        let entries = registry().read().ok()?;
        PRELUDE_IDS.into_iter().find_map(|id| {
            let entry = entries.definitions.get(usize::try_from(id).ok()?)?;
            let position = entry.ctors.iter().position(|ctor| ctor.name == name)?;
            Some((id, position, Arc::clone(entry)))
        })
    }?;
    Some((id, position, (*adt).clone()))
}

/// Finds a constructor by name across all registered types.
#[must_use]
pub fn find_ctor(name: &str) -> Option<(AdtId, usize, Adt)> {
    let (id, position, adt) = {
        let entries = registry().read().ok()?;
        entries
            .definitions
            .iter()
            .enumerate()
            .find_map(|(index, entry)| {
                let position = entry.ctors.iter().position(|ctor| ctor.name == name)?;
                Some((AdtId::try_from(index).ok()?, position, Arc::clone(entry)))
            })
    }?;
    Some((id, position, (*adt).clone()))
}

/// The number of registered types, for tests.
#[must_use]
pub fn count() -> usize {
    registry().read().map_or(0, |guard| guard.definitions.len())
}

/// Resets the registry to the prelude types.
///
/// Test code must have exclusive use of the registry and discard every prior
/// user-defined `AdtId` before calling this function.
pub fn reset() {
    if let Ok(mut table) = schemas().write() {
        table.clear();
    }
    if let Ok(mut guard) = registry().write() {
        *guard = Registry::with_prelude();
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
        set_ctors(OPTION_ID, Vec::new());
        set_module(OPTION_ID, ModulePath::new(["Other"]));
        assert!(module_of(OPTION_ID).is_prelude());
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
        let (first, created) = intern_with_status(adt.clone());
        let (second, reused) = intern_with_status(adt);
        assert!(created);
        assert!(!reused);
        assert_eq!(first, second);
    }

    #[test]
    fn host_interning_cannot_capture_a_same_named_source_declaration() {
        let name = "HostInternNominalIsolation";
        let source = allocate_in(ModulePath::new(["User"]), name, Vec::new());
        let host = intern(Adt {
            name: name.to_string(),
            params: Vec::new(),
            ctors: Vec::new(),
        });
        assert_ne!(source, host);
        assert_eq!(key_of(source).module, ModulePath::new(["User"]));
        assert!(key_of(host).module.is_prelude());
    }

    #[test]
    fn allocation_never_reuses_an_id() {
        // §3.2.4: two compilation units that declare the same name
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
