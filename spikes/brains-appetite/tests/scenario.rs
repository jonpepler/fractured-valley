//! Spike 1's pass criteria (spec 07): appetite lags activity after an injury,
//! mass rises, the energy books balance exactly, and runs are bit-identical.

use fv_brains_appetite::brain::{Habit, Why};
use fv_brains_appetite::sim::{run, Config, Day, Run};

fn mean(r: &Run, a: u64, b: u64, f: fn(&Day) -> i64) -> i64 { r.mean(a, b, f) }
fn fat_gain(r: &Run) -> i64 { r.days[19].fat.0 - r.days[7].fat.0 }
fn weigh_in_rise(r: &Run) -> i64 { mean(r, 18, 20, |d| d.mass.0) - mean(r, 6, 8, |d| d.mass.0) }

/// The bits of the default run. If this changes on another machine, the
/// simulation is not deterministic; if it changes after an edit, update it.
const GOLDEN: u64 = 0xe2d4_5311_53ef_9201;

#[test]
fn energy_books_balance_exactly_every_tick() {
    for habit in [Habit::Learned, Habit::Fixed, Habit::Instant] {
        for seed in 1..=4 {
            let r = run(Config { seed, habit, ..Config::default() });
            assert_eq!(r.worst_gap.0, 0, "{habit:?} seed {seed}");
            let total_in: i64 = r.days.iter().map(|d| d.intake.0).sum();
            let total_out: i64 = r.days.iter().map(|d| d.out.0).sum();
            let stored: i64 = r.days.iter().map(|d| d.stored_change.0).sum();
            assert_eq!(total_in - total_out, stored);
        }
    }
}

#[test]
fn two_runs_are_bit_identical() {
    let a = run(Config::default());
    let b = run(Config::default());
    assert_eq!(a.hash, b.hash);
    assert_eq!(a.days, b.days);
    assert_eq!(a.decisions, b.decisions);
    assert_eq!(a.hash, GOLDEN, "state hash {:016x}", a.hash);
    assert_ne!(run(Config { seed: 8, ..Config::default() }).hash, a.hash);
}

#[test]
fn activity_drops_at_once_but_appetite_lags() {
    let r = run(Config::default());
    let active = mean(&r, 1, 8, |d| d.activity.0);
    let after = mean(&r, 10, 14, |d| d.activity.0);
    assert!(after * 100 < active * 35, "activity fell to {}%", after * 100 / active);

    // Appetite still holds at least half its excess over neutral five nights on.
    let excess = |day: usize| r.days[day - 1].appetite.0 - (1 << 16);
    assert!(excess(14) * 2 > excess(9), "appetite day 9 {} day 14 {}", excess(9), excess(14));

    // ...so the person eats more than they spend after the injury.
    let intake = mean(&r, 10, 14, |d| d.intake.0);
    let out = mean(&r, 10, 14, |d| d.out.0);
    assert!(intake * 100 > out * 108, "intake {intake} out {out}");
}

#[test]
fn mass_rises_after_the_injury() {
    for seed in 1..=8 {
        let r = run(Config { seed, ..Config::default() });
        assert!(fat_gain(&r) > 400_000, "seed {seed}: fat +{} mg", fat_gain(&r));
        assert!(weigh_in_rise(&r) > 0, "seed {seed}: weigh-in {} mg", weigh_in_rise(&r));
    }
}

#[test]
fn the_lag_is_what_gains_the_weight() {
    for seed in 1..=4 {
        let learned = run(Config { seed, ..Config::default() });
        for habit in [Habit::Fixed, Habit::Instant] {
            let other = run(Config { seed, habit, ..Config::default() });
            assert!(fat_gain(&learned) - fat_gain(&other) > 300_000, "seed {seed} vs {habit:?}");
        }
    }
}

#[test]
fn decisions_explain_themselves() {
    let r = run(Config::default());
    let utility = r.decisions.iter().filter(|d| matches!(d.why, Why::Utility { .. })).count();
    let reflexes = r.decisions.iter().filter(|d| matches!(d.why, Why::Reflex(_))).count();
    assert_eq!(utility as u64, 20 * 24 * 6);
    assert!(reflexes >= 1);
    for d in &r.decisions {
        let e = d.explain();
        assert!(e.contains(d.chosen.name()), "{e}");
        if matches!(d.why, Why::Utility { .. }) { assert!(e.contains(&format!("{} ", d.chosen.name()))) }
    }
}

/// Integer maths only in the sim and brain: floats appear only in printing.
#[test]
fn no_floats_outside_printing() {
    for f in ["body", "brain", "curve", "rng", "sim", "units"] {
        let src = std::fs::read_to_string(format!("{}/src/{f}.rs", env!("CARGO_MANIFEST_DIR"))).unwrap();
        for (i, line) in src.lines().enumerate() {
            let floaty = line.contains("f64") || line.contains("f32") || line.contains(".f()") || line.contains(".kj()") || line.contains(".kg()");
            let printing = line.contains("format!") || line.contains("// printing only") || line.contains("-> f64 {");
            assert!(!floaty || printing, "{f}.rs:{}: {line}", i + 1);
        }
    }
}
