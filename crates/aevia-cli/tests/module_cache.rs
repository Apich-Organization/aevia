//! The per-module `.aevi` cache across processes (spec §9.11).
//!
//! `AdtId`s are process-local, so the property worth testing end to end is that
//! a *new process* restores user types from disk and the program, which
//! constructs and matches their constructors, behaves exactly as when it was
//! checked from source.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("aevia-module-cache-{name}-{}", std::process::id()));
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
        std::fs::write(&path, source).expect("write file");
        path
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// Every `.aevi` file with its modification time.
    fn aevi(&self) -> Vec<(String, std::time::SystemTime)> {
        let dir = self.0.join(".aevia").join("cache").join("modules");
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "aevi"))
                    .map(|entry| {
                        (
                            entry.file_name().to_string_lossy().to_string(),
                            entry.metadata().unwrap().modified().unwrap(),
                        )
                    })
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

fn aevia(
    dir: &Path,
    args: &[&str],
) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aevia"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("runs aevia")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n")
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
import Math

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
        println(\"{Math.sqrt(16.0)}\")
";

const EXPECTED: &str = "square 2.0\ntri 3.0 1.0\n18.0\n25.0\n7\n4.0\n";

fn project(scratch: &Scratch) {
    scratch.write("Shapes.aev", SHAPES);
    scratch.write("Util.aev", UTIL);
    scratch.write("main.aev", MAIN);
}

#[test]
fn a_new_process_restores_user_types_from_disk_and_runs_the_program() {
    let scratch = Scratch::new("run");
    project(&scratch);

    let cold = aevia(scratch.path(), &["run", "main.aev"]);
    assert_eq!(stdout(&cold), EXPECTED, "{}", stderr(&cold));
    let written = scratch.aevi();
    // Shapes, Util, main and the standard-library modules they pull in.
    assert!(written.len() >= 4, "{written:?}");

    // The second process restores every module: no file is rewritten.
    let warm = aevia(scratch.path(), &["run", "main.aev"]);
    assert_eq!(stdout(&warm), EXPECTED, "{}", stderr(&warm));
    // Warnings are part of the cached result and are replayed unchanged.
    assert!(stderr(&cold).contains("W0101"), "{}", stderr(&cold));
    assert_eq!(stderr(&warm), stderr(&cold));
    assert_eq!(scratch.aevi(), written, "a warm run must not rewrite files");

    // A third process, with the same result again (the files are stable).
    let again = aevia(scratch.path(), &["run", "main.aev"]);
    assert_eq!(stdout(&again), EXPECTED);
    assert_eq!(scratch.aevi(), written);
}

#[test]
fn an_edit_rewrites_only_what_it_can_change() {
    let scratch = Scratch::new("edit");
    project(&scratch);
    assert_eq!(
        stdout(&aevia(scratch.path(), &["run", "main.aev"])),
        EXPECTED
    );
    let before = scratch.aevi();

    // A same-length body edit: `Shapes` is re-checked and stored under a new
    // key; the interface is unchanged, so no other module gets a new entry.
    scratch.write(
        "Shapes.aev",
        &SHAPES.replace("let scale = 2.0", "let scale = 3.0"),
    );
    let output = aevia(scratch.path(), &["run", "main.aev"]);
    assert_eq!(
        stdout(&output),
        EXPECTED.replace("18.0\n", "27.0\n"),
        "{}",
        stderr(&output)
    );
    let after = scratch.aevi();
    let added: Vec<_> = after.iter().filter(|file| !before.contains(file)).collect();
    assert_eq!(added.len(), 1, "{added:?}");
    assert!(added[0].0.starts_with("Shapes-"), "{added:?}");
}

#[test]
fn check_uses_the_cache_and_no_cache_writes_nothing() {
    let scratch = Scratch::new("check");
    project(&scratch);
    let output = aevia(scratch.path(), &["check", "main.aev", "--no-cache"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(!scratch.path().join(".aevia").exists());

    let output = aevia(scratch.path(), &["run", "main.aev", "--no-cache"]);
    assert_eq!(stdout(&output), EXPECTED, "{}", stderr(&output));
    assert!(!scratch.path().join(".aevia").exists());

    let output = aevia(scratch.path(), &["check", "main.aev"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("ok"), "{}", stdout(&output));
    assert!(scratch.aevi().len() >= 3);

    // Editing the entry misses the whole-program record but not the modules.
    let before = scratch.aevi();
    scratch.write("main.aev", &format!("{MAIN}\n## edited\n"));
    let output = aevia(scratch.path(), &["check", "main.aev"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let after = scratch.aevi();
    let added: Vec<_> = after.iter().filter(|file| !before.contains(file)).collect();
    assert_eq!(added.len(), 1, "{added:?}");
}

#[test]
fn a_corrupt_file_is_a_silent_miss() {
    let scratch = Scratch::new("corrupt");
    project(&scratch);
    let cold = aevia(scratch.path(), &["run", "main.aev"]);
    assert_eq!(stdout(&cold), EXPECTED);
    let dir = scratch.path().join(".aevia").join("cache").join("modules");
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        std::fs::write(entry.path(), b"AEVI garbage").unwrap();
    }
    let output = aevia(scratch.path(), &["run", "main.aev"]);
    assert_eq!(stdout(&output), EXPECTED, "{}", stderr(&output));
    assert_eq!(stderr(&output), stderr(&cold));
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        assert_ne!(std::fs::read(entry.path()).unwrap(), b"AEVI garbage");
    }
}
