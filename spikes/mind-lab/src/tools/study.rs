//! Scoring tools against the truth, on a stream (`data`).
//!
//! - `schedules`: each observer gets its own copy of each tool, told what it saw, asked every
//!   `data::ASK_EVERY` ticks where each subject is; top-1 and top-3 hits against the truth, per
//!   day (how fast it learns), storage, fuel and time per ask.
//! - `with_maker`: each observer has a belt with a tool-maker. A question about `subject` that no
//!   tool answers is "thought through" (logged with a cost); each night the maker may make a tool,
//!   which answers from then on. Scored the same way.

use super::belt::{self, Belt, Reach, ToolConfig};
use super::cost::Brain;
use super::data::{Ev, Stream, DAY};
use super::lang::*;
use super::maker::{mask, Made};
use super::run::Tool;
use crate::rng::StateHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct Score {
    pub name: String,
    pub n: u64, pub hit1: u64, pub hit3: u64,
    /// Per day: (questions, top-1 hits).
    pub per_day: Vec<(u64, u64)>,
    /// Packed state bytes at the end (mean per observer) and the description's bytes.
    pub bytes: usize, pub desc_bytes: usize,
    pub asks: u64, pub ask_fuel: u64, pub ask_ns: u64,
    pub observe_fuel: u64, pub observes: u64,
    pub bound: i64,
    /// Hash of every answer and the final states (determinism).
    pub hash: u64,
}
impl Score {
    /// Days from `from` on: (questions, top-1 hits).
    pub fn tail(&self, from: usize) -> (u64, u64) { self.per_day.iter().skip(from).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1)) }
    pub fn acc1(&self) -> i64 { if self.n == 0 { 0 } else { (self.hit1 * 1000 / self.n) as i64 } }
    pub fn acc3(&self) -> i64 { if self.n == 0 { 0 } else { (self.hit3 * 1000 / self.n) as i64 } }
    /// Top-1 accuracy (per mille) on day `d`.
    pub fn day_acc(&self, d: usize) -> i64 { self.per_day.get(d).map_or(0, |&(n, h)| if n == 0 { 0 } else { (h * 1000 / n) as i64 }) }
    /// The first day whose accuracy reaches `share` per mille of the last third's accuracy.
    pub fn learnt_by(&self, share: i64) -> usize {
        let k = self.per_day.len();
        let tail: (u64, u64) = self.per_day[k * 2 / 3..].iter().fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        let target = if tail.0 == 0 { 0 } else { (tail.1 * 1000 / tail.0) as i64 * share / 1000 };
        (0..k).find(|&d| self.day_acc(d) >= target).unwrap_or(k)
    }
}

fn record(sc: &mut Score, day: usize, truth: i32, ans: &[(i32, i32)], h: &mut StateHasher) {
    if sc.per_day.len() <= day { sc.per_day.resize(day + 1, (0, 0)); }
    sc.n += 1; sc.per_day[day].0 += 1;
    if ans.first().is_some_and(|a| a.0 == truth) { sc.hit1 += 1; sc.per_day[day].1 += 1; }
    if ans.iter().take(3).any(|a| a.0 == truth) { sc.hit3 += 1; }
    h.write_i32(truth); h.write_u64(ans.len() as u64); for a in ans { h.write_i32(a.0); h.write_i32(a.1); }
}

