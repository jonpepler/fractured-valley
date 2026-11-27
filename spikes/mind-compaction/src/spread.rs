//! The reference think: the thought-graph spike's bounded, modulated spreading activation
//! (`spikes/thought-graph`, `spread.rs`, commit `fe89380`), copied without the viewer trace.
//! Integer only; ties broken by node id. Generic over a mind's form, so every compact form runs
//! exactly this think and differs only in what its links decode to.

use crate::graph::{lt, tag, Graph};

/// Global modulators. They are not nodes: they scale how activation crosses links.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods { pub hunger: i32, pub arousal: i32, pub mood: i32, pub fatigue: i32, pub stress: i32 }

/// One out-link as a form reports it.
#[derive(Clone, Copy, Debug)]
pub struct Edge { pub to: u32, pub w: i32, pub ty: u8, pub val: i32 }

/// A form of a mind that can be thought with.
pub trait Form {
    fn len(&self) -> usize;
    fn val(&self, n: u32) -> i32;
    fn tags(&self, n: u32) -> u8;
    fn edges(&self, u: u32, out: &mut Vec<Edge>);
}

impl Form for Graph {
    fn len(&self) -> usize { self.val.len() }
    #[inline] fn val(&self, n: u32) -> i32 { self.val[n as usize] as i32 }
    #[inline] fn tags(&self, n: u32) -> u8 { self.tags[n as usize] }
    #[inline]
    fn edges(&self, u: u32, out: &mut Vec<Edge>) {
        for l in self.links(u) {
            let w = self.slow[l] as i32;
            if w <= 0 { continue; }
            out.push(Edge { to: self.targets[l], w, ty: self.ltype[l], val: self.lval[l] as i32 });
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub hops: usize,
    pub frontier: usize,
    pub hop_decay: i32,
    pub persist: i32,
    pub carry_spread: i32,
    pub min_act: i32,
    pub max_act: i32,
    pub ws_k: usize,
    pub active_cap: usize,
    pub budget: i32,
}
impl Default for Params {
    fn default() -> Self {
        Params { hops: 4, frontier: 48, hop_decay: 550, persist: 500, carry_spread: 300, min_act: 6, max_act: 4000, ws_k: 8, active_cap: 256, budget: 6000 }
    }
}

pub const TYPE_GAIN: [i32; lt::COUNT] = [1000, 1000, 1000, 1000, 700, 600, 1000, 900];

/// How strongly activation crosses an edge, per mille (verbatim from the thought graph).
#[inline]
pub fn gain(m: &Mods, ty: u8, tags_to: u8, val: i32) -> i32 {
    let mut g = TYPE_GAIN[ty as usize] as i64;
    let (a, h, f) = (m.arousal as i64, m.hunger as i64, m.fatigue as i64);
    let neg_mood = (-m.mood).max(0) as i64;
    let pos_mood = m.mood.max(0) as i64;
    let val = val.clamp(-1000, 1000) as i64;
    let s = m.stress as i64;
    if ty == lt::NEAR || ty == lt::LIKE || ty == lt::ASSOC { g = g * (1000 - a / 2) / 1000 * (1000 - s / 2) / 1000; }
    if tags_to & tag::ANGER != 0 { g += g * s * 2 / 1000; }
    if tags_to & tag::FOOD != 0 { g += g * h / 1000; }
    if tags_to & tag::REST != 0 { g += g * f / 1000; }
    if val < 0 {
        g += g * (-val) / 1000 * (a + neg_mood) / 500;
    } else if val > 0 {
        g += g * val / 1000 * pos_mood / 1000;
        g -= g * val / 1000 * neg_mood / 1000;
    }
    g = g * (1000 - f / 3) / 1000;
    g.max(0) as i32
}

/// Activation state and scratch space. Dense (one i32 per node), but only `active` is walked.
#[derive(Clone, Debug)]
pub struct Activation {
    pub act: Vec<i32>,
    pub active: Vec<u32>,
    pub inc: Vec<i32>,
    pub touched: Vec<u32>,
    pub edges: Vec<Edge>,
    pub visits: u32,
    pub hops_run: u32,
}

impl Activation {
    pub fn new(n: usize) -> Self { Activation { act: vec![0; n], active: vec![], inc: vec![0; n], touched: vec![], edges: vec![], visits: 0, hops_run: 0 } }
    /// Scale everything on the mind by a per-mille factor (a surprise interrupting a thought).
    pub fn damp(&mut self, per_mille: i32) {
        for &n in &self.active { let a = &mut self.act[n as usize]; *a = (*a as i64 * per_mille as i64 / 1000) as i32; }
    }
    pub fn clear(&mut self) { for &n in &self.active { self.act[n as usize] = 0; } self.active.clear(); }
    /// Bytes of state per mind: activation and the incoming scratch (dense), plus the active list.
    pub fn bytes(&self) -> usize { self.act.len() * 4 + self.inc.len() * 4 + self.active.capacity() * 4 }
    #[inline]
    pub fn add(&mut self, n: u32, amt: i32, max: i32) {
        let a = &mut self.act[n as usize];
        if *a == 0 && amt > 0 { self.active.push(n); }
        *a = (*a + amt).min(max);
    }
    pub fn top(&self, k: usize) -> Vec<u32> {
        let mut v: Vec<u32> = self.active.iter().copied().filter(|&n| self.act[n as usize] > 0).collect();
        v.sort_by_key(|&n| (std::cmp::Reverse(self.act[n as usize]), n));
        v.truncate(k);
        v
    }
}

/// One think, verbatim from the thought graph (without the trace).
pub fn think<F: Form + ?Sized>(f: &F, st: &mut Activation, m: &Mods, inject: &[(u32, i32)], p: &Params) -> Vec<u32> {
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
    frontier.sort_by_key(|&(n, a)| (std::cmp::Reverse(a), n));
    frontier.truncate(p.frontier);

    let mut edges = std::mem::take(&mut st.edges);
    st.visits = 0;
    st.hops_run = 0;
    for _hop in 0..p.hops {
        if frontier.is_empty() { break; }
        st.hops_run += 1;
        for &(u, src) in &frontier {
            edges.clear();
            f.edges(u, &mut edges);
            st.visits += edges.len() as u32;
            let out_sum: i64 = edges.iter().map(|e| e.w as i64).sum::<i64>().max(1000);
            for e in &edges {
                let tv = f.tags(e.to);
                let g = gain(m, e.ty, tv, f.val(e.to) + e.val);
                let flow = (src as i64 * e.w as i64 / out_sum * g as i64 / 1000 * p.hop_decay as i64 / 1000) as i32;
                if flow < p.min_act { continue; }
                let slot = &mut st.inc[e.to as usize];
                if *slot == 0 { st.touched.push(e.to); }
                *slot += flow;
            }
        }
        let mut next: Vec<(u32, i32)> = Vec::with_capacity(st.touched.len());
        let touched = std::mem::take(&mut st.touched);
        for &v in &touched {
            let amt = st.inc[v as usize];
            st.inc[v as usize] = 0;
            st.add(v, amt, p.max_act);
            next.push((v, amt));
        }
        st.touched = touched; st.touched.clear();
        next.sort_by_key(|&(n, a)| (std::cmp::Reverse(a), n));
        next.truncate(p.frontier);
        frontier = next;
    }
    st.edges = edges;
    finish(st, p)
}

/// The end of a think: the attention budget, the active cap, and the workspace.
pub fn finish(st: &mut Activation, p: &Params) -> Vec<u32> {
    let total: i64 = st.active.iter().map(|&n| st.act[n as usize] as i64).sum();
    if total > p.budget as i64 {
        let mut keep = Vec::with_capacity(st.active.len());
        for &n in &st.active {
            let a = &mut st.act[n as usize];
            *a = (*a as i64 * p.budget as i64 / total) as i32;
            if *a >= p.min_act { keep.push(n) } else { *a = 0 }
        }
        st.active = keep;
    }
    if st.active.len() > p.active_cap {
        let mut v = st.active.clone();
        v.sort_by_key(|&n| (std::cmp::Reverse(st.act[n as usize]), n));
        for &n in &v[p.active_cap..] { st.act[n as usize] = 0; }
        v.truncate(p.active_cap);
        st.active = v;
    }
    st.active.sort_unstable();
    st.top(p.ws_k)
}

impl<T: Form + ?Sized> Form for &T {
    fn len(&self) -> usize { (**self).len() }
    #[inline] fn val(&self, n: u32) -> i32 { (**self).val(n) }
    #[inline] fn tags(&self, n: u32) -> u8 { (**self).tags(n) }
    #[inline] fn edges(&self, u: u32, out: &mut Vec<Edge>) { (**self).edges(u, out) }
}
