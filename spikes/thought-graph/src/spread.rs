//! How a thought moves within a tick: bounded spreading activation, modulated by a few global
//! signals. Integer only; ties broken by node id. Generic over the mind's form, so the packed
//! graph and the compiled bit-vector form run exactly the same think.

use crate::graph::{lt, tag, Graph};

/// Global modulators. They are not nodes: they scale how activation crosses links.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    /// 0..1000. Makes food-ward links stronger.
    pub hunger: i32,
    /// 0..1000, fed by surprise and pain. Makes negative paths likelier, damps idle place-to-place drift.
    pub arousal: i32,
    /// -1000..1000, a slow average of the workspace's valence. Low mood favours negative paths.
    pub mood: i32,
    /// 0..1000, read from the mind's consolidation backlog (sleep pressure, not a timer). Favours
    /// rest and anger; damps all spreading a little.
    pub fatigue: i32,
    /// 0..1000, separate from mood. Shortens thinking (the mind sets fewer hops and a smaller
    /// frontier from it), damps slow associative links, and favours anger.
    pub stress: i32,
}

/// One out-link as a form reports it.
#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub to: u32,
    /// Effective weight 0..1000 (slow + fast in the graph form; decoded in the compiled form).
    pub w: i32,
    pub ty: u8,
    /// Link valence (the compiled form doesn't keep it: always 0 there).
    pub val: i32,
    /// Link index in the graph form, for the viewer; `u32::MAX` if there isn't one.
    pub link: u32,
}

/// A form of a mind that can be thought with.
pub trait Form {
    fn len(&self) -> usize;
    fn val(&self, n: u32) -> i32;
    fn tags(&self, n: u32) -> u8;
    fn edges(&self, u: u32, out: &mut Vec<Edge>);
}