/// Score each description on a stream. `subject`: only questions about this one (or all).
pub fn schedules(st: &Stream, descs: &[Desc], subject: Option<i32>, observer: Option<usize>, timed: bool) -> Vec<Score> {
    descs.iter().map(|d| {
        let d = Arc::new(d.clone());
        let mut sc = Score { name: d.name.clone(), desc_bytes: d.size(), bound: super::cost::bound(&d, &[]).ask, ..Score::default() };
        let mut tools: Vec<Tool> = st.observers.iter().map(|_| Tool::new(d.clone())).collect();
        let oi = |o: usize| st.observers.iter().position(|&x| x == o).unwrap();
        let mut h = StateHasher::new(0x5c0e);
        for ev in &st.evs {
            match ev {
                Ev::Obs(o, t, r) => { let mut r = *r; belt::stamp(&mut r, *t, DAY); let u = tools[oi(*o)].observe(&r, 100_000).unwrap_or(100_000); sc.observe_fuel += u as u64; sc.observes += 1; }
                Ev::Query(o, t, q, truth) => {
                    if subject.is_some_and(|s| q[F::Who as usize] != s) || observer.is_some_and(|x| x != *o) { continue; }
                    let mut q = *q; belt::stamp(&mut q, *t, DAY);
                    let t0 = timed.then(std::time::Instant::now);
                    let (a, used) = tools[oi(*o)].query("", &q, 100_000, &[]);
                    if let Some(t0) = t0 { sc.ask_ns += t0.elapsed().as_nanos() as u64; }
                    sc.asks += 1; sc.ask_fuel += used as u64;
                    record(&mut sc, (*t / DAY) as usize, *truth, &a.unwrap_or_default(), &mut h);
                }
                Ev::Night(t) => { for x in tools.iter_mut() { let _ = x.forget(*t as i32, 1_000_000); } }
            }
        }
        sc.bytes = tools.iter().map(|t| t.bytes()).sum::<usize>() / tools.len().max(1);
        for t in &tools { h.write_u64(t.state_hash()); }
        sc.hash = h.finish();
        sc
    }).collect()
}

/// What a maker did for one observer.
#[derive(Clone, Debug)]
pub struct MakerRun { pub observer: usize, pub made: Vec<(u32, Made, String, String)>, pub score: Score, pub thought: u64, pub belt_hash: u64, pub belt_bytes: usize }

/// The question "where is `subject`" (a place, answered by sightings, given who and the time).
pub fn where_is(subject: i32) -> belt::Question { (F::Place as i32, kind::SEEN, mask(&[F::Who]), subject) }

/// Each observer with its own belt and tool-maker (`maker_src`), asking where `subject` is.
/// Questions answered by no tool are thought through at `think_cost` link reads each; scored as
/// misses (the comparison with thinking is the village demo's job). Returns one run per observer.
pub fn with_maker(st: &Stream, maker_src: &str, subject: i32, think_cost: i32) -> Vec<MakerRun> {
    st.observers.iter().map(|&o| {
        let mut b = Belt::with_maker(ToolConfig { on: true, ..ToolConfig::default() }, maker_src);
        let mut sc = Score { name: format!("observer {o}"), ..Score::default() };
        let mut h = StateHasher::new(0x5c0f);
        let mut thought = 0;
        let pattern = [subject as u32];
        for ev in &st.evs {
            match ev {
                Ev::Obs(x, t, r) if *x == o => b.tell(*r, *t),
                Ev::Query(x, t, q, truth) if *x == o && q[F::Who as usize] == subject => {
                    // Scored only when a tool answered; otherwise thought through (counted).
                    match b.bound(&pattern).and_then(|bi| b.pick(bi, Reach::calm(Brain::Slow))) {
                        Some(tool) => { let ans = b.ask(tool, *q, *t, Reach::calm(Brain::Slow)).map(|x| x.0).unwrap_or_default(); record(&mut sc, (*t / DAY) as usize, *truth, &ans, &mut h); }
                        None => { b.thought(where_is(subject), &pattern, think_cost, *t); thought += 1; }
                    }
                }
                Ev::Night(t) => { b.sleep(*t); }
                _ => {}
            }
        }
        let made = b.made.iter().map(|(t, m, _, _, n, c)| (*t, *m, n.clone(), c.clone().unwrap_or_default())).collect();
        sc.hash = h.finish();
        MakerRun { observer: o, made, score: sc, thought, belt_hash: b.state_hash(), belt_bytes: b.bytes() }
    }).collect()
}

impl Hash for MakerRun { fn hash<H: Hasher>(&self, h: &mut H) { h.write_u64(self.observer as u64); h.write_u64(self.belt_hash); h.write_u64(self.score.hash); } }
