//! How tall are the campfire's flames, and how hot is its plume? Against
//! Heskestad's flame height and McCaffrey's centreline temperatures.
//!
//!   cargo run --release -p fv-core --example flame_height -- [packets|field]
//!
//! Env: TICKS (default 900), N (box side, default 40), NZ (default 40).
//!
//! - Heat release: chemical energy that went away, per second.
//! - Flame: a cell whose flame released more than 200 kW/m³ this tick
//!   (Smokeview's default for drawing fire), or whose gas is above 600 °C. The flame top each tick is the highest such cell
//!   above the fire's footprint (±2 cells); the flame height is the median
//!   top (Heskestad's L is the 50 % intermittency height), measured from
//!   the fuel bed's base, as Heskestad's is.
//! - Heat release by height: where flames released their heat (from the
//!   flame light each burning cell gave, which is 35 % of its heat).
//! - Centreline: time-averaged gas temperature over the footprint's middle
//!   2×2 columns, by height, beside McCaffrey's (1979) correlation.

use fv_core::rad::{Radiation, Sky};
use fv_core::scene::campfire;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::units::joules;

/// Heat release a cell needs to show as flame: 200 kW/m³, Smokeview's
/// default for drawing fire from FDS output, over a 1 L cell and a 50 ms
/// tick (10 J), µJ.
const VISIBLE_UJ: f64 = 200e3 * 1e-3 * 0.05 * 1e6;

fn env(k: &str, d: usize) -> usize { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }

