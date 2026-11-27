//! The village: its valley for a seed, its people and animals, a month of days, and what is
//! measured along the way.
//!
//! **Orchestration** (everything here that is not a mind's own choice, listed in the stage note):
//! the valley's layout, who knows what, who feels what for whom, each person's temperament and
//! character, all from the seed; the clock (dusk and night each day); on some seeds the spring
//! dries on a seeded day; bushes regrow, wounds heal one a night and a caught deer is back on its
//! meadow next morning (world rules); and the tier each mind is in (the engine's choice). Nothing
//! else is scripted: no one is placed, told, led or made thirsty after the start.

use super::fox;
use super::life::{Life, Stats};
use crate::danger::boar;
use crate::hunt::{deer, hunter};
use crate::mind::{Temperament, IGNORE_BELOW};
use crate::people::person::{Person, Traits};
use crate::people::scenarios::{DAY, DUSK, NIGHT};
use crate::people::sim::Valley;
use crate::people::species::{village_people, Founding, CALL_HABIT};
use crate::people::world::{self as pw, Beast, Bush, Deer, Event, FoxBody, Kind, Thing, VillageState, World, CAMP, GRID_H, GRID_W, OAK, ROCK, SPRING, STREAM, WINDMILL};
use crate::rng::hash;
use crate::store::Store;
use crate::think::Form;

/// What to build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    pub seed: u64,
    pub people: usize,
    pub days: u32,
    /// The cheap tier (7.7): minds further than `far` cells from the observer think every `every`
    /// ticks (`every` 1: everyone in full detail).
    pub every: u32,
    pub far: i32,
    /// Where the observer (a player) stands: the camp (`None`) or a point.
    pub observer: Option<(i32, i32)>,
    pub foxes: usize,
    pub boar: bool,
    pub deer: bool,
    pub gossip: bool,
    /// Stage 5's principled place pruning (`Config::prune_places`).
    pub prune: bool,
    /// The voice model (off: the old falloff, ordinary speech heard to about 27 cells).
    pub voice: bool,
    /// The two surprise fixes, `expect_at_edge` and `habituate` (off: an ablation).
    pub fixes: bool,
    /// Round 2 (9.5): every mind (people and animals) thinks every this many ticks.
    pub think_every: u32,
}
impl Setup {
    pub fn new(seed: u64, people: usize, days: u32) -> Setup {
        Setup { seed, people, days, every: 1, far: 16, observer: None, foxes: 3, boar: true, deer: true, gossip: true, prune: false, voice: true, fixes: true, think_every: 1 }
    }
}

/// The seed's valley: where everything is (every seed a different valley).
#[derive(Clone, Debug)]
pub struct Layout {
    pub camp: (i32, i32), pub stream: (i32, i32), pub spring: (i32, i32), pub rock: (i32, i32), pub windmill: (i32, i32), pub oak: (i32, i32),
    pub bushes: Vec<(i32, i32)>, pub meadows: Vec<(i32, i32)>, pub wallow: (i32, i32), pub dens: Vec<(i32, i32)>,
    /// The day the spring dries (if it does).
    pub dries: Option<u32>,
}
pub const BUSH0: u32 = 20;
pub const BUSHES: usize = 6;

