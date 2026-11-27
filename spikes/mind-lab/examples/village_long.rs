//! Round 2, stage 1: a long run as a smoke test of memory over a life (9.4). One village for
//! many days on a few seeds; writes a per-day summary to `out/village/long-<tag>.csv`.
//! `cargo run --release --example village_long -- <seeds> <people> <days> [think_every] [tag]`
//! Seeds run in parallel, one thread each; every run is single-threaded and deterministic.
use fv_mind_lab::village::runs::{run, Setup};
use std::io::Write;
use std::sync::Mutex;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seeds: u64 = a.get(1).and_then(|x| x.parse().ok()).unwrap_or(4);
    let n: usize = a.get(2).and_then(|x| x.parse().ok()).unwrap_or(30);
    let days: u32 = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(180);
    let every: u32 = a.get(4).and_then(|x| x.parse().ok()).unwrap_or(1);
    let tag = a.get(5).cloned().unwrap_or_else(|| "long".into());
    std::fs::create_dir_all("out/village").unwrap();
    let out = Mutex::new(std::fs::File::create(format!("out/village/{tag}.csv")).unwrap());
    writeln!(out.lock().unwrap(), "seed,people,think_every,day,meals,drinks,gossip,help,proposals,caught,gored,surprised,said,trust,warmth,warm_ge_900,warm_le_m900,before_b,after_b,places_b,people_b,sources_b,plans_b,other_own_b,fast_before_b,state_b,expect_b,layer_b,cold_b,cold_nodes,nodes,links,places,source_links,told,went_cold,revived,hash").unwrap();
    std::thread::scope(|sc| {
        for seed in 1..=seeds {
            let out = &out;
            sc.spawn(move || {
                let mut s = Setup::new(seed, n, days);
                s.think_every = every;
                let t0 = std::time::Instant::now();
                let (rec, _) = run(&s, 1, None);
                let mut lines = vec![];
                for d in 0..rec.after.len() {
                    let (b, af) = (&rec.before[d], &rec.after[d]);
                    let m = |v: &Vec<fv_mind_lab::village::runs::Storage>, f: &dyn Fn(&fv_mind_lab::village::runs::Storage) -> usize| v.iter().map(f).sum::<usize>() as f64 / n as f64;
                    let dr = rec.day.get(d).copied().unwrap_or_default();
                    let fe = rec.feel[d];
                    lines.push(format!("{seed},{n},{every},{},{},{},{},{},{},{},{},{},{},{:.1},{:.1},{:.4},{:.4},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.2},{:.1},{:.1},{:.2},{:.2},{:.2},{},{},{:016x}",
                        d + 1, dr.meals, dr.drinks, dr.gossip, dr.help, dr.proposals, dr.caught, dr.gored, dr.surprised, dr.said, fe.trust, fe.warmth, fe.hot, fe.cold,
                        m(b, &|s| s.total()), m(af, &|s| s.total()), m(af, &|s| s.places), m(af, &|s| s.people), m(af, &|s| s.sources), m(af, &|s| s.plans), m(af, &|s| s.other_own),
                        m(b, &|s| s.fast), m(af, &|s| s.state), m(af, &|s| s.expect), m(af, &|s| s.layer), m(af, &|s| s.cold), m(af, &|s| s.cold_nodes), m(af, &|s| s.nodes), m(af, &|s| s.links), m(af, &|s| s.place_count), m(af, &|s| s.source_links), m(af, &|s| s.told),
                        fe.went_cold, fe.revived, rec.hash));
                }
                let mut f = out.lock().unwrap();
                for l in lines { writeln!(f, "{l}").unwrap(); }
                eprintln!("seed {seed}: {} days in {:.0} s, hash {:016x}", days, t0.elapsed().as_secs_f64(), rec.hash);
            });
        }
    });
}
