//! Instance generation and the oracle driver.
use mc::gen::{self, Info, Overlay, OverlaySpec, Tick};
use mc::graph::Graph;
use mc::spread::{think, Activation, Params};
use crate::model::{self, State};

pub struct Species { pub base: Graph, pub info: Info }

pub fn species(n: usize, seed: u64) -> Species { let (base, info) = gen::species(n, seed); Species { base, info } }

pub fn overlay(s: &Species, who: u32, seed: u64) -> Overlay { gen::overlay(&s.base, &s.info, OverlaySpec::DEFAULT, seed, who) }

/// Ticks for instance `who`. Workloads are generated over the base graph with a per-instance seed.
pub fn ticks(s: &Species, who: u32, seed: u64, t: usize) -> Vec<Tick> {
    gen::workload(&s.base, &s.info, seed ^ (who as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15), t)
}

pub fn oracle_state(st: &Activation) -> State {
    let mut s: State = st.active.iter().map(|&n| (n, st.act[n as usize])).filter(|x| x.1 > 0).collect();
    s.sort_unstable(); s
}

/// Run the oracle over one materialised instance; returns the per-tick hashes.
pub fn run_oracle(g: &Graph, ticks: &[Tick], p: &Params) -> Vec<u64> {
    let mut st = Activation::new(g.len());
    ticks.iter().map(|t| {
        if t.damp != 1000 { st.damp(t.damp); }
        let out = think(g, &mut st, &t.mods, &t.inject, p);
        model::hash(&oracle_state(&st), &out)
    }).collect()
}

pub fn run_model(g: &Graph, ticks: &[Tick], p: &Params) -> Vec<u64> {
    let mut st = model::Dense::new(g.len());
    ticks.iter().map(|t| {
        if t.damp != 1000 { st.damp(t.damp); }
        let out = st.think(g, &t.mods, &t.inject, p);
        model::hash(&st.state(), &out)
    }).collect()
}

/// An adversarial small instance: few nodes, many parallel edges, coarse weights (so many ties),
/// large gains, saturating injections.
pub fn adversarial(seed: u64) -> (Graph, Vec<Tick>) {
    use crate::rand::Rng;
    use mc::graph::Builder;
    use mc::spread::Mods;
    let mut r = Rng(seed);
    let n = if r.below(4) == 0 { r.range(256, 3000) } else { r.range(2, 300) } as u32;
    let extreme = r.below(3) == 0;
    let mut b = Builder::default();
    for _ in 0..n { let v = r.range(-1000, 1000) as i32 / 100 * 100; b.node(v, r.below(256) as u8 & !64, None); }
    let deg = r.range(1, 30);
    for s in 0..n {
        let d = r.below(deg as u64 + 1);
        for _ in 0..d {
            let t = if r.below(4) == 0 { r.below(n as u64) as u32 } else { (s + r.below(8) as u32) % n };
            let w = [1, 10, 100, 250, 500, 1000][r.below(6) as usize] * if r.below(20) == 0 { 0 } else { 1 };
            let ev = if r.below(10) == 0 { r.range(-1000, 1000) as i32 } else { 0 };
            b.link(s, t, r.below(8) as u8, w, ev);
        }
    }
    let g = b.build();
    let ticks = (0..r.range(1, 80)).map(|_| {
        let mut pick = |lo: i64, hi: i64| if extreme { if r.below(2) == 0 { lo } else { hi } } else { r.range(lo, hi) } as i32;
        let mods = Mods { hunger: pick(0, 1000), arousal: pick(0, 1000), mood: pick(-1000, 1000), fatigue: pick(0, 1000), stress: pick(0, 1000) };
        let k = r.below(25);
        let inject = (0..k).map(|_| (r.below(n as u64) as u32, [1, 5, 6, 7, 100, 300, 1000, 3999, 4000, 5000, 20000, 100000][r.below(12) as usize])).collect();
        let damp = if r.below(5) == 0 { r.range(0, 1000) as i32 } else { 1000 };
        Tick { mods, inject, damp }
    }).collect();
    (g, ticks)
}

/// Run the exact sparse kernel over one instance; per-tick hashes.
pub fn run_exact(sh: &crate::shared::Shared, d: crate::delta::Delta, ticks: &[Tick], p: &Params) -> Vec<u64> {
    let mut inst = crate::exact::Inst::new();
    let mut sc = crate::exact::Scratch::new();
    ticks.iter().map(|t| {
        if t.damp != 1000 { inst.damp(t.damp); }
        let out = crate::exact::step(sh, &d, &mut inst, &t.mods, &t.inject, p, &mut sc);
        model::hash(&inst.state(), &out)
    }).collect()
}

/// Run any kernel with the `fast` step signature over one instance; per-tick hashes.
pub fn run_with(sh: &crate::shared::Shared, d: &crate::delta::Delta, ticks: &[Tick], p: &Params,
    mut stepf: impl FnMut(&crate::shared::Shared, &crate::delta::Delta, &mut crate::exact::Inst, &Tick, &Params) -> u64) -> Vec<u64> {
    let mut inst = crate::exact::Inst::new();
    ticks.iter().map(|t| { if t.damp != 1000 { inst.damp(t.damp); } stepf(sh, d, &mut inst, t, p) }).collect()
}

pub fn run_fast(sh: &crate::shared::Shared, d: &crate::delta::Delta, ticks: &[Tick], p: &Params) -> Vec<u64> {
    let mut sc = crate::fast::Scratch::new();
    run_with(sh, d, ticks, p, |sh, d, inst, t, p| crate::fast::step(sh, d, inst, &t.mods, &t.inject, p, &mut sc).1)
}

/// Instances built to overflow i32: layered graphs of maximum-gain nodes, extreme modifiers and
/// huge injections. Exercises the wrap-around paths (negative x, ghosts, duplicate entries).
pub fn overflowing(seed: u64) -> (Graph, Vec<Tick>) {
    use crate::rand::Rng;
    use mc::graph::Builder;
    use mc::spread::Mods;
    let mut r = Rng(seed ^ 0xdead_beef);
    let width = r.range(1, 6) as u32;
    let layers = r.range(3, 7) as u32;
    let mut b = Builder::default();
    for _ in 0..width * layers { b.node(if r.below(4) == 0 { r.range(-1000, 1000) as i32 } else { -1000 }, if r.below(3) == 0 { r.below(256) as u8 } else { 131 }, None); }
    for l in 0..layers {
        for i in 0..width { for j in 0..width {
            let to = if l + 1 < layers { (l + 1) * width + j } else if r.below(2) == 0 { j } else { continue };
            b.link(l * width + i, to, [0, 0, 7, 4][r.below(4) as usize], [1000 / width as i32, 1000, 250, 1][r.below(4) as usize], 0);
        } }
        if r.below(3) == 0 { let s = r.below((width * layers) as u64) as u32; b.link(l * width, s, 0, 1000, 0); }
    }
    let g = b.build();
    let n = g.len() as u64;
    let ticks = (0..r.range(1, 40)).map(|_| {
        let mods = Mods { hunger: 1000, arousal: 1000, mood: -1000, fatigue: 1000, stress: r.range(0, 1000) as i32 };
        let inject = (0..r.range(0, 4)).map(|_| (r.below(n) as u32, [1000, 4000, 20000, 100000, 1_000_000][r.below(5) as usize])).collect();
        let damp = if r.below(4) == 0 { r.range(0, 1000) as i32 } else { 1000 };
        Tick { mods, inject, damp }
    }).collect();
    (g, ticks)
}