/// McCaffrey (1979) centreline temperature rise, K, at height z (m) above a
/// fire releasing q (kW), ambient t0 (K): 2g·ΔT/T₀ = (κ/C)²·(z/Q^⅖)^(2η−1).
pub fn mccaffrey(z: f64, q: f64, t0: f64) -> f64 {
    let s = z / q.powf(0.4);
    let (k, eta) = if s < 0.08 { (6.8, 0.5) } else if s < 0.2 { (1.9, 0.0) } else { (1.1, -1.0 / 3.0) };
    (k / 0.9f64).powi(2) * s.powf(2.0 * eta - 1.0) * t0 / (2.0 * 9.81)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = if args.first().map(String::as_str) == Some("field") { Radiation::Field } else { Radiation::Packets };
    let (n, nz, ticks) = (env("N", 40), env("NZ", 40), env("TICKS", 900) as u64);
    let (torch_j, torch_ticks) = (500, 200);
    let s = campfire(n, nz);
    let (cx, cy, size) = s.fire;
    let base_z = 4usize; // the fuel bed's bottom layer
    let mut sim = Sim::new(s.world, Sky::NIGHT);
    sim.radiation = mode;
    sim.flames = Some(Vec::new());
    if let Ok(v) = std::env::var("WIND") { let v: i64 = v.parse().unwrap(); sim.flow.wind_mean = [v, v / 4] }
    let mut seq = 0;
    for tick in 0..torch_ticks { for &(x, y, z) in &s.torch {
        sim.schedule(Scheduled { tick, peer: 0, seq, command: Command::Heat { x: x as u16, y: y as u16, z: z as u16, energy: joules(torch_j) } });
        seq += 1;
    }}
    let ambient = sim.world.ambient.0 as f64 / 1000.0;
    let mut last_chem = sim.totals().chemical - sim.books.edge_chemical();
    let (mut hrr_sum, mut count) = (0.0, 0usize);
    let mut tops = Vec::new();
    let mut heat_z = vec![0.0f64; nz];
    let mut temp_z = vec![0.0f64; nz];
    let mut hot_frac = vec![0.0f64; nz];
    let mut max_z = vec![0.0f64; nz];
    let mut up_z = vec![0.0f64; nz];
    let mut fuel_z = vec![0.0f64; nz];
    // Upward flux through the top face of each layer, over the footprint ±4:
    // [total gas kg/s, O2 kg/s, fuel kg/s, heat above ambient kW].
    let mut flux_z = vec![[0.0f64; 4]; nz];
    println!("campfire ({mode:?}) tick | heat release kW | flame top m | gap");
    for tick in 1..=ticks {
        sim.step();
        let chem = sim.totals().chemical - sim.books.edge_chemical();
        let hrr = (last_chem - chem) as f64 * 1e-6 / 0.05 / 1000.0;
        last_chem = chem;
        let w = &sim.world;
        let flames = sim.flames.as_ref().unwrap();
        let mut top = 0usize;
        for z in base_z..nz {
            let mut any = false;
            for y in cy.saturating_sub(2)..(cy + size + 2).min(w.ny) { for x in cx.saturating_sub(2)..(cx + size + 2).min(w.nx) {
                let i = w.idx(x, y, z);
                let c = &w.cells[i];
                if flames[i] as f64 / 0.35 > VISIBLE_UJ || (c.gas_volume_ml(&w.subs) > 0 && c.gas_temperature().celsius_f64() > 600.0) { any = true }
                if tick > torch_ticks { heat_z[z] += flames[i] as f64 / 0.35 * 1e-6 }
            }}
            if any { top = z + 1 }
            if tick > torch_ticks {
                let mut t = 0.0;
                let mut hot = 0.0;
                for y in cy + size / 2 - 1..cy + size / 2 + 1 { for x in cx + size / 2 - 1..cx + size / 2 + 1 {
                    let c = w.get(x, y, z);
                    let tg = c.gas_temperature().0 as f64 / 1000.0;
                    t += tg;
                    if tg > 873.0 { hot += 1.0 }
                }}
                temp_z[z] += t / 4.0;
                let mut m = 0.0f64;
                for y in 0..w.ny { for x in 0..w.nx {
                    let c = w.get(x, y, z);
                    m = m.max(c.gas_temperature().0 as f64 / 1000.0);
                    fuel_z[z] += c.gas[fv_core::substance::Gas::Fuel as usize].0 as f64 * 1e-6;
                }}
                max_z[z] += m;
                for y in cy.saturating_sub(4)..(cy + size + 4).min(w.ny) { for x in cx.saturating_sub(4)..(cx + size + 4).min(w.nx) {
                    use fv_core::transport::FaceVelocity;
                    let u = sim.flow.face(x as i64, y as i64, z as i64, 2) as f64 / 1000.0;
                    let donor = if u > 0.0 { w.get(x, y, z) } else if z + 1 < nz { w.get(x, y, z + 1) } else { continue };
                    let ml = donor.gas_volume_ml(&w.subs) as f64;
                    if ml <= 0.0 { continue }
                    // kg/m³ = µg/mL ÷ 1000; × u × 0.01 m²
                    let per = u * 0.01 / ml * 1e-3; // µg in the cell → kg/s through the face
                    flux_z[z][0] += donor.gas_mass().0 as f64 * per;
                    flux_z[z][1] += donor.gas[fv_core::substance::Gas::Oxygen as usize].0 as f64 * per;
                    flux_z[z][2] += donor.gas[fv_core::substance::Gas::Fuel as usize].0 as f64 * per;
                    flux_z[z][3] += donor.gas_mass().0 as f64 * per * 1.05 * (donor.gas_temperature().0 as f64 / 1000.0 - ambient);
                }}
                use fv_core::transport::FaceVelocity;
                up_z[z] += sim.flow.face((cx + size / 2) as i64, (cy + size / 2) as i64, z as i64, 2) as f64 / 1000.0;
                hot_frac[z] += hot / 4.0;
            }
        }
        let top_m = top.saturating_sub(base_z) as f64 * 0.1;
        if tick > torch_ticks { hrr_sum += hrr; count += 1; tops.push(top_m) }
        if tick % 50 == 0 { println!("{tick:5} | {hrr:7.1} | {top_m:4.1} | {}", sim.energy_gap()) }
        assert_eq!(sim.world.first_impossible(), None);
    }
    let c = count.max(1) as f64;
    {
        let w = &sim.world;
        let fresh = fv_core::substance::FRESH_AIR[fv_core::substance::Gas::Oxygen as usize].0 as f64;
        for z in base_z..base_z + 5 {
            let mut line = String::new();
            for y in cy.saturating_sub(1)..cy + size + 1 { for x in cx.saturating_sub(1)..cx + size + 1 {
                let c = w.get(x, y, z);
                let ml = c.gas_volume_ml(&w.subs).max(1) as f64;
                let o2 = c.gas[fv_core::substance::Gas::Oxygen as usize].0 as f64 / (fresh * ml / fv_core::substance::CELL_VOLUME_ML as f64);
                line += &format!("{}{:.0}/{:.0}/{:.0} ", ["", "*"][(c.substance != 0) as usize], o2 * 100.0, c.gas[fv_core::substance::Gas::Fuel as usize].0 as f64 / 1000.0, c.gas_temperature().celsius_f64());
            }
            line += "\n    "; }
            println!("z{z} O2 % of fresh / fuel mg / gas °C (* = fuel cell):\n    {line}");
        }
    }
    let q = hrr_sum / c;
    tops.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = tops.get(tops.len() / 2).copied().unwrap_or(0.0);
    let d = size as f64 * 0.1;
    let heskestad = 0.235 * q.powf(0.4) - 1.02 * d;
    let total_heat: f64 = heat_z.iter().sum();
    println!("mean heat release {q:.1} kW over {count} ticks; flames released {:.1} kW", total_heat / c / 0.05 / 1000.0);
    println!("flame height (median top above the bed's base) {median:.2} m; Heskestad L = 0.235 Q^0.4 - 1.02 D = {heskestad:.2} m (D = {d:.1} m)");
    println!("flame heat by height (z: share): {}", heat_z.iter().enumerate().filter(|(_, v)| **v / total_heat.max(1e-9) > 0.001).map(|(z, v)| format!("{z}:{:.3}", v / total_heat.max(1e-9))).collect::<Vec<_>>().join(" "));
    println!("upward flux through each layer's top (footprint ±4 cells): z top m | gas kg/s | O2 kg/s | fuel g/s | heat kW");
    for z in base_z..nz { let f = flux_z[z]; println!("  {:4.2} | {:6.3} | {:6.4} | {:6.2} | {:6.1}", (z + 1 - base_z) as f64 * 0.1, f[0] / c, f[1] / c, f[2] / c * 1000.0, f[3] / c) }
    println!("centreline: height above bed base m | mean gas °C | share of time > 600 °C | McCaffrey °C | hottest in layer °C | up m/s | fuel gas g in layer");
    for z in base_z..nz {
        let h = (z - base_z) as f64 * 0.1 + 0.05;
        println!("  {h:4.2} | {:6.0} | {:4.2} | {:6.0} | {:6.0} | {:5.2} | {:6.2}", temp_z[z] / c - 273.15, hot_frac[z] / c, ambient + mccaffrey(h, q, ambient) - 273.15, max_z[z] / c - 273.15, up_z[z] / c, fuel_z[z] / c);
    }
}
