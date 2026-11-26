use fv_core::cell::{Cell, VOLATILE, CHAR};
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;
fn main() {
    let n = 16;
    let mut w = World::new(n, n, 16, 9, celsius(15));
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(15), &subs);
    let kindling = Cell::solid_of(KINDLING, 1000, 80, celsius(15), &subs);
    for z in 0..4 { for y in 0..n { for x in 0..n { w.set(x, y, z, stone) } } }
    for y in 7..10 { for x in 7..10 { w.set(x, y, 4, kindling) } }
    let mut sim = Sim::new(w, Sky::NIGHT);
    for t in 0..100 { sim.schedule(Scheduled { tick: t, peer: 0, seq: 0, command: Command::Heat { x: 8, y: 8, z: 4, energy: joules(400) } }) }
    for tick in 0..300 {
        sim.step();
        if tick % 10 != 9 { continue }
        let w = &sim.world; let c = w.get(8, 8, 4); let up = w.get(8, 8, 5); let nb = w.get(7, 8, 4);
        println!("{tick}: solid {:.0}C gas {:.0}C water {} vol {} char {} fuel {} O2 {} | above gas {:.0}C fuel {} | nb solid {:.0}C vol {}", c.solid_temperature(&w.subs).celsius_f64(), c.gas_temperature().celsius_f64(), c.water.0, c.solid[VOLATILE].0, c.solid[CHAR].0, c.gas[Gas::Fuel as usize].0, c.gas[Gas::Oxygen as usize].0, up.gas_temperature().celsius_f64(), up.gas[Gas::Fuel as usize].0, nb.solid_temperature(&w.subs).celsius_f64(), nb.solid[VOLATILE].0);
    }
}
