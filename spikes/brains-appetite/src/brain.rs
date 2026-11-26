//! The brain, layered as in spec 07:
//!
//! - **reflexes** every tick, on raw readings (stop swallowing when full, stop
//!   when exhausted, flinch to rest on pain);
//! - **choosing** every [`THINK_EVERY`] ticks, by utility over a few actions,
//!   each a product of considerations (integer curves over body readings);
//! - **retrospection** once a night over the day's experience log, which
//!   compiles a day of experience into one number: the appetite habit.

use crate::body::{Body, Effort, Spent, TICKS_PER_DAY};
use crate::curve::Curve;
use crate::rng::{below, hash};
use crate::units::*;

pub const THINK_EVERY: u64 = 10;
/// Scores within this share of the best are a shortlist; chance picks among them.
pub const SHORTLIST: Q16 = Q16::milli(920);
/// Doing what you are already doing gets a small bonus (avoids dithering).
pub const MOMENTUM: Q16 = Q16::milli(1_150);

const SALT_CHOICE: u64 = 0xC401CE;
const SALT_FOOD: u64 = 0xF00D;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action { Eat, Rest, Work, Idle }
pub const ACTIONS: [Action; 4] = [Action::Eat, Action::Rest, Action::Work, Action::Idle];

impl Action {
    pub fn name(self) -> &'static str {
        match self { Action::Eat => "eat", Action::Rest => "rest", Action::Work => "work", Action::Idle => "idle" }
    }
    /// What the body is asked to do.
    pub fn effort(self) -> Effort {
        match self {
            Action::Work => Effort { mech: watts(60), overhead: watts(20) },
            Action::Idle => Effort { mech: watts(8), overhead: watts(15) },
            Action::Eat => Effort { mech: Power::ZERO, overhead: watts(15) },
            Action::Rest => Effort::default(),
        }
    }
    fn weight(self) -> Q16 {
        match self { Action::Eat => Q16::milli(1_200), Action::Rest => Q16::milli(900), Action::Work => Q16::milli(850), Action::Idle => Q16::milli(350) }
    }
}

// Curves. Inputs over hours of the day run 0:00, 1:30, ..., 24:00.
pub const HUNGER: Curve = Curve::milli("hunger", [0, 0, 10, 30, 70, 130, 220, 340, 480, 620, 740, 840, 910, 960, 985, 995, 1000]);
pub const ROOM: Curve = Curve::milli("room", [0, 0, 50, 150, 300, 450, 600, 720, 820, 890, 940, 970, 985, 995, 1000, 1000, 1000]);
pub const AWAKE: Curve = Curve::milli("awake", [50, 20, 20, 50, 400, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 900, 300, 50]);
pub const SLEEPY: Curve = Curve::milli("sleepy", [1000, 1000, 1000, 900, 400, 100, 50, 50, 80, 80, 50, 50, 80, 150, 350, 850, 1000]);
pub const DUTY: Curve = Curve::milli("duty", [0, 0, 0, 0, 200, 900, 1000, 1000, 700, 1000, 1000, 900, 300, 0, 0, 0, 0]);
pub const ABLE: Curve = Curve::milli("able", [0, 0, 20, 50, 100, 160, 230, 310, 400, 490, 580, 670, 760, 840, 910, 960, 1000]);
pub const FUELLED: Curve = Curve::milli("fuelled", [0, 100, 300, 550, 750, 880, 950, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000]);

/// What the brain senses this tick: readings of the body's stores and the
/// world, all from the previous tick (read-previous, write-own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Senses {
    pub glycogen_fill: Q16,
    pub gut_fill: Q16,
    pub capability: Q16,
    /// Time of day, 0..1.
    pub hour: Q16,
    pub work_offered: bool,
    pub pain: bool,
}

impl Senses {
    pub fn read(body: &Body, tick: u64, work_offered: bool, pain: bool) -> Senses {
        Senses {
            glycogen_fill: body.glycogen_fill(),
            gut_fill: body.gut_fill(),
            capability: body.capability(),
            hour: Q16::ratio((tick % TICKS_PER_DAY) as i64, TICKS_PER_DAY as i64),
            work_offered,
            pain,
        }
    }
}

