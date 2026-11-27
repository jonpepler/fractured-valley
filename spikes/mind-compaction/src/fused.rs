//! A fused think: the reference think specialised to one layout, exact against it.
//!
//! - No edge list: each sender's links are read straight from the CSR arrays, twice (once for
//!   the out-sum, once to send), instead of being copied into a `Vec<Edge>`.
//! - One node record (`val << 16 | tags`, 4 bytes): reading a target's valence and tags is one
//!   memory access, not two arrays.
//! - The gain of a link whose felt valence is 0 (most of them) depends only on the link type and
//!   the target's gain tags, so it comes from a 64-entry table built at the start of each think.
//!   Others run only the valence stage of the gain on top of a per-think table. Exact.
//! - The frontier is selected with `select_nth_unstable`, as in `think2`.
//! - `approx`: the gain as one fixed-point multiplier per (type, tags) and sign of valence, built
//!   per think, and the flow as one multiply-shift. Integer and deterministic, not bit-exact.

use crate::decide::scores;
use crate::gen::Tick;
use crate::graph::Graph;
use crate::spread::{Activation, Edge, Form, Mods, Params};
use crate::think2::{tagclass, Gain};
use crate::variants::Variant;
use std::cmp::Reverse;

pub struct Fused { pub offsets: Vec<u32>, pub targets: Vec<u32>, pub slow: Vec<i16>, pub lval: Vec<i16>, pub ltype: Vec<u8>, pub node: Vec<u32>, pub anchors: usize }
impl Fused {
    pub fn compile(g: &Graph) -> Fused {
        Fused { offsets: g.offsets.clone(), targets: g.targets.clone(), slow: g.slow.clone(), lval: g.lval.clone(), ltype: g.ltype.clone(),
            node: (0..g.len()).map(|i| ((g.val[i] as u16 as u32) << 16) | g.tags[i] as u32).collect(), anchors: g.anchors.len() }
    }
    pub fn bytes(&self) -> usize { self.offsets.len() * 4 + self.targets.len() * 9 + self.node.len() * 4 + self.anchors * std::mem::size_of::<crate::graph::Anchor>() }
}
impl Form for Fused {
    fn len(&self) -> usize { self.node.len() }
    #[inline] fn val(&self, n: u32) -> i32 { (self.node[n as usize] >> 16) as u16 as i16 as i32 }
    #[inline] fn tags(&self, n: u32) -> u8 { self.node[n as usize] as u8 }
    fn edges(&self, u: u32, out: &mut Vec<Edge>) {
        for l in self.offsets[u as usize] as usize..self.offsets[u as usize + 1] as usize {
            let w = self.slow[l] as i32;
            if w > 0 { out.push(Edge { to: self.targets[l], w, ty: self.ltype[l], val: self.lval[l] as i32 }); }
        }
    }
}


fn top(v: &mut Vec<(u32, i32)>, k: usize) {
    let key = |&(n, a): &(u32, i32)| (Reverse(a), n);
    if v.len() > k { v.select_nth_unstable_by_key(k - 1, key); v.truncate(k); }
    v.sort_unstable_by_key(key);
}

