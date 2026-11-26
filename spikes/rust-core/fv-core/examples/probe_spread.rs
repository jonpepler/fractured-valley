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
    let grass = Cell::solid_of(GRASS, 1000, std::env::var("MOIST").map(|m| m.parse().unwrap()).unwrap_or(150), celsius(20), &w.subs);
    for y in 0..n { for x in 0..n { for z in 0..6 { w.set(x, y, z, soil) } w.set(x, y, 6, grass); } }
    let mut sim = Sim::new(w, Sky::NIGHT);
    sim.flow.wind_mean = [std::env::args().nth(1).map(|a| a.parse().unwrap()).unwrap_or(500), 0];
    for t in 0..80 { for dy in 0..2u16 { sim.schedule(Scheduled { tick: t, peer: 0, seq: dy as u32, command: Command::Heat { x: 10, y: 16 + dy, z: 6, energy: joules(250) } }) } }
    for tick in 0..400 {
        sim.step();
        let w = &sim.world;
        let show = |x: usize, y: usize, z: usize| { let c = w.get(x, y, z); format!("{:>5.0}/{:>5.0}C v{:>4} f{:>4} o{:>3} s{:>3}", c.gas_temperature().celsius_f64(), c.solid_temperature(&w.subs).celsius_f64(), c.solid[0].0/1000, c.gas[4].0/1000, c.gas[0].0/1000, c.gas[5].0/1000) };
        let glow: i64 = sim.light.glow.iter().map(|g| g[0]+g[1]+g[2]).sum();
        if tick % 100 == 99 { let burnt = w.cells.iter().filter(|c| c.substance == GRASS && c.solid[0].0 < 1_000_000).count(); println!("burnt {burnt}"); }
        if tick % 10 == 0 && std::env::var("QUIET").is_err() { println!("{tick:3} lit:{} | +x:{} | +2x:{} | above:{} | rad out {:.0} J glow {:.0} J", show(10,16,6), show(11,16,6), show(12,16,6), show(10,16,7), sim.books.radiation_out as f64/1e6, glow as f64/1e6); }
    }
}
