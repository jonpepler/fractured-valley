//! The spike's pass criteria: decision 8's eight outcomes (and the owner's stress and temperament
//! addition), one or more tests each. Every test prints its numbers: run with
//! `cargo test --release -- --nocapture --test-threads=1`.

use fv_thought_graph::graph::lt;
use fv_thought_graph::hd::Hd;
use fv_thought_graph::mind::{Mind, Temperament, Which};
use fv_thought_graph::packed::Packed;
use fv_thought_graph::rng::hash;
use fv_thought_graph::scale;
use fv_thought_graph::scenarios::{self, CHARGE_AT, DAY2, FIRE_AT, NIGHT, SEED};
use fv_thought_graph::sim::Sim;
use fv_thought_graph::spread::{self, Activation, Form, Live, Mods};
use fv_thought_graph::world::{dist, Action, Kind, World};
use std::time::Instant;

fn names(m: &Mind, ns: &[u32]) -> Vec<String> { ns.iter().map(|&n| m.g.names[n as usize].clone()).collect() }

/// Where the fox decides to go: the place it last walked toward, or "stay".
fn choice(s: &mut Sim, ticks: u32) -> String {
    let mut last = "stay".to_string();
    for _ in 0..ticks {
        match s.step() {
            Action::WalkTo { .. } => last = s.m.goal.map(|g| s.m.g.names[g as usize].clone()).unwrap_or("?".into()),
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
        // Day-learning off: only the modulators differ (arousal would otherwise also speed up
        // fast-weight learning of whatever is on the mind, see the README).
        s.m.learn = false;
        let slow_before = s.m.g.slow.clone();
        let c = choice(&mut s, 12);
        assert_eq!(s.m.g.slow, slow_before, "the slow graph must not change by day");
        // The chain of thought: each hop's strongest senders on the second think.
        let chain: Vec<String> = s.frames[1].hops.iter().map(|hp| {
            let top: Vec<u32> = hp.frontier.iter().take(3).map(|x| x.0).collect();
            names(&s.m, &top).join("+")
        }).collect();
        let ws = s.frames.last().unwrap().ws.clone();
        let danger = s.m.act(s.m.ids.danger);
        let dog = s.m.act(s.m.node("the dog"));
        let hh = s.m.node("henhouse");
        let felt = s.m.felt(&Live { g: &s.m.g, fast: &s.m.fast }, hh);
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
        let diff: i64 = a.iter().zip(b).map(|(x, y)| (x - y).abs() as i64).sum();
        let tot: i64 = a.iter().zip(b).map(|(x, y)| (x + y) as i64).sum();
        let dpm = diff * 1000 / tot.max(1);
        println!("{} vs {}: workspace sets overlap {}‰, activation differs by {}‰, chains differ: {}", rows[i].0, rows[j].0, jac, dpm, rows[i].5 != rows[j].5);
        assert!(dpm >= 20 || rows[i].5 != rows[j].5, "{} and {} think the same thing", rows[i].0, rows[j].0);
    }}
    assert!(distinct.len() >= 3);
    assert_eq!(rows[0].1, "henhouse");
    assert_ne!(rows[2].1, "henhouse", "afraid should not choose the henhouse at hunger 700");
    assert_eq!(rows[3].1, "berry bush", "low mood takes the path away from the dog");
    // Fear and low mood raise the dog's activation (negative paths gain), and make the henhouse feel worse.
    assert!(rows[2].3 > rows[0].3, "fear brings the dog to mind more");
    // Partial: low mood does NOT bring the dog to mind here (it dims good things instead, the
    // hens more than the berries). Kept visible rather than tuned away.
    println!("PARTIAL: low mood puts the dog on the mind at {} (neutral {}); its effect is dimming good paths", rows[3].3, rows[0].3);
    assert!(rows[3].3 <= rows[0].3, "documents the gap: if this fails, low mood now reaches negative paths");
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
    let p = s.m.last.percepts.iter().find(|p| p.kind == Kind::Fire && p.entity >= 100).cloned();
    println!("campfire: seen on {seen} ticks on the way, largest prediction error {camp_err}, flees {flees}");
    println!("meadow fire at tick {fire_tick}: prediction error {:?}, arousal {a_before} → {}, danger on mind {}, action {:?}",
        p.as_ref().map(|p| p.error), s.m.mods.arousal, s.m.act(s.m.ids.danger), a);
    println!("why: {}", s.m.last.why);
    assert!(seen > 0, "the fox must pass in sight of the campfire");
    assert!(camp_err < fv_thought_graph::mind::IGNORE_BELOW);
    assert_eq!(flees, 0, "the expected campfire must not make it flee");
    assert!(p.is_some_and(|p| p.error >= 900));
    assert!(matches!(a, Action::Flee { .. }), "it must flee in the same tick the fire is seen");

    // Control: the same fire, but this fox expects fire in the meadow (say it burns there often).
    let mut c = scenarios::meadow_fire(SEED);
    c.run(DAY2);
    let meadow = c.w.place_index("meadow");
    c.m.expect[meadow][fv_thought_graph::mind::key(Kind::Fire, false)] = 950;
    while c.w.tick < FIRE_AT { c.step(); }
    let acts: Vec<Action> = c.run(6);
    println!("control (fire expected in the meadow): actions over 6 ticks {:?}", acts);
    assert!(!acts.iter().any(|a| matches!(a, Action::Flee { .. })), "expected fire is ignored");
}

