//! Packets against the field, on the same scenes.
//!
//!   cargo run --release -p fv-core --example compare_light -- campfire
//!   cargo run --release -p fv-core --example compare_light -- tree
//!
//! 1. A frozen comparison: run to a tick with packets, then from that one
//!    state radiate many times with each method (the packets with a new
//!    random stream each time). Gives the mean light on each region, and the
//!    noise: how much a cell's light varies between runs of the same state.
//! 2. A full run with each method: ms per tick (whole step and radiation
//!    alone), the books every tick, and frame-to-frame change of the light.

use fv_core::cell::Cell;
use fv_core::rad::{radiate_by, Light, Radiation, Sky};
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;
use std::time::Instant;

struct Scene { world: World, torch: Vec<(usize, usize, usize)>, sky: Sky, regions: Vec<(&'static str, Vec<(usize, usize, usize)>)> }

fn campfire(n: usize, nz: usize) -> Scene {
    let mut w = World::new(n, n, nz, 7, celsius(15));
    let subs = w.subs.clone();
    let t = celsius(15);
    let stone = Cell::solid_of(STONE, 1000, 2, t, &subs);
    let soil = Cell::solid_of(SOIL, 1000, 150, t, &subs);
    let grass = Cell::solid_of(GRASS, 1000, 900, t, &subs);
    let wood = Cell::solid_of(WOOD, 1000, 100, t, &subs);
    let kindling = Cell::solid_of(KINDLING, 1000, 80, t, &subs);
    for y in 0..n { for x in 0..n { for z in 0..4 { w.set(x, y, z, if z < 2 { stone } else { soil }) } w.set(x, y, 4, grass) } }
    let (cx, cy) = (n / 2 - 2, n / 2 - 2);
    for y in cy..cy + 4 { for x in cx..cx + 4 { w.set(x, y, 4, kindling); w.set(x, y, 5, if (x + y) % 2 == 0 { kindling } else { wood }) } }
    for x in cx..cx + 4 { w.set(x, cy, 6, wood); w.set(x, cy + 3, 6, wood) }
    for z in 5..14 { for x in cx - 6..cx + 10 { w.set(x, cy - 4, z, stone) } }
    fv_core::scene::bond_all(&mut w, fv_core::scene::natural_bonds);
    let torch = vec![(cx, cy, 4), (cx + 2, cy + 1, 4), (cx + 1, cy + 3, 4), (cx + 3, cy + 2, 4)];
    // Ground (grass tops) 3–6 cells from the fire, on the open side; the wall's face; far corners.
    let dist = |x: usize, y: usize| { let dx = if x < cx { cx - x } else if x >= cx + 4 { x + 1 - cx - 4 } else { 0 }; let dy = if y < cy { cy - y } else if y >= cy + 4 { y + 1 - cy - 4 } else { 0 }; dx.max(dy) };
    let ground: Vec<_> = (0..n).flat_map(|y| (0..n).map(move |x| (x, y))).filter(|&(x, y)| y > cy - 4 && (3..=6).contains(&dist(x, y))).map(|(x, y)| (x, y, 4)).collect();
    let wall: Vec<_> = (5..14).flat_map(|z| (cx - 6..cx + 10).map(move |x| (x, cy - 4, z))).collect();
    let far: Vec<_> = (0..n).flat_map(|y| (0..n).map(move |x| (x, y))).filter(|&(x, y)| y > cy - 4 && dist(x, y) >= 12).map(|(x, y)| (x, y, 4)).collect();
    Scene { world: w, torch, sky: Sky::NIGHT, regions: vec![("ground 3-6 cells from fire", ground), ("stone wall (all cells)", wall), ("far ground (>=12 cells)", far)] }
}

fn tree(n: usize, nz: usize) -> Scene {
    let (w, torch) = fv_core::scene::crib_and_tree(7, n, nz, false);
    let torch: Vec<_> = torch.iter().enumerate().filter(|(i, _)| i % 3 == 0).map(|(_, p)| (p[0], p[1], p[2])).collect();
    let g = 6;
    let (tx, ty) = (n / 2, n / 2 + 3);
    // Ground under the canopy (shade), open ground away from tree and crib.
    let mut shade = Vec::new();
    let mut open = Vec::new();
    for y in 0..n { for x in 0..n {
        if w.get(x, y, g).substance != GRASS { continue }
        let leaves_above = (g + 1..nz).any(|z| w.get(x, y, z).substance == LEAVES);
        let r2 = (x as i64 - tx as i64).pow(2) + (y as i64 - ty as i64).pow(2);
        if leaves_above && r2 < 16 { shade.push((x, y, g)) }
        if !leaves_above && (y < 4 || y + 4 > n) { open.push((x, y, g)) }
    }}
    let leaves: Vec<_> = (0..nz).flat_map(|z| (0..n).flat_map(move |y| (0..n).map(move |x| (x, y, z)))).filter(|&(x, y, z)| w.get(x, y, z).substance == LEAVES).collect();
    let trunk: Vec<_> = (g + 1..g + 10).flat_map(|z| [(tx, ty, z), (tx + 1, ty, z), (tx, ty + 1, z), (tx + 1, ty + 1, z)]).collect();
    Scene { world: w, torch, sky: Sky::afternoon(), regions: vec![("open ground", open), ("ground in the tree's shade", shade), ("leaves", leaves), ("trunk (lower 9 cells)", trunk)] }
}

fn vis(l: &Light, w: &World, p: (usize, usize, usize)) -> [i64; 3] { l.incident[w.idx(p.0, p.1, p.2)] }

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "campfire".into());
    let ticks: u64 = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(900);
    let freeze_at: u64 = std::env::args().nth(3).and_then(|v| v.parse().ok()).unwrap_or(400);
    let make = || if which == "tree" { tree(40, 32) } else { campfire(40, 32) };
    let to_w_m2 = 0.002f64; // µJ per tick on a 0.01 m² face → W/m²

