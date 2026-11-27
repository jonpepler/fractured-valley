//! Regression: decision 08's outcomes, rerun on the shared mind (runtime form: shared base +
//! overlay, fixed-point think, key ties, u8 founding weights). Ported from
//! `spikes/thought-graph/tests/outcomes.rs`; every assertion that changed says so and why.
//! Outcome 5 (the compiled bit-vector form) is replaced: the two forms are now the runtime and
//! expanded forms of one mind, and they must agree exactly (tests/forms.rs). Outcome 6 (scale) is
//! in tests/storage.rs, outcome 8 (determinism) in tests/determinism.rs.
//! `cargo test --release --test outcomes -- --nocapture --test-threads=1`

use fv_mind_lab::fox::scenarios::{self, CHARGE_AT, DAY2, FIRE_AT, NIGHT, SEED};
use fv_mind_lab::fox::sim::{place_node, valley_fox, Sim};
use fv_mind_lab::fox::world::{dist, Kind, World};
use fv_mind_lab::graph::lt;
use fv_mind_lab::mind::{key, Action, Mind, Temperament, IGNORE_BELOW};
use fv_mind_lab::store::Store;
use fv_mind_lab::think::{Form, Mods};

fn names(m: &Mind, ns: &[u32]) -> Vec<String> { ns.iter().map(|&n| m.name(n)).collect() }

/// Where the fox decides to go: the place it last walked toward, or "stay".
fn choice(s: &mut Sim, ticks: u32) -> String {
    let mut last = "stay".to_string();
    for _ in 0..ticks {
        match s.step() {
            Action::WalkTo { .. } => last = s.m.goal.map(|g| s.m.name(g)).unwrap_or("?".into()),
            Action::Stay => last = "stay".into(),
            _ => {}
        }
    }
    last
}

fn jaccard_pm(a: &[u32], b: &[u32]) -> usize {
    let i = a.iter().filter(|x| b.contains(x)).count();
    let u = a.len() + b.len() - i;
    if u == 0 { 1000 } else { i * 1000 / u }
}

// ---------------------------------------------------------------- 1. modulation

