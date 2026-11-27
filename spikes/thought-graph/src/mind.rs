//! The fox's mind: a slow graph, fast weights on its links, a few modulators, predictions per
//! place, an episode buffer, and sleep. Each tick:
//!
//! 1. **Sense and predict.** Each thing in sight is compared with what the fox expects at that
//!    place. A match is ignored (a faint touch of activation). A prediction error injects
//!    activation into the thing and its place, and raises arousal.
//! 2. **Inject** body states (hunger, fatigue, pain) and the current place (context).
//! 3. **Think:** bounded, modulated spreading (`spread::think`), with either form of the mind.
//! 4. **Feel:** mood moves slowly toward the workspace's valence; arousal follows danger on mind.
//! 5. **Learn by day:** links between co-active workspace nodes gain fast weight, scaled by
//!    arousal; fast weights fade. Salient moments go into the episode buffer.
//! 6. **Act** from the workspace: flee a sensed danger, eat, or walk to the place an anchored
//!    node points at, weighed by how that place's associations feel right now.
//!
//! At night `sleep` replays the episodes and consolidates fast weights into slow ones.

use crate::fox::{self, Ids};
use crate::graph::{tag, Anchor, Graph};
use crate::hd::Hd;
use crate::spread::{self, gain, Activation, Edge, Form, Hop, Live, Mods, Params};
use crate::rng::hash;
use crate::world::{d2, Action, Kind, World, DOG, SIGHT};
use std::hash::{Hash, Hasher};

/// Expectation keys: kind × (approaching or not).
pub const KEYS: usize = 10;
pub fn key(k: Kind, approaching: bool) -> usize { k.idx() * 2 + approaching as usize }

/// Prediction errors below this are ignored.
pub const IGNORE_BELOW: i32 = 400;
/// A matched percept still gets this much activation: seen, not attended.
pub const SEEN_TOUCH: i32 = 40;
/// Context activation for the place the fox is at.
pub const PLACE_CONTEXT: i32 = 300;
/// Danger activation needed to flee (if a sensed source is on the mind).
pub const FLEE_AT: i32 = 300;
/// How much of the carried-over thought a full surprise wipes, per mille.
pub const INTERRUPT: i32 = 700;
/// A walk needs at least this score (activation × feeling / 1000).
pub const WALK_AT: i32 = 60;
/// A considered place that feels bad is pushed out of mind by this per mille (rejected), so
/// other options can come up.
pub const REJECT_DAMP: i32 = 500;
/// A new goal must beat the current one by this per mille.
pub const SWITCH_MARGIN: i32 = 250;

/// An individual's temperament: seeded per individual, and expressed as brain structure and
/// modulator parameters, never as a script.
/// - `reactivity` (0..1000): how fast stress rises, and how much stress cuts thinking short.
/// - `aggression` (0..1000): how strongly danger and pain link to anger in its graph.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Temperament { pub reactivity: i32, pub aggression: i32 }
impl Temperament {
    pub const MEDIUM: Temperament = Temperament { reactivity: 500, aggression: 500 };
    /// From `hash(seed, individual, salt)`. Reactivity and aggression are correlated (a quick
    /// fox tends to be quick to anger): aggression is the mean of reactivity and its own roll.
    pub fn seeded(seed: u64, individual: u32) -> Temperament {
        let reactivity = (hash(seed, individual as u64, 1, 0x7e3f) % 1001) as i32;
        let own = (hash(seed, individual as u64, 2, 0x7e3f) % 1001) as i32;
        Temperament { reactivity, aggression: (reactivity + own) / 2 }
    }
    /// How much a tick's stressors raise stress, per mille.
    pub fn stress_gain(&self) -> i32 { 200 + self.reactivity * 6 / 5 }
    /// How much full stress cuts thinking, per mille.
    pub fn stress_cut(&self) -> i32 { 250 + self.reactivity * 7 / 10 }
}

/// The think parameters under stress: fewer hops and a smaller frontier. At full stress a very
/// reactive mind thinks one hop through a frontier of a handful of nodes.
pub fn stressed_params(base: &Params, stress: i32, t: &Temperament) -> Params {
    let cut = stress as i64 * t.stress_cut() as i64 / 1000; // 0..950
    let mut p = *base;
    p.hops = (base.hops as i64 - (base.hops as i64 - 1) * cut / 950).max(1) as usize;
    p.frontier = (base.frontier as i64 * (1000 - cut * 85 / 100) / 1000).max(6) as usize;
    p
}

