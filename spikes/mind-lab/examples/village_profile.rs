//! Where a village tick's time goes (single thread): the person layer, the mind's step, the think.
use fv_mind_lab::village::runs::{build, set_tiers, Setup};
use fv_mind_lab::think::{THINK_NS, TIME_THINK};
use fv_mind_lab::mind::STEP_NS;
use fv_mind_lab::village::life::PERSON_NS;
use std::sync::atomic::Ordering;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = args.get(1).and_then(|x| x.parse().ok()).unwrap_or(30);
    let days: u32 = args.get(2).and_then(|x| x.parse().ok()).unwrap_or(2);
    let s = Setup::new(1, n, days);
    let mut v = build(&s);
    let ord: Vec<usize> = (0..n).collect();
    TIME_THINK.store(true, Ordering::Relaxed);
    let t = std::time::Instant::now();
    for _ in 0..days * 301 { set_tiers(&mut v, &s); v.step(1, &ord); }
    let el = t.elapsed().as_secs_f64();
    let (tn, tc) = THINK_NS.with(|c| c.get());
    let st = STEP_NS.with(|c| c.get());
    let (pn, pc) = PERSON_NS.with(|c| c.get());
    println!("wall {el:.2} s; brains {:.1} µs per person-tick", v.brain_ns as f64 / 1e3 / (pc as f64));
    println!("person tick {:.1} µs ({pc}); mind step calm {:.1} µs ({}) surprised {:.1} µs ({}); think {:.1} µs ({tc})", pn as f64 / pc as f64 / 1e3, st[0].0 as f64 / st[0].1 as f64 / 1e3, st[0].1, st[1].0 as f64 / st[1].1.max(1) as f64 / 1e3, st[1].1, tn as f64 / tc as f64 / 1e3);
    let ev = fv_mind_lab::think::EVALUATED.with(|c| c.get());
    // The same run again with the early stop off, to count the links it would have looked at.
    fv_mind_lab::think::EARLY_STOP.store(fv_mind_lab::think::STOP_OFF, Ordering::Relaxed);
    fv_mind_lab::think::EVALUATED.with(|c| c.set(0));
    let mut v2 = build(&s);
    for _ in 0..days * 301 { set_tiers(&mut v2, &s); v2.step(1, &ord); }
    let ev_off = fv_mind_lab::think::EVALUATED.with(|c| c.get());
    println!("links whose target is looked up: {ev} with the early stop, {ev_off} without ({:.0}% skipped); same hash: {}", 100.0 - ev as f64 * 100.0 / ev_off as f64, v.state_hash() == v2.state_hash());
    println!("shares of the person tick: mind step {:.0}%, of which think {:.0}%", (st[0].0 + st[1].0) as f64 * 100.0 / pn as f64, tn as f64 * 100.0 / pn as f64);
}
