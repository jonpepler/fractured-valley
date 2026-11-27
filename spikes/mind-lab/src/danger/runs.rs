//! The danger scenarios. Staging (stated, never a choice of a mind's): where people start, who is
//! away, the plan each person set out with (a node of its own, formed at the start), and where the
//! boar is put when it comes into sight. Everything after that (what each mind predicts, feels,
//! chooses, says and learns, and what the boar does) is the minds' own.
//!
//! The focal person is Bryn. Seeds move the landmarks (the valley), the spot of the meeting, the
//! side the boar comes from, where the allies stand, each person's pacing errors and which quiet
//! sounds are caught.

use super::boar;
use crate::graph::lt;
use crate::mind::{Temperament, R_NAMES};
use crate::people::person::Person;
use crate::people::scenarios::{Setup, ASH, BRYN, CAI, DEE};
use crate::people::sim::Valley;
use crate::people::world::{dist, Event, Kind, OAK};
use crate::rng::hash;
use crate::store::Store;

/// Temperaments compared: (name, make-up).
pub const TEMPERS: [(&str, Temperament); 4] = [
    ("calm", Temperament { reactivity: 150, aggression: 200 }),
    ("medium", Temperament { reactivity: 500, aggression: 500 }),
    ("reactive, timid", Temperament { reactivity: 900, aggression: 200 }),
    ("reactive, fierce", Temperament { reactivity: 900, aggression: 900 }),
];
/// The tick the boar comes into sight (after a lead-in with the plan in mind).
pub const MEET: u32 = 12;
/// Ticks watched after the meeting.
pub const WATCH: u32 = 40;

#[derive(Clone, Copy, Debug)]
pub struct Case { pub seed: u64, pub temper: Temperament, pub allies: bool, pub expected: bool }

/// Where the meeting happens and where the boar comes from, for a seed.
pub fn geometry(seed: u64) -> ((i32, i32), (i32, i32)) {
    let j = |k: u64, r: i32| (hash(seed, k, 0, 0xd4e7) % (2 * r as u64 + 1)) as i32 - r;
    let spot = (30 + j(1, 3), 15 + j(2, 2));
    const SIDES: [(i32, i32); 6] = [(8, 0), (6, -5), (6, 5), (0, -8), (-6, -5), (8, -3)];
    let (dx, dy) = SIDES[(hash(seed, 3, 0, 0xd4e7) % 6) as usize];
    (spot, (spot.0 + dx, spot.1 + dy))
}

/// Add the plan node a person set out with: a hunter's ("hunt the boar by the oak": the boar, the
/// oak's place, standing braced, fighting) or a forager's ("gather by the oak"). It is a node of
/// the person's own, and is formed as an intent.
pub fn give_plan(p: &mut Person, hunter: bool) -> u32 {
    let ids = p.ids;
    let oak = p.lex_place(OAK);
    let n = p.m.store.add_node(if hunter { "plan: hunt the boar by the oak" } else { "plan: gather by the oak" }, 200, 0, None, p.m.who);
    let mut links: Vec<(u32, u32, i32)> = vec![];
    if hunter { links.extend([(n, ids.boar, 700), (n, ids.hold, 600), (n, ids.fight, 300), (ids.boar, n, 300)]); } else { links.extend([(n, ids.water, 400)]); }
    if let Some(o) = oak { links.push((n, o, 500)); links.push((o, n, 200)); }
    for (u, v, w) in links { p.m.store.add_link(u, v, lt::ASSOC, w, 0); }
    p.m.form_intent(n);
    n
}

/// Build the valley for a case: Bryn at the spot with its plan; allies (Cai and Dee, who count on
/// one another) two cells off, or away; Ash away; the boar out of sight until `MEET`.
pub fn build(c: &Case, ally_w: i32, ally_dist: i32) -> (Valley, u32) { build_cast(c, &Cast { to_allies: ally_w, from_allies: ally_w, ally_dist, ..Cast::default() }) }
/// Who stands where and counts on whom: Bryn's ally links to Cai and Dee and theirs to Bryn, how
/// far off they stand, Bryn's habit of calling, and the others' temperament.
#[derive(Clone, Copy, Debug)]
pub struct Cast { pub to_allies: i32, pub from_allies: i32, pub ally_dist: i32, pub calls: i32, pub others: Temperament,
    /// Cai and Dee set out to hunt the boar too (each with a plan of its own).
    pub others_hunt: bool }
