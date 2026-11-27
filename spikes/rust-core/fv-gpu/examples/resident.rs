//! Diffusion and carrying on the GPU for whole ticks of the dead tree, the
//! state staying on the GPU between the two phases: hashes against the
//! CPU every tick, and where the time goes.
//!
//!     cargo run --release -p fv-gpu --example resident
//! (no GPU: VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json)
use fv_core::rad::Sky;
use fv_core::scene::crib_and_tree;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::units::*;
use fv_gpu::pool::{shuffled, Mode, PoolDiffusion};
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
    let (at, ticks) = (env("AT", 30), env("TICKS", 10));
    let (mut a, mut b) = (scene(), scene());
    for _ in 0..at { a.step(); b.step() }
    let n = b.world.bricks();
    let mut pool = PoolDiffusion::with_mode(&b.world, shuffled(n, 3), Mode::Scratch, true).expect("GPU");
    pool.carry_once = std::env::var("CARRY_TWICE").is_err();
    println!("adapter: {} ({:?})", pool.adapter_info.name, pool.adapter_info.device_type);
    let g = Arc::new(Mutex::new(pool));
    let (g1, g2) = (g.clone(), g.clone());
    let log: Arc<Mutex<Vec<(String, f64, Vec<f64>, fv_gpu::Timings)>>> = Arc::new(Mutex::new(Vec::new()));
    let (l1, l2) = (log.clone(), log.clone());
    b.diffuse_hook = Some(Box::new(move |w, flow| { let mut p = g1.lock().unwrap(); let t0 = Instant::now(); p.diffuse(w, flow); l1.lock().unwrap().push(("diffuse".into(), t0.elapsed().as_secs_f64() * 1e3, p.kernel_ms.clone(), p.last)) }));
    b.advect_hook = Some(Box::new(move |w, flow, steps| { let mut p = g2.lock().unwrap(); let t0 = Instant::now(); p.advect(w, flow, steps); l2.lock().unwrap().push(("advect".into(), t0.elapsed().as_secs_f64() * 1e3, p.kernel_ms.clone(), p.last)) }));
    let mut same = 0;
    let mut cpu_adv = f64::MAX;
    for _ in 0..ticks {
        // The CPU's carrying alone, timed on a copy of the state it starts from.
        a.step();
        b.step();
        if a.hash() == b.hash() { same += 1 }
    }
    for _ in 0..5 {
        let mut w = a.world.clone();
        let t0 = Instant::now();
        fv_core::transport::advect(&mut w, &a.flow, a.advect_steps);
        cpu_adv = cpu_adv.min(t0.elapsed().as_secs_f64() * 1e3);
    }
    println!("{ticks} ticks from tick {at}: {same} hashes equal; gaps {} {}", b.energy_gap(), b.mass_gap());
    let log = log.lock().unwrap();
    for name in ["diffuse", "advect"] {
        let rows: Vec<_> = log.iter().filter(|r| r.0 == name).collect();
        let best = |f: &dyn Fn(&(String, f64, Vec<f64>, fv_gpu::Timings)) -> f64| rows.iter().map(|r| f(r)).fold(f64::MAX, f64::min);
        let kernels = rows.iter().map(|r| r.2.clone()).min_by(|x, y| x.iter().sum::<f64>().partial_cmp(&y.iter().sum::<f64>()).unwrap()).unwrap();
        println!("{name:8} best (ms): total {:.1} | kernels {:.1} ({}) | upload {:.1} | readback {:.1} | CPU packing {:.1} + unpacking {:.1}",
            best(&|r| r.1), kernels.iter().sum::<f64>(), kernels.iter().map(|x| format!("{x:.1}")).collect::<Vec<_>>().join(" + "),
            best(&|r| r.3.upload), best(&|r| r.3.readback), best(&|r| r.3.prepare), best(&|r| r.3.unpack));
    }
    println!("CPU advect (4 threads if parallel): {cpu_adv:.1} ms");
}
