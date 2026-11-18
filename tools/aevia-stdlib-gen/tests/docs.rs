//! The generated standard library carries real documentation.
//!
//! Every re-exported name is documented with at least the signature chapter 8
//! states for it; names that have a kernel operator or a documented
//! `aevia-std` function also carry its description, and the generator's
//! notice appears once per file as a plain comment, never as documentation.
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
    assert!(
        described > 100,
        "only {described} of {bindings} names carry a kernel or Rust description"
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
        "Symbolic derivative",
        "**Signature:** `diff :",
        "**Kernel signature**",
        "**Dimensions:**",
        "*Source: rssn operator",
    ] {
        assert!(
            text.contains(needle),
            "diff's documentation lacks `{needle}`:\n{text}"
        );
    }
}