    // ---- 1. Frozen comparison on one state ----
    {
        let s = make();
        let mut sim = Sim::new(s.world, s.sky);
        sim.radiation = Radiation::Packets;
        schedule_torch(&mut sim, &s.torch);
        for _ in 0..freeze_at { sim.step() }
        let mut base = sim.world.clone();
        fv_core::chem::react(&mut base); // sets this tick's flame light
        println!("== {which}: frozen state at tick {freeze_at} ==");
        let runs = 24;
        for mode in [Radiation::Packets, Radiation::Field] {
            let mut per_run: Vec<Vec<f64>> = Vec::new(); // per region, per run: mean light
            let mut cell_samples: Vec<Vec<Vec<f64>>> = s.regions.iter().map(|(_, c)| vec![Vec::new(); c.len()]).collect();
            let mut colour = vec![[0f64; 3]; s.regions.len()];
            let mut ms = 0.0;
            let mut light = Light::new(base.cells.len());
            { let mut w = base.clone(); radiate_by(mode, &mut w, &sim.sky, &mut light); } // warm up
            for r in 0..runs {
                let mut w = base.clone();
                w.tick = freeze_at + r as u64 * 7919;
                let light = &mut light;
                let t = Instant::now();
                radiate_by(mode, &mut w, &sim.sky, light);
                ms += t.elapsed().as_secs_f64() * 1000.0;
                let mut row = Vec::new();
                for (k, (_, cells)) in s.regions.iter().enumerate() {
                    let mut sum = 0f64;
                    for (j, &p) in cells.iter().enumerate() {
                        let v = vis(light, &w, p);
                        let tot = (v[0] + v[1] + v[2]) as f64 * to_w_m2;
                        for b in 0..3 { colour[k][b] += v[b] as f64 }
                        cell_samples[k][j].push(tot);
                        sum += tot;
                    }
                    row.push(sum / cells.len().max(1) as f64);
                }
                per_run.push(row);
                if r == 0 {
                    // Where the visible light went: emitted, caught by the cells that glow
                    // (the fire itself), caught by everything else (first arrivals and bounces).
                    let glow: i64 = light.glow.iter().map(|g| g[0] + g[1] + g[2]).sum();
                    let (mut own, mut other) = (0i64, 0i64);
                    for i in 0..w.cells.len() {
                        let v: i64 = light.incident[i].iter().sum();
                        if light.glow[i].iter().sum::<i64>() > 0 { own += v } else { other += v }
                    }
                    println!("    visible emitted {:.2} W; caught by glowing cells {:.2} W, by the rest {:.2} W", glow as f64 * 2e-5, own as f64 * 2e-5, other as f64 * 2e-5);
                }
                if mode == Radiation::Field && r >= 1 { break } // deterministic: one more run only shows it repeats
            }
            println!("  {mode:?}: radiate {:.1} ms", ms / per_run.len() as f64);
            for (k, (name, _)) in s.regions.iter().enumerate() {
                let mean: f64 = per_run.iter().map(|r| r[k]).sum::<f64>() / per_run.len() as f64;
                // Noise: per cell, std/mean across runs, averaged over cells with light.
                let mut rel = 0f64; let mut cnt = 0;
                for samples in &cell_samples[k] {
                    let m = samples.iter().sum::<f64>() / samples.len() as f64;
                    if m <= 0.0 { continue }
                    let var = samples.iter().map(|v| (v - m).powi(2)).sum::<f64>() / samples.len() as f64;
                    rel += var.sqrt() / m; cnt += 1;
                }
                let c = colour[k]; let ct = (c[0] + c[1] + c[2]).max(1.0);
                println!("    {name:30} mean {mean:9.3} W/m²  cell noise {:5.1} %  lit cells {cnt}/{}  colour r/g/b {:.2}/{:.2}/{:.2}",
                         if cnt > 0 { rel / cnt as f64 * 100.0 } else { 0.0 }, cell_samples[k].len(), c[0] / ct, c[1] / ct, c[2] / ct);
            }
        }
    }

