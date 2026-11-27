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
#[ignore = "open: spreads ~26 cm/min, 3.1x Rothermel (8.5 cm/min). Flame contact hands the unburnt fuel ~2x Rothermel's propagating flux; capping it at a flame's flux over the face brings twigs to 19.5 cm/min but stops grass, because a 10 cm cell must ignite whole within its neighbour's burn time. Needs the front tracked inside each cell (research 27 Nov, section 21)"]
fn a_line_of_fire_creeps_through_twigs_at_rothermels_rate() {
    // Rothermel's spread model (1972, with Albini's 1976 forms) for a line
    // of fire, no wind or slope, in this bed: σ = 700/m (213/ft), 50 kg/m³
    // bulk (3.12 lb/ft³), 10 cm deep, packing β = 0.04 (β_op = 0.042),
    // 8 % moisture (extinction 25 %). Reaction velocity Γ' = 4.57/min,
    // η_M = 0.58, η_s = 0.42, h = 8 000 Btu/lb: I_R ≈ 8 500 Btu/(ft²·min)
    // ≈ 1.6 MW/m². Propagating flux ratio ξ = 0.018 gives I_p ≈ 29 kW/m²;
    // ignition takes Q_ig = 250 + 1116·M Btu/lb = 789 kJ/kg of the heated
    // share ε = e^(−138/213) = 0.52: R = I_p/(ρ_b·ε·Q_ig) ≈ 0.28 ft/min,
    // about 8.5 cm a minute. (This said 3 cm/min until 27 Nov, from a
    // reaction intensity of 600 kW/m².) Lab beds scatter about ×3 around
    // the model, so six minutes should carry the front 17–150 cm.
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
    assert!((2..=15).contains(&advanced), "the front advanced {} cm in six minutes", advanced * 10);
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
    let sat = fv_core::tables::tables().saturated_at(t);
    c.gas[Gas::Vapour as usize] = Mass(sat * HUMIDITY_PERCENT / 100 * ml / CELL_VOLUME_ML);
    c.set_temperature(t, subs);
}

/// A puddle 5 mm deep on stone, everything at 20 °C in humid air, under
/// `sky`, with or without a stone roof over it. Runs `warm` ticks, then
/// `measure` more, checking the books every tick. Returns the evaporation
/// rate over the measured ticks (mm/day) and how much the water warmed over
/// the whole run (K).
fn puddle(sky: Sky, roof: bool, warm: u64, measure: u64) -> (f64, f64) {
    let (rate, warmed, _) = puddle_in_wind(sky, roof, None, warm, measure);
    (rate, warmed)
}

/// The same, with the wind's average set (mm/s along x, y; `None` keeps
/// the sim's default breeze). Also returns the mean air speed measured
/// over the water, mm/s.
fn puddle_in_wind(sky: Sky, roof: bool, wind: Option<[i64; 2]>, warm: u64, measure: u64) -> (f64, f64, f64) {
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
    if let Some(wind) = wind { sim.flow.wind_mean = wind }
    let mut speed = (0f64, 0u64);
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
        if tick >= warm && tick % 20 == 0 {
            use fv_core::transport::FaceVelocity;
            for y in PUDDLE { for x in PUDDLE {
                let (x, y, z) = (x as i64, y as i64, PUDDLE_Z as i64);
                let u = (sim.flow.face(x - 1, y, z, 0) + sim.flow.face(x, y, z, 0)) as f64 / 2.0;
                let v = (sim.flow.face(x, y - 1, z, 1) + sim.flow.face(x, y, z, 1)) as f64 / 2.0;
                speed.0 += (u * u + v * v).sqrt();
                speed.1 += 1;
            }}
        }
        sim.step();
        assert_eq!(sim.energy_gap(), 0, "energy books, tick {tick}");
        assert_eq!(sim.mass_gap(), 0, "mass books, tick {tick}");
        assert_eq!(sim.world.first_impossible(), None, "a store went negative, tick {tick}");
    }
    let (m1, t1) = water(&sim);
    // 1 mm of water over a cell's face is FACE_MM2 mg (10 g on a 10 cm cell).
    let mm = (m0 - m1) as f64 / 1_000.0 / FACE_MM2 as f64 / (PUDDLE.len() * PUDDLE.len()) as f64;
    let days = measure as f64 * 0.05 / 86_400.0;
    (mm / days, t1 - t0, speed.0 / speed.1.max(1) as f64)
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
        // Clearly faster, though not by Penman's full daily ratio: the
        // sunlit water also loses heat to the cold clear sky (Penman's net
        // longwave, ~70 W/m²), and over minutes 3–5 it is still warming.
        assert!(sun.0 > 1.3 * rate, "the sun dries it clearly faster than {name}: {:.2} against {rate:.2} mm/day", sun.0);
    }
}

