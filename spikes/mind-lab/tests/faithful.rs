//! Is the port faithful? With the three new choices switched off (the old exact arithmetic,
//! ties by array index, and founding weights held exactly instead of in steps of 4), the shared
//! mind must reproduce the original fox from `spikes/thought-graph` tick for tick: the same
//! action, position, modulators, workspace and backlog. Then each new choice is switched on alone,
//! to measure how far it moves the fox.
//! `cargo test --release --test faithful -- --nocapture`

use fv_mind_lab::fox::{scenarios as new, sim::Sim};
use fv_mind_lab::mind::Action;
use fv_thought_graph as old;

#[derive(Clone, Copy, Debug)]
struct Choices { fixed: bool, key_ties: bool, quantised: bool }
const LEGACY: Choices = Choices { fixed: false, key_ties: false, quantised: false };

fn apply(s: &mut Sim, c: Choices) {
    if !c.quantised { s.m.store = s.fox.mind_exact(s.m.temperament, s.m.who).store; }
    s.m.params.exact = !c.fixed;
    s.m.params.index_ties = !c.key_ties;
}

fn act_old(a: old::world::Action) -> String {
    use old::world::Action as A;
    match a { A::Stay => "stay".into(), A::WalkTo { x, y } => format!("walk({x},{y})"), A::Flee { from_x, from_y, speed } => format!("flee({from_x},{from_y},{speed})"),
        A::Snap { entity } => format!("snap({entity})"), A::Eat { entity } => format!("eat({entity})"), A::Sleep => "sleep".into() }
}
fn act_new(a: Action) -> String {
    match a { Action::Stay => "stay".into(), Action::WalkTo { x, y } => format!("walk({x},{y})"), Action::Flee { from_x, from_y, speed } => format!("flee({from_x},{from_y},{speed})"),
        Action::Snap { entity } => format!("snap({entity})"), Action::Eat { entity } => format!("eat({entity})"), Action::Sleep => "sleep".into() }
}

/// Run both for `ticks`; return the first tick they differ at (and how), or None.
fn compare(mut o: old::sim::Sim, mut n: Sim, ticks: u32) -> Option<(u32, String)> {
    for _ in 0..ticks {
        let t = o.w.tick;
        let (ao, an) = (act_old(o.step()), act_new(n.step()));
        let (fo, f) = (&o.w.fox, n.w.fox());
        let mo = o.m.mods;
        let mn = n.m.mods;
        let wso: Vec<String> = o.m.last.ws.iter().map(|&x| o.m.g.names[x as usize].clone()).collect();
        let wsn: Vec<String> = n.m.last.ws.iter().map(|&x| n.m.name(x)).collect();
        let a = (ao.clone(), fo.x, fo.y, fo.hunger, fo.pain, mo.hunger, mo.arousal, mo.mood, mo.fatigue, mo.stress, o.m.backlog());
        let b = (an.clone(), f.x, f.y, f.hunger, f.pain, mn.hunger, mn.arousal, mn.mood, mn.fatigue, mn.stress, n.m.backlog());
        if a != b || wso != wsn { return Some((t, format!("old {a:?} {wso:?}\n      new {b:?} {wsn:?}"))); }
    }
    None
}

