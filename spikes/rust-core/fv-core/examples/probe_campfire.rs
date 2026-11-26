use fv_core::cell::Cell;
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;
fn main() {
    let mut w = World::new(32, 32, 32, 9, celsius(15));
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(15), &subs);
    for z in 0..4 { for y in 0..32 { for x in 0..32 { w.set(x, y, z, stone) } } }
    let wood = Cell::solid_of(WOOD, 1000, 100, celsius(15), &subs);
    let kindling = Cell::solid_of(KINDLING, 1000, 80, celsius(15), &subs);
    let (cx, cy, z0) = (15, 15, 4);
    for y in cy..cy + 3 { for x in cx..cx + 3 { w.set(x, y, z0, kindling) } }
    for x in cx..cx + 3 { w.set(x, cy, z0 + 1, wood); w.set(x, cy + 2, z0 + 1, wood) }
    { let mut w2 = w.clone(); let r = fv_core::structure::settle(&mut w2); println!("settle at start: broke {:?} crushed {:?} loose {:?} worst {:.3}", r.broke, r.crushed, r.loose, r.worst); }
    let mut sim = Sim::new(w, Sky::NIGHT);
    for t in 0..100 { for (x, y) in [(15u16, 15u16), (17, 15), (16, 16)] { sim.schedule(Scheduled { tick: t, peer: 0, seq: (x as u32) << 8 | y as u32, command: Command::Heat { x, y, z: 4, energy: joules(400) } }) } }
    for tick in 0..400 {
        sim.step();
        if tick % 10 == 0 && tick < 120 { let fl: Vec<String> = sim.bodies.pieces.iter().map(|p| { let b = &sim.bodies.physics.bodies[p.handle]; let t = b.translation(); format!("{:?}@({:.1},{:.1},{:.2}) v{:.2}", p.cells[0].1.substance, t.x*10.0, t.y*10.0, t.z*10.0, b.linvel().length()) }).collect(); println!("{tick} in flight: {:?}", fl); }
        if tick % 25 == 0 {
            let w = &sim.world;
            let k = w.get(16, 16, 4);
            let ki: i64 = (15..18).flat_map(|y| (15..18).map(move |x| (x, y))).map(|(x, y)| w.get(x, y, 4).solid[0].0).sum();
            let maxg = w.cells.iter().map(|c| c.gas_temperature().0).max().unwrap() as f64 / 1000.0 - 273.15;
            let glow: i64 = sim.light.glow.iter().map(|g| g[0] + g[1] + g[2]).sum();
            let inc: i64 = sim.light.incident.iter().map(|g| g[0] + g[1] + g[2]).sum();
            println!("{tick} kindling(16,16) gas {:.0}C solid {:.0}C water {} vol {}  kindling volatile total {} g  max gas {:.0}C  glow {} µJ incident {} µJ radout {:.0} J", k.gas_temperature().celsius_f64(), k.solid_temperature(&w.subs).celsius_f64(), k.water.0/1000, k.solid[0].0/1000, ki/1_000_000, maxg, glow, inc, sim.books.radiation_out as f64/1e6);
        }
    }
}
