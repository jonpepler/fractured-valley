use fv_core::rad::Sky;
use fv_core::scene::crib_and_tree;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
fn main() {
    let (w, torch) = crib_and_tree(4, 48, 48, true);
    let mut sim = Sim::new(w, Sky::afternoon());
    for t in 0..200 { for (i, p) in torch.iter().enumerate().filter(|(i, _)| i % 3 == 0) { sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x: p[0] as u16, y: p[1] as u16, z: p[2] as u16, energy: joules(500) } }) } }
    for _ in 0..3600 { sim.step() }
    let w = &sim.world; let s = &w.subs;
    // The hottest log faces, and the gas in the open cells next to them.
    let mut logs: Vec<(f64, usize, usize, usize)> = vec![];
    for z in 0..w.nz { for y in 0..w.ny { for x in 0..w.nx { let c = w.get(x, y, z); if c.substance == WOOD && z < 20 { logs.push((c.surface_temperature(s).celsius_f64(), x, y, z)) } } } }
    logs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    for &(t, x, y, z) in logs.iter().take(5) {
        print!("log ({x},{y},{z}) face {t:.0}C core {:.0}C |", w.get(x, y, z).solid_temperature(s).celsius_f64());
        for (dx, dy, dz) in [(1i64, 0i64, 0i64), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1)] {
            let (nx, ny, nz) = ((x as i64 + dx) as usize, (y as i64 + dy) as usize, (z as i64 + dz) as usize);
            let n = w.get(nx, ny, nz);
            if n.substance != WOOD { print!(" n{:?} sub{} gas {:.0}C fuel {} O2 {}", (dx, dy, dz), n.substance, n.gas_temperature().celsius_f64(), n.gas[Gas::Fuel as usize].0, n.gas[Gas::Oxygen as usize].0) }
        }
        println!(" own fuel {} O2 {}", w.get(x, y, z).gas[Gas::Fuel as usize].0, w.get(x, y, z).gas[Gas::Oxygen as usize].0);
    }
}
