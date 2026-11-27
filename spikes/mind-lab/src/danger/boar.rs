//! The wild boar as a species, built the way the fox was: concepts and their links authored once in
//! a shared base; what an individual knows of the valley (its wallow) on its own overlay. It lives
//! in the people's valley and tells apart the same kinds of thing (`people::world::Kind`), each
//! mapped to one of its own concepts: any person is "people", the landmarks are "a thing".
//!
//! A boar is a dangerous animal by its make-up, not by a script: its temperament is aggressive, so
//! danger, pain and loss lead strongly to anger, and anger to fighting. It uses the same danger
//! responses as people (flee, fight, freeze, hold; it cannot call), and it charges what it fights.

use crate::graph::{lt, tag, Builder};
use crate::mind::{Geometry, KindInfo, Mind, Responses, Roles, Species, Temperament};
use crate::people::world::{Kind, World, GRID_H, GRID_W, PLACE_RADIUS, SIGHT};
use crate::rng::hash;
use crate::store::{Base, Runtime, Store};
use crate::think::Form;
use std::sync::{Arc, OnceLock};

pub const BOAR_SPECIES: u64 = 0xb0a7;
/// A boar's make-up: quick to react, very quick to anger.
pub const BOAR_TEMPER: Temperament = Temperament { reactivity: 600, aggression: 900 };

#[derive(Clone, Copy, Debug, Hash)]
pub struct BoarIds { pub hunger: u32, pub fatigue: u32, pub pain: u32, pub food: u32, pub danger: u32, pub rest: u32, pub anger: u32, pub loss: u32, pub comfort: u32,
    pub people: u32, pub kin: u32, pub thing: u32, pub water: u32, pub flee: u32, pub fight: u32, pub freeze: u32, pub hold: u32 }

pub struct Boars { pub species: Species, pub ids: BoarIds }

pub fn boars() -> Arc<Boars> {
    static B: OnceLock<Arc<Boars>> = OnceLock::new();
    B.get_or_init(|| Arc::new(build(1, false))).clone()
}

/// Stage 5: this species for the village (a valley of `n` people, berry bushes and foxes): the same
/// base, with a kinds table that can tell every person apart and sees bushes and foxes as things.
pub fn boars_for(n: usize) -> Arc<Boars> {
    static V: std::sync::Mutex<Vec<(usize, Arc<Boars>)>> = std::sync::Mutex::new(Vec::new());
    let mut v = V.lock().unwrap();
    if let Some(p) = v.iter().find(|x| x.0 == n) { return p.1.clone(); }
    let p = Arc::new(build(n, true));
    v.push((n, p.clone()));
    p
}

