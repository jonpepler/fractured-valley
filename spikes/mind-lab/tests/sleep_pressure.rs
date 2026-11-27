//! Regression, ported from `spikes/thought-graph/tests/sleep_pressure.rs` onto the shared mind.
//! The owner's consolidation model (27 Nov): idle brain time is consolidation time, whatever the
//! body is doing; skipping sleep doesn't escape it; the backlog of unconsolidated memory *is*
//! tiredness, and a full backlog makes thinking measurably worse. Every test prints its numbers:
//! `cargo test --release --test sleep_pressure -- --nocapture --test-threads=1`.
//! The "slow graph alone" is the mind's expanded form with its fast weights zeroed.

use fv_mind_lab::fox::scenarios::{self, DAY, DAY2, NIGHT, SEED};
use fv_mind_lab::fox::sim::{valley_fox, Sim};
use fv_mind_lab::graph::lt;
use fv_mind_lab::mind::{Action, Mind, Temperament, BACKLOG_CAP, THINK_BUDGET};
use fv_mind_lab::store::{Expanded, Store};
use fv_mind_lab::think::{self as spread, Activation, Form, Mods};
use std::time::Instant;

fn imagine_henhouse(m: &Mind) -> i32 { let hh = m.node("henhouse"); m.imagine(&[(hh, 1000)], &Mods::default(), 3).1[m.roles.danger as usize] }

fn hh_danger(m: &Mind) -> (i16, i16) {
    let l = m.link_of(m.node("henhouse"), m.roles.danger, lt::FEELS).unwrap();
    (m.store.slow(l), m.store.fast(l))
}

/// The mind's slow graph alone: its expanded form with every fast weight zeroed.
fn slow_only(m: &Mind) -> Expanded { let mut e = m.store.expand(); e.fast.iter_mut().for_each(|f| *f = 0); e }

/// Cued recall, as the mind is now (its fast weights and its own fatigue), against what its slow
/// graph alone holds when rested. For each of the six founding places as a cue: three thinks
/// from the cue; the workspace is compared with the rested, slow-only workspace from the same cue.
/// Returns (links visited per think, ns per think, workspace overlap ‰, mean activation reaching
/// the workspace, links visited per think by the same slow graph rested).
fn recall(m: &Mind) -> (u32, u128, i64, i64, u32) {
    let slow = slow_only(m);
    let now = Mods { fatigue: m.mods.fatigue, ..Mods::default() };
    let (mut visits, mut thinks, mut overlap, mut mass, mut ref_visits) = (0u32, 0u32, 0i64, 0i64, 0u32);
    let t0 = Instant::now();
    for pi in 0..6 {
        let cue = m.place_nodes[pi];
        let mut a = Activation::new(m.store.len());
        let mut b = Activation::new(m.store.len());
        let (mut wa, mut wb) = (vec![], vec![]);
        for _ in 0..3 {
            wa = spread::think(&m.store, &mut a, &now, &[(cue, 1000)], &m.params, None);
            visits += a.visits; thinks += 1;
            wb = spread::think(&slow, &mut b, &Mods::default(), &[(cue, 1000)], &m.params, None);
            ref_visits += b.visits;
        }
        let common = wa.iter().filter(|n| wb.contains(n)).count() as i64;
        overlap += common * 1000 / wb.len().max(1) as i64;
        mass += wa.iter().map(|&n| a.act[n as usize] as i64).sum::<i64>();
    }
    let ns = t0.elapsed().as_nanos() / (thinks as u128 * 2);
    (visits / thinks, ns, overlap / 6, mass / 6, ref_visits / thinks)
}

/// The dog charges a resting fox (as in the temperament test), with a given mind.
fn charge(mut m: Mind) -> (u32, u32, u32, Option<u32>, i32, u32) {
    let mut s = scenarios::dog_charges(SEED, 0);
    m.st.clear(); m.goal = None; m.flee_left = 0; m.mods.arousal = 0; m.mods.stress = 0; m.napping = false; m.puzzle = None;
    m.temperament = Temperament::MEDIUM;
    s.m = m;
    s.run(scenarios::CHARGE_AT);
    let (mut hops, mut snaps, mut flee, mut first, mut peak) = (0, 0, 0, None, 0);
    for k in 0..16 {
        let a = s.step();
        hops += s.m.last.hops_run;
        peak = peak.max(s.m.mods.stress);
        if first.is_none() && matches!(a, Action::Flee { .. } | Action::Snap { .. }) { first = Some(k); }
        if matches!(a, Action::Snap { .. }) { snaps += 1; }
        if matches!(a, Action::Flee { .. }) { flee += 1; }
    }
    let bites = s.w.log.iter().filter(|l| l.1.contains("bites")).count() as u32;
    (hops * 10 / 16, snaps, flee, first, peak, bites)
}

