//! One mind, for any species: a slow graph (either form, through [`Store`]), sparse fast weights,
//! a few modulators, predictions per place, an episode buffer, a consolidation backlog, and sleep.
//! Ported from the fox in `spikes/thought-graph` (`mind.rs`); nothing in here knows about foxes.
//! What a species is (its concepts, the kinds of thing it can tell apart, its founding graph) comes
//! from a [`Species`]; what it senses comes in a [`Sense`] each tick. Each tick:
//!
//! 1. **Sense and predict.** Each thing in sight is compared with what the mind expects at that
//!    place. A match is ignored (a faint touch of activation). A prediction error injects
//!    activation into the thing and its place, and raises arousal. An expected landmark that is
//!    missing is an error too (absence), and may bring loss and a puzzle to search on.
//! 2. **Inject** body states (hunger, fatigue, pain) and the current place (context).
//! 3. **Think:** bounded, modulated spreading (`think::think`), as deep as stress allows.
//! 4. **Feel:** mood moves slowly toward the workspace's valence; arousal follows danger on mind.
//! 5. **Learn by day:** fast weights between co-active workspace nodes (Hebbian, scaled by
//!    arousal), extinction on absence, expectations, volatility, episodes of salient moments.
//! 6. **Act** from the workspace: snap, flee, eat, search, nap, stay or walk to a place.
//! 7. **Consolidate in idle time:** a settled tick spends the think budget it didn't use on the
//!    backlog (episodes first, then the largest fast weights).
//!
//! Sleep pressure: the backlog (fast weight plus what unreplayed episodes would still write) *is*
//! fatigue. Sleep is the same consolidation with a far bigger budget.
//!
//! **Activation is swap-in state**: the mind keeps its [`Parked`] active list between ticks; the
//! dense arrays are borrowed from the thread for the length of one `step`.

use crate::graph::{lt, tag, Anchor, LinkKey};
use crate::rng::hash;
use crate::store::{Base, Expanded, Link, Runtime, Store};
use crate::think::{self, tie, Activation, Edge, Form, Hop, Mods, Params, Parked};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Prediction errors below this are ignored.
pub const IGNORE_BELOW: i32 = 400;
/// A matched percept still gets this much activation: seen, not attended.
pub const SEEN_TOUCH: i32 = 40;
/// Context activation for the place the mind's body is at.
pub const PLACE_CONTEXT: i32 = 300;
/// Danger activation needed to flee (if a sensed source is on the mind).
pub const FLEE_AT: i32 = 300;
/// How much of the carried-over thought a full surprise wipes, per mille.
pub const INTERRUPT: i32 = 700;
/// A walk needs at least this score (activation × feeling / 1000).
pub const WALK_AT: i32 = 60;
/// A considered place that feels bad is pushed out of mind by this per mille.
pub const REJECT_DAMP: i32 = 500;
/// A new goal must beat the current one by this per mille.
pub const SWITCH_MARGIN: i32 = 250;
/// Work a waking tick can afford, in link visits.
pub const THINK_BUDGET: i64 = 600;
/// Work per tick of sleep.
pub const SLEEP_BUDGET: i64 = 8000;
/// A night's sleep, in ticks; it runs inside the `Night` tick.
pub const NIGHT_TICKS: u32 = 240;
/// Replaying an episode re-runs it through the graph: this many hops over its nodes' links.
pub const REPLAY_HOPS: i64 = 2;
/// Share of an episode's remaining strength one replay moves into the slow graph, per mille.
pub const REPLAY_AWAKE: i64 = 250;
pub const REPLAY_ASLEEP: i64 = 1000;
/// Budget units to consolidate one unit of fast weight.
pub const TRANSFER_COST: i64 = 60;
/// Backlog at which fatigue is full; over it, the mind compacts lossily.
pub const BACKLOG_CAP: i64 = 30000;
/// Waking encoding that happens even when calm, per mille of the arousal scale.
pub const BASE_ENCODE: i64 = 60;
/// Day learning rate (see `hebbian`).
pub const HEBB_RATE: i64 = 4000;
/// Extinction: per tick an expected thing is missing, its links gain a negative trace of error / this.
pub const EXTINCT_RATE: i32 = 6;
/// Fast weights fade by 1/128 every this many ticks (a half-life of ~9 days).
pub const FAST_FADE_EVERY: u32 = 32;
/// Naps start at this fatigue, and end below `WAKE_AT` (or when surprised, hurt or very hungry).
pub const NAP_AT: i32 = 500;
pub const WAKE_AT: i32 = 100;
pub const WAKE_HUNGER: i32 = 900;
/// Score a surprising, not-bad thing in sight adds to its place, per mille of its error.
pub const NOVELTY_PULL: i32 = 600;
/// A fading link of the mind's own below this slow weight (with no fast trace) is forgotten
/// outright: too weak to matter (the base itself stores weights in steps of 4).
pub const PRUNE_BELOW: i16 = 8;
/// Unresolved prediction error at which the mind searches.
pub const SEARCH_AT: i32 = 300;

// ------------------------------------------------------------------- stage 3: danger and intent
/// A held intent is topped up to this activation each tick it is in the workspace (rehearsal:
/// keeping a plan in mind). Out of the workspace it is not rehearsed and fades with everything else.
pub const INTENT_HOLD: i32 = 500;
/// An intent out of the workspace this many ticks running is dropped: the plan is gone.
pub const INTENT_LAPSE: u32 = 3;
/// Focus per mille of the intent's activation (so a fully held intent, 500, gives focus 1000).
pub const FOCUS_GAIN: i32 = 2000;
/// Matched, tracked companions (people) seen nearby: the activation they add grows with arousal,
/// `SEEN_TOUCH + arousal × COMPANY / 1000` (in alarm one looks to others; Schachter 1959).
pub const COMPANY: i32 = 1000;
/// Social buffering: the stress a tick adds is cut by the "ally" concept's activation × this / 1000.
pub const BUFFER: i32 = 1000;
/// Courage from company: the "ally" concept's activation × this / 1000 (to at most 1000) scales
/// standing and fighting up by `(1000 + courage)` and running away down by `(1000 - courage / 2)`.
pub const COURAGE: i32 = 4000;
/// A considered response (fight, call, freeze, hold, aid) needs at least this activation.
pub const OPTION_AT: i32 = 60;
/// A heard alarm is remembered (as somewhere to go or flee from) this many ticks.
pub const ALARM_KEEP: u32 = 40;
/// The five danger responses, and aid (going to someone who called).
pub const R_FLEE: u8 = 0;
pub const R_FIGHT: u8 = 1;
pub const R_CALL: u8 = 2;
pub const R_FREEZE: u8 = 3;
pub const R_HOLD: u8 = 4;
pub const R_AID: u8 = 5;
pub const R_NAMES: [&str; 6] = ["flee", "fight", "call", "freeze", "hold", "aid"];

/// A species' danger responses (stage 3): nodes in its graph, `u32::MAX` where it has none. The
/// reflex is read straight off danger (the fast path); the rest are options that thought must reach.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Responses { pub flee: u32, pub fight: u32, pub call: u32, pub freeze: u32, pub hold: u32, pub aid: u32,
    /// "Someone who stands with me": seen companions linked to it embolden (and buffer stress).
    pub ally: u32,
    /// "A cry for help", heard.
    pub cry: u32 }
impl Responses { pub fn node(&self, r: u8) -> u32 { [self.flee, self.fight, self.call, self.freeze, self.hold, self.aid][r as usize] } }

/// Stage 3 state: the held intent, a heard alarm, the response under way.
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Held {
    /// The node of the plan held in mind, if any.
    pub intent: Option<u32>,
    /// Ticks running the intent has been out of the workspace.
    pub lapse: u32,
    /// When the last intent was dropped (and which).
    pub dropped: Option<(u32, u32)>,
    /// Activation to inject next step (forming an intent).
    pub boost: Option<(u32, i32)>,
    /// The last alarm heard: (who cried, x, y, tick).
    pub alarm: Option<(u32, i32, i32, u32)>,
    /// The response chosen last tick (it keeps a margin, as a walk's goal does).
    pub response: Option<u8>,
    /// Staging and probes: hold stress at a value.
    pub pin_stress: Option<i32>,
    /// The last competition: (response, score) for each considered, and the reflex.
    pub last_scores: Vec<(u8, i32)>,
    /// The voice, a second channel beside the body: crying out this tick (the call node active
    /// enough while a threat is sensed), whatever the body does.
    pub calling: bool,
}

/// The design question for brief 09, made a setting: consolidating plain fast weight (the day's
/// routine strengthening) keeps 1/`sleep_keep` of it in the slow graph; the rest is downscaled
/// away (synaptic homeostasis). Replayed episodes are kept in full either way. The stage-0
/// spike used 8 (1/8); 3 (1/3) was the alternative it reported.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Config {
    pub sleep_keep: i64,
    /// Ablation: store the mind's own slow weights in steps of 4 too (as the base does), with
    /// consolidation's increments truncated. Off by default (the overlay keeps i16 weights).
    pub overlay_steps_of_4: bool,
    /// Day learning rounds **stochastically** (stage 2): a link's increment is the exact product
    /// `co-activation × (arousal + 60) × room`, divided once, and the remainder becomes one more
    /// unit with probability remainder / divisor, rolled from `hash(mind, tick, link, salt)`. The
    /// expected increment is exact, so a dimly held thought still learns, slowly. Off: the old
    /// chained divisions, which truncate calm learning of dim nodes to zero (stage 1's finding).
    pub stochastic_learning: bool,
    /// Forgetting: each night, every one of the mind's **own** links learnt during its life (one
    /// that touches a node added after `Mind::young_from`) loses this per mille of its slow weight,
    /// per link type (stochastically rounded, so the expected loss is exact). A link that falls
    /// below `PRUNE_BELOW`, with no fast trace, is removed. Use puts weight back through
    /// consolidation. All zero (no forgetting) unless a species sets it; the fox adds no nodes,
    /// so it is untouched.
    pub own_fade: [u16; lt::COUNT],
    /// Stage 3: **comfort is gated by need**, as food is by hunger and rest by fatigue. The pull of
    /// a place's comfort (the species' `Roles::comfort`) counts `need` per mille, where need is
    /// the largest of fatigue, stress and low mood. Off: the old rule, where comfort counted
    /// `(300 + arousal)` per mille whatever the body needed (so a thirsty person could stay home,
    /// stage 2's finding, and a surprise made home pull harder).
    pub comfort_gate: bool,
    /// Stage 3: **a cost of distance in choosing.** A place's score is discounted by
    /// `walk_cost / (walk_cost + d)` for a place `d` cells away (the value of a place halves at
    /// `walk_cost` cells: hyperbolic discounting of effort). 0: no cost (stage 2).
    pub walk_cost: i32,
    /// Stage 3: **commitment.** A goal being walked to is kept while nothing beats it by
    /// `SWITCH_MARGIN`, even when the best place is the one the body stands at (stage 2 let the
    /// place underfoot win outright, so a walk could stop one step out, which is the dithering
    /// that made stage 2 remove its walking cost). Off: the old rule.
    pub hold_goal: bool,
    /// Stage 3, ablation: a held intent biases spreading towards itself (focus). Off: the intent
    /// is still held and still predicts, but gives no focus.
    pub focus: bool,
}
impl Default for Config { fn default() -> Self { Config { sleep_keep: 8, overlay_steps_of_4: false, stochastic_learning: true, own_fade: [0; lt::COUNT], comfort_gate: true, walk_cost: WALK_COST, hold_goal: true, focus: true } } }

