//! A person: a shared-library mind (`Mind<Runtime>`) plus the senses and habits a talking
//! species needs around it. Everything here reads only what this person senses (the world as it
//! was at the start of the tick, and last tick's sounds) and writes only this person.
//!
//! - **Hearing and reading → grounding.** A claim is grounded in the listener's *own* landmarks
//!   (`ground`): a tentative place where its own map puts the description, a tentative water node
//!   held there, links to "water", and **source links** to the teller (and to whoever the teller
//!   had it from), weighted by trust. Encoding closes part of each link's gap to 1000 per hearing
//!   (a delta rule toward a ceiling), so a faded fact relearns faster than a fresh one (savings).
//! - **Checking.** A told place whose water is seen within its uncertainty is **confirmed**: it is
//!   re-anchored onto the real thing, its links rise to what seeing gives, trust and warmth for the
//!   teller rise, a favour is owed. A dry hollow seen there **disconfirms** it: trust falls, and the
//!   episode is held open as a mistake or a lie until evidence decides.
//! - **Notes** are physical objects with a node of their own. Reading one grounds its claim with
//!   the note as a source; a note the mind's thought reaches while it is carried is reread.
//! - **Talk** is chosen from the mind's own state: ask when thirsty and no water is near in mind,
//!   answer from what imagining "water" brings up, thank, correct oneself, or (a liar) mislead.
//!
//! What is a rule here rather than an emergent effect is listed in the stage note: the encoding
//! targets, the offloading discount, the trust updates and the lie/mistake evidence weights.

use super::species::{found, people, Founding, Ids};
use super::talk::{band_dir, from_ref, Content, Drawn, Ref, Say};
use super::world::{d2, dist, Deed, Kind, World, NAMES, PEOPLE, PLACE_RADIUS, SPEAK, TALK};
use crate::graph::{lt, tag, Anchor};
use crate::mind::{key, Action, Mind, Sense, Temperament};
use crate::rng::hash;
use crate::store::{Runtime, Store};
use crate::think::Form;
use std::hash::{Hash, Hasher};

/// How a person prefers to pass on what it knows.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Medium { Speak, Note, Map }

/// Character, as numbers (per mille).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Traits { pub honesty: i32, pub helpful: i32 }
impl Default for Traits { fn default() -> Self { Traits { honesty: 900, helpful: 900 } } }

/// The people layer's record of one grounded claim (bookkeeping for checking and for measures;
/// what the mind *believes* is in its graph).
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Told {
    pub thing: u32,
    pub place: usize,
    pub from: Option<usize>,
    pub via: Option<usize>,
    pub note: Option<u32>,
    pub overheard: bool,
    pub drawn: bool,
    pub est: (i32, i32),
    pub tol: i32,
    pub at: u32,
    pub times: u32,
    pub arrived: Option<u32>,
    pub confirmed: Option<u32>,
    pub dry: Option<u32>,
    /// The same teller said this place was dry before saying it held water.
    pub self_contradiction: bool,
    pub corrected: bool,
    pub trust_before: i32,
    pub landmark_checks: (u32, u32),
    /// This record is a claim that the place is dry (not that it holds water).
    pub dry_claim: bool,
    /// Warmth already lowered for a lie.
    pub blamed: bool,
    /// The telling met a belief of my own that the place is dry: (told strength, own strength).
    pub conflict: Option<(i32, i32)>,
}

#[derive(Clone, Debug)]
pub struct Person {
    pub i: usize,
    pub m: Mind<Runtime>,
    pub ids: Ids,
    pub traits: Traits,
    pub medium: Medium,
    /// Landmark word → this person's own place node for it.
    pub lex: Vec<(u32, u32)>,
    /// This person's node for each other person.
    pub pnode: [u32; PEOPLE],
    pub told: Vec<Told>,
    /// (note id, own node for it, last tick read).
    pub notes: Vec<(u32, u32, u32)>,
    pub owe_thanks: Vec<usize>,
    /// (person, water thing) pairs already corrected.
    pub corrected_to: Vec<(usize, u32)>,
    /// Keeps a model of what others know (ablation: off).
    pub model_on: bool,
    /// Offloading on: a fact read from a note one keeps is encoded less in the head.
    pub offload: bool,
    pub last_ask: Option<u32>,
    /// What this person said, by kind (for measures): tells, reminds, refusals, lies, corrections.
    pub said: [u32; 6],
    pub reads: Vec<u32>,
    pub groundings_failed: u32,
    /// What it will say on the next ticks (an answer in two parts).
    pub pending: Vec<(Say, usize)>,
    /// Landmarks (words) it would like to know where they are: it asks whoever is near.
    pub curious: Vec<u32>,
    /// Staging only: this person is away (it neither thinks nor acts; the scenario has sent it off).
    pub away: bool,
    /// Stage 3: how far each other person was when last seen (for "came to me" or "left me").
    pub seen_d: [i32; PEOPLE],
    /// Stage 4: the hunting layer (plans, roles, the betrayal), for those who hunt.
    pub hunt: Option<Box<crate::hunt::hunter::Hunter>>,
}

pub const READ_AT: i32 = 60;
pub const REREAD_GAP: u32 = 40;
pub const ASK_GAP: u32 = 120;
pub const KNOWS_AT: i32 = 300;
/// The deadline a told claim keeps for being checked (in ticks; then it is left as it is).
pub const SAID_TELL: usize = 0;
pub const SAID_REMIND: usize = 1;
pub const SAID_REFUSE: usize = 2;
pub const SAID_LIE: usize = 3;
pub const SAID_CORRECT: usize = 4;
pub const SAID_THANKS: usize = 5;

