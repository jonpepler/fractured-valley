//! Benchmark plumbing: instance sets, lockstep drivers, order-independent hashes, timing.

use crate::delta::Delta;
use crate::exact::{Inst, Scratch};
use crate::harness::{self, Species};
use crate::model;
use crate::shared::Shared;
use mc::gen::{OverlaySpec, Tick};
use mc::spread::Params;
use std::sync::Barrier;
use std::time::Instant;

pub struct Set { pub sh: Shared, pub deltas: Vec<Delta>, pub ticks: Vec<Vec<Tick>> }

/// M instances over one species of N nodes, each with a private overlay (scaled by num/den) and workload.
pub fn make_set(sp: &Species, m: usize, t: usize, num: u32, den: u32, threads: usize) -> Set {
    let sh = Shared::new(&sp.base);
    let spec = OverlaySpec::DEFAULT.scaled(num, den);
    let mut deltas: Vec<Delta> = vec![Delta::default(); m];
    let chunk = m.div_ceil(threads.max(1));
    std::thread::scope(|s| {
        for (c, part) in deltas.chunks_mut(chunk).enumerate() {
            let (sh, sp) = (&sh, sp);
            s.spawn(move || for (i, d) in part.iter_mut().enumerate() {
                let who = (c * chunk + i) as u32;
                *d = Delta::new(sh, &mc::gen::overlay(&sp.base, &sp.info, spec, 0x5eed, who));
            });
        }
    });
    let ticks = (0..m as u32).map(|who| harness::ticks(sp, who, 0x7157, t)).collect();
    Set { sh, deltas, ticks }
}

/// Order-independent combination of per-(instance, tick) hashes.
#[inline]
pub fn fold(acc: u64, who: u32, tick: u32, h: u64) -> u64 {
    let mut z = h ^ (who as u64) << 32 ^ tick as u64 ^ 0x9e37_79b9_7f4a_7c15;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    acc.wrapping_add(z ^ (z >> 31))
}

pub struct Run { pub secs: f64, pub hash: u64, pub visits: u64 }

/// Exact kernel, lockstep: every tick, all instances step; instances split over threads, a
/// barrier per tick. `order` permutes the instance processing order (for the order test).
pub fn run_exact(set: &Set, p: &Params, threads: usize, order: Option<&[u32]>) -> Run { run_kernel(set, p, threads, order, crate::exact::step) }

pub type StepFn = fn(&Shared, &Delta, &mut Inst, &mc::spread::Mods, &[(u32, i32)], &Params, &mut Scratch) -> Vec<u32>;

pub fn run_kernel(set: &Set, p: &Params, threads: usize, order: Option<&[u32]>, stepf: StepFn) -> Run {
    let m = set.deltas.len();
    let ids: Vec<u32> = order.map(|o| o.to_vec()).unwrap_or_else(|| (0..m as u32).collect());
    let mut insts: Vec<(u32, Inst)> = ids.iter().map(|&i| (i, Inst::new())).collect();
    let nt = set.ticks[0].len();
    let threads = threads.max(1).min(m);
    let chunk = m.div_ceil(threads);
    let bar = Barrier::new(threads);
    let t0 = Instant::now();
    let res: Vec<(u64, u64)> = std::thread::scope(|s| {
        let hs: Vec<_> = insts.chunks_mut(chunk).map(|part| {
            let (bar, set) = (&bar, set);
            s.spawn(move || {
                let mut sc = Scratch::new();
                let mut acc = 0u64;
                for t in 0..nt {
                    for (who, inst) in part.iter_mut() {
                        let tk = &set.ticks[*who as usize][t];
                        if tk.damp != 1000 { inst.damp(tk.damp); }
                        let out = stepf(&set.sh, &set.deltas[*who as usize], inst, &tk.mods, &tk.inject, p, &mut sc);
                        acc = fold(acc, *who, t as u32, model::hash(&inst.state(), &out));
                    }
                    bar.wait();
                }
                (acc, sc.visits)
            })
        }).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let secs = t0.elapsed().as_secs_f64();
    Run { secs, hash: res.iter().fold(0u64, |a, r| a.wrapping_add(r.0)), visits: res.iter().map(|r| r.1).sum() }
}

/// The `fast` kernel under the same lockstep driver.
pub fn run_fast(set: &Set, p: &Params, threads: usize, order: Option<&[u32]>) -> Run {
    let m = set.deltas.len();
    let ids: Vec<u32> = order.map(|o| o.to_vec()).unwrap_or_else(|| (0..m as u32).collect());
    let mut insts: Vec<(u32, Inst)> = ids.iter().map(|&i| (i, Inst::new())).collect();
    let nt = set.ticks[0].len();
    let threads = threads.max(1).min(m);
    let chunk = m.div_ceil(threads);
    let bar = Barrier::new(threads);
    let t0 = Instant::now();
    let res: Vec<u64> = std::thread::scope(|s| {
        let hs: Vec<_> = insts.chunks_mut(chunk).map(|part| {
            let (bar, set) = (&bar, set);
            s.spawn(move || {
                let mut sc = crate::fast::Scratch::new();
                let mut acc = 0u64;
                for t in 0..nt {
                    for (who, inst) in part.iter_mut() {
                        let tk = &set.ticks[*who as usize][t];
                        if tk.damp != 1000 { inst.damp(tk.damp); }
                        let (_, h) = crate::fast::step(&set.sh, &set.deltas[*who as usize], inst, &tk.mods, &tk.inject, p, &mut sc);
                        acc = fold(acc, *who, t as u32, h);
                    }
                    bar.wait();
                }
                acc
            })
        }).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    Run { secs: t0.elapsed().as_secs_f64(), hash: res.iter().fold(0u64, |a, r| a.wrapping_add(*r)), visits: 0 }
}

/// Oracle over materialised graphs (one instance at a time; dense state, so small M only).
pub fn run_oracle(sp: &Species, set: &Set, p: &Params, num: u32, den: u32, m: usize) -> Run {
    let spec = OverlaySpec::DEFAULT.scaled(num, den);
    let mut acc = 0u64; let mut secs = 0.0;
    for who in 0..m as u32 {
        let g = mc::gen::materialise(&sp.base, &mc::gen::overlay(&sp.base, &sp.info, spec, 0x5eed, who));
        let t0 = Instant::now();
        let hs = harness::run_oracle(&g, &set.ticks[who as usize], p);
        secs += t0.elapsed().as_secs_f64();
        for (t, h) in hs.into_iter().enumerate() { acc = fold(acc, who, t as u32, h); }
    }
    Run { secs, hash: acc, visits: 0 }
}

/// Minimum time of `passes` runs; panics unless every pass hashes the same.
pub fn best(passes: usize, mut f: impl FnMut() -> Run) -> Run {
    let mut b: Option<Run> = None;
    for _ in 0..passes {
        let r = f();
        if let Some(x) = &b { assert_eq!(x.hash, r.hash, "hash changed between passes"); }
        if b.as_ref().is_none_or(|x| r.secs < x.secs) { b = Some(r); }
    }
    b.unwrap()
}
