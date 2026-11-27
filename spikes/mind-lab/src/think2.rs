//! The thinking step, version 2 (round 2, stage 2; decision 9.2's direction "B"). The definition
//! is `SPEC-think.md` in this crate; this file is that spec as CPU code, and `gpu/` runs the same
//! step as a WGSL compute shader, bit for bit.
//!
//! What changes from `think::think` (v1), all on purpose:
//! - **Gain from knots.** Per think, each of the 80 gain classes (link type × the target's
//!   food / rest / anger tags) gets three factors, at felt valence −1000, 0 and +1000, with the
//!   hop decay folded in. Between knots the factor is the straight line. All in 32-bit integers.
//! - **Saturating 32-bit arithmetic** with stated caps (`A_CAP`, `INC_CAP`, `F_CAP`): nothing wraps.
//! - **Links stored heaviest first, with each node's weight sum.** A sender stops at the first link
//!   whose flow cannot reach `min_act` even at the largest factor its link types can have: exact by
//!   construction, because the factor is piecewise linear between knots and the links that follow
//!   are no heavier.
//! - **Storage that makes the stop pay:** the species base is kept sorted once
//!   ([`SortedBase`], shared); a mind's own differences (overlay reweights, link valences, fast
//!   weights, added links) are compiled into per-node patches ([`Compiled`]), recompiled only for
//!   the nodes that changed.
//!
//! Persisting, carrying, injecting, frontier selection, the budget, the cap and the workspace keep
//! v1's meaning; the spec pins down every rounding and tie.

use crate::graph::{link_key, link_pos, lt, LinkKey};
use crate::think::{tagclass, Activation, Edge, Form, Hop, Mods, Params, TYPE_GAIN};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Gain classes: link type × target tag class.
pub const CLASSES: usize = lt::COUNT * 8;
/// Largest sender amount, persisted activation and injection.
pub const A_CAP: i32 = 16_383;
/// Largest sum of flows into one node in one hop.
pub const INC_CAP: i32 = 1 << 30;
/// Largest per-link factor (gain × decay × focus × courage, in thousandths).
pub const F_CAP: i32 = 262_143;
/// "No patch" in `Compiled::patch_of`.
pub const NONE: u32 = u32::MAX;

/// Measurement only: with this off, senders read every link (no early stop). Results are
/// identical either way (tests check it); the switch exists to measure what the stop saves.
pub static PRUNE: AtomicBool = AtomicBool::new(true);

/// One link as v2 stores it: target, weight (1..1000), gain class, link valence, and its position
/// among the source's links (its `LinkKey`, for traces only).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CEdge { pub to: u32, pub w: u16, pub class: u8, pub lval: i16, pub pos: u16 }
impl CEdge {
    /// The GPU's second word: weight (10 bits), class (7 bits), link valence + 1024 (11 bits).
    #[inline] pub fn packed(&self) -> u32 { self.w as u32 | (self.class as u32) << 10 | ((self.lval as i32 + 1024) as u32) << 17 }
}

/// Sort a node's links heaviest first (then by position), and give their sum and type mask.
fn finish(e: &mut [CEdge]) -> (u32, u16) {
    e.sort_unstable_by_key(|x| (std::cmp::Reverse(x.w), x.pos));
    let sum: u32 = e.iter().map(|x| x.w as u32).sum();
    let mask = e.iter().fold(0u16, |m, x| m | 1 << (x.class / 8));
    (sum.max(1000), mask)
}

/// A node's links in v2's form.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Patch { pub e: Vec<CEdge>, pub den: u32, pub tmask: u16 }
impl Patch {
    pub fn from_edges<F: Form + ?Sized>(f: &F, edges: &[Edge]) -> Patch {
        let mut e: Vec<CEdge> = edges.iter().filter(|x| x.w > 0).map(|x| CEdge { to: x.to, w: x.w as u16, class: (x.ty as usize * 8 + tagclass(f.tags(x.to))) as u8, lval: x.val.clamp(-1000, 1000) as i16, pos: link_pos(x.link) as u16 }).collect();
        let (den, tmask) = finish(&mut e);
        Patch { e, den, tmask }
    }
    pub fn bytes(&self) -> usize { self.e.len() * 8 + 8 }
}

