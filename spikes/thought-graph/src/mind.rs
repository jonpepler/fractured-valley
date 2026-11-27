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
//! 7. **Consolidate in idle time.** On a settled tick (no surprise, no pain, not fleeing, not
//!    stressed) the think budget it didn't use replays the backlog into the slow graph.
//!
//! **Backlog and sleep pressure.** Fast weights and unreplayed episodes are a backlog. Fast
//! weights no longer vanish overnight: they fade slowly and leave only by consolidation (or lossy
//! compaction when the backlog is over capacity). Tiredness is the backlog's size: `mods.fatigue`
//! is read from it, not from a timer. Sleep is a state with a large consolidation budget.

use crate::fox::{self, Ids};
use crate::graph::lt;
use crate::graph::{tag, Anchor, Graph};
use crate::hd::Hd;
use crate::spread::{self, gain, Activation, Edge, Form, Hop, Live, Mods, Params};
use crate::rng::hash;
use crate::world::{d2, Action, Kind, World, DOG, PLACE_RADIUS, SIGHT};
use std::hash::{Hash, Hasher};

/// Expectation keys: kind × (approaching or not).
pub const KEYS: usize = 12;
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

/// Work a waking tick can afford, in link visits. A think costs what it visits; on a settled tick
/// the rest goes to consolidation.
pub const THINK_BUDGET: i64 = 600;
/// Work per tick of sleep (nothing to sense, nothing to decide).
pub const SLEEP_BUDGET: i64 = 8000;
/// A night's sleep, in ticks; it runs inside the `Night` tick.
pub const NIGHT_TICKS: u32 = 240;
/// Replaying an episode re-runs it through the graph: this many hops over its nodes' links.
pub const REPLAY_HOPS: i64 = 2;
/// Share of an episode's remaining strength one replay moves into the slow graph, per mille.
pub const REPLAY_AWAKE: i64 = 250;
pub const REPLAY_ASLEEP: i64 = 1000;
/// Budget units to consolidate one unit of fast weight (interleaving it with what's already
/// known is the expensive part; see the README).
pub const TRANSFER_COST: i64 = 60;
/// Consolidating plain fast weight keeps 1/this of it in the slow graph; the rest is downscaled
/// away (synaptic homeostasis). Replayed episodes are what sleep keeps in full.
pub const TRANSFER_KEEP: i64 = 8;
/// Backlog (fast weight + unreplayed episodes) at which fatigue is full; over it, the mind
/// compacts lossily.
pub const BACKLOG_CAP: i64 = 30000;
/// Waking encoding that happens even when calm, per mille of the arousal scale.
pub const BASE_ENCODE: i64 = 60;
/// Day learning: fast weight gained per tick is co-activation × (arousal + `BASE_ENCODE`) / this,
/// scaled by the room left on the link.
pub const HEBB_RATE: i64 = 4000;
/// Extinction: per tick an expected thing is missing, the links between the place and it (and its
/// kind's concept) gain a
/// negative fast trace of the prediction error / this.
pub const EXTINCT_RATE: i32 = 6;
/// Fast weights fade by 1/128 every this many ticks (about 1/4096 a tick: a half-life of ~9 days).
pub const FAST_FADE_EVERY: u32 = 32;
/// A tired fox at a place it rests at naps at this fatigue, and wakes below `WAKE_AT` (or when
/// surprised, hurt or very hungry).
pub const NAP_AT: i32 = 500;
pub const WAKE_AT: i32 = 100;
pub const WAKE_HUNGER: i32 = 900;
/// Score a surprising, not-bad thing in sight adds to its place, per mille of its prediction error
/// (curiosity; Berlyne). It fades as the fox learns to expect the thing there.
pub const NOVELTY_PULL: i32 = 600;
/// Unresolved prediction error at which the fox searches. (Volatility makes it build faster.)
pub const SEARCH_AT: i32 = 300;

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

/// What consolidation did: per tick, and in total.
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub struct Consol {
    /// Budget units spent consolidating last tick, and over the mind's life.
    pub last_units: i64,
    pub total_units: i64,
    /// Settled ticks (consolidation ran) and episode replays.
    pub idle_ticks: u32,
    pub replays: u32,
    /// Fast weight moved out by consolidation; fast weight and episode strength lost to compaction.
    pub moved: i64,
    pub lost: i64,
    /// Work forced on the think by compaction last tick.
    pub forced: i64,
}

/// An unresolved prediction error: an expected thing missing from a place. It drives search
/// (curiosity) until the thing is found again or the error fades.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Puzzle { pub place: usize, pub kind: Kind, pub strength: i32, pub since: u32 }

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
    /// Expected things missing from places in view: (place index, kind, prediction error).
    pub absent: Vec<(usize, Kind, i32)>,
    /// Places learnt this tick.
    pub learnt: Vec<u32>,
    /// Backlog after this tick, and the consolidation units spent on it.
    pub backlog: i64,
    pub consolidated: i64,
    pub searching: bool,
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
    /// A fox that does not sleep: at night it lies awake in its den, thinking (and consolidating
    /// only what idle waking time allows).
    pub skip_sleep: bool,
    pub temperament: Temperament,
    /// Kind of each of `entity_nodes` (recognition is by look and place, not identity).
    pub entity_kinds: Vec<Kind>,
    /// Ablation switch: consolidate in idle waking time (on by default).
    pub idle_consolidation: bool,
    pub consol: Consol,
    pub puzzle: Option<Puzzle>,
    /// The current search waypoint and how many waypoints this search has tried.
    pub search_wp: Option<(i32, i32)>,
    pub search_n: u32,
    /// Asleep by its own choice (a nap), as opposed to the scripted night.
    pub napping: bool,
    /// Learnt volatility per kind of thing (0..1000): how often the fox's predictions about where
    /// that kind is have failed lately (expected uncertainty). It down-weights those prediction
    /// errors' arousal and stress, and makes search start sooner.
    pub volatility: [i16; Kind::ALL.len()],
}