/// Default `Config::walk_cost`: a place this many cells away counts half.
pub const WALK_COST: i32 = 120;

/// An individual's temperament: seeded per individual, and expressed as brain structure and
/// modulator parameters, never as a script.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Temperament { pub reactivity: i32, pub aggression: i32 }
impl Temperament {
    pub const MEDIUM: Temperament = Temperament { reactivity: 500, aggression: 500 };
    /// From `hash(seed, individual, salt)`. Aggression is the mean of reactivity and its own roll.
    pub fn seeded(seed: u64, individual: u32) -> Temperament {
        let reactivity = (hash(seed, individual as u64, 1, 0x7e3f) % 1001) as i32;
        let own = (hash(seed, individual as u64, 2, 0x7e3f) % 1001) as i32;
        Temperament { reactivity, aggression: (reactivity + own) / 2 }
    }
    pub fn stress_gain(&self) -> i32 { 200 + self.reactivity * 6 / 5 }
    pub fn stress_cut(&self) -> i32 { 250 + self.reactivity * 7 / 10 }
}

/// The think parameters under stress: fewer hops and a smaller frontier.
pub fn stressed_params(base: &Params, stress: i32, t: &Temperament) -> Params {
    let cut = stress as i64 * t.stress_cut() as i64 / 1000;
    let mut p = *base;
    p.hops = (base.hops as i64 - (base.hops as i64 - 1) * cut / 950).max(1) as usize;
    p.frontier = (base.frontier as i64 * (1000 - cut * 85 / 100) / 1000).max(6) as usize;
    p
}

// ------------------------------------------------------------------------------------- species

/// The concepts every mind's machinery refers to (each species names its own nodes for them).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Roles { pub hunger: u32, pub fatigue: u32, pub pain: u32, pub food: u32, pub danger: u32, pub rest: u32, pub anger: u32, pub loss: u32,
    /// The comfort a place can give (gated by need, `Config::comfort_gate`); `u32::MAX` if none.
    pub comfort: u32,
    /// Danger responses (stage 3); `None`: the fox's old reflex (flee or snap).
    pub resp: Option<Responses> }

/// A kind of thing a species can tell apart by its senses.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct KindInfo {
    pub name: &'static str,
    /// The concept a thing of this kind is an instance of.
    pub concept: u32,
    /// Fixed things that define a place (seeing one where no place is known teaches a place).
    pub landmark: bool,
    /// Can be eaten.
    pub edible: bool,
    /// Can be slept in.
    pub shelter: bool,
    /// A creature that can hurt (pain marks it as a source; it can be snapped at).
    pub creature: bool,
    /// A moving thing the mind keeps track of (a person): seen within `TRACK_NEAR` cells of where
    /// the mind last placed it, it is predicted, whatever the place.
    pub tracked: bool,
}

/// A tracked thing seen this close to where the mind last saw it is no surprise.
pub const TRACK_NEAR: i32 = 3;

/// What a mind's senses and body give it each tick.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Sense {
    pub tick: u32,
    pub seed: u64,
    pub body: Body,
    pub seen: Vec<Seen>,
    /// Activation a species' own senses add this tick, as (node, amount): what was heard or read,
    /// already grounded in this mind's own nodes. Empty for the fox.
    pub cues: Vec<(u32, i32)>,
    /// Stage 3: alarms heard this tick, located by ear: (the node of who cried, x, y).
    pub alarms: Vec<(u32, i32, i32)>,
}
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Body { pub x: i32, pub y: i32, pub hunger: i32, pub pain: i32, pub asleep: bool,
    /// Time-of-day sleepiness (0..1000), the circadian half of the two-process model of sleep
    /// (Borbély 1982): fatigue is the larger of this and sleep pressure (the backlog). 0 for the fox.
    pub drowsy: i32 }
/// A thing in sight: its kind (an index into the species' kinds), where, whether it came closer.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Seen { pub id: u32, pub kind: u8, pub x: i32, pub y: i32, pub approaching: bool, pub available: bool }

/// Sizes of the world as a species perceives it.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Geometry { pub sight: i32, pub place_radius: i32, pub grid_w: i32, pub grid_h: i32 }

/// A species: its shared founding graph and what its mind's machinery needs to know about it.
#[derive(Clone)]
pub struct Species {
    pub base: Arc<Base>,
    pub roles: Roles,
    pub kinds: Arc<Vec<KindInfo>>,
    /// Founding places, then `spare_places` unlearnt slots.
    pub place_nodes: Vec<u32>,
    pub spare_places: usize,
    /// The anchor x of a place slot not yet learnt.
    pub unlearnt: i16,
    /// Near-link weight between places `d` cells apart.
    pub near_weight: fn(i32) -> i32,
    /// (entity id, node) of the things a mind is born knowing, and their kinds.
    pub entity_nodes: Vec<(u32, u32)>,
    pub entity_kinds: Vec<u8>,
    /// Founding expectations: (place index, kind) seen there (900).
    pub expect: Vec<(usize, u8)>,
    pub geo: Geometry,
    /// The founding links an individual's temperament sets: (from, to, type, weight).
    pub temperament: fn(&Roles, &Temperament) -> Vec<(u32, u32, u8, i32)>,
    /// Feelings a place the mind adds itself gets empty `feels` slots to (both ways).
    pub feelings: Vec<u32>,
}

// ------------------------------------------------------------------------------------- state

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Episode { pub tick: u32, pub place: Option<u32>, pub ws: Vec<(u32, i32)>, pub valence: i32, pub arousal: i32 }

/// What consolidation did: per tick, and in total.
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub struct Consol { pub last_units: i64, pub total_units: i64, pub idle_ticks: u32, pub replays: u32, pub moved: i64, pub lost: i64, pub forced: i64 }

/// An unresolved prediction error: an expected thing missing from a place.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Puzzle { pub place: usize, pub kind: u8, pub strength: i32, pub since: u32 }

#[derive(Clone, Debug)]
pub struct Percept { pub entity: u32, pub kind: u8, pub x: i32, pub y: i32, pub place: Option<usize>, pub approaching: bool, pub node: u32, pub expected: i32, pub error: i32 }

/// What a mind does this tick.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Action {
    Stay,
    /// Walk one cell toward a point.
    WalkTo { x: i32, y: i32 },
    /// Run directly away from a point, `speed` cells a tick.
    Flee { from_x: i32, from_y: i32, speed: i32 },
    /// Stand and snap at a creature within two cells.
    Snap { entity: u32 },
    /// Eat from a thing within one cell.
    Eat { entity: u32 },
    Sleep,
    /// Stage 3. Stand one's ground facing a creature (braced).
    Hold { entity: u32 },
    /// Stage 3. Go still.
    Freeze,
    /// Stage 3. Cry out for help (the species' own layer turns it into a sound).
    Call,
    /// Stage 3. Close on a creature to fight it (a boar's charge, a person's advance).
    Charge { entity: u32, x: i32, y: i32 },
}

/// What happened in the mind this tick (kept for traces and tests).
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub ws: Vec<u32>,
    pub hops: Vec<Hop>,
    pub percepts: Vec<Percept>,
    pub inject: Vec<(u32, i32)>,
    pub scores: Vec<(u32, i32, i32)>,
    pub action: Option<Action>,
    pub why: String,
    pub hops_run: u32,
    pub frontier_cap: usize,
    pub visits: u32,
    pub absent: Vec<(usize, u8, i32)>,
    pub learnt: Vec<u32>,
    pub backlog: i64,
    pub consolidated: i64,
    pub searching: bool,
}

#[derive(Clone, Debug)]
pub struct Mind<S: Store = Runtime> {
    pub store: S,
    /// Who this mind is (seeds its own nodes' keys).
    pub who: u64,
    pub st: Parked,
    pub mods: Mods,
    pub params: Params,
    pub cfg: Config,
    pub roles: Roles,
    pub kinds: Arc<Vec<KindInfo>>,
    pub geo: Geometry,
    pub unlearnt: i16,
    pub spare_places: usize,
    pub near_weight: fn(i32) -> i32,
    pub place_nodes: Vec<u32>,
    pub entity_nodes: Vec<(u32, u32)>,
    pub entity_kinds: Vec<u8>,
    /// Per place, per key (kind × approaching): how much the mind expects to see it there.
    pub expect: Vec<Vec<i16>>,
    pub episodes: Vec<Episode>,
    pub goal: Option<u32>,
    pub flee_left: i32,
    pub flee_from: (i32, i32),
    pub prev_pain: i32,
    pub trace: bool,
    pub last: Report,
    pub learn: bool,
    pub pin_arousal: Option<i32>,
    pub pin_mood: Option<i32>,
    pub skip_sleep: bool,
    pub temperament: Temperament,
    pub idle_consolidation: bool,
    pub consol: Consol,
    pub puzzle: Option<Puzzle>,
    pub search_wp: Option<(i32, i32)>,
    pub search_n: u32,
    pub napping: bool,
    /// Learnt volatility per kind (0..1000).
    pub volatility: Vec<i16>,
    pub feelings: Vec<u32>,
    /// Nodes from this id on were learnt in the mind's life (not founding): links touching them
    /// can fade (`Config::own_fade`).
    pub young_from: u32,
    /// Nights slept (seeds the rounding of forgetting).
    pub nights: u32,
    /// Stage 3: intent, alarm, the response under way.
    pub held: Held,
    /// Stage 3: this species closes on what it fights (a boar charges); others only snap in reach.
    pub charges: bool,
}