impl Hash for Person {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.m.state_hash().hash(h); self.traits.hash(h); self.medium.hash(h); self.lex.hash(h); self.pnode.hash(h); self.told.hash(h); self.notes.hash(h);
        // Element by element: std hashes a slice of `usize` as raw bytes (8 on native, 4 on wasm32),
        // which made native and wasm32 hashes part as soon as someone owed thanks (stage 3).
        self.owe_thanks.len().hash(h); for &a in &self.owe_thanks { (a as u64).hash(h); } self.corrected_to.hash(h); self.model_on.hash(h); self.offload.hash(h); self.last_ask.hash(h); self.said.hash(h); self.pending.hash(h); self.curious.hash(h); self.away.hash(h); self.seen_d.hash(h);
        if let Some(x) = &self.hunt { x.hash(h); }
    }
}

pub(crate) fn strengthen(m: &mut Mind<Runtime>, u: u32, v: u32, ty: u8, rate: i32, lval: Option<i32>) {
    // A delta rule toward a ceiling of 1000, with the claim's strength as the learning rate: a
    // first hearing at strength r gives r; each later one closes r per mille of what is left. So
    // a faded link relearns past where a fresh one starts (savings).
    let rate = rate.clamp(0, 1000);
    match m.store.find(u, v, ty) {
        Some(k) => {
            let l = m.store.link(k);
            let w = l.slow as i32;
            m.store.set_slow(k, (w + ((1000 - w) * rate + 999) / 1000).min(1000) as i16);
            if let Some(v) = lval { m.store.set_lval(k, (l.lval as i32 + (v - l.lval as i32) / 2) as i16); }
        }
        None => { m.store.add_link(u, v, ty, rate, lval.unwrap_or(0)); }
    }
}
pub(crate) fn weight(m: &Mind<Runtime>, u: u32, v: u32, ty: u8) -> i32 { m.store.find(u, v, ty).map(|k| m.store.slow(k) as i32).unwrap_or(0) }

impl Person {
    pub fn new(w: &World, i: usize, t: Temperament, f: &Founding) -> Person {
        let (m, lex, pnode) = found(w, i, t, f);
        Person { i, m, ids: people().ids, traits: Traits::default(), medium: Medium::Speak, lex, pnode, told: vec![], notes: vec![], owe_thanks: vec![], corrected_to: vec![],
            model_on: true, offload: true, last_ask: None, said: [0; 6], reads: vec![], groundings_failed: 0, pending: vec![], curious: vec![], away: false, seen_d: [-1; PEOPLE], hunt: None }
    }
    pub fn state_hash(&self) -> u64 { let mut h = crate::rng::StateHasher::new(self.m.who); self.hash(&mut h); h.finish() }

    pub fn trust(&self, j: usize) -> i32 { weight(&self.m, self.pnode[j], self.ids.truth, lt::ASSOC) }
    pub fn set_trust(&mut self, j: usize, v: i32) { let k = self.m.store.find(self.pnode[j], self.ids.truth, lt::ASSOC).unwrap(); self.m.store.set_slow(k, v.clamp(0, 1000) as i16); }
    pub fn warmth(&self, j: usize) -> i32 { self.m.store.val(self.pnode[j]) }
    pub fn debt(&self, j: usize) -> i32 { weight(&self.m, self.pnode[j], self.ids.debt, lt::ASSOC) }
    pub fn place_xy(&self, pi: usize) -> (i32, i32) { self.m.store.anchor(self.m.place_nodes[pi]).unwrap().xy() }
    pub fn lex_place(&self, wd: u32) -> Option<u32> { self.lex.iter().find(|l| l.0 == wd).map(|l| l.1) }
    fn pindex(&self, pn: u32) -> usize { self.m.place_nodes.iter().position(|&p| p == pn).unwrap() }
    /// The place a water thing is held at (its AT link).
    pub fn place_of_thing(&self, n: u32) -> Option<usize> {
        self.m.store.links_vec(n).into_iter().filter(|l| l.ty == lt::AT && l.slow > 0).max_by_key(|l| (l.slow, std::cmp::Reverse(l.to))).and_then(|l| self.m.place_nodes.iter().position(|&p| p == l.to))
    }
    /// The water things this mind holds (founded, seen or told), with the place each is held at.
    pub fn water_things(&self) -> Vec<(u32, usize)> {
        let mut v = vec![];
        for (k, &(_, n)) in self.m.entity_nodes.iter().enumerate() {
            if self.m.entity_kinds[k] == Kind::Water.k() { if let Some(pi) = self.place_of_thing(n) { if !v.iter().any(|x: &(u32, usize)| x.0 == n) { v.push((n, pi)); } } }
        }
        v
    }
    /// Does this mind expect water at a place (its own prediction, at least `IGNORE_BELOW`)?
    pub fn believes_wet(&self, pi: usize) -> bool { self.m.expect[pi][key(Kind::Water.k(), false)] as i32 >= crate::mind::IGNORE_BELOW }
    /// Has this mind come to expect a dry hollow at a place (it has seen it dry)?
    pub fn seen_dry(&self, pi: usize) -> bool {
        [Kind::DryFresh, Kind::DryOld].iter().any(|k| self.m.expect[pi][key(k.k(), false)] as i32 >= crate::mind::IGNORE_BELOW)
    }
    /// Seeing a landmark corrects where the mind had put its place: a place whose main expected
    /// landmark is seen within the place radius is moved onto it (and its distances relinked).
    fn correct_map(&mut self, s: &Sense) {
        for e in &s.seen {
            if !self.m.kinds[e.kind as usize].landmark { continue; }
            let Some(pi) = self.m.place_index_at(e.x, e.y) else { continue };
            let ex = &self.m.expect[pi];
            let k = key(e.kind, false);
            if (ex[k] as i32) < crate::mind::IGNORE_BELOW || (0..ex.len()).any(|j| ex[j] > ex[k]) { continue; }
            if self.place_xy(pi) == (e.x, e.y) { continue; }
            self.m.store.set_anchor(self.m.place_nodes[pi], Anchor::Place { x: e.x as i16, y: e.y as i16 });
            for q in 0..self.m.place_nodes.len() { self.m.link_near(pi, q); }
        }
    }

