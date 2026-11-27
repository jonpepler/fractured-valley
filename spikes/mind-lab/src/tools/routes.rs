//! Journeys, and the maps minds make of them (the owner's priority example for the tool-maker).
//!
//! A generated world (hand-written): 30 places on a 6 × 5 lattice, joined to their neighbours
//! except where a hashed fifth of the links are missing (walls). Travellers walk journeys, one
//! link a tick, resting 10 ticks between: **commuters** between three places of their own,
//! **roamers** to any place. Three changes happen to the world:
//! - day `SHORTCUT`: a new link opens (the diagonal that most shortens the commuters' ways);
//! - day `BLOCK`: a link on the first commuter's usual way closes;
//! - days `DOG..DOG+3`: a dog sits at a place on that way. A traveller next to it is surprised
//!   (a `surprise` record at that place) and frightened (stress 900, calming 20 a tick).
//!
//! **Thinking a route through** (no tool, or no answer) is a stand-in, hand-written: breadth-first
//! search over the true links, avoiding where the dog was seen in the last 60 ticks; its cost is
//! the links it examines (as link reads). It is an optimistic oracle: it always finds the best way.
//! Each step thought through is told to the tool-maker as a question ("which place next, given
//! here and the goal"). A traveller with a tool also thinks a journey through anyway with chance
//! 1 in 20 (curiosity, hand-written), which is how a shortcut is ever found.
//!
//! **Stepping**: the traveller tries the step its tool (or thought) gives. A missing link is a
//! blocked step: a `moved` record with `val` −1000 (a prediction error the tool learns from),
//! and it stays put. Every step taken is a `moved` record (place, next, goal).
//!
//! Variants: `Think` (always think), `Kit` (the hand-written `routes` tool, bound by hand),
//! `Maker` (the tool-maker builds a tool in sleep, and its cheap version for when frightened).

use super::belt::{Belt, Reach, ToolConfig};
use super::cost::Brain;
use super::kit;
use super::lang::*;
use super::maker::mask;
use crate::rng::{hash, StateHasher};
use std::hash::Hasher;

pub const GW: i32 = 6;
pub const GH: i32 = 5;
pub const NP: usize = (GW * GH) as usize;
pub const DAY: u32 = 301;
pub const SHORTCUT: u32 = 6;
pub const BLOCK: u32 = 10;
pub const DOG: u32 = 13;
/// "On the way somewhere": the pattern a route tool is bound to (no mind here, so a stand-in node).
pub const ON_THE_WAY: u32 = 1 << 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Variant { Think, Kit, Maker }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Habit { Commuter, Roamer }

#[derive(Clone, Debug)]
pub struct World { pub links: Vec<Vec<usize>>, pub dog: Option<usize>, pub shortcut: (usize, usize), pub blocked: (usize, usize) }

fn xy(p: usize) -> (i32, i32) { (p as i32 % GW, p as i32 / GW) }
fn linked(l: &[Vec<usize>], a: usize, b: usize) -> bool { l[a].contains(&b) }
fn add(l: &mut [Vec<usize>], a: usize, b: usize) { if !linked(l, a, b) { l[a].push(b); l[b].push(a); l[a].sort_unstable(); l[b].sort_unstable(); } }
fn cut(l: &mut [Vec<usize>], a: usize, b: usize) { l[a].retain(|&x| x != b); l[b].retain(|&x| x != a); }

/// Shortest way from `a` to `b` (avoiding `avoid` unless it is the goal): (path, links examined).
pub fn bfs(l: &[Vec<usize>], a: usize, b: usize, avoid: Option<usize>) -> (Option<Vec<usize>>, u32) {
    let mut prev = vec![usize::MAX; NP];
    let mut q = std::collections::VecDeque::new();
    prev[a] = a; q.push_back(a);
    let mut looked = 0;
    while let Some(u) = q.pop_front() {
        if u == b { break; }
        for &v in &l[u] { looked += 1; if prev[v] == usize::MAX && (Some(v) != avoid || v == b) { prev[v] = u; q.push_back(v); } }
    }
    if prev[b] == usize::MAX { return (None, looked); }
    let mut path = vec![b];
    while *path.last().unwrap() != a { let p = prev[*path.last().unwrap()]; path.push(p); }
    path.reverse();
    (Some(path), looked)
}

fn goals(seed: u64, who: usize, habit: Habit) -> Vec<usize> {
    match habit { Habit::Commuter => (0..3).map(|k| (hash(seed, who as u64, k, 0x60a1) % NP as u64) as usize).collect(), Habit::Roamer => (0..NP).collect() }
}

