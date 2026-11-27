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

/// The pinned teaching runs (stage 2): people talking, a dry spring, an eavesdropper, a note lost
/// and a map, each stepped on `threads` threads in the order given (a permutation of 0..4).
pub fn people_runs(threads: usize) -> Vec<(&'static str, u64)> {
    use crate::people::person::Medium;
    use crate::people::scenarios::{Setup, Spring, BRYN, DAY, DEE};
    use crate::people::world::Event;
    let order: Vec<usize> = (0..4).collect();
    let drive = |mut v: crate::people::sim::Valley, ticks: u32| { for _ in 0..ticks { v.step(threads, &order); } v.state_hash() };
    let mut out = vec![];
    out.push(("teaching: told, found, thanked (1 day)", drive(Setup::new(1).temper(1).build(), DAY)));
    let mut s = Setup::new(2).temper(0); s.spring = Spring::DriesAt(5);
    out.push(("teaching: the spring dries, Ash corrects itself (3 days)", { let mut v = s.build(); let (x, y) = v.w.spring(); v.at(DAY + 20, Event::Put { who: 0, x: x + 2, y: y + 2 }); drive(v, 3 * DAY) }));
    let mut s = Setup::new(3).temper(2); s.pos[DEE] = (17, 25); s.knows[DEE].retain(|&x| x != crate::people::world::STREAM);
    out.push(("teaching: an eavesdropper (2 days)", drive(s.build(), 2 * DAY)));
    let mut s = Setup::new(4).temper(1); s.medium = Medium::Note; s.days = 5;
    out.push(("teaching: a note, lost on day 2 (5 days)", { let mut v = s.build(); v.at(2 * DAY + 50, Event::LoseNotes { who: BRYN }); drive(v, 5 * DAY) }));
    let mut s = Setup::new(5).temper(0); s.medium = Medium::Map;
    out.push(("teaching: a drawn map (1 day)", drive(s.build(), DAY)));
    out
}

/// The teaching valley (4 people, a note, a dry spring later) stepped on `threads` threads in
/// `order`, for `ticks`: the hash of the world and every mind.
pub fn people_order(threads: usize, order: &[usize], ticks: u32) -> u64 {
    use crate::people::scenarios::{Setup, Spring, DEE};
    let mut s = Setup::new(9).temper(1); s.medium = crate::people::person::Medium::Note; s.pos[DEE] = (17, 25); s.spring = Spring::DriesAt(400);
    let mut v = s.build();
    for _ in 0..ticks { v.step(threads, order); }
    v.state_hash()
}
