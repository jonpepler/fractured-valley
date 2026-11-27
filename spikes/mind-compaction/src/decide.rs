//! The fixed decision rule every variant is scored on: the fox's walk choice, from
//! `spikes/thought-graph` `mind.rs` (`felt` and the walk part of `choose`), trimmed of the world.
//!
//! Candidates are the places in the workspace and the places that anchored things in it are at.
//! A place scores *on my mind × how it feels*, where the feeling is a one-hop look-ahead over its
//! links scaled by the modulators (a somatic marker). The best place above a bar is the action;
//! otherwise the mind stays. Each variant decides with its own form, so a lossy form's lost
//! link valence or merged weights show up in the action.

use crate::graph::tag;
use crate::spread::{Edge, Form, Mods};

pub const STAY: u32 = u32::MAX;
pub const WALK_AT: i32 = 60;

pub fn felt<F: Form + ?Sized>(f: &F, m: &Mods, p: u32, edges: &mut Vec<Edge>) -> i32 {
    let neg_mood = (-m.mood).max(0) as i64;
    let (a, h, fa) = (m.arousal as i64, m.hunger as i64, m.fatigue as i64);
    let good = |val: i64, tags: u8| -> i64 {
        if tags & tag::FOOD != 0 { val * 2 * h / 1000 } else if tags & tag::REST != 0 { val * 2 * fa / 1000 } else { val * (300 + a) / 1000 }
    };
    edges.clear();
    f.edges(p, edges);
    let own = f.val(p) as i64;
    let mut v = if own < 0 { own * (1000 + 2 * a + neg_mood) / 1000 } else { good(own, f.tags(p)) };
    for e in edges.iter() {
        let val = (f.val(e.to) + e.val).clamp(-1000, 1000) as i64;
        if f.tags(e.to) & tag::PLACE != 0 { v += e.w as i64 * val / 2000; continue; }
        v += if val < 0 { e.w as i64 * val / 1000 * (1000 + 2 * a + neg_mood) / 1000 } else { e.w as i64 * good(val, f.tags(e.to)) / 1000 };
    }
    v as i32
}

fn place_of<F: Form + ?Sized>(f: &F, n: u32, edges: &mut Vec<Edge>) -> Option<u32> {
    edges.clear();
    f.edges(n, edges);
    edges.iter().filter(|e| f.tags(e.to) & tag::PLACE != 0).max_by_key(|e| (e.w, std::cmp::Reverse(e.to))).map(|e| e.to)
}

/// Every candidate place (in the form's ids) with its score.
pub fn scores<F: Form + ?Sized>(f: &F, act: &dyn Fn(u32) -> i32, ws: &[u32], m: &Mods) -> Vec<(u32, i32)> {
    let mut edges = vec![];
    let mut cands: Vec<u32> = vec![];
    for &n in ws {
        let t = f.tags(n);
        if t & tag::PLACE != 0 { cands.push(n) } else if t & tag::ANCHORED != 0 { if let Some(p) = place_of(f, n, &mut edges) { cands.push(p) } }
    }
    cands.sort_unstable();
    cands.dedup();
    cands.into_iter().map(|p| (p, score_of(f, act, ws, m, p, &mut edges))).collect()
}

/// One place's score: on my mind (the place, plus attractive things in the workspace that are
/// at it) × how it feels.
pub fn score_of<F: Form + ?Sized>(f: &F, act: &dyn Fn(u32) -> i32, ws: &[u32], m: &Mods, p: u32, edges: &mut Vec<Edge>) -> i32 {
    let mut a = act(p);
    for &n in ws {
        if n != p && f.tags(n) & tag::ANCHORED != 0 && f.tags(n) & tag::PLACE == 0 && f.val(n) > 0 && place_of(f, n, edges) == Some(p) { a += act(n); }
    }
    (a as i64 * felt(f, m, p, edges).max(0) as i64 / 1000) as i32
}

/// The best-scoring place at or above the bar (ties to the lower id), or `STAY`.
pub fn pick(scores: &[(u32, i32)]) -> u32 {
    match scores.iter().max_by_key(|&&(p, s)| (s, std::cmp::Reverse(p))) { Some(&(p, s)) if s >= WALK_AT => p, _ => STAY }
}

/// The chosen place (in the form's ids), or `STAY`.
pub fn decide<F: Form + ?Sized>(f: &F, act: &dyn Fn(u32) -> i32, ws: &[u32], m: &Mods) -> u32 { pick(&scores(f, act, ws, m)) }
