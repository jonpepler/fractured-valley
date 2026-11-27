//! Runs the people's valley: every person reads the world as it was at the start of the tick
//! (things, people, notes, and last tick's sounds), writes only itself, and returns a deed; the
//! world then applies every deed in person order. People can be stepped on any number of threads
//! in any order. Night: everyone sleeps **where they are** (no teleport home); from dusk their
//! thoughts turn to rest, and those it pulls walk home on their own.

use super::person::Person;
use super::world::{Deed, Kind, World};
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
    /// Stage 4: the deer's minds (deer `k` is `w.deer[k]`), stepped in the same tick, after boars.
    pub deer_minds: Vec<crate::mind::Mind<crate::store::Runtime>>,
    /// Stage 5: the foxes' minds (fox `k` is the village's fox `k`), stepped after the deer.
    pub fox_minds: Vec<crate::mind::Mind<crate::store::Runtime>>,
    /// Stage 5: wall time of the brains' pass (people, then animals), for measures; never hashed.
    pub brain_ns: u64,
}

impl Valley {
    pub fn new(w: World, folk: Vec<Person>) -> Valley { Valley { w, folk, record: None, frames: vec![], snapshots: vec![], beasts: vec![], deer_minds: vec![], fox_minds: vec![], brain_ns: 0 } }
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
            run(&mut self.folk, threads, order, |i, p| { p.night(&senses[i]); Deed::default() });
            let bs: Vec<_> = (0..self.beasts.len()).map(|b| self.w.sense_boar(b)).collect();
            let border: Vec<usize> = (0..self.beasts.len()).collect();
            crate::fox::sim::run_minds(&mut self.beasts, threads, &border, |b, m| { m.night(&bs[b]); Action::Sleep });
            let ds: Vec<_> = (0..self.deer_minds.len()).map(|k| self.w.sense_deer(k)).collect();
            let dorder: Vec<usize> = (0..self.deer_minds.len()).collect();
            crate::fox::sim::run_minds(&mut self.deer_minds, threads, &dorder, |k, m| { m.night(&ds[k]); Action::Sleep });
            let fs: Vec<_> = (0..self.fox_minds.len()).map(|k| self.w.sense_fox(k)).collect();
            let forder: Vec<usize> = (0..self.fox_minds.len()).collect();
            crate::fox::sim::run_minds(&mut self.fox_minds, threads, &forder, |k, m| { m.night(&fs[k]); Action::Sleep });
            if let Some(r) = self.record { let (w, v) = crate::fox::viewer::snapshot(&self.folk[r].m.store); self.snapshots.push((self.w.tick, w, v)); }
        }
        let w = &self.w;
        // Stage 5: in the village the few animals are stepped on this thread (a thread per animal
        // pass cost more than their thinking); their results don't depend on it.
        let athreads = if self.fox_minds.is_empty() { threads } else { 1 };
        let t0 = std::time::Instant::now();
        let deeds = run(&mut self.folk, threads, order, |_, p| if p.village.is_some() { crate::village::life::think(p, w) } else { p.think(w) });
        // Boars read the same start-of-tick world (in reverse order, so order is exercised too).
        let border: Vec<usize> = (0..self.beasts.len()).rev().collect();
        let boar_acts = crate::fox::sim::run_minds(&mut self.beasts, athreads, &border, |b, m| { let s = w.sense_boar(b); m.live(&s) });
        // Deer read the same start-of-tick world (people hidden by cover are not seen).
        let dorder: Vec<usize> = (0..self.deer_minds.len()).rev().collect();
        let deer_acts = crate::fox::sim::run_minds(&mut self.deer_minds, athreads, &dorder, |k, m| { let s = w.sense_deer(k); m.live(&s) });
        let forder: Vec<usize> = (0..self.fox_minds.len()).rev().collect();
        let fox_acts = crate::fox::sim::run_minds(&mut self.fox_minds, athreads, &forder, |k, m| { let s = w.sense_fox(k); m.live(&s) });
        self.brain_ns += t0.elapsed().as_nanos() as u64;
        if let Some(r) = self.record {
            let m = &self.folk[r].m;
            let rep = &m.last;
            let me = &self.w.people[r];
            self.frames.push(Frame {
                tick: self.w.tick, me: (me.x, me.y),
                others: (0..self.w.people.len()).filter(|&j| j != r).map(|j| (self.w.people[j].x, self.w.people[j].y, super::world::name(j as usize).to_string())).collect(),
                things: self.w.things.iter().filter(|t| t.alive && t.kind != Kind::Hut).map(|t| (t.x, t.y, t.name.clone()))
                    .chain(self.w.notes.iter().filter(|n| n.holder.is_none()).map(|n| (n.x, n.y, "a note".to_string())))
                    .chain(self.w.deer.iter().filter(|d| d.caught.is_none()).map(|d| (d.x, d.y, "DEER".to_string()))).collect(),
                mods: m.mods, thirst: me.thirst, act: m.st.act.clone(), hops: rep.hops.clone(), ws: rep.ws.clone(), action: deeds[r].action.unwrap_or(Action::Stay), why: rep.why.clone(),
                events: vec![], fast: m.store.fast_entries().into_iter().filter(|&(_, f)| f >= 20).collect(),
                percepts: rep.percepts.iter().map(|p| (p.node, p.error)).collect(), slept, backlog: rep.backlog,
                boar: self.w.boars.first().map(|q| (q.x, q.y)), focus: m.mods.focus, pain: me.pain,
            });
        }
        self.w.post_step_full(&deeds, &boar_acts, &deer_acts, &fox_acts);
        if self.record.is_some() { if let Some(f) = self.frames.last_mut() { f.events = self.w.log[log0..].iter().map(|l| l.1.clone()).collect(); } }
    }
    pub fn run(&mut self, ticks: u32) { let order: Vec<usize> = (0..self.folk.len()).collect(); for _ in 0..ticks { self.step(1, &order); } }
    pub fn run_until(&mut self, tick: u32) { let order: Vec<usize> = (0..self.folk.len()).collect(); while self.w.tick < tick { self.step(1, &order); } }

    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new(self.w.seed);
        self.w.hash_into(&mut h);
        for p in &self.folk { p.state_hash().hash(&mut h); }
        for m in &self.beasts { m.state_hash().hash(&mut h); }
        for m in &self.deer_minds { m.state_hash().hash(&mut h); }
        for m in &self.fox_minds { m.state_hash().hash(&mut h); }
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

