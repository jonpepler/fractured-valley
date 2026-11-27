//! Track A: an exact sparse kernel. Same results as `model::Dense` (and so the oracle), but the
//! per-instance state is a sorted list of at most a few hundred entries, the graph is shared, and
//! the dense arrays are replaced by a per-thread hash table that is reset by generation stamps.

use crate::delta::Delta;
use crate::model::{key, merge, top_k};
use crate::shared::{unkey, Shared, KEYS};
use mc::spread::{gain, Mods, Params};

pub const GHOST: u32 = 1 << 31;

/// One instance's state (its delta lives apart, so states can be cloned and reset cheaply).
#[derive(Clone, Debug, Default)]
pub struct Inst {
    /// Every node with a nonzero x or an entry in the active list, sorted by node. The top bit of
    /// the node marks a ghost (x != 0 but not active; only after an i32 wrap).
    pub st: Vec<(u32, i32)>,
    /// Extra occurrences in the active list (a node listed twice; only after an i32 wrap).
    pub dups: Vec<u32>,
}

impl Inst {
    pub fn new() -> Inst { Inst { st: Vec::new(), dups: Vec::new() } }
    /// Canonical state (as `model::Dense::state`).
    pub fn state(&self) -> Vec<(u32, i32)> { self.st.iter().filter(|x| x.0 & GHOST == 0 && x.1 > 0).copied().collect() }
    pub fn damp(&mut self, pm: i32) {
        // The oracle's damp walks the active list; a duplicated node is damped twice.
        for x in self.st.iter_mut() { if x.0 & GHOST == 0 { x.1 = (x.1 as i64 * pm as i64 / 1000) as i32; } }
        for &n in &self.dups { let i = self.st.binary_search_by_key(&n, |x| x.0).unwrap(); self.st[i].1 = (self.st[i].1 as i64 * pm as i64 / 1000) as i32; }
    }
    pub fn bytes(&self) -> usize { self.st.capacity() * 8 + self.dups.capacity() * 4 }
}

#[derive(Clone, Copy, Default)]
struct Slot { node: u32, gen: u32, act: i32, inc: i32, mult: u32 }

/// Open-addressing table node -> (act, inc, multiplicity in the active list).
pub struct Table { slots: Vec<Slot>, mask: usize, gen: u32, pub used: Vec<u32> }

impl Table {
    pub fn new(cap: usize) -> Table { let c = cap.next_power_of_two(); Table { slots: vec![Slot::default(); c], mask: c - 1, gen: 1, used: Vec::new() } }
    fn reset(&mut self) {
        self.gen = self.gen.wrapping_add(1);
        if self.gen == 0 { for s in self.slots.iter_mut() { s.gen = 0; } self.gen = 1; }
        self.used.clear();
    }
    #[inline]
    fn slot(&mut self, n: u32) -> usize {
        let mut i = (n.wrapping_mul(0x9e37_79b1) >> 7) as usize & self.mask;
        loop {
            let s = &mut self.slots[i];
            if s.gen != self.gen { *s = Slot { node: n, gen: self.gen, act: 0, inc: 0, mult: 0 }; self.used.push(i as u32); break; }
            if s.node == n { break; }
            i = (i + 1) & self.mask;
        }
        if self.used.len() * 2 > self.slots.len() { self.grow(); return self.slot(n); }
        i
    }
    fn grow(&mut self) {
        let old: Vec<Slot> = self.used.iter().map(|&i| self.slots[i as usize]).collect();
        let c = self.slots.len() * 2;
        self.slots = vec![Slot::default(); c]; self.mask = c - 1; self.gen = 1; self.used.clear();
        for s in old {
            let mut i = (s.node.wrapping_mul(0x9e37_79b1) >> 7) as usize & self.mask;
            while self.slots[i].gen == self.gen { i = (i + 1) & self.mask; }
            self.slots[i] = s; self.slots[i].gen = self.gen; self.used.push(i as u32);
        }
    }
}