/// A species base in v2's form, built once and shared by every mind of the species: each node's
/// links with a positive weight, heaviest first; the weight sum (at least 1000); the link types
/// present.
#[derive(Debug, Default)]
pub struct SortedBase { pub off: Vec<u32>, pub e: Vec<CEdge>, pub den: Vec<u32>, pub tmask: Vec<u16>, pub keys: Vec<u32>, pub uid: u64 }
static UID: AtomicU64 = AtomicU64::new(1);
pub fn next_uid() -> u64 { UID.fetch_add(1, Ordering::Relaxed) }
impl SortedBase {
    pub fn build(b: &crate::store::Base) -> SortedBase {
        let n = b.len();
        let mut sb = SortedBase { off: vec![0], uid: next_uid(), keys: b.keys.clone(), ..Default::default() };
        for u in 0..n as u32 {
            let mut e = vec![];
            b.decode(u, |k, l| if l.w > 0 { e.push(CEdge { to: l.to, w: l.w as u16, class: (l.ty as usize * 8 + tagclass(b.tags[l.to as usize])) as u8, lval: l.lval, pos: k as u16 }) });
            let (den, tmask) = finish(&mut e);
            sb.e.extend(e); sb.off.push(sb.e.len() as u32); sb.den.push(den); sb.tmask.push(tmask);
        }
        sb
    }
    pub fn len(&self) -> usize { self.den.len() }
    pub fn is_empty(&self) -> bool { self.den.is_empty() }
    #[inline] pub fn node(&self, u: u32) -> (&[CEdge], u32, u16) { let u = u as usize; (&self.e[self.off[u] as usize..self.off[u + 1] as usize], self.den[u], self.tmask[u]) }
    /// Bytes as the GPU holds it (8 per link, 16 per node: offset, sum, mask, key).
    pub fn bytes(&self) -> usize { self.e.len() * 8 + self.len() * 16 }
}

/// One mind's overlay compiled in: for every node whose links differ from the base (an overlay
/// reweight or link valence, a fast weight, an added link) or that the mind added, its effective
/// links as a [`Patch`]. Kept up to date node by node: the store marks a node dirty when one of its
/// links changes, and the next think recompiles only those.
#[derive(Clone, Debug, Default)]
pub struct Inner {
    pub ready: bool,
    pub uid: u64,
    /// Bumped by every change that can alter a think (links, node valence, added nodes).
    pub version: u64,
    pub patch_of: Vec<u32>,
    pub patches: Vec<Patch>,
    pub free: Vec<u32>,
    pub dirty: Vec<u32>,
    pub is_dirty: Vec<bool>,
    /// Measures: nodes recompiled, and refreshes.
    pub recompiled: u64,
    pub refreshes: u64,
}
impl Inner {
    pub fn set(&mut self, u: u32, p: Option<Patch>) {
        let u = u as usize;
        if self.patch_of.len() <= u { self.patch_of.resize(u + 1, NONE); }
        match (self.patch_of[u], p) {
            (NONE, None) => {}
            (i, None) => { self.patches[i as usize] = Patch::default(); self.free.push(i); self.patch_of[u] = NONE; }
            (NONE, Some(p)) => { let i = match self.free.pop() { Some(i) => { self.patches[i as usize] = p; i } None => { self.patches.push(p); self.patches.len() as u32 - 1 } }; self.patch_of[u] = i; }
            (i, Some(p)) => self.patches[i as usize] = p,
        }
    }
    pub fn patched(&self) -> usize { self.patch_of.iter().filter(|&&i| i != NONE).count() }
    pub fn bytes(&self) -> usize { self.patch_of.len() * 4 + self.patches.iter().filter(|p| !p.e.is_empty() || p.den != 0).map(|p| p.bytes()).sum::<usize>() }
}

/// The per-mind cache, behind a lock so a think through `&Form` can refresh it.
#[derive(Debug, Default)]
pub struct Compiled(pub std::sync::Mutex<Inner>);
impl Clone for Compiled {
    fn clone(&self) -> Self { let mut i = self.0.lock().unwrap().clone(); i.uid = next_uid(); Compiled(std::sync::Mutex::new(i)) }
}
impl Compiled {
    pub fn new() -> Compiled { Compiled(std::sync::Mutex::new(Inner { uid: next_uid(), ..Default::default() })) }
    /// Node `u`'s links changed.
    #[inline] pub fn touch(&mut self, u: u32) {
        let c = self.0.get_mut().unwrap();
        c.version += 1;
        if !c.ready { return; }
        let i = u as usize;
        if c.is_dirty.len() <= i { c.is_dirty.resize(i + 1, false); }
        if !c.is_dirty[i] { c.is_dirty[i] = true; c.dirty.push(u); }
    }
    /// Something else a think reads changed (a node's valence, a new node).
    #[inline] pub fn bump(&mut self) { self.0.get_mut().unwrap().version += 1; }
}

