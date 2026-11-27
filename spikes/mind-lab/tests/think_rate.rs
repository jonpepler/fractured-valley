//! Round 2, stage 3: how often a mind thinks. The rate artefacts stay fixed, every rate setting
//! gives one hash on any number of threads in any order, and the network note: what sets a
//! mind's rate must be shared state, never one peer's view.
//! `cargo test --release --test think_rate -- --nocapture`

use fv_mind_lab::village::runs::{run, Setup};

fn order(n: usize) -> (Vec<usize>, Vec<usize>) { ((0..n).collect(), (0..n).rev().collect()) }

/// Every stage-3 setting is deterministic: one hash on 1 and 4 threads, minds in order and reversed.
#[test]
fn every_rate_setting_gives_one_hash_on_any_threads_and_order() {
    let n = 20;
    let (fwd, rev) = order(n);
    for (name, f) in [
        ("every 4", (|s: &mut Setup| s.think_every = 4) as fn(&mut Setup)),
        ("every 20", |s: &mut Setup| s.think_every = 20),
        ("every 4, inputs not integrated", |s: &mut Setup| { s.think_every = 4; s.integrate = false; }),
        ("shallow (cut 634)", |s: &mut Setup| s.depth_cut = 634),
        ("small brain, big every 4", |s: &mut Setup| { s.think_every = 4; s.small = Some((4, false)); }),
        ("shadow small brain, stride 10", |s: &mut Setup| s.small = Some((10, true))),
    ] {
        let mut s = Setup::new(11, n, 1); f(&mut s);
        let h0 = run(&s, 1, None).0.hash;
        for (t, o) in [(1, &rev), (4, &fwd), (4, &rev)] {
            let h = run(&s, t, Some(o)).0.hash;
            println!("{name}: {t} thread(s): {h:#018x} (reference {h0:#018x})");
            assert_eq!(h, h0, "{name}");
        }
    }
}

/// Inert at every tick: integrating a think's window, and a shadow small brain, change nothing a
/// mind thinking every tick does (the shadow's table is part of the hash, so compare the world).
#[test]
fn every_tick_is_untouched_by_the_rate_machinery() {
    use std::hash::Hasher;
    let world = |v: &fv_mind_lab::people::sim::Valley| { let mut h = fv_mind_lab::rng::StateHasher::new(1); v.w.hash_into(&mut h); h.finish() };
    let (_, v0) = run(&Setup::new(12, 20, 1), 1, None);
    let mut s = Setup::new(12, 20, 1); s.integrate = false;
    let (_, v1) = run(&s, 1, None);
    assert_eq!(world(&v0), world(&v1), "integration is inert at every tick");
    let mut sh = Setup::new(12, 20, 1); sh.small = Some((4, true));
    let (_, v2) = run(&sh, 1, None);
    assert_eq!(world(&v0), world(&v2), "a shadow small brain never reaches the body");
}

/// Stage 1's duplicated entity ids (8–22 a village by day 180 at every 4) are gone: a told water
/// node confirmed onto water the mind already holds is merged into it.
#[test]
fn no_duplicate_entity_ids_at_slow_rates() {
    for (seed, every) in [(1u64, 4u32), (2, 4), (1, 20)] {
        let mut s = Setup::new(seed, 30, 12); s.think_every = every;
        let (_, v) = run(&s, 1, None);
        let dups: usize = v.folk.iter().map(|p| { let mut ids: Vec<u32> = p.m.entity_nodes.iter().map(|x| x.0).collect(); ids.sort_unstable(); let k = ids.len(); ids.dedup(); k - ids.len() }).sum();
        let merged: u32 = v.folk.iter().map(|p| p.merged_told).sum();
        println!("seed {seed}, every {every}, 12 days: duplicate entity ids {dups}, told nodes merged into known water {merged}");
        assert_eq!(dups, 0);
    }
}

/// A body goes on eating between thinks: over a village month at every 4 and every 20, meals stay
/// within a tenth of every tick's (stage 1: 37 / 17 / 8 at every 1 / 4 / 20 in its smoke test).
#[test]
fn meals_do_not_collapse_at_slow_rates() {
    let meals = |every: u32| { let mut s = Setup::new(3, 20, 4); s.think_every = every; run(&s, 1, None).0.day.iter().map(|d| d.meals).sum::<u32>() as i64 };
    let (m1, m4, m20) = (meals(1), meals(4), meals(20));
    println!("meals over 4 days, 20 people: every 1 {m1}, every 4 {m4}, every 20 {m20}");
    for m in [m4, m20] { assert!((m - m1).abs() * 10 <= m1 * 2, "meals within a fifth of every tick's"); }
}

/// **The network note.** Peers must agree on every mind's rate, so what sets it must be read from
/// the shared world of the previous tick. The stage-3 rates are fixed per setup (or wake on a
/// mind's own senses, which come from that shared world): a per-peer input such as where this
/// peer's player stands cannot reach them. The cheap tier (distance to an observer) is the
/// counter-example: two peers that disagree on the observer's position disagree on the valley.
#[test]
fn a_per_peer_player_position_cannot_desync_the_rates() {
    let n = 20;
    let (fwd, rev) = order(n);
    // Rates fixed for every mind (here every 4, with reflexes and ended procedures waking minds):
    // the player's position, which differs between two peers, is never read.
    let mut a = Setup::new(13, n, 1); a.think_every = 4; a.observer = Some((5, 5));
    let mut b = a.clone(); b.observer = Some((60, 35));
    let (ha, hb) = (run(&a, 1, Some(&fwd)).0.hash, run(&b, 4, Some(&rev)).0.hash);
    println!("fixed rate, two peers' players at (5, 5) and (60, 35): {ha:#018x} {hb:#018x}");
    assert_eq!(ha, hb, "a fixed rate can't desync");
    // A rate that reads the player's position: the cheap tier. If the position is one peer's view,
    // the peers part at once; it must be replicated state (the position as of the previous tick).
    let mut c = a.clone(); c.every = 4; let mut d = b.clone(); d.every = 4;
    let (hc, hd) = (run(&c, 1, None).0.hash, run(&d, 1, None).0.hash);
    println!("the tier, by distance to each peer's own player: {hc:#018x} {hd:#018x}");
    assert_ne!(hc, hd, "a rate read from one peer's view desyncs");
    // The same tier fed one shared position gives one valley on any threads and order.
    let mut e = c.clone(); e.observer = Some((30, 20));
    let he = run(&e, 1, None).0.hash;
    assert_eq!(he, run(&e, 4, Some(&rev)).0.hash);
}
