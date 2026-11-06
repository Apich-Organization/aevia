//! Every manifest namespace is available through the normal module loader.
#[test]
fn all_live_kernel_namespaces_import_and_check() {
    let files = aevia_rssn::codegen::namespace_files(aevia_rssn::registry());
    let mut entry = String::from("module NamespaceProbe\n");
    for (path, _) in &files {
        let name = path.strip_suffix(".aev").expect("generated Aevia module");
        entry.push_str(&format!("import {name}\n"));
    }
    entry.push_str("\nfn main(): IO Unit = println(\"\")\n");
    let dir = std::env::temp_dir().join(format!("aevia-kernel-namespaces-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("probe.aev");
    std::fs::write(&path, entry).unwrap();
    let program = aevia_core::driver::load(&path);
    let errors: Vec<_> = program
        .diagnostics
        .items
        .iter()
        .filter(|d| d.code.starts_with('E'))
        .map(|d| d.render(&program.sources))
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        errors.is_empty(),
        "{} namespace import errors (first 20):\n{}",
        errors.len(),
        errors
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(
        program.modules.len(),
        files.len() + 1,
        "each namespace should resolve once"
    );
}

#[test]
fn unknown_kernel_namespace_reports_missing_module() {
    let dir = std::env::temp_dir().join(format!(
        "aevia-missing-kernel-namespace-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("probe.aev");
    std::fs::write(&path, "import Rssn_this_namespace_does_not_exist\n").unwrap();
    let program = aevia_core::driver::load(&path);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        program
            .diagnostics
            .items
            .iter()
            .any(|d| d.code == "E0607" && d.message.contains("Rssn_this_namespace_does_not_exist"))
    );
}
