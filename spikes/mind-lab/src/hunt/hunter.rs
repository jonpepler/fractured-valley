//! A person who hunts: the stage-4 layer around a person's mind, as `people::person` is the
//! stage-2 layer. Everything here reads only what this person senses (its percepts, what it hears)
//! and its own mind, and writes only this person. **Nothing moves a group**: a plan is words in
//! the air; each hearer grounds the places in its own landmarks, builds its own plan node, decides
//! for itself whether to join, and acts on its own role only while the plan is held in its mind.
//!
//! - **Hearing a plan** (`hear_plan`): the places are grounded as a told place is in teaching
//!   (`Person::ground_refs`); the hearer adds a plan node of its own, linked to the quarry, its
//!   role, its own spot, the proposer (a source link carrying trust) and each other member. Whether
//!   it joins is read off its own mind: how the plan feels to it now (`Mind::felt`, which carries
//!   its warmth and trust towards the proposer, the quarry's worth, the role) less what its body
//!   needs. It agrees out loud, declines, or says nothing.
//! - **A plan is a prediction** (`predict`): joining sets the mind's `PlanState` expectations:
//!   where each member will be (hidden, for those waiting), where the quarry will be and how fast
//!   it may move, and, after the signal, that drivers and quarry will be on the move. A broken
//!   prediction is a prediction error in the shared mind (surprise, damping, and the mind's own
//!   "broken plan" node, which links to danger).
//! - **Acting on a role** (`act`): while the plan is the mind's held intent and no danger response
//!   has the body, go to one's spot, wait (keeping still by cover hides you), drive on the signal,
//!   strike the quarry in reach. These are hand-written rules (listed in the stage note).
//! - **The leader** (whoever has the idea: staging) composes the plan from its own map, proposes
//!   it, keeps count of who agreed, whistles when it expects everyone in place, and re-plans out
//!   loud (rules) when someone declines or when its own mind registers a breach.
//! - **The betrayal**: a hearer whose feeling for the plan's target outweighs its feeling for the
//!   proposer forms a counter-intent instead: it seeks the target out and tells it the plan in a
//!   whisper, out of the others' sight. The target grounds the plan (as predictions of its own),
//!   and proposes a counter-plan to the one who warned it. Afterwards the group weighs evidence of
//!   who told (`evidence`), as teaching weighs a lie.

use crate::graph::{lt, tag, Anchor};
use crate::mind::{key, Action, Expect, Sense, BREACH_MISSING, IGNORE_BELOW};
use crate::people::person::{strengthen, weight, Person};
use crate::people::talk::{band_dir, from_ref, Plan, Ref, Role, Say, Target};
use crate::people::world::{d2, dist, name, person_id, Kind, Utterance, World, DEER_ID, GRID_H, GRID_W, LOW, SHOUT, SPEAK, TALK, WHISPER};
use crate::store::Store;
use crate::think::Form;

/// A person's own hunting concepts (nodes of its own overlay: hunting know-how, founded the same
/// for every hunter, never shared).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Know { pub deer_kind: u32, pub deer: u32, pub drive: u32, pub wait: u32, pub signal: u32, pub broken: u32, pub betrayal: u32 }

/// Hash a list of `usize` item by item (std hashes `[usize]` as raw bytes: 8 a value native, 4 on
/// wasm32, which parted the builds in stage 3 and again here).
fn hash_usizes<H: std::hash::Hasher>(v: &[usize], h: &mut H) { use std::hash::Hash; v.len().hash(h); for &x in v { (x as u64).hash(h); } }

/// A plan as this mind holds it: its own nodes and its own grounding of every place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub node: u32, pub rev: u8, pub by: usize, pub target: Target, pub role: Role,
    /// My spot (own place index) and where I believe it is, with its uncertainty.
    pub place: usize, pub at: (i32, i32, i32),
    /// Where I believe the quarry is (or will pass), with uncertainty.
    pub tgt: (i32, i32, i32),
    /// Every member as I grounded them: (who, role, x, y, tol).
    pub members: Vec<(usize, Role, i32, i32, i32)>,
    pub formed: u32,
    /// When I heard (or gave) the signal.
    pub signal: Option<u32>,
    /// Members I heard agree.
    pub agreed: Vec<usize>,
    /// A plan of my own, alone (the uncoordinated condition).
    pub solo: bool,
}

impl std::hash::Hash for Held {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.node.hash(h); self.rev.hash(h); (self.by as u64).hash(h); self.target.hash(h); self.role.hash(h); (self.place as u64).hash(h); self.at.hash(h); self.tgt.hash(h);
        self.members.len().hash(h); for m in &self.members { (m.0 as u64).hash(h); m.1.hash(h); m.2.hash(h); m.3.hash(h); m.4.hash(h); }
        self.formed.hash(h); self.signal.hash(h); hash_usizes(&self.agreed, h); self.solo.hash(h);
    }
}

/// The leader's book: the plan it proposed and what it heard back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lead { pub rev: u8, pub said_at: Option<u32>, pub declined: Vec<usize>, pub agreed: Vec<usize>, pub signalled: Option<u32>, pub replans: u32, pub breaches_seen: usize, pub gave_up: bool }

impl std::hash::Hash for Lead {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.rev.hash(h); self.said_at.hash(h); hash_usizes(&self.declined, h); hash_usizes(&self.agreed, h); self.signalled.hash(h); self.replans.hash(h); (self.breaches_seen as u64).hash(h); self.gave_up.hash(h);
    }
}