impl Default for Cast { fn default() -> Self { Cast { to_allies: 700, from_allies: 700, ally_dist: 2, calls: crate::people::species::CALL_HABIT, others: Temperament::MEDIUM, others_hunt: false } } }
pub fn build_cast(c: &Case, cast: &Cast) -> (Valley, u32) {
    let (ally_w, ally_dist) = (cast.to_allies, cast.ally_dist);
    let (spot, from) = geometry(c.seed);
    let mut su = Setup::new(c.seed);
    su.days = 1;
    su.temperaments = [Temperament::MEDIUM, c.temper, cast.others, cast.others];
    su.calls[BRYN] = cast.calls;
    let j = |k: u64| (hash(c.seed, k, 7, 0xa11e) % 3) as i32 - 1;
    su.pos = [(10, 22), spot, (spot.0 - ally_dist, spot.1 + 1 + j(1)), (spot.0 - ally_dist + 1 + j(2), spot.1 - 1)];
    for a in [CAI, DEE] { su.ally[BRYN][a] = ally_w; su.ally[a][BRYN] = cast.from_allies; }
    su.ally[CAI][DEE] = 700; su.ally[DEE][CAI] = 700;
    let mut v = su.build();
    for i in 0..4 { v.w.people[i].thirst = 0; }
    v.at(0, Event::Away { who: ASH });
    if !c.allies { v.at(0, Event::Away { who: CAI }); v.at(0, Event::Away { who: DEE }); }
    // The boar: out of sight, then put where it comes into view; its wallow lies behind it.
    let (wx, wy) = (from.0 + (from.0 - spot.0) / 2, from.1 + (from.1 - spot.1) / 2);
    v.w.boars.push(crate::people::world::Beast { x: 62, y: 1, px: 62, py: 1, pain: 0, asleep: false, struck: 0, braced: 0, gored: 0, routed: None });
    v.beasts.push(boar::mind(&v.w, 0, wx.clamp(0, 63), wy.clamp(0, 39)));
    let _ = from;
    let plan = give_plan(&mut v.folk[BRYN], c.expected);
    if cast.others_hunt { for j in [CAI, DEE] { give_plan(&mut v.folk[j], true); } }
    (v, plan)
}

/// Run the lead-in, then (staging) put the boar where it comes into view: on the seed's side of
/// Bryn, `d` cells off, wherever Bryn has got to. Its wallow lies behind it.
pub fn meet(v: &mut Valley, seed: u64, d: i32) {
    v.run_until(MEET);
    // (Review: the seeds varied outcomes too little.) The distance at which the boar is seen also
    // varies with the seed: `d` − 2 to `d` + 1 cells (still in sight).
    let d = d - 2 + (hash(seed, 4, 0, 0xd4e7) % 4) as i32;
    let (spot, from) = geometry(seed);
    let k = dist(spot.0, spot.1, from.0, from.1).max(1);
    let me = &v.w.people[BRYN];
    let (x, y) = ((me.x + (from.0 - spot.0) * d / k).clamp(0, 63), (me.y + (from.1 - spot.1) * d / k).clamp(0, 39));
    let q = &mut v.w.boars[0];
    q.x = x; q.y = y; q.px = x; q.py = y;
    if let Some(m) = v.beasts.first_mut() { let pn = m.place_nodes[0]; m.store.set_anchor(pn, crate::graph::Anchor::Place { x: (x + (from.0 - spot.0) / 2).clamp(0, 63) as i16, y: (y + (from.1 - spot.1) / 2).clamp(0, 39) as i16 }); }
}

