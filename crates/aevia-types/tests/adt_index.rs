//! Nominal registry lookup preserves stable type identities across reuse.

use aevia_types::ModulePath;
use aevia_types::Type;
use aevia_types::adt;
use aevia_types::keys::ModuleOrigin;
use aevia_types::persist;
use std::sync::Barrier;
use std::thread;

fn declared(
    module: ModulePath,
    name: &str,
    fields: Vec<Type>,
) -> adt::AdtId {
    let id = adt::allocate_in(module, name, vec![]);
    adt::set_ctors(
        id,
        vec![adt::Ctor {
            name: format!("{name}Constructor"),
            fields,
        }],
    );
    id
}

fn grow_registry() {
    for n in 0..192 {
        let _ = adt::allocate_in(
            ModulePath::new(["RegistryIndexNoise"]),
            &format!("Unused{n}"),
            vec![],
        );
    }
}

#[test]
fn lookup_and_persistence_use_the_first_complete_matching_schema() {
    let name = "RegistryIndexItem";
    let path = |package: &str| {
        ModulePath::new(["SameDisplay"]).with_origin(ModuleOrigin {
            package: package.into(),
            source: "src/Main.aev".into(),
        })
    };
    let module = path("source-package");
    let _foreign = declared(path("foreign-package"), name, vec![Type::Bool]);
    let _incomplete = adt::allocate_in(module.clone(), name, vec![]);
    let different = declared(module.clone(), name, vec![Type::Str]);
    let first = declared(module.clone(), name, vec![Type::Bool]);
    let latest = declared(module, name, vec![Type::Bool]);
    assert_ne!(adt::key_of(different), adt::key_of(latest));
    assert_eq!(adt::key_of(first), adt::key_of(latest));
    assert_eq!(adt::find_complete(&adt::key_of(latest)), Some(first));

    let bytes = persist::encode(&Type::Adt(latest, vec![])).unwrap();
    assert_eq!(
        persist::decode::<Type>(&bytes).unwrap(),
        Type::Adt(first, vec![])
    );
}

#[test]
fn a_primed_cache_adds_finalized_types_in_original_allocation_order() {
    let module = ModulePath::new(["RegistryIndexPrimed"]);
    let name = "PrimedItem";
    let earliest = adt::allocate_in(module.clone(), name, vec![]);
    let latest = declared(module, name, vec![Type::Bool]);
    let key = adt::key_of(latest);
    grow_registry();
    for _ in 0..12 {
        assert_eq!(adt::find_complete(&key), Some(latest));
    }

    adt::set_ctors(
        earliest,
        vec![adt::Ctor {
            name: format!("{name}Constructor"),
            fields: vec![Type::Bool],
        }],
    );
    assert_eq!(adt::key_of(earliest), key);
    assert_eq!(adt::find_complete(&key), Some(earliest));
}

#[test]
fn a_cached_miss_is_invalidated_when_the_first_candidate_finalizes() {
    grow_registry();
    let pending = adt::allocate("RegistryIndexAbsence", vec![]);
    let key = adt::key_of(pending);
    assert_eq!(adt::find_complete(&key), None);
    assert_eq!(adt::find_complete(&key), None);
    adt::set_ctors(pending, vec![]);
    assert_eq!(adt::find_complete(&key), Some(pending));
}

#[test]
fn schema_lookup_can_be_retried_after_forward_declarations_are_finalized() {
    let module = ModulePath::new(["RegistryIndexForward"]);
    let child = adt::allocate_in(module.clone(), "Child", vec![]);
    let parent = declared(module.clone(), "Parent", vec![Type::Adt(child, vec![])]);
    assert!(adt::key_of(parent).schema.is_none());
    adt::set_ctors(
        child,
        vec![adt::Ctor {
            name: "ChildConstructor".into(),
            fields: vec![Type::Bool],
        }],
    );
    let key = adt::key_of(parent);
    assert!(key.schema.is_some());
    let matching_child = declared(module.clone(), "Child", vec![Type::Bool]);
    let matching_parent = declared(module, "Parent", vec![Type::Adt(matching_child, vec![])]);
    assert_eq!(adt::key_of(matching_parent), key);
    assert_eq!(adt::find_complete(&key), Some(parent));
}

#[test]
fn concurrent_host_interning_allocates_one_type_and_excludes_same_named_sources() {
    let name = "RegistryIndexConcurrentHost";
    let conflicting = declared(ModulePath::new(["RegistryIndex"]), name, vec![]);
    let barrier = Barrier::new(8);
    thread::scope(|scope| {
        let jobs: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    adt::intern(adt::Adt {
                        name: name.into(),
                        params: vec![],
                        ctors: vec![],
                    })
                })
            })
            .collect();
        let mut ids = jobs.into_iter().map(|job| job.join().unwrap());
        let first = ids.next().unwrap();
        assert_ne!(first, conflicting);
        assert!(adt::module_of(first).is_prelude());
        assert!(ids.all(|id| id == first));
    });
}

#[test]
fn host_lookup_respects_module_changes_on_unfinished_records() {
    let name = "RegistryIndexHostMoved";
    let intern = || {
        adt::intern(adt::Adt {
            name: name.into(),
            params: vec![],
            ctors: vec![],
        })
    };
    let original = intern();
    let other_module = ModulePath::new(["RegistryIndex"]);
    adt::set_module(original, other_module.clone());
    assert_eq!(adt::module_of(original), other_module);
    let host = intern();
    assert_ne!(host, original);
    assert_eq!(intern(), host);
    adt::set_module(original, ModulePath::prelude());
    assert_eq!(intern(), original);
}
