//! Round 2, stage 1 (foundations): the new rules, each shown on its own.
//! `cargo test --release --test round2 -- --nocapture`

use fv_mind_lab::graph::lt;
use fv_mind_lab::people::person::{Person, REP_KEEP};
use fv_mind_lab::people::scenarios::{Setup, ASH, BRYN};
use fv_mind_lab::people::world::SPRING;
use fv_mind_lab::store::Store;
use fv_mind_lab::village::runs::{run, Setup as VSetup};

fn ash_tells_bryn_of_the_spring(v: &mut fv_mind_lab::people::sim::Valley) -> Option<usize> {
    let ash = &v.folk[ASH];
    let pn = ash.lex_place(SPRING).unwrap();
    let pi = ash.m.place_nodes.iter().position(|&p| p == pn).unwrap();
    let thing = ash.water_things().into_iter().find(|x| x.1 == pi).unwrap().0;
    let c = ash.describe(pi, false, false, BRYN, thing);
    let tick = v.w.tick;
    let mut cues = vec![];
    v.folk[BRYN].ground(&c, Some(ASH), false, None, tick, &mut cues)
}
fn fact(p: &Person, thing: u32) -> i32 { p.m.store.find(thing, p.ids.water, lt::KIND).map(|k| p.m.store.slow(k) as i32).unwrap_or(0) }

/// 9.8: a told place that is never visited fades into cold storage (kept, out of the running
/// graph); being told of it again brings back the same node with what was attached, and the
/// relearning starts from where the links had faded to (savings).
#[test]
fn a_told_place_never_visited_goes_dormant_and_a_telling_brings_it_back() {
    let mut v = Setup::new(1).temper(0).build();
    let t = ash_tells_bryn_of_the_spring(&mut v).expect("grounded");
    let (thing, place) = (v.folk[BRYN].told[t].thing, v.folk[BRYN].m.place_nodes[v.folk[BRYN].told[t].place]);
    let fresh = fact(&v.folk[BRYN], thing);
    // Fade faster than people do (the mechanism, not the rate): 200 per mille a night.
    let b = &mut v.folk[BRYN];
    for f in b.m.cfg.own_fade.iter_mut() { if *f > 0 { *f = 200; } }
    let hot0 = b.m.bytes().1;
    let mut night = None;
    let sense = v.w.sense(BRYN, true);
    for k in 0..40 { b.night(&sense); if b.m.is_cold(place) && night.is_none() { night = Some(k + 1); } }
    let (cold_nodes, cold_b, hot1) = (b.m.cold.len(), b.m.cold_bytes(), b.m.bytes().1);
    if std::env::var("FV_DEBUG").is_ok() { for l in b.m.store.links_vec(place) { eprintln!("place → {} {} {} {}", b.m.name(l.to), lt::NAMES[l.ty as usize], l.slow, l.fast); } for u in 0..fv_mind_lab::think::Form::len(&b.m.store) as u32 { for l in b.m.store.links_vec(u) { if l.to == place { eprintln!("{} → place {} {}", b.m.name(u), lt::NAMES[l.ty as usize], l.slow); } } } eprintln!("cold {:?}", b.m.cold.iter().map(|d| b.m.name(d.node)).collect::<Vec<_>>()); }
    println!("told fact {fresh} at first; dormant from night {night:?}: {cold_nodes} nodes in cold storage ({cold_b} B); own hot bytes {hot0} → {hot1}; told records {}", b.told.len());
    assert!(night.is_some() && b.m.is_cold(thing), "the told place and its water go dormant once every connection has faded");
    assert!(b.told.is_empty(), "the told record goes with the claim (it fades like everything else)");
    assert!(hot1 < hot0, "dormant memory costs nothing in the running graph");
    // Told again (Ash describes it the same way): the same nodes come back.
    let t2 = ash_tells_bryn_of_the_spring(&mut v).expect("grounded again");
    let b = &v.folk[BRYN];
    let again = fact(b, b.told[t2].thing);
    println!("told again: same place node {}, same water node {}; fact {again} against {fresh} for a first telling; revived {} times", b.m.place_nodes[b.told[t2].place] == place, b.told[t2].thing == thing, b.m.dormancy.1);
    assert_eq!(b.m.place_nodes[b.told[t2].place], place, "a telling is a cue: the dormant place comes back");
    assert_eq!(b.told[t2].thing, thing, "and with it what was held there");
    assert!(again > fresh, "savings: relearning from the faded links beats learning afresh");
}

