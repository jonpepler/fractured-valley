//! Golden run: every tick's state hash, the books and the time per tick for
//! the scenes whose speed matters. Write it once before a speed change and
//! again after; the two files must be identical apart from the timings.
//!
//!     cargo run --release -p fv-core --example golden > before.txt
//!     cargo run --release -p fv-core --example golden > after.txt
//!     diff <(grep -v ms/tick before.txt) <(grep -v ms/tick after.txt)
//!
//! `TICKS` sets how many ticks each scene runs (default 400); arguments pick
//! scenes (default all): tree-dead, tree-alive, campfire, valley, tree-field.
//! `FV_THREADS=1` runs single-threaded.
use fv_core::rad::{Radiation, Sky};
use fv_core::scene::{campfire, crib_and_tree, valley};
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::units::*;

fn tree(dead: bool) -> Sim {
    let (w, torch) = crib_and_tree(4, 48, 48, dead);
    let mut sim = Sim::new(w, Sky::afternoon());
    for t in 0..200 {
        for (i, p) in torch.iter().enumerate().filter(|(i, _)| i % 3 == 0) {
            sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x: p[0] as u16, y: p[1] as u16, z: p[2] as u16, energy: joules(500) } })
        }
    }
    sim
}

fn fire() -> Sim {
    let s = campfire(40, 32);
    let mut sim = Sim::new(s.world, Sky::afternoon());
    for t in 0..100 {
        for (i, p) in s.torch.iter().enumerate() {
            sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x: p.0 as u16, y: p.1 as u16, z: p.2 as u16, energy: joules(250) } })
        }
    }
    sim
}

fn vale() -> Sim {
    let v = valley(5, 32, 32, 60);
    let (cx, cy) = (v.crib.0.min(28), v.crib.1.min(28));
    let gz = v.ground_at(cx, cy) as u16 + 1;
    let mut sim = Sim::new(v.world, Sky::afternoon());
    for t in 0..40 { sim.schedule(Scheduled { tick: t, peer: 0, seq: 0, command: Command::Heat { x: cx as u16, y: cy as u16, z: gz, energy: joules(250) } }) }
    sim
}

fn main() {
    if std::env::var("FV_THREADS").ok().as_deref() == Some("1") { fv_core::world::THREADS.store(1, core::sync::atomic::Ordering::Relaxed) }
    let ticks: u64 = std::env::var("TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(400);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let all = ["tree-dead", "tree-alive", "campfire", "valley", "tree-field"];
    let scenes: Vec<&str> = if args.is_empty() { all.to_vec() } else { args.iter().map(|s| s.as_str()).collect() };
    for name in scenes {
        let mut sim = match name {
            "tree-dead" => tree(true),
            "tree-alive" => tree(false),
            "campfire" => fire(),
            "valley" => vale(),
            "tree-field" => { let mut s = tree(true); s.radiation = Radiation::Field; s }
            s => panic!("unknown scene {s}"),
        };
        let mut window = std::time::Instant::now();
        let start = window;
        for t in 1..=ticks {
            sim.step();
            println!("{name} {t} {:016x}", sim.hash());
            if t % 100 == 0 {
                println!("{name} {t} gaps {} {} impossible {:?}", sim.energy_gap(), sim.mass_gap(), sim.world.first_impossible());
                println!("{name} {t} ms/tick {:.1}", window.elapsed().as_secs_f64() * 1000.0 / 100.0);
                window = std::time::Instant::now();
            }
        }
        println!("{name} books {:?}", sim.books);
        println!("{name} totals {:?}", sim.totals());
        println!("{name} all ms/tick {:.1}", start.elapsed().as_secs_f64() * 1000.0 / ticks as f64);
        eprintln!("{name}: {:.1} ms/tick", start.elapsed().as_secs_f64() * 1000.0 / ticks as f64);
    }
}
