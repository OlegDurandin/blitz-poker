//! Deterministic, serializable PRNG (SplitMix64).
//!
//! The generator state is part of the match snapshot, so restoring a saved match
//! never reshuffles an unfinished hand. The state is serialized as a hex string
//! because JSON numbers lose precision above 2^53 on the JavaScript side.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rng {
    #[serde(with = "hex_u64")]
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix(self.state)
    }

    /// Uniform value in `0..bound` without modulo bias.
    pub fn below(&mut self, bound: u64) -> u64 {
        assert!(bound > 0);
        let zone = u64::MAX - u64::MAX % bound;
        loop {
            let v = self.next_u64();
            if v < zone {
                return v % bound;
            }
        }
    }

    /// Uniform float in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    /// An independent seed derived from the current state without advancing it.
    pub fn derive(&self, salt: u64) -> u64 {
        mix(self.state ^ mix(salt.wrapping_add(0x5851_F42D_4C95_7F2D)))
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            items.swap(i, j);
        }
    }
}

/// SplitMix64 finalizer; also usable as a cheap hash for deriving sub-seeds.
pub fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

pub mod hex_u64 {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{v:016x}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let s = String::deserialize(d)?;
        u64::from_str_radix(&s, 16).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_resumable() {
        let mut a = Rng::new(42);
        a.next_u64();
        let snapshot = serde_json::to_string(&a).unwrap();
        let mut b: Rng = serde_json::from_str(&snapshot).unwrap();
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn below_stays_in_range() {
        let mut rng = Rng::new(7);
        for bound in 1..60 {
            for _ in 0..50 {
                assert!(rng.below(bound) < bound);
            }
        }
    }
}
