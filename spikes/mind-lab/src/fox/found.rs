//! The fox as a species: its founding graph, built from the valley it lives in, compiled once into
//! a shared [`Base`]. Ported from `spikes/thought-graph` (`fox.rs`). Concepts and their links are
//! authored; places, the things at them, and how places link are read from the world:
//! - **near** links from real distance between every pair of places;
//! - **like** links between places that hold the same kinds of thing;
//! - **holds / at** links between a place and each thing within `PLACE_RADIUS` of it.
//!
//! Every place and thing also gets empty `feels` slots (weight 0), and the base has
//! `SPARE_PLACES` unlearnt place nodes, so experience has somewhere to write. (A mind can also
//! add nodes and links of its own; the fox keeps its slots so its results stay comparable.)
//!
//! **Temperament** sets three founding links (danger, pain and loss to anger). The base holds them
//! for a medium fox; an individual whose own weights differ keeps them in its overlay.

use super::world::{dist, Kind, World, GRID_H, GRID_W, PLACE_RADIUS, SIGHT};
use crate::graph::{lt, tag, Anchor, Builder};
use crate::mind::{Geometry, KindInfo, Mind, Roles, Species, Temperament};
use crate::store::{Base, Runtime, Store};
use std::sync::Arc;

pub const NEAR_RANGE: i32 = 48;
pub const SPARE_PLACES: usize = 4;
pub const UNLEARNT: i16 = -999;
/// The seed of the fox species' stable node keys.
pub const FOX_SPECIES: u64 = 0xf0c5;
pub fn near_weight(d: i32) -> i32 { if d < NEAR_RANGE { 600 * (NEAR_RANGE - d) / NEAR_RANGE } else { 0 } }

/// Ids of the fox's concept nodes.
#[derive(Clone, Copy, Debug, Hash)]
pub struct Ids { pub hunger: u32, pub fatigue: u32, pub pain: u32, pub food: u32, pub water: u32, pub fire: u32, pub danger: u32, pub rest: u32, pub comfort: u32, pub anger: u32, pub loss: u32 }

fn kind_val(k: Kind) -> i32 { match k { Kind::Hen => 500, Kind::Berry => 250, Kind::Water => 200, Kind::Dog => -300, Kind::Fire => -150, Kind::Den => 300 } }
fn kind_tags(k: Kind) -> u8 {
    match k { Kind::Hen | Kind::Berry => tag::FOOD, Kind::Dog => tag::DANGER | tag::CREATURE, Kind::Fire => tag::DANGER, Kind::Water => 0, Kind::Den => tag::REST }
}
pub fn kind_concept(ids: &Ids, k: Kind) -> u32 {
    match k { Kind::Hen | Kind::Berry => ids.food, Kind::Water => ids.water, Kind::Fire => ids.fire, Kind::Dog => ids.danger, Kind::Den => ids.rest }
}

/// The three links temperament sets.
pub fn temperament_links(r: &Roles, t: &Temperament) -> Vec<(u32, u32, u8, i32)> {
    vec![(r.loss, r.anger, lt::WANTS, 100 + t.aggression / 4), (r.danger, r.anger, lt::ASSOC, 100 + t.aggression * 6 / 10), (r.pain, r.anger, lt::WANTS, 100 + t.aggression / 2)]
}

/// The fox species of a valley: the founding graph (authored, unquantised) and the species.
pub struct Fox { pub species: Species, pub ids: Ids, pub founding: Builder }

