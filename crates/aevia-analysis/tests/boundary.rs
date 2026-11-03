//! The architectural boundary, as a test.
//!
//! `aevia-analysis` is the semantic core; `aevia-lsp` is a translation table
//! over it. The rule that keeps that true is mechanical — *no protocol crate,
//! no protocol vocabulary* — so it is checked mechanically rather than left to
//! review. A future contributor who adds `lsp-types` to make one query easier
//! breaks this test, and the failure says why.

use std::path::{Path, PathBuf};

/// The crate's own manifest, read at compile time.
const MANIFEST: &str = include_str!("../Cargo.toml");

/// Crates that would bind the core to a protocol or a runtime.
const FORBIDDEN_CRATES: &[&str] = &[
    "lsp-types",
    "lsp_server",
    "lsp",
    "jsonrpc",
    "jsonrpc-core",
    "jsonrpc_core",
    "tower-lsp",
    "tower",
    "tokio",
    "async-std",
    "serde",
    "serde_json",
    "axum",
    "hyper",
];

/// Words that belong to the protocol and must not appear in the core.
const FORBIDDEN_WORDS: &[&str] = &[
    "textDocument/",
    "workspace/",
    "window/",
    "jsonrpc",
    "lsp_types",
    "lsp-types",
    "Content-Length",
    "publishDiagnostics",
    "capabilities",
];

#[test]
fn the_manifest_depends_on_the_compiler_and_nothing_else() {
    let dependencies = MANIFEST
        .split("[dependencies]")
        .nth(1)
        .expect("the crate has a [dependencies] section");
    let dependencies = dependencies.split("\n[").next().unwrap_or(dependencies);
    // Comments are allowed to talk about the boundary; the dependencies are
    // not allowed to cross it.
    let declarations: Vec<&str> = dependencies
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let lowered = declarations.join("\n").to_lowercase();
    for forbidden in FORBIDDEN_CRATES {
        assert!(
            !lowered.contains(forbidden),
            "`{forbidden}` is a protocol or runtime crate and must not be a dependency of \
             aevia-analysis:\n{}",
            declarations.join("\n")
        );
    }
    // Every dependency is a workspace member or a pure-Rust utility crate that
    // carries no protocol.
    for line in declarations {
        assert!(
            line.contains("workspace = true") || line.starts_with("aevia-"),
            "unexpected dependency line `{line}`: allow it only if it is free of protocol \
             vocabulary"
        );
    }
}

#[test]
fn the_sources_use_no_protocol_vocabulary() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = 0;
    for file in rust_files(&root) {
        let text = std::fs::read_to_string(&file).expect("a source file");
        let lowered = text.to_lowercase();
        for word in FORBIDDEN_WORDS {
            assert!(
                !lowered.contains(&word.to_lowercase()),
                "{} mentions `{word}`; that belongs in aevia-lsp",
                file.display()
            );
        }
        checked += 1;
    }
    assert!(checked >= 5, "the source scan found only {checked} files");
}

/// Every `.rs` file under `directory`.
fn rust_files(directory: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    files
}
