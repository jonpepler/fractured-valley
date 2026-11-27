//! People as a species. The shared base holds only **concepts** (thirst, water, rest, trust, lie,
//! mistake, a debt, a note…): what every person's mind has the same. Everything a person knows
//! about the valley and about the others (places, where the landmarks are, the stream, each
//! other person, how far they trust them) is founded on that person's **own overlay**, with
//! their own positions, so two minds never share a node. Places a person learns or is told of
//! are new nodes of its own (the species has no spare place slots: places grow).
//!
//! Roles: the mind's "hunger" is thirst and its "food" is water, so a thirsty person's thought
//! runs to water exactly as a hungry fox's runs to food.

use super::world::{person_id, Kind, World, GRID_H, GRID_W, NAMES, PEOPLE, PLACE_RADIUS, SIGHT, CAMP};
use crate::graph::{lt, tag, Anchor, Builder};
use crate::mind::{key, Geometry, KindInfo, Mind, Roles, Species, Temperament};
use crate::rng::hash;
use crate::store::{Base, Runtime, Store};
use crate::think::Form;
use std::sync::{Arc, OnceLock};

pub const PEOPLE_SPECIES: u64 = 0x9e0f1e;
pub const NEAR_RANGE: i32 = 48;
pub fn near_weight(d: i32) -> i32 { if d < NEAR_RANGE { 600 * (NEAR_RANGE - d) / NEAR_RANGE } else { 0 } }

/// Forgetting per night, per link type, per mille (see `Config::own_fade`): told facts fade
/// slowly (half-life ~46 nights), who told you fades fast (~11 nights). Places' distances and the
/// empty feeling slots don't fade.
pub fn fade() -> [u16; lt::COUNT] {
    let mut f = [0u16; lt::COUNT];
    for t in [lt::KIND, lt::HOLDS, lt::AT, lt::ASSOC] { f[t as usize] = 15; }
    f[lt::SOURCE as usize] = 60;
    f
}

/// Ids of the people's concept nodes (the same in every person's mind: they are the base's).
#[derive(Clone, Copy, Debug, Hash)]
pub struct Ids { pub thirst: u32, pub fatigue: u32, pub pain: u32, pub water: u32, pub danger: u32, pub rest: u32, pub anger: u32, pub loss: u32,
    pub comfort: u32, pub person: u32, pub truth: u32, pub lie: u32, pub mistake: u32, pub debt: u32, pub note: u32, pub landmark: u32, pub dry: u32 }

pub fn temperament_links(r: &Roles, t: &Temperament) -> Vec<(u32, u32, u8, i32)> { crate::fox::found::temperament_links(r, t) }

pub struct People { pub species: Species, pub ids: Ids }

/// The people species (built once per process).
pub fn people() -> Arc<People> {
    static P: OnceLock<Arc<People>> = OnceLock::new();
    P.get_or_init(|| Arc::new(build())).clone()
}

fn build() -> People {
    let t = Temperament::MEDIUM;
    let mut b = Builder::new(PEOPLE_SPECIES);
    let thirst = b.node("thirst", -200, tag::BODY, None);
    let fatigue = b.node("fatigue", -100, tag::BODY, None);
    let pain = b.node("pain", -600, tag::BODY, None);
    let water = b.node("water", 300, tag::FOOD, None);
    let danger = b.node("danger", -700, tag::DANGER, None);
    let rest = b.node("rest", 300, tag::REST, None);
    let anger = b.node("anger", -200, tag::ANGER, None);
    let loss = b.node("loss", -500, 0, None);
    let comfort = b.node("comfort", 500, 0, None);
    let person = b.node("a person", 100, 0, None);
    let truth = b.node("truth", 300, 0, None);
    let lie = b.node("a lie", -500, 0, None);
    let mistake = b.node("a mistake", -100, 0, None);
    let debt = b.node("a favour owed", 100, 0, None);
    let note = b.node("a note", 100, 0, None);
    let landmark = b.node("a landmark", 0, 0, None);
    let dry = b.node("dry ground", -300, 0, None);
    b.link(loss, anger, lt::WANTS, 100 + t.aggression / 4);
    b.both(danger, anger, lt::ASSOC, 100 + t.aggression * 6 / 10, 300);
    b.link(pain, anger, lt::WANTS, 100 + t.aggression / 2);
    b.link(thirst, water, lt::WANTS, 800);
    b.link(fatigue, rest, lt::WANTS, 800);
    b.link(pain, danger, lt::WANTS, 700);
    b.both(rest, comfort, lt::ASSOC, 500, 400);
    b.link(dry, loss, lt::ASSOC, 400);
    b.link(lie, anger, lt::ASSOC, 300);
    b.canonical();
    let ids = Ids { thirst, fatigue, pain, water, danger, rest, anger, loss, comfort, person, truth, lie, mistake, debt, note, landmark, dry };
    let roles = Roles { hunger: thirst, fatigue, pain, food: water, danger, rest, anger, loss };
    let concept = |k: Kind| match k {
        Kind::Ash | Kind::Bryn | Kind::Cai | Kind::Dee => person, Kind::Water => water, Kind::Rock | Kind::Windmill | Kind::Oak => landmark,
        Kind::Hut => rest, Kind::Note => note, Kind::DryFresh | Kind::DryOld => dry,
    };
    let kinds = Kind::ALL.iter().map(|&k| KindInfo { name: k.name(), concept: concept(k), landmark: k.landmark(), edible: k == Kind::Water, shelter: k == Kind::Hut,
        creature: false, tracked: matches!(k, Kind::Ash | Kind::Bryn | Kind::Cai | Kind::Dee) }).collect();
    let species = Species {
        base: Arc::new(Base::compile(b)), roles, kinds: Arc::new(kinds), place_nodes: vec![], spare_places: 0, unlearnt: -999, near_weight,
        entity_nodes: vec![], entity_kinds: vec![], expect: vec![], geo: Geometry { sight: SIGHT, place_radius: PLACE_RADIUS, grid_w: GRID_W, grid_h: GRID_H },
        temperament: temperament_links, feelings: vec![water, danger, pain, loss, comfort, rest],
    };
    People { species, ids }
}