type Pair = (&'static str, fn() -> (old::sim::Sim, Sim), u32);
fn cases() -> Vec<Pair> {
    let v: Vec<Pair> = vec![
        ("dog day, night, day 2", (|| (old::scenarios::dog_day(7), new::dog_day(7))) as fn() -> (old::sim::Sim, Sim), 2 * 301),
        ("meadow fire", (|| (old::scenarios::meadow_fire(7), new::meadow_fire(7))) as fn() -> (old::sim::Sim, Sim), 301 + 120),
        ("busy day", (|| (old::scenarios::busy_day(7), new::busy_day(7))) as fn() -> (old::sim::Sim, Sim), 301 + 50),
        ("sleepless eventful, 3 days", (|| (old::scenarios::sleepless_eventful_days(7, 3), new::sleepless_eventful_days(7, 3))) as fn() -> (old::sim::Sim, Sim), 3 * 301),
        ("home moved near, 6 days", (|| (old::scenarios::home_moved(7, old::scenarios::HomeMove::Near, 6), new::home_moved(7, new::HomeMove::Near, 6))) as fn() -> (old::sim::Sim, Sim), 6 * 301),
        ("home removed, 6 days", (|| (old::scenarios::home_moved(7, old::scenarios::HomeMove::Removed, 6), new::home_moved(7, new::HomeMove::Removed, 6))) as fn() -> (old::sim::Sim, Sim), 6 * 301),
        ("home moved daily, 8 days", (|| (old::scenarios::home_moved(7, old::scenarios::HomeMove::Daily, 8), new::home_moved(7, new::HomeMove::Daily, 8))) as fn() -> (old::sim::Sim, Sim), 8 * 301),
        ("low mood at den", (|| (old::scenarios::mood_at(7, "den"), new::mood_at(7, "den"))) as fn() -> (old::sim::Sim, Sim), 80),
        ("dog charges #3", (|| (old::scenarios::dog_charges(7, 3), new::dog_charges(7, 3))) as fn() -> (old::sim::Sim, Sim), 20),
        ("dog charges #26 (calm)", (|| (old::scenarios::dog_charges(7, 26), new::dog_charges(7, 26))) as fn() -> (old::sim::Sim, Sim), 20),
        ("dog charges #11", (|| (old::scenarios::dog_charges(7, 11), new::dog_charges(7, 11))) as fn() -> (old::sim::Sim, Sim), 20),
    ];
    v
}

#[test]
fn legacy_settings_reproduce_the_original_fox_tick_for_tick() {
    let mut bad = vec![];
    for (name, mk, ticks) in cases() {
        let (o, mut n) = mk();
        apply(&mut n, LEGACY);
        let r = compare(o, n, ticks);
        println!("{name:28} {ticks:5} ticks: {}", match &r { None => "identical".to_string(), Some((t, d)) => format!("DIFFERS at tick {t}\n      {d}") });
        if r.is_some() { bad.push(name); }
    }
    assert!(bad.is_empty(), "the port diverges from the original in {bad:?}");
}

#[test]
fn each_new_choice_alone_how_far_it_moves_the_fox() {
    let variants = [
        ("fixed-point gain only", Choices { fixed: true, ..LEGACY }),
        ("key ties only", Choices { key_ties: true, ..LEGACY }),
        ("u8 founding weights only", Choices { quantised: true, ..LEGACY }),
        ("all three (the shared mind)", Choices { fixed: true, key_ties: true, quantised: true }),
    ];
    for (vn, c) in variants {
        println!("{vn}:");
        for (name, mk, ticks) in cases() {
            let (o, mut n) = mk();
            apply(&mut n, c);
            let r = compare(o, n, ticks);
            let (mut o, mut n) = mk();
            apply(&mut n, c);
            let (mut same, mut first) = (0, None);
            for t in 0..ticks { if act_old(o.step()) == act_new(n.step()) { same += 1; } else if first.is_none() { first = Some(t); } }
            println!("  {name:28}: full state {}; same action on {same} of {ticks} ticks, first different action {first:?}",
                match r { None => format!("identical over {ticks} ticks"), Some((t, _)) => format!("first differs at tick {t}") });
        }
    }
}

/// The same check across seeds and temperaments (the legacy settings stand in for the original
/// fox in tests/sensitivity.rs, so they must match it there too).
#[test]
fn legacy_matches_the_original_across_seeds_and_temperaments() {
    let mut runs = 0; let mut ticks_all = 0;
    for seed in [7u64, 11, 23, 42] {
        for who in [None, Some(2u32), Some(9), Some(18), Some(26)] {
            let t = who.map(|i| fv_mind_lab::mind::Temperament::seeded(7, i));
            let mk: [(&str, Box<dyn Fn() -> (old::sim::Sim, Sim)>, u32); 4] = [
                ("dog day", Box::new(move || (old::scenarios::dog_day(seed), new::dog_day(seed))), 2 * 301),
                ("home near", Box::new(move || (old::scenarios::home_moved(seed, old::scenarios::HomeMove::Near, 6), new::home_moved(seed, new::HomeMove::Near, 6))), 6 * 301),
                ("home mid", Box::new(move || (old::scenarios::home_moved(seed, old::scenarios::HomeMove::Mid, 6), new::home_moved(seed, new::HomeMove::Mid, 6))), 6 * 301),
                ("home daily", Box::new(move || (old::scenarios::home_moved(seed, old::scenarios::HomeMove::Daily, 6), new::home_moved(seed, new::HomeMove::Daily, 6))), 6 * 301),
            ];
            for (name, f, ticks) in mk.iter() {
                let (mut o, mut n) = f();
                if let Some(t) = t {
                    let ot = old::mind::Temperament { reactivity: t.reactivity, aggression: t.aggression };
                    o.m = old::mind::Mind::with_temperament(&o.w, ot);
                    n.m = n.fox.mind(t, seed);
                }
                apply(&mut n, LEGACY);
                let r = compare(o, n, *ticks);
                assert!(r.is_none(), "seed {seed} {who:?} {name}: {:?}", r);
                runs += 1; ticks_all += ticks;
            }
        }
    }
    println!("{runs} runs, {ticks_all} ticks: legacy settings identical to the original fox on every tick");
}

/// Decisions only: how often the shared mind (all three choices on) takes the same action as the
/// original fox, tick by tick, before and after their paths first part.
#[test]
fn shared_mind_against_the_original_decision_by_decision() {
    for (name, mk, ticks) in cases() {
        let (mut o, mut n) = mk();
        let (mut same, mut first) = (0, None);
        for t in 0..ticks {
            let (a, b) = (act_old(o.step()), act_new(n.step()));
            if a == b { same += 1; } else if first.is_none() { first = Some(t); }
        }
        let pos = (o.w.fox.x, o.w.fox.y) == (n.w.fox().x, n.w.fox().y);
        println!("{name:28}: same action on {same} of {ticks} ticks ({}‰); first different action at {:?}; same place at the end {pos}", same * 1000 / ticks, first);
    }
}
