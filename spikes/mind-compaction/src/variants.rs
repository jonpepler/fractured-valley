//! Every variant behind one interface, so the harness can replay the same workload through each
//! and compare it with the reference tick by tick.

use crate::decide::scores;
use crate::gen::Tick;
use crate::graph::Graph;
use crate::spread::{self, Activation, Edge, Form, Mods, Params};
use crate::think2::{think_opt, GainMode, Memo, Opts, Select};

pub trait Variant {
    /// One think on a tick whose node ids are already in this variant's numbering.
    fn think(&mut self, t: &Tick) -> Vec<u32>;
    fn act(&self, n: u32) -> i32;
    /// What is on the mind: (node, activation), in this variant's numbering.
    fn active(&self) -> Vec<(u32, i32)>;
    /// The decision rule's candidate places and scores (this variant's ids).
    fn scores(&self, ws: &[u32], m: &Mods) -> Vec<(u32, i32)>;
    /// Bytes of structure per mind (shared parts excluded; see `shared_bytes`).
    fn bytes(&self) -> usize;
    /// Bytes shared by every mind of the species (a base graph, a codebook, thread scratch).
    fn shared_bytes(&self) -> usize { 0 }
    /// Bytes of state kept per mind between thinks.
    fn state_bytes(&self) -> usize;
    fn visits(&self) -> u32;
    fn reset(&mut self);
    /// The renumbering: reference node `i` is `perm[i]` here.
    fn perm(&self) -> Option<&[u32]> { None }
    fn note(&self) -> String { String::new() }
}

#[derive(Clone, Copy)]
pub enum Mode { Ref, Opt(Opts) }

/// A form thought with by the reference think (or a rewritten one), dense activation.
pub struct Std<F: Form> { pub f: F, pub st: Activation, pub p: Params, pub bytes: usize, pub shared: usize, pub perm: Option<Vec<u32>>, pub mode: Mode, memo: Memo }
impl<F: Form> Std<F> {
    pub fn new(f: F, bytes: usize) -> Self { let n = f.len(); Std { f, st: Activation::new(n), p: Params::default(), bytes, shared: 0, perm: None, mode: Mode::Ref, memo: Memo::default() } }
    pub fn mode(mut self, m: Mode) -> Self { self.mode = m; self }
    pub fn perm(mut self, p: Vec<u32>) -> Self { self.perm = Some(p); self }
    pub fn shared(mut self, b: usize) -> Self { self.shared = b; self }
}
impl<F: Form> Variant for Std<F> {
    fn think(&mut self, t: &Tick) -> Vec<u32> {
        if t.damp < 1000 { self.st.damp(t.damp); }
        match self.mode {
            Mode::Ref => spread::think(&self.f, &mut self.st, &t.mods, &t.inject, &self.p),
            Mode::Opt(o) => think_opt(&self.f, &mut self.st, &t.mods, &t.inject, &self.p, o, &mut self.memo, None),
        }
    }
    fn act(&self, n: u32) -> i32 { self.st.act[n as usize] }
    fn active(&self) -> Vec<(u32, i32)> { self.st.active.iter().map(|&n| (n, self.st.act[n as usize])).collect() }
    fn scores(&self, ws: &[u32], m: &Mods) -> Vec<(u32, i32)> { scores(&self.f, &|n| self.st.act[n as usize], ws, m) }
    fn bytes(&self) -> usize { self.bytes }
    fn shared_bytes(&self) -> usize { self.shared }
    fn state_bytes(&self) -> usize { self.st.bytes() }
    fn visits(&self) -> u32 { self.st.visits }
    fn reset(&mut self) { self.st = Activation::new(self.f.len()); self.memo = Memo::default(); }
    fn perm(&self) -> Option<&[u32]> { self.perm.as_deref() }
}

/// Dense activation only while thinking: between thinks a mind keeps just its active list
/// (≤ 256 × 8 bytes). The dense arrays are per-thread scratch, shared by every mind the thread
/// steps. Exact against the reference.
pub struct Swap<F: Form> { pub inner: Std<F>, pub saved: Vec<(u32, i32)> }
impl<F: Form> Variant for Swap<F> {
    fn think(&mut self, t: &Tick) -> Vec<u32> {
        let st = &mut self.inner.st;
        for &(n, a) in &self.saved { st.act[n as usize] = a; st.active.push(n); }
        let ws = self.inner.think(t);
        let st = &mut self.inner.st;
        self.saved = st.active.iter().map(|&n| (n, st.act[n as usize])).collect();
        st.clear();
        ws
    }
    fn act(&self, n: u32) -> i32 { self.saved.binary_search_by_key(&n, |s| s.0).map(|i| self.saved[i].1).unwrap_or(0) }
    fn active(&self) -> Vec<(u32, i32)> { self.saved.clone() }
    fn scores(&self, ws: &[u32], m: &Mods) -> Vec<(u32, i32)> { scores(&self.inner.f, &|n| self.act(n), ws, m) }
    fn bytes(&self) -> usize { self.inner.bytes }
    fn shared_bytes(&self) -> usize { self.inner.shared + self.inner.st.bytes() }
    fn state_bytes(&self) -> usize { self.saved.capacity().max(self.inner.p.active_cap) * 8 }
    fn visits(&self) -> u32 { self.inner.st.visits }
    fn reset(&mut self) { self.inner.reset(); self.saved.clear(); }
    fn perm(&self) -> Option<&[u32]> { self.inner.perm.as_deref() }
}

