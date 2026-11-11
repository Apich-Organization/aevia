//! The persistence codec of checked modules (the `.aevi` body, spec §9.11).
//!
//! A [`CheckedModule`](crate::CheckedModule) is full of [`AdtId`]s, and an
//! `AdtId` is a process-local witness that must never enter a cache
//! (spec §7.3). This module is the one place where that rule is enforced:
//!
//! * **Encoding.** Every `AdtId` field serializes through [`adt_ref`] (or
//!   [`adt_pair_map`]). While [`encode`] runs, a thread-local context hands out
//!   a dense local index per distinct id and remembers the id. Afterwards the
//!   full definition of each collected type (symbolic [`AdtKey`], module,
//!   parameters, kinds, constructors) is written into an ADT table; the
//!   definitions are themselves encoded through the same adapter, so cycles go
//!   through indices. The reserved prelude ids (`Option`, `Result`,
//!   `Ordering`) are encoded as themselves.
//! * **Decoding.** [`decode`] first *re-interns* every table entry into this
//!   process's registry: a complete type with the same [`AdtKey`] (module,
//!   name, declaration schema) that is already registered is reused (the
//!   standard library and imported modules usually are), otherwise a fresh id
//!   is allocated. Only then is the body decoded, with the `old index -> new
//!   id` map installed.
//!
//! The bytes are deterministic for a given value as long as the value contains
//! no hash-ordered map, which is what lets a digest of an encoded interface
//! serve as a dependency key.

use crate::adt;
use crate::adt::Adt;
use crate::adt::AdtId;
use crate::adt::Ctor;
use crate::adt::ParamKind;
use crate::keys::AdtKey;
use crate::keys::ModulePath;
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde::de::Error as _;
use serde::ser::Error as _;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;

/// Indices below this are the reserved prelude ids, encoded as themselves.
const RESERVED: u32 = 3;

/// Why a value could not be encoded or decoded.
#[derive(Debug)]
pub struct PersistError(pub String);

impl fmt::Display for PersistError {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PersistError {}

impl From<bincode::Error> for PersistError {
    fn from(error: bincode::Error) -> Self {
        Self(error.to_string())
    }
}

#[derive(Default)]
struct EncodeContext {
    index: HashMap<AdtId, u32>,
    order: Vec<AdtId>,
}

thread_local! {
    static ENCODE: RefCell<Option<EncodeContext>> = const { RefCell::new(None) };
    static DECODE: RefCell<Option<Vec<AdtId>>> = const { RefCell::new(None) };
}

/// Clears a thread-local context on every exit path.
struct Scope(fn());

impl Drop for Scope {
    fn drop(&mut self) {
        (self.0)();
    }
}

fn encoded_index(id: AdtId) -> Option<u32> {
    if id < RESERVED {
        return Some(id);
    }
    ENCODE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let context = slot.as_mut()?;
        if let Some(index) = context.index.get(&id) {
            return Some(*index);
        }
        let index = RESERVED + u32::try_from(context.order.len()).ok()?;
        context.index.insert(id, index);
        context.order.push(id);
        Some(index)
    })
}

fn decoded_id(index: u32) -> Option<AdtId> {
    if index < RESERVED {
        return Some(index);
    }
    DECODE.with(|cell| {
        cell.borrow()
            .as_ref()?
            .get(usize::try_from(index - RESERVED).ok()?)
            .copied()
    })
}

/// Serde adapter for a bare `AdtId` field.
pub mod adt_ref {
    use super::*;
    use serde::Deserializer;
    use serde::Serializer;

    /// Writes the local index of `id`.
    ///
    /// # Errors
    ///
    /// Fails outside [`encode`].
    pub fn serialize<S: Serializer>(
        id: &AdtId,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let index = encoded_index(*id)
            .ok_or_else(|| S::Error::custom("an AdtId was serialized outside persist::encode"))?;
        serializer.serialize_u32(index)
    }

    /// Reads a local index and maps it to this process's id.
    ///
    /// # Errors
    ///
    /// Fails outside [`decode`] or for an index the ADT table does not hold.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<AdtId, D::Error> {
        let index = u32::deserialize(deserializer)?;
        decoded_id(index).ok_or_else(|| D::Error::custom("unknown ADT index in cache entry"))
    }
}

struct PairRef<'a>(&'a (AdtId, usize));

impl Serialize for PairRef<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let index = encoded_index(self.0.0)
            .ok_or_else(|| S::Error::custom("an AdtId was serialized outside persist::encode"))?;
        (index, self.0.1).serialize(serializer)
    }
}

struct PairOwned((AdtId, usize));

impl<'de> Deserialize<'de> for PairOwned {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (index, ctor) = <(u32, usize)>::deserialize(deserializer)?;
        let id = decoded_id(index)
            .ok_or_else(|| D::Error::custom("unknown ADT index in cache entry"))?;
        Ok(Self((id, ctor)))
    }
}

