//! The scripted situations shared by the tests and the viewer, ported from `spikes/thought-graph`. Scripts set world events and
//! starting body states only; every choice comes from the fox's mind.

use super::sim::Sim;
use super::world::Event;

pub const SEED: u64 = 7;
/// Day 1 runs ticks 0..300; night falls at 300; day 2 starts at 301.
pub const NIGHT: u32 = 300;
pub const DAY2: u32 = NIGHT + 1;
pub const MORNING_HUNGER: i32 = 700;

/// Day 1: a hungry fox (700) leaves its den; the dog guards the henhouse. Night at 300, and on
/// day 2 the fox wakes hungry again (700).
pub fn dog_day(seed: u64) -> Sim {
    let mut s = Sim::new(seed);
    s.w.foxes[0].hunger = MORNING_HUNGER;
    s.at(NIGHT, Event::Night);
    s.at(DAY2, Event::SetHunger(MORNING_HUNGER));
    s
}

/// The same days with no dog: the control.
pub fn no_dog_day(seed: u64) -> Sim {
    let mut s = dog_day(seed);
    s.at(0, Event::NoDog);
    s
}

/// The same days, but the night is not slept on: the day's fast weights and episodes are simply
/// lost (as if the fox lay awake until they faded). A control for sleep.
pub fn dog_day_no_sleep(seed: u64) -> Sim {
    let mut s = dog_day(seed);
    s.m.skip_sleep = true;
    s
}

/// Day 2 of `dog_day`, with a fire breaking out in the meadow, on the fox's way to the berries,
/// at 326, out at 370. On the way it passes the camp, where a campfire always burns.
pub const FIRE_AT: u32 = 326;
pub const FIRE_XY: (i32, i32) = (35, 16);
pub fn meadow_fire(seed: u64) -> Sim {
    let mut s = dog_day(seed);
    s.at(FIRE_AT, Event::FireAt { x: FIRE_XY.0, y: FIRE_XY.1 });
    s.at(370, Event::FiresOut);
    s
}

/// A fox at a place, not hungry, starting in a low mood (-500), nothing else happening.
pub fn mood_at(seed: u64, place: &str) -> Sim {
    let mut s = Sim::new(seed);
    let p = s.w.places[s.w.place_index(place)].clone();
    s.w.foxes[0].x = p.x; s.w.foxes[0].y = p.y; s.w.foxes[0].hunger = 0;
    s.m.mods.mood = -500;
    s
}

/// The modulator states compared in outcome 1: (name, hunger, pinned arousal, pinned mood).
pub const STATES: [(&str, i32, i32, i32); 4] = [
    ("neutral", 700, 0, 0),
    ("hungry", 950, 0, 0),
    ("afraid", 700, 600, 0),
    ("low mood", 700, 0, -600),
];

/// The fox at its den in one of `STATES`, held there (arousal and mood pinned), with day-learning
/// off so that only the modulators differ.
pub fn modulated(seed: u64, state: usize) -> Sim {
    let (_, h, a, m) = STATES[state];
    let mut s = Sim::new(seed);
    s.w.foxes[0].hunger = h;
    s.m.pin_arousal = Some(a);
    if m != 0 { s.m.pin_mood = Some(m); }
    s.m.learn = false;
    s
}

/// The dog charges a resting fox. The fox lies 10 cells from the dog's post, at the edge of
/// sight, not hungry. At tick 3 the dog gives chase. `individual` picks a seeded temperament.
pub const CHARGE_AT: u32 = 3;
pub fn dog_charges(seed: u64, individual: u32) -> Sim {
    let mut s = Sim::with(seed, crate::mind::Temperament::seeded(seed, individual));
    s.w.foxes[0].x = 24; s.w.foxes[0].y = 20; s.w.foxes[0].hunger = 200;
    s.at(CHARGE_AT, Event::DogChase);
    s
}

// ---------------------------------------------------------------- sleep pressure and home moves

/// Ticks per day in multi-day runs: day `d` (0-based) runs `d·DAY..d·DAY+NIGHT`, night falls at
/// `d·DAY + NIGHT`, and the next morning the fox wakes hungry (700).
pub const DAY: u32 = NIGHT + 1;
pub fn night_of(d: u32) -> u32 { d * DAY + NIGHT }
pub fn morning_of(d: u32) -> u32 { d * DAY }

