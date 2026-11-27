//! Diffusion, the flow, carrying and reaction on the GPU for whole ticks of the dead
//! tree, the state staying on the GPU from one phase to the next: hashes
//! against the CPU every tick, and where the time goes.
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
    // The state comes back once a tick, after the reaction (EAGER=1: after every phase).
    pool.lazy = std::env::var("EAGER").is_err();
    println!("adapter: {} ({:?})", pool.adapter_info.name, pool.adapter_info.device_type);
    let g = Arc::new(Mutex::new(pool));
    let (g1, g2, g3, g4) = (g.clone(), g.clone(), g.clone(), g.clone());
    let log: Arc<Mutex<Vec<(String, f64, Vec<f64>, fv_gpu::Timings)>>> = Arc::new(Mutex::new(Vec::new()));
    let (l1, l2, l3, l4) = (log.clone(), log.clone(), log.clone(), log.clone());
    b.flow_hook = Some(Box::new(move |w, flow| { let mut p = g4.lock().unwrap(); let t0 = Instant::now(); p.flow_step(w, flow); l4.lock().unwrap().push(("flow".into(), t0.elapsed().as_secs_f64() * 1e3, p.kernel_ms.clone(), p.last)) }));
    b.diffuse_hook = Some(Box::new(move |w, flow| { let mut p = g1.lock().unwrap(); let t0 = Instant::now(); p.diffuse(w, flow); l1.lock().unwrap().push(("diffuse".into(), t0.elapsed().as_secs_f64() * 1e3, p.kernel_ms.clone(), p.last)) }));
    b.advect_hook = Some(Box::new(move |w, flow, steps| { let mut p = g2.lock().unwrap(); let t0 = Instant::now(); p.advect(w, flow, steps); l2.lock().unwrap().push(("advect".into(), t0.elapsed().as_secs_f64() * 1e3, p.kernel_ms.clone(), p.last)) }));
    b.react_hook = Some(Box::new(move |w, flow| { let mut p = g3.lock().unwrap(); let t0 = Instant::now(); p.react(w, flow); l3.lock().unwrap().push(("react".into(), t0.elapsed().as_secs_f64() * 1e3, p.kernel_ms.clone(), p.last)) }));
    let mut same = 0;
    let (mut gpu_tick, mut cpu_tick) = (Vec::new(), Vec::new());
    let (mut cpu_adv, mut cpu_react, mut cpu_flow) = (f64::MAX, f64::MAX, f64::MAX);
    for _ in 0..ticks {
        // The CPU's carrying alone, timed on a copy of the state it starts from.
        let t0 = Instant::now();
        a.step();
        cpu_tick.push(t0.elapsed().as_secs_f64() * 1e3);
        let t0 = Instant::now();
        b.step();
        gpu_tick.push(t0.elapsed().as_secs_f64() * 1e3);
        if a.hash() == b.hash() && a.flow == b.flow { same += 1 }
    }
    for _ in 0..5 {
        let mut w = a.world.clone();
        let t0 = Instant::now();
        fv_core::transport::advect(&mut w, &a.flow, a.advect_steps);
        cpu_adv = cpu_adv.min(t0.elapsed().as_secs_f64() * 1e3);
        let mut w = a.world.clone();
        let t0 = Instant::now();
        fv_core::chem::react_in(&mut w, &a.flow);
        cpu_react = cpu_react.min(t0.elapsed().as_secs_f64() * 1e3);
        let mut f = a.flow.clone();
        let t0 = Instant::now();
        f.step(&a.world);
        cpu_flow = cpu_flow.min(t0.elapsed().as_secs_f64() * 1e3);
    }
    println!("{ticks} ticks from tick {at}: {same} hashes and flows equal; gaps {} {}", b.energy_gap(), b.mass_gap());
    let log = log.lock().unwrap();
    for name in ["diffuse", "flow", "advect", "react"] {
        let rows: Vec<_> = log.iter().filter(|r| r.0 == name).collect();
        let best = |f: &dyn Fn(&(String, f64, Vec<f64>, fv_gpu::Timings)) -> f64| rows.iter().map(|r| f(r)).fold(f64::MAX, f64::min);
        let kernels = rows.iter().map(|r| r.2.clone()).min_by(|x, y| x.iter().sum::<f64>().partial_cmp(&y.iter().sum::<f64>()).unwrap()).unwrap();
        println!("{name:8} best (ms): total {:.1} | kernels {:.1} ({}) | upload {:.1} | readback {:.1} | CPU packing {:.1} + unpacking {:.1}",
            best(&|r| r.1), kernels.iter().sum::<f64>(), kernels.iter().map(|x| format!("{x:.1}")).collect::<Vec<_>>().join(" + "),
            best(&|r| r.3.upload), best(&|r| r.3.readback), best(&|r| r.3.prepare), best(&|r| r.3.unpack));
    }
    // The one number: the four phases per tick on the GPU (every call's whole
    // time, transfers included) against the same four on the CPU.
    let phase_ms = |name: &str| { let v: Vec<f64> = log.iter().filter(|r| r.0 == name).map(|r| r.1).collect(); v.iter().copied().fold(f64::MAX, f64::min) };
    let gpu4 = phase_ms("diffuse") + phase_ms("flow") + phase_ms("advect") + phase_ms("react");
    let mut cpu_diff = f64::MAX;
    for _ in 0..5 { let mut w = a.world.clone(); let t0 = Instant::now(); fv_core::transport::diffuse(&mut w, &a.flow); cpu_diff = cpu_diff.min(t0.elapsed().as_secs_f64() * 1e3) }
    let cpu4 = cpu_diff + cpu_flow + cpu_adv + cpu_react;
    let best = |v: &[f64]| v.iter().copied().fold(f64::MAX, f64::min);
    println!("TICK: the four phases {gpu4:.1} ms on the GPU against {cpu4:.1} ms on the CPU ({:.1}x); whole tick {:.1} ms against {:.1} ms (best of {ticks}); exact: {}",
        cpu4 / gpu4, best(&gpu_tick), best(&cpu_tick), same == ticks);
    println!("CPU, 4 threads (copying the world included): flow {cpu_flow:.1} ms, advect {cpu_adv:.1} ms, react {cpu_react:.1} ms");
}
