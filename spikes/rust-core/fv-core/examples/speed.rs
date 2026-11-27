//! Time per tick for the spike scenes, as JSON, for the CI speed check
//! (`ci/speed/check.mjs`). Each scene runs `WARM` ticks (default 20) and then
//! is timed over `TICKS` (default 60).
//!
//!     cargo run --release -p fv-core --example speed > speed.json
//!
//! Scenes: the dead tree and the living tree (`probe_tree`, 48³), the
//! campfire at night (40×40×32) and the valley (the browser determinism
//! scenario, 32³). Decision 06 also names the burning room and the
//! half-burnt column, which aren't scenes yet; add them here when they are.
use fv_core::rad::Sky;
use fv_core::scene::{campfire, crib_and_tree, valley};
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::units::*;
use std::time::Instant;

fn heat(sim: &mut Sim, ticks: u64, points: &[[usize; 3]], energy: Energy) {
    for t in 0..ticks { for (i, p) in points.iter().enumerate() { sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x: p[0] as u16, y: p[1] as u16, z: p[2] as u16, energy } }) } }
}

fn scene(name: &str) -> Sim {
    match name {
        "dead-tree" | "living-tree" => {
            let (w, torch) = crib_and_tree(4, 48, 48, name == "dead-tree");
            let mut sim = Sim::new(w, Sky::afternoon());
            let points: Vec<[usize; 3]> = torch.iter().enumerate().filter(|(i, _)| i % 3 == 0).map(|(_, p)| *p).collect();
            heat(&mut sim, 200, &points, joules(500));
            sim
        }
        "night-fire" => {
            let s = campfire(40, 32);
            let mut sim = Sim::new(s.world, Sky::NIGHT);
            let points: Vec<[usize; 3]> = s.torch.iter().map(|p| [p.0, p.1, p.2]).collect();
            heat(&mut sim, 100, &points, joules(250));
            sim
        }
        "valley" => {
            let v = valley(5, 32, 32, 60);
            let (cx, cy) = (v.crib.0.min(28), v.crib.1.min(28));
            let gz = v.ground_at(cx, cy);
            let mut sim = Sim::new(v.world, Sky::afternoon());
            heat(&mut sim, 40, &[[cx, cy, gz + 1]], joules(250));
            sim
        }
        s => panic!("unknown scene {s}"),
    }
}

fn main() {
    let warm: u64 = std::env::var("WARM").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    let ticks: u64 = std::env::var("TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
    let names = ["dead-tree", "living-tree", "night-fire", "valley"];
    let mut out = Vec::new();
    for name in names {
        let mut sim = scene(name);
        for _ in 0..warm { sim.step() }
        let t0 = Instant::now();
        for _ in 0..ticks { sim.step() }
        let ms = t0.elapsed().as_secs_f64() * 1000.0 / ticks as f64;
        assert_eq!(sim.energy_gap(), 0, "{name}: energy books must balance");
        eprintln!("{name}: {ms:.1} ms/tick");
        out.push(format!("    \"{name}\": {ms:.2}"));
    }
    println!("{{\n  \"cores\": {},\n  \"msPerTick\": {{\n{}\n  }}\n}}", cores(), out.join(",\n"));
}

fn cores() -> usize { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) }
