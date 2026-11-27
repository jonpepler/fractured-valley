//! Each phase of a tick on its own, repeated on copies of the same world (the
//! dead tree at tick `AT`, default 30), reporting the fastest of `REPS` runs
//! (default 15): steadier than timing whole ticks.
//!
//!     cargo run --release -p fv-core --example phase_bench
//!     RAYON_NUM_THREADS=1 cargo run --release -p fv-core --example phase_bench
use fv_core::rad::{Light, Sky};
use fv_core::scene::crib_and_tree;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::units::*;
use std::time::Instant;

fn main() {
    let at: u64 = std::env::var("AT").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
    let reps: usize = std::env::var("REPS").ok().and_then(|v| v.parse().ok()).unwrap_or(15);
    let (w, torch) = crib_and_tree(4, 48, 48, true);
    let mut sim = Sim::new(w, Sky::afternoon());
    for t in 0..200 { for (i, p) in torch.iter().enumerate().filter(|(i, _)| i % 3 == 0) { sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x: p[0] as u16, y: p[1] as u16, z: p[2] as u16, energy: joules(500) } }) } }
    for _ in 0..at { sim.step() }
    let best = |f: &mut dyn FnMut()| -> f64 {
        let mut b = f64::MAX;
        for _ in 0..reps { let t0 = Instant::now(); f(); b = b.min(t0.elapsed().as_secs_f64() * 1000.0) }
        b
    };
    let (w0, flow0) = (sim.world.clone(), sim.flow.clone());
    let diffuse = best(&mut || { let mut w = w0.clone(); fv_core::transport::diffuse(&mut w, &flow0) });
    let flow = best(&mut || { let mut f = flow0.clone(); f.step(&w0) });
    let advect = best(&mut || { let mut w = w0.clone(); fv_core::transport::advect(&mut w, &flow0, sim.advect_steps) });
    let react = best(&mut || { let mut w = w0.clone(); fv_core::chem::react_in(&mut w, &flow0) });
    let radiate = best(&mut || { let mut w = w0.clone(); let mut l = Light::new(w.cells.len()); fv_core::rad::radiate_by(sim.radiation, &mut w, &sim.sky, &mut l) });
    let structure = { let mut b = f64::MAX; for _ in 0..reps.min(3) { let mut w = w0.clone(); let t0 = Instant::now(); let _ = fv_core::structure::settle(&mut w); b = b.min(t0.elapsed().as_secs_f64() * 1000.0) } b };
    let clone = best(&mut || { let _ = w0.clone(); });
    println!("tick {at}, best of {reps} (ms; copying the world, {clone:.1} ms, is included in each but structure):");
    println!("diffuse {diffuse:.1} | flow {flow:.1} | advect {advect:.1} | react {react:.1} | radiate {radiate:.1} | structure (every 10th tick) {structure:.0}");
    println!("a tick without the structure check: {:.1}; averaged over 10 ticks with it: {:.1}", diffuse + flow + advect + react + radiate - 4.0 * clone, diffuse + flow + advect + react + radiate - 4.0 * clone + structure / 10.0);
}
