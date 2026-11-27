//! A weaker learner: teach the fox where the spring is by **showing** it, not saying it (7.3).
//!
//! The fox is the real fox species (`fox::found`), founded on a fox's view of the people's
//! valley: its den at the camp, the stream where the people's stream is. It can't hear words, so
//! the only way to teach it is to take it there: it is led beside Ash (who walks to the spring on
//! its own, to drink: in this setup Ash knows no other water) for a while, its mind sensing what a fox senses along the way (water, the
//! den). The same walk is given to a person (Bryn, led beside Ash), and a third mind is only told
//! (Bryn, told once and never going). After each night: does thinking of water bring the spring's
//! place to mind, and how strongly is it linked to water?
//!
//! The fox has no thirst (its founding graph has hunger → food only), so it can't be tested on
//! *using* the spring: only on what it holds.

use super::person::Person;
use super::scenarios::{Setup, ASH, BRYN, DAY, DEE, DUSK, NIGHT};
use super::world::{d2, Deed, Event, Kind, World};
use crate::fox::found::{self, Fox};
use crate::fox::world as fw;
use crate::mind::{Mind, Sense, Seen, Temperament};
use crate::store::{Runtime, Store};
use crate::think::Mods;
use std::sync::Arc;

/// The fox species for the people's valley: a den at the camp, the stream at the stream.
pub fn valley_fox(w: &World) -> Fox {
    let mut fwld = fw::World::valley(w.seed);
    let hut = w.thing(super::world::CAMP);
    let st = w.thing(super::world::STREAM);
    fwld.places = vec![fw::Place { name: "den", x: hut.x, y: hut.y, home: true }, fw::Place { name: "stream", x: st.x, y: st.y, home: false }];
    let e = |id, kind, name: &str, x, y| fw::Entity { id, kind, name: name.into(), x, y, px: x, py: y, servings: 0, alive: true };
    fwld.entities = vec![e(4, fw::Kind::Water, "the stream water", st.x, st.y), e(fw::BURROW, fw::Kind::Den, "the burrow", hut.x, hut.y)];
    found::species(&fwld)
}

/// What the fox senses of the people's valley: water and the hut (as its den), nothing else.
pub fn fox_sense(w: &World, body: usize, hunger: i32) -> Sense {
    let p = &w.people[body];
    let seen = if p.asleep { vec![] } else {
        w.things.iter().filter(|t| t.alive && d2(t.x, t.y, p.x, p.y) <= fw::SIGHT * fw::SIGHT).filter_map(|t| match t.kind {
            Kind::Water => Some(Seen { id: t.id, kind: fw::Kind::Water.k(), x: t.x, y: t.y, approaching: false, available: false }),
            Kind::Hut => Some(Seen { id: t.id, kind: fw::Kind::Den.k(), x: t.x, y: t.y, approaching: false, available: false }),
            _ => None,
        }).collect()
    };
    Sense { tick: w.tick, seed: w.seed, body: crate::mind::Body { x: p.x, y: p.y, hunger, pain: 0, asleep: p.asleep, drowsy: 0 }, seen, cues: vec![] }
}

/// After each night: (fox, shown person, told person), each as (the spring comes to mind from
/// "water": the activation of its place or of water held there, after 2 thinks; the link weight
/// water → it, slow + fast).
#[derive(Clone, Debug, Default)]
pub struct DemoDay { pub fox: (i32, i32), pub shown: (i32, i32), pub told: (i32, i32), pub fox_knows_place: bool, pub shown_knows_place: bool }

