//! The brain-tick interface: a mind's own tools (its "belt") and a few cheap calls.
//!
//! - `tell(record)`: what the mind sensed or did, stamped with the day and time of day, goes to
//!   every tool and to the tool-maker's log.
//! - `ask(tool, query, reach)`: an answer, ranked, if the tool is within reach (below).
//! - `bind(pattern, tools, query)`: when every node of `pattern` is in the workspace, the first
//!   of `tools` within reach is asked, and its answer comes back as **cues** (`tick`):
//!   activation on the mind's own nodes for the answer, as if it came to mind. Nothing else in
//!   the mind changes.
//! - `thought(question, pattern, cost)`: a question the mind thought through instead, and what
//!   that cost; the tool-maker counts these.
//! - `sleep()`: the tool-maker runs (bounded fuel); what it makes is filled from the log and
//!   bound to the question's pattern, with its cheap version (if any) as the fallback.
//!
//! **Reach.** Each tool's worst-case cost is worked out from its description when it is added
//! (`cost::bound`, sub-tools included). A call is allowed only if that bound is under the
//! ceiling for the brain asking (`Brain::Fast` every tick, `Slow` in a think, `Idle` in idle
//! time and sleep), cut by stress (`cost::ceiling`). A frightened mind cannot reach its map.
//!
//! The belt is the mind's own: no tool or record is shared with another mind. It is off unless
//! `ToolConfig::on` (default off), so nothing that does not ask for it changes.

use super::cost::{self, Bound, Brain, Ceilings};
use super::kit;
use super::lang::*;
use super::maker::Made;
use super::run::{Ans, Out, Tool};
use crate::rng::StateHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Buckets of time of day.
pub const TOD: i32 = 12;

/// Fill in the tick, the day and the time-of-day bucket.
pub fn stamp(r: &mut Rec, tick: u32, day_len: u32) {
    let d = day_len.max(1);
    r[F::Tick as usize] = tick as i32;
    r[F::Day as usize] = (tick / d) as i32;
    r[F::Tod as usize] = ((tick % d) as i64 * TOD as i64 / d as i64) as i32;
}

/// Settings for a mind's tools. Off by default.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct ToolConfig {
    pub on: bool,
    /// Ticks in a day (the mind's sense of time).
    pub day_len: u32,
    /// Fuel for one tell, and a night's tool-making.
    pub tell_fuel: i64,
    pub sleep_fuel: i64,
    /// What each brain may reach for (static bound ceilings).
    pub ceilings: Ceilings,
    /// Activation an answer's best value brings to mind (others in proportion to their score).
    pub gain: i32,
    /// A bound tool is asked at most once in this many ticks.
    pub quiet: u32,
}
impl Default for ToolConfig { fn default() -> Self { ToolConfig { on: false, day_len: 301, tell_fuel: 400, sleep_fuel: 40_000_000, ceilings: Ceilings::default(), gain: 800, quiet: 5 } } }

/// The state of the mind that asks: which brain, how stressed, and how much stress cuts it.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Reach { pub brain: Brain, pub stress: i32, pub cut: i32 }
impl Reach {
    pub fn calm(brain: Brain) -> Reach { Reach { brain, stress: 0, cut: 0 } }
}

/// A tool bound to a question pattern: tools in order of preference (the full one, then cheaper).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bind { pub pattern: Vec<u32>, pub tools: Vec<usize>, pub query: Rec, pub last: Option<u32>, pub fired: u32 }
// By hand: a `Vec<usize>` hashes as raw bytes, 8 per item natively and 4 on wasm32 (the wasm
// test caught this here too).
impl Hash for Bind {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.pattern.hash(h);
        h.write_u64(self.tools.len() as u64); for &t in &self.tools { h.write_u64(t as u64); }
        self.query.hash(h); self.last.hash(h); self.fired.hash(h);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BeltStats { pub tells: u64, pub tell_fuel: u64, pub asks: u64, pub ask_fuel: u64, pub fired: u64, pub spent: u64, pub sleep_fuel: u64, pub refused: u64, pub fell_back: u64 }

/// A question: (wanted field, kind of record that answers, given fields mask, subject).
pub type Question = (i32, i32, i32, i32);

#[derive(Clone, Debug)]
pub struct Belt {
    pub cfg: ToolConfig,
    pub tools: Vec<Tool>,
    /// Each tool's static bounds (with the tools before it as its sub-tools).
    pub bounds: Vec<Bound>,
    pub binds: Vec<Bind>,
    pub maker: Option<Tool>,
    /// The pattern each question was asked with, for binding what the maker makes.
    pub questions: Vec<(Question, Vec<u32>)>,
    /// What the maker made, each night it made something: (tick, how it scored, tool index, cheap
    /// version's index, the tool's name and the cheap one's, as made).
    pub made: Vec<(u32, Made, usize, Option<usize>, String, Option<String>)>,
    pub stats: BeltStats,
}

impl Belt {
    pub fn new(cfg: ToolConfig) -> Belt { Belt { cfg, tools: vec![], bounds: vec![], binds: vec![], maker: None, questions: vec![], made: vec![], stats: BeltStats::default() } }
    /// A belt with a tool-maker.
    pub fn with_maker(cfg: ToolConfig, maker_src: &str) -> Belt { let mut b = Belt::new(cfg); b.maker = Some(Tool::from(kit::load(maker_src))); b }