/// First choice from the den at hunger 700, given a mind (12 ticks, as in outcome 3).
fn morning_choice(mut m: Mind) -> String {
    let mut s = Sim::new(SEED);
    s.w.foxes[0].hunger = 700;
    m.st.clear(); m.goal = None; m.flee_left = 0; m.mods.arousal = 0; m.mods.stress = 0; m.napping = false; m.puzzle = None;
    s.m = m;
    let mut last = "stay".to_string();
    for _ in 0..12 {
        match s.step() {
            Action::WalkTo { .. } => last = s.m.goal.map(|g| s.m.name(g)).unwrap_or("?".into()),
            Action::Stay => last = "stay".into(),
            Action::Sleep => last = "sleep".into(),
            _ => {}
        }
    }
    last
}

// ---------------------------------------------------------------- (a)

#[test]
fn a_skipping_sleep_does_not_skip_consolidation() {
    // Bitten on day 1, then a night lying awake. Before, the fear simply faded by morning.
    let mut s = scenarios::dog_day_no_sleep(SEED);
    s.run(NIGHT);
    let evening = (hh_danger(&s.m), s.m.consol);
    s.run(DAY2 - NIGHT);
    let morning = hh_danger(&s.m);
    let fear = imagine_henhouse(&s.m);
    // Ablation: the same fox with idle consolidation turned off holds the fear only as fast trace.
    let mut x = scenarios::dog_day_no_sleep(SEED);
    x.m.idle_consolidation = false;
    x.run(DAY2);
    let x_m = hh_danger(&x.m);
    let x_fear = imagine_henhouse(&x.m);
    let x_fast_only = { let m = x.m.clone(); let e = slow_only(&m); let mm = m.map_store(e); let hh = mm.node("henhouse"); mm.imagine(&[(hh, 1000)], &Mods::default(), 3).1[mm.roles.danger as usize] };
    let mut c = scenarios::no_dog_day(SEED);
    c.run(DAY2);
    let c_fear = imagine_henhouse(&c.m);
    let founding = imagine_henhouse(&valley_fox().mind(Temperament::MEDIUM, SEED));
    println!("sleepless fox, henhouse→danger (slow, fast): nightfall {:?}, next morning {:?}", evening.0, morning);
    println!("  consolidated by idle time on day 1: {} settled ticks, {} episode replays, {} fast weight moved",
        evening.1.idle_ticks, evening.1.replays, evening.1.moved);
    println!("  danger from thinking of the henhouse next morning: {fear} (never met the dog: {c_fear})");
    println!("ablation, idle consolidation off: henhouse→danger {:?}; danger {x_fear}, of which {x_fast_only} from the slow graph alone", x_m);
    assert!(evening.0 .0 > 100, "the bite reached the slow graph during the waking day");
    assert!(morning.0 >= evening.0 .0);
    assert!(fear * 10 > c_fear * 15, "the next day the henhouse still brings danger to mind");
    assert_eq!(x_m.0, 0, "without idle consolidation nothing reaches the slow graph by day");
    println!("  (the founding mind's henhouse brings danger {founding})");
    assert!(x_fast_only <= founding, "and the fear would be held only by fast weights");
}

// ---------------------------------------------------------------- (b), (c)

