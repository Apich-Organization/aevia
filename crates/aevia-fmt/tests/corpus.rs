//! The formatter against every program in the tree.
//!
//! `examples/**/*.aev` and `tests/*.aev` are the corpus the language is
//! specified against, so a formatter that cannot print them is not finished.
//! For each file the test asserts the two properties the formatter promises:
//! the printed text parses to the same tree (spans excluded), and formatting it
//! again changes nothing.

use aevia_fmt::fingerprint;
use aevia_fmt::format_source;
use aevia_syntax::Module;
use aevia_syntax::lex;
use aevia_syntax::parse_module;
use std::path::Path;
use std::path::PathBuf;

fn corpus() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let mut files = Vec::new();
    for directory in ["examples", "tests"] {
        collect(&root.join(directory), &mut files);
    }
    files.sort();
    assert!(!files.is_empty(), "the corpus is empty");
    files
}

fn collect(
    dir: &Path,
    out: &mut Vec<PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "aev") {
            out.push(path);
        }
    }
}

fn parse(source: &str) -> Module {
    let lexed = lex(0, source).expect("lexes");
    let (module, diagnostics) = parse_module(0, source, &lexed);
    assert!(
        !diagnostics.has_errors(),
        "parse errors: {:?}",
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    module
}

#[test]
fn every_program_formats_without_changing_its_tree() {
    for file in corpus() {
        let name = file.display();
        let source = std::fs::read_to_string(&file).expect("reads");
        let formatted = format_source(&source).unwrap_or_else(|error| {
            panic!("{name}: {error}");
        });
        let before = fingerprint::module(&parse(&source));
        let after = fingerprint::module(&parse(&formatted));
        assert_eq!(before, after, "{name}: the tree changed");
    }
}

#[test]
fn every_program_is_a_fixed_point() {
    for file in corpus() {
        let name = file.display();
        let source = std::fs::read_to_string(&file).expect("reads");
        let once = format_source(&source).expect("formats");
        let twice = format_source(&once).expect("formats again");
        assert_eq!(once, twice, "{name}: not idempotent");
    }
}

#[test]
fn every_comment_in_the_corpus_survives() {
    // A formatter that silently drops comments is worse than no formatter.
    for file in corpus() {
        let name = file.display();
        let source = std::fs::read_to_string(&file).expect("reads");
        let formatted = format_source(&source).expect("formats");
        let before = aevia_fmt::comments::scan(&source);
        let after = aevia_fmt::comments::scan(&formatted);
        let texts = |list: &[aevia_fmt::comments::Comment]| {
            list.iter().map(|c| c.text.clone()).collect::<Vec<_>>()
        };
        assert_eq!(texts(&before), texts(&after), "{name}: comments changed");
    }
}
