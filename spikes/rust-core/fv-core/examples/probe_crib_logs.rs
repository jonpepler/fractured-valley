//! The dead tree's crib on its own: heat release, how hot its logs' faces
//! get, how fast they give off gas, and what touches them.
//!     SEED=4 SECONDS=120 cargo run --release -p fv-core --example probe_crib_logs [-- tree]
//! `tree` runs the full dead-tree scene (48³) instead of the crib alone.
use fv_core::rad::Sky;
use fv_core::scene::{crib, crib_and_tree};
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
fn main() {
    let seed: u64 = std::env::var("SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
    let secs: u64 = std::env::var("SECONDS").ok().and_then(|v| v.parse().ok()).unwrap_or(120);
    let tree = std::env::args().any(|a| a == "tree");
    let (w, torch, (cx, cy, z0)) = if tree { let (w, t) = crib_and_tree(seed, 48, 48, true); (w, t, (20, 19, 7)) } else { crib(24, 32, seed) };
    let mut sim = Sim::new(w, Sky::afternoon());
    sim.flames = Some(vec![]);
    for t in 0..200 { for (i, p) in torch.iter().enumerate().filter(|(i, _)| i % 3 == 0) { sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x: p[0] as u16, y: p[1] as u16, z: p[2] as u16, energy: joules(500) } }) } }
    let tb = fv_core::tables::tables();
    let chem = |sim: &Sim| sim.totals().chemical - sim.books.edge_chemical();
    let mut last = chem(&sim);
    let (mut flame_near, mut faces_flaming, mut faces, mut gas_t, mut light_near) = (0f64, 0usize, 0usize, 0f64, 0i64);
    let mut flame_all = 0i64; let (mut fill, mut contact) = (0i64, 0i64);
    for tick in 1..=secs * 20 {
        sim.step();
        let w = &sim.world; let s = &w.subs;
        let fl = sim.flames.as_ref().unwrap();
        flame_all += fl.iter().sum::<i64>();
        if tick % 20 == 0 {
            let sides = fv_core::transport::sides(w);
            // Faces of logs against open cells: is the cell next door flaming?
            for z in z0 + 1..z0 + 9 { for y in cy..cy + 8 { for x in cx..cx + 10 {
                if w.get(x, y, z).substance != WOOD { continue }
                for (dx, dy, dz) in [(1i64, 0i64, 0i64), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
                    let (nx, ny, nz) = ((x as i64 + dx) as usize, (y as i64 + dy) as usize, (z as i64 + dz) as usize);
                    let n = w.get(nx, ny, nz);
                    if !n.permeable(s) { continue }
                    faces += 1;
                    let (i, j) = (w.idx(x, y, z), w.idx(nx, ny, nz));
                    let sd = &sides[j];
                    if sd.flame_fill > 0 { faces_flaming += 1; fill += sd.flame_fill }
                    let axis = if dx != 0 { 0 } else if dy != 0 { 1 } else { 2 };
                    let into_log = if dx + dy + dz > 0 { -fv_core::transport::diffuse_face(w.get(x, y, z), &sides[i], n, sd, 0, axis).flame.0 } else { fv_core::transport::diffuse_face(n, sd, w.get(x, y, z), &sides[i], 0, axis).flame.0 };
                    contact += into_log;
                    gas_t += n.gas_temperature().celsius_f64();
                    let f = fl[w.idx(nx, ny, nz)];
                    light_near += f;
                    if f > 0 { flame_near += 1.0 }
                }
            }}}
        }
        if tick % 200 == 0 {
            let now = chem(&sim);
            let hrr_kw = (last - now) as f64 / 1e6 / 10.0 / 1e3; last = now;
            let (mut n, mut hist, mut pyro, mut char_g, mut kind, mut vol) = (0, [0usize; 6], 0i64, 0i64, 0i64, 0i64);
            for z in z0..z0 + 9 { for y in cy..cy + 8 { for x in cx..cx + 10 {
                let c = w.get(x, y, z);
                if c.substance == KINDLING { kind += c.solid[0].0 }
                if c.substance != WOOD { continue }
                n += 1;
                let t = c.surface_temperature(s).celsius_f64();
                hist[match t as i64 { ..=100 => 0, 101..=200 => 1, 201..=300 => 2, 301..=400 => 3, 401..=600 => 4, _ => 5 }] += 1;
                pyro += fv_core::chem::pyrolysis_rate(c, s, tb).0;
                char_g += c.solid[1].0; vol += c.solid[0].0;
            }}}
            let f = faces.max(1) as f64;
            println!("{:4}s HRR {:5.0} kW (flame light {:4.0} kW) | log faces ≤100:{} ≤200:{} ≤300:{} ≤400:{} ≤600:{} >600:{} of {n} | log gas {:5.1} g/s vol {:.1} kg char {:.1} kg | kindling {:.2} kg | open faces: flame fills {:3.0}% of {:3.0}% (contact {:5.0} W/face), next to flame light {:3.0}%, gas {:3.0}C, flame light {:5.0} W/face, {} faces | gap {} {}",
                tick / 20, hrr_kw, flame_all as f64 / 1e6 / 10.0 / 1e3, hist[0], hist[1], hist[2], hist[3], hist[4], hist[5], pyro as f64 / 1e6 * 20.0, vol as f64 / 1e9, char_g as f64 / 1e9, kind as f64 / 1e9,
                fill as f64 / 10.0 / faces_flaming.max(1) as f64, 100.0 * faces_flaming as f64 / f, contact as f64 / 1e6 * 20.0 / f, 100.0 * flame_near / f, gas_t / f, light_near as f64 / 1e6 * 20.0 / f, faces / 10, sim.energy_gap(), w.first_impossible().is_none());
            flame_all = 0; fill = 0; contact = 0; faces = 0; faces_flaming = 0; flame_near = 0.0; gas_t = 0.0; light_near = 0;
        }
    }
}