    // ------------------------------------------------------------------------- describe

    /// Describe a place the way this mind holds it: by the two landmarks it knows by name nearest
    /// to where it believes the place is (spoken), or by every named landmark within 30 cells
    /// (drawn).
    pub fn describe(&self, pi: usize, drawn: bool, dry: bool, listener: usize, thing: u32) -> Content {
        let (px, py) = self.place_xy(pi);
        let pn = self.m.place_nodes[pi];
        let mut lm: Vec<(i32, u32, i32, i32)> = self.lex.iter().filter(|l| l.1 != pn).map(|&(wd, n)| {
            let (x, y) = self.m.store.anchor(n).unwrap().xy(); (d2(x, y, px, py), wd, x, y)
        }).collect();
        lm.sort();
        let conf = if self.told.iter().any(|t| t.thing == thing && t.confirmed.is_none()) { weight(&self.m, thing, self.ids.water, lt::KIND).min(900) } else { 900 };
        // "X said so": the strongest source link from the thing to a person who isn't the listener.
        let via = (0..PEOPLE).filter(|&j| j != self.i && j != listener).map(|j| (weight(&self.m, thing, self.pnode[j], lt::SOURCE), j)).filter(|x| x.0 >= 150).max().map(|x| x.1);
        if drawn {
            let d: Vec<Drawn> = lm.iter().filter(|x| x.0 <= 30 * 30).map(|&(_, wd, x, y)| Drawn { landmark: wd, dx: ((x - px) as f32 / 2.0).round() as i8, dy: ((y - py) as f32 / 2.0).round() as i8 }).collect();
            Content { refs: vec![], drawn: d, conf, via, dry }
        } else {
            let r: Vec<Ref> = lm.iter().take(2).map(|&(_, wd, x, y)| { let (band, dir) = band_dir(px - x, py - y); Ref { landmark: wd, dir, band } }).collect();
            Content { refs: r, drawn: vec![], conf, via, dry }
        }
    }

    // ------------------------------------------------------------------------- ground

