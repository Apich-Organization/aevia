//! The package archive: a deterministic `.tar.gz` and its SHA-256.
//!
//! # Determinism is the point
//!
//! The checksum is what publishers sign and what clients verify, so two packs
//! of the same tree must produce the same bytes on any machine: entries are
//! sorted, modification times are zero, owners are zero, modes are fixed, and
//! the gzip header carries no timestamp or file name. Without that, a rebuild
//! on another machine would produce a different checksum and every signature
//! would look stale.
//!
//! # What goes in
//!
//! The manifest, `src/`, `examples/`, and the top-level files that make a
//! package readable (README, LICENSE, CHANGELOG, NOTICE). Build output,
//! `.git`, `.aevia` caches and vendored trees never travel: an archive that
//! carries a `target/` directory is a way to smuggle binaries past review.

use std::collections::BTreeMap;
use std::io::Read;
use std::io::Write;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use sha2::Digest;

use crate::error::E_TARBALL;
use crate::error::Error;
use crate::error::Result;
use crate::manifest::PackageManifest;

/// The largest archive the registry accepts.
pub const MAX_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024;

/// The largest unpacked tree the registry will write.
pub const MAX_UNPACKED_BYTES: u64 = 64 * 1024 * 1024;

/// The most entries an archive may hold.
pub const MAX_ENTRIES: usize = 4096;

/// Directories that are never packaged, at any depth.
const SKIPPED_DIRECTORIES: &[&str] = &["target", ".git", ".aevia", "vendor", "node_modules"];

/// Directories that are packaged whole when they exist.
const INCLUDED_DIRECTORIES: &[&str] = &["src", "examples"];

/// Top-level files that are packaged when they exist.
const INCLUDED_FILES: &[&str] = &[
    "aevia.toml",
    "aevia-pkg.toml",
    "README.md",
    "README",
    "LICENSE",
    "LICENSE.md",
    "LICENSE.txt",
    "LICENCE",
    "COPYING",
    "CHANGELOG.md",
    "NOTICE",
];

/// Packs a package directory into a deterministic gzipped tarball.
///
/// # Errors
///
/// `E_TARBALL` when `aevia.toml` is missing from `dir`, when an entry is too
/// large, or when the archive cannot be written.
pub fn pack_dir(
    dir: &Path,
    manifest: &PackageManifest,
) -> Result<Vec<u8>> {
    let manifest_path = dir.join("aevia.toml");
    if !manifest_path.is_file() {
        return Err(Error::new(
            E_TARBALL,
            format!("`{}` has no `aevia.toml`", dir.display()),
        ));
    }
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    for name in INCLUDED_FILES {
        let path = dir.join(name);
        if path.is_file() {
            files.insert((*name).to_string(), path);
        }
    }
    for name in INCLUDED_DIRECTORIES {
        let root = dir.join(name);
        if root.is_dir() {
            collect(&root, name, &mut files)?;
        }
    }
    // The entry file must exist, or the package cannot run.
    let entry = manifest.entry_path(dir);
    if !entry.is_file() {
        return Err(Error::new(
            E_TARBALL,
            format!(
                "the entry file `{}` is missing; `[package] entry` names it",
                manifest.entry
            ),
        ));
    }
    let mut tarball = tar::Builder::new(Vec::new());
    tarball.mode(tar::HeaderMode::Deterministic);
    for (name, path) in &files {
        let bytes = std::fs::read(path)
            .map_err(|error| Error::io(format!("cannot read `{}`", path.display()), &error))?;
        let mut header = tar::Header::new_gnu();
        header.set_path(name).map_err(|error| {
            Error::new(E_TARBALL, format!("`{name}` cannot be a tar path: {error}"))
        })?;
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        tarball
            .append(&header, bytes.as_slice())
            .map_err(|error| Error::new(E_TARBALL, format!("cannot append `{name}`: {error}")))?;
    }
    let body = tarball
        .into_inner()
        .map_err(|error| Error::new(E_TARBALL, format!("cannot finish the tar: {error}")))?;
    let mut encoder = flate2::GzBuilder::new()
        .mtime(0)
        .operating_system(255)
        .write(Vec::new(), flate2::Compression::new(6));
    encoder
        .write_all(&body)
        .map_err(|error| Error::io("cannot compress the archive", &error))?;
    let archive = encoder
        .finish()
        .map_err(|error| Error::io("cannot compress the archive", &error))?;
    if archive.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(Error::new(
            E_TARBALL,
            format!(
                "the archive is {} bytes, over the {} byte limit",
                archive.len(),
                MAX_ARCHIVE_BYTES
            ),
        ));
    }
    Ok(archive)
}

