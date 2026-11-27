//! Determinism for the danger scenarios (stage 3): pinned hashes; identical on 1, 2, 4 and 8
//! threads with people stepped in order, reversed and shuffled (people and the boar see and hear
//! each other, and the boar steps in the same tick); the hash reacts to small changes. Native
//! against wasm32 is in determinism.rs (it covers these runs too).
//! `cargo test --release --test danger_determinism -- --nocapture`

use fv_mind_lab::danger::runs::{build, meet, Case, TEMPERS, WATCH};
use fv_mind_lab::hashes::{danger_order, danger_runs};
use fv_mind_lab::store::Store;
use fv_mind_lab::think::Form;

/// Pinned when stage 3 was written; moved after review (meeting distance varies with the seed,
/// a braced charge is no longer a goring).
/// Moved in stage 4, not by behaviour: a deer kind lengthens each mind's expectation table (and the
/// boar's). The worlds' own hashes over these runs are unchanged (checked when stage 4 was written).
// Round 2 (stage 1): re-pinned. Behaviour changed on purpose: needs act only on spreading (9.3),
// the village's fixes are the default (9.6), sleep keeps a share set by the day's novelty (9.4),
// dormancy and feelings by evidence (9.4, 9.8), graded choices (9.6), and the link-type table
// grew by one (`told`). Native and wasm32 agree on every one (`tools/wasm-hashes.sh`).
const PINNED: [u64; 3] = [0xe5ee7801f0ea0943, 0xef7eeed65908690d, 0x230494dabeaebad3];

#[test]
fn danger_runs_give_identical_pinned_hashes() {
    let (a, b) = (danger_runs(1), danger_runs(1));
    for ((n, x), (_, y)) in a.iter().zip(&b) { println!("{x:#018x} {y:#018x}  {n}"); }
    assert_eq!(a, b);
    assert_eq!(a, danger_runs(4), "the same on 4 threads");
    assert_eq!(a.iter().map(|x| x.1).collect::<Vec<_>>(), PINNED, "a pinned hash moved: behaviour changed");
}

#[test]
fn danger_same_hash_on_any_thread_count_and_any_order() {
    let orders: [(&str, Vec<usize>); 3] = [("in order", vec![0, 1, 2, 3]), ("reversed", vec![3, 2, 1, 0]), ("shuffled", vec![2, 0, 3, 1])];
    for call in [false, true] {
        let reference = fv_mind_lab::hashes::danger_order_case(1, &orders[0].1, call);
        for t in [1, 2, 4, 8] { for (name, o) in &orders {
            let h = fv_mind_lab::hashes::danger_order_case(t, o, call);
            println!("{} {t} thread(s), people {name}: {h:#018x}", if call { "a cry for help:" } else { "allies and a boar:" });
            assert_eq!(h, reference);
        } }
    }
    let _ = danger_order;
}

#[test]
fn danger_hash_is_not_blind() {
    let h = |seed: u64, t: usize, ally: i32, tweak: bool| {
        let mut v = build(&Case { seed, temper: TEMPERS[t].1, allies: true, expected: true }, ally, 2).0;
        if tweak { let m = &mut v.beasts[0]; let pn = m.place_nodes[0]; let v0 = m.store.val(pn); m.store.set_val(pn, (v0 + 1) as i16); }
        meet(&mut v, seed, 8); v.run(WATCH); v.state_hash()
    };
    let base = h(1, 0, 700, false);
    for (what, x) in [("seed", h(2, 0, 700, false)), ("temperament", h(1, 1, 700, false)), ("one unit of an ally link", h(1, 0, 699, false)), ("one unit of the boar's wallow", h(1, 0, 700, true))] {
        println!("{what}: {base:#018x} vs {x:#018x}");
        assert_ne!(base, x, "{what} must move the hash");
    }
}
