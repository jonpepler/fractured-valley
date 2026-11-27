//! The teaching scenarios as runs that return their numbers (the tests check and print them, the
//! viewer records some of them). Each takes a seed and a temperament set, so every outcome can be
//! checked across both.

use super::person::{error_of, Medium, Person, Told};
use super::scenarios::{bryn_told, Setup, Spring, ASH, BRYN, CAI, DAY, DEE};
use super::sim::Valley;
use super::world::{d2, dist, Event, World, CAMP, OAK, SPRING};
use crate::graph::lt;
use crate::store::Store;
use crate::think::Mods;

pub fn setup(seed: u64, set: u32) -> Setup { Setup::new(seed).temper(set) }

/// Link weight u → v of a type in a person's mind.
pub fn w(p: &Person, u: u32, v: u32, ty: u8) -> i32 { p.m.store.find(u, v, ty).map(|k| p.m.store.slow(k) as i32).unwrap_or(0) }
pub fn lval(p: &Person, u: u32, v: u32, ty: u8) -> i32 { p.m.store.find(u, v, ty).map(|k| p.m.store.link(k).lval as i32).unwrap_or(0) }

/// A non-disturbing test of use, on a copy of the valley: `who` is made thirsty (700) at the camp
/// with nobody to ask. Returns (does it walk to within 3 cells of `target` in `ticks`; over the
/// first 12 ticks, the best score its choice gave the told place, and the best it gave any other
/// water place). The second part reads the mind's own weighing, so it measures "would it rather
/// go there than to the water it already knew", even where the camp holds it back (see the note).
pub fn use_probe(v: &Valley, who: usize, target: (i32, i32), ticks: u32) -> Use {
    let mut c = v.clone();
    c.record = None;
    let t = c.w.tick;
    c.w.script.retain(|e| e.0 < t);
    c.at(t, Event::SetThirst { who, v: 700 });
    for j in 0..4 { if j != who { c.at(t, Event::Put { who: j, x: 60, y: 2 + j as i32 }); } }
    c.at(t, Event::Put { who, x: 10, y: 23 });
    let mut u = Use::default();
    for k in 0..ticks {
        c.run(1);
        let p = &c.folk[who];
        if k < 12 {
            for &(pl, a, felt) in &p.m.last.scores {
                let sc = a * felt.max(0) / 1000;
                let pi = p.m.place_nodes.iter().position(|&x| x == pl).unwrap();
                let is_target = d2(p.place_xy(pi).0, p.place_xy(pi).1, target.0, target.1) <= 9;
                let water = p.water_things().iter().any(|x| x.1 == pi);
                if is_target { u.told = u.told.max(sc); } else if water { u.other = u.other.max(sc); }
            }
        }
        let b = &c.w.people[who];
        if d2(b.x, b.y, target.0, target.1) <= 9 { u.goes = true; break; }
    }
    u
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Use { pub goes: bool, pub told: i32, pub other: i32 }
impl Use { pub fn prefers(&self) -> bool { self.told > self.other } }
pub fn goes_to(v: &Valley, who: usize, target: (i32, i32), ticks: u32) -> bool { use_probe(v, who, target, ticks).goes }

// ------------------------------------------------------------------------- 1. telling

#[derive(Clone, Debug)]
pub struct S1 { pub told: bool, pub first_drink: Option<(u32, u32)>, pub est_err: i32, pub tol: i32, pub max_act: i32, pub fact: i32, pub source: i32, pub source_lval: i32, pub confirmed: Option<u32>, pub ws_ticks: u32 }

/// Ash is asked by thirsty Bryn at the camp and tells it of the spring (or, the control, says
/// nothing). One day.
pub fn s1(seed: u64, set: u32, told: bool, trust: i32) -> (S1, Valley) {
    let mut su = setup(seed, set);
    su.days = 1;
    su.trust[BRYN][ASH] = trust;
    if !told { su.traits[ASH].helpful = -1000; }
    let mut v = su.build();
    let (mut max_act, mut ws_ticks, mut snap) = (0, 0, None);
    for _ in 0..DAY {
        v.run(1);
        if let Some(t) = bryn_told(&v) {
            let b = &v.folk[BRYN];
            max_act = max_act.max(b.m.act(t.thing)); if b.m.last.ws.contains(&t.thing) { ws_ticks += 1; }
            if snap.is_none() { snap = Some((t.clone(), w(b, t.thing, b.ids.water, lt::KIND), w(b, t.thing, b.pnode[ASH], lt::SOURCE), lval(b, t.thing, b.pnode[ASH], lt::SOURCE))); }
        }
    }
    let spring = v.w.spring();
    let r = match snap {
        Some((t, fact, source, lv)) => { let (e, tol) = error_of(&t, spring); S1 { told, first_drink: v.w.people[BRYN].first_drink, est_err: e, tol, max_act, fact, source, source_lval: lv, confirmed: bryn_told(&v).and_then(|t| t.confirmed), ws_ticks } }
        None => S1 { told, first_drink: v.w.people[BRYN].first_drink, est_err: -1, tol: -1, max_act: 0, fact: 0, source: 0, source_lval: 0, confirmed: None, ws_ticks: 0 },
    };
    (r, v)
}

// ------------------------------------------------------------------------- 2. checking

#[derive(Clone, Debug, Default)]
pub struct Confirmed { pub anchored_on_spring: bool, pub fact_after: i32, pub source_by_night: Vec<i32>, pub fact_by_night: Vec<i32>, pub trust: (i32, i32), pub warmth: (i32, i32),
    pub ash_from_spring: i32, pub ash_feeling: i32 }

/// Told, goes, finds the spring; then `nights` more days with no further talk about it.
pub fn s2_confirmed(seed: u64, set: u32, nights: u32) -> (Confirmed, Valley) {
    let mut su = setup(seed, set);
    su.days = nights + 1;
    let mut v = su.build();
    let t0 = (v.folk[BRYN].trust(ASH), v.folk[BRYN].warmth(ASH));
    let mut r = Confirmed { trust: (t0.0, 0), warmth: (t0.1, 0), ..Default::default() };
    for d in 0..=nights {
        v.run_until(d * DAY + DAY);
        if let Some(t) = bryn_told(&v) {
            let b = &v.folk[BRYN];
            r.source_by_night.push(w(b, t.thing, b.pnode[ASH], lt::SOURCE));
            r.fact_by_night.push(w(b, t.thing, b.ids.water, lt::KIND));
        }
    }
    let b = &v.folk[BRYN];
    if let Some(t) = bryn_told(&v) {
        r.anchored_on_spring = t.confirmed.is_some() && b.m.store.anchor(t.thing).is_some_and(|a| matches!(a, crate::graph::Anchor::Entity { id, .. } if id == SPRING));
        r.fact_after = w(b, t.thing, b.ids.water, lt::KIND);
        let (_, act, _) = b.m.imagine(&[(t.thing, 1000)], &Mods::default(), 2);
        r.ash_from_spring = act[b.pnode[ASH] as usize];
        r.ash_feeling = b.warmth(ASH) + lval(b, t.thing, b.pnode[ASH], lt::SOURCE);
    }
    r.trust.1 = b.trust(ASH); r.warmth.1 = b.warmth(ASH);
    (r, v)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DryCase { HonestFresh, HonestStale, LieHeard, LieUnheard }

#[derive(Clone, Debug, Default)]
pub struct Dry { pub found_dry: bool, pub arousal_peak: i32, pub ash_on_mind: bool, pub ash_act_peak: i32, pub ash_from_spring_after: i32, pub trust: (i32, i32, i32), pub warmth: (i32, i32), pub verdict: (i32, i32), pub corrected: bool,
    pub contradiction: bool, pub lie_told: bool, pub ash_feeling_after: i32 }

/// The spring is dry when Bryn gets there. Honest mistake (Ash's belief was out of date: dried
/// just after the telling, or long ago without Ash knowing) or a lie (Ash knew; Bryn may or may
/// not have overheard Ash telling Dee it was dry). Three days.
pub fn s2_dry(seed: u64, set: u32, case: DryCase) -> (Dry, Valley) {
    let mut su = setup(seed, set);
    su.days = 3;
    match case {
        DryCase::HonestFresh => su.spring = Spring::DriesAt(5),
        DryCase::HonestStale => su.spring = Spring::DryOld,
        DryCase::LieHeard | DryCase::LieUnheard => {
            su.spring = Spring::DryOld; su.ash_knows_dry = true;
            su.traits[ASH].honesty = 200; su.warmth[ASH][BRYN] = 100; su.warmth[ASH][DEE] = 600;
        }
    }
    if case == DryCase::LieHeard { su.pos[DEE] = (9, 22); }
    let mut v = su.build();
    // An errand takes Ash past the spring on day 1 (staging: what it then sees, believes and says
    // is its own). The liar already knows.
    if matches!(case, DryCase::HonestFresh | DryCase::HonestStale) { let (x, y) = v.w.spring(); v.at(DAY + 20, Event::Put { who: ASH, x: x + 2, y: y + 2 }); }
    if case == DryCase::LieHeard { v.w.people[DEE].thirst = 650; v.w.people[BRYN].thirst = 400; }
    let t0 = v.folk[BRYN].trust(ASH);
    let w0 = v.folk[BRYN].warmth(ASH);
    let mut r = Dry::default();
    let mut dry_at = None;
    let mut ws_at_dry = vec![];
    for _ in 0..3 * DAY {
        v.run(1);
        let b = &v.folk[BRYN];
        if let Some(t) = bryn_told(&v) {
            if let (Some(d), None) = (t.dry, dry_at) { dry_at = Some(d); r.trust.1 = b.trust(ASH); }
            if let Some(d) = dry_at { if v.w.tick <= d + 4 { r.arousal_peak = r.arousal_peak.max(b.m.mods.arousal); if b.m.last.ws.contains(&b.pnode[ASH]) { r.ash_on_mind = true; } r.ash_act_peak = r.ash_act_peak.max(b.m.act(b.pnode[ASH])); ws_at_dry.push(b.m.last.ws.iter().map(|&n| (b.m.name(n), b.m.act(n))).collect::<Vec<_>>()); } }
        }
    }
    let b = &v.folk[BRYN];
    if let Some(t) = bryn_told(&v) {
        r.found_dry = t.dry.is_some();
        r.verdict = b.verdict(t.thing);
        r.corrected = t.corrected;
        r.contradiction = t.self_contradiction;
        r.ash_feeling_after = lval(b, t.thing, b.pnode[ASH], lt::SOURCE) + b.warmth(ASH);
        let (_, act, _) = b.m.imagine(&[(t.thing, 1000)], &Mods::default(), 2);
        r.ash_from_spring_after = act[b.pnode[ASH] as usize];
    }
    r.lie_told = v.folk[ASH].said[super::person::SAID_LIE] > 0;
    if std::env::var("FV_DEBUG").is_ok() { for w in &ws_at_dry { eprintln!("ws at dry {w:?}"); } }
    r.trust = (t0, r.trust.1, b.trust(ASH));
    r.warmth = (w0, b.warmth(ASH));
    (r, v)
}

// ------------------------------------------------------------------------- 3. conflict

/// Bryn already believes the spring's place is dry (at `prior` confidence); Ash (trusted at
/// `trust`) says it holds water. Does Bryn go, thirsty? (It is in fact wet.)
pub fn s3_conflict(seed: u64, set: u32, trust: i32, prior: i32) -> (bool, Option<(i32, i32)>, Option<u32>) {
    let mut su = setup(seed, set);
    su.days = 1;
    su.trust[BRYN][ASH] = trust;
    su.bryn_prior_dry = prior;
    let mut v = su.build();
    v.run(DAY);
    let t = v.folk[BRYN].told.iter().find(|t| t.from == Some(ASH)).cloned();
    let drank_spring = v.w.people[BRYN].first_drink.is_some_and(|d| d.1 == SPRING);
    (drank_spring, t.as_ref().and_then(|t| t.conflict), t.and_then(|t| t.confirmed))
}

// ------------------------------------------------------------------------- 4. passing it on

#[derive(Clone, Debug, Default)]
pub struct Chain { pub bryn_err: i32, pub cai_err: i32, pub cai_tol: i32, pub cai_refs_grounded: bool, pub cai_from_bryn: i32, pub cai_from_ash: i32, pub cai_drank_spring: bool, pub via_said: bool, pub cai_source_links: usize }

/// Ash tells Bryn. Cai arrives and asks Bryn: before Bryn has been (`confirmed_first` false) or
/// the next morning, after Bryn found it.
pub fn s4_chain(seed: u64, set: u32, confirmed_first: bool) -> (Chain, Valley) {
    let mut su = setup(seed, set);
    su.days = 3;
    su.pos[CAI] = (24, 6);
    let mut v = su.build();
    v.w.people[CAI].thirst = 0;
    let arrive = if confirmed_first { DAY + 2 } else { 3 };
    v.at(arrive, Event::Put { who: CAI, x: 13, y: 24 });
    if confirmed_first { v.at(arrive, Event::Put { who: BRYN, x: 12, y: 23 }); }
    v.at(arrive, Event::SetThirst { who: CAI, v: 650 });
    v.run_until(3 * DAY);
    let spring = v.w.spring();
    let mut r = Chain::default();
    if let Some(t) = bryn_told(&v) { r.bryn_err = error_of(t, spring).0; }
    let c = &v.folk[CAI];
    if let Some(t) = c.told.iter().find(|t| t.from == Some(BRYN) && !t.dry_claim) {
        r.cai_refs_grounded = true;
        let (e, tol) = error_of(t, spring); r.cai_err = e; r.cai_tol = tol;
        r.cai_from_bryn = w(c, t.thing, c.pnode[BRYN], lt::SOURCE);
        r.cai_from_ash = w(c, t.thing, c.pnode[ASH], lt::SOURCE);
        r.via_said = t.via == Some(ASH);
        r.cai_source_links = c.m.store.links_vec(t.thing).iter().filter(|l| l.ty == lt::SOURCE).count();
    } else { r.cai_err = -1; }
    r.cai_drank_spring = v.w.people[CAI].first_drink.is_some_and(|d| d.1 == SPRING);
    (r, v)
}

#[derive(Clone, Debug, Default)]
pub struct Eaves { pub dee_heard: bool, pub dee_err: i32, pub dee_from_ash: i32, pub dee_drank_spring: bool, pub control_drank_spring: bool, pub ash_model_dee: (i32, i32), pub ash_reply_to_dee: Option<String> }

/// Dee stands within earshot (but isn't addressed) when Ash tells Bryn; later Dee is thirsty.
/// The next morning Dee asks Ash. Control: Dee out of earshot.
pub fn s4_eavesdrop(seed: u64, set: u32, dee_knows_water: bool) -> (Eaves, Valley) {
    let run = |near: bool| {
        let mut su = setup(seed, set);
        if !dee_knows_water { su.knows[DEE].retain(|&x| x != super::world::STREAM); }
        su.days = 2;
        su.pos[DEE] = if near { (17, 25) } else { (40, 30) };
        let mut v = su.build();
        v.w.people[DEE].thirst = 0;
        v.at(60, Event::SetThirst { who: DEE, v: 700 });
        v.at(DAY + 1, Event::Put { who: DEE, x: 11, y: 24 });
        v.at(DAY + 1, Event::SetThirst { who: DEE, v: 650 });
        v
    };
    let mut v = run(true);
    v.run_until(DAY);
    let mut r = Eaves::default();
    let d = &v.folk[DEE];
    if let Some(t) = d.told.iter().find(|t| t.from == Some(ASH) && t.overheard) {
        r.dee_heard = true; r.dee_err = error_of(t, v.w.spring()).0; r.dee_from_ash = w(d, t.thing, d.pnode[ASH], lt::SOURCE);
    }
    r.dee_drank_spring = v.w.people[DEE].first_drink.is_some_and(|x| x.1 == SPRING);
    let a = &v.folk[ASH];
    let spring_node = a.m.entity_nodes.iter().find(|x| x.0 == SPRING).map(|x| x.1).unwrap();
    r.ash_model_dee.0 = w(a, a.pnode[DEE], spring_node, lt::ASSOC);
    let n0 = v.w.said.len();
    v.run_until(2 * DAY);
    let a = &v.folk[ASH];
    r.ash_model_dee.1 = w(a, a.pnode[DEE], spring_node, lt::ASSOC);
    r.ash_reply_to_dee = v.w.said[n0..].iter().find(|u| u.speaker == ASH && u.to == Some(DEE)).map(|u| u.say.render());
    let mut c = run(false);
    c.run_until(DAY);
    r.control_drank_spring = c.w.people[DEE].first_drink.is_some_and(|x| x.1 == SPRING);
    (r, v)
}

// ------------------------------------------------------------------------- 5. forgetting

/// A life in which the told fact isn't rehearsed: Bryn is never thirsty enough to think of water
/// (its thirst is reset every 50 ticks), and Ash, the source, goes away for good soon after the
/// telling (out of sight and not thinking: staging). Without this, thinking of water every
/// evening and seeing Ash every day rehearse both links.
pub fn keep_quiet(v: &mut Valley, nights: u32) {
    for t in (10..(nights + 1) * DAY).step_by(50) { v.at(t, Event::SetThirst { who: BRYN, v: 0 }); }
    v.at(5, Event::Away { who: ASH });
}

#[derive(Clone, Debug, Default)]
pub struct Fade { pub fact: Vec<i32>, pub source: Vec<i32>, pub uses: Vec<(u32, Use)>, pub source_gone: Option<u32>, pub fact_gone: Option<u32> }

/// Told once, never goes (not thirsty again), `nights` nights. The source link and the fact are
/// tracked night by night; on probe nights a copy of Bryn is made thirsty to see if it would go.
pub fn s5_fade(seed: u64, set: u32, trust: i32, warmth: i32, nights: u32, probes: &[u32], quiet: bool) -> Fade { s5_fade_with(seed, set, trust, warmth, nights, probes, quiet, STAGE3) }
/// `s5_fade` with stage 2's or stage 3's choosing (the home-pull regression).
#[allow(clippy::too_many_arguments)]
pub fn s5_fade_with(seed: u64, set: u32, trust: i32, warmth: i32, nights: u32, probes: &[u32], quiet: bool, c: Choosing) -> Fade {
    let mut su = setup(seed, set);
    su.days = nights + 1;
    su.trust[BRYN][ASH] = trust;
    su.warmth[BRYN][ASH] = warmth;
    let mut v = su.build();
    set_choosing(&mut v, c);
    v.at(3, Event::SetThirst { who: BRYN, v: 0 });
    if quiet { keep_quiet(&mut v, nights); } else { for d in 1..=nights { v.at(d * DAY, Event::SetThirst { who: BRYN, v: 0 }); } }
    let mut r = Fade::default();
    for d in 0..=nights {
        v.run_until(d * DAY + DAY);
        let Some(t) = bryn_told(&v).cloned() else { break };
        let b = &v.folk[BRYN];
        let (f, s) = (w(b, t.thing, b.ids.water, lt::KIND), w(b, t.thing, b.pnode[ASH], lt::SOURCE));
        r.fact.push(f); r.source.push(s);
        if s == 0 && r.source_gone.is_none() { r.source_gone = Some(d + 1); }
        if f == 0 && r.fact_gone.is_none() { r.fact_gone = Some(d + 1); }
        if probes.contains(&(d + 1)) { let xy = v.folk[BRYN].place_xy(t.place); r.uses.push((d + 1, use_probe(&v, BRYN, xy, 70))); }
    }
    r
}

// ------------------------------------------------------------------------- 6. notes

#[derive(Clone, Debug, Default)]
pub struct NoteRun { pub fact_first: i32, pub note_has_it: i32, pub fact_by_night: Vec<i32>, pub rereads_by_day: Vec<u32>, pub note_has_it_by_night: Vec<i32>, pub used_at_end: Use }

/// Ash writes (or says) it; Bryn keeps the note, loses it on day `lose_on`, or (spoken) has none.
/// Bryn never goes: its thirst is reset each morning, so it only grows thirsty towards evening,
/// which is when thought of water may reach the note. `nights` nights. `note_has_it_by_night` is
/// the fact → note link ("the note has it").
pub fn s6_note(seed: u64, set: u32, medium: Medium, lose_on: Option<u32>, nights: u32, offload: bool, quiet: bool) -> (NoteRun, Valley) {
    let mut su = setup(seed, set);
    su.days = nights + 1;
    su.medium = medium;
    su.offload = offload;
    let mut v = su.build();
    v.at(4, Event::SetThirst { who: BRYN, v: 0 });
    if quiet { keep_quiet(&mut v, nights); } else { for d in 1..=nights { v.at(d * DAY, Event::SetThirst { who: BRYN, v: 0 }); } }
    // Lost where Bryn won't come on it again (dropped, then carried off by the wind: staging).
    if let Some(d) = lose_on { v.at(d * DAY + 50, Event::LoseNotes { who: BRYN }); v.at(d * DAY + 51, Event::MoveNote { id: 100, x: 62, y: 1 }); }
    let mut r = NoteRun::default();
    v.run_until(6);
    if let Some(t) = bryn_told(&v) {
        let b = &v.folk[BRYN];
        r.fact_first = w(b, t.thing, b.ids.water, lt::KIND);
        r.note_has_it = b.notes.first().map(|n| w(b, n.1, t.thing, lt::ASSOC)).unwrap_or(0);
    }
    for d in 0..nights {
        v.run_until(d * DAY + DAY);
        let b = &v.folk[BRYN];
        if let Some(t) = bryn_told(&v) { r.fact_by_night.push(w(b, t.thing, b.ids.water, lt::KIND)); }
        r.rereads_by_day.push(b.reads.iter().filter(|&&x| x >= d * DAY && x < d * DAY + DAY).count() as u32);
        r.note_has_it_by_night.push(b.notes.first().map(|n| w(b, t_thing(b), n.1, lt::ASSOC)).unwrap_or(0));
    }
    if let Some(t) = bryn_told(&v) { let xy = v.folk[BRYN].place_xy(t.place); r.used_at_end = use_probe(&v, BRYN, xy, 70); }
    (r, v)
}
fn t_thing(b: &Person) -> u32 { b.told.iter().find(|t| t.from == Some(ASH)).map(|t| t.thing).unwrap_or(0) }

#[derive(Clone, Debug, Default)]
pub struct Found { pub fact_before: i32, pub note_link_before: i32, pub cue_act: i32, pub fact_after_one_read: i32, pub fresh_after_one_read: i32, pub used_after: Use, pub used_before: Use,
    pub other_reads: bool, pub other_err: i32, pub other_drank_spring: bool, pub other_source: i32 }

/// The note is lost on day 1 at the camp; Bryn comes upon it on day `find_on` (it is put where
/// Bryn stands). Savings: the fact after that one reading, against a Bryn reading it fresh. And
/// someone else (Cai) finding a lost note.
pub fn s6_found(seed: u64, set: u32, find_on: u32) -> Found {
    let mut su = setup(seed, set);
    su.days = find_on + 2;
    su.medium = Medium::Note;
    let mut v = su.build();
    v.at(4, Event::SetThirst { who: BRYN, v: 0 });
    keep_quiet(&mut v, find_on + 1);
    v.at(DAY + 10, Event::LoseNotes { who: BRYN });
    v.at(DAY + 11, Event::MoveNote { id: 100, x: 62, y: 1 });
    let mut r = Found::default();
    v.run_until(find_on * DAY + 5);
    let t = bryn_told(&v).cloned().unwrap();
    let b = &v.folk[BRYN];
    r.fact_before = w(b, t.thing, b.ids.water, lt::KIND);
    r.note_link_before = b.notes.first().map(|n| w(b, n.1, t.thing, lt::ASSOC)).unwrap_or(0);
    r.used_before = use_probe(&v, BRYN, b.place_xy(t.place), 70);
    let (bx, by) = (v.w.people[BRYN].x, v.w.people[BRYN].y);
    let at = v.w.tick;
    v.at(at, Event::MoveNote { id: 100, x: bx + 3, y: by });
    v.run(1);
    r.cue_act = v.folk[BRYN].m.act(t.thing);
    v.at(v.w.tick, Event::MoveNote { id: 100, x: v.w.people[BRYN].x, y: v.w.people[BRYN].y });
    let reads0 = v.folk[BRYN].reads.len();
    for _ in 0..4 { v.run(1); if v.folk[BRYN].reads.len() > reads0 { break; } }
    let b = &v.folk[BRYN];
    r.fact_after_one_read = w(b, t.thing, b.ids.water, lt::KIND);
    r.used_after = use_probe(&v, BRYN, b.place_xy(t.place), 70);
    // Fresh: a Bryn who was never told reads the same note for the first time.
    let mut f = setup(seed, set);
    f.days = 1; f.traits[ASH].helpful = -1000;
    let mut fv = f.build();
    fv.w.people[BRYN].thirst = 0;
    let note = v.w.notes[0].clone();
    fv.w.notes.push(super::world::Note { holder: None, x: fv.w.people[BRYN].x, y: fv.w.people[BRYN].y, ..note.clone() });
    for _ in 0..4 { fv.run(1); if !fv.folk[BRYN].reads.is_empty() { break; } }
    let fb = &fv.folk[BRYN];
    r.fresh_after_one_read = fb.told.first().map(|t| w(fb, t.thing, fb.ids.water, lt::KIND)).unwrap_or(0);
    // Someone else finds a lost note: Cai, at the camp, with the note at its feet.
    let mut c = setup(seed, set);
    c.days = 1; c.traits[ASH].helpful = -1000; c.pos[CAI] = (14, 24);
    let mut cv = c.build();
    cv.w.people[BRYN].thirst = 0; cv.w.people[CAI].thirst = 0;
    cv.w.notes.push(super::world::Note { holder: None, x: 14, y: 24, ..note });
    cv.at(30, Event::SetThirst { who: CAI, v: 700 });
    cv.run(DAY);
    let cp = &cv.folk[CAI];
    if let Some(t) = cp.told.first() {
        r.other_reads = true; r.other_err = error_of(t, cv.w.spring()).0; r.other_source = w(cp, t.thing, cp.pnode[ASH], lt::SOURCE);
    }
    r.other_drank_spring = cv.w.people[CAI].first_drink.is_some_and(|d| d.1 == SPRING);
    r
}

// ------------------------------------------------------------------------- 7. maps

#[derive(Clone, Debug, Default)]
pub struct MapRun { pub err: i32, pub tol: i32, pub refs: usize, pub find_ticks: Option<u32>, pub trust: (i32, i32), pub checks: (u32, u32), pub fact_night_20: i32, pub used_night_20: Use, pub cues: usize, pub drank_spring: bool }

/// Spoken, written or drawn, Bryn goes on day 0; how precisely was it placed, how long to find,
/// how far is it trusted, and (a second valley where Bryn never goes) how well it's remembered.
pub fn s7_map(seed: u64, set: u32, medium: Medium, offload: bool) -> MapRun {
    let mut su = setup(seed, set);
    su.days = 1; su.medium = medium; su.offload = offload;
    let mut v = su.build();
    let t0 = v.folk[BRYN].trust(ASH);
    // Trust right after hearing or reading it, before any visit.
    let mut t1 = t0;
    for _ in 0..8 { v.run(1); if bryn_told(&v).is_some() { t1 = v.folk[BRYN].trust(ASH); break; } }
    v.run_until(DAY);
    let mut r = MapRun { trust: (t0, t1), ..Default::default() };
    if let Some(t) = bryn_told(&v) {
        let (e, tol) = error_of(t, v.w.spring());
        r.err = e; r.tol = tol; r.find_ticks = t.confirmed.map(|c| c - t.at); r.checks = t.landmark_checks;
        let b = &v.folk[BRYN];
        r.cues = b.m.store.links_vec(b.m.place_nodes[t.place]).iter().filter(|l| l.ty == lt::NEAR && l.slow > 0).count();
    }
    r.drank_spring = v.w.people[BRYN].first_drink.is_some_and(|d| d.1 == SPRING);
    let mut su = setup(seed, set);
    su.days = 21; su.medium = medium; su.offload = offload;
    let mut v = su.build();
    v.at(4, Event::SetThirst { who: BRYN, v: 0 });
    keep_quiet(&mut v, 20);
    v.run_until(20 * DAY);
    if let Some(t) = bryn_told(&v).cloned() {
        let b = &v.folk[BRYN];
        r.fact_night_20 = w(b, t.thing, b.ids.water, lt::KIND);
        r.used_night_20 = use_probe(&v, BRYN, b.place_xy(t.place), 70);
    }
    r
}

// ------------------------------------------------------------------------- 8. teacher

#[derive(Clone, Debug, Default)]
pub struct Teacher { pub thanked: bool, pub ash_warmth: (i32, i32), pub bryn_debt: i32, pub shared_back: Option<bool>, pub tells: u32, pub reminds: u32 }

/// Bryn is helped (told, finds the spring, thanks Ash) or not (control). On day 1 Ash, curious
/// about the old oak, asks Bryn where it is: does Bryn tell (it is moderately helpful, 300)? On
/// days 1 and 2 Bryn asks Ash for water again: does Ash repeat itself (`model_on` off) or remind?
pub fn s8_teacher(seed: u64, set: u32, helped: bool, model_on: bool) -> Teacher {
    let mut su = setup(seed, set);
    su.days = 3;
    su.model_on = model_on;
    su.traits[BRYN].helpful = 300;
    if !helped { su.traits[ASH].helpful = -1000; }
    let mut v = su.build();
    let w0 = v.folk[ASH].warmth(BRYN);
    v.run_until(DAY + 1);
    if !helped { v.folk[ASH].traits.helpful = 900; }
    let mut r = Teacher { ash_warmth: (w0, v.folk[ASH].warmth(BRYN)), thanked: v.folk[BRYN].said[super::person::SAID_THANKS] > 0, bryn_debt: v.folk[BRYN].debt(ASH), ..Default::default() };
    v.folk[ASH].curious = vec![OAK];
    // Morning of day 1 at the camp: Ash asks about the oak; later Bryn is thirsty and asks again.
    v.at(DAY + 2, Event::Put { who: ASH, x: 10, y: 23 }); v.at(DAY + 2, Event::Put { who: BRYN, x: 12, y: 23 });
    // Only the two of them at the camp today, so Ash's question is put to Bryn.
    for j in [CAI, DEE] { v.at(DAY + 2, Event::Put { who: j, x: 62, y: 38 }); v.at(DAY + super::scenarios::DUSK + 1, Event::Put { who: j, x: 62, y: 38 }); }
    v.folk[ASH].last_ask = None;
    v.at(DAY + 20, Event::SetThirst { who: BRYN, v: 650 });
    v.at(2 * DAY + 2, Event::Put { who: ASH, x: 10, y: 23 }); v.at(2 * DAY + 2, Event::Put { who: BRYN, x: 12, y: 23 });
    v.at(2 * DAY + 3, Event::SetThirst { who: BRYN, v: 650 });
    let n0 = v.w.said.len();
    let said0 = v.folk[ASH].said;
    v.run_until(3 * DAY);
    let s = &v.w.said[n0..];
    // The answer to Ash's first question (it asks again until it knows).
    r.shared_back = s.iter().find(|u| u.speaker == ASH && u.to == Some(BRYN) && matches!(u.say, super::talk::Say::AskOther(_)))
        .map(|q| s.iter().any(|u| u.speaker == BRYN && u.tick == q.tick + 1 && matches!(u.say, super::talk::Say::TellOther(..))));
    let a = &v.folk[ASH];
    r.tells = a.said[super::person::SAID_TELL] - said0[super::person::SAID_TELL];
    r.reminds = a.said[super::person::SAID_REMIND] - said0[super::person::SAID_REMIND];
    r
}

/// The camp and spring of a world (for reports).
pub fn where_things(w: &World) -> String { let c = w.thing(CAMP); let s = w.spring(); format!("camp ({}, {}), spring ({}, {}), {} cells apart", c.x, c.y, s.0, s.1, dist(c.x, c.y, s.0, s.1)) }
/// Unused here; keeps the told record type in the public interface of the runs.
pub type TellRecord = Told;

// ------------------------------------------------------------------------- stage 3: home's pull

/// Stage 3's choosing settings, to compare against stage 2's: (comfort gated by need, walking cost,
/// commitment). `STAGE2` is all off.
pub type Choosing = (bool, i32, bool);
pub const STAGE2: Choosing = (false, 0, false);
pub const STAGE3: Choosing = (true, crate::mind::WALK_COST, true);
pub fn set_choosing(v: &mut Valley, c: Choosing) {
    for p in v.folk.iter_mut() { p.m.cfg.comfort_gate = c.0; p.m.cfg.walk_cost = c.1; p.m.cfg.hold_goal = c.2; }
}

#[derive(Clone, Debug, Default)]
pub struct HomePull { pub camp_comfort: i32, pub camp_rest: i32, pub left_at: Option<u32>, pub drank_at: Option<u32>, pub dithers: u32 }

/// Stage 2's finding, as a probe: Bryn spends `days` quiet days at the camp (never thirsty, nobody
/// talks: Ash is away), then wakes thirsty (700) with nobody near. Does home hold it? Returns the
/// camp's learnt links to comfort and rest, the tick (from waking) it first stands more than 3
/// cells from the camp and first drinks, and how often its walk reversed (a step towards a place,
/// then straight back: dithering).
pub fn home_pull(seed: u64, set: u32, days: u32, c: Choosing) -> HomePull {
    let mut su = setup(seed, set);
    su.days = days + 1;
    su.pos = [(10, 23), (10, 22), (55, 34), (54, 35)];
    let mut v = su.build();
    set_choosing(&mut v, c);
    v.at(0, Event::Away { who: ASH }); v.at(0, Event::Away { who: CAI }); v.at(0, Event::Away { who: DEE });
    for t in (0..days * DAY).step_by(25) { v.at(t, Event::SetThirst { who: BRYN, v: 0 }); }
    v.run_until(days * DAY);
    v.at(days * DAY, Event::SetThirst { who: BRYN, v: 700 });
    let b = &v.folk[BRYN];
    let cn = b.lex_place(CAMP).unwrap();
    let mut r = HomePull { camp_comfort: w(b, cn, b.ids.comfort, lt::ASSOC), camp_rest: w(b, cn, b.ids.rest, lt::ASSOC), ..Default::default() };
    let drinks0 = v.w.people[BRYN].drinks;
    let mut path: Vec<(i32, i32)> = vec![];
    for k in 0..120 {
        v.run(1);
        let p = &v.w.people[BRYN];
        path.push((p.x, p.y));
        if r.left_at.is_none() && dist(p.x, p.y, 10, 22) > 3 { r.left_at = Some(k); }
        if r.drank_at.is_none() && p.drinks > drinks0 { r.drank_at = Some(k); break; }
    }
    // A reversal: three positions where the middle step is undone by the next.
    r.dithers = path.windows(3).filter(|x| x[0] == x[2] && x[0] != x[1]).count() as u32;
    r
}
