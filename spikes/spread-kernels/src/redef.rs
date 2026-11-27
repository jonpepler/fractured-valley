//! Track B: a redefined step with the same intent, cheaper, and 32-bit only (so the same
//! arithmetic runs on a GPU, in WASM and natively).
//!
//! Differences from the oracle, all deliberate:
//! 1. **Gain is interpolated.** Per step and gain class, the oracle's `gain` is called at three
//!    knots (value -1000, 0, 1000); in between, the gain is the straight line between knots.
//!    About 3 oracle calls per class used instead of one per distinct (class, value).
//! 2. **Gain and hop decay are folded** into one per-mille factor per edge,
//!    `f = clamp(g * hop_decay / 1000, 0, 32767)`, and the flow is `q * f / 1000` with
//!    `q = a * w / den`: two truncations instead of three.
//! 3. **No wrap-around.** Frontier amounts are capped at 65535 and increments saturate at 2^30,
//!    so every product fits in u32/i32. Negative gains give no flow.
//! 4. **Exact pruning.** The interpolated factor is piecewise linear, so its maximum over a
//!    class is at a knot: edges that cannot reach `min_act` are skipped without any assumption.
//!
//! Decay, seeding, frontier selection, the budget and support cap, and the output are as in
//! the oracle.

use crate::delta::Delta;
use crate::exact::Inst;
use crate::fast::{select, Table};
use crate::model::key;
use crate::shared::{kclass, kval, Shared, TY_REP};
use mc::spread::{gain, Mods, Params};

pub const A_CAP: i32 = 65_535;
pub const INC_CAP: i32 = 1 << 30;
pub const F_CAP: i32 = 32_767;

/// Per-step factors per gain class: f at value -1000, 0, 1000 (folded with hop decay).
#[derive(Clone, Copy, Default)]
pub struct Knots { pub f: [[i32; 3]; 32], pub fmax: [i32; 32] }

/// The knots for one modifier set: 3 oracle gain calls per class in `classes` (bit mask).
pub fn knots(m: &Mods, hop_decay: i32, classes: u32) -> Knots {
    let mut k = Knots::default();
    let mut mask = classes;
    while mask != 0 {
        let c = mask.trailing_zeros(); mask &= mask - 1;
        let ty = TY_REP[(c >> 3) as usize];
        let tc = (c & 7) as u8; let tg = (tc & 3) | ((tc >> 2) << 7);
        for (j, v) in [-1000, 0, 1000].into_iter().enumerate() { k.f[c as usize][j] = gain(m, ty, tg, v); }
        let g = k.f[c as usize];
        for j in 0..3 { k.f[c as usize][j] = fold(g[j], hop_decay); }
        k.fmax[c as usize] = k.f[c as usize].iter().copied().max().unwrap();
    }
    k
}

#[inline] fn fold(g: i32, hd: i32) -> i32 { ((g.clamp(-F_CAP * 2, F_CAP * 2) * hd) / 1000).clamp(0, F_CAP) }

/// The interpolated factor for a key. `g(v) = g0 + (g± - g0) * |v| / 1000`, then folded; the
/// interpolation is done on the folded factors directly (both are linear in g).
#[inline(always)]
pub fn factor(k: &Knots, key: u16) -> i32 {
    let f = &k.f[kclass(key) as usize];
    let v = kval(key);
    if v < 0 { f[1] + (f[0] - f[1]) * (-v) / 1000 } else { f[1] + (f[2] - f[1]) * v / 1000 }
}

pub struct Scratch { pub t: Table, cand: Vec<(u32, i32)>, touched: Vec<u32>, c: Vec<(u32, i32)>, pub k: Knots, pub visits: u64, pub expanded: u64, lazy: Option<(Mods, i32, u32)> }
impl Scratch { pub fn new() -> Self { Scratch { t: Table::new(4096), cand: vec![], touched: vec![], c: vec![], k: Knots::default(), visits: 0, expanded: 0, lazy: None } } }

/// Lazy knots: fill class `c` on first use (no-op when knots were precomputed).
#[inline(always)]
fn ensure(k: &mut Knots, lazy: &mut Option<(Mods, i32, u32)>, c: u32) {
    if let Some((m, hd, done)) = lazy {
        if *done >> c & 1 == 0 { let one = knots(m, *hd, 1 << c); k.f[c as usize] = one.f[c as usize]; k.fmax[c as usize] = one.fmax[c as usize]; *done |= 1 << c; }
    }
}
impl Default for Scratch { fn default() -> Self { Self::new() } }