fn collect(
    root: &Path,
    prefix: &str,
    files: &mut BTreeMap<String, PathBuf>,
) -> Result<()> {
    let entries = std::fs::read_dir(root)
        .map_err(|error| Error::io(format!("cannot read `{}`", root.display()), &error))?;
    let mut children: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|error| Error::io(format!("cannot read `{}`", root.display()), &error))?;
        children.push(entry.path());
    }
    children.sort();
    for path in children {
        let name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().to_string());
        let file_type = path
            .symlink_metadata()
            .map_err(|error| Error::io(format!("cannot stat `{}`", path.display()), &error))?
            .file_type();
        if file_type.is_dir() {
            if SKIPPED_DIRECTORIES.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            let child_prefix = format!("{prefix}/{name}");
            collect(&path, &child_prefix, files)?;
        } else if file_type.is_file() {
            files.insert(format!("{prefix}/{name}"), path);
        }
        // A symlink is silently dropped: a package that needs one is a package
        // whose contents would depend on the publishing machine.
    }
    Ok(())
}

/// `sha256:<hex>` of an archive.
#[must_use]
pub fn checksum(bytes: &[u8]) -> String {
    let digest = sha2::Sha256::digest(bytes);
    format!("sha256:{}", hex::encode(digest))
}

/// Checks an archive against an expected `sha256:<hex>`.
///
/// # Errors
///
/// `E_TARBALL` with both digests when they differ.
pub fn verify_checksum(
    bytes: &[u8],
    expected: &str,
) -> Result<()> {
    let actual = checksum(bytes);
    if !actual.eq_ignore_ascii_case(expected.trim()) {
        return Err(Error::new(
            E_TARBALL,
            format!("the archive is {actual}, but {expected} was signed"),
        ));
    }
    Ok(())
}

/// The entry names of an archive, in order.
///
/// # Errors
///
/// `E_TARBALL` when the archive does not decompress or holds an unsafe path.
pub fn entries(bytes: &[u8]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in walk(bytes)? {
        out.push(entry.0);
    }
    Ok(out)
}

/// The UTF-8 text files of an archive, by entry name.
///
/// The documentation builder reads this instead of unpacking to disk: a page is
/// rendered from memory, so a malformed archive never touches the filesystem.
///
/// # Errors
///
/// `E_TARBALL` when the archive is malformed or holds an unsafe path.
pub fn text_files(bytes: &[u8]) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for (name, data) in walk(bytes)? {
        if (name.ends_with(".aev") || name.ends_with(".toml") || name.ends_with(".md"))
            && let Ok(text) = String::from_utf8(data) {
                out.push((name, text));
            }
    }
    Ok(out)
}

/// Unpacks an archive into `dest`, refusing anything that escapes it.
///
/// # Errors
///
/// `E_TARBALL` for a traversal path, an absolute path, a symlink, a device, too
/// many entries or too many bytes.
pub fn unpack(
    bytes: &[u8],
    dest: &Path,
) -> Result<Vec<String>> {
    let mut written = Vec::new();
    for (name, data) in walk(bytes)? {
        let relative = safe_relative(&name)?;
        let target = dest.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                Error::io(format!("cannot create `{}`", parent.display()), &error)
            })?;
        }
        std::fs::write(&target, &data)
            .map_err(|error| Error::io(format!("cannot write `{}`", target.display()), &error))?;
        written.push(name);
    }
    Ok(written)
}

/// Reads every regular file of an archive into memory, in order.
fn walk(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(Error::new(
            E_TARBALL,
            format!("the archive is over the {MAX_ARCHIVE_BYTES} byte limit"),
        ));
    }
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|error| Error::new(E_TARBALL, format!("the archive is not a tar.gz: {error}")))?;
    let mut out = Vec::new();
    let mut total = 0u64;
    for entry in entries {
        let mut entry = entry
            .map_err(|error| Error::new(E_TARBALL, format!("cannot read an entry: {error}")))?;
        if out.len() >= MAX_ENTRIES {
            return Err(Error::new(
                E_TARBALL,
                format!("the archive holds more than {MAX_ENTRIES} entries"),
            ));
        }
        let kind = entry.header().entry_type();
        let path = entry
            .path()
            .map_err(|error| Error::new(E_TARBALL, format!("cannot read a path: {error}")))?
            .to_string_lossy()
            .replace('\\', "/");
        match kind {
            | tar::EntryType::Regular | tar::EntryType::Continuous => {},
            | tar::EntryType::Directory => continue,
            | other => {
                return Err(Error::new(
                    E_TARBALL,
                    format!("`{path}` is a {other:?} entry; a package holds regular files only"),
                ));
            },
        }
        safe_relative(&path)?;
        total += entry.size();
        if total > MAX_UNPACKED_BYTES {
            return Err(Error::new(
                E_TARBALL,
                format!("the archive unpacks to more than {MAX_UNPACKED_BYTES} bytes"),
            ));
        }
        let mut data = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
        entry
            .read_to_end(&mut data)
            .map_err(|error| Error::new(E_TARBALL, format!("cannot read `{path}`: {error}")))?;
        out.push((path, data));
    }
    if out.is_empty() {
        return Err(Error::new(E_TARBALL, "the archive is empty"));
    }
    Ok(out)
}