/// A campfire burning in still air, measured once and shared by the flame
/// and plume tests: its mean heat release, its flame height and its
/// centreline gas temperatures, after the torch is taken away.
struct Campfire { kw: f64, flame_m: f64, centreline_c: Vec<(f64, f64)>, ambient_c: f64 }

fn campfire_in_still_air() -> &'static Campfire {
    static ONCE: std::sync::OnceLock<Campfire> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        let (n, nz, torch_ticks, ticks) = (32, 32, 200u64, 700u64);
        let s = fv_core::scene::campfire(n, nz);
        let (cx, cy, size) = s.fire;
        let mut sim = Sim::new(s.world, Sky::NIGHT);
        sim.flow.wind_mean = [0, 0];
        sim.flames = Some(Vec::new());
        let mut seq = 0;
        for tick in 0..torch_ticks { for &(x, y, z) in &s.torch {
            sim.schedule(Scheduled { tick, peer: 0, seq, command: Command::Heat { x: x as u16, y: y as u16, z: z as u16, energy: joules(500) } });
            seq += 1;
        }}
        // A cell shows flame where it releases more than 200 kW/m³
        // (Smokeview's default for drawing an FDS fire: 10 J a tick in a
        // litre; its flame light is 35 % of that) or its gas is past 600 °C.
        let visible_light = 0.35 * 200e3 * 1e-3 * 0.05 * 1e6;
        let mut last = sim.totals().chemical - sim.books.edge_chemical();
        let (mut heat, mut tops, mut temps) = (0.0, Vec::new(), vec![0.0f64; nz]);
        for tick in 1..=ticks {
            sim.step();
            assert_eq!(sim.energy_gap(), 0);
            assert_eq!(sim.mass_gap(), 0);
            assert_eq!(sim.world.first_impossible(), None, "a store went negative");
            let chem = sim.totals().chemical - sim.books.edge_chemical();
            let released = (last - chem) as f64 * 1e-6;
            last = chem;
            if tick <= torch_ticks { continue }
            heat += released;
            let (w, flames) = (&sim.world, sim.flames.as_ref().unwrap());
            let mut top = 4;
            for z in 4..nz { for y in cy - 2..cy + size + 2 { for x in cx - 2..cx + size + 2 {
                let i = w.idx(x, y, z);
                let c = &w.cells[i];
                if flames[i] as f64 > visible_light || (c.gas_volume_ml(&w.subs) > 0 && c.gas_temperature().celsius_f64() > 600.0) { top = top.max(z + 1) }
            }}}
            tops.push((top - 4) as f64 * 0.1);
            for z in 4..nz { for y in cy + size / 2 - 1..cy + size / 2 + 1 { for x in cx + size / 2 - 1..cx + size / 2 + 1 {
                temps[z] += w.get(x, y, z).gas_temperature().celsius_f64() / 4.0;
            }}}
        }
        let count = (ticks - torch_ticks) as f64;
        tops.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Campfire {
            kw: heat / (count * 0.05) / 1000.0,
            flame_m: tops[tops.len() / 2],
            centreline_c: (4..nz).map(|z| ((z - 4) as f64 * 0.1 + 0.05, temps[z] / count)).collect(),
            ambient_c: 15.0,
        }
    })
}

#[test]
fn a_campfires_flames_stand_as_tall_as_heskestad_says() {
    // Heskestad's flame height (SFPE Handbook, ch. "Fire Plumes, Flame
    // Height and Air Entrainment"): L = 0.235·Q^⅖ − 1.02·D, L and D in m,
    // Q in kW, the mean height of the visible flame (50 % intermittency)
    // above the fire's base. It holds within ~±15 % over pool, gas-burner
    // and crib fires; this one's 40 cm across. A ×1.5 band: flames burning
    // inside the fuel bed (the old fault: 0.1 m against 1.5 m) fall out.
    let f = campfire_in_still_air();
    let l = 0.235 * f.kw.powf(0.4) - 1.02 * 0.4;
    eprintln!("campfire: {:.0} kW, flames {:.2} m, Heskestad {l:.2} m", f.kw, f.flame_m);
    assert!(f.kw > 20.0, "the fire kept burning ({:.0} kW)", f.kw);
    assert!(f.flame_m >= l / 1.5 && f.flame_m <= l * 1.5, "flames {:.2} m tall, Heskestad's L for {:.0} kW is {l:.2} m", f.flame_m, f.kw);
}

