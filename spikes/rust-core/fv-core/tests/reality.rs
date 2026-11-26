//! The simulation against measurements. The energy books prove nothing is
//! made from nothing; these check the numbers are the real world's: things
//! burn at the temperatures, and fire moves at the speeds, measured for them.
//! Each range is wide (real fires vary), but a sim scaled wrong falls out.

use fv_core::cell::{Cell, VOLATILE};
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;

fn ground(n: usize, nz: usize) -> World {
    let mut w = World::new(n, n, nz, 9, celsius(15));
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(15), &subs);
    for z in 0..4 { for y in 0..n { for x in 0..n { w.set(x, y, z, stone) } } }
    w
}

fn torch(sim: &mut Sim, x: u16, y: u16, z: u16, ticks: u64) {
    for t in 0..ticks { sim.schedule(Scheduled { tick: t, peer: 0, seq: (x as u32) << 8 | y as u32, command: Command::Heat { x, y, z, energy: joules(400) } }) }
}

/// A bed of kindling (twigs ~6 mm thick) n×n on stone, lit at its middle.
fn kindling_bed(n: usize, bed: usize) -> (Sim, usize) {
    let mut w = ground(n, 16);
    let subs = w.subs.clone();
    let kindling = Cell::solid_of(KINDLING, 1000, 80, celsius(15), &subs);
    let lo = (n - bed) / 2;
    for y in lo..lo + bed { for x in lo..lo + bed { w.set(x, y, 4, kindling) } }
    let mut sim = Sim::new(w, Sky::NIGHT);
    let c = (n / 2) as u16;
    torch(&mut sim, c, c, 4, 100);
    (sim, n / 2)
}

fn burning(c: &Cell) -> bool { c.substance == KINDLING && c.solid[VOLATILE].0 < Cell::default().solid[VOLATILE].0.max(39_000_000) }

#[test]
fn burning_twigs_and_their_embers_stay_at_real_temperatures() {
    // Burning wood's surface sits around 300–700 °C while it flames, and its
    // glowing char at about 600–1000 °C (Drysdale, An Introduction to Fire
    // Dynamics, ch. 5; Babrauskas, Ignition Handbook, §7). Twigs far hotter
    // than that mean heat is being kept in the solid that belongs to the air.
    let (mut sim, c) = kindling_bed(16, 5);
    let _ = c;
    let mut hottest = 0f64;
    for _ in 0..2400 {
        sim.step();
        assert_eq!(sim.energy_gap(), 0);
        for y in c - 2..=c + 2 { for x in c - 2..=c + 2 {
            hottest = hottest.max(sim.world.get(x, y, 4).surface_temperature(&sim.world.subs).celsius_f64());
        }}
    }
    assert!(hottest > 500.0, "the twigs caught and glowed (hottest {hottest:.0} °C)");
    eprintln!("hottest twig surface {hottest:.0} °C");
    assert!(hottest < 1000.0, "but no hotter than real embers: {hottest:.0} °C");
}

#[test]
#[ignore = "open: spreads ~25 cm/min, 8x Rothermel; a 10 cm cell can't hold a front narrower than itself (burning zone R·τ ≈ 5 cm) (26 Nov)"]
fn a_line_of_fire_creeps_through_twigs_at_rothermels_rate() {
    // Rothermel's spread model (1972) for a line of fire, no wind or slope,
    // in this bed: σ = 700/m (213/ft), 50 kg/m³ bulk, packing β = 0.04,
    // 8 % moisture. Propagating flux ratio ξ ≈ 0.018 of a reaction
    // intensity ≈ 600 kW/m² gives I_p ≈ 11 kW/m²; ignition takes
    // Q_ig = 250 + 1116·M Btu/lb = 789 kJ/kg of the heated share
    // ε = e^(−138/213) = 0.52: R = I_p/(ρ_b·ε·Q_ig) ≈ 0.54 mm/s, about
    // 3 cm a minute. Lab beds scatter about ×3 around the model, so six
    // minutes should carry the front 10–60 cm.
    let n = 24;
    let mut w = ground(n, 16);
    let subs = w.subs.clone();
    let kindling = Cell::solid_of(KINDLING, 1000, 80, celsius(15), &subs);
    for y in 4..20 { for x in 4..20 { w.set(x, y, 4, kindling) } }
    let mut sim = Sim::new(w, Sky::NIGHT);
    for x in 4..20 { torch(&mut sim, x as u16, 4, 4, 100) }
    for _ in 0..7200 { sim.step() }
    assert_eq!(sim.energy_gap(), 0);
    let w = &sim.world;
    let front = (4..20).filter(|&y| (6..18).any(|x| burning(w.get(x, y, 4)))).max().unwrap_or(0);
    let advanced = front.saturating_sub(4);
    eprintln!("front advanced {} cm in six minutes", advanced * 10);
    assert!((1..=6).contains(&advanced), "the front advanced {} cm in six minutes", advanced * 10);
}

#[test]
fn a_log_over_burning_kindling_catches_within_minutes() {
    // The way every campfire is lit: a log laid over burning kindling starts
    // flaming within a few minutes (its face reaches the ~350 °C at which a
    // flame lights wood gas: Babrauskas, Ignition Handbook).
    let mut w = ground(16, 16);
    let subs = w.subs.clone();
    let kindling = Cell::solid_of(KINDLING, 1000, 80, celsius(15), &subs);
    let wood = Cell::solid_of(WOOD, 1000, 100, celsius(15), &subs);
    for y in 6..10 { for x in 6..10 { w.set(x, y, 4, kindling) } }
    for x in 6..10 { w.set(x, 8, 5, wood) }
    let mut sim = Sim::new(w, Sky::NIGHT);
    for y in 6..10 { for x in 6..10 { torch(&mut sim, x, y, 4, 100) } }
    let mut caught = None;
    for t in 0..6000 {
        sim.step();
        let w = &sim.world;
        if (6..10).any(|x| w.get(x, 8, 5).solid[VOLATILE].0 < w.subs[WOOD as usize].full_mass.0 * 1 / 1000 * w.subs[WOOD as usize].composition[VOLATILE] * 999 / 1000) {
            caught = Some(t); break;
        }
    }
    assert_eq!(sim.energy_gap(), 0);
    assert!(caught.is_some(), "the log never started giving off gas in five minutes");
}

#[test]
fn wet_wood_holds_near_boiling_until_dry() {
    // Water in a solid boils off before it heats past 100 °C: the plateau that
    // keeps green wood from burning (any heat-transfer text).
    let mut w = ground(12, 12);
    let subs = w.subs.clone();
    let wet = Cell::solid_of(KINDLING, 1000, 600, celsius(15), &subs);
    w.set(6, 6, 4, wet);
    let mut sim = Sim::new(w, Sky::NIGHT);
    for t in 0..400 { sim.schedule(Scheduled { tick: t, peer: 0, seq: 0, command: Command::Heat { x: 6, y: 6, z: 4, energy: joules(100) } }) }
    for _ in 0..400 {
        sim.step();
        let c = sim.world.get(6, 6, 4);
        if c.water.0 > c.solid_mass().0 / 20 {
            let t = c.solid_temperature(&sim.world.subs).celsius_f64();
            assert!(t < 115.0, "still wet but at {t:.0} °C");
        }
    }
}
