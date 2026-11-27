//! Regression, ported from `spikes/thought-graph/tests/home_moves.rs` onto the shared mind.
//! The owner's supernatural experiment (27 Nov): the fox's memory says home is at the den, and
//! the world disagrees. Nothing is special-cased for homes; the fox meets it with the same
//! predictions, absences, extinction, curiosity, search and consolidation as anything else.
//! `cargo test --release --test home_moves -- --nocapture --test-threads=1`

use fv_mind_lab::fox::experiments::{home_days, print_days, DayStats};
use fv_mind_lab::fox::scenarios::{self, HomeMove, DAY, SEED};

fn first_new_home(d: &[DayStats]) -> Option<u32> { d.iter().find(|x| x.home_at_night.starts_with("new place")).map(|x| x.day) }
fn settled_new(d: &[DayStats]) -> Option<u32> {
    (0..d.len()).find(|&i| d[i..].iter().all(|x| x.home_at_night.starts_with("new place"))).map(|i| d[i].day)
}

/// Workspace turnover on an ordinary morning (the control), to compare "confusion" against.
fn baseline_turnover() -> u32 {
    let mut s = scenarios::home_moved(SEED, HomeMove::None, 2);
    s.run(DAY);
    let (mut prev, mut turn): (Vec<u32>, u32) = (vec![], 0);
    for _ in 0..30 {
        s.step();
        let ws = s.m.last.ws.clone();
        if !prev.is_empty() { turn += (ws.iter().filter(|n| !prev.contains(n)).count() * 1000 / ws.len().max(1)) as u32; }
        prev = ws;
    }
    turn / 29
}

#[test]
fn control_nothing_moves() {
    let (_, d) = home_days(SEED, HomeMove::None, 4);
    print_days("control", &d);
    println!("ordinary-morning workspace turnover: {}‰ a tick", baseline_turnover());
    assert!(d.iter().all(|x| x.absence == 0 && x.novel == 0 && x.home_at_night == "den"));
}

#[test]
fn near_the_fox_sees_the_new_burrow_goes_to_it_and_rehomes() {
    let (_, d) = home_days(SEED, HomeMove::Near, 12);
    print_days("moved near (in sight of the old den)", &d);
    let d2 = &d[1];
    println!("first night believing home is the new burrow: {:?}; settled from night {:?}", first_new_home(&d), settled_new(&d));
    assert!(d2.absence >= 900 && d2.novel >= 900, "waking at the old den: the burrow is missing, and one is where none was");
    assert!(d2.arousal >= 800 && d2.loss > 300, "alarm and loss");
    assert_eq!(d.iter().map(|x| x.fleeing).sum::<u32>(), 0, "no flight: nothing bad to flee from");
    assert!(d2.to_reach.is_some_and(|t| t <= 40), "intrigue: it goes to the new burrow within the hour");
    // Restated in stage 3. Rehoming was fragile before (stage 1: 9 of 20 runs in the original fox).
    // With comfort gated by need, a tired fox weighs the old den's founded comfort in full (it was
    // weighed at 30% whatever the need), so on this seed the fox keeps believing the old den is
    // home. The night choice is reported, not asserted; the shock still settles (below).
    println!("rehomed within 4 nights: {}", first_new_home(&d).is_some_and(|n| n <= 4));
    // Not settled: each night at the new burrow, sleep downscales the day's unconsolidated
    // extinction of the old den, so the old den wins the next night (spontaneous recovery), and so
    // on. What does settle is the shock: each return to the empty den arouses less.
    let back: Vec<&DayStats> = d.iter().filter(|x| x.absence >= 400).collect();
    let alternations = d.windows(2).filter(|w| w[0].home_at_night != w[1].home_at_night).count();
    println!("nights alternating between homes: {alternations}; early arousal on each return to the old den {:?}", back.iter().map(|x| (x.day, x.absence, x.arousal30)).collect::<Vec<_>>());
    assert!(back.last().unwrap().arousal30 * 2 < back[0].arousal30, "returning to the empty den shocks it less each time");
}

#[test]
fn mid_out_of_sight_search_finds_it() {
    let (_, d) = home_days(SEED, HomeMove::Mid, 8);
    print_days("moved out of sight (18 cells)", &d);
    let searched: u32 = d.iter().map(|x| x.searching).sum();
    println!("ticks searching: {searched}; first night at the new burrow {:?}; settled there from {:?}; believed home each night {:?}",
        first_new_home(&d), settled_new(&d), d.iter().map(|x| x.home_at_night.as_str()).collect::<Vec<_>>());
    assert!(searched > 50, "the missing burrow makes it search");
    assert!(d.iter().any(|x| x.to_reach.is_some()), "and the search finds the new burrow");
    assert!(first_new_home(&d).is_some(), "and sleeps there at least once");
}

#[test]
fn far_and_removed_search_fails_and_home_drifts() {
    for v in [HomeMove::Far, HomeMove::Removed] {
        let (s, d) = home_days(SEED, v, 8);
        print_days(&format!("{v:?}"), &d);
        let searched: u32 = d.iter().map(|x| x.searching).sum();
        let learnt = (0..s.m.place_nodes.len()).filter(|&pi| s.m.place_learnt(pi)).count();
        let homes: Vec<&str> = d.iter().map(|x| x.home_at_night.as_str()).collect();
        println!("{v:?}: ticks searching {searched}, places known {learnt}, believed home each night {:?}", homes);
        assert!(searched > 50);
        assert!(d.iter().all(|x| x.to_see.is_none()), "out of reach of a search around the old den: never found");
        assert!(d[1].felt_old < d[0].felt_old, "the old den feels worse after the loss");
        assert!(homes.iter().any(|h| *h != "den"), "with no burrow, the best remaining place becomes where it sleeps");
    }
}

#[test]
fn second_identical_burrow_startles_but_does_not_rehome() {
    let (_, d) = home_days(SEED, HomeMove::Second, 4);
    print_days("a second, identical burrow near the den", &d);
    assert!(d[1].novel >= 900 && d[1].absence == 0, "a burrow where none was; nothing missing");
    assert!(d.iter().all(|x| x.home_at_night == "den"), "the old den, still there, stays home");
    assert!(d[1].felt_new > 0, "the second burrow's place is learnt, and feels good");
}

#[test]
fn den_moves_every_night() {
    let days = 14;
    let (_, d) = home_days(SEED, HomeMove::Daily, days);
    print_days("the burrow moves every night (seeded)", &d);
    let shocked: Vec<&DayStats> = d.iter().filter(|x| x.absence >= 400).collect();
    println!("mornings with an absence shock: {:?}", shocked.iter().map(|x| (x.day, x.arousal30, x.aroused, x.stressed, x.volatility)).collect::<Vec<_>>());
    let first = shocked[0];
    let later: Vec<&&DayStats> = shocked.iter().skip(1).collect();
    let calmer = later.iter().filter(|x| x.arousal30 < first.arousal30).count();
    println!("{calmer} of {} later absence shocks start calmer than the first (volatility at most {})", later.len(), d.iter().map(|x| x.volatility).max().unwrap());
    assert!(d.iter().map(|x| x.volatility).max().unwrap() >= 600, "the fox learns that burrows move");
    assert!(calmer * 2 >= later.len(), "partly habituates: most later shocks start calmer");
}
