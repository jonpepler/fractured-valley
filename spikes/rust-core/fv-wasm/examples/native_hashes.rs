//! Writes the native hash list for the determinism scenario as JSON, for the
//! browser builds to compare against.
//!
//!     cargo run --release -p fv-wasm --example native_hashes > web/dist/native.json
//!
//! The scenario: seed 5, a 32×32×32 valley, afternoon sky, the crib lit for
//! 40 ticks, 60 ticks in all. The same settings are in web/src/scenario.ts.

use fv_wasm::{hex, scenario};

fn main() {
    let (seed, n, nz, lit, ticks) = (5u64, 32usize, 32usize, 40u64, 60usize);
    let mut sim = scenario(seed, n, nz, lit);
    // Single-threaded and threaded must agree; this build runs single-threaded
    // (fv-wasm turns fv-core's `parallel` feature off unless `threads` is on).
    let start = std::time::Instant::now();
    let hashes: Vec<String> = (0..ticks).map(|_| { sim.step(); format!("\"{}\"", hex(sim.hash())) }).collect();
    let ms = start.elapsed().as_secs_f64() * 1000.0 / ticks as f64;
    assert_eq!(sim.energy_gap(), 0, "energy books must balance");
    assert_eq!(sim.mass_gap(), 0, "mass books must balance");
    println!(
        "{{\n  \"scenario\": {{ \"seed\": {seed}, \"n\": {n}, \"nz\": {nz}, \"lit\": {lit}, \"ticks\": {ticks} }},\n  \"platform\": \"{}-{}\",\n  \"browser\": \"native\",\n  \"msPerTick\": {ms:.3},\n  \"energyGap\": \"{}\",\n  \"massGap\": \"{}\",\n  \"hashes\": [{}]\n}}",
        std::env::consts::OS, std::env::consts::ARCH, sim.energy_gap(), sim.mass_gap(), hashes.join(", ")
    );
}
