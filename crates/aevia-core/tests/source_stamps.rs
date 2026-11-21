//! Source dependencies must round-trip through a line-oriented check cache.

use aevia_core::driver;
use aevia_core::source::stamp_is_current;
use std::path::Path;
use std::path::PathBuf;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("aevia-stamp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(
        &self,
        path: impl AsRef<Path>,
        source: &str,
    ) -> PathBuf {
        let path = self.0.join(path);
        std::fs::write(&path, source).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const SOURCE: &str = "module Main\nlet value = 1\n";

#[test]
fn plain_utf8_path_records_keep_their_legacy_format() {
    let scratch = Scratch::new("legacy");
    let path = scratch.write("Unicode β with %09.aev", SOURCE);
    let program = driver::load(&path);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    let dependency = program
        .dependencies
        .iter()
        .find(|item| item.path() == path)
        .unwrap();
    let stamp = dependency.stamp();
    let prefix = format!(
        "{}\t{}\t1\t",
        path.display(),
        dependency.physical_path().display()
    );
    let hash = stamp.strip_prefix(&prefix).unwrap();
    assert_eq!(hash.len(), 16);
    assert!(stamp_is_current(&stamp));
    assert!(!stamp_is_current(&format!("{stamp}\textra")));
    std::fs::write(path, SOURCE.replace('1', "2")).unwrap();
    assert!(!stamp_is_current(&stamp));
}

#[test]
fn versioned_native_path_records_validate_paths_and_content() {
    use std::fmt::Write as _;

    let encode = |path: &Path| {
        let mut field = String::new();
        #[cfg(windows)]
        let bytes = {
            use std::os::windows::ffi::OsStrExt as _;
            path.as_os_str().encode_wide().flat_map(u16::to_le_bytes)
        };
        #[cfg(not(windows))]
        let bytes = path.as_os_str().as_encoded_bytes().iter().copied();
        for byte in bytes {
            write!(field, "{byte:02x}").unwrap();
        }
        field
    };
    let scratch = Scratch::new("versioned");
    let path = scratch.write("Data β 𝛑.aev", SOURCE);
    let program = driver::load(&path);
    assert!(!program.diagnostics.has_errors());
    let dependency = program
        .dependencies
        .iter()
        .find(|item| item.path() == path)
        .unwrap();
    let plain = dependency.stamp();
    let (_, hash) = plain.rsplit_once('\t').unwrap();
    let logical = encode(&path);
    let physical = encode(dependency.physical_path());
    let record = |logical: &str| format!("path-v2\t{logical}\t{physical}\t1\t{hash}");
    assert!(stamp_is_current(&record(&logical)));
    assert!(!stamp_is_current(&record(&format!("{logical}g0"))));
    assert!(!stamp_is_current(&record(&format!("{logical}a"))));
    #[cfg(windows)]
    assert!(!stamp_is_current(&record(&format!("{logical}ab"))));
    std::fs::write(path, SOURCE.replace('1', "2")).unwrap();
    assert!(!stamp_is_current(&record(&logical)));
}

#[cfg(unix)]
#[test]
fn control_characters_and_non_utf8_paths_round_trip_without_aliasing() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    let scratch = Scratch::new("raw");
    for filename in [
        b"tab\tentry.aev".as_slice(),
        b"newline\nentry.aev",
        b"mixed\r\n\tentry.aev",
        b"nonutf8-\xff-entry.aev",
    ] {
        let path = scratch.write(OsStr::from_bytes(filename), SOURCE);
        let program = driver::load(&path);
        assert!(
            !program.diagnostics.has_errors(),
            "{:?}",
            program.diagnostics.items
        );
        let mut records = String::new();
        for dependency in &program.dependencies {
            records.push_str("dep ");
            records.push_str(&dependency.stamp());
            records.push('\n');
        }
        assert_eq!(
            records.lines().count(),
            program.dependencies.len(),
            "{filename:?}"
        );
        let current = || {
            records
                .lines()
                .all(|line| stamp_is_current(line.strip_prefix("dep ").unwrap()))
        };
        assert!(current(), "{filename:?}");
        if path.to_str().is_none() {
            let lossy_alias = PathBuf::from(path.to_string_lossy().as_ref());
            std::fs::write(lossy_alias, SOURCE.replace('1', "2")).unwrap();
            assert!(current(), "a different lossy alias is not consulted");
        }
        std::fs::write(path, SOURCE.replace('1', "2")).unwrap();
        assert!(!current(), "a same-length edit invalidates {filename:?}");
    }
}

#[cfg(unix)]
#[test]
fn encoded_candidates_notice_creation_and_same_content_symlink_retargets() {
    use std::os::unix::fs::symlink;

    let scratch = Scratch::new("changes\tline");
    const HELPER: &str = "module Helper\nlet value = 1\n";
    let entry = scratch.write(
        "main.aev",
        "import Helper\nimport Missing\nlet value = Helper.value\n",
    );
    let first = scratch.write("target\nfirst.aev", HELPER);
    let second = scratch.write("target\nsecond.aev", HELPER);
    let alias = scratch.0.join("Helper.aev");
    symlink(first, &alias).unwrap();
    let program = driver::load(&entry);
    let find = |path: &Path| {
        program
            .dependencies
            .iter()
            .find(|item| item.path() == path)
            .unwrap()
            .stamp()
    };
    let loaded = find(&alias);
    let missing_path = scratch.0.join("Missing.aev");
    let missing = find(&missing_path);
    assert!(stamp_is_current(&loaded));
    assert!(stamp_is_current(&missing));
    std::fs::remove_file(&alias).unwrap();
    symlink(second, &alias).unwrap();
    assert!(!stamp_is_current(&loaded));
    assert!(stamp_is_current(&missing));
    std::fs::write(&missing_path, "module Missing\nlet value = 1\n").unwrap();
    assert!(!stamp_is_current(&missing));
}

#[cfg(unix)]
#[test]
fn an_encoded_read_failure_becomes_stale_when_the_file_can_be_read() {
    let scratch = Scratch::new("read-failure");
    let path = scratch.0.join("broken\tentry.aev");
    std::fs::write(&path, [0xff]).unwrap();
    let program = driver::load(&path);
    assert!(program.diagnostics.has_errors());
    let dependency = program
        .dependencies
        .iter()
        .find(|item| item.path() == path)
        .unwrap();
    let stamp = dependency.stamp();
    assert!(stamp.ends_with("\t!InvalidData"));
    assert!(stamp_is_current(&stamp));
    std::fs::write(path, SOURCE).unwrap();
    assert!(!stamp_is_current(&stamp));
}

#[cfg(unix)]
#[test]
fn a_symlink_retarget_between_non_utf8_names_with_the_same_display_is_stale() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::symlink;

    let scratch = Scratch::new("raw-symlink");
    let first = scratch.write(OsStr::from_bytes(b"target-\xff.aev"), SOURCE);
    let second = scratch.write(OsStr::from_bytes(b"target-\xfe.aev"), SOURCE);
    assert_eq!(first.to_string_lossy(), second.to_string_lossy());
    let alias = scratch.0.join("main.aev");
    symlink(first, &alias).unwrap();
    let program = driver::load(&alias);
    assert!(!program.diagnostics.has_errors());
    let dependency = program
        .dependencies
        .iter()
        .find(|item| item.path() == alias)
        .unwrap();
    let stamp = dependency.stamp();
    assert!(stamp_is_current(&stamp));
    std::fs::remove_file(&alias).unwrap();
    symlink(second, &alias).unwrap();
    assert!(!stamp_is_current(&stamp));
}