#[test]
fn b_c_backlog_grows_without_sleep_degrades_thinking_and_sleep_restores_it() {
    // Quiet days first: the backlog grows night by night (waking potentiation outpaces idle
    // consolidation, including the hours lying awake), slowly, as a monotonous life adds little.
    let mut q = scenarios::sleepless_days(SEED, 4);
    let mut plateau = vec![];
    for d in 0..4 { while q.w.tick < d * DAY + NIGHT - 1 { q.step(); } plateau.push((q.m.backlog(), q.m.mods.fatigue)); }
    println!("a sleepless fox over 4 quiet days: backlog and fatigue at each nightfall {:?} ", plateau);
    // Stage 2 changed day learning to round stochastically (calm learning of dim thoughts no
    // longer truncates to zero). The backlog now fills faster and levels off by the fourth night
    // (where learning and the fast weights' own fade balance), instead of still climbing: so the
    // claim is "it grows, and never falls back by more than 1%", not "it grows every night".
    assert!(plateau.windows(2).all(|w| w[1].0 * 100 > w[0].0 * 99) && plateau[2].0 > plateau[0].0 && plateau[3].1 > plateau[0].1, "without sleep the backlog grows, and fatigue with it");

    let days = 4;
    let mut s = scenarios::sleepless_eventful_days(SEED, days);
    let rested = s.m.clone();
    println!("a sleepless fox over {days} eventful days (a fire and a dog chase each afternoon; capacity {BACKLOG_CAP}):");
    println!("  day  tick  backlog  fatigue  lost-to-compaction  visits/think (slow graph rested)  ns/think  recall-overlap  recall-mass");
    let r0 = recall(&rested);
    println!("  rested       {:6}  {:7}  {:18}  {:12} ({:3})  {:20}  {:13}‰  {:11}", 0, 0, 0, r0.0, r0.4, r0.1, r0.2, r0.3);
    let mut rows = vec![];
    let mut tired = None;
    for d in 0..days {
        for &k in &[150u32, NIGHT - 1] {
            while s.w.tick < d * DAY + k { s.step(); }
            let r = recall(&s.m);
            println!("  {:3}  {:4}  {:7}  {:7}  {:18}  {:12} ({:3})  {:20}  {:13}‰  {:11}", d + 1, k, s.m.backlog(), s.m.mods.fatigue, s.m.consol.lost, r.0, r.4, r.1, r.2, r.3);
            rows.push((s.m.backlog(), s.m.mods.fatigue, r));
        }
        if d == 2 { tired = Some(s.m.clone()); }
    }
    let tired = tired.unwrap();
    // An eventful life fills it far faster: most of the capacity on the first day, and it stays high.
    // Restated in round 2: round 1's first eventful day ended at fatigue 852; now 669 (round 2's
    // rules and new defaults together; the cause is not separated). It still stays above 600.
    assert!(rows[1].1 >= 600 && rows.iter().all(|r| r.1 >= 600), "eventful sleepless days keep fatigue high");
    let rt = recall(&tired);
    // Not more expensive: fatigue damps spreading, so a tired think visits *fewer* links than its
    // own slow graph would rested. Tired thought is narrower and dimmer, and less faithful.
    println!("tired think: {} links visited against {} for its own slow graph rested (narrower, not costlier)", rt.0, rt.4);
    assert!(rt.0 < rt.4);
    assert!(rt.2 < r0.2 && rt.3 < r0.3, "and recall less faithful to what the slow graph holds, and fainter");

    // Decisions: the same mind rested and three sleepless days in.
    let (cr, ct) = (morning_choice(rested.clone()), morning_choice(tired.clone()));
    println!("first choice from the den at hunger 700: rested {cr}, after 3 sleepless days {ct}");
    let (dr, dt) = (charge(rested.clone()), charge(tired.clone()));
    for (n, d) in [("rested", dr), ("tired", dt)] {
        println!("the dog charges, {n:6}: mean hops {}.{}, snaps {}, flee ticks {}, first reaction after {:?}, peak stress {}, bitten {}", d.0 / 10, d.0 % 10, d.1, d.2, d.3, d.4, d.5);
    }
    assert!(dt.0 < dr.0 || dt.1 > dr.1 || dt.4 > dr.4, "a tired fox meets the dog differently");

    // (c) One night's sleep for the tired fox.
    let mut slept = tired.clone();
    let before = (slept.backlog(), slept.mods.fatigue);
    slept.skip_sleep = false;
    slept.sleep();
    let rs = recall(&slept);
    println!("one night's sleep: backlog {} → {}, fatigue {} → {}; visits/think {} (its slow graph rested: {}) → {} ({}), recall overlap {}‰ → {}‰ (rested {}‰), mass {} → {}",
        before.0, slept.backlog(), before.1, slept.mods.fatigue, rt.0, rt.4, rs.0, rs.4, rt.2, rs.2, r0.2, rt.3, rs.3);
    println!("  (the slow graph itself grew: consolidation filled empty slots, so a rested think now visits {} links, not {})", rs.4, r0.4);
    println!("  after sleep: first choice {}, dog charge {:?}", morning_choice(slept.clone()), charge(slept.clone()));
    assert_eq!(slept.backlog(), 0, "a night's sleep clears the backlog");
    assert_eq!(slept.mods.fatigue, 0);
    assert!(rs.0 == rs.4 && rs.2 == 1000 && rs.3 > rt.3, "and thinking is restored: full breadth, faithful, full-strength recall");
}

// ---------------------------------------------------------------- (d)