/// Event-driven: skip the think when the input is the same as at the last think and every
/// modulator is within `tol` of it, and reuse that think's workspace.
pub struct Skip<F: Form> { pub inner: Std<F>, pub tol: i32, last: Option<Tick>, last_ws: Vec<u32>, pub skips: u32, pub thinks: u32 }
impl<F: Form> Skip<F> { pub fn new(inner: Std<F>, tol: i32) -> Self { Skip { inner, tol, last: None, last_ws: vec![], skips: 0, thinks: 0 } } }
fn close(a: &Mods, b: &Mods, tol: i32) -> bool {
    (a.hunger - b.hunger).abs() <= tol && (a.arousal - b.arousal).abs() <= tol && (a.mood - b.mood).abs() <= tol && (a.fatigue - b.fatigue).abs() <= tol && (a.stress - b.stress).abs() <= tol
}
impl<F: Form> Variant for Skip<F> {
    fn think(&mut self, t: &Tick) -> Vec<u32> {
        self.thinks += 1;
        if let Some(l) = &self.last {
            if t.damp == 1000 && l.inject == t.inject && close(&l.mods, &t.mods, self.tol) { self.skips += 1; self.inner.st.visits = 0; return self.last_ws.clone(); }
        }
        let ws = self.inner.think(t);
        self.last = Some(t.clone());
        self.last_ws = ws.clone();
        ws
    }
    fn act(&self, n: u32) -> i32 { self.inner.act(n) }
    fn active(&self) -> Vec<(u32, i32)> { self.inner.active() }
    fn scores(&self, ws: &[u32], m: &Mods) -> Vec<(u32, i32)> { self.inner.scores(ws, m) }
    fn bytes(&self) -> usize { self.inner.bytes }
    fn state_bytes(&self) -> usize { self.inner.state_bytes() }
    fn visits(&self) -> u32 { self.inner.st.visits }
    fn reset(&mut self) { self.inner.reset(); self.last = None; self.last_ws.clear(); self.skips = 0; self.thinks = 0; }
    fn perm(&self) -> Option<&[u32]> { self.inner.perm.as_deref() }
    fn note(&self) -> String { format!("skipped {} of {} thinks", self.skips, self.thinks) }
}

/// Delta input: what was already injected last tick spreads only at the carry share.
pub struct Delta<F: Form> { pub inner: Std<F>, last: Vec<(u32, i32)>, memo: Memo }
impl<F: Form> Delta<F> { pub fn new(inner: Std<F>) -> Self { Delta { inner, last: vec![], memo: Memo::default() } } }
impl<F: Form> Variant for Delta<F> {
    fn think(&mut self, t: &Tick) -> Vec<u32> {
        let cs = self.inner.p.carry_spread;
        let fresh: Vec<i32> = t.inject.iter().map(|&(n, a)| {
            let prev = self.last.iter().filter(|x| x.0 == n).map(|x| x.1).sum::<i32>();
            (a - prev).max(0) + a.min(prev) * cs / 1000
        }).collect();
        self.last = t.inject.clone();
        if t.damp < 1000 { self.inner.st.damp(t.damp); }
        let o = Opts { select: Select::Sort, memo: false, fixed: false, gain: GainMode::Full };
        think_opt(&self.inner.f, &mut self.inner.st, &t.mods, &t.inject, &self.inner.p, o, &mut self.memo, Some(&fresh))
    }
    fn act(&self, n: u32) -> i32 { self.inner.act(n) }
    fn active(&self) -> Vec<(u32, i32)> { self.inner.active() }
    fn scores(&self, ws: &[u32], m: &Mods) -> Vec<(u32, i32)> { self.inner.scores(ws, m) }
    fn bytes(&self) -> usize { self.inner.bytes }
    fn state_bytes(&self) -> usize { self.inner.state_bytes() + self.last.capacity() * 8 }
    fn visits(&self) -> u32 { self.inner.st.visits }
    fn reset(&mut self) { self.inner.reset(); self.last.clear(); }
    fn perm(&self) -> Option<&[u32]> { self.inner.perm.as_deref() }
}

// ---------------------------------------------------------------------------------------------
// Hierarchical spreading over supernodes.

/// The node graph seen through a cluster mask: links into clusters outside it are dropped.
pub struct Masked<'a> { pub g: &'a Graph, pub cluster: &'a [u32], pub allowed: Vec<bool> }
impl Form for Masked<'_> {
    fn len(&self) -> usize { self.g.len() }
    #[inline] fn val(&self, n: u32) -> i32 { self.g.val[n as usize] as i32 }
    #[inline] fn tags(&self, n: u32) -> u8 { self.g.tags[n as usize] }
    #[inline]
    fn edges(&self, u: u32, out: &mut Vec<Edge>) {
        for l in self.g.links(u) {
            let v = self.g.targets[l];
            let w = self.g.slow[l] as i32;
            if w <= 0 || !self.allowed[self.cluster[v as usize] as usize] { continue; }
            out.push(Edge { to: v, w, ty: self.g.ltype[l], val: self.g.lval[l] as i32 });
        }
    }
}

