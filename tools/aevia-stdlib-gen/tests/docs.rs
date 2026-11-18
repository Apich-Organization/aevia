//! The generated standard library carries real documentation.
//!
//! Every re-exported name is documented with the signature chapter 8 states
//! for it and a description written for the Aevia caller (the guide in
//! `aevia-stdlib-model`), every declared type and function has a `##` block,
//! and the generator's notice appears once per file as a plain comment, never
//! as documentation.
use std::path::Path;

fn stdlib_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/aevia-std/stdlib")
}

fn sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![stdlib_dir()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "aev") {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                out.push((name, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn the_generator_notice_is_not_documentation() {
    for (name, source) in sources() {
        assert!(
            !source
                .lines()
                .any(|line| line.starts_with("## Generated standard library")),
            "{name}: the notice must be a plain `#` comment, not a `##` doc line"
        );
    }
}

#[test]
fn every_reexported_name_is_documented() {
    let mut bindings = 0;
    let mut described = 0;
    for (name, source) in sources() {
        // The unit and constant modules are generated from the manifest's
        // tables, with their own per-constant documentation.
        if source.contains("module Units.") {
            continue;
        }
        let lines: Vec<&str> = source.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            let Some(rest) = line.strip_prefix("let ") else {
                continue;
            };
            if rest
                .split_once(" = ")
                .is_none_or(|(left, right)| left != right)
            {
                continue;
            }
            bindings += 1;
            assert!(
                index > 0 && lines[index - 1].starts_with("##"),
                "{name}: `{line}` has no `##` documentation"
            );
            let mut at = index;
            while at > 0 && lines[at - 1].starts_with("##") {
                at -= 1;
                if lines[at].contains("*Source:") {
                    described += 1;
                    break;
                }
            }
        }
    }
    assert!(bindings > 300, "only {bindings} documented bindings found");
    assert_eq!(
        described, bindings,
        "every re-exported name must carry a description, not just its signature"
    );
}

#[test]
fn an_operator_entry_has_signature_kernel_signature_dimensions_and_prose() {
    let calculus = sources()
        .into_iter()
        .find(|(name, _)| name == "Calculus.aev")
        .expect("Calculus.aev")
        .1;
    let start = calculus
        .find("let diff = diff")
        .expect("diff is re-exported");
    let block: Vec<&str> = calculus[..start]
        .lines()
        .rev()
        .take_while(|l| l.starts_with("##"))
        .collect();
    let text = block.into_iter().rev().collect::<Vec<_>>().join("\n");
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
}

/// Every `type`, `fn` and `let` declaration of a generated module, with the
/// `##` block above it (empty when there is none).
fn declarations(source: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let is_declaration = ["type ", "fn ", "let "]
            .iter()
            .any(|keyword| line.starts_with(keyword));
        if !is_declaration {
            continue;
        }
        let mut at = index;
        while at > 0 && lines[at - 1].starts_with("##") {
            at -= 1;
        }
        found.push((line.to_string(), lines[at..index].join("\n")));
    }
    found
}

#[test]
fn every_declaration_of_a_generated_module_has_a_description() {
    let mut missing = Vec::new();
    for (name, source) in sources() {
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
}

#[test]
fn a_description_is_more_than_its_signature() {
    let mut thin = Vec::new();
    for (name, source) in sources() {
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
}

#[test]
fn the_documentation_does_not_speak_rust() {
    let mut offences = Vec::new();
    for (name, source) in sources() {
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
}