impl<S: Store> Hash for Mind<S> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.temperament.hash(h); self.store.hash_into(h); self.st.hash(h); self.mods.hash(h);
        self.expect.hash(h); self.episodes.hash(h); self.goal.hash(h); self.flee_left.hash(h); self.flee_from.hash(h); self.prev_pain.hash(h);
        self.consol.hash(h); self.puzzle.hash(h); self.search_wp.hash(h); self.search_n.hash(h); self.napping.hash(h); self.volatility.hash(h);
        // Settings too: two minds that would behave differently next tick must not hash alike.
        self.cfg.hash(h); self.params.hash(h); self.learn.hash(h); self.skip_sleep.hash(h); self.pin_arousal.hash(h); self.pin_mood.hash(h); self.idle_consolidation.hash(h);
        self.place_nodes.hash(h); self.entity_nodes.hash(h); self.entity_kinds.hash(h); self.young_from.hash(h); self.nights.hash(h); self.held.hash(h); self.charges.hash(h);
    }
}

/// Expectation key: kind × (approaching or not).
#[inline] pub fn key(kind: u8, approaching: bool) -> usize { kind as usize * 2 + approaching as usize }
#[inline] pub fn d2(ax: i32, ay: i32, bx: i32, by: i32) -> i32 { (ax - bx) * (ax - bx) + (ay - by) * (ay - by) }
#[inline] pub fn dist(ax: i32, ay: i32, bx: i32, by: i32) -> i32 { (d2(ax, ay, bx, by) as u32).isqrt() as i32 }

impl Mind<Runtime> {
    /// A new mind of a species, on the runtime form: the shared base, an overlay holding only the
    /// founding links its temperament changes.
    pub fn new(sp: &Species, temperament: Temperament, who: u64) -> Mind<Runtime> {
        let mut store = Runtime::new(sp.base.clone());
        for (u, v, ty, w) in (sp.temperament)(&sp.roles, &temperament) {
            let k = store.find(u, v, ty).expect("temperament link in the base");
            // Founding knowledge is held at the base's precision (steps of 4), so an individual's
            // temperament links are stored the same way; it differs only where that differs.
            store.set_slow(k, crate::store::quant_w(w as i16));
        }
        Mind::with_store(sp, store, temperament, who)
    }
    /// The inspectable form of this mind (every other part of it unchanged).
    pub fn expand(&self) -> Mind<Expanded> { self.map_store(self.store.expand()) }
}
impl Mind<Expanded> {
    pub fn compile(&self, base: &Arc<Base>) -> Mind<Runtime> { self.map_store(self.store.compile(base)) }
}

impl<S: Store> Mind<S> {
    pub fn with_store(sp: &Species, store: S, temperament: Temperament, who: u64) -> Mind<S> {
        let nk = sp.kinds.len();
        let mut m = Mind {
            store, who, st: Parked::default(), mods: Mods::default(), params: Params::default(), cfg: Config::default(), roles: sp.roles, kinds: sp.kinds.clone(),
            geo: sp.geo, unlearnt: sp.unlearnt, spare_places: sp.spare_places, near_weight: sp.near_weight,
            place_nodes: sp.place_nodes.clone(), entity_nodes: sp.entity_nodes.clone(), entity_kinds: sp.entity_kinds.clone(),
            expect: vec![vec![0i16; nk * 2]; sp.place_nodes.len()], episodes: vec![], goal: None, flee_left: 0, flee_from: (0, 0), prev_pain: 0,
            trace: false, last: Report::default(), learn: true, pin_arousal: None, pin_mood: None, skip_sleep: false, temperament,
            idle_consolidation: true, consol: Consol::default(), puzzle: None, search_wp: None, search_n: 0, napping: false, volatility: vec![0; nk],
            feelings: sp.feelings.clone(), young_from: 0, nights: 0, held: Held::default(), charges: false,
        };
        m.young_from = m.store.len() as u32;
        for &(pi, k) in &sp.expect { m.expect[pi][key(k, false)] = 900; }
        m
    }
    /// The same mind with its long-term store in another form.
    pub fn map_store<T: Store>(&self, store: T) -> Mind<T> {
        Mind { store, who: self.who, st: self.st.clone(), mods: self.mods, params: self.params, cfg: self.cfg, roles: self.roles, kinds: self.kinds.clone(), geo: self.geo,
            unlearnt: self.unlearnt, spare_places: self.spare_places, near_weight: self.near_weight, place_nodes: self.place_nodes.clone(), entity_nodes: self.entity_nodes.clone(),
            entity_kinds: self.entity_kinds.clone(), expect: self.expect.clone(), episodes: self.episodes.clone(), goal: self.goal, flee_left: self.flee_left, flee_from: self.flee_from,
            prev_pain: self.prev_pain, trace: self.trace, last: self.last.clone(), learn: self.learn, pin_arousal: self.pin_arousal, pin_mood: self.pin_mood, skip_sleep: self.skip_sleep,
            temperament: self.temperament, idle_consolidation: self.idle_consolidation, consol: self.consol, puzzle: self.puzzle, search_wp: self.search_wp, search_n: self.search_n,
            napping: self.napping, volatility: self.volatility.clone(), feelings: self.feelings.clone(), young_from: self.young_from, nights: self.nights, held: self.held.clone(), charges: self.charges }
    }

    /// A hash of everything the mind holds (form-independent).
    pub fn state_hash(&self) -> u64 { let mut h = crate::rng::StateHasher::new(self.who); self.hash(&mut h); h.finish() }

    /// Bytes the mind holds between ticks, by part: (shared base, own long-term, fast weights,
    /// state: active list, expectations, episodes, volatility and the rest).
    pub fn bytes(&self) -> (usize, usize, usize, usize) {
        let b = self.store.bytes();
        let ep: usize = self.episodes.iter().map(|e| 20 + e.ws.len() * 8).sum();
        let state = self.st.bytes() + self.expect.iter().map(|e| e.len() * 2).sum::<usize>() + ep + self.volatility.len() * 2
            + std::mem::size_of::<Mods>() + 64 + self.place_nodes.len() * 4 + self.entity_nodes.len() * 9
            // The last tick's report, kept for traces (could be dropped outside debugging).
            + std::mem::size_of::<Report>() + self.last.ws.len() * 4 + self.last.percepts.len() * std::mem::size_of::<Percept>()
            + self.last.inject.len() * 8 + self.last.scores.len() * 12 + self.last.why.len() + self.last.absent.len() * 16
            // Stage 3: intent, alarm, the response under way and its last scores.
            + std::mem::size_of::<Held>() + self.held.last_scores.len() * 8;
        (b.shared, b.own, b.fast, state)
    }

    fn calm_factor(&self, k: u8) -> i32 { 1000 - self.volatility[k as usize] as i32 * 7 / 10 }
    fn volatile_event(&mut self, k: u8) { let v = &mut self.volatility[k as usize]; *v += (1000 - *v) / 4; }
    fn concept(&self, k: u8) -> u32 { self.kinds[k as usize].concept }
    fn tk(&self, n: u32) -> u64 { tie(&self.store, &self.params, n) }

    pub fn place_learnt(&self, pi: usize) -> bool { self.store.anchor(self.place_nodes[pi]).is_some_and(|a| a.xy().0 != self.unlearnt as i32) }
    fn place_xy(&self, pi: usize) -> (i32, i32) { self.store.anchor(self.place_nodes[pi]).map(|a| a.xy()).unwrap_or((-999, -999)) }

    /// The place a point is at, by the mind's own map: the nearest learnt place within the
    /// species' place radius, ties to the lower index.
    pub fn place_index_at(&self, x: i32, y: i32) -> Option<usize> {
        let r = self.geo.place_radius;
        let mut best: Option<(i32, usize)> = None;
        for pi in 0..self.place_nodes.len() {
            if !self.place_learnt(pi) { continue; }
            let (px, py) = self.place_xy(pi);
            let d = d2(x, y, px, py);
            if d <= r * r && best.is_none_or(|(bd, _)| d < bd) { best = Some((d, pi)); }
        }
        best.map(|b| b.1)
    }

    /// Where the mind believes home is: the learnt place that feels best to a tired, calm mind.
    pub fn home(&self) -> u32 {
        let tired = Mods { fatigue: 1000, ..Mods::default() };
        (0..self.place_nodes.len()).filter(|&pi| self.place_learnt(pi)).map(|pi| self.place_nodes[pi])
            .max_by_key(|&p| (self.felt_with(&self.store, p, &tired), std::cmp::Reverse(self.tk(p)))).unwrap()
    }
    pub fn home_xy(&self) -> (i32, i32) { self.store.anchor(self.home()).unwrap().xy() }

