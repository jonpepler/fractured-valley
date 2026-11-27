//! The hunt and the betrayal: scenarios and their measures.
//!
//! **Staging** (stated, never a choice of a mind's): the valley (the seed places the camp, the
//! landmarks, the deer's two meadows and a rock within reach of them, where everyone starts, and
//! how warmly each person feels towards the others); who has the idea of the hunt (Ash); for the
//! betrayal, the rivalry (the group's history with Dee) and Dee's habit (it sets out for water at
//! `HABIT`). Everything after that (what is proposed and how, who joins, what each predicts, what
//! breaks, what anyone does or says, the deer's flight) is the minds' own.

use super::deer;
use super::hunter::{self, start_solo};
use crate::graph::lt;
use crate::mind::{Temperament, BREACH_MISSING, BREACH_PACE};
use crate::people::person::Person;
use crate::people::scenarios::{ASH, BRYN, CAI, DEE};
use crate::people::species::{Founding, CALL_HABIT};
use crate::people::sim::Valley;
use crate::people::talk::{Role, Say, Target};
use crate::people::world::{Deer, Event, Kind, World, CAMP, GRID_H, GRID_W, OAK, PEOPLE, ROCK, STREAM, WINDMILL};
use crate::rng::hash;
use crate::store::Store;
use crate::think::Form;

/// Temperament sets: everyone calm; seeded individuals; everyone reactive.
pub const SETS: [&str; 3] = ["calm", "seeded", "reactive"];
pub fn temperaments(seed: u64, set: usize) -> [Temperament; PEOPLE] {
    match set {
        0 => [Temperament { reactivity: 150, aggression: 200 }; PEOPLE],
        2 => [Temperament { reactivity: 900, aggression: 500 }; PEOPLE],
        _ => std::array::from_fn(|i| Temperament::seeded(seed ^ 0x4e7, i as u32)),
    }
}
/// How long a hunt lasts (ticks) if the deer isn't caught.
pub const HUNT_TICKS: u32 = 220;

/// The seed's valley: where everything is. Every seed is a different valley.
#[derive(Clone, Debug)]
pub struct Valley4 { pub camp: (i32, i32), pub rock: (i32, i32), pub windmill: (i32, i32), pub oak: (i32, i32), pub stream: (i32, i32), pub meadows: Vec<(i32, i32)>, pub starts: [(i32, i32); PEOPLE], pub deer_at: usize,
    /// warmth[i][j] and trust[i][j], seeded.
    pub warmth: [[i32; PEOPLE]; PEOPLE], pub trust: [[i32; PEOPLE]; PEOPLE] }

pub fn layout(seed: u64) -> Valley4 {
    let r = |k: u64, lo: i32, hi: i32| lo + (hash(seed, k, 0, 0x4a17) % (hi - lo + 1) as u64) as i32;
    let camp = (r(1, 6, 14), r(2, 16, 26));
    // The deer's range: somewhere in the east half; two meadows a few cells apart.
    let centre = (r(3, 30, 54), r(4, 8, 30));
    const D16: [(i32, i32); 16] = [(1000, 0), (924, 383), (707, 707), (383, 924), (0, 1000), (-383, 924), (-707, 707), (-924, 383), (-1000, 0), (-924, -383), (-707, -707), (-383, -924), (0, -1000), (383, -924), (707, -707), (924, -383)];
    let a = D16[r(5, 0, 15) as usize];
    let mr = r(6, 2, 4);
    let clamp = |x: i32, y: i32| (x.clamp(2, GRID_W - 3), y.clamp(2, GRID_H - 3));
    let meadows = vec![clamp(centre.0 + a.0 * mr / 1000, centre.1 + a.1 * mr / 1000), clamp(centre.0 - a.0 * mr / 1000, centre.1 - a.1 * mr / 1000)];
    // A rock within reach of the range (6 to 12 cells off, at a seeded bearing): cover exists, but
    // where varies.
    let b = D16[r(7, 0, 15) as usize];
    let rd = r(8, 6, 12);
    let rock = clamp(centre.0 + b.0 * rd / 1000, centre.1 + b.1 * rd / 1000);
    let windmill = (r(9, 20, 58), r(10, 4, 35));
    let oak = (r(11, 16, 50), r(12, 3, 36));
    let stream = (r(13, 50, 60), r(14, 30, 37));
    let starts = std::array::from_fn(|i| clamp(camp.0 + r(20 + i as u64, -3, 3), camp.1 + r(30 + i as u64, -3, 3)));
    let deer_at = r(15, 0, 1) as usize;
    let mut warmth = [[300; PEOPLE]; PEOPLE];
    let mut trust = [[600; PEOPLE]; PEOPLE];
    const WARM: [i32; 5] = [-300, 0, 200, 400, 600];
    const TRUST: [i32; 4] = [300, 500, 650, 800];
    for i in 0..PEOPLE { for j in 0..PEOPLE { if i != j { warmth[i][j] = WARM[r(40 + (i * 4 + j) as u64, 0, 4) as usize]; trust[i][j] = TRUST[r(60 + (i * 4 + j) as u64, 0, 3) as usize]; } } }
    Valley4 { camp, rock, windmill, oak, stream, meadows, starts, deer_at, warmth, trust }
}

