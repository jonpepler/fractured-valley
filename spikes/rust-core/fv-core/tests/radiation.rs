//! Radiation as one system: heat, light and ionising radiation travel the
//! same way, every packet is counted, and the light a scene is lit by is only
//! what the simulation emitted.

use fv_core::cell::Cell;
use fv_core::rad::Sky;
use fv_core::scene::{bond_all, natural_bonds};
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;

fn ground(n: usize, nz: usize, depth: usize) -> World {
    let mut w = World::new(n, n, nz, 9, celsius(15));
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(15), &subs);
    for z in 0..depth { for y in 0..n { for x in 0..n { w.set(x, y, z, stone) } } }
    w
}

/// A campfire: a bed of kindling (dry twigs) with a few logs laid over it.
/// Fires start in kindling; a torch can't light a 10 cm log on its own.
fn campfire(w: &mut World, cx: usize, cy: usize, z0: usize) {
    let subs = w.subs.clone();
    let wood = Cell::solid_of(WOOD, 1000, 100, celsius(15), &subs);
    let kindling = Cell::solid_of(KINDLING, 1000, 80, celsius(15), &subs);
    for y in cy..cy + 3 { for x in cx..cx + 3 { w.set(x, y, z0, kindling) } }
    for x in cx..cx + 3 { w.set(x, cy, z0 + 1, wood); w.set(x, cy + 2, z0 + 1, wood) }
}

fn light_torch(sim: &mut Sim, x: u16, y: u16, z: u16, ticks: u64) {
    for t in 0..ticks { sim.schedule(Scheduled { tick: t, peer: 0, seq: (x as u32) << 8 | y as u32, command: Command::Heat { x, y, z, energy: joules(400) } }) }
}

#[test]
fn at_night_a_fire_lights_only_what_is_near_it() {
    let mut w = ground(32, 32, 4);
    campfire(&mut w, 15, 15, 4);
    let mut sim = Sim::new(w, Sky::NIGHT);
    for (x, y) in [(15, 15), (17, 15), (16, 16)] { light_torch(&mut sim, x, y, 4, 100) }
    let mut near = 0i64;
    let mut far = 0i64;
    for _ in 0..800 {
        sim.step();
        assert_eq!(sim.energy_gap(), 0);
        let w = &sim.world;
        let lit = |x: usize, y: usize| { let l = sim.light.incident[w.idx(x, y, 3)]; l[0] + l[1] + l[2] };
        near += lit(13, 15) + lit(19, 16) + lit(16, 13);
        far += lit(1, 1) + lit(30, 30) + lit(1, 30);
    }
    assert_eq!(sim.books.radiation_in, 0, "nothing came in from the sky");
    assert!(near > 0, "the fire's own light reached the ground beside it");
    assert!(near > 20 * far.max(1), "and much less of it reached far corners ({near} near, {far} far)");
}

#[test]
fn a_wall_beside_a_fire_gets_hot_on_its_other_side() {
    let mut w = ground(32, 32, 4);
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(15), &subs);
    // A stone wall one cell thick, 2 m long and 1.2 m high, with a fire 20 cm from it.
    for z in 4..16 { for x in 6..26 { w.set(x, 14, z, stone) } }
    bond_all(&mut w, natural_bonds);
    // A fire built against the wall: two campfires side by side.
    campfire(&mut w, 13, 15, 4);
    campfire(&mut w, 17, 15, 4);
    let mut sim = Sim::new(w, Sky::NIGHT);
    for (x, y) in [(13, 15), (15, 16), (17, 15), (19, 16)] { light_torch(&mut sim, x, y, 4, 100) }
    let outside = |s: &Sim| (14..20).map(|x| s.world.get(x, 14, 5).solid_temperature(&s.world.subs).celsius_f64()).fold(f64::MIN, f64::max);
    let before = outside(&sim);
    // Stone warms slowly (2 kJ/K in a 10 cm cell): two minutes of fire.
    for _ in 0..2400 { sim.step() }
    assert_eq!(sim.energy_gap(), 0);
    let after = outside(&sim);
    assert!(after > before + 2.0, "the wall's other side warmed from {before:.1} to {after:.1} °C");
}

#[test]
fn a_radioactive_rock_warms_what_is_near_it_and_stone_stops_its_rays() {
    let mut w = ground(32, 32, 8);
    let subs = w.subs.clone();
    let ore = Cell::solid_of(ORE, 1000, 0, celsius(15), &subs);
    for z in 6..9 { for y in 14..17 { for x in 14..17 { w.set(x, y, z, ore) } } }
    let mut sim = Sim::new(w, Sky::NIGHT);
    let t0 = sim.world.get(15, 15, 7).solid_temperature(&sim.world.subs);
    let nuclear0: i64 = sim.world.cells.iter().map(|c| c.nuclear.0).sum();
    let mut dose_air = 0i64;
    let mut dose_stone_near = 0i64;
    let mut dose_stone_deep = 0i64;
    for _ in 0..600 {
        sim.step();
        assert_eq!(sim.energy_gap(), 0, "decay energy is counted to the microjoule");
        let w = &sim.world;
        dose_air += sim.light.dose[w.idx(15, 15, 11)];
        dose_stone_near += sim.light.dose[w.idx(15, 15, 5)];
        dose_stone_deep += sim.light.dose[w.idx(15, 15, 1)];
    }
    let t1 = sim.world.get(15, 15, 7).solid_temperature(&sim.world.subs);
    let nuclear1: i64 = sim.world.cells.iter().map(|c| c.nuclear.0).sum();
    assert!(nuclear1 < nuclear0, "it decayed");
    assert!(t1 > t0, "and warmed itself: {:.2} → {:.2} °C", t0.celsius_f64(), t1.celsius_f64());
    assert!(dose_stone_near > 0, "its rays reached the stone just under it");
    assert!(dose_stone_near > 10 * dose_stone_deep.max(1), "stone stops them: {dose_stone_near} µJ 1 cell down, {dose_stone_deep} µJ 5 cells down");
    let _ = dose_air;
}
