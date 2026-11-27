//! The pinned determinism runs, shared by the tests and the `hashes` binary (which is also built
//! for wasm32 and run under node, so the native and browser builds can be compared).

use crate::fox::scenarios::{self, HomeMove, DAY, DAY2};
use crate::fox::sim::Pack;

/// (name, hash) for each pinned run. `pack_threads` steps the many-fox run on that many threads
/// (1 under wasm32, which has no threads here).
pub fn runs(pack_threads: usize) -> Vec<(&'static str, u64)> {
    let seed = scenarios::SEED;
    let mut v = vec![];
    let mut s = scenarios::meadow_fire(seed); s.run(DAY2 + 80); v.push(("two days, the dog, a night, a meadow fire", s.state_hash()));
    let mut s = scenarios::sleepless_days(seed, 2); s.run(2 * DAY); v.push(("sleepless, 2 quiet days", s.state_hash()));
    let mut s = scenarios::busy_day(seed); s.run(DAY + 50); v.push(("busy day", s.state_hash()));
    let mut s = scenarios::home_moved(seed, HomeMove::Daily, 4); s.run(4 * DAY); v.push(("den moved every night, 4 days", s.state_hash()));
    let mut s = scenarios::dog_charges(seed, 18); s.run(20); v.push(("the dog charges a quick fox", s.state_hash()));
    v.push(("16 foxes in one valley, 2 days", pack(seed, 16, 2 * DAY, pack_threads, &(0..16).collect::<Vec<_>>())));
    v
}

/// `n` foxes sharing one valley (berries, the dog, a fire, nights) for `ticks`, on `threads`
/// threads, stepping minds in `order`. Returns the hash of the world and every mind.
pub fn pack(seed: u64, n: usize, ticks: u32, threads: usize, order: &[usize]) -> u64 {
    let mut p = Pack::new(seed, n);
    p.w.script.push((120, crate::fox::world::Event::FireAt { x: 35, y: 16 }));
    p.w.script.push((170, crate::fox::world::Event::FiresOut));
    p.w.script.push((200, crate::fox::world::Event::DogChase));
    for d in 0..ticks / DAY + 1 { p.w.script.push((scenarios::night_of(d), crate::fox::world::Event::Night)); }
    for _ in 0..ticks { p.step(threads, order); }
    p.state_hash()
}