#[derive(Clone, Debug, Hash)]
pub struct Episode { pub tick: u32, pub place: Option<u32>, pub ws: Vec<(u32, i32)>, pub valence: i32, pub arousal: i32 }

#[derive(Clone, Debug)]
pub struct Percept {
    pub entity: u32, pub kind: Kind, pub x: i32, pub y: i32, pub place: Option<usize>, pub approaching: bool,
    pub node: u32, pub expected: i32, pub error: i32,
}

/// What happened in the mind this tick (kept for traces and tests).
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub ws: Vec<u32>,
    pub hops: Vec<Hop>,
    pub percepts: Vec<Percept>,
    pub inject: Vec<(u32, i32)>,
    /// Candidate places and their scores (act part, felt part).
    pub scores: Vec<(u32, i32, i32)>,
    pub action: Option<Action>,
    pub why: String,
    /// How deep this think went: hops run, frontier cap, links visited.
    pub hops_run: u32,
    pub frontier_cap: usize,
    pub visits: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which { Graph, Compiled }

#[derive(Clone, Debug)]
pub struct Mind {
    pub g: Graph,
    pub fast: Vec<i16>,
    pub st: Activation,
    pub mods: Mods,
    pub params: Params,
    pub ids: Ids,
    pub place_nodes: Vec<u32>,
    pub entity_nodes: Vec<(u32, u32)>,
    /// Per place, per key: how much the fox expects to see it there (0..1000).
    pub expect: Vec<[i16; KEYS]>,
    pub episodes: Vec<Episode>,
    pub goal: Option<u32>,
    pub flee_left: i32,
    pub flee_from: (i32, i32),
    prev_pain: i32,
    pub which: Which,
    pub compiled: Option<Hd>,
    pub trace: bool,
    pub last: Report,
    /// Turn learning off (for side-by-side comparisons of a single think).
    pub learn: bool,
    /// Hold arousal or mood at a value (a sustained state, for comparisons).
    pub pin_arousal: Option<i32>,
    pub pin_mood: Option<i32>,
    /// A control: at night, drop the day's fast weights and episodes without replaying them.
    pub skip_sleep: bool,
    pub temperament: Temperament,
}

impl Hash for Mind {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.temperament.hash(h); self.g.hash(h); self.fast.hash(h); self.st.act.hash(h); self.st.active.hash(h); self.mods.hash(h);
        self.expect.hash(h); self.episodes.hash(h); self.goal.hash(h); self.flee_left.hash(h); self.flee_from.hash(h);
    }
}

impl Mind {
    /// A fox of medium temperament (the one every scenario uses unless it says otherwise).
    pub fn new(w: &World) -> Mind { Self::with_temperament(w, Temperament::MEDIUM) }

    pub fn with_temperament(w: &World, temperament: Temperament) -> Mind {
        let f = fox::found(w, &temperament);
        let n = f.graph.len();
        let links = f.graph.link_count();
        let mut expect = vec![[0i16; KEYS]; w.places.len()];
        for e in w.entities.iter().filter(|e| e.alive) {
            if let Some(p) = w.place_at(e.x, e.y) { expect[p][key(e.kind, false)] = 900; }
        }
        Mind {
            g: f.graph, fast: vec![0; links], st: Activation::new(n), mods: Mods::default(), params: Params::default(),
            ids: f.ids, place_nodes: f.place_nodes, entity_nodes: f.entity_nodes, expect, episodes: vec![], goal: None,
            flee_left: 0, flee_from: (0, 0), prev_pain: 0, which: Which::Graph, compiled: None, trace: false,
            last: Report::default(), learn: true, pin_arousal: None, pin_mood: None, skip_sleep: false, temperament,
        }
    }

    pub fn node(&self, name: &str) -> u32 { self.g.id(name) }
    pub fn place_node(&self, w: &World, name: &str) -> u32 { self.place_nodes[w.place_index(name)] }
    pub fn act(&self, n: u32) -> i32 { self.st.act[n as usize] }