/// One consideration as it was weighed: the curve, its input and its output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weighed { pub name: &'static str, pub input: Q16, pub output: Q16 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scored {
    pub action: Action,
    pub weighed: [Option<Weighed>; 3],
    pub momentum: bool,
    pub score: Q16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    Reflex(&'static str),
    /// Chosen by utility from a shortlist of `of` options.
    Utility { of: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub tick: u64,
    pub chosen: Action,
    pub why: Why,
    pub scores: [Scored; 4],
    pub appetite: Q16,
}

impl Decision {
    /// "Which consideration won, and the scores."
    pub fn explain(&self) -> String {
        let t = self.tick % TICKS_PER_DAY;
        let mut s = format!("day {:>2} {:02}:{:02}  {:<4} ", self.tick / TICKS_PER_DAY + 1, t / 60, t % 60, self.chosen.name());
        match self.why {
            Why::Reflex(r) => { s += &format!("reflex: {r}"); return s }
            Why::Utility { of } => s += &format!("(shortlist {of}, appetite {:.2})", self.appetite.f()),
        }
        let mut sorted = self.scores;
        sorted.sort_by(|a, b| b.score.cmp(&a.score).then(a.action.cmp(&b.action)));
        for sc in sorted.iter().filter(|x| x.score > Q16::ZERO).take(3) {
            s += &format!("\n      {:<4} {:.3} =", sc.action.name(), sc.score.f());
            for w in sc.weighed.iter().flatten() {
                s += &format!(" {}({:.2})={:.2}", w.name, w.input.f(), w.output.f());
            }
            s += &format!(" ×w{:.2}", sc.action.weight().f());
            if sc.action == Action::Eat { s += &format!(" ×appetite{:.2}", self.appetite.f()) }
            if sc.momentum { s += " ×momentum" }
        }
        s
    }
}

/// One entry of experience: what was done over a think interval and how it felt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Experience {
    pub spent: Energy,
    pub ate: Energy,
    /// The hunger reading at the start of the interval (after appetite).
    pub hunger: Q16,
    pub awake: bool,
}

/// How the habit is allowed to learn (for comparison runs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Habit {
    /// Nightly retrospection, slow to fade (the design).
    Learned,
    /// No retrospection: appetite stays at 1.
    Fixed,
    /// Retrospection with no lag: appetite jumps to the target each night.
    Instant,
}

/// What a night's retrospection saw and did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Night {
    pub spent: Energy,
    pub at_rest: Energy,
    pub demand: Q16,
    pub hungry_share: Q16,
    pub mass_change_week: Mass,
    pub target: Q16,
    pub before: Q16,
    pub after: Q16,
}

/// Retrospection rules: demand = yesterday's spending ÷ a quiet day's;
/// target = 1 + (demand − 1) × DEMAND_GAIN
///   + (hungry share − HUNGRY_USUAL) × HUNGER_GAIN − kg gained over the last week × WEIGHT_GAIN.
pub const DEMAND_GAIN: Q16 = Q16::milli(1_000);
pub const HUNGER_GAIN: Q16 = Q16::milli(300);
/// The share of waking time spent hungry that counts as normal.
pub const HUNGRY_USUAL: Q16 = Q16::milli(200);
pub const WEIGHT_GAIN_PER_KG: Q16 = Q16::milli(100);
/// Share of the gap to the target closed each night, rising and fading.
pub const RISE_RATE: Q16 = Q16::milli(350);
pub const FADE_RATE: Q16 = Q16::milli(50);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Brain {
    pub creature: u64,
    pub seed: u64,
    pub habit: Habit,
    /// The habit: a gain on the hunger reading. 1.0 is neutral.
    pub appetite: Q16,
    pub doing: Action,
    pub food: usize,
    /// Today's experience, emptied by each night's retrospection.
    pub log: Vec<Experience>,
    /// Each morning's weigh-in (memory of how the body has changed).
    pub mass_memory: Vec<Mass>,
}

impl Brain {
    pub fn new(seed: u64, creature: u64, habit: Habit) -> Brain {
        Brain { creature, seed, habit, appetite: Q16::ONE, doing: Action::Rest, food: 0, log: Vec::new(), mass_memory: Vec::new() }
    }

    pub fn hunger_input(&self, s: &Senses) -> Q16 { (s.glycogen_fill.inv() * self.appetite).clamp01() }

    fn score(&self, a: Action, s: &Senses) -> Scored {
        let w = |c: &Curve, x: Q16| Some(Weighed { name: c.name, input: x, output: c.eval(x) });
        let weighed = match a {
            Action::Eat => [w(&HUNGER, self.hunger_input(s)), w(&ROOM, s.gut_fill.inv()), w(&AWAKE, s.hour)],
            Action::Rest => [w(&SLEEPY, s.hour), None, None],
            Action::Work => {
                let offered = if s.work_offered { s.hour } else { Q16::ZERO };
                [w(&DUTY, offered), w(&ABLE, s.capability), w(&FUELLED, s.glycogen_fill)]
            }
            Action::Idle => [w(&AWAKE, s.hour), None, None],
        };
        let mut score = a.weight();
        for c in weighed.iter().flatten() { score = score * c.output }
        if a == Action::Eat { score = score * self.appetite }
        let momentum = a == self.doing;
        if momentum { score = score * MOMENTUM }
        Scored { action: a, weighed, momentum, score }
    }

