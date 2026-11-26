use fv_core::cell::{Cell, VOLATILE, CHAR};
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;
fn main() {
    let n = 24;
    let mut w = World::new(n, n, 16, 9, celsius(15));
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(15), &subs);
    let kindling = Cell::solid_of(KINDLING, 1000, 80, celsius(15), &subs);
    for z in 0..4 { for y in 0..n { for x in 0..n { w.set(x, y, z, stone) } } }
    for y in 6..19 { for x in 6..19 { w.set(x, y, 4, kindling) } }
    let mut sim = Sim::new(w, Sky::NIGHT);
    for t in 0..100 { sim.schedule(Scheduled { tick: t, peer: 0, seq: 0, command: Command::Heat { x: 12, y: 12, z: 4, energy: joules(400) } }) }
    for tick in 0..4800 {
        sim.step();
        if tick % 400 != 399 { continue }
        let w = &sim.world; let s = &w.subs;
        let c = w.get(12, 12, 4); let nb = w.get(13, 12, 4);
        println!("{:>4}s centre surf {:.0} core {:.0} vol {} char {} | nb surf {:.0} core {:.0} gas {:.0} water {} vol {}", (tick + 1) / 20,
            c.surface_temperature(s).celsius_f64(), c.solid_temperature(s).celsius_f64(), c.solid[VOLATILE].0 / 1000, c.solid[CHAR].0 / 1000,
            nb.surface_temperature(s).celsius_f64(), nb.solid_temperature(s).celsius_f64(), nb.gas_temperature().celsius_f64(), nb.water.0 / 1000, nb.solid[VOLATILE].0 / 1000);
    }
}