/// Every gain class used by the shared graph or a delta's edges.
pub fn classes(sh: &Shared, ds: &[Delta]) -> u32 {
    let mut m = sh.cmask.iter().fold(0, |a, &x| a | x);
    for d in ds { for o in &d.ovr { m |= 1 << kclass(o.2); } for e in &d.extra { m |= 1 << kclass(e.key); } }
    m
}

#[inline(always)]
fn edge(t: &mut Table, touched: &mut Vec<u32>, k: &Knots, a: u32, den: u32, min: i32, to: u32, w: i16, key: u16) {
    if w <= 0 { return; }
    let q = a * w as u32 / den;
    let f = factor(k, key);
    if f <= 0 { return; }
    let fl = (q * f as u32 / 1000) as i32;
    if fl < min { return; }
    let i = t.slot(to);
    let s = &mut t.slots[i];
    if s.inc == 0 { touched.push(to); }
    s.inc = (s.inc + fl).min(INC_CAP);
}

/// One step with precomputed knots in `sc.k` (see `step` for the usual entry point).
pub fn step_with_knots(sh: &Shared, d: &Delta, inst: &mut Inst, inject: &[(u32, i32)], p: &Params, sc: &mut Scratch) -> (Vec<u32>, u64) { step_k::<true>(sh, d, inst, inject, p, sc) }

/// Without pruning (the reference the pruned kernel is tested against).
pub fn step_unpruned(sh: &Shared, d: &Delta, inst: &mut Inst, m: &Mods, inject: &[(u32, i32)], p: &Params, sc: &mut Scratch) -> (Vec<u32>, u64) {
    sc.k = knots(m, p.hop_decay, u32::MAX);
    step_k::<false>(sh, d, inst, inject, p, sc)
}

