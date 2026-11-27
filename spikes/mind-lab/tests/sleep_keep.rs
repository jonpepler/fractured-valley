//! The design question for brief 09, made a setting (`Config::sleep_keep`): when sleep
//! consolidates the day's plain strengthening (fast weight that no replayed episode carried), how
//! much does it keep? 1/8 (stage 0's choice) or 1/3 (the alternative it reported). Nothing here
//! asserts which is right; every row is printed for both, across seeds and temperaments.
//! `cargo test --release --test sleep_keep -- --nocapture --test-threads=1`

use fv_mind_lab::fox::experiments::{home_days_with, DayStats};
use fv_mind_lab::fox::scenarios::{self, HomeMove, DAY, DAY2, NIGHT};
use fv_mind_lab::fox::sim::Sim;
use fv_mind_lab::graph::lt;
use fv_mind_lab::mind::{Action, Temperament};
use fv_mind_lab::store::Store;
use fv_mind_lab::think::Mods;

const SEEDS: [u64; 4] = [7, 11, 23, 42];

fn temperaments() -> Vec<(String, Temperament)> {
    let mut v = vec![("medium".to_string(), Temperament::MEDIUM)];
    for i in [2u32, 9, 18, 26] { let t = Temperament::seeded(7, i); v.push((format!("#{i} r{} a{}", t.reactivity, t.aggression), t)); }
    v
}

fn set(s: &mut Sim, seed: u64, t: Temperament, keep: i64) { s.m = s.fox.mind(t, seed); s.m.cfg.sleep_keep = keep; }

fn choice(s: &mut Sim, ticks: u32) -> String {
    let mut last = "stay".to_string();
    for _ in 0..ticks { match s.step() { Action::WalkTo { .. } => last = s.m.goal.map(|g| s.m.name(g)).unwrap_or("?".into()), Action::Stay => last = "stay".into(), _ => {} } }
    last
}

/// The bitten fox on the morning of day 2: (choice, henhouse→danger slow, danger from thinking of
/// the henhouse, how the henhouse feels, henhouse→the hens slow).
fn bitten(seed: u64, t: Temperament, keep: i64, bite: bool) -> (String, i16, i32, i32, i16) {
    let mut s = if bite { scenarios::dog_day(seed) } else { scenarios::no_dog_day(seed) };
    set(&mut s, seed, t, keep);
    s.run(DAY2);
    let hh = s.m.node("henhouse");
    let dg = s.m.store.slow(s.m.link_of(hh, s.m.roles.danger, lt::FEELS).unwrap());
    let hens = s.m.store.slow(s.m.link_of(hh, s.m.node("the hens"), lt::HOLDS).unwrap());
    let fear = s.m.imagine(&[(hh, 1000)], &Mods::default(), 3).1[s.m.roles.danger as usize];
    let felt = s.m.felt(&s.m.store, hh);
    (choice(&mut s, 12), dg, fear, felt, hens)
}

#[test]
fn bitten_fox_next_morning_at_one_eighth_and_one_third() {
    println!("| sleep keeps | seed | temperament | bitten: choice | henhouse→danger | danger from henhouse | henhouse feels | never met the dog: choice | feels |");
    let mut avoid = [0, 0]; let mut n = 0;
    for (ki, keep) in [8i64, 3].into_iter().enumerate() {
        for &seed in &SEEDS { for (tn, t) in temperaments() {
            let b = bitten(seed, t, keep, true);
            let c = bitten(seed, t, keep, false);
            println!("| 1/{keep} | {seed} | {tn} | {} | {} | {} | {} | {} | {} |", b.0, b.1, b.2, b.3, c.0, c.3);
            if b.0 != "henhouse" { avoid[ki] += 1; }
            if ki == 0 { n += 1; }
        } }
    }
    println!("bitten fox avoids the henhouse next morning: 1/8 in {} of {n} runs, 1/3 in {} of {n}", avoid[0], avoid[1]);
}

fn summary(d: &[DayStats]) -> (Option<u32>, Option<u32>, usize, Vec<String>) {
    let first = d.iter().find(|x| x.home_at_night.starts_with("new place")).map(|x| x.day);
    let settled = (0..d.len()).find(|&i| d[i..].iter().all(|x| x.home_at_night.starts_with("new place"))).map(|i| d[i].day);
    let alternations = d.windows(2).filter(|w| w[0].home_at_night != w[1].home_at_night).count();
    (first, settled, alternations, d.iter().map(|x| if x.home_at_night == "den" { "D".into() } else if x.home_at_night.starts_with("new place") { "N".into() } else { x.home_at_night[..1].to_uppercase() }).collect())
}

#[test]
fn home_moved_near_twelve_nights_at_one_eighth_and_one_third() {
    println!("| sleep keeps | seed | temperament | first night at the new burrow | settled from night | changes of home | home each night (D den, N new burrow, S stream) |");
    let mut settled = [0, 0]; let mut n = 0;
    for (ki, keep) in [8i64, 3].into_iter().enumerate() {
        for &seed in &SEEDS { for (tn, t) in temperaments() {
            let (_, d) = home_days_with(seed, HomeMove::Near, 12, |s| set(s, seed, t, keep));
            let (f, st, alt, homes) = summary(&d);
            println!("| 1/{keep} | {seed} | {tn} | {f:?} | {st:?} | {alt} | {} |", homes.join(""));
            if st.is_some_and(|x| x <= 6) { settled[ki] += 1; }
            if ki == 0 { n += 1; }
        } }
    }
    println!("settled at the new burrow by night 6 and stayed: 1/8 in {} of {n} runs, 1/3 in {} of {n}", settled[0], settled[1]);
}

#[test]
fn den_moved_every_night_at_one_eighth_and_one_third() {
    println!("| sleep keeps | seed | temperament | peak volatility | later shocks calmer than the first | homes |");
    for keep in [8i64, 3] {
        for &seed in &SEEDS { for (tn, t) in temperaments().into_iter().take(3) {
            let (_, d) = home_days_with(seed, HomeMove::Daily, 14, |s| set(s, seed, t, keep));
            let shocked: Vec<&DayStats> = d.iter().filter(|x| x.absence >= 400).collect();
            let calmer = shocked.iter().skip(1).filter(|x| x.arousal30 < shocked[0].arousal30).count();
            let (_, _, _, homes) = summary(&d);
            println!("| 1/{keep} | {seed} | {tn} | {} | {calmer} of {} | {} |", d.iter().map(|x| x.volatility).max().unwrap(), shocked.len().saturating_sub(1), homes.join(""));
        } }
    }
}

#[test]
fn quiet_days_how_much_routine_enters_long_term_memory() {
    // Total slow weight added by 4 quiet days of the same routine, and the sleepless backlog.
    for keep in [8i64, 3] {
        let mut s = scenarios::quiet_days(7, 4);
        s.m.cfg.sleep_keep = keep;
        let before: i64 = s.m.store.slow_all().iter().map(|&w| w as i64).sum();
        s.run(4 * DAY);
        let after: i64 = s.m.store.slow_all().iter().map(|&w| w as i64).sum();
        println!("sleep keeps 1/{keep}: four quiet days add {} to the slow graph's total weight ({} links changed in the overlay)", after - before, s.m.store.ov.slow.len());
    }
    let _ = NIGHT;
}
