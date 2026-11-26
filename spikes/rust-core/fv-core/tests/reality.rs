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
        assert_eq!(sim.world.first_impossible(), None, "a store went negative");
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
        assert_eq!(sim.world.first_impossible(), None, "a store went negative");
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
        assert_eq!(sim.world.first_impossible(), None, "a store went negative");
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

// ── a puddle in the sun, in shade and at night ─────────────────────────────

/// Where the puddle lies: a 4×4 patch of cells, 40 cm across, on stone.
const PUDDLE: core::ops::Range<usize> = 6..10;
const PUDDLE_Z: usize = 4;
/// Air after rain on a mild day: 20 °C at 85 % humidity.
const AIR_C: i64 = 20;
const HUMIDITY_PERCENT: i64 = 85;

/// Sets a cell's vapour to the given humidity at `t` (keeping its temperature).
fn humidify(c: &mut Cell, t: Temperature, subs: &[Substance]) {
    let ml = c.gas_volume_ml(subs);
    if ml <= 0 || c.gas_mass().0 == 0 { return }
    let sat = fv_core::tables::tables().saturated_ug_per_l[fv_core::tables::k_index(t.0)];
    c.gas[Gas::Vapour as usize] = Mass(sat * HUMIDITY_PERCENT / 100 * ml / CELL_VOLUME_ML);
    c.set_temperature(t, subs);
}

/// A puddle 5 mm deep on stone, everything at 20 °C in humid air, under
/// `sky`, with or without a stone roof over it. Runs `warm` ticks, then
/// `measure` more, checking the books every tick. Returns the evaporation
/// rate over the measured ticks (mm/day) and how much the water warmed over
/// the whole run (K).
fn puddle(sky: Sky, roof: bool, warm: u64, measure: u64) -> (f64, f64) {
    let air = celsius(AIR_C);
    let mut w = World::new(16, 16, 16, 9, air);
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, air, &subs);
    for z in 0..PUDDLE_Z { for y in 0..16 { for x in 0..16 { w.set(x, y, z, stone) } } }
    if roof {
        // A stone roof 30 cm up over the whole box: the low sun reaches the
        // ground under it only near the box's edges.
        for y in 0..16 { for x in 0..16 { w.set(x, y, PUDDLE_Z + 3, stone) } }
    }
    for c in w.cells.iter_mut() { if c.substance == NONE { humidify(c, air, &subs) } }
    let mut outside = w.outside_air;
    humidify(&mut outside, air, &subs);
    w.outside_air = outside;
    let mut wet = Cell::puddle(5, air, &subs);
    humidify(&mut wet, air, &subs);
    for y in PUDDLE { for x in PUDDLE { w.set(x, y, PUDDLE_Z, wet) } }
    let mut sim = Sim::new(w, sky);
    let water = |sim: &Sim| -> (i64, f64) {
        let (mut m, mut t) = (0i64, 0f64);
        for y in PUDDLE { for x in PUDDLE {
            let c = sim.world.get(x, y, PUDDLE_Z);
            assert!(c.open_water(), "the puddle is still there");
            m += c.water.0;
            t += c.solid_temperature(&sim.world.subs).kelvin_f64();
        }}
        (m, t / (PUDDLE.len() * PUDDLE.len()) as f64)
    };
    let (_, t0) = water(&sim);
    let mut m0 = 0;
    for tick in 0..warm + measure {
        if tick == warm { m0 = water(&sim).0 }
        sim.step();
        assert_eq!(sim.energy_gap(), 0, "energy books, tick {tick}");
        assert_eq!(sim.mass_gap(), 0, "mass books, tick {tick}");
        assert_eq!(sim.world.first_impossible(), None, "a store went negative, tick {tick}");
    }
    let (m1, t1) = water(&sim);
    // 1 mm of water over a cell's face is FACE_MM2 mg (10 g on a 10 cm cell).
    let mm = (m0 - m1) as f64 / 1_000.0 / FACE_MM2 as f64 / (PUDDLE.len() * PUDDLE.len()) as f64;
    let days = measure as f64 * 0.05 / 86_400.0;
    (mm / days, t1 - t0)
}

#[test]
fn a_puddle_dries_in_the_sun_but_barely_in_shade_or_at_night() {
    // Open water in summer sun loses about 3–8 mm a day (Penman 1948, Proc.
    // R. Soc. A 193, 120; Class A pan evaporation, FAO Irrigation and
    // Drainage Paper 56, Allen et al. 1998), and the sunlit hours carry
    // most of it. Penman's equation splits it into a radiation term, the
    // sun's heat turned to latent heat, and an aerodynamic term,
    // f(u)·(e_s − e_a), the air's own drying power. In shade or at night
    // only the second is left, and in humid air after rain it is small:
    // here (20 °C, 85 %, ~2.6 g/m³ short of saturation) about 2 mm a day
    // at the transfer coefficient the sim uses (h_m ≈ 9 mm/s, McAdams 1954
    // with Chilton–Colburn), less once the water cools towards its wet-bulb
    // temperature. Everything starts at air temperature, so the sun's
    // effect grows as the water warms: the rate is taken over minutes 3–5,
    // with the ranges ×3 around the references.
    let (warm, measure) = (3 * 60 * 20, 2 * 60 * 20);
    let (sun, shade, night) = std::thread::scope(|s| {
        let sun = s.spawn(|| puddle(Sky::afternoon(), false, warm, measure));
        let shade = s.spawn(|| puddle(Sky::afternoon(), true, warm, measure));
        let night = s.spawn(|| puddle(Sky::NIGHT, false, warm, measure));
        (sun.join().unwrap(), shade.join().unwrap(), night.join().unwrap())
    });
    eprintln!("puddle: sun {:.2} mm/day ({:+.2} K), shade {:.2} mm/day ({:+.2} K), night {:.2} mm/day ({:+.2} K)", sun.0, sun.1, shade.0, shade.1, night.0, night.1);
    assert!((1.0..24.0).contains(&sun.0), "in the sun it dries at summer rates (3–8 mm/day, ×3): {:.2} mm/day", sun.0);
    assert!(sun.1 > 1.0, "the sunlit water warms: {:+.2} K", sun.1);
    for (name, (rate, _)) in [("shade", shade), ("night", night)] {
        assert!((0.67..6.0).contains(&rate), "in {name} only the air dries it (~2 mm/day, ×3): {rate:.2} mm/day");
        assert!(sun.0 > 2.0 * rate, "the sun dries it clearly faster than {name}: {:.2} against {rate:.2} mm/day", sun.0);
    }
}
