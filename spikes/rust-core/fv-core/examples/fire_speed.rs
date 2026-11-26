//! How fast a line of fire creeps through a fuel bed in still air, at
//! whatever cell size the build has (`--features cell-5cm`). The setup is
//! the same in metres at any cell size, so the two can be compared:
//!
//! - **grass**: a 2.8 m square bed 10 cm deep on 40 cm of soil, 100 ‰
//!   moisture (as `probe_grass`), lit along one edge for 5 s;
//! - **kindling**: a 1.6 m square bed of 6 mm twigs 10 cm deep on 40 cm of
//!   stone, 80 ‰ moisture (as the Rothermel test in `tests/reality.rs`).
//!
//! Rothermel (1972) for these beds, no wind: grass ≈ 0.53 m/min, kindling
//! ≈ 3 cm/min; lab beds scatter about ×3 around the model.
//!
//!     cargo run --release --example fire_speed -- grass 120
//!     FV_FLAME_LEAN_PERMILLE=300 cargo run --release --features cell-5cm --example fire_speed -- kindling 360
//!
//! `WIND` (mm/s) sets a wind along the line's direction of travel; unset is still air.
use fv_core::cell::{Cell, VOLATILE};
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::{World, TICK};

/// Metres to cells.
fn cells(mm: i64) -> usize { (mm / CELL_MM) as usize }

fn main() {
    let fuel = std::env::args().nth(1).unwrap_or("grass".into());
    let seconds: i64 = std::env::args().nth(2).map(|v| v.parse().unwrap()).unwrap_or(120);
    let wind: i64 = std::env::var("WIND").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let (side_mm, edge_mm, sub, moisture, ground, ground_moist, lost_permille) = match fuel.as_str() {
        // As probe_grass: 3.2 m box, bed from 0.2 m to 3.0 m; a cell counts
        // as reached once it has lost a tenth of its volatile matter.
        "grass" => (3_200, 200, GRASS, 100, SOIL, 150, 100),
        // As the Rothermel test: 2.4 m box, bed from 0.4 m to 2.0 m; reached
        // once it's lost a fortieth (the test's 39 of 40 g).
        "kindling" => (2_400, 400, KINDLING, 80, STONE, 2, 25),
        f => panic!("unknown fuel {f}: grass or kindling"),
    };
    let n = cells(side_mm);
    let nz = cells(1_600);
    let (lo, hi) = (cells(edge_mm), cells(side_mm - edge_mm));
    let (z0, z1) = (cells(400), cells(500)); // the bed: 40–50 cm up
    let mut w = World::new(n, n, nz, 9, celsius(15));
    let subs = w.subs.clone();
    let floor = Cell::solid_of(ground, 1000, ground_moist, celsius(15), &subs);
    let bed = Cell::solid_of(sub, 1000, moisture, celsius(15), &subs);
    for z in 0..z0 { for y in 0..n { for x in 0..n { w.set(x, y, z, floor) } } }
    for z in z0..z1 { for y in lo..hi { for x in lo..hi { w.set(x, y, z, bed) } } }
    let full = bed.solid[VOLATILE].0;
    let mut sim = Sim::new(w, Sky::NIGHT);
    sim.flow.wind_mean = [0, wind];
    // A torch along the first 10 cm of the bed for 5 s: 400 J a tick per
    // litre of fuel lit (as the probes at 10 cm).
    let torch = joules(400).0 * CELL_VOLUME_ML / 1_000;
    let lit = cells(100);
    let mut seq = 0;
    for t in 0..(5_000 / TICK.0) as u64 { for z in z0..z1 { for y in lo..lo + lit { for x in lo..hi {
        seq += 1;
        sim.schedule(Scheduled { tick: t, peer: 0, seq, command: Command::Heat { x: x as u16, y: y as u16, z: z as u16, energy: Energy(torch) } });
    }}}}
    // Measure the front over the bed's middle half across.
    let (mx0, mx1) = (lo + (hi - lo) / 4, hi - (hi - lo) / 4);
    let ticks = seconds * 1_000 / TICK.0;
    let mut ran = ticks;
    let bed_mm = ((hi - lo) as i64 * CELL_MM) as f64;
    let every = 5_000 / TICK.0;
    let debug = std::env::var("DEBUG").is_ok();
    let mut track: Vec<(f64, f64)> = Vec::new();
    let started = std::time::Instant::now();
    eprintln!("{fuel}: {n}×{n}×{nz} cells of {CELL_MM} mm, wind {wind} mm/s");
    for tick in 0..ticks {
        sim.step();
        if debug && (tick + 1) % (1_000 / TICK.0) == 0 && tick < 40 * 1_000 / TICK.0 {
            let w = &sim.world;
            let x = (lo + hi) / 2;
            print!("{:>3}s", (tick + 1) * TICK.0 / 1_000);
            for dy in 0..4 { for z in z0..z1 {
                let c = w.get(x, lo + lit - 1 + dy, z);
                print!(" | y+{dy} z{}: skin {:>4.0} gas {:>4.0} vol {:>3}% fuel {:>5}", z - z0, c.surface_temperature(&w.subs).celsius_f64(), c.gas_temperature().celsius_f64(), c.solid[VOLATILE].0 * 100 / full.max(1), c.gas[Gas::Fuel as usize].0);
            }}
            println!();
        }
        if (tick + 1) % every != 0 { continue }
        let w = &sim.world;
        let reached = |y: usize| (z0..z1).any(|z| (mx0..mx1).any(|x| w.get(x, y, z).solid[VOLATILE].0 < full * (1_000 - lost_permille) / 1_000));
        let front = (lo..hi).filter(|&y| reached(y)).max();
        let mm = front.map(|y| ((y - lo + 1) as i64 * CELL_MM) as f64).unwrap_or(0.0);
        let s = ((tick + 1) * TICK.0) as f64 / 1_000.0;
        track.push((s, mm));
        println!("{s:>5.0} s  front {:>6.0} mm  gap {} {}", mm, sim.energy_gap(), sim.mass_gap());
        assert_eq!(sim.energy_gap(), 0);
        assert_eq!(sim.mass_gap(), 0);
        assert_eq!(sim.world.first_impossible(), None, "a store went negative");
        if mm >= bed_mm { ran = tick + 1; break }
    }
    let ms_per_tick = started.elapsed().as_secs_f64() * 1_000.0 / ran as f64;
    // Speed: least-squares slope of the front once the torch is out (from
    // 10 s on), beyond the torch's own reach (the first 20 cm) and before
    // the bed's far 20 cm.
    let pts: Vec<_> = track.iter().copied().filter(|&(s, mm)| s >= 10.0 && mm >= 200.0 && mm <= bed_mm - 200.0).collect();
    let rate = if pts.len() >= 2 {
        let k = pts.len() as f64;
        let (sx, sy) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
        let (mx, my) = (sx / k, sy / k);
        let num: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
        let den: f64 = pts.iter().map(|p| (p.0 - mx) * (p.0 - mx)).sum();
        num / den * 60.0 / 1_000.0
    } else { f64::NAN };
    let last = track.last().map(|p| p.1).unwrap_or(0.0);
    println!("RESULT fuel={fuel} cell_mm={CELL_MM} wind={wind} front_end_mm={last:.0} rate_m_per_min={rate:.3} points={} ms_per_tick={ms_per_tick:.1}", pts.len());
}