fn step_k<const PRUNE: bool>(sh: &Shared, d: &Delta, inst: &mut Inst, inject: &[(u32, i32)], p: &Params, sc: &mut Scratch) -> (Vec<u32>, u64) {
    assert!(p.min_act > 0 && p.frontier > 0 && p.active_cap > 0 && (0..=1000).contains(&p.hop_decay) && p.max_act <= A_CAP);
    let Scratch { t, cand, touched, c, k, visits, expanded, lazy } = sc;
    t.reset(); cand.clear();
    for &(n, x) in &inst.st {
        let i = t.slot(n);
        let s = &mut t.slots[i];
        s.act = x * p.persist / 1000;
        if s.act < p.min_act { s.act = 0; continue; }
        s.mult = 1;
        let sd = s.act * p.carry_spread / 1000;
        if sd > 0 { s.seed = sd; s.seeded = 1; }
    }
    for &(n, a) in inject {
        let a = a.clamp(0, A_CAP);
        let i = t.slot(n); let s = &mut t.slots[i];
        if s.act == 0 && a > 0 { s.mult = 1; }
        s.act = (s.act + a).min(p.max_act);
        s.seed = (s.seed + a).min(A_CAP); s.seeded = 1;
    }
    for &i in &t.used { let s = &t.slots[i as usize]; if s.seeded != 0 && s.seed > 0 { cand.push((s.node, s.seed)); } }
    select(cand, p.frontier);
    let min = p.min_act;
    for _ in 0..p.hops {
        if cand.is_empty() { break; }
        *expanded += cand.len() as u64;
        for &(u, a) in cand.iter() {
            let a = a as u32;
            match d.find(u) {
                None => {
                    if u >= d.base_n { continue; }
                    let den = sh.den[u as usize] as u32;
                    let mut mask = sh.cmask[u as usize];
                    let mut fmax = 0;
                    while mask != 0 { let c = mask.trailing_zeros(); mask &= mask - 1; ensure(k, lazy, c); fmax = fmax.max(k.fmax[c as usize]); }
                    if PRUNE && fmax <= 0 { continue; }
                    // q * fmax / 1000 >= min  <=>  q >= ceil(1000 min / fmax)  <=>  w >= ceil(qmin den / a)
                    let wmin = if PRUNE { ((1000 * min as u32).div_ceil(fmax as u32) as u64 * den as u64).div_ceil(a as u64) } else { 0 };
                    for e in &sh.edges[sh.range(u)] {
                        if (e.w as u64) < wmin { break; }
                        *visits += 1;
                        edge(t, touched, k, a, den, min, e.to, e.w, e.key);
                    }
                }
                Some(i) => {
                    let den = d.den[i] as u32;
                    let mut mask = d.pmask[i];
                    let mut fmax = 0;
                    while mask != 0 { let c = mask.trailing_zeros(); mask &= mask - 1; ensure(k, lazy, c); fmax = fmax.max(k.fmax[c as usize]); }
                    if PRUNE && fmax <= 0 { continue; }
                    let wmin = if PRUNE { ((1000 * min as u32).div_ceil(fmax as u32) as u64 * den as u64).div_ceil(a as u64) } else { 0 };
                    if u < d.base_n {
                        let ov = &d.ovr[d.ostart[i] as usize..d.ostart[i + 1] as usize];
                        let mut j = 0;
                        for (l, e) in sh.edges[sh.range(u)].iter().enumerate() {
                            let (w, kk) = if j < ov.len() && ov[j].0 as usize == l { j += 1; (ov[j - 1].1, ov[j - 1].2) } else { (e.w, e.key) };
                            if (w as u64) < wmin || w <= 0 { continue; }
                            *visits += 1;
                            edge(t, touched, k, a, den, min, e.to, w, kk);
                        }
                    }
                    for e in &d.extra[d.xstart[i] as usize..d.xstart[i + 1] as usize] { if e.w > 0 && (e.w as u64) >= wmin { *visits += 1; edge(t, touched, k, a, den, min, e.to, e.w, e.key); } }
                }
            }
        }
        cand.clear();
        for &n in touched.iter() {
            let i = t.slot(n);
            let s = &mut t.slots[i];
            let v = std::mem::take(&mut s.inc);
            if s.act == 0 && v > 0 { s.mult = 1; }
            s.act = (s.act + v).min(p.max_act);
            cand.push((n, v.min(A_CAP)));
        }
        touched.clear();
        select(cand, p.frontier);
    }
    // finish, as the oracle
    c.clear();
    for &i in &t.used { let s = &t.slots[i as usize]; if s.mult > 0 { c.push((s.node, s.act)); } }
    let tot: i64 = c.iter().map(|x| x.1 as i64).sum();
    if tot > p.budget as i64 {
        for x in c.iter_mut() { x.1 = (x.1 as i64 * p.budget as i64 / tot) as i32; }
        c.retain(|x| x.1 >= p.min_act);
    }
    if c.len() > p.active_cap { c.select_nth_unstable_by_key(p.active_cap - 1, |&(n, a)| key(n, a)); c.truncate(p.active_cap); }
    c.sort_unstable_by_key(|x| x.0);
    inst.st.clear(); inst.dups.clear();
    inst.st.extend_from_slice(c);
    cand.clear();
    cand.extend(c.iter().copied().filter(|x| x.1 > 0));
    select(cand, p.ws_k);
    cand.sort_unstable_by_key(|&(n, a)| key(n, a));
    let out: Vec<u32> = cand.iter().map(|x| x.0).collect();
    let h = crate::fast::state_hash(&inst.st, &out);
    (out, h)
}

/// One step: knots for this step's modifiers (only the classes in `classes`), then the kernel.
pub fn step(sh: &Shared, d: &Delta, inst: &mut Inst, m: &Mods, inject: &[(u32, i32)], p: &Params, classes: u32, sc: &mut Scratch) -> (Vec<u32>, u64) {
    sc.k = knots(m, p.hop_decay, classes);
    step_with_knots(sh, d, inst, inject, p, sc)
}

/// Damp, as the oracle's (x stays in i32 range in this kernel).
pub fn damp(inst: &mut Inst, pm: i32) { for x in inst.st.iter_mut() { x.1 = x.1 * pm / 1000; } }

/// As `step`, but knots are computed only for the classes this step actually meets (the
/// result is identical: knots of unused classes are never read).
pub fn step_lazy(sh: &Shared, d: &Delta, inst: &mut Inst, m: &Mods, inject: &[(u32, i32)], p: &Params, sc: &mut Scratch) -> (Vec<u32>, u64) {
    sc.k = Knots::default();
    sc.lazy = Some((*m, p.hop_decay, 0));
    let r = step_with_knots(sh, d, inst, inject, p, sc);
    sc.lazy = None;
    r
}