/// Per-step memo of gain by key, direct mapped, reset by generation: (generation << 32 | gain).
pub struct GainCache { e: Vec<u64>, gen: u32, pub misses: u64 }
impl GainCache {
    pub fn new() -> Self { GainCache { e: vec![0; KEYS], gen: 0, misses: 0 } }
    pub fn reset(&mut self) { self.gen = self.gen.wrapping_add(1); if self.gen == 0 { self.e.iter_mut().for_each(|t| *t = 0); self.gen = 1; } }
    #[inline]
    pub fn get(&mut self, m: &Mods, k: u16) -> i32 {
        let x = self.e[k as usize];
        if (x >> 32) as u32 == self.gen { return x as u32 as i32; }
        self.miss(m, k)
    }
    #[cold]
    fn miss(&mut self, m: &Mods, k: u16) -> i32 {
        let (ty, tg, v) = unkey(k);
        let g = gain(m, ty, tg, v);
        self.misses += 1;
        self.e[k as usize] = (self.gen as u64) << 32 | g as u32 as u64; g
    }
}

/// Exact division by a fixed divisor for dividends below 2^32 (Lemire, Kaser & Kurz 2019:
/// with c = ceil(2^64 / d), n / d = (c * n) >> 64 for all 32-bit n and d).
#[derive(Clone, Copy)]
pub struct Recip { c: u64, d: u64 }
impl Recip {
    #[inline] pub fn new(d: u32) -> Recip { Recip { c: (u64::MAX / d as u64).wrapping_add(1), d: d as u64 } }
    #[inline] pub fn div(self, n: u64) -> u64 { if n >> 32 == 0 && self.d > 1 { ((self.c as u128 * n as u128) >> 64) as u64 } else { n / self.d } }
}
impl Default for GainCache { fn default() -> Self { Self::new() } }

/// Per-thread scratch.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats { pub steps: u64, pub expanded: u64, pub patched: u64, pub visits: u64, pub passed: u64, pub touched: u64, pub seeds: u64, pub gain_misses: u64, pub state: u64, pub rounds: u64 }

pub struct Scratch { pub t: Table, pub g: GainCache, cand: Vec<(u32, i32)>, active: Vec<u32>, touched: Vec<u32>, pub visits: u64, pub stats: Stats }
impl Scratch {
    pub fn new() -> Self { Scratch { t: Table::new(4096), g: GainCache::new(), cand: vec![], active: vec![], touched: vec![], visits: 0, stats: Stats::default() } }
}
impl Default for Scratch { fn default() -> Self { Self::new() } }

#[inline]
fn add(t: &mut Table, active: &mut Vec<u32>, n: u32, a: i32, max: i32) {
    let i = t.slot(n);
    let s = &mut t.slots[i];
    if s.act == 0 && a > 0 { active.push(n); s.mult += 1; }
    s.act = s.act.wrapping_add(a).min(max);
}

/// One step of one instance. Returns the top `ws_k` nodes.
pub fn step(sh: &Shared, d: &Delta, inst: &mut Inst, m: &Mods, inject: &[(u32, i32)], p: &Params, sc: &mut Scratch) -> Vec<u32> { step_cfg::<true>(sh, d, inst, m, inject, p, sc) }