/// What a think reads links through: the shared sorted base and the mind's patches.
pub struct View<'a> { pub base: &'a SortedBase, pub c: std::sync::MutexGuard<'a, Inner> }
impl View<'_> {
    #[inline]
    pub fn node(&self, u: u32) -> (&[CEdge], u32, u16) {
        match self.c.patch_of.get(u as usize).copied().unwrap_or(NONE) {
            NONE => if (u as usize) < self.base.len() { self.base.node(u) } else { (&[], 1000, 0) },
            i => { let p = &self.c.patches[i as usize]; (&p.e, p.den, p.tmask) }
        }
    }
}

/// The 80 classes' factors at felt valence −1000, 0, +1000 (thousandths, hop decay folded in),
/// and per link type the largest of them. `SPEC-think.md` §3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Knots { pub f: [[i32; 3]; CLASSES], pub bt: [i32; lt::COUNT] }
impl Knots {
    pub fn new(m: &Mods, hop_decay: i32) -> Knots {
        let hd = hop_decay.clamp(0, 1000);
        let a = m.arousal.clamp(0, 1000);
        let h = m.hunger.max(m.hunger2).clamp(0, 1000);
        let fa = m.fatigue.clamp(0, 1000);
        let s = m.stress.clamp(0, 1000);
        let mood = m.mood.clamp(-1000, 1000);
        let (neg, pos) = ((-mood).max(0), mood.max(0));
        let fk = 1000 - fa / 3;
        let mult = [1000 + 2 * (a + neg), 1000, 1000 + pos - neg];
        let mut k = Knots { f: [[0; 3]; CLASSES], bt: [0; lt::COUNT] };
        for ty in 0..lt::COUNT {
            for tc in 0..8usize {
                let mut x = TYPE_GAIN[ty];
                let t = ty as u8;
                if t == lt::NEAR || t == lt::LIKE || t == lt::ASSOC || t == lt::SOURCE || t == lt::TOLD { x = x * (1000 - a / 2) / 1000 * (1000 - s / 2) / 1000; }
                if tc & 4 != 0 { x += x * s * 2 / 1000 + x * fa / 1000; }
                if tc & 1 != 0 { x += x * h / 1000; }
                if tc & 2 != 0 { x += x * fa / 1000; }
                let g0 = (x * fk / 1000).max(0);
                for j in 0..3 { k.f[ty * 8 + tc][j] = (g0 * mult[j] / 1000 * hd / 1000).min(F_CAP); }
                k.bt[ty] = k.bt[ty].max(k.f[ty * 8 + tc].iter().copied().max().unwrap());
            }
        }
        k
    }
    /// The factor of a link of class `c` into a target of felt valence `v` (node valence + link
    /// valence, clamped to ±1000): the straight line between knots, truncated towards zero.
    #[inline(always)]
    pub fn factor(&self, c: usize, v: i32) -> i32 {
        let f = &self.f[c];
        if v < 0 { f[1] + (f[0] - f[1]) * (-v) / 1000 } else { f[1] + (f[2] - f[1]) * v / 1000 }
    }
}

