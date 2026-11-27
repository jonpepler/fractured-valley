//! Track A, second version: the exact sparse kernel with the bookkeeping cut down.
//!
//! Same contract as `exact::step` (bit-identical results, tested), but:
//! - frontier sets are *selected*, never sorted: the order in which a frontier is expanded
//!   cannot change any sum (i32 wrapping addition is commutative), so only the set matters;
//! - seeds and injections are merged through the hash table instead of a sort;
//! - decay, seeding and loading the state happen in one pass;
//! - the state hash is computed in place.

use crate::delta::Delta;
use crate::exact::{GainCache, Inst, GHOST};
use crate::model::key;
use crate::shared::{PEdge, Shared};
use mc::spread::{Mods, Params};

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Slot { node: u32, gen: u32, act: i32, inc: i32, seed: i32, mult: u32, seeded: u32 }

pub struct Table { slots: Vec<Slot>, mask: usize, gen: u32, used: Vec<u32> }
impl Table {
    fn new(cap: usize) -> Table { let c = cap.next_power_of_two(); Table { slots: vec![Slot::default(); c], mask: c - 1, gen: 1, used: Vec::new() } }
    fn reset(&mut self) {
        self.gen = self.gen.wrapping_add(1);
        if self.gen == 0 { for s in self.slots.iter_mut() { s.gen = 0; } self.gen = 1; }
        self.used.clear();
    }
    #[inline(always)]
    fn slot(&mut self, n: u32) -> usize {
        let mut i = (n.wrapping_mul(0x9e37_79b1) >> 10) as usize & self.mask;
        loop {
            let s = self.slots[i];
            if s.gen != self.gen {
                if (self.used.len() + 1) * 2 > self.slots.len() { self.grow(); return self.slot(n); }
                self.slots[i] = Slot { node: n, gen: self.gen, act: 0, inc: 0, seed: 0, mult: 0, seeded: 0 };
                self.used.push(i as u32);
                return i;
            }
            if s.node == n { return i; }
            i = (i + 1) & self.mask;
        }
    }
    #[cold]
    fn grow(&mut self) {
        let old: Vec<Slot> = self.used.iter().map(|&i| self.slots[i as usize]).collect();
        let c = self.slots.len() * 2;
        self.slots = vec![Slot::default(); c]; self.mask = c - 1; self.gen = 1; self.used.clear();
        for s in old {
            let mut i = (s.node.wrapping_mul(0x9e37_79b1) >> 10) as usize & self.mask;
            while self.slots[i].gen == self.gen { i = (i + 1) & self.mask; }
            self.slots[i] = s; self.slots[i].gen = self.gen; self.used.push(i as u32);
        }
    }
}

pub struct Scratch { t: Table, pub g: GainCache, cand: Vec<(u32, i32)>, touched: Vec<u32>, c: Vec<(u32, i32)>, pub log: Option<Vec<(u8, u32)>> }
impl Scratch { pub fn new() -> Self { Scratch { t: Table::new(4096), g: GainCache::new(), cand: vec![], touched: vec![], c: vec![], log: None } } }
impl Default for Scratch { fn default() -> Self { Self::new() } }

/// Keep the `k` largest by (amount desc, id asc); order unspecified.
#[inline]
pub fn select(c: &mut Vec<(u32, i32)>, k: usize) {
    if c.len() > k { c.select_nth_unstable_by_key(k - 1, |&(n, a)| key(n, a)); c.truncate(k); }
}

#[inline(always)]
fn add(t: &mut Table, n: u32, a: i32, max: i32) -> usize {
    let i = t.slot(n);
    let s = &mut t.slots[i];
    if s.act == 0 && a > 0 { s.mult += 1; }
    s.act = s.act.wrapping_add(a).min(max);
    i
}

#[inline(always)]
fn edge(t: &mut Table, touched: &mut Vec<u32>, g: &mut GainCache, m: &Mods, a: i64, den: i64, hd: i64, min: i32, to: u32, w: i16, k: u16) {
    if w <= 0 { return; }
    // Signed throughout, truncating toward zero: a frontier amount can be negative after an
    // i32 wrap, and gain can be negative at extreme modifiers, so their product can pass.
    let q = a * w as i64 / den;
    let fl = q * g.get(m, k) as i64 / 1000 * hd / 1000;
    let fl = fl as i32;
    if fl < min { return; }
    let i = t.slot(to);
    let s = &mut t.slots[i];
    if s.inc == 0 { touched.push(to); }
    s.inc = s.inc.wrapping_add(fl);
}