fn place_near<S: Store>(m: &Mind<S>, xy: (i32, i32)) -> Option<u32> {
    (0..m.place_nodes.len()).filter(|&pi| m.place_learnt(pi)).map(|pi| m.place_nodes[pi]).find(|&p| { let (x, y) = m.store.anchor(p).unwrap().xy(); d2(x, y, xy.0, xy.1) <= 6 * 6 })
}
fn recall<S: Store>(m: &Mind<S>, water: u32, xy: (i32, i32)) -> (i32, i32) {
    let Some(p) = place_near(m, xy) else { return (0, 0) };
    let (_, act, _) = m.imagine(&[(water, 1000)], &Mods::default(), 2);
    // The spring comes to mind as its place or as water held there (whichever is stronger), and
    // is linked to water directly or through that water.
    let held: Vec<u32> = m.store.links_vec(p).iter().filter(|l| l.ty == crate::graph::lt::HOLDS && l.slow + l.fast > 0).map(|l| l.to).collect();
    let a = held.iter().map(|&n| act[n as usize]).chain([act[p as usize]]).max().unwrap_or(0);
    let direct = m.store.links_vec(water).iter().filter(|l| l.to == p).map(|l| l.slow as i32 + l.fast as i32).max().unwrap_or(0);
    let via = held.iter().map(|&n| m.store.links_vec(water).iter().filter(|l| l.to == n).map(|l| l.slow as i32 + l.fast as i32).max().unwrap_or(0)).max().unwrap_or(0);
    (a, direct.max(via))
}

/// Lead the fox (in Dee's body) and Bryn beside Ash to the spring on each of `demos` days; a told
/// Bryn runs in a parallel valley. Returns the measures after each night.
pub fn demonstrate(seed: u64, t: Temperament, demos: u32, days: u32) -> Vec<DemoDay> {
    let mut su = Setup::new(seed);
    su.days = days;
    su.temperaments = [t; 4];
    // A demonstration: the leader goes to the spring of its own accord (it knows no other water),
    // and the person being shown doesn't send it elsewhere.
    su.knows[ASH] = vec![super::world::ROCK, super::world::WINDMILL, super::world::SPRING];
    su.traits[BRYN].helpful = -1000;
    // The fox (in Dee's body) starts at its den by the camp, where the people know it is.
    su.pos[DEE] = (9, 23);
    let mut v = su.build();
    let fox: Arc<Fox> = Arc::new(valley_fox(&v.w));
    let mut fm: Mind<Runtime> = fox.mind(t, seed ^ 0xf0);
    v.w.people[BRYN].thirst = 0;
    for d in 0..demos {
        v.at(d * DAY, Event::SetThirst { who: ASH, v: 700 });
        v.at(d * DAY, Event::Lead { who: DEE, by: ASH, until: d * DAY + 90 });
        v.at(d * DAY, Event::Lead { who: BRYN, by: ASH, until: d * DAY + 90 });
    }
    // The told mind: Bryn asks Ash, is told, and is then not thirsty enough to go.
    let mut su2 = Setup::new(seed);
    su2.days = days; su2.temperaments = [t; 4];
    let mut told = su2.build();
    told.at(3, Event::SetThirst { who: BRYN, v: 0 });
    for d in 1..days { told.at(d * DAY, Event::SetThirst { who: BRYN, v: 0 }); }
    let spring = v.w.spring();
    let mut out = vec![];
    let order: Vec<usize> = (0..4).collect();
    for _ in 0..days * DAY {
        // The people's valley, with the fox's mind in Dee's body.
        v.w.pre_step();
        if v.w.night_now {
            for i in [ASH, BRYN, 2] { let s = v.w.sense(i, true); v.folk[i].m.night(&s); }
            let s = fox_sense(&v.w, DEE, 200); fm.night(&s);
        }
        let w = &v.w;
        let mut deeds: Vec<Deed> = super::sim::run(&mut v.folk, 1, &order, |i, p: &mut Person| if i == DEE { Deed::default() } else { p.think(w) });
        let s = fox_sense(&v.w, DEE, 200);
        let a = fm.step(&s); fm.commit(&s, a);
        deeds[DEE].action = Some(a);
        v.w.post_step(&deeds);
        told.run(1);
        if v.w.tick % DAY == 1 && v.w.tick > 1 {
            let b = &v.folk[BRYN];
            let tb = &told.folk[BRYN];
            out.push(DemoDay {
                fox: recall(&fm, fox.ids.water, spring), shown: recall(&b.m, b.ids.water, spring), told: recall(&tb.m, tb.ids.water, spring),
                fox_knows_place: place_near(&fm, spring).is_some(), shown_knows_place: place_near(&b.m, spring).is_some(),
            });
        }
    }
    let _ = (DUSK, NIGHT);
    out
}