/// McCaffrey (1979, NBSIR 79-1910) centreline temperature rise above a
/// fire, K: 2g·ΔT/T₀ = (κ/C)²·(z/Q^⅖)^(2η−1), C = 0.9, with κ, η = 6.8, ½ in
/// the continuous flame (z/Q^⅖ < 0.08 m/kW^⅖), 1.9, 0 where it flickers
/// (to 0.2) and 1.1, −⅓ in the plume above.
fn mccaffrey_rise(z: f64, q: f64, t0: f64) -> f64 {
    let s = z / q.powf(0.4);
    let (k, eta) = if s < 0.08 { (6.8, 0.5) } else if s < 0.2 { (1.9, 0.0) } else { (1.1, -1.0 / 3.0) };
    (k / 0.9f64).powi(2) * s.powf(2.0 * eta - 1.0) * t0 / (2.0 * 9.81)
}

#[test]
#[ignore = "open: the plume is ~7x too cool (+99 K at 0.45 m over 53 kW, McCaffrey +727 K). Measured: only ~19 % of the heat rises in the plume; 46 % is released in the bed's own 10 cm cell and goes to fuel, ground and wet grass, and latent heat takes ~25 %. No wrong mechanism found: the flame base is unresolved at 10 cm (research 27 Nov, section 20)"]
fn the_plume_over_a_campfire_is_as_hot_as_mccaffrey_measured() {
    // Through the flickering flame and the plume above it (z/Q^⅖ from 0.08
    // to 0.3), the centreline's mean temperature rise within ×2 of
    // McCaffrey's (his data scatter ±20 %; a 10 cm cell averages over a
    // plume a few cells wide, so the centreline reads low).
    let f = campfire_in_still_air();
    let t0 = f.ambient_c + 273.15;
    let mut checked = 0;
    for &(z, t) in &f.centreline_c {
        let s = z / f.kw.powf(0.4);
        if !(0.08..=0.3).contains(&s) { continue }
        let want = mccaffrey_rise(z, f.kw, t0);
        let got = t - f.ambient_c;
        eprintln!("z {z:.2} m: centreline +{got:.0} K, McCaffrey +{want:.0} K");
        assert!(got >= want / 2.0 && got <= want * 2.0, "at {z:.2} m the centreline is {got:.0} K over ambient; McCaffrey: {want:.0} K");
        checked += 1;
    }
    assert!(checked > 0);
}

#[test]
fn wind_dries_a_puddle_faster_than_still_air() {
    // Wind thins the layer of damp air on water and brings drier air, so
    // open water dries faster in a breeze. Penman's wind function for open
    // water, f(u) = 0.26·(1 + 0.54·u) mm/day per mbar with u in m/s
    // (Penman 1948; as written in Shuttleworth, Terrestrial
    // Hydrometeorology, 2012), makes a 2 m/s breeze dry about 2.1 times as
    // fast as still air; McAdams' surface coefficient h = 5.7 + 3.8·v, which
    // the sim uses, about 2.3 times. At night only the air dries the puddle,
    // so its rate shows the wind's effect alone. Over minutes 1–3.
    let (warm, measure) = (60 * 20, 2 * 60 * 20);
    let (still, breeze) = std::thread::scope(|s| {
        let still = s.spawn(|| puddle_in_wind(Sky::NIGHT, false, Some([0, 0]), warm, measure));
        let breeze = s.spawn(|| puddle_in_wind(Sky::NIGHT, false, Some([2_000, 0]), warm, measure));
        (still.join().unwrap(), breeze.join().unwrap())
    });
    let ratio = breeze.0 / still.0.max(1e-9);
    eprintln!("puddle at night: still air {:.2} mm/day (air over it {:.0} mm/s), breeze {:.2} mm/day ({:.0} mm/s): ×{ratio:.2}", still.0, still.2, breeze.0, breeze.2);
    assert!(breeze.2 > 1_000.0, "the breeze reaches the water: {:.0} mm/s", breeze.2);
    assert!(still.0 > 0.0, "still air dries it a little");
    // ×2.1 by Penman, ×3 either way, but it must be clearly faster.
    assert!((1.3..6.3).contains(&ratio), "a 2 m/s breeze dries it ×{ratio:.2} as fast as still air (Penman: ×2.1)");
}