// ------------------------------------------------------------------------- stage 5: a pool

/// Stage 5: brains as their own pass (7.10) on persistent threads. A thread spawned for every
/// tick cost about 100 µs for four, and, worse, freed each person's last-tick allocations on
/// whichever thread's allocator arena it happened to get, so four threads ran *slower* than one.
/// Here each worker lives for the whole run and always gets the same people (by their place in
/// the stepping order), so what a person allocated is freed where it was allocated.
pub struct Pool { tx: Vec<std::sync::mpsc::Sender<Job>>, rx: std::sync::mpsc::Receiver<(usize, Option<Vec<(usize, Person, Deed)>>)>, handles: Vec<std::thread::JoinHandle<()>> }
enum Job { Think(std::sync::Arc<World>, Vec<(usize, Person)>), Night(std::sync::Arc<Vec<crate::mind::Sense>>, Vec<(usize, Person)>) }

impl Pool {
    pub fn new(threads: usize) -> Pool {
        let (dtx, rx) = std::sync::mpsc::channel();
        let mut tx = vec![];
        let mut handles = vec![];
        for k in 0..threads.max(1) {
            let (jtx, jrx) = std::sync::mpsc::channel::<Job>();
            let dtx = dtx.clone();
            handles.push(std::thread::spawn(move || {
                while let Ok(job) = jrx.recv() {
                    // (Review: a panicking worker must fail the run, not leave it waiting.)
                    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match job {
                        Job::Think(w, ps) => ps.into_iter().map(|(i, mut p)| { let d = if p.village.is_some() { crate::village::life::think(&mut p, &w) } else { p.think(&w) }; (i, p, d) }).collect(),
                        Job::Night(s, ps) => ps.into_iter().map(|(i, mut p)| { p.night(&s[i]); (i, p, Deed::default()) }).collect::<Vec<_>>(),
                    }));
                    let failed = out.is_err();
                    if dtx.send((k, out.ok())).is_err() || failed { break; }
                }
            }));
            tx.push(jtx);
        }
        Pool { tx, rx, handles }
    }
    pub fn threads(&self) -> usize { self.tx.len() }
    /// Hand each worker its share of `folk` (by `order`), run `job` on them, and put them back.
    fn run(&self, folk: &mut Vec<Person>, order: &[usize], job: impl Fn(Vec<(usize, Person)>) -> Job) -> Vec<Deed> {
        let n = folk.len();
        let mut slots: Vec<Option<Person>> = std::mem::take(folk).into_iter().map(Some).collect();
        let chunk = n.div_ceil(self.tx.len()).max(1);
        let mut sent = 0;
        for (k, part) in order.chunks(chunk).enumerate() {
            let ps: Vec<(usize, Person)> = part.iter().map(|&i| (i, slots[i].take().expect("order is a permutation"))).collect();
            self.tx[k].send(job(ps)).expect("worker alive");
            sent += 1;
        }
        let mut deeds = vec![Deed::default(); n];
        for _ in 0..sent { let (k, out) = self.rx.recv().expect("worker alive"); let out = out.unwrap_or_else(|| panic!("pool worker {k} panicked")); for (i, p, d) in out { slots[i] = Some(p); deeds[i] = d; } }
        *folk = slots.into_iter().map(|p| p.expect("every person back")).collect();
        deeds
    }
}
impl Drop for Pool {
    fn drop(&mut self) { self.tx.clear(); for h in self.handles.drain(..) { let _ = h.join(); } }
}