/// Spread first over a graph of clusters (summed link weights between clusters), keep the
/// `keep` most active clusters plus the core, then think at node level only inside them.
pub struct Hier<'a> {
    pub m: Masked<'a>,
    pub st: Activation,
    pub p: Params,
    cg_off: Vec<u32>,
    cg_to: Vec<u32>,
    cg_w: Vec<u32>,
    score: Vec<i64>,
    pub keep: usize,
    pub kept_nodes: u64,
    pub thinks: u64,
}
impl<'a> Hier<'a> {
    pub fn new(g: &'a Graph, cluster: &'a [u32], keep: usize) -> Self {
        let nc = *cluster.iter().max().unwrap_or(&0) as usize + 1;
        let mut pairs: Vec<(u32, u32, u32)> = vec![];
        for u in 0..g.len() as u32 {
            for l in g.links(u) {
                let (cu, cv) = (cluster[u as usize], cluster[g.targets[l] as usize]);
                if cu != cv { pairs.push((cu, cv, g.slow[l] as u32)); }
            }
        }
        pairs.sort_unstable_by_key(|p| (p.0, p.1));
        let (mut off, mut to, mut w) = (vec![0u32; nc + 1], vec![], vec![]);
        let mut last: Option<(u32, u32)> = None;
        for p in pairs {
            if last == Some((p.0, p.1)) { *w.last_mut().unwrap() += p.2; continue; }
            last = Some((p.0, p.1));
            to.push(p.1); w.push(p.2); off[p.0 as usize + 1] += 1;
        }
        for i in 0..nc { off[i + 1] += off[i]; }
        Hier { m: Masked { g, cluster, allowed: vec![false; nc] }, st: Activation::new(g.len()), p: Params::default(), cg_off: off, cg_to: to, cg_w: w, score: vec![0; nc], keep, kept_nodes: 0, thinks: 0 }
    }
}
impl Variant for Hier<'_> {
    fn think(&mut self, t: &Tick) -> Vec<u32> {
        let cl = self.m.cluster;
        let mut touched: Vec<u32> = vec![];
        fn bump(score: &mut [i64], touched: &mut Vec<u32>, c: u32, a: i64) { if score[c as usize] == 0 { touched.push(c); } score[c as usize] += a; }
        for &(n, a) in &t.inject { bump(&mut self.score, &mut touched, cl[n as usize], a as i64); }
        for &n in &self.st.active { bump(&mut self.score, &mut touched, cl[n as usize], self.st.act[n as usize] as i64); }
        // One hop at cluster level, fan-out normalised, from the seeds.
        let seeds: Vec<(u32, i64)> = touched.iter().map(|&c| (c, self.score[c as usize])).collect();
        for (c, s) in seeds {
            let r = self.cg_off[c as usize] as usize..self.cg_off[c as usize + 1] as usize;
            let sum: i64 = self.cg_w[r.clone()].iter().map(|&w| w as i64).sum::<i64>().max(1000);
            for k in r { let add = s * self.cg_w[k] as i64 / sum / 2; if add > 0 { bump(&mut self.score, &mut touched, self.cg_to[k], add); } }
        }
        let mut ranked: Vec<(i64, u32)> = touched.iter().map(|&c| (self.score[c as usize], c)).collect();
        ranked.sort_unstable_by_key(|&(s, c)| (std::cmp::Reverse(s), c));
        for &c in &touched { self.score[c as usize] = 0; }
        self.m.allowed[0] = true;
        for &(_, c) in ranked.iter().take(self.keep) { self.m.allowed[c as usize] = true; }
        self.thinks += 1;
        if t.damp < 1000 { self.st.damp(t.damp); }
        let ws = spread::think(&self.m, &mut self.st, &t.mods, &t.inject, &self.p);
        for &(_, c) in ranked.iter().take(self.keep) { self.m.allowed[c as usize] = false; }
        ws
    }
    fn act(&self, n: u32) -> i32 { self.st.act[n as usize] }
    fn active(&self) -> Vec<(u32, i32)> { self.st.active.iter().map(|&n| (n, self.st.act[n as usize])).collect() }
    fn scores(&self, ws: &[u32], m: &Mods) -> Vec<(u32, i32)> { scores(self.m.g, &|n| self.st.act[n as usize], ws, m) }
    fn bytes(&self) -> usize { self.m.g.bytes() + self.m.cluster.len() * 4 + self.cg_off.len() * 4 + self.cg_to.len() * 8 + self.m.allowed.len() }
    fn state_bytes(&self) -> usize { self.st.bytes() }
    fn visits(&self) -> u32 { self.st.visits }
    fn reset(&mut self) { self.st = Activation::new(self.m.g.len()); }
}
