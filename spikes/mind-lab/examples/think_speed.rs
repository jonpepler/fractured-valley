//! Round 2, stage 2: the thinking step's cost on real minds (the village, the fox runs, the hunt
//! runs), one thread, best of 3. Run once per step: `FV_THINK=v1 cargo run --release --example
//! think_speed` and `FV_THINK=v2 …`. With v2 it also runs with the early stop off (identical
//! hashes) and prints what the stop and the compiled overlay did.
use fv_mind_lab::hashes;
use fv_mind_lab::mind::Config;
use fv_mind_lab::think::{THINK_NS, TIME_THINK};
use fv_mind_lab::think2::{self, PRUNE};
use fv_mind_lab::village::runs::{run, Setup};
use std::sync::atomic::Ordering;

fn timed(f: &dyn Fn() -> u64) -> (f64, f64, u64, u64) {
    let mut best = (f64::MAX, f64::MAX, 0, 0);
    for _ in 0..3 {
        THINK_NS.with(|c| c.set((0, 0)));
        let t = std::time::Instant::now();
        let h = f();
        let el = t.elapsed().as_secs_f64();
        let (ns, n) = THINK_NS.with(|c| c.get());
        if el < best.0 { best = (el, ns as f64 / n.max(1) as f64 / 1000.0, n, h); }
    }
    best
}

fn main() {
    let v2 = Config::default().think_v2;
    let a: Vec<String> = std::env::args().collect();
    let days: u32 = a.get(1).and_then(|x| x.parse().ok()).unwrap_or(3);
    TIME_THINK.store(true, Ordering::Relaxed);
    let cases: Vec<(&str, Box<dyn Fn() -> u64>)> = vec![
        ("village: 30 people", Box::new(move || run(&Setup::new(7, 30, days), 1, None).0.hash)),
        ("fox: the pinned runs", Box::new(|| hashes::runs(1).iter().fold(0, |h, x| h ^ x.1.rotate_left(7))) ),
        ("hunt: the pinned runs", Box::new(|| hashes::hunt_runs(1).iter().fold(0, |h, x| h ^ x.1.rotate_left(7))) ),
        ("teaching and danger: the pinned runs", Box::new(|| hashes::people_runs(1).iter().chain(hashes::danger_runs(1).iter()).fold(0, |h, x| h ^ x.1.rotate_left(7))) ),
    ];
    println!("step {}", if v2 { "v2" } else { "v1" });
    for (name, f) in &cases {
        think2::take_stats();
        let (el, us, n, h) = timed(f.as_ref());
        let s = think2::take_stats();
        println!("{name}: {el:.3} s in all; {n} thinks at {us:.2} µs a think; hash {h:016x}");
        if v2 {
            let per = |x: u64| x as f64 / s.thinks.max(1) as f64;
            println!("    of which bringing the compiled overlay up to date: {:.2} µs a think", s.refresh_ns as f64 / 1000.0 / s.thinks.max(1) as f64);
            println!("    per think: {:.1} senders, {:.1} links evaluated, {:.1} stopped early, {:.1} flows, {:.1} touched; active before cap {:.1} (max {}); candidates a hop max {}; thinks with a cap binding (over the 3 runs): inject {} frontier {} sum {} factor {}",
                per(s.senders), per(s.evaluated), per(s.stopped), per(s.flows), per(s.touched), per(s.active_sum), s.active_max, s.cand_max, s.sat_inject, s.sat_front, s.sat_inc, s.sat_factor);
            PRUNE.store(false, Ordering::Relaxed);
            let (el2, us2, _, h2) = timed(f.as_ref());
            PRUNE.store(true, Ordering::Relaxed);
            println!("    early stop off: {el2:.3} s, {us2:.2} µs a think; same hash: {}", h2 == h);
        }
    }
}
