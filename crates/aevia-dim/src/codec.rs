//! Shared `bincode` encoding helpers for non-stdlib types (`BigRational`, `IndexMap`).

use bincode_next::Decode;
use bincode_next::Encode;
use bincode_next::Fingerprint;
use bincode_next::config::Config;
use bincode_next::de::Decoder;
use bincode_next::enc::Encoder;
use bincode_next::error::DecodeError;
use bincode_next::error::EncodeError;
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_rational::BigRational;
use std::hash::Hash;

pub(crate) const fn mix_hash(
    acc: u64,
    label: &[u8],
    child: u64,
) -> u64 {
    let acc = bincode_next::rapidhash::v3::rapidhash_v3_seeded(
        label,
        &bincode_next::rapidhash::v3::RapidSecrets::seed_cpp(acc),
    );
    bincode_next::rapidhash::v3::rapidhash_v3_seeded(
        &child.to_le_bytes(),
        &bincode_next::rapidhash::v3::RapidSecrets::seed_cpp(acc),
    )
}

pub(crate) const fn rational_schema_hash<C: Config>() -> u64 {
    bincode_next::rapidhash::v3::rapidhash_v3_seeded(
        b"BigRational",
        &bincode_next::rapidhash::v3::RapidSecrets::seed_cpp(C::CONFIG_HASH),
    )
}

pub(crate) fn encode_rational<E: Encoder>(
    rat: &BigRational,
    encoder: &mut E,
) -> Result<(), EncodeError> {
    rat.numer().to_signed_bytes_le().encode(encoder)?;
    rat.denom().to_signed_bytes_le().encode(encoder)?;
    Ok(())
}

pub(crate) fn decode_rational<D: Decoder>(decoder: &mut D) -> Result<BigRational, DecodeError> {
    let numer_bytes = Vec::<u8>::decode(decoder)?;
    let denom_bytes = Vec::<u8>::decode(decoder)?;
    let numer = BigInt::from_signed_bytes_le(&numer_bytes);
    let denom = BigInt::from_signed_bytes_le(&denom_bytes);
    if denom == BigInt::from(0) {
        return Err(DecodeError::OtherString(
            "zero denominator in BigRational".to_string(),
        ));
    }
    Ok(BigRational::new(numer, denom))
}

pub(crate) const fn index_map_schema_hash<C: Config, K: Fingerprint<C>, V: Fingerprint<C>>() -> u64
{
    let h = bincode_next::rapidhash::v3::rapidhash_v3_seeded(
        b"IndexMap",
        &bincode_next::rapidhash::v3::RapidSecrets::seed_cpp(C::CONFIG_HASH),
    );
    mix_hash(mix_hash(h, b"K", K::SCHEMA_HASH), b"V", V::SCHEMA_HASH)
}

pub(crate) fn encode_index_map<K: Encode, V: Encode, E: Encoder>(
    map: &IndexMap<K, V>,
    encoder: &mut E,
) -> Result<(), EncodeError> {
    map.len().encode(encoder)?;
    for (k, v) in map {
        k.encode(encoder)?;
        v.encode(encoder)?;
    }
    Ok(())
}

pub(crate) fn decode_index_map<Context, K, V, D>(
    decoder: &mut D
) -> Result<IndexMap<K, V>, DecodeError>
where
    K: Decode<Context> + Eq + Hash,
    V: Decode<Context>,
    D: Decoder<Context = Context>,
{
    let len = usize::decode(decoder)?;
    let mut map = IndexMap::with_capacity(len.min(1024));
    for _ in 0..len {
        let k = K::decode(decoder)?;
        let v = V::decode(decoder)?;
        map.insert(k, v);
    }
    Ok(map)
}
