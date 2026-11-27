//! Seeded hashing: the only source of chance. The same inputs give the same
//! number on every machine, so "random" choices are part of the deterministic
//! state.

use crate::cell::Cell;

#[inline]
pub fn mix(mut z: u64) -> u64 {
    // splitmix64 finaliser
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[inline]
pub fn hash(a: u64, b: u64, c: u64, d: u64) -> u64 {
    mix(mix(mix(mix(a ^ 0x9e37_79b9_7f4a_7c15).wrapping_add(b)).wrapping_add(c)).wrapping_add(d))
}

/// A tiny stream of numbers from one seed, for a packet's journey.
#[derive(Clone, Copy, Debug)]
pub struct Stream(pub u64);
impl Stream {
    #[inline]
    pub fn next(&mut self) -> u64 { self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15); mix(self.0) }
    /// A whole number in 0..n.
    #[inline]
    pub fn below(&mut self, n: u64) -> u64 { ((self.next() as u128 * n as u128) >> 64) as u64 }
    /// True with probability p / 2³².
    #[inline]
    pub fn chance(&mut self, p: u64) -> bool { (self.next() >> 32) < p }
}

pub struct Hasher(u64);
impl Hasher {
    pub fn new(seed: u64) -> Hasher { Hasher(mix(seed)) }
    #[inline]
    pub fn u64(&mut self, v: u64) { self.0 = mix(self.0 ^ v).wrapping_add(0x9e37_79b9_7f4a_7c15) }
    pub fn cell(&mut self, c: &Cell) {
        self.u64(c.substance as u64 | (c.bonds as u64) << 8 | (c.lit as u64) << 16);
        for m in c.solid { self.u64(m.0 as u64) }
        self.u64(c.water.0 as u64);
        for m in c.gas { self.u64(m.0 as u64) }
        self.u64(c.energy.0 as u64);
        self.u64(c.solid_energy.0 as u64);
        self.u64(c.skin.0 as u64);
        self.u64(c.nuclear.0 as u64);
    }
    pub fn finish(&self) -> u64 { mix(self.0) }
}
