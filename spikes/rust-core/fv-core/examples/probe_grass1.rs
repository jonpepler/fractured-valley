use fv_core::cell::Cell;
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;
fn main() {
    let n = 12;
    let mut w = World::new(n, n, 12, 9, celsius(15));
    let subs = w.subs.clone();
    let soil = Cell::solid_of(SOIL, 1000, 150, celsius(15), &subs);
    let grass = Cell::solid_of(GRASS, 1000, 100, celsius(15), &subs);
    for z in 0..4 { for y in 0..n { for x in 0..n { w.set(x, y, z, soil) } } }
    for y in 0..n { for x in 0..n { w.set(x, y, 4, grass) } }
    let mut sim = Sim::new(w, Sky::NIGHT);
    for t in 0..60 { sim.schedule(Scheduled { tick: t, peer: 0, seq: 0, command: Command::Heat { x: 6, y: 6, z: 4, energy: joules(400) } }) }
    for _ in 0..40 {
        sim.step();
        let w = &sim.world; let s = &w.subs; let c = w.get(6, 6, 4);
        println!("vol {} fuel {} flame {} | skin {:.0}C ({} J/K) core {:.0}C ({} J/K) gas {:.0}C water {} µg | E skin {} core {} gas {}", c.solid[fv_core::cell::VOLATILE].0, c.gas[Gas::Fuel as usize].0, c.flame.0, c.surface_temperature(s).celsius_f64(), c.skin_capacity(s).0 / 1_000_000_000, c.solid_temperature(s).celsius_f64(), c.core_capacity(s).0 / 1_000_000_000, c.gas_temperature().celsius_f64(), c.water.0, c.skin.0, c.solid_energy.0, c.energy.0);
    }
}