/// What one person is founded knowing: which landmarks (by world id), how far off its paces put
/// them (up to `pacing` cells, seeded per person and landmark), its trust and warmth towards each
/// other person, and, for the spring, whether it last saw it wet or dry.
#[derive(Clone, Debug)]
pub struct Founding { pub knows: Vec<u32>, pub pacing: i32, pub trust: [i32; PEOPLE], pub warmth: [i32; PEOPLE], pub spring_dry: bool, pub dry_conf: i32 }

/// A founded mind and its lexicon (landmark word → its own place node) and its node for each
/// other person (`u32::MAX` for itself).
pub fn found(w: &World, i: usize, t: Temperament, f: &Founding) -> (Mind<Runtime>, Vec<(u32, u32)>, [u32; PEOPLE]) {
    let p = people();
    let ids = p.ids;
    let who = hash(w.seed, i as u64, 0, 0x9e09);
    let mut m = Mind::new(&p.species, t, who);
    m.cfg.own_fade = fade();
    let mut lex = vec![];
    let hut = w.thing(CAMP);
    let camp = m.add_place("camp", hut.x, hut.y, None);
    let cn = m.place_nodes[camp];
    m.store.set_val(cn, 400);
    m.expect[camp][key(Kind::Hut.k(), false)] = 900;
    for (a, b2, ab, ba) in [(cn, ids.rest, 600, 600), (cn, ids.comfort, 700, 500)] { m.store.add_link(a, b2, lt::ASSOC, ab, 0); m.store.add_link(b2, a, lt::ASSOC, ba, 0); }
    lex.push((CAMP, cn));
    for &wd in &f.knows {
        let th = w.thing(wd);
        let err = |k: u64| if f.pacing == 0 { 0 } else { (hash(w.seed, i as u64, wd as u64 * 2 + k, 0xbace) % (2 * f.pacing as u64 + 1)) as i32 - f.pacing };
        let (x, y) = (th.x + err(0), th.y + err(1));
        let pi = m.add_place(&th.name.replace("the ", ""), x, y, None);
        let pn = m.place_nodes[pi];
        lex.push((wd, pn));
        if th.kind == Kind::Water {
            let dry = wd == super::world::SPRING && f.spring_dry;
            let n = m.store.add_node(&th.name, if dry { -100 } else { 200 }, tag::FOOD, Some(Anchor::Entity { id: th.id, x: x as i16, y: y as i16 }), m.who);
            for (a, b2, ty, wt) in [(n, ids.water, lt::KIND, 700), (ids.water, n, lt::ASSOC, 600), (n, pn, lt::AT, 800), (pn, n, lt::HOLDS, 700)] { m.store.add_link(a, b2, ty, wt, 0); }
            m.entity_nodes.push((th.id, n)); m.entity_kinds.push(Kind::Water.k());
            if dry { m.expect[pi][key(Kind::DryOld.k(), false)] = f.dry_conf as i16; m.store.add_link(n, ids.dry, lt::KIND, f.dry_conf.min(1000), 0); } else { m.expect[pi][key(Kind::Water.k(), false)] = 900; }
        } else {
            m.expect[pi][key(th.kind.k(), false)] = 900;
        }
    }
    let mut pnode = [u32::MAX; PEOPLE];
    for j in (0..PEOPLE).filter(|&j| j != i) {
        let q = &w.people[j];
        let n = m.store.add_node(NAMES[j], f.warmth[j], tag::CREATURE, Some(Anchor::Entity { id: person_id(j), x: q.x as i16, y: q.y as i16 }), m.who);
        m.store.add_link(n, ids.truth, lt::ASSOC, f.trust[j], 0);
        m.store.add_link(n, ids.debt, lt::ASSOC, 0, 0);
        m.store.add_link(n, ids.person, lt::KIND, 600, 0);
        m.entity_nodes.push((person_id(j), n)); m.entity_kinds.push(Kind::person(j).k());
        pnode[j] = n;
    }
    m.young_from = m.store.len() as u32;
    (m, lex, pnode)
}