pub fn think(f: &Fused, st: &mut Activation, m: &Mods, inject: &[(u32, i32)], p: &Params, approx: bool) -> Vec<u32> {
    let table = Gain::new(m);
    let mut keep = Vec::with_capacity(st.active.len());
    for &n in &st.active {
        let a = &mut st.act[n as usize];
        *a = (*a as i64 * p.persist as i64 / 1000) as i32;
        if *a >= p.min_act { keep.push(n) } else { *a = 0 }
    }
    st.active = keep;
    for &n in &st.active {
        let c = (st.act[n as usize] as i64 * p.carry_spread as i64 / 1000) as i32;
        if c > 0 { st.inc[n as usize] = c; st.touched.push(n); }
    }
    for &(n, a) in inject {
        st.add(n, a, p.max_act);
        if st.inc[n as usize] == 0 { st.touched.push(n); }
        st.inc[n as usize] += a;
    }
    let mut frontier: Vec<(u32, i32)> = st.touched.iter().map(|&n| (n, st.inc[n as usize])).collect();
    for &n in &st.touched { st.inc[n as usize] = 0; }
    st.touched.clear();
    top(&mut frontier, p.frontier);
    let mut next: Vec<(u32, i32)> = vec![];
    let dk = p.hop_decay as i64;
    st.visits = 0;
    st.hops_run = 0;
    for _hop in 0..p.hops {
        if frontier.is_empty() { break; }
        st.hops_run += 1;
        for &(u, src) in &frontier {
            let r = f.offsets[u as usize] as usize..f.offsets[u as usize + 1] as usize;
            let mut out_sum = 0i64;
            for &w in &f.slow[r.clone()] { if w > 0 { out_sum += w as i64; st.visits += 1; } }
            let out_sum = out_sum.max(1000);
            let kf = ((src as i64) << 24) / out_sum;
            for l in r {
                let w = f.slow[l] as i64;
                if w <= 0 { continue; }
                let to = f.targets[l];
                let nd = f.node[to as usize];
                let val = (nd >> 16) as u16 as i16 as i32 + f.lval[l] as i32;
                let ty = f.ltype[l];
                let k = ty as usize * 8 + tagclass(nd as u8);
                let g = if approx { table.approx(k, val) } else { table.get(k, val) };
                let flow = if approx { ((w * kf) >> 24) * g * dk / 1_000_000 } else { src as i64 * w / out_sum * g / 1000 * dk / 1000 } as i32;
                if flow < p.min_act { continue; }
                let slot = &mut st.inc[to as usize];
                if *slot == 0 { st.touched.push(to); }
                *slot += flow;
            }
        }
        next.clear();
        let touched = std::mem::take(&mut st.touched);
        for &v in &touched {
            let amt = st.inc[v as usize];
            st.inc[v as usize] = 0;
            st.add(v, amt, p.max_act);
            next.push((v, amt));
        }
        st.touched = touched; st.touched.clear();
        top(&mut next, p.frontier);
        std::mem::swap(&mut frontier, &mut next);
    }
    finish_fast(st, p)
}

/// The reference `finish`, with selection instead of full sorts (exact): the active cap keeps
/// the same 256 nodes, and the workspace is the same top 8.
pub fn finish_fast(st: &mut Activation, p: &Params) -> Vec<u32> {
    let total: i64 = st.active.iter().map(|&n| st.act[n as usize] as i64).sum();
    if total > p.budget as i64 {
        let act = &mut st.act;
        st.active.retain(|&n| {
            let a = &mut act[n as usize];
            *a = (*a as i64 * p.budget as i64 / total) as i32;
            if *a >= p.min_act { true } else { *a = 0; false }
        });
    }
    let act = &st.act;
    let key = |&n: &u32| (Reverse(act[n as usize]), n);
    if st.active.len() > p.active_cap {
        st.active.select_nth_unstable_by_key(p.active_cap - 1, key);
        for &n in &st.active[p.active_cap..] { st.act[n as usize] = 0; }
        st.active.truncate(p.active_cap);
    }
    st.active.sort_unstable();
    let act = &st.act;
    let key = |&n: &u32| (Reverse(act[n as usize]), n);
    let mut ws: Vec<u32> = st.active.iter().copied().filter(|&n| act[n as usize] > 0).collect();
    if ws.len() > p.ws_k { ws.select_nth_unstable_by_key(p.ws_k - 1, key); ws.truncate(p.ws_k); }
    ws.sort_unstable_by_key(key);
    ws
}

pub struct FusedMind { pub f: Fused, pub st: Activation, pub p: Params, pub approx: bool }
impl FusedMind { pub fn new(g: &Graph, approx: bool) -> Self { FusedMind { f: Fused::compile(g), st: Activation::new(g.len()), p: Params::default(), approx } } }
impl Variant for FusedMind {
    fn think(&mut self, t: &Tick) -> Vec<u32> {
        if t.damp < 1000 { self.st.damp(t.damp); }
        think(&self.f, &mut self.st, &t.mods, &t.inject, &self.p, self.approx)
    }
    fn act(&self, n: u32) -> i32 { self.st.act[n as usize] }
    fn active(&self) -> Vec<(u32, i32)> { self.st.active.iter().map(|&n| (n, self.st.act[n as usize])).collect() }
    fn scores(&self, ws: &[u32], m: &Mods) -> Vec<(u32, i32)> { scores(&self.f, &|n| self.st.act[n as usize], ws, m) }
    fn bytes(&self) -> usize { self.f.bytes() }
    fn state_bytes(&self) -> usize { self.st.bytes() }
    fn visits(&self) -> u32 { self.st.visits }
    fn reset(&mut self) { self.st = Activation::new(self.f.len()); }
}
