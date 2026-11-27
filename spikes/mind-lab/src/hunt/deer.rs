//! The deer as a species, built the way the fox and the boar were: concepts and their links
//! authored once in a shared base; what an individual knows of the valley (the meadows it grazes)
//! on its own overlay. It lives in the people's valley and tells apart the same kinds of thing
//! (`people::world::Kind`), each mapped to one of its own concepts: any person is "people".
//!
//! A deer is prey by its make-up, not by a script: its temperament is reactive and mild, people
//! are a kind of danger to it, and danger runs strongly to running away. It grows hungry and its
//! thought of hunger runs to grass, which its meadows hold. It uses the shared danger responses
//! (flee, freeze, hold; it cannot call, and barely fights). What it learns, it learns through the
//! shared mind's ordinary machinery: places where it met people take on the feeling of danger
//! (fast weights on its places' empty `feels` slots, kept by sleep), and its expectations of
//! seeing people at a place grow when it sees them there.

use crate::graph::{lt, tag, Builder};
use crate::mind::{Geometry, KindInfo, Mind, Responses, Roles, Species, Temperament};
use crate::people::world::{Kind, World, GRID_H, GRID_W, PLACE_RADIUS, SIGHT};
use crate::rng::hash;
use crate::store::{Base, Runtime, Store};
use crate::think::Form;
use std::sync::{Arc, OnceLock};

pub const DEER_SPECIES: u64 = 0xdee7;
/// A deer's make-up: very quick to react, slow to anger.
pub const DEER_TEMPER: Temperament = Temperament { reactivity: 800, aggression: 100 };

#[derive(Clone, Copy, Debug, Hash)]
pub struct DeerIds { pub hunger: u32, pub fatigue: u32, pub pain: u32, pub grass: u32, pub danger: u32, pub rest: u32, pub anger: u32, pub loss: u32, pub comfort: u32,
    pub people: u32, pub kin: u32, pub thing: u32, pub water: u32, pub flee: u32, pub fight: u32, pub freeze: u32, pub hold: u32 }

pub struct DeerSpecies { pub species: Species, pub ids: DeerIds }

pub fn deer() -> Arc<DeerSpecies> {
    static D: OnceLock<Arc<DeerSpecies>> = OnceLock::new();
    D.get_or_init(|| Arc::new(build(1, false))).clone()
}

/// Stage 5: this species for the village (a valley of `n` people, berry bushes and foxes): the same
/// base, with a kinds table that can tell every person apart and sees bushes and foxes as things.
pub fn deer_for(n: usize) -> Arc<DeerSpecies> {
    static V: std::sync::Mutex<Vec<(usize, Arc<DeerSpecies>)>> = std::sync::Mutex::new(Vec::new());
    let mut v = V.lock().unwrap();
    if let Some(p) = v.iter().find(|x| x.0 == n) { return p.1.clone(); }
    let p = Arc::new(build(n, true));
    v.push((n, p.clone()));
    p
}