    /// Learn a new place at a cell: take a spare slot (or forget the learnt place that matters
    /// least), anchor it there, and link it by real distance to every place already known.
    pub fn learn_place(&mut self, x: i32, y: i32) -> Option<usize> {
        let n0 = self.place_nodes.len() - self.spare_places;
        let pi = match (n0..self.place_nodes.len()).find(|&pi| !self.place_learnt(pi)) {
            Some(pi) => pi,
            // A species without spare slots grows a new place node instead.
            None if self.spare_places == 0 => { let n = self.place_nodes.len(); return Some(self.add_place(&format!("place {}", n + 1), x, y, None)); }
            None => {
                let here = self.place_index_at(x, y);
                let weight = |pi: usize| -> i64 { self.store.links_vec(self.place_nodes[pi]).iter().map(|l| l.slow as i64 + l.fast as i64).sum() };
                let pi = (n0..self.place_nodes.len()).filter(|&pi| Some(pi) != here).min_by_key(|&pi| (weight(pi), pi))?;
                let pn = self.place_nodes[pi];
                let mut ls = vec![];
                for u in 0..self.store.len() as u32 {
                    ls.clear(); self.store.links(u, &mut ls);
                    for l in &ls { if u == pn || l.to == pn { self.store.set_slow(l.key, 0); self.store.set_fast(l.key, 0); self.store.set_lval(l.key, 0); } }
                }
                self.store.set_val(pn, 0);
                pi
            }
        };
        let pn = self.place_nodes[pi];
        self.store.set_anchor(pn, Anchor::Place { x: x as i16, y: y as i16 });
        for qi in 0..self.place_nodes.len() {
            if qi == pi || !self.place_learnt(qi) { continue; }
            let (qx, qy) = self.place_xy(qi);
            let wgt = (self.near_weight)(dist(x, y, qx, qy)) as i16;
            let q = self.place_nodes[qi];
            for (a, b) in [(pn, q), (q, pn)] { if let Some(l) = self.store.find(a, b, lt::NEAR) { self.store.set_slow(l, wgt); } }
        }
        let nk = self.kinds.len();
        self.expect[pi] = vec![0; nk * 2];
        Some(pi)
    }

    /// Add a new place node of the mind's own (a place it found, or one it was told of), anchored
    /// at a cell, with empty `feels` slots to the species' feelings. It is linked by real distance
    /// (`near_weight`) to the learnt places listed in `near`, or to every learnt place if `None`.
    /// Returns its place index.
    pub fn add_place(&mut self, name: &str, x: i32, y: i32, near: Option<&[usize]>) -> usize {
        let pn = self.store.add_node(name, 0, tag::PLACE, Some(Anchor::Place { x: x as i16, y: y as i16 }), self.who);
        for c in self.feelings.clone() { self.store.add_link(pn, c, lt::FEELS, 0, 0); self.store.add_link(c, pn, lt::FEELS, 0, 0); }
        let pi = self.place_nodes.len();
        let others: Vec<usize> = match near { Some(v) => v.to_vec(), None => (0..pi).filter(|&q| self.place_learnt(q)).collect() };
        self.place_nodes.push(pn);
        self.expect.push(vec![0; self.kinds.len() * 2]);
        for qi in others { self.link_near(pi, qi); }
        pi
    }
    /// Link two learnt places by their real distance (both ways), or reweigh the link if it exists.
    pub fn link_near(&mut self, pi: usize, qi: usize) {
        if pi == qi || !self.place_learnt(pi) || !self.place_learnt(qi) { return; }
        let ((x, y), (qx, qy)) = (self.place_xy(pi), self.place_xy(qi));
        let w = (self.near_weight)(dist(x, y, qx, qy));
        let (p, q) = (self.place_nodes[pi], self.place_nodes[qi]);
        for (a, b) in [(p, q), (q, p)] {
            match self.store.find(a, b, lt::NEAR) { Some(k) => self.store.set_slow(k, w as i16), None => if w > 0 { self.store.add_link(a, b, lt::NEAR, w, 0); } }
        }
    }

    pub fn node(&self, name: &str) -> u32 { self.store.id(name) }
    pub fn name(&self, n: u32) -> String { self.store.name(n).to_string() }
    pub fn act(&self, n: u32) -> i32 { self.st.get(n) }

    /// Recognition by look and place, never by identity.
    fn recognise(&self, vis: &[Seen]) -> Vec<u32> {
        let r2 = self.geo.place_radius * self.geo.place_radius;
        let mut node: Vec<u32> = vis.iter().map(|e| self.concept(e.kind)).collect();
        for (i, &(_, n)) in self.entity_nodes.iter().enumerate() {
            let k = self.entity_kinds[i];
            let (ax, ay) = self.store.anchor(n).map(|a| a.xy()).unwrap_or((0, 0));
            let creature = self.store.tags(n) & tag::CREATURE != 0;
            let best = vis.iter().enumerate().filter(|(_, e)| e.kind == k)
                .map(|(j, e)| (d2(e.x, e.y, ax, ay), e.id, j))
                .filter(|&(d, _, _)| creature || d <= r2).min();
            if let Some((_, _, j)) = best { node[j] = n; }
        }
        node
    }

    /// Senses and predictions: what the mind sees, and how surprising each thing is.
    pub fn perceive(&self, s: &Sense) -> Vec<Percept> {
        if s.body.asleep { return vec![]; }
        let nodes = self.recognise(&s.seen);
        s.seen.iter().zip(nodes).map(|(e, node)| {
            let place = self.place_index_at(e.x, e.y);
            let k = key(e.kind, e.approaching);
            let mut expected = place.map_or(0, |p| self.expect[p][k] as i32);
            // Stage 3: a held intent predicts what it is about (a hunter out for a boar expects one).
            if let Some(i) = self.held.intent {
                let ia = self.st.get(i).min(INTENT_HOLD);
                let c = self.concept(e.kind);
                let w = self.store.links_vec(i).iter().filter(|l| l.to == c || l.to == node).map(|l| (l.slow as i32 + l.fast as i32).max(0)).max().unwrap_or(0);
                expected = expected.max((w * ia / INTENT_HOLD).min(900));
            }
            if self.kinds[e.kind as usize].tracked {
                if let Some(a) = self.store.anchor(node) { let (ax, ay) = a.xy(); if d2(e.x, e.y, ax, ay) <= TRACK_NEAR * TRACK_NEAR { expected = expected.max(900); } }
            }
            Percept { entity: e.id, kind: e.kind, x: e.x, y: e.y, place, approaching: e.approaching, node, expected, error: 1000 - expected }
        }).collect()
    }

    /// Absence: an expected landmark missing from a place in clear view (within half of sight).
    pub fn absences(&self, s: &Sense, percepts: &[Percept]) -> Vec<(usize, u8, i32)> {
        let half = self.geo.sight / 2;
        let mut out = vec![];
        for pi in 0..self.place_nodes.len() {
            if !self.place_learnt(pi) { continue; }
            let (px, py) = self.place_xy(pi);
            if d2(s.body.x, s.body.y, px, py) > half * half { continue; }
            for k in (0..self.kinds.len() as u8).filter(|&k| self.kinds[k as usize].landmark) {
                let e = self.expect[pi][key(k, false)] as i32;
                if e >= IGNORE_BELOW && !percepts.iter().any(|p| p.kind == k && p.place == Some(pi)) { out.push((pi, k, e)); }
            }
        }
        out
    }

    fn missing_node(&self, pi: usize, k: u8) -> u32 {
        let (px, py) = self.place_xy(pi);
        let r2 = self.geo.place_radius * self.geo.place_radius;
        self.entity_nodes.iter().enumerate().find(|(i, (_, n))| self.entity_kinds[*i] == k
            && self.store.anchor(*n).is_some_and(|a| { let (x, y) = a.xy(); d2(x, y, px, py) <= r2 }))
            .map(|(_, (_, n))| *n).unwrap_or_else(|| self.concept(k))
    }

    /// Unconsolidated work: fast weight plus what each unreplayed episode would still write.
    pub fn backlog(&self) -> i64 { self.store.fast_total() + self.episodes.iter().map(|e| self.episode_weight(e)).sum::<i64>() }
    fn episode_nodes(&self, ep: &Episode) -> Vec<(u32, i32)> {
        let mut nodes = ep.ws.clone();
        let top = nodes.iter().map(|x| x.1).max().unwrap_or(1).max(1);
        if let Some(p) = ep.place { if !nodes.iter().any(|x| x.0 == p) { nodes.push((p, top)) } }
        nodes
    }
    pub fn episode_weight(&self, ep: &Episode) -> i64 {
        let nodes = self.episode_nodes(ep);
        let mut links = 0i64;
        let mut ls = vec![];
        for &(u, _) in &nodes { ls.clear(); self.store.links(u, &mut ls); for l in &ls { if nodes.iter().any(|x| x.0 == l.to) { links += 1; } } }
        links * ep.arousal as i64 / 6
    }
    /// Sleep pressure: fatigue (0..1000) read from the backlog.
    pub fn pressure(&self) -> i32 { (self.backlog() * 1000 / BACKLOG_CAP).clamp(0, 1000) as i32 }

    /// Write a slow weight the mind learnt (in steps of 4 under the ablation).
    fn write_slow(&mut self, key: LinkKey, w: i64) {
        let w = w.clamp(0, 1000);
        let w = if self.cfg.overlay_steps_of_4 { w / 4 * 4 } else { w };
        self.store.set_slow(key, w as i16);
    }

    /// One tick of the mind. Reads what it senses, returns its action. Dense activation is
    /// borrowed from this thread for the length of the step.
    pub fn step(&mut self, s: &Sense) -> Action {
        let mut a = Activation::attach(&self.st, self.store.len());
        let act = self.step_with(s, &mut a);
        self.st = a.detach();
        act
    }