    /// Ground a claim in this mind's own nodes. `from`: who said or wrote it (None: unknown).
    /// Returns the index of the told record, and adds cues (what hearing it brings to mind).
    pub fn ground(&mut self, c: &Content, from: Option<usize>, overheard: bool, note: Option<u32>, tick: u32, cues: &mut Vec<(u32, i32)>) -> Option<usize> {
        let ids = self.ids;
        let trust = from.map(|a| self.trust(a)).unwrap_or(400);
        // Where my own map puts it.
        let mut ests: Vec<(i32, i32, i32)> = vec![];
        for r in &c.refs { if let Some(n) = self.lex_place(r.landmark) { let (x, y) = self.m.store.anchor(n).unwrap().xy(); ests.push(from_ref(x, y, r)); } }
        let mut unknown: Vec<Drawn> = vec![];
        for d in &c.drawn {
            match self.lex_place(d.landmark) { Some(n) => { let (x, y) = self.m.store.anchor(n).unwrap().xy(); ests.push((x - d.dx as i32 * 2, y - d.dy as i32 * 2, 2)); } None => unknown.push(*d) }
        }
        if ests.is_empty() { self.groundings_failed += 1; return None; }
        let k = ests.len() as i32;
        let est = (ests.iter().map(|e| e.0).sum::<i32>() / k, ests.iter().map(|e| e.1).sum::<i32>() / k);
        let spread = ests.iter().map(|e| dist(e.0, e.1, est.0, est.1)).max().unwrap_or(0);
        let tol = ests.iter().map(|e| e.2).sum::<i32>() / k + spread / 2;
        // A drawn map can check itself against what I know: every pair of drawn landmarks I know
        // whose drawn offset agrees with mine (within 3 cells) is a small confirmed claim.
        let (mut agree, mut disagree) = (0u32, 0u32);
        let known: Vec<(u32, i32, i32)> = c.drawn.iter().filter_map(|d| self.lex_place(d.landmark).map(|n| { let (x, y) = self.m.store.anchor(n).unwrap().xy(); (d.landmark, x - d.dx as i32 * 2, y - d.dy as i32 * 2) })).collect();
        for a in 0..known.len() { for b in a + 1..known.len() { if dist(known[a].1, known[a].2, known[b].1, known[b].2) <= 3 { agree += 1 } else { disagree += 1 } } }
        // Landmarks on a map I didn't know become places of my own, where the map puts them.
        for d in unknown {
            let (x, y) = ((est.0 + d.dx as i32 * 2).clamp(0, 63), (est.1 + d.dy as i32 * 2).clamp(0, 39));
            let pi = self.m.add_place(&format!("landmark {} (from a map)", d.landmark), x, y, None);
            let pn = self.m.place_nodes[pi];
            self.lex.push((d.landmark, pn));
        }
        // Which place: water I already know (or a place I was told of) within the description's
        // uncertainty, or a new place of my own.
        let r2 = tol.max(PLACE_RADIUS).pow(2);
        let known_water = self.water_things().into_iter().map(|(_, pi)| { let (x, y) = self.place_xy(pi); (d2(x, y, est.0, est.1), pi) })
            .chain(self.told.iter().map(|t| { let (x, y) = self.place_xy(t.place); (d2(x, y, est.0, est.1), t.place) }))
            .filter(|x| x.0 <= r2).min().map(|x| x.1);
        // (Never a landmark's place: the water would then be moved onto the spring when found,
        // and the landmark's place with it.)
        let pi = match known_water {
            Some(pi) => pi,
            None => {
                let name = format!("water place, told by {}", from.map(|a| NAMES[a]).unwrap_or("a note"));
                self.m.add_place(&name, est.0, est.1, None)
            }
        };
        let pn = self.m.place_nodes[pi];
        // The water held there: one I already have at that place, or a new tentative node.
        let s = match self.water_things().into_iter().find(|x| x.1 == pi) {
            Some((s, _)) => s,
            None => {
                let s = self.m.store.add_node(&format!("water (from {})", from.map(|a| NAMES[a]).unwrap_or("a note")), 200, tag::FOOD, Some(Anchor::Place { x: est.0 as i16, y: est.1 as i16 }), self.m.who);
                self.m.entity_nodes.push((0x8000_0000 | s, s)); self.m.entity_kinds.push(Kind::Water.k());
                s
            }
        };
        let idx = match self.told.iter().position(|t| t.thing == s && t.from == from && t.dry_claim == c.dry) {
            Some(t) => { self.told[t].times += 1; t }
            None => {
                self.told.push(Told { thing: s, place: pi, from, via: c.via, note, overheard, drawn: !c.drawn.is_empty(), est, tol, at: tick, times: 1, arrived: None, confirmed: None, dry: None,
                    self_contradiction: false, corrected: false, trust_before: trust, landmark_checks: (agree, disagree), dry_claim: c.dry, blamed: false, conflict: None });
                self.told.len() - 1
            }
        };
        if agree + disagree > 0 && self.told[idx].times == 1 {
            if let Some(a) = from { let t0 = self.trust(a); self.set_trust(a, t0 + agree as i32 * (1000 - t0) / 20 - disagree as i32 * t0 / 10); }
        }
        let trust = from.map(|a| self.trust(a)).unwrap_or(400);
        let mut enc = 300 + trust * 7 / 10;
        if overheard { enc = enc * 7 / 10; }
        let holding = note.is_some_and(|nid| self.notes.iter().any(|x| x.0 == nid));
        if holding && self.offload { enc = enc * 7 / 10; }
        let tf = c.conf * enc / 1000;
        let ts = if overheard { 350 } else { 700 };
        let lv = trust - 500;
        if c.dry {
            // "No water there": the expectation falls, and if this same teller later says it holds
            // water, that contradicts what they said before.
            let e = &mut self.m.expect[pi][key(Kind::Water.k(), false)];
            *e = (*e as i32 - c.conf * trust / 1000).max(0) as i16;
            strengthen(&mut self.m, pn, ids.dry, lt::ASSOC, tf, None);
        } else {
            // The same teller said this place was dry: one of the two claims is false. That is
            // evidence of a lie at once, and trust falls as for a failed claim.
            if let Some(a) = from {
                if !self.told[idx].self_contradiction && self.told.iter().any(|t| t.place == pi && t.from == Some(a) && t.dry_claim) {
                    self.told[idx].self_contradiction = true;
                    strengthen(&mut self.m, s, ids.lie, lt::ASSOC, 700, None);
                    let tr = self.trust(a); self.set_trust(a, tr * 6 / 10);
                    // Caught out: blamed as for a lie found at the spring (warmth falls once).
                    self.told[idx].blamed = true;
                    let n = self.pnode[a]; let v = self.m.store.val(n) as i32; self.m.store.set_val(n, (v - 300).max(-1000) as i16);
                }
            }
            strengthen(&mut self.m, s, ids.water, lt::KIND, tf, None);
            strengthen(&mut self.m, ids.water, s, lt::ASSOC, tf * 6 / 7, None);
            strengthen(&mut self.m, s, pn, lt::AT, tf, None);
            strengthen(&mut self.m, pn, s, lt::HOLDS, tf, None);
            // Conflict with my own belief that it is dry: the stronger belief wins, by its margin.
            // Told strength is stated confidence × trust; my own is how strongly I expect the
            // hollow. If the telling wins, I come to expect water there (and the water there to
            // feel good) in proportion to the margin; if not, I keep my belief and only remember
            // being told.
            let told = c.conf * trust / 1000;
            let own = [Kind::DryFresh, Kind::DryOld].iter().map(|k| self.m.expect[pi][key(k.k(), false)] as i32).max().unwrap_or(0);
            let won = told > own;
            if own > 0 { self.told[idx].conflict = Some((told, own)); }
            if won {
                let e = &mut self.m.expect[pi][key(Kind::Water.k(), false)];
                *e = (*e as i32).max(told - own) as i16;
                if own > 0 { let v = (200 * (told - own) / told.max(1)) as i16; if self.m.store.val(s) < v as i32 { self.m.store.set_val(s, v); } }
            }
        }
        if let Some(a) = from {
            let an = self.pnode[a];
            strengthen(&mut self.m, s, an, lt::SOURCE, ts, Some(lv));
            strengthen(&mut self.m, an, s, lt::SOURCE, ts / 2, None);
            strengthen(&mut self.m, pn, an, lt::SOURCE, ts, Some(lv));
            cues.push((an, 200));
        }
        if let Some(v) = c.via { if v != self.i && Some(v) != from { let vn = self.pnode[v]; let lv2 = self.trust(v) - 500; strengthen(&mut self.m, s, vn, lt::SOURCE, ts / 2, Some(lv2)); } }
        if let Some(nid) = note {
            if let Some(&(_, nn, _)) = self.notes.iter().find(|x| x.0 == nid) {
                strengthen(&mut self.m, nn, s, lt::ASSOC, 800, None);
                strengthen(&mut self.m, s, nn, lt::ASSOC, 600, None);
                if let Some(a) = from { strengthen(&mut self.m, nn, self.pnode[a], lt::SOURCE, ts, Some(lv)); }
            }
        }
        cues.push((s, 500)); cues.push((pn, 300));
        Some(idx)
    }

