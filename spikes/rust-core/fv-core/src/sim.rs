//! One tick of the world, in order, with the books kept.

use crate::chem;
use crate::flow::Flow;
use crate::ledger::{Books, Totals};
use crate::rad::{self, Light, Sky};
use crate::transport::{advect, diffuse};
use crate::units::*;
use crate::world::World;

/// Something a player (or a test) does to the world. Commands are the only
/// way anything comes in from outside besides what crosses the box's edge,
/// and the books count them.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    /// Hold a flame to a cell: put `energy` into it this tick (a torch gives
    /// ~1–5 kW, so 50–250 J a tick). The flame touches the solid if there is
    /// one, else it heats the air.
    Heat { x: u16, y: u16, z: u16, energy: Energy },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Scheduled { pub tick: u64, pub peer: u16, pub seq: u32, pub command: Command }

pub struct Sim {
    pub world: World,
    pub flow: Flow,
    pub books: Books,
    pub light: Light,
    pub sky: Sky,
    pub queue: Vec<Scheduled>,
    /// Air motion sub-steps per tick (each can carry one cell's worth).
    pub advect_steps: i64,
    /// Replaces the diffusion phase (used to split it between peers). It
    /// must leave the world exactly as `transport::diffuse` would.
    pub diffuse_hook: Option<Box<dyn FnMut(&mut World, &Flow) + Send>>,
}

impl Sim {
    pub fn new(world: World, sky: Sky) -> Sim {
        let flow = Flow::new(&world);
        let books = Books::open(&world);
        let light = Light::new(world.cells.len());
        Sim { world, flow, books, light, sky, queue: Vec::new(), advect_steps: 2, diffuse_hook: None }
    }

    pub fn schedule(&mut self, s: Scheduled) {
        assert!(s.tick >= self.world.tick, "command for a tick already simulated");
        self.queue.push(s);
        self.queue.sort_by_key(|s| (s.tick, s.peer, s.seq));
    }

    pub fn step(&mut self) {
        let w = &mut self.world;
        for t in w.tallies.iter_mut() { *t = Default::default() }
        // Commands for this tick, in a fixed order.
        let now = w.tick;
        let (due, later): (Vec<_>, Vec<_>) = self.queue.drain(..).partition(|s| s.tick == now);
        self.queue = later;
        for s in due { apply(w, &mut self.books, s.command) }
        self.flow.step(w);
        advect(w, &self.flow, self.advect_steps);
        match self.diffuse_hook.as_mut() {
            Some(hook) => hook(w, &self.flow),
            None => diffuse(w, &self.flow),
        }
        chem::react(w);
        rad::radiate(w, &self.sky, &mut self.light);
        for t in &w.tallies { self.books.record(t) }
        w.tick += 1;
    }

    pub fn totals(&self) -> Totals { Totals::of(&self.world) }
    pub fn energy_gap(&self) -> i128 { self.books.energy_gap(&self.totals()) }
    pub fn mass_gap(&self) -> i128 { self.books.mass_gap(&self.totals()) }
    pub fn hash(&self) -> u64 {
        let mut h = crate::rng::Hasher::new(self.world.hash());
        for v in &self.flow.vel { for a in v { h.u64(*a as u64) } }
        h.finish()
    }
}

fn apply(w: &mut World, books: &mut Books, c: Command) {
    match c {
        Command::Heat { x, y, z, energy } => {
            let (x, y, z) = (x as usize, y as usize, z as usize);
            if x < w.nx && y < w.ny && z < w.nz {
                let subs = w.subs.clone();
                let c = w.get_mut(x, y, z);
                if c.has_solid() { c.solid_energy += energy } else { c.energy += energy }
                let _ = subs;
                books.commands += energy.0 as i128;
            }
        }
    }
}