/// Focus and courage multipliers (thousandths) for one think, and their sets.
pub struct Boost { pub fg: i32, pub cg: i32, pub focus: Vec<u32>, pub courage: Vec<u32> }
impl Boost {
    pub fn new<F: Form + ?Sized>(f: &F, m: &Mods) -> Boost {
        let mut edges = vec![];
        let focus: Vec<u32> = if m.focus > 0 && (m.intent as usize) < f.len() {
            f.edges(m.intent, &mut edges);
            let mut v: Vec<u32> = edges.iter().map(|e| e.to).collect(); v.push(m.intent); v.sort_unstable(); v.dedup(); v
        } else { vec![] };
        edges.clear();
        let courage: Vec<u32> = if m.courage > 0 && (m.ally as usize) < f.len() {
            f.edges(m.ally, &mut edges);
            let mut v: Vec<u32> = edges.iter().map(|e| e.to).collect(); v.sort_unstable(); v.dedup(); v
        } else { vec![] };
        Boost { fg: if focus.is_empty() { 1000 } else { 1000 + m.focus.clamp(0, 1000) }, cg: if courage.is_empty() { 1000 } else { 1000 + m.courage.clamp(0, 1000) }, focus, courage }
    }
    /// The largest factor a sender with link types `tmask` can give (§4).
    #[inline] pub fn bound(&self, k: &Knots, tmask: u16) -> u32 {
        let mut b = 0; let mut m = tmask;
        while m != 0 { let t = m.trailing_zeros() as usize; m &= m - 1; b = b.max(k.bt[t]); }
        (b * self.fg / 1000 * self.cg / 1000).min(F_CAP) as u32
    }
    /// The factor into `to` after focus and courage, before the `F_CAP` clamp.
    #[inline] pub fn apply(&self, f: i32, to: u32) -> i32 {
        let mut f = f;
        if self.fg != 1000 && self.focus.binary_search(&to).is_ok() { f = f * self.fg / 1000; }
        if self.cg != 1000 && self.courage.binary_search(&to).is_ok() { f = f * self.cg / 1000; }
        f
    }
}

/// Saturation and work counters (measures; also compared with the GPU's flags).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub thinks: u64,
    /// Links whose flow was computed, and links skipped by the early stop.
    pub evaluated: u64, pub stopped: u64,
    /// Flows that passed `min_act` (on a GPU, one atomic add each), senders expanded, nodes touched.
    pub flows: u64, pub senders: u64, pub touched: u64,
    /// Times a cap bound: an injection or persisted value above `A_CAP`, a frontier amount above
    /// `A_CAP`, a hop's sum above `INC_CAP`, a factor above `F_CAP`.
    pub sat_inject: u64, pub sat_front: u64, pub sat_inc: u64, pub sat_factor: u64,
    /// Active nodes before the budget and cap (summed), and the largest seen; candidates per hop.
    pub active_sum: u64, pub active_max: u64, pub cand_sum: u64, pub cand_max: u64, pub hops: u64,
    /// Nanoseconds bringing the compiled overlay up to date (when `TIME_THINK` is on).
    pub refresh_ns: u64,
}
thread_local! { pub static STATS: std::cell::Cell<Stats> = const { std::cell::Cell::new(Stats { thinks: 0, evaluated: 0, stopped: 0, flows: 0, senders: 0, touched: 0, sat_inject: 0, sat_front: 0, sat_inc: 0, sat_factor: 0, active_sum: 0, active_max: 0, cand_sum: 0, cand_max: 0, hops: 0, refresh_ns: 0 }) }; }
pub fn take_stats() -> Stats { STATS.with(|c| c.replace(Stats::default())) }

/// Saturation flags of one think (bit 0 inject/persist, 1 frontier, 2 hop sum, 3 factor).
pub const SAT_INJECT: u32 = 1;
pub const SAT_FRONT: u32 = 2;
pub const SAT_INC: u32 = 4;
pub const SAT_FACTOR: u32 = 8;

/// The order of nodes among equals: larger amount first, then smaller stable key, then smaller id.
#[inline] fn rank_key<F: Form + ?Sized>(f: &F, n: u32, a: i32) -> (std::cmp::Reverse<i32>, u32, u32) { (std::cmp::Reverse(a), f.key(n), n) }

/// Keep the `k` best of `v` by `rank_key` (order of the kept ones unspecified).
fn keep_top<F: Form + ?Sized>(f: &F, v: &mut Vec<(u32, i32)>, k: usize) {
    if v.len() > k {
        if k == 0 { v.clear(); return; }
        v.select_nth_unstable_by_key(k - 1, |&(n, a)| rank_key(f, n, a));
        v.truncate(k);
    }
}

/// What one think did, beyond the state it leaves in `Activation`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Out { pub ws: Vec<u32>, pub visits: u32, pub hops_run: u32, pub flags: u32 }