/// Build the valley for a seed: things moved where the layout says, everyone founded on that.
fn world_for(seed: u64, l: &Valley4) -> World {
    let mut w = World::valley(seed);
    for t in w.things.iter_mut() {
        let p = match t.id { CAMP => l.camp, ROCK => l.rock, WINDMILL => l.windmill, OAK => l.oak, STREAM => l.stream, _ => (t.x, t.y) };
        t.x = p.0; t.y = p.1;
    }
    // The spring plays no part (nobody here knows it): it is taken out of the valley, so that it
    // doesn't surprise every hunter who passes it. (A first version left it in: found in the viewer.)
    if let Some(t) = w.things.iter_mut().find(|t| t.id == crate::people::world::SPRING) { t.alive = false; }
    w.meadows = l.meadows.clone();
    w.cover = true;
    w
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode { Planned, Solo }

/// A hunt case: seed, temperament set, planned or each alone.
#[derive(Clone, Copy, Debug)]
pub struct Case { pub seed: u64, pub set: usize, pub mode: Mode }

pub fn build(c: &Case) -> Valley {
    let l = layout(c.seed);
    let mut w = world_for(c.seed, &l);
    for (i, &(x, y)) in l.starts.iter().enumerate() { w.people[i].x = x; w.people[i].y = y; w.people[i].px = x; w.people[i].py = y; w.people[i].thirst = 0; }
    let m0 = l.meadows[l.deer_at];
    w.deer.push(Deer { x: m0.0, y: m0.1, px: m0.0, py: m0.1, hunger: 300, asleep: false, caught: None, ran: 0, missed: 0, escaped: None });
    let temps = temperaments(c.seed, c.set);
    let folk: Vec<Person> = (0..PEOPLE).map(|i| {
        let f = Founding { knows: vec![STREAM, ROCK, WINDMILL, OAK], pacing: 2, trust: l.trust[i], warmth: l.warmth[i], spring_dry: false, dry_conf: 900, ally: [0; PEOPLE], calls: CALL_HABIT };
        Person::new(&w, i, temps[i], &f)
    }).collect();
    let mut v = Valley::new(w, folk);
    v.deer_minds = vec![deer::mind(&v.w, 0)];
    for i in 0..PEOPLE { hunter::found(&mut v.folk[i], &v.w, Some(m0), 2); }
    match c.mode {
        Mode::Planned => { v.folk[ASH].hunt.as_mut().unwrap().lead = Some(Target::Deer); }
        Mode::Solo => { for i in 0..PEOPLE { start_solo(&mut v.folk[i], 0); } }
    }
    v
}

/// What happened in one hunt.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub caught: Option<(u32, usize)>,
    /// The deer got away into the woods at this tick.
    pub escaped: Option<u32>,
    /// Cells walked or run by the hunters until the catch (or the end); ticks the deer ran.
    pub effort: u32, pub deer_ran: u32, pub missed: u32,
    /// Plan: proposed at, signal at, re-plans; each member's decision (accepted, said: 0 nothing,
    /// 1 agree, 2 decline), and role.
    pub proposed: Option<u32>, pub signal: Option<u32>, pub replans: u32,
    pub joined: u32, pub declined: u32, pub silent: u32,
    /// Breaches noticed in all minds, by kind (out of place, too fast, missing), and what followed
    /// (kept, re-planned, abandoned, improvised).
    pub breaches: [u32; 3], pub reactions: [u32; 4],
    /// Ticks each hunter held a plan (summed), and the deer's learnt danger at its meadows after.
    pub held_ticks: u32, pub deer_learnt: Vec<i32>,
    pub roles: Vec<(usize, Role)>,
}

