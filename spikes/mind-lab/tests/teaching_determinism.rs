//! Determinism for the teaching scenarios: pinned hashes; identical on 1, 2, 4 and 8 threads with
//! people stepped in order, reversed and shuffled (people here do sense and hear each other, so
//! this tests read-previous/write-own for real); and the hash reacts to a seed, a temperament,
//! one person's trust, and one unit of one told link. Native against wasm32 is in determinism.rs.
//! `cargo test --release --test teaching_determinism -- --nocapture`

use fv_mind_lab::hashes::{people_order, people_runs};
use fv_mind_lab::people::scenarios::{Setup, ASH, BRYN, DAY};
use fv_mind_lab::store::Store;

/// Pinned when stage 2 was written. If behaviour changes these move, and the change must be
/// explained.
const PINNED: [u64; 5] = [0x0601_89bb_9f81_66b3, 0x8a9f_3f00_7638_2192, 0xdb3c_f778_de8c_d635, 0xd803_475e_d0d2_8144, 0xda42_b5e7_dab1_9c06];

#[test]
fn teaching_runs_give_identical_pinned_hashes() {
    let (a, b) = (people_runs(1), people_runs(1));
    for ((n, x), (_, y)) in a.iter().zip(&b) { println!("{x:#018x} {y:#018x}  {n}"); }
    assert_eq!(a, b);
    assert_eq!(a, people_runs(4), "the same on 4 threads");
    assert_eq!(a.iter().map(|x| x.1).collect::<Vec<_>>(), PINNED, "a pinned hash moved: behaviour changed");
}

#[test]
fn people_same_hash_on_any_thread_count_and_any_order() {
    let ticks = 2 * DAY;
    let orders: [(&str, Vec<usize>); 3] = [("in order", vec![0, 1, 2, 3]), ("reversed", vec![3, 2, 1, 0]), ("shuffled", vec![2, 0, 3, 1])];
    let reference = people_order(1, &orders[0].1, ticks);
    let mut rows = 0;
    for threads in [1, 2, 4, 8] {
        for (name, order) in &orders {
            let h = people_order(threads, order, ticks);
            println!("4 people, a note, an eavesdropper, {ticks} ticks, {threads} thread(s), people {name:9}: {h:#018x}");
            assert_eq!(h, reference, "{threads} threads, {name}");
            rows += 1;
        }
    }
    println!("{rows} configurations, one hash");
}

#[test]
fn the_teaching_hash_is_not_blind() {
    let h = |s: Setup, t: u32| { let mut v = s.build(); v.run(t); v.state_hash() };
    let base = h(Setup::new(1), 40);
    assert_ne!(base, h(Setup::new(2), 40), "seed");
    assert_ne!(base, h(Setup::new(1).temper(2), 40), "temperament");
    let mut s = Setup::new(1); s.trust[BRYN][ASH] = 601;
    assert_ne!(base, h(s, 40), "one unit of trust");
    let mut v = Setup::new(1).build(); v.run(40);
    let t = v.folk[BRYN].told[0].thing;
    let k = v.folk[BRYN].m.store.find(t, v.folk[BRYN].ids.water, fv_mind_lab::graph::lt::KIND).unwrap();
    let w = v.folk[BRYN].m.store.slow(k);
    let h0 = v.state_hash();
    v.folk[BRYN].m.store.set_slow(k, w + 1);
    assert_ne!(h0, v.state_hash(), "one unit of one told link");
}
