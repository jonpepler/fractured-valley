//! Stage 4 storage and cost: what hearing and joining a plan adds to a mind, the prediction
//! structures, the hunting layer, the deer's mind, and µs per tick for a hunt valley.
//! `cargo test --release --test hunt_storage -- --nocapture`

use fv_mind_lab::hunt::runs::{build, build_betrayal, BCase, Case, Lean, Mode};
use fv_mind_lab::mind::{Expect, PlanState};
use fv_mind_lab::hunt::hunter::{Held, Hunter};
use fv_mind_lab::think::Form;
use std::time::Instant;

#[test]
fn what_a_plan_costs() {
    let mut v = build(&Case { seed: 7, set: 1, mode: Mode::Planned });
    let before: Vec<_> = v.folk.iter().map(|p| (p.m.bytes(), p.m.store.len())).collect();
    for _ in 0..3 { v.step(1, &[0, 1, 2, 3]); }
    for (i, p) in v.folk.iter().enumerate() {
        let ((sh, own0, _, st0), n0) = before[i];
        let ((_, own1, _, st1), n1) = (p.m.bytes(), p.m.store.len());
        let h = p.hunt.as_ref().unwrap();
        println!("{}: shared {sh} B; own {own0} → {own1} B (+{}), nodes {n0} → {n1}; state {st0} → {st1} B; predictions {} × {} B; joined {:?}",
            ["Ash", "Bryn", "Cai", "Dee"][i], own1 - own0, h.plan.as_ref().map(|_| p.m.plan.expects.len()).unwrap_or(0), std::mem::size_of::<Expect>(), h.log.decisions.iter().map(|d| d.2).collect::<Vec<_>>());
    }
    let h = v.folk[1].hunt.as_ref().unwrap();
    let held_heap = h.plan.as_ref().map(|p| p.members.len() * 24 + p.agreed.len() * 8).unwrap_or(0);
    println!("a plan held (people layer): {} B + {held_heap} B of members; the hunting layer: {} B + logs; PlanState {} B; Expect {} B",
        std::mem::size_of::<Held>(), std::mem::size_of::<Hunter>(), std::mem::size_of::<PlanState>(), std::mem::size_of::<Expect>());
    let d = &v.deer_minds[0]; let (sh, own, fast, st) = d.bytes();
    println!("a deer's mind: shared {sh} B, own {own} B, fast {fast} B, state {st} B ({} nodes)", d.store.len());
    assert!(std::mem::size_of::<Expect>() <= 48);
}

fn time(mut f: impl FnMut() -> fv_mind_lab::people::sim::Valley, ticks: u32) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..5 { let mut v = f(); let t = Instant::now(); for _ in 0..ticks { v.step(1, &[0, 1, 2, 3]); } best = best.min(t.elapsed().as_secs_f64() * 1e6 / ticks as f64); }
    best
}

#[test]
fn cost_per_tick() {
    let planned = time(|| build(&Case { seed: 7, set: 1, mode: Mode::Planned }), 120);
    let solo = time(|| build(&Case { seed: 7, set: 1, mode: Mode::Solo }), 120);
    let bare = time(|| { let mut v = build(&Case { seed: 7, set: 1, mode: Mode::Solo }); for p in v.folk.iter_mut() { p.hunt = None; p.m.plan = PlanState::default(); p.m.held.intent = None; } v }, 120);
    let betray = time(|| build_betrayal(&BCase { seed: 1, set: 1, lean: Lean::Cai, caution: 10, honest_teller: false }), 120);
    println!("µs a tick (4 people + a deer, best of 5, 120 ticks): planned hunt {planned:.1}, each alone {solo:.1}, no hunting layer {bare:.1}; the betrayal valley {betray:.1}");
}
