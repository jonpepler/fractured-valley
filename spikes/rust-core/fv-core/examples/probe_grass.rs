use fv_core::cell::{Cell, VOLATILE};
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;
fn main() {
    let moisture: i64 = std::env::args().nth(1).map(|v| v.parse().unwrap()).unwrap_or(100);
    let n = 32;
    let mut w = World::new(n, n, 16, 9, celsius(15));
    let subs = w.subs.clone();
    let soil = Cell::solid_of(SOIL, 1000, 150, celsius(15), &subs);
    let grass = Cell::solid_of(GRASS, 1000, moisture, celsius(15), &subs);
    for z in 0..4 { for y in 0..n { for x in 0..n { w.set(x, y, z, soil) } } }
    for y in 2..30 { for x in 2..30 { w.set(x, y, 4, grass) } }
    let full = grass.solid[VOLATILE].0;
    let mut sim = Sim::new(w, Sky::NIGHT);
    let wind: i64 = std::env::var("WIND").ok().and_then(|v| v.parse().ok()).unwrap_or(-1);
    if wind >= 0 { sim.flow.wind_mean = [0, wind] }
    for t in 0..100 { for x in 2..30u16 { sim.schedule(Scheduled { tick: t, peer: 0, seq: x as u32, command: Command::Heat { x, y: 2, z: 4, energy: joules(400) } }) } }
    for tick in 0..2400 {
        sim.step();
        if tick % 4000 == 19 { let w = &sim.world; for y in [3usize, 4] { let c = w.get(16, y, 4); print!("  y{y}: skin {:.0} gas {:.0} vol {} fuel {} O2 {} |", c.surface_temperature(&w.subs).celsius_f64(), c.gas_temperature().celsius_f64(), c.solid[VOLATILE].0 / 1000, c.gas[Gas::Fuel as usize].0, c.gas[Gas::Oxygen as usize].0) } println!(" t={}s", (tick + 1) / 20) }
        if tick % 100 != 99 { continue }
        let w = &sim.world;
        let front = (2..30).filter(|&y| (8..24).any(|x| w.get(x, y, 4).solid[VOLATILE].0 < full * 9 / 10)).max().unwrap_or(0);
        let hot = (2..30).map(|y| w.get(16, y, 4).surface_temperature(&w.subs).celsius_f64() as i64).collect::<Vec<_>>();
        println!("{:>3}s gap {} front {} cm | surface along y: {:?}", (tick + 1) / 20, sim.energy_gap(), (front.max(2) - 2) * 10, &hot[..12]);
    }
}