/// `days` quiet days (no dog): a hungry fox leaves its den each morning; night each evening.
pub fn quiet_days(seed: u64, days: u32) -> Sim {
    let mut s = Sim::new(seed);
    s.w.foxes[0].hunger = MORNING_HUNGER;
    s.at(0, Event::NoDog);
    for d in 0..days { s.at(night_of(d), Event::Night); s.at(night_of(d) + 1, Event::SetHunger(MORNING_HUNGER)); }
    s
}

/// The same days for a fox that never sleeps: each night it lies awake in its den.
pub fn sleepless_days(seed: u64, days: u32) -> Sim {
    let mut s = quiet_days(seed, days);
    s.m.skip_sleep = true;
    s
}

/// A busy day: the dog bites at the henhouse, and a fire breaks out in the meadow in the afternoon.
pub fn busy_day(seed: u64) -> Sim {
    let mut s = dog_day(seed);
    s.at(160, Event::FireAt { x: FIRE_XY.0, y: FIRE_XY.1 });
    s.at(200, Event::FiresOut);
    s.at(220, Event::DogChase);
    s
}

/// The home experiment. On quiet days, while the fox is away at the henhouse on day 1 (tick
/// `MOVE_AT`), something supernatural happens to its burrow at the den (8, 30).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeMove {
    /// Nothing happens (the control).
    None,
    /// Moved far away, out of sight of everywhere the fox goes.
    Far,
    /// Moved near: within sight of the old den.
    Near,
    /// Moved out of sight of the old den, but within the reach of a search around it.
    Mid,
    /// Removed: there is no burrow any more.
    Removed,
    /// A second, identical-looking burrow appears near the den; the old one stays.
    Second,
    /// Moved to a new seeded place every night.
    Daily,
}
pub const MOVE_AT: u32 = 100;
pub const FAR_XY: (i32, i32) = (54, 34);
pub const NEAR_XY: (i32, i32) = (15, 35);
pub const MID_XY: (i32, i32) = (4, 12);
/// Where the burrow is put on night `d` of the daily variant: seeded, anywhere in the valley.
pub fn daily_xy(seed: u64, d: u32) -> (i32, i32) {
    let h = |k| crate::rng::hash(seed, d as u64, k, 0xd3e);
    (4 + (h(1) % 56) as i32, 4 + (h(2) % 32) as i32)
}
pub fn home_moved(seed: u64, v: HomeMove, days: u32) -> Sim {
    let mut s = quiet_days(seed, days);
    let ev = match v {
        HomeMove::None => None,
        HomeMove::Far => Some(Event::MoveDen { x: FAR_XY.0, y: FAR_XY.1 }),
        HomeMove::Near => Some(Event::MoveDen { x: NEAR_XY.0, y: NEAR_XY.1 }),
        HomeMove::Mid => Some(Event::MoveDen { x: MID_XY.0, y: MID_XY.1 }),
        HomeMove::Removed => Some(Event::RemoveDen),
        HomeMove::Second => Some(Event::AddDen { x: NEAR_XY.0, y: NEAR_XY.1 }),
        HomeMove::Daily => None,
    };
    if let Some(e) = ev { s.at(MOVE_AT, e); }
    if v == HomeMove::Daily {
        for d in 0..days { let (x, y) = daily_xy(seed, d); s.at(night_of(d), Event::MoveDen { x, y }); }
    }
    s
}

/// Eventful days for a fox that never sleeps: the dog is about, and each afternoon a fire breaks
/// out somewhere (seeded) and later the dog gives chase.
pub fn sleepless_eventful_days(seed: u64, days: u32) -> Sim {
    let mut s = Sim::new(seed);
    s.w.foxes[0].hunger = MORNING_HUNGER;
    s.m.skip_sleep = true;
    for d in 0..days {
        let h = |k| crate::rng::hash(seed, d as u64, k, 0xf1e);
        let t = morning_of(d);
        s.at(t + 150, Event::FireAt { x: 10 + (h(1) % 44) as i32, y: 6 + (h(2) % 28) as i32 });
        s.at(t + 190, Event::FiresOut);
        s.at(t + 230, Event::DogChase);
        s.at(night_of(d), Event::Night);
        s.at(night_of(d) + 1, Event::SetHunger(MORNING_HUNGER));
    }
    s
}
