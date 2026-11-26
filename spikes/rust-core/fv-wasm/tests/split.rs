//! Native checks of what the browser relies on: driving a tick in parts
//! (through the byte wire format) gives exactly `Sim::step`'s result, and the
//! spot-check catches a wrong value and repairs it.

use fv_wasm::{scenario, Engine};

fn solo(ticks: usize) -> Vec<String> {
    let mut s = Engine::new(5, 32, 32, 40);
    (0..ticks).map(|_| { s.step().unwrap(); s.hash() }).collect()
}

/// Two "peers" in one process: each computes half the bricks and passes the
/// bytes to the other. `corrupt_at`: peer 0 sends one wrong value that tick.
fn pair(ticks: usize, corrupt_at: Option<f64>) -> (Vec<String>, Vec<String>, Vec<f64>) {
    let mut p = [Engine::new(5, 32, 32, 40), Engine::new(5, 32, 32, 40)];
    let (mut h0, mut h1, mut caught) = (vec![], vec![], vec![]);
    for _ in 0..ticks {
        for e in p.iter_mut() { e.begin_tick().unwrap() }
        let n = p[0].brick_count();
        let share = |me: u32| (0..n).filter(|b| b % 2 == me).collect::<Vec<u32>>();
        let out0 = p[0].diffuse_part(&share(0)).unwrap();
        let out1 = p[1].diffuse_part(&share(1)).unwrap();
        let sent0 = if Some(p[0].tick()) == corrupt_at { p[0].corrupt_for_test(&out0).unwrap() } else { out0.clone() };
        p[0].add_part(&out0).unwrap();
        assert!(p[0].check_and_add_part(&out1).unwrap().is_empty());
        p[1].add_part(&out1).unwrap();
        if !p[1].check_and_add_part(&sent0).unwrap().is_empty() { caught.push(p[1].tick()) }
        for e in p.iter_mut() { e.finish_tick().unwrap() }
        h0.push(p[0].hash());
        h1.push(p[1].hash());
        assert_eq!(p[1].energy_gap(), "0");
        assert_eq!(p[1].mass_gap(), "0");
    }
    (h0, h1, caught)
}

#[test]
fn a_tick_in_parts_over_bytes_matches_sim_step() {
    let want = solo(60);
    let (a, b, caught) = pair(60, None);
    assert_eq!(a, want);
    assert_eq!(b, want);
    assert!(caught.is_empty());
}

#[test]
fn a_wrong_value_is_caught_and_repaired() {
    let want = solo(30);
    let (a, b, caught) = pair(30, Some(12.0));
    assert_eq!(caught, vec![12.0]);
    assert_eq!(a, want);
    assert_eq!(b, want, "the receiver used its own result for the bad brick");
}

#[test]
fn a_checkpoint_round_trips_through_the_engine() {
    let mut a = Engine::new(5, 32, 32, 40);
    for _ in 0..20 { a.step().unwrap() }
    // Rigid bodies are not in checkpoints yet: wait until nothing is falling.
    while !a.can_checkpoint() { a.step().unwrap() }
    let mut b = Engine::import(&a.export().unwrap()).unwrap();
    assert_eq!(a.hash(), b.hash());
    for _ in 0..20 { a.step().unwrap(); b.step().unwrap(); assert_eq!(a.hash(), b.hash()) }
    let _ = scenario; // the same scenario the example uses
}
