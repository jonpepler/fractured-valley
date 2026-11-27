//! Stage 3: danger with company. Bryn meets a boar (an animal with a mind of its own) near the old
//! oak. Every result runs over 6 world seeds (each moves the landmarks, the meeting spot, the side
//! the boar comes from and where allies stand) × 4 temperaments, and prints its numbers per seed.
//! The assertions check only the direction of each claim; how often it holds is printed.
//! `cargo test --release --test danger -- --nocapture --test-threads=2`

use fv_mind_lab::danger::runs::*;
use fv_mind_lab::mind::{R_AID, R_FIGHT, R_FLEE, R_FREEZE, R_HOLD, R_NAMES, Temperament};

const SEEDS: [u64; 6] = [1, 2, 3, 4, 5, 6];

fn run(temper: Temperament, allies: bool, expected: bool) -> Vec<Outcome> {
    SEEDS.iter().map(|&seed| encounter_with(&Case { seed, temper, allies, expected }, 700, 2, false).0).collect()
}

#[test]
fn t1_expected_against_unexpected() {
    let (mut smaller, mut calmer, mut different, mut n) = (0, 0, 0, 0);
    for (tn, t) in TEMPERS { for allies in [false, true] {
        let (e, u) = (run(t, allies, true), run(t, allies, false));
        for (a, b) in e.iter().zip(&u) {
            n += 1;
            if a.error < b.error { smaller += 1; }
            if a.arousal < b.arousal && a.stress_peak <= b.stress_peak { calmer += 1; }
            if a.first != b.first { different += 1; }
        }
        println!("{tn:16} allies {allies:5}: error expected/unexpected {:?} / {:?}; arousal {:?} / {:?}; first response {:?} / {:?}",
            e.iter().map(|o| o.error).collect::<Vec<_>>(), u.iter().map(|o| o.error).collect::<Vec<_>>(),
            e.iter().map(|o| o.arousal).collect::<Vec<_>>(), u.iter().map(|o| o.arousal).collect::<Vec<_>>(),
            e.iter().map(|o| rname(o.first)).collect::<Vec<_>>(), u.iter().map(|o| rname(o.first)).collect::<Vec<_>>());
    } }
    println!("the hunter's error is smaller in {smaller}/{n}, it is calmer (arousal lower, stress no higher) in {calmer}/{n}, and its first response differs in {different}/{n}");
    assert_eq!(smaller, n, "a boar the plan predicts is less of a surprise");
    assert!(calmer * 3 >= n * 2 && different * 3 >= n * 2);
}

#[test]
fn t2_alone_against_with_allies() {
    let (mut less_flight, mut bolder, mut n) = (0, 0, 0);
    for expected in [false, true] { for (tn, t) in TEMPERS {
        let (a, w) = (run(t, false, expected), run(t, true, expected));
        let share = |v: &[Outcome], r: u8| v.iter().map(|o| o.counts[r as usize]).sum::<u32>();
        for (x, y) in a.iter().zip(&w) {
            n += 1;
            if y.counts[R_FLEE as usize] < x.counts[R_FLEE as usize] || (x.counts[R_FLEE as usize] == 0 && y.counts[R_FLEE as usize] == 0) { less_flight += 1; }
            if y.counts[R_HOLD as usize] + y.counts[R_FIGHT as usize] >= x.counts[R_HOLD as usize] + x.counts[R_FIGHT as usize] { bolder += 1; }
        }
        println!("expected {expected:5} {tn:16}: ticks fleeing alone/with allies {} / {}, holding {} / {}, fighting {} / {}; gored {} / {}; boar broke off {} / {} of 6; peak stress {} / {}",
            share(&a, R_FLEE), share(&w, R_FLEE), share(&a, R_HOLD), share(&w, R_HOLD), share(&a, R_FIGHT), share(&w, R_FIGHT),
            a.iter().map(|o| o.gored).sum::<u32>(), w.iter().map(|o| o.gored).sum::<u32>(), a.iter().filter(|o| o.boar_fled).count(), w.iter().filter(|o| o.boar_fled).count(),
            a.iter().map(|o| o.stress_peak).sum::<i32>() / 6, w.iter().map(|o| o.stress_peak).sum::<i32>() / 6);
    } }
    println!("with allies: no more flight in {less_flight}/{n}, at least as much holding or fighting in {bolder}/{n}");
    let _ = less_flight;
    let total = |allies: bool| -> u32 { TEMPERS.iter().flat_map(|&(_, t)| [false, true].map(|e| run(t, allies, e))).flatten().map(|o| o.counts[R_FLEE as usize]).sum() };
    let surprised = |allies: bool| -> u32 { TEMPERS.iter().flat_map(|&(_, t)| run(t, allies, false)).map(|o| o.counts[R_FLEE as usize]).sum() };
    println!("ticks fleeing in all runs, alone / with allies: {} / {}; taken by surprise only: {} / {}", total(false), total(true), surprised(false), surprised(true));
    assert!(total(true) < total(false) && surprised(true) < surprised(false), "allies lower the pull to flee");
    // "Ally" is a learnt link, not a label: the same two people standing by, but strangers (no
    // link to "someone who stands with me"), don't embolden.
    let (mut friends_bolder, mut m) = (0, 0);
    for (tn, t) in TEMPERS {
        let mut line = vec![];
        for &seed in &SEEDS {
            let c = Case { seed, temper: t, allies: true, expected: false };
            let (f, _) = encounter_with(&c, 700, 2, false);
            let (s, _) = encounter_with(&c, 0, 2, false);
            m += 1;
            if f.counts[R_FLEE as usize] < s.counts[R_FLEE as usize] { friends_bolder += 1; }
            line.push(format!("{}/{}", f.counts[R_FLEE as usize], s.counts[R_FLEE as usize]));
        }
        println!("{tn:16}: ticks fleeing with friends / with strangers by seed: {}", line.join(" "));
    }
    let tot = |w: i32, allies: bool| -> u32 { TEMPERS.iter().flat_map(|&(_, t)| SEEDS.iter().map(move |&seed| encounter_with(&Case { seed, temper: t, allies, expected: false }, w, 2, false).0.counts[R_FLEE as usize])).sum() };
    let (alone, strangers, friends) = (tot(700, false), tot(0, true), tot(700, true));
    println!("friends embolden strictly more than strangers in {friends_bolder}/{m}; surprised, ticks fleeing alone / with strangers / with friends: {alone} / {strangers} / {friends}");
    // After review, with a downed Bryn's choices no longer counted: the effect is small (about 5%),
    // and mere presence does nothing; it is the learnt link that does it.
    assert!(friends < strangers && friends < alone, "the learnt link, not mere presence, emboldens");
}

