//! A tiny deterministic generator for the property-style tests.
//!
//! No `proptest`: the crate has no dependencies, and a fixed-seed xorshift gives
//! reproducible failures (the seed is in the test) without a shrinker the
//! small inputs here do not need.

use crate::req::VersionReq;
use crate::version::Identifier;
use crate::version::Version;

/// xorshift64*.
pub struct Rng(u64);

impl Rng {
    /// A generator; the seed is mixed so that small seeds are not degenerate.
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    /// The next 64 random bits.
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `0..bound`.
    pub fn below(
        &mut self,
        bound: u64,
    ) -> u64 {
        self.next() % bound.max(1)
    }

    /// True with probability `percent`/100.
    pub fn chance(
        &mut self,
        percent: u64,
    ) -> bool {
        self.below(100) < percent
    }

    fn identifiers(
        &mut self,
        allow_padded: bool,
    ) -> Vec<Identifier> {
        let count = 1 + self.below(3);
        (0..count)
            .map(|_| {
                match self.below(4) {
                    | 0 => Identifier::Numeric(self.below(12)),
                    | 1 => Identifier::Alpha("alpha".to_string()),
                    | 2 => Identifier::Alpha("rc".to_string()),
                    | _ => {
                        if allow_padded {
                            Identifier::Alpha("007".to_string())
                        } else {
                            Identifier::Alpha("beta-1".to_string())
                        }
                    },
                }
            })
            .collect()
    }

    /// A random version, sometimes with a pre-release and build metadata.
    pub fn version(&mut self) -> Version {
        let mut version = Version::new(self.below(4), self.below(4), self.below(4));
        if self.chance(30) {
            version.pre = self.identifiers(false);
            // Numeric pre-release identifiers must not look padded; the pool
            // above never produces `007` for them.
            version
                .pre
                .retain(|id| !matches!(id, Identifier::Alpha(text) if text == "007"));
            if version.pre.is_empty() {
                version.pre.push(Identifier::Numeric(1));
            }
        }
        if self.chance(15) {
            version.build = self.identifiers(true);
        }
        version
    }

    /// A random requirement built from the operator table.
    pub fn requirement(&mut self) -> VersionReq {
        let count = 1 + self.below(2);
        let parts: Vec<String> = (0..count)
            .map(|_| {
                let op = ["", "^", "~", "=", ">", ">=", "<", "<="][self.below(8) as usize];
                let major = self.below(4);
                match self.below(3) {
                    | 0 => format!("{op}{major}"),
                    | 1 => format!("{op}{major}.{}", self.below(4)),
                    | _ => format!("{op}{major}.{}.{}", self.below(4), self.below(4)),
                }
            })
            .collect();
        VersionReq::parse(&parts.join(", ")).expect("generated requirements parse")
    }
}