    /// Compile the current graph (slow + fast) into the bit-vector form and think with it.
    pub fn use_compiled(&mut self, bits: usize, seed: u64) {
        self.compiled = Some(Hd::compile(&Live { g: &self.g, fast: &self.fast }, &self.g, bits, seed));
        self.which = Which::Compiled;
    }
    pub fn recompile(&mut self) {
        if let Some(c) = &self.compiled { let (b, s) = (c.bits, c.seed); self.compiled = Some(Hd::compile(&Live { g: &self.g, fast: &self.fast }, &self.g, b, s)); }
    }

    fn node_of_entity(&self, id: u32, k: Kind) -> u32 {
        if let Some(&(_, n)) = self.entity_nodes.iter().find(|(e, _)| *e == id) { return n; }
        match k { Kind::Hen | Kind::Berry => self.ids.food, Kind::Water => self.ids.water, Kind::Fire => self.ids.fire, Kind::Dog => self.ids.danger }
    }

    /// Senses and predictions: what the fox sees, and how surprising each thing is.
    pub fn perceive(&self, w: &World) -> Vec<Percept> {
        w.visible().into_iter().map(|e| {
            let place = w.place_at(e.x, e.y);
            let k = key(e.kind, e.approaching);
            let expected = place.map_or(0, |p| self.expect[p][k] as i32);
            Percept { entity: e.id, kind: e.kind, x: e.x, y: e.y, place, approaching: e.approaching,
                node: self.node_of_entity(e.id, e.kind), expected, error: 1000 - expected }
        }).collect()
    }

