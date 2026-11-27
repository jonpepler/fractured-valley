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

/// The pinned danger runs (stage 3): a hunter and a forager meet a boar, alone or with allies, and
/// a call for help carries to two people out of sight. People and the boar step on `threads` threads.
pub fn danger_runs(threads: usize) -> Vec<(&'static str, u64)> {
    use crate::danger::runs::{build, build_cast, meet, Case, Cast, MEET, TEMPERS, WATCH};
    let drive = |mut v: crate::people::sim::Valley, seed: u64| { let order: Vec<usize> = (0..4).collect(); meet(&mut v, seed, 8); let _ = MEET; for _ in 0..WATCH { v.step(threads, &order); } v.state_hash() };
    let mut out = vec![];
    out.push(("danger: a hunter alone meets a boar", drive(build(&Case { seed: 1, temper: TEMPERS[1].1, allies: false, expected: true }, 700, 2).0, 1)));
    out.push(("danger: a forager with two allies is surprised", drive(build(&Case { seed: 2, temper: TEMPERS[3].1, allies: true, expected: false }, 700, 2).0, 2)));
    out.push(("danger: a cry for help to two out of sight", drive(build_cast(&Case { seed: 3, temper: TEMPERS[2].1, allies: true, expected: false }, &Cast { ally_dist: 16, calls: 700, ..Cast::default() }).0, 3)));
    out
}

/// A danger valley (Bryn, two allies, the boar) on `threads` threads with people stepped in `order`.
pub fn danger_order(threads: usize, order: &[usize]) -> u64 { danger_order_case(threads, order, false) }
/// The same, or (`call`) Bryn alone crying out to Cai and Dee out of sight.
pub fn danger_order_case(threads: usize, order: &[usize], call: bool) -> u64 {
    use crate::danger::runs::{build, build_cast, meet, Case, Cast, TEMPERS, WATCH};
    let mut v = if call { build_cast(&Case { seed: 6, temper: TEMPERS[2].1, allies: true, expected: false }, &Cast { ally_dist: 16, calls: 700, ..Cast::default() }).0 }
        else { build(&Case { seed: 5, temper: TEMPERS[0].1, allies: true, expected: true }, 700, 2).0 };
    meet(&mut v, 5, 8);
    for _ in 0..WATCH { v.step(threads, order); }
    v.state_hash()
}

/// The pinned hunt runs (stage 4): a planned hunt, the same people each hunting alone, and an
/// ambush of a person that one of the group gives away. Stepped on `threads` threads.
pub fn hunt_runs(threads: usize) -> Vec<(&'static str, u64)> {
    use crate::hunt::runs::{build, build_betrayal, BCase, Case, Lean, Mode, BETRAY_TICKS, HUNT_TICKS};
    let order: Vec<usize> = (0..4).collect();
    let drive = |mut v: crate::people::sim::Valley, ticks: u32| { for _ in 0..ticks { v.step(threads, &order); } v.state_hash() };
    vec![
        ("hunt: a planned hunt (seed 2)", drive(build(&Case { seed: 2, set: 1, mode: Mode::Planned }), HUNT_TICKS)),
        ("hunt: the same people, each alone (seed 2)", drive(build(&Case { seed: 2, set: 1, mode: Mode::Solo }), HUNT_TICKS)),
        ("hunt: an ambush given away (seed 1, Cai leans to Dee)", drive(build_betrayal(&BCase { seed: 1, set: 1, lean: Lean::Cai, caution: 10, honest_teller: false }), BETRAY_TICKS)),
        // (Review) A run where Ash overhears Bryn's whisper, so the wasm32 check covers it too.
        ("hunt: a careless teller overheard (seed 2, Bryn)", drive(build_betrayal(&BCase { seed: 2, set: 1, lean: Lean::Bryn, caution: 2, honest_teller: false }), BETRAY_TICKS)),
    ]
}

/// A hunt valley stepped on `threads` threads in `order`: a planned hunt (`betray` false) or the
/// betrayal (true). The hash of the world and every mind, the deer's included.
pub fn hunt_order(threads: usize, order: &[usize], betray: bool) -> u64 {
    use crate::hunt::runs::{build, build_betrayal, BCase, Case, Lean, Mode};
    let mut v = if betray { build_betrayal(&BCase { seed: 9, set: 2, lean: Lean::Bryn, caution: 10, honest_teller: false }) } else { build(&Case { seed: 7, set: 1, mode: Mode::Planned }) };
    for _ in 0..120 { v.step(threads, order); }
    v.state_hash()
}

/// The pinned village runs (stage 5): 30 people for two days in full detail and with the cheap
/// tier, and 50 people for a day with place pruning on. Every run on one thread (the village's
/// pool gives the same hash on any number: `tests/village_determinism.rs`).
pub fn village_runs() -> Vec<(&'static str, u64)> {
    use crate::village::runs::{run, Setup};
    let mut cheap = Setup::new(2, 30, 2); cheap.every = 4;
    let mut big = Setup::new(3, 50, 1); big.prune = true;
    // Round 2: every mind thinking every 4 ticks (the per-tick rates, buffered senses, reflexes).
    let mut slow = Setup::new(9, 20, 2); slow.think_every = 4;
    // Round 2, stage 3: a big brain every 4 ticks with a small brain between, and shallow thinking.
    let mut small = Setup::new(9, 20, 2); small.apply_mode("b4");
    let mut shallow = Setup::new(10, 20, 1); shallow.apply_mode("s634");
    // (Review: every tick did move in stage 3, by the told-water merge, which no earlier pin hit.)
    let merge = Setup::new(2, 30, 2);
    vec![
        ("village: 20 people, 2 days, every mind thinking every 4 ticks (seed 9)", run(&slow, 1, None).0.hash),
        ("village: 30 people, 2 days (seed 1)", run(&Setup::new(1, 30, 2), 1, None).0.hash),
        ("village: 30 people, 2 days, the cheap tier (seed 2)", run(&cheap, 1, None).0.hash),
        ("village: 50 people, a day, pruning on (seed 3)", run(&big, 1, None).0.hash),
        ("village: 20 people, 2 days, a big brain every 4 ticks and a small brain between (seed 9)", run(&small, 1, None).0.hash),
        ("village: 20 people, a day, every mind thinking shallowly (cut 634, seed 10)", run(&shallow, 1, None).0.hash),
        ("village: 30 people, 2 days, every tick, a told water merged into a known one (seed 2)", run(&merge, 1, None).0.hash),
    ]
}
