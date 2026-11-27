//! Where the diffusion phase's time goes on the CPU (dead tree, tick AT).
use fv_core::rad::Sky;
use fv_core::scene::crib_and_tree;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::transport::*;
use fv_core::units::*;
use std::time::Instant;

fn main() {
    let at: u64 = std::env::var("AT").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
    let (w, torch) = crib_and_tree(4, 48, 48, true);
    let mut sim = Sim::new(w, Sky::afternoon());
    for t in 0..200 { for (i, p) in torch.iter().enumerate().filter(|(i, _)| i % 3 == 0) { sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x: p[0] as u16, y: p[1] as u16, z: p[2] as u16, energy: joules(500) } }) } }
    for _ in 0..at { sim.step() }
    let w = sim.world.clone();
    let best = |f: &mut dyn FnMut()| { let mut b = f64::MAX; for _ in 0..15 { let t0 = Instant::now(); f(); b = b.min(t0.elapsed().as_secs_f64() * 1e3) } b };
    let reach = best(&mut || { std::hint::black_box(fv_core::chem::flame_reach(&w)); });
    let sides_t = best(&mut || { std::hint::black_box(sides(&w)); });
    let p = prepare(&w, &sim.flow);
    let prep = best(&mut || { std::hint::black_box(prepare(&w, &sim.flow)); });
    let limits = best(&mut || { std::hint::black_box(gas_limits(&w, &p.faces)); });
    let whole = best(&mut || { let mut w2 = w.clone(); diffuse(&mut w2, &sim.flow) });
    let clone = best(&mut || { std::hint::black_box(w.clone()); });
    println!("cells {} | flame_reach {reach:.1} | sides (incl. reach) {sides_t:.1} | prepare (sides+vel+faces+limits) {prep:.1} | limits {limits:.1} | whole diffuse {:.1} (clone {clone:.1} removed)", w.cells.len(), whole - clone);
}