/// One think, v2 (`SPEC-think.md`). Same contract as `think::think`: `st` holds the mind's
/// activation (dense, with its active list) before and after; returns the workspace.
pub fn think<F: Form + ?Sized>(f: &F, st: &mut Activation, m: &Mods, inject: &[(u32, i32)], p: &Params, trace: Option<&mut Vec<Hop>>) -> Vec<u32> {
    let timed = crate::think::TIME_THINK.load(Ordering::Relaxed);
    let t0 = if timed { Some(std::time::Instant::now()) } else { None };
    let rec = RECORDING.with(|r| r.get());
    let state_in: Vec<(u32, i32)> = if rec { let mut a: Vec<u32> = st.active.clone(); a.sort_unstable(); a.dedup(); a.into_iter().map(|n| (n, st.act[n as usize])).filter(|x| x.1 != 0).collect() } else { vec![] };
    let view = f.sorted_view();
    if let Some(t0) = t0 { let dt = t0.elapsed().as_nanos() as u64; STATS.with(|c| { let mut s = c.get(); s.refresh_ns += dt; c.set(s); }); }
    let out = think_in(f, view.as_ref(), st, m, inject, p, trace);
    if rec {
        if let Some(v) = &view { record(f, v, &state_in, m, inject, p, st, &out); }
    }
    drop(view);
    if let Some(t0) = t0 {
        let dt = t0.elapsed().as_nanos() as u64;
        crate::think::THINK_NS.with(|c| { let (a, b) = c.get(); c.set((a + dt, b + 1)); });
    }
    out.ws
}

