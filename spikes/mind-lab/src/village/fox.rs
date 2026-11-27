//! A fox for the village, built the way the deer and the boar were: concepts and links in a shared
//! base; its den and the bushes it knows on its own overlay. It lives in the people's valley and
//! tells apart the same kinds of thing (`people::world::Kind`), each mapped to one of its own
//! concepts: any person is "people", a berry bush is "berries".
//!
//! It is a forager by its make-up, not by a script: hunger runs to berries, which its bushes hold,
//! so it competes with the villagers for the same bushes. People are a kind of danger to it (less
//! than to a deer), and danger runs to running away. (Not the original fox of stages 0 and 1, whose
//! pinned runs live in `crate::fox`: this one shares only the shape.)

use crate::graph::{lt, tag, Builder};
use crate::mind::{Geometry, KindInfo, Mind, Responses, Roles, Species, Temperament};
use crate::people::world::{Kind, World, GRID_H, GRID_W, PLACE_RADIUS, SIGHT};
use crate::rng::hash;
use crate::store::{Base, Runtime, Store};
use crate::think::Form;
use std::sync::Arc;

pub const FOX_SPECIES: u64 = 0xf0c5;
pub const FOX_TEMPER: Temperament = Temperament { reactivity: 600, aggression: 300 };

#[derive(Clone, Copy, Debug, Hash)]
pub struct FoxIds { pub hunger: u32, pub fatigue: u32, pub pain: u32, pub berries: u32, pub danger: u32, pub rest: u32, pub anger: u32, pub loss: u32, pub comfort: u32,
    pub people: u32, pub kin: u32, pub thing: u32, pub water: u32, pub beast: u32, pub flee: u32, pub fight: u32, pub freeze: u32, pub hold: u32 }

pub struct Foxes { pub species: Species, pub ids: FoxIds }

/// The village fox species for a valley of `n` people (built once per `n`).
pub fn foxes(n: usize) -> Arc<Foxes> {
    static V: std::sync::Mutex<Vec<(usize, Arc<Foxes>)>> = std::sync::Mutex::new(Vec::new());
    let mut v = V.lock().unwrap();
    if let Some(p) = v.iter().find(|x| x.0 == n) { return p.1.clone(); }
    let p = Arc::new(build(n));
    v.push((n, p.clone()));
    p
}