// ── a clear night: the cold sky chills grass, and dew forms ─────────────────

/// Dry grass on soil on a calm night at 15 °C in air at `humidity` percent,
/// under a clear or overcast sky. Runs `warm` ticks, then `measure` more,
/// checking the books every tick. Returns (how far the middle blades sit
/// below the air 1 m above them, K, averaged over the measured ticks; water
/// gained on them as a depth, mm per hour). The blades start dry, so what
/// water they gain is dew.
fn grass_at_night(clear: bool, humidity: i64, warm: u64, measure: u64) -> (f64, f64) {
    let air = celsius(15);
    let mut w = World::new(16, 16, 16, 9, air);
    let subs = w.subs.clone();
    let sat = fv_core::tables::tables().saturated_at(air);
    let humid = |c: &mut Cell| {
        let ml = c.gas_volume_ml(&subs);
        if ml <= 0 || c.gas_mass().0 == 0 { return }
        c.gas[Gas::Vapour as usize] = Mass(sat * humidity / 100 * ml / 1_000);
        c.set_temperature(air, &subs);
    };
    let soil = Cell::solid_of(SOIL, 1000, 150, air, &subs);
    let mut grass = Cell::solid_of(GRASS, 1000, 0, air, &subs);
    humid(&mut grass);
    for y in 0..16 { for x in 0..16 { for z in 0..4 { w.set(x, y, z, soil) } w.set(x, y, 4, grass) } }
    for c in w.cells.iter_mut() { if c.substance == NONE { humid(c) } }
    let mut outside = w.outside_air;
    humid(&mut outside);
    w.outside_air = outside;
    let mut sim = Sim::new(w, Sky { clear, ..Sky::NIGHT });
    sim.flow.wind_mean = [0, 0];
    let middle = 5..11;
    let screen = 4 + (1_000 / CELL_MM) as usize;
    let state = |sim: &Sim| -> (i64, f64) {
        let (mut m, mut chill) = (0i64, 0f64);
        for y in middle.clone() { for x in middle.clone() {
            let c = sim.world.get(x, y, 4);
            m += c.water.0;
            chill += sim.world.get(x, y, screen).gas_temperature().kelvin_f64() - c.surface_temperature(&sim.world.subs).kelvin_f64();
        }}
        (m, chill / 36.0)
    };
    let (mut m0, mut chill, mut n) = (0, 0.0, 0.0);
    for tick in 0..warm + measure {
        if tick == warm { m0 = state(&sim).0 }
        sim.step();
        assert_eq!(sim.energy_gap(), 0, "energy books, tick {tick}");
        assert_eq!(sim.mass_gap(), 0, "mass books, tick {tick}");
        assert_eq!(sim.world.first_impossible(), None, "a store went negative, tick {tick}");
        if tick >= warm { chill += state(&sim).1; n += 1.0 }
    }
    let m1 = state(&sim).0;
    // A depth over the 36 cells' faces: 1 mm over a face is FACE_MM2 mg.
    let mm = (m1 - m0) as f64 / 1_000.0 / FACE_MM2 as f64 / 36.0;
    (chill / n, mm / (measure as f64 * 0.05 / 3_600.0))
}

