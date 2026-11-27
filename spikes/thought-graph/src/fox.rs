//! The fox's founding mind, built from the world it lives in. Concepts and their links are
//! authored (a fox is born knowing hunger wants food and dogs are dangerous); places, the things
//! at them, and how places link to each other are read from the world:
//! - **near** links from real distance between every pair of places;
//! - **like** links between places that hold the same kinds of thing;
//! - **holds / at** links between a place and each thing within `PLACE_RADIUS` of it.
//!
//! Every place and thing also gets empty `feels` slots (weight 0) to the feeling concepts, so
//! experience has somewhere to write without changing the packed structure.

use crate::graph::{lt, tag, Anchor, Builder, Graph};
use crate::mind::Temperament;
use crate::world::{dist, Kind, World};

/// Places this far apart or more have no near link.
pub const NEAR_RANGE: i32 = 48;

/// Ids of the fox's fixed concept nodes.
#[derive(Clone, Copy, Debug, Hash)]
pub struct Ids {
    pub hunger: u32, pub fatigue: u32, pub pain: u32,
    pub food: u32, pub water: u32, pub fire: u32, pub danger: u32, pub rest: u32, pub comfort: u32,
    pub anger: u32,
}

pub struct Founding {
    pub graph: Graph,
    pub ids: Ids,
    /// Node of each world place, by place index.
    pub place_nodes: Vec<u32>,
    /// (entity id, node) for the things the fox already knows.
    pub entity_nodes: Vec<(u32, u32)>,
}

fn kind_val(k: Kind) -> i32 { match k { Kind::Hen => 500, Kind::Berry => 250, Kind::Water => 200, Kind::Dog => -300, Kind::Fire => -150 } }
fn kind_tags(k: Kind) -> u8 {
    match k { Kind::Hen | Kind::Berry => tag::FOOD, Kind::Dog => tag::DANGER | tag::CREATURE, Kind::Fire => tag::DANGER, Kind::Water => 0 }
}

/// `t` sets the individual's structure: how strongly danger and pain lead to anger.
pub fn found(w: &World, t: &Temperament) -> Founding {
    let mut b = Builder::default();
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
    let ids = Ids { hunger, fatigue, pain, food, water, fire, danger, rest, comfort, anger };
    // Temperament is structure: a quick-tempered fox's danger and pain lead to anger more strongly.
    b.both(danger, anger, lt::ASSOC, 100 + t.aggression * 6 / 10, 300);
    b.link(pain, anger, lt::WANTS, 100 + t.aggression / 2);

    b.link(hunger, food, lt::WANTS, 800);
    b.link(fatigue, rest, lt::WANTS, 800);
    b.link(pain, danger, lt::WANTS, 700);
    b.both(fire, danger, lt::KIND, 700, 300);
    b.both(rest, comfort, lt::ASSOC, 500, 400);

    let place_nodes: Vec<u32> = w.places.iter().map(|p| {
        b.node(p.name, if p.home { 400 } else { 0 }, tag::PLACE, Some(Anchor::Place { x: p.x as i16, y: p.y as i16 }))
    }).collect();

    let kind_concept = |k: Kind| match k { Kind::Hen | Kind::Berry => food, Kind::Water => water, Kind::Fire => fire, Kind::Dog => danger };
    let mut entity_nodes = vec![];
    // What each place holds, by kind concept: for "like" links.
    let mut holds: Vec<Vec<u32>> = vec![vec![]; w.places.len()];
    for e in w.entities.iter().filter(|e| e.alive) {
        let n = b.node(&e.name, kind_val(e.kind), kind_tags(e.kind), Some(Anchor::Entity { id: e.id, x: e.x as i16, y: e.y as i16 }));
        entity_nodes.push((e.id, n));
        let c = kind_concept(e.kind);
        match e.kind {
            Kind::Dog => b.both(n, c, lt::KIND, 700, 400),
            Kind::Fire => b.both(n, c, lt::KIND, 700, 400),
            _ => b.both(n, c, lt::KIND, 700, 600),
        }
        if let Some(pi) = w.place_at(e.x, e.y) {
            b.link(place_nodes[pi], n, lt::HOLDS, 700);
            b.link(n, place_nodes[pi], lt::AT, 800);
            holds[pi].push(c);
        }
        // Empty slots for experience: this thing and pain, this thing and danger.
        b.both(n, pain, lt::FEELS, 0, 0);
        b.both(n, danger, lt::FEELS, 0, 0);
        if e.kind == Kind::Dog { b.both(n, anger, lt::FEELS, 0, 0); }
    }

    let feelings = [food, water, fire, danger, rest, pain];
    for (i, p) in w.places.iter().enumerate() {
        let pn = place_nodes[i];
        if p.home {
            b.both(pn, rest, lt::ASSOC, 600, 600);
            b.both(pn, comfort, lt::ASSOC, 700, 500);
        }
        for &c in &feelings { b.both(pn, c, lt::FEELS, 0, 0); }
        for (j, q) in w.places.iter().enumerate() {
            if i == j { continue; }
            let d = dist(p.x, p.y, q.x, q.y);
            if d < NEAR_RANGE { b.link(pn, place_nodes[j], lt::NEAR, 600 * (NEAR_RANGE - d) / NEAR_RANGE); }
            let shared = holds[i].iter().filter(|c| holds[j].contains(c)).count() as i32;
            if shared > 0 { b.link(pn, place_nodes[j], lt::LIKE, 300 * shared); }
        }
    }
    Founding { graph: b.build(), ids, place_nodes, entity_nodes }
}