impl Valley {
    /// `step` with the people's pass on a `Pool` (the animals, few, on this thread). Gives the same
    /// world and minds as `step` on any number of threads in any order. (No viewer recording.)
    pub fn step_pooled(&mut self, pool: &Pool, order: &[usize]) {
        assert!(self.record.is_none(), "recording uses `step`");
        self.w.pre_step();
        let one: Vec<usize> = vec![];
        let _ = one;
        if self.w.night_now {
            let senses = std::sync::Arc::new((0..self.folk.len()).map(|i| self.w.sense(i, true)).collect::<Vec<_>>());
            pool.run(&mut self.folk, order, |ps| Job::Night(senses.clone(), ps));
            let bs: Vec<_> = (0..self.beasts.len()).map(|b| self.w.sense_boar(b)).collect();
            for (b, m) in self.beasts.iter_mut().enumerate() { m.night(&bs[b]); }
            let ds: Vec<_> = (0..self.deer_minds.len()).map(|k| self.w.sense_deer(k)).collect();
            for (k, m) in self.deer_minds.iter_mut().enumerate() { m.night(&ds[k]); }
            let fs: Vec<_> = (0..self.fox_minds.len()).map(|k| self.w.sense_fox(k)).collect();
            for (k, m) in self.fox_minds.iter_mut().enumerate() { m.night(&fs[k]); }
        }
        let t0 = std::time::Instant::now();
        let w = std::sync::Arc::new(std::mem::replace(&mut self.w, World::valley(0)));
        let deeds = pool.run(&mut self.folk, order, |ps| Job::Think(w.clone(), ps));
        let w = std::sync::Arc::try_unwrap(w).unwrap_or_else(|_| panic!("the world is shared after the pass"));
        let step = |m: &mut crate::mind::Mind<crate::store::Runtime>, s: crate::mind::Sense| m.live(&s);
        let boar_acts: Vec<Action> = (0..self.beasts.len()).rev().map(|b| (b, step(&mut self.beasts[b], w.sense_boar(b)))).collect::<Vec<_>>().into_iter().rev().map(|x| x.1).collect();
        let deer_acts: Vec<Action> = (0..self.deer_minds.len()).rev().map(|k| (k, step(&mut self.deer_minds[k], w.sense_deer(k)))).collect::<Vec<_>>().into_iter().rev().map(|x| x.1).collect();
        let fox_acts: Vec<Action> = (0..self.fox_minds.len()).rev().map(|k| (k, step(&mut self.fox_minds[k], w.sense_fox(k)))).collect::<Vec<_>>().into_iter().rev().map(|x| x.1).collect();
        self.w = w;
        self.brain_ns += t0.elapsed().as_nanos() as u64;
        self.w.post_step_full(&deeds, &boar_acts, &deer_acts, &fox_acts);
    }
}
