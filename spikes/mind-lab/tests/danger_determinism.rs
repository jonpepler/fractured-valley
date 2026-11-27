//! Determinism for the danger scenarios (stage 3): pinned hashes; identical on 1, 2, 4 and 8
//! threads with people stepped in order, reversed and shuffled (people and the boar see and hear
//! each other, and the boar steps in the same tick); the hash reacts to small changes. Native
//! against wasm32 is in determinism.rs (it covers these runs too).
//! `cargo test --release --test danger_determinism -- --nocapture`

use fv_mind_lab::danger::runs::{build, meet, Case, TEMPERS, WATCH};
use fv_mind_lab::hashes::{danger_order, danger_runs};
use fv_mind_lab::store::Store;
use fv_mind_lab::think::Form;

/// Pinned when stage 3 was written.
const PINNED: [u64; 3] = [0x3c66_7839_ac9e_0302, 0x19a2_4ce7_461d_051b, 0x1168_8470_37ad_bef8];

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
    let reference = danger_order(1, &orders[0].1);
    for t in [1, 2, 4, 8] { for (name, o) in &orders {
        let h = danger_order(t, o);
        println!("{t} thread(s), people {name}: {h:#018x}");
        assert_eq!(h, reference);
    } }
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
