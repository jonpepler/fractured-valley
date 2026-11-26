use fv_core::cell::Cell;
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;
fn main() {
    let n = 16;
    let mut w = World::new(n, n, 16, 7, celsius(20));
    let soil = Cell::solid_of(SOIL, 1000, 150, celsius(20), &w.subs);
    let grass = Cell::solid_of(GRASS, 1000, 150, celsius(20), &w.subs);
    for y in 0..n { for x in 0..n { for z in 0..6 { w.set(x, y, z, soil) } w.set(x, y, 6, grass); } }
    println!("grass C={} uJ/K T={}", grass.heat_capacity(&w.subs).0, grass.temperature(&w.subs).0);
    let mut sim = Sim::new(w, Sky::NIGHT);
    for t in 0..60 { sim.schedule(Scheduled { tick: t, peer: 0, seq: 0, command: Command::Heat { x: 8, y: 8, z: 6, energy: joules(250) } }) }
    for tick in 0..70 {
        let before = *sim.world.get(8, 8, 6);
        sim.step();
        let c = sim.world.get(8, 8, 6);
        if tick % 3 == 0 { println!("{tick} T={:.0}C water={} volatile={} fuel={} o2={} soot={} gas={}", c.temperature(&sim.world.subs).celsius_f64(), c.water.0/1000, c.solid[0].0/1000, c.gas[4].0/1000, c.gas[0].0/1000, c.gas[5].0/1000, c.gas_mass().0/1000); }
    }
}
