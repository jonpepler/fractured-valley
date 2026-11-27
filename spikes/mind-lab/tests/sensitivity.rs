//! Don't tune to pass: each fox outcome is checked over 4 world seeds × 5 temperaments, for the
//! original fox (the shared mind's legacy settings, proven identical to `spikes/thought-graph`
//! tick for tick in tests/faithful.rs) and for the shared mind. It prints how often each holds.
//! Nothing here asserts a pass rate: the regression tests assert the pinned cases; this reports
//! how far they generalise.
//! Note: in the fox's valley the world seed only seeds search waypoints and where a moved den
//! goes each night, so it moves the home experiments, not the single-day outcomes.
//! `cargo test --release --test sensitivity -- --nocapture --test-threads=1`

use fv_mind_lab::fox::experiments::{home_days_with, DayStats};
use fv_mind_lab::fox::scenarios::{self, HomeMove, DAY2, FIRE_AT, NIGHT};
use fv_mind_lab::fox::sim::{place_node, Sim};
use fv_mind_lab::fox::world::Kind;
use fv_mind_lab::mind::{Action, Temperament};
use fv_mind_lab::think::Mods;

const SEEDS: [u64; 4] = [7, 11, 23, 42];
fn temperaments() -> Vec<Temperament> { let mut v = vec![Temperament::MEDIUM]; for i in [2u32, 9, 18, 26] { v.push(Temperament::seeded(7, i)); } v }

#[derive(Clone, Copy, PartialEq)]
enum Setting { Original, Shared }

/// Give a scenario's sim a mind of temperament `t` (keeping what the scenario set on it), in a setting.
fn prep(mut s: Sim, seed: u64, t: Temperament, st: Setting) -> Sim {
    let old = s.m.clone();
    s.m = if st == Setting::Original { s.fox.mind_exact(t, seed) } else { s.fox.mind(t, seed) };
    s.m.skip_sleep = old.skip_sleep; s.m.pin_arousal = old.pin_arousal; s.m.pin_mood = old.pin_mood; s.m.learn = old.learn; s.m.mods = old.mods;
    if st == Setting::Original { s.m.params.exact = true; s.m.params.index_ties = true; s.m.cfg.stochastic_learning = false; s.m.cfg.comfort_gate = false; s.m.cfg.walk_cost = 0; s.m.cfg.hold_goal = false;
        // (Round 2: the original fox had none of round 2's rules or new defaults.)
        let c = s.m.cfg; s.m.cfg = fv_mind_lab::mind::Config { sleep_keep: c.sleep_keep, own_fade: c.own_fade, stochastic_learning: false, comfort_gate: false, walk_cost: 0, hold_goal: false, ..fv_mind_lab::mind::Config::round1() }; }
    s
}

fn choice(s: &mut Sim, ticks: u32) -> String {
    let mut last = "stay".to_string();
    for _ in 0..ticks { match s.step() { Action::WalkTo { .. } => last = s.m.goal.map(|g| s.m.name(g)).unwrap_or("?".into()), Action::Stay => last = "stay".into(), _ => {} } }
    last
}
fn fear(s: &Sim) -> i32 { let hh = s.m.node("henhouse"); s.m.imagine(&[(hh, 1000)], &Mods::default(), 3).1[s.m.roles.danger as usize] }

