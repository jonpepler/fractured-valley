//! Stage 4: the hunt and the betrayal. Each test prints its numbers (run with --nocapture).
//! Sampling: 12 seeds (each a different valley: camp, landmarks, the deer's range and a rock near
//! it, starts, and everyone's warmth and trust towards the others) × 3 temperament sets.
//! `cargo test --release --test hunt -- --nocapture --test-threads=4`

use fv_mind_lab::hunt::runs::*;
use fv_mind_lab::people::talk::Role;
use fv_mind_lab::people::world::Event;

const SEEDS: std::ops::RangeInclusive<u64> = 1..=12;

/// (Review: a signature with the deer's running ticks in it was too fine to mean much.) Who caught
/// it, and who joined, declined or said nothing.
fn sig(o: &Outcome) -> String { format!("{:?}/{}/{}/{}", o.caught.map(|c| c.1), o.joined, o.declined, o.silent) }

#[test]
fn seeds_make_different_valleys_and_different_hunts() {
    let mut sigs = vec![];
    for seed in SEEDS {
        let l = layout(seed);
        let (o, _) = hunt(&Case { seed, set: 1, mode: Mode::Planned });
        println!("seed {seed:>2}: camp {:?} rock {:?} meadows {:?} starts {:?} | caught {:?}, joined {} declined {} silent {}, deer ran {} ticks", l.camp, l.rock, l.meadows, l.starts, o.caught, o.joined, o.declined, o.silent, o.deer_ran);
        sigs.push(sig(&o));
    }
    let mut d = sigs.clone(); d.sort(); d.dedup();
    println!("distinct outcomes over {} seeds: {}", sigs.len(), d.len());
    assert!(d.len() >= 6, "seeds should give mostly different hunts (stages 1 to 3 found they barely did)");
}

/// (Review) 12 seeds were too few: over 200 the comparison reversed. Temperament sets give identical
/// planned hunts (roles take the body), so the sample is seeds, one set each.
#[test]
fn planned_hunt_against_the_same_people_each_alone() {
    let n = 200u64;
    let (mut c, mut both, mut op, mut oa, mut eff, mut ran, mut ticks) = ([0u32; 2], 0, 0, 0, [0u64; 2], [0u64; 2], [0u64; 2]);
    for seed in 1..=n {
        let (p, _) = hunt(&Case { seed, set: 1, mode: Mode::Planned });
        let (a, _) = hunt(&Case { seed, set: 1, mode: Mode::Solo });
        assert_eq!(a.proposed, None); assert_eq!(a.joined + a.declined + a.silent, 0, "nobody talks when each hunts alone");
        for (k, o) in [&p, &a].into_iter().enumerate() { if let Some((t, _)) = o.caught { c[k] += 1; ticks[k] += t as u64; } eff[k] += o.effort as u64; ran[k] += o.deer_ran as u64; }
        match (p.caught.is_some(), a.caught.is_some()) { (true, true) => both += 1, (true, false) => op += 1, (false, true) => oa += 1, _ => {} }
    }
    println!("{n} valleys (seeded temperaments): caught planned {} / alone {}; both {both}, only planned {op}, only alone {oa}", c[0], c[1]);
    println!("  mean tick of a catch {} / {}; cells walked a hunt {} / {}; per catch {} / {}; ticks the deer ran a hunt {} / {}",
        ticks[0] / c[0].max(1) as u64, ticks[1] / c[1].max(1) as u64, eff[0] / n, eff[1] / n, eff[0] / c[0].max(1) as u64, eff[1] / c[1].max(1) as u64, ran[0] / n, ran[1] / n);
    // Sign test on the discordant pairs (exact, two-sided), in per mille.
    let (k, m) = (op.min(oa) as u64, (op + oa) as u64);
    let mut tail = 0f64; let mut b = 1f64; for i in 0..=m { if i <= k { tail += b; } b = b * (m - i) as f64 / (i + 1) as f64; }
    let pval = (2.0 * tail / 2f64.powi(m as i32)).min(1.0);
    println!("  sign test on {m} discordant valleys: p = {pval:.3}");
    // Temperament sets, seeds 1-12: are planned hunts identical across sets?
    let same = (1..=12).filter(|&seed| { let o: Vec<_> = (0..3).map(|set| hunt(&Case { seed, set, mode: Mode::Planned }).0.caught).collect(); o[0] == o[1] && o[1] == o[2] }).count();
    println!("  planned hunts identical across the 3 temperament sets in {same}/12 valleys");
    // Ablation: the hiding rule off, 100 valleys.
    let (mut hp, mut ha) = (0, 0);
    for seed in 1..=100 { if hunt_with(&Case { seed, set: 1, mode: Mode::Planned }, None, &[], false).0.caught.is_some() { hp += 1; } if hunt_with(&Case { seed, set: 1, mode: Mode::Solo }, None, &[], false).0.caught.is_some() { ha += 1; } }
    println!("  hiding off, 100 valleys: planned {hp} / alone {ha}");
    // The claim as measured when stage 4 was reviewed: planning does not help catches; it saves walking.
    assert!(c[0] <= c[1], "measured: a planned hunt catches no more often than the same people alone");
    assert!(eff[0] * 10 < eff[1] * 9, "a planned hunt costs at least 10% less walking a hunt");
}