pub fn hunt(c: &Case) -> (Outcome, Valley) { hunt_rec(c, None) }
pub fn hunt_rec(c: &Case, rec: Option<usize>) -> (Outcome, Valley) { hunt_ev(c, rec, &[]) }
/// A hunt with world events added (staging a breach: someone made very thirsty partway).
pub fn hunt_ev(c: &Case, rec: Option<usize>, evs: &[(u32, Event)]) -> (Outcome, Valley) {
    let mut v = build(c);
    for (t, e) in evs { v.at(*t, e.clone()); }
    let mut v = match rec { Some(r) => v.recording(r), None => v };
    let mut o = Outcome::default();
    let order: Vec<usize> = (0..PEOPLE).collect();
    for _ in 0..HUNT_TICKS {
        let before: Vec<(i32, i32)> = v.w.people.iter().map(|p| (p.x, p.y)).collect();
        v.step(1, &order);
        for (i, p) in v.w.people.iter().enumerate() { o.effort += (p.x - before[i].0).abs().max((p.y - before[i].1).abs()) as u32; }
        for p in &v.folk { if p.m.held.intent.is_some() && p.hunt.as_ref().unwrap().plan.as_ref().is_some_and(|hp| Some(hp.node) == p.m.held.intent) { o.held_ticks += 1; } }
        if v.w.deer[0].caught.is_some() || v.w.deer[0].escaped.is_some() { break; }
    }
    finish(&mut o, &v);
    (o, v)
}

fn finish(o: &mut Outcome, v: &Valley) {
    let d = &v.w.deer[0];
    o.caught = d.caught; o.escaped = d.escaped; o.deer_ran = d.ran; o.missed = d.missed;
    if let Some(h) = v.folk[ASH].hunt.as_ref() { o.proposed = h.book.said_at.filter(|_| h.lead.is_some()); o.signal = h.book.signalled; o.replans = h.book.replans; }
    for p in &v.folk {
        let h = p.hunt.as_ref().unwrap();
        for dcs in &h.log.decisions { if dcs.2 { o.joined += 1 } else if dcs.4 == 2 { o.declined += 1 } else { o.silent += 1 } }
        for r in &h.log.reactions {
            o.breaches[(r.3 as usize).min(2)] += 1;
            if r.4 != u8::MAX { o.reactions[r.4 as usize] += 1; }
        }
        if let Some(hp) = &h.plan { if !hp.solo { o.roles.push((p.i, hp.role)); } }
    }
    o.deer_learnt = deer::meadow_danger(&v.deer_minds[0]);
    let _ = (BREACH_MISSING, BREACH_PACE);
}

// ------------------------------------------------------------------------------- betrayal

/// When Dee sets out for water (its habit, which the group knows).
pub const HABIT: u32 = 40;
pub const BETRAY_TICKS: u32 = 160;

/// Who leans towards Dee, and how: nobody; Cai (likes Dee and owes it); Bryn likewise; Cai only a
/// little (borderline).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lean { Nobody, Cai, Bryn, CaiSlight }
pub const LEANS: [Lean; 4] = [Lean::Nobody, Lean::Cai, Lean::Bryn, Lean::CaiSlight];

#[derive(Clone, Copy, Debug)]
pub struct BCase { pub seed: u64, pub set: usize, pub lean: Lean,
    /// The one who tells checks for watchers within this many cells (sight: careful).
    pub caution: i32,
    /// Honest people say nothing when they won't join; dishonest ones agree out loud and lie.
    pub honest_teller: bool }

