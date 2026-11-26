use fv_core::cell::Cell;
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;
fn main() {
    let n = 32;
    let mut w = World::new(n, n, 32, 7, celsius(20));
    let soil = Cell::solid_of(SOIL, 1000, 150, celsius(20), &w.subs);
    let grass = Cell::solid_of(GRASS, 1000, 150, celsius(20), &w.subs);
    for y in 0..n { for x in 0..n { for z in 0..6 { w.set(x, y, z, soil) } w.set(x, y, 6, grass); } }
    let mut sim = Sim::new(w, Sky::NIGHT);
    for t in 0..60 { sim.schedule(Scheduled { tick: t, peer: 0, seq: 0, command: Command::Heat { x: 10, y: 16, z: 6, energy: joules(250) } }) }
    for tick in 0..60 {
        sim.step();
        if (24..40).contains(&tick) {
            let w = &sim.world;
            let mut best: Vec<(i64, usize, usize, usize)> = vec![];
            for z in 0..w.nz { for y in 0..w.ny { for x in 0..w.nx { let c = w.get(x, y, z); if c.gas[4].0 > 30_000 { best.push((c.gas[4].0, x, y, z)) } } } }
            best.sort_by_key(|b| -b.0);
            let desc: Vec<String> = best.iter().take(5).map(|(f, x, y, z)| { let c = w.get(*x, *y, *z); format!("({x},{y},{z}) fuel={} o2={} T={:.0}C sub={}", f / 1000, c.gas[0].0 / 1000, c.temperature(&w.subs).celsius_f64(), c.substance) }).collect();
            println!("{tick}: {} cells with fuel; {}", best.len(), desc.join(" | "));
        }
    }
}
