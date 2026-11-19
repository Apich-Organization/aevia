//! Archive storage on disk.
//!
//! The archive key is its SHA-256 checksum, recorded with the signed version.
//! Legacy stores with `{owner}/{name}/{version}.tar.gz` are read in place.
//!
//! The database remains the authority: a file on disk that no row mentions is
//! garbage, and a row whose file is missing is a hard error rather than a 404,
//! because it means the storage and the catalogue have diverged.

use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;

use aevia_registry_core::Error;
use aevia_registry_core::PackageId;

/// `E_IO` for storage failures.
pub const E_IO: &str = "E_IO";

/// The archive store.
#[derive(Clone, Debug)]
pub struct Storage {
    root: PathBuf,
}

impl Storage {
    /// Opens the store, creating the root when it is missing.
    ///
    /// # Errors
    ///
    /// `E_IO` when the directory cannot be created.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, Error> {
        let root = root.into();
        std::fs::create_dir_all(&root)
            .map_err(|error| Error::io(format!("cannot create `{}`", root.display()), &error))?;
        let objects = root.join("sha256");
        std::fs::create_dir_all(&objects)
            .map_err(|error| Error::io(format!("cannot create `{}`", objects.display()), &error))?;
        sync_directory(&root)?;
        Ok(Self { root })
    }

    /// The root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn legacy_path(
        &self,
        id: &PackageId,
        version: &str,
    ) -> PathBuf {
        self.root
            .join(id.owner())
            .join(id.name())
            .join(format!("{version}.tar.gz"))
    }

    fn archive_path(
        &self,
        checksum: &str,
    ) -> Result<PathBuf, Error> {
        let digest = checksum
            .strip_prefix("sha256:")
            .filter(|digest| {
                digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            .ok_or_else(|| Error::new(E_IO, "the archive has an invalid SHA-256 digest"))?;
        Ok(self
            .root
            .join("sha256")
            .join(format!("{}.tar.gz", digest.to_ascii_lowercase())))
    }

    /// Publishes a durable archive object under its computed checksum.
    ///
    /// # Errors
    ///
    /// `E_IO` when the file cannot be written.
    pub fn write(
        &self,
        checksum: &str,
        bytes: &[u8],
    ) -> Result<(), Error> {
        let path = self.archive_path(checksum)?;
        let directory = self.root.join("sha256");
        let temporary = directory.join(format!(".{}.partial", crate::http::random_hex(16)));
        let install = || -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, &path)
        };
        if let Err(error) = install() {
            let _ = std::fs::remove_file(&temporary);
            return Err(Error::io(
                format!("cannot install `{}`", path.display()),
                &error,
            ));
        }
        sync_directory(&directory)?;
        Ok(())
    }

    /// Reads and verifies an archive against the signed catalogue digest.
    ///
    /// # Errors
    ///
    /// `E_IO` when the file is missing, `E_INTEGRITY` when its digest differs.
    pub fn read(
        &self,
        id: &PackageId,
        version: &str,
        checksum: &str,
    ) -> Result<Vec<u8>, Error> {
        let path = self.archive_path(checksum)?;
        let bytes = std::fs::read(&path)
            .or_else(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    std::fs::read(self.legacy_path(id, version))
                } else {
                    Err(error)
                }
            })
            .map_err(|error| {
                Error::new(
                    E_IO,
                    format!(
                        "the archive for {id} {version} is missing from storage (`{}`): {error}",
                        path.display()
                    ),
                )
            })?;
        let actual = aevia_registry_core::tarball::checksum(&bytes);
        if !actual.eq_ignore_ascii_case(checksum) {
            return Err(Error::new(
                "E_INTEGRITY",
                format!(
                    "the stored archive of {id} {version} hashes to {actual}, expected {checksum}"
                ),
            ));
        }
        Ok(bytes)
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), Error> {
    std::fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| Error::io(format!("cannot sync `{}`", path.display()), &error))
}

#[cfg(not(unix))]
const fn sync_directory(_path: &Path) -> Result<(), Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archives_round_trip() {
        let directory = tempfile::tempdir().expect("tempdir");
        let storage = Storage::open(directory.path()).expect("open");
        let id: PackageId = "apich/units-extra".parse().expect("name");
        let hash = aevia_registry_core::tarball::checksum(b"bytes");
        storage.write(&hash, b"bytes").expect("write");
        assert_eq!(storage.read(&id, "0.1.0", &hash).expect("read"), b"bytes");
    }

    #[test]
    fn legacy_archives_are_verified_on_read() {
        let directory = tempfile::tempdir().expect("tempdir");
        let storage = Storage::open(directory.path()).expect("open");
        let id: PackageId = "apich/legacy".parse().expect("name");
        let path = storage.legacy_path(&id, "0.1.0");
        std::fs::create_dir_all(path.parent().expect("archive parent")).expect("parent exists");
        std::fs::write(&path, b"bytes").expect("write");
        let hash = aevia_registry_core::tarball::checksum(b"bytes");
        assert_eq!(storage.read(&id, "0.1.0", &hash).expect("read"), b"bytes");
        std::fs::write(&path, b"corrupt").expect("alter file");
        assert_eq!(
            storage.read(&id, "0.1.0", &hash).expect_err("corrupt").code,
            "E_INTEGRITY"
        );
    }

    #[test]
    fn a_missing_archive_names_the_path() {
        let directory = tempfile::tempdir().expect("tempdir");
        let storage = Storage::open(directory.path()).expect("open");
        let id: PackageId = "apich/units-extra".parse().expect("name");
        let hash = aevia_registry_core::tarball::checksum(b"bytes");
        let error = storage.read(&id, "9.9.9", &hash).expect_err("missing");
        assert!(
            error.message.contains("missing from storage"),
            "{}",
            error.message
        );
    }
}