/// Rejects absolute paths, `..` and Windows drive prefixes.
fn safe_relative(name: &str) -> Result<PathBuf> {
    if name.is_empty() || name.starts_with('/') || name.contains('\0') {
        return Err(Error::new(
            E_TARBALL,
            format!("`{name}` is not a relative path"),
        ));
    }
    if name.len() > 1 && name.as_bytes()[1] == b':' {
        return Err(Error::new(
            E_TARBALL,
            format!("`{name}` is an absolute Windows path"),
        ));
    }
    let mut out = PathBuf::new();
    for component in Path::new(name).components() {
        match component {
            | Component::Normal(part) => out.push(part),
            | Component::CurDir => {},
            | _ => {
                return Err(Error::new(
                    E_TARBALL,
                    format!("`{name}` escapes the package directory"),
                ));
            },
        }
    }
    if out.as_os_str().is_empty() {
        return Err(Error::new(E_TARBALL, format!("`{name}` names no file")));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("src")).expect("src");
        std::fs::write(
            dir.path().join("aevia.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .expect("manifest");
        std::fs::write(
            dir.path().join("src/main.aev"),
            "## The demo entry.\nfn main(): IO Unit = do\n    println(\"hi\")\n",
        )
        .expect("entry");
        std::fs::write(dir.path().join("README.md"), "# demo\n\nA demo package.\n")
            .expect("readme");
        // Build output must never travel.
        std::fs::create_dir_all(dir.path().join("src/target")).expect("target");
        std::fs::write(dir.path().join("src/target/junk.bin"), b"\0\0\0").expect("junk");
        dir
    }

    #[test]
    fn packing_is_byte_identical_across_runs() {
        let dir = package();
        let manifest = PackageManifest::load(dir.path()).expect("manifest");
        let first = pack_dir(dir.path(), &manifest).expect("pack");
        let second = pack_dir(dir.path(), &manifest).expect("pack");
        assert_eq!(checksum(&first), checksum(&second));
        assert_eq!(first, second);
    }

    #[test]
    fn the_archive_holds_the_source_and_not_the_build_output() {
        let dir = package();
        let manifest = PackageManifest::load(dir.path()).expect("manifest");
        let archive = pack_dir(dir.path(), &manifest).expect("pack");
        let names = entries(&archive).expect("entries");
        assert!(names.contains(&"aevia.toml".to_string()), "{names:?}");
        assert!(names.contains(&"src/main.aev".to_string()), "{names:?}");
        assert!(names.contains(&"README.md".to_string()), "{names:?}");
        assert!(!names.iter().any(|name| name.contains("junk")), "{names:?}");
    }

    #[test]
    fn unpacking_round_trips() {
        let dir = package();
        let manifest = PackageManifest::load(dir.path()).expect("manifest");
        let archive = pack_dir(dir.path(), &manifest).expect("pack");
        let out = tempfile::tempdir().expect("tempdir");
        let written = unpack(&archive, out.path()).expect("unpack");
        assert!(written.contains(&"src/main.aev".to_string()));
        let text = std::fs::read_to_string(out.path().join("src/main.aev")).expect("read");
        assert!(text.contains("The demo entry."));
    }

    #[test]
    fn traversal_and_absolute_paths_are_refused() {
        // The tar writer refuses to *build* a `..` entry, so the check is
        // exercised where it lives: the path sanitiser every entry passes
        // through before it touches the filesystem.
        for name in [
            "../escape",
            "/etc/passwd",
            "C:/Windows/system32",
            "a/../../b",
            "",
        ] {
            let error = safe_relative(name).expect_err(name);
            assert_eq!(error.code, E_TARBALL, "{name}");
        }
        assert!(safe_relative("src/main.aev").is_ok());
        assert!(safe_relative("./src/main.aev").is_ok());
    }

    #[test]
    fn a_symlink_entry_is_refused() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_path("link").expect("path");
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_link_name("/etc/passwd").expect("link");
        header.set_cksum();
        builder.append(&header, std::io::empty()).expect("append");
        let tar_bytes = builder.into_inner().expect("tar");
        let mut encoder = flate2::GzBuilder::new().write(Vec::new(), flate2::Compression::new(6));
        encoder.write_all(&tar_bytes).expect("gzip");
        let archive = encoder.finish().expect("gzip");
        let error = unpack(&archive, tempfile::tempdir().expect("t").path()).expect_err("refused");
        assert!(error.message.contains("Symlink"), "{}", error.message);
    }

    #[test]
    fn a_checksum_mismatch_names_both_digests() {
        let error = verify_checksum(b"abc", "sha256:00").expect_err("mismatch");
        assert!(error.message.contains("sha256:"), "{}", error.message);
    }
}
