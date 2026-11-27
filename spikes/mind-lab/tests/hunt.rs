//! Stage 4: the hunt and the betrayal. Each test prints its numbers (run with --nocapture).
//! Sampling: 12 seeds (each a different valley: camp, landmarks, the deer's range and a rock near
//! it, starts, and everyone's warmth and trust towards the others) × 3 temperament sets.
//! `cargo test --release --test hunt -- --nocapture --test-threads=4`

use fv_mind_lab::hunt::runs::*;
use fv_mind_lab::people::talk::Role;
use fv_mind_lab::people::world::Event;

const SEEDS: std::ops::RangeInclusive<u64> = 1..=12;
const N: &[&str] = &["Ash", "Bryn", "Cai", "Dee"];

fn sig(o: &Outcome) -> String { format!("{:?}/{}/{}/{}/{}", o.caught, o.joined, o.declined, o.silent, o.deer_ran) }

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
    assert!(d.len() >= 9, "seeds should give mostly different hunts (stages 1 to 3 found they barely did)");
}

#[test]
fn planned_hunt_against_the_same_people_each_alone() {
    println!("{:<8} {:<9} | {:>6} {:>10} {:>8} {:>9} | per seed (tick/catcher, - not caught)", "mode", "set", "caught", "mean tick", "effort", "deer ran");
    let mut total = [0u32; 2];
    let mut effort = [0u32; 2];
    let mut both = (0, 0, 0, 0);
    for set in 0..3 {
        let mut per_mode = vec![];
        for (mi, mode) in [Mode::Planned, Mode::Solo].into_iter().enumerate() {
            let (mut n, mut ticks, mut eff, mut ran, mut line) = (0u32, 0u32, 0u32, 0u32, String::new());
            let mut res = vec![];
            for seed in SEEDS {
                let (o, _) = hunt(&Case { seed, set, mode });
                if let Some((t, w)) = o.caught { n += 1; ticks += t; line += &format!("{t}/{} ", &N[w][..1]); } else { line += "- "; }
                eff += o.effort; ran += o.deer_ran;
                res.push(o.caught.is_some());
                if mode == Mode::Solo { assert_eq!(o.proposed, None); assert_eq!(o.joined + o.declined + o.silent, 0, "nobody talks when each hunts alone"); }
            }
            total[mi] += n; effort[mi] += eff;
            per_mode.push(res);
            println!("{:<8} {:<9} | {:>3}/12 {:>10} {:>8} {:>9} | {line}", format!("{mode:?}"), SETS[set], n, if n > 0 { ticks / n } else { 0 }, eff / 12, ran / 12);
        }
        for k in 0..12 { match (per_mode[0][k], per_mode[1][k]) { (true, true) => both.0 += 1, (true, false) => both.1 += 1, (false, true) => both.2 += 1, _ => both.3 += 1 } }
    }
    println!("over 36 runs: planned {} caught, alone {} caught; both {}, only planned {}, only alone {}, neither {}", total[0], total[1], both.0, both.1, both.2, both.3);
    println!("effort over 36 runs: planned {} cells, alone {} cells", effort[0], effort[1]);
    // The claims, as measured when stage 4 was written (catches 15 against 12: a small edge, not
    // significant over 36 paired runs; effort about 30% lower).
    assert!(total[0] >= total[1], "a planned hunt catches the deer no less often than the same people each alone");
    assert!(effort[0] * 10 < effort[1] * 9, "a planned hunt costs at least 10% less walking");
}

#[test]
fn each_hearer_decides_for_itself() {
    // Every decision on a proposal, with the hearer's warmth and trust towards the proposer (Ash).
    let (mut rows, mut by_warmth) = (vec![], std::collections::BTreeMap::<i32, (u32, u32)>::new());
    let mut roles = [0u32; 2];
    for seed in SEEDS { for set in 0..3 {
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
    for seed in SEEDS { for set in 0..3 {
        let (o, v) = hunt(&Case { seed, set, mode: Mode::Planned });
        for k in 0..3 { kinds[k] += o.breaches[k]; } for k in 0..4 { react[k] += o.reactions[k]; }
        if o.signal.is_some() { signals += 1; }
        replans += o.replans;
        if v.folk[0].hunt.as_ref().unwrap().log.events.iter().any(|e| e.1.contains("moved early")) { early += 1; }
    } }
    println!("36 planned hunts: breaches noticed (all minds) out of place {}, too fast {}, missing {}", kinds[0], kinds[1], kinds[2]);
    println!("  what followed within 15 ticks: kept the plan {}, re-planned out loud {}, abandoned {}, improvised {}", react[0], react[1], react[2], react[3]);
    println!("  signals given {signals}/36, of which on the deer moving early {early}; re-plans after a decline or a missing member {replans}");
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
            for (_, v, _) in &o.verdicts { match (v, o.teller) { (Some(x), Some(t)) if *x == t => right += 1, (Some(_), _) => wrong += 1, _ => und += 1 } }
        } }
        wrong_total += wrong;
        println!("{:<10} {:>4} | {teller:>2} ({right_teller}) {warned:>7} {counter:>8} {joined:>7} | {dh:>9} {gh:>9} | {right}/{wrong}/{und}; overheard {over}", format!("{lean:?}"), caution);
        if matches!(lean, Lean::Cai | Lean::Bryn) { assert_eq!(right_teller, 24, "the one who leans to Dee is the one who tells, in every run"); }
        if lean == Lean::Nobody { assert_eq!(teller, 0); }
    } }
    assert_eq!(wrong_total, 0, "nobody innocent is blamed");
}
