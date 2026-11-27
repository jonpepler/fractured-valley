//! Storage and performance of the shared mind (stage 1, item 6): bytes per mind (shared base,
//! own overlay, fast weights, state) and µs per tick, per think and per consolidation, at fox
//! size and at 10k and 100k nodes, on the runtime form against the expanded form. Learning is
//! measured on the runtime form, which the compaction lab never did.
//! Timings are wall-clock minimums or means on a shared VM: read them as orders of magnitude.
//! `cargo test --release --test storage -- --nocapture --test-threads=1`

use fv_mind_lab::fox::scenarios::{DAY, NIGHT};
use fv_mind_lab::fox::sim::Sim;
use fv_mind_lab::fox::world::Event;
use fv_mind_lab::graph::lt;
use fv_mind_lab::mind::{Mind, Temperament, THINK_BUDGET};
use fv_mind_lab::scale::big_fox;
use fv_mind_lab::store::{Runtime, Store};
use fv_mind_lab::think::{self, Activation, Form, Mods};
use std::time::Instant;

/// Two days: an eventful first (a fire in the afternoon, then the dog charges), a night, and a
/// second with another fire, so the mind has something to learn and consolidate at every size.
fn day(sim: &mut Sim) {
    sim.w.foxes[0].hunger = 700;
    for (t, e) in [(160, Event::FireAt { x: 10, y: 24 }), (200, Event::FiresOut), (220, Event::DogChase), (NIGHT, Event::Night), (NIGHT + 1, Event::SetHunger(700)), (DAY + 160, Event::FireAt { x: 35, y: 16 }), (DAY + 200, Event::FiresOut)] { sim.at(t, e); }
}

fn think_ns<F: Form>(f: &F, m: &Mind, n: u32, exact: bool) -> f64 {
    let p = fv_mind_lab::think::Params { exact, ..m.params };
    let mut st = Activation::new(f.len());
    let inj = [(m.roles.hunger, 400), (m.node("den"), 300), (m.node("fire"), 600)];
    let mods = Mods { hunger: 700, arousal: 300, mood: -100, fatigue: 100, stress: 0, ..Mods::default() };
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t0 = Instant::now();
        for _ in 0..n { std::hint::black_box(think::think(f, &mut st, &mods, &inj, &p, None)); }
        best = best.min(t0.elapsed().as_nanos() as f64 / n as f64);
    }
    best
}

/// Minimum over 5 passes of the mean time of `f`, each call on its own fresh clone of `m` (the
/// clones are made before timing; many big clones walked in turn are cache-cold).
fn on_clone<S: Store>(m: &Mind<S>, reps: u32, f: impl Fn(&mut Mind<S>)) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..5 {
        let mut ms: Vec<Mind<S>> = (0..reps).map(|_| m.clone()).collect();
        let t0 = Instant::now();
        for x in ms.iter_mut() { f(x); }
        best = best.min(t0.elapsed().as_nanos() as f64 / reps as f64);
    }
    best
}