pub fn layout(seed: u64) -> Layout {
    let r = |k: u64, lo: i32, hi: i32| lo + (hash(seed, k, 0, 0x7111) % (hi - lo + 1) as u64) as i32;
    let camp = (r(1, 7, 15), r(2, 14, 26));
    let stream = (r(3, 52, 60), r(4, 30, 37));
    let spring = (r(5, 30, 48), r(6, 3, 9));
    let rock = (r(7, 24, 38), r(8, 12, 28));
    let windmill = (r(9, 40, 56), r(10, 13, 26));
    let oak = (r(11, 16, 30), r(12, 3, 11));
    let fixed = [camp, stream, spring, rock, windmill, oak];
    let mut bushes: Vec<(i32, i32)> = vec![];
    let mut k = 100;
    while bushes.len() < BUSHES {
        let b = (r(k, 4, GRID_W - 5), r(k + 1, 3, GRID_H - 4));
        k += 2;
        if fixed.iter().chain(bushes.iter()).all(|&(x, y)| pw::dist(x, y, b.0, b.1) >= 6) { bushes.push(b); }
    }
    let mc = (r(20, 40, 56), r(21, 26, 35));
    let meadows = vec![(mc.0 - 2, mc.1), (mc.0 + 2, mc.1 - 1)];
    let wallow = (r(22, 50, 60), r(23, 2, 8));
    let dens = (0..4).map(|d| (r(30 + d, 3, GRID_W - 4), r(40 + d, 2, GRID_H - 3))).collect();
    let dries = (r(24, 0, 1) == 1).then(|| r(25, 6, 20) as u32);
    Layout { camp, stream, spring, rock, windmill, oak, bushes, meadows, wallow, dens, dries }
}

/// Pairwise feelings from the seed: warmth[i][j] and trust[i][j].
pub fn feelings(seed: u64, n: usize) -> (Vec<Vec<i32>>, Vec<Vec<i32>>) {
    const WARM: [i32; 8] = [-400, -200, 0, 100, 200, 300, 400, 600];
    const TRUST: [i32; 4] = [300, 500, 650, 800];
    let r = |a: usize, b: usize, s: u64, m: u64| (hash(seed, (a * 1000 + b) as u64, s, 0xfee1) % m) as usize;
    let warmth = (0..n).map(|i| (0..n).map(|j| if i == j { 0 } else { WARM[r(i, j, 1, 8)] }).collect()).collect();
    let trust = (0..n).map(|i| (0..n).map(|j| if i == j { 0 } else { TRUST[r(i, j, 2, 4)] }).collect()).collect();
    (warmth, trust)
}

