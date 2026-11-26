//! A toy world on a line: a few places, bodies, and the air that carries last tick's sounds.
//! Each tick every brain reads the same `View` of last tick and writes only itself and an
//! `Action`; the world applies the actions afterwards, in id order.

use crate::brain::{Action, Brain, Notes};
use crate::model::{Ent, Utt};
use crate::rng::StateHasher;
use std::hash::{Hash, Hasher};

pub const DAY: u32 = 1000;
pub const NIGHT: u32 = 800;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Place { pub ent: Ent, pub pos: i32, pub holds: Ent }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Body { pub pos: i32, pub hunger: i32, pub pain: i32, pub asleep: bool, pub voice: i32 }

/// How an utterance travels. Only voice exists so far; writing would be a lasting mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Medium { Voice }

/// An utterance in the air for one tick. Nobody is sent it: anyone in earshot perceives it.
/// `facing` is who the speaker turned to, a cue listeners can perceive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Spoken { pub from: u8, pub facing: Option<u8>, pub utt: Utt, pub vol: i32, pub pos: i32, pub medium: Medium }

/// Scripted world events (never lines of dialogue).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event { Put(u8, i32), Hunger(u8, i32), Pain(u8, i32) }

/// What a brain may read: the previous tick's world.
pub struct View<'a> {
    pub seed: u64,
    pub tick: u32,
    pub night: bool,
    pub bodies: &'a [Body],
    pub heard: &'a [Spoken],
    pub places: &'a [Place],
}

pub struct World {
    pub seed: u64,
    pub tick: u32,
    pub bodies: Vec<Body>,
    pub brains: Vec<Brain>,
    pub places: Vec<Place>,
    pub air: Vec<Spoken>,
    pub script: Vec<(u32, Event)>,
    pub explain: bool,
    pub trace: Vec<String>,
    /// Every utterance made: (tick, speaker, facing, content).
    pub said: Vec<(u32, u8, Option<u8>, Utt)>,
}

pub fn valley_places() -> Vec<Place> {
    vec![
        Place { ent: Ent::ColdBox, pos: 40, holds: Ent::Food },
        Place { ent: Ent::Well, pos: 20, holds: Ent::Water },
        Place { ent: Ent::Meadow, pos: 60, holds: Ent::Flowers },
    ]
}

pub fn stamp(t: u32) -> String { format!("d{} {:03}", t / DAY + 1, t % DAY) }

impl World {
    /// `starts`: each brain's starting position.
    pub fn new(seed: u64, brains: Vec<Brain>, starts: &[i32]) -> World {
        let bodies = starts.iter().map(|&pos| Body { pos, hunger: 0, pain: 0, asleep: false, voice: 1000 }).collect();
        World { seed, tick: 0, bodies, brains, places: valley_places(), air: vec![], script: vec![], explain: false, trace: vec![], said: vec![] }
    }

    pub fn at(&mut self, day: u32, t: u32, e: Event) { self.script.push(((day - 1) * DAY + t, e)); }

    pub fn step(&mut self) { let order: Vec<usize> = (0..self.brains.len()).collect(); self.step_order(&order) }

    pub fn step_order(&mut self, order: &[usize]) {
        for &(tk, e) in &self.script {
            if tk != self.tick { continue; }
            match e {
                Event::Put(i, p) => self.bodies[i as usize].pos = p,
                Event::Hunger(i, h) => self.bodies[i as usize].hunger = h,
                Event::Pain(i, p) => self.bodies[i as usize].pain = p,
            }
            if self.explain { self.trace.push(format!("[{}] world: {:?}", stamp(self.tick), e)); }
        }
        let n = self.brains.len();
        let view = View { seed: self.seed, tick: self.tick, night: self.tick % DAY >= NIGHT, bodies: &self.bodies, heard: &self.air, places: &self.places };
        let mut acts = vec![Action::Idle; n];
        let mut notes: Vec<Vec<String>> = vec![vec![]; n];
        for &i in order {
            let mut ex = Notes::new(self.explain);
            acts[i] = self.brains[i].think(&view, &mut ex);
            notes[i] = ex.lines;
        }
        let mut air = vec![];
        for i in 0..n {
            let who = Ent::Agent(i as u8).word();
            for l in &notes[i] { self.trace.push(format!("[{}] {:<4}  · {}", stamp(self.tick), who, l)); }
            let b = &mut self.bodies[i];
            match acts[i] {
                Action::Say { utt, facing, vol } => {
                    air.push(Spoken { from: i as u8, facing, utt, vol, pos: b.pos, medium: Medium::Voice });
                    self.said.push((self.tick, i as u8, facing, utt));
                    if self.explain {
                        let to = facing.map(|f| format!(" (to {})", Ent::Agent(f).word())).unwrap_or_default();
                        self.trace.push(format!("[{}] {:<4}{}: «{}»   vol {}", stamp(self.tick), who, to, utt.render(i as u8, facing), vol));
                    }
                }
                Action::Walk { to } => {
                    if let Some(pl) = self.places.iter().find(|p| p.ent == to) { b.pos += (pl.pos - b.pos).signum(); }
                }
                Action::Eat => {
                    if self.places.iter().any(|p| (p.pos - b.pos).abs() <= 1 && p.holds == Ent::Food) { b.hunger = 0; }
                }
                Action::Sleep => b.asleep = true,
                Action::Wake => b.asleep = false,
                Action::Idle => {}
            }
        }
        self.air = air;
        self.tick += 1;
    }

    pub fn run_to(&mut self, day: u32, t: u32) { let end = (day - 1) * DAY + t; while self.tick < end { self.step() } }

    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new(self.seed);
        self.tick.hash(&mut h);
        self.bodies.hash(&mut h);
        self.air.hash(&mut h);
        self.brains.hash(&mut h);
        h.finish()
    }

    /// Utterances by `who` in ticks `from..to`.
    pub fn said_by(&self, who: u8, from: u32, to: u32) -> Vec<Utt> {
        self.said.iter().filter(|s| s.1 == who && s.0 >= from && s.0 < to).map(|s| s.3).collect()
    }
}
