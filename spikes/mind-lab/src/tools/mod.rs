//! Round 2's toolkit (decision 9.10): tools a mind makes and uses instead of thinking something
//! through. A tool is **data** (a description in a small, typed, integer language) run by one
//! deterministic, fuel-bounded interpreter. The foundational tools, the schedule alternatives
//! and the tool-maker are all written in it; the tool-maker builds new descriptions from the
//! questions a mind keeps asking and what it has seen; the belt is the brain-tick interface.
//!
//! - `lang`: the description language (types, text and binary forms, hashing, plain words).
//! - `run`: the interpreter (state, fuel).
//! - `kit`: the foundational set, the schedule alternatives and the makers, as text.
//! - `cost`: static worst-case cost from a description, and which brain may reach for what.
//! - `maker`: the tool-maker's search (the `make` primitive).
//! - `ash`: the brain-tick demo (Bryn finds Ash, in a real village).
//! - `belt`: a mind's tools and the calls a brain tick makes (`tell`, `ask`, `bind`, `tick`, `sleep`).
//! - `routes`: journeys in a generated world, and the maps minds make of them.
//! - `data`: observation streams (a generated routine valley, and a real village run).
//! - `study`: scoring tools against the truth.

pub mod ash;
pub mod belt;
pub mod cost;
pub mod data;
pub mod kit;
pub mod lang;
pub mod maker;
pub mod routes;
pub mod run;
pub mod study;

/// The toolkit's determinism runs: (name, hash). The first four use only generated worlds (pinned
/// in the tests); the last two run a real village (compared across builds, not pinned, as the
/// village's own code moves).
pub fn hashes() -> Vec<(&'static str, u64)> {
    use std::hash::{Hash, Hasher};
    let mut out = vec![];
    let st = data::routine(1, 8, 10, &[1, 2, 3], 100);
    let descs: Vec<lang::Desc> = kit::SCHEDULES.iter().map(|s| kit::load(s)).collect();
    let mut h = crate::rng::StateHasher::new(0x7a51);
    for s in study::schedules(&st, &descs, None, None, false) { h.write_u64(s.hash); }
    out.push(("tools: six schedule tools, routine valley, 10 days", h.finish()));
    let st = data::routine(2, 8, 16, &[1, 2], 100);
    let mut h = crate::rng::StateHasher::new(0x7a52);
    for r in study::with_maker(&st, kit::MAKER, 0, 400) { r.hash(&mut h); }
    out.push(("tools: the tool-maker on a routine valley, 16 days", h.finish()));
    let w = routes::world(1, &routes::commuter_goals(1));
    let mut h = crate::rng::StateHasher::new(0x7a53);
    for v in [routes::Variant::Kit, routes::Variant::Maker] { h.write_u64(routes::travel(1, 2, routes::Habit::Roamer, 15, v, &w).hash); }
    out.push(("tools: a roamer's journeys and maps, 15 days", h.finish()));
    let mut h = crate::rng::StateHasher::new(0x7a54);
    for src in kit::FOUNDATION.iter().chain(kit::SCHEDULES.iter()).chain([kit::MAKER, kit::FRUGAL_MAKER, kit::GO_FIND].iter()) { let d = kit::load(src); h.write_u64(d.digest()); h.write_u64(cost::bound(&d, &[]).ask as u64); }
    out.push(("tools: every kit description and its static bound", h.finish()));
    let st = data::village(3, 10, 3, &[1, 2], 4);
    let mut h = crate::rng::StateHasher::new(0x7a55);
    for s in study::schedules(&st, &descs, None, None, false) { h.write_u64(s.hash); }
    out.push(("tools (village): six schedule tools, 3 days", h.finish()));
    let d = ash::run(2, 12, 12, kit::MAKER);
    out.push(("tools (village): Bryn and Ash, 12 days", d.belt_hash ^ d.qs.iter().fold(0u64, |a, q| a.rotate_left(5) ^ q.think_cost as u64 ^ (q.tool_fuel as u64) << 20)));
    out
}