/// The step itself, through `view` when the form has one (else each sender's links are sorted
/// on the spot from `Form::edges`, which gives the same result).
pub fn think_in<F: Form + ?Sized>(f: &F, view: Option<&View>, st: &mut Activation, m: &Mods, inject: &[(u32, i32)], p: &Params, mut trace: Option<&mut Vec<Hop>>) -> Out {
    assert!(p.min_act >= 1 && p.min_act <= p.max_act && (0..=1000).contains(&p.persist) && (0..=1000).contains(&p.carry_spread) && (0..=1000).contains(&p.hop_decay) && p.max_act <= A_CAP && (0..=131_071).contains(&p.budget), "params outside v2's ranges");
    let n = f.len();
    st.ensure(n);
    let k = Knots::new(m, p.hop_decay);
    let boost = Boost::new(f, m);
    let prune = PRUNE.load(Ordering::Relaxed);
    let mut s = STATS.with(|c| c.get());
    s.thinks += 1;
    let mut flags = 0u32;
    let min = p.min_act;
    let lim = 1000 * min as u32;
    // 1. Persist (state in: each active node once).
    st.active.sort_unstable(); st.active.dedup();
    let mut keep = Vec::with_capacity(st.active.len());
    for &u in &st.active {
        let a = &mut st.act[u as usize];
        if *a > p.max_act { flags |= SAT_INJECT; }
        let x = (*a).clamp(0, p.max_act) * p.persist / 1000;
        if x >= min { *a = x; keep.push(u); } else { *a = 0; }
    }
    st.active = keep;
    // 2. Seeds: the carried share, then the input.
    st.touched.clear();
    for i in 0..st.active.len() {
        let u = st.active[i];
        let c = st.act[u as usize] * p.carry_spread / 1000;
        if c > 0 { st.inc[u as usize] = c; st.touched.push(u); }
    }
    for &(u, a) in inject {
        assert!((u as usize) < n, "inject outside the mind");
        if a > A_CAP { flags |= SAT_INJECT; }
        let a = a.clamp(0, A_CAP);
        let x = &mut st.act[u as usize];
        if *x == 0 && a > 0 { st.active.push(u); }
        *x = (*x + a).min(p.max_act);
        if a > 0 {
            let inc = &mut st.inc[u as usize];
            if *inc == 0 { st.touched.push(u); }
            *inc = (*inc + a).min(INC_CAP);
        }
    }
    let mut frontier: Vec<(u32, i32)> = Vec::with_capacity(st.touched.len());
    for &u in &st.touched {
        let v = std::mem::take(&mut st.inc[u as usize]);
        if v > A_CAP { flags |= SAT_FRONT; }
        frontier.push((u, v.min(A_CAP)));
    }
    st.touched.clear();
    keep_top(f, &mut frontier, p.frontier);
    // 3. Hops.
    let mut visits = 0u32;
    let mut hops_run = 0u32;
    let mut buf: Vec<Edge> = vec![];
    let mut local: Patch;
    for _ in 0..p.hops {
        if frontier.is_empty() { break; }
        hops_run += 1;
        s.hops += 1;
        let mut flows = vec![];
        for &(u, a) in &frontier {
            s.senders += 1;
            let (edges, den, tmask) = match view {
                Some(v) => v.node(u),
                None => { buf.clear(); f.edges(u, &mut buf); local = Patch::from_edges(f, &buf); (&local.e[..], local.den, local.tmask) }
            };
            visits += edges.len() as u32;
            let b = boost.bound(&k, tmask);
            let a = a as u32;
            for (i, e) in edges.iter().enumerate() {
                let q = a * e.w as u32 / den;
                if prune && q * b < lim { s.stopped += (edges.len() - i) as u64; break; }
                s.evaluated += 1;
                let v = (f.val(e.to) + e.lval as i32).clamp(-1000, 1000);
                let raw = k.factor(e.class as usize, v);
                let fac = boost.apply(raw, e.to);
                if fac > F_CAP { flags |= SAT_FACTOR; }
                let fac = fac.min(F_CAP);
                let fl = (q * fac as u32 / 1000) as i32;
                if fl < min { continue; }
                s.flows += 1;
                let slot = &mut st.inc[e.to as usize];
                if *slot == 0 { st.touched.push(e.to); }
                if *slot + fl > INC_CAP { flags |= SAT_INC; }
                *slot = (*slot + fl).min(INC_CAP);
                if trace.is_some() { flows.push((u, e.to, link_key(u, e.pos as usize), fl)); }
            }
        }
        let mut next: Vec<(u32, i32)> = Vec::with_capacity(st.touched.len());
        s.touched += st.touched.len() as u64;
        for &v in &st.touched {
            let raw = std::mem::take(&mut st.inc[v as usize]);
            let x = &mut st.act[v as usize];
            if *x == 0 { st.active.push(v); }
            *x = (*x + raw).min(p.max_act);
            if raw > A_CAP { flags |= SAT_FRONT; }
            next.push((v, raw.min(A_CAP)));
        }
        st.touched.clear();
        s.cand_sum += next.len() as u64; s.cand_max = s.cand_max.max(next.len() as u64);
        keep_top(f, &mut next, p.frontier);
        if let Some(t) = trace.as_deref_mut() {
            let mut fr = frontier.clone(); fr.sort_by_key(|&(n, a)| rank_key(f, n, a));
            flows.sort_by_key(|&(u, v, _, a): &(u32, u32, LinkKey, i32)| (std::cmp::Reverse(a), f.key(u), u, f.key(v), v));
            flows.truncate(40);
            t.push(Hop { frontier: fr, flows });
        }
        frontier = next;
    }
    // 4. Budget, cap, workspace.
    s.active_sum += st.active.len() as u64; s.active_max = s.active_max.max(st.active.len() as u64);
    let total: i64 = st.active.iter().map(|&u| st.act[u as usize] as i64).sum();
    if total > p.budget as i64 {
        let mut keep = Vec::with_capacity(st.active.len());
        for &u in &st.active {
            let a = &mut st.act[u as usize];
            *a = (*a as i64 * p.budget as i64 / total) as i32;
            if *a >= min { keep.push(u) } else { *a = 0 }
        }
        st.active = keep;
    }
    if st.active.len() > p.active_cap {
        let mut v: Vec<(u32, i32)> = st.active.iter().map(|&u| (u, st.act[u as usize])).collect();
        v.sort_unstable_by_key(|&(u, a)| rank_key(f, u, a));
        for &(u, _) in &v[p.active_cap..] { st.act[u as usize] = 0; }
        st.active = v[..p.active_cap].iter().map(|x| x.0).collect();
    }
    st.active.sort_unstable();
    let mut ws: Vec<(u32, i32)> = st.active.iter().map(|&u| (u, st.act[u as usize])).filter(|x| x.1 > 0).collect();
    keep_top(f, &mut ws, p.ws_k);
    ws.sort_unstable_by_key(|&(u, a)| rank_key(f, u, a));
    st.visits = visits; st.hops_run = hops_run;
    if flags & SAT_INJECT != 0 { s.sat_inject += 1; }
    if flags & SAT_FRONT != 0 { s.sat_front += 1; }
    if flags & SAT_INC != 0 { s.sat_inc += 1; }
    if flags & SAT_FACTOR != 0 { s.sat_factor += 1; }
    STATS.with(|c| c.set(s));
    Out { ws: ws.into_iter().map(|x| x.0).collect(), visits, hops_run, flags }
}