    // ------------------------------------------------------------------------- notes

    fn note_node(&mut self, id: u32, writer: usize, x: i32, y: i32) -> u32 {
        if let Some(&(_, n, _)) = self.notes.iter().find(|x| x.0 == id) { return n; }
        let n = self.m.store.add_node(&format!("note from {}", NAMES[writer]), 150, tag::CREATURE, Some(Anchor::Entity { id, x: x as i16, y: y as i16 }), self.m.who);
        self.m.entity_nodes.push((id, n)); self.m.entity_kinds.push(Kind::Note.k());
        strengthen(&mut self.m, n, self.ids.note, lt::KIND, 600, None);
        self.notes.push((id, n, 0));
        n
    }
    fn read(&mut self, w: &World, id: u32, cues: &mut Vec<(u32, i32)>) {
        let note = w.notes.iter().find(|n| n.id == id).unwrap();
        let (content, writer) = (note.content.clone(), note.writer);
        let n = self.note_node(id, writer, note.x, note.y);
        let from = if writer == self.i { None } else { Some(writer) };
        self.ground(&content, from, false, Some(id), w.tick, cues);
        if let Some(x) = self.notes.iter_mut().find(|x| x.0 == id) { x.2 = w.tick; }
        cues.push((n, 400));
        self.reads.push(w.tick);
    }

    // ------------------------------------------------------------------------- a tick

    /// One tick: hear, read, check, think, speak. Reads the world as it was at the start of the
    /// tick; writes only this person (and returns what it does to the world).
    pub fn think(&mut self, w: &World) -> Deed {
        let i = self.i;
        let mut deed = Deed::default();
        if self.away || w.people[i].away { return deed; }
        let mut s: Sense = w.sense(i, false);
        if s.body.asleep { let a = self.m.step(&s); self.m.commit(&s, a); deed.action = Some(a); return deed; }
        let mut cues = vec![];
        let mut alarms = vec![];
        // Hearing.
        let heard = w.heard(i);
        let mut asked_water: Vec<usize> = vec![];
        let mut asked_other: Vec<(usize, u32)> = vec![];
        for (u, loud, to_me) in &heard {
            let a = u.speaker;
            cues.push((self.pnode[a], (*loud / 2).min(400)));
            match &u.say {
                Say::AskWater => { if *to_me { asked_water.push(a); } }
                // Stage 3: a cry for help. Who cried is known by voice and where by ear; what it
                // means (come, flee, ignore it) is up to this mind.
                Say::Help => {
                    cues.push((self.ids.cry, (*loud / 2).min(600)));
                    let n = self.pnode[a];
                    self.m.store.set_anchor(n, Anchor::Entity { id: super::world::person_id(a), x: u.x as i16, y: u.y as i16 });
                    alarms.push((n, u.x, u.y));
                }
                Say::AskOther(wd) => { if *to_me { asked_other.push((a, *wd)); } }
                Say::Thanks => {
                    if *to_me { let n = self.pnode[a]; let v = self.m.store.val(n); self.m.store.set_val(n, (v + (1000 - v) / 8) as i16); }
                }
                Say::Tell(c) | Say::Remind(c) | Say::Correct(c) => {
                    let idx = self.ground(c, Some(a), !*to_me, None, w.tick, &mut cues);
                    if let (Say::Correct(_), Some(t)) = (&u.say, idx) {
                        // The teller warns that the water it told of is gone: an honest mistake.
                        let s = self.told[t].thing;
                        for k in 0..self.told.len() { if self.told[k].thing == s && self.told[k].from == Some(a) && !self.told[k].dry_claim { self.told[k].corrected = true; } }
                        self.reappraise(s);
                    }
                }
                // Stage 4: plans, answers, signals and warnings are heard by the hunting layer.
                Say::Propose(_) | Say::Agree(_) | Say::Decline(_) | Say::Signal | Say::Caught | Say::Warn(_) => {}
                Say::TellOther(wd, c) => {
                    // A landmark I asked about: a place of my own where my map puts it.
                    if *to_me && self.lex_place(*wd).is_none() {
                        let mut ests = vec![];
                        for r in &c.refs { if let Some(n) = self.lex_place(r.landmark) { let (x, y) = self.m.store.anchor(n).unwrap().xy(); ests.push(from_ref(x, y, r)); } }
                        if !ests.is_empty() {
                            let k = ests.len() as i32;
                            let (x, y) = (ests.iter().map(|e| e.0).sum::<i32>() / k, ests.iter().map(|e| e.1).sum::<i32>() / k);
                            let pi = self.m.add_place(&format!("landmark {wd} (told by {})", NAMES[a]), x, y, None);
                            let pn = self.m.place_nodes[pi];
                            self.lex.push((*wd, pn));
                            let (an, lv) = (self.pnode[a], self.trust(a) - 500); strengthen(&mut self.m, pn, an, lt::SOURCE, 700, Some(lv));
                        }
                    }
                }
            }
        }
        // Notes: newly handed ones are read at once; one the mind's thought reaches is reread; a
        // note on the ground within reach is picked up.
        let carried: Vec<(u32, usize, i32, i32)> = w.notes.iter().filter(|n| n.holder == Some(i)).map(|n| (n.id, n.writer, n.x, n.y)).collect();
        for (id, _, _, _) in &carried {
            match self.notes.iter().find(|x| x.0 == *id).copied() {
                None => self.read(w, *id, &mut cues),
                Some((_, n, last)) => if self.m.act(n) >= READ_AT && w.tick >= last + REREAD_GAP { self.read(w, *id, &mut cues); },
            }
        }
        if let Some(n) = w.notes.iter().find(|n| n.holder.is_none() && (n.x - s.body.x).abs().max((n.y - s.body.y).abs()) <= 1) { deed.pick_up = Some(n.id); }
        // Checking what I was told against what I see, and my map against the landmarks.
        self.check(&s, w.tick);
        self.correct_map(&s);
        if w.dusk { cues.push((self.ids.rest, 600)); cues.push((self.ids.fatigue, 300)); }
        if self.hunt.is_some() { self.hunt_hear(w, &s, &heard, &mut cues); }
        s.cues = cues;
        s.alarms = alarms;
        let a = self.m.step(&s);
        let a = if self.hunt.is_some() { self.hunt_act(&s, a) } else { a };
        self.m.commit(&s, a);
        deed.action = Some(a);
        self.learn_allies(&s);
        if a == Action::Call || self.m.held.calling { deed.say = Some((Say::Help, None, super::world::SHOUT)); return deed; }
        if self.hunt.is_some() {
            if let Some(say) = self.hunt_speak(&s, w.tick) { self.hunt_after(&s, w.tick, Some(&say.0)); deed.say = Some(say); return deed; }
            self.hunt_after(&s, w.tick, None);
        }
        // Seeing someone at a water place I know: they know it too (Ash learns it was overheard).
        if self.model_on {
            for p in &self.m.last.percepts.clone() {
                if let Some(j) = (0..PEOPLE).find(|&j| j != i && self.pnode[j] == p.node) {
                    for (thing, pi) in self.water_things() {
                        let (px, py) = self.place_xy(pi);
                        if d2(p.x, p.y, px, py) <= 2 * 2 { strengthen(&mut self.m, self.pnode[j], thing, lt::ASSOC, 700, None); }
                    }
                }
            }
        }
        deed.say = self.speak(w, &s, &asked_water, &asked_other, &mut deed);
        deed
    }