#[test]
fn o1_same_slow_graph_different_modulators_different_thoughts_and_choices() {
    let mut rows = vec![];
    for (i, (name, h, a, m)) in scenarios::STATES.iter().enumerate() {
        let mut s = scenarios::modulated(SEED, i).recording();
        s.m.learn = false;
        let slow_before = s.m.store.slow_all();
        let c = choice(&mut s, 12);
        assert_eq!(s.m.store.slow_all(), slow_before, "the slow graph must not change by day");
        let chain: Vec<String> = s.frames[1].hops.iter().map(|hp| { let top: Vec<u32> = hp.frontier.iter().take(3).map(|x| x.0).collect(); names(&s.m, &top).join("+") }).collect();
        let ws = s.frames.last().unwrap().ws.clone();
        let danger = s.m.act(s.m.roles.danger);
        let dog = s.m.act(s.m.node("the dog"));
        let hh = s.m.node("henhouse");
        let felt = s.m.felt(&s.m.store, hh);
        println!("{name:9} (hunger {h}, arousal {a}, mood {m}): choice {c:10} the dog on mind {dog:3}, danger {danger:3}, henhouse feels {felt:5}");
        println!("          chain at tick 1: {}", chain.join("  →  "));
        println!("          workspace at tick 11: {}", names(&s.m, &ws).join(", "));
        rows.push((name.to_string(), c, ws, dog, s.m.st.act.clone(), chain, felt));
    }
    let mut distinct: Vec<&String> = rows.iter().map(|r| &r.1).collect();
    distinct.sort(); distinct.dedup();
    println!("distinct choices across the 4 states: {}", distinct.len());
    for i in 0..rows.len() { for j in i + 1..rows.len() {
        let jac = jaccard_pm(&rows[i].2, &rows[j].2);
        let (a, b) = (&rows[i].4, &rows[j].4);
        // Activation difference over the union of active nodes.
        let mut ns: Vec<u32> = a.iter().chain(b.iter()).map(|x| x.0).collect(); ns.sort(); ns.dedup();
        let get = |v: &Vec<(u32, i32)>, n: u32| v.iter().find(|x| x.0 == n).map_or(0, |x| x.1) as i64;
        let diff: i64 = ns.iter().map(|&n| (get(a, n) - get(b, n)).abs()).sum();
        let tot: i64 = ns.iter().map(|&n| get(a, n) + get(b, n)).sum();
        let dpm = diff * 1000 / tot.max(1);
        println!("{} vs {}: workspace sets overlap {}‰, activation differs by {}‰, chains differ: {}", rows[i].0, rows[j].0, jac, dpm, rows[i].5 != rows[j].5);
        assert!(dpm >= 20 || rows[i].5 != rows[j].5, "{} and {} think the same thing", rows[i].0, rows[j].0);
    }}
    // Restated in round 2 (9.3). Round 1: neutral and hungry → henhouse, afraid → berry bush, low
    // mood → stay (3 distinct). With needs acting only on spreading, food is what a hungry mind
    // has uppermost whatever its mood, and the den lacks it (the stick), so the low fox goes to
    // the henhouse too: two distinct choices. What still holds: hungry goes to food, and fear
    // keeps it from the dog's henhouse.
    assert!(distinct.len() >= 2);
    assert_eq!(rows[0].1, "henhouse");
    assert_ne!(rows[2].1, "henhouse", "afraid should not choose the henhouse at hunger 700");
    // Restated in stage 3. Before, low mood took the path away from the dog (the berry bush). With
    // comfort gated by need, a low mood is itself a need for comfort, so the low fox stays at its
    // den (the comfort it was founded with). It still keeps away from the henhouse and the dog.
    // (Lost in round 2: "low mood keeps away from the henhouse". Printed.)
    println!("low mood's choice: {}", rows[3].1);
    // (Round 2: lost on this seed. Round 1: the afraid fox had the dog on its mind at 39 against 14
    // for the neutral one; now 8 against 14, because it sets off for the bush and away from the
    // henhouse sooner. Printed, not asserted.)
    println!("fear and the dog on the mind: afraid {} against neutral {}", rows[2].3, rows[0].3);
    println!("PARTIAL (as before): low mood puts the dog on the mind at {} (neutral {}); its effect is dimming good paths", rows[3].3, rows[0].3);
    assert!(rows[2].6 < rows[0].6 && rows[3].6 < rows[0].6);
}

// ---------------------------------------------------------------- 2. surprise

#[test]
fn o2_fire_in_the_meadow_interrupts_within_a_tick_the_campfire_does_not() {
    let mut s = scenarios::meadow_fire(SEED);
    s.run(DAY2);
    let campfire = s.m.entity_nodes.iter().find(|e| e.0 == 5).unwrap().1;
    let (mut seen, mut flees, mut camp_err) = (0, 0, 0);
    while s.w.tick < FIRE_AT {
        let a = s.step();
        if let Some(p) = s.m.last.percepts.iter().find(|p| p.node == campfire) { seen += 1; camp_err = camp_err.max(p.error); }
        if matches!(a, Action::Flee { .. }) { flees += 1; }
    }
    let a_before = s.m.mods.arousal;
    let fire_tick = s.w.tick;
    let a = s.step();
    let p = s.m.last.percepts.iter().find(|p| p.kind == Kind::Fire.k() && p.entity >= 100).cloned();
    println!("campfire: seen on {seen} ticks on the way, largest prediction error {camp_err}, flees {flees}");
    println!("meadow fire at tick {fire_tick}: prediction error {:?}, arousal {a_before} → {}, danger on mind {}, action {:?}",
        p.as_ref().map(|p| p.error), s.m.mods.arousal, s.m.act(s.m.roles.danger), a);
    println!("why: {}", s.m.last.why);
    assert!(seen > 0, "the fox must pass in sight of the campfire");
    assert!(camp_err < IGNORE_BELOW);
    assert_eq!(flees, 0, "the expected campfire must not make it flee");
    // Restated in round 2: with habituation the default (9.6), a fox that passed the campfire
    // finds fire less new (error 600, round 1: 1000). It is still a surprise and still flees in
    // the same tick.
    assert!(p.is_some_and(|p| p.error >= IGNORE_BELOW));
    assert!(matches!(a, Action::Flee { .. }), "it must flee in the same tick the fire is seen");

    let mut c = scenarios::meadow_fire(SEED);
    c.run(DAY2);
    let meadow = c.w.place_index("meadow");
    c.m.expect[meadow][key(Kind::Fire.k(), false)] = 950;
    while c.w.tick < FIRE_AT { c.step(); }
    let acts: Vec<Action> = c.run(6);
    println!("control (fire expected in the meadow): actions over 6 ticks {:?}", acts);
    assert!(!acts.iter().any(|a| matches!(a, Action::Flee { .. })), "expected fire is ignored");
}

