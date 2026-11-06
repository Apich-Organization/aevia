//! Each generated namespace binding must correspond to its own manifest row.
use aevia_rssn::bindings::Coverage;
use aevia_rssn::bindings::catalog;
use aevia_rssn::codegen;
use aevia_rssn::registry;
use std::collections::BTreeMap;

#[test]
fn every_manifest_binding_is_in_the_right_namespace_with_the_right_target() {
    let registry = registry();
    let files = codegen::namespace_files(registry);
    let groups: BTreeMap<_, _> = files
        .iter()
        .map(|(path, source)| {
            let group = source
                .lines()
                .next()
                .unwrap()
                .strip_prefix("## Kernel rule set: ")
                .unwrap()
                .strip_suffix('.')
                .unwrap();
            (group, (path, source))
        })
        .collect();
    assert_eq!(
        groups.len(),
        files.len(),
        "each namespace must identify a unique rule set"
    );
    for row in catalog(registry) {
        let (path, source) = groups
            .get(row.op.rule_set.as_str())
            .expect("missing rule set");
        let function = format!("fn {}", row.name);
        let value = format!("let {}:", row.name);
        let declarations: Vec<_> = source
            .lines()
            .filter(|line| {
                line.strip_prefix(&function)
                    .is_some_and(|tail| tail.starts_with('(') || tail.starts_with('<'))
                    || line.starts_with(&value)
            })
            .collect();
        assert_eq!(declarations.len(), 1, "{}: binding {}", path, row.name);
        let line = declarations[0];
        match row.coverage {
            | Coverage::Typed => {
                let key = row
                    .op
                    .key(row.overload.unwrap())
                    .expect("typed binding has a key");
                let hash: String = key
                    .signature_hash
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                assert!(
                    line.contains(&format!("Rssn.invoke(\"{}\", \"{}\",", row.op.name, hash)),
                    "{path}: wrong pinned target for {}: {line}",
                    row.name
                );
            },
            | Coverage::Dynamic(_) | Coverage::Unavailable(_) => {
                assert!(
                    line.contains(&format!("Rssn.dynamic(\"{}\", args)", row.op.name))
                        || line.contains(&format!(
                            "Rssn.call_ho(\"{}\", args, callbacks)",
                            row.op.name
                        )),
                    "{path}: wrong dynamic target for {}: {line}",
                    row.name
                );
            },
        }
    }
}

#[test]
fn colliding_rule_set_names_remain_unique_and_order_independent() {
    let groups = ["normal", "a-b", "a.b"];
    let names = codegen::namespace_names_for(groups);
    assert_eq!(
        names,
        codegen::namespace_names_for(["a.b", "normal", "a-b", "a-b"])
    );
    assert!(
        names
            .iter()
            .any(|(name, group)| name == "Rssn_normal" && group == "normal")
    );
    let collisions: Vec<_> = names
        .iter()
        .filter(|(_, group)| group.starts_with("a"))
        .collect();
    assert_eq!(collisions.len(), 2);
    assert!(
        collisions
            .iter()
            .all(|(name, _)| name.starts_with("Rssn_a_b_") && name.len() > "Rssn_a_b_".len())
    );
    assert_ne!(collisions[0].0, collisions[1].0);
    let expanded = codegen::namespace_names_for(["normal", "a-b", "a.b", "a b"]);
    for (name, group) in &names {
        assert!(
            expanded
                .iter()
                .any(|pair| pair == &(name.clone(), group.clone())),
            "adding a third collision renamed {group}"
        );
    }
    // An ordinary name can also collide with a generated hash-suffixed name.
    let cross = collisions[0].0.strip_prefix("Rssn_").unwrap();
    let cross_names = codegen::namespace_names_for(["a-b", "a.b", cross]);
    let unique: std::collections::BTreeSet<_> = cross_names.iter().map(|(name, _)| name).collect();
    assert_eq!(unique.len(), cross_names.len());
    assert_eq!(
        cross_names,
        codegen::namespace_names_for([cross, "a.b", "a-b"])
    );
}