    /// The reflex layer, every tick. Returns a decision if a reflex fired.
    pub fn reflex(&mut self, tick: u64, s: &Senses) -> Option<Decision> {
        let why = if s.pain && self.doing != Action::Rest {
            Some((Action::Rest, "pain: stop and rest"))
        } else if self.doing == Action::Eat && s.gut_fill >= Q16::milli(980) {
            Some((Action::Idle, "stomach full: stop swallowing"))
        } else if self.doing == Action::Work && s.glycogen_fill < Q16::milli(50) {
            Some((Action::Rest, "exhausted: stop working"))
        } else { None };
        let (a, r) = why?;
        self.doing = a;
        Some(Decision { tick, chosen: a, why: Why::Reflex(r), scores: self.score_all(s), appetite: self.appetite })
    }

    fn score_all(&self, s: &Senses) -> [Scored; 4] { ACTIONS.map(|a| self.score(a, s)) }

    /// The choosing layer: utility, with seeded chance among the top options.
    pub fn choose(&mut self, tick: u64, s: &Senses) -> Decision {
        let scores = self.score_all(s);
        let best = scores.iter().map(|x| x.score).max().unwrap();
        let bar = best * SHORTLIST;
        let short: Vec<&Scored> = scores.iter().filter(|x| x.score >= bar && x.score > Q16::ZERO).collect();
        let total: i64 = short.iter().map(|x| x.score.0).sum();
        let chosen = if total <= 0 { Action::Rest } else {
            let mut roll = below(hash(self.seed, tick, self.creature, SALT_CHOICE), total as u64) as i64;
            let mut pick = short[0].action;
            for x in &short { if roll < x.score.0 { pick = x.action; break } roll -= x.score.0 }
            pick
        };
        if chosen == Action::Eat && self.doing != Action::Eat {
            // Bread or meat: 60/40, by seeded chance.
            self.food = if below(hash(self.seed, tick, self.creature, SALT_FOOD), 10) < 6 { 0 } else { 1 };
        }
        self.doing = chosen;
        Decision { tick, chosen, why: Why::Utility { of: short.len().max(1) as u8 }, scores, appetite: self.appetite }
    }

    pub fn remember(&mut self, spent: Spent, ate: Energy, s: &Senses, fresh_interval: bool) {
        if fresh_interval || self.log.is_empty() {
            self.log.push(Experience { hunger: self.hunger_input(s), awake: AWAKE.eval(s.hour) >= Q16::milli(500), ..Default::default() });
        }
        let e = self.log.last_mut().unwrap();
        e.spent += spent.basal + spent.activity;
        e.ate += ate;
    }

    /// The nightly pass: a day of experience compiled into the appetite habit.
    /// `weighed` is the morning's weigh-in, the person's experience of their weight.
    pub fn retrospect(&mut self, body: &Body, weighed: Mass) -> Option<Night> {
        let mass = weighed;
        self.mass_memory.push(mass);
        let spent: Energy = self.log.iter().map(|e| e.spent).fold(Energy::ZERO, |a, b| a + b);
        // The yardstick, derived from the body: a quiet day, asleep a third of
        // it and pottering the rest.
        let day = Duration(TICKS_PER_DAY as i64 * crate::body::TICK.0);
        let idle = Action::Idle.effort();
        let pottering = Power(idle.mech.0 * 1_000 / crate::body::MUSCLE_EFFICIENCY_PM) + idle.overhead;
        let at_rest = body.basal_power() * day + pottering * Duration(day.0 * 2 / 3);
        let awake = self.log.iter().filter(|e| e.awake).count() as i64;
        let hungry = self.log.iter().filter(|e| e.awake && e.hunger >= Q16::milli(500)).count() as i64;
        self.log.clear();
        if self.habit == Habit::Fixed { return None }

        let demand = spent.ratio(at_rest);
        let hungry_share = Q16::ratio(hungry, awake.max(1));
        let n = self.mass_memory.len();
        let week_ago = self.mass_memory[n.saturating_sub(8)];
        let gained = Mass((mass - week_ago).0.max(0));
        let weight_term = Q16((WEIGHT_GAIN_PER_KG.0 as i128 * gained.0 as i128 / 1_000_000) as i64);
        let target = Q16(Q16::ONE.0 + ((demand - Q16::ONE) * DEMAND_GAIN).0 + ((hungry_share - HUNGRY_USUAL) * HUNGER_GAIN).0 - weight_term.0);
        let target = Q16(target.0.clamp(Q16::milli(600).0, Q16::milli(3_000).0));
        let before = self.appetite;
        let rate = match self.habit {
            Habit::Instant => Q16::ONE,
            _ => if target > before { RISE_RATE } else { FADE_RATE },
        };
        self.appetite = before + (target - before) * rate;
        Some(Night { spent, at_rest, demand, hungry_share, mass_change_week: mass - week_ago, target, before, after: self.appetite })
    }

    pub fn hash_into(&self, h: &mut crate::rng::Hasher) {
        h.i64(self.appetite.0);
        h.u64(self.doing as u64);
        h.u64(self.food as u64);
        h.u64(self.log.len() as u64);
        for e in &self.log { h.i64(e.spent.0); h.i64(e.ate.0); h.i64(e.hunger.0) }
        for m in &self.mass_memory { h.i64(m.0) }
    }
}