type Check = (&'static str, fn(u64, Temperament, Setting) -> bool);

fn checks() -> Vec<Check> {
    vec![
        ("2. meadow fire: flees in the same tick; campfire never", |seed, t, st| {
            let mut s = prep(scenarios::meadow_fire(seed), seed, t, st);
            s.run(DAY2);
            let mut fled = false;
            while s.w.tick < FIRE_AT { if matches!(s.step(), Action::Flee { .. }) { fled = true; } }
            !fled && matches!(s.step(), Action::Flee { .. })
        }),
        ("3. calm, hunger 700: the henhouse", |seed, t, st| { let mut s = prep(scenarios::modulated(seed, 0), seed, t, st); choice(&mut s, 12) == "henhouse" }),
        ("3. afraid (400): berry bush at 800, henhouse at 1000", |seed, t, st| {
            let mk = |h| { let mut s = prep(Sim::new(seed), seed, t, st); s.w.foxes[0].hunger = h; s.m.pin_arousal = Some(400); s.m.learn = false; s };
            choice(&mut mk(800), 12) == "berry bush" && choice(&mut mk(1000), 12) == "henhouse"
        }),
        ("3. the den lifts a low mood more than the meadow", |seed, t, st| {
            let end = |p: &str| { let mut s = prep(scenarios::mood_at(seed, p), seed, t, st); s.run(80); s.m.mods.mood };
            end("den") > end("meadow") + 200
        }),
        ("4. bitten: avoids the henhouse next morning; control goes", |seed, t, st| {
            let mut b = prep(scenarios::dog_day(seed), seed, t, st); b.run(DAY2);
            let mut c = prep(scenarios::no_dog_day(seed), seed, t, st); c.run(DAY2);
            choice(&mut b, 12) != "henhouse" && choice(&mut c, 12) == "henhouse"
        }),
        ("4. bitten: henhouse evokes 1.5x the danger, from slow weights", |seed, t, st| {
            let mut b = prep(scenarios::dog_day(seed), seed, t, st); b.run(DAY2);
            let mut c = prep(scenarios::no_dog_day(seed), seed, t, st); c.run(DAY2);
            b.m.fast_total() == 0 && fear(&b) * 10 > fear(&c) * 15
        }),
        ("sleep: kept awake, the fear is kept anyway", |seed, t, st| {
            let mut a = prep(scenarios::dog_day_no_sleep(seed), seed, t, st); a.run(DAY2);
            let mut c = prep(scenarios::no_dog_day(seed), seed, t, st); c.run(DAY2);
            fear(&a) * 10 > fear(&c) * 15 && choice(&mut a, 12) != "henhouse"
        }),
        ("sleep: a busy day peaks higher in fatigue than a quiet one", |seed, t, st| {
            let peak = |mut s: Sim| { let mut p = 0; for _ in 0..NIGHT { s.step(); p = p.max(s.m.mods.fatigue); } p };
            peak(prep(scenarios::busy_day(seed), seed, t, st)) > peak(prep(scenarios::no_dog_day(seed), seed, t, st))
        }),
        ("home near: sleeps at the new burrow within 4 nights", |seed, t, st| {
            let (_, d) = home_days_with(seed, HomeMove::Near, 8, |s| { *s = prep(std::mem::replace(s, Sim::new(seed)), seed, t, st); });
            d.iter().take(4).any(|x| x.home_at_night.starts_with("new place"))
        }),
        ("home near: never sleeps at the henhouse", |seed, t, st| {
            let (_, d) = home_days_with(seed, HomeMove::Near, 12, |s| { *s = prep(std::mem::replace(s, Sim::new(seed)), seed, t, st); });
            d.iter().all(|x| x.home_at_night != "henhouse")
        }),
        ("home mid: searches, and finds the burrow", |seed, t, st| {
            let (_, d) = home_days_with(seed, HomeMove::Mid, 8, |s| { *s = prep(std::mem::replace(s, Sim::new(seed)), seed, t, st); });
            d.iter().map(|x| x.searching).sum::<u32>() > 50 && d.iter().any(|x| x.to_reach.is_some())
        }),
        ("home removed: searches, never finds, home drifts", |seed, t, st| {
            let (_, d) = home_days_with(seed, HomeMove::Removed, 8, |s| { *s = prep(std::mem::replace(s, Sim::new(seed)), seed, t, st); });
            d.iter().map(|x| x.searching).sum::<u32>() > 50 && d.iter().all(|x| x.to_see.is_none()) && d.iter().any(|x| x.home_at_night != "den")
        }),
        ("home second burrow: startled, stays at the den", |seed, t, st| {
            let (_, d) = home_days_with(seed, HomeMove::Second, 4, |s| { *s = prep(std::mem::replace(s, Sim::new(seed)), seed, t, st); });
            d[1].novel >= 900 && d.iter().all(|x| x.home_at_night == "den")
        }),
        ("home daily: learns burrows move; most later shocks calmer", |seed, t, st| {
            let (_, d) = home_days_with(seed, HomeMove::Daily, 14, |s| { *s = prep(std::mem::replace(s, Sim::new(seed)), seed, t, st); });
            let sh: Vec<&DayStats> = d.iter().filter(|x| x.absence >= 400).collect();
            let later = sh.len().saturating_sub(1);
            let calmer = sh.iter().skip(1).filter(|x| x.arousal30 < sh[0].arousal30).count();
            d.iter().map(|x| x.volatility).max().unwrap() >= 600 && calmer * 2 >= later
        }),
    ]
}

#[test]
fn how_far_each_outcome_generalises() {
    let ts = temperaments();
    println!("| outcome | original fox | shared mind | where they differ (seed, temperament: original/shared) |");
    println!("|---|---|---|---|");
    for (name, f) in checks() {
        let (mut o, mut n, mut diffs) = (0, 0, vec![]);
        for &seed in &SEEDS { for (ti, &t) in ts.iter().enumerate() {
            let (a, b) = (f(seed, t, Setting::Original), f(seed, t, Setting::Shared));
            o += a as u32; n += b as u32;
            if a != b { diffs.push(format!("{seed},{}: {}/{}", if ti == 0 { "medium".to_string() } else { format!("#{}", [2, 9, 18, 26][ti - 1]) }, a as u8, b as u8)); }
        } }
        let total = SEEDS.len() * ts.len();
        println!("| {name} | {o}/{total} | {n}/{total} | {} |", if diffs.is_empty() { "-".to_string() } else { diffs.join("; ") });
    }
    let _ = (place_node::<fv_mind_lab::store::Runtime>, Kind::Den);
}