/// Build the fox species from a world (its places and the things in them).
pub fn species(w: &World) -> Fox {
    let t = Temperament::MEDIUM;
    let mut b = Builder::new(FOX_SPECIES);
    let hunger = b.node("hunger", -200, tag::BODY, None);
    let fatigue = b.node("fatigue", -100, tag::BODY, None);
    let pain = b.node("pain", -600, tag::BODY, None);
    let food = b.node("food", 300, tag::FOOD, None);
    let water = b.node("water", 150, 0, None);
    let fire = b.node("fire", -300, tag::DANGER, None);
    let danger = b.node("danger", -700, tag::DANGER, None);
    let rest = b.node("rest", 300, tag::REST, None);
    let comfort = b.node("comfort", 500, 0, None);
    let anger = b.node("anger", -200, tag::ANGER, None);
    let loss = b.node("loss", -500, 0, None);
    let ids = Ids { hunger, fatigue, pain, food, water, fire, danger, rest, comfort, anger, loss };
    let roles = Roles { hunger, fatigue, pain, food, danger, rest, anger, loss, comfort };
    b.link(loss, anger, lt::WANTS, 100 + t.aggression / 4);
    b.both(danger, anger, lt::ASSOC, 100 + t.aggression * 6 / 10, 300);
    b.link(pain, anger, lt::WANTS, 100 + t.aggression / 2);
    b.link(hunger, food, lt::WANTS, 800);
    b.link(fatigue, rest, lt::WANTS, 800);
    b.link(pain, danger, lt::WANTS, 700);
    b.both(fire, danger, lt::KIND, 700, 300);
    b.both(rest, comfort, lt::ASSOC, 500, 400);

    let place_nodes: Vec<u32> = w.places.iter().map(|p| b.node(p.name, if p.home { 400 } else { 0 }, tag::PLACE, Some(Anchor::Place { x: p.x as i16, y: p.y as i16 }))).collect();
    let mut entity_nodes = vec![];
    let mut entity_kinds = vec![];
    let mut holds: Vec<Vec<u32>> = vec![vec![]; w.places.len()];
    let mut expect = vec![];
    for e in w.entities.iter().filter(|e| e.alive) {
        let n = b.node(&e.name, kind_val(e.kind), kind_tags(e.kind), Some(Anchor::Entity { id: e.id, x: e.x as i16, y: e.y as i16 }));
        entity_nodes.push((e.id, n));
        entity_kinds.push(e.kind.k());
        let c = kind_concept(&ids, e.kind);
        match e.kind { Kind::Dog | Kind::Fire => b.both(n, c, lt::KIND, 700, 400), _ => b.both(n, c, lt::KIND, 700, 600) }
        if let Some(pi) = w.place_at(e.x, e.y) {
            b.link(place_nodes[pi], n, lt::HOLDS, 700);
            b.link(n, place_nodes[pi], lt::AT, 800);
            holds[pi].push(c);
            expect.push((pi, e.kind.k()));
        }
        b.both(n, pain, lt::FEELS, 0, 0);
        b.both(n, danger, lt::FEELS, 0, 0);
        if e.kind == Kind::Dog { b.both(n, anger, lt::FEELS, 0, 0); }
    }
    let spares: Vec<u32> = (0..SPARE_PLACES).map(|i| b.node(&format!("new place {}", i + 1), 0, tag::PLACE, Some(Anchor::Place { x: UNLEARNT, y: UNLEARNT }))).collect();
    let feelings = [food, water, fire, danger, rest, pain, comfort, loss];
    for &sp in &spares {
        for &c in &feelings { b.both(sp, c, lt::FEELS, 0, 0); }
        for &q in place_nodes.iter().chain(&spares) { if q != sp { b.both(sp, q, lt::NEAR, 0, 0); } }
    }
    for (i, p) in w.places.iter().enumerate() {
        let pn = place_nodes[i];
        if p.home { b.both(pn, rest, lt::ASSOC, 600, 600); b.both(pn, comfort, lt::ASSOC, 700, 500); }
        for &c in &feelings { b.both(pn, c, lt::FEELS, 0, 0); }
        for (j, q) in w.places.iter().enumerate() {
            if i == j { continue; }
            let d = dist(p.x, p.y, q.x, q.y);
            if d < NEAR_RANGE { b.link(pn, place_nodes[j], lt::NEAR, near_weight(d)); }
            let shared = holds[i].iter().filter(|c| holds[j].contains(c)).count() as i32;
            if shared > 0 { b.link(pn, place_nodes[j], lt::LIKE, 300 * shared); }
        }
    }
    let mut place_nodes = place_nodes;
    place_nodes.extend(spares);
    b.canonical();
    let kinds = Kind::ALL.iter().map(|&k| KindInfo { name: k.name(), concept: kind_concept(&ids, k), landmark: k.landmark(), edible: matches!(k, Kind::Hen | Kind::Berry), shelter: k == Kind::Den, creature: k == Kind::Dog, tracked: false }).collect();
    let species = Species {
        base: Arc::new(Base::compile(b.clone())), roles, kinds: Arc::new(kinds), place_nodes, spare_places: SPARE_PLACES, unlearnt: UNLEARNT, near_weight,
        entity_nodes, entity_kinds, expect, geo: Geometry { sight: SIGHT, place_radius: PLACE_RADIUS, grid_w: GRID_W, grid_h: GRID_H }, temperament: temperament_links,
        feelings: feelings.to_vec(),
    };
    Fox { species, ids, founding: b }
}

impl Fox {
    /// A fox of a temperament, on the runtime form (shared base + overlay).
    pub fn mind(&self, t: Temperament, who: u64) -> Mind<Runtime> { Mind::new(&self.species, t, who) }
    /// Ablation: the same fox with its founding weights exact (unquantised), held in its overlay.
    pub fn mind_exact(&self, t: Temperament, who: u64) -> Mind<Runtime> {
        let mut m = Mind::new(&self.species, t, who);
        let mut pos = vec![0usize; self.founding.len()];
        for &(u, _, _, w, _) in &self.founding.links {
            let k = crate::graph::link_key(u, pos[u as usize]);
            pos[u as usize] += 1;
            if m.store.slow(k) != w { m.store.set_slow(k, w); }
        }
        for (u, v, ty, w) in temperament_links(&m.roles, &t) { let k = m.store.find(u, v, ty).unwrap(); m.store.set_slow(k, w as i16); }
        m
    }
}