pub fn step_cfg<const RECIP: bool>(sh: &Shared, d: &Delta, inst: &mut Inst, m: &Mods, inject: &[(u32, i32)], p: &Params, sc: &mut Scratch) -> Vec<u32> {
    let Scratch { t, g, cand, active, touched, visits, stats } = sc;
    stats.steps += 1; stats.state += inst.st.len() as u64;
    t.reset(); g.reset();
    active.clear(); cand.clear();
    // Load state. The active list is rebuilt in node order; its order never matters, only
    // multiplicity does.
    for &(n, x) in &inst.st {
        let node = n & !GHOST;
        let i = t.slot(node);
        t.slots[i].act = x;
        if n & GHOST == 0 { t.slots[i].mult = 1; }
    }
    for &n in &inst.dups { let i = t.slot(n); t.slots[i].mult += 1; }
    // 1. decay (each occurrence in the active list), drop below min.
    for &(n, _) in &inst.st {
        if n & GHOST != 0 { continue; }
        let i = t.slot(n);
        let s = &mut t.slots[i];
        let k = s.mult; s.mult = 0;
        for _ in 0..k {
            s.act = (s.act as i64 * p.persist as i64 / 1000) as i32;
            if s.act < p.min_act { s.act = 0; } else { s.mult += 1; }
        }
        for _ in 0..s.mult { active.push(n); }
    }
    // 2. seeds
    for &n in active.iter() {
        let i = t.slot(n);
        let s = t.slots[i].act * p.carry_spread / 1000;
        if s > 0 { cand.push((n, s)); }
    }
    for &(n, a) in inject { add(t, active, n, a, p.max_act); cand.push((n, a)); }
    merge(cand);
    stats.seeds += cand.len() as u64;
    top_k(cand, p.frontier);
    // 3. rounds
    for _ in 0..p.hops {
        if cand.is_empty() { break; }
        stats.rounds += 1;
        for &(u, a) in cand.iter() {
            stats.expanded += 1;
            if d.find(u).is_some() { stats.patched += 1; }
            let mut rc: Option<Recip> = None;
            let au = a.max(0) as u64;
            let cnt = d.for_edges(sh, u, |den, to, w, k| {
                if w <= 0 { return; }
                let r = *rc.get_or_insert_with(|| Recip::new(den as u32));
                // Signed (a can be negative after an i32 wrap, gain at extreme modifiers).
                let q = if RECIP && a > 0 { r.div(au * w as u64) as i64 } else { a as i64 * w as i64 / den as i64 };
                let fl = q * g.get(m, k) as i64 / 1000 * p.hop_decay as i64 / 1000;
                let fl = fl as i32;
                if fl < p.min_act { return; }
                stats.passed += 1;
                let i = t.slot(to);
                let s = &mut t.slots[i];
                if s.inc == 0 { touched.push(to); }
                s.inc = s.inc.wrapping_add(fl);
            });
            *visits += cnt as u64; stats.visits += cnt as u64;
        }
        cand.clear();
        stats.touched += touched.len() as u64;
        for &n in touched.iter() {
            let i = t.slot(n);
            let v = std::mem::take(&mut t.slots[i].inc);
            add(t, active, n, v, p.max_act);
            cand.push((n, v));
        }
        touched.clear();
        top_k(cand, p.frontier);
    }
    finish(inst, p, t, active, cand)
}

fn finish(inst: &mut Inst, p: &Params, t: &mut Table, active: &mut Vec<u32>, c: &mut Vec<(u32, i32)>) -> Vec<u32> {
    c.clear();
    for &n in active.iter() { let i = t.slot(n); c.push((n, t.slots[i].act)); }
    // Every table entry leaves the active list now; survivors are re-added below.
    for &i in &t.used { t.slots[i as usize].mult = 0; }
    let tot: i64 = c.iter().map(|x| x.1 as i64).sum();
    if tot > p.budget as i64 {
        for x in c.iter_mut() { x.1 = (x.1 as i64 * p.budget as i64 / tot) as i32; let i = t.slot(x.0); t.slots[i].act = x.1; }
        c.retain(|x| if x.1 < p.min_act { let i = t.slot(x.0); t.slots[i].act = 0; false } else { true });
    }
    if c.len() > p.active_cap {
        c.select_nth_unstable_by_key(p.active_cap - 1, |&(n, a)| key(n, a));
        for &(n, _) in &c[p.active_cap..] { let i = t.slot(n); t.slots[i].act = 0; }
        c.truncate(p.active_cap);
    }
    for &(n, _) in c.iter() { let i = t.slot(n); t.slots[i].mult += 1; }
    // write back: every node with x != 0 or in the active list
    inst.st.clear(); inst.dups.clear();
    for &i in &t.used {
        let s = &t.slots[i as usize];
        if s.mult > 0 { inst.st.push((s.node, s.act)); for _ in 1..s.mult { inst.dups.push(s.node); } }
        else if s.act != 0 { inst.st.push((s.node | GHOST, s.act)); }
    }
    inst.st.sort_unstable_by_key(|x| x.0 & !GHOST);
    let mut out: Vec<(u32, i32)> = c.iter().copied().filter(|x| x.1 > 0).collect();
    top_k(&mut out, p.ws_k);
    out.into_iter().map(|x| x.0).collect()
}

impl Inst {
    /// From a dense (act, active list) pair as the oracle keeps it.
    pub fn from_dense(act: &[i32], active: &[u32]) -> Inst {
        let mut cnt: std::collections::BTreeMap<u32, u32> = Default::default();
        for &n in active { *cnt.entry(n).or_default() += 1; }
        for (n, &x) in act.iter().enumerate() { if x != 0 { cnt.entry(n as u32).or_default(); } }
        let mut inst = Inst::new();
        for (&n, &c) in &cnt {
            inst.st.push((if c == 0 { n | GHOST } else { n }, act[n as usize]));
            for _ in 1..c { inst.dups.push(n); }
        }
        inst
    }
}
