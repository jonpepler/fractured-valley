//! Stage 5: the spread-kernels early stop, applied to the runtime form. Runs every pinned run
//! (fox, teaching, danger, hunt) with the early stop off, as a per-link skip, and as a sort-and-stop,
//! checks that every hash is identical, and reports the time and how many links had their target
//! looked up. `cargo run --release --example early_stop`
use fv_mind_lab::hashes;
use fv_mind_lab::think::{EARLY_STOP, EVALUATED, STOP_OFF, STOP_SKIP, STOP_SORTED, STOP_NODE, NODE_SKIPS, THINK_NS, TIME_THINK};
use std::sync::atomic::Ordering;
use std::time::Instant;

fn all() -> Vec<(&'static str, u64)> {
    let mut v = hashes::runs(1);
    v.extend(hashes::people_runs(1));
    v.extend(hashes::danger_runs(1));
    v.extend(hashes::hunt_runs(1));
    v
}

fn all_off() -> Vec<(&'static str, u64)> { let m = EARLY_STOP.load(Ordering::Relaxed); EARLY_STOP.store(STOP_OFF, Ordering::Relaxed); let t = TIME_THINK.swap(false, Ordering::Relaxed); let v = all(); TIME_THINK.store(t, Ordering::Relaxed); EARLY_STOP.store(m, Ordering::Relaxed); v }

fn main() {
    let modes = [(STOP_OFF, "off"), (STOP_SKIP, "skip"), (STOP_SORTED, "sorted")];
    TIME_THINK.store(true, Ordering::Relaxed);
    let mut first: Option<Vec<(&str, u64)>> = None;
    {
        // Not exact (it changes the idle budget): how much a whole-sender skip could save at most.
        EARLY_STOP.store(STOP_NODE, Ordering::Relaxed);
        NODE_SKIPS.with(|c| c.set(0)); THINK_NS.with(|c| c.set((0, 0)));
        let t = Instant::now(); let hs = all(); let el = t.elapsed().as_secs_f64();
        let (ns, n) = THINK_NS.with(|c| c.get());
        let same = hs.iter().zip(&all_off()).filter(|(a, b)| a == b).count();
        println!("   node: {el:.3} s; {:.2} µs a think; senders skipped unread {}; hashes identical to 'off' in {same} of {} (not exact: it changes the idle budget)", ns as f64 / n as f64 / 1000.0, NODE_SKIPS.with(|c| c.get()), hs.len());
    }
    for (m, name) in modes {
        EARLY_STOP.store(m, Ordering::Relaxed);
        let mut best = f64::MAX;
        let mut hs = vec![];
        let mut ev = 0;
        let mut think_best = f64::MAX;
        for _ in 0..5 {
            EVALUATED.with(|c| c.set(0));
            THINK_NS.with(|c| c.set((0, 0)));
            let t = Instant::now();
            hs = all();
            best = best.min(t.elapsed().as_secs_f64());
            ev = EVALUATED.with(|c| c.get());
            let (ns, n) = THINK_NS.with(|c| c.get());
            think_best = think_best.min(ns as f64 / n as f64 / 1000.0);
        }
        let thinks = THINK_NS.with(|c| c.get().1); println!("{name:>7}: {:.3} s for all {} pinned runs (best of 5); {think_best:.2} µs a think; {thinks} thinks; links evaluated {ev} ({:.0} a think)", best, hs.len(), ev as f64 / thinks as f64);
        match &first { None => first = Some(hs), Some(f) => { for (a, b) in f.iter().zip(&hs) { assert_eq!(a, b, "hash differs with early stop {name}"); } println!("         every hash identical to 'off'"); } }
    }
}