    /// Stage 3: who stands with me, learnt from what I see while I am in danger. Someone who comes
    /// towards me or is near the threat with me counts more as an ally; someone who moves away
    /// from me counts less. (A rule, like the trust updates: listed in the stage note.)
    fn learn_allies(&mut self, s: &Sense) {
        let ids = self.ids;
        // The threat: a creature in sight that feels bad to this mind (its own valence, as in
        // `Mind::respond`), not a kind named here.
        let threat = self.m.last.percepts.iter().filter(|p| self.m.kinds[p.kind as usize].creature && self.m.store.val(p.node) < 0).map(|p| (p.x, p.y)).next();
        let in_danger = self.m.act(ids.danger) >= crate::mind::OPTION_AT && threat.is_some();
        for j in (0..PEOPLE).filter(|&j| j != self.i) {
            let Some(p) = self.m.last.percepts.iter().find(|p| p.node == self.pnode[j]).cloned() else { self.seen_d[j] = -1; continue };
            let d = dist(p.x, p.y, s.body.x, s.body.y);
            if in_danger {
                let with_me = p.approaching || threat.is_some_and(|(tx, ty)| dist(p.x, p.y, tx, ty) <= 4);
                let left = self.seen_d[j] >= 0 && d > self.seen_d[j] && !with_me;
                if with_me { strengthen(&mut self.m, self.pnode[j], ids.ally, lt::ASSOC, 60, None); }
                else if left { if let Some(k) = self.m.store.find(self.pnode[j], ids.ally, lt::ASSOC) { let w0 = self.m.store.slow(k) as i32; self.m.store.set_slow(k, (w0 * 9 / 10) as i16); } }
            }
            self.seen_d[j] = d;
        }
    }