    /// One tick of the mind. Reads the world, returns the fox's action.
    pub fn step(&mut self, w: &World) -> Action {
        let fx = &w.fox;
        self.mods.hunger = fx.hunger;
        self.mods.fatigue = fx.fatigue;
        let mut rep = Report::default();
        if fx.asleep { rep.action = Some(Action::Sleep); rep.why = "asleep".into(); self.last = rep; return Action::Sleep; }

        // 1–2. Sense, predict, inject.
        let percepts = self.perceive(w);
        let mut inject: Vec<(u32, i32)> = vec![];
        let mut surprised: Vec<(u32, i32, i32)> = vec![];
        let mut surprise = 0;
        let mut max_error = 0;
        for p in &percepts {
            // Seeing a known thing updates where its anchor says it is.
            if let Some(&(_, n)) = self.entity_nodes.iter().find(|(e, _)| *e == p.entity) {
                self.g.set_anchor(n, Anchor::Entity { id: p.entity, x: p.x as i16, y: p.y as i16 });
            }
            if p.error < IGNORE_BELOW {
                inject.push((p.node, SEEN_TOUCH));
            } else {
                inject.push((p.node, p.error));
                surprised.push((p.node, p.x, p.y));
                if let Some(pl) = p.place {
                    inject.push((self.place_nodes[pl], p.error / 2));
                    surprised.push((self.place_nodes[pl], p.x, p.y));
                }
                surprise += p.error / 2;
                max_error = max_error.max(p.error);
            }
        }
        if fx.hunger > 100 { inject.push((self.ids.hunger, fx.hunger / 2)); }
        if fx.fatigue > 300 { inject.push((self.ids.fatigue, fx.fatigue / 3)); }
        if fx.pain > 0 { inject.push((self.ids.pain, fx.pain)); }
        let here = w.place_at(fx.x, fx.y);
        if let Some(pl) = here { inject.push((self.place_nodes[pl], PLACE_CONTEXT)); }
        let pain_rise = (fx.pain - self.prev_pain).max(0);
        self.prev_pain = fx.pain;
        if pain_rise > 0 { for p in &percepts { if p.kind == Kind::Dog { surprised.push((p.node, p.x, p.y)); } } }
        let a_before = self.mods.arousal;
        self.mods.arousal = (self.mods.arousal + surprise + pain_rise).min(1000);
        if let Some(a) = self.pin_arousal { self.mods.arousal = a; }
        if let Some(m) = self.pin_mood { self.mods.mood = m; }

        // A surprise interrupts: the thought carried over from last tick is damped.
        if max_error > 0 { self.st.damp(1000 - max_error * INTERRUPT / 1000); }

        // Stress rises with pain, surprise and danger on the mind, at the temperament's rate.
        let tg = self.temperament.stress_gain() as i64;
        let stressor = pain_rise as i64 + surprise as i64 / 2 + self.st.act[self.ids.danger as usize] as i64 / 16;
        self.mods.stress = (self.mods.stress as i64 + stressor * tg / 1000).min(1000) as i32;

        // 3. Think, as deep as stress allows.
        let p = stressed_params(&self.params, self.mods.stress, &self.temperament);
        let mut hops = vec![];
        let tr = if self.trace { Some(&mut hops) } else { None };
        let ws = match (self.which, &self.compiled) {
            (Which::Compiled, Some(c)) => spread::think(c, &mut self.st, &self.mods, &inject, &p, tr),
            _ => spread::think(&Live { g: &self.g, fast: &self.fast }, &mut self.st, &self.mods, &inject, &p, tr),
        };
        rep.hops_run = self.st.hops_run; rep.frontier_cap = p.frontier; rep.visits = self.st.visits;
        self.mods.stress -= self.mods.stress / 40 + (self.mods.stress > 0) as i32;

        // 4. Feel.
        let (mut sa, mut sv) = (0i64, 0i64);
        for &n in &ws { let a = self.st.act[n as usize] as i64; sa += a; sv += a * self.g.val[n as usize] as i64; }
        let wv = if sa > 0 { (sv / sa) as i32 } else { 0 };
        self.mods.mood += (wv - self.mods.mood) / 32;
        let m = &mut self.mods;
        m.arousal = (m.arousal + self.st.act[self.ids.danger as usize] / 64).min(1000);
        m.arousal -= m.arousal / 10;
        if let Some(a) = self.pin_arousal { m.arousal = a; }
        if let Some(md) = self.pin_mood { m.mood = md; }

        // 5. Learn by day.
        if self.learn {
            self.learn_expectations(w, &percepts);
            self.hebbian(&ws);
            for f in self.fast.iter_mut() { *f -= *f / 128 + (*f > 0) as i16; }
            if self.mods.arousal - a_before >= 250 || pain_rise >= 200 {
                self.episodes.push(Episode { tick: w.tick, place: here.map(|p| self.place_nodes[p]), ws: ws.iter().map(|&n| (n, self.st.act[n as usize])).collect(), valence: wv, arousal: self.mods.arousal });
                if self.episodes.len() > 32 { self.episodes.remove(0); }
            }
        }

        // 6. Act.
        let (action, why, scores) = match (self.which, &self.compiled) {
            (Which::Compiled, Some(c)) => self.choose(c, w, &ws, &percepts, &surprised),
            _ => { let f = Live { g: &self.g, fast: &self.fast }; self.choose(&f, w, &ws, &percepts, &surprised) }
        };
        match action {
            Action::Flee { from_x, from_y, .. } => { if self.flee_left == 0 { self.flee_left = 4; } self.flee_from = (from_x, from_y); }
            Action::WalkTo { .. } => {}
            _ => {}
        }
        // Rejected options: places on the mind that feel bad are pushed down, with their contents.
        if !matches!(action, Action::Flee { .. }) {
            for &(p, a, felt) in &scores {
                if felt < 0 && a > 0 {
                    self.st.act[p as usize] = (self.st.act[p as usize] as i64 * (1000 - REJECT_DAMP) as i64 / 1000) as i32;
                    for &n in &ws {
                        if self.g.tags[n as usize] & tag::PLACE == 0 && self.g.tags[n as usize] & tag::ANCHORED != 0 && self.g.find(n, p, crate::graph::lt::AT).is_some() {
                            self.st.act[n as usize] = (self.st.act[n as usize] as i64 * (1000 - REJECT_DAMP) as i64 / 1000) as i32;
                        }
                    }
                }
            }
        }
        rep.ws = ws; rep.hops = hops; rep.percepts = percepts; rep.inject = inject; rep.scores = scores;
        rep.action = Some(action); rep.why = why;
        self.last = rep;
        action
    }

