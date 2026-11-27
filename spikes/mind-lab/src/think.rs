//! How a thought moves within a tick: bounded spreading activation, modulated by a few global
//! signals. Ported from `spikes/thought-graph` (`spread.rs`), with the compaction lab's
//! recommendations (8.6) made the definition:
//!
//! - **Fixed point is the reference.** Per think, the gain is one multiplier per (link type,
//!   target's gain tags) and sign of felt valence ([`Gain`]); the flow is one multiply-shift, with
//!   one division per sender. Integer only and deterministic by definition.
//! - **Ties are broken by each node's stable key**, never by its array index, so renumbering or
//!   compacting a mind cannot change what it thinks.
//! - **Activation is swap-in state.** [`Activation`] is dense scratch used only while a mind is
//!   thinking (one per thread, shared by every mind the thread steps); between thinks a mind keeps
//!   only its [`Parked`] active list.
//!
//! The old arithmetic (chained divisions) and index ties stay available as ablations
//! (`Params::exact`, `Params::index_ties`), only so the effect of each change can be measured.

use crate::graph::{lt, tag, LinkKey};

/// Global modulators. They are not nodes: they scale how activation crosses links.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    /// 0..1000. Makes food-ward links stronger.
    pub hunger: i32,
    /// 0..1000, fed by surprise and pain. Makes negative paths likelier, damps idle place-to-place drift.
    pub arousal: i32,
    /// -1000..1000, a slow average of the workspace's valence. Low mood favours negative paths.
    pub mood: i32,
    /// 0..1000, read from the mind's consolidation backlog (sleep pressure). Favours rest and
    /// anger; damps all spreading a little.
    pub fatigue: i32,
    /// 0..1000. Shortens thinking (fewer hops, a smaller frontier), damps slow associative links,
    /// and favours anger.
    pub stress: i32,
}

/// One out-link as a form reports it to the think.
#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub to: u32,
    /// Effective weight 1..1000 (slow + fast).
    pub w: i32,
    pub ty: u8,
    /// Link valence.
    pub val: i32,
    pub link: LinkKey,
}

