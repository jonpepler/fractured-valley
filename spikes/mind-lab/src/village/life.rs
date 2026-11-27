//! The village layer around a person (stage 5), as `hunt::hunter` is the hunting layer: what a
//! villager does that a teaching or hunting scenario never needed. Everything here reads only
//! what this person senses (its percepts, what it hears) and its own mind, and writes only this
//! person, except the tier, which is the engine's choice of how often to think (7.7).
//!
//! - **The tier (7.7).** A mind in full detail thinks every tick. A distant mind thinks every
//!   `every` ticks (staggered by person), and on the ticks between it keeps doing what it last
//!   chose (the body walks on to its goal without sensing the way: approximate movement), hears
//!   (what it hears is kept and taken in at its next think), and does only its mind's upkeep
//!   (`Mind::upkeep`). It thinks at once if it is spoken to, hears a shout, or sees a creature
//!   (a boar) or a stranger coming at it: the wake-up rules.
//! - **Gossip** (a hand-written rule, listed in the stage note). A person with a strong feeling
//!   about someone it is thinking of tells a companion "X is good to know" or "watch out for X".
//!   The hearer's feeling about X moves by `GOSSIP_STEP` × its trust in the teller, and it keeps a
//!   source link from X to the teller (which fades, as stage 2's do).
//! - **The idea of a hunt** (a hand-written trigger). A hungry hunter who sees the deer while the
//!   deer is on its mind and someone is with it has the idea: it becomes the leader of stage 4's
//!   hunting layer, which composes, proposes and runs the plan. Everything after the idea is
//!   stage 4's. When the plan is no longer held (or the day ends) the hunting layer is reset.

use crate::graph::lt;
use crate::hunt::hunter::Lead;
use crate::mind::{Action, IGNORE_BELOW};
use crate::people::person::{strengthen, Person};
use crate::people::talk::{Say, Target};
use crate::people::world::{Deed, Kind, Utterance, World, SHOUT, SPEAK, TALK};
use crate::rng::hash;
use crate::store::Store;
use crate::think::Form;
use std::cell::Cell;

/// A villager's own state for the village layer.
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Life {
    /// Think every this many ticks (1: full detail). Set by the engine (the tier), not the mind.
    pub every: u32,
    /// What it chose at its last think (continued between thinks).
    pub last: Option<Action>,
    /// Heard since its last think (taken in at the next).
    pub buffer: Vec<(Utterance, i32, bool)>,
    /// Gossip told: (tick, listener, about).
    pub gossiped: Vec<(u32, u16, u16)>,
    /// When it last had the idea of a hunt, and when the plan it holds was last held in mind.
    pub led_at: Option<u32>,
    pub plan_seen: u32,
    /// Measures (never read by the mind): thinks, skipped ticks, woken early, gossip said and
    /// heard, hunts led and joined, ticks surprised.
    pub stats: Stats,
    /// Did it think this tick (for measures that read the mind's last report).
    pub thought: bool,
}

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub struct Stats { pub thinks: u32, pub skipped: u32, pub woken: u32, pub gossip_said: u32, pub gossip_heard: u32, pub hunts_led: u32, pub surprised: u32 }

/// Gossip: a feeling about someone at least this strong is worth passing on.
pub const GOSSIP_AT: i32 = 350;
/// How far one piece of gossip moves the hearer's feeling (× its trust in the teller / 1000);
/// half if overheard.
pub const GOSSIP_STEP: i32 = 60;
/// The same piece (listener, about) is not told again within this many ticks; and a think passes
/// it on with this chance per mille.
pub const GOSSIP_GAP: u32 = 301;
pub const GOSSIP_P: i32 = 150;
/// The idea of a hunt: hungry at least this much, and not within this many ticks of the last.
pub const HUNT_HUNGER: i32 = 450;
pub const HUNT_GAP: u32 = 600;
/// A plan not held in mind for this many ticks is over.
pub const PLAN_OVER: u32 = 20;

thread_local! {
    /// (ns, person-ticks) of the village layer's whole person tick on this thread, when
    /// `think::TIME_THINK` is on (measures only).
    pub static PERSON_NS: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
}