/// Build the village for a setup.
pub fn build(s: &Setup) -> Valley {
    let l = layout(s.seed);
    let n = s.people;
    // (Review: a person's look and gossip's "about" are bytes; keep well inside them.)
    assert!((1..=200).contains(&n), "a village holds 1 to 200 people");
    let mut w = World::valley(s.seed);
    for t in w.things.iter_mut() {
        let p = match t.id { CAMP => l.camp, STREAM => l.stream, SPRING => l.spring, ROCK => l.rock, WINDMILL => l.windmill, OAK => l.oak, _ => (t.x, t.y) };
        t.x = p.0; t.y = p.1;
    }
    for (b, &(x, y)) in l.bushes.iter().enumerate() { w.things.push(Thing { id: BUSH0 + b as u32, kind: Kind::Berry, name: format!("bush {}", b + 1), x, y, alive: true }); }
    let r = |k: u64, m: u64| hash(s.seed, k, 7, 0x7112) % m;
    w.people = (0..n).map(|i| {
        let (x, y) = ((l.camp.0 + r(i as u64 * 2, 9) as i32 - 4).clamp(0, GRID_W - 1), (l.camp.1 + r(i as u64 * 2 + 1, 9) as i32 - 4).clamp(0, GRID_H - 1));
        pw::Person { x, y, px: x, py: y, thirst: r(500 + i as u64, 400) as i32, asleep: false, drinks: 0, pain: 0, gored: 0, first_drink: None, away: false }
    }).collect();
    w.meadows = l.meadows.clone();
    let foxes = (0..s.foxes).map(|k| { let (x, y) = l.dens[k % l.dens.len()]; FoxBody { x, y, px: x, py: y, hunger: 300, asleep: false, meals: 0 } }).collect();
    w.village = Some(Box::new(VillageState {
        hunger: (0..n).map(|i| r(700 + i as u64, 400) as i32).collect(), meals: vec![0; n],
        bushes: (0..BUSHES).map(|b| Bush { id: BUSH0 + b as u32, servings: 4, max: 4, regrow: 40 }).collect(),
        foxes, voice: s.voice, heal: true, deer_home: if s.deer { vec![l.meadows[0]] } else { vec![] }, feasts: vec![],
        boar_hunger: if s.boar { vec![300] } else { vec![] }, boar_meals: if s.boar { vec![0] } else { vec![] },
    }));
    if s.deer { let m0 = l.meadows[0]; w.deer.push(Deer { x: m0.0, y: m0.1, px: m0.0, py: m0.1, hunger: 300, asleep: false, caught: None, ran: 0, missed: 0, escaped: None }); }
    if s.boar { let (x, y) = l.wallow; w.boars.push(Beast { x, y, px: x, py: y, pain: 0, asleep: false, struck: 0, braced: 0, gored: 0, routed: None }); }
    if let Some(d) = l.dries { w.script.push((d * DAY + 30, Event::DrySpring)); w.script.push((d * DAY + 31 + 3 * DAY, Event::AgeHollow)); }
    for d in 0..s.days { w.script.push((d * DAY + DUSK, Event::Dusk)); w.script.push((d * DAY + NIGHT, Event::Night)); }

    let sp = village_people(n);
    let (warmth, trust) = feelings(s.seed, n);
    let landmarks = [STREAM, SPRING, ROCK, WINDMILL, OAK];
    let chance = [850u64, 400, 600, 600, 600];
    let folk: Vec<Person> = (0..n).map(|i| {
        let h = |k: u64, m: u64| hash(s.seed, i as u64, k, 0x7113) % m;
        let mut knows: Vec<u32> = landmarks.iter().zip(chance).enumerate().filter(|(k, (_, c))| h(*k as u64, 1000) < *c).map(|(_, (&wd, _))| wd).collect();
        let nb = 2 + h(10, 3) as usize;
        let mut bs: Vec<u32> = (0..BUSHES as u32).collect();
        bs.sort_by_key(|&b| h(20 + b as u64, 1 << 30));
        knows.extend(bs.into_iter().take(nb).map(|b| BUSH0 + b));
        let ally: Vec<i32> = warmth[i].iter().map(|&x| if x >= 400 { 300 } else { 0 }).collect();
        // A quarter have learnt (their history) to cry out when in danger, as stage 3's caller had.
        let calls = if h(50, 4) == 0 { 700 } else { CALL_HABIT };
        let f = Founding { knows, pacing: 2, trust: trust[i].clone(), warmth: warmth[i].clone(), spring_dry: false, dry_conf: 900, ally, calls };
        let t = Temperament::seeded(s.seed ^ 0x5e7, i as u32);
        let mut p = Person::new_in(&sp, &w, i, t, &f);
        p.traits = Traits { honesty: if h(30, 10) == 0 { 300 } else { 900 }, helpful: 500 + h(31, 501) as i32 };
        p.m.cfg.prune_places = s.prune;
        p.m.cfg.expect_at_edge = s.fixes;
        p.m.cfg.food_contingent = true;
        p.m.cfg.felt_without_sources = true;
        p.m.cfg.habituate = s.fixes;
        // Everyone knows how to hunt; a third know where the deer grazes. A deer is food.
        let knows_meadow = s.deer && h(40, 3) == 0;
        hunter::found(&mut p, &w, if knows_meadow { Some(l.meadows[0]) } else { None }, 2);
        let dk = p.hunt.as_ref().unwrap().k.deer_kind;
        p.m.store.add_link(dk, sp.ids.food, crate::graph::lt::ASSOC, 500, 0);
        p.m.young_from = p.m.store.len() as u32;
        p.village = Some(Box::new(Life { every: 1, base_every: s.think_every.max(1), no_gossip: !s.gossip, ..Life::default() }));
        p
    }).collect();
    let mut v = Valley::new(w, folk);
    if s.deer { v.deer_minds = vec![deer::mind_in(&deer::deer_for(n), &v.w, 0)]; }
    if s.boar {
        let (x, y) = l.wallow;
        let mut near: Vec<(i32, i32)> = l.bushes.clone();
        near.sort_by_key(|&(bx, by)| (pw::d2(bx, by, x, y), bx, by));
        v.beasts = vec![boar::forager(&boar::boars_for(n), &v.w, 0, x, y, &near[..2])];
    }
    for m in v.deer_minds.iter_mut().chain(v.beasts.iter_mut()) { m.cfg.expect_at_edge = s.fixes; m.cfg.food_contingent = true; m.cfg.habituate = s.fixes; }
    let fx = fox::foxes(n);
    v.fox_minds = (0..s.foxes).map(|k| {
        let den = l.dens[k % l.dens.len()];
        let mut near: Vec<(i32, i32)> = l.bushes.clone();
        near.sort_by_key(|&(x, y)| (pw::d2(x, y, den.0, den.1), x, y));
        let mut m = fox::mind(&fx, &v.w, k, den, &near[..2]);
        m.cfg.expect_at_edge = s.fixes; m.cfg.food_contingent = true; m.cfg.habituate = s.fixes;
        m
    }).collect();
    // Round 2 (9.5): every mind at the setup's thinking rate, staggered.
    for (k, m) in v.deer_minds.iter_mut().chain(v.beasts.iter_mut()).chain(v.fox_minds.iter_mut()).enumerate() { m.every = s.think_every.max(1); m.phase = k as u32; }
    v
}