impl Hash for Mind {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.temperament.hash(h); self.g.hash(h); self.fast.hash(h); self.st.act.hash(h); self.st.active.hash(h); self.mods.hash(h);
        self.expect.hash(h); self.episodes.hash(h); self.goal.hash(h); self.flee_left.hash(h); self.flee_from.hash(h);
        self.consol.hash(h); self.puzzle.hash(h); self.search_wp.hash(h); self.napping.hash(h); self.volatility.hash(h);
    }
}

impl Mind {
    /// A fox of medium temperament (the one every scenario uses unless it says otherwise).
    pub fn new(w: &World) -> Mind { Self::with_temperament(w, Temperament::MEDIUM) }

    pub fn with_temperament(w: &World, temperament: Temperament) -> Mind {
        let f = fox::found(w, &temperament);
        let n = f.graph.len();
        let links = f.graph.link_count();
        let mut m = Mind {
            g: f.graph, fast: vec![0; links], st: Activation::new(n), mods: Mods::default(), params: Params::default(),
            ids: f.ids, expect: vec![[0i16; KEYS]; f.place_nodes.len()], place_nodes: f.place_nodes, entity_nodes: f.entity_nodes, episodes: vec![], goal: None,
            flee_left: 0, flee_from: (0, 0), prev_pain: 0, which: Which::Graph, compiled: None, trace: false,
            last: Report::default(), learn: true, pin_arousal: None, pin_mood: None, skip_sleep: false, temperament,
            entity_kinds: f.entity_kinds, idle_consolidation: true, consol: Consol::default(), puzzle: None, search_wp: None, search_n: 0, napping: false, volatility: [0; Kind::ALL.len()],
        };
        for e in w.entities.iter().filter(|e| e.alive) {
            if let Some(p) = m.place_index_at(e.x, e.y) { m.expect[p][key(e.kind, false)] = 900; }
        }
        m
    }

    /// How much of a prediction error about kind `k` still arouses, per mille.
    pub fn calm_factor(&self, k: Kind) -> i32 { 1000 - self.volatility[k.idx()] as i32 * 7 / 10 }
    /// A prediction about where a kind is failed (it went missing, or turned up somewhere new).
    fn volatile_event(&mut self, k: Kind) { let v = &mut self.volatility[k.idx()]; *v += (1000 - *v) / 4; }

    /// Whether a place slot has been learnt (founding places always are).
    pub fn place_learnt(&self, pi: usize) -> bool { self.g.anchor(self.place_nodes[pi]).is_some_and(|a| a.xy().0 != fox::UNLEARNT as i32) }
    fn place_xy(&self, pi: usize) -> (i32, i32) { self.g.anchor(self.place_nodes[pi]).map(|a| a.xy()).unwrap_or((-999, -999)) }

    /// The place a point is at, by the mind's own map: the nearest learnt place within
    /// `PLACE_RADIUS`, ties to the lower index. (The world's labels are never read.)
    pub fn place_index_at(&self, x: i32, y: i32) -> Option<usize> {
        let mut best: Option<(i32, usize)> = None;
        for pi in 0..self.place_nodes.len() {
            if !self.place_learnt(pi) { continue; }
            let (px, py) = self.place_xy(pi);
            let d = d2(x, y, px, py);
            if d <= PLACE_RADIUS * PLACE_RADIUS && best.map_or(true, |(bd, _)| d < bd) { best = Some((d, pi)); }
        }
        best.map(|b| b.1)
    }

    /// Where the fox believes home is: the learnt place that feels best to a tired, calm fox.
    pub fn home(&self) -> u32 {
        let tired = Mods { fatigue: 1000, ..Mods::default() };
        let f = Live { g: &self.g, fast: &self.fast };
        (0..self.place_nodes.len()).filter(|&pi| self.place_learnt(pi)).map(|pi| self.place_nodes[pi])
            .max_by_key(|&p| (self.felt_with(&f, p, &tired), std::cmp::Reverse(p))).unwrap()
    }
    pub fn home_xy(&self) -> (i32, i32) { self.g.anchor(self.home()).unwrap().xy() }

