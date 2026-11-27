//! Static cost: the bound worked out from a description always covers what a call really costs,
//! and reach follows it (the fast brain and a frightened mind are refused expensive tools).
use fv_mind_lab::rng::hash;
use fv_mind_lab::tools::belt::{Belt, Reach, ToolConfig};
use fv_mind_lab::tools::cost::{self, Brain, Ceilings};
use fv_mind_lab::tools::kit;
use fv_mind_lab::tools::lang::*;
use fv_mind_lab::tools::maker::{build, Shape};
use fv_mind_lab::tools::run::Tool;

fn random_rec(seed: u64, i: u64) -> Rec {
    let r = |k: u64, m: u64| (hash(seed, i, k, 0xc057) % m) as i32;
    let kinds = [kind::SEEN, kind::DID, kind::TOLD, kind::SURPRISE, kind::MOVED];
    let mut x = rec_of(&[(F::Kind, kinds[r(0, 5) as usize]), (F::Who, r(1, 12)), (F::Place, r(2, 40)), (F::X, r(3, 64)), (F::Y, r(4, 40)), (F::Act, r(5, 6)),
        (F::Val, r(6, 2001) - 1000), (F::What, r(7, 300)), (F::To, r(8, 12)), (F::Next, r(9, 40)), (F::Goal, r(10, 40))]);
    if r(11, 4) == 0 { x[F::Val as usize] = NONE; }
    fv_mind_lab::tools::belt::stamp(&mut x, i as u32, 301);
    x
}

fn tools() -> Vec<Desc> {
    let mut v: Vec<Desc> = kit::FOUNDATION.iter().chain(kit::SCHEDULES.iter()).map(|s| kit::load(s)).collect();
    for both in [false, true] { for avoid in [false, true] { for journeys in [false, true] { v.push(build(F::Next, kind::MOVED, &Shape::Map { both, avoid, journeys }, 64)); } } }
    v.push(build(F::Place, kind::SEEN, &Shape::Table { feats: vec![Feat::Field(F::Who), Feat::Prev, Feat::Since(25)], cell: Cell::Votes(3), back: true }, 512));
    v
}

#[test]
fn the_bound_covers_every_call() {
    let ds = tools();
    // Small caps make tables fill, evict and stay full: the worst case is reached more often.
    for small in [false, true] {
        let mut belt_tools: Vec<Tool> = ds.iter().map(|d| { let mut d = d.clone(); if small { for s in d.slots.iter_mut() { if !matches!(s.cell, Cell::Ring(_)) { s.cap = 8; } } } Tool::from(d) }).collect();
        let env: Vec<Desc> = belt_tools.iter().map(|t| (*t.desc).clone()).collect();
        let envr: Vec<&Desc> = env.iter().collect();
        let mut worst = vec![(0i64, 0i64, 0i64, 0i64); belt_tools.len()];
        for i in 0..6000u64 {
            let r = random_rec(7 + small as u64, i);
            for (k, t) in belt_tools.iter_mut().enumerate() {
                let b = cost::bound(&t.desc, &envr);
                let used = t.observe(&r, i64::MAX / 4).unwrap();
                assert!(used <= b.observe, "{}: observe used {used} > bound {}", t.desc.name, b.observe);
                worst[k].0 = worst[k].0.max(used); worst[k].1 = b.observe;
                if i % 1000 == 999 { let u = t.forget(i as i32, i64::MAX / 4).unwrap(); assert!(u <= b.forget, "{}: forget {u} > {}", t.desc.name, b.forget); }
            }
            let q = random_rec(99, i);
            for k in 0..belt_tools.len() {
                let t = &belt_tools[k];
                let b = cost::bound(&t.desc, &envr);
                let (a, used) = t.query("", &q, i64::MAX / 4, &belt_tools);
                a.unwrap();
                assert!(used <= b.ask, "{}: ask used {used} > bound {}", t.desc.name, b.ask);
                worst[k].2 = worst[k].2.max(used); worst[k].3 = b.ask;
                for (v, _) in &t.desc.views { let vb = cost::view_bound(&t.desc, v, &envr); let (_, u) = t.query(v, &q, i64::MAX / 4, &belt_tools); assert!(u <= vb, "{} view {v}: {u} > {vb}", t.desc.name); }
            }
        }
        if !small { println!("{:>40} {:>10} {:>10} {:>10} {:>10}", "tool", "observe", "bound", "ask", "bound"); for (d, w) in ds.iter().zip(&worst) { println!("{:>40} {:>10} {:>10} {:>10} {:>10}", d.name, w.0, w.1, w.2, w.3); } }
    }
}

#[test]
fn composition_is_costed_and_works() {
    let mut b = Belt::new(ToolConfig { on: true, ..ToolConfig::default() });
    let tt = b.add(kit::load(kit::TIMETABLE));
    let rt = b.add(kit::load(kit::ROUTES));
    let gf = b.add(kit::load(kit::GO_FIND));
    // Ash is at place 7 in the mornings; from place 1 towards 7 I go by 3.
    for d in 0..4u32 { b.tell(rec_of(&[(F::Kind, kind::SEEN), (F::Who, 0), (F::Place, 7)]), d * 301 + 60); b.tell(rec_of(&[(F::Kind, kind::MOVED), (F::Place, 1), (F::Next, 3), (F::Goal, 7)]), d * 301 + 100); }
    let calm = Reach::calm(Brain::Slow);
    let (a, used) = b.ask(gf, rec_of(&[(F::Who, 0), (F::Place, 1)]), 4 * 301 + 61, calm).unwrap();
    println!("go-find: {a:?}, {used} fuel; bounds: timetable {}, routes {}, go-find {}", b.bounds[tt].ask, b.bounds[rt].ask, b.bounds[gf].ask);
    assert_eq!(a[0].0, 3);
    assert!(b.bounds[gf].ask >= b.bounds[tt].ask + b.bounds[rt].ask, "a composed tool's bound includes its sub-tools'");
    assert!(used <= b.bounds[gf].ask);
}

#[test]
fn reach_follows_cost_and_stress() {
    let mut b = Belt::new(ToolConfig { on: true, ..ToolConfig::default() });
    let cheap = b.add(kit::load(kit::ROUTES));
    let map = b.add(build(F::Next, kind::MOVED, &Shape::Map { both: true, avoid: true, journeys: false }, 512));
    let c = Ceilings::default();
    println!("routes bound {}, map bound {}; ceilings fast {} slow {}", b.bounds[cheap].ask, b.bounds[map].ask, c.fast, c.slow);
    assert!(b.reachable(cheap, Reach::calm(Brain::Fast)), "the fast brain can use a cheap tool");
    assert!(!b.reachable(map, Reach::calm(Brain::Fast)), "the fast brain is refused the map");
    assert!(b.reachable(map, Reach::calm(Brain::Slow)), "calm thought can use the map");
    // A reactive temperament's stress cut (250 + 900 × 0.7 = 880) at high stress.
    let scared = Reach { brain: Brain::Slow, stress: 900, cut: 880 };
    assert!(!b.reachable(map, scared), "a frightened mind cannot reach its map");
    assert!(b.reachable(cheap, scared), "but can still use the way it knows");
    assert!(b.reachable(map, Reach { brain: Brain::Idle, stress: 1000, cut: 1000 }), "idle time is not cut");
    assert!(b.ask(map, rec_of(&[(F::Place, 1), (F::Goal, 2)]), 5, scared).is_none());
    assert_eq!(b.stats.refused, 1);
}
