//! Rewritten thinks over the same forms, one idea at a time, so each can be measured alone:
//!
//! - **frontier selection**: the reference sorts every touched node to keep the top 48. `Nth`
//!   partitions with `select_nth_unstable` and sorts only the 48; `Heap` keeps a fixed-size
//!   min-heap of 48. Both pick exactly the same frontier (the order is total: activation, then id).
//! - **gain memo**: the gain depends only on (link type, the target's gain-relevant tags, felt
//!   valence) within a think, so it is cached per think in a small direct-mapped table. Exact.
//! - **fixed point**: the one division by a variable per link (by the sender's out-sum) becomes
//!   one division per sender and a multiply-shift per link. Integer and deterministic, but it
//!   rounds once instead of three times, so it is not bit-exact against the reference.
//! - **gain table** (`GainMode`): the type/tag stage of the gain once per think, the valence
//!   stage per link (exact), or the whole gain as one fixed-point multiplier (approximate).
//! - **delta input**: steady input is not re-spread. New or increased input spreads in full;
//!   input that was already there last tick is added to activation but spreads only at the
//!   carry share, like the carried thought. Not faithful by design: it measures what event-driven
//!   thinking would change.

use crate::graph::{lt, tag};
use crate::spread::{finish, gain, Activation, Form, Mods, Params};
use std::cmp::Reverse;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Select { Sort, Nth, Heap }

/// How the gain is computed: `spread::gain` per link, the exact two-stage table, or the
/// fixed-point table (not bit-exact).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GainMode { Full, Table, Approx }

#[inline]
pub fn tagclass(t: u8) -> usize { ((t & tag::FOOD) | (t & tag::REST) | ((t & tag::ANGER) >> 5)) as usize }

/// The gain split in two exact stages: the link type and target tags (a per-think table), then
/// the felt valence and fatigue (per link). Same operations in the same order as `spread::gain`.
pub struct Gain { pre: [i64; lt::COUNT * 8], zero: [i64; lt::COUNT * 8], an: i64, neg: i64, pos: i64, fk: i64, cneg: i64, cpos: i64 }
impl Gain {
    pub fn new(m: &Mods) -> Gain {
        let (a, h, f, s) = (m.arousal as i64, m.hunger as i64, m.fatigue as i64, m.stress as i64);
        let neg = (-m.mood).max(0) as i64;
        let pos = m.mood.max(0) as i64;
        // Fixed point (approx mode): gain = zero[k] × (2^20 + |val| × c) >> 20.
        let mut g = Gain { pre: [0; lt::COUNT * 8], zero: [0; lt::COUNT * 8], an: a + neg, neg, pos, fk: 1000 - f / 3,
            cneg: (a + neg) * (1 << 20) / 500_000, cpos: (pos - neg) * (1 << 20) / 1_000_000 };
        for ty in 0..lt::COUNT {
            for tc in 0..8usize {
                let mut x = crate::spread::TYPE_GAIN[ty] as i64;
                let t = ty as u8;
                if t == lt::NEAR || t == lt::LIKE || t == lt::ASSOC { x = x * (1000 - a / 2) / 1000 * (1000 - s / 2) / 1000; }
                if tc & 4 != 0 { x += x * s * 2 / 1000; }
                if tc & 1 != 0 { x += x * h / 1000; }
                if tc & 2 != 0 { x += x * f / 1000; }
                g.pre[ty * 8 + tc] = x;
                g.zero[ty * 8 + tc] = (x * g.fk / 1000).max(0);
            }
        }
        g
    }
    #[inline]
    pub fn get(&self, k: usize, val: i32) -> i64 {
        if val == 0 { return self.zero[k]; }
        let mut g = self.pre[k];
        let val = val.clamp(-1000, 1000) as i64;
        if val < 0 { g += g * (-val) / 1000 * self.an / 500; } else { g += g * val / 1000 * self.pos / 1000; g -= g * val / 1000 * self.neg / 1000; }
        (g * self.fk / 1000).max(0)
    }
    /// One multiply-shift per link instead of the chained divisions. Not bit-exact.
    #[inline]
    pub fn approx(&self, k: usize, val: i32) -> i64 {
        let v = val.clamp(-1000, 1000) as i64;
        let c = if v < 0 { -v * self.cneg } else { v * self.cpos };
        (self.zero[k] * ((1 << 20) + c) >> 20).max(0)
    }
}



#[derive(Clone, Copy, Debug)]
pub struct Opts { pub select: Select, pub memo: bool, pub fixed: bool, pub gain: GainMode }

