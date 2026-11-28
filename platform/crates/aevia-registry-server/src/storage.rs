//! Archive storage on disk.
//!
//! The archive key is its SHA-256 checksum, recorded with the signed version.
//! Legacy stores with `{owner}/{name}/{version}.tar.gz` are read in place.
//!
//! The database remains the authority: a file on disk that no row mentions is
//! garbage, and a row whose file is missing is a hard error rather than a 404,
//! because it means the storage and the catalogue have diverged.

use std::fs::File;
use std::io;
use std::io::Read as _;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;

use sha2::Digest as _;
use sha2::Sha256;

use aevia_registry_core::Error;
use aevia_registry_core::PackageId;
use aevia_registry_core::tarball;

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
    ) -> io::Result<PathBuf> {
        if version.contains(['/', '\\', ':']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the version is not a valid archive filename",
            ));
        }
        Ok(self
            .root
            .join(id.owner())
            .join(id.name())
            .join(format!("{version}.tar.gz")))
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
        self.read_archive(id, version, checksum, |file, size| {
            let capacity = usize::try_from(size.saturating_add(1))
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let mut bytes = Vec::with_capacity(capacity);
            file.read_to_end(&mut bytes)?;
            let actual = tarball::checksum(&bytes);
            Ok((bytes, actual))
        })
        .map(|(bytes, _)| bytes)
    }

    /// Checks the signed digest and measures the archive with a fixed buffer.
    #[allow(
        clippy::single_call_fn,
        reason = "HEAD does not retain the verified bytes"
    )]
    pub(crate) fn verified_size(
        &self,
        id: &PackageId,
        version: &str,
        checksum: &str,
    ) -> Result<u64, Error> {
        self.read_archive(id, version, checksum, |file, _| {
            let mut digest = Sha256::new();
            io::copy(file, &mut digest)?;
            Ok(((), format!("sha256:{}", hex::encode(digest.finalize()))))
        })
        .map(|((), size)| size)
    }

    fn read_archive<T>(
        &self,
        id: &PackageId,
        version: &str,
        checksum: &str,
        read: impl FnOnce(&mut io::Take<File>, u64) -> io::Result<(T, String)>,
    ) -> Result<(T, u64), Error> {
        let path = self.archive_path(checksum)?;
        let missing = |error: io::Error| {
            Error::new(
                E_IO,
                format!(
                    "the archive for {id} {version} is missing from storage (`{}`): {error}",
                    path.display()
                ),
            )
        };
        let (file, size) = open_regular_archive(&path)
            .or_else(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    open_regular_archive(&self.legacy_path(id, version)?)
                } else {
                    Err(error)
                }
            })
            .map_err(missing)?;
        if size > tarball::MAX_ARCHIVE_BYTES {
            return Err(Error::new(
                "E_INTEGRITY",
                format!(
                    "the stored archive of {id} {version} is {size} bytes, above the {} byte limit",
                    tarball::MAX_ARCHIVE_BYTES,
                ),
            ));
        }
        // The extra byte distinguishes an unchanged file from one that grew
        // while it was being read, without allocating for the longer file.
        let limit = size.saturating_add(1);
        let mut file = file.take(limit);
        let (value, actual) = read(&mut file, size).map_err(missing)?;
        if file.limit() != 1 {
            return Err(Error::new(
                "E_INTEGRITY",
                format!(
                    "the stored archive of {id} {version} changed size while reading from storage"
                ),
            ));
        }
        if !actual.eq_ignore_ascii_case(checksum) {
            return Err(Error::new(
                "E_INTEGRITY",
                format!(
                    "the stored archive of {id} {version} hashes to {actual}, expected {checksum}"
                ),
            ));
        }
        Ok((value, size))
    }
}

fn open_regular_archive(path: &Path) -> io::Result<(File, u64)> {
    // Check the type before opening, as opening a FIFO can block without a writer.
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the archive is not a regular file",
        ));
    }
    Ok((File::open(path)?, metadata.len()))
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
        assert_eq!(storage.verified_size(&id, "0.1.0", &hash).expect("size"), 5);
    }

    #[test]
    fn legacy_archives_are_verified_on_read() {
        let directory = tempfile::tempdir().expect("tempdir");
        let storage = Storage::open(directory.path()).expect("open");
        let id: PackageId = "apich/legacy".parse().expect("name");
        let path = storage.legacy_path(&id, "0.1.0").expect("archive path");
        std::fs::create_dir_all(path.parent().expect("archive parent")).expect("parent exists");
        std::fs::write(&path, b"bytes").expect("write");
        let hash = aevia_registry_core::tarball::checksum(b"bytes");
        assert_eq!(storage.read(&id, "0.1.0", &hash).expect("read"), b"bytes");
        assert_eq!(storage.verified_size(&id, "0.1.0", &hash).expect("size"), 5);
        std::fs::write(&path, b"corrupt").expect("alter file");
        assert_eq!(
            storage.read(&id, "0.1.0", &hash).expect_err("corrupt").code,
            "E_INTEGRITY"
        );
        assert_eq!(
            storage
                .verified_size(&id, "0.1.0", &hash)
                .expect_err("corrupt")
                .code,
            "E_INTEGRITY"
        );
    }

    #[test]
    fn archive_reads_enforce_the_published_size_limit() {
        let directory = tempfile::tempdir().expect("tempdir");
        let storage = Storage::open(directory.path()).expect("open");
        let id: PackageId = "apich/legacy".parse().expect("name");
        let hash = tarball::checksum(b"fixture");
        File::create(storage.archive_path(&hash).expect("archive path"))
            .expect("fixture file")
            .set_len(tarball::MAX_ARCHIVE_BYTES.saturating_add(1))
            .expect("oversized file");
        for error in [
            storage.read(&id, "0.1.0", &hash).expect_err("oversized"),
            storage
                .verified_size(&id, "0.1.0", &hash)
                .expect_err("oversized"),
        ] {
            assert_eq!(error.code, "E_INTEGRITY");
            assert!(error.message.contains("byte limit"));
        }
    }

    #[test]
    fn archive_reads_reject_growth_at_the_measured_boundary() {
        let directory = tempfile::tempdir().expect("tempdir");
        let storage = Storage::open(directory.path()).expect("open");
        let id: PackageId = "apich/fixture".parse().expect("name");
        let hash = tarball::checksum(b"bytes");
        storage.write(&hash, b"bytes").expect("write");
        let path = storage.archive_path(&hash).expect("archive path");
        let error = storage
            .read_archive(&id, "0.1.0", &hash, |reader, _| {
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(&path)?
                    .write_all(b" appended")?;
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes)?;
                assert_eq!(bytes, b"bytes ");
                Ok(((), tarball::checksum(&bytes)))
            })
            .expect_err("the file grew after its metadata was read");
        assert_eq!(error.code, "E_INTEGRITY");
        assert!(error.message.contains("changed size"));
    }

    #[test]
    fn legacy_versions_are_single_filename_components() {
        let directory = tempfile::tempdir().expect("tempdir");
        let storage = Storage::open(directory.path()).expect("open");
        let id: PackageId = "apich/legacy".parse().expect("name");
        for version in ["old/1.0.0", "old\\1.0.0", "old:1.0.0"] {
            assert_eq!(
                storage
                    .legacy_path(&id, version)
                    .expect_err("invalid filename")
                    .kind(),
                io::ErrorKind::InvalidInput
            );
        }
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
