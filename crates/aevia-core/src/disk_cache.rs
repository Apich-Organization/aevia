//! The per-module on-disk checked-module cache, `.aevi` files (spec §7.3, §9.11).
//!
//! One file holds one checked module: its diagnostics, its complete
//! [`CheckedModule`] (everything the runtime and the editor read, not just the
//! interface) and the digest of its interface. The body is written by
//! [`aevia_types::persist`], which keeps every process-local `AdtId` out of
//! the file and re-interns the types on load.
//!
//! ```text
//! offset  size  field
//!      0     4  magic "AEVI"
//!      4     4  FORMAT_VERSION, little endian
//!      8    16  cache key (the same 128 bits that are in the file name)
//!     24     8  body length
//!     32     8  body checksum (FNV-1a 64)
//!     40     …  body: persist::encode(Entry)
//! ```
//!
//! The key is a stable (process- and build-independent) 128-bit hash of every
//! input of the check: the module's source text and `FileId`, its identity and
//! import bindings, the *digest of the interface of every module in its
//! dependency closure* (so an edit that leaves a dependency's interface alone
//! is an early cutoff), the capability set and the compiler identity. A
//! missing, truncated, corrupt, foreign or old-format file is a silent miss.

#![cfg_attr(
    not(test),
    deny(
        missing_docs,
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::nursery,
        clippy::panic,
        clippy::pedantic,
        clippy::single_call_fn,
        clippy::unwrap_used
    )
)]

use aevia_syntax::Diagnostics;
use aevia_syntax::FileId;
use aevia_syntax::Span;
use aevia_types::check::CheckOptions;
use aevia_types::check::CheckedModule;
use aevia_types::check::ModuleInterface;
use aevia_types::persist;
use std::fmt::Write as _;
use std::io::ErrorKind;
use std::io::Read as _;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

/// Version of the `.aevi` layout and key inputs.
pub const FORMAT_VERSION: u32 = 4;

const MAGIC: &[u8; 4] = b"AEVI";
const HEADER_LEN: usize = 40;
const SMALL_ENTRY_LEN: usize = 4096;

/// How many `.aevi` files of one module name are kept (older ones are stale
/// revisions of the same module and are pruned after a write).
const KEEP_PER_MODULE: usize = 4;

/// A 128-bit stable digest.
pub type Digest = [u8; 16];

/// A stable (not `DefaultHasher`) 128-bit hash: two independent FNV-1a lanes.
#[derive(Clone)]
pub struct StableHasher {
    a: u64,
    b: u64,
}

impl Default for StableHasher {
    fn default() -> Self {
        Self {
            a: 0xcbf2_9ce4_8422_2325,
            b: 0x8422_2325_cbf2_9ce4 ^ 0x9e37_79b9_7f4a_7c15,
        }
    }
}

impl StableHasher {
    /// A fresh hasher.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds raw bytes.
    pub fn bytes(
        &mut self,
        data: &[u8],
    ) {
        for &byte in data {
            self.a = (self.a ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
            self.b = (self.b ^ u64::from(byte))
                .wrapping_mul(0x9e37_79b1_85eb_ca87)
                .rotate_left(29);
        }
    }

    /// Feeds one length-framed field, so field boundaries are unambiguous.
    pub fn field(
        &mut self,
        data: &[u8],
    ) {
        self.bytes(&(data.len() as u64).to_le_bytes());
        self.bytes(data);
    }

    /// The digest.
    #[must_use]
    pub fn finish(&self) -> Digest {
        let mut out = [0u8; 16];
        for (dest, byte) in out
            .iter_mut()
            .zip(self.a.to_le_bytes().into_iter().chain(self.b.to_le_bytes()))
        {
            *dest = byte;
        }
        out
    }
}

fn fnv64(data: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &byte in data {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// What identifies the compiler that wrote an entry: the checker's behaviour
/// must be a function of this string. The executable's size and modification
/// time make a rebuilt checker invalidate entries even at an unchanged version.
#[allow(
    clippy::single_call_fn,
    reason = "keeps process identity generation and caching together"
)]
fn compiler_identity() -> &'static str {
    static IDENTITY: OnceLock<String> = OnceLock::new();
    IDENTITY.get_or_init(|| {
        let exe = std::env::current_exe()
            .and_then(std::fs::metadata)
            .map_or_else(
                |_| String::new(),
                |meta| format!("{}|{:?}", meta.len(), meta.modified().ok()),
            );
        format!(
            "aevia-core {}|aevi-format {FORMAT_VERSION}|codec {}|rssn {}|exe {exe}",
            env!("CARGO_PKG_VERSION"),
            persist::codec::CODEC_ID,
            aevia_rssn::registry().manifest_hash
        )
    })
}

