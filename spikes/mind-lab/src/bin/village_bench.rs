//! Stage 5: the cost of a village mind, the same code natively and for wasm32 (run under node):
//! `cargo run --release --bin village_bench -- 30 3` and
//! `node tools/wasi.mjs target/wasm32-wasip1/release/village_bench.wasm 30 3`.
//! For each early-stop setting, best of 3 runs on one thread: µs per mind-tick (the brains' pass,
//! people and animals, per person), µs per think, the hash (identical across settings).
use fv_mind_lab::think::{EARLY_STOP, STOP_OFF, STOP_SKIP, STOP_SORTED};
use fv_mind_lab::village::runs::{run, Setup};
use std::sync::atomic::Ordering;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: usize = a.get(1).and_then(|x| x.parse().ok()).unwrap_or(30);
    let days: u32 = a.get(2).and_then(|x| x.parse().ok()).unwrap_or(3);
    let every: u32 = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(1);
    let target = if cfg!(target_arch = "wasm32") { "wasm32" } else { "native" };
    let mut hashes = vec![];
    for (m, name) in [(STOP_OFF, "off"), (STOP_SKIP, "skip"), (STOP_SORTED, "sorted")] {
        EARLY_STOP.store(m, Ordering::Relaxed);
        let mut best = f64::MAX;
        let mut h = 0;
        for _ in 0..3 {
            let mut s = Setup::new(7, n, days);
            s.every = every;
            let t = std::time::Instant::now();
            let (rec, _) = run(&s, 1, None);
            let _ = t;
            best = best.min(rec.brain_ns as f64 / 1e3 / (rec.ticks as f64 * n as f64));
            h = rec.hash;
        }
        hashes.push(h);
        println!("{target} {n} people {days} days every {every}: early stop {name}: {best:.2} µs per mind-tick; {:.0} minds per 20 Hz tick per core; hash {h:016x}", 50_000.0 / best);
    }
    assert!(hashes.iter().all(|&x| x == hashes[0]), "early stop changed the result");
}
