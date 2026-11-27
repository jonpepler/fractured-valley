//! Track B distance from the oracle (via the exact kernel, which is bit-identical to it).
use fv_spread_kernels::{bench::make_set, exact::Inst, fast, harness::species, metrics::Dist, redef};
use mc::spread::Params;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: usize = a.get(1).map(|x| x.parse().unwrap()).unwrap_or(100_000);
    let m: usize = a.get(2).map(|x| x.parse().unwrap()).unwrap_or(100);
    let t: usize = a.get(3).map(|x| x.parse().unwrap()).unwrap_or(200);
    let p = Params::default();
    let sp = species(n, 7);
    let set = make_set(&sp, m, t, 1, 1, 4);
    let classes = redef::classes(&set.sh, &set.deltas);
    let mut dist = Dist::default();
    let (mut fs, mut rs) = (fast::Scratch::new(), redef::Scratch::new());
    for who in 0..m {
        let (mut ia, mut ib) = (Inst::new(), Inst::new());
        for tk in &set.ticks[who] {
            if tk.damp != 1000 { ia.damp(tk.damp); redef::damp(&mut ib, tk.damp); }
            let (oa, _) = fast::step_pruned(&set.sh, &set.deltas[who], &mut ia, &tk.mods, &tk.inject, &p, &mut fs);
            let (ob, _) = redef::step(&set.sh, &set.deltas[who], &mut ib, &tk.mods, &tk.inject, &p, classes, &mut rs);
            dist.add(&ia.state(), &oa, &ib.state(), &ob);
        }
    }
    println!("redef N {n} M {m} T {t} classes {}: top8 overlap {:.4} (identical lists {:.4}), rel L1 {:.4}, argmax differs {:.4}",
        classes.count_ones(), dist.top8(), dist.top8_exact(), dist.rel_l1(), dist.argmax_rate());
}
