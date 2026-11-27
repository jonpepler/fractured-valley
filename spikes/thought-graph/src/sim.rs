//! Runs the world and the fox's mind together, and records what happened each tick.

use crate::mind::Mind;
use crate::rng::StateHasher;
use crate::spread::{Hop, Mods};
use crate::world::{Action, DogMode, Event, Kind, World, DOG};
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug)]
pub struct Frame {
    pub tick: u32,
    pub fox: (i32, i32),
    pub dog: Option<(i32, i32, DogMode)>,
    /// (x, y, expected?) for every burning fire.
    pub fires: Vec<(i32, i32, bool)>,
    pub mods: Mods,
    pub hunger: i32,
    pub pain: i32,
    pub act: Vec<i32>,
    pub hops: Vec<Hop>,
    pub ws: Vec<u32>,
    pub action: Action,
    pub why: String,
    pub events: Vec<String>,
    /// (link, fast weight) for links with a visible fast weight.
    pub fast: Vec<(u32, i16)>,
    /// (node, prediction error) for each thing in sight.
    pub percepts: Vec<(u32, i32)>,
    pub slept: bool,
}

pub struct Sim {
    pub w: World,
    pub m: Mind,
    pub record: bool,
    pub frames: Vec<Frame>,
    /// Slow weights after each sleep, for the viewer: (tick, weights).
    pub slow_snapshots: Vec<(u32, Vec<i16>, Vec<i16>)>,
    /// Ticks at which `Night` slept the fox.
    pub nights: Vec<u32>,
    /// Record frames only from this tick on.
    pub record_from: u32,
    /// When thinking with the compiled form, recompile it from the graph this often (it is a
    /// snapshot: the day's fast weights are learnt in the graph form).
    pub recompile_every: u32,
}

impl Sim {
    pub fn new(seed: u64) -> Sim {
        let w = World::valley(seed);
        let m = Mind::new(&w);
        let snap = (0, m.g.slow.clone(), m.g.val.clone());
        Sim { w, m, record: false, frames: vec![], slow_snapshots: vec![snap], nights: vec![], record_from: 0, recompile_every: 10 }
    }
    pub fn recording(mut self) -> Sim { self.record = true; self.m.trace = true; self }
    pub fn at(&mut self, tick: u32, e: Event) { self.w.script.push((tick, e)); }

    pub fn step(&mut self) -> Action {
        let log_before = self.w.log.len();
        self.w.pre_step();
        let slept = self.w.night_now;
        if slept {
            self.m.sleep();
            self.nights.push(self.w.tick);
            self.slow_snapshots.push((self.w.tick, self.m.g.slow.clone(), self.m.g.val.clone()));
        }
        if self.m.compiled.is_some() && self.recompile_every > 0 && self.w.tick % self.recompile_every == 0 { self.m.recompile(); }
        let a = self.m.step(&self.w);
        self.m.commit(&self.w, a);
        if self.record && self.w.tick >= self.record_from {
            let r = &self.m.last;
            let dog = self.w.entity(DOG).filter(|d| d.alive).map(|d| (d.x, d.y, self.w.dog_mode));
            self.frames.push(Frame {
                tick: self.w.tick, fox: (self.w.fox.x, self.w.fox.y), dog,
                fires: self.w.entities.iter().filter(|e| e.alive && e.kind == Kind::Fire).map(|e| (e.x, e.y, e.id < 100)).collect(),
                mods: self.m.mods, hunger: self.w.fox.hunger, pain: self.w.fox.pain, act: self.m.st.act.clone(),
                hops: r.hops.clone(), ws: r.ws.clone(), action: a, why: r.why.clone(),
                events: self.w.log[log_before..].iter().map(|l| l.1.clone()).collect(),
                fast: self.m.fast.iter().enumerate().filter(|(_, &f)| f >= 20).map(|(l, &f)| (l as u32, f)).collect(),
                percepts: r.percepts.iter().map(|p| (p.node, p.error)).collect(), slept,
            });
        }
        let log_now = self.w.log.len();
        self.w.post_step(a);
        if self.record && self.w.tick > self.record_from && self.w.log.len() > log_now {
            if let Some(f) = self.frames.last_mut() { f.events.extend(self.w.log[log_now..].iter().map(|l| l.1.clone())); }
        }
        a
    }

    pub fn run(&mut self, ticks: u32) -> Vec<Action> { (0..ticks).map(|_| self.step()).collect() }

    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new(self.w.seed);
        self.w.hash_into(&mut h);
        self.m.hash(&mut h);
        h.finish()
    }
}
