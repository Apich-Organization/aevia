//! Archive storage on disk.
//!
//! One directory per package, one file per version, plus the detached
//! signature. The layout is the identity: `{storage}/{owner}/{name}/{version}.tar.gz`.
//! That makes a backup a `tar` of one directory, and it means a package can be
//! inspected — or restored — without a database.
//!
//! The database remains the authority: a file on disk that no row mentions is
//! garbage, and a row whose file is missing is a hard error rather than a 404,
//! because it means the storage and the catalogue have diverged.

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
        Ok(Self { root })
    }

    /// The root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn archive_path(
        &self,
        id: &PackageId,
        version: &str,
    ) -> PathBuf {
        self.root
            .join(id.owner())
            .join(id.name())
            .join(format!("{version}.tar.gz"))
    }

    /// Writes an archive, replacing any file already at that version.
    ///
    /// # Errors
    ///
    /// `E_IO` when the file cannot be written.
    pub fn write(
        &self,
        id: &PackageId,
        version: &str,
        bytes: &[u8],
    ) -> Result<(), Error> {
        let path = self.archive_path(id, version);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                Error::io(format!("cannot create `{}`", parent.display()), &error)
            })?;
        }
        // Write to a temporary file in the same directory and rename it, so a
        // reader never sees a half-written archive.
        let temporary = path.with_extension("tar.gz.partial");
        std::fs::write(&temporary, bytes).map_err(|error| {
            Error::io(format!("cannot write `{}`", temporary.display()), &error)
        })?;
        std::fs::rename(&temporary, &path)
            .map_err(|error| Error::io(format!("cannot move `{}`", path.display()), &error))?;
        Ok(())
    }

    /// Reads an archive.
    ///
    /// # Errors
    ///
    /// `E_IO` when the file is missing or unreadable.
    pub fn read(
        &self,
        id: &PackageId,
        version: &str,
    ) -> Result<Vec<u8>, Error> {
        let path = self.archive_path(id, version);
        std::fs::read(&path).map_err(|error| {
            Error::new(
                E_IO,
                format!(
                    "the archive for {id} {version} is missing from storage (`{}`): {error}",
                    path.display()
                ),
            )
        })
    }

    /// Whether an archive exists.
    #[must_use]
    pub fn exists(
        &self,
        id: &PackageId,
        version: &str,
    ) -> bool {
        self.archive_path(id, version).is_file()
    }

    /// Removes an archive, if it is there.
    ///
    /// # Errors
    ///
    /// `E_IO` when the file exists but cannot be removed.
    pub fn remove(
        &self,
        id: &PackageId,
        version: &str,
    ) -> Result<(), Error> {
        let path = self.archive_path(id, version);
        match std::fs::remove_file(&path) {
            | Ok(()) => Ok(()),
            | Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            | Err(error) => Err(Error::io(
                format!("cannot remove `{}`", path.display()),
                &error,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archives_round_trip() {
        let directory = tempfile::tempdir().expect("tempdir");
        let storage = Storage::open(directory.path()).expect("open");
        let id: PackageId = "apich/units-extra".parse().expect("name");
        storage.write(&id, "0.1.0", b"bytes").expect("write");
        assert!(storage.exists(&id, "0.1.0"));
        assert_eq!(storage.read(&id, "0.1.0").expect("read"), b"bytes");
        storage.remove(&id, "0.1.0").expect("remove");
        assert!(!storage.exists(&id, "0.1.0"));
        // Removing twice is not an error.
        storage.remove(&id, "0.1.0").expect("remove again");
    }

    #[test]
    fn a_missing_archive_names_the_path() {
        let directory = tempfile::tempdir().expect("tempdir");
        let storage = Storage::open(directory.path()).expect("open");
        let id: PackageId = "apich/units-extra".parse().expect("name");
        let error = storage.read(&id, "9.9.9").expect_err("missing");
        assert!(
            error.message.contains("missing from storage"),
            "{}",
            error.message
        );
    }
}