/// The world for a seed, and the commuters' goals (to place the changes on their ways).
pub fn world(seed: u64, commuters: &[Vec<usize>]) -> World {
    let mut links = vec![vec![]; NP];
    for p in 0..NP { let (x, y) = xy(p); if x + 1 < GW { add(&mut links, p, p + 1); } if y + 1 < GH { add(&mut links, p, p + GW as usize); } }
    // Walls: drop a hashed fifth of the links, keeping everything connected.
    let mut all: Vec<(usize, usize)> = (0..NP).flat_map(|a| links[a].iter().filter(move |&&b| b > a).map(move |&b| (a, b))).collect();
    all.sort_by_key(|&(a, b)| hash(seed, a as u64, b as u64, 0x3a11));
    for &(a, b) in all.iter().take(all.len() / 5) { cut(&mut links, a, b); if (0..NP).any(|t| bfs(&links, 0, t, None).0.is_none()) { add(&mut links, a, b); } }
    // The shortcut: the diagonal that most shortens the commuters' ways between their goals.
    let pairs: Vec<(usize, usize)> = commuters.iter().flat_map(|g| (0..g.len()).map(move |i| (g[i], g[(i + 1) % g.len()]))).filter(|p| p.0 != p.1).collect();
    let total = |l: &[Vec<usize>]| pairs.iter().map(|&(a, b)| bfs(l, a, b, None).0.map_or(99, |p| p.len())).sum::<usize>();
    let base = total(&links);
    let mut best = ((0, 0), 0usize);
    for p in 0..NP { let (x, y) = xy(p); for (dx, dy) in [(1, 1), (-1, 1)] { let (nx, ny) = (x + dx, y + dy); if nx < 0 || nx >= GW || ny >= GH { continue; } let q = (ny * GW + nx) as usize;
        let mut l = links.clone(); add(&mut l, p, q); let gain = base.saturating_sub(total(&l)); if gain > best.1 { best = ((p, q), gain); } } }
    // The blocked link: the middle link of the first commuter's first way (after the shortcut), if it can go.
    let mut l = links.clone(); add(&mut l, best.0 .0, best.0 .1);
    let path = pairs.first().and_then(|&(a, b)| bfs(&l, a, b, None).0).unwrap_or_default();
    let mut blocked = (0, 0);
    let n = path.len().saturating_sub(1);
    for k in (0..n).map(|o| (n / 2 + o) % n) {
        let (a, b) = (path[k], path[k + 1]); let mut l2 = l.clone(); cut(&mut l2, a, b);
        if (0..NP).all(|t| bfs(&l2, a, t, None).0.is_some()) { blocked = (a, b); break; }
    }
    World { links, dog: None, shortcut: best.0, blocked }
}

#[derive(Clone, Debug, Default)]
pub struct Day { pub circles: u32, pub journeys: u32, pub steps: u32, pub best: u32, pub bumps: u32, pub thought_steps: u32, pub tool_steps: u32, pub think_cost: u64, pub tool_fuel: u64, pub fell_back: u32, pub used_shortcut: u32, pub could_shortcut: u32 }

#[derive(Clone, Debug)]
pub struct Run { pub variant: Variant, pub habit: Habit, pub days: Vec<Day>, pub made: Vec<(u32, String, i32, i32, Option<String>)>, pub final_tools: Vec<String>, pub hash: u64, pub bytes: usize, pub bound: Vec<(String, i64)> }

