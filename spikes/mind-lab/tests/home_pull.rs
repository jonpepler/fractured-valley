//! Stage 3, fix 1 and 2: home's pull, before and after. Stage 2 found that a quiet day at the camp
//! saturates camp → comfort and camp → rest, and that comfort wasn't gated by need, so a thirsty
//! person stayed home. Stage 3 gates comfort by need (fatigue, stress, low mood), adds a cost of
//! distance (`Config::walk_cost`) and commitment to a goal on the way (`Config::hold_goal`).
//! Each setting alone and together, over 6 seeds × 3 temperament sets.
//! `cargo test --release --test home_pull -- --nocapture`

use fv_mind_lab::people::runs::*;

#[test]
fn a_thirsty_person_leaves_home_after_quiet_days() {
    let variants: [(&str, Choosing); 5] = [("stage 2 (all off)", STAGE2), ("comfort gated only", (true, 0, false)), ("walking cost only", (false, fv_mind_lab::mind::WALK_COST, false)),
        ("commitment only", (false, 0, true)), ("stage 3 (all on)", STAGE3)];
    let mut table = vec![];
    for (name, c) in variants {
        for days in [1u32, 3] {
            let (mut left, mut drank, mut dith, mut n, mut comfort, mut rest) = (0, 0, 0, 0, 0, 0);
            let mut per_seed = vec![];
            for seed in 1..=6u64 {
                let mut s_drank = 0;
                for set in 0..3 {
                    let r = home_pull(seed, set, days, c);
                    n += 1; comfort += r.camp_comfort; rest += r.camp_rest; dith += r.dithers;
                    if r.left_at.is_some() { left += 1; }
                    if r.drank_at.is_some() { drank += 1; s_drank += 1; }
                }
                per_seed.push(s_drank);
            }
            println!("{name:22} after {days} quiet day(s): camp→comfort {} camp→rest {} | leaves home in {left}/{n}, drinks within 120 ticks in {drank}/{n} (per seed {per_seed:?}), reversals {dith}", comfort / n, rest / n);
            table.push((name, days, left, drank, n));
        }
    }
    let get = |nm: &str, d: u32| table.iter().find(|x| x.0 == nm && x.1 == d).unwrap().3;
    // (Review: the old assertion could not fail.) Stage 2 already left home here, so this probe
    // only checks stage 3 is no worse; the case stage 2 failed is the next test. The walking cost
    // alone is worse (home is 0 cells away): printed, and it is why it ships with the comfort gate.
    assert!(get("stage 3 (all on)", 1) >= get("stage 2 (all off)", 1) && get("stage 3 (all on)", 3) >= get("stage 2 (all off)", 3));
    assert!(get("walking cost only", 3) < get("stage 3 (all on)", 3), "the walking cost alone keeps people home");
    assert!(get("stage 3 (all on)", 3) * 3 >= 18 * 2, "after three quiet days, most thirsty people still go for water");
}

/// Stage 2's own case: Bryn, told of the spring, then quiet nights with Ash away; on nights 1 and 5
/// a copy is made thirsty at the camp with nobody about (`use_probe`). Does it walk to the told
/// spring, and does it walk to any water at all?
#[test]
fn stage_2s_told_and_quiet_case() {
    let mut night5 = vec![];
    for (name, c) in [("stage 2", STAGE2), ("comfort gated only", (true, 0, false)), ("stage 3", STAGE3)] {
        let (mut goes, mut pref, mut n) = ([0; 2], [0; 2], 0);
        let mut per_seed = vec![];
        for seed in 1..=6u64 {
            let mut s = 0;
            for set in 0..3 {
                let f = s5_fade_with(seed, set, 800, 300, 5, &[1, 5], true, c);
                n += 1;
                for (i, (_, u)) in f.uses.iter().enumerate() { if u.goes { goes[i] += 1; if i == 0 { s += 1; } } if u.prefers() { pref[i] += 1; } }
            }
            per_seed.push(s);
        }
        night5.push(goes[1]);
        println!("{name:20}: walks to the told spring on night 1 / 5 in {} / {} of {n} (night 1 per seed {per_seed:?}); prefers it in its own weighing {} / {}", goes[0], goes[1], pref[0], pref[1]);
    }
    assert!(night5[2] > night5[0], "after five quiet nights, gating comfort lets a thirsty Bryn leave home for the told spring");
}