#[test]
fn d_a_busy_day_builds_more_pressure_than_a_quiet_one() {
    let run = |mut s: Sim| {
        let (mut peak, mut naps, mut spent, mut settled) = (0, 0, 0i64, 0);
        for _ in 0..NIGHT {
            s.step();
            peak = peak.max(s.m.mods.fatigue);
            if s.m.last.why.starts_with("napping") { naps += 1; }
            spent += s.m.last.consolidated;
            if s.m.last.consolidated > 0 { settled += 1; }
        }
        (s.m.backlog(), peak, naps, spent, settled, s.m.consol.replays, s.m.consol.lost)
    };
    let quiet = run(scenarios::no_dog_day(SEED));
    let busy = run(scenarios::busy_day(SEED));
    let mut q2 = scenarios::no_dog_day(SEED); q2.m.skip_sleep = true;
    let mut b2 = scenarios::busy_day(SEED); b2.m.skip_sleep = true;
    let (quiet_awake, busy_awake) = (run(q2), run(b2));
    for (n, r) in [("quiet", quiet), ("busy (bite, fire, second chase)", busy), ("quiet, no naps", quiet_awake), ("busy, no naps", busy_awake)] {
        println!("{n:32}: backlog at nightfall {:6}, peak fatigue {:4}, ticks napping {:3}, consolidation spent {:7} over {:3} ticks, replays {:3}, lost {}",
            r.0, r.1, r.2, r.3, r.4, r.5, r.6);
    }
    assert!(busy.1 > quiet.1, "the eventful day peaks higher in sleep pressure");
    assert!(busy_awake.0 > quiet_awake.0, "and, with no chance to nap, ends with a bigger backlog");
    assert!(busy.2 > quiet.2, "the busy day's fox needed to nap");
}

// ---------------------------------------------------------------- idle compute

#[test]
fn idle_consolidation_uses_spare_budget() {
    let mut s = scenarios::sleepless_days(SEED, 2);
    let (mut units, mut ticks, mut visits, mut n) = (0i64, 0i64, 0i64, 0i64);
    for _ in 0..DAY + 150 {
        s.step();
        let r = &s.m.last;
        visits += r.visits as i64; n += 1;
        if r.consolidated > 0 { units += r.consolidated; ticks += 1; }
    }
    // Wall time of one idle consolidation against one think, on the same mind.
    let m = s.m.clone();
    let iters = 2000;
    let t0 = Instant::now();
    for _ in 0..iters { std::hint::black_box(m.clone()); }
    let clone_ns = t0.elapsed().as_nanos() / iters;
    let t0 = Instant::now();
    for _ in 0..iters { let mut c = m.clone(); std::hint::black_box(c.consolidate(THINK_BUDGET - visits / n, false)); }
    let cons_ns = (t0.elapsed().as_nanos() / iters).saturating_sub(clone_ns);
    let mut st = Activation::new(m.store.len());
    let inj = [(m.roles.hunger, 400), (m.node("den"), 300)];
    let t0 = Instant::now();
    for _ in 0..iters { std::hint::black_box(spread::think(&m.store, &mut st, &m.mods, &inj, &m.params, None)); }
    let think_ns = t0.elapsed().as_nanos() / iters;
    println!("budget {THINK_BUDGET} link visits a tick; a think visits {} on average", visits / n);
    println!("idle consolidation ran on {ticks} of {n} waking ticks, spending {} units a tick when it ran (≤ the {} left over)", units / ticks.max(1), THINK_BUDGET - visits / n);
    println!("wall time: one idle consolidation {cons_ns} ns, one think {think_ns} ns (backlog {})", m.backlog());
    assert!(units / ticks.max(1) <= THINK_BUDGET);
}

// ---------------------------------------------------------------- (e)

#[test]
fn e_consolidation_and_home_runs_are_deterministic() {
    let a = || { let mut s = scenarios::sleepless_days(SEED, 2); s.run(2 * DAY); s.state_hash() };
    let b = || { let mut s = scenarios::busy_day(SEED); s.run(DAY + 50); s.state_hash() };
    let c = || { let mut s = scenarios::home_moved(SEED, scenarios::HomeMove::Daily, 4); s.run(4 * DAY); s.state_hash() };
    let (a1, a2, b1, b2, c1, c2) = (a(), a(), b(), b(), c(), c());
    println!("sleepless 2 days {a1:#018x} {a2:#018x}; busy day {b1:#018x} {b2:#018x}; den moved daily, 4 days {c1:#018x} {c2:#018x}");
    assert_eq!((a1, b1, c1), (a2, b2, c2));
    let par: Vec<u64> = std::thread::scope(|sc| {
        let h = [sc.spawn(c), sc.spawn(b), sc.spawn(a)];
        h.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(par, vec![c1, b1, a1], "the same on threads");
}
