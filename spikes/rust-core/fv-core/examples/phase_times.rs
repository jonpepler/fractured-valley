//! Wall-clock time of each phase of a tick, for the dead-tree scene (as
//! `probe_tree dead`). It runs `Sim::step` itself, then repeats each phase on
//! a copy to time it, so it never changes what the sim computes.
//!
//!     cargo run --release -p fv-core --example phase_times
//!     TICKS=300 EVERY=50 FV_THREADS=1 cargo run --release -p fv-core --example phase_times
use fv_core::rad::Sky;
use fv_core::scene::crib_and_tree;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::units::*;
use std::time::Instant;

fn main() {
    if std::env::var("FV_THREADS").ok().as_deref() == Some("1") { fv_core::world::THREADS.store(1, core::sync::atomic::Ordering::Relaxed) }
    let ticks: u64 = std::env::var("TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let every: u64 = std::env::var("EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(50);
    let (w, torch) = crib_and_tree(4, 48, 48, true);
    let mut sim = Sim::new(w, Sky::afternoon());
    for t in 0..200 { for (i, p) in torch.iter().enumerate().filter(|(i, _)| i % 3 == 0) { sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x: p[0] as u16, y: p[1] as u16, z: p[2] as u16, energy: joules(500) } }) } }
    let names = ["diffuse", "flow", "advect", "react", "radiate", "structure"];
    let mut sum = [0f64; 7];
    let mut n = 0;
    for t in 1..=ticks {
        // Time each phase on a copy of the world as it stands before this tick.
        let mut w = sim.world.clone();
        let mut flow = sim.flow.clone();
        let mut light = fv_core::rad::Light::new(w.cells.len());
        let ms = |t0: Instant| t0.elapsed().as_secs_f64() * 1000.0;
        let t0 = Instant::now(); fv_core::transport::diffuse(&mut w, &flow); sum[0] += ms(t0);
        let t0 = Instant::now(); flow.step(&w); sum[1] += ms(t0);
        let t0 = Instant::now(); fv_core::transport::advect(&mut w, &flow, sim.advect_steps); sum[2] += ms(t0);
        let t0 = Instant::now(); fv_core::chem::react_in(&mut w, &flow); sum[3] += ms(t0);
        let t0 = Instant::now(); fv_core::rad::radiate_by(sim.radiation, &mut w, &sim.sky, &mut light); sum[4] += ms(t0);
        if w.tick % sim.structure_every == 0 { let t0 = Instant::now(); let _ = fv_core::structure::settle(&mut w); sum[5] += ms(t0) }
        let t0 = Instant::now(); sim.step(); let total = ms(t0);
        n += 1;
        sum[6] += total;
        if t % every == 0 {
            let line: Vec<String> = names.iter().zip(sum).map(|(k, v)| format!("{k} {:.1}", v / n as f64)).collect();
            println!("tick {t}: step {:.1} ms | {}", sum[6] / n as f64, line.join(" | "));
            sum = [0.0; 7];
            n = 0;
        }
    }
}
