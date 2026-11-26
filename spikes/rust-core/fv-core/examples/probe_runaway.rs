use fv_core::cell::Cell;
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;
fn main() {
    let n = 40;
    let mut w = World::new(n, n, 32, 7, celsius(15));
    let subs = w.subs.clone();
    let t = celsius(15);
    let stone = Cell::solid_of(STONE, 1000, 2, t, &subs);
    let soil = Cell::solid_of(SOIL, 1000, 150, t, &subs);
    let grass = Cell::solid_of(GRASS, 1000, 900, t, &subs);
    let wood = Cell::solid_of(WOOD, 1000, 100, t, &subs);
    let kindling = Cell::solid_of(KINDLING, 1000, 80, t, &subs);
    for y in 0..n { for x in 0..n { for z in 0..4 { w.set(x, y, z, if z < 2 { stone } else { soil }) } w.set(x, y, 4, grass) } }
    let (cx, cy) = (n / 2 - 2, n / 2 - 2);
    for y in cy..cy + 4 { for x in cx..cx + 4 { w.set(x, y, 4, kindling); w.set(x, y, 5, if (x + y) % 2 == 0 { kindling } else { wood }) } }
    for x in cx..cx + 4 { w.set(x, cy, 6, wood); w.set(x, cy + 3, 6, wood) }
    for z in 5..14 { for x in cx - 6..cx + 10 { w.set(x, cy - 4, z, stone) } }
    fv_core::scene::bond_all(&mut w, fv_core::scene::natural_bonds);
    let mut sim = Sim::new(w, Sky::NIGHT);
    for tick in 0..100 { for (i, (x, y)) in [(cx, cy), (cx + 2, cy + 1), (cx + 1, cy + 3), (cx + 3, cy + 2)].iter().enumerate() { sim.schedule(Scheduled { tick, peer: 0, seq: i as u32, command: Command::Heat { x: *x as u16, y: *y as u16, z: 4, energy: joules(400) } }) } }
    for tick in 0..std::env::var("TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(60) {
        sim.step();
        let w = &sim.world;
        let (mut hot, mut hottest, mut at) = (0, 0i64, (0, 0, 0));
        for z in 0..w.nz { for y in 0..w.ny { for x in 0..w.nx { let c = w.get(x, y, z); let t = c.temperature(&w.subs).0; if t > 1_500_000 { hot += 1 } if t > hottest { hottest = t; at = (x, y, z) } } } }
        let c = w.get(at.0, at.1, at.2);
        let burning = w.cells.iter().filter(|c| c.substance == KINDLING && c.solid[fv_core::cell::VOLATILE].0 < 39_000_000).count();
        let maxgas = w.cells.iter().map(|c| c.gas_temperature().0).max().unwrap();
        let wood_hot = w.cells.iter().filter(|c| c.substance == WOOD && c.surface_temperature(&w.subs).0 > 600_000).count();
        if tick % 50 != 49 { continue }
        println!("kindling burning {burning}/24, wood faces >327C {wood_hot}, max gas {:.0}C", maxgas as f64 / 1000.0 - 273.15);
        println!("{tick} gap {} mass {} | cells>1500K {hot} | hottest {:.0}C at {:?} sub {} solid_mass {} skin {} solidE {} gasE {} cap {}", sim.energy_gap(), sim.mass_gap(), hottest as f64 / 1000.0 - 273.15, at, c.substance, c.solid_mass().0, c.skin.0, c.solid_energy.0, c.energy.0, c.solid_capacity(&w.subs).0);
    }
}
