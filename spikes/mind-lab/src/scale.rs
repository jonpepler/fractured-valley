//! Big minds for measuring storage and cost: the fox's founding graph plus a generated body of
//! knowledge, compiled into one species base, so the real fox scenarios (senses, thinking,
//! learning, consolidation, sleep) run on minds of 10k and 100k nodes.
//!
//! The generator follows `spikes/thought-graph` (`scale.rs`): communities of 64 nodes, each node
//! with 6–18 out-links (80% inside its community, 15% to the next, 5% anywhere), weights
//! 100–900, 10% food, 5% danger, 5% rest, 40% with a valence. The generated nodes are open
//! connectors (concepts: no places, no anchors), so they can colour thought but never become
//! somewhere to walk. Each of the fox's concepts links out to 8 generated nodes, and each community
//! links back to two concepts (so the fox's own links keep their share of what a concept sends).

use crate::fox::found::{self, Fox};
use crate::fox::world::World;
use crate::graph::{lt, tag};
use crate::rng::hash;
use crate::store::Base;
use std::sync::Arc;

pub const COMMUNITY: u64 = 64;

/// The fox species with `extra` generated nodes of knowledge added to its founding graph.
pub fn big_fox(extra: usize, seed: u64) -> Arc<Fox> {
    let mut fox = found::species(&World::valley(0));
    if extra == 0 { return Arc::new(fox); }
    let b = &mut fox.founding;
    let n0 = b.len() as u64;
    let h = |i: u64, salt: u64| hash(seed, i, salt, 0x5ca1e);
    for i in 0..extra as u64 {
        let t = h(i, 4) % 100;
        let tags = if t < 10 { tag::FOOD } else if t < 15 { tag::DANGER } else if t < 20 { tag::REST } else { 0 };
        let val = if h(i, 5) % 100 < 40 { (h(i, 6) % 1401) as i32 - 700 } else { 0 };
        b.node(&format!("k{i}"), val, tags, None);
    }
    let comms = (extra as u64).div_ceil(COMMUNITY);
    let types = [lt::ASSOC, lt::KIND, lt::ASSOC, lt::FEELS];
    for i in 0..extra as u64 {
        let deg = 6 + h(i, 7) % 13;
        let c = i / COMMUNITY;
        for k in 0..deg {
            let r = h(i, 100 + k) % 100;
            let tc = if r < 80 { c } else if r < 95 { (c + 1) % comms } else { h(i, 200 + k) % comms };
            let lo = tc * COMMUNITY;
            let hi = ((tc + 1) * COMMUNITY).min(extra as u64);
            let j = lo + h(i, 300 + k) % (hi - lo).max(1);
            if j == i { continue; }
            b.link((n0 + i) as u32, (n0 + j) as u32, types[(h(i, 400 + k) % 4) as usize], 100 + (h(i, 500 + k) % 801) as i32);
        }
    }
    // Bridges. Each of the fox's concepts links out to 8 generated nodes (so its own links keep
    // their share of what it sends), and each community links back to two concepts.
    let ids = fox.ids;
    let concepts = [ids.hunger, ids.fatigue, ids.pain, ids.food, ids.water, ids.fire, ids.danger, ids.rest, ids.comfort, ids.anger, ids.loss];
    for (ci, &fc) in concepts.iter().enumerate() {
        for k in 0..8u64 {
            let j = (n0 + h(ci as u64, 650 + k) % extra as u64) as u32;
            b.link(fc, j, lt::ASSOC, 100 + (h(ci as u64, 660 + k) % 300) as i32);
        }
    }
    for c in 0..comms {
        for k in 0..2 {
            let fc = concepts[(h(c, 600 + k) % concepts.len() as u64) as usize];
            let lo = c * COMMUNITY;
            let hi = ((c + 1) * COMMUNITY).min(extra as u64);
            let j = (n0 + lo + h(c, 700 + k) % (hi - lo)) as u32;
            b.link(j, fc, lt::ASSOC, 100 + (h(c, 900 + k) % 300) as i32);
        }
    }
    b.canonical();
    fox.species.base = Arc::new(Base::compile(b.clone()));
    Arc::new(fox)
}