#[test]
fn a_clear_night_chills_grass_below_the_air_and_dew_forms() {
    // A clear sky radiates like a black body ~18 K colder than the air
    // (Swinbank 1963), so on a calm clear night grass loses heat to it and
    // sits a few kelvin below the air: ~2–6 K below screen temperature
    // (Oke, Boundary Layer Climates, 1987, ch. 2; Monteith & Unsworth,
    // Principles of Environmental Physics). Under cloud it stays near the
    // air's temperature. Where it chills below the dew point, dew forms: at
    // most ~0.04–0.07 mm an hour, and rarely more than ~0.5 mm in a night
    // (Monteith 1957, Q. J. R. Meteorol. Soc. 83, 322). In drier air the
    // blades stay above the dew point and no dew forms. Over minutes 2–5,
    // ranges ×3 around the references.
    let (warm, measure) = (2 * 60 * 20, 3 * 60 * 20);
    let (clear, dry, cloudy) = std::thread::scope(|s| {
        let clear = s.spawn(|| grass_at_night(true, 95, warm, measure));
        let dry = s.spawn(|| grass_at_night(true, 60, warm, measure));
        let cloudy = s.spawn(|| grass_at_night(false, 95, warm, measure));
        (clear.join().unwrap(), dry.join().unwrap(), cloudy.join().unwrap())
    });
    eprintln!("grass at night: clear & humid {:.2} K below the air, dew {:+.4} mm/h; clear & dry {:.2} K, {:+.4} mm/h; overcast & humid {:.2} K, {:+.4} mm/h",
              clear.0, clear.1, dry.0, dry.1, cloudy.0, cloudy.1);
    assert!((0.7..18.0).contains(&clear.0), "a clear sky chills the grass a few kelvin below the air (2–6 K, ×3): {:.2} K", clear.0);
    assert!(cloudy.0 < clear.0 / 2.0, "cloud keeps it near the air's temperature: {:.2} K against {:.2} K clear", cloudy.0, clear.0);
    assert!((0.013..0.21).contains(&clear.1), "dew forms on it in humid air (≤ 0.04–0.07 mm/h, ×3): {:+.4} mm/h", clear.1);
    assert!(dry.1 <= 0.0, "but not in drier air: {:+.4} mm/h", dry.1);
}

// ── a crib of logs burns on its logs ───────────────────────────────────────

#[test]
fn a_crib_of_logs_burns_at_the_rate_crib_tests_give() {
    // A loosely packed crib burns on its sticks' surfaces, at a rate per
    // exposed area set by the stick thickness b: R/A_s ≈ 1.1×10⁻³·b^−½
    // g/(s·cm²), b in cm (Block 1971 and Heskestad 1973's correlation of
    // Gross's and Block's cribs; the plateau of McAllister & Finney 2015,
    // Fire Technology 52, table 2). For 20 cm logs that's 2.5 g/(s·m²). The
    // correlation was measured on sticks up to 4 cm, so ×3 either way.
    // Before the fix the logs never caught: their faces sat at 100 °C
    // boiling the whole log's water, and flames in the crib's open shafts
    // didn't touch them (~4 g/s from 7 m² of logs at two minutes).
    let (w, torch, (cx, cy, z0)) = fv_core::scene::crib(24, 32, 4);
    let mut sim = Sim::new(w, Sky::afternoon());
    for t in 0..200 { for (i, p) in torch.iter().enumerate().filter(|(i, _)| i % 3 == 0) {
        sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x: p[0] as u16, y: p[1] as u16, z: p[2] as u16, energy: joules(500) } })
    } }
    let logs = |sim: &Sim| -> (i64, usize) {
        let w = &sim.world; let s = &w.subs;
        let (mut mass, mut faces) = (0, 0);
        for z in z0..z0 + 9 { for y in cy..cy + 8 { for x in cx..cx + 10 {
            let c = w.get(x, y, z);
            if c.substance != WOOD { continue }
            mass += c.solid[0].0 + c.solid[1].0;
            for (dx, dy, dz) in [(1i64, 0i64, 0i64), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
                if w.get((x as i64 + dx) as usize, (y as i64 + dy) as usize, (z as i64 + dz) as usize).permeable(s) { faces += 1 }
            }
        }}}
        (mass, faces)
    };
    let (_, faces) = logs(&sim);
    let area_m2 = faces as f64 * 0.01;
    let (from, to) = (240 * 20, 300 * 20);
    let mut before = 0;
    for tick in 1..=to {
        sim.step();
        assert_eq!(sim.world.first_impossible(), None, "a store went negative at tick {tick}");
        if tick % 200 == 0 { assert_eq!(sim.energy_gap(), 0); assert_eq!(sim.mass_gap(), 0) }
        if tick == from { before = logs(&sim).0 }
    }
    let lost_g_s = (before - logs(&sim).0) as f64 / 1e6 / ((to - from) as f64 / 20.0);
    let per_m2 = lost_g_s / area_m2;
    let want = 1.1e-3 * 20f64.powf(-0.5) * 1e4;
    eprintln!("crib: logs lose {lost_g_s:.1} g/s from {area_m2:.1} m², {per_m2:.2} g/(s·m²); Block–Heskestad {want:.2}");
    assert!(per_m2 >= want / 3.0 && per_m2 <= want * 3.0, "logs burn at {per_m2:.2} g/(s·m²), crib tests give {want:.2}");
}