#[test]
fn each_hearer_decides_for_itself() {
    // Every decision on a proposal, with the hearer's warmth and trust towards the proposer (Ash).
    let (mut rows, mut by_warmth) = (vec![], std::collections::BTreeMap::<i32, (u32, u32)>::new());
    let mut roles = [0u32; 2];
    // (Review: the three temperament sets gave the same decisions, so one set; 40 valleys.)
    for seed in 1..=40 { for set in [1] {
        let (o, v) = hunt(&Case { seed, set, mode: Mode::Planned });
        let l = layout(seed);
        for p in &v.folk {
            let h = p.hunt.as_ref().unwrap();
            for d in &h.log.decisions {
                let e = by_warmth.entry(l.warmth[p.i][0]).or_default();
                if d.2 { e.0 += 1 } else { e.1 += 1 }
                rows.push((p.i, d.2, d.3, d.4));
            }
        }
        for (_, r) in &o.roles { roles[match r { Role::Drive => 0, Role::Wait => 1 }] += 1; }
    } }
    let (j, a, dcl) = (rows.iter().filter(|r| r.1).count(), rows.iter().filter(|r| !r.1 && r.3 == 1).count(), rows.iter().filter(|r| r.3 == 2).count());
    println!("decisions heard: {} (joined {j}; of those not joining, declined out loud {dcl}, lied 'I'm in' {a}, said nothing {})", rows.len(), rows.len() - j - dcl - a);
    for (w, (y, n)) in &by_warmth { println!("  warmth towards Ash {w:>4}: joined {y}, did not {n}"); }
    println!("roles held at the end (members who joined, and Ash): drive {}, wait {}", roles[0], roles[1]);
    assert!(j > 0 && rows.len() > j, "some join and some don't");
    let rate = |w: i32| by_warmth.get(&w).map(|x| x.0 * 1000 / (x.0 + x.1).max(1)).unwrap_or(0);
    assert!(rate(600) > rate(-300), "warmer hearers join more often (the decision reads the hearer's own feeling for the proposer)");
}

#[test]
fn breaches_of_the_plan_and_what_follows() {
    let (mut kinds, mut react) = ([0u32; 3], [0u32; 4]);
    let (mut signals, mut early, mut replans) = (0, 0, 0);
    for seed in 1..=40 { for set in [1] {
        let (o, v) = hunt(&Case { seed, set, mode: Mode::Planned });
        for k in 0..3 { kinds[k] += o.breaches[k]; } for k in 0..4 { react[k] += o.reactions[k]; }
        if o.signal.is_some() { signals += 1; }
        replans += o.replans;
        if v.folk[0].hunt.as_ref().unwrap().log.events.iter().any(|e| e.1.contains("moved early")) { early += 1; }
    } }
    println!("40 planned hunts: breaches noticed (all minds) out of place {}, too fast {}, missing {}", kinds[0], kinds[1], kinds[2]);
    println!("  what followed within 15 ticks: kept the plan {}, re-planned out loud {}, abandoned {}, improvised {}", react[0], react[1], react[2], react[3]);
    println!("  signals given {signals}/40, of which on the deer moving early {early}; re-plans after a decline or a missing member {replans}");
    // A staged breach: Bryn made very thirsty at tick 12 (its own mind then weighs water against the plan).
    let (mut held, mut went) = (0, 0);
    for seed in SEEDS {
        let (_, v) = hunt_ev(&Case { seed, set: 1, mode: Mode::Planned }, None, &[(12, Event::SetThirst { who: 1, v: 950 })]);
        let b = &v.folk[1];
        let joined = b.hunt.as_ref().unwrap().log.decisions.iter().any(|d| d.2);
        if !joined { continue; }
        if b.m.held.intent.is_some() && b.hunt.as_ref().unwrap().plan.as_ref().map(|p| p.node) == b.m.held.intent { held += 1 } else { went += 1 }
    }
    println!("staged: Bryn made very thirsty at tick 12, among seeds where it joined: still holds the plan at the end {held}, dropped it {went}");
    assert!(kinds.iter().sum::<u32>() > 0, "plans do get broken");
}