    fn step_with(&mut self, s: &Sense, st: &mut Activation) -> Action {
        let fx = s.body;
        self.mods.hunger = fx.hunger;
        let mut rep = Report::default();
        let b = self.backlog();
        self.consol.forced = if b > BACKLOG_CAP && self.learn { self.compact(b) } else { 0 };
        self.mods.fatigue = self.pressure().max(fx.drowsy);
        if fx.asleep { rep.action = Some(Action::Sleep); rep.why = "asleep".into(); rep.backlog = self.backlog(); self.last = rep; return Action::Sleep; }
        if self.napping {
            let woken = self.perceive(s).iter().any(|p| p.error >= IGNORE_BELOW) || fx.pain > self.prev_pain || fx.hunger >= WAKE_HUNGER;
            if woken || self.mods.fatigue < WAKE_AT { self.napping = false; } else {
                self.consol.last_units = 0;
                self.consolidate(SLEEP_BUDGET, true);
                self.mods.fatigue = self.pressure().max(fx.drowsy);
                self.mods.arousal -= self.mods.arousal / 10; self.mods.stress -= self.mods.stress / 20;
                st.damp(900);
                rep.action = Some(Action::Sleep); rep.why = format!("napping (fatigue {})", self.mods.fatigue);
                rep.backlog = self.backlog(); rep.consolidated = self.consol.last_units;
                self.last = rep; return Action::Sleep;
            }
        }

        // 1–2. Sense, predict, inject. A landmark where the mind knows no place teaches it one.
        if self.learn {
            for e in &s.seen {
                if self.kinds[e.kind as usize].landmark && self.place_index_at(e.x, e.y).is_none() {
                    if let Some(pi) = self.learn_place(e.x, e.y) { rep.learnt.push(self.place_nodes[pi]); }
                }
            }
        }
        // A species that grows place nodes may have just added one.
        st.ensure(self.store.len());
        let percepts = self.perceive(s);
        let mut inject: Vec<(u32, i32)> = vec![];
        let mut surprised: Vec<(u32, i32, i32)> = vec![];
        let mut surprise = 0;
        let mut max_error = 0;
        for p in &percepts {
            if self.entity_nodes.iter().any(|&(_, n)| n == p.node) {
                self.store.set_anchor(p.node, Anchor::Entity { id: p.entity, x: p.x as i16, y: p.y as i16 });
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
                surprise += p.error / 2 * self.calm_factor(p.kind) / 1000;
                max_error = max_error.max(p.error);
                if self.kinds[p.kind as usize].landmark && rep.learnt.iter().any(|&n| p.place.is_some_and(|pi| self.place_nodes[pi] == n)) { self.volatile_event(p.kind); }
            }
        }
        // A newly learnt place: the thing seen there is linked to it (a first, fast trace).
        for p in &percepts {
            if let Some(pi) = p.place { if rep.learnt.contains(&self.place_nodes[pi]) {
                let pn = self.place_nodes[pi];
                for (a, b) in [(pn, p.node), (p.node, pn)] {
                    if let Some(l) = self.store.links_vec(a).into_iter().find(|l| l.to == b) { self.store.set_fast(l.key, (l.fast as i32 + p.error / 2).min(1000) as i16); }
                }
            }}
        }
        // Absence: something expected isn't there.
        let absent = self.absences(s, &percepts);
        for &(pi, k, e) in &absent {
            let n = self.missing_node(pi, k);
            inject.push((n, e));
            inject.push((self.place_nodes[pi], e / 2));
            let nv = self.store.val(n);
            if nv > 0 { inject.push((self.roles.loss, e * nv / 600)); }
            surprise += e / 2 * self.calm_factor(k) / 1000;
            max_error = max_error.max(e);
            let fresh = self.puzzle.is_none_or(|pz| pz.place != pi || pz.kind != k);
            if fresh { self.volatile_event(k); }
            let gain = e / 8 * (1000 + self.volatility[k as usize] as i32 * 2) / 1000;
            let pz = self.puzzle.get_or_insert(Puzzle { place: pi, kind: k, strength: 0, since: s.tick });
            if pz.place != pi || pz.kind != k { *pz = Puzzle { place: pi, kind: k, strength: pz.strength / 2, since: s.tick }; }
            pz.strength = (pz.strength + gain).min(1000);
        }
        if let Some(pz) = &mut self.puzzle {
            if percepts.iter().any(|p| p.kind == pz.kind && p.place != Some(pz.place)) { pz.strength -= pz.strength / 8 + 20; }
            pz.strength -= pz.strength / 400 + 1;
            if pz.strength <= 0 { self.puzzle = None; self.search_wp = None; self.search_n = 0; }
        }
        let absent_now = absent.clone();
        rep.absent = absent;
        if fx.hunger > 100 { inject.push((self.roles.hunger, fx.hunger / 2)); }
        let fatigue = self.mods.fatigue;
        if fatigue > 300 { inject.push((self.roles.fatigue, fatigue / 3)); }
        if fx.pain > 0 { inject.push((self.roles.pain, fx.pain)); }
        let here = self.place_index_at(fx.x, fx.y);
        if let Some(pl) = here { inject.push((self.place_nodes[pl], PLACE_CONTEXT)); }
        inject.extend_from_slice(&s.cues);
        let pain_rise = (fx.pain - self.prev_pain).max(0);
        self.prev_pain = fx.pain;
        if pain_rise > 0 { for p in &percepts { if self.kinds[p.kind as usize].creature { surprised.push((p.node, p.x, p.y)); } } }
        let a_before = self.mods.arousal;
        self.mods.arousal = (self.mods.arousal + surprise + pain_rise).min(1000);
        if let Some(a) = self.pin_arousal { self.mods.arousal = a; }
        if let Some(m) = self.pin_mood { self.mods.mood = m; }
        // Stage 3: a companion seen where expected draws more attention the more alarmed the mind
        // is now (in alarm one looks to others).
        for p in &percepts { if p.error < IGNORE_BELOW && self.kinds[p.kind as usize].tracked && self.mods.arousal > 0 { inject.push((p.node, self.mods.arousal * COMPANY / 1000)); } }

        if max_error > 0 { st.damp(1000 - max_error * INTERRUPT / 1000); }

        // Stage 3: alarms heard; the held intent rehearsed (topped up while it is in the workspace).
        for &(n, x, y) in &s.alarms { self.held.alarm = Some((n, x, y, s.tick)); }
        if self.held.alarm.is_some_and(|a| s.tick > a.3 + ALARM_KEEP) { self.held.alarm = None; }
        self.mods.focus = 0;
        if let Some(i) = self.held.intent {
            let carried = (st.act[i as usize] as i64 * self.params.persist as i64 / 1000) as i32;
            let mut a0 = carried;
            if let Some((n, a)) = self.held.boost.take() { inject.push((n, a)); if n == i { a0 += a; } }
            else if self.last.ws.contains(&i) {
                // Rehearsal costs the same capacity stress takes from thinking (8.4): it is cut by
                // stress in the proportion this temperament's stress cuts its thought.
                let cut = (self.mods.stress as i64 * self.temperament.stress_cut() as i64 / 1000).min(1000);
                let top = ((INTENT_HOLD - carried).max(0) as i64 * (1000 - cut) / 1000) as i32;
                if top > 0 { inject.push((i, top)); a0 += top; }
            }
            self.mods.intent = i;
            self.mods.focus = if self.cfg.focus { (a0 as i64 * FOCUS_GAIN as i64 / 1000).clamp(0, 1000) as i32 } else { 0 };
        }

        let tg = self.temperament.stress_gain() as i64 * (1000 + self.mods.fatigue as i64 / 2) / 1000;
        // Stage 3: companions linked to "ally" buffer the stress a tick adds (social buffering).
        let buffer = self.roles.resp.filter(|r| r.ally != u32::MAX).map_or(0, |r| st.act[r.ally as usize] as i64 * BUFFER as i64 / 1000);
        let stressor = (pain_rise as i64 + surprise as i64 / 2 + st.act[self.roles.danger as usize] as i64 / 16 - buffer).max(0);
        self.mods.stress = (self.mods.stress as i64 + stressor * tg / 1000).min(1000) as i32;
        if let Some(x) = self.held.pin_stress { self.mods.stress = x; }

        // 3. Think, as deep as stress allows, in what budget compaction left.
        let mut p = stressed_params(&self.params, self.mods.stress, &self.temperament);
        if self.consol.forced > 0 {
            let left = (THINK_BUDGET - self.consol.forced).max(THINK_BUDGET / 10);
            p.frontier = (p.frontier as i64 * left / THINK_BUDGET).max(6) as usize;
        }
        let mut hops = vec![];
        let tr = if self.trace { Some(&mut hops) } else { None };
        let ws = think::think(&self.store, st, &self.mods, &inject, &p, tr);
        rep.hops_run = st.hops_run; rep.frontier_cap = p.frontier; rep.visits = st.visits;
        self.mods.stress -= self.mods.stress / 40 + (self.mods.stress > 0) as i32;
        if let Some(x) = self.held.pin_stress { self.mods.stress = x; }
        // Stage 3: an intent out of the workspace for long enough is dropped.
        if let Some(i) = self.held.intent {
            if ws.contains(&i) { self.held.lapse = 0; } else { self.held.lapse += 1; }
            if self.held.lapse >= INTENT_LAPSE { self.held.dropped = Some((s.tick, i)); self.held.intent = None; self.held.lapse = 0; }
        }

        // 4. Feel.
        let (mut sa, mut sv) = (0i64, 0i64);
        for &n in &ws { let a = st.act[n as usize] as i64; sa += a; sv += a * self.store.val(n) as i64; }
        let wv = if sa > 0 { (sv / sa) as i32 } else { 0 };
        self.mods.mood += (wv - self.mods.mood) / 32;
        let m = &mut self.mods;
        m.arousal = (m.arousal + st.act[self.roles.danger as usize] / 64).min(1000);
        m.arousal -= m.arousal / 10;
        if let Some(a) = self.pin_arousal { m.arousal = a; }
        if let Some(md) = self.pin_mood { m.mood = md; }

        // 5. Learn by day.
        if self.learn {
            self.learn_expectations(s, &percepts);
            self.hebbian(st, &ws, here.map(|pi| self.place_nodes[pi]), s.tick);
            for &(pi, k, e) in &absent_now {
                let (pn, n, c) = (self.place_nodes[pi], self.missing_node(pi, k), self.concept(k));
                for (a, b) in [(pn, n), (n, pn), (pn, c), (c, pn)] {
                    for l in self.store.links_vec(a) { if l.to == b {
                        let floor = -(l.slow as i32);
                        let f = self.store.fast(l.key) as i32;
                        self.store.set_fast(l.key, (f - e / EXTINCT_RATE).max(floor) as i16);
                    } }
                }
                if self.store.val(n) > 0 {
                    for (a, b) in [(pn, self.roles.loss), (self.roles.loss, pn)] {
                        if let Some(l) = self.store.find(a, b, lt::FEELS) {
                            let x = self.store.link(l);
                            self.store.set_fast(l, (x.fast as i32 + e / EXTINCT_RATE).min(1000 - x.slow as i32) as i16);
                        }
                    }
                }
            }
            if s.tick % FAST_FADE_EVERY == 0 { self.store.map_fast(&mut |f| f - (f / 128 + f.signum())); }
            if s.tick % 8 == 0 { for v in self.volatility.iter_mut() { *v -= *v / 256; } }
            if self.mods.arousal - a_before >= 250 || pain_rise >= 200 {
                self.episodes.push(Episode { tick: s.tick, place: here.map(|p| self.place_nodes[p]), ws: ws.iter().map(|&n| (n, st.act[n as usize])).collect(), valence: wv, arousal: self.mods.arousal });
                if self.episodes.len() > 32 { self.episodes.remove(0); }
            }
        }

        // 6. Act: first the danger responses (stage 3), if this species has them and danger is about.
        let resp = self.roles.resp.and_then(|rs| self.respond(&rs, st, s, &ws, &percepts));
        self.held.response = resp.as_ref().map(|r| r.2);
        self.held.calling = resp.is_some() && self.roles.resp.is_some_and(|rs| rs.call != u32::MAX && st.act[rs.call as usize] >= OPTION_AT);
        self.held.last_scores = resp.as_ref().map(|r| r.3.clone()).unwrap_or_default();
        let (action, why, scores) = match resp { Some((a, w, _, _)) => (a, w, vec![]), None => self.choose(st, s, &ws, &percepts, &surprised) };
        if let Action::Flee { from_x, from_y, .. } = action { if self.flee_left == 0 && self.roles.resp.is_none() { self.flee_left = 4; } self.flee_from = (from_x, from_y); }
        if !matches!(action, Action::Flee { .. }) {
            for &(p, a, felt) in &scores {
                if felt < 0 && a > 0 {
                    st.act[p as usize] = (st.act[p as usize] as i64 * (1000 - REJECT_DAMP) as i64 / 1000) as i32;
                    for &n in &ws {
                        let t = self.store.tags(n);
                        if t & tag::PLACE == 0 && t & tag::ANCHORED != 0 && self.store.find(n, p, lt::AT).is_some() {
                            st.act[n as usize] = (st.act[n as usize] as i64 * (1000 - REJECT_DAMP) as i64 / 1000) as i32;
                        }
                    }
                }
            }
        }
        // 7. Idle time is consolidation time.
        self.consol.last_units = 0;
        let settled = max_error < IGNORE_BELOW && pain_rise == 0 && self.flee_left == 0 && self.mods.stress < 400
            && !matches!(action, Action::Flee { .. } | Action::Snap { .. } | Action::Hold { .. } | Action::Freeze | Action::Call | Action::Charge { .. });
        if self.learn && self.idle_consolidation && settled {
            let units = THINK_BUDGET - st.visits as i64 - self.consol.forced;
            if units > 0 { self.consolidate(units, false); self.consol.idle_ticks += 1; }
        }
        rep.searching = why.starts_with("search");
        rep.ws = ws; rep.hops = hops; rep.percepts = percepts; rep.inject = inject; rep.scores = scores;
        rep.action = Some(action); rep.why = why;
        rep.backlog = self.backlog(); rep.consolidated = self.consol.last_units;
        self.last = rep;
        action
    }

