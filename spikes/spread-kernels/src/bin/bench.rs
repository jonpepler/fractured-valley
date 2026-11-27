//! The benchmark table. `bench <N,..> <M,..> [variants] [threads,..] [passes]`
//! Appends rows to results/bench.csv. Every pass must hash the same (else it panics).
use fv_spread_kernels::{bench::*, harness::species};
use mc::spread::Params;
use std::io::Write;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let ns: Vec<usize> = a[1].split(',').map(|x| x.parse().unwrap()).collect();
    let ms: Vec<usize> = a[2].split(',').map(|x| x.parse().unwrap()).collect();
    let vs: Vec<String> = a.get(3).map(|x| x.split(',').map(String::from).collect()).unwrap_or(VARIANTS.iter().map(|s| s.to_string()).collect());
    let ths: Vec<usize> = a.get(4).map(|x| x.split(',').map(|y| y.parse().unwrap()).collect()).unwrap_or(vec![1, 4]);
    let passes: usize = a.get(5).map(|x| x.parse().unwrap()).unwrap_or(5);
    let p = Params::default();
    std::fs::create_dir_all("results").unwrap();
    let path = "results/bench.csv";
    let new = !std::path::Path::new(path).exists();
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    if new { writeln!(f, "variant,n,m,threads,ticks,overlay,passes,us_per_step_wall,steps_per_s,steps_per_s_per_core,hash,delta_bytes_per_instance,state_bytes_per_instance,edges_visited_per_step,nodes_expanded_per_step,est_bytes_read_per_step").unwrap(); }
    for &n in &ns {
        let sp = species(n, 7);
        for &m in &ms {
            // Default individuals everywhere except N=1M with M=10k (10x smaller deltas, to fit in RAM).
            let (num, den) = if n >= 1_000_000 && m > 1000 { (1, 10) } else { (1, 1) };
            let t = (200_000 / m).clamp(10, 2000);
            let set = make_set(&sp, m, t, num, den, 4);
            let delta = set.deltas.iter().map(|d| d.bytes()).sum::<usize>() / m;
            let steps = (m * t) as f64;
            let mut rows = vec![];
            if m <= 10 && vs.iter().any(|v| v == "oracle") {
                let o = best(passes.min(3), || run_oracle(&sp, &set, &p, num, den, m));
                rows.push(("oracle".to_string(), 1, o, (0.0, 0.0, 0.0)));
            }
            for v in vs.iter().filter(|v| *v != "oracle") {
                let c = if v == "exact-v1" || v == "redef-lazy" { (0.0, 0.0, 0.0) } else { counters(v, &set, &p) };
                for &th in &ths {
                    let r = best(passes, || run_variant(v, &set, &p, th, None));
                    rows.push((v.clone(), th, r, c));
                }
            }
            for (v, th, r, c) in rows {
                // estimate: 8 B per visited edge, 28 B per expanded node (offset, den, class mask,
                // rank word), 16 B per state entry (read + write)
                let est = c.0 * 8.0 + c.1 * 28.0 + c.2 * 16.0;
                let line = format!("{v},{n},{m},{th},{t},{num}/{den},{passes},{:.3},{:.0},{:.0},{:016x},{delta},{:.0},{:.0},{:.0},{:.0}",
                    r.secs * 1e6 / steps, steps / r.secs, steps / r.secs / th as f64, r.hash, c.2 * 8.0, c.0, c.1, est);
                println!("{line}");
                writeln!(f, "{line}").unwrap();
            }
        }
    }
}
