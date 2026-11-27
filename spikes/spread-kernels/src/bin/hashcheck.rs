//! Hashes of every kernel over a fixed instance set, single-threaded and without std threads,
//! so the same binary runs natively and as wasm32-wasip1 under node. Prints one line per kernel.
use fv_spread_kernels::{bench::fold, delta::Delta, exact::{self, Inst}, fast, harness, redef, shared::Shared};
use mc::spread::Params;
use std::time::Instant;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: usize = a.get(1).map(|x| x.parse().unwrap()).unwrap_or(10_000);
    let m: u32 = a.get(2).map(|x| x.parse().unwrap()).unwrap_or(8);
    let t: usize = a.get(3).map(|x| x.parse().unwrap()).unwrap_or(100);
    let p = Params::default();
    let sp = harness::species(n, 7);
    let sh = Shared::new(&sp.base);
    let ds: Vec<Delta> = (0..m).map(|w| Delta::new(&sh, &harness::overlay(&sp, w, 0x5eed))).collect();
    let tks: Vec<_> = (0..m).map(|w| harness::ticks(&sp, w, 0x7157, t)).collect();
    let classes = redef::classes(&sh, &ds);
    for k in ["exact-v1", "exact-fast", "exact-pruned", "redef"] {
        let mut acc = 0u64;
        let (mut es, mut fs, mut rs) = (exact::Scratch::new(), fast::Scratch::new(), redef::Scratch::new());
        let t0 = Instant::now();
        let mut insts: Vec<Inst> = (0..m).map(|_| Inst::new()).collect();
        for tick in 0..t {
            for w in 0..m as usize {
                let (inst, tk, d) = (&mut insts[w], &tks[w][tick], &ds[w]);
                let h = match k {
                    "exact-v1" => { if tk.damp != 1000 { inst.damp(tk.damp); } let o = exact::step(&sh, d, inst, &tk.mods, &tk.inject, &p, &mut es); fast::state_hash(&inst.st, &o) }
                    "exact-fast" => { if tk.damp != 1000 { inst.damp(tk.damp); } fast::step(&sh, d, inst, &tk.mods, &tk.inject, &p, &mut fs).1 }
                    "exact-pruned" => { if tk.damp != 1000 { inst.damp(tk.damp); } fast::step_pruned(&sh, d, inst, &tk.mods, &tk.inject, &p, &mut fs).1 }
                    _ => { if tk.damp != 1000 { redef::damp(inst, tk.damp); } redef::step(&sh, d, inst, &tk.mods, &tk.inject, &p, classes, &mut rs).1 }
                };
                acc = fold(acc, w as u32, tick as u32, h);
            }
        }
        let us = t0.elapsed().as_secs_f64() * 1e6 / (m as usize * t) as f64;
        println!("{k:13} N {n} M {m} T {t}: hash {acc:016x}  {us:.2} us/step");
    }
}