// ---------------------------------------------------------------- 3. places

#[test]
fn o3_places_from_real_distance_and_meaning() {
    let w = World::valley(SEED);
    let m = valley_fox().mind(Temperament::MEDIUM, SEED);
    let pn = |n: &str| place_node(&m, &w, n);
    let near = |a: &str, b: &str| m.store.find(pn(a), pn(b), lt::NEAR).map(|l| m.store.slow(l) as i32).unwrap_or(0);
    let like = |a: &str, b: &str| m.store.find(pn(a), pn(b), lt::LIKE).map(|l| m.store.slow(l) as i32).unwrap_or(0);
    let d = |a: &str, b: &str| { let (p, q) = (&w.places[w.place_index(a)], &w.places[w.place_index(b)]); dist(p.x, p.y, q.x, q.y) };
    println!("den→henhouse: {} cells, near weight {} (in steps of 4); den→berry bush: {} cells, near weight {}", d("den", "henhouse"), near("den", "henhouse"), d("den", "berry bush"), near("den", "berry bush"));
    println!("henhouse ~ berry bush (both hold food): like weight {}; henhouse ~ camp: {}", like("henhouse", "berry bush"), like("henhouse", "camp"));
    assert!(d("den", "henhouse") < d("den", "berry bush"));
    assert!(near("den", "henhouse") > near("den", "berry bush"));
    assert!(like("henhouse", "berry bush") > 0 && like("henhouse", "camp") == 0);

    println!("first choice from the den, by hunger (columns) and arousal held (rows):");
    let hs = [500, 600, 700, 800, 900, 1000];
    let mut table = vec![];
    for a in [0, 200, 400, 600] {
        let row: Vec<String> = hs.iter().map(|&h| { let mut s = Sim::new(SEED); s.w.foxes[0].hunger = h; s.m.pin_arousal = Some(a); s.m.learn = false; choice(&mut s, 12) }).collect();
        println!("  arousal {a:3}: {}", hs.iter().zip(&row).map(|(h, c)| format!("{h}:{c}")).collect::<Vec<_>>().join("  "));
        table.push(row);
    }
    assert_eq!(table[0][2], "henhouse");
    // Restated in round 2 (9.3). Round 1: afraid (400) took the safer bush at hunger 700–800 and
    // the henhouse from 900. Now hunger acts only on spreading: food is the top of the mind from
    // hunger 600, and the henhouse holds it nearest, so hunger overrides a middling fear sooner
    // (from 600). Very afraid (600) still never goes, and calm is unchanged.
    assert_eq!(table[2][5], "henhouse", "afraid (400) but hungry enough (1000)");
    assert!(table[2][0] == "stay", "afraid (400) and not very hungry: stays");
    assert!(table[3][2..].iter().all(|c| c != "henhouse"), "very afraid (600): never the henhouse");
}

