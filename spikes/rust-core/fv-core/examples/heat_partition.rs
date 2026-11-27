//! Where a campfire's heat goes (still air, the plume test's setup): heat
//! released against what leaves the box as radiation and through its edges,
//! and what is stored in the gas, the fuel and the ground. Also the plume's
//! convective heat through a few heights over the fire.
//!     cargo run --release -p fv-core --example heat_partition
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::transport::FaceVelocity;
use fv_core::units::joules;

fn main() {
    let (n, nz, torch_ticks, ticks) = (32usize, 32usize, 200u64, 700u64);
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
    let amb = sim.world.ambient;
    // [gas, fuel solids, ground solids, other solids] thermal energy, J.
    let stores = |sim: &Sim| -> [f64; 4] {
        let w = &sim.world; let mut o = [0.0; 4];
        for c in &w.cells {
            o[0] += (c.energy.0 - c.latent().0 - (c.gas_capacity() * sim.world.ambient).0) as f64 * 1e-6;
            let k = match c.substance { KINDLING | WOOD => 1, STONE | SOIL | GRASS => 2, _ => 3 };
            o[k] += (c.solid_energy.0 + c.skin.0) as f64 * 1e-6;
        }
        o
    };
    let _ = amb;
    let (mut chem0, mut rad0, mut edge0, mut st0) = (0i128, 0i128, 0i128, [0.0; 4]);
    let mut flame_light = 0.0f64; let (mut edges, mut edges_latent, mut edges_mass) = ([0.0f64; 4], [0.0f64; 4], [0.0f64; 4]);
    let heights = [2usize, 4, 7, 10, 14, 18, 22, 26];
    let mut conv = [0.0f64; 8]; let mut mix = [0.0f64; 8]; let mut side_mix = [0.0f64; 8]; let mut conv_near = [0.0f64; 8];
    for tick in 1..=ticks {
        sim.step();
        if tick == torch_ticks {
            chem0 = sim.totals().chemical - sim.books.edge_chemical();
            rad0 = sim.books.radiation_out - sim.books.radiation_in; edge0 = sim.books.edge_energy; st0 = stores(&sim);
        }
        if tick <= torch_ticks { continue }
        flame_light += sim.flames.as_ref().unwrap().iter().sum::<i64>() as f64 * 1e-6;
        let w = &sim.world;
        for b in w.brick_ids() {
            let (ox, oy, oz) = w.brick_origin(b);
            let t = &w.tallies[b];
            let top = oz + 8 >= w.nz;
            let side = ox == 0 || oy == 0 || ox + 8 >= w.nx || oy + 8 >= w.ny;
            let k = if top && !side { 0 } else if top { 1 } else if side { 2 } else { 3 };
            edges[k] -= t.energy_in as f64 * 1e-6;
            let lat: i64 = t.gas_in[Gas::Vapour as usize];
            edges_latent[k] -= (fv_core::units::Mass(lat) * LATENT_HEAT_VAPOUR).0 as f64 * 1e-6;
            edges_mass[k] -= t.gas_in.iter().sum::<i64>() as f64 * 1e-9;
        }
        let sides = fv_core::transport::sides(w);
        for (h, &dz) in heights.iter().enumerate() {
            let z = 4 + dz - 1;
            for y in 0..w.ny { for x in 0..w.nx {
                let (i, j) = (w.idx(x, y, z), w.idx(x, y, z + 1));
                let f = fv_core::transport::diffuse_face(&w.cells[i], &sides[i], &w.cells[j], &sides[j], 0, 2);
                mix[h] += (f.carried.0 + f.energy.0) as f64 * 1e-6 * 20.0 / 1e3;
            }}
            // sideways out of the column over the fire at this height
            for y in cy.saturating_sub(2)..cy + size + 2 { for x in [cx - 3, cx + size + 1] {
                let (i, j) = (w.idx(x, y, z + 1), w.idx(x + 1, y, z + 1));
                let f = fv_core::transport::diffuse_face(&w.cells[i], &sides[i], &w.cells[j], &sides[j], 0, 0);
                side_mix[h] += if x < cx { -(f.carried.0 + f.energy.0) } else { f.carried.0 + f.energy.0 } as f64 * 1e-6 * 20.0 / 1e3;
            }}
        }
        for (h, &dz) in heights.iter().enumerate() {
            let z = 4 + dz - 1; // top face of layer z: dz cells above the bed's base
            for y in 0..w.ny { for x in 0..w.nx {
                let u = sim.flow.face(x as i64, y as i64, z as i64, 2) as f64 / 1000.0;
                let donor = if u > 0.0 { w.get(x, y, z) } else { w.get(x, y, z + 1) };
                let ml = donor.gas_volume_ml(&w.subs) as f64;
                if ml <= 0.0 { continue }
                // heat above ambient in the donor, J, times the share of it crossing per second
                let excess = (donor.energy.0 - donor.latent().0 - (donor.gas_capacity() * w.ambient).0) as f64 * 1e-6;
                let near = x + 6 >= cx && x < cx + size + 6 && y + 6 >= cy && y < cy + size + 6;
                let kw = excess * u * 0.01 / (ml * 1e-6) / 1e3;
                conv[h] += kw;
                if near { conv_near[h] += kw }
            }}
        }
    }
    let secs = (ticks - torch_ticks) as f64 * 0.05;
    let q = (chem0 - (sim.totals().chemical - sim.books.edge_chemical())) as f64 * 1e-6 / secs / 1e3;
    let rad = ((sim.books.radiation_out - sim.books.radiation_in) - rad0) as f64 * 1e-6 / secs / 1e3;
    let edge = -((sim.books.edge_energy - edge0) as f64) * 1e-6 / secs / 1e3;
    let st = stores(&sim);
    for (k, name) in ["top", "top corner/edge", "sides", "inside"].iter().enumerate() { println!("  out through {name} bricks: {:.1} kW (latent {:.1} kW), gas {:.3} kg/s", edges[k] / secs / 1e3, edges_latent[k] / secs / 1e3, edges_mass[k] / secs) }
    let d: Vec<f64> = (0..4).map(|k| (st[k] - st0[k]) / secs / 1e3).collect();
    println!("over {secs:.0} s after the torch: heat released {q:.1} kW (flames {:.1} kW, their light {:.1} kW)", flame_light / 0.35 / secs / 1e3, flame_light / secs / 1e3);
    println!("  left as radiation {rad:.1} kW ({:.0} %)", 100.0 * rad / q);
    println!("  left through the box's edges (gas carrying heat) {edge:.1} kW ({:.0} %)", 100.0 * edge / q);
    println!("  stored: gas (sensible, above ambient) {:.1} kW, fuel {:.1} kW, ground {:.1} kW, other {:.1} kW", d[0], d[1], d[2], d[3]);
    println!("  sum {:.1} kW (released − pyrolysis heat etc. is the rest)", rad + edge + d.iter().sum::<f64>());
    for (h, &dz) in heights.iter().enumerate() { { let k = (ticks - torch_ticks) as f64; println!("  heat carried up through {:.1} m: whole layer {:.1} kW ({:.0} % of release), over the fire ±6 cells {:.1} kW; by mixing (still air) up {:.1} kW, out of the column's x sides {:.1} kW", dz as f64 * 0.1, conv[h] / k, 100.0 * conv[h] / k / q, conv_near[h] / k, mix[h] / k, side_mix[h] / k) } }
}
