//! Runs the valley and its foxes' minds together. [`Sim`] is one fox (the scenarios), recording
//! what happened each tick for the viewer. [`Pack`] is many foxes in one valley, stepped on any
//! number of threads in any order: each tick every mind reads the world as it was at the start of
//! the tick and writes only itself; the world then applies every action in fox order.

use super::found::{self, Fox};
use super::world::{DogMode, Event, Kind, World, DOG};
use crate::graph::LinkKey;
use crate::mind::{Action, Mind, Temperament};
use crate::rng::StateHasher;
use crate::store::{Runtime, Store};
use crate::think::{Hop, Mods};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

/// The fox species of the standard valley, built once per process and shared by every mind.
pub fn valley_fox() -> Arc<Fox> {
    static FOX: OnceLock<Arc<Fox>> = OnceLock::new();
    FOX.get_or_init(|| Arc::new(found::species(&World::valley(0)))).clone()
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub tick: u32,
    pub fox: (i32, i32),
    pub dog: Option<(i32, i32, DogMode)>,
    pub fires: Vec<(i32, i32, bool)>,
    pub mods: Mods,
    pub hunger: i32,
    pub pain: i32,
    pub act: Vec<(u32, i32)>,
    pub hops: Vec<Hop>,
    pub ws: Vec<u32>,
    pub action: Action,
    pub why: String,
    pub events: Vec<String>,
    pub fast: Vec<(LinkKey, i16)>,
    pub percepts: Vec<(u32, i32)>,
    pub slept: bool,
    pub backlog: i64,
    pub consolidated: i64,
    pub burrows: Vec<(i32, i32)>,
}

pub struct Sim<S: Store = Runtime> {
    pub w: World,
    pub m: Mind<S>,
    pub fox: Arc<Fox>,
    pub record: bool,
    pub frames: Vec<Frame>,
    pub nights: Vec<u32>,
    pub record_from: u32,
    /// Slow weights and valences at the start and after each night (when recording).
    pub snapshots: Vec<(u32, Vec<i16>, Vec<i16>)>,
}

impl Sim<Runtime> {
    pub fn new(seed: u64) -> Sim<Runtime> { Self::with(seed, Temperament::MEDIUM) }
    pub fn with(seed: u64, t: Temperament) -> Sim<Runtime> { Self::with_fox(seed, t, valley_fox()) }
    /// A fox of another species build (e.g. a big mind from `scale`).
    pub fn with_fox(seed: u64, t: Temperament, fox: Arc<Fox>) -> Sim<Runtime> {
        let w = World::valley(seed);
        let m = fox.mind(t, seed);
        Sim { w, m, fox, record: false, frames: vec![], nights: vec![], record_from: 0, snapshots: vec![] }
    }
    /// The same run, with the mind in its expanded (inspectable) form.
    pub fn expanded(self) -> Sim<crate::store::Expanded> {
        Sim { m: self.m.expand(), w: self.w, fox: self.fox, record: self.record, frames: self.frames, nights: self.nights, record_from: self.record_from, snapshots: self.snapshots }
    }
}

impl<S: Store> Sim<S> {
    pub fn recording(mut self) -> Self {
        self.record = true; self.m.trace = true;
        let (w, v) = super::viewer::snapshot(&self.m.store);
        self.snapshots.push((self.w.tick, w, v));
        self
    }
    pub fn at(&mut self, tick: u32, e: Event) { self.w.script.push((tick, e)); }

