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
        assert_eq!(sim.world.first_impossible(), None, "a store went negative");
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
fn a_wall_beside_a_fire_gets_hot_and_the_heat_soaks_in() {
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
    let hottest = |s: &Sim, f: &dyn Fn(&Cell) -> f64| (14..20).map(|x| f(s.world.get(x, 14, 5))).fold(f64::MIN, f64::max);
    let subs = sim.world.subs.clone();
    let face = |c: &Cell| c.surface_temperature(&subs).celsius_f64();
    let core = |c: &Cell| c.solid_temperature(&subs).celsius_f64();
    let (face0, core0) = (hottest(&sim, &face), hottest(&sim, &core));
    // Two minutes of fire.
    for _ in 0..2400 { sim.step() }
    assert_eq!(sim.energy_gap(), 0);
        assert_eq!(sim.world.first_impossible(), None, "a store went negative");
    let (face1, core1) = (hottest(&sim, &face), hottest(&sim, &core));
    // Stone's face (its outer centimetre) heats within minutes; the heat
    // soaks into the rest over tens of minutes (10 cm of stone: L²/α ≈ 40 min).
    assert!(face1 > face0 + 15.0, "the wall's face warmed from {face0:.1} to {face1:.1} °C");
    assert!(core1 > core0 + 0.3, "and heat soaked into it: {core0:.2} → {core1:.2} °C");
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

/// A bed of kindling lit at one point, or all over (the same torch per cell):
/// the centre's peak temperature and glow over 30 s.
fn kindling_fire(lit: &[(u16, u16)]) -> (Sim, f64, f64, i64) {
    let mut w = ground(16, 16, 4);
    let subs = w.subs.clone();
    let kindling = Cell::solid_of(KINDLING, 1000, 80, celsius(15), &subs);
    for y in 6..11 { for x in 6..11 { w.set(x, y, 4, kindling) } }
    let mut sim = Sim::new(w, Sky::NIGHT);
    for &(x, y) in lit { light_torch(&mut sim, x, y, 4, 100) }
    let (mut solid, mut gas, mut glow) = (0f64, 0f64, 0i64);
    for _ in 0..600 {
        sim.step();
        assert_eq!(sim.energy_gap(), 0);
        assert_eq!(sim.world.first_impossible(), None, "a store went negative");
        let c = sim.world.get(8, 8, 4);
        if sim.world.tick > 150 { solid = solid.max(c.surface_temperature(&sim.world.subs).celsius_f64()) }
        gas = gas.max(c.gas_temperature().celsius_f64());
        glow += sim.light.glow[sim.world.idx(8, 8, 4)].iter().sum::<i64>();
    }
    (sim, solid, gas, glow)
}

#[test]
fn a_bigger_fire_burns_hotter_and_brighter() {
    // Nothing sets this: burning twigs surrounded by burning twigs take in
    // each other's heat and light, so lose less and run hotter.
    let (_, small_solid, small_gas, small_glow) = kindling_fire(&[(8, 8)]);
    let all: Vec<(u16, u16)> = (7..10).flat_map(|y| (7..10).map(move |x| (x, y))).collect();
    let (sim, big_solid, big_gas, big_glow) = kindling_fire(&all);
    eprintln!("twigs {small_solid:.0} → {big_solid:.0} °C, gas {small_gas:.0} → {big_gas:.0} °C, glow {small_glow} → {big_glow}");
    assert!(big_solid > small_solid + 30.0, "twigs: {small_solid:.0} °C alone, {big_solid:.0} °C in a bigger fire");
    assert!(big_gas > small_gas + 30.0, "gas: {small_gas:.0} °C alone, {big_gas:.0} °C in a bigger fire");
    // Brighter too, but by less than it is hotter: most of a burning cell's
    // visible light is its flame's, set by the heat it releases rather than by
    // temperature (soot's spectrum makes flames ~3× brighter in the visible
    // than a grey body would), and only the hot twigs' share grows with heat.
    assert!(big_glow > small_glow * 104 / 100, "glow: {small_glow} µJ alone, {big_glow} µJ in a bigger fire");
    // And the heat stays near the fire: books that balance can still hide
    // energy put in the wrong place, so bound it too.
    let w = &sim.world;
    for z in 0..w.nz { for y in 0..w.ny { for x in 0..w.nx {
        // Sideways: straight above is the plume (a ~50 kW fire's flames
        // reach ~0.9 m: Heskestad's flame height).
        let far = x.abs_diff(8).max(y.abs_diff(8)) >= 5;
        let t = w.get(x, y, z).temperature(&w.subs).celsius_f64();
        if far { assert!(t < 150.0, "{t:.0} °C at ({x}, {y}, {z}), far from the fire") }
    }}}
}

#[test]
fn soot_glows_more_in_the_visible_than_a_grey_body() {
    // A grey body at 1 400 K gives ~0.025 % of its emission as visible light;
    // soot (ε ∝ λ^−0.83) ~0.084 %, by direct integration of Planck's law.
    let t = fv_core::tables::tables();
    let one = fv_core::units::Fraction::ONE.0 as f64;
    let visible = |b: [u64; 4]| (b[1] + b[2] + b[3]) as f64 / one;
    let (grey, soot) = (visible(t.bands[1_400]), visible(t.soot_bands[1_400]));
    eprintln!("visible at 1400 K: grey {grey:.6}, soot {soot:.6}");
    assert!((grey - 0.000254).abs() < 0.00001, "grey {grey}");
    assert!((soot - 0.000839).abs() < 0.00002, "soot {soot}");
    // Every row still adds up to all of it.
    for k in [300, 1_000, 1_400, 3_000, 5_778] { assert_eq!(t.soot_bands[k].iter().sum::<u64>(), fv_core::units::Fraction::ONE.0) }
}
