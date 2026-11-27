//! Round 2, stage 3: the cost of a mind-tick at each thinking rate, the same code natively and for
//! wasm32 (under node): `cargo run --release --bin rate_bench -- <modes> [people] [days] [reps]` and
//! `node tools/wasi.mjs target/wasm32-wasip1/release/rate_bench.wasm <modes> ...`.
//! Modes as `Setup::apply_mode` (e1, e4, s634, b4, ...). Best of `reps` runs on one thread: µs per
//! mind-tick (the brains' pass, people and animals, per person), thinks per person-tick, the hash.
use fv_mind_lab::village::runs::{run, stats, Setup};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let modes: Vec<String> = a.get(1).map(|m| m.split(',').map(String::from).collect()).unwrap_or(vec!["e1".into()]);
    let n: usize = a.get(2).and_then(|x| x.parse().ok()).unwrap_or(30);
    let days: u32 = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(3);
    let reps: usize = a.get(4).and_then(|x| x.parse().ok()).unwrap_or(3);
    let target = if cfg!(target_arch = "wasm32") { "wasm32" } else { "native" };
    for mode in &modes {
        let mut best = f64::MAX; let mut h = 0; let mut thinks = 0.0;
        for _ in 0..reps {
            let mut s = Setup::new(7, n, days); s.apply_mode(mode);
            let (rec, v) = run(&s, 1, None);
            best = best.min(rec.brain_ns as f64 / 1e3 / (rec.ticks as f64 * n as f64));
            thinks = stats(&v).iter().map(|x| x.thinks as f64).sum::<f64>() / (rec.ticks as f64 * n as f64);
            h = rec.hash;
        }
        println!("{target},{mode},{n},{days},{best:.3},{thinks:.4},{h:016x}");
    }
}