    /// How a place feels right now: a one-hop look-ahead over what it holds and what's attached
    /// to it, by link weight, each feeling scaled by the drives and modulators (a somatic marker:
    /// considering the place evokes its associations, on the mind or not).
    /// - bad things count more when aroused or in a low mood;
    /// - food counts in proportion to hunger, rest to fatigue;
    /// - other good things (comfort, the place itself, e.g. home) count more when aroused (safety);
    /// - neighbouring places count at half weight, by their own valence: a place near where
    ///   something bad happened feels a little worse.
    pub fn felt<F: Form>(&self, f: &F, p: u32) -> i32 {
        let m = &self.mods;
        let neg_mood = (-m.mood).max(0) as i64;
        let (a, h, fa) = (m.arousal as i64, m.hunger as i64, m.fatigue as i64);
        let good = |val: i64, tags: u8| -> i64 {
            if tags & tag::FOOD != 0 { val * 2 * h / 1000 } else if tags & tag::REST != 0 { val * 2 * fa / 1000 } else { val * (300 + a) / 1000 }
        };
        let mut edges = vec![];
        f.edges(p, &mut edges);
        let own = f.val(p) as i64;
        let mut v = if own < 0 { own * (1000 + 2 * a + neg_mood) / 1000 } else { good(own, f.tags(p)) };
        for e in edges {
            let val = (f.val(e.to) + e.val).clamp(-1000, 1000) as i64;
            if f.tags(e.to) & tag::PLACE != 0 { v += e.w as i64 * val / 2000; continue; }
            v += if val < 0 { e.w as i64 * val / 1000 * (1000 + 2 * a + neg_mood) / 1000 } else { e.w as i64 * good(val, f.tags(e.to)) / 1000 };
        }
        v as i32
    }

    /// The place an anchored thing is at: its strongest `at` link to a place.
    fn place_of<F: Form>(f: &F, n: u32) -> Option<u32> {
        let mut edges = vec![];
        f.edges(n, &mut edges);
        edges.iter().filter(|e| f.tags(e.to) & tag::PLACE != 0).max_by_key(|e| (e.w, std::cmp::Reverse(e.to))).map(|e| e.to)
    }

