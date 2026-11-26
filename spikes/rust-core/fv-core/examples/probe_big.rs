use fv_core::cell::Cell;
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;
fn run(lit: &[(u16, u16)]) -> (f64, f64, f64) {
    let n = 16;
    let mut w = World::new(n, n, 16, 9, celsius(15));
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(15), &subs);
    let kindling = Cell::solid_of(KINDLING, 1000, 80, celsius(15), &subs);
    for z in 0..4 { for y in 0..n { for x in 0..n { w.set(x, y, z, stone) } } }
    for y in 6..11 { for x in 6..11 { w.set(x, y, 4, kindling) } }
    let mut sim = Sim::new(w, Sky::NIGHT);
    for t in 0..100 { for (i, &(x, y)) in lit.iter().enumerate() { sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x, y, z: 4, energy: joules(400) } }) } }
    let (mut peak_solid, mut peak_gas, mut glow) = (0f64, 0f64, 0f64);
    for _ in 0..600 {
        sim.step();
        assert_eq!(sim.energy_gap(), 0);
        let c = sim.world.get(8, 8, 4);
        if sim.world.tick > 150 { peak_solid = peak_solid.max(c.solid_temperature(&sim.world.subs).celsius_f64()); }
        peak_gas = peak_gas.max(c.gas_temperature().celsius_f64());
        glow += sim.light.glow[sim.world.idx(8, 8, 4)].iter().sum::<i64>() as f64;
    }
    (peak_solid, peak_gas, glow / 600.0 * 2e-5)
}
fn main() {
    let small = run(&[(8, 8)]);
    let big: Vec<(u16, u16)> = (7..10).flat_map(|y| (7..10).map(move |x| (x, y))).collect();
    let big = run(&big);
    println!("small: centre solid peak {:.0}C gas peak {:.0}C glow {:.2} W", small.0, small.1, small.2);
    println!("big:   centre solid peak {:.0}C gas peak {:.0}C glow {:.2} W", big.0, big.1, big.2);
}
