//! The brick-pool diffusion (brief 10's layout) on the dead tree: one phase
//! compared cell by cell, best-of-REPS timings next to the dense version's,
//! then TICKS whole ticks compared by hash.
//!
//!     VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json cargo run --release -p fv-gpu --example pool_diffuse
use fv_core::rad::Sky;
use fv_core::scene::crib_and_tree;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::units::*;
use fv_gpu::pool::{shuffled, PoolDiffusion};
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
    let mut pool = PoolDiffusion::from_world(&sim.world, shuffled(n, 7)).expect("GPU");
    let mut dense = fv_gpu::GpuDiffusion::new(&sim.world).expect("GPU");
    println!("adapter: {} ({:?})", pool.adapter_info.name, pool.adapter_info.device_type);
    let mut cpu_w = sim.world.clone();
    fv_core::transport::diffuse(&mut cpu_w, &sim.flow);
    let mut w = sim.world.clone();
    pool.diffuse(&mut w, &sim.flow);
    let bad = (0..w.cells.len()).filter(|&i| w.cells[i] != cpu_w.cells[i]).count();
    let tb = (0..w.tallies.len()).filter(|&b| w.tallies[b] != cpu_w.tallies[b]).count();
    println!("tick {at}: {} cells, {bad} differ; {tb} of {n} tallies differ", w.cells.len());
    let (mut pk, mut dk) = ([f64::MAX; 2], [f64::MAX; 4]);
    let (mut pt, mut dt) = (f64::MAX, f64::MAX);
    for _ in 0..reps {
        let mut w = sim.world.clone();
        pool.diffuse(&mut w, &sim.flow);
        for j in 0..2 { pk[j] = pk[j].min(pool.kernel_ms[j]) }
        pt = pt.min(pool.last.total());
        let mut w = sim.world.clone();
        dense.diffuse(&mut w, &sim.flow);
        dt = dt.min(dense.last.total());
        let k = dense.kernel_times();
        for j in 0..4 { dk[j] = dk[j].min(k[j]) }
    }
    println!("pool kernels (ms): limits pass {:.1} + apply pass {:.1} = {:.1}; phase with transfers {pt:.1}", pk[0], pk[1], pk[0] + pk[1]);
    println!("dense kernels (ms): faces {:.1} + limits {:.1} + bricks {:.1} + tallies {:.1} = {:.1}; phase with transfers {dt:.1}", dk[0], dk[1], dk[2], dk[3], dk.iter().sum::<f64>());
    let mut b = scene();
    for _ in 0..at { b.step() }
    let g = Arc::new(Mutex::new(pool));
    let g2 = g.clone();
    b.diffuse_hook = Some(Box::new(move |w, flow| g2.lock().unwrap().diffuse(w, flow)));
    let mut same = 0;
    for _ in 0..ticks { sim.step(); b.step(); if sim.hash() == b.hash() { same += 1 } }
    println!("{ticks} whole ticks: {same} hashes equal; gaps {} {}", b.energy_gap(), b.mass_gap());
}
