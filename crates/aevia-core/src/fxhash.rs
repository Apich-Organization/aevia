//! A small multiplicative hasher for the interpreter's hot, trusted-key maps.
//!
//! The standard library's `RandomState` is SipHash-1-3, which is chosen to
//! resist adversarial keys. Every map this is used for is keyed by a source
//! span, a pointer identity or a program identifier, and sits on the
//! per-call path, where hashing a key with SipHash cost more than the lookup
//! itself. This is the FxHash scheme (one rotate, xor and multiply per word).

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
    fn add(
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
        let mut rest = bytes;
        while rest.len() >= 8 {
            let (head, tail) = rest.split_at(8);
            let mut word = [0u8; 8];
            word.copy_from_slice(head);
            self.add(u64::from_le_bytes(word));
            rest = tail;
        }
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
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
