//! The teaching scenarios' setups. Scripts place people and set world events (night, a spring
//! drying, a note dropped) and starting thirst only: every utterance, walk, drink and belief comes
//! from the people's own minds.
//!
//! The cast: Ash knows the spring behind the ridge (and the stream, the big rock, the windmill).
//! Bryn knows the stream, the rock, the windmill and the old oak, not the spring. Cai knows the
//! stream, the windmill and the oak (not the rock). Dee knows the stream, the rock and the windmill.
//! Everyone knows the camp. Each person's paces put each landmark up to `pacing` cells off.

use super::person::{Medium, Person, Traits};
use super::sim::Valley;
use super::species::Founding;
use super::world::{Event, World, OAK, PEOPLE, ROCK, SPRING, STREAM, WINDMILL};
use crate::mind::Temperament;

/// Day `d` runs `d·DAY .. d·DAY + DUSK`; dusk to night; night at `d·DAY + NIGHT`.
pub const DAY: u32 = 301;
pub const DUSK: u32 = 240;
pub const NIGHT: u32 = 300;
pub const ASH: usize = 0;
pub const BRYN: usize = 1;
pub const CAI: usize = 2;
pub const DEE: usize = 3;

/// What happens to the spring: it stays wet, dries at a tick (leaving a fresh hollow), or has
/// long been dry (an old, cracked hollow).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spring { Wet, DriesAt(u32), DryOld }

#[derive(Clone, Debug)]
pub struct Setup {
    pub seed: u64,
    pub temperaments: [Temperament; PEOPLE],
    /// trust[i][j]: how far i trusts j at the start.
    pub trust: [[i32; PEOPLE]; PEOPLE],
    pub warmth: [[i32; PEOPLE]; PEOPLE],
    pub pacing: i32,
    pub traits: [Traits; PEOPLE],
    pub medium: Medium,
    /// Ash last saw the spring dry (it believes it dry).
    pub ash_knows_dry: bool,
    /// What happens to the spring.
    pub spring: Spring,
    pub knows: [Vec<u32>; PEOPLE],
    pub days: u32,
    pub model_on: bool,
    pub offload: bool,
    /// Bryn knows the spring's place and believes it dry, this strongly (0: Bryn doesn't know it).
    pub bryn_prior_dry: i32,
    /// Where everyone starts (before their minds are founded, so they know where the others are).
    pub pos: [(i32, i32); PEOPLE],
    /// Stage 3: ally[i][j], how far i counts on j to stand with it (0: not at all).
    pub ally: [[i32; PEOPLE]; PEOPLE],
    /// Stage 3: each person's habit of crying out in danger (see `Founding::calls`).
    pub calls: [i32; PEOPLE],
}

impl Setup {
    pub fn new(seed: u64) -> Setup {
        Setup {
            seed, temperaments: [Temperament::MEDIUM; PEOPLE], trust: [[600; PEOPLE]; PEOPLE], warmth: [[300; PEOPLE]; PEOPLE], pacing: 2,
            traits: [Traits::default(); PEOPLE], medium: Medium::Speak, ash_knows_dry: false, spring: Spring::Wet,
            knows: [vec![STREAM, ROCK, WINDMILL, SPRING], vec![STREAM, ROCK, WINDMILL, OAK], vec![STREAM, WINDMILL, OAK], vec![STREAM, ROCK, WINDMILL]],
            days: 3, model_on: true, offload: true, bryn_prior_dry: 0, pos: [(10, 23), (12, 23), (55, 34), (54, 35)], ally: [[0; PEOPLE]; PEOPLE], calls: [super::species::CALL_HABIT; PEOPLE],
        }
    }
    /// Seeded temperaments for everyone (individual `k` of a set).
    pub fn temper(mut self, set: u32) -> Setup { for i in 0..PEOPLE { self.temperaments[i] = Temperament::seeded(self.seed ^ 0x7e, set * 4 + i as u32); } self }

    /// Build the valley: everyone at the camp unless moved; Bryn thirsty (650), the others not.
    pub fn build(&self) -> Valley {
        let mut w = World::valley(self.seed);
        match self.spring {
            Spring::Wet => {}
            Spring::DriesAt(t) => w.script.push((t, Event::DrySpring)),
            Spring::DryOld => { w.script.push((0, Event::DrySpring)); w.script.push((0, Event::AgeHollow)); }
        }
        for (i, &(x, y)) in self.pos.iter().enumerate() { w.people[i].x = x; w.people[i].y = y; w.people[i].px = x; w.people[i].py = y; }
        w.people[BRYN].thirst = 650;
        for i in [ASH, CAI, DEE] { w.people[i].thirst = 0; }
        let folk: Vec<Person> = (0..PEOPLE).map(|i| {
            let mut knows = self.knows[i].clone();
            if i == BRYN && self.bryn_prior_dry > 0 { knows.push(SPRING); }
            let spring_dry = (i == ASH && self.ash_knows_dry) || (i == BRYN && self.bryn_prior_dry > 0);
            let dry_conf = if i == BRYN { self.bryn_prior_dry } else { 900 };
            let f = Founding { knows, pacing: self.pacing, trust: self.trust[i].to_vec(), warmth: self.warmth[i].to_vec(), spring_dry, dry_conf, ally: self.ally[i].to_vec(), calls: self.calls[i] };
            let mut p = Person::new(&w, i, self.temperaments[i], &f);
            p.traits = self.traits[i];
            p.model_on = self.model_on;
            p.offload = self.offload;
            if i == ASH { p.medium = self.medium; }
            p
        }).collect();
        let mut v = Valley::new(w, folk);
        for d in 0..self.days { v.at(d * DAY + DUSK, Event::Dusk); v.at(d * DAY + NIGHT, Event::Night); }
        v
    }
}

/// Bryn's told record from Ash about the spring, if any.
/// (The water claim from Ash nearest the real spring: Bryn may also have overheard Ash on the stream.)
pub fn bryn_told(v: &Valley) -> Option<&super::person::Told> {
    let (sx, sy) = v.w.spring();
    v.folk[BRYN].told.iter().filter(|t| t.from == Some(ASH) && !t.dry_claim).min_by_key(|t| super::world::d2(t.est.0, t.est.1, sx, sy))
}
