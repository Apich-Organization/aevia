//! A small multiplicative hasher for the interpreter's hot, trusted-key maps.
//!
//! The standard library's `RandomState` is SipHash-1-3, which is chosen to
//! resist adversarial keys. Every map this is used for is keyed by a source
//! span, a pointer identity or a program identifier, and sits on the
//! per-call path, where hashing a key with `SipHash` cost more than the lookup
//! itself. This is the `FxHash` scheme (one rotate, xor and multiply per word).

#![deny(
    missing_docs,
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::nursery,
    clippy::panic,
    clippy::pedantic,
    clippy::single_call_fn,
    clippy::unwrap_used
)]

use std::hash::BuildHasherDefault;
use std::hash::Hasher;

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

/// The Fx hasher.
#[derive(Clone, Copy, Default)]
pub struct FxHasher {
    hash: u64,
}

impl FxHasher {
    #[inline]
    const fn add(
        &mut self,
        word: u64,
    ) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(
        &mut self,
        bytes: &[u8],
    ) {
        let (words, rest) = bytes.as_chunks::<8>();
        for &word in words {
            self.add(u64::from_le_bytes(word));
        }
        let mut word = [0u8; 8];
        if !rest.is_empty()
            && let Some(prefix) = word.get_mut(..rest.len())
        {
            prefix.copy_from_slice(rest);
            self.add(u64::from_le_bytes(word) ^ ((rest.len() as u64) << 56));
        }
    }

    #[inline]
    fn write_u8(
        &mut self,
        value: u8,
    ) {
        self.add(u64::from(value));
    }

    #[inline]
    fn write_u16(
        &mut self,
        value: u16,
    ) {
        self.add(u64::from(value));
    }

    #[inline]
    fn write_u32(
        &mut self,
        value: u32,
    ) {
        self.add(u64::from(value));
    }

    #[inline]
    fn write_u64(
        &mut self,
        value: u64,
    ) {
        self.add(value);
    }

    #[inline]
    fn write_usize(
        &mut self,
        value: usize,
    ) {
        self.add(value as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

/// The `BuildHasher` for [`FxHasher`].
pub type FxBuild = BuildHasherDefault<FxHasher>;

/// A `HashMap` using [`FxHasher`].
pub type FxHashMap<K, V> = std::collections::HashMap<K, V, FxBuild>;
