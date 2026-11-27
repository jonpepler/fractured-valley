//! People as a species. The shared base holds only **concepts** (thirst, water, rest, trust, lie,
//! mistake, a debt, a note…): what every person's mind has the same. Everything a person knows
//! about the valley and about the others (places, where the landmarks are, the stream, each
//! other person, how far they trust them) is founded on that person's **own overlay**, with
//! their own positions, so two minds never share a node. Places a person learns or is told of
//! are new nodes of its own (the species has no spare place slots: places grow).
//!
//! Roles: the mind's "hunger" is thirst and its "food" is water, so a thirsty person's thought
//! runs to water exactly as a hungry fox's runs to food.

use super::world::{name, person_id, person_kind, Kind, World, GRID_H, GRID_W, PLACE_RADIUS, SIGHT, CAMP};
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
    f[lt::TOLD as usize] = 60;
    f
}

/// Ids of the people's concept nodes (the same in every person's mind: they are the base's).
#[derive(Clone, Copy, Debug, Hash)]
pub struct Ids { pub thirst: u32, pub fatigue: u32, pub pain: u32, pub water: u32, pub danger: u32, pub rest: u32, pub anger: u32, pub loss: u32,
    pub comfort: u32, pub person: u32, pub truth: u32, pub lie: u32, pub mistake: u32, pub debt: u32, pub note: u32, pub landmark: u32, pub dry: u32,
    /// Stage 3: a boar (the kind), the five danger responses and aid, "an ally", "a cry for help".
    pub boar: u32, pub flee: u32, pub fight: u32, pub call: u32, pub freeze: u32, pub hold: u32, pub aid: u32, pub ally: u32, pub cry: u32,
    /// Stage 5 (the village's people only; `u32::MAX` otherwise): hunger, food (what a berry bush
    /// is an instance of), and "an animal" (a fox).
    pub hunger: u32, pub food: u32, pub animal: u32 }

/// The founding links a person's temperament sets: the fox's three (aggression to anger), and
/// (stage 3) reactivity to keeping still: a reactive person's thought of danger runs more to
/// freezing (behavioural inhibition, as aggression runs it to anger).
pub fn temperament_links(r: &Roles, t: &Temperament) -> Vec<(u32, u32, u8, i32)> {
    let mut v = crate::fox::found::temperament_links(r, t);
    if let Some(rs) = r.resp { v.push((r.danger, rs.freeze, lt::ASSOC, freeze_weight(t))); }
    v
}
pub fn freeze_weight(t: &Temperament) -> i32 { 50 + t.reactivity * 45 / 100 }

pub struct People { pub species: Species, pub ids: Ids,
    /// Stage 5: how many people the kinds table can tell apart, and whether this is the village's
    /// species (more concepts: hunger and food; more kinds: berry bushes and foxes).
    pub n: usize, pub village: bool }

/// The people species (built once per process).
pub fn people() -> Arc<People> {
    static P: OnceLock<Arc<People>> = OnceLock::new();
    P.get_or_init(|| Arc::new(build(false, super::world::PEOPLE))).clone()
}

/// Stage 5: the village's people species for a valley of `n` people (built once per `n`, so every
/// villager shares one base). Its base is the teaching base with three concepts added at the end
/// (hunger, food, an animal), so every concept id the earlier layers use is the same.
pub fn village_people(n: usize) -> Arc<People> {
    static V: std::sync::Mutex<Vec<(usize, Arc<People>)>> = std::sync::Mutex::new(Vec::new());
    let mut v = V.lock().unwrap();
    if let Some(p) = v.iter().find(|x| x.0 == n) { return p.1.clone(); }
    let p = Arc::new(build(true, n));
    v.push((n, p.clone()));
    p
}