/// One tick of a villager: the tier's choice, then the person's own tick (`Person::think_heard`),
/// then the village layer's talk and the idea of a hunt.
pub fn think(p: &mut Person, w: &World) -> Deed {
    let timed = crate::think::TIME_THINK.load(std::sync::atomic::Ordering::Relaxed);
    let t0 = if timed { Some(std::time::Instant::now()) } else { None };
    let d = think_inner(p, w);
    if let Some(t0) = t0 { PERSON_NS.with(|c| { let (a, b) = c.get(); c.set((a + t0.elapsed().as_nanos() as u64, b + 1)); }); }
    d
}

fn think_inner(p: &mut Person, w: &World) -> Deed {
    let i = p.i;
    let heard = w.heard(i);
    let tick = w.tick;
    let asleep = w.people[i].asleep;
    let life = p.village.as_mut().unwrap();
    life.thought = false;
    let every = life.every.max(1);
    if every > 1 && !asleep && life.last.is_some() && (tick + i as u32) % every != 0 {
        // The wake-up rules: spoken to, a shout, a creature in sight, someone coming at me.
        let s = w.sense(i, false);
        let woken = heard.iter().any(|(u, _, to_me)| *to_me || u.vol >= SHOUT)
            || s.seen.iter().any(|e| p.m.kinds.get(e.kind as usize).is_some_and(|k| k.creature) || (e.approaching && crate::mind::dist(e.x, e.y, s.body.x, s.body.y) <= 2));
        let life = p.village.as_mut().unwrap();
        if !woken {
            life.buffer.extend(heard);
            life.stats.skipped += 1;
            let a = match life.last.unwrap() {
                a @ (Action::WalkTo { .. } | Action::Flee { .. } | Action::Charge { .. } | Action::Hold { .. } | Action::Freeze) => a,
                _ => Action::Stay,
            };
            p.m.upkeep(tick);
            return Deed { action: Some(a), ..Deed::default() };
        }
        life.stats.woken += 1;
    }
    let life = p.village.as_mut().unwrap();
    let mut all = std::mem::take(&mut life.buffer);
    all.extend(heard);
    hear_gossip(p, &all);
    let mut deed = p.think_heard(w, all);
    let life = p.village.as_mut().unwrap();
    life.thought = true;
    life.stats.thinks += 1;
    life.last = deed.action;
    if asleep { return deed; }
    if p.m.last.percepts.iter().any(|q| q.error >= IGNORE_BELOW) || !p.m.last.absent.is_empty() { p.village.as_mut().unwrap().stats.surprised += 1; }
    hunt_life(p, w);
    if deed.say.is_none() && deed.write.is_none() { deed.say = gossip(p, w); }
    deed
}

/// Who (by person index) a node of this mind stands for.
fn who(p: &Person, n: u32) -> Option<usize> { p.pnode.iter().position(|&x| x == n) }

/// Gossip heard: the feeling about X moves towards the teller's, by the hearer's trust in it.
fn hear_gossip(p: &mut Person, heard: &[(Utterance, i32, bool)]) {
    for (u, _, to_me) in heard {
        let Say::Gossip { about, good } = u.say else { continue };
        let (a, j) = (u.speaker, about as usize);
        if j == p.i || a == p.i || j >= p.pnode.len() { continue; }
        let step = GOSSIP_STEP * p.trust(a) / 1000 / if *to_me { 1 } else { 2 };
        let n = p.pnode[j];
        let v = p.m.store.val(n);
        p.m.store.set_val(n, (v + if good { step } else { -step }).clamp(-1000, 1000) as i16);
        let lv = p.trust(a) - 500;
        let an = p.pnode[a];
        strengthen(&mut p.m, n, an, lt::SOURCE, 300, Some(lv));
        p.village.as_mut().unwrap().stats.gossip_heard += 1;
    }
}