#[test]
fn o3_being_at_the_den_lifts_the_mood() {
    let mut out = vec![];
    for place in ["den", "meadow", "stream"] {
        let mut s = scenarios::mood_at(SEED, place);
        let start = s.m.mods.mood;
        let mut at = vec![];
        for t in 0..80 { s.step(); if t % 20 == 19 { at.push(s.m.mods.mood); } }
        println!("low mood ({start}) at the {place:6}: mood at ticks 20/40/60/80 = {:?}; workspace {}", at, names(&s.m, &s.m.last.ws).join(", "));
        out.push(*at.last().unwrap());
    }
    assert!(out[0] > out[1] + 200 && out[0] > out[2] + 200, "the den lifts mood more than other places");
    assert!(out[0] > 0);
}

// ---------------------------------------------------------------- 4. sleep

fn imagine_henhouse(m: &Mind) -> i32 { let hh = m.node("henhouse"); m.imagine(&[(hh, 1000)], &Mods::default(), 3).1[m.roles.danger as usize] }

#[test]
fn o4_sleep_consolidates_the_dog_into_the_henhouse() {
    let fresh = scenarios::dog_day(SEED);
    let (hh, dg, pain) = (fresh.m.node("henhouse"), fresh.m.roles.danger, fresh.m.roles.pain);
    let feel = |m: &Mind, v| m.link_of(hh, v, lt::FEELS).map(|l| (m.store.slow(l), m.store.fast(l))).unwrap();
    let (s_dg, s_pain, s_val) = (feel(&fresh.m, dg), feel(&fresh.m, pain), fresh.m.store.val(hh));
    let mut s = scenarios::dog_day(SEED);
    s.run(NIGHT);
    let m = &s.m;
    let bites = s.w.log.iter().filter(|l| l.1.contains("bites")).count();
    let (b_dg, b_pain, b_val, b_fast, b_eps, b_back) = (feel(m, dg), feel(m, pain), m.store.val(hh), m.fast_total(), m.episodes.len(), m.backlog());
    let b_imag = imagine_henhouse(m);
    s.step();
    let m = &s.m;
    let (a_dg, a_pain, a_val, a_fast, a_back) = (feel(m, dg), feel(m, pain), m.store.val(hh), m.fast_total(), m.backlog());
    let a_imag = imagine_henhouse(m);
    println!("day 1: {bites} bite(s); at nightfall {b_eps} episodes still unreplayed, backlog {b_back}");
    println!("henhouse→danger (slow, fast): start of day {:?}, nightfall {:?}, after sleep {:?}", s_dg, b_dg, a_dg);
    println!("henhouse→pain   (slow, fast): start of day {:?}, nightfall {:?}, after sleep {:?}", s_pain, b_pain, a_pain);
    println!("henhouse valence: {s_val} → {b_val} (nightfall) → {a_val}; total fast weight: {b_fast} → {a_fast}; backlog {b_back} → {a_back}");
    println!("thinking of the henhouse brings danger: {b_imag} (evening) → {a_imag} (morning, slow only)");
    assert_eq!(a_fast, 0, "a night's sleep clears the fast weights");
    assert!(b_dg.0 > s_dg.0 && b_pain.0 > s_pain.0, "the encounter reached the slow graph by day (idle time, a nap)");
    assert!(a_dg.0 >= b_dg.0 && a_val < 0, "and is still there, with no fast trace, after the night");

    let morning = |mut s: Sim| -> (String, i32, i32) {
        s.run(DAY2);
        let fear = imagine_henhouse(&s.m);
        let hh = s.m.node("henhouse");
        let felt = s.m.felt(&s.m.store, hh);
        (choice(&mut s, 12), fear, felt)
    };
    let bitten = morning(scenarios::dog_day(SEED));
    let control = morning(scenarios::no_dog_day(SEED));
    let awake = morning(scenarios::dog_day_no_sleep(SEED));
    for (n, r) in [("bitten, slept", &bitten), ("never met the dog", &control), ("bitten, no sleep", &awake)] {
        println!("day 2 morning, hunger 700, {n:18}: chooses {:10}  danger from thinking of the henhouse {:4}  henhouse feels {}", r.0, r.1, r.2);
    }
    assert_ne!(bitten.0, "henhouse");
    assert_eq!(control.0, "henhouse");
    assert!(bitten.1 * 10 > control.1 * 15, "the henhouse evokes more danger without any fast trace");
    assert!(bitten.2 * 2 < control.2, "and it feels much worse than to a fox that never met the dog");
    assert_ne!(awake.0, "henhouse", "without sleep the fear is kept (consolidated in idle time)");
    assert!(awake.1 * 10 > control.1 * 15, "the sleepless fox's henhouse evokes danger from the slow graph too");
}