/// 9.8: revisiting the place is a cue too.
#[test]
fn revisiting_a_dormant_place_brings_it_back() {
    let mut v = Setup::new(2).temper(0).build();
    let t = ash_tells_bryn_of_the_spring(&mut v).expect("grounded");
    let (pi, thing) = (v.folk[BRYN].told[t].place, v.folk[BRYN].told[t].thing);
    let place = v.folk[BRYN].m.place_nodes[pi];
    let b = &mut v.folk[BRYN];
    let (x, y) = b.place_xy(pi);
    for f in b.m.cfg.own_fade.iter_mut() { if *f > 0 { *f = 200; } }
    let sense = v.w.sense(BRYN, true);
    for _ in 0..40 { b.night(&sense); }
    assert!(b.m.is_cold(place));
    // Stand there, awake.
    let mut s = v.w.sense(BRYN, false); s.body.x = x; s.body.y = y; s.body.asleep = false; s.seen.clear();
    v.folk[BRYN].m.step(&s);
    let b = &v.folk[BRYN];
    println!("standing at the dormant place: place back {}, water back {}", !b.m.is_cold(place), !b.m.is_cold(thing));
    assert!(!b.m.is_cold(place) && !b.m.is_cold(thing), "standing where it was brings the place and its water back");
}

/// 9.4: feelings about people decay towards the person's own baseline (beta reputation with
/// forgetting); under steady evidence they level off instead of climbing to the ceiling.
#[test]
fn feelings_decay_to_a_baseline_and_level_off() {
    let mut v = Setup::new(3).temper(0).build();
    let sense = v.w.sense(BRYN, true);
    let b = &mut v.folk[BRYN];
    let (t0, w0) = (b.trust(ASH), b.warmth(ASH));
    b.feel_evidence(ASH, 5000, 0, 5000, 0);
    let (t1, w1) = (b.trust(ASH), b.warmth(ASH));
    let mut tr = vec![];
    for _ in 0..200 { b.night(&sense); tr.push((b.trust(ASH), b.warmth(ASH))); }
    println!("trust {t0} → {t1} after good news → {:?} after 50 nights → {:?} after 200 (keeping {REP_KEEP}‰ a night)", tr[49], tr[199]);
    assert!(t1 > t0 && w1 > w0);
    assert!(tr[199].0 < tr[49].0 && tr[49].0 < t1 && (tr[199].0 - t0) * 10 < t1 - t0, "with nothing new, trust decays back towards the baseline");
    // Steady good news every night: it levels off below the ceiling.
    let mut lv = vec![];
    for _ in 0..400 { b.feel_evidence(ASH, 1000, 0, 1000, 0); b.night(&sense); lv.push(b.warmth(ASH)); }
    println!("warmth under one piece of good news a night: night 1 {}, 100 {}, 200 {}, 400 {}", lv[0], lv[99], lv[199], lv[399]);
    assert!((lv[399] - lv[199]) * 10 < lv[199] - lv[0] && lv[399] < 1000, "steady evidence levels off below the ceiling");
}

/// 9.4: day learning and consolidation leave feelings about people alone (round 1: trust drifted
/// up in 100 of 100 villages, partly because a person and "truth" were co-active).
#[test]
fn idle_consolidation_does_not_move_trust() {
    for protect in [true, false] {
        let mut s = VSetup::new(7, 12, 2); s.gossip = false;
        let mut v = fv_mind_lab::village::runs::build(&s);
        for p in v.folk.iter_mut() { p.m.cfg.protect_social = protect; }
        let before: Vec<i32> = v.folk.iter().flat_map(|p| (0..12).filter(move |&j| j != p.i).map(move |j| p.trust(j))).collect();
        let order: Vec<usize> = (0..12).collect();
        for _ in 0..2 * 301 { v.step(1, &order); }
        let after: Vec<i32> = v.folk.iter().flat_map(|p| (0..12).filter(move |&j| j != p.i).map(move |j| p.trust(j))).collect();
        let moved = before.iter().zip(&after).filter(|(a, b)| a != b).count();
        let up = before.iter().zip(&after).map(|(a, b)| (*b - *a) as i64).sum::<i64>();
        println!("feelings protected {protect}: {moved} of {} trusts moved over two days without gossip, net {up:+}", before.len());
        if protect {
            // Only episodes move trust here (a confirmed claim, a hunt that fed someone).
            let ev: usize = v.folk.iter().map(|p| p.rel.iter().filter(|r| r.rt + r.st > 0).count()).sum();
            assert!(moved <= ev, "with no episode about someone, trust in them doesn't move");
        }
    }
}

