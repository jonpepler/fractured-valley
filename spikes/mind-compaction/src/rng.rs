//! Seeded hashing, the only source of chance. Copied from `spikes/thought-graph` (itself from
//! `spikes/brains-talk`), not depended on.

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

/// A number in 0..1000 from `hash(seed, tick, id, salt)`.
#[inline]
pub fn roll(seed: u64, tick: u32, id: u32, salt: u64) -> i32 {
    (hash(seed, tick as u64, id as u64, salt) % 1000) as i32
}

/// A `std::hash::Hasher` that gives the same bits on every machine: integers are fed as
/// little-endian 64-bit values, so `usize`/`isize` (lengths, enum tags) don't depend on width.
pub struct StateHasher(u64);
impl StateHasher {
    pub fn new(seed: u64) -> Self { StateHasher(mix(seed)) }
}
impl std::hash::Hasher for StateHasher {
    fn finish(&self) -> u64 { mix(self.0) }
    fn write(&mut self, bytes: &[u8]) { for &b in bytes { self.write_u64(b as u64) } }
    fn write_u8(&mut self, i: u8) { self.write_u64(i as u64) }
    fn write_u16(&mut self, i: u16) { self.write_u64(i as u64) }
    fn write_u32(&mut self, i: u32) { self.write_u64(i as u64) }
    fn write_u64(&mut self, i: u64) { self.0 = mix(self.0 ^ i.wrapping_add(0x9e37_79b9_7f4a_7c15)) }
    fn write_usize(&mut self, i: usize) { self.write_u64(i as u64) }
    fn write_i8(&mut self, i: i8) { self.write_u64(i as i64 as u64) }
    fn write_i16(&mut self, i: i16) { self.write_u64(i as i64 as u64) }
    fn write_i32(&mut self, i: i32) { self.write_u64(i as i64 as u64) }
    fn write_i64(&mut self, i: i64) { self.write_u64(i as u64) }
    fn write_isize(&mut self, i: isize) { self.write_u64(i as i64 as u64) }
}
