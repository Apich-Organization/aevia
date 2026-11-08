//! The per-module on-disk checked-module cache, `.aevi` (spec §7.3, §9.11).
//!
//! A cached module is restored with the process-local `AdtId`s remapped, so
//! the tests run the restored program and match on its constructors rather than
//! comparing bytes: the remapping is correct exactly when the program behaves
//! as it did when it was freshly checked. The cross-process half of that claim
//! is covered by `aevia-cli/tests/module_cache.rs`.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::disk_cache::DiskCache;
use aevia_core::driver;
use aevia_core::driver::LoadStats;
use aevia_core::driver::Program;
use aevia_core::manifest::Roots;
use aevia_core::source::SourceOverlays;
use aevia_types::check::Capabilities;
use aevia_types::check::CheckOptions;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "aevia-disk-cache-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create scratch directory");
        Self(path)
    }

    fn write(
        &self,
        name: &str,
        source: &str,
    ) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, source).expect("write module");
        path
    }

    fn cache(&self) -> DiskCache {
        DiskCache::at(self.0.join(".aevia").join("cache").join("modules"))
    }

    fn aevi_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(self.cache().dir())
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "aevi"))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        files
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const SHAPES: &str = "\
module Shapes

type Shape = | Circle(Float) | Square(Float) | Tri(Float, Float)
type Vec2 = { x: Float, y: Float }

class Area a where
    area(x: a): Float

instance Area for Shape where
    area(s) = match s with
        | Circle(r) -> 3.0 * r * r
        | Square(w) -> w * w
        | Tri(b, h) -> 0.5 * b * h

let scale = 2.0

fn area_of(s: Shape): Float = area(s)
fn norm2(v: Vec2): Float = v.x * v.x + v.y * v.y
fn first(p, q) = p
";

const UTIL: &str = "\
module Util

import Shapes { Shape, Circle, Square, Tri, Vec2, area_of, norm2, scale }

fn scaled(s: Shape): Float = scale * area_of(s)
fn flip(s: Shape): Shape = match s with
    | Circle(r) -> Square(r)
    | Square(w) -> Circle(w)
    | Tri(b, h) -> Tri(h, b)
fn length2(x: Float, y: Float): Float = norm2({ x = x, y = y })
";

const MAIN: &str = "\
import Shapes { Shape, Circle, Square, Tri, first }
import Util

fn name(s: Shape): String = match s with
    | Circle(r) -> \"circle {r}\"
    | Square(w) -> \"square {w}\"
    | Tri(b, h) -> \"tri {b} {h}\"