fn build(n: usize, village: bool) -> Boars {
    let t = Temperament::MEDIUM;
    let mut b = Builder::new(BOAR_SPECIES);
    let hunger = b.node("hunger", -200, tag::BODY, None);
    let fatigue = b.node("fatigue", -100, tag::BODY, None);
    let pain = b.node("pain", -600, tag::BODY, None);
    let food = b.node("roots", 300, tag::FOOD, None);
    let danger = b.node("danger", -700, tag::DANGER, None);
    let rest = b.node("rest", 300, tag::REST, None);
    let anger = b.node("anger", -200, tag::ANGER, None);
    let loss = b.node("loss", -500, 0, None);
    let comfort = b.node("comfort", 500, 0, None);
    let people = b.node("people", -300, tag::DANGER | tag::CREATURE, None);
    let kin = b.node("another boar", 200, 0, None);
    let thing = b.node("a thing", 0, 0, None);
    let water = b.node("water", 150, 0, None);
    let flee = b.node("run", -100, 0, None);
    let fight = b.node("fight", -100, tag::ANGER, None);
    let freeze = b.node("keep still", -200, 0, None);
    let hold = b.node("stand", 100, 0, None);
    for (u, v, ty, w) in crate::fox::found::temperament_links(&Roles { hunger, fatigue, pain, food, danger, rest, anger, loss, comfort, resp: None, hunger2: u32::MAX, food2: u32::MAX }, &t) {
        if ty == lt::ASSOC && u == danger { b.both(danger, anger, lt::ASSOC, w, 300); } else { b.link(u, v, ty, w); }
    }
    b.link(hunger, food, lt::WANTS, 800);
    b.link(fatigue, rest, lt::WANTS, 800);
    b.link(pain, danger, lt::WANTS, 700);
    b.both(rest, comfort, lt::ASSOC, 500, 400);
    b.link(people, danger, lt::KIND, 700);
    b.link(danger, flee, lt::WANTS, 500);
    b.link(anger, fight, lt::WANTS, 700);
    b.link(danger, freeze, lt::ASSOC, 150);
    b.link(danger, hold, lt::ASSOC, 250);
    b.canonical();
    let ids = BoarIds { hunger, fatigue, pain, food, danger, rest, anger, loss, comfort, people, kin, thing, water, flee, fight, freeze, hold };
    let resp = Responses { flee, fight, call: u32::MAX, freeze, hold, aid: u32::MAX, ally: u32::MAX, cry: u32::MAX };
    let roles = Roles { hunger, fatigue, pain, food, danger, rest, anger, loss, comfort, resp: Some(resp), hunger2: u32::MAX, food2: u32::MAX };
    let concept = |k: Kind| match k {
        Kind::Ash | Kind::Bryn | Kind::Cai | Kind::Dee => people, Kind::Boar => kin, Kind::Water => water,
        // Stage 5: in the village a boar forages at the berry bushes (its "roots").
        Kind::Berry => food, _ => thing,
    };
    let info = |k: Kind| KindInfo { name: k.name(), concept: concept(k), landmark: k.landmark(), edible: k == Kind::Berry, shelter: false,
        creature: matches!(k, Kind::Ash | Kind::Bryn | Kind::Cai | Kind::Dee), tracked: false };
    let kinds = crate::people::species::village_kinds(Kind::ALL.iter().map(|&k| info(k)).collect(), n, village, info(Kind::Berry), info(Kind::Fox));
    let species = Species {
        base: Arc::new(Base::compile(b)), roles, kinds: Arc::new(kinds), place_nodes: vec![], spare_places: 0, unlearnt: -999, near_weight: crate::people::species::near_weight,
        entity_nodes: vec![], entity_kinds: vec![], expect: vec![], geo: Geometry { sight: SIGHT, place_radius: PLACE_RADIUS, grid_w: GRID_W, grid_h: GRID_H },
        temperament: crate::fox::found::temperament_links, feelings: vec![food, danger, pain, loss, comfort, rest],
    };
    Boars { species, ids }
}

/// A boar's mind: its wallow at `(hx, hy)` (a place of its own, linked to rest and comfort).
pub fn mind(w: &World, b: usize, hx: i32, hy: i32) -> Mind<Runtime> { mind_in(&boars(), w, b, hx, hy, true) }
/// A boar's mind of a given boar species (stage 5: the village's), hunted or not.
pub fn mind_in(sp: &Boars, w: &World, b: usize, hx: i32, hy: i32, hunted: bool) -> Mind<Runtime> {
    let mut m = Mind::new(&sp.species, BOAR_TEMPER, hash(w.seed, b as u64, 0, 0xb0a7));
    m.charges = true;
    let pi = m.add_place("wallow", hx, hy, None);
    let pn = m.place_nodes[pi];
    m.store.set_val(pn, 400);
    for (a, c, ab, ba) in [(pn, sp.ids.rest, 600, 600), (pn, sp.ids.comfort, 700, 500)] { m.store.add_link(a, c, lt::ASSOC, ab, 0); m.store.add_link(c, a, lt::ASSOC, ba, 0); }
    // This boar has been hunted: people make it angry (its history, authored as founding knowledge
    // of its own, like a person's trust).
    if hunted { m.store.add_link(sp.ids.people, sp.ids.anger, lt::ASSOC, HUNTED, 0); }
    m.young_from = m.store.len() as u32;
    m
}
/// Stage 5: a village boar also knows bushes where it forages (places holding its food).
pub fn forager(sp: &Boars, w: &World, b: usize, hx: i32, hy: i32, bushes: &[(i32, i32)]) -> Mind<Runtime> {
    let mut m = mind_in(sp, w, b, hx, hy, false);
    for (bi, &(x, y)) in bushes.iter().enumerate() {
        let pi = m.add_place(&format!("bush {}", bi + 1), x, y, None);
        let pn = m.place_nodes[pi];
        for (a, c, ty, ab) in [(pn, sp.ids.food, lt::HOLDS, 700), (sp.ids.food, pn, lt::ASSOC, 500)] { m.store.add_link(a, c, ty, ab, 0); }
        m.expect[pi][crate::mind::key(Kind::Berry.k(), false)] = 900;
    }
    m.young_from = m.store.len() as u32;
    m
}
/// How strongly a hunted boar's thought of people runs to anger.
pub const HUNTED: i32 = 600;