    pub fn stamp(&self, r: &mut Rec, tick: u32) { stamp(r, tick, self.cfg.day_len) }
    fn bound_of(&self, d: &Desc) -> Bound { let env: Vec<&Desc> = self.tools.iter().map(|t| &*t.desc).collect(); cost::bound(d, &env) }
    pub fn add(&mut self, d: Desc) -> usize { let b = self.bound_of(&d); self.tools.push(Tool::new(Arc::new(d))); self.bounds.push(b); self.tools.len() - 1 }
    fn replace(&mut self, i: usize, t: Tool) { let b = self.bound_of(&t.desc); self.tools[i] = t; self.bounds[i] = b; }
    pub fn find(&self, name: &str) -> Option<usize> { self.tools.iter().position(|t| t.desc.name == name) }

    /// May this mind, in this state, reach for this tool?
    pub fn reachable(&self, tool: usize, r: Reach) -> bool { self.bounds.get(tool).is_some_and(|b| b.ask <= cost::ceiling(&self.cfg.ceilings, r.brain, r.stress, r.cut)) }

    /// Tell every tool (and the maker) a record, at `tick`.
    pub fn tell(&mut self, mut r: Rec, tick: u32) {
        if !self.cfg.on { return; }
        self.stamp(&mut r, tick);
        let lim = self.cfg.tell_fuel;
        for t in self.tools.iter_mut().chain(self.maker.iter_mut()) {
            self.stats.tells += 1;
            match t.observe(&r, lim) { Ok(u) => self.stats.tell_fuel += u as u64, Err(_) => { self.stats.tell_fuel += lim as u64; self.stats.spent += 1; } }
        }
    }
    /// Ask a tool, now, if it is within reach (`None` if refused). Returns the answer and the fuel
    /// it took. The call's fuel limit is the tool's own static bound, so it can never run out.
    pub fn ask(&mut self, tool: usize, mut q: Rec, tick: u32, reach: Reach) -> Option<(Ans, i64)> {
        if !self.cfg.on || tool >= self.tools.len() { return None; }
        if !self.reachable(tool, reach) { self.stats.refused += 1; return None; }
        self.stamp(&mut q, tick);
        // Read only, with the mind's other tools to hand as sub-tools.
        let (r, used) = self.tools[tool].query("", &q, self.bounds[tool].ask, &self.tools);
        let u = &mut self.tools[tool].uses; u.asks += 1; u.ask_fuel += used as u64;
        self.stats.asks += 1; self.stats.ask_fuel += used as u64;
        match r { Ok(v) => Some((v, used)), Err(_) => { self.stats.spent += 1; Some((vec![], used)) } }
    }
    pub fn bind(&mut self, pattern: Vec<u32>, tools: Vec<usize>, query: Rec) {
        let mut p = pattern; p.sort_unstable(); p.dedup();
        self.binds.retain(|b| b.pattern != p);
        self.binds.push(Bind { pattern: p, tools, query, last: None, fired: 0 });
    }
    /// A question thought through (no tool answered it), and its cost in link reads.
    pub fn thought(&mut self, q: Question, pattern: &[u32], cost: i32, tick: u32) {
        if !self.cfg.on { return; }
        if !self.questions.iter().any(|x| x.0 == q) { let mut p = pattern.to_vec(); p.sort_unstable(); p.dedup(); self.questions.push((q, p)); }
        // A question about no one in particular is about "anyone" (-1): a missing key would not be counted.
        let mut r = rec_of(&[(F::Kind, kind::ASKED), (F::Want, q.0), (F::From, q.1), (F::Given, q.2), (F::Who, if q.3 == NONE { ANYONE } else { q.3 }), (F::Cost, cost)]);
        self.stamp(&mut r, tick);
        if let Some(m) = self.maker.as_mut() { let _ = m.observe(&r, self.cfg.tell_fuel); }
    }
    /// The binding (its index) whose pattern is in this workspace, if any.
    pub fn bound(&self, ws: &[u32]) -> Option<usize> { self.binds.iter().position(|b| b.pattern.iter().all(|n| ws.contains(n))) }
    /// The first tool of a binding within reach.
    pub fn pick(&self, bi: usize, reach: Reach) -> Option<usize> { self.binds.get(bi)?.tools.iter().copied().find(|&t| self.reachable(t, reach)) }