/// Serde adapter for a map whose values are `(AdtId, constructor index)`.
///
/// Written as a sequence of `(key, (index, ctor))` pairs, so it works for
/// `IndexMap` and `HashMap` alike.
pub mod adt_pair_map {
    use super::*;

    /// Writes the entries with their ADT ids as local indices.
    ///
    /// # Errors
    ///
    /// Fails outside [`encode`].
    pub fn serialize<K, S>(
        map: &indexmap::IndexMap<K, (AdtId, usize)>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        K: Serialize + std::hash::Hash + Eq,
        S: serde::Serializer,
    {
        serializer.collect_seq(map.iter().map(|(key, value)| (key, PairRef(value))))
    }

    /// Reads the entries and maps their ADT indices to this process's ids.
    ///
    /// # Errors
    ///
    /// Fails outside [`decode`] or for an unknown index.
    pub fn deserialize<'de, K, M, D>(deserializer: D) -> Result<M, D::Error>
    where
        K: Deserialize<'de>,
        M: FromIterator<(K, (AdtId, usize))>,
        D: serde::Deserializer<'de>,
    {
        let entries: Vec<(K, PairOwned)> = Vec::deserialize(deserializer)?;
        Ok(entries
            .into_iter()
            .map(|(key, pair)| (key, pair.0))
            .collect())
    }
}

/// Serde adapter for a `HashMap` that must encode deterministically: the
/// entries are written in key order.
pub mod sorted_map {
    use super::*;
    use std::collections::BTreeMap;
    use std::hash::Hash;

    /// Writes the entries sorted by key.
    ///
    /// # Errors
    ///
    /// Propagates the serializer's error.
    pub fn serialize<K, V, S>(
        map: &HashMap<K, V>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        K: Serialize + Ord,
        V: Serialize,
        S: serde::Serializer,
    {
        let sorted: BTreeMap<&K, &V> = map.iter().collect();
        serializer.collect_map(sorted)
    }

    /// Reads the entries back into a `HashMap`.
    ///
    /// # Errors
    ///
    /// Propagates the deserializer's error.
    pub fn deserialize<'de, K, V, D>(deserializer: D) -> Result<HashMap<K, V>, D::Error>
    where
        K: Deserialize<'de> + Eq + Hash,
        V: Deserialize<'de>,
        D: serde::Deserializer<'de>,
    {
        HashMap::deserialize(deserializer)
    }
}

/// [`adt_pair_map`] for a `HashMap`, written in key order so the encoding is
/// deterministic.
pub mod sorted_adt_pair_map {
    use super::*;
    use std::hash::Hash;