/// What a person did and noticed, for measures (never read by its mind).
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Log {
    /// (tick, plan rev, accepted, score, said: 0 nothing, 1 agree, 2 decline).
    pub decisions: Vec<(u32, u8, bool, i32, u8)>,
    /// (tick, what): role phases, signals, strikes, warnings.
    pub events: Vec<(u32, String)>,
    /// Cells moved, and ticks on which the body ran or charged.
    pub moved: u32,
    /// Breaches: (tick, node, error, kind) copied from the mind, and what followed within
    /// `REACT_WINDOW` ticks: 0 kept the plan, 1 re-planned out loud, 2 abandoned it, 3 improvised.
    pub reactions: Vec<(u32, u32, i32, u8, u8)>,
    pub warned_at: Option<u32>,
    pub improvised: u32,
    /// (Review) The ticks it improvised on, and the ticks a breach of its own mind's made it
    /// re-plan out loud (an early whistle or a new plan); ordinary whistles don't count.
    pub improvised_at: Vec<u32>,
    pub replan_at: Vec<u32>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Hunter {
    pub k: Know,
    /// Staging: this person has the idea of the hunt (it will propose a plan for this target).
    pub lead: Option<Target>,
    /// The uncoordinated condition: hunts alone with a plan of its own, never proposes.
    pub solo: bool,
    pub plan: Option<Held>,
    /// A plan I know of that is against me (a warning): its predictions are mine.
    pub against_me: Option<Held>,
    pub book: Lead,
    /// Counter-intent: tell this person the plan (the betrayal), and the plan to tell.
    pub warn: Option<(usize, u32, Plan)>,
    /// How close a member of the group may be (in my sight) when I whisper the plan away.
    pub caution: i32,
    /// Plans heard (latest per proposer and revision) so a repeat isn't re-grounded.
    pub heard: Vec<(usize, u8)>,
    pub out: Vec<(Say, Option<usize>, i32)>,
    pub log: Log,
    /// The breaches of my mind's I have already looked at.
    pub seen_breaches: usize,
    /// My verdict: who told (if decided).
    pub verdict: Option<usize>,
    pub blamed: bool,
    /// Where my body was last tick.
    pub last_xy: (i32, i32),
    /// What this mind knows of the target's habit: when it sets out for water (staging, like trust).
    pub habit: Option<u32>,
    /// When I last struck at the deer (so I notice I caught it: it is gone from where I struck).
    pub struck: Option<u32>,
    /// Who (my nodes) was in my sight last tick (to notice someone arriving).
    pub in_sight: Vec<u32>,
}

/// A plan is joined if it feels at least this good, after what the body needs.
pub const ACCEPT_AT: i32 = 0;
/// A target I feel this much better about than the proposer, I warn instead.
pub const LOYAL: i32 = 100;
/// The driver's spot: this many cells beyond the quarry from the cover.
pub const DRIVE_D: i32 = 8;
/// Cover counts if it is this far from the quarry (cells).
pub const COVER_MIN: i32 = 4;
pub const COVER_MAX: i32 = 18;
/// What followed a breach is read this many ticks later.
pub const REACT_WINDOW: u32 = 15;
/// A timed prediction (a person passing a spot) is open this many ticks after its expected time.
pub const PASS_WINDOW: u32 = 25;
/// Founding: how strongly a plan's quarry and members are linked to it, and its effort.
pub const PLAN_TARGET: i32 = 700;
pub const PLAN_PLACE: i32 = 600;
pub const PLAN_MEMBER: i32 = 300;
pub const EFFORT: [i32; 2] = [400, 150];

impl Hunter {
    pub fn new(k: Know) -> Hunter {
        Hunter { k, lead: None, solo: false, plan: None, against_me: None, book: Lead::default(), warn: None, caution: crate::people::world::SIGHT, heard: vec![], out: vec![],
            log: Log::default(), seen_breaches: 0, verdict: None, blamed: false, last_xy: (-1, -1), habit: None, struck: None, in_sight: vec![] }
    }
}

/// Found a person as a hunter: its own hunting concepts and, if it knows where the deer grazes
/// (`deer_at`, the truth, put off by its own pacing), a node for the deer and its meadow.
pub fn found(p: &mut Person, w: &World, deer_at: Option<(i32, i32)>, pacing: i32) {
    let ids = p.ids;
    let who = p.m.who;
    let st = &mut p.m.store;
    let deer_kind = st.add_node("a deer", 400, 0, None, who);
    let drive = st.add_node("drive the quarry", -50, 0, None, who);
    let wait = st.add_node("wait in hiding", 50, 0, None, who);
    let signal = st.add_node("the signal", 0, 0, None, who);
    let broken = st.add_node("a broken plan", -400, 0, None, who);
    let betrayal = st.add_node("a betrayal", -600, 0, None, who);
    for (u, v, wt) in [(broken, ids.danger, 400), (broken, ids.anger, 200), (betrayal, ids.anger, 400), (betrayal, ids.danger, 300), (signal, drive, 500), (deer_kind, drive, 200), (deer_kind, wait, 200)] { st.add_link(u, v, lt::ASSOC, wt, 0); }
    let (dx, dy) = deer_at.unwrap_or((-1, -1));
    let err = |k: u64| if pacing == 0 { 0 } else { (crate::rng::hash(w.seed, p.i as u64, 0xdee + k, 0xbace) % (2 * pacing as u64 + 1)) as i32 - pacing };
    let (ex, ey) = ((dx + err(0)).clamp(0, GRID_W - 1), (dy + err(1)).clamp(0, GRID_H - 1));
    let deer = p.m.store.add_node("the deer", 400, tag::CREATURE, Some(Anchor::Entity { id: DEER_ID, x: ex as i16, y: ey as i16 }), who);
    p.m.store.add_link(deer, deer_kind, lt::KIND, 700, 0);
    p.m.entity_nodes.push((DEER_ID, deer)); p.m.entity_kinds.push(Kind::Deer.k());
    if deer_at.is_some() {
        let pi = p.m.add_place("the deer's meadow", ex, ey, None);
        let pn = p.m.place_nodes[pi];
        p.m.store.add_link(deer, pn, lt::AT, 700, 0); p.m.store.add_link(pn, deer, lt::HOLDS, 600, 0);
    }
    p.m.plan.broken = Some(broken);
    p.m.young_from = p.m.store.len() as u32;
    p.hunt = Some(Box::new(Hunter::new(Know { deer_kind, deer, drive, wait, signal, broken, betrayal })));
}

/// The uncoordinated condition: a hunter with a plan of its own (hunt the deer where I believe it
/// is), held as an intent like any other; no talk.
pub fn start_solo(p: &mut Person, tick: u32) {
    let Some((x, y)) = p.m.store.anchor(p.hunt.as_ref().unwrap().k.deer).map(|a| a.xy()) else { return };
    let i = p.i;
    if let Some(hp) = p.adopt(i, 0, Target::Deer, (x, y, 3), vec![(i, Role::Drive, x, y, 3)], tick, true) {
        p.m.form_intent(hp.node);
        p.predict(&hp, tick);
        let h = p.hunt.as_mut().unwrap();
        h.solo = true;
        h.plan = Some(hp);
    }
}

fn unit(dx: i32, dy: i32, len: i32) -> (i32, i32) { let d = dist(0, 0, dx, dy).max(1); (dx * len / d, dy * len / d) }
fn clampxy(x: i32, y: i32) -> (i32, i32) { (x.clamp(1, GRID_W - 2), y.clamp(1, GRID_H - 2)) }

impl Person {
    fn hunter(&self) -> &Hunter { self.hunt.as_ref().unwrap() }

    /// Where my own map puts a place described by references (as `ground` does for a told
    /// claim): the mean of each known landmark's reading, and the uncertainty.
    pub fn ground_refs(&self, refs: &[Ref]) -> Option<(i32, i32, i32)> {
        let mut ests = vec![];
        for r in refs { if let Some(n) = self.lex_place(r.landmark) { let (x, y) = self.m.store.anchor(n).unwrap().xy(); ests.push(from_ref(x, y, r)); } }
        if ests.is_empty() { return None; }
        let k = ests.len() as i32;
        let est = (ests.iter().map(|e| e.0).sum::<i32>() / k, ests.iter().map(|e| e.1).sum::<i32>() / k);
        let spread = ests.iter().map(|e| dist(e.0, e.1, est.0, est.1)).max().unwrap_or(0);
        Some((est.0.clamp(0, GRID_W - 1), est.1.clamp(0, GRID_H - 1), ests.iter().map(|e| e.2).sum::<i32>() / k + spread / 2))
    }
    /// Describe a point the way this mind holds its map: the two landmarks it knows nearest to it.
    pub fn describe_xy(&self, x: i32, y: i32) -> Vec<Ref> {
        let mut lm: Vec<(i32, u32, i32, i32)> = self.lex.iter().map(|&(wd, n)| { let (lx, ly) = self.m.store.anchor(n).unwrap().xy(); (d2(lx, ly, x, y), wd, lx, ly) }).collect();
        lm.sort();
        lm.iter().take(2).map(|&(_, wd, lx, ly)| { let (band, dir) = band_dir(x - lx, y - ly); Ref { landmark: wd, dir, band } }).collect()
    }
    /// The node this mind holds for a plan's target.
    fn target_node(&self, t: Target) -> u32 { match t { Target::Deer => self.hunter().k.deer, Target::Person(j) => self.pnode[j as usize] } }
    fn target_entity(t: Target) -> u32 { match t { Target::Deer => DEER_ID, Target::Person(j) => person_id(j as usize) } }
    /// Where this mind sees the target now, if in sight.
    fn target_seen(&self, t: Target) -> Option<(i32, i32, bool)> {
        let n = self.target_node(t);
        self.m.last.percepts.iter().find(|p| p.node == n).map(|p| (p.x, p.y, p.approaching))
    }
    /// Where I believe someone (by my node for them) is: last seen.
    fn believed(&self, n: u32) -> Option<(i32, i32)> { self.m.store.anchor(n).map(|a| a.xy()) }

    // ------------------------------------------------------------------------- plan nodes

    /// Build my own plan node for a plan whose places I have grounded, and the predictions it makes.
    fn adopt(&mut self, by: usize, rev: u8, target: Target, tgt: (i32, i32, i32), members: Vec<(usize, Role, i32, i32, i32)>, tick: u32, solo: bool) -> Option<Held> {
        let me = members.iter().find(|m| m.0 == self.i).copied()?;
        let ids = self.ids;
        let k = self.hunter().k;
        let tname = match target { Target::Deer => "the deer".to_string(), Target::Person(j) => name(j as usize).to_string() };
        let name = if solo { format!("plan: hunt {tname} myself") } else if by == self.i { format!("plan #{rev} (mine): {tname}") } else { format!("plan #{rev} from {}: {tname}", name(by as usize)) };
        let who = self.m.who;
        let node = self.m.store.add_node(&name, 200, 0, None, who);
        // A spot inside a place I already know (right by the rock) is that place: a new place there
        // would take the rock's sightings and surprise me every tick (found in the viewer), and a
        // copy of its expectations would make one of the two miss the rock (found next).
        let pi = match self.m.place_index_at(me.2, me.3) {
            Some(k) => k,
            None => self.m.add_place(&format!("my spot ({}) in plan #{rev}", crate::people::talk::ROLE_WORDS[me.1.i()]), me.2, me.3, None),
        };
        let pn = self.m.place_nodes[pi];
        let tn = self.target_node(target);
        let role_c = match me.1 { Role::Drive => k.drive, Role::Wait => k.wait };
        let st = &mut self.m.store;
        for (u, v, wt) in [(node, pn, PLAN_PLACE), (pn, node, 200), (node, tn, PLAN_TARGET), (tn, node, 300), (node, role_c, 600), (node, ids.fatigue, EFFORT[me.1.i()]), (k.signal, node, 300)] { st.add_link(u, v, lt::ASSOC, wt, 0); }
        for m in &members { if m.0 != self.i { let mn = self.pnode[m.0]; self.m.store.add_link(node, mn, lt::ASSOC, PLAN_MEMBER, 0); self.m.store.add_link(mn, node, lt::ASSOC, PLAN_MEMBER / 2, 0); } }
        if by != self.i {
            let (an, lv) = (self.pnode[by], self.trust(by) - 500);
            strengthen(&mut self.m, node, an, lt::SOURCE, 700, Some(lv));
            strengthen(&mut self.m, an, node, lt::SOURCE, 350, None);
        }
        Some(Held { node, rev, by, target, role: me.1, place: pi, at: (me.2, me.3, me.4), tgt, members, formed: tick, signal: None, agreed: vec![], solo })
    }

    /// Whether I'd join a plan, read off my own mind: how the plan node feels to me now, less what
    /// my body needs (the strongest of tiredness, stress and thirst, a third of it).
    fn plan_score(&self, node: u32) -> i32 {
        let need = self.m.mods.fatigue.max(self.m.mods.stress).max(self.m.mods.hunger);
        self.m.felt(&self.m.store, node) - need / 3
    }

    /// Set my mind's predictions from the plan I hold (replacing any earlier ones).
    pub fn predict(&mut self, hp: &Held, tick: u32) {
        let k = self.hunter().k;
        let mut ex: Vec<Expect> = vec![];
        let speed = 1;
        let (tx, ty, ttol) = hp.tgt;
        for &(j, role, x, y, tol) in &hp.members {
            if j == self.i { continue; }
            let n = self.pnode[j];
            let (bx, by) = self.believed(n).unwrap_or((x, y));
            let eta = tick.max(hp.formed) + (dist(bx, by, x, y) / speed) as u32 + 4;
            match (role, hp.signal) {
                (Role::Wait, _) => ex.push(Expect { intent: hp.node, node: n, x, y, tol: tol + 1, pace: 0, hidden: true, from: eta, until: u32::MAX, w: 900 }),
                (Role::Drive, None) => ex.push(Expect { intent: hp.node, node: n, x, y, tol: tol + 2, pace: 1, hidden: false, from: eta, until: u32::MAX, w: 900 }),
                (Role::Drive, Some(t)) => { let r = dist(x, y, tx, ty) + tol; ex.push(Expect { intent: hp.node, node: n, x: (x + tx) / 2, y: (y + ty) / 2, tol: r, pace: 1, hidden: false, from: t, until: u32::MAX, w: 900 }); }
            }
        }
        match (hp.target, hp.signal) {
            (Target::Deer, None) => ex.push(Expect { intent: hp.node, node: k.deer, x: tx, y: ty, tol: ttol + 4, pace: 1, hidden: false, from: hp.formed, until: u32::MAX, w: 900 }),
            (Target::Deer, Some(t)) => ex.push(Expect { intent: hp.node, node: k.deer, x: tx, y: ty, tol: ttol + 4 + dist(tx, ty, hp.at.0, hp.at.1), pace: 2, hidden: false, from: t, until: u32::MAX, w: 900 }),
            (Target::Person(j), _) => {
                // The target passes the predicted spot on its way, when its habit takes it there (as
                // this mind knows the habit), within a window.
                let n = self.pnode[j as usize];
                let (bx, by) = self.believed(n).unwrap_or((tx, ty));
                let eta = self.hunter().habit.unwrap_or(hp.formed).max(hp.formed) + dist(bx, by, tx, ty) as u32;
                let open = hp.signal.unwrap_or(eta.saturating_sub(PASS_WINDOW / 2));
                ex.push(Expect { intent: hp.node, node: n, x: tx, y: ty, tol: ttol, pace: 1, hidden: false, from: open.min(eta), until: eta + PASS_WINDOW, w: 900 });
            }
        }
        self.m.plan.expects = ex;
    }

    // ------------------------------------------------------------------------- hearing

    /// Hear the stage-4 utterances of this tick (before thinking). `heard`: (utterance, loudness,
    /// addressed to me).
    pub fn hunt_hear(&mut self, w: &World, s: &Sense, heard: &[(Utterance, i32, bool)], cues: &mut Vec<(u32, i32)>) {
        let tick = w.tick;
        // I struck at the deer last tick and it is gone from sight: I caught it.
        if self.hunter().struck == Some(tick.wrapping_sub(1)) && !s.seen.iter().any(|e| e.id == DEER_ID) {
            let h = self.hunt.as_mut().unwrap(); h.out.push((Say::Caught, None, SHOUT)); h.log.events.push((tick, "caught the deer".into()));
        }
        for (u, _loud, to_me) in heard {
            let a = u.speaker;
            match &u.say {
                Say::Propose(p) => {
                    let mine = p.roles.iter().any(|r| r.0 as usize == self.i);
                    if !mine { self.overheard_plan(a, p, u.vol < 0, tick); continue; }
                    if self.hunter().heard.contains(&(a, p.rev)) { continue; }
                    self.hunt.as_mut().unwrap().heard.push((a, p.rev));
                    self.hear_plan(a, p, tick, u.vol < 0, cues);
                }
                Say::Agree(rev) => {
                    let h = self.hunt.as_mut().unwrap();
                    if let Some(hp) = h.plan.as_mut() { if hp.by == a || hp.members.iter().any(|m| m.0 == a) { if !hp.agreed.contains(&a) { hp.agreed.push(a); } } }
                    if h.lead.is_some() && *rev == h.book.rev && !h.book.agreed.contains(&a) { h.book.agreed.push(a); }
                }
                Say::Decline(rev) => {
                    let h = self.hunt.as_mut().unwrap();
                    if h.lead.is_some() && *rev == h.book.rev && !h.book.declined.contains(&a) { h.book.declined.push(a); h.log.events.push((tick, format!("{} declined", name(a as usize)))); }
                }
                Say::Signal => {
                    let hp = self.hunter().plan.clone();
                    if let Some(mut hp) = hp { if hp.by == a && hp.signal.is_none() {
                        hp.signal = Some(tick);
                        cues.push((self.hunter().k.signal, 600));
                        self.predict(&hp, tick);
                        let h = self.hunt.as_mut().unwrap();
                        h.log.events.push((tick, "heard the signal".into()));
                        h.plan = Some(hp);
                    } }
                }
                Say::Caught => {
                    // The hunt worked: trust and warmth towards whoever proposed the plan I held rise.
                    if let Some(by) = self.hunter().plan.as_ref().filter(|hp| !hp.solo && hp.target == Target::Deer).map(|hp| hp.by) {
                        if by != self.i { let t = self.trust(by); self.set_trust(by, t + (1000 - t) / 10); let n = self.pnode[by]; let v = self.m.store.val(n); self.m.store.set_val(n, (v + (1000 - v) / 16) as i16); }
                    }
                }
                Say::Warn(p) => {
                    if *to_me { self.hear_warning(a, p, tick, cues); } else { self.overheard_warning(a, p, tick); }
                }
                _ => {}
            }
        }
    }

    /// A plan addressed to me (I have a role in it): ground it, decide, and say so.
    fn hear_plan(&mut self, a: usize, p: &Plan, tick: u32, whispered: bool, cues: &mut Vec<(u32, i32)>) {
        let Some(tgt) = self.ground_refs(&p.at) else { self.hunt.as_mut().unwrap().log.decisions.push((tick, p.rev, false, i32::MIN, 0)); return };
        let mut members = vec![];
        for (j, role, refs) in &p.roles {
            match self.ground_refs(refs) { Some((x, y, t)) => members.push((*j as usize, *role, x, y, t)), None => if *j as usize == self.i { self.hunt.as_mut().unwrap().log.decisions.push((tick, p.rev, false, i32::MIN, 0)); return; } }
        }
        if !members.iter().any(|m| m.0 == a) { members.push((a, Role::Wait, tgt.0, tgt.1, tgt.2)); }
        let Some(hp) = self.adopt(a, p.rev, p.target, tgt, members, tick, false) else { return };
        cues.push((hp.node, 400)); cues.push((self.pnode[a], 200));
        // The plan against a person: do I feel more for the target than for the one proposing?
        if let Target::Person(r) = p.target {
            let r = r as usize;
            // Already on the target's side (I warned it, or I'm in its counter-plan): a new plan of
            // the group's changes nothing for me.
            if self.hunter().warn.as_ref().is_some_and(|w| w.0 == r) { return; }
            if r != self.i {
                let (fr, fp) = (self.m.felt(&self.m.store, self.pnode[r]), self.m.felt(&self.m.store, self.pnode[a]));
                if fr > 0 && fr > fp + LOYAL {
                    // The betrayal: a counter-intent of my own, to tell the target. Outwardly I
                    // agree if I'm not honest, and say nothing if I am.
                    let who = self.m.who;
                    let wn = self.m.store.add_node(&format!("tell {} of the plan", name(r as usize)), 200, 0, None, who);
                    for (u, v, wt) in [(wn, self.pnode[r], 700), (wn, hp.node, 500), (self.pnode[r], wn, 300)] { self.m.store.add_link(u, v, lt::ASSOC, wt, 0); }
                    self.m.form_intent(wn);
                    let honest = self.traits.honesty >= 500;
                    let to_tell = self.restate(&hp, p.by);
                    let h = self.hunt.as_mut().unwrap();
                    h.warn = Some((r, wn, to_tell));
                    h.log.decisions.push((tick, p.rev, false, fr - fp, if honest { 0 } else { 1 }));
                    h.log.events.push((tick, format!("feels more for {} ({fr}) than for {} ({fp}): will warn", name(r as usize), name(a as usize))));
                    if !honest { h.out.push((Say::Agree(p.rev), Some(a), if whispered { WHISPER } else { SPEAK })); }
                    return;
                }
            }
        }
        let score = self.plan_score(hp.node);
        let accept = score >= ACCEPT_AT;
        let warm = self.warmth(a) >= 0;
        let said = if accept { if warm { 1 } else { 0 } } else if warm { 2 } else { 0 };
        let vol = if whispered { WHISPER } else { SPEAK };
        let h = self.hunt.as_mut().unwrap();
        h.log.decisions.push((tick, p.rev, accept, score, said));
        match said { 1 => h.out.push((Say::Agree(p.rev), Some(a), vol)), 2 => h.out.push((Say::Decline(p.rev), Some(a), vol)), _ => {} }
        if accept {
            h.log.events.push((tick, format!("joins plan #{} from {} as {} (score {score})", p.rev, name(a as usize), crate::people::talk::ROLE_WORDS[hp.role.i()])));
            let mut hp = hp;
            hp.agreed.push(a);
            if let Some(old) = h.plan.as_ref().filter(|o| o.by == a && o.signal.is_some()) { hp.signal = old.signal; }
            h.plan = Some(hp.clone());
            self.m.form_intent(hp.node);
            self.predict(&hp, tick);
        }
    }

    /// The plan restated from my own grounding (to pass on): places described from my own map.
    fn restate(&self, hp: &Held, by: u8) -> Plan {
        let roles = hp.members.iter().map(|&(j, r, x, y, _)| (j as u8, r, self.describe_xy(x, y))).collect();
        Plan { rev: hp.rev, by: self.i as u8, target: hp.target, at: self.describe_xy(hp.tgt.0, hp.tgt.1), roles, via: Some(by) }
    }

    /// A plan I wasn't given a role in, overheard (the target, or someone left out): if it is
    /// against me, it is a warning I overheard.
    fn overheard_plan(&mut self, a: usize, p: &Plan, whispered: bool, tick: u32) {
        if p.target == Target::Person(self.i as u8) && self.hunter().against_me.is_none() { self.hear_warning_from(a, p, tick, true); }
        let _ = whispered;
    }

    // ------------------------------------------------------------------------- the betrayal

    /// Told, in private, of a plan against me: ground it as predictions of my own, and propose a
    /// counter-plan to the one who told me.
    fn hear_warning(&mut self, a: usize, p: &Plan, tick: u32, cues: &mut Vec<(u32, i32)>) {
        cues.push((self.pnode[a], 300));
        self.hear_warning_from(a, p, tick, false);
    }
    fn hear_warning_from(&mut self, a: usize, p: &Plan, tick: u32, overheard: bool) {
        let Some(tgt) = self.ground_refs(&p.at) else { return };
        let mut members = vec![];
        for (j, role, refs) in &p.roles { if let Some((x, y, t)) = self.ground_refs(refs) { members.push((*j as usize, *role, x, y, t)); } }
        if members.is_empty() { return; }
        let ids = self.ids;
        let who = self.m.who;
        let by = p.via.unwrap_or(p.by) as usize;
        let node = self.m.store.add_node(&format!("their plan against me ({})", name(by as usize)), -500, 0, None, who);
        for &(j, _, _, _, _) in &members { if j != self.i { let n = self.pnode[j]; self.m.store.add_link(node, n, lt::ASSOC, PLAN_MEMBER, 0); } }
        self.m.store.add_link(node, ids.danger, lt::ASSOC, 500, 0);
        if !overheard { let lv = self.trust(a) - 500; strengthen(&mut self.m, node, self.pnode[a], lt::SOURCE, 700, Some(lv)); }
        let held = Held { node, rev: p.rev, by, target: p.target, role: Role::Wait, place: 0, at: (tgt.0, tgt.1, tgt.2), tgt, members: members.clone(), formed: tick, signal: None, agreed: vec![], solo: false };
        let h = self.hunt.as_mut().unwrap();
        h.log.events.push((tick, format!("{} of the plan against me from {}", if overheard { "overhears" } else { "is warned" }, name(a as usize))));
        h.log.warned_at = Some(tick);
        h.against_me = Some(held);
        // The counter-plan (if it was a friend who told me, and I believe them): we come round
        // behind the ones lying in wait, and fall on them first. Proposed in a whisper to the teller.
        if overheard || self.trust(a) < 400 || self.warmth(a) <= 0 { return; }
        let waiters: Vec<(usize, i32, i32)> = members.iter().filter(|m| m.1 == Role::Wait && m.0 != a).map(|m| (m.0, m.2, m.3)).collect();
        let Some(&(lead, wx, wy)) = waiters.first() else { return };
        let me = self.believed_me();
        // Behind them: on the far side of their spot from where they expect me to come.
        let (ux, uy) = unit(wx - tgt.0, wy - tgt.1, 5);
        let (bx, by2) = clampxy(wx + ux, wy + uy);
        let _ = me;
        let roles = vec![(self.i as u8, Role::Drive, self.describe_xy(bx, by2)), (a as u8, Role::Drive, self.describe_xy(bx + uy.signum(), by2 - ux.signum()))];
        let plan = Plan { rev: 1, by: self.i as u8, target: Target::Person(lead as u8), at: self.describe_xy(wx, wy), roles, via: None };
        let members2 = vec![(self.i, Role::Drive, bx, by2, 2), (a, Role::Drive, bx + uy.signum(), by2 - ux.signum(), 2)];
        if let Some(hp) = self.adopt(self.i, 1, Target::Person(lead as u8), (wx, wy, 3), members2, tick, false) {
            self.m.form_intent(hp.node);
            let h = self.hunt.as_mut().unwrap();
            h.lead = Some(Target::Person(lead as u8));
            h.book = Lead { rev: 1, said_at: Some(tick), ..Lead::default() };
            h.plan = Some(hp.clone());
            h.out.push((Say::Propose(plan), Some(a), WHISPER));
            h.log.events.push((tick, format!("counter-plan: come round behind {} with {}", name(lead as usize), name(a as usize))));
            self.predict(&hp, tick);
        }
    }
    fn believed_me(&self) -> (i32, i32) { self.hunter().last_xy }

    /// Overheard someone warning the target (a whisper caught): direct evidence of who told.
    fn overheard_warning(&mut self, a: usize, p: &Plan, tick: u32) {
        if p.target == Target::Person(self.i as u8) { self.hear_warning_from(a, p, tick, true); return; }
        let k = self.hunter().k;
        strengthen(&mut self.m, self.pnode[a], k.betrayal, lt::ASSOC, 900, None);
        self.hunt.as_mut().unwrap().log.events.push((tick, format!("overheard {} whisper the plan away", name(a as usize))));
    }

    // ------------------------------------------------------------------------- the leader

    /// The leader composes a plan from its own map and its own view of who is with it.
    fn compose(&mut self, s: &Sense, tick: u32, exclude: &[usize]) -> Option<(Plan, Vec<(usize, Role, i32, i32, i32)>, (i32, i32, i32))> {
        let target = self.hunter().lead?;
        // Who is with me: the people I see (my percepts) within talking distance, not the target.
        let mut with: Vec<(i32, usize)> = (0..self.pnode.len()).filter(|&j| j != self.i && Target::Person(j as u8) != target && !exclude.contains(&j))
            .filter(|&j| self.m.last.percepts.iter().any(|p| p.node == self.pnode[j] && dist(p.x, p.y, s.body.x, s.body.y) <= TALK + 4))
            .map(|j| (-self.warmth(j), j)).collect();
        with.sort();
        if with.is_empty() { return None; }
        // Where the quarry is (deer: where I last saw it or its meadow; a person: where it will
        // pass on its way to water, which I know as its habit).
        let (qx, qy, route_from) = match target {
            Target::Deer => { let (x, y) = self.believed(self.hunter().k.deer)?; (x, y, None) }
            Target::Person(r) => {
                let (rx, ry) = self.believed(self.pnode[r as usize])?;
                let water = self.water_things().into_iter().map(|(_, pi)| self.place_xy(pi)).min_by_key(|&(x, y)| d2(x, y, rx, ry))?;
                (water.0, water.1, Some((rx, ry)))
            }
        };
        // Cover: a landmark place of my own that I expect to hide behind, near the quarry (deer) or
        // near its route (a person).
        let covers: Vec<(i32, i32, i32)> = self.lex.iter().filter_map(|&(_, n)| {
            let pi = self.m.place_nodes.iter().position(|&p| p == n)?;
            let hides = [Kind::Rock, Kind::Windmill, Kind::Oak].iter().any(|k| self.m.expect[pi][key(k.k(), false)] as i32 >= IGNORE_BELOW);
            if !hides { return None; }
            let (x, y) = self.place_xy(pi);
            let d = match route_from { None => dist(x, y, qx, qy), Some((rx, ry)) => seg_dist(x, y, rx, ry, qx, qy) };
            Some((d, x, y))
        }).collect();
        let (cx, cy) = match route_from {
            None => covers.iter().filter(|c| c.0 >= COVER_MIN && c.0 <= COVER_MAX).min_by_key(|c| (c.0, c.1, c.2)).map(|c| (c.1, c.2))?,
            Some(_) => covers.iter().filter(|c| c.0 <= 8).min_by_key(|c| (c.0, c.1, c.2)).map(|c| (c.1, c.2))?,
        };
        // Spots: waiters right by the cover on its far side; drivers beyond the quarry.
        let (ux, uy) = match route_from { None => unit(cx - qx, cy - qy, 1), Some((rx, ry)) => unit(cx - (rx + qx) / 2, cy - (ry + qy) / 2, 1) };
        let w1 = clampxy(cx + ux, cy + uy);
        let w2 = clampxy(cx + ux + uy, cy + uy - ux);
        let (tx, ty) = match route_from {
            None => (qx, qy),
            // The person is expected to pass the point of its route nearest the cover.
            Some((rx, ry)) => nearest_on_seg(cx, cy, rx, ry, qx, qy),
        };
        let (dx, dy) = match route_from { None => { let (vx, vy) = unit(qx - cx, qy - cy, DRIVE_D); clampxy(qx + vx, qy + vy) } Some((rx, ry)) => clampxy(rx, ry) };
        let mut members = vec![(self.i, Role::Wait, w1.0, w1.1, 1)];
        for (k, &(_, j)) in with.iter().enumerate() {
            if k == 0 && with.len() >= 2 { members.push((j, Role::Wait, w2.0, w2.1, 1)); } else { members.push((j, Role::Drive, dx + k as i32 - 1, dy, 2)); }
        }
        let rev = self.hunter().book.rev + 1;
        let roles = members.iter().map(|&(j, r, x, y, _)| (j as u8, r, self.describe_xy(x, y))).collect();
        let plan = Plan { rev, by: self.i as u8, target, at: self.describe_xy(tx, ty), roles, via: None };
        let _ = tick;
        Some((plan, members, (tx, ty, 3)))
    }

    /// The leader's turn to speak (propose, re-plan, signal); everyone's queued answers first.
    pub fn hunt_speak(&mut self, s: &Sense, tick: u32) -> Option<(Say, Option<usize>, i32)> {
        self.hunt.as_ref()?;
        if !self.hunter().out.is_empty() { return Some(self.hunt.as_mut().unwrap().out.remove(0)); }
        // The betrayal: whisper the plan to its target when it is close and nobody of the group is
        // within `caution` cells in my sight.
        if let Some((r, wn, plan)) = self.hunter().warn.clone() {
            if self.hunter().log.warned_at.is_none() && self.m.held.intent == Some(wn) {
                let near_r = self.m.last.percepts.iter().any(|p| p.node == self.pnode[r] && dist(p.x, p.y, s.body.x, s.body.y) <= 1);
                let caution = self.hunter().caution;
                let watched = self.m.last.percepts.iter().any(|p| (0..self.pnode.len()).any(|j| j != r && j != self.i && self.pnode[j] == p.node) && dist(p.x, p.y, s.body.x, s.body.y) <= caution);
                if near_r && !watched {
                    let h = self.hunt.as_mut().unwrap();
                    h.log.warned_at = Some(tick);
                    h.log.events.push((tick, format!("whispers the plan to {}", name(r as usize))));
                    return Some((Say::Warn(plan), Some(r), WHISPER));
                }
            }
        }
        if self.hunter().lead.is_none() || self.hunter().solo { return None; }
        let book = self.hunter().book.clone();
        if book.gave_up { return None; }
        // Propose once some company is in sight.
        if book.said_at.is_none() {
            if let Some((plan, members, tgt)) = self.compose(s, tick, &[]) { return self.propose(plan, members, tgt, tick, false); }
            return None;
        }
        // Re-plan out loud: someone declined, or my own mind registered a breach.
        let new_breaches: Vec<(u32, u32, i32, u8)> = self.m.plan.breaches.iter().skip(book.breaches_seen).copied().collect();
        self.hunt.as_mut().unwrap().book.breaches_seen = self.m.plan.breaches.len();
        let hp = self.hunter().plan.clone()?;
        if self.m.held.intent != Some(hp.node) { return None; }
        let k = self.hunter().k;
        if hp.signal.is_none() && hp.target == Target::Deer && new_breaches.iter().any(|b| b.1 == k.deer) {
            // The deer moved before the signal: drive now.
            self.hunt.as_mut().unwrap().log.replan_at.push(tick);
            return self.signal(hp, tick, "the deer moved early: drive now");
        }
        let missing: Vec<usize> = new_breaches.iter().filter(|b| b.3 == BREACH_MISSING).filter_map(|b| (0..self.pnode.len()).find(|&j| self.pnode[j] == b.1)).filter(|j| hp.members.iter().any(|m| m.0 == *j)).collect();
        let declined: Vec<usize> = book.declined.iter().copied().filter(|j| hp.members.iter().any(|m| m.0 == *j)).collect();
        if (!declined.is_empty() || !missing.is_empty()) && book.replans < 2 {
            if !missing.is_empty() { self.hunt.as_mut().unwrap().log.replan_at.push(tick); }
            let mut ex = declined.clone(); ex.extend(missing.iter().copied());
            if let Some((plan, members, tgt)) = self.compose(s, tick, &ex) { return self.propose(plan, members, tgt, tick, true); }
            let h = self.hunt.as_mut().unwrap(); h.book.gave_up = true; h.log.events.push((tick, "no one left to plan with".into()));
            return None;
        }
        // The signal: when I am at my spot and I expect everyone in place.
        if hp.signal.is_none() {
            let here = dist(s.body.x, s.body.y, hp.at.0, hp.at.1) <= 1;
            let ready = match hp.target {
                Target::Deer => hp.members.iter().filter(|m| m.0 != self.i && (book.agreed.contains(&m.0) || !book.declined.contains(&m.0))).all(|m| {
                    let (bx, by) = self.believed(self.pnode[m.0]).unwrap_or((m.2, m.3));
                    let _ = (bx, by);
                    self.m.plan.expects.iter().find(|x| x.node == self.pnode[m.0]).is_none_or(|x| tick >= x.from)
                }),
                // The counter-plan (I am the target of theirs): when my partner has agreed and we
                // are both at our spots; the group's plan: when the target comes into sight.
                Target::Person(_) if self.hunter().against_me.is_some() => hp.members.iter().filter(|m| m.0 != self.i).all(|m|
                    book.agreed.contains(&m.0) && self.m.last.percepts.iter().any(|p| p.node == self.pnode[m.0] && dist(p.x, p.y, m.2, m.3) <= 2)),
                Target::Person(r) => self.m.last.percepts.iter().any(|p| p.node == self.pnode[r as usize]),
            };
            if here && ready { return self.signal(hp, tick, "everyone should be in place"); }
        }
        None
    }
    fn propose(&mut self, plan: Plan, members: Vec<(usize, Role, i32, i32, i32)>, tgt: (i32, i32, i32), tick: u32, replan: bool) -> Option<(Say, Option<usize>, i32)> {
        let target = plan.target;
        let hp = self.adopt(self.i, plan.rev, target, tgt, members, tick, false)?;
        self.m.form_intent(hp.node);
        self.predict(&hp, tick);
        let h = self.hunt.as_mut().unwrap();
        h.book = Lead { rev: plan.rev, said_at: Some(tick), declined: vec![], agreed: vec![], signalled: None, replans: h.book.replans + replan as u32, breaches_seen: h.book.breaches_seen, gave_up: false };
        h.log.events.push((tick, format!("{} plan #{}", if replan { "re-plans out loud:" } else { "proposes" }, plan.rev)));
        h.plan = Some(hp);
        // A plot against a person is spoken low; a hunt plan aloud; a re-plan is called out.
        let vol = match (target, replan) { (Target::Person(_), _) => LOW, (_, true) => SHOUT, _ => SPEAK };
        Some((Say::Propose(plan), None, vol))
    }
    fn signal(&mut self, mut hp: Held, tick: u32, why: &str) -> Option<(Say, Option<usize>, i32)> {
        hp.signal = Some(tick);
        self.predict(&hp, tick);
        let h = self.hunt.as_mut().unwrap();
        h.book.signalled = Some(tick);
        h.log.events.push((tick, format!("whistles: {why}")));
        h.plan = Some(hp);
        Some((Say::Signal, None, SHOUT))
    }

    // ------------------------------------------------------------------------- acting

    /// What the body does, given what the mind chose: a held plan's role takes the body unless a
    /// danger response has it.
    pub fn hunt_act(&mut self, s: &Sense, a: Action) -> Action {
        if self.hunt.is_none() { return a; }
        self.hunt.as_mut().unwrap().last_xy = (s.body.x, s.body.y);
        if self.m.held.response.is_some() || s.body.asleep { return a; }
        let intent = self.m.held.intent;
        let (bx, by) = (s.body.x, s.body.y);
        // The counter-intent: go to the one I mean to warn.
        if let Some((r, wn, _)) = self.hunter().warn.clone() {
            if intent == Some(wn) && self.hunter().log.warned_at.is_none() {
                // Where it is if I see it; else where I last saw it (and wait there for it).
                let seen_r = self.m.last.percepts.iter().find(|p| p.node == self.pnode[r]).map(|p| (p.x, p.y));
                let (x, y) = seen_r.or(self.believed(self.pnode[r])).unwrap_or((bx, by));
                return if dist(x, y, bx, by) <= 1 { Action::Stay } else { Action::WalkTo { x, y } };
            }
        }
        let Some(hp) = self.hunter().plan.clone() else { return self.improvise(s, a) };
        if intent != Some(hp.node) { return self.improvise(s, a); }
        let seen = self.target_seen(hp.target);
        let ent = Self::target_entity(hp.target);
        let reach = |x: i32, y: i32| (x - bx).abs().max((y - by).abs());
        if let Some((x, y, _)) = seen { if reach(x, y) <= 2 { if hp.target == Target::Deer { self.hunt.as_mut().unwrap().struck = Some(s.tick); } return Action::Snap { entity: ent }; } }
        let (px, py) = (hp.at.0, hp.at.1);
        let tgt_now = match hp.target { Target::Deer => seen.map(|(x, y, _)| (x, y)).or(self.believed(self.target_node(hp.target))).unwrap_or((hp.tgt.0, hp.tgt.1)),
            Target::Person(_) => seen.map(|(x, y, _)| (x, y)).unwrap_or((hp.tgt.0, hp.tgt.1)) };
        if hp.solo { return Action::WalkTo { x: tgt_now.0, y: tgt_now.1 }; }
        match hp.role {
            Role::Wait => {
                if let Some((x, y, _)) = seen { if reach(x, y) <= 4 { return Action::Charge { entity: ent, x, y }; } }
                if dist(bx, by, px, py) > 0 { Action::WalkTo { x: px, y: py } } else { Action::Stay }
            }
            Role::Drive => {
                if hp.signal.is_some() { return Action::WalkTo { x: tgt_now.0, y: tgt_now.1 }; }
                if dist(bx, by, px, py) > 0 { Action::WalkTo { x: px, y: py } } else { Action::Stay }
            }
        }
    }
    /// No plan held: if the deer is on my mind and in sight, go after it alone (improvising);
    /// otherwise whatever my mind chose.
    fn improvise(&mut self, s: &Sense, a: Action) -> Action {
        let k = self.hunter().k;
        if !self.m.last.ws.contains(&k.deer) && !self.m.last.ws.contains(&k.deer_kind) { return a; }
        let Some((x, y, _)) = self.target_seen(Target::Deer) else { return a };
        if self.hunter().plan.is_some() { let h = self.hunt.as_mut().unwrap(); h.log.improvised += 1; h.log.improvised_at.push(s.tick); }
        if (x - s.body.x).abs().max((y - s.body.y).abs()) <= 2 { self.hunt.as_mut().unwrap().struck = Some(s.tick); return Action::Snap { entity: DEER_ID }; }
        Action::WalkTo { x, y }
    }

    /// After the tick: copy my mind's new breaches, judge what followed earlier ones, weigh the
    /// evidence of a betrayal.
    pub fn hunt_after(&mut self, s: &Sense, tick: u32, said: Option<&Say>) {
        if self.hunt.is_none() { return; }
        let new: Vec<(u32, u32, i32, u8)> = self.m.plan.breaches.iter().skip(self.hunter().seen_breaches).copied().collect();
        let nb = self.m.plan.breaches.len();
        let intent = self.m.held.intent;
        let hp_node = self.hunter().plan.as_ref().map(|h| h.node);
        let h = self.hunt.as_mut().unwrap();
        h.seen_breaches = nb;
        for b in new { h.log.reactions.push((b.0, b.1, b.2, b.3, u8::MAX)); }
        // What followed each breach, REACT_WINDOW ticks on (or as soon as a re-plan it caused).
        // (Review: any whistle counted as a re-plan, and improvising was a lifetime counter.)
        let _ = said;
        let replanned = h.log.replan_at.last() == Some(&tick);
        let imp = h.log.improvised_at.clone();
        for r in h.log.reactions.iter_mut().filter(|r| r.4 == u8::MAX) {
            if replanned { r.4 = 1; continue; }
            if tick >= r.0 + REACT_WINDOW {
                let improvised = imp.iter().any(|&t| t > r.0 && t <= r.0 + REACT_WINDOW);
                r.4 = if improvised { 3 } else if intent.is_some() && intent == hp_node { 0 } else { 2 };
            }
        }
        let _ = s;
        self.evidence(tick);
    }

    /// Who told? Each mind of the group weighs its own evidence (rules, like the lie evidence of
    /// stage 2): any member knew the plan (a little); a member missing from its post (more); a
    /// member seen with the target when the target came at us (a lot); a whisper overheard (most,
    /// in `overheard_warning`). The links are the mind's own (member → "a betrayal").
    fn evidence(&mut self, tick: u32) {
        let Some(hp) = self.hunter().plan.clone() else { return };
        let Target::Person(r) = hp.target else { return };
        let r = r as usize;
        if hp.by == r || hp.members.iter().any(|m| m.0 == r) { return; }
        let k = self.hunter().k;
        let rn = self.pnode[r];
        let reactions = self.hunter().log.reactions.clone();
        // A sign the target knew: it broke my prediction of it (missing on its way, or elsewhere).
        let knew = reactions.iter().any(|x| x.1 == rn);
        let members: Vec<usize> = hp.members.iter().map(|m| m.0).filter(|&j| j != self.i).collect();
        let r_at = self.m.last.percepts.iter().find(|p| p.node == rn).map(|p| (p.x, p.y));
        let before: Vec<i32> = members.iter().map(|&j| weight(&self.m, self.pnode[j], k.betrayal, lt::ASSOC)).collect();
        for &j in &members {
            let jn = self.pnode[j];
            let key = |what: u8| (tick, jn, what);
            let _ = key;
            if knew && weight(&self.m, jn, k.betrayal, lt::ASSOC) == 0 { strengthen(&mut self.m, jn, k.betrayal, lt::ASSOC, 200, None); }
            if knew && reactions.iter().any(|x| x.1 == jn) && !self.hunter().log.events.iter().any(|e| e.1 == format!("missing: {}", name(j as usize))) {
                strengthen(&mut self.m, jn, k.betrayal, lt::ASSOC, 300, None);
                self.hunt.as_mut().unwrap().log.events.push((tick, format!("missing: {}", name(j as usize))));
            }
            // Seen with the target before the ambush was sprung (before the signal), away from the
            // post the plan gave it.
            let post = hp.members.iter().find(|m| m.0 == j).map(|m| (m.2, m.3, m.4));
            // (A first version counted ambushers striking the target.) It counts when the member
            // comes into my sight with the target (within 4 cells of it), away from its post.
            let arrived = !self.hunter().in_sight.contains(&jn);
            if let (Some((rx, ry)), true) = (r_at, arrived) {
                if self.m.last.percepts.iter().any(|p| p.node == jn && dist(p.x, p.y, rx, ry) <= 4 && post.is_none_or(|(x, y, t)| dist(p.x, p.y, x, y) > t + 2)) && !self.hunter().log.events.iter().any(|e| e.1 == format!("seen with {}: {}", name(r as usize), name(j as usize))) {
                    strengthen(&mut self.m, jn, k.betrayal, lt::ASSOC, 600, None);
                    self.hunt.as_mut().unwrap().log.events.push((tick, format!("seen with {}: {}", name(r as usize), name(j as usize))));
                }
            }
        }
        self.hunt.as_mut().unwrap().in_sight = self.m.last.percepts.iter().map(|p| p.node).collect();
        let after: Vec<i32> = members.iter().map(|&j| weight(&self.m, self.pnode[j], k.betrayal, lt::ASSOC)).collect();
        if after == before { return; }
        // The verdict: one member clearly ahead.
        let mut ranked: Vec<(i32, usize)> = members.iter().zip(&after).map(|(&j, &w)| (w, j)).collect();
        ranked.sort_by_key(|x| (std::cmp::Reverse(x.0), x.1));
        let top = ranked[0];
        let second = ranked.get(1).map(|x| x.0).unwrap_or(0);
        if top.0 >= 500 && top.0 >= second + 200 {
            if self.hunter().verdict != Some(top.1) {
                let j = top.1;
                let t = self.trust(j); self.set_trust(j, t * 3 / 10);
                let n = self.pnode[j]; let v = self.m.store.val(n) as i32; self.m.store.set_val(n, (v - 300).max(-1000) as i16);
                let h = self.hunt.as_mut().unwrap();
                h.verdict = Some(j); h.blamed = true;
                h.log.events.push((tick, format!("concludes {} told", name(j as usize))));
            }
        }
    }
}

/// Distance from a point to a segment (cells).
pub fn seg_dist(px: i32, py: i32, ax: i32, ay: i32, bx: i32, by: i32) -> i32 { let (x, y) = nearest_on_seg(px, py, ax, ay, bx, by); dist(px, py, x, y) }
pub fn nearest_on_seg(px: i32, py: i32, ax: i32, ay: i32, bx: i32, by: i32) -> (i32, i32) {
    let (vx, vy) = ((bx - ax) as i64, (by - ay) as i64);
    let l2 = vx * vx + vy * vy;
    if l2 == 0 { return (ax, ay); }
    let t = (((px - ax) as i64 * vx + (py - ay) as i64 * vy) * 1000 / l2).clamp(0, 1000);
    (ax + (vx * t / 1000) as i32, ay + (vy * t / 1000) as i32)
}