// ------------------------------------------------------------------------------------- GPU form
//
// The flat u32 layouts the GPU shader reads (`SPEC-think.md` §7). Built here, without any GPU
// dependency, so the CPU side owns the format.

/// A species base for the GPU: `[n, nedges, off(n+1), edges(2 each: to, packed), den(n), tmask(n), key(n)]`.
pub fn pack_base(sb: &SortedBase) -> Vec<u32> {
    let n = sb.len();
    let mut v = Vec::with_capacity(2 + (n + 1) + sb.e.len() * 2 + n * 3);
    v.push(n as u32); v.push(sb.e.len() as u32);
    v.extend_from_slice(&sb.off);
    for e in &sb.e { v.push(e.to); v.push(e.packed()); }
    v.extend_from_slice(&sb.den);
    v.extend(sb.tmask.iter().map(|&t| t as u32));
    v.extend_from_slice(&sb.keys);
    v
}

/// One mind for the GPU: `[n, nbase, npatch, npedges, patch_of(n), pdesc(4 each: off, len, den, tmask),
/// pedges(2 each), val(n, two i16 a word), key of own nodes(n − nbase)]`.
pub fn pack_mind<F: Form + ?Sized>(f: &F, v: &View) -> Vec<u32> {
    let n = f.len();
    let nbase = v.base.len();
    let mut patch_of = vec![NONE; n];
    let mut desc = vec![];
    let mut edges = vec![];
    let mut np = 0u32;
    for u in 0..n {
        let i = v.c.patch_of.get(u).copied().unwrap_or(NONE);
        if i == NONE { continue; }
        let p = &v.c.patches[i as usize];
        patch_of[u] = np; np += 1;
        desc.extend_from_slice(&[edges.len() as u32 / 2, p.e.len() as u32, p.den, p.tmask as u32]);
        for e in &p.e { edges.push(e.to); edges.push(e.packed()); }
    }
    let mut out = Vec::with_capacity(4 + n + desc.len() + edges.len() + n / 2 + 1 + n.saturating_sub(nbase));
    out.extend_from_slice(&[n as u32, nbase as u32, np, edges.len() as u32 / 2]);
    out.extend_from_slice(&patch_of);
    out.extend_from_slice(&desc);
    out.extend_from_slice(&edges);
    for i in (0..n).step_by(2) { let a = f.val(i as u32) as i16 as u16 as u32; let b = if i + 1 < n { f.val(i as u32 + 1) as i16 as u16 as u32 } else { 0 }; out.push(a | b << 16); }
    for u in nbase..n { out.push(f.key(u as u32)); }
    out
}

/// Words of a think's fixed header in the GPU input (`SPEC-think.md` §7).
pub const IN_HEAD: usize = 28;
/// Output words for a think with these params.
pub fn out_words(p: &Params) -> usize { 6 + p.ws_k + 2 * p.active_cap }

/// One think's input for the GPU: the header (mods, params, where its mind, base, lists and
/// output are) followed by its lists.
#[allow(clippy::too_many_arguments)]
pub fn pack_input(out: &mut Vec<u32>, mind_off: u32, base_off: u32, out_off: u32, m: &Mods, p: &Params, state: &[(u32, i32)], inject: &[(u32, i32)]) {
    let at = out.len() as u32;
    let lists = at + IN_HEAD as u32;
    out.extend_from_slice(&[mind_off, base_off, out_off,
        m.hunger as u32, m.hunger2 as u32, m.arousal as u32, m.mood as u32, m.fatigue as u32, m.stress as u32, m.focus as u32, m.intent, m.courage as u32, m.ally,
        p.hops as u32, p.frontier as u32, p.hop_decay as u32, p.persist as u32, p.carry_spread as u32, p.min_act as u32, p.max_act as u32, p.ws_k as u32, p.active_cap as u32, p.budget as u32,
        lists, state.len() as u32, lists + 2 * state.len() as u32, inject.len() as u32, 0]);
    for &(n, a) in state { out.push(n); out.push(a as u32); }
    for &(n, a) in inject { out.push(n); out.push(a as u32); }
}

/// Read one think's output back: (out, state as (node, activation), sorted by node).
pub fn unpack_output(o: &[u32]) -> (Out, Vec<(u32, i32)>) {
    let (flags, visits, hops_run, nws) = (o[0], o[1], o[2], o[3] as usize);
    let ns = o[4] as usize;
    let ws = o[6..6 + nws].to_vec();
    let base = 6 + o[5] as usize;
    let st = (0..ns).map(|i| (o[base + 2 * i], o[base + 2 * i + 1] as i32)).collect();
    (Out { ws, visits, hops_run, flags }, st)
}