#[test]
fn the_deer_learns_where_it_met_people() {
    let (mut learnt, mut runs) = (0, 0);
    for seed in SEEDS {
        let (o, _) = hunt(&Case { seed, set: 1, mode: Mode::Planned });
        let top = *o.deer_learnt.iter().max().unwrap_or(&0);
        println!("seed {seed:>2}: deer's danger at its places {:?}, ran {} ticks", o.deer_learnt, o.deer_ran);
        runs += 1; if top > 0 { learnt += 1; }
    }
    println!("the deer linked a place of its own to danger in {learnt}/{runs} hunts");
    assert!(learnt > 0);
}

#[test]
fn the_betrayal() {
    println!("{:<10} {:>4} | {:>6} {:>7} {:>8} {:>7} | {:>9} {:>9} | verdicts (right/wrong/undecided)", "who leans", "caut", "teller", "warned", "counter", "joined", "Dee hurt", "grp hurt");
    let mut wrong_total = 0;
    for lean in LEANS { for caution in [10, 2] {
        let (mut teller, mut right_teller, mut warned, mut counter, mut joined, mut dh, mut gh, mut right, mut wrong, mut und, mut over) = (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        for seed in 1..=8 { for set in [0, 1, 2] {
            let (o, _) = betrayal(&BCase { seed, set, lean, caution, honest_teller: false });
            let leaner = match lean { Lean::Nobody => None, Lean::Cai | Lean::CaiSlight => Some(2), Lean::Bryn => Some(1) };
            if o.teller.is_some() { teller += 1; if o.teller == leaner { right_teller += 1; } }
            if o.warned_at.is_some() { warned += 1; }
            if o.counter { counter += 1; } if o.counter_joined { joined += 1; }
            dh += o.dee_struck; gh += o.group_struck; over += o.overheard.len();
            for (_, v, _) in &o.verdicts { match (v, o.whisperer) { (Some(x), Some(t)) if *x == t => right += 1, (Some(_), _) => wrong += 1, _ => und += 1 } }
        } }
        wrong_total += wrong;
        println!("{:<10} {:>4} | {teller:>2} ({right_teller}) {warned:>7} {counter:>8} {joined:>7} | {dh:>9} {gh:>9} | {right}/{wrong}/{und}; overheard {over}", format!("{lean:?}"), caution);
        if matches!(lean, Lean::Cai | Lean::Bryn) { assert_eq!(right_teller, 24, "the one who leans to Dee is the one who tells, in every run"); }
        if lean == Lean::Nobody { assert_eq!(teller, 0); }
    } }
    assert_eq!(wrong_total, 0, "nobody innocent is blamed");
    // (Review) Where telling starts: Cai feels `w` towards Dee, 200 towards Ash, owes Dee nothing.
    let mut line = String::from("Cai's warmth to Dee → tells (of 24):");
    for w in [0, 100, 200, 300, 400, 500, 600] {
        let n = (1..=8).flat_map(|seed| [0, 1, 2].map(move |set| (seed, set))).filter(|&(seed, set)| {
            let c = BCase::new(seed, set, Lean::CaiSlight); betrayal_on(&c, build_betrayal_warmth(seed, set, w), None).0.teller.is_some() }).count();
        line += &format!(" {w}: {n}");
    }
    println!("{line}");
}