fn build(n: usize, village: bool) -> DeerSpecies {
    let t = Temperament::MEDIUM;
    let mut b = Builder::new(DEER_SPECIES);
    let hunger = b.node("hunger", -200, tag::BODY, None);
    let fatigue = b.node("fatigue", -100, tag::BODY, None);
    let pain = b.node("pain", -600, tag::BODY, None);
    let grass = b.node("grass", 300, tag::FOOD, None);
    let danger = b.node("danger", -700, tag::DANGER, None);
    let rest = b.node("rest", 300, tag::REST, None);
    let anger = b.node("anger", -200, tag::ANGER, None);
    let loss = b.node("loss", -500, 0, None);
    let comfort = b.node("comfort", 500, 0, None);
    let people = b.node("people", -300, tag::DANGER | tag::CREATURE, None);
    let kin = b.node("another deer", 300, 0, None);
    let thing = b.node("a thing", 0, 0, None);
    let water = b.node("water", 150, 0, None);
    let flee = b.node("run", -100, 0, None);
    let fight = b.node("fight", -100, tag::ANGER, None);
    let freeze = b.node("keep still", -200, 0, None);
    let hold = b.node("stand", 100, 0, None);
    for (u, v, ty, w) in crate::fox::found::temperament_links(&Roles { hunger, fatigue, pain, food: grass, danger, rest, anger, loss, comfort, resp: None, hunger2: u32::MAX, food2: u32::MAX }, &t) {
        if ty == lt::ASSOC && u == danger { b.both(danger, anger, lt::ASSOC, w, 300); } else { b.link(u, v, ty, w); }
    }
    b.link(hunger, grass, lt::WANTS, 800);
    b.link(fatigue, rest, lt::WANTS, 800);
    b.link(pain, danger, lt::WANTS, 700);
    b.both(rest, comfort, lt::ASSOC, 500, 400);
    b.link(people, danger, lt::KIND, 700);
    b.link(danger, flee, lt::WANTS, 600);
    b.link(anger, fight, lt::WANTS, 300);
    b.link(danger, freeze, lt::ASSOC, 250);
    b.link(danger, hold, lt::ASSOC, 50);
    b.link(kin, comfort, lt::ASSOC, 300);
    b.canonical();
    let ids = DeerIds { hunger, fatigue, pain, grass, danger, rest, anger, loss, comfort, people, kin, thing, water, flee, fight, freeze, hold };
    let resp = Responses { flee, fight, call: u32::MAX, freeze, hold, aid: u32::MAX, ally: u32::MAX, cry: u32::MAX };
    let roles = Roles { hunger, fatigue, pain, food: grass, danger, rest, anger, loss, comfort, resp: Some(resp), hunger2: u32::MAX, food2: u32::MAX };
    let concept = |k: Kind| match k {
        Kind::Ash | Kind::Bryn | Kind::Cai | Kind::Dee => people, Kind::Deer => kin, Kind::Water => water, _ => thing,
    };
    let info = |k: Kind| KindInfo { name: k.name(), concept: concept(k), landmark: k.landmark(), edible: false, shelter: false,
        creature: matches!(k, Kind::Ash | Kind::Bryn | Kind::Cai | Kind::Dee), tracked: false };
    let kinds = crate::people::species::village_kinds(Kind::ALL.iter().map(|&k| info(k)).collect(), n, village, info(Kind::Berry), info(Kind::Fox));
    let species = Species {
        base: Arc::new(Base::compile(b)), roles, kinds: Arc::new(kinds), place_nodes: vec![], spare_places: 0, unlearnt: -999, near_weight: crate::people::species::near_weight,
        entity_nodes: vec![], entity_kinds: vec![], expect: vec![], geo: Geometry { sight: SIGHT, place_radius: PLACE_RADIUS, grid_w: GRID_W, grid_h: GRID_H },
        temperament: crate::fox::found::temperament_links, feelings: vec![grass, danger, pain, loss, comfort, rest],
    };
    DeerSpecies { species, ids }
}

/// A deer's mind: the meadows it grazes (places of its own, holding grass), at the world's
/// meadows (it has lived there all its life, so it knows them exactly).
pub fn mind(w: &World, k: usize) -> Mind<Runtime> { mind_in(&deer(), w, k) }
/// A deer's mind of a given deer species (stage 5: the village's).
pub fn mind_in(sp: &DeerSpecies, w: &World, k: usize) -> Mind<Runtime> {
    let mut m = Mind::new(&sp.species, DEER_TEMPER, hash(w.seed, k as u64, 0, 0xdee7));
    m.cfg.flee_from_all = true;
    for (mi, &(x, y)) in w.meadows.iter().enumerate() {
        let pi = m.add_place(&format!("meadow {}", mi + 1), x, y, None);
        let pn = m.place_nodes[pi];
        m.store.set_val(pn, 300);
        for (a, c, ty, ab) in [(pn, sp.ids.grass, lt::HOLDS, 700), (sp.ids.grass, pn, lt::ASSOC, 500), (pn, sp.ids.comfort, lt::ASSOC, 400)] { m.store.add_link(a, c, ty, ab, 0); }
    }
    m.young_from = m.store.len() as u32;
    m
}

/// How strongly deer `m` has come to feel danger at each of its meadows (its own learnt `feels`
/// link, slow + fast), for measures.
pub fn meadow_danger(m: &Mind<Runtime>) -> Vec<i32> {
    let d = deer().ids.danger;
    m.place_nodes.iter().map(|&pn| m.store.links_vec(pn).iter().filter(|l| l.to == d).map(|l| l.slow as i32 + l.fast as i32).max().unwrap_or(0)).collect()
}
