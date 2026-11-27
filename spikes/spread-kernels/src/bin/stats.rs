//! Workload statistics from the exact kernel's counters.
use fv_spread_kernels::{bench::make_set, exact::{step, Inst, Scratch}, harness::species};
use mc::spread::Params;
fn main() {
    let p = Params::default();
    for &n in &[10_000usize, 100_000, 1_000_000] {
        let sp = species(n, 7);
        let set = make_set(&sp, 32, 200, 1, 1, 4);
        let mut sc = Scratch::new();
        for (who, d) in set.deltas.iter().enumerate() {
            let mut inst = Inst::new();
            for tk in &set.ticks[who] { if tk.damp != 1000 { inst.damp(tk.damp); } step(&set.sh, d, &mut inst, &tk.mods, &tk.inject, &p, &mut sc); }
        }
        let s = sc.stats; let k = s.steps as f64;
        println!("N {n}: per step: state {:.0} seeds {:.0} rounds {:.2} expanded {:.0} (patched {:.0}) visits {:.0} passed {:.0} touched {:.0} gain-misses {:.0}; delta {} KB",
            s.state as f64 / k, s.seeds as f64 / k, s.rounds as f64 / k, s.expanded as f64 / k, s.patched as f64 / k, s.visits as f64 / k, s.passed as f64 / k, s.touched as f64 / k, sc.g.misses as f64 / k, set.deltas[0].bytes() >> 10);
    }
}