fn build(village: bool, n: usize) -> People {
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
    // Stage 3: danger with company. A boar is a dangerous kind of creature; the responses are
    // concepts that thought can reach; an ally and a cry for help are ideas like trust.
    let boar = b.node("a boar", -300, tag::DANGER | tag::CREATURE, None);
    let flee = b.node("run away", -100, 0, None);
    let fight = b.node("fight", -100, tag::ANGER, None);
    let call = b.node("call for help", 0, 0, None);
    let freeze = b.node("keep still", -200, 0, None);
    let hold = b.node("stand my ground", 100, 0, None);
    let aid = b.node("go to help", 100, 0, None);
    let ally = b.node("someone who stands with me", 400, 0, None);
    let cry = b.node("a cry for help", -300, 0, None);
    b.link(loss, anger, lt::WANTS, 100 + t.aggression / 4);
    b.both(danger, anger, lt::ASSOC, 100 + t.aggression * 6 / 10, 300);
    b.link(pain, anger, lt::WANTS, 100 + t.aggression / 2);
    b.link(thirst, water, lt::WANTS, 800);
    b.link(fatigue, rest, lt::WANTS, 800);
    b.link(pain, danger, lt::WANTS, 700);
    b.both(rest, comfort, lt::ASSOC, 500, 400);
    b.link(dry, loss, lt::ASSOC, 400);
    b.link(lie, anger, lt::ASSOC, 300);
    b.link(boar, danger, lt::KIND, 700);
    b.link(danger, flee, lt::WANTS, 500);
    b.link(danger, freeze, lt::ASSOC, freeze_weight(&t));
    b.link(danger, call, lt::ASSOC, CALL_HABIT);
    b.link(danger, hold, lt::ASSOC, 150);
    b.link(anger, fight, lt::WANTS, 600);
    b.link(ally, hold, lt::ASSOC, 600);
    b.link(ally, fight, lt::ASSOC, 300);
    b.link(ally, aid, lt::ASSOC, 400);
    b.link(cry, danger, lt::ASSOC, 400);
    b.link(cry, aid, lt::ASSOC, 500);
    let (mut hunger, mut food, mut animal) = (u32::MAX, u32::MAX, u32::MAX);
    if village {
        hunger = b.node("hunger", -200, tag::BODY, None);
        food = b.node("food", 300, tag::FOOD, None);
        animal = b.node("an animal", 0, 0, None);
        b.link(hunger, food, lt::WANTS, 800);
    }
    b.canonical();
    let ids = Ids { thirst, fatigue, pain, water, danger, rest, anger, loss, comfort, person, truth, lie, mistake, debt, note, landmark, dry, boar, flee, fight, call, freeze, hold, aid, ally, cry, hunger, food, animal };
    let resp = crate::mind::Responses { flee, fight, call, freeze, hold, aid, ally, cry };
    let roles = Roles { hunger: thirst, fatigue, pain, food: water, danger, rest, anger, loss, comfort, resp: Some(resp), hunger2: hunger, food2: food };
    let concept = |k: Kind| match k {
        Kind::Ash | Kind::Bryn | Kind::Cai | Kind::Dee => person, Kind::Water => water, Kind::Rock | Kind::Windmill | Kind::Oak => landmark,
        Kind::Hut => rest, Kind::Note => note, Kind::DryFresh | Kind::DryOld => dry, Kind::Boar => boar,
        // Stage 4: a person with no node of its own for a deer sees "a thing" (hunters have one).
        Kind::Deer => landmark,
        // Stage 5: the village's kinds.
        Kind::Berry => food, Kind::Fox => animal,
    };
    let info = |k: Kind| KindInfo { name: k.name(), concept: concept(k), landmark: k.landmark(), edible: matches!(k, Kind::Water | Kind::Berry), shelter: k == Kind::Hut,
        creature: k == Kind::Boar, tracked: matches!(k, Kind::Ash | Kind::Bryn | Kind::Cai | Kind::Dee) };
    let kinds = village_kinds(Kind::ALL.iter().map(|&k| info(k)).collect(), n, village, info(Kind::Berry), info(Kind::Fox));
    let species = Species {
        base: Arc::new(Base::compile(b)), roles, kinds: Arc::new(kinds), place_nodes: vec![], spare_places: 0, unlearnt: -999, near_weight,
        entity_nodes: vec![], entity_kinds: vec![], expect: vec![], geo: Geometry { sight: SIGHT, place_radius: PLACE_RADIUS, grid_w: GRID_W, grid_h: GRID_H },
        temperament: temperament_links, feelings: if village { vec![water, danger, pain, loss, comfort, rest, food] } else { vec![water, danger, pain, loss, comfort, rest] },
    };
    People { species, ids, n, village }
}

