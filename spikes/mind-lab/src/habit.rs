//! Round 2, stage 3 (9.5's C, the owner's interest): **a small brain kept aligned with a big
//! one.** A first try, as an exploration.
//!
//! - **The big brain** is the mind's full think (spreading, feeling, choosing), run every `every`
//!   ticks (`Mind::every`) or when a reflex interrupts.
//! - **The small brain** runs on the ticks between: the reflexes (unchanged), the body's procedures
//!   (a walk, a meal: `Mind::idle`), and a **habit table**, a tool written in the toolkit's
//!   language (`tools::lang`) and run by its interpreter, within the fast brain's reach (its
//!   worst-case cost, worked out from the description, must be under `Ceilings::fast`, 64, cut by
//!   stress as for any tool).
//! - **Alignment (distillation).** After every big think, what it chose is told to the habit table
//!   as a `did` record keyed by the situation it chose in (where the body is, and the need
//!   uppermost in the body). The table keeps the two commonest choices per situation, fading at
//!   night. So the small brain's "weights" are the big brain's own past choices, compiled.
//! - **When the small brain acts.** On a tick without a think, if what the body was doing has
//!   ended (`Mind::ended`) or the situation has changed since the last think, it asks the table
//!   and does the commonest choice there (walk to that place, eat what is in reach, stay). With no
//!   answer, or the table out of reach, the body goes on as before (`Mind::idle`).
//! - **Shadow mode** (for measuring): a mind thinking every tick keeps a table trained on every
//!   `every`-th tick's choice, and on the other ticks the table's answer is compared with what the
//!   full think chose. Nothing it does reaches the body.
//!
//! Hand-written (listed in the stage note): the situation key (place, and the strongest need over
//! `NEED_AT`, split at `NEED_HIGH`), the action code, the table's shape (2 votes, 512 entries,
//! fading by 1/8 a night), and when the small brain is consulted.

use crate::mind::{Action, Body, Mind, Sense};
use crate::rng::StateHasher;
use crate::store::Store;
use crate::tools::cost::{self, Brain, Ceilings};
use crate::tools::lang::{kind, parse, rec_of, Desc, F, NONE};
use crate::tools::run::{Out, Tool};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// The habit table, in the toolkit's language.
pub const HABIT: &str = r#"
(tool "habit"
  (slot did (key place node) (votes 2) act 512)
  (observe (if (= kind did) (put did (place what) act 1)))
  (ask (top did (place what) 1))
  (forget (fade did 3)))"#;

/// A need counts in the situation from this level, and is "high" from the second.
pub const NEED_AT: i32 = 300;
pub const NEED_HIGH: i32 = 700;
/// Action codes: class × `CODE` + the goal's place index (walks).
pub const CODE: i32 = 1024;
pub const WALK: i32 = 1;
pub const EAT: i32 = 2;
pub const STAY: i32 = 3;

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub struct SmallStats {
    /// Big thinks distilled into the table; ticks the small brain was consulted; answers acted on;
    /// consultations with no answer; out of reach (stress).
    pub trained: u32, pub asked: u32, pub acted: u32, pub empty: u32, pub unreachable: u32,
    /// Shadow mode: the table's answer against the full think's choice.
    pub agree: u32, pub disagree: u32, pub no_answer: u32,
    /// Shadow mode's baseline: "carry on with the last trained choice" (what a mind thinking every
    /// `stride` ticks without a small brain does) against the full think's choice.
    pub carry_agree: u32, pub carry_disagree: u32,
    /// Fuel spent asking (link reads' worth in the interpreter's units).
    pub fuel: u64,
}

#[derive(Clone, Debug)]
pub struct Small {
    pub tool: Tool,
    /// The table's worst-case ask cost (from its description).
    pub bound: i64,
    pub ceilings: Ceilings,
    /// Shadow mode's training stride (every this many ticks the full choice is distilled).
    pub stride: u32,
    pub shadow: bool,
    /// The situation at the last think (or the last consultation).
    pub key: (i32, i32),
    /// Shadow mode: the choice at the last training tick.
    pub last_code: i32,
    pub stats: SmallStats,
}

impl Hash for Small {
    fn hash<H: Hasher>(&self, h: &mut H) { self.tool.state_hash().hash(h); self.stride.hash(h); self.shadow.hash(h); self.key.hash(h); self.last_code.hash(h); }
}

impl Small {
    pub fn new(stride: u32, shadow: bool) -> Small {
        let d: Desc = parse(HABIT).expect("the habit table parses");
        let bound = cost::bound(&d, &[]).ask;
        Small { tool: Tool::new(Arc::new(d)), bound, ceilings: Ceilings::default(), stride: stride.max(1), shadow, key: (NONE, NONE), last_code: NONE, stats: SmallStats::default() }
    }
    pub fn state_hash(&self) -> u64 { let mut h = StateHasher::new(0x5a11); self.hash(&mut h); h.finish() }
}

