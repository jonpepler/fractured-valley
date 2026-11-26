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
    let grass = Cell::solid_of(GRASS, 1000, 50, celsius(20), &w.subs);
    for y in 0..n { for x in 0..n { for z in 0..6 { w.set(x, y, z, soil) } w.set(x, y, 6, grass); } }
    let mut sim = Sim::new(w, Sky::NIGHT);
    sim.flow.wind_mean = [2000, 0];
    for t in 0..80 { for y in 0..32u16 { sim.schedule(Scheduled { tick: t, peer: 0, seq: y as u32, command: Command::Heat { x: 10, y, z: 6, energy: joules(250) } }) } }
    for tick in 0..600 {
        sim.step();
        if tick % 25 != 0 { continue }
        let w = &sim.world;
        let row = |z: usize| (9..16).map(|x| { let c = w.get(x, 16, z); format!("{:>4.0}/{:<4.0}", c.gas_temperature().celsius_f64(), c.solid_temperature(&w.subs).celsius_f64()) }).collect::<Vec<_>>().join(" ");
        let fuel: Vec<String> = (9..16).map(|x| format!("{:>5}", w.get(x, 16, 6).solid[0].0 / 1000)).collect();
        let vel = sim.flow.block_face(5, 8, 3, 0);
        let up = sim.flow.block_face(5, 8, 3, 2);
        println!("{tick:3} z6 {} | z7 {} | volatile z6 {} | u={vel} w={up}", row(6), row(7), fuel.join(""));
    }
}