    /// Spend `units` of budget on the backlog: the strongest episode first, then fast weights,
    /// largest first. Returns the units spent.
    pub fn consolidate(&mut self, mut units: i64, asleep: bool) -> i64 {
        let start = units;
        let share = if asleep { REPLAY_ASLEEP } else { REPLAY_AWAKE };
        loop {
            let Some(i) = (0..self.episodes.len()).max_by_key(|&i| (self.episodes[i].arousal, std::cmp::Reverse(self.episodes[i].tick))) else { break };
            let nodes = self.episode_nodes(&self.episodes[i]);
            let cost: i64 = nodes.iter().map(|&(u, _)| self.store.links_vec(u).len() as i64).sum::<i64>() * REPLAY_HOPS;
            if cost > units { break; }
            units -= cost;
            self.replay(i, share);
            self.consol.replays += 1;
            if self.episodes[i].arousal < 50 { self.episodes.remove(i); }
        }
        if units >= TRANSFER_COST {
            let mut dirty = self.store.fast_entries();
            dirty.sort_by_key(|&(k, f)| (std::cmp::Reverse(f.unsigned_abs()), k));
            let keep = self.cfg.sleep_keep;
            for (k, f) in dirty {
                let t = (f.unsigned_abs() as i64).min(units / TRANSFER_COST);
                if t <= 0 { break; }
                let sg = f.signum() as i64;
                self.store.set_fast(k, f - (sg * t) as i16);
                let slow = self.store.slow(k) as i64;
                self.write_slow(k, slow + sg * t / keep);
                units -= t * TRANSFER_COST;
                self.consol.moved += t;
            }
        }
        let spent = start - units;
        self.consol.last_units += spent;
        self.consol.total_units += spent;
        spent
    }

    /// Replay one episode into the slow links between its nodes and the feelings of its anchored
    /// places and things.
    fn replay(&mut self, i: usize, share: i64) {
        let ep = self.episodes[i].clone();
        let s = ep.arousal as i64 * share / 1000;
        let nodes = self.episode_nodes(&ep);
        let top = nodes.iter().map(|x| x.1).max().unwrap_or(1).max(1) as i64;
        for &(u, au) in &nodes {
            let anch_u = self.store.tags(u) & tag::ANCHORED != 0;
            for l in self.store.links_vec(u) {
                if !nodes.iter().any(|x| x.0 == l.to) { continue; }
                if l.fast < 0 {
                    let t = (-(l.fast as i64)).min(s / 2);
                    self.store.set_fast(l.key, l.fast + t as i16);
                    self.write_slow(l.key, l.slow as i64 - t);
                    continue;
                }
                self.write_slow(l.key, l.slow as i64 + s / 6);
                if anch_u || self.store.tags(l.to) & tag::ANCHORED != 0 {
                    let lv = l.lval as i64;
                    self.store.set_lval(l.key, (lv + (ep.valence as i64 - lv) * s / 1000 / 4) as i16);
                }
            }
            if anch_u {
                let sh = au as i64 * 1000 / top;
                let v = self.store.val(u) as i64;
                self.store.set_val(u, (v + (ep.valence as i64 - v) * s / 1000 * sh / 1000 / 3) as i16);
            }
        }
        self.episodes[i].arousal -= s as i32;
    }

    /// Lossy compaction: scale every fast weight and episode down to 90% of capacity.
    fn compact(&mut self, b: i64) -> i64 {
        let keep = BACKLOG_CAP * 900 / b;
        let mut work = 0;
        self.store.map_fast(&mut |f| { work += 1; (f as i64 * keep / 1000) as i16 });
        for e in self.episodes.iter_mut() { e.arousal = (e.arousal as i64 * keep / 1000) as i32; work += 1; }
        self.episodes.retain(|e| e.arousal >= 50);
        self.consol.lost += b - self.backlog();
        work
    }

    /// How a place feels right now: a one-hop look-ahead over what it holds and what's attached
    /// to it, each feeling scaled by the drives and modulators (a somatic marker).
    pub fn felt<F: Form + ?Sized>(&self, f: &F, p: u32) -> i32 { self.felt_with(f, p, &self.mods) }
    pub fn felt_with<F: Form + ?Sized>(&self, f: &F, p: u32, m: &Mods) -> i32 {
        let neg_mood = (-m.mood).max(0) as i64;
        let (a, h, fa) = (m.arousal as i64, m.hunger as i64, m.fatigue as i64);
        // What comfort is worth now: tiredness, stress or a low mood (stage 3).
        let need = fa.max(m.stress as i64).max(neg_mood).min(1000);
        let comfort = if self.cfg.comfort_gate { self.roles.comfort } else { u32::MAX };
        let good = |val: i64, tags: u8, n: u32| -> i64 {
            if tags & tag::FOOD != 0 { val * 2 * h / 1000 } else if tags & tag::REST != 0 { val * 2 * fa / 1000 } else if n == comfort { val * need / 1000 } else { val * (300 + a) / 1000 }
        };
        let mut edges = vec![];
        f.edges(p, &mut edges);
        let own = f.val(p) as i64;
        let mut v = if own < 0 { own * (1000 + 2 * a + neg_mood) / 1000 } else { good(own, f.tags(p), p) };
        for e in edges {
            let val = (f.val(e.to) + e.val).clamp(-1000, 1000) as i64;
            if f.tags(e.to) & tag::PLACE != 0 { v += e.w as i64 * val / 2000; continue; }
            v += if val < 0 { e.w as i64 * val / 1000 * (1000 + 2 * a + neg_mood) / 1000 } else { e.w as i64 * good(val, f.tags(e.to), e.to) / 1000 };
        }
        v as i32
    }

    /// The place an anchored thing is at: its strongest link to a place.
    fn place_of(&self, n: u32) -> Option<u32> {
        let mut edges: Vec<Edge> = vec![];
        self.store.edges(n, &mut edges);
        edges.iter().filter(|e| self.store.tags(e.to) & tag::PLACE != 0).max_by_key(|e| (e.w, std::cmp::Reverse(self.tk(e.to)))).map(|e| e.to)
    }

