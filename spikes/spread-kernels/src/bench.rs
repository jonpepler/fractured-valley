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
pub fn run_fast(set: &Set, p: &Params, threads: usize, order: Option<&[u32]>) -> Run { run_fastlike(set, p, threads, order, crate::fast::step) }

pub type FastFn = fn(&Shared, &Delta, &mut Inst, &mc::spread::Mods, &[(u32, i32)], &Params, &mut crate::fast::Scratch) -> (Vec<u32>, u64);

pub fn run_fastlike(set: &Set, p: &Params, threads: usize, order: Option<&[u32]>, stepf: FastFn) -> Run {
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
                        let (_, h) = stepf(&set.sh, &set.deltas[*who as usize], inst, &tk.mods, &tk.inject, p, &mut sc);
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

/// Generic lockstep driver: `f(who, inst, tick, scratch)` damps and steps one instance and
/// returns its hash. Instances are split over threads, one barrier per tick.
pub fn run_gen<S, M, F>(set: &Set, threads: usize, order: Option<&[u32]>, mk: M, f: F) -> Run
where M: Fn() -> S + Sync, F: Fn(usize, &mut Inst, &Tick, &mut S) -> u64 + Sync {
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
            let (bar, set, mk, f) = (&bar, set, &mk, &f);
            s.spawn(move || {
                let mut sc = mk();
                let mut acc = 0u64;
                for t in 0..nt {
                    for (who, inst) in part.iter_mut() {
                        let h = f(*who as usize, inst, &set.ticks[*who as usize][t], &mut sc);
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

pub const VARIANTS: [&str; 5] = ["exact-v1", "exact-fast", "exact-pruned", "redef", "redef-lazy"];

/// Run a named variant.
pub fn run_variant(name: &str, set: &Set, p: &Params, threads: usize, order: Option<&[u32]>) -> Run {
    let classes = crate::redef::classes(&set.sh, &set.deltas);
    match name {
        "exact-v1" => run_gen(set, threads, order, crate::exact::Scratch::new, |who, inst, tk, sc| {
            if tk.damp != 1000 { inst.damp(tk.damp); }
            let out = crate::exact::step(&set.sh, &set.deltas[who], inst, &tk.mods, &tk.inject, p, sc);
            crate::fast::state_hash(&inst.st, &out)
        }),
        "exact-fast" | "exact-pruned" => {
            let f: FastFn = if name == "exact-fast" { crate::fast::step } else { crate::fast::step_pruned };
            run_gen(set, threads, order, crate::fast::Scratch::new, |who, inst, tk, sc| {
                if tk.damp != 1000 { inst.damp(tk.damp); }
                f(&set.sh, &set.deltas[who], inst, &tk.mods, &tk.inject, p, sc).1
            })
        }
        "redef" => run_gen(set, threads, order, crate::redef::Scratch::new, |who, inst, tk, sc| {
            if tk.damp != 1000 { crate::redef::damp(inst, tk.damp); }
            crate::redef::step(&set.sh, &set.deltas[who], inst, &tk.mods, &tk.inject, p, classes, sc).1
        }),
        "redef-lazy" => run_gen(set, threads, order, crate::redef::Scratch::new, |who, inst, tk, sc| {
            if tk.damp != 1000 { crate::redef::damp(inst, tk.damp); }
            crate::redef::step_lazy(&set.sh, &set.deltas[who], inst, &tk.mods, &tk.inject, p, sc).1
        }),
        _ => panic!("unknown variant {name}"),
    }
}

/// Counters from one single-threaded pass: (edges visited, nodes expanded, mean state entries) per step.
pub fn counters(name: &str, set: &Set, p: &Params) -> (f64, f64, f64) {
    let classes = crate::redef::classes(&set.sh, &set.deltas);
    let (mut st, mut steps) = (0u64, 0u64);
    let mut fs = crate::fast::Scratch::new(); let mut rs = crate::redef::Scratch::new();
    for (who, d) in set.deltas.iter().enumerate() {
        let mut inst = Inst::new();
        for tk in &set.ticks[who] {
            steps += 1;
            if name.starts_with("redef") {
                if tk.damp != 1000 { crate::redef::damp(&mut inst, tk.damp); }
                crate::redef::step(&set.sh, d, &mut inst, &tk.mods, &tk.inject, p, classes, &mut rs);
            } else {
                if tk.damp != 1000 { inst.damp(tk.damp); }
                if name == "exact-pruned" { crate::fast::step_pruned(&set.sh, d, &mut inst, &tk.mods, &tk.inject, p, &mut fs); }
                else { crate::fast::step(&set.sh, d, &mut inst, &tk.mods, &tk.inject, p, &mut fs); }
            }
            st += inst.st.len() as u64;
        }
    }
    let (vv, ee) = if name.starts_with("redef") { (rs.visits, rs.expanded) } else { (fs.visits, fs.expanded) };
    (vv as f64 / steps as f64, ee as f64 / steps as f64, st as f64 / steps as f64)
}
