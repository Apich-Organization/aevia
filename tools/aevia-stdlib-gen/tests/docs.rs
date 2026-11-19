//! The generated standard library carries real documentation.
//!
//! Every re-exported name is documented with the signature chapter 8 states
//! for it and a description written for the Aevia caller (the guide in
//! `aevia-stdlib-model`), every declared type and function has a `##` block,
//! and the generator's notice appears once per file as a plain comment, never
//! as documentation.
use std::path::Path;

fn sources() -> std::io::Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut stack =
        vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/aevia-std/stdlib")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "aev") {
                let name = path
                    .file_name()
                    .ok_or_else(|| std::io::Error::other("source path has no file name"))?
                    .to_string_lossy()
                    .into_owned();
                out.push((name, std::fs::read_to_string(&path)?));
            }
        }
    }
    out.sort();
    Ok(out)
}

#[test]
fn the_generator_notice_is_not_documentation() -> Result<(), Box<dyn std::error::Error>> {
    for (name, source) in sources()? {
        assert!(
            !source
                .lines()
                .filter_map(|line| line.strip_prefix("##"))
                .any(|doc| {
                    let doc = doc.trim().to_ascii_lowercase();
                    doc.starts_with("generated standard library")
                        || doc.starts_with("generated from the rssn kernel manifest")
                        || doc.contains("cargo run -p aevia-stdlib-gen")
                        || (doc.starts_with("rssn ") && doc.contains(", api "))
                }),
            "{name}: the notice must be a plain `#` comment, not a `##` doc line"
        );
    }
    Ok(())
}

#[test]
fn every_reexported_name_is_documented() -> Result<(), Box<dyn std::error::Error>> {
    let mut bindings = 0_usize;
    let mut described = 0_usize;
    for (name, source) in sources()? {
        // The unit and constant modules are generated from the manifest's
        // tables, with their own per-constant documentation.
        if source.contains("module Units.") {
            continue;
        }
        for (line, doc) in declarations(&source) {
            let Some(rest) = line.strip_prefix("let ") else {
                continue;
            };
            if rest
                .split_once(" = ")
                .is_none_or(|(left, right)| left != right)
            {
                continue;
            }
            bindings = bindings.saturating_add(1);
            assert!(
                !doc.is_empty(),
                "{name}: `{line}` has no `##` documentation"
            );
            if doc.contains("*Source:") {
                described = described.saturating_add(1);
            }
        }
    }
    assert!(bindings > 300, "only {bindings} documented bindings found");
    assert_eq!(
        described, bindings,
        "every re-exported name must carry a description, not just its signature"
    );
    Ok(())
}

#[test]
fn an_operator_entry_has_signature_kernel_signature_dimensions_and_prose()
-> Result<(), Box<dyn std::error::Error>> {
    let (_, calculus) = sources()?
        .into_iter()
        .find(|(name, _)| name == "Calculus.aev")
        .ok_or("missing Calculus.aev")?;
    let (_, text) = declarations(&calculus)
        .into_iter()
        .find(|(declaration, _)| declaration.starts_with("let diff = diff"))
        .ok_or("diff is not re-exported")?;
    for needle in [
        "derivative",
        "**Signature:** `diff :",
        "**Kernel signature**",
        "**Dimensions:**",
        "*Source: Aevia standard library guide.*",
    ] {
        assert!(
            text.contains(needle),
            "diff's documentation lacks `{needle}`:\n{text}"
        );
    }
    Ok(())
}

/// Every `type`, `fn` and `let` declaration of a generated module, with the
/// `##` block above it (empty when there is none).
fn declarations(source: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut docs = Vec::new();
    for line in source.lines() {
        if line.starts_with("##") {
            docs.push(line);
            continue;
        }
        if ["type ", "fn ", "let "]
            .iter()
            .any(|keyword| line.starts_with(keyword))
        {
            found.push((line.to_string(), docs.join("\n")));
        }
        docs.clear();
    }
    found
}

#[test]
fn every_declaration_of_a_generated_module_has_a_description()
-> Result<(), Box<dyn std::error::Error>> {
    let mut missing = Vec::new();
    for (name, source) in sources()? {
        // Unit and constant modules document each constant from the
        // manifest's own tables.
        if source.contains("module Units.") {
            continue;
        }
        for (declaration, docs) in declarations(&source) {
            if docs.trim_start_matches("##").trim().is_empty() {
                missing.push(format!("{name}: {declaration}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "declarations with no `##` description:\n{}",
        missing.join("\n")
    );
    Ok(())
}

#[test]
fn a_description_is_more_than_its_signature() -> Result<(), Box<dyn std::error::Error>> {
    let mut thin = Vec::new();
    for (name, source) in sources()? {
        if source.contains("module Units.") {
            continue;
        }
        for (declaration, docs) in declarations(&source) {
            let prose = docs
                .lines()
                .map(|line| line.trim_start_matches('#').trim())
                .filter(|line| !line.is_empty() && !line.starts_with("**Signature:**"))
                .count();
            if prose == 0 {
                thin.push(format!("{name}: {declaration}"));
            }
        }
    }
    assert!(
        thin.is_empty(),
        "declarations documented by a signature alone:\n{}",
        thin.join("\n")
    );
    Ok(())
}

#[test]
fn the_documentation_does_not_speak_rust() -> Result<(), Box<dyn std::error::Error>> {
    let mut offences = Vec::new();
    for (name, source) in sources()? {
        if source.contains("module Units.") {
            continue;
        }
        for (declaration, docs) in declarations(&source) {
            // Kernel signatures list the kernel's own type names; only the
            // prose is checked.
            let prose: Vec<&str> = docs
                .lines()
                .filter(|line| !line.contains("**Kernel signature") && !line.contains("- `"))
                .collect();
            if let Some(word) = aevia_stdlib_model::guides::find_rust_ism(&prose.join(" ")) {
                offences.push(format!("{name}: {declaration} mentions `{word}`"));
            }
        }
    }
    assert!(
        offences.is_empty(),
        "documentation written for Rust developers:\n{}",
        offences.join("\n")
    );
    Ok(())
}
