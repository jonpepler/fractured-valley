//! The scripted situations shared by the tests and the viewer. Scripts set world events and
//! starting body states only; every choice comes from the fox's mind.

use crate::sim::Sim;
use crate::world::Event;

pub const SEED: u64 = 7;
/// Day 1 runs ticks 0..300; night falls at 300; day 2 starts at 301.
pub const NIGHT: u32 = 300;
pub const DAY2: u32 = NIGHT + 1;
pub const MORNING_HUNGER: i32 = 700;

/// Day 1: a hungry fox (700) leaves its den; the dog guards the henhouse. Night at 300, and on
/// day 2 the fox wakes hungry again (700).
pub fn dog_day(seed: u64) -> Sim {
    let mut s = Sim::new(seed);
    s.w.fox.hunger = MORNING_HUNGER;
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
    s.w.fox.x = p.x; s.w.fox.y = p.y; s.w.fox.hunger = 0;
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
    s.w.fox.hunger = h;
    s.m.pin_arousal = Some(a);
    if m != 0 { s.m.pin_mood = Some(m); }
    s.m.learn = false;
    s
}

/// The dog charges a resting fox. The fox lies 10 cells from the dog's post, at the edge of
/// sight, not hungry. At tick 3 the dog gives chase. `individual` picks a seeded temperament.
pub const CHARGE_AT: u32 = 3;
pub fn dog_charges(seed: u64, individual: u32) -> Sim {
    let mut s = Sim::new(seed);
    s.m = crate::mind::Mind::with_temperament(&s.w, crate::mind::Temperament::seeded(seed, individual));
    s.w.fox.x = 24; s.w.fox.y = 20; s.w.fox.hunger = 200;
    s.at(CHARGE_AT, Event::DogChase);
    s
}
