//! The village's brains as their own pass on 1, 2, 4 and 8 threads, with minds in order, reversed
//! and shuffled: one hash, and the time.
use fv_mind_lab::village::runs::{run, Setup};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = args.get(1).and_then(|x| x.parse().ok()).unwrap_or(30);
    let days: u32 = args.get(2).and_then(|x| x.parse().ok()).unwrap_or(2);
    let mut s = Setup::new(3, n, days);
    s.every = args.get(3).and_then(|x| x.parse().ok()).unwrap_or(1);
    let fwd: Vec<usize> = (0..n).collect();
    let rev: Vec<usize> = (0..n).rev().collect();
    let mut shuf = fwd.clone();
    for i in (1..n).rev() { let j = (fv_mind_lab::rng::hash(9, i as u64, 0, 0x5f) % (i as u64 + 1)) as usize; shuf.swap(i, j); }
    let mut first = None;
    for th in [1usize, 2, 4, 8] {
        for (name, o) in [("in order", &fwd), ("reversed", &rev), ("shuffled", &shuf)] {
            let t = std::time::Instant::now();
            let (rec, _) = run(&s, th, Some(o));
            let el = t.elapsed().as_secs_f64();
            println!("{th} threads, {name}: {:016x}  {:.2} s, brains {:.1} µs/person-tick", rec.hash, el, rec.brain_ns as f64 / 1e3 / (rec.ticks as f64 * n as f64));
            match first { None => first = Some(rec.hash), Some(h) => assert_eq!(h, rec.hash, "hash differs") }
        }
    }
    println!("one hash");
}