    /// Learn a new place at a cell: take a spare slot, anchor it there, and link it by real
    /// distance to every place already known. Returns its place index, or None if no slot is left.
    pub fn learn_place(&mut self, x: i32, y: i32) -> Option<usize> {
        let n0 = self.place_nodes.len() - fox::SPARE_PLACES;
        // A free slot, or else forget the learnt place that matters least (the smallest total
        // link weight) and reuse it: a mind has room for only so many places.
        let pi = match (n0..self.place_nodes.len()).find(|&pi| !self.place_learnt(pi)) {
            Some(pi) => pi,
            None => {
                let here = self.place_index_at(x, y);
                let weight = |pi: usize| -> i64 { self.g.links(self.place_nodes[pi]).map(|l| self.g.slow[l] as i64 + self.fast[l] as i64).sum() };
                let pi = (n0..self.place_nodes.len()).filter(|&pi| Some(pi) != here).min_by_key(|&pi| (weight(pi), pi))?;
                let pn = self.place_nodes[pi];
                for u in 0..self.g.len() as u32 { for l in self.g.links(u) {
                    if u == pn || self.g.targets[l] == pn { self.g.slow[l] = 0; self.fast[l] = 0; self.g.lval[l] = 0; }
                } }
                self.g.val[pn as usize] = 0;
                pi
            }
        };
        let pn = self.place_nodes[pi];
        self.g.set_anchor(pn, Anchor::Place { x: x as i16, y: y as i16 });
        for qi in 0..self.place_nodes.len() {
            if qi == pi || !self.place_learnt(qi) { continue; }
            let (qx, qy) = self.place_xy(qi);
            let wgt = fox::near_weight(crate::world::dist(x, y, qx, qy)) as i16;
            let q = self.place_nodes[qi];
            for (a, b) in [(pn, q), (q, pn)] { if let Some(l) = self.g.find(a, b, lt::NEAR) { self.g.slow[l] = wgt; } }
        }
        self.expect[pi] = [0; KEYS];
        Some(pi)
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

    /// Recognition by look and place, never by identity: each known thing is matched to the
    /// visible thing of its kind nearest where it was last seen (a creature anywhere in sight, a
    /// fixed thing only within `PLACE_RADIUS` of there). Anything unmatched is just an instance of
    /// its kind ("a burrow", "a fire").
    fn recognise(&self, vis: &[&crate::world::Entity]) -> Vec<u32> {
        let mut node: Vec<u32> = vis.iter().map(|e| fox::kind_concept(&self.ids, e.kind)).collect();
        for (i, &(_, n)) in self.entity_nodes.iter().enumerate() {
            let k = self.entity_kinds[i];
            let (ax, ay) = self.g.anchor(n).map(|a| a.xy()).unwrap_or((0, 0));
            let creature = self.g.tags[n as usize] & tag::CREATURE != 0;
            let best = vis.iter().enumerate().filter(|(_, e)| e.kind == k)
                .map(|(j, e)| (d2(e.x, e.y, ax, ay), e.id, j))
                .filter(|&(d, _, _)| creature || d <= PLACE_RADIUS * PLACE_RADIUS).min();
            if let Some((_, _, j)) = best { node[j] = n; }
        }
        node
    }

    /// Senses and predictions: what the fox sees, and how surprising each thing is.
    pub fn perceive(&self, w: &World) -> Vec<Percept> {
        let vis = w.visible();
        let nodes = self.recognise(&vis);
        vis.into_iter().zip(nodes).map(|(e, node)| {
            let place = self.place_index_at(e.x, e.y);
            let k = key(e.kind, e.approaching);
            let expected = place.map_or(0, |p| self.expect[p][k] as i32);
            Percept { entity: e.id, kind: e.kind, x: e.x, y: e.y, place, approaching: e.approaching, node, expected, error: 1000 - expected }
        }).collect()
    }

    /// Absence: an expected landmark missing from a place in clear view (within half of sight).
    /// Creatures move and fires come and go, so only landmarks are missed.
    pub fn absences(&self, w: &World, percepts: &[Percept]) -> Vec<(usize, Kind, i32)> {
        let fx = &w.fox;
        let mut out = vec![];
        for pi in 0..self.place_nodes.len() {
            if !self.place_learnt(pi) { continue; }
            let (px, py) = self.place_xy(pi);
            if d2(fx.x, fx.y, px, py) > (SIGHT / 2) * (SIGHT / 2) { continue; }
            for k in Kind::ALL.into_iter().filter(|k| k.landmark()) {
                let e = self.expect[pi][key(k, false)] as i32;
                if e >= IGNORE_BELOW && !percepts.iter().any(|p| p.kind == k && p.place == Some(pi)) { out.push((pi, k, e)); }
            }
        }
        out
    }

    /// The node a missing thing comes to mind as: the known thing of that kind at that place, or
    /// the kind's concept.
    fn missing_node(&self, pi: usize, k: Kind) -> u32 {
        let (px, py) = self.place_xy(pi);
        self.entity_nodes.iter().enumerate().find(|(i, (_, n))| self.entity_kinds[*i] == k
            && self.g.anchor(*n).is_some_and(|a| { let (x, y) = a.xy(); d2(x, y, px, py) <= PLACE_RADIUS * PLACE_RADIUS }))
            .map(|(_, (_, n))| *n).unwrap_or_else(|| fox::kind_concept(&self.ids, k))
    }

    /// Unconsolidated work: fast weight plus what each unreplayed episode would still write.
    pub fn backlog(&self) -> i64 { self.fast_total() + self.episodes.iter().map(|e| self.episode_weight(e)).sum::<i64>() }
    fn episode_nodes(&self, ep: &Episode) -> Vec<(u32, i32)> {
        let mut nodes = ep.ws.clone();
        let top = nodes.iter().map(|x| x.1).max().unwrap_or(1).max(1);
        if let Some(p) = ep.place { if !nodes.iter().any(|x| x.0 == p) { nodes.push((p, top)) } }
        nodes
    }
    pub fn episode_weight(&self, ep: &Episode) -> i64 {
        let nodes = self.episode_nodes(ep);
        let mut links = 0i64;
        for &(u, _) in &nodes { for l in self.g.links(u) { if nodes.iter().any(|x| x.0 == self.g.targets[l]) { links += 1; } } }
        links * ep.arousal as i64 / 6
    }
    /// Sleep pressure: fatigue (0..1000) read from the backlog.
    pub fn pressure(&self) -> i32 { (self.backlog() * 1000 / BACKLOG_CAP).clamp(0, 1000) as i32 }

    /// One tick of the mind. Reads the world, returns the fox's action.
    pub fn step(&mut self, w: &World) -> Action {
        let fx = &w.fox;
        self.mods.hunger = fx.hunger;
        let mut rep = Report::default();
        // Over capacity, the mind must compact before it can think (lossy, and it costs the think).
        let b = self.backlog();
        self.consol.forced = if b > BACKLOG_CAP && self.learn { self.compact(b) } else { 0 };
        // Tiredness is the backlog: sleep pressure, not a timer.
        self.mods.fatigue = self.pressure();
        if fx.asleep { rep.action = Some(Action::Sleep); rep.why = "asleep".into(); rep.backlog = self.backlog(); self.last = rep; return Action::Sleep; }
        // A nap: sleep's budget each tick. Light sleep still senses: a surprise, pain or strong
        // hunger wakes it, and so does a cleared backlog.
        if self.napping {
            let woken = self.perceive(w).iter().any(|p| p.error >= IGNORE_BELOW) || fx.pain > self.prev_pain || fx.hunger >= WAKE_HUNGER;
            if woken || self.mods.fatigue < WAKE_AT { self.napping = false; } else {
                self.consol.last_units = 0;
                self.consolidate(SLEEP_BUDGET, true);
                self.mods.fatigue = self.pressure();
                self.mods.arousal -= self.mods.arousal / 10; self.mods.stress -= self.mods.stress / 20;
                self.st.damp(900);
                rep.action = Some(Action::Sleep); rep.why = format!("napping (fatigue {})", self.mods.fatigue);
                rep.backlog = self.backlog(); rep.consolidated = self.consol.last_units;
                self.last = rep; return Action::Sleep;
            }
        }

        // 1–2. Sense, predict, inject. A landmark where the mind knows no place teaches it one.
        if self.learn {
            for e in w.visible() {
                if e.kind.landmark() && self.place_index_at(e.x, e.y).is_none() {
                    if let Some(pi) = self.learn_place(e.x, e.y) { rep.learnt.push(self.place_nodes[pi]); }
                }
            }
        }
        let percepts = self.perceive(w);
        let mut inject: Vec<(u32, i32)> = vec![];
        let mut surprised: Vec<(u32, i32, i32)> = vec![];
        let mut surprise = 0;
        let mut max_error = 0;
        for p in &percepts {
            // Seeing a known thing updates where its anchor says it is.
            if self.entity_nodes.iter().any(|&(_, n)| n == p.node) {
                self.g.set_anchor(p.node, Anchor::Entity { id: p.entity, x: p.x as i16, y: p.y as i16 });
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
                // An error about a kind known to move about arouses less (precision weighting).
                surprise += p.error / 2 * self.calm_factor(p.kind) / 1000;
                max_error = max_error.max(p.error);
                if p.kind.landmark() && rep.learnt.iter().any(|&n| p.place.is_some_and(|pi| self.place_nodes[pi] == n)) { self.volatile_event(p.kind); }
            }
        }
        // A newly learnt place: the thing seen there is linked to it (a first, fast trace).
        for p in &percepts {
            if let Some(pi) = p.place { if rep.learnt.contains(&self.place_nodes[pi]) {
                let pn = self.place_nodes[pi];
                for (a, b) in [(pn, p.node), (p.node, pn)] {
                    if let Some(l) = self.g.links(a).find(|&l| self.g.targets[l] == b) { self.fast[l] = (self.fast[l] as i32 + p.error / 2).min(1000) as i16; }
                }
            }}
        }
        // Absence: something expected isn't there. The missing thing comes to mind, with its
        // place, and if it was wanted, loss (frustration). Unresolved, it becomes a puzzle.
        let absent = self.absences(w, &percepts);
        for &(pi, k, e) in &absent {
            let n = self.missing_node(pi, k);
            inject.push((n, e));
            inject.push((self.place_nodes[pi], e / 2));
            if self.g.val[n as usize] > 0 { inject.push((self.ids.loss, e * self.g.val[n as usize] as i32 / 600)); }
            surprise += e / 2 * self.calm_factor(k) / 1000;
            max_error = max_error.max(e);
            let fresh = self.puzzle.map_or(true, |pz| pz.place != pi || pz.kind != k);
            if fresh { self.volatile_event(k); }
            // Where things of this kind are known to move, the search drive builds faster.
            let gain = e / 8 * (1000 + self.volatility[k.idx()] as i32 * 2) / 1000;
            let pz = self.puzzle.get_or_insert(Puzzle { place: pi, kind: k, strength: 0, since: w.tick });
            if pz.place != pi || pz.kind != k { *pz = Puzzle { place: pi, kind: k, strength: pz.strength / 2, since: w.tick }; }
            pz.strength = (pz.strength + gain).min(1000);
        }
        // Finding the missing kind of thing (anywhere) resolves the puzzle; otherwise it fades.
        if let Some(pz) = &mut self.puzzle {
            if percepts.iter().any(|p| p.kind == pz.kind && p.place != Some(pz.place)) { pz.strength -= pz.strength / 8 + 20; }
            pz.strength -= pz.strength / 400 + 1;
            if pz.strength <= 0 { self.puzzle = None; self.search_wp = None; self.search_n = 0; }
        }
        let absent_now = absent.clone();
        rep.absent = absent;
        if fx.hunger > 100 { inject.push((self.ids.hunger, fx.hunger / 2)); }
        let fatigue = self.mods.fatigue;
        if fatigue > 300 { inject.push((self.ids.fatigue, fatigue / 3)); }
        if fx.pain > 0 { inject.push((self.ids.pain, fx.pain)); }
        let here = self.place_index_at(fx.x, fx.y);
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

        // Stress rises with pain, surprise and danger on the mind, at the temperament's rate, and
        // faster in a tired mind.
        let tg = self.temperament.stress_gain() as i64 * (1000 + self.mods.fatigue as i64 / 2) / 1000;
        let stressor = pain_rise as i64 + surprise as i64 / 2 + self.st.act[self.ids.danger as usize] as i64 / 16;
        self.mods.stress = (self.mods.stress as i64 + stressor * tg / 1000).min(1000) as i32;

        // 3. Think, as deep as stress allows, in what budget compaction left.
        let mut p = stressed_params(&self.params, self.mods.stress, &self.temperament);
        if self.consol.forced > 0 {
            let left = (THINK_BUDGET - self.consol.forced).max(THINK_BUDGET / 10);
            p.frontier = (p.frontier as i64 * left / THINK_BUDGET).max(6) as usize;
        }
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
            self.hebbian(&ws, here.map(|pi| self.place_nodes[pi]));
            // Extinction (negative prediction error, Rescorla–Wagner): the expected thing wasn't
            // there, so the place↔thing links get a negative fast trace. Consolidated, it weakens
            // the slow link; faded unconsolidated, the old association comes back.
            for &(pi, k, e) in &absent_now {
                let (pn, n, c) = (self.place_nodes[pi], self.missing_node(pi, k), fox::kind_concept(&self.ids, k));
                // The thing, and what it stood for (its kind's concept: a burrow means rest).
                for (a, b) in [(pn, n), (n, pn), (pn, c), (c, pn)] {
                    for l in self.g.links(a) { if self.g.targets[l] == b {
                        let floor = -(self.g.slow[l] as i32);
                        self.fast[l] = (self.fast[l] as i32 - e / EXTINCT_RATE).max(floor) as i16;
                    } }
                }
                // ...and the place itself is where the loss was felt (error-driven, not Hebbian).
                if self.g.val[n as usize] > 0 {
                    for (a, b) in [(pn, self.ids.loss), (self.ids.loss, pn)] {
                        if let Some(l) = self.g.find(a, b, lt::FEELS) { self.fast[l] = (self.fast[l] as i32 + e / EXTINCT_RATE).min(1000 - self.g.slow[l] as i32) as i16; }
                    }
                }
            }
            if w.tick % FAST_FADE_EVERY == 0 { for f in self.fast.iter_mut() { *f -= *f / 128 + f.signum(); } }
            if w.tick % 8 == 0 { for v in self.volatility.iter_mut() { *v -= *v / 256; } }
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
        // 7. Idle time is consolidation time: a settled tick spends what the think didn't.
        self.consol.last_units = 0;
        let settled = max_error < IGNORE_BELOW && pain_rise == 0 && self.flee_left == 0 && self.mods.stress < 400
            && !matches!(action, Action::Flee { .. } | Action::Snap { .. });
        if self.learn && self.idle_consolidation && settled {
            let units = THINK_BUDGET - self.st.visits as i64 - self.consol.forced;
            if units > 0 { self.consolidate(units, false); self.consol.idle_ticks += 1; }
        }
        rep.searching = why.starts_with("search");
        rep.ws = ws; rep.hops = hops; rep.percepts = percepts; rep.inject = inject; rep.scores = scores;
        rep.action = Some(action); rep.why = why;
        rep.backlog = self.backlog(); rep.consolidated = self.consol.last_units;
        self.last = rep;
        action
    }

    /// Spend `units` of budget on the backlog: the strongest episode first (salient things are
    /// replayed first), then fast weights, largest first. Returns the units spent.
    pub fn consolidate(&mut self, mut units: i64, asleep: bool) -> i64 {
        let start = units;
        let share = if asleep { REPLAY_ASLEEP } else { REPLAY_AWAKE };
        loop {
            let Some(i) = (0..self.episodes.len()).max_by_key(|&i| (self.episodes[i].arousal, std::cmp::Reverse(self.episodes[i].tick))) else { break };
            let nodes = self.episode_nodes(&self.episodes[i]);
            let cost: i64 = nodes.iter().map(|&(u, _)| self.g.links(u).len() as i64).sum::<i64>() * REPLAY_HOPS;
            if cost > units { break; }
            units -= cost;
            self.replay(i, share);
            self.consol.replays += 1;
            if self.episodes[i].arousal < 50 { self.episodes.remove(i); }
        }
        if units >= TRANSFER_COST {
            let mut dirty: Vec<usize> = (0..self.fast.len()).filter(|&l| self.fast[l] != 0).collect();
            dirty.sort_by_key(|&l| (std::cmp::Reverse(self.fast[l].unsigned_abs()), l));
            for l in dirty {
                let t = (self.fast[l].unsigned_abs() as i64).min(units / TRANSFER_COST);
                if t <= 0 { break; }
                let sg = self.fast[l].signum() as i64;
                self.fast[l] -= (sg * t) as i16;
                self.g.slow[l] = (self.g.slow[l] as i64 + sg * t / TRANSFER_KEEP).clamp(0, 1000) as i16;
                units -= t * TRANSFER_COST;
                self.consol.moved += t;
            }
        }
        let spent = start - units;
        self.consol.last_units += spent;
        self.consol.total_units += spent;
        spent
    }

    /// Replay one episode: move `share`‰ of its remaining strength into the slow links between its
    /// nodes, and its feeling onto the anchored places and things in it (where and what, never the
    /// concepts), in proportion to how active each was. Feelings move toward the episode's
    /// valence, never past it.
    fn replay(&mut self, i: usize, share: i64) {
        let ep = self.episodes[i].clone();
        let s = ep.arousal as i64 * share / 1000;
        let nodes = self.episode_nodes(&ep);
        let top = nodes.iter().map(|x| x.1).max().unwrap_or(1).max(1) as i64;
        let anchored = |g: &Graph, n: u32| g.tags[n as usize] & tag::ANCHORED != 0;
        for &(u, au) in &nodes {
            for l in self.g.links(u) {
                let v = self.g.targets[l];
                if !nodes.iter().any(|x| x.0 == v) { continue; }
                if self.fast[l] < 0 {
                    // A link under extinction in a replayed episode: the replay keeps the
                    // extinction (the salient fact was that the thing wasn't there).
                    let t = (-(self.fast[l] as i64)).min(s / 2);
                    self.fast[l] += t as i16;
                    self.g.slow[l] = (self.g.slow[l] as i64 - t).max(0) as i16;
                    continue;
                }
                self.g.slow[l] = (self.g.slow[l] as i64 + s / 6).min(1000) as i16;
                if anchored(&self.g, u) || anchored(&self.g, v) {
                    let lv = self.g.lval[l] as i64;
                    self.g.lval[l] = (lv + (ep.valence as i64 - lv) * s / 1000 / 4) as i16;
                }
            }
            if anchored(&self.g, u) {
                let sh = au as i64 * 1000 / top;
                let v = self.g.val[u as usize] as i64;
                self.g.val[u as usize] = (v + (ep.valence as i64 - v) * s / 1000 * sh / 1000 / 3) as i16;
            }
        }
        self.episodes[i].arousal -= s as i32;
    }

    /// Lossy compaction: scale every fast weight and episode down to 90% of capacity. Small traces
    /// round away. Returns the work it took (one unit per trace touched), which the think loses.
    fn compact(&mut self, b: i64) -> i64 {
        let keep = BACKLOG_CAP * 900 / b; // per mille
        let mut work = 0;
        for f in self.fast.iter_mut().filter(|f| **f != 0) { *f = (*f as i64 * keep / 1000) as i16; work += 1; }
        for e in self.episodes.iter_mut() { e.arousal = (e.arousal as i64 * keep / 1000) as i32; work += 1; }
        self.episodes.retain(|e| e.arousal >= 50);
        self.consol.lost += b - self.backlog();
        work
    }

    /// How a place feels right now: a one-hop look-ahead over what it holds and what's attached
    /// to it, by link weight, each feeling scaled by the drives and modulators (a somatic marker:
    /// considering the place evokes its associations, on the mind or not).
    /// - bad things count more when aroused or in a low mood;
    /// - food counts in proportion to hunger, rest to fatigue;
    /// - other good things (comfort, the place itself, e.g. home) count more when aroused (safety);
    /// - neighbouring places count at half weight, by their own valence: a place near where
    ///   something bad happened feels a little worse.
    pub fn felt<F: Form>(&self, f: &F, p: u32) -> i32 { self.felt_with(f, p, &self.mods) }
    pub fn felt_with<F: Form>(&self, f: &F, p: u32, m: &Mods) -> i32 {
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
        // A tired fox is irritable: the bar falls with fatigue too.
        let bar = (danger as i64 * (1400 - self.temperament.aggression - self.mods.fatigue / 4) as i64 / 1000) as i32;
        if ws.contains(&self.ids.anger) && anger >= bar.max(flee_at) {
            if let Some(d) = percepts.iter().find(|p| p.entity == DOG && (p.x - fx.x).abs().max((p.y - fx.y).abs()) <= 2) {
                return (Action::Snap { entity: d.entity }, format!("anger {anger} against danger {danger} (bar {bar}): snap"), vec![]);
            }
        }
        if self.flee_left > 0 {
            return (Action::Flee { from_x: self.flee_from.0, from_y: self.flee_from.1, speed }, "still fleeing".into(), vec![]);
        }
        if (ws.contains(&self.ids.danger) && danger >= flee_at) || (ws.contains(&self.ids.anger) && anger >= flee_at) {
            // Flee from something bad: a surprising thing that feels negative (a dog, a fire), not
            // from a surprise that is welcome or neutral (a burrow where none was, a place).
            let src = surprised.iter().filter(|s| ws.contains(&s.0) && f.val(s.0) < 0).max_by_key(|s| (act(s.0), std::cmp::Reverse(s.0)));
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
        // Seen and wanted: an unfamiliar thing in sight (one the mind holds only as its kind's
        // concept) whose concept is on the mind pulls toward where it is seen, by the drive it
        // serves. (Known things pull through their place links below.)
        let drive = |n: u32| if f.tags(n) & tag::REST != 0 { self.mods.fatigue } else if f.tags(n) & tag::FOOD != 0 { self.mods.hunger } else { 0 };
        let mut seen_pull: Vec<(u32, i32)> = vec![];
        let mut novel_pull: Vec<(u32, i32)> = vec![];
        for p in percepts {
            if f.tags(p.node) & tag::ANCHORED != 0 || f.val(p.node) <= 0 || !ws.contains(&p.node) { continue; }
            // Novelty pulls too (intrigue): a surprising thing that isn't bad is worth a look.
            // It adds to the score directly: curiosity is about what's unknown, not how good the
            // place feels.
            let novelty = if p.error >= IGNORE_BELOW { p.error * NOVELTY_PULL / 1000 } else { 0 };
            if let Some(pi) = p.place { let pn = self.place_nodes[pi]; cands.push(pn); seen_pull.push((pn, act(p.node) * drive(p.node) / 1000)); novel_pull.push((pn, novelty)); }
        }
        cands.sort_unstable(); cands.dedup();
        let mut scores = vec![];
        for &p in &cands {
            let mut a = act(p) + seen_pull.iter().filter(|x| x.0 == p).map(|x| x.1).max().unwrap_or(0);
            // Attractive things on the mind pull toward the place they're at.
            // A thing to rest in pulls only as much as the fox is tired (as in `felt`).
            for &n in ws { if n != p && f.tags(n) & tag::ANCHORED != 0 && f.tags(n) & tag::PLACE == 0 && f.val(n) > 0 && Self::place_of(f, n) == Some(p) {
                a += if f.tags(n) & tag::REST != 0 { act(n) * self.mods.fatigue / 1000 } else { act(n) };
            } }
            scores.push((p, a, self.felt(f, p)));
        }
        // On my mind × wanted: a place that feels bad isn't walked to at all.
        let nov = |p: u32| novel_pull.iter().filter(|x| x.0 == p).map(|x| x.1).max().unwrap_or(0);
        let total = |s: &(u32, i32, i32)| (s.1 as i64 * s.2.max(0) as i64 / 1000) as i32 + if s.2 >= 0 { nov(s.0) } else { 0 };
        let best = scores.iter().max_by_key(|s| (total(s), std::cmp::Reverse(s.0))).copied();
        // Search: an unresolved prediction error (something expected is missing) and the best
        // option is the very place it went missing from, or nothing pulls. Staying there can't
        // resolve it, so the fox looks around: waypoints ring that place, widening as the search
        // goes on (area-restricted search). Nothing about homes is special here.
        if let Some(pz) = self.puzzle {
            let pp = self.place_nodes[pz.place];
            let best_ok = best.filter(|b| total(b) >= WALK_AT);
            if pz.strength >= SEARCH_AT && best_ok.map_or(true, |b| b.0 == pp) {
                let wp = match self.search_wp {
                    Some((x, y)) if (x - fx.x).abs().max((y - fx.y).abs()) > 1 => (x, y),
                    _ => self.next_waypoint(w, pz),
                };
                return (Action::WalkTo { x: wp.0, y: wp.1 }, format!("search for the missing {} (puzzle {})", pz.kind.name(), pz.strength), scores);
            }
        }
        // The best place is where I am: stay.
        if let (Some(b), Some(h)) = (best, here) { if b.0 == h {
            // Tired, with rest on the mind, at a place that holds rest: sleep (a nap).
            // Somewhere to sleep: a thing to rest in, seen here (a burrow), not just remembered.
            let hi = self.place_nodes.iter().position(|&p| p == h);
            let rests_here = percepts.iter().any(|p| p.kind == Kind::Den && p.place.is_some() && p.place == hi);
            let calm = self.mods.arousal < 300 && self.mods.stress < 400;
            let puzzled_here = self.puzzle.is_some_and(|pz| pz.strength >= SEARCH_AT && self.place_nodes[pz.place] == h);
            if !self.skip_sleep && calm && !puzzled_here && self.mods.fatigue >= NAP_AT && ws.contains(&self.ids.rest) && rests_here && fx.hunger < WAKE_HUNGER {
                return (Action::Sleep, format!("tired (fatigue {}), resting at {}: sleeps", self.mods.fatigue, name(h)), scores);
            }
            return (Action::Stay, format!("stay at {} (score {})", name(h), total(&b)), scores);
        } }
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

    /// The next search waypoint: a hashed bearing from the puzzle's place, at a radius that
    /// grows with each waypoint tried (4 cells, then up to 30).
    fn next_waypoint(&self, w: &World, pz: Puzzle) -> (i32, i32) {
        let (px, py) = self.place_xy(pz.place);
        let n = self.search_n as i32;
        let r = (4 + n * 3).min(30);
        let h = hash(w.seed, pz.since as u64, n as u64, 0x5ea2c);
        // Integer bearing: one of 16 directions.
        const DIRS: [(i32, i32); 16] = [(1000, 0), (924, 383), (707, 707), (383, 924), (0, 1000), (-383, 924), (-707, 707), (-924, 383),
            (-1000, 0), (-924, -383), (-707, -707), (-383, -924), (0, -1000), (383, -924), (707, -707), (924, -383)];
        let (dx, dy) = DIRS[(h % 16) as usize];
        ((px + dx * r / 1000).clamp(0, crate::world::GRID_W - 1), (py + dy * r / 1000).clamp(0, crate::world::GRID_H - 1))
    }

    /// Track the goal the world actually received (so commitment survives).
    pub fn commit(&mut self, w: &World, a: Action) {
        if self.last.searching {
            if let Action::WalkTo { x, y } = a { if self.search_wp != Some((x, y)) { self.search_wp = Some((x, y)); self.search_n += 1; } }
        }
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
            Action::Sleep => { self.goal = None; if !w.fox.asleep { self.napping = true; } }
        }
    }

    fn learn_expectations(&mut self, w: &World, percepts: &[Percept]) {
        let fx = &w.fox;
        for pi in 0..self.place_nodes.len() {
            if !self.place_learnt(pi) { continue; }
            let (plx, ply) = self.place_xy(pi);
            let d = d2(fx.x, fx.y, plx, ply);
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
    /// Links touching a place learn only while the fox is at that place (context-gated: what a
    /// place holds is learnt by being there, or in replay, not by thinking of it from afar).
    fn hebbian(&mut self, ws: &[u32], here: Option<u32>) {
        // Waking always encodes a little; arousal makes it much stronger.
        let a = self.mods.arousal as i64 + BASE_ENCODE;
        for &u in ws {
            for l in self.g.links(u) {
                let v = self.g.targets[l];
                // Loss is learnt from prediction error only (see `step`), never by co-activation.
                if v == u || !ws.contains(&v) || u == self.ids.loss || v == self.ids.loss { continue; }
                // A link under extinction (a negative trace: its prediction just failed) is not
                // re-strengthened by co-activation until that trace has gone.
                if self.fast[l] < 0 { continue; }
                let place = |n: u32| self.g.tags[n as usize] & tag::PLACE != 0;
                if (place(u) && Some(u) != here) || (place(v) && Some(v) != here) { continue; }
                let co = self.st.act[u as usize].min(1000) as i64 * self.st.act[v as usize].min(1000) as i64 / 1000;
                // Soft bound on the trace: a link that already carries a lot of today's potentiation
                // takes on little more. Well-known links still collect waking potentiation (it
                // adds nothing to what they mean, but it must be consolidated away: synaptic
                // homeostasis, Tononi & Cirelli).
                let room = (1000 - self.fast[l] as i64).max(0);
                let d = (co * a / HEBB_RATE * room / 1000) as i32;
                self.fast[l] = (self.fast[l] as i32 + d).min(1000) as i16;
            }
        }
    }

    /// A night. A sleeping fox spends `NIGHT_TICKS × SLEEP_BUDGET` on its backlog. A fox that
    /// doesn't sleep (`skip_sleep`) lies awake where it is: its mind runs `NIGHT_TICKS` waking
    /// ticks with the body held still, thinking and consolidating only in idle budget.
    pub fn night(&mut self, w: &World) {
        if !self.skip_sleep { self.sleep(); return; }
        let mut wd = w.clone();
        wd.fox.asleep = false;
        for _ in 0..NIGHT_TICKS { self.step(&wd); wd.tick += 1; }
        self.goal = None; self.flee_left = 0; self.search_wp = None;
    }

    /// Sleep: a night's consolidation budget spent on the backlog (episodes first, then fast
    /// weights), then a rested mind: nothing on it, arousal and stress gone, mood halved.
    pub fn sleep(&mut self) {
        self.consol.last_units = 0;
        self.consolidate(NIGHT_TICKS as i64 * SLEEP_BUDGET, true);
        self.st.clear();
        self.mods.arousal = 0;
        self.mods.stress = 0;
        self.mods.mood /= 2;
        self.mods.fatigue = self.pressure();
        self.goal = None;
        self.flee_left = 0;
        self.search_wp = None;
        self.recompile();
    }

    /// Total fast weight, by size (negative traces from extinction count too).
    pub fn fast_total(&self) -> i64 { self.fast.iter().map(|&f| (f as i64).abs()).sum() }
    /// Effective weight of the strongest link u → v of any type.
    pub fn weight(&self, u: u32, v: u32) -> (i32, i32) {
        self.g.links(u).filter(|&l| self.g.targets[l] == v).map(|l| (self.g.slow[l] as i32, self.fast[l] as i32)).max().unwrap_or((0, 0))
    }
    pub fn link_of(&self, u: u32, v: u32, ty: u8) -> Option<usize> { self.g.find(u, v, ty) }
    /// Edges of a node in the live graph form.
    pub fn live_edges(&self, u: u32) -> Vec<Edge> { let mut e = vec![]; Live { g: &self.g, fast: &self.fast }.edges(u, &mut e); e }
    pub fn gain_for(&self, ty: u8, to: u32) -> i32 { gain(&self.mods, ty, self.g.tags[to as usize], self.g.val[to as usize] as i32) }
}