fn build(n: usize) -> Foxes {
    let t = Temperament::MEDIUM;
    let mut b = Builder::new(FOX_SPECIES);
    let hunger = b.node("hunger", -200, tag::BODY, None);
    let fatigue = b.node("fatigue", -100, tag::BODY, None);
    let pain = b.node("pain", -600, tag::BODY, None);
    let berries = b.node("berries", 300, tag::FOOD, None);
    let danger = b.node("danger", -700, tag::DANGER, None);
    let rest = b.node("rest", 300, tag::REST, None);
    let anger = b.node("anger", -200, tag::ANGER, None);
    let loss = b.node("loss", -500, 0, None);
    let comfort = b.node("comfort", 500, 0, None);
    let people = b.node("people", -200, tag::DANGER | tag::CREATURE, None);
    let kin = b.node("another fox", 100, 0, None);
    let thing = b.node("a thing", 0, 0, None);
    let water = b.node("water", 150, 0, None);
    let beast = b.node("a big beast", -400, tag::DANGER | tag::CREATURE, None);
    let flee = b.node("run", -100, 0, None);
    let fight = b.node("fight", -100, tag::ANGER, None);
    let freeze = b.node("keep still", -200, 0, None);
    let hold = b.node("stand", 100, 0, None);
    for (u, v, ty, w) in crate::fox::found::temperament_links(&Roles { hunger, fatigue, pain, food: berries, danger, rest, anger, loss, comfort, resp: None, hunger2: u32::MAX, food2: u32::MAX }, &t) {
        if ty == lt::ASSOC && u == danger { b.both(danger, anger, lt::ASSOC, w, 300); } else { b.link(u, v, ty, w); }
    }
    b.link(hunger, berries, lt::WANTS, 800);
    b.link(fatigue, rest, lt::WANTS, 800);
    b.link(pain, danger, lt::WANTS, 700);
    b.both(rest, comfort, lt::ASSOC, 500, 400);
    b.link(people, danger, lt::KIND, 500);
    b.link(beast, danger, lt::KIND, 700);
    b.link(danger, flee, lt::WANTS, 600);
    b.link(anger, fight, lt::WANTS, 300);
    b.link(danger, freeze, lt::ASSOC, 200);
    b.link(danger, hold, lt::ASSOC, 50);
    b.canonical();
    let ids = FoxIds { hunger, fatigue, pain, berries, danger, rest, anger, loss, comfort, people, kin, thing, water, beast, flee, fight, freeze, hold };
    let resp = Responses { flee, fight, call: u32::MAX, freeze, hold, aid: u32::MAX, ally: u32::MAX, cry: u32::MAX };
    let roles = Roles { hunger, fatigue, pain, food: berries, danger, rest, anger, loss, comfort, resp: Some(resp), hunger2: u32::MAX, food2: u32::MAX };
    let person = |k: Kind| matches!(k, Kind::Ash | Kind::Bryn | Kind::Cai | Kind::Dee);
    let concept = |k: Kind| match k { _ if person(k) => people, Kind::Fox => kin, Kind::Water => water, Kind::Berry => berries, Kind::Boar => beast, _ => thing };
    let info = |k: Kind| KindInfo { name: k.name(), concept: concept(k), landmark: k.landmark(), edible: k == Kind::Berry, shelter: false,
        creature: person(k) || k == Kind::Boar, tracked: false };
    let kinds = crate::people::species::village_kinds(Kind::ALL.iter().map(|&k| info(k)).collect(), n, true, info(Kind::Berry), info(Kind::Fox));
    let species = Species {
        base: Arc::new(Base::compile(b)), roles, kinds: Arc::new(kinds), place_nodes: vec![], spare_places: 0, unlearnt: -999, near_weight: crate::people::species::near_weight,
        entity_nodes: vec![], entity_kinds: vec![], expect: vec![], geo: Geometry { sight: SIGHT, place_radius: PLACE_RADIUS, grid_w: GRID_W, grid_h: GRID_H },
        temperament: crate::fox::found::temperament_links, feelings: vec![berries, danger, pain, loss, comfort, rest],
    };
    Foxes { species, ids }
}

/// A fox's mind: its den (a place of its own, linked to rest and comfort) and the bushes it knows
/// (places holding berries, where it expects to see a bush).
pub fn mind(sp: &Foxes, w: &World, k: usize, den: (i32, i32), bushes: &[(i32, i32)]) -> Mind<Runtime> {
    let mut m = Mind::new(&sp.species, FOX_TEMPER, hash(w.seed, k as u64, 0, 0xf0c5));
    let pi = m.add_place("den", den.0, den.1, None);
    let pn = m.place_nodes[pi];
    m.store.set_val(pn, 400);
    for (a, c, ab, ba) in [(pn, sp.ids.rest, 600, 600), (pn, sp.ids.comfort, 700, 500)] { m.store.add_link(a, c, lt::ASSOC, ab, 0); m.store.add_link(c, a, lt::ASSOC, ba, 0); }
    for (bi, &(x, y)) in bushes.iter().enumerate() {
        let pi = m.add_place(&format!("bush {}", bi + 1), x, y, None);
        let pn = m.place_nodes[pi];
        for (a, c, ty, ab) in [(pn, sp.ids.berries, lt::HOLDS, 700), (sp.ids.berries, pn, lt::ASSOC, 500)] { m.store.add_link(a, c, ty, ab, 0); }
        m.expect[pi][crate::mind::key(Kind::Berry.k(), false)] = 900;
    }
    m.young_from = m.store.len() as u32;
    m
}