    // ---- 2. Full runs ----
    for mode in [Radiation::Packets, Radiation::Field] {
        let s = make();
        let mut sim = Sim::new(s.world, s.sky);
        sim.radiation = mode;
        schedule_torch(&mut sim, &s.torch);
        let mut step_ms = 0.0;
        let mut rad_ms = 0.0;
        let mut rad_n = 0;
        let mut prev: Vec<Vec<f64>> = Vec::new();
        let mut flicker = vec![(0f64, 0f64); s.regions.len()];
        let mut means = vec![0f64; s.regions.len()];
        let mut worst_gap = 0i128;
        let mut probe = Light::new(sim.world.cells.len());
        let window = (ticks / 2)..ticks;
        for tick in 0..ticks {
            let t = Instant::now();
            sim.step();
            step_ms += t.elapsed().as_secs_f64() * 1000.0;
            worst_gap = worst_gap.max(sim.energy_gap().abs());
            if tick % 100 == 50 {
                // Radiation alone, on a copy of this state.
                let mut w = sim.world.clone();
                fv_core::chem::react(&mut w);
                let light = &mut probe;
                let t = Instant::now();
                radiate_by(mode, &mut w, &sim.sky, light);
                rad_ms += t.elapsed().as_secs_f64() * 1000.0;
                rad_n += 1;
            }
            let cur: Vec<Vec<f64>> = s.regions.iter().map(|(_, cells)| cells.iter().map(|&p| { let v = vis(&sim.light, &sim.world, p); (v[0] + v[1] + v[2]) as f64 * to_w_m2 }).collect()).collect();
            if window.contains(&tick) && !prev.is_empty() {
                for k in 0..cur.len() {
                    let d: f64 = cur[k].iter().zip(&prev[k]).map(|(a, b)| (a - b).abs()).sum();
                    let m: f64 = cur[k].iter().sum();
                    flicker[k].0 += d; flicker[k].1 += m;
                    means[k] += m / cur[k].len().max(1) as f64;
                }
            }
            prev = cur;
        }
        let nwin = (window.end - window.start) as f64;
        println!("== {which}: full run, {mode:?}, {ticks} ticks ==");
        println!("  step {:.1} ms/tick, radiation alone {:.1} ms, worst |energy gap| {worst_gap}", step_ms / ticks as f64, rad_ms / rad_n.max(1) as f64);
        for (k, (name, _)) in s.regions.iter().enumerate() {
            println!("    {name:30} mean {:9.3} W/m²  frame-to-frame change {:5.1} % (ticks {}..{})", means[k] / nwin, flicker[k].0 / flicker[k].1.max(1e-9) * 100.0, window.start, window.end);
        }
    }
}

fn schedule_torch(sim: &mut Sim, torch: &[(usize, usize, usize)]) {
    let mut seq = 0;
    for tick in 0..200 {
        for &(x, y, z) in torch {
            sim.schedule(Scheduled { tick, peer: 0, seq, command: Command::Heat { x: x as u16, y: y as u16, z: z as u16, energy: joules(500) } });
            seq += 1;
        }
    }
}
