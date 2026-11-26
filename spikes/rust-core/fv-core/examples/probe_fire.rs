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
    let grass = Cell::solid_of(GRASS, 1000, 150, celsius(20), &w.subs); // dry-ish grass (15 %)
    let post = Cell::solid_of(WOOD, 1000, 120, celsius(20), &w.subs);
    for y in 0..n { for x in 0..n {
        for z in 0..6 { w.set(x, y, z, soil) }
        w.set(x, y, 6, grass);
    }}
    for z in 6..16 { for y in 15..17 { for x in 15..17 { w.set(x, y, z, post) } } }
    let mut sim = Sim::new(w, Sky::NIGHT);
    for t in 0..10 { for dy in 0..2u16 { sim.schedule(Scheduled { tick: t, peer: 0, seq: dy as u32, command: Command::Heat { x: 10, y: 16 + dy, z: 6, energy: joules(1500) } }) } }
    let t0 = std::time::Instant::now();
    for tick in 0..=600u64 {
        sim.step();
        assert_eq!(sim.energy_gap(), 0, "energy gap at {tick}");
        assert_eq!(sim.mass_gap(), 0, "mass gap at {tick}");
        if tick % 50 == 0 {
            let w = &sim.world;
            let (mut hot, mut maxt, mut fuel, mut soot, mut burnt_grass, mut post_vol) = (0, 0i64, 0i64, 0i64, 0, 0i64);
            for c in &w.cells {
                let t = c.temperature(&w.subs).0;
                maxt = maxt.max(t);
                if t > 700_000 { hot += 1 }
                fuel += c.gas[Gas::Fuel as usize].0;
                soot += c.gas[Gas::Soot as usize].0;
                if c.substance == GRASS && c.solid[0].0 < 1000 { burnt_grass += 1 }
                if c.substance == WOOD { post_vol += c.solid[0].0 }
            }
            println!("{tick:4} maxT={:.0}C cells>700K={hot} fuel={}g soot={}g burntGrass={burnt_grass} postVolatile={}g radOut={:.1}kJ edge={:.1}kJ  {:.0}ms/tick",
                maxt as f64 / 1000.0 - 273.15, fuel / 1000, soot / 1000, post_vol / 1000, sim.books.radiation_out as f64 / 1e6, sim.books.edge_energy as f64 / 1e6,
                t0.elapsed().as_millis() as f64 / (tick + 1) as f64);
        }
    }
}