    /// Check unresolved claims against what is in sight: confirm on finding the water, disconfirm
    /// on finding the place dry.
    fn check(&mut self, s: &Sense, tick: u32) {
        let ids = self.ids;
        for t in 0..self.told.len() {
            if self.told[t].confirmed.is_some() || self.told[t].dry.is_some() || self.told[t].dry_claim { continue; }
            let pi = self.told[t].place;
            let pn = self.m.place_nodes[pi];
            let (px, py) = self.place_xy(pi);
            if self.told[t].arrived.is_none() && d2(s.body.x, s.body.y, px, py) <= 3 * 3 { self.told[t].arrived = Some(tick); }
            let snap = self.told[t].tol + PLACE_RADIUS;
            let s_node = self.told[t].thing;
            if let Some(e) = s.seen.iter().filter(|e| e.kind == Kind::Water.k() && d2(e.x, e.y, px, py) <= snap * snap).min_by_key(|e| (d2(e.x, e.y, px, py), e.id)) {
                // Confirmed: the tentative place and water are moved onto the real thing.
                self.m.store.set_anchor(pn, Anchor::Place { x: e.x as i16, y: e.y as i16 });
                self.m.store.set_anchor(s_node, Anchor::Entity { id: e.id, x: e.x as i16, y: e.y as i16 });
                if let Some(k) = self.m.entity_nodes.iter().position(|x| x.1 == s_node) { self.m.entity_nodes[k].0 = e.id; }
                for q in 0..self.m.place_nodes.len() { self.m.link_near(pi, q); }
                for (u, v, ty) in [(s_node, ids.water, lt::KIND), (ids.water, s_node, lt::ASSOC), (s_node, pn, lt::AT), (pn, s_node, lt::HOLDS)] {
                    if let Some(k) = self.m.store.find(u, v, ty) { let w0 = self.m.store.slow(k); self.m.store.set_slow(k, w0.max(800)); }
                }
                self.m.expect[pi][key(Kind::Water.k(), false)] = 900;
                for k in 0..self.told.len() { if self.told[k].thing == s_node && self.told[k].confirmed.is_none() { self.told[k].confirmed = Some(tick); } }
                if let Some(a) = self.told[t].from {
                    let tr = self.trust(a); self.set_trust(a, tr + (1000 - tr) / 5);
                    let n = self.pnode[a]; let v = self.m.store.val(n); self.m.store.set_val(n, (v + (1000 - v) / 8) as i16);
                    strengthen(&mut self.m, n, ids.debt, lt::ASSOC, 600, None);
                    if !self.owe_thanks.contains(&a) { self.owe_thanks.push(a); }
                }
                continue;
            }
            if let Some(h) = s.seen.iter().find(|e| (e.kind == Kind::DryFresh.k() || e.kind == Kind::DryOld.k()) && d2(e.x, e.y, px, py) <= PLACE_RADIUS * PLACE_RADIUS) {
                let fresh = h.kind == Kind::DryFresh.k();
                for k in 0..self.told.len() { if self.told[k].thing == s_node && self.told[k].dry.is_none() && !self.told[k].dry_claim { self.told[k].dry = Some(tick); } }
                // The episode is held open: a mistake or a lie? The hollow's look is the first
                // evidence: freshly dry means it may have held water when I was told.
                // A freshly dry hollow says it probably held water when I was told (a mistake is
                // likelier); an old one says it was dry then, which a liar and a teller who hadn't
                // been back would both leave behind: no evidence either way.
                strengthen(&mut self.m, s_node, ids.mistake, lt::ASSOC, if fresh { 700 } else { 500 }, None);
                strengthen(&mut self.m, s_node, ids.lie, lt::ASSOC, if fresh { 200 } else { 500 }, None);
                let e = &mut self.m.expect[pi][key(Kind::Water.k(), false)]; *e = 0;
                if let Some(a) = self.told[t].from { self.told[t].trust_before = self.trust(a); }
                self.reappraise(s_node);
            }
        }
    }

    /// Weigh the evidence on a disconfirmed claim and set trust in its teller (and warmth, for a
    /// lie) accordingly. Lie or mistake is read off the mind's own links: thing → lie, thing →
    /// mistake.
    pub fn reappraise(&mut self, s_node: u32) {
        let ids = self.ids;
        let Some(t) = self.told.iter().position(|t| t.thing == s_node && t.dry.is_some() && t.from.is_some()) else { return };
        if self.told[t].self_contradiction { strengthen(&mut self.m, s_node, ids.lie, lt::ASSOC, 1000, None); strengthen(&mut self.m, s_node, ids.lie, lt::ASSOC, 1000, None); }
        if self.told[t].corrected { strengthen(&mut self.m, s_node, ids.mistake, lt::ASSOC, 1000, None); strengthen(&mut self.m, s_node, ids.mistake, lt::ASSOC, 1000, None); }
        let (lie, mis) = (weight(&self.m, s_node, ids.lie, lt::ASSOC), weight(&self.m, s_node, ids.mistake, lt::ASSOC));
        let a = self.told[t].from.unwrap();
        let tb = self.told[t].trust_before;
        let factor = if lie >= mis + 200 { 3 } else if mis >= lie + 200 { 9 } else { 6 };
        self.set_trust(a, tb * factor / 10);
        if lie >= mis + 200 && !self.told[t].blamed { self.told[t].blamed = true; let n = self.pnode[a]; let v = self.m.store.val(n) as i32; self.m.store.set_val(n, (v - 300).max(-1000) as i16); }
    }
    /// How this mind classes a disconfirmed claim now: (lie weight, mistake weight).
    pub fn verdict(&self, s_node: u32) -> (i32, i32) { (weight(&self.m, s_node, self.ids.lie, lt::ASSOC), weight(&self.m, s_node, self.ids.mistake, lt::ASSOC)) }

    // ------------------------------------------------------------------------- speech

    fn roll(&self, w: &World, salt: u64) -> i32 { (hash(w.seed, w.tick as u64, self.i as u64, salt) % 1000) as i32 }
    fn near_people(&self, w: &World, r: i32) -> Vec<usize> {
        let me = &w.people[self.i];
        let mut v: Vec<(i32, usize)> = (0..PEOPLE).filter(|&j| j != self.i && !w.people[j].asleep).map(|j| (d2(me.x, me.y, w.people[j].x, w.people[j].y), j)).filter(|x| x.0 <= r * r).collect();
        v.sort(); v.into_iter().map(|x| x.1).collect()
    }