/// The per-think gain cache.
pub struct Memo { keys: Vec<u32>, vals: Vec<i32>, last: Option<Mods> }
impl Default for Memo { fn default() -> Self { Memo { keys: vec![u32::MAX; 2048], vals: vec![0; 2048], last: None } } }
impl Memo {
    fn begin(&mut self, m: &Mods) {
        if self.last != Some(*m) {
            self.last = Some(*m);
            self.keys.iter_mut().for_each(|k| *k = u32::MAX);
        }
    }
    #[inline]
    fn get(&mut self, m: &Mods, ty: u8, tags_to: u8, val: i32) -> i32 {
        let v = val.clamp(-1000, 1000);
        let tb = (tags_to & tag::FOOD) | (tags_to & tag::REST) | ((tags_to & tag::ANGER) >> 5);
        let key = (ty as u32) | ((tb as u32) << 3) | (((v + 1000) as u32) << 6);
        let i = (key.wrapping_mul(0x9e37_79b1) >> 21) as usize;
        if self.keys[i] == key { return self.vals[i]; }
        let g = gain(m, ty, tags_to, v);
        self.keys[i] = key;
        self.vals[i] = g;
        g
    }
}

fn select(v: &mut Vec<(u32, i32)>, k: usize, how: Select) {
    let key = |&(n, a): &(u32, i32)| (Reverse(a), n);
    match how {
        Select::Sort => { v.sort_by_key(key); v.truncate(k); }
        Select::Nth => {
            if v.len() > k { v.select_nth_unstable_by_key(k - 1, key); v.truncate(k); }
            v.sort_unstable_by_key(key);
        }
        Select::Heap => {
            if v.len() > k {
                // A max-heap on the sort key holds the k best; its top is the worst kept.
                let mut h: std::collections::BinaryHeap<(Reverse<i32>, u32)> = std::collections::BinaryHeap::with_capacity(k + 1);
                for &(n, a) in v.iter() {
                    let item = (Reverse(a), n);
                    if h.len() < k { h.push(item); } else if item < *h.peek().unwrap() { h.pop(); h.push(item); }
                }
                v.clear();
                v.extend(h.into_sorted_vec().into_iter().map(|(Reverse(a), n)| (n, a)));
            } else { v.sort_unstable_by_key(key); }
        }
    }
}

/// The reference think with the chosen rewrites. With `Opts { select: Sort, memo: false,
/// fixed: false }` it is the reference, line for line. `fresh` (delta input): per-mille of
/// each injection that spreads; `None` spreads all input in full.
#[allow(clippy::too_many_arguments)]
pub fn think_opt<F: Form + ?Sized>(f: &F, st: &mut Activation, m: &Mods, inject: &[(u32, i32)], p: &Params, o: Opts, memo: &mut Memo, fresh: Option<&[i32]>) -> Vec<u32> {
    if o.memo { memo.begin(m); }
    let table = if o.gain == GainMode::Full { None } else { Some(Gain::new(m)) };
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
    for (i, &(n, a)) in inject.iter().enumerate() {
        st.add(n, a, p.max_act);
        let spread = match fresh { Some(fr) => fr[i], None => a };
        if spread <= 0 { continue; }
        if st.inc[n as usize] == 0 { st.touched.push(n); }
        st.inc[n as usize] += spread;
    }
    let mut frontier: Vec<(u32, i32)> = st.touched.iter().map(|&n| (n, st.inc[n as usize])).collect();
    for &n in &st.touched { st.inc[n as usize] = 0; }
    st.touched.clear();
    select(&mut frontier, p.frontier, o.select);

    let mut edges = std::mem::take(&mut st.edges);
    let mut next: Vec<(u32, i32)> = vec![];
    st.visits = 0;
    st.hops_run = 0;
    let dk = p.hop_decay as i64;
    for _hop in 0..p.hops {
        if frontier.is_empty() { break; }
        st.hops_run += 1;
        for &(u, src) in &frontier {
            edges.clear();
            f.edges(u, &mut edges);
            st.visits += edges.len() as u32;
            let out_sum: i64 = edges.iter().map(|e| e.w as i64).sum::<i64>().max(1000);
            let k = ((src as i64) << 24) / out_sum;
            for e in &edges {
                let tv = f.tags(e.to);
                let val = f.val(e.to) + e.val;
                let g = match &table {
                    Some(t) => { let k = e.ty as usize * 8 + tagclass(tv); if o.gain == GainMode::Approx { t.approx(k, val) } else { t.get(k, val) } }
                    None => (if o.memo { memo.get(m, e.ty, tv, val) } else { gain(m, e.ty, tv, val) }) as i64,
                };
                let flow = if o.fixed { ((e.w as i64 * k) >> 24) * g * dk / 1_000_000 } else { src as i64 * e.w as i64 / out_sum * g / 1000 * dk / 1000 } as i32;
                if flow < p.min_act { continue; }
                let slot = &mut st.inc[e.to as usize];
                if *slot == 0 { st.touched.push(e.to); }
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
        select(&mut next, p.frontier, o.select);
        std::mem::swap(&mut frontier, &mut next);
    }
    st.edges = edges;
    finish(st, p)
}