#[inline(always)]
fn expand(sh: &Shared, d: &Delta, u: u32, a: i32, t: &mut Table, touched: &mut Vec<u32>, g: &mut GainCache, m: &Mods, p: &Params) {
    if a == 0 { return; } // every flow is 0 < min_act
    let (au, hd, min) = (a as i64, p.hop_decay as i64, p.min_act);
    match d.find(u) {
        None => {
            if u >= d.base_n { return; }
            let den = sh.den[u as usize] as i64;
            for e in &sh.edges[sh.range(u)] { edge(t, touched, g, m, au, den, hd, min, e.to, e.w, e.key); }
        }
        Some(i) => {
            let den = d.den[i] as i64;
            if u < d.base_n {
                let ov = &d.ovr[d.ostart[i] as usize..d.ostart[i + 1] as usize];
                let r = sh.range(u);
                let es: &[PEdge] = &sh.edges[r];
                let mut j = 0;
                for (l, e) in es.iter().enumerate() {
                    if j < ov.len() && ov[j].0 as usize == l { edge(t, touched, g, m, au, den, hd, min, e.to, ov[j].1, ov[j].2); j += 1; }
                    else { edge(t, touched, g, m, au, den, hd, min, e.to, e.w, e.key); }
                }
            }
            for e in &d.extra[d.xstart[i] as usize..d.xstart[i + 1] as usize] { edge(t, touched, g, m, au, den, hd, min, e.to, e.w, e.key); }
        }
    }
}

/// One step. Returns (top `ws_k` nodes in order, hash of (state, output)) — the same hash as
/// `model::hash(&state, &out)`.
pub fn step(sh: &Shared, d: &Delta, inst: &mut Inst, m: &Mods, inject: &[(u32, i32)], p: &Params, sc: &mut Scratch) -> (Vec<u32>, u64) {
    assert!(p.min_act > 0 && p.frontier > 0 && p.active_cap > 0);
    let Scratch { t, g, cand, touched, c, log } = sc;
    t.reset(); g.reset(); cand.clear();
    // load + decay (once per occurrence in the active list) + seeds
    for &n in &inst.dups { let i = t.slot(n); t.slots[i].mult += 1; }
    for &(n, x) in &inst.st {
        let i = t.slot(n & !GHOST);
        let s = &mut t.slots[i];
        s.act = x;
        if n & GHOST != 0 { continue; }
        let k = s.mult + 1; s.mult = 0;
        for _ in 0..k {
            s.act = (s.act as i64 * p.persist as i64 / 1000) as i32;
            if s.act < p.min_act { s.act = 0; } else { s.mult += 1; }
        }
        let sd = s.act * p.carry_spread / 1000;
        if s.mult > 0 && sd > 0 { s.seed = sd.wrapping_mul(s.mult as i32); s.seeded = 1; }
    }
    for &(n, a) in inject { let i = add(t, n, a, p.max_act); let s = &mut t.slots[i]; s.seed = s.seed.wrapping_add(a); s.seeded = 1; }
    for &i in &t.used { let s = &t.slots[i as usize]; if s.seeded != 0 { cand.push((s.node, s.seed)); } }
    select(cand, p.frontier);
    // rounds
    for r in 0..p.hops {
        if cand.is_empty() { break; }
        if let Some(l) = log { l.extend(cand.iter().map(|x| (r as u8, x.0))); }
        for &(u, a) in cand.iter() { expand(sh, d, u, a, t, touched, g, m, p); }
        cand.clear();
        for &n in touched.iter() {
            let i = t.slot(n);
            let v = std::mem::take(&mut t.slots[i].inc);
            add(t, n, v, p.max_act);
            cand.push((n, v));
        }
        touched.clear();
        select(cand, p.frontier);
    }
    // finish
    c.clear();
    for &i in &t.used { let s = &mut t.slots[i as usize]; for _ in 0..s.mult { c.push((s.node, s.act)); } s.mult = 0; }
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
    inst.st.clear(); inst.dups.clear();
    for &i in &t.used {
        let s = &t.slots[i as usize];
        if s.mult > 0 { inst.st.push((s.node, s.act)); for _ in 1..s.mult { inst.dups.push(s.node); } }
        else if s.act != 0 { inst.st.push((s.node | GHOST, s.act)); }
    }
    inst.st.sort_unstable_by_key(|x| x.0 & !GHOST);
    // output: top ws_k of the active entries with x > 0
    cand.clear();
    cand.extend(c.iter().copied().filter(|x| x.1 > 0));
    select(cand, p.ws_k);
    cand.sort_unstable_by_key(|&(n, a)| key(n, a));
    let out: Vec<u32> = cand.iter().map(|x| x.0).collect();
    (out.clone(), state_hash(&inst.st, &out))
}

/// `model::hash(&inst.state(), out)` without building the state.
pub fn state_hash(st: &[(u32, i32)], out: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u64| { h ^= v; h = h.wrapping_mul(0x0000_0100_0000_01b3); h ^= h >> 29; };
    for &(n, a) in st { if n & GHOST == 0 && a > 0 { mix(n as u64); mix(a as u64); } }
    mix(0xffff_ffff);
    for &n in out { mix(n as u64); }
    h
}