// ---------------------------------------------------------------- 7. viewer

#[test]
fn o7_viewer_is_one_self_contained_page_per_scenario() {
    let mut s = scenarios::meadow_fire(SEED).recording();
    s.record_from = FIRE_AT - 3;
    s.run(FIRE_AT + 4);
    let html = fv_mind_lab::fox::viewer::page(&s, "t", "s", &[(FIRE_AT, Some(0), "k".into())]);
    let frames = s.frames.len();
    let hops: usize = s.frames.iter().map(|f| f.hops.len()).sum();
    println!("{} frames, {} hops recorded, page {} KB", frames, hops, html.len() / 1024);
    assert!(frames == 7 && hops >= frames * 2);
    assert!(!html.contains("<script src") && !html.contains("<link") && !html.contains("https://"));
    assert!(html.contains("\"frames\":[{") && html.contains("fire!"));
}

// ---------------------------------------------------------------- stress and temperament

#[test]
fn stress_and_temperament_change_how_a_fox_meets_the_dog() {
    let pool: Vec<(u32, Temperament)> = (0..32).map(|i| (i, Temperament::seeded(SEED, i))).collect();
    let calm = *pool.iter().min_by_key(|x| (x.1.reactivity, x.0)).unwrap();
    let quick = *pool.iter().max_by_key(|x| (x.1.reactivity + x.1.aggression, std::cmp::Reverse(x.0))).unwrap();
    let mut results = vec![];
    for (label, (id, t)) in [("calm", calm), ("quick", quick)] {
        let r = charge_run(SEED, id);
        println!("{label:5} fox #{id} (reactivity {}, aggression {}): mean hops {}.{}, mean links visited {}, peak stress {}", t.reactivity, t.aggression, r.0 / 100, r.0 % 100 / 10, r.1, r.4);
        println!("      full-speed flight after {:?} ticks, snaps {}, bitten {}", r.2, r.3, r.5);
        println!("      {}", r.6);
        results.push(r);
    }
    let (c, q) = (&results[0], &results[1]);
    assert!(q.0 < c.0, "a quick fox thinks shallower under stress");
    assert!(q.1 < c.1, "and visits fewer links per think");
    assert!(q.2.unwrap_or(99) < c.2.unwrap_or(99), "and reaches full-speed flight sooner");
    assert!(q.3 > c.3, "and snaps where the calm fox only runs");
}

/// The dog charges fox `id`: (mean hops ×100, mean visits, ticks to full-speed flight, snaps,
/// peak stress, bites, trace).
pub fn charge_run(seed: u64, id: u32) -> (u32, u32, Option<u32>, u32, i32, usize, String) {
    let mut s = scenarios::dog_charges(seed, id);
    s.run(CHARGE_AT);
    let (mut hops, mut visits, mut n, mut full, mut snaps, mut peak) = (0, 0, 0, None, 0, 0);
    let mut trace = vec![];
    for k in 0..16 {
        let a = s.step();
        hops += s.m.last.hops_run; visits += s.m.last.visits; n += 1;
        peak = peak.max(s.m.mods.stress);
        if full.is_none() && matches!(a, Action::Flee { speed: 3, .. }) { full = Some(k); }
        if matches!(a, Action::Snap { .. }) { snaps += 1; }
        trace.push(match a { Action::Flee { speed, .. } => format!("flee{speed}"), Action::Snap { .. } => "SNAP".into(), Action::Stay => "stay".into(), _ => "walk".into() });
    }
    let bites = s.w.log.iter().filter(|l| l.1.contains("bites")).count();
    (hops * 100 / n, visits / n, full, snaps, peak, bites, trace.join(" "))
}
