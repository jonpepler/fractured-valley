//! Runs the people's valley: every person reads the world as it was at the start of the tick
//! (things, people, notes, and last tick's sounds), writes only itself, and returns a deed; the
//! world then applies every deed in person order. People can be stepped on any number of threads
//! in any order. Night: everyone sleeps **where they are** (no teleport home); from dusk their
//! thoughts turn to rest, and those it pulls walk home on their own.

use super::person::Person;
use super::world::{Deed, Kind, World, PEOPLE};
use crate::graph::LinkKey;
use crate::mind::Action;
use crate::rng::StateHasher;
use crate::store::Store;
use crate::think::{Hop, Mods};
use std::hash::{Hash, Hasher};

/// One recorded tick of one person's mind (for the viewer).
#[derive(Clone, Debug)]
pub struct Frame {
    pub tick: u32,
    pub me: (i32, i32),
    pub others: Vec<(i32, i32, String)>,
    pub things: Vec<(i32, i32, String)>,
    pub mods: Mods,
    pub thirst: i32,
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
    /// Stage 3: where the first boar is, and the mind's focus.
    pub boar: Option<(i32, i32)>,
    pub focus: i32,
    pub pain: i32,
}

#[derive(Clone)]
pub struct Valley {
    pub w: World,
    pub folk: Vec<Person>,
    /// Record this person's mind each tick (for the viewer).
    pub record: Option<usize>,
    pub frames: Vec<Frame>,
    pub snapshots: Vec<(u32, Vec<i16>, Vec<i16>)>,
    /// Stage 3: the boars' minds (boar `b` is `w.boars[b]`), stepped in the same tick as people.
    pub beasts: Vec<crate::mind::Mind<crate::store::Runtime>>,
}

impl Valley {
    pub fn new(w: World, folk: Vec<Person>) -> Valley { Valley { w, folk, record: None, frames: vec![], snapshots: vec![], beasts: vec![] } }
    pub fn at(&mut self, tick: u32, e: super::world::Event) { self.w.script.push((tick, e)); }
    pub fn recording(mut self, who: usize) -> Self {
        self.record = Some(who); self.folk[who].m.trace = true;
        let (w, v) = crate::fox::viewer::snapshot(&self.folk[who].m.store);
        self.snapshots.push((self.w.tick, w, v));
        self
    }

    /// One tick, on `threads` threads, visiting people in `order` (a permutation).
    pub fn step(&mut self, threads: usize, order: &[usize]) {
        let log0 = self.w.log.len();
        self.w.pre_step();
        let slept = self.w.night_now;
        if slept {
            let senses: Vec<_> = (0..self.folk.len()).map(|i| self.w.sense(i, true)).collect();
            run(&mut self.folk, threads, order, |i, p| { p.m.night(&senses[i]); Deed::default() });
            let bs: Vec<_> = (0..self.beasts.len()).map(|b| self.w.sense_boar(b)).collect();
            let border: Vec<usize> = (0..self.beasts.len()).collect();
            crate::fox::sim::run_minds(&mut self.beasts, threads, &border, |b, m| { m.night(&bs[b]); Action::Sleep });
            if let Some(r) = self.record { let (w, v) = crate::fox::viewer::snapshot(&self.folk[r].m.store); self.snapshots.push((self.w.tick, w, v)); }
        }
        let w = &self.w;
        let deeds = run(&mut self.folk, threads, order, |_, p| p.think(w));
        // Boars read the same start-of-tick world (in reverse order, so order is exercised too).
        let border: Vec<usize> = (0..self.beasts.len()).rev().collect();
        let boar_acts = crate::fox::sim::run_minds(&mut self.beasts, threads, &border, |b, m| { let s = w.sense_boar(b); let a = m.step(&s); m.commit(&s, a); a });
        if let Some(r) = self.record {
            let m = &self.folk[r].m;
            let rep = &m.last;
            let me = &self.w.people[r];
            self.frames.push(Frame {
                tick: self.w.tick, me: (me.x, me.y),
                others: (0..PEOPLE).filter(|&j| j != r).map(|j| (self.w.people[j].x, self.w.people[j].y, super::world::NAMES[j].to_string())).collect(),
                things: self.w.things.iter().filter(|t| t.alive && t.kind != Kind::Hut).map(|t| (t.x, t.y, t.name.clone()))
                    .chain(self.w.notes.iter().filter(|n| n.holder.is_none()).map(|n| (n.x, n.y, "a note".to_string()))).collect(),
                mods: m.mods, thirst: me.thirst, act: m.st.act.clone(), hops: rep.hops.clone(), ws: rep.ws.clone(), action: deeds[r].action.unwrap_or(Action::Stay), why: rep.why.clone(),
                events: vec![], fast: m.store.fast_entries().into_iter().filter(|&(_, f)| f >= 20).collect(),
                percepts: rep.percepts.iter().map(|p| (p.node, p.error)).collect(), slept, backlog: rep.backlog,
                boar: self.w.boars.first().map(|q| (q.x, q.y)), focus: m.mods.focus, pain: me.pain,
            });
        }
        self.w.post_step_with(&deeds, &boar_acts);
        if self.record.is_some() { if let Some(f) = self.frames.last_mut() { f.events = self.w.log[log0..].iter().map(|l| l.1.clone()).collect(); } }
    }
    pub fn run(&mut self, ticks: u32) { let order: Vec<usize> = (0..self.folk.len()).collect(); for _ in 0..ticks { self.step(1, &order); } }
    pub fn run_until(&mut self, tick: u32) { let order: Vec<usize> = (0..self.folk.len()).collect(); while self.w.tick < tick { self.step(1, &order); } }

    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new(self.w.seed);
        self.w.hash_into(&mut h);
        for p in &self.folk { p.state_hash().hash(&mut h); }
        for m in &self.beasts { m.state_hash().hash(&mut h); }
        h.finish()
    }
}

/// Step every person with `f(index, person)` on `threads` threads, visiting them in `order`;
/// results come back in person order, whatever order they ran in.
pub fn run<F>(folk: &mut [Person], threads: usize, order: &[usize], f: F) -> Vec<Deed>
where F: Fn(usize, &mut Person) -> Deed + Sync {
    let n = folk.len();
    let mut slots: Vec<Option<&mut Person>> = folk.iter_mut().map(Some).collect();
    let mut work: Vec<(usize, &mut Person)> = order.iter().map(|&i| (i, slots[i].take().expect("order is a permutation"))).collect();
    let mut out = vec![Deed::default(); n];
    if threads <= 1 { for (i, p) in work.iter_mut() { out[*i] = f(*i, p); } return out; }
    let chunk = n.div_ceil(threads).max(1);
    let results: Vec<Vec<(usize, Deed)>> = std::thread::scope(|sc| {
        let hs: Vec<_> = work.chunks_mut(chunk).map(|part| { let f = &f; sc.spawn(move || part.iter_mut().map(|(i, p)| (*i, f(*i, p))).collect::<Vec<_>>()) }).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (i, d) in results.into_iter().flatten() { out[i] = d; }
    out
}
