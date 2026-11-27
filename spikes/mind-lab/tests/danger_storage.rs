//! Stage 3: what danger costs. Bytes a plan, an ally link and the danger state add to a person's
//! mind; a boar's mind; and µs per tick of the danger valley (four people, one boar), against the
//! same valley with no boar and no plans.
//! `cargo test --release --test danger_storage -- --nocapture --test-threads=1`

use fv_mind_lab::danger::runs::*;
use fv_mind_lab::people::scenarios::BRYN;
use std::time::Instant;

fn own(m: &fv_mind_lab::mind::Mind) -> (usize, usize, usize, usize) { m.bytes() }

#[test]
fn bytes_and_time() {
    let c = Case { seed: 1, temper: TEMPERS[1].1, allies: true, expected: true };
    // A plan: build without, then add it.
    let (mut v0, _) = build(&c, 0, 2);
    let before = own(&v0.folk[BRYN].m);
    let p = give_plan(&mut v0.folk[BRYN], true);
    let after = own(&v0.folk[BRYN].m);
    let links = fv_mind_lab::store::Store::links_vec(&v0.folk[BRYN].m.store, p).len();
    println!("a person's mind: shared base {} B; own {} B, fast {} B, state {} B", before.0, before.1, before.2, before.3);
    println!("a hunter's plan: +{} B own ({} links out of the plan node, plus links into it), +{} B state", after.1 - before.1, links, after.3 as i64 - before.3 as i64);
    let (v1, _) = build(&c, 700, 2);
    let w_ally = own(&v1.folk[BRYN].m);
    let (v2, _) = build(&c, 0, 2);
    let n_ally = own(&v2.folk[BRYN].m);
    println!("two ally links: +{} B own", w_ally.1 as i64 - n_ally.1 as i64);
    let (o, v) = encounter_with(&c, 700, 2, false);
    let e = own(&v.folk[BRYN].m);
    println!("after the encounter ({} ticks): own {} B (+{}), fast {} B, state {} B; held state is {} B of it", WATCH, e.1, e.1 as i64 - w_ally.1 as i64, e.2, e.3, std::mem::size_of::<fv_mind_lab::mind::Held>());
    let b = v.beasts[0].bytes();
    println!("a boar's mind: shared base {} B, own {} B, fast {} B, state {} B (first response of Bryn: {})", b.0, b.1, b.2, b.3, rname(o.first));
    // Time: best of 5, the watch after the meeting.
    let time = |boar: bool| -> f64 {
        let mut best = f64::MAX;
        for _ in 0..5 {
            let (mut v, _) = build(&c, 700, 2);
            if !boar { v.beasts.clear(); v.w.boars.clear(); for p in v.folk.iter_mut() { p.m.held.intent = None; } }
            if boar { meet(&mut v, 1, 8); } else { v.run_until(MEET); }
            let t = Instant::now();
            v.run(WATCH);
            best = best.min(t.elapsed().as_secs_f64() * 1e6 / WATCH as f64);
        }
        best
    };
    let (with, without) = (time(true), time(false));
    println!("µs per tick of the valley: with the boar and plans {with:.1} (3 people awake + 1 boar), without {without:.1}");
    assert!(with > 0.0);
}