/// The counters of one load, shared with the caller that wants them.
#[derive(Debug, Default)]
pub struct Counters {
    /// Modules the checker actually ran on.
    pub checked: AtomicUsize,
    /// Modules restored from `.aevi` files.
    pub disk_hits: AtomicUsize,
    /// `.aevi` files written.
    pub disk_writes: AtomicUsize,
}

impl Counters {
    pub(crate) fn bump(counter: &AtomicUsize) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn get(counter: &AtomicUsize) -> usize {
        counter.load(Ordering::Relaxed)
    }
}

/// A directory of `.aevi` files.
#[derive(Clone, Debug)]
pub struct DiskCache {
    dir: Arc<PathBuf>,
}

/// The inputs a key is computed from, other than the interface digests.
pub struct KeyInputs<'a> {
    /// The module's source text.
    pub text: &'a str,
    /// Its `FileId` in this load.
    pub file: FileId,
    /// Its module path as written (or the file stem).
    pub path: &'a [String],
    /// The nominal identity the loader supplied.
    pub identity: &'a Option<aevia_types::ModulePath>,
    /// The import bindings (module, prefix, names).
    pub bindings: &'a [aevia_types::check::InterfaceBinding],
    /// Resolved re-export origins, in declaration order.
    pub reexports: &'a [(Span, aevia_types::ModulePath)],
    /// The checker options (capability set).
    pub options: &'a CheckOptions,
    /// The interface digests of its dependency closure, in checker order.
    pub interfaces: &'a [Digest],
}

/// The cache key of one module and the file it lives in.
#[derive(Clone, Debug)]
pub struct Key {
    digest: Digest,
    file_name: String,
    prefix: String,
}

#[derive(bincode_next::Encode)]
struct EntryRef<'a> {
    file: FileId,
    interface: Digest,
    diagnostics: &'a Diagnostics,
    module: &'a CheckedModule,
}

impl<C: bincode_next::config::Config> bincode_next::Fingerprint<C> for EntryRef<'_> {
    const SCHEMA_HASH: u64 = <Entry as bincode_next::Fingerprint<C>>::SCHEMA_HASH;
}

#[derive(bincode_next::Decode, bincode_next::Fingerprint)]
struct Entry {
    file: FileId,
    interface: Digest,
    diagnostics: Diagnostics,
    module: CheckedModule,
}

/// A module restored from disk.
pub struct Hit {
    /// Its diagnostics (warnings; errors are never stored).
    pub diagnostics: Diagnostics,
    /// The checked module, with `AdtId`s of this process.
    pub module: CheckedModule,
    /// Its interface digest.
    pub interface: Digest,
}

