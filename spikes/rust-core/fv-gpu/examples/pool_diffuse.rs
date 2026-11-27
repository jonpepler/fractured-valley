//! The brick-pool diffusion (brief 10's layout) on the dead tree, in each of
//! its variants (faces in workgroup memory, or in a global scratch; i64 or
//! compact u32 gas masses): one phase compared cell by cell, best-of-REPS
//! kernel times next to the dense version's, then TICKS whole ticks.
//!
//!     cargo run --release -p fv-gpu --example pool_diffuse
//! (on a machine without a GPU: VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json)
use fv_core::rad::Sky;
use fv_core::scene::crib_and_tree;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::units::*;
use fv_gpu::pool::{shuffled, Mode, PoolDiffusion};
use std::sync::{Arc, Mutex};

fn scene() -> Sim {
    let (w, torch) = crib_and_tree(4, 48, 48, true);
    let mut sim = Sim::new(w, Sky::afternoon());
    for t in 0..200 { for (i, p) in torch.iter().enumerate().filter(|(i, _)| i % 3 == 0) { sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x: p[0] as u16, y: p[1] as u16, z: p[2] as u16, energy: joules(500) } }) } }
    sim
}

fn main() {
    let env = |k: &str, d: u64| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let (at, ticks, reps) = (env("AT", 30), env("TICKS", 20), env("REPS", 10) as usize);
    let mut sim = scene();
    for _ in 0..at { sim.step() }
    let n = sim.world.bricks();
    let cells = sim.world.cells.len() as f64;
    let mut cpu_w = sim.world.clone();
    fv_core::transport::diffuse(&mut cpu_w, &sim.flow);
    let mut dense = fv_gpu::GpuDiffusion::new(&sim.world).expect("GPU");
    println!("adapter: {} ({:?}); {} cells", dense.adapter_info.name, dense.adapter_info.device_type, cells);
    let mut dk = [f64::MAX; 4];
    for _ in 0..reps { let mut w = sim.world.clone(); dense.diffuse(&mut w, &sim.flow); let k = dense.kernel_times(); for j in 0..4 { dk[j] = dk[j].min(k[j]) } }
    println!("dense                      kernels {:6.1} ms  (faces {:.1}, limits {:.1}, apply {:.1}, tallies {:.1})", dk.iter().sum::<f64>(), dk[0], dk[1], dk[2], dk[3]);
    for (mode, compact) in [(Mode::Workgroup, false), (Mode::Scratch, false), (Mode::Scratch, true)] {
        let mut pool = PoolDiffusion::with_mode(&sim.world, shuffled(n, 7), mode, compact).expect("GPU");
        let mut w = sim.world.clone();
        pool.diffuse(&mut w, &sim.flow);
        let bad = (0..w.cells.len()).filter(|&i| w.cells[i] != cpu_w.cells[i]).count() + (0..n).filter(|&b| w.tallies[b] != cpu_w.tallies[b]).count();
        let mut best = vec![f64::MAX; pool.kernel_ms.len()];
        for _ in 0..reps { let mut w = sim.world.clone(); pool.diffuse(&mut w, &sim.flow); for j in 0..best.len() { best[j] = best[j].min(pool.kernel_ms[j]) } }
        let total: f64 = best.iter().sum();
        let label = format!("pool {:?}{}", mode, if compact { ", compact" } else { "" });
        println!("{label:26} kernels {total:6.1} ms  ({}); {bad} cells/tallies differ", best.iter().map(|x| format!("{x:.1}")).collect::<Vec<_>>().join(" + "));
        if mode == Mode::Scratch && compact && ticks > 0 {
            let mut a = sim.clone_for_test();
            let mut b = sim.clone_for_test();
            let g = Arc::new(Mutex::new(pool));
            b.diffuse_hook = Some(Box::new(move |w, flow| g.lock().unwrap().diffuse(w, flow)));
            let mut same = 0;
            for _ in 0..ticks { a.step(); b.step(); if a.hash() == b.hash() { same += 1 } }
            println!("  {ticks} whole ticks: {same} hashes equal; gaps {} {}", b.energy_gap(), b.mass_gap());
        }
    }
}

trait CloneForTest { fn clone_for_test(&self) -> Sim; }
impl CloneForTest for Sim {
    // A fresh run to the same tick (Sim holds boxed hooks, so it isn't Clone).
    fn clone_for_test(&self) -> Sim {
        let mut s = scene();
        while s.world.tick < self.world.tick { s.step() }
        s
    }
}
