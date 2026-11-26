//! The scenario: one person, ~20 days. Work is on offer every day; on day 9 at
//! 09:00 an injury leaves 15% of their muscle power for the rest of the run.

use crate::body::*;
use crate::brain::*;
use crate::rng::Hasher;
use crate::units::*;

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub seed: u64,
    pub days: u64,
    pub injury_day: u64,
    /// Share of muscle power the injury leaves, ‰.
    pub injury_cap_pm: i64,
    pub habit: Habit,
}

impl Default for Config {
    fn default() -> Config { Config { seed: 7, days: 20, injury_day: 9, injury_cap_pm: 150, habit: Habit::Learned } }
}

/// One row of the day-by-day table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Day {
    pub day: u64,
    pub work_minutes: u64,
    pub eat_minutes: u64,
    /// Metabolic cost above basal.
    pub activity: Energy,
    /// Basal + activity (what the person "felt" spending).
    pub spent: Energy,
    /// Chemical energy eaten.
    pub intake: Energy,
    /// Everything that left the body: heat, work and waste.
    pub out: Energy,
    /// Change in the energy stored inside the body (= intake − out, exactly).
    pub stored_change: Energy,
    /// The morning weigh-in, at 06:00 before breakfast.
    pub mass: Mass,
    /// Fat at the end of the day.
    pub fat: Mass,
    /// The habit used during this day.
    pub appetite: Q16,
    /// Glycogen fill at the weigh-in.
    pub glycogen_fill: Q16,
    pub hash: u64,
}

pub struct Run {
    pub days: Vec<Day>,
    pub decisions: Vec<Decision>,
    pub nights: Vec<Night>,
    pub body: Body,
    pub brain: Brain,
    /// The largest |energy gap| seen after any tick (should be 0).
    pub worst_gap: Energy,
    pub hash: u64,
}

pub fn state_hash(tick: u64, body: &Body, brain: &Brain) -> u64 {
    let mut h = Hasher::new(brain.seed);
    h.u64(tick);
    body.hash_into(&mut h);
    brain.hash_into(&mut h);
    h.finish()
}

pub fn run(cfg: Config) -> Run {
    let mut body = Body::adult();
    let mut brain = Brain::new(cfg.seed, 1, cfg.habit);
    let mut days = Vec::new();
    let mut decisions = Vec::new();
    let mut nights = Vec::new();
    let mut worst_gap = Energy::ZERO;
    let mut today = Day { day: 1, appetite: brain.appetite, ..Default::default() };
    let out_of = |b: &Body| b.books.heat_out + b.books.work_out + b.books.waste_out;
    let mut out_mark = out_of(&body);
    let mut stored_mark = body.stored();
    let injury_tick = (cfg.injury_day - 1) * TICKS_PER_DAY + 9 * TICKS_PER_HOUR;

    for tick in 0..cfg.days * TICKS_PER_DAY {
        let pain = tick == injury_tick;
        if pain { body.injury_cap_pm = cfg.injury_cap_pm }
        // Senses read the body as the last tick left it.
        let s = Senses::read(&body, tick, true, pain);
        let think = tick % THINK_EVERY == 0;
        if let Some(d) = brain.reflex(tick, &s) { decisions.push(d) }
        if think { decisions.push(brain.choose(tick, &s)) }

        // The intent is applied to the body.
        let action = brain.doing;
        let ate = if action == Action::Eat { body.eat(brain.food, if brain.food == 0 { grams(15) } else { grams(12) }) } else { Energy::ZERO };
        let spent = body.step(action.effort());
        let gap = body.energy_gap();
        if gap.0.abs() > worst_gap.0.abs() { worst_gap = gap }
        brain.remember(spent, ate, &s, think);

        match action { Action::Work => today.work_minutes += 1, Action::Eat => today.eat_minutes += 1, _ => {} }
        today.activity += spent.activity;
        today.spent += spent.basal + spent.activity;
        today.intake += ate;

        if tick % TICKS_PER_DAY == 6 * TICKS_PER_HOUR {
            today.mass = body.mass();
            today.glycogen_fill = body.glycogen_fill();
        }
        if (tick + 1) % TICKS_PER_DAY == 0 {
            today.out = out_of(&body) - out_mark;
            today.stored_change = body.stored() - stored_mark;
            out_mark = out_of(&body);
            stored_mark = body.stored();
            today.fat = body.fat;
            if let Some(n) = brain.retrospect(&body, today.mass) { nights.push(n) }
            today.hash = state_hash(tick, &body, &brain);
            days.push(today);
            today = Day { day: today.day + 1, appetite: brain.appetite, ..Default::default() };
        }
    }
    let hash = state_hash(cfg.days * TICKS_PER_DAY, &body, &brain);
    Run { days, decisions, nights, body, brain, worst_gap, hash }
}

impl Run {
    pub fn csv(&self) -> String {
        let mut s = String::from("day,work_h,eat_min,activity_kJ,spent_kJ,intake_kJ,out_kJ,stored_change_kJ,mass_kg,fat_kg,appetite,glycogen_fill\n");
        for d in &self.days {
            s += &format!("{},{:.2},{},{:.0},{:.0},{:.0},{:.0},{:.0},{:.3},{:.3},{:.3},{:.3}\n", d.day, d.work_minutes as f64 / 60.0, d.eat_minutes,
                d.activity.kj(), d.spent.kj(), d.intake.kj(), d.out.kj(), d.stored_change.kj(), d.mass.kg(), d.fat.kg(), d.appetite.f(), d.glycogen_fill.f()); // printing only
        }
        s
    }

    /// Mean of a per-day number over days `from..=to` (1-based).
    pub fn mean(&self, from: u64, to: u64, f: impl Fn(&Day) -> i64) -> i64 {
        let v: Vec<i64> = self.days.iter().filter(|d| d.day >= from && d.day <= to).map(f).collect();
        v.iter().sum::<i64>() / v.len().max(1) as i64
    }
}