#[allow(
    clippy::single_call_fn,
    reason = "separates portable filenames from cache key hashing"
)]
fn sanitize(path: &[String]) -> String {
    let joined = if path.is_empty() {
        "entry".to_string()
    } else {
        path.join(".")
    };
    joined
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The digest of a module's interface: its encoding with every `AdtId`
/// replaced by the type's symbolic definition, hashed. `None` when the
/// interface cannot be encoded (it is then simply not cacheable).
#[must_use]
pub fn interface_digest(interface: &ModuleInterface) -> Option<Digest> {
    let bytes = persist::encode(interface).ok()?;
    let mut hasher = StableHasher::new();
    hasher.field(&bytes);
    Some(hasher.finish())
}

fn bincode_bytes<T: bincode_next::Encode + ?Sized>(value: &T) -> Option<Vec<u8>> {
    bincode_next::encode_to_vec(
        value,
        bincode_next::config::standard().with_deterministic_bincode(),
    )
    .ok()
}

impl DiskCache {
    /// A cache stored in `dir` (created on the first write).
    #[must_use]
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: Arc::new(dir.into()),
        }
    }

    /// The conventional location next to an entry file:
    /// `<entry dir>/.aevia/cache/modules`.
    #[must_use]
    pub fn beside(entry: &Path) -> Self {
        let base = entry
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        Self::at(base.join(".aevia").join("cache").join("modules"))
    }

    /// The directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Computes the key of a module.
    #[must_use]
    pub fn key(
        &self,
        inputs: &KeyInputs<'_>,
    ) -> Option<Key> {
        let mut hasher = StableHasher::new();
        hasher.field(compiler_identity().as_bytes());
        hasher.field(inputs.text.as_bytes());
        hasher.field(&inputs.file.to_le_bytes());
        hasher.field(&bincode_bytes(inputs.path)?);
        hasher.field(&bincode_bytes(inputs.identity)?);
        hasher.field(&bincode_bytes(inputs.bindings)?);
        hasher.field(&bincode_bytes(inputs.reexports)?);
        hasher.field(&bincode_bytes(inputs.options)?);
        for digest in inputs.interfaces {
            hasher.field(digest);
        }
        let digest = hasher.finish();
        let prefix = sanitize(inputs.path);
        let mut filename_digest = String::with_capacity(16);
        for byte in digest.first_chunk::<8>()? {
            let _ = write!(filename_digest, "{byte:02x}");
        }
        Some(Key {
            file_name: format!("{prefix}-{filename_digest}.aevi"),
            prefix,
            digest,
        })
    }

    /// Restores a module, or `None` for any kind of miss.
    #[must_use]
    pub fn load(
        &self,
        key: &Key,
        file: FileId,
    ) -> Option<Hit> {
        let mut file_handle = std::fs::File::open(self.dir.join(&key.file_name)).ok()?;
        // Read the size and data from the opened file even if a writer replaces the path.
        let size = usize::try_from(file_handle.metadata().ok()?.len()).ok()?;
        let body_size = size.checked_sub(HEADER_LEN)?;
        let mut prefix = [0; SMALL_ENTRY_LEN];
        let prefix_size = if size <= prefix.len() {
            size
        } else {
            HEADER_LEN
        };
        let prefix = prefix.get_mut(..prefix_size)?;
        file_handle.read_exact(prefix).ok()?;
        let (magic, rest) = prefix.split_first_chunk::<4>()?;
        let (version, rest) = rest.split_first_chunk::<4>()?;
        let (digest, rest) = rest.split_first_chunk::<16>()?;
        if magic != MAGIC || *version != FORMAT_VERSION.to_le_bytes() || *digest != key.digest {
            return None;
        }
        let (length, rest) = rest.split_first_chunk::<8>()?;
        let (checksum, prefix_body) = rest.split_first_chunk::<8>()?;
        let length = u64::from_le_bytes(*length);
        if usize::try_from(length).ok()? != body_size {
            return None;
        }
        let mut heap_body = Vec::new();
        let body = if prefix_body.len() == body_size {
            prefix_body
        } else {
            heap_body.try_reserve_exact(body_size).ok()?;
            let read = (&mut file_handle)
                .take(length)
                .read_to_end(&mut heap_body)
                .ok()?;
            if read != body_size {
                return None;
            }
            heap_body.as_slice()
        };
        let eof = matches!(
            file_handle.read_exact(&mut [0]),
            Err(error) if error.kind() == ErrorKind::UnexpectedEof
        );
        if !eof || u64::from_le_bytes(*checksum) != fnv64(body) {
            return None;
        }
        let entry: Entry = persist::decode(body).ok()?;
        // Spans are only valid against the `FileId` the entry was checked under.
        (entry.file == file).then_some(Hit {
            diagnostics: entry.diagnostics,
            module: entry.module,
            interface: entry.interface,
        })
    }

    /// Writes a checked module atomically. Returns whether a file was written.
    pub fn store(
        &self,
        key: &Key,
        file: FileId,
        interface: Digest,
        diagnostics: &Diagnostics,
        module: &CheckedModule,
    ) -> bool {
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

        let Ok(body) = persist::encode(&EntryRef {
            file,
            interface,
            diagnostics,
            module,
        }) else {
            return false;
        };
        let mut header = [0; HEADER_LEN];
        let fields = MAGIC
            .iter()
            .copied()
            .chain(FORMAT_VERSION.to_le_bytes())
            .chain(key.digest)
            .chain((body.len() as u64).to_le_bytes())
            .chain(fnv64(&body).to_le_bytes());
        for (dest, byte) in header.iter_mut().zip(fields) {
            *dest = byte;
        }
        if std::fs::create_dir_all(&*self.dir).is_err() {
            return false;
        }
        let temporary = self.dir.join(format!(
            "{}.{}.{}.tmp",
            key.file_name,
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let Ok(mut cache_file) = std::fs::File::create_new(&temporary) else {
            return false;
        };
        let written = cache_file
            .write_all(&header)
            .and_then(|()| cache_file.write_all(&body));
        drop(cache_file);
        if written.is_err() {
            let _ = std::fs::remove_file(&temporary);
            return false;
        }
        if std::fs::rename(&temporary, self.dir.join(&key.file_name)).is_err() {
            let _ = std::fs::remove_file(&temporary);
            return false;
        }
        self.prune(key);
        true
    }

    /// Removes the oldest stale revisions of the same module.
    fn prune(
        &self,
        key: &Key,
    ) {
        let Ok(entries) = std::fs::read_dir(&*self.dir) else {
            return;
        };
        let wanted = format!("{}-", key.prefix);
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                let tail = name.strip_prefix(&wanted)?.strip_suffix(".aevi")?;
                (tail.len() == 16 && tail.bytes().all(|b| b.is_ascii_hexdigit())).then(|| {
                    (
                        entry
                            .metadata()
                            .and_then(|meta| meta.modified())
                            .unwrap_or(std::time::UNIX_EPOCH),
                        entry.path(),
                    )
                })
            })
            .collect();
        if files.len() <= KEEP_PER_MODULE {
            return;
        }
        files.sort_by_key(|file| std::cmp::Reverse(file.0));
        for (_, path) in files.into_iter().skip(KEEP_PER_MODULE) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aevia_syntax::Diagnostic;

    #[test]
    fn round_trip_and_corruption_handling() {
        let dir =
            std::env::temp_dir().join(format!("aevia-disk-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = DiskCache::at(&dir);

        let options = CheckOptions::default();
        let path = vec!["Demo".to_string()];
        let key = cache
            .key(&KeyInputs {
                text: "let x = 1\n",
                file: 0,
                path: &path,
                identity: &None,
                bindings: &[],
                reexports: &[],
                options: &options,
                interfaces: &[],
            })
            .expect("key");

        let module = CheckedModule::default();
        let digest = interface_digest(&module.interface()).expect("interface digest");
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(Diagnostic::warning("W0101", "unused", Span::new(0, 4, 5)));

        assert!(cache.store(&key, 0, digest, &diagnostics, &module));
        let hit = cache.load(&key, 0).expect("cache hit");
        assert_eq!(hit.interface, digest);
        assert_eq!(hit.diagnostics.items.len(), 1);
        assert_eq!(hit.diagnostics.items[0].code, "W0101");

        // Corrupt a byte in the stored file and confirm load fails closed.
        let file = dir.join(&key.file_name);
        let mut raw = std::fs::read(&file).expect("read cached file");
        let last = raw.len() - 1;
        raw[last] ^= 0xff;
        std::fs::write(&file, &raw).expect("write corrupt");
        assert!(cache.load(&key, 0).is_none());

        // Wrong FORMAT_VERSION must also fail closed.
        raw[4..8].copy_from_slice(&(FORMAT_VERSION - 1).to_le_bytes());
        std::fs::write(&file, &raw).expect("write stale version");
        assert!(cache.load(&key, 0).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn codec_rejects_trailing_bytes_and_matches_rssn_deterministic_encoding() {
        let sample = vec![("alpha".to_string(), 1u64), ("beta".to_string(), 2u64)];
        let encoded = persist::codec::encode(&sample).expect("encode");
        let decoded: Vec<(String, u64)> = persist::codec::decode(&encoded).expect("decode");
        assert_eq!(decoded, sample);

        let mut with_trailing = encoded;
        with_trailing.push(0x00);
        assert!(persist::codec::decode::<Vec<(String, u64)>>(&with_trailing).is_err());

        // Verify deterministic encoding matches plain standard deterministic config for OpKey.
        let op = aevia_rssn::OpKey::new("core", "add", None);
        let det = bincode_next::encode_to_vec(
            &op,
            bincode_next::config::standard().with_deterministic_bincode(),
        )
        .expect("deterministic encode");
        let fp = persist::codec::encode(&op).expect("fingerprinted encode");
        // Fingerprinted payload has an 8-byte schema hash prefix followed by identical deterministic bytes.
        assert_eq!(fp.len(), det.len() + 8);
        assert_eq!(&fp[8..], &det[..]);
    }
}