    fn speak(&mut self, w: &World, s: &Sense, asked: &[usize], asked_other: &[(usize, u32)], deed: &mut Deed) -> Option<(Say, Option<usize>, i32)> {
        let near = self.near_people(w, TALK);
        if let Some(k) = self.pending.iter().position(|p| near.contains(&p.1)) { let (say, j) = self.pending.remove(k); return Some((say, Some(j), SPEAK)); }
        // Thanks first.
        if let Some(k) = self.owe_thanks.iter().position(|a| near.contains(a)) {
            let a = self.owe_thanks.remove(k); self.said[SAID_THANKS] += 1; return Some((Say::Thanks, Some(a), SPEAK));
        }
        // An honest teller who finds its water gone warns whoever it believes it told.
        if self.traits.honesty >= 500 {
            for (thing, pi) in self.water_things() {
                if !self.seen_dry(pi) { continue; }
                for &j in &near {
                    if weight(&self.m, self.pnode[j], thing, lt::ASSOC) >= KNOWS_AT && !self.corrected_to.contains(&(j, thing)) {
                        self.corrected_to.push((j, thing)); self.said[SAID_CORRECT] += 1;
                        return Some((Say::Correct(self.describe(pi, false, true, j, thing)), Some(j), SPEAK));
                    }
                }
            }
        }
        if let Some(&a) = asked.first() {
            // What comes to mind when I think of water.
            // What comes to mind when I think of water, here (the place I'm at is part of the cue).
            let mut cue = vec![(self.ids.water, 1000)];
            if let Some(pi) = self.m.place_index_at(s.body.x, s.body.y) { cue.push((self.m.place_nodes[pi], crate::mind::PLACE_CONTEXT)); }
            let (_, act, _) = self.m.imagine(&cue, &crate::think::Mods::default(), 2);
            // The question is "where?": rank by the place that comes to mind (then its water).
            let mut cands: Vec<(i32, u32, usize)> = self.water_things().into_iter().map(|(n, pi)| (act[self.m.place_nodes[pi] as usize] * 4 + act[n as usize] / 256, n, pi)).collect();
            cands.sort_by_key(|x| (std::cmp::Reverse(x.0), x.1));
            let (ax, ay) = (w.people[a].x, w.people[a].y);
            let lying = self.traits.honesty < 500 && self.warmth(a) < 300;
            // Honest, and I know of water nearer the asker that has gone dry: say so first.
            let pick = if lying { cands.iter().find(|x| !self.believes_wet(x.2)) } else { cands.iter().find(|x| self.believes_wet(x.2) && { let (px, py) = self.place_xy(x.2); d2(px, py, ax, ay) > PLACE_RADIUS * PLACE_RADIUS }) };
            let willing = self.roll(w, 0x7e11) < self.traits.helpful + self.warmth(a) / 2 + self.debt(a) / 2;
            match (pick, willing) {
                (Some(&(_, thing, pi)), true) => {
                    if lying { self.said[SAID_LIE] += 1; }
                    let knows = self.model_on && weight(&self.m, self.pnode[a], thing, lt::ASSOC) >= KNOWS_AT;
                    if knows { self.said[SAID_REMIND] += 1; return Some((Say::Remind(self.describe(pi, false, false, a, thing)), Some(a), SPEAK)); }
                    self.said[SAID_TELL] += 1;
                    if self.model_on {
                        strengthen(&mut self.m, self.pnode[a], thing, lt::ASSOC, 800, None);
                        // Anyone else in earshot may have heard.
                        for j in self.near_people(w, 14) { if j != a { strengthen(&mut self.m, self.pnode[j], thing, lt::ASSOC, 300, None); } }
                    }
                    let c = self.describe(pi, self.medium == Medium::Map, false, a, thing);
                    let (px, py) = self.place_xy(pi);
                    let warn = if lying { None } else { cands.iter().filter(|x| self.seen_dry(x.2)).map(|x| { let (qx, qy) = self.place_xy(x.2); (d2(qx, qy, ax, ay), x.1, x.2) })
                        .filter(|x| x.0 < d2(px, py, ax, ay)).min() };
                    if self.medium == Medium::Speak {
                        if let Some((_, dn, dpi)) = warn {
                            self.pending.push((Say::Tell(c), a));
                            return Some((Say::Tell(self.describe(dpi, false, true, a, dn)), Some(a), SPEAK));
                        }
                        return Some((Say::Tell(c), Some(a), SPEAK));
                    }
                    deed.write = Some((c, a));
                    return None;
                }
                _ => { self.said[SAID_REFUSE] += 1; }
            }
        }
        if let Some(&(a, wd)) = asked_other.first() {
            if let Some(pn) = self.lex_place(wd) {
                let willing = self.roll(w, 0x7e12) < self.traits.helpful + self.warmth(a) / 2 + self.debt(a) / 2;
                if willing { let pi = self.pindex(pn); self.said[SAID_TELL] += 1; return Some((Say::TellOther(wd, self.describe(pi, false, false, a, u32::MAX)), Some(a), SPEAK)); }
                self.said[SAID_REFUSE] += 1;
            }
        }
        // Ask about a landmark I'd like to know.
        if let Some(&wd) = self.curious.iter().find(|&&wd| self.lex_place(wd).is_none()) {
            if let Some(&j) = near.first() { if self.last_ask.is_none_or(|t| w.tick >= t + ASK_GAP) { self.last_ask = Some(w.tick); return Some((Say::AskOther(wd), Some(j), SPEAK)); } }
        }
        // Ask, if thirsty and my own mind isn't taking me to water.
        if s.body.hunger >= 600 && self.last_ask.is_none_or(|t| w.tick >= t + ASK_GAP) {
            if let Some(&j) = near.first() {
                let going = match deed.action {
                    Some(Action::WalkTo { x, y }) => self.water_things().into_iter().any(|(_, pi)| self.place_xy(pi) == (x, y)),
                    Some(Action::Eat { .. }) => true,
                    _ => false,
                };
                if !going { self.last_ask = Some(w.tick); return Some((Say::AskWater, Some(j), SPEAK)); }
            }
        }
        None
    }
}

/// How far a told place was put from the truth, and how unsure the mind was (its search radius).
pub fn error_of(t: &Told, truth: (i32, i32)) -> (i32, i32) { (dist(t.est.0, t.est.1, truth.0, truth.1), t.tol) }