/// Stage 5: a species' kinds table for a valley of `n` people: the 14 kinds every species tells
/// apart today; then, in the village, a berry bush and a fox; then one look for each person after
/// the fourth (each told apart as the first person's look is). Unchanged for 4 people and no village.
pub fn village_kinds(mut base: Vec<KindInfo>, n: usize, village: bool, berry: KindInfo, fox: KindInfo) -> Vec<KindInfo> {
    let count = super::world::kind_count(n, village);
    if count == base.len() { return base; }
    base.push(berry); base.push(fox);
    let person = base[0].clone();
    for j in super::world::PEOPLE..n { base.push(KindInfo { name: name(j), ..person.clone() }); }
    debug_assert_eq!(base.len(), count);
    base
}

/// What one person is founded knowing: which landmarks (by world id), how far off its paces put
/// them (up to `pacing` cells, seeded per person and landmark), its trust and warmth towards each
/// other person, and, for the spring, whether it last saw it wet or dry.
#[derive(Clone, Debug)]
pub struct Founding { pub knows: Vec<u32>, pub pacing: i32, pub trust: Vec<i32>, pub warmth: Vec<i32>, pub spring_dry: bool, pub dry_conf: i32,
    /// Stage 3: how far this person counts on each other to stand with it, from their history
    /// (a link from their node to "someone who stands with me"; 0: none). It is learnt further.
    pub ally: Vec<i32>,
    /// Stage 3: how far this person has learnt to cry out when in danger (danger → call; the
    /// species' founding is `CALL_HABIT`).
    pub calls: i32 }
pub const CALL_HABIT: i32 = 250;

/// A founded mind and its lexicon (landmark word → its own place node) and its node for each
/// other person (`u32::MAX` for itself).
pub fn found(w: &World, i: usize, t: Temperament, f: &Founding) -> (Mind<Runtime>, Vec<(u32, u32)>, Vec<u32>) { found_in(&people(), w, i, t, f) }
/// `found` for a given people species (stage 5: the village's). Everyone in the valley `w` gets a
/// node of their own in this mind (people are a vector of any size).
pub fn found_in(p: &People, w: &World, i: usize, t: Temperament, f: &Founding) -> (Mind<Runtime>, Vec<(u32, u32)>, Vec<u32>) {
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
        } else if th.kind == Kind::Berry && ids.food != u32::MAX {
            // Stage 5: a berry bush the person knows, held as water is: a thing of its own at its
            // place, an instance of food.
            let n = m.store.add_node(&th.name, 200, tag::FOOD, Some(Anchor::Entity { id: th.id, x: x as i16, y: y as i16 }), m.who);
            for (a, b2, ty, wt) in [(n, ids.food, lt::KIND, 700), (ids.food, n, lt::ASSOC, 600), (n, pn, lt::AT, 800), (pn, n, lt::HOLDS, 700)] { m.store.add_link(a, b2, ty, wt, 0); }
            m.entity_nodes.push((th.id, n)); m.entity_kinds.push(Kind::Berry.k());
            m.expect[pi][key(Kind::Berry.k(), false)] = 900;
        } else {
            m.expect[pi][key(th.kind.k(), false)] = 900;
        }
    }
    let mut pnode = vec![u32::MAX; w.people.len()];
    for j in (0..w.people.len()).filter(|&j| j != i) {
        let q = &w.people[j];
        let n = m.store.add_node(name(j), f.warmth[j], tag::CREATURE, Some(Anchor::Entity { id: person_id(j), x: q.x as i16, y: q.y as i16 }), m.who);
        m.store.add_link(n, ids.truth, lt::ASSOC, f.trust[j], 0);
        m.store.add_link(n, ids.debt, lt::ASSOC, 0, 0);
        m.store.add_link(n, ids.person, lt::KIND, 600, 0);
        if f.ally[j] > 0 { m.store.add_link(n, ids.ally, lt::ASSOC, f.ally[j], 0); }
        m.entity_nodes.push((person_id(j), n)); m.entity_kinds.push(person_kind(j));
        pnode[j] = n;
    }
    if f.calls != CALL_HABIT { let k = m.store.find(ids.danger, ids.call, lt::ASSOC).unwrap(); m.store.set_slow(k, crate::store::quant_w(f.calls as i16)); }
    m.young_from = m.store.len() as u32;
    (m, lex, pnode)
}