    /// The brain-tick call: for each binding whose pattern is in the workspace (and not asked in the
    /// last `quiet` ticks), ask the first of its tools within reach, with the binding's query
    /// filled in from `ctx` (where the mind is, where it is going: whatever the query leaves
    /// missing); turn the answer into cues through `ground` (value → node). Returns the cues, the
    /// fuel used, and the tools asked.
    pub fn tick(&mut self, ws: &[u32], tick: u32, ctx: &Rec, reach: Reach, ground: &dyn Fn(i32) -> Option<u32>) -> (Vec<(u32, i32)>, i64, Vec<usize>) {
        let (mut cues, mut fuel, mut asked) = (vec![], 0, vec![]);
        if !self.cfg.on { return (cues, 0, asked); }
        for bi in 0..self.binds.len() {
            let b = &self.binds[bi];
            if !b.pattern.iter().all(|n| ws.contains(n)) || b.last.is_some_and(|t| tick < t + self.cfg.quiet) { continue; }
            let mut q = b.query;
            for f in FIELDS { if q[f as usize] == NONE { q[f as usize] = ctx[f as usize]; } }
            let Some(tool) = self.pick(bi, reach) else { self.stats.refused += 1; continue };
            if Some(&tool) != self.binds[bi].tools.first() { self.stats.fell_back += 1; }
            let Some((ans, used)) = self.ask(tool, q, tick, reach) else { continue };
            fuel += used; asked.push(tool);
            let b = &mut self.binds[bi];
            b.last = Some(tick); b.fired += 1; self.stats.fired += 1;
            let top = ans.first().map_or(1, |x| x.1.max(1)) as i64;
            for (v, s) in ans { if let Some(n) = ground(v) { cues.push((n, (self.cfg.gain as i64 * s.max(0) as i64 / top) as i32)); } }
        }
        (cues, fuel, asked)
    }

    fn filled(&self, d: Desc) -> Tool {
        let mut t = Tool::new(Arc::new(d));
        if let Some(log) = self.maker.as_ref().and_then(|m| m.slots.iter().find_map(|s| match s { super::run::St::R(r) => Some(r.clone()), _ => None })) {
            let mut day = NONE;
            for r in log.iter() { if r[F::Day as usize] != day { if day != NONE { let _ = t.forget(r[F::Tick as usize], 1_000_000); } day = r[F::Day as usize]; } let _ = t.observe(r, self.cfg.tell_fuel); }
        }
        t
    }

    /// Sleep: forget, then let the maker make a tool for the question that cost most. A new tool is
    /// filled by replaying the maker's log and bound to the question's pattern (replacing one it
    /// made before for the same question if the new one scores higher), with its cheap version
    /// behind it. Returns the tool index.
    pub fn sleep(&mut self, tick: u32) -> Option<usize> {
        if !self.cfg.on { return None; }
        for t in self.tools.iter_mut() { let _ = t.forget(tick as i32, 1_000_000); }
        let m = self.maker.as_mut()?;
        let (out, used) = m.ask(&rec(), self.cfg.sleep_fuel);
        self.stats.sleep_fuel += used as u64;
        let _ = m.forget(tick as i32, 100_000);
        let Ok(Out::Made(d, made, cheap)) = out else { return None };
        let key = (made.want, made.from, made.given, made.who);
        let prev = self.made.iter().rev().find(|x| (x.1.want, x.1.from, x.1.given, x.1.who) == key).map(|x| (x.0, x.1, x.2, x.3));
        if let Some((_, pm, pi, _)) = prev {
            if self.tools[pi].desc.shape() == d.shape() || made.score <= pm.score { return None; }
        }
        let (name, cheap_name) = (d.name.clone(), cheap.as_ref().map(|c| c.0.name.clone()));
        let t = self.filled(*d);
        let idx = match prev { Some((_, _, pi, _)) => { self.replace(pi, t); pi } None => { let b = self.bound_of(&t.desc); self.tools.push(t); self.bounds.push(b); self.tools.len() - 1 } };
        let cheap_idx = cheap.map(|c| {
            let t = self.filled(c.0);
            match prev.and_then(|p| p.3) { Some(ci) => { self.replace(ci, t); ci } None => { let b = self.bound_of(&t.desc); self.tools.push(t); self.bounds.push(b); self.tools.len() - 1 } }
        });
        self.made.push((tick, made, idx, cheap_idx, name, cheap_name));
        if let Some((_, pat)) = self.questions.iter().find(|q| q.0 == key) {
            let q = rec_of(&[(F::Who, made.who), (F::Want, made.want)]);
            let pat = pat.clone();
            let mut tools = vec![idx];
            tools.extend(cheap_idx);
            self.bind(pat, tools, q);
        }
        Some(idx)
    }

    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new(0x7b17);
        self.cfg.hash(&mut h);
        h.write_u64(self.tools.len() as u64);
        for t in &self.tools { t.hash(&mut h); }
        self.bounds.hash(&mut h);
        self.binds.hash(&mut h);
        if let Some(m) = &self.maker { m.hash(&mut h); }
        h.write_u64(self.made.len() as u64);
        for (t, m, i, c, _, _) in &self.made { h.write_u32(*t); m.hash(&mut h); h.write_u64(*i as u64); h.write_u64(c.map_or(u64::MAX, |x| x as u64)); }
        h.finish()
    }
    /// Packed bytes: each tool's description and state, the maker's too.
    pub fn bytes(&self) -> usize { self.tools.iter().chain(self.maker.iter()).map(|t| t.desc.size() + t.bytes()).sum::<usize>() + self.binds.iter().map(|b| b.pattern.len() * 4 + 16).sum::<usize>() }
}
