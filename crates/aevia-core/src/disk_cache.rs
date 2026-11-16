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

use aevia_syntax::Diagnostics;
use aevia_syntax::FileId;
use aevia_types::check::CheckOptions;
use aevia_types::check::CheckedModule;
use aevia_types::check::ModuleInterface;
use aevia_types::persist;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

/// The `.aevi` layout version; bump it with any change to the encoded types.
pub const FORMAT_VERSION: u32 = 3;

const MAGIC: &[u8; 4] = b"AEVI";
const HEADER_LEN: usize = 40;

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
        out[..8].copy_from_slice(&self.a.to_le_bytes());
        out[8..].copy_from_slice(&self.b.to_le_bytes());
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

fn hex(digest: &Digest) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// What identifies the compiler that wrote an entry: the checker's behaviour
/// must be a function of this string. The executable's size and modification
/// time make a rebuilt checker invalidate entries even at an unchanged version.
fn compiler_identity() -> &'static str {
    static IDENTITY: OnceLock<String> = OnceLock::new();
    IDENTITY.get_or_init(|| {
        let exe = std::env::current_exe()
            .and_then(std::fs::metadata)
            .map(|meta| format!("{}|{:?}", meta.len(), meta.modified().ok()))
            .unwrap_or_default();
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
    /// The prefixes of the flat import table.
    pub prefixes: &'a [String],
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
        hasher.field(&bincode_bytes(inputs.prefixes)?);
        hasher.field(&bincode_bytes(inputs.options)?);
        for digest in inputs.interfaces {
            hasher.field(digest);
        }
        let digest = hasher.finish();
        let prefix = sanitize(inputs.path);
        Some(Key {
            file_name: format!("{prefix}-{}.aevi", &hex(&digest)[..16]),
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
        let bytes = std::fs::read(self.dir.join(&key.file_name)).ok()?;
        if bytes.len() < HEADER_LEN || &bytes[..4] != MAGIC {
            return None;
        }
        if bytes[4..8] != FORMAT_VERSION.to_le_bytes() || bytes[8..24] != key.digest {
            return None;
        }
        let length = usize::try_from(u64::from_le_bytes(bytes[24..32].try_into().ok()?)).ok()?;
        let checksum = u64::from_le_bytes(bytes[32..40].try_into().ok()?);
        let body = bytes.get(HEADER_LEN..)?;
        if body.len() != length || fnv64(body) != checksum {
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
        let Ok(body) = persist::encode(&EntryRef {
            file,
            interface,
            diagnostics,
            module,
        }) else {
            return false;
        };
        let mut data = Vec::with_capacity(HEADER_LEN + body.len());
        data.extend_from_slice(MAGIC);
        data.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        data.extend_from_slice(&key.digest);
        data.extend_from_slice(&(body.len() as u64).to_le_bytes());
        data.extend_from_slice(&fnv64(&body).to_le_bytes());
        data.extend_from_slice(&body);
        if std::fs::create_dir_all(&*self.dir).is_err() {
            return false;
        }
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        let temporary = self.dir.join(format!(
            "{}.{}.{}.tmp",
            key.file_name,
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        if std::fs::write(&temporary, &data).is_err() {
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
    use aevia_syntax::Span;

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
                prefixes: &[],
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
