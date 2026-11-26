//! How big and how bright is the campfire? Heat release, visible light and
//! illuminance on the ground near it, against measurements.
//!
//!   cargo run --release -p fv-core --example fire_budget -- [packets|field]
//!
//! Heat release rate = chemical energy that went away (burnt), per second.
//! Visible = light the sim emitted in the red, green and blue bands.
//! Illuminance: the light arriving on grass tops 1 m and 2 m from the fire
//! (W/m² per band) times the luminous efficacy of each band for glowing soot
//! at ~1 400 K (683·V(λ) averaged over the band, weighted by the source's
//! spectrum: red 600–700 nm ≈ 70 lm/W, green 500–600 nm ≈ 580 lm/W, blue
//! < 500 nm ≈ 120 lm/W).
//!
//! It also prints where the visible light is emitted (by height) and where
//! it lands (the fire's own fuel, the ground, the wall, smoke), and how much
//! unburnt fuel gas rises out of the fuel bed.

use fv_core::rad::{Radiation, Sky};
use fv_core::scene::campfire;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::units::joules;

const LM_PER_W: [f64; 3] = [70.0, 580.0, 120.0];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = if args.first().map(String::as_str) == Some("field") { Radiation::Field } else { Radiation::Packets };
    let (n, nz, torch_j, torch_ticks, ticks) = (40, 32, 500, 200, 900);
    let s = campfire(n, nz);
    let (cx, cy, size) = s.fire;
    let mut sim = Sim::new(s.world, Sky::NIGHT);
    sim.radiation = mode;
    let mut seq = 0;
    for tick in 0..torch_ticks { for &(x, y, z) in &s.torch {
        sim.schedule(Scheduled { tick, peer: 0, seq, command: Command::Heat { x: x as u16, y: y as u16, z: z as u16, energy: joules(torch_j) } });
        seq += 1;
    }}
    // Grass tops at a given distance (cells) from the fire's edge, on the side away from the wall.
    let ring = |d: usize| -> Vec<usize> {
        let w = &sim.world;
        let mut v = Vec::new();
        for y in cy + size..w.ny { for x in 0..w.nx {
            let dx = if x < cx { cx - x } else if x >= cx + size { x + 1 - cx - size } else { 0 };
            let dy = y + 1 - cy - size;
            let r = ((dx * dx + dy * dy) as f64).sqrt();
            if (r - d as f64).abs() <= 0.5 { v.push(w.idx(x, y, 4)) }
        }}
        v
    };
    let (r1, r2) = (ring(10), ring(20));
    let mut last_chem = sim.totals().chemical - sim.books.edge_chemical();
    let (mut sum, mut count) = ([0.0f64; 6], 0);
    let mut lumens_sum = 0.0;
    let mut where_ = [0.0f64; 5];
    let mut by_z = vec![0.0f64; sim.world.nz];
    let mut ground_by_r = vec![0.0f64; 30];
    let mut ground_n = vec![0usize; 30];
    println!("campfire ({mode:?}): tick | heat release kW | visible W (R G B) | lux at 1 m | lux at 2 m");
    for tick in 1..=ticks {
        sim.step();
        let chem = sim.totals().chemical - sim.books.edge_chemical();
        let hrr = (last_chem - chem) as f64 * 1e-6 / 0.05 / 1000.0;
        last_chem = chem;
        let vis = (0..3).map(|b| sim.light.glow.iter().map(|g| g[b] as f64).sum::<f64>() * 1e-6 / 0.05).collect::<Vec<_>>();
        let lux = |cells: &[usize]| cells.iter().map(|&i| (0..3).map(|b| sim.light.incident[i][b] as f64 * 1e-6 / 0.05 / 0.01 * LM_PER_W[b]).sum::<f64>()).sum::<f64>() / cells.len().max(1) as f64;
        let (l1, l2) = (lux(&r1), lux(&r2));
        if tick > torch_ticks { sum[0] += hrr; sum[1] += vis.iter().sum::<f64>(); sum[2] += l1; sum[3] += l2; sum[4] = sum[4].max(hrr); lumens_sum += (0..3).map(|b| vis[b] * LM_PER_W[b]).sum::<f64>(); count += 1 }
        if tick > torch_ticks {
            let w = &sim.world;
            for z in 0..w.nz { for y in 0..w.ny { for x in 0..w.nx {
                let i = w.idx(x, y, z);
                by_z[z] += sim.light.glow[i].iter().map(|&v| v as f64).sum::<f64>();
                let e: f64 = sim.light.incident[i].iter().map(|&v| v as f64).sum();
                if z == 4 && y >= cy + size {
                    let dx = if x < cx { cx - x } else if x >= cx + size { x + 1 - cx - size } else { 0 };
                    let r = (((dx * dx) + (y + 1 - cy - size).pow(2)) as f64).sqrt().round() as usize;
                    if r < 30 { ground_by_r[r] += e; ground_n[r] += 1 }
                }
                if e == 0.0 { continue }
                let c = w.get(x, y, z);
                let in_fire = x >= cx && x < cx + size && y >= cy && y < cy + size;
                let k = if in_fire && z >= 4 && z < 4 + size { 0 } else if c.substance == fv_core::substance::GRASS && z == 4 { 1 }
                    else if c.substance == fv_core::substance::STONE && z > 4 { 2 } else if c.substance == fv_core::substance::NONE { 3 } else { 4 };
                where_[k] += e;
            }}}
        }
        if tick % 50 == 0 { println!("{tick:5} | {hrr:7.1} | {:6.1} ({:.1} {:.2} {:.3}) | {l1:6.2} | {l2:6.2}", vis.iter().sum::<f64>(), vis[0], vis[1], vis[2]) }
    }
    let c = count.max(1) as f64;
    let total: f64 = where_.iter().sum();
    let zt: f64 = by_z.iter().sum();
    {
        let w = &sim.world; let f = fv_core::substance::Gas::Fuel as usize; let o = fv_core::substance::Gas::Oxygen as usize;
        let mut by_z = vec![(0i64, 0i64, 0.0f64); w.nz];
        for z in 0..w.nz { for y in 0..w.ny { for x in 0..w.nx { let c = w.get(x, y, z); by_z[z].0 += c.gas[f].0; by_z[z].1 += c.gas[o].0; by_z[z].2 = by_z[z].2.max(c.gas_temperature().celsius_f64()); }}}
        println!("fuel gas mg / O2 g / max gas °C by z: {}", by_z.iter().enumerate().take(16).map(|(z, v)| format!("{z}:{}/{}/{:.0}", v.0 / 1000, v.1 / 1_000_000, v.2)).collect::<Vec<_>>().join(" "));
        println!("fuel that left the box: {} g", -sim.books.edge_gas[f] / 1_000_000);
    }
    println!("visible emitted by height (z, share): {}", by_z.iter().enumerate().filter(|(_, v)| **v > 0.0).map(|(z, v)| format!("{z}:{:.3}", v / zt)).collect::<Vec<_>>().join(" "));
    println!("ground light by distance from the fire's edge (cells: W/m^2 visible): {}", (1..25).map(|r| format!("{r}:{:.3}", ground_by_r[r] / ground_n[r].max(1) as f64 * 1e-6 / 0.05 / 0.01 / c)).collect::<Vec<_>>().join(" "));
    println!("where visible light arrived (share of all arrivals): fire footprint {:.3}, ground {:.3}, wall {:.3}, smoke/air {:.3}, other solid {:.3}",
             where_[0] / total, where_[1] / total, where_[2] / total, where_[3] / total, where_[4] / total);
    println!("after the torch: mean heat release {:.1} kW (peak {:.1}), visible {:.1} W, {:.0} lm ({:.3} lm/W of heat); {:.2} lux at 1 m, {:.2} lux at 2 m",
             sum[0] / c, sum[4], sum[1] / c, lumens_sum / c, lumens_sum / sum[0].max(1e-9) / 1000.0, sum[2] / c, sum[3] / c);
}