/// The betrayal valley: the seed's layout; Ash, Bryn and Cai at the camp; Dee across the valley,
/// due to set out for the stream at `HABIT`. The group dislikes Dee and links it to danger (a
/// history of rivalry); Dee feels the same about Ash. Whoever leans towards Dee feels warmly
/// towards it and owes it a favour, and coolly towards Ash.
pub fn build_betrayal(c: &BCase) -> Valley {
    let l = layout(c.seed);
    let mut w = world_for(c.seed, &l);
    // Dee's start: far side of the stream from the camp, so its way to water is its own.
    let r = |k: u64, lo: i32, hi: i32| lo + (hash(c.seed, k, 1, 0xbe7a) % (hi - lo + 1) as u64) as i32;
    let dee0 = (r(1, 26, 46), r(2, 3, 12));
    let mut starts = l.starts;
    starts[DEE] = dee0;
    for (i, &(x, y)) in starts.iter().enumerate() { w.people[i].x = x; w.people[i].y = y; w.people[i].px = x; w.people[i].py = y; w.people[i].thirst = 0; }
    let mut warmth = l.warmth; let mut trust = l.trust;
    for i in [ASH, BRYN, CAI] { for j in [ASH, BRYN, CAI] { if i != j { warmth[i][j] = warmth[i][j].max(200); trust[i][j] = trust[i][j].max(500); } } warmth[i][DEE] = -300; trust[i][DEE] = 300; }
    warmth[DEE][ASH] = -300;
    let lean = match c.lean { Lean::Nobody => None, Lean::Cai | Lean::CaiSlight => Some(CAI), Lean::Bryn => Some(BRYN) };
    if let Some(t) = lean {
        let slight = c.lean == Lean::CaiSlight;
        warmth[t][DEE] = if slight { 350 } else { 600 }; trust[t][DEE] = 700;
        warmth[t][ASH] = if slight { 200 } else { 0 };
        warmth[DEE][t] = 600; trust[DEE][t] = 700;
    }
    let temps = temperaments(c.seed, c.set);
    let knows: [Vec<u32>; PEOPLE] = std::array::from_fn(|_| vec![STREAM, ROCK, WINDMILL, OAK]);
    let mut folk: Vec<Person> = (0..PEOPLE).map(|i| {
        let f = Founding { knows: knows[i].clone(), pacing: 2, trust: trust[i], warmth: warmth[i], spring_dry: false, dry_conf: 900, ally: [0; PEOPLE], calls: CALL_HABIT };
        Person::new(&w, i, temps[i], &f)
    }).collect();
    for p in folk.iter_mut() { hunter::found(p, &w, None, 2); }
    // The rivalry: in the group's minds Dee is linked to danger, and in Dee's mind Ash is.
    for i in [ASH, BRYN, CAI] {
        let p = &mut folk[i];
        if Some(i) != lean { let (n, d) = (p.pnode[DEE], p.ids.danger); p.m.store.add_link(n, d, lt::ASSOC, 400, 0); }
        p.hunt.as_mut().unwrap().habit = Some(HABIT);
    }
    { let p = &mut folk[DEE]; let (n, d) = (p.pnode[ASH], p.ids.danger); p.m.store.add_link(n, d, lt::ASSOC, 400, 0); }
    if let Some(t) = lean { let p = &mut folk[t]; let n = p.pnode[DEE]; p.m.store.add_link(n, p.ids.debt, lt::ASSOC, 600, 0); }
    for p in folk.iter_mut() { if p.i != DEE { p.traits.honesty = if c.honest_teller { 900 } else { 300 }; } p.hunt.as_mut().unwrap().caution = c.caution; p.m.young_from = p.m.store.len() as u32; }
    folk[ASH].hunt.as_mut().unwrap().lead = Some(Target::Person(DEE as u8));
    let mut v = Valley::new(w, folk);
    // Dee is about its own business, out of the valley, until its habit brings it out of its
    // place at `HABIT`, thirsty (staging).
    v.at(0, Event::Away { who: DEE });
    v.at(HABIT, Event::Back { who: DEE, x: dee0.0, y: dee0.1 });
    v.at(HABIT, Event::SetThirst { who: DEE, v: 800 });
    v
}