/// Gossip told: someone I feel strongly about is on my mind, and a companion I like is near.
fn gossip(p: &mut Person, w: &World) -> Option<(Say, Option<usize>, i32)> {
    let tick = w.tick;
    if w.dusk { return None; }
    let me = (w.people[p.i].x, w.people[p.i].y);
    // A companion in sight and in talking distance (my percepts), that I don't dislike.
    let listener = p.m.last.percepts.iter().filter_map(|q| who(p, q.node).map(|j| (crate::mind::d2(q.x, q.y, me.0, me.1), j)))
        .filter(|&(d, j)| d <= TALK * TALK && p.warmth(j) >= 0).min().map(|x| x.1)?;
    let about = p.m.last.ws.iter().filter_map(|&n| who(p, n)).filter(|&j| j != listener && p.warmth(j).abs() >= GOSSIP_AT)
        .max_by_key(|&j| (p.warmth(j).abs(), std::cmp::Reverse(j)))?;
    let life = p.village.as_ref().unwrap();
    if life.gossiped.iter().any(|g| g.1 as usize == listener && g.2 as usize == about && tick < g.0 + GOSSIP_GAP) { return None; }
    if (hash(w.seed, tick as u64, p.i as u64, 0x9055) % 1000) as i32 >= GOSSIP_P { return None; }
    let good = p.warmth(about) > 0;
    let life = p.village.as_mut().unwrap();
    life.gossiped.retain(|g| tick < g.0 + GOSSIP_GAP);
    life.gossiped.push((tick, listener as u16, about as u16));
    life.stats.gossip_said += 1;
    Some((Say::Gossip { about: about as u8, good }, Some(listener), SPEAK))
}

/// The idea of a hunt, and the end of one.
fn hunt_life(p: &mut Person, w: &World) {
    let Some(h) = p.hunt.as_ref() else { return };
    let tick = w.tick;
    let k = h.k;
    let holding = h.plan.as_ref().is_some_and(|hp| p.m.held.intent == Some(hp.node));
    if holding { p.village.as_mut().unwrap().plan_seen = tick; }
    // Over: a plan (or the idea) not held for a while, or the day ending.
    let stale = (h.plan.is_some() || h.lead.is_some()) && (w.dusk || tick > p.village.as_ref().unwrap().plan_seen + PLAN_OVER);
    if stale { reset_hunt(p); return; }
    let h = p.hunt.as_ref().unwrap();
    if h.plan.is_some() || h.lead.is_some() || w.dusk { return; }
    let life = p.village.as_ref().unwrap();
    if life.led_at.is_some_and(|t| tick < t + HUNT_GAP) { return; }
    let hungry = w.village.as_ref().map_or(0, |v| v.hunger[p.i]) >= HUNT_HUNGER;
    let deer_seen = p.m.last.percepts.iter().any(|q| q.kind == Kind::Deer.k());
    let on_mind = p.m.last.ws.contains(&k.deer) || p.m.last.ws.contains(&k.deer_kind);
    let company = p.m.last.percepts.iter().any(|q| who(p, q.node).is_some() && crate::mind::dist(q.x, q.y, w.people[p.i].x, w.people[p.i].y) <= TALK + 4);
    if hungry && deer_seen && on_mind && company {
        let h = p.hunt.as_mut().unwrap();
        h.lead = Some(Target::Deer);
        h.book = Lead { rev: h.book.rev, ..Lead::default() };
        let life = p.village.as_mut().unwrap();
        life.led_at = Some(tick); life.plan_seen = tick; life.stats.hunts_led += 1;
    }
}

/// Clear the hunting layer after a hunt (keeping the plan revision, so a new plan is new to hearers).
pub fn reset_hunt(p: &mut Person) {
    let node = p.hunt.as_ref().and_then(|h| h.plan.as_ref().map(|hp| hp.node));
    // The plan is over: its node is a spent intent, whose links fade faster than facts.
    if let Some(n) = node { if !p.m.spent.contains(&n) { p.m.spent.push(n); } }
    let h = p.hunt.as_mut().unwrap();
    h.plan = None; h.lead = None; h.solo = false; h.out.clear(); h.warn = None;
    h.book = Lead { rev: h.book.rev, ..Lead::default() };
    p.m.plan.expects.clear();
    if node.is_some() && p.m.held.intent == node { p.m.held.intent = None; p.m.held.lapse = 0; }
}

/// A villager's mind's own count of things it would say it knows (for measures).
pub fn knows(p: &Person) -> (usize, usize, usize) {
    let places = (0..p.m.place_nodes.len()).filter(|&pi| p.m.place_learnt(pi)).count();
    let waters = p.water_things().len();
    let bushes = p.m.entity_kinds.iter().filter(|&&k| k == Kind::Berry.k()).count();
    (places, waters, bushes)
}