/// The graph form, read live: slow weights plus today's fast weights.
pub struct Live<'a> { pub g: &'a Graph, pub fast: &'a [i16] }
impl Form for Live<'_> {
    fn len(&self) -> usize { self.g.len() }
    fn val(&self, n: u32) -> i32 { self.g.val[n as usize] as i32 }
    fn tags(&self, n: u32) -> u8 { self.g.tags[n as usize] }
    fn edges(&self, u: u32, out: &mut Vec<Edge>) {
        for l in self.g.links(u) {
            let w = (self.g.slow[l] as i32 + self.fast[l] as i32).min(1000);
            if w <= 0 { continue; }
            out.push(Edge { to: self.g.targets[l], w, ty: self.g.ltype[l], val: self.g.lval[l] as i32, link: l as u32 });
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Params {
    /// Hops per think.
    pub hops: usize,
    /// At most this many nodes send activation in a hop.
    pub frontier: usize,
    /// Per-mille kept per hop.
    pub hop_decay: i32,
    /// Per-mille of last tick's activation carried into this tick (sitting in a thought).
    pub persist: i32,
    /// Per-mille of the carried thought that spreads again this tick. New input always spreads in
    /// full. Keeps the loop gain below one, so a thought nothing feeds fades.
    pub carry_spread: i32,
    /// Flows and activations below this are dropped.
    pub min_act: i32,
    pub max_act: i32,
    /// Workspace size (top-k).
    pub ws_k: usize,
    /// At most this many nodes stay active between ticks.
    pub active_cap: usize,
    /// Attention budget: after a think, total activation is scaled down to at most this
    /// (global inhibition), so loops can't run away and a strong input crowds others out.
    pub budget: i32,
}
impl Default for Params {
    fn default() -> Self {
        Params { hops: 4, frontier: 48, hop_decay: 550, persist: 500, carry_spread: 300, min_act: 6, max_act: 4000, ws_k: 8, active_cap: 256, budget: 6000 }
    }
}

/// Base gain per link type, per mille.
pub const TYPE_GAIN: [i32; lt::COUNT] = [1000, 1000, 1000, 1000, 700, 600, 1000, 900];

/// How strongly activation crosses an edge, given the modulators: per mille.
/// `val` is the felt valence of arriving: the target's valence plus the link's.
pub fn gain(m: &Mods, ty: u8, tags_to: u8, val: i32) -> i32 {
    let mut g = TYPE_GAIN[ty as usize] as i64;
    let (a, h, f) = (m.arousal as i64, m.hunger as i64, m.fatigue as i64);
    let neg_mood = (-m.mood).max(0) as i64;
    let pos_mood = m.mood.max(0) as i64;
    let val = val.clamp(-1000, 1000) as i64;
    let s = m.stress as i64;
    if ty == lt::NEAR || ty == lt::LIKE || ty == lt::ASSOC { g = g * (1000 - a / 2) / 1000 * (1000 - s / 2) / 1000; }
    // Stress favours anger; so does fatigue (a tired mind is irritable).
    if tags_to & tag::ANGER != 0 { g += g * s * 2 / 1000 + g * f / 1000; }
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

/// What one hop did, for the viewer.
#[derive(Clone, Debug, Default)]
pub struct Hop {
    /// The nodes that sent, with what they sent.
    pub frontier: Vec<(u32, i32)>,
    /// (from, to, link index or u32::MAX, amount), largest first, capped.
    pub flows: Vec<(u32, u32, u32, i32)>,
}

/// Activation state and scratch space. Activation is dense (one i32 per node) but only the
/// `active` list is ever walked, so a tick costs the same in a 1k or a 100k mind.
#[derive(Clone, Debug)]
pub struct Activation {
    pub act: Vec<i32>,
    pub active: Vec<u32>,
    inc: Vec<i32>,
    touched: Vec<u32>,
    edges: Vec<Edge>,
    /// Links visited and hops run in the last think (how deep the thought went).
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
    pub fn bytes(&self) -> usize { self.act.len() * 4 + self.inc.len() * 4 + self.active.capacity() * 4 }
    fn add(&mut self, n: u32, amt: i32, max: i32) {
        let a = &mut self.act[n as usize];
        if *a == 0 && amt > 0 { self.active.push(n); }
        *a = (*a + amt).min(max);
    }
    /// The top `k` active nodes, by activation then id.
    pub fn top(&self, k: usize) -> Vec<u32> {
        let mut v: Vec<u32> = self.active.iter().copied().filter(|&n| self.act[n as usize] > 0).collect();
        v.sort_by_key(|&n| (std::cmp::Reverse(self.act[n as usize]), n));
        v.truncate(k);
        v
    }
}

/// One think: decay what was on the mind, add this tick's input, spread for `hops` hops through
/// a capped frontier, and return the workspace (the top-k). If `trace` is given, each hop's
/// frontier and largest flows are recorded.
pub fn think<F: Form>(f: &F, st: &mut Activation, m: &Mods, inject: &[(u32, i32)], p: &Params, mut trace: Option<&mut Vec<Hop>>) -> Vec<u32> {
    // Carry over (and forget) last tick's thought.
    let mut keep = Vec::with_capacity(st.active.len());
    for &n in &st.active {
        let a = &mut st.act[n as usize];
        *a = (*a as i64 * p.persist as i64 / 1000) as i32;
        if *a >= p.min_act { keep.push(n) } else { *a = 0 }
    }
    st.active = keep;
    // Hop 0 sends this tick's input in full and a share of the carried thought.
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
        let mut flows = vec![];
        for &(u, src) in &frontier {
            edges.clear();
            f.edges(u, &mut edges);
            st.visits += edges.len() as u32;
            // Fan-out: a node splits what it sends across its links (no gain from many links).
            let out_sum: i64 = edges.iter().map(|e| e.w as i64).sum::<i64>().max(1000);
            for e in &edges {
                let tv = f.tags(e.to);
                let g = gain(m, e.ty, tv, f.val(e.to) + e.val);
                let flow = (src as i64 * e.w as i64 / out_sum * g as i64 / 1000 * p.hop_decay as i64 / 1000) as i32;
                if flow < p.min_act { continue; }
                let slot = &mut st.inc[e.to as usize];
                if *slot == 0 { st.touched.push(e.to); }
                *slot += flow;
                if trace.is_some() { flows.push((u, e.to, e.link, flow)); }
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
        if let Some(t) = trace.as_deref_mut() {
            flows.sort_by_key(|&(u, v, _, a)| (std::cmp::Reverse(a), u, v));
            flows.truncate(40);
            t.push(Hop { frontier: frontier.clone(), flows });
        }
        frontier = next;
    }
    st.edges = edges;

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