    fn choose(&self, st: &Activation, s: &Sense, ws: &[u32], percepts: &[Percept], surprised: &[(u32, i32, i32)]) -> (Action, String, Vec<(u32, i32, i32)>) {
        let f = &self.store;
        let fx = s.body;
        let act = |n: u32| st.act[n as usize];
        let name = |n: u32| self.name(n);
        let stress = self.mods.stress;
        let speed = if stress >= 500 { 3 } else { 2 };
        let flee_at = FLEE_AT * (1000 - stress / 2) / 1000;
        let danger = act(self.roles.danger);
        let anger = act(self.roles.anger);
        let bar = (danger as i64 * (1400 - self.temperament.aggression - self.mods.fatigue / 4) as i64 / 1000) as i32;
        if ws.contains(&self.roles.anger) && anger >= bar.max(flee_at) {
            if let Some(d) = percepts.iter().find(|p| self.kinds[p.kind as usize].creature && (p.x - fx.x).abs().max((p.y - fx.y).abs()) <= 2) {
                return (Action::Snap { entity: d.entity }, format!("anger {anger} against danger {danger} (bar {bar}): snap"), vec![]);
            }
        }
        if self.flee_left > 0 {
            return (Action::Flee { from_x: self.flee_from.0, from_y: self.flee_from.1, speed }, "still fleeing".into(), vec![]);
        }
        if (ws.contains(&self.roles.danger) && danger >= flee_at) || (ws.contains(&self.roles.anger) && anger >= flee_at) {
            let src = surprised.iter().filter(|x| ws.contains(&x.0) && f.val(x.0) < 0).max_by_key(|x| (act(x.0), std::cmp::Reverse(self.tk(x.0))));
            if let Some(&(n, x, y)) = src {
                return (Action::Flee { from_x: x, from_y: y, speed }, format!("danger {danger} on my mind (bar {flee_at}), from {}", name(n)), vec![]);
            }
        }
        if fx.hunger >= 150 {
            let seen = |id: u32| s.seen.iter().find(|e| e.id == id).is_some_and(|e| e.available);
            for p in percepts {
                if self.kinds[p.kind as usize].edible && (p.x - fx.x).abs().max((p.y - fx.y).abs()) <= 1
                    && (ws.contains(&p.node) || ws.contains(&self.roles.food)) && seen(p.entity) {
                    return (Action::Eat { entity: p.entity }, format!("eat {}", name(p.node)), vec![]);
                }
            }
        }
        let here = self.place_nodes.iter().copied().find(|&p| f.anchor(p).is_some_and(|a| { let (x, y) = a.xy(); (x - fx.x).abs().max((y - fx.y).abs()) <= 1 }));
        let mut cands: Vec<u32> = vec![];
        for &n in ws {
            let t = f.tags(n);
            if t & tag::PLACE != 0 { cands.push(n) } else if t & tag::ANCHORED != 0 { if let Some(p) = self.place_of(n) { cands.push(p) } }
        }
        let drive = |n: u32| if f.tags(n) & tag::REST != 0 { self.mods.fatigue } else if f.tags(n) & tag::FOOD != 0 { self.mods.hunger } else { 0 };
        let mut seen_pull: Vec<(u32, i32)> = vec![];
        let mut novel_pull: Vec<(u32, i32)> = vec![];
        for p in percepts {
            if f.tags(p.node) & tag::ANCHORED != 0 || f.val(p.node) <= 0 || !ws.contains(&p.node) { continue; }
            let novelty = if p.error >= IGNORE_BELOW { p.error * NOVELTY_PULL / 1000 } else { 0 };
            if let Some(pi) = p.place { let pn = self.place_nodes[pi]; cands.push(pn); seen_pull.push((pn, act(p.node) * drive(p.node) / 1000)); novel_pull.push((pn, novelty)); }
        }
        cands.sort_unstable(); cands.dedup();
        let mut scores = vec![];
        for &p in &cands {
            let mut a = act(p) + seen_pull.iter().filter(|x| x.0 == p).map(|x| x.1).max().unwrap_or(0);
            for &n in ws { if n != p && f.tags(n) & tag::ANCHORED != 0 && f.tags(n) & tag::PLACE == 0 && f.val(n) > 0 && self.place_of(n) == Some(p) {
                a += if f.tags(n) & tag::REST != 0 { act(n) * self.mods.fatigue / 1000 } else { act(n) };
            } }
            scores.push((p, a, self.felt(f, p)));
        }
        let nov = |p: u32| novel_pull.iter().filter(|x| x.0 == p).map(|x| x.1).max().unwrap_or(0);
        // The cost of distance (stage 3): a place's worth is discounted by how far it is to walk.
        let wc = self.cfg.walk_cost as i64;
        let far = |p: u32| -> i64 { if wc <= 0 { return 1000; } let (x, y) = f.anchor(p).map(|a| a.xy()).unwrap_or((fx.x, fx.y)); wc * 1000 / (wc + dist(x, y, fx.x, fx.y) as i64) };
        let total = |s: &(u32, i32, i32)| (((s.1 as i64 * s.2.max(0) as i64 / 1000) as i32 + if s.2 >= 0 { nov(s.0) } else { 0 }) as i64 * far(s.0) / 1000) as i32;
        let best = scores.iter().max_by_key(|s| (total(s), std::cmp::Reverse(self.tk(s.0)))).copied();
        if let Some(pz) = self.puzzle {
            let pp = self.place_nodes[pz.place];
            let best_ok = best.filter(|b| total(b) >= WALK_AT);
            if pz.strength >= SEARCH_AT && best_ok.is_none_or(|b| b.0 == pp) {
                let wp = match self.search_wp {
                    Some((x, y)) if (x - fx.x).abs().max((y - fx.y).abs()) > 1 => (x, y),
                    _ => self.next_waypoint(s, pz),
                };
                return (Action::WalkTo { x: wp.0, y: wp.1 }, format!("search for the missing {} (puzzle {})", self.kinds[pz.kind as usize].name, pz.strength), scores);
            }
        }
        let goal = match self.goal { Some(g) if Some(g) == here => None, g => g };
        let goal_score = goal.and_then(|g| scores.iter().find(|s| s.0 == g)).map(total);
        // Commitment (stage 3): a goal on the way is kept while nothing beats it by the margin,
        // the place underfoot included.
        let keep_goal = self.cfg.hold_goal && matches!((best, goal, goal_score), (Some(b), Some(g), Some(gs)) if b.0 != g && total(&b) * 1000 < gs * (1000 + SWITCH_MARGIN) && gs >= WALK_AT);
        if let (Some(b), Some(h), false) = (best, here, keep_goal) { if b.0 == h {
            let hi = self.place_nodes.iter().position(|&p| p == h);
            let rests_here = percepts.iter().any(|p| self.kinds[p.kind as usize].shelter && p.place.is_some() && p.place == hi);
            let calm = self.mods.arousal < 300 && self.mods.stress < 400;
            let puzzled_here = self.puzzle.is_some_and(|pz| pz.strength >= SEARCH_AT && self.place_nodes[pz.place] == h);
            if !self.skip_sleep && calm && !puzzled_here && self.mods.fatigue >= NAP_AT && ws.contains(&self.roles.rest) && rests_here && fx.hunger < WAKE_HUNGER {
                return (Action::Sleep, format!("tired (fatigue {}), resting at {}: sleeps", self.mods.fatigue, name(h)), scores);
            }
            return (Action::Stay, format!("stay at {} (score {})", name(h), total(&b)), scores);
        } }
        let pick = match (best, goal, goal_score) {
            (Some(b), Some(g), Some(gs)) if b.0 != g && total(&b) * 1000 < gs * (1000 + SWITCH_MARGIN) && gs >= WALK_AT => Some((g, gs)),
            (Some(b), _, _) if total(&b) >= WALK_AT => Some((b.0, total(&b))),
            _ => None,
        };
        let scores_out = scores.clone();
        match pick {
            Some((p, sc)) => {
                let (x, y) = f.anchor(p).map(|a| a.xy()).unwrap_or((fx.x, fx.y));
                (Action::WalkTo { x, y }, format!("walk to {} (score {sc})", name(p)), scores_out)
            }
            None => (Action::Stay, "nothing pulls".into(), scores_out),
        }
    }

    /// Stage 3: the danger-response competition. Every response is a node, scored by its activation.
    /// **Flee is the reflex**: one strong hop from danger, in play as soon as danger passes the flee
    /// bar, and made stronger by stress. The **options** (fight, call, freeze, hold, aid) must be
    /// reached by thought (at least `OPTION_AT` active), so they need more hops, which stress takes
    /// away. Company read off the mind's "ally" node emboldens. The response under way keeps a margin.
    /// Returns None when no threat is sensed or heard, or nothing passes its bar (ordinary life).
    pub fn respond(&self, rs: &Responses, st: &Activation, s: &Sense, ws: &[u32], percepts: &[Percept]) -> Option<(Action, String, u8, Vec<(u8, i32)>)> {
        let fx = s.body;
        let act = |n: u32| if n == u32::MAX { 0 } else { st.act[n as usize] };
        let threat = percepts.iter().filter(|p| self.kinds[p.kind as usize].creature && self.store.val(p.node) < 0)
            .min_by_key(|p| (d2(p.x, p.y, fx.x, fx.y), p.entity)).cloned();
        let alarm = self.held.alarm.filter(|a| s.tick <= a.3 + ALARM_KEEP);
        if threat.is_none() && alarm.is_none() { return None; }
        let stress = self.mods.stress;
        let speed = if stress >= 500 { 3 } else { 2 };
        let from = threat.as_ref().map(|t| (t.x, t.y)).or(alarm.map(|a| (a.1, a.2))).unwrap();
        let flee_at = FLEE_AT * (1000 - stress / 2) / 1000;
        let danger = act(self.roles.danger);
        // The reflex: once danger on the mind passes the bar, "run" is in play straight away (one
        // strong hop from danger), whether or not it made the workspace; stress makes it stronger.
        let reflex = ws.contains(&self.roles.danger) && danger >= flee_at;
        let flee = if reflex || ws.contains(&rs.flee) { act(rs.flee) * (1000 + stress) / 1000 } else { 0 };
        let mut sc: Vec<(u8, i32)> = vec![(R_FLEE, if flee >= OPTION_AT { flee } else { 0 })];
        for r in [R_FIGHT, R_CALL, R_FREEZE, R_HOLD, R_AID] {
            let n = rs.node(r);
            if n == u32::MAX { continue; }
            // What the body can do here: fight, hold and freeze face a threat in sight; aid goes to
            // a cry heard with no threat in sight.
            let can = match r { R_FIGHT | R_HOLD | R_FREEZE => threat.is_some(), R_AID => threat.is_none() && alarm.is_some(), _ => true };
            if can && act(n) >= OPTION_AT { sc.push((r, act(n))); }
        }
        // Company (sensed, through the mind's own learnt links to "ally") emboldens.
        let courage = if rs.ally == u32::MAX { 0 } else { (act(rs.ally) as i64 * COURAGE as i64 / 1000).min(1000) as i32 };
        for x in sc.iter_mut() { match x.0 { R_FLEE => x.1 = x.1 * (1000 - courage / 2) / 1000, R_HOLD | R_FIGHT | R_AID => x.1 = x.1 * (1000 + courage) / 1000, _ => {} } }
        if let Some(r) = self.held.response { for x in sc.iter_mut() { if x.0 == r { x.1 = x.1 * (1000 + SWITCH_MARGIN) / 1000; } } }
        let best = sc.iter().filter(|x| x.1 > 0).max_by_key(|x| (x.1, std::cmp::Reverse(x.0))).copied()?;
        let list: Vec<String> = sc.iter().map(|x| format!("{} {}", R_NAMES[x.0 as usize], x.1)).collect();
        let why = format!("{}: {} (danger {danger}, stress {stress}, reflex bar {flee_at}, courage {courage})", R_NAMES[best.0 as usize], list.join(", "));
        let a = match best.0 {
            R_FLEE => Action::Flee { from_x: from.0, from_y: from.1, speed },
            R_FIGHT => { let t = threat.as_ref().unwrap(); if (t.x - fx.x).abs().max((t.y - fx.y).abs()) <= 2 { Action::Snap { entity: t.entity } } else { Action::Charge { entity: t.entity, x: t.x, y: t.y } } }
            R_HOLD => Action::Hold { entity: threat.as_ref().unwrap().entity },
            R_FREEZE => Action::Freeze,
            R_CALL => Action::Call,
            _ => { let al = alarm.unwrap(); Action::WalkTo { x: al.1, y: al.2 } }
        };
        Some((a, why, best.0, sc))
    }