/// 9.4: gossip remembers what was said to whom, and the same news is worth less the second time.
#[test]
fn gossip_heard_twice_counts_less() {
    use fv_mind_lab::people::talk::Say;
    use fv_mind_lab::people::world::Utterance;
    let s = VSetup::new(8, 6, 1);
    let mut v = fv_mind_lab::village::runs::build(&s);
    let p = &mut v.folk[0];
    let w0 = p.warmth(2);
    let u = Utterance { tick: 0, speaker: 1, x: 0, y: 0, vol: 500, to: Some(0), say: Say::Gossip { about: 2, good: true } };
    fv_mind_lab::village::life::hear_gossip(p, &[(u.clone(), 300, true)]);
    let w1 = p.warmth(2);
    fv_mind_lab::village::life::hear_gossip(p, &[(u, 300, true)]);
    let w2 = p.warmth(2);
    println!("warmth towards 2: {w0} → {w1} (first hearing) → {w2} (the same again)");
    assert!(w1 > w0 && w2 - w1 < w1 - w0, "the second hearing adds less");
}

/// 9.4: sleep keeps more of a novel day than an ordinary one.
#[test]
fn sleep_keeps_more_of_a_novel_day() {
    use fv_mind_lab::fox::scenarios;
    let mut quiet = scenarios::sleepless_days(scenarios::SEED, 1);
    quiet.m.skip_sleep = false;
    while quiet.w.tick < scenarios::NIGHT - 1 { quiet.step(); }
    let mut busy = scenarios::busy_day(scenarios::SEED);
    while busy.w.tick < scenarios::NIGHT - 1 { busy.step(); }
    println!("share kept at nightfall: quiet day {}‰ (novelty {}), busy day {}‰ (novelty {})", quiet.m.keep_share(), quiet.m.day_novelty, busy.m.keep_share(), busy.m.day_novelty);
    assert!(busy.m.keep_share() > quiet.m.keep_share());
}

/// 9.5: every mind (people and animals) can think every N ticks. A smoke test, not the study
/// (stage 3): at 1, 4 and 20 the village runs, and each gives one hash on 1, 2 and 4 threads in
/// order and reversed.
#[test]
fn think_every_runs_and_is_deterministic() {
    for every in [1u32, 4, 20] {
        let mut s = VSetup::new(9, 20, 2); s.think_every = every;
        let rev: Vec<usize> = (0..20).rev().collect();
        let (r1, v) = run(&s, 1, None);
        let hs = [r1.hash, run(&s, 2, None).0.hash, run(&s, 4, Some(&rev)).0.hash, run(&s, 1, Some(&rev)).0.hash];
        let thinks: u32 = fv_mind_lab::village::runs::stats(&v).iter().map(|x| x.thinks).sum();
        let meals: u32 = r1.day.iter().map(|d| d.meals).sum();
        let drinks: u32 = r1.day.iter().map(|d| d.drinks).sum();
        let gossip: u32 = r1.day.iter().map(|d| d.gossip).sum();
        println!("think every {every:2}: {thinks} thinks, meals {meals}, drinks {drinks}, gossip {gossip}; hashes {hs:x?}");
        assert!(hs.iter().all(|&h| h == hs[0]), "one hash on any threads and order");
        if every > 1 { assert!(thinks < 20 * 2 * 301 / 2); }
    }
}

/// 9.5: the fox's many-fox valley at a slower thinking rate is deterministic too (all minds).
#[test]
fn foxes_thinking_every_4_ticks_give_one_hash() {
    use fv_mind_lab::fox::sim::Pack;
    let h = |threads: usize, rev: bool| {
        let mut p = Pack::new(5, 12);
        for (k, m) in p.minds.iter_mut().enumerate() { m.every = 4; m.phase = k as u32; }
        let order: Vec<usize> = if rev { (0..12).rev().collect() } else { (0..12).collect() };
        for _ in 0..400 { p.step(threads, &order); }
        p.state_hash()
    };
    let hs = [h(1, false), h(2, true), h(4, false), h(8, true)];
    println!("12 foxes thinking every 4 ticks: {hs:x?}");
    assert!(hs.iter().all(|&x| x == hs[0]));
}
