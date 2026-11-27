//! Determinism for the hunt and the betrayal (stage 4): pinned hashes; identical on 1, 2, 4 and 8
//! threads with people stepped in order, reversed and shuffled (people and the deer see and hear
//! each other, plans travel as sound, whispers and the hiding rule all read last tick's world);
//! the hash reacts to small changes. Native against wasm32 is in determinism.rs (it covers these
//! runs too).
//! `cargo test --release --test hunt_determinism -- --nocapture`

use fv_mind_lab::hashes::{hunt_order, hunt_runs};
use fv_mind_lab::hunt::runs::{build, Case, Mode, HUNT_TICKS};
use fv_mind_lab::store::Store;
use fv_mind_lab::think::Form;

/// Pinned when stage 4 was written.
const PINNED: [u64; 3] = [0x4e6055aceb4c2caa, 0x424622861fa04383, 0x79560a4d50d2ad5c];

#[test]
fn hunt_runs_give_identical_pinned_hashes() {
    let (a, b) = (hunt_runs(1), hunt_runs(1));
    for ((n, x), (_, y)) in a.iter().zip(&b) { println!("{x:#018x} {y:#018x}  {n}"); }
    assert_eq!(a, b);
    assert_eq!(a, hunt_runs(4), "the same on 4 threads");
    assert_eq!(a.iter().map(|x| x.1).collect::<Vec<_>>(), PINNED, "a pinned hash moved: behaviour changed");
}

#[test]
fn hunt_same_hash_on_any_thread_count_and_any_order() {
    let orders: [(&str, Vec<usize>); 3] = [("in order", vec![0, 1, 2, 3]), ("reversed", vec![3, 2, 1, 0]), ("shuffled", vec![2, 0, 3, 1])];
    for betray in [false, true] {
        let reference = hunt_order(1, &orders[0].1, betray);
        for t in [1, 2, 4, 8] { for (name, o) in &orders {
            let h = hunt_order(t, o, betray);
            println!("{} {t} thread(s), people {name}: {h:#018x}", if betray { "the betrayal:" } else { "a planned hunt:" });
            assert_eq!(h, reference);
        } }
    }
}

#[test]
fn hunt_hash_is_not_blind() {
    let h = |seed: u64, set: usize, mode: Mode, tweak: u8| {
        let mut v = build(&Case { seed, set, mode });
        match tweak {
            1 => { let m = &mut v.deer_minds[0]; let pn = m.place_nodes[0]; let x = m.store.val(pn); m.store.set_val(pn, (x + 1) as i16); }
            2 => { let p = &mut v.folk[1]; let n = p.pnode[0]; let x = p.m.store.val(n); p.m.store.set_val(n, (x + 1) as i16); }
            _ => {}
        }
        for _ in 0..HUNT_TICKS / 2 { v.step(1, &[0, 1, 2, 3]); }
        v.state_hash()
    };
    let base = h(2, 1, Mode::Planned, 0);
    for (what, x) in [("seed", h(3, 1, Mode::Planned, 0)), ("temperament set", h(2, 0, Mode::Planned, 0)), ("planned or alone", h(2, 1, Mode::Solo, 0)),
        ("one unit of the deer's meadow", h(2, 1, Mode::Planned, 1)), ("one unit of Bryn's warmth to Ash", h(2, 1, Mode::Planned, 2))] {
        println!("{what}: {base:#018x} vs {x:#018x}");
        assert_ne!(base, x, "{what} must move the hash");
    }
}