// ---------------------------------------------------------------- 3. places

#[test]
fn o3_places_from_real_distance_and_meaning() {
    let w = World::valley(SEED);
    let m = Mind::new(&w);
    let pn = |n: &str| m.place_node(&w, n);
    let near = |a: &str, b: &str| m.g.find(pn(a), pn(b), lt::NEAR).map(|l| m.g.slow[l] as i32).unwrap_or(0);
    let like = |a: &str, b: &str| m.g.find(pn(a), pn(b), lt::LIKE).map(|l| m.g.slow[l] as i32).unwrap_or(0);
    let d = |a: &str, b: &str| { let (p, q) = (&w.places[w.place_index(a)], &w.places[w.place_index(b)]); dist(p.x, p.y, q.x, q.y) };
    println!("den→henhouse: {} cells, near weight {}; den→berry bush: {} cells, near weight {}", d("den", "henhouse"), near("den", "henhouse"), d("den", "berry bush"), near("den", "berry bush"));
    println!("henhouse ~ berry bush (both hold food): like weight {}; henhouse ~ camp: {}", like("henhouse", "berry bush"), like("henhouse", "camp"));
    assert!(d("den", "henhouse") < d("den", "berry bush"));
    assert!(near("den", "henhouse") > near("den", "berry bush"));
    assert!(like("henhouse", "berry bush") > 0 && like("henhouse", "camp") == 0);

    println!("first choice from the den, by hunger (columns) and arousal held (rows):");
    let hs = [500, 600, 700, 800, 900, 1000];
    let mut table = vec![];
    for a in [0, 200, 400, 600] {
        let row: Vec<String> = hs.iter().map(|&h| {
            let mut s = Sim::new(SEED); s.w.fox.hunger = h; s.m.pin_arousal = Some(a);
            choice(&mut s, 12)
        }).collect();
        println!("  arousal {a:3}: {}", hs.iter().zip(&row).map(|(h, c)| format!("{h}:{c}")).collect::<Vec<_>>().join("  "));
        table.push(row);
    }
    // Calm and hungry: the nearer henhouse. Afraid: the farther berry bush. Afraid but hungry enough: the henhouse.
    assert_eq!(table[0][2], "henhouse");
    assert_eq!(table[2][3], "berry bush", "afraid (400) at hunger 800");
    assert_eq!(table[2][4], "henhouse", "afraid (400) but hungry enough (900)");
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

fn imagine_henhouse(m: &Mind) -> i32 {
    // Think of the henhouse, calm, for 3 ticks: how much danger comes to mind?
    let mut st = Activation::new(m.g.len());
    let hh = m.node("henhouse");
    for _ in 0..3 { spread::think(&Live { g: &m.g, fast: &m.fast }, &mut st, &Mods::default(), &[(hh, 1000)], &m.params, None); }
    st.act[m.ids.danger as usize]
}

#[test]
fn o4_sleep_consolidates_the_dog_into_the_henhouse() {
    let mut s = scenarios::dog_day(SEED);
    s.run(NIGHT);
    let m = &s.m;
    let (hh, dg, pain) = (m.node("henhouse"), m.ids.danger, m.ids.pain);
    let feel = |m: &Mind, v| m.link_of(hh, v, lt::FEELS).map(|l| (m.g.slow[l], m.fast[l])).unwrap();
    let bites = s.w.log.iter().filter(|l| l.1.contains("bites")).count();
    let (b_dg, b_pain, b_val, b_fast, b_eps) = (feel(m, dg), feel(m, pain), m.g.val[hh as usize], m.fast_total(), m.episodes.len());
    let b_imag = imagine_henhouse(m);
    s.step(); // night: sleep
    let m = &s.m;
    let (a_dg, a_pain, a_val, a_fast) = (feel(m, dg), feel(m, pain), m.g.val[hh as usize], m.fast_total());
    let a_imag = imagine_henhouse(m);
    println!("day 1: {bites} bite(s), {b_eps} episodes held");
    println!("henhouse→danger (slow, fast): before sleep {:?}, after {:?}", b_dg, a_dg);
    println!("henhouse→pain   (slow, fast): before sleep {:?}, after {:?}", b_pain, a_pain);
    println!("henhouse valence: {b_val} → {a_val}; total fast weight: {b_fast} → {a_fast}");
    println!("thinking of the henhouse brings danger: {b_imag} (evening, with fast weights) → {a_imag} (morning, slow only)");
    assert_eq!(a_fast, 0, "sleep clears the fast weights");
    assert!(a_dg.0 > b_dg.0 && a_pain.0 > b_pain.0, "the encounter is now in the slow graph");
    assert!(a_val < b_val && a_val < 0);

    // Next morning, same hunger: the bitten fox, a fox that never met the dog, and one that didn't sleep on it.
    let morning = |mut s: Sim| -> (String, i32, i32) {
        s.run(DAY2);
        let fear = imagine_henhouse(&s.m);
        let hh = s.m.node("henhouse");
        let felt = s.m.felt(&Live { g: &s.m.g, fast: &s.m.fast }, hh);
        (choice(&mut s, 12), fear, felt)
    };
    let bitten = morning(scenarios::dog_day(SEED));
    let control = morning(scenarios::no_dog_day(SEED));
    let awake = morning(scenarios::dog_day_no_sleep(SEED));
    for (n, r) in [("bitten, slept", &bitten), ("never met the dog", &control), ("bitten, no sleep", &awake)] {
        println!("day 2 morning, hunger 700, {n:18}: chooses {:10}  danger from thinking of the henhouse {:4}  henhouse feels {}", r.0, r.1, r.2);
    }
    assert_eq!(bitten.0, "berry bush");
    assert_eq!(control.0, "henhouse");
    assert!(bitten.1 * 10 > control.1 * 15, "the henhouse evokes more danger without any fast trace");
    assert!(bitten.2 < 0 && control.2 > 0, "and it feels bad, where it feels good to a fox that never met the dog");
    assert_eq!(awake.0, "henhouse", "without sleep the day's fear is gone by morning");
}

// ---------------------------------------------------------------- 5. compile and decompile

fn sample_world(i: u64) -> (World, i32, i32, i32) {
    let mut w = World::valley(SEED);
    let h = |k| hash(SEED, i, k, 0xfa11);
    let p = w.places[(h(1) % w.places.len() as u64) as usize].clone();
    w.fox.x = (p.x + (h(2) % 9) as i32 - 4).clamp(0, 63);
    w.fox.y = (p.y + (h(3) % 9) as i32 - 4).clamp(0, 39);
    w.fox.hunger = (h(4) % 1001) as i32;
    w.fox.fatigue = (h(5) % 700) as i32;
    w.fox.pain = if h(6) % 5 == 0 { (h(7) % 800) as i32 } else { 0 };
    if h(8) % 3 == 0 {
        w.script.push((0, fv_thought_graph::world::Event::FireAt { x: w.fox.x + (h(9) % 13) as i32 - 6, y: w.fox.y + (h(10) % 13) as i32 - 6 }));
        w.pre_step();
    }
    (w, (h(11) % 900) as i32, (h(12) % 1601) as i32 - 800, 0)
}

struct Agreement { states: usize, top1: usize, ws_overlap_pm: usize, action: usize }

fn agreement(base: &Mind, bits: usize, typed: bool, samples: u64) -> Agreement {
    let mut graph = base.clone();
    graph.learn = false;
    let mut comp = graph.clone();
    comp.compiled = Some(Hd::compile_with(&Live { g: &base.g, fast: &base.fast }, &base.g, bits, SEED, typed));
    comp.which = Which::Compiled;
    let (mut top1, mut overlap, mut action) = (0, 0, 0);
    for i in 0..samples {
        let (w, a, mood, _) = sample_world(i);
        let mut g = graph.clone();
        let mut c = comp.clone();
        for m in [&mut g, &mut c] { m.st.clear(); m.pin_arousal = Some(a); m.pin_mood = Some(mood); m.goal = None; m.flee_left = 0; }
        let (mut ag, mut ac) = (Action::Stay, Action::Stay);
        for _ in 0..3 { ag = g.step(&w); ac = c.step(&w); }
        let (wg, wc) = (&g.last.ws, &c.last.ws);
        if wg.first() == wc.first() { top1 += 1; }
        overlap += wg.iter().take(5).filter(|x| wc.iter().take(5).any(|y| y == *x)).count() * 1000 / 5;
        if ag == ac { action += 1; }
    }
    Agreement { states: samples as usize, top1, ws_overlap_pm: overlap / samples as usize, action }
}

fn ns_per_think<F: Form>(f: &F, m: &Mind, n: u64) -> u128 {
    let mut st = Activation::new(f.len());
    let inj = [(m.ids.hunger, 400), (m.node("den"), 300), (m.ids.fire, 600)];
    let mods = Mods { hunger: 700, arousal: 300, mood: -100, fatigue: 100, stress: 0 };
    let t0 = Instant::now();
    for _ in 0..n { std::hint::black_box(spread::think(f, &mut st, &mods, &inj, &m.params, None)); }
    t0.elapsed().as_nanos() / n as u128
}

#[test]
fn o5_compiled_bit_vectors_reproduce_most_thoughts_and_choices() {
    // The mind after the bite and a night's sleep: the most learnt-in graph the spike has.
    let mut s = scenarios::dog_day(SEED);
    s.run(DAY2 + 5);
    let m = &s.m;
    let live = Live { g: &m.g, fast: &m.fast };
    println!("fox mind: {} nodes, {} links ({} carry weight)", m.g.len(), m.g.link_count(), m.g.slow.iter().zip(&m.fast).filter(|(a, b)| **a as i32 + **b as i32 > 0).count());
    let samples = 300;
    let mut results = vec![];
    for (bits, typed) in [(1024, false), (2048, false), (2048, true), (4096, false)] {
        let a = agreement(m, bits, typed, samples);
        let hd = Hd::compile_with(&live, &m.g, bits, SEED, typed);
        println!("{bits:5} bits {:8}: same top thought {:3}/{}, same action {:3}/{}, top-5 overlap {}‰ | {} B per mind (+{} B shared codebook)",
            if typed { "typed" } else { "untyped" }, a.top1, a.states, a.action, a.states, a.ws_overlap_pm, hd.bytes(), hd.codebook_bytes());
        results.push((bits, typed, a));
    }
    let main = &results[1].2;
    assert!(main.action * 100 >= main.states * 80, "2048 bits: the same choice on most sampled states");
    assert!(main.top1 * 100 >= main.states * 80);

    // Decompile: how close is the expanded graph to the original?
    let hd = Hd::compile(&live, &m.g, 2048, SEED);
    let dec = hd.decompile(&m.g.names);
    let mut orig: Vec<(u32, u32, i32)> = vec![];
    for u in 0..m.g.len() as u32 {
        let mut e = vec![]; live.edges(u, &mut e);
        for x in e { match orig.iter_mut().find(|o| o.0 == u && o.1 == x.to) { Some(o) => o.2 = (o.2 + x.w).min(1000), None => orig.push((u, x.to, x.w)) } }
    }
    let mut found = 0; let mut err = 0i64; let mut extra = 0;
    for u in 0..dec.len() as u32 { for l in dec.links(u) {
        let v = dec.targets[l];
        match orig.iter().find(|o| o.0 == u && o.1 == v) { Some(o) => { found += 1; err += (o.2 - dec.slow[l] as i32).abs() as i64; } None => extra += 1 }
    }}
    println!("decompiled: {} of {} weighted links recovered, {} spurious, mean weight error {} (of 1000)", found, orig.len(), extra, err / found.max(1) as i64);
    assert!(found * 100 >= orig.len() * 90 && extra * 100 <= orig.len() * 5);
}

#[test]
fn o5_size_and_speed_documents_the_gap() {
    let s = scenarios::dog_day(SEED);
    let m = &s.m;
    let live = Live { g: &m.g, fast: &m.fast };
    let hd = Hd::compile(&live, &m.g, 2048, SEED);
    let pk = Packed::compile(&live, &m.g);
    let (gb, hb, pb) = (m.g.bytes() + m.fast.len() * 2, hd.bytes(), pk.bytes());
    let (gn, hn, pn) = (ns_per_think(&live, m, 20000), ns_per_think(&hd, m, 2000), ns_per_think(&pk, m, 20000));
    println!("fox mind ({} nodes): graph {gb} B, {gn} ns/think | bit vectors (2048) {hb} B (+{} B codebook), {hn} ns/think | packed {pb} B, {pn} ns/think",
        m.g.len(), hd.codebook_bytes());
    println!("bit vectors vs graph: {}‰ of the size, {}× the time per think", hb * 1000 / gb, hn / gn.max(1));
    // Honest gap: at this degree (~8 links per node) a 2048-bit bundle per node is bigger than the
    // packed graph, and cleanup (scanning every node) is slower. The quantised packed form is the
    // one that is smaller and as fast.
    assert!(hb > gb, "documented: the bit-vector form is not smaller here");
    assert!(hn > gn, "documented: the bit-vector form is not faster on a CPU");
    assert!(pb < gb);
}

// ---------------------------------------------------------------- 6. scale

#[test]
fn o6_cost_per_think_and_bytes_per_mind_at_scale() {
    println!("{:>7} {:>8} | {:>10} {:>9} | {:>10} {:>11} {:>8} | {:>10} {:>9} | {:>9}", "nodes", "links", "graph B", "ns/think", "bits B", "ns/think", "agree", "packed B", "ns/think", "state B");
    let mut rows = vec![];
    for (n, gt, ht, cmp) in [(1_000, 4000, 200, 100), (10_000, 4000, 20, 20), (100_000, 2000, 2, 4)] {
        let r = scale::bench(n, 2048, gt, ht, cmp, 42);
        println!("{:>7} {:>8} | {:>10} {:>9} | {:>10} {:>11} {:>4}/{:<3} | {:>10} {:>9} | {:>9}", r.nodes, r.links, r.graph_bytes, r.ns_graph, r.hd_bytes, r.ns_hd, r.same_top1, r.compared, r.packed_bytes, r.ns_packed, r.state_bytes);
        println!("        per node: graph {} B, bits {} B (+{} B codebook), packed {} B; 1024-bit form {} B, same top {}/{}; compile {} ms",
            r.graph_bytes / n, r.hd_bytes / n, r.codebook_bytes / n, r.packed_bytes / n, r.hd_half_bytes / n, r.half_same_top1, r.compared, r.compile_ms);
        rows.push(r);
    }
    // The graph think is bounded (hops × frontier), so its cost barely moves with mind size.
    assert!(rows[2].ns_graph < rows[0].ns_graph * 4);
    // The bit-vector think scans every node: documented as growing with the mind.
    assert!(rows[2].ns_hd > rows[0].ns_hd * 20);
}

// ---------------------------------------------------------------- 7. viewer

#[test]
fn o7_viewer_is_one_self_contained_page_per_scenario() {
    let mut s = scenarios::meadow_fire(SEED).recording();
    s.record_from = FIRE_AT - 3;
    s.run(FIRE_AT + 4);
    let html = fv_thought_graph::viewer::page(&s, "t", "s", "graph", None, &[(FIRE_AT, Some(0), "k".into())]);
    let frames = s.frames.len();
    let hops: usize = s.frames.iter().map(|f| f.hops.len()).sum();
    println!("{} frames, {} hops recorded, page {} KB", frames, hops, html.len() / 1024);
    assert!(frames == 7 && hops >= frames * 2);
    assert!(!html.contains("<script src") && !html.contains("<link") && !html.contains("http://") || html.contains("http://www.w3.org/2000/svg"));
    assert!(!html.contains("https://"));
    assert!(html.contains("\"frames\":[{") && html.contains("fire!"));
}

// ---------------------------------------------------------------- 8. determinism

const PINNED: u64 = 0xe8a4_2f93_38b4_31fb;

fn full_run() -> u64 {
    let mut s = scenarios::meadow_fire(SEED);
    s.run(DAY2 + 80);
    s.state_hash()
}

#[test]
fn o8_identical_runs_give_identical_hashes() {
    assert_eq!(hash(1, 2, 3, 4), 0x5a6b_0e97_f4a4_11b8, "the mixer itself is pinned");
    let (a, b) = (full_run(), full_run());
    println!("two days with the dog, a night, and a meadow fire: {a:#018x} and {b:#018x}");
    assert_eq!(a, b);
    if PINNED != 0 { assert_eq!(a, PINNED, "the pinned hash moved: behaviour changed"); }
    // The compiled form too.
    let run_c = || { let mut s = scenarios::meadow_fire(SEED); s.run(DAY2); s.m.use_compiled(2048, SEED); s.run(60); s.state_hash() };
    let (c1, c2) = (run_c(), run_c());
    println!("compiled-form run: {c1:#018x} and {c2:#018x}");
    assert_eq!(c1, c2);
}

#[test]
fn o8_thread_and_brain_order_do_not_matter() {
    // Several foxes (different temperaments), each in its own world, stepped on threads spawned
    // in reverse order, against sequentially: the same hash for each.
    let ids: Vec<u32> = (0..6).collect();
    let seq: Vec<u64> = ids.iter().map(|&i| { let mut s = scenarios::dog_charges(SEED, i); s.run(20); s.state_hash() }).collect();
    let par: Vec<u64> = std::thread::scope(|sc| {
        let hs: Vec<_> = ids.iter().rev().map(|&i| sc.spawn(move || { let mut s = scenarios::dog_charges(SEED, i); s.run(20); (i, s.state_hash()) })).collect();
        let mut v: Vec<(u32, u64)> = hs.into_iter().map(|h| h.join().unwrap()).collect();
        v.sort(); v.into_iter().map(|x| x.1).collect()
    });
    println!("sequential {:x?}\nthreads    {:x?}", seq, par);
    assert_eq!(seq, par);

    // Two minds reading the same world view each tick, stepped A-then-B or B-then-A: minds read
    // the world and write only themselves, so order cannot matter.
    let w0 = { let mut s = scenarios::meadow_fire(SEED); s.run(FIRE_AT); s.w };
    let mk = |t| { let mut m = Mind::with_temperament(&w0, t); m.learn = true; m };
    let (t1, t2) = (Temperament::seeded(SEED, 1), Temperament::seeded(SEED, 4));
    let (mut a1, mut b1, mut a2, mut b2) = (mk(t1), mk(t2), mk(t1), mk(t2));
    let mut w = w0.clone();
    for _ in 0..10 {
        let (x, y) = (a1.step(&w), b1.step(&w));
        let (q, p) = (b2.step(&w), a2.step(&w));
        assert_eq!((x, y), (p, q));
        w.pre_step(); w.post_step(x);
    }
    use std::hash::{Hash, Hasher};
    let hm = |m: &Mind| { let mut h = fv_thought_graph::rng::StateHasher::new(1); m.hash(&mut h); h.finish() };
    assert_eq!(hm(&a1), hm(&a2));
    assert_eq!(hm(&b1), hm(&b2));
}

// ---------------------------------------------------------------- stress and temperament

#[test]
fn stress_and_temperament_change_how_a_fox_meets_the_dog() {
    // Two seeded individuals: the least and the most reactive of the first 32.
    let pool: Vec<(u32, Temperament)> = (0..32).map(|i| (i, Temperament::seeded(SEED, i))).collect();
    let calm = *pool.iter().min_by_key(|x| (x.1.reactivity, x.0)).unwrap();
    let quick = *pool.iter().max_by_key(|x| (x.1.reactivity + x.1.aggression, std::cmp::Reverse(x.0))).unwrap();
    let mut results = vec![];
    for (label, (id, t)) in [("calm", calm), ("quick", quick)] {
        let mut s = scenarios::dog_charges(SEED, id);
        s.run(CHARGE_AT);
        let (mut hops, mut visits, mut n) = (0, 0, 0);
        let (mut first_action, mut full_flight, mut snaps, mut peak) = (None, None, 0, 0);
        let mut trace = vec![];
        for k in 0..16 {
            let a = s.step();
            let r = &s.m.last;
            hops += r.hops_run; visits += r.visits; n += 1;
            peak = peak.max(s.m.mods.stress);
            if first_action.is_none() && !matches!(a, Action::Stay | Action::WalkTo { .. }) { first_action = Some(k); }
            if full_flight.is_none() && matches!(a, Action::Flee { speed: 3, .. }) { full_flight = Some(k); }
            if matches!(a, Action::Snap { .. }) { snaps += 1; }
            trace.push(match a { Action::Flee { speed, .. } => format!("flee{speed}"), Action::Snap { .. } => "SNAP".into(), Action::Stay => "stay".into(), _ => "walk".into() });
        }
        let bites = s.w.log.iter().filter(|l| l.1.contains("bites")).count();
        println!("{label:5} fox #{id} (reactivity {}, aggression {}): mean hops {}.{}, mean links visited {}, frontier at peak {}, peak stress {peak}",
            t.reactivity, t.aggression, hops / n, hops * 10 / n % 10, visits / n, s.m.last.frontier_cap);
        println!("      first reaction after {:?} ticks, full-speed flight after {:?}, snaps {snaps}, bitten {bites}", first_action, full_flight);
        println!("      {}", trace.join(" "));
        results.push((hops * 100 / n, visits / n, full_flight.unwrap_or(99), snaps, peak));
    }
    let (c, q) = (&results[0], &results[1]);
    assert!(q.0 < c.0, "a quick fox thinks shallower under stress");
    assert!(q.1 < c.1, "and visits fewer links per think");
    assert!(q.2 < c.2, "and reaches full-speed flight sooner");
    assert!(q.3 > c.3, "and snaps where the calm fox only runs");
}