#[test]
fn storage_and_cost_at_fox_size_10k_and_100k() {
    println!("| nodes | links | base (shared) B | per node | overlay newborn | overlay after 2 days | fast B | state B | expanded B | per extra mind (own+fast+state) |");
    let mut perf = vec![];
    for extra in [0usize, 10_000 - 27, 100_000 - 27] {
        let t0 = Instant::now();
        let fox = big_fox(extra, 42);
        let build_ms = t0.elapsed().as_millis();
        let base = fox.species.base.clone();
        let n = base.len();
        let newborn = fox.mind(Temperament::MEDIUM, 1);
        let mut s = Sim::with_fox(7, Temperament::MEDIUM, fox.clone());
        day(&mut s);
        let mut x = Sim { w: s.w.clone(), m: s.m.expand(), fox: s.fox.clone(), record: false, frames: vec![], nights: vec![], record_from: 0, snapshots: vec![] };
        let (mut step_rt, mut step_ex, mut steps) = (0u128, 0u128, 0u128);
        let mut night_mind: Option<Mind<Runtime>> = None;
        let mut tick_max = 0u128;
        let mut actions_same = true;
        let (mut gen_in_ws, mut ws_n) = (0usize, 0usize);
        for t in 0..2 * DAY {
            if t == NIGHT { night_mind = Some(s.m.clone()); }
            let t0 = Instant::now(); let a = s.step(); let d = t0.elapsed().as_nanos(); step_rt += d; tick_max = tick_max.max(d);
            let t1 = Instant::now(); let b = x.step(); step_ex += t1.elapsed().as_nanos();
            steps += 1;
            if a != b { actions_same = false; }
            gen_in_ws += s.m.last.ws.iter().filter(|&&q| q as usize >= 27).count(); ws_n += s.m.last.ws.len();
        }
        let same_hash = s.m.state_hash() == x.m.state_hash();
        assert!(actions_same && same_hash, "{n} nodes: the two forms must agree");
        let (sh, own, fast, st) = s.m.bytes();
        let ex = s.m.store.expand();
        println!("| {n} | {} | {sh} | {:.1} | {} | {own} | {fast} | {st} | {} | {} |", base.links, sh as f64 / n as f64, newborn.bytes().1, ex.graph_bytes() + ex.fast.len() * 2, own + fast + st);
        // Costs.
        let m = &s.m;
        let (th_rt, th_ex, th_old) = (think_ns(&m.store, m, 2000, false), think_ns(&ex, m, 2000, false), think_ns(&ex, m, 2000, true));
        let nm = night_mind.unwrap();
        let backlog_ns = on_clone(&nm, 200, |c| { std::hint::black_box(c.backlog()); });
        println!("|   | one backlog() call: {:.1} µs with {} episodes held (a tick calls it about 5 times) |", backlog_ns / 1000.0, nm.episodes.len());
        let backlog = nm.backlog();
        let units = THINK_BUDGET - 170;
        // Idle consolidation of fast weights alone (the episodes set aside), and of the backlog as it
        // is at nightfall (the strongest episode is replayed first, if the budget covers it).
        let mut nf = nm.clone(); nf.episodes.clear();
        let idle = on_clone(&nf, 200, |c| { c.consolidate(units, false); });
        let idle_ex = on_clone(&nf.expand(), 200, |c| { c.consolidate(units, false); });
        let replay = on_clone(&nm, 200, |c| { c.consolidate(units, false); });
        let replay_ex = on_clone(&nm.expand(), 200, |c| { c.consolidate(units, false); });
        println!("|   | idle consolidation at nightfall with its {} episodes: {:.2} µs runtime ({:.2} expanded) |", nm.episodes.len(), replay / 1000.0, replay_ex / 1000.0);
        let night = on_clone(&nm, 5, |c| c.sleep());
        let night_ex = on_clone(&nm.expand(), 5, |c| c.sleep());
        // Adding a told fact: one node and four links.
        let (water, stream) = (m.node("water"), m.node("stream"));
        let add = |st: &mut dyn FnMut(u32, u32, u8, i32) | { st(water, 0, lt::ASSOC, 400); };
        let _ = add;
        let told_rt = on_clone(m, 50, |c| { let sp = c.store.add_node("spring", 100, 0, None, 9); for (u, v, ty) in [(water, sp, lt::ASSOC), (sp, water, lt::KIND), (sp, stream, lt::AT), (stream, sp, lt::HOLDS)] { c.store.add_link(u, v, ty, 400, 0); } });
        let mx = m.expand();
        let told_ex = on_clone(&mx, 5, |c| { let sp = c.store.add_node("spring", 100, 0, None, 9); for (u, v, ty) in [(water, sp, lt::ASSOC), (sp, water, lt::KIND), (sp, stream, lt::AT), (stream, sp, lt::HOLDS)] { c.store.add_link(u, v, ty, 400, 0); } });
        let bites = s.w.log.iter().filter(|l| l.1.contains("bites")).count();
        println!("|   | {n} nodes: generated knowledge is {}‰ of the workspace; bitten {bites}; {} episodes replayed; overlay {} reweights, {} valence changes |", gen_in_ws * 1000 / ws_n.max(1), s.m.consol.replays, s.m.store.ov.slow.len(), s.m.store.ov.lval.len());
        perf.push((n, build_ms, step_rt / steps, step_ex / steps, tick_max, th_rt, th_ex, idle, idle_ex, night, night_ex, backlog, told_rt, told_ex, th_old));
    }
    println!();
    println!("| nodes | base build ms | µs per tick, runtime (expanded) | slowest tick µs | µs per think, runtime (expanded) [expanded, old exact arithmetic] | µs per idle consolidation of fast weights (expanded) | µs per night's sleep (expanded) | backlog at nightfall | µs to add a told fact, runtime (expanded) |");
    for p in &perf {
        println!("| {} | {} | {:.1} ({:.1}) | {:.0} | {:.2} ({:.2}) [{:.2}] | {:.2} ({:.2}) | {:.0} ({:.0}) | {} | {:.2} ({:.1}) |",
            p.0, p.1, p.2 as f64 / 1000.0, p.3 as f64 / 1000.0, p.4 as f64 / 1000.0, p.5 / 1000.0, p.6 / 1000.0, p.14 / 1000.0, p.7 / 1000.0, p.8 / 1000.0, p.9 / 1000.0, p.10 / 1000.0, p.11, p.12 / 1000.0, p.13 / 1000.0);
    }
}