    pub fn step(&mut self) -> Action {
        let log_before = self.w.log.len();
        self.w.pre_step();
        let slept = self.w.night_now;
        if slept {
            // The fox spends the night where its mind believes home is (it doesn't walk there).
            let (x, y) = self.m.home_xy();
            self.w.foxes[0].x = x; self.w.foxes[0].y = y;
            let s = self.w.sense_awake(0);
            self.m.night(&s);
            self.nights.push(self.w.tick);
            if self.record { let (w, v) = super::viewer::snapshot(&self.m.store); self.snapshots.push((self.w.tick, w, v)); }
        }
        let s = self.w.sense(0);
        let a = self.m.step(&s);
        self.m.commit(&s, a);
        if self.record && self.w.tick >= self.record_from {
            let r = &self.m.last;
            let dog = self.w.entity(DOG).filter(|d| d.alive).map(|d| (d.x, d.y, self.w.dog_mode));
            let f = self.w.fox();
            self.frames.push(Frame {
                tick: self.w.tick, fox: (f.x, f.y), dog,
                fires: self.w.entities.iter().filter(|e| e.alive && e.kind == Kind::Fire).map(|e| (e.x, e.y, e.id < 100)).collect(),
                mods: self.m.mods, hunger: f.hunger, pain: f.pain, act: self.m.st.act.clone(),
                hops: r.hops.clone(), ws: r.ws.clone(), action: a, why: r.why.clone(),
                events: self.w.log[log_before..].iter().map(|l| l.1.clone()).collect(),
                fast: self.m.store.fast_entries().into_iter().filter(|&(_, f)| f >= 20).collect(),
                percepts: r.percepts.iter().map(|p| (p.node, p.error)).collect(), slept, backlog: r.backlog, consolidated: r.consolidated,
                burrows: self.w.entities.iter().filter(|e| e.alive && e.kind == Kind::Den).map(|e| (e.x, e.y)).collect(),
            });
        }
        let log_now = self.w.log.len();
        self.w.post_step(&[a]);
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

/// Many foxes sharing one valley and one species base.
pub struct Pack {
    pub w: World,
    pub minds: Vec<Mind<Runtime>>,
}

impl Pack {
    /// `n` foxes of seeded temperaments, at the den; the dog guards the henhouse.
    pub fn new(seed: u64, n: usize) -> Pack {
        let fox = valley_fox();
        let mut w = World::valley_with(seed, n);
        for (i, f) in w.foxes.iter_mut().enumerate() { f.hunger = 400 + (crate::rng::hash(seed, i as u64, 0, 0x9ac) % 600) as i32; }
        let minds = (0..n).map(|i| fox.mind(Temperament::seeded(seed, i as u32), seed ^ ((i as u64) << 32))).collect();
        Pack { w, minds }
    }
    /// One tick. Minds are stepped on `threads` threads, visiting them in `order` (a permutation
    /// of the minds; each thread takes a contiguous slice of it). Every mind reads the same senses
    /// (the world at the start of the tick) and writes only itself.
    pub fn step(&mut self, threads: usize, order: &[usize]) {
        self.w.pre_step();
        if self.w.night_now {
            for i in 0..self.minds.len() {
                let (x, y) = self.minds[i].home_xy();
                self.w.foxes[i].x = x; self.w.foxes[i].y = y;
            }
            let senses: Vec<_> = (0..self.minds.len()).map(|i| self.w.sense_awake(i)).collect();
            run_minds(&mut self.minds, threads, order, |i, m| { m.night(&senses[i]); Action::Sleep });
        }
        let senses: Vec<_> = (0..self.minds.len()).map(|i| self.w.sense(i)).collect();
        let actions = run_minds(&mut self.minds, threads, order, |i, m| { let a = m.step(&senses[i]); m.commit(&senses[i], a); a });
        self.w.post_step(&actions);
    }
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new(self.w.seed);
        self.w.hash_into(&mut h);
        for m in &self.minds { m.state_hash().hash(&mut h); }
        h.finish()
    }
}

/// Step every mind with `f(index, mind)`, on `threads` threads, in `order`. Returns each mind's
/// result in mind order (whatever order they ran in).
pub fn run_minds<F>(minds: &mut [Mind<Runtime>], threads: usize, order: &[usize], f: F) -> Vec<Action>
where F: Fn(usize, &mut Mind<Runtime>) -> Action + Sync {
    let n = minds.len();
    let mut slots: Vec<Option<&mut Mind<Runtime>>> = minds.iter_mut().map(Some).collect();
    let mut work: Vec<(usize, &mut Mind<Runtime>)> = order.iter().map(|&i| (i, slots[i].take().expect("order is a permutation"))).collect();
    let mut out = vec![Action::Stay; n];
    if threads <= 1 {
        for (i, m) in work.iter_mut() { out[*i] = f(*i, m); }
        return out;
    }
    let chunk = n.div_ceil(threads).max(1);
    let results: Vec<Vec<(usize, Action)>> = std::thread::scope(|sc| {
        let hs: Vec<_> = work.chunks_mut(chunk).map(|part| { let f = &f; sc.spawn(move || part.iter_mut().map(|(i, m)| (*i, f(*i, m))).collect::<Vec<_>>()) }).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (i, a) in results.into_iter().flatten() { out[i] = a; }
    out
}

/// The node of one of the valley's founding places, by the world's name for it.
pub fn place_node<S: Store>(m: &Mind<S>, w: &World, name: &str) -> u32 { m.place_nodes[w.place_index(name)] }
