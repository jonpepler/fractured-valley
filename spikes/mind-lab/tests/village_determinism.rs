//! Determinism for the village (stage 5): pinned hashes; one hash on 1, 2, 4 and 8 threads (the
//! brains' pass on a pool of persistent threads) with minds in order, reversed and shuffled, in
//! full detail and with the cheap tier; the hash reacts to small changes. Native against wasm32
//! is in determinism.rs (it covers these runs too).
//! `cargo test --release --test village_determinism -- --nocapture`

use fv_mind_lab::hashes::village_runs;
use fv_mind_lab::village::runs::{run, Setup};

/// Pinned when stage 5 was written.
const PINNED: [u64; 3] = [0xc6de73db1522c1bc, 0x643637be3ce2a470, 0xce42a81eae48647e];

#[test]
fn village_runs_give_identical_pinned_hashes() {
    let (a, b) = (village_runs(), village_runs());
    for ((n, x), (_, y)) in a.iter().zip(&b) { println!("{x:#018x} {y:#018x}  {n}"); }
    assert_eq!(a, b);
    assert_eq!(a.iter().map(|x| x.1).collect::<Vec<_>>(), PINNED, "a pinned hash moved: behaviour changed");
}

#[test]
fn village_same_hash_on_any_thread_count_and_any_order() {
    let n = 30;
    let fwd: Vec<usize> = (0..n).collect();
    let rev: Vec<usize> = (0..n).rev().collect();
    let mut shuf = fwd.clone();
    for i in (1..n).rev() { let j = (fv_mind_lab::rng::hash(9, i as u64, 0, 0x5f) % (i as u64 + 1)) as usize; shuf.swap(i, j); }
    for every in [1, 4] {
        let mut s = Setup::new(5, n, 2);
        s.every = every;
        let reference = run(&s, 1, None).0.hash;
        for t in [1, 2, 4, 8] { for (name, o) in [("in order", &fwd), ("reversed", &rev), ("shuffled", &shuf)] {
            let h = run(&s, t, Some(o)).0.hash;
            println!("think every {every}: {t} thread(s), minds {name}: {h:#018x}");
            assert_eq!(h, reference);
        } }
    }
}

#[test]
fn village_hash_is_not_blind() {
    let base = run(&Setup::new(4, 20, 1), 1, None).0.hash;
    let mut cheap = Setup::new(4, 20, 1); cheap.every = 4;
    let mut no_gossip = Setup::new(4, 20, 1); no_gossip.gossip = false;
    let mut one_trust = fv_mind_lab::village::runs::build(&Setup::new(4, 20, 1));
    let t = one_trust.folk[1].trust(0); one_trust.folk[1].set_trust(0, t + 1);
    let one: Vec<usize> = (0..20).collect();
    for _ in 0..301 { one_trust.step(1, &one); }
    for (what, x) in [("seed", run(&Setup::new(5, 20, 1), 1, None).0.hash), ("people", run(&Setup::new(4, 21, 1), 1, None).0.hash), ("the cheap tier", run(&cheap, 1, None).0.hash),
        ("one unit of trust", one_trust.state_hash()), ("gossip off", run(&no_gossip, 1, None).0.hash)] {
        println!("{what}: {base:#018x} vs {x:#018x}");
        assert_ne!(base, x, "{what} must move the hash");
    }
}
