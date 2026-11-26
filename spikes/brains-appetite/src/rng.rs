//! Seeded hashing, the only source of chance: `hash(seed, tick, creature, salt)`.
//! The same mixer as `spikes/rust-core/fv-core/src/rng.rs` (copied, not shared,
//! so this spike stands alone).

#[inline]
pub fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[inline]
pub fn hash(a: u64, b: u64, c: u64, d: u64) -> u64 {
    mix(mix(mix(mix(a ^ 0x9e37_79b9_7f4a_7c15).wrapping_add(b)).wrapping_add(c)).wrapping_add(d))
}

/// A whole number in 0..n from a hash.
pub fn below(h: u64, n: u64) -> u64 { ((h as u128 * n as u128) >> 64) as u64 }

/// Folds state into one number, for comparing runs.
pub struct Hasher(u64);
impl Hasher {
    pub fn new(seed: u64) -> Hasher { Hasher(mix(seed)) }
    pub fn u64(&mut self, v: u64) { self.0 = mix(self.0 ^ v).wrapping_add(0x9e37_79b9_7f4a_7c15) }
    pub fn i64(&mut self, v: i64) { self.u64(v as u64) }
    pub fn finish(&self) -> u64 { mix(self.0) }
}