    /// Stage 3: form an intent (a plan node of this mind's own): it is brought to mind now and held
    /// by rehearsal while it stays in the workspace.
    pub fn form_intent(&mut self, n: u32) { self.held.intent = Some(n); self.held.lapse = 0; self.held.boost = Some((n, 1000)); }

    /// The next search waypoint: a hashed bearing from the puzzle's place, at a radius that
    /// grows with each waypoint tried.
    fn next_waypoint(&self, s: &Sense, pz: Puzzle) -> (i32, i32) {
        let (px, py) = self.place_xy(pz.place);
        let n = self.search_n as i32;
        let r = (4 + n * 3).min(30);
        let h = hash(s.seed, pz.since as u64, n as u64, 0x5ea2c);
        const DIRS: [(i32, i32); 16] = [(1000, 0), (924, 383), (707, 707), (383, 924), (0, 1000), (-383, 924), (-707, 707), (-924, 383),
            (-1000, 0), (-924, -383), (-707, -707), (-383, -924), (0, -1000), (383, -924), (707, -707), (924, -383)];
        let (dx, dy) = DIRS[(h % 16) as usize];
        ((px + dx * r / 1000).clamp(0, self.geo.grid_w - 1), (py + dy * r / 1000).clamp(0, self.geo.grid_h - 1))
    }

    /// Track the action the world actually received (so commitment survives).
    pub fn commit(&mut self, s: &Sense, a: Action) {
        if self.last.searching {
            if let Action::WalkTo { x, y } = a { if self.search_wp != Some((x, y)) { self.search_wp = Some((x, y)); self.search_n += 1; } }
        }
        match a {
            Action::WalkTo { x, y } => { self.goal = self.place_nodes.iter().copied().find(|&p| self.store.anchor(p).map(|a| a.xy()) == Some((x, y))); }
            Action::Flee { .. } => { self.flee_left = (self.flee_left - 1).max(0); self.goal = None; }
            Action::Eat { .. } | Action::Snap { .. } => {}
            Action::Hold { .. } | Action::Freeze | Action::Call | Action::Charge { .. } => { self.goal = None; }
            Action::Stay => {
                let fx = s.body;
                if self.goal.is_some_and(|g| self.store.anchor(g).is_some_and(|a| { let (x, y) = a.xy(); (x - fx.x).abs().max((y - fx.y).abs()) <= 1 })) { self.goal = None; }
            }
            Action::Sleep => { self.goal = None; if !s.body.asleep { self.napping = true; } }
        }
    }

    fn learn_expectations(&mut self, s: &Sense, percepts: &[Percept]) {
        let sight = self.geo.sight;
        for pi in 0..self.place_nodes.len() {
            if !self.place_learnt(pi) { continue; }
            let (plx, ply) = self.place_xy(pi);
            let d = d2(s.body.x, s.body.y, plx, ply);
            if d > sight * sight { continue; }
            for k in 0..self.kinds.len() as u8 {
                for appr in [false, true] {
                    let seen = percepts.iter().any(|p| p.place == Some(pi) && p.kind == k && p.approaching == appr);
                    let e = &mut self.expect[pi][key(k, appr)];
                    if seen { *e += (1000 - *e) / 32; } else if d <= 16 { *e -= *e / 64; }
                }
            }
        }
    }

    /// Fast weights: links between co-active workspace nodes strengthen, scaled by arousal. Links
    /// touching a place learn only while the body is at that place.
    fn hebbian(&mut self, st: &Activation, ws: &[u32], here: Option<u32>, tick: u32) {
        let a = self.mods.arousal as i64 + BASE_ENCODE;
        let mut ls: Vec<Link> = vec![];
        for &u in ws {
            ls.clear(); self.store.links(u, &mut ls);
            for l in &ls {
                let v = l.to;
                if v == u || !ws.contains(&v) || u == self.roles.loss || v == self.roles.loss { continue; }
                if l.fast < 0 { continue; }
                let place = |n: u32| self.store.tags(n) & tag::PLACE != 0;
                if (place(u) && Some(u) != here) || (place(v) && Some(v) != here) { continue; }
                let co = st.act[u as usize].min(1000) as i64 * st.act[v as usize].min(1000) as i64 / 1000;
                let room = (1000 - l.fast as i64).max(0);
                let d = if self.cfg.stochastic_learning {
                    let (num, den) = (co * a * room, HEBB_RATE * 1000);
                    let r = num % den;
                    (num / den + (r > 0 && (hash(self.who, tick as u64, l.key, 0x4ebb) % den as u64) < r as u64) as i64) as i32
                } else { (co * a / HEBB_RATE * room / 1000) as i32 };
                if d != 0 { self.store.set_fast(l.key, (l.fast as i32 + d).min(1000) as i16); }
            }
        }
    }

    /// A night: asleep, a night's consolidation; lying awake (`skip_sleep`), `NIGHT_TICKS`
    /// waking ticks with the body held still.
    pub fn night(&mut self, s: &Sense) {
        if !self.skip_sleep { self.sleep(); return; }
        let mut wd = s.clone();
        wd.body.asleep = false;
        for _ in 0..NIGHT_TICKS { self.step(&wd); wd.tick += 1; }
        self.goal = None; self.flee_left = 0; self.search_wp = None;
    }

    /// Sleep: a night's consolidation budget spent on the backlog, then a rested mind.
    pub fn sleep(&mut self) {
        self.consol.last_units = 0;
        self.consolidate(NIGHT_TICKS as i64 * SLEEP_BUDGET, true);
        self.fade_own();
        self.nights += 1;
        self.st.clear();
        self.mods.arousal = 0;
        self.mods.stress = 0;
        self.mods.mood /= 2;
        self.mods.fatigue = self.pressure();
        self.goal = None;
        self.flee_left = 0;
        self.search_wp = None;
    }

    /// Nightly forgetting of the links the mind learnt in its life (see `Config::own_fade`).
    /// Returns (links faded, links removed).
    pub fn fade_own(&mut self) -> (u32, u32) {
        if self.cfg.own_fade.iter().all(|&f| f == 0) { return (0, 0); }
        let (mut faded, mut gone) = (0, vec![]);
        for u in self.store.own_sources() {
            let bd = self.store.base_degree(u);
            for l in self.store.links_vec(u).into_iter().skip(bd) {
                if u < self.young_from && l.to < self.young_from { continue; }
                let r = self.cfg.own_fade[l.ty as usize] as i64;
                if r == 0 || l.slow <= 0 { if l.slow < PRUNE_BELOW && l.fast == 0 && r > 0 { gone.push(l.key); } continue; }
                let num = l.slow as i64 * r;
                let d = num / 1000 + ((hash(self.who, self.nights as u64, l.key, 0xfade) % 1000) < (num % 1000) as u64) as i64;
                let w = (l.slow as i64 - d).max(0);
                self.store.set_slow(l.key, w as i16);
                faded += 1;
                if w < PRUNE_BELOW as i64 && l.fast == 0 { gone.push(l.key); }
            }
        }
        let n = gone.len() as u32;
        gone.sort_unstable();
        for k in gone.into_iter().rev() { self.store.remove_link(k); }
        (faded, n)
    }

    pub fn fast_total(&self) -> i64 { self.store.fast_total() }
    pub fn link_of(&self, u: u32, v: u32, ty: u8) -> Option<LinkKey> { self.store.find(u, v, ty) }
    /// (slow, fast) of the strongest link u → v of any type.
    pub fn weight(&self, u: u32, v: u32) -> (i32, i32) {
        self.store.links_vec(u).iter().filter(|l| l.to == v).map(|l| (l.slow as i32, l.fast as i32)).max().unwrap_or((0, 0))
    }
    /// Think from a cue on a copy of the mind's activation, without changing the mind: what
    /// comes to mind (the workspace) and how strongly each node is active after `thinks` thinks.
    pub fn imagine(&self, cue: &[(u32, i32)], mods: &Mods, thinks: usize) -> (Vec<u32>, Vec<i32>, u32) {
        let mut a = Activation::new(self.store.len());
        let mut ws = vec![];
        let mut visits = 0;
        for _ in 0..thinks { ws = think::think(&self.store, &mut a, mods, cue, &self.params, None); visits += a.visits; }
        (ws, a.act, visits)
    }
}
