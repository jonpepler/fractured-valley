//! One village over a month: storage per mind per day (mean over people), before and after sleep.
use fv_mind_lab::village::runs::{run, Setup};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = args.get(1).and_then(|x| x.parse().ok()).unwrap_or(30);
    let days: u32 = args.get(2).and_then(|x| x.parse().ok()).unwrap_or(30);
    let seed: u64 = args.get(3).and_then(|x| x.parse().ok()).unwrap_or(1);
    let prune = args.get(4).is_some_and(|x| x == "prune");
    let mut s = Setup::new(seed, n, days);
    s.prune = prune;
    let t = std::time::Instant::now();
    let (rec, _v) = run(&s, 4, None);
    println!("{n} people, {days} days, prune {prune}: {:.1} s, hash {:016x}", t.elapsed().as_secs_f64(), rec.hash);
    println!("day | total B before → after | own | places | people | sources | plans | fast | state (expect) | layer | places# | source links | told | nodes");
    for d in 0..rec.before.len() {
        let m = |v: &Vec<fv_mind_lab::village::runs::Storage>, f: &dyn Fn(&fv_mind_lab::village::runs::Storage) -> usize| v.iter().map(f).sum::<usize>() / v.len();
        let (b, a) = (&rec.before[d], &rec.after[d]);
        println!("{d} | {} → {} | {} | {} | {} | {} | {} | {} → {} | {} ({}) | {} | {} | {} | {} | {}", m(b, &|s| s.total()), m(a, &|s| s.total()), m(a, &|s| s.own()), m(a, &|s| s.places), m(a, &|s| s.people), m(a, &|s| s.sources), m(a, &|s| s.plans), m(b, &|s| s.fast), m(a, &|s| s.fast), m(a, &|s| s.state), m(a, &|s| s.expect), m(a, &|s| s.layer), m(a, &|s| s.place_count), m(a, &|s| s.source_links), m(a, &|s| s.told), m(a, &|s| s.nodes));
    }
    for (d, r) in rec.day.iter().enumerate() { println!("day {d}: {r:?}"); }
}