/// What happened, for the focal person.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    /// Prediction error of the boar when first seen; arousal and stress just after; peak stress.
    pub error: i32, pub arousal: i32, pub stress: i32, pub stress_peak: i32,
    /// The first response chosen, and how many ticks each response was chosen over the watch.
    pub first: Option<u8>, pub counts: [u32; 6],
    /// The plan: still held 5 ticks after the meeting; dropped at (tick after meeting).
    pub plan_held: bool, pub plan_dropped: Option<u32>,
    /// Hurt (times gored), the boar struck and braced against, the boar driven off (it ran).
    pub gored: u32, pub struck: u32, pub braced: u32, pub boar_fled: bool,
    /// The tick (after the meeting) Bryn went down, if it did; responses aren't counted after it.
    pub down_at: Option<u32>,
    /// The competition's scores at the first response.
    pub first_scores: Vec<(u8, i32)>,
    /// Hops thought in the first tick after the meeting, and the frontier allowed.
    pub hops: u32, pub frontier: usize,
    /// Ally link to Cai at the end (it is learnt).
    pub ally_cai: (i32, i32),
}

/// Run one encounter.
pub fn encounter(c: &Case) -> (Outcome, Valley) { encounter_with(c, 700, 2, true) }
pub fn encounter_with(c: &Case, ally_w: i32, ally_dist: i32, record: bool) -> (Outcome, Valley) {
    encounter_cast(c, &Cast { to_allies: ally_w, from_allies: ally_w, ally_dist, ..Cast::default() }, record)
}
pub fn encounter_cast(c: &Case, cast: &Cast, record: bool) -> (Outcome, Valley) {
    let (v, plan) = build_cast(c, cast);
    let mut v = if record { v.recording(BRYN) } else { v };
    let mut o = Outcome::default();
    let ally0 = { let b = &v.folk[BRYN]; w(b, b.pnode[CAI], b.ids.ally) };
    meet(&mut v, c.seed, 8);
    let mut seen = false;
    let mut boar_d0 = 0;
    for k in 0..WATCH {
        v.run(1);
        let b = &v.folk[BRYN];
        if !seen { if let Some(p) = b.m.last.percepts.iter().find(|p| p.kind == Kind::Boar.k()) {
            seen = true; o.error = p.error; o.arousal = b.m.mods.arousal; o.stress = b.m.mods.stress; o.hops = b.m.last.hops_run; o.frontier = b.m.last.frontier_cap;
            let q = &v.w.boars[0]; let me = &v.w.people[BRYN]; boar_d0 = dist(q.x, q.y, me.x, me.y);
        } }
        o.stress_peak = o.stress_peak.max(b.m.mods.stress);
        // (Review: a downed Bryn's choices were counted though never carried out.)
        if v.w.people[BRYN].gored >= crate::people::world::DOWN_AFTER { if o.down_at.is_none() { o.down_at = Some(k); } continue; }
        if let Some(r) = b.m.held.response { o.counts[r as usize] += 1; if o.first.is_none() { o.first = Some(r); o.first_scores = b.m.held.last_scores.clone(); } }
        if k == 5 { o.plan_held = b.m.held.intent == Some(plan); }
        if o.plan_dropped.is_none() { if let Some((t, n)) = b.m.held.dropped { if n == plan { o.plan_dropped = Some(t.saturating_sub(MEET)); } } }
    }
    let q = &v.w.boars[0];
    let me = &v.w.people[BRYN];
    o.gored = v.w.people[BRYN].gored;
    o.struck = q.struck; o.braced = q.braced;
    // (Review: this used to be a distance test, which a fleeing Bryn also passed.)
    let _ = (me, boar_d0);
    o.boar_fled = q.routed.is_some();
    let b = &v.folk[BRYN];
    o.ally_cai = (ally0, w(b, b.pnode[CAI], b.ids.ally));
    (o, v)
}

pub fn w(p: &Person, u: u32, v: u32) -> i32 { p.m.store.find(u, v, lt::ASSOC).map(|k| p.m.store.slow(k) as i32).unwrap_or(0) }
pub fn rname(r: Option<u8>) -> &'static str { r.map(|r| R_NAMES[r as usize]).unwrap_or("none") }