/// A form of a mind that can be thought with.
pub trait Form {
    fn len(&self) -> usize;
    fn val(&self, n: u32) -> i32;
    fn tags(&self, n: u32) -> u8;
    /// The node's stable key (for tie-breaking).
    fn key(&self, n: u32) -> u32;
    /// Links of `u` with a positive effective weight, in link-key order.
    fn edges(&self, u: u32, out: &mut Vec<Edge>);
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Params {
    /// Hops per think.
    pub hops: usize,
    /// At most this many nodes send activation in a hop.
    pub frontier: usize,
    /// Per-mille kept per hop.
    pub hop_decay: i32,
    /// Per-mille of last tick's activation carried into this tick (sitting in a thought).
    pub persist: i32,
    /// Per-mille of the carried thought that spreads again this tick.
    pub carry_spread: i32,
    /// Flows and activations below this are dropped.
    pub min_act: i32,
    pub max_act: i32,
    /// Workspace size (top-k).
    pub ws_k: usize,
    /// At most this many nodes stay active between ticks.
    pub active_cap: usize,
    /// Attention budget: total activation is scaled down to at most this after a think.
    pub budget: i32,
    /// Ablation: the thought graph's old exact arithmetic (chained divisions) instead of the
    /// fixed-point reference.
    pub exact: bool,
    /// Ablation: break ties by array index (the old rule) instead of the stable node key.
    pub index_ties: bool,
}
impl Default for Params {
    fn default() -> Self {
        Params { hops: 4, frontier: 48, hop_decay: 550, persist: 500, carry_spread: 300, min_act: 6, max_act: 4000, ws_k: 8, active_cap: 256, budget: 6000, exact: false, index_ties: false }
    }
}

/// The order of a node among equals: its stable key (then its index, only on a key collision).
#[inline]
pub fn tie<F: Form + ?Sized>(f: &F, p: &Params, n: u32) -> u64 {
    if p.index_ties { n as u64 } else { ((f.key(n) as u64) << 32) | n as u64 }
}

/// Base gain per link type, per mille.
pub const TYPE_GAIN: [i32; lt::COUNT] = [1000, 1000, 1000, 1000, 700, 600, 1000, 900];

/// The target's tags that matter to the gain: food, rest, anger.
#[inline]
pub fn tagclass(t: u8) -> usize { ((t & tag::FOOD) | (t & tag::REST) | ((t & tag::ANGER) >> 5)) as usize }

/// The gain, defined per think in fixed point (the compaction lab's two-stage table). For each
/// (link type, target tag class): the type and tag stage once per think (`zero`, `pre`); then the
/// felt valence either through the exact stage (`get`, the old definition) or as one fixed-point
/// multiplier (`fixed`, the reference).
pub struct Gain { pre: [i64; lt::COUNT * 8], zero: [i64; lt::COUNT * 8], an: i64, neg: i64, pos: i64, fk: i64, cneg: i64, cpos: i64 }
impl Gain {
    pub fn new(m: &Mods) -> Gain {
        let (a, h, f, s) = (m.arousal as i64, m.hunger as i64, m.fatigue as i64, m.stress as i64);
        let neg = (-m.mood).max(0) as i64;
        let pos = m.mood.max(0) as i64;
        let mut g = Gain { pre: [0; lt::COUNT * 8], zero: [0; lt::COUNT * 8], an: a + neg, neg, pos, fk: 1000 - f / 3,
            cneg: (a + neg) * (1 << 20) / 500_000, cpos: (pos - neg) * (1 << 20) / 1_000_000 };
        for ty in 0..lt::COUNT {
            for tc in 0..8usize {
                let mut x = TYPE_GAIN[ty] as i64;
                let t = ty as u8;
                if t == lt::NEAR || t == lt::LIKE || t == lt::ASSOC { x = x * (1000 - a / 2) / 1000 * (1000 - s / 2) / 1000; }
                // Stress favours anger; so does fatigue (a tired mind is irritable).
                if tc & 4 != 0 { x += x * s * 2 / 1000 + x * f / 1000; }
                if tc & 1 != 0 { x += x * h / 1000; }
                if tc & 2 != 0 { x += x * f / 1000; }
                g.pre[ty * 8 + tc] = x;
                g.zero[ty * 8 + tc] = (x * g.fk / 1000).max(0);
            }
        }
        g
    }
    /// The old exact gain (the thought graph's `spread::gain`, stage by stage).
    #[inline]
    pub fn exact(&self, k: usize, val: i32) -> i64 {
        if val == 0 { return self.zero[k]; }
        let mut g = self.pre[k];
        let val = val.clamp(-1000, 1000) as i64;
        if val < 0 { g += g * (-val) / 1000 * self.an / 500; } else { g += g * val / 1000 * self.pos / 1000; g -= g * val / 1000 * self.neg / 1000; }
        (g * self.fk / 1000).max(0)
    }
    /// The reference: a negative felt valence boosts by (arousal + low mood), a positive one by
    /// (good mood − low mood), as one multiplier `zero × (2^20 + |val| × c) >> 20`.
    #[inline]
    pub fn fixed(&self, k: usize, val: i32) -> i64 {
        let v = val.clamp(-1000, 1000) as i64;
        let c = if v < 0 { -v * self.cneg } else { v * self.cpos };
        ((self.zero[k] * ((1 << 20) + c)) >> 20).max(0)
    }
    #[inline]
    pub fn get(&self, k: usize, val: i32, exact: bool) -> i64 { if exact { self.exact(k, val) } else { self.fixed(k, val) } }
}

/// The gain of one link under modulators (for inspection; the think builds the table once).
pub fn gain(m: &Mods, ty: u8, tags_to: u8, val: i32, exact: bool) -> i32 { Gain::new(m).get(ty as usize * 8 + tagclass(tags_to), val, exact) as i32 }

/// What one hop did, for the viewer.
#[derive(Clone, Debug, Default)]
pub struct Hop {
    /// The nodes that sent, with what they sent.
    pub frontier: Vec<(u32, i32)>,
    /// (from, to, link, amount), largest first, capped.
    pub flows: Vec<(u32, u32, LinkKey, i32)>,
}

/// What a mind keeps between thinks: its active nodes and their activation, sorted by node
/// (at most `active_cap` of them). Everything dense is scratch.
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Parked { pub act: Vec<(u32, i32)> }
impl Parked {
    pub fn get(&self, n: u32) -> i32 { self.act.binary_search_by_key(&n, |a| a.0).map(|i| self.act[i].1).unwrap_or(0) }
    pub fn clear(&mut self) { self.act.clear(); }
    pub fn bytes(&self) -> usize { self.act.len() * 8 }
}

/// Dense activation and scratch, used only while a mind thinks. `act` is valid for the mind
/// attached to it; `active` lists the nodes that may be non-zero.
#[derive(Clone, Debug, Default)]
pub struct Activation {
    pub act: Vec<i32>,
    pub active: Vec<u32>,
    inc: Vec<i32>,
    touched: Vec<u32>,
    edges: Vec<Edge>,
    /// Links visited and hops run in the last think.
    pub visits: u32,
    pub hops_run: u32,
}

thread_local! {
    static SCRATCH: std::cell::RefCell<Activation> = std::cell::RefCell::new(Activation::default());
}

impl Activation {
    pub fn new(n: usize) -> Self { Activation { act: vec![0; n], inc: vec![0; n], ..Default::default() } }
    /// Take this thread's scratch, sized for `n` nodes, with a parked state loaded into it.
    pub fn attach(parked: &Parked, n: usize) -> Activation {
        let mut a = SCRATCH.with(|s| std::mem::take(&mut *s.borrow_mut()));
        if a.act.len() < n { a.act.resize(n, 0); a.inc.resize(n, 0); }
        debug_assert!(a.active.is_empty());
        for &(n, v) in &parked.act { a.act[n as usize] = v; a.active.push(n); }
        a
    }
    /// Park the active list and give the scratch back to the thread, all zero again.
    pub fn detach(mut self) -> Parked {
        self.active.sort_unstable();
        let p = Parked { act: self.active.iter().filter(|&&n| self.act[n as usize] != 0).map(|&n| (n, self.act[n as usize])).collect() };
        self.clear();
        self.visits = 0; self.hops_run = 0;
        SCRATCH.with(|s| *s.borrow_mut() = self);
        p
    }
    /// Scale everything on the mind by a per-mille factor (a surprise interrupting a thought).
    pub fn damp(&mut self, per_mille: i32) {
        for &n in &self.active { let a = &mut self.act[n as usize]; *a = (*a as i64 * per_mille as i64 / 1000) as i32; }
    }
    pub fn clear(&mut self) { for &n in &self.active { self.act[n as usize] = 0; } self.active.clear(); }
    /// Dense scratch bytes (per thread, not per mind).
    pub fn bytes(&self) -> usize { self.act.len() * 4 + self.inc.len() * 4 }
    fn add(&mut self, n: u32, amt: i32, max: i32) {
        let a = &mut self.act[n as usize];
        if *a == 0 && amt > 0 { self.active.push(n); }
        *a = (*a + amt).min(max);
    }
    /// The top `k` active nodes, by activation then stable key.
    pub fn top<F: Form + ?Sized>(&self, f: &F, p: &Params, k: usize) -> Vec<u32> {
        let mut v: Vec<u32> = self.active.iter().copied().filter(|&n| self.act[n as usize] > 0).collect();
        v.sort_by_key(|&n| (std::cmp::Reverse(self.act[n as usize]), tie(f, p, n)));
        v.truncate(k);
        v
    }
}

/// One think: decay what was on the mind, add this tick's input, spread for `hops` hops through
/// a capped frontier, and return the workspace (the top-k). If `trace` is given, each hop's
/// frontier and largest flows are recorded.
pub fn think<F: Form + ?Sized>(f: &F, st: &mut Activation, m: &Mods, inject: &[(u32, i32)], p: &Params, mut trace: Option<&mut Vec<Hop>>) -> Vec<u32> {
    let table = Gain::new(m);
    let order = |st: &Activation, v: &mut Vec<(u32, i32)>| { let _ = st; v.sort_by_key(|&(n, a)| (std::cmp::Reverse(a), tie(f, p, n))); v.truncate(p.frontier); };
    // Carry over (and forget) last tick's thought.
    let mut keep = Vec::with_capacity(st.active.len());
    for &n in &st.active {
        let a = &mut st.act[n as usize];
        *a = (*a as i64 * p.persist as i64 / 1000) as i32;
        if *a >= p.min_act { keep.push(n) } else { *a = 0 }
    }
    st.active = keep;
    // Hop 0 sends this tick's input in full and a share of the carried thought.
    for i in 0..st.active.len() {
        let n = st.active[i];
        let c = (st.act[n as usize] as i64 * p.carry_spread as i64 / 1000) as i32;
        if c > 0 { st.inc[n as usize] = c; st.touched.push(n); }
    }
    for &(n, a) in inject {
        st.add(n, a, p.max_act);
        if st.inc[n as usize] == 0 { st.touched.push(n); }
        st.inc[n as usize] += a;
    }
    let mut frontier: Vec<(u32, i32)> = st.touched.iter().map(|&n| (n, st.inc[n as usize])).collect();
    for i in 0..st.touched.len() { let n = st.touched[i]; st.inc[n as usize] = 0; }
    st.touched.clear();
    order(st, &mut frontier);

    let mut edges = std::mem::take(&mut st.edges);
    let dk = p.hop_decay as i64;
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
            let kf = ((src as i64) << 24) / out_sum;
            for e in &edges {
                let tv = f.tags(e.to);
                let g = table.get(e.ty as usize * 8 + tagclass(tv), f.val(e.to) + e.val, p.exact);
                let flow = if p.exact { src as i64 * e.w as i64 / out_sum * g / 1000 * dk / 1000 } else { ((e.w as i64 * kf) >> 24) * g * dk / 1_000_000 } as i32;
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
        order(st, &mut next);
        if let Some(t) = trace.as_deref_mut() {
            flows.sort_by_key(|&(u, v, _, a)| (std::cmp::Reverse(a), tie(f, p, u), tie(f, p, v)));
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
        v.sort_by_key(|&n| (std::cmp::Reverse(st.act[n as usize]), tie(f, p, n)));
        for &n in &v[p.active_cap..] { st.act[n as usize] = 0; }
        v.truncate(p.active_cap);
        st.active = v;
    }
    st.active.sort_unstable();
    st.top(f, p, p.ws_k)
}