/// The situation: the mind's own place index where the body is (or none), and the strongest need
/// over `NEED_AT` (its body node, ×2, +1 if over `NEED_HIGH`; 0 for none). Read from the body and
/// the mind's own places, without thinking.
pub fn situation<S: Store>(m: &Mind<S>, b: &Body) -> (i32, i32) {
    let place = m.place_index_at(b.x, b.y).map_or(NONE, |p| p as i32);
    let r = &m.roles;
    let needs = [(r.hunger, b.hunger), (r.hunger2, if r.hunger2 == u32::MAX { 0 } else { b.hunger2 }), (r.fatigue, m.mods.fatigue)];
    let top = needs.iter().filter(|x| x.0 != u32::MAX && x.1 >= NEED_AT).max_by_key(|x| (x.1, std::cmp::Reverse(x.0)));
    (place, top.map_or(0, |&(n, v)| n as i32 * 2 + (v >= NEED_HIGH) as i32))
}

/// An action as a code (`None`: not a habit: danger responses, sleep, snaps).
pub fn code_of<S: Store>(m: &Mind<S>, a: &Action) -> Option<i32> {
    match *a {
        Action::WalkTo { x, y } => m.place_index_at(x, y).map(|p| WALK * CODE + p as i32),
        Action::Eat { .. } => Some(EAT * CODE),
        Action::Stay => Some(STAY * CODE),
        _ => None,
    }
}

impl<S: Store> Mind<S> {
    /// After a big think: distil its choice into the small brain's table (or, in shadow mode,
    /// train on every `stride`-th tick and score the table on the others).
    pub fn distil(&mut self, s: &Sense, a: &Action) {
        let key = situation(self, &s.body);
        let code = code_of(self, a);
        let Some(sm) = self.small.as_mut() else { return };
        let r = |c: i32| rec_of(&[(F::Kind, kind::DID), (F::Place, key.0), (F::What, key.1), (F::Act, c)]);
        if sm.shadow {
            if (s.tick + self.phase) % sm.stride == 0 {
                if let Some(c) = code { let _ = sm.tool.observe(&r(c), 64); sm.stats.trained += 1; }
                sm.last_code = code.unwrap_or(NONE);
            } else if let Some(c) = code {
                if sm.last_code == c { sm.stats.carry_agree += 1 } else { sm.stats.carry_disagree += 1 }
                let (out, fuel) = sm.tool.ask(&r(NONE), sm.bound.max(1));
                sm.stats.fuel += fuel as u64;
                match out.ok().map(|o| o.top()) { Some(x) if x != NONE => if x == c { sm.stats.agree += 1 } else { sm.stats.disagree += 1 }, _ => sm.stats.no_answer += 1 }
            }
            return;
        }
        if let Some(c) = code { let _ = sm.tool.observe(&r(c), 64); sm.stats.trained += 1; }
        sm.key = key;
    }

    /// On a tick without a big think: the small brain's choice, if it makes one. Consulted when what
    /// the body was doing has ended or the situation has changed since the last think.
    pub fn small_choice(&mut self, s: &Sense) -> Option<Action> {
        let shadow = self.small.as_ref().is_none_or(|sm| sm.shadow);
        if shadow || s.body.asleep { return None; }
        let key = situation(self, &s.body);
        let ended = self.ended(s);
        let (stress, cut) = (self.mods.stress, self.temperament.stress_cut());
        let sm = self.small.as_mut().unwrap();
        if !ended && key == sm.key { return None; }
        sm.key = key;
        sm.stats.asked += 1;
        if sm.bound > cost::ceiling(&sm.ceilings, Brain::Fast, stress, cut) { sm.stats.unreachable += 1; return None; }
        let q = rec_of(&[(F::Kind, kind::DID), (F::Place, key.0), (F::What, key.1)]);
        let (out, fuel) = sm.tool.ask(&q, sm.bound.max(1));
        sm.stats.fuel += fuel as u64;
        let top = out.ok().map(|o: Out| o.top()).unwrap_or(NONE);
        if top == NONE { sm.stats.empty += 1; return None; }
        let a = match top / CODE {
            WALK => { let pi = (top % CODE) as usize; if pi < self.place_nodes.len() && self.place_learnt(pi) { let (x, y) = self.place_xy(pi); Some(Action::WalkTo { x, y }) } else { None } }
            EAT => s.seen.iter().filter(|e| self.can_eat(s, e.id)).min_by_key(|e| e.id).map(|e| Action::Eat { entity: e.id }),
            STAY => Some(Action::Stay),
            _ => None,
        };
        let sm = self.small.as_mut().unwrap();
        if a.is_some() { sm.stats.acted += 1; } else { sm.stats.empty += 1; }
        a
    }

    /// A night: the table fades.
    pub fn small_night(&mut self) {
        let n = self.nights as i32;
        if let Some(sm) = self.small.as_mut() { let _ = sm.tool.forget(n, i64::MAX / 4); }
    }
}