    /// Writes the entries sorted by key.
    ///
    /// # Errors
    ///
    /// Fails outside [`encode`].
    pub fn serialize<K, S>(
        map: &HashMap<K, (AdtId, usize)>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        K: Serialize + Ord,
        S: serde::Serializer,
    {
        let mut entries: Vec<_> = map.iter().collect();
        entries.sort_by(|left, right| left.0.cmp(right.0));
        serializer.collect_seq(
            entries
                .into_iter()
                .map(|(key, value)| (key, PairRef(value))),
        )
    }

    /// Reads the entries and maps their ADT indices to this process's ids.
    ///
    /// # Errors
    ///
    /// Fails outside [`decode`] or for an unknown index.
    pub fn deserialize<'de, K, D>(deserializer: D) -> Result<HashMap<K, (AdtId, usize)>, D::Error>
    where
        K: Deserialize<'de> + Eq + Hash,
        D: serde::Deserializer<'de>,
    {
        super::adt_pair_map::deserialize(deserializer)
    }
}

/// One entry of the ADT table.
#[derive(Serialize, Deserialize)]
struct AdtRecord {
    key: AdtKey,
    module: ModulePath,
    name: String,
    params: Vec<String>,
    kinds: Vec<ParamKind>,
    complete: bool,
    /// `Vec<Ctor>` encoded through the adapter (indices into the same table).
    ctors: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct Payload {
    adts: Vec<AdtRecord>,
    body: Vec<u8>,
}

fn record_of(id: AdtId) -> Result<AdtRecord, PersistError> {
    let adt = adt::get(id).ok_or_else(|| PersistError(format!("unregistered ADT #{id}")))?;
    let ctors = bincode::serialize(&adt.ctors)?;
    Ok(AdtRecord {
        key: adt::key_of(id),
        module: adt::module_of(id),
        name: adt.name.clone(),
        params: adt.params.clone(),
        kinds: adt::param_kinds_of(id),
        complete: adt::is_complete(id),
        ctors,
    })
}

/// Encodes `value`, collecting and writing every ADT it mentions.
///
/// # Errors
///
/// Fails when a mentioned type is not registered or the codec rejects the value.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, PersistError> {
    ENCODE.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_some() {
            return Err(PersistError("persist::encode is not re-entrant".into()));
        }
        *slot = Some(EncodeContext::default());
        Ok(())
    })?;
    let _scope = Scope(|| ENCODE.with(|cell| *cell.borrow_mut() = None));
    let body = bincode::serialize(value)?;
    let mut adts = Vec::new();
    let mut next = 0;
    loop {
        let id = ENCODE.with(|cell| {
            cell.borrow()
                .as_ref()
                .and_then(|context| context.order.get(next).copied())
        });
        let Some(id) = id else { break };
        adts.push(record_of(id)?);
        next += 1;
    }
    Ok(bincode::serialize(&Payload { adts, body })?)
}

/// Decodes bytes written by [`encode`], re-interning every ADT first.
///
/// # Errors
///
/// Fails on malformed bytes; the registry may keep types allocated for a
/// partially decoded entry, which is harmless (ids are never reused).
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, PersistError> {
    decode_with(bytes, true)
}

/// Like [`decode`], but never reuses a type this process already registered:
/// every source type of the table gets a fresh id. This is what a process that
/// had not seen the declarations would do, so tests use it to exercise the
/// remapping inside one process.
///
/// # Errors
///
/// Fails on malformed bytes.
pub fn decode_fresh<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, PersistError> {
    decode_with(bytes, false)
}

fn decode_with<T: DeserializeOwned>(
    bytes: &[u8],
    reuse: bool,
) -> Result<T, PersistError> {
    let payload: Payload = bincode::deserialize(bytes)?;
    // Phase 1: resolve or allocate every type, so the map is complete before
    // any constructor field (which may name any entry, or itself) is read.
    let mut map = Vec::with_capacity(payload.adts.len());
    let mut fresh = Vec::new();
    for (position, record) in payload.adts.iter().enumerate() {
        let (id, is_new) = resolve(record, reuse);
        if is_new {
            fresh.push(position);
        }
        map.push(id);
    }
    DECODE.with(|cell| *cell.borrow_mut() = Some(map.clone()));
    let _scope = Scope(|| DECODE.with(|cell| *cell.borrow_mut() = None));
    // Phase 2: constructors of the types this process did not already have.
    for position in fresh {
        let record = &payload.adts[position];
        let ctors: Vec<Ctor> = bincode::deserialize(&record.ctors)?;
        if record.complete || !ctors.is_empty() {
            adt::set_ctors(map[position], ctors);
        }
    }
    Ok(bincode::deserialize(&payload.body)?)
}

/// The id for one record, and whether this process had to create it.
fn resolve(
    record: &AdtRecord,
    reuse: bool,
) -> (AdtId, bool) {
    if record.module.is_prelude() {
        // A host/opaque or builtin type: interned by name, shared by every unit.
        let probe = Adt {
            name: record.name.clone(),
            params: record.params.clone(),
            ctors: Vec::new(),
        };
        let before = adt::count();
        let id = adt::intern(probe);
        let created = usize::try_from(id).is_ok_and(|index| index >= before);
        return (id, created);
    }
    if reuse
        && record.key.schema.is_some()
        && let Some(existing) = adt::find_complete(&record.key)
    {
        return (existing, false);
    }
    let id = adt::allocate_in(record.module.clone(), &record.name, record.params.clone());
    adt::set_param_kinds(id, record.kinds.clone());
    (id, true)
}

/// Generates a `with` adapter that stores a table as "the default" when it
/// equals the table every module starts from, and in full otherwise.
///
/// Nearly every module declares no units and no dimensions, yet the full SI
/// unit table and dimension environment were the bulk (20–30 KB) of every
/// `.aevi` file, so decoding one cost about as much as checking the module.
/// Equality is decided on the encoded bytes, so no `PartialEq` is needed and
/// a table that differs in any way is kept exactly.
macro_rules! default_delta {
    ($name:ident, $ty:ty, $base:expr) => {
        #[doc = concat!("Stores a `", stringify!($ty), "` as a marker when it is the default.")]
        pub mod $name {
            use super::*;

            thread_local! {
                static BASE: (Vec<u8>, $ty) = {
                    let base: $ty = $base;
                    (bincode::serialize(&base).unwrap_or_default(), base)
                };
            }

            /// Serializes `value` (`None` when it is the default).
            ///
            /// # Errors
            /// The serializer's.
            pub fn serialize<S: serde::Serializer>(
                value: &$ty,
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                let bytes = bincode::serialize(value).map_err(S::Error::custom)?;
                let is_base = BASE.with(|(base, _)| !base.is_empty() && *base == bytes);
                if is_base {
                    Option::<&$ty>::None.serialize(serializer)
                } else {
                    Some(value).serialize(serializer)
                }
            }

            /// Deserializes a table, restoring the default from its marker.
            ///
            /// # Errors
            /// The deserializer's.
            pub fn deserialize<'de, D: serde::Deserializer<'de>>(
                deserializer: D
            ) -> Result<$ty, D::Error> {
                let value = Option::<$ty>::deserialize(deserializer)?;
                Ok(value.unwrap_or_else(|| BASE.with(|(_, base)| base.clone())))
            }
        }
    };
}

default_delta!(
    default_units,
    aevia_dim::UnitTable,
    aevia_rssn::unit_table()
);
default_delta!(default_dims, aevia_dim::DimEnv, aevia_dim::DimEnv::si());