/// What happened in one betrayal run.
#[derive(Clone, Debug, Default)]
pub struct BOutcome {
    pub proposed: Option<u32>,
    /// Who decided to warn (from their own minds), and when the warning was whispered.
    pub teller: Option<usize>, pub warned_at: Option<u32>,
    /// Whether anyone of the group overheard the whisper.
    pub overheard: Vec<usize>,
    /// Dee proposed a counter-plan; the teller joined it.
    pub counter: bool, pub counter_joined: bool,
    /// Blows struck: (by, at) counts. Dee struck; group members struck.
    pub blows: Vec<(u32, usize, usize)>,
    /// Breaches in the group's minds (Ash, Bryn, Cai who weren't the teller), by kind.
    pub group_breaches: [u32; 3],
    /// Each group member's verdict (who told), and its betrayal weights per other member.
    pub verdicts: Vec<(usize, Option<usize>, Vec<(usize, i32)>)>,
    /// Trust and warmth of each group member towards the teller, before and after.
    pub trust_in_teller: Vec<(usize, i32, i32)>,
    /// Dee's first response on meeting the group, and the group's responses (first) on the breach.
    pub responses: Vec<(usize, Option<u8>)>,
    pub dee_struck: u32, pub group_struck: u32,
}

pub fn betrayal(c: &BCase) -> (BOutcome, Valley) { betrayal_rec(c, None) }
pub fn betrayal_rec(c: &BCase, rec: Option<usize>) -> (BOutcome, Valley) {
    let v = build_betrayal(c);
    let mut v = match rec { Some(r) => v.recording(r), None => v };
    let order: Vec<usize> = (0..PEOPLE).collect();
    let mut o = BOutcome::default();
    let t0: Vec<(usize, i32)> = [ASH, BRYN, CAI].iter().map(|&i| (i, 0)).collect();
    let _ = t0;
    let lean = match c.lean { Lean::Nobody => None, Lean::Cai | Lean::CaiSlight => Some(CAI), Lean::Bryn => Some(BRYN) };
    let before: Vec<(usize, i32)> = [ASH, BRYN, CAI].iter().filter(|&&i| Some(i) != lean).map(|&i| (i, lean.map(|t| v.folk[i].trust(t)).unwrap_or(0))).collect();
    let mut first_resp: [Option<u8>; PEOPLE] = [None; PEOPLE];
    for _ in 0..BETRAY_TICKS {
        v.step(1, &order);
        for i in 0..PEOPLE { if first_resp[i].is_none() { if let Some(r) = v.folk[i].m.held.response { first_resp[i] = Some(r); } } }
    }
    let h = |i: usize| v.folk[i].hunt.as_ref().unwrap();
    o.proposed = h(ASH).book.said_at;
    for i in [BRYN, CAI] { if h(i).warn.is_some() { o.teller = Some(i); o.warned_at = h(i).log.warned_at; } }
    for i in [ASH, BRYN, CAI] { if h(i).log.events.iter().any(|e| e.1.starts_with("overheard")) { o.overheard.push(i); } }
    o.counter = h(DEE).lead.is_some();
    if let Some(t) = o.teller { o.counter_joined = h(t).plan.as_ref().is_some_and(|hp| hp.by == DEE); }
    o.blows = v.w.blows.clone();
    o.dee_struck = v.w.blows.iter().filter(|b| b.2 == DEE).count() as u32;
    o.group_struck = v.w.blows.iter().filter(|b| b.2 != DEE).count() as u32;
    for i in [ASH, BRYN, CAI] {
        if Some(i) == o.teller { continue; }
        for r in &h(i).log.reactions { o.group_breaches[(r.3 as usize).min(2)] += 1; }
        let k = h(i).k;
        let ws: Vec<(usize, i32)> = [ASH, BRYN, CAI].iter().filter(|&&j| j != i).map(|&j| (j, crate::people::person::weight(&v.folk[i].m, v.folk[i].pnode[j], k.betrayal, lt::ASSOC))).collect();
        o.verdicts.push((i, h(i).verdict, ws));
    }
    if let Some(t) = lean { for &(i, tb) in &before { o.trust_in_teller.push((i, tb, v.folk[i].trust(t))); } }
    o.responses = (0..PEOPLE).map(|i| (i, first_resp[i])).collect();
    let _ = (Kind::Deer, Say::Signal);
    (o, v)
}
