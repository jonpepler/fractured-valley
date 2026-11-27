//! The bigger-fire test's two twig fires: flame heat, visible light and
//! oxygen per cell alight, from 7.5 s to 30 s.
use fv_core::cell::{Cell, VOLATILE};
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;
fn run(lit: &[(u16, u16)]) {
    let mut w = World::new(16, 16, 16, 9, celsius(15));
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(15), &subs);
    for z in 0..4 { for y in 0..16 { for x in 0..16 { w.set(x, y, z, stone) } } }
    let k = Cell::solid_of(KINDLING, 1000, 80, celsius(15), &subs);
    for y in 6..11 { for x in 6..11 { w.set(x, y, 4, k) } }
    let full = k.solid[VOLATILE].0;
    let mut sim = Sim::new(w, Sky::NIGHT);
    sim.flames = Some(Vec::new());
    for &(x, y) in lit { for t in 0..100 { sim.schedule(Scheduled { tick: t, peer: 0, seq: (x as u32) << 8 | y as u32, command: Command::Heat { x, y, z: 4, energy: joules(400) } }) } }
    let (mut heat, mut glow, mut burning, mut o2, mut bed_heat) = (0f64, 0f64, 0f64, 0f64, 0f64);
    for t in 1..=600 {
        sim.step();
        if t <= 150 { continue }
        let w = &sim.world;
        let fl = sim.flames.as_ref().unwrap();
        heat += fl.iter().sum::<i64>() as f64 / 0.35;
        glow += sim.light.glow.iter().map(|g| g.iter().sum::<i64>()).sum::<i64>() as f64;
        for y in 6..11 { for x in 6..11 {
            let c = w.get(x, y, 4);
            bed_heat += fl[w.idx(x, y, 4)] as f64 / 0.35;
            if c.solid[VOLATILE].0 < full * 39 / 40 { burning += 1.0; o2 += c.gas[Gas::Oxygen as usize].0 as f64 / FRESH_AIR[Gas::Oxygen as usize].0 as f64 * 1000.0 / c.gas_volume_ml(&w.subs).max(1) as f64 }
        }}
    }
    let b = burning.max(1.0);
    println!("lit at {} point(s): cells alight {:.1} on average; per cell alight: flame heat {:.2} kW ({:.0} % of it in the bed layer), light {:.0} µJ/tick, O2 in burning cells {:.0} % of fresh",
        lit.len(), b / 450.0, heat / b / 0.05 / 1e9, 100.0 * bed_heat / heat.max(1.0), glow / b, 100.0 * o2 / b);
}
fn main() {
    run(&[(8, 8)]);
    let all: Vec<(u16, u16)> = (7..10).flat_map(|y| (7..10).map(move |x| (x, y))).collect();
    run(&all);
}