    fn choose<F: Form>(&self, f: &F, w: &World, ws: &[u32], percepts: &[Percept], surprised: &[(u32, i32, i32)]) -> (Action, String, Vec<(u32, i32, i32)>) {
        let fx = &w.fox;
        let act = |n: u32| self.st.act[n as usize];
        let name = |n: u32| self.g.names[n as usize].clone();
        // Flee: danger on the mind, and a sensed source of it in the workspace.
        // Stress makes flight likelier (a lower bar) and quicker (3 cells a tick).
        let stress = self.mods.stress;
        let speed = if stress >= 500 { 3 } else { 2 };
        let flee_at = FLEE_AT * (1000 - stress / 2) / 1000;
        let danger = act(self.ids.danger);
        let anger = act(self.ids.anger);
        // Snap: the dog within two cells, and anger high enough against danger. The bar falls
        // with aggression: anger × 1000 ≥ danger × (1400 − aggression).
        let bar = (danger as i64 * (1400 - self.temperament.aggression) as i64 / 1000) as i32;
        if ws.contains(&self.ids.anger) && anger >= bar.max(flee_at) {
            if let Some(d) = percepts.iter().find(|p| p.entity == DOG && (p.x - fx.x).abs().max((p.y - fx.y).abs()) <= 2) {
                return (Action::Snap { entity: d.entity }, format!("anger {anger} against danger {danger} (bar {bar}): snap"), vec![]);
            }
        }
        if self.flee_left > 0 {
            return (Action::Flee { from_x: self.flee_from.0, from_y: self.flee_from.1, speed }, "still fleeing".into(), vec![]);
        }
        if (ws.contains(&self.ids.danger) && danger >= flee_at) || (ws.contains(&self.ids.anger) && anger >= flee_at) {
            let src = surprised.iter().filter(|s| ws.contains(&s.0)).max_by_key(|s| (act(s.0), std::cmp::Reverse(s.0)));
            if let Some(&(n, x, y)) = src {
                return (Action::Flee { from_x: x, from_y: y, speed }, format!("danger {danger} on my mind (bar {flee_at}), from {}", name(n)), vec![]);
            }
        }
        // Eat: hungry, food on the mind, food within reach.
        if fx.hunger >= 150 {
            for p in percepts {
                if matches!(p.kind, Kind::Hen | Kind::Berry) && (p.x - fx.x).abs().max((p.y - fx.y).abs()) <= 1
                    && (ws.contains(&p.node) || ws.contains(&self.ids.food))
                    && w.entity(p.entity).is_some_and(|e| e.servings > 0) {
                    return (Action::Eat { entity: p.entity }, format!("eat {}", name(p.node)), vec![]);
                }
            }
        }
        // Walk: places in the workspace, and places that anchored things in it are at.
        // Arrived: within a cell of a place's coordinates.
        let here = self.place_nodes.iter().copied().find(|&p| self.g.anchor(p).is_some_and(|a| { let (x, y) = a.xy(); (x - fx.x).abs().max((y - fx.y).abs()) <= 1 }));
        let mut cands: Vec<u32> = vec![];
        for &n in ws {
            let t = f.tags(n);
            if t & tag::PLACE != 0 { cands.push(n) }
            else if t & tag::ANCHORED != 0 { if let Some(p) = Self::place_of(f, n) { cands.push(p) } }
        }
        cands.sort_unstable(); cands.dedup();
        let mut scores = vec![];
        for &p in &cands {
            let mut a = act(p);
            // Attractive things on the mind pull toward the place they're at.
            for &n in ws { if n != p && f.tags(n) & tag::ANCHORED != 0 && f.tags(n) & tag::PLACE == 0 && f.val(n) > 0 && Self::place_of(f, n) == Some(p) { a += act(n); } }
            scores.push((p, a, self.felt(f, p)));
        }
        // On my mind × wanted: a place that feels bad isn't walked to at all.
        let total = |s: &(u32, i32, i32)| (s.1 as i64 * s.2.max(0) as i64 / 1000) as i32;
        let best = scores.iter().max_by_key(|s| (total(s), std::cmp::Reverse(s.0))).copied();
        // The best place is where I am: stay.
        if let (Some(b), Some(h)) = (best, here) { if b.0 == h { return (Action::Stay, format!("stay at {} (score {})", name(h), total(&b)), scores); } }
        // Arrived at the goal with nothing to eat: done.
        let goal = match self.goal { Some(g) if Some(g) == here => None, g => g };
        let goal_score = goal.and_then(|g| scores.iter().find(|s| s.0 == g)).map(total);
        let pick = match (best, goal, goal_score) {
            (Some(b), Some(g), Some(gs)) if b.0 != g && total(&b) * 1000 < gs * (1000 + SWITCH_MARGIN) && gs >= WALK_AT => Some((g, gs)),
            (Some(b), _, _) if total(&b) >= WALK_AT => Some((b.0, total(&b))),
            _ => None,
        };
        let scores_out = scores.clone();
        match pick {
            Some((p, s)) => {
                let (x, y) = self.g.anchor(p).map(|a| a.xy()).unwrap_or((fx.x, fx.y));
                (Action::WalkTo { x, y }, format!("walk to {} (score {s})", name(p)), scores_out)
            }
            None => (Action::Stay, "nothing pulls".into(), scores_out),
        }
    }

    /// Track the goal the world actually received (so commitment survives).
    pub fn commit(&mut self, w: &World, a: Action) {
        match a {
            Action::WalkTo { x, y } => {
                self.goal = self.place_nodes.iter().copied().find(|&p| self.g.anchor(p).map(|a| a.xy()) == Some((x, y)));
            }
            Action::Flee { .. } => { self.flee_left = (self.flee_left - 1).max(0); self.goal = None; }
            Action::Eat { .. } | Action::Snap { .. } => {}
            Action::Stay => {
                let fx = &w.fox;
                if self.goal.is_some_and(|g| self.g.anchor(g).is_some_and(|a| { let (x, y) = a.xy(); (x - fx.x).abs().max((y - fx.y).abs()) <= 1 })) { self.goal = None; }
            }
            Action::Sleep => self.goal = None,
        }
    }

    fn learn_expectations(&mut self, w: &World, percepts: &[Percept]) {
        let fx = &w.fox;
        for (pi, pl) in w.places.iter().enumerate() {
            let d = d2(fx.x, fx.y, pl.x, pl.y);
            if d > SIGHT * SIGHT { continue; }
            for k in Kind::ALL {
                for appr in [false, true] {
                    let seen = percepts.iter().any(|p| p.place == Some(pi) && p.kind == k && p.approaching == appr);
                    let e = &mut self.expect[pi][key(k, appr)];
                    if seen { *e += (1000 - *e) / 32; } else if d <= 16 { *e -= *e / 64; }
                }
            }
        }
    }

