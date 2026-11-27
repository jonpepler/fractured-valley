//! One tick of the world, in order, with the books kept.

use crate::bodies::Bodies;
use crate::chem;
use crate::structure;
use crate::flow::Flow;
use crate::ledger::{Books, Totals};
use crate::rad::{self, Light, Radiation, Sky};
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
    /// Let burnable gas into a cell, as a gas burner does: `mass` of fuel
    /// gas at the cell's gas temperature (its chemical energy and heat come
    /// in with it). Only into a cell with room for gas.
    Gas { x: u16, y: u16, z: u16, mass: Mass },
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
    /// Pieces that came loose and are falling.
    pub bodies: Bodies,
    /// Look at what holds things up every this many ticks.
    pub structure_every: u64,
    /// How radiation travels (packets or a field).
    pub radiation: Radiation,
    /// When set, the heat each cell's flame released last tick, µJ (for
    /// probes and tests that measure where flames are; not part of the state).
    pub flames: Option<Vec<i64>>,
}

impl Sim {
    pub fn new(world: World, sky: Sky) -> Sim {
        let flow = Flow::new(&world);
        let books = Books::open(&world);
        let light = Light::new(world.cells.len());
        Sim { world, flow, books, light, sky, queue: Vec::new(), advect_steps: crate::flow::ADVECT_STEPS, diffuse_hook: None, bodies: Bodies::new(), structure_every: 10, radiation: Radiation::default(), flames: None }
    }

    pub fn schedule(&mut self, s: Scheduled) {
        assert!(s.tick >= self.world.tick, "command for a tick already simulated");
        self.queue.push(s);
        self.queue.sort_by_key(|s| (s.tick, s.peer, s.seq));
    }

    pub fn step(&mut self) {
        self.step_before_diffuse();
        let w = &mut self.world;
        match self.diffuse_hook.as_mut() {
            Some(hook) => hook(w, &self.flow),
            None => diffuse(w, &self.flow),
        }
        self.step_after_diffuse();
    }

    /// The first part of `step`, up to the diffusion phase (this tick's
    /// commands). With
    /// `step_after_diffuse` this lets a caller that cannot block (a web page
    /// waiting for its peers' bricks) drive the diffusion phase itself, e.g.
    /// with `transport::diffuse_part` and `apply_parts`. `step` is exactly
    /// before + diffuse + after.
    pub fn step_before_diffuse(&mut self) {
        let w = &mut self.world;
        for t in w.tallies.iter_mut() { *t = Default::default() }
        // Commands for this tick, in a fixed order.
        let now = w.tick;
        let (due, later): (Vec<_>, Vec<_>) = self.queue.drain(..).partition(|s| s.tick == now);
        self.queue = later;
        for s in due { apply(w, &mut self.books, s.command) }
    }

    /// The rest of `step`, after the diffusion phase.
    pub fn step_after_diffuse(&mut self) {
        let w = &mut self.world;
        // Heat crosses faces (diffusion) before the air moves: a flame's
        // heat, released into its gas at the end of the last tick, reaches
        // the fuel beside it before the plume carries that gas away.
        self.flow.step(w);
        advect(w, &self.flow, self.advect_steps);
        chem::react_in(w, &self.flow);
        if let Some(f) = self.flames.as_mut() {
            f.resize(w.cells.len(), 0);
            for (o, c) in f.iter_mut().zip(&w.cells) { *o = c.flame.0 }
        }
        rad::radiate_by(self.radiation, w, &self.sky, &mut self.light);
        // What holds things up; whatever came loose starts to fall.
        if w.tick % self.structure_every == 0 {
            let report = structure::settle(w);
            let mut tally = Default::default();
            for piece in &report.loose { self.bodies.detach(w, piece, &mut tally) }
            self.books.record(&tally);
        }
        self.bodies.step(w, &mut self.books);
        for t in &w.tallies { self.books.record(t) }
        w.tick += 1;
    }

    /// Every store, in the grid and riding falling pieces.
    pub fn totals(&self) -> Totals {
        let mut t = Totals::of(&self.world);
        for (_, _, c) in self.bodies.cells() { t.add(c, &self.world.subs) }
        t.bodies += self.bodies.kinetic() as i128;
        t
    }
    pub fn energy_gap(&self) -> i128 { self.books.energy_gap(&self.totals()) }
    pub fn mass_gap(&self) -> i128 { self.books.mass_gap(&self.totals()) }
    pub fn hash(&self) -> u64 {
        let mut h = crate::rng::Hasher::new(self.world.hash());
        for v in &self.flow.vel { for a in v { h.u64(*a as u64) } }
        for (pos, rot, c) in self.bodies.cells() {
            for v in pos.iter().chain(rot.iter()) { h.u64(v.to_bits() as u64) }
            h.cell(c);
        }
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
                *c.surface_store(&subs) += energy;
                let _ = subs;
                books.commands += energy.0 as i128;
            }
        }
        Command::Gas { x, y, z, mass } => {
            let (x, y, z) = (x as usize, y as usize, z as usize);
            if x < w.nx && y < w.ny && z < w.nz && mass.0 > 0 {
                let subs = w.subs.clone();
                let c = w.get_mut(x, y, z);
                if c.gas_volume_ml(&subs) > 0 {
                    let fuel = crate::substance::Gas::Fuel as usize;
                    use crate::substance::{GAS_CHEMICAL, GAS_SPECIFIC_HEAT};
                    let heat = (mass * GAS_SPECIFIC_HEAT[fuel]) * c.gas_temperature();
                    c.gas[fuel] += mass;
                    c.energy += heat;
                    books.commands += (heat + mass * GAS_CHEMICAL[fuel]).0 as i128;
                    books.commands_mass += mass.0 as i128;
                }
            }
        }
    }
}
