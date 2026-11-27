//! The diffusion phase on the GPU against the CPU, on the dead tree:
//! one phase compared cell by cell at tick AT (default 30), then TICKS
//! (default 20) whole ticks with the GPU phase in the sim, hashes compared
//! every tick.
//!
//!     VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json cargo run --release -p fv-gpu --example gpu_diffuse
use fv_core::rad::Sky;
use fv_core::scene::crib_and_tree;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::units::*;
use fv_gpu::{GpuDiffusion, Timings};
use std::sync::{Arc, Mutex};
use std::time::Instant;

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
    let mut gpu = GpuDiffusion::new(&sim.world).expect("GPU");
    println!("adapter: {} ({:?}, {:?})", gpu.adapter_info.name, gpu.adapter_info.backend, gpu.adapter_info.device_type);

    // One phase, cell by cell.
    let mut cpu_w = sim.world.clone();
    let t0 = Instant::now();
    fv_core::transport::diffuse(&mut cpu_w, &sim.flow);
    let cpu_ms = t0.elapsed().as_secs_f64() * 1e3;
    let mut gpu_w = sim.world.clone();
    gpu.diffuse(&mut gpu_w, &sim.flow);
    let bad = (0..cpu_w.cells.len()).filter(|&i| cpu_w.cells[i] != gpu_w.cells[i]).count();
    let tally_bad = (0..cpu_w.tallies.len()).filter(|&b| cpu_w.tallies[b] != gpu_w.tallies[b]).count();
    if let Some(i) = (0..cpu_w.cells.len()).find(|&i| cpu_w.cells[i] != gpu_w.cells[i]) {
        println!("first differing cell {i}:\n cpu {:?}\n gpu {:?}", cpu_w.cells[i], gpu_w.cells[i]);
    }
    println!("tick {at}: {} cells, {bad} differ; {} bricks, {tally_bad} tallies differ; hash cpu {:016x} gpu {:016x}", cpu_w.cells.len(), cpu_w.tallies.len(), cpu_w.hash(), gpu_w.hash());

    // Timings: best of `reps`.
    let mut best = Timings { prepare: f64::MAX, upload: f64::MAX, compute: f64::MAX, readback: f64::MAX, unpack: f64::MAX };
    let mut best_total = f64::MAX;
    for _ in 0..reps {
        let mut w = sim.world.clone();
        gpu.diffuse(&mut w, &sim.flow);
        let t = gpu.last;
        best.prepare = best.prepare.min(t.prepare); best.upload = best.upload.min(t.upload); best.compute = best.compute.min(t.compute);
        best.readback = best.readback.min(t.readback); best.unpack = best.unpack.min(t.unpack); best_total = best_total.min(t.total());
    }
    let mut kt = [f64::MAX; 4];
    for _ in 0..reps { let k = gpu.kernel_times(); for j in 0..4 { kt[j] = kt[j].min(k[j]) } }
    println!("kernels alone (ms): faces {:.1} | limits {:.1} | bricks {:.1} | tallies {:.1}", kt[0], kt[1], kt[2], kt[3]);
    let mut cpu_best = cpu_ms;
    for _ in 0..reps { let mut w = sim.world.clone(); let t0 = Instant::now(); fv_core::transport::diffuse(&mut w, &sim.flow); cpu_best = cpu_best.min(t0.elapsed().as_secs_f64() * 1e3) }
    println!("best of {reps} (ms): GPU phase {best_total:.1} = prepare {:.1} + upload {:.1} + compute {:.1} + readback {:.1} + unpack {:.1}; CPU phase {cpu_best:.1}", best.prepare, best.upload, best.compute, best.readback, best.unpack);

    // Whole ticks: the sim with the GPU phase in place of the CPU's.
    let mut a = sim;
    let mut b = scene();
    for _ in 0..at { b.step() }
    let g = Arc::new(Mutex::new(gpu));
    let g2 = g.clone();
    b.diffuse_hook = Some(Box::new(move |w, flow| g2.lock().unwrap().diffuse(w, flow)));
    let mut same = 0;
    for t in 0..ticks {
        a.step();
        b.step();
        if a.hash() == b.hash() { same += 1 } else { println!("tick {}: hashes differ", at + t + 1) }
    }
    println!("{ticks} whole ticks from tick {at}: {same} hashes equal; GPU run energy gap {} mass gap {} impossible {:?}", b.energy_gap(), b.mass_gap(), b.world.first_impossible());
}
