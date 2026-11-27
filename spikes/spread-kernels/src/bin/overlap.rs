//! How much do instances' frontiers overlap at one tick? (Is batching by source worth it?)
use fv_spread_kernels::{bench::make_set, exact::Inst, fast::{step, Scratch}, harness::species};
use mc::spread::Params;
use std::collections::HashSet;
fn main() {
    let p = Params::default();
    for &n in &[10_000usize, 100_000, 1_000_000] {
        let sp = species(n, 7);
        let m = 1000;
        let set = make_set(&sp, m, 40, 1, 1, 4);
        let mut insts: Vec<Inst> = set.deltas.iter().map(|_| Inst::new()).collect();
        let mut sc = Scratch::new();
        for t in 0..40 {
            let mut all = vec![];
            for (who, inst) in insts.iter_mut().enumerate() {
                sc.log = Some(vec![]);
                let tk = &set.ticks[who][t];
                if tk.damp != 1000 { inst.damp(tk.damp); }
                step(&set.sh, &set.deltas[who], inst, &tk.mods, &tk.inject, &p, &mut sc);
                all.extend(sc.log.take().unwrap());
            }
            if t % 13 == 12 {
                for r in 0..4u8 {
                    let e: Vec<u32> = all.iter().filter(|x| x.0 == r && (x.1 as usize) < n).map(|x| x.1).collect();
                    let d: HashSet<u32> = e.iter().copied().collect();
                    let edges: usize = d.iter().map(|&u| set.sh.range(u).len()).sum();
                    let edges_all: usize = e.iter().map(|&u| set.sh.range(u).len()).sum();
                    print!("r{r}: {} entries / {} distinct ({:.1}x, edge reuse {:.1}x)  ", e.len(), d.len(), e.len() as f64 / d.len().max(1) as f64, edges_all as f64 / edges.max(1) as f64);
                }
                println!(" [N {n} M {m} tick {t}]");
            }
        }
    }
}