/// Set each villager's tier for this tick: far from the observer, the cheap tier.
pub fn set_tiers(v: &mut Valley, s: &Setup) {
    let obs = s.observer.unwrap_or_else(|| { let c = v.w.thing(CAMP); (c.x, c.y) });
    for (i, p) in v.folk.iter_mut().enumerate() {
        let q = &v.w.people[i];
        let far = pw::d2(q.x, q.y, obs.0, obs.1) > s.far * s.far;
        if let Some(l) = p.village.as_mut() { l.every = if s.every > 1 && far { s.every } else { 1 }; }
    }
}

/// What the run recorded, per person and per day.
#[derive(Clone, Debug, Default)]
pub struct Record {
    pub n: usize,
    /// Per person: ticks at each place class (`PLACE_CLASSES`).
    pub at: Vec<[u32; PLACE_CLASSES]>,
    /// Per pair (i × n + j): ticks within 3 cells of each other, awake.
    pub close: Vec<u32>,
    /// Per person: cells walked.
    pub walked: Vec<u32>,
    /// Utterances by kind (`SAY_KINDS`), per person.
    pub said: Vec<[u32; SAY_KINDS]>,
    /// Per day: (drinks, meals) summed over people, gossip, help calls, plans proposed, deer caught,
    /// gorings, ticks surprised (all people).
    pub day: Vec<DayRec>,
    /// Storage per person, before and after each night.
    pub before: Vec<Vec<Storage>>,
    pub after: Vec<Vec<Storage>>,
    /// Where each person slept each night (place class).
    pub slept: Vec<Vec<u8>>,
    pub hash: u64,
    pub brain_ns: u64,
    pub ticks: u32,
    /// Per person: ticks in a danger response, ticks surprised.
    pub danger: Vec<u32>,
    pub surprised: Vec<u32>,
    /// Ticks on which the mind thought and was in a danger response (the cheap tier's body keeps
    /// its last response between thinks, which `danger` counts).
    pub danger_thought: Vec<u32>,
    /// Words heard per person per awake tick (summed), and utterances in the air (summed over ticks).
    pub heard: u64,
    pub air: u64,
    /// Round 2: per night, after sleep, over every ordered pair: (mean trust, mean warmth, share of
    /// warmth at or above 900, share at or below −900, dormant nodes went cold / came back in all).
    pub feel: Vec<Feel>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Feel { pub trust: f64, pub warmth: f64, pub hot: f64, pub cold: f64, pub went_cold: u32, pub revived: u32 }
/// Round 2: feelings over every ordered pair of villagers, and dormancy totals.
pub fn feel(v: &Valley) -> Feel {
    let n = v.folk.len();
    let (mut t, mut w, mut hot, mut cold, mut k) = (0i64, 0i64, 0, 0, 0);
    for p in &v.folk { for j in 0..n { if j == p.i { continue; } t += p.trust(j) as i64; let x = p.warmth(j); w += x as i64; if x >= 900 { hot += 1; } if x <= -900 { cold += 1; } k += 1; } }
    let k = k.max(1) as f64;
    let (went_cold, revived) = v.folk.iter().fold((0, 0), |a, p| (a.0 + p.m.dormancy.0, a.1 + p.m.dormancy.1));
    Feel { trust: t as f64 / k, warmth: w as f64 / k, hot: hot as f64 / k, cold: cold as f64 / k, went_cold, revived }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct DayRec { pub drinks: u32, pub meals: u32, pub gossip: u32, pub help: u32, pub proposals: u32, pub caught: u32, pub gored: u32, pub surprised: u32, pub said: u32, pub feast: u32 }

pub const PLACE_CLASSES: usize = 8;
pub const PLACE_NAMES: [&str; PLACE_CLASSES] = ["camp", "stream", "spring", "bushes", "landmarks", "meadow", "wallow", "elsewhere"];
pub const SAY_KINDS: usize = 8;
pub const SAY_NAMES: [&str; SAY_KINDS] = ["ask water", "tell / remind", "thanks", "correct", "help", "plans", "gossip", "other"];

pub fn say_kind(s: &crate::people::talk::Say) -> usize {
    use crate::people::talk::Say::*;
    match s { AskWater => 0, Tell(_) | Remind(_) | TellOther(..) => 1, Thanks => 2, Correct(_) => 3, Help => 4, Propose(_) | Agree(_) | Decline(_) | Signal | Caught | Warn(_) => 5, Gossip { .. } => 6, AskOther(_) => 7 }
}

/// The place class of a cell (for measures only: this reads the world, a mind never does).
pub fn place_class(w: &World, l: &Layout, x: i32, y: i32) -> usize {
    let near = |p: (i32, i32), r: i32| pw::d2(x, y, p.0, p.1) <= r * r;
    if near(l.camp, 5) { 0 } else if near(l.stream, 4) { 1 } else if near(l.spring, 4) { 2 } else if l.bushes.iter().any(|&b| near(b, 3)) { 3 }
    else if [l.rock, l.windmill, l.oak].iter().any(|&b| near(b, 4)) { 4 } else if l.meadows.iter().any(|&m| near(m, 5)) { 5 } else if near(l.wallow, 5) { 6 } else { let _ = w; 7 }
}

/// One mind's storage, by part (bytes).
#[derive(Clone, Copy, Debug, Default)]
pub struct Storage {
    /// The species base's bytes divided among the minds that share it.
    pub base_share: usize,
    /// Own overlay, by part: places (own place nodes and every link touching one), people (links
    /// touching one's nodes for others, not sources), source links, plans (the hunting layer's
    /// nodes and links), notes, everything else of its own (node valence, anchors, sparse
    /// reweights, other added links).
    pub places: usize, pub people: usize, pub sources: usize, pub plans: usize, pub notes: usize, pub other_own: usize,
    pub fast: usize,
    /// The mind's state between ticks (active list, expectation tables, episodes, the last report).
    pub state: usize,
    /// Of `state`: the expectation tables (places × kinds × 2 × 2 B).
    pub expect: usize,
    /// The people layer's records (told claims, notes, model of others) and the hunting layer.
    pub layer: usize,
    pub nodes: usize, pub links: usize, pub place_count: usize, pub source_links: usize, pub told: usize,
    /// Round 2 (9.8): cold storage (dormant nodes), not part of `total` (the running mind).
    pub cold: usize, pub cold_nodes: usize,
}
impl Storage {
    pub fn own(&self) -> usize { self.places + self.people + self.sources + self.plans + self.notes + self.other_own }
    pub fn total(&self) -> usize { self.base_share + self.own() + self.fast + self.state + self.layer }
}

pub fn storage(p: &Person, sharers: usize) -> Storage {
    use crate::graph::{lt, tag};
    use crate::think::Form;
    let m = &p.m;
    let st = &m.store;
    let (shared, own, fast, state) = m.bytes();
    let link_b = std::mem::size_of::<crate::store::Added>();
    let is_person = |n: u32| p.pnode.contains(&n);
    let plan_nodes: Vec<u32> = p.hunt.as_ref().map(|h| { let k = h.k; vec![k.deer_kind, k.deer, k.drive, k.wait, k.signal, k.broken, k.betrayal] }).unwrap_or_default();
    let is_plan = |n: u32| plan_nodes.contains(&n) || (n as usize >= st.base.len() && st.name(n).starts_with("plan"));
    let is_note = |n: u32| p.notes.iter().any(|x| x.1 == n);
    let mut s = Storage { base_share: shared / sharers.max(1), fast, state, ..Storage::default() };
    for a in &st.ov.added {
        let (u, v) = (a.src, a.to);
        let place = st.tags(u) & tag::PLACE != 0 || st.tags(v) & tag::PLACE != 0;
        if a.ty == lt::SOURCE { s.sources += link_b; s.source_links += 1; }
        else if is_plan(u) || is_plan(v) { s.plans += link_b; }
        else if is_note(u) || is_note(v) { s.notes += link_b; }
        else if place { s.places += link_b; }
        else if is_person(u) || is_person(v) { s.people += link_b; }
        else { s.other_own += link_b; }
    }
    // Own nodes: 5 B each plus 2 B of valence, plus an anchor if any; by the same classes.
    let anchor_b = std::mem::size_of::<(u32, crate::graph::Anchor)>();
    for n in st.base.len() as u32..st.len() as u32 {
        let b = 7 + if st.anchor(n).is_some() { anchor_b } else { 0 };
        if st.tags(n) & tag::PLACE != 0 { s.places += b; } else if is_plan(n) { s.plans += b; } else if is_note(n) { s.notes += b; } else if is_person(n) { s.people += b; } else { s.other_own += b; }
    }
    let counted = s.own();
    s.other_own += own.saturating_sub(counted);
    s.expect = m.expect.iter().map(|e| e.len() * 2).sum();
    s.layer = p.told.len() * std::mem::size_of::<crate::people::person::Told>() + p.notes.len() * 12 + p.lex.len() * 8 + p.pnode.len() * 8
        + p.hunt.as_ref().map_or(0, |_| std::mem::size_of::<hunter::Hunter>()) + p.village.as_ref().map_or(0, |_| std::mem::size_of::<Life>()) + p.heard_buf.len() * 64
        // Round 2: the evidence behind feelings about people.
        + p.rel.len() * std::mem::size_of::<crate::people::person::Rel>();
    s.cold = m.cold_bytes(); s.cold_nodes = m.cold.len();
    s.nodes = st.len(); s.links = st.link_count(); s.told = p.told.len();
    s.place_count = (0..m.place_nodes.len()).filter(|&pi| m.place_learnt(pi)).count();
    s
}

/// Run a village for its days (on `threads` threads, minds in `order`, or in index order), recording.
pub fn run(s: &Setup, threads: usize, order: Option<&[usize]>) -> (Record, Valley) {
    let mut v = build(s);
    let l = layout(s.seed);
    let n = s.people;
    let ord: Vec<usize> = order.map(|o| o.to_vec()).unwrap_or_else(|| (0..n).collect());
    let mut rec = Record { n, at: vec![[0; PLACE_CLASSES]; n], close: vec![0; n * n], walked: vec![0; n], said: vec![[0; SAY_KINDS]; n], danger: vec![0; n], surprised: vec![0; n], danger_thought: vec![0; n], ..Record::default() };
    let ticks = s.days * DAY;
    // More than one thread: a pool that lives for the whole run (see `sim::Pool`).
    let pool = (threads > 1).then(|| crate::people::sim::Pool::new(threads));
    let mut day = DayRec::default();
    let (mut drinks0, mut meals0) = (0u32, 0u32);
    for _ in 0..ticks {
        let t = v.w.tick;
        set_tiers(&mut v, s);
        let night = v.w.script.iter().any(|(at, e)| *at == t && *e == Event::Night);
        if night { rec.before.push(v.folk.iter().map(|p| storage(p, n)).collect()); rec.slept.push((0..n).map(|i| place_class(&v.w, &l, v.w.people[i].x, v.w.people[i].y) as u8).collect()); }
        let gored0: u32 = v.w.boars.iter().map(|b| b.gored).sum();
        let caught0 = v.w.deer.iter().filter(|d| d.caught.is_some()).count();
        match &pool { Some(pl) => v.step_pooled(pl, &ord), None => v.step(1, &ord) }
        if night { rec.after.push(v.folk.iter().map(|p| storage(p, n)).collect()); rec.feel.push(feel(&v)); }
        // Measures (the world as it is after the tick, and each mind's last report).
        for i in 0..n {
            let q = &v.w.people[i];
            rec.at[i][place_class(&v.w, &l, q.x, q.y)] += 1;
            rec.walked[i] += (q.x - q.px).unsigned_abs().max((q.y - q.py).unsigned_abs());
            let p = &v.folk[i];
            if p.m.held.response.is_some() { rec.danger[i] += 1; if p.village.as_ref().is_some_and(|l| l.thought) { rec.danger_thought[i] += 1; } }
            if p.village.as_ref().is_some_and(|l| l.thought) && (p.m.last.percepts.iter().any(|x| x.error >= IGNORE_BELOW) || !p.m.last.absent.is_empty()) { rec.surprised[i] += 1; day.surprised += 1; }
            if !q.asleep { for j in i + 1..n { let o = &v.w.people[j]; if pw::d2(q.x, q.y, o.x, o.y) <= 9 { rec.close[i * n + j] += 1; rec.close[j * n + i] += 1; } } }
        }
        rec.air += v.w.air.len() as u64;
        if !v.w.air.is_empty() { for i in 0..n { if !v.w.people[i].asleep { rec.heard += v.w.heard(i).len() as u64; } } }
        for u in &v.w.air {
            let k = say_kind(&u.say);
            rec.said[u.speaker][k] += 1;
            day.said += 1;
            match k { 4 => day.help += 1, 6 => day.gossip += 1, _ => {} }
            if matches!(u.say, crate::people::talk::Say::Propose(_)) { day.proposals += 1; }
        }
        day.gored += v.w.boars.iter().map(|b| b.gored).sum::<u32>() - gored0;
        day.caught += (v.w.deer.iter().filter(|d| d.caught.is_some()).count() > caught0) as u32;
        // Keep the world's logs short (they are for viewers and tests of single scenarios).
        v.w.log.clear(); v.w.said.clear();
        if (t + 1) % DAY == 0 {
            let drinks: u32 = v.w.people.iter().map(|p| p.drinks).sum();
            let meals: u32 = v.w.village.as_ref().unwrap().meals.iter().sum();
            day.drinks = drinks - drinks0; day.meals = meals - meals0; drinks0 = drinks; meals0 = meals;
            day.feast = v.w.village.as_ref().unwrap().feasts.len() as u32;
            rec.day.push(day);
            day = DayRec::default();
        }
    }
    rec.hash = v.state_hash();
    rec.brain_ns = v.brain_ns;
    rec.ticks = ticks;
    (rec, v)
}

/// The village's stats per person, from its layer (for the character summary).
pub fn stats(v: &Valley) -> Vec<Stats> { v.folk.iter().map(|p| p.village.as_ref().map(|l| l.stats).unwrap_or_default()).collect() }