/// One traveller of `habit` (person `who`) for `days`, alone in the world, with a variant.
pub fn travel(seed: u64, who: usize, habit: Habit, days: u32, variant: Variant, world0: &World) -> Run {
    let mut w = world0.clone();
    let gs = goals(seed, who, habit);
    let cfg = ToolConfig { on: true, day_len: DAY, ..ToolConfig::default() };
    let mut belt = match variant { Variant::Maker => Belt::with_maker(cfg, kit::MAKER), _ => Belt::new(cfg) };
    if variant == Variant::Kit { let t = belt.add(kit::load(kit::ROUTES)); belt.bind(vec![ON_THE_WAY], vec![t], rec()); }
    let question = (F::Next as i32, kind::MOVED, mask(&[F::Place, F::Goal]), NONE);
    let mut here = gs[0];
    let mut gi = 0usize;
    let mut goal = gs[1 % gs.len()];
    let mut rest = 0u32;
    let mut stress = 0i32;
    let mut dog_seen: Option<u32> = None;
    let mut curious = false;
    let mut path_taken: Vec<usize> = vec![here];
    let mut out = Run { variant, habit, days: vec![], made: vec![], final_tools: vec![], hash: 0, bytes: 0, bound: vec![] };
    let mut day = Day::default();
    let mut h = StateHasher::new(0x6007);
    let mut journey_best = 0u32;
    for t in 0..days * DAY {
        let d = t / DAY;
        if t % DAY == 0 {
            if d == SHORTCUT { let (a, b) = w.shortcut; add(&mut w.links, a, b); }
            if d == BLOCK { let (a, b) = w.blocked; cut(&mut w.links, a, b); }
            w.dog = if (DOG..DOG + 3).contains(&d) { Some(w.blocked.1) } else { None };
        }
        stress = (stress - 20).max(0);
        // The dog, seen from here or next door.
        if let Some(dp) = w.dog { if dp == here || linked(&w.links, here, dp) {
            if dog_seen.is_none_or(|s| t >= s + 30) { belt.tell(rec_of(&[(F::Kind, kind::SURPRISE), (F::Place, dp as i32), (F::Val, 800)]), t); }
            dog_seen = Some(t); stress = 900;
        } }
        if rest > 0 { rest -= 1; } else if here == goal {
            // Arrived: rest, then the next goal.
            day.journeys += 1;
            if w.shortcut.0 != w.shortcut.1 && d >= SHORTCUT && journey_best > 0 { let (a, b) = w.shortcut; let used = path_taken.windows(2).any(|p| (p[0] == a && p[1] == b) || (p[0] == b && p[1] == a)); let could = bfs(&w.links, path_taken[0], goal, None).0.is_some_and(|p| p.windows(2).any(|p| (p[0] == a && p[1] == b) || (p[0] == b && p[1] == a))); if could { day.could_shortcut += 1; if used { day.used_shortcut += 1; } } }
            rest = 10;
            gi += 1;
            goal = match habit { Habit::Commuter => gs[(gi + 1) % gs.len()], Habit::Roamer => (hash(seed, who as u64, gi as u64, 0x60a2) % NP as u64) as usize };
            if goal == here { goal = (goal + 1) % NP; }
            path_taken = vec![here];
            journey_best = bfs(&w.links, here, goal, None).0.map_or(0, |p| p.len() as u32 - 1);
            day.best += journey_best;
            curious = variant != Variant::Think && hash(seed, who as u64, gi as u64, 0xc0de) % 20 == 0;
        } else {
            if journey_best == 0 { journey_best = bfs(&w.links, here, goal, None).0.map_or(0, |p| p.len() as u32 - 1); day.best += journey_best; }
            let avoid = dog_seen.filter(|&s| t < s + 60).and(w.dog);
            let reach = Reach { brain: Brain::Slow, stress, cut: 880 };
            let ctx = rec_of(&[(F::Place, here as i32), (F::Goal, goal as i32)]);
            let mut next = None;
            if !curious {
                // The bound tool, the first of its tools within reach (the full map when calm, the
                // cheap version when frightened).
                if let Some(bi) = belt.bound(&[ON_THE_WAY]) {
                    if let Some(tool) = belt.pick(bi, reach) {
                        if Some(&tool) != belt.binds[bi].tools.first() { day.fell_back += 1; }
                        if let Some((ans, fuel)) = belt.ask(tool, ctx, t, reach) {
                            day.tool_fuel += fuel as u64;
                            next = ans.first().map(|a| a.0 as usize);
                            if next.is_some() { day.tool_steps += 1; }
                        }
                    }
                }
            }
            // Going back where it just came from is noticed (going in circles), and thought through.
            if next.is_some() && path_taken.len() >= 2 && next == Some(path_taken[path_taken.len() - 2]) { day.circles += 1; next = None; }
            if next.is_none() {
                let (p, looked) = bfs(&w.links, here, goal, avoid);
                day.think_cost += looked as u64; day.thought_steps += 1;
                belt.thought(question, &[ON_THE_WAY], looked as i32, t);
                next = p.and_then(|p| p.get(1).copied());
            }
            if let Some(n) = next {
                day.steps += 1;
                h.write_u64(n as u64);
                if linked(&w.links, here, n) {
                    belt.tell(rec_of(&[(F::Kind, kind::MOVED), (F::Place, here as i32), (F::Next, n as i32), (F::Goal, goal as i32)]), t);
                    here = n; path_taken.push(n);
                } else {
                    day.bumps += 1;
                    belt.tell(rec_of(&[(F::Kind, kind::MOVED), (F::Place, here as i32), (F::Next, n as i32), (F::Goal, goal as i32), (F::Val, -1000)]), t);
                }
            }
        }
        if t % DAY == DAY - 1 {
            if let Some(i) = belt.sleep(t) { let m = belt.made.last().unwrap(); out.made.push((d, m.4.clone(), m.1.acc, m.1.bytes, m.5.clone())); let _ = i; }
            out.days.push(std::mem::take(&mut day));
        }
    }
    out.final_tools = belt.tools.iter().map(|t| t.desc.to_string()).collect();
    out.bound = belt.tools.iter().zip(&belt.bounds).map(|(t, b)| (t.desc.name.clone(), b.ask)).collect();
    out.bytes = belt.bytes();
    h.write_u64(belt.state_hash());
    out.hash = h.finish();
    out
}

/// The commuters' goals for a seed (travellers 0 and 1 commute; 2 and 3 roam).
pub fn cast() -> Vec<(usize, Habit)> { vec![(0, Habit::Commuter), (1, Habit::Commuter), (2, Habit::Roamer), (3, Habit::Roamer)] }
pub fn commuter_goals(seed: u64) -> Vec<Vec<usize>> { cast().iter().filter(|c| c.1 == Habit::Commuter).map(|c| goals(seed, c.0, c.1)).collect() }