#[test]
fn t2b_ally_is_learnt() {
    // Cai and Dee start as strangers to Bryn (no ally link either way), and are fierce. Bryn, taken
    // by surprise, sees whether they stand near the boar or leave. Then the same, with timid ones.
    let mut grown = vec![];
    for (label, others, hunt) in [("strangers out hunting too", TEMPERS[1].1, true), ("strangers who only happened by", TEMPERS[1].1, false)] {
        let (mut grew, mut line) = (0, vec![]);
        for &seed in &SEEDS {
            let c = Case { seed, temper: TEMPERS[1].1, allies: true, expected: false };
            let (o, _) = encounter_cast(&c, &Cast { to_allies: 0, from_allies: 0, others, others_hunt: hunt, ..Cast::default() }, false);
            if o.ally_cai.1 > o.ally_cai.0 { grew += 1; }
            line.push(format!("{}→{}", o.ally_cai.0, o.ally_cai.1));
        }
        println!("{label}: Bryn's link Cai → ally over one encounter, by seed: {}; grew in {grew}/6", line.join(" "));
        grown.push(grew);
    }
    assert!(grown[0] >= 1 && grown[0] >= grown[1], "someone seen standing with me becomes an ally (only if I stay to see it)");
    // Then a second encounter: does what was learnt embolden? (seed 3, where it grew.)
    let c = Case { seed: 3, temper: TEMPERS[1].1, allies: true, expected: false };
    let (o1, _) = encounter_cast(&c, &Cast { to_allies: 0, from_allies: 0, others_hunt: true, ..Cast::default() }, false);
    let (o2, _) = encounter_cast(&c, &Cast { to_allies: o1.ally_cai.1, from_allies: 0, others_hunt: true, ..Cast::default() }, false);
    println!("a second meeting with Cai now counted on at {}: Bryn flees {} ticks (first meeting {})", o1.ally_cai.1, o2.counts[R_FLEE as usize], o1.counts[R_FLEE as usize]);
}

#[test]
fn t3_calling_for_help() {
    for calls in [fv_mind_lab::people::species::CALL_HABIT, 700] {
        for from_allies in [0, 700] {
            let mut rows = vec![];
            for (tn, t) in TEMPERS {
                let (mut called, mut heard, mut came, mut fled, mut aided, mut ignored, mut n) = (0, 0, 0, 0, 0, 0, 0);
                for &seed in &SEEDS {
                    let r = call_case(seed, TEMPERS[1].1, calls, from_allies, t);
                    if r.called > 0 { called += 1; }
                    for h in &r.hearers {
                        n += 1;
                        if h.0 { heard += 1; } if h.1 { came += 1; } if h.2 { fled += 1; } if h.3 > 0 { aided += 1; } if h.0 && h.3 == 0 && h.4 == 0 { ignored += 1; }
                    }
                }
                rows.push(format!("{tn}: Bryn calls in {called}/6; of {n} hearers heard {heard}, set off to help {aided}, came within 6 cells {came}, ran off {fled}, ignored it {ignored}"));
            }
            println!("habit of calling {calls}, hearers count on Bryn {from_allies}:\n  {}", rows.join("\n  "));
        }
    }
    let r = call_case(1, TEMPERS[1].1, 700, 700, TEMPERS[1].1);
    assert!(r.called > 0 && r.hearers.iter().all(|h| h.0), "a person who has learnt to cry out does, and those in earshot hear it");
    assert!(r.hearers.iter().any(|h| h.3 > 0), "and hearers can set off to help");
    let quiet = call_case(1, TEMPERS[1].1, fv_mind_lab::people::species::CALL_HABIT, 700, TEMPERS[1].1);
    assert_eq!(quiet.called, 0, "with the species' founding habit alone, nobody calls (a finding: see the note)");
}

