//! A black-box reconstruction of the oracle's step, written from PROBLEM.md and from probing the
//! oracle's public functions. Dense state, `Form`-based, deliberately plain: it is the semantic
//! reference that the fast kernels are compared against, and it is itself compared against
//! `mc::spread::think` in the tests.

use mc::spread::{gain, Edge, Form, Mods, Params};

/// Sparse canonical state: (node, x) sorted by node, every x > 0.
pub type State = Vec<(u32, i32)>;

/// Order used for every top-k: larger amount first, then smaller id.
#[inline]
pub fn key(n: u32, a: i32) -> u64 { ((!(a as u32 ^ 0x8000_0000)) as u64) << 32 | n as u64 }

pub fn top_k(c: &mut Vec<(u32, i32)>, k: usize) {
    if c.len() > k {
        c.select_nth_unstable_by_key(k - 1, |&(n, a)| key(n, a));
        c.truncate(k);
    }
    c.sort_unstable_by_key(|&(n, a)| key(n, a));
}

pub struct Dense { pub act: Vec<i32>, pub active: Vec<u32>, pub inc: Vec<i32>, pub touched: Vec<u32>, edges: Vec<Edge> }

impl Dense {
    pub fn new(n: usize) -> Self { Dense { act: vec![0; n], active: vec![], inc: vec![0; n], touched: vec![], edges: vec![] } }
    pub fn state(&self) -> State {
        let mut s: State = self.active.iter().map(|&n| (n, self.act[n as usize])).filter(|x| x.1 > 0).collect();
        s.sort_unstable(); s
    }
    fn add(&mut self, n: u32, a: i32, max: i32) {
        let x = &mut self.act[n as usize];
        if *x == 0 && a > 0 { self.active.push(n); }
        *x = x.wrapping_add(a).min(max);
    }
    pub fn damp(&mut self, pm: i32) { for &n in &self.active { let x = &mut self.act[n as usize]; *x = (*x as i64 * pm as i64 / 1000) as i32; } }

    pub fn think<F: Form + ?Sized>(&mut self, f: &F, m: &Mods, inject: &[(u32, i32)], p: &Params) -> Vec<u32> {
        // 1. decay, drop below min
        let act = &mut self.act;
        self.active.retain(|&n| { let x = &mut act[n as usize]; *x = (*x as i64 * p.persist as i64 / 1000) as i32; if *x < p.min_act { *x = 0; false } else { true } });
        // 2. seeds
        let mut cand: Vec<(u32, i32)> = vec![];
        for &n in &self.active { let s = self.act[n as usize] * p.carry_spread / 1000; if s > 0 { cand.push((n, s)); } }
        for &(n, a) in inject { self.add(n, a, p.max_act); cand.push((n, a)); }
        merge(&mut cand);
        top_k(&mut cand, p.frontier);
        // 3. rounds
        for _r in 0..p.hops {
            if cand.is_empty() { break; }
            for &(u, a) in &cand {
                self.edges.clear();
                f.edges(u, &mut self.edges);
                let tot: i32 = self.edges.iter().map(|e| e.w).sum();
                let den = tot.max(1000);
                for e in &self.edges {
                    let mut fl = a as i64 * e.w as i64 / den as i64;
                    fl = fl * gain(m, e.ty, f.tags(e.to), f.val(e.to) + e.val) as i64 / 1000;
                    fl = fl * p.hop_decay as i64 / 1000;
                    let fl = fl as i32;
                    if fl < p.min_act { continue; }
                    let t = e.to as usize;
                    if self.inc[t] == 0 { self.touched.push(e.to); }
                    self.inc[t] = self.inc[t].wrapping_add(fl);
                }
            }
            cand.clear();
            for i in 0..self.touched.len() {
                let t = self.touched[i];
                let v = std::mem::take(&mut self.inc[t as usize]);
                self.add(t, v, p.max_act);
                cand.push((t, v));
            }
            self.touched.clear();
            top_k(&mut cand, p.frontier);
        }
        self.finish(p)
    }

    pub fn finish(&mut self, p: &Params) -> Vec<u32> {
        let mut c: Vec<(u32, i32)> = self.active.iter().map(|&n| (n, self.act[n as usize])).collect();
        let tot: i64 = c.iter().map(|x| x.1 as i64).sum();
        if tot > p.budget as i64 {
            for x in c.iter_mut() { x.1 = (x.1 as i64 * p.budget as i64 / tot) as i32; self.act[x.0 as usize] = x.1; }
            let act = &mut self.act;
            c.retain(|x| if x.1 < p.min_act { act[x.0 as usize] = 0; false } else { true });
        }
        if c.len() > p.active_cap {
            c.select_nth_unstable_by_key(p.active_cap - 1, |&(n, a)| key(n, a));
            for &(n, _) in &c[p.active_cap..] { self.act[n as usize] = 0; }
            c.truncate(p.active_cap);
        }
        c.sort_unstable();
        self.active.clear();
        self.active.extend(c.iter().map(|x| x.0));
        let mut t: Vec<(u32, i32)> = c.into_iter().filter(|x| x.1 > 0).collect();
        top_k(&mut t, p.ws_k);
        t.into_iter().map(|x| x.0).collect()
    }
}

/// Merge duplicate ids by summing.
pub fn merge(c: &mut Vec<(u32, i32)>) {
    c.sort_unstable_by_key(|x| x.0);
    let mut w = 0;
    for r in 0..c.len() {
        if w > 0 && c[w - 1].0 == c[r].0 { c[w - 1].1 += c[r].1; } else { c[w] = c[r]; w += 1; }
    }
    c.truncate(w);
}

/// A stable 64-bit hash of (state, output).
pub fn hash(s: &State, out: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u64| { h ^= v; h = h.wrapping_mul(0x0000_0100_0000_01b3); h ^= h >> 29; };
    for &(n, a) in s { mix(n as u64); mix(a as u64); }
    mix(0xffff_ffff);
    for &n in out { mix(n as u64); }
    h
}