    /// Fast weights: links between co-active workspace nodes strengthen, scaled by arousal.
    fn hebbian(&mut self, ws: &[u32]) {
        let a = self.mods.arousal as i64;
        for &u in ws {
            for l in self.g.links(u) {
                let v = self.g.targets[l];
                if v == u || !ws.contains(&v) { continue; }
                let co = self.st.act[u as usize].min(1000) as i64 * self.st.act[v as usize].min(1000) as i64 / 1000;
                let d = (co * a / 4000) as i32;
                self.fast[l] = (self.fast[l] as i32 + d).min(1000) as i16;
            }
        }
    }

    /// Sleep: replay the day's episodes into the fast weights, consolidate fast into slow, clear
    /// fast. Each episode also leaves its feeling on the anchored things and places in it (where
    /// and what, not the concepts), in proportion to how active each was, and on the links
    /// between them. Feelings move toward the episode's valence, never past it.
    pub fn sleep(&mut self) {
        if self.skip_sleep {
            self.episodes.clear();
            self.fast.iter_mut().for_each(|f| *f = 0);
            self.st.clear(); self.mods.arousal = 0; self.mods.stress = 0; self.mods.mood /= 2; self.goal = None; self.flee_left = 0;
            return;
        }
        let eps = std::mem::take(&mut self.episodes);
        for ep in &eps {
            let s = ep.arousal as i64;
            let mut nodes = ep.ws.clone();
            let top = nodes.iter().map(|x| x.1).max().unwrap_or(1).max(1) as i64;
            if let Some(p) = ep.place { if !nodes.iter().any(|x| x.0 == p) { nodes.push((p, top as i32)) } }
            let anchored = |g: &Graph, n: u32| g.tags[n as usize] & tag::ANCHORED != 0;
            for &(u, au) in &nodes {
                for l in self.g.links(u) {
                    let v = self.g.targets[l];
                    if !nodes.iter().any(|x| x.0 == v) { continue; }
                    self.fast[l] = (self.fast[l] as i64 + s / 2).min(1000) as i16;
                    if anchored(&self.g, u) || anchored(&self.g, v) {
                        let lv = self.g.lval[l] as i64;
                        self.g.lval[l] = (lv + (ep.valence as i64 - lv) * s / 1000 / 4) as i16;
                    }
                }
                if anchored(&self.g, u) {
                    let share = au as i64 * 1000 / top;
                    let v = self.g.val[u as usize] as i64;
                    self.g.val[u as usize] = (v + (ep.valence as i64 - v) * s / 1000 * share / 1000 / 3) as i16;
                }
            }
        }
        for l in 0..self.fast.len() {
            self.g.slow[l] = (self.g.slow[l] as i32 + self.fast[l] as i32 / 3).min(1000) as i16;
            self.fast[l] = 0;
        }
        self.st.clear();
        self.mods.arousal = 0;
        self.mods.stress = 0;
        self.mods.mood /= 2;
        self.goal = None;
        self.flee_left = 0;
        self.recompile();
    }

    pub fn fast_total(&self) -> i64 { self.fast.iter().map(|&f| f as i64).sum() }
    /// Effective weight of the strongest link u → v of any type.
    pub fn weight(&self, u: u32, v: u32) -> (i32, i32) {
        self.g.links(u).filter(|&l| self.g.targets[l] == v).map(|l| (self.g.slow[l] as i32, self.fast[l] as i32)).max().unwrap_or((0, 0))
    }
    pub fn link_of(&self, u: u32, v: u32, ty: u8) -> Option<usize> { self.g.find(u, v, ty) }
    /// Edges of a node in the live graph form.
    pub fn live_edges(&self, u: u32) -> Vec<Edge> { let mut e = vec![]; Live { g: &self.g, fast: &self.fast }.edges(u, &mut e); e }
    pub fn gain_for(&self, ty: u8, to: u32) -> i32 { gain(&self.mods, ty, self.g.tags[to as usize], self.g.val[to as usize] as i32) }
}