fn main(): IO Unit =
    do
        println(name(Util.flip(Circle(2.0))))
        println(name(Util.flip(Tri(1.0, 3.0))))
        println(\"{Util.scaled(Square(3.0))}\")
        println(\"{Util.length2(3.0, 4.0)}\")
        println(\"{first(7, 8)}\")
";

const EXPECTED: &str = "square 2.0\ntri 3.0 1.0\n18.0\n25.0\n7\n";

fn project(scratch: &Scratch) -> PathBuf {
    scratch.write("Shapes.aev", SHAPES);
    scratch.write("Util.aev", UTIL);
    scratch.write("main.aev", MAIN)
}

fn load_with(
    entry: &Path,
    disk: &DiskCache,
    options: &CheckOptions,
) -> (Program, LoadStats) {
    driver::load_with_disk_cache(
        entry,
        Roots::adjacent(entry),
        &SourceOverlays::new(),
        options,
        disk,
    )
}

fn load(
    entry: &Path,
    disk: &DiskCache,
) -> (Program, LoadStats) {
    let (program, stats) = load_with(entry, disk, &CheckOptions::default());
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    (program, stats)
}

fn output(program: &Program) -> String {
    let mut interp = Interp::new(&program.entry().expect("entry").checked)
        .with_buffer()
        .with_mode(Mode::Ir);
    let diagnostics = driver::run_modules(&mut interp, program);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    interp.run_main().expect("main runs");
    interp.captured()
}

#[test]
fn a_second_loader_restores_every_module_and_the_program_runs_identically() {
    let scratch = Scratch::new("warm");
    let entry = project(&scratch);
    let disk = scratch.cache();

    let (cold, stats) = load(&entry, &disk);
    assert_eq!(stats.modules, 3);
    assert_eq!(stats.disk_hits, 0);
    assert_eq!(stats.checks, 3);
    assert_eq!(stats.disk_writes, 3);
    assert_eq!(scratch.aevi_files().len(), 3);
    assert_eq!(output(&cold), EXPECTED);

    // A separate loader: nothing is shared with the first but the files.
    let (warm, stats) = load(&entry, &disk);
    assert_eq!(stats.modules, 3);
    assert_eq!(stats.disk_hits, 3, "{stats:?}");
    assert_eq!(stats.checks, 0, "{stats:?}");
    assert_eq!(stats.disk_writes, 0);
    // Constructors, pattern matching and the instance work on restored ADTs.
    assert_eq!(output(&warm), EXPECTED);
}

#[test]
fn the_standard_library_is_cached_too() {
    let scratch = Scratch::new("stdlib");
    let entry = scratch.write(
        "main.aev",
        "import Math\nfn main(): IO Unit = println(\"{Math.sqrt(16.0)}\")\n",
    );
    let disk = scratch.cache();
    let (cold, first) = load(&entry, &disk);
    let expected = output(&cold);
    assert!(first.disk_writes >= 2, "{first:?}");
    let (warm, second) = load(&entry, &disk);
    assert_eq!(second.checks, 0, "{second:?}");
    assert_eq!(second.disk_hits, second.modules);
    assert_eq!(output(&warm), expected);
}

#[test]
fn editing_a_body_without_changing_the_interface_rechecks_only_that_module() {
    let scratch = Scratch::new("body");
    let entry = project(&scratch);
    let disk = scratch.cache();
    let (_, _) = load(&entry, &disk);

    // Same length, so no span after it moves: `scale` is `3.0` now.
    scratch.write(
        "Shapes.aev",
        &SHAPES.replace("let scale = 2.0", "let scale = 3.0"),
    );
    let (program, stats) = load(&entry, &disk);
    assert_eq!(stats.checks, 1, "{stats:?}");
    assert_eq!(stats.disk_hits, 2, "{stats:?}");
    assert_eq!(stats.disk_writes, 1);
    assert_eq!(output(&program), EXPECTED.replace("18.0\n", "27.0\n"));
}

#[test]
fn editing_an_interface_rechecks_the_dependents() {
    let scratch = Scratch::new("interface");
    let entry = project(&scratch);
    let disk = scratch.cache();
    let (_, _) = load(&entry, &disk);

    scratch.write("Shapes.aev", &format!("{SHAPES}fn third(a, b, c) = c\n"));
    let (program, stats) = load(&entry, &disk);
    assert_eq!(stats.checks, 3, "{stats:?}");
    assert_eq!(stats.disk_hits, 0, "{stats:?}");
    assert_eq!(output(&program), EXPECTED);
}

#[test]
fn a_corrupt_truncated_or_foreign_file_is_a_miss_that_is_overwritten() {
    let scratch = Scratch::new("corrupt");
    let entry = project(&scratch);
    let disk = scratch.cache();
    let (_, _) = load(&entry, &disk);
    let files = scratch.aevi_files();
    assert_eq!(files.len(), 3);

    // Garbage, truncation, and a flipped byte inside the body.
    std::fs::write(&files[0], b"not an aevi file").unwrap();
    let truncated = std::fs::read(&files[1]).unwrap();
    std::fs::write(&files[1], &truncated[..truncated.len() / 2]).unwrap();
    let mut flipped = std::fs::read(&files[2]).unwrap();
    let last = flipped.len() - 1;
    flipped[last] ^= 0xff;
    std::fs::write(&files[2], &flipped).unwrap();

    let (program, stats) = load(&entry, &disk);
    assert_eq!(stats.disk_hits, 0, "{stats:?}");
    assert_eq!(stats.checks, 3, "{stats:?}");
    assert_eq!(stats.disk_writes, 3);
    assert_eq!(output(&program), EXPECTED);

    // The damaged files were replaced, so the next load is warm again.
    let (_, stats) = load(&entry, &disk);
    assert_eq!(stats.disk_hits, 3, "{stats:?}");
}

#[test]
fn a_different_capability_set_is_a_miss() {
    let scratch = Scratch::new("capabilities");
    let entry = project(&scratch);
    let disk = scratch.cache();
    let (_, _) = load(&entry, &disk);

    let bare = CheckOptions {
        capabilities: Capabilities::from_names(Vec::<String>::new()),
    };
    let (program, stats) = load_with(&entry, &disk, &bare);
    assert!(!program.diagnostics.has_errors());
    assert_eq!(stats.disk_hits, 0, "{stats:?}");
    assert_eq!(stats.checks, 3);
    // Both sets now have entries side by side.
    let (_, stats) = load_with(&entry, &disk, &bare);
    assert_eq!(stats.disk_hits, 3, "{stats:?}");
    let (_, stats) = load(&entry, &disk);
    assert_eq!(stats.disk_hits, 3, "{stats:?}");
}

#[test]
fn a_changed_import_binding_is_a_miss() {
    let scratch = Scratch::new("bindings");
    let entry = project(&scratch);
    let disk = scratch.cache();
    let (_, _) = load(&entry, &disk);
    // The entry now imports `Util` under an alias: same text is not enough.
    let other = MAIN
        .replace("import Util\n", "import Util as U\n")
        .replace("Util.", "U.");
    scratch.write("main.aev", &other);
    let (program, stats) = load(&entry, &disk);
    assert_eq!(stats.checks, 1, "{stats:?}");
    assert_eq!(output(&program), EXPECTED);
}

/// Replaces every process-local id in a `Debug` rendering by `#`.
fn normalise(debug: &str) -> String {
    let mut text = debug.to_string();
    for marker in ["Adt(", "adt: ", ": ("] {
        let mut out = String::new();
        let mut rest = text.as_str();
        while let Some(at) = rest.find(marker) {
            let (head, tail) = rest.split_at(at + marker.len());
            out.push_str(head);
            let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 {
                out.push('#');
            }
            rest = &tail[digits..];
        }
        out.push_str(rest);
        text = out;
    }
    text
}

/// The `Debug` rendering of the parts of a checked module that are ordered
/// maps (the span-keyed `HashMap`s render in arbitrary order; the byte-for-byte
/// comparison of the whole encoding covers those), ids normalised.
fn ordered(module: &aevia_types::CheckedModule) -> String {
    normalise(&format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        module.name,
        module.identity,
        module.interface(),
        module.constructors,
        module.local_classes,
        module.dims,
        module.tests,
        module.stability,
        module.aliases,
        module.unit_system,
        module.registrations,
        module.errors,
    ))
}

#[test]
fn the_codec_round_trips_checked_modules_with_fresh_adt_ids() {
    use aevia_types::persist;

    let scratch = Scratch::new("roundtrip");
    let entry = project(&scratch);
    let disk = scratch.cache();
    let (program, _) = load(&entry, &disk);
    let names: Vec<_> = program.modules.iter().map(|m| m.path.join(".")).collect();
    assert!(names.iter().any(|name| name == "Shapes"), "{names:?}");

    let math = scratch.write("usemath.aev", "import Math\nlet x = Math.sqrt(4.0)\n");
    let (math_program, _) = load(&math, &disk);

    for module in program.modules.iter().chain(math_program.modules.iter()) {
        let checked = &module.checked;
        let bytes = persist::encode(checked).expect("encodes");
        // Deterministic: the same value encodes to the same bytes.
        assert_eq!(bytes, persist::encode(checked).unwrap());
        // `decode_fresh` allocates new ids for the module's own types, as a
        // process that had never seen them would.
        let fresh: aevia_types::CheckedModule = persist::decode_fresh(&bytes).expect("decodes");
        assert_eq!(
            ordered(checked),
            ordered(&fresh),
            "module {}",
            module.path.join(".")
        );
        // Symbolic identity survives: re-encoding the decoded module gives the
        // same bytes, since the table is written by key and structure, not id.
        assert_eq!(persist::encode(&fresh).unwrap(), bytes);
        // And the reusing decoder maps onto the registered types themselves.
        let same: aevia_types::CheckedModule = persist::decode(&bytes).expect("decodes");
        assert_eq!(persist::encode(&same).unwrap(), bytes);
        assert_eq!(
            format!("{:?}", checked.constructors),
            format!("{:?}", same.constructors)
        );
        assert_eq!(
            same.constructors.values().collect::<Vec<_>>(),
            checked.constructors.values().collect::<Vec<_>>(),
            "reuse maps onto the registered types themselves"
        );
    }
    let shapes = program
        .modules
        .iter()
        .find(|module| module.path == ["Shapes"])
        .unwrap();
    let bytes = persist::encode(&shapes.checked).unwrap();
    let fresh: aevia_types::CheckedModule = persist::decode_fresh(&bytes).unwrap();
    let (adt, _) = fresh.constructors["Circle"];
    let (original, _) = shapes.checked.constructors["Circle"];
    assert_ne!(adt, original, "decode_fresh must remap, not alias");
    assert_eq!(aevia_types::adt::name_of(adt), "Shape");
    assert_eq!(aevia_types::adt::get(adt).unwrap().ctors.len(), 3);
}
