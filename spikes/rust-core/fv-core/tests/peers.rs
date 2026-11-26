//! Keeps two capabilities alive: a peer can join late from a checkpoint, and
//! peers can split a tick's work, swap results as bytes-in-spirit (whole
//! brick results over channels), check each other, and stay identical to a
//! world computed alone.

use fv_core::checkpoint;
use fv_core::rad::Sky;
use fv_core::rng::hash;
use fv_core::scene::valley;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::transport::{apply_parts, diffuse_part, BrickResult};
use fv_core::units::*;
use std::sync::mpsc;

fn lit(seed: u64) -> Sim {
    let v = valley(seed, 32, 32, 60);
    let (cx, cy) = (v.crib.0.min(28), v.crib.1.min(28));
    let gz = v.ground_at(cx, cy) as u16 + 1;
    let mut sim = Sim::new(v.world, Sky::afternoon());
    for t in 0..40 { sim.schedule(Scheduled { tick: t, peer: 0, seq: 0, command: Command::Heat { x: cx as u16, y: cy as u16, z: gz, energy: joules(250) } }) }
    sim
}

#[test]
fn a_peer_joining_from_a_checkpoint_stays_identical_and_balanced() {
    let mut a = lit(5);
    for _ in 0..30 { a.step() }
    let bytes = checkpoint::export(&a);
    let mut b = checkpoint::import(&bytes);
    assert_eq!(a.hash(), b.hash(), "the checkpoint holds the whole state");
    for _ in 0..30 {
        a.step();
        b.step();
        assert_eq!(a.hash(), b.hash());
        assert_eq!(b.energy_gap(), 0);
        assert_eq!(b.mass_gap(), 0);
    }
}

/// Peer `me` of two owns the bricks where b % 2 == me. Each tick's diffusion
/// phase: compute mine, send them, receive theirs, spot-check a seeded
/// quarter of theirs by recomputing, then finish the phase with both.
fn peer(me: usize, ticks: usize, corrupt_at: Option<u64>, tx: mpsc::Sender<Vec<BrickResult>>, rx: mpsc::Receiver<Vec<BrickResult>>) -> (Vec<u64>, Vec<u64>, usize) {
    let mut sim = lit(5);
    let mismatches = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sent = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (m2, s2) = (mismatches.clone(), sent.clone());
    sim.diffuse_hook = Some(Box::new(move |w, flow| {
        let mine: Vec<usize> = w.brick_ids().filter(|b| b % 2 == me).collect();
        let mut out = diffuse_part(w, flow, &mine);
        if me == 0 && Some(w.tick) == corrupt_at {
            // Send one wrong value in a brick the other peer will check.
            let k = out.iter().position(|r| hash(w.seed, w.tick, r.brick as u64, 0xc4ec) % 4 == 0).unwrap();
            out[k].cells[7].energy.0 ^= 1;
        }
        s2.fetch_add(out.len(), std::sync::atomic::Ordering::Relaxed);
        tx.send(out.clone()).unwrap();
        let theirs = rx.recv().unwrap();
        let check: Vec<usize> = theirs.iter().map(|r| r.brick).filter(|b| hash(w.seed, w.tick, *b as u64, 0xc4ec) % 4 == 0).collect();
        let redo = diffuse_part(w, flow, &check);
        for r in &redo {
            let got = theirs.iter().find(|t| t.brick == r.brick).unwrap();
            if got != r { m2.lock().unwrap().push(w.tick) }
        }
        let all: Vec<BrickResult> = out.into_iter().chain(theirs).collect();
        apply_parts(w, &all);
    }));
    let mut hashes = Vec::new();
    for _ in 0..ticks { sim.step(); hashes.push(sim.hash()) }
    let m = mismatches.lock().unwrap().clone();
    (hashes, m, sent.load(std::sync::atomic::Ordering::Relaxed))
}

fn two_peers(ticks: usize, corrupt_at: Option<u64>) -> [(Vec<u64>, Vec<u64>, usize); 2] {
    let (tx0, rx1) = mpsc::channel();
    let (tx1, rx0) = mpsc::channel();
    let a = std::thread::spawn(move || peer(0, ticks, corrupt_at, tx0, rx0));
    let b = std::thread::spawn(move || peer(1, ticks, corrupt_at, tx1, rx1));
    [a.join().unwrap(), b.join().unwrap()]
}

#[test]
fn two_peers_splitting_the_work_match_a_world_computed_alone() {
    let mut solo = lit(5);
    let want: Vec<u64> = (0..40).map(|_| { solo.step(); solo.hash() }).collect();
    let [p0, p1] = two_peers(40, None);
    assert_eq!(p0.0, want);
    assert_eq!(p1.0, want);
    assert!(p0.1.is_empty() && p1.1.is_empty(), "honest peers pass every spot-check");
    assert!(p0.2 > 100 && p1.2 > 100, "both really did share the work");
}

#[test]
fn a_peer_sending_a_wrong_brick_is_caught_the_tick_it_happens() {
    let [_, p1] = two_peers(20, Some(12));
    assert_eq!(p1.1.first(), Some(&12));
}