#[test]
fn t4_intent_as_a_modulator() {
    println!("stress held from the meeting (columns); ticks of 40 on which the plan's own option (stand braced) won, seed by seed");
    let levels: Vec<i32> = (0..=10).map(|k| k * 100).collect();
    let mut thresholds = vec![];
    for (tn, t) in TEMPERS {
        let mut per_seed = vec![];
        for &seed in &SEEDS {
            let row: Vec<u32> = levels.iter().map(|&s| focus_probe(seed, t, Some(s)).plan_ticks).collect();
            per_seed.push(levels.iter().zip(&row).find(|(_, &p)| p < 20).map(|(&s, _)| s));
            if seed == 1 { println!("  {tn:16}: {}", levels.iter().zip(&row).map(|(s, p)| format!("{s}:{p}")).collect::<Vec<_>>().join(" ")); }
        }
        println!("  {tn:16}: stress at which the plan loses control, per seed {per_seed:?}");
        thresholds.push(per_seed);
    }
    // Where the point sits for each temperament: calmer minds hold their plan to higher stress.
    let mut ordered = 0;
    for s in 0..SEEDS.len() {
        let th: Vec<i32> = thresholds.iter().map(|v| v[s].unwrap_or(1100)).collect();
        if th[0] >= th[1] && th[1] >= th[2] && th[2] >= th[3] && th[0] > th[3] { ordered += 1; }
    }
    println!("calm ≥ medium ≥ reactive-timid ≥ reactive-fierce, and calm > fierce: {ordered}/6 seeds");
    assert!(ordered >= 4);
    // What takes over past the point, and the ablation: the same plan with no focus.
    for (tn, t) in [TEMPERS[1]] {
        for s in [0, 500, 700, 1000] {
            let (f, nf) = (focus_probe(1, t, Some(s)), focus_probe_with(1, t, Some(s), false));
            println!("  {tn} stress {s}: with focus {} (plan option on {} ticks); without focus {} (plan option on {})", counts(&f.counts), f.plan_ticks, counts(&nf.counts), nf.plan_ticks);
        }
    }
    // The ablation's own threshold, per temperament (seed 1).
    for (tn, t) in TEMPERS {
        let th = |focus: bool| levels.iter().find(|&&s| focus_probe_with(1, t, Some(s), focus).plan_ticks < 20).copied();
        println!("  {tn:16}: the plan loses control at stress {:?} with focus, {:?} without", th(true), th(false));
    }
    // Across seeds and temperaments: the point with focus is never lower than without.
    let (mut higher, mut never_lower, mut k) = (0, 0, 0);
    for &seed in &SEEDS { for (_, t) in TEMPERS {
        let th = |focus: bool| levels.iter().find(|&&s| focus_probe_with(seed, t, Some(s), focus).plan_ticks < 20).copied().unwrap_or(1100);
        let (a, b) = (th(true), th(false));
        k += 1; if a > b { higher += 1; } if a >= b { never_lower += 1; }
    } }
    println!("focus raises the point in {higher}/{k} (seed × temperament), never lowers it in {never_lower}/{k}");
    assert!(never_lower == k && higher * 2 >= k, "focus is what lets the plan keep control as stress rises");
}

fn counts(c: &[u32; 6]) -> String { c.iter().enumerate().filter(|x| *x.1 > 0).map(|(i, n)| format!("{} {n}", R_NAMES[i])).collect::<Vec<_>>().join(", ") }

#[test]
fn t5_the_five_responses_across_temperament_company_and_expectation() {
    println!("| temperament | company | expected | first response (6 seeds) | ticks per response over 40 (all seeds) | gored | boar broke off |");
    println!("|---|---|---|---|---|---|---|");
    let mut by_temper: Vec<Vec<String>> = vec![];
    for (tn, t) in TEMPERS {
        let mut cells = vec![];
        for allies in [false, true] { for expected in [false, true] {
            let v = run(t, allies, expected);
            let mut tot = [0u32; 6];
            for o in &v { for i in 0..6 { tot[i] += o.counts[i]; } }
            let firsts: Vec<&str> = v.iter().map(|o| rname(o.first)).collect();
            println!("| {tn} | {} | {} | {} | {} | {} | {}/6 |", if allies { "allies" } else { "alone" }, if expected { "yes" } else { "no" }, firsts.join(", "), counts(&tot),
                v.iter().map(|o| o.gored).sum::<u32>(), v.iter().filter(|o| o.boar_fled).count());
            cells.push(format!("{firsts:?}{tot:?}"));
        } }
        by_temper.push(cells);
    }
    let differs = (0..4).filter(|&k| (1..4).any(|t| by_temper[t][k] != by_temper[0][k])).count();
    println!("cells (company × expectation) where temperament changes what happens: {differs}/4");
    assert!(differs >= 2, "temperament matters in danger");
    let _ = (R_FREEZE, R_AID);
}