/// Intent as a modulator, probed: a hunter alone, plan in mind, meets the boar, which (staging)
/// stands still 6 cells off, while the hunter's stress is held at `stress` from the meeting. Over
/// `WATCH` ticks: is the plan still held; when did it drop; on how many ticks did the plan's own
/// option (hold) win; what won otherwise; the focus on the first tick.
#[derive(Clone, Debug, Default)]
pub struct Focus { pub held: bool, pub dropped: Option<u32>, pub plan_ticks: u32, pub counts: [u32; 6], pub focus0: i32, pub focus_end: i32, pub plan_act: i32 }
pub fn focus_probe(seed: u64, temper: Temperament, stress: Option<i32>) -> Focus { focus_probe_with(seed, temper, stress, true) }
pub fn focus_probe_with(seed: u64, temper: Temperament, stress: Option<i32>, focus: bool) -> Focus {
    let c = Case { seed, temper, allies: false, expected: true };
    let (mut v, plan) = build(&c, 700, 2);
    v.folk[BRYN].m.cfg.focus = focus;
    v.beasts.clear();
    meet(&mut v, seed, 6);
    v.folk[BRYN].m.held.pin_stress = stress;
    let mut f = Focus::default();
    for k in 0..WATCH {
        v.run(1);
        let m = &v.folk[BRYN].m;
        if k == 0 { f.focus0 = m.mods.focus; }
        if let Some(r) = m.held.response { f.counts[r as usize] += 1; if r == crate::mind::R_HOLD && m.held.intent == Some(plan) { f.plan_ticks += 1; } }
        if f.dropped.is_none() { if let Some((t, n)) = m.held.dropped { if n == plan { f.dropped = Some(t - MEET); } } }
    }
    let m = &v.folk[BRYN].m;
    f.held = m.held.intent == Some(plan); f.focus_end = m.mods.focus; f.plan_act = m.act(plan);
    f
}

/// Calling for help: Bryn, alone and taken by surprise, has learnt to cry out (`calls`); Cai and
/// Dee are out of sight (16 cells off) but in earshot, and count on Bryn as much as
/// `from_allies`. Do they come, flee, or carry on? Returns whether Bryn called and, for each
/// hearer, (heard a cry, came within 6 cells, ran off, ticks it chose to aid, ticks it fled).
#[derive(Clone, Debug, Default)]
pub struct CallRun { pub called: u32, pub first_call: Option<u32>, pub hearers: Vec<(bool, bool, bool, u32, u32)>, pub gored: u32, pub routed: bool }
pub fn call_case(seed: u64, temper: Temperament, calls: i32, from_allies: i32, others: Temperament) -> CallRun {
    let c = Case { seed, temper, allies: true, expected: false };
    let (mut v, _) = build_cast(&c, &Cast { to_allies: 700, from_allies, ally_dist: 16, calls, others, others_hunt: false });
    meet(&mut v, seed, 8);
    let start: Vec<i32> = [CAI, DEE].iter().map(|&j| { let (a, b) = (&v.w.people[j], &v.w.people[BRYN]); dist(a.x, a.y, b.x, b.y) }).collect();
    let mut r = CallRun { hearers: vec![(false, false, false, 0, 0); 2], ..Default::default() };
    for k in 0..WATCH {
        v.run(1);
        if v.w.air.iter().any(|u| u.speaker == BRYN && u.say == crate::people::talk::Say::Help) { r.called += 1; if r.first_call.is_none() { r.first_call = Some(k); } }
        for (h, &j) in [CAI, DEE].iter().enumerate() {
            let (a, b) = (&v.w.people[j], &v.w.people[BRYN]);
            let d = dist(a.x, a.y, b.x, b.y);
            let m = &v.folk[j].m;
            if m.held.alarm.is_some() { r.hearers[h].0 = true; }
            // (Review: count coming and running off only once Bryn has called.)
            if r.first_call.is_some() {
                if d <= 6 { r.hearers[h].1 = true; }
                if d >= start[h] + 4 { r.hearers[h].2 = true; }
            }
            match m.held.response { Some(crate::mind::R_AID) => r.hearers[h].3 += 1, Some(crate::mind::R_FLEE) => r.hearers[h].4 += 1, _ => {} }
        }
    }
    r.gored = v.w.people[BRYN].gored; r.routed = v.w.boars[0].routed.is_some();
    r
}