// ------------------------------------------------------------------------------------- recording
//
// For checking the GPU over long runs: every v2 think on this thread, with its inputs, outputs
// and a snapshot of its mind whenever the mind changed, handed in batches to a sink.

thread_local! {
    static RECORDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static RECORDER: std::cell::RefCell<Option<Recorder>> = const { std::cell::RefCell::new(None) };
}

/// A mind as the GPU needs it, at one version.
#[derive(Clone, Debug)]
pub struct Snap { pub id: u64, pub uid: u64, pub base: u64, pub words: std::sync::Arc<Vec<u32>> }
/// One recorded think.
#[derive(Clone, Debug)]
pub struct Rec { pub snap: u64, pub uid: u64, pub base: u64, pub mods: Mods, pub params: Params, pub state_in: Vec<(u32, i32)>, pub inject: Vec<(u32, i32)>, pub out: Out, pub state_out: Vec<(u32, i32)>, pub n: usize }
/// A batch: the thinks, the new snapshots they need (older ones were in earlier batches), and new
/// bases (packed).
pub struct Batch { pub recs: Vec<Rec>, pub snaps: Vec<Snap>, pub bases: Vec<(u64, std::sync::Arc<Vec<u32>>)> }
pub struct Recorder {
    pub sink: Box<dyn FnMut(Batch)>,
    pub every: usize,
    pub recs: Vec<Rec>, pub snaps: Vec<Snap>, pub bases: Vec<(u64, std::sync::Arc<Vec<u32>>)>,
    last: std::collections::HashMap<u64, (u64, u64)>,
    seen_bases: std::collections::HashSet<u64>,
    next_snap: u64,
    pub snap_words: u64,
}
/// Start recording this thread's v2 thinks; `sink` gets a batch every `every` thinks.
pub fn record_start(every: usize, sink: Box<dyn FnMut(Batch)>) {
    RECORDER.with(|r| *r.borrow_mut() = Some(Recorder { sink, every, recs: vec![], snaps: vec![], bases: vec![], last: Default::default(), seen_bases: Default::default(), next_snap: 0, snap_words: 0 }));
    RECORDING.with(|r| r.set(true));
}
/// Flush what is left and stop. Returns the words of snapshots made.
pub fn record_stop() -> u64 {
    RECORDING.with(|r| r.set(false));
    let rec = RECORDER.with(|r| r.borrow_mut().take());
    if let Some(mut r) = rec { flush(&mut r); r.snap_words } else { 0 }
}
fn flush(r: &mut Recorder) {
    if r.recs.is_empty() && r.snaps.is_empty() { return; }
    let b = Batch { recs: std::mem::take(&mut r.recs), snaps: std::mem::take(&mut r.snaps), bases: std::mem::take(&mut r.bases) };
    (r.sink)(b);
}
#[allow(clippy::too_many_arguments)]
fn record<F: Form + ?Sized>(f: &F, v: &View, state_in: &[(u32, i32)], m: &Mods, inject: &[(u32, i32)], p: &Params, st: &Activation, out: &Out) {
    RECORDER.with(|r| {
        let mut g = r.borrow_mut();
        let Some(r) = g.as_mut() else { return };
        let (uid, ver, base) = (v.c.uid, v.c.version, v.base.uid);
        if r.seen_bases.insert(base) { r.bases.push((base, std::sync::Arc::new(pack_base(v.base)))); }
        let snap = match r.last.get(&uid) {
            Some(&(sv, id)) if sv == ver => id,
            _ => {
                let id = r.next_snap; r.next_snap += 1;
                let words = pack_mind(f, v);
                r.snap_words += words.len() as u64;
                r.snaps.push(Snap { id, uid, base, words: std::sync::Arc::new(words) });
                r.last.insert(uid, (ver, id));
                id
            }
        };
        let state_out: Vec<(u32, i32)> = st.active.iter().map(|&n| (n, st.act[n as usize])).collect();
        r.recs.push(Rec { snap, uid, base, mods: *m, params: *p, state_in: state_in.to_vec(), inject: inject.to_vec(), out: out.clone(), state_out, n: f.len() });
        if r.recs.len() >= r.every { flush(r); }
    });
}
