//! Checkpoints: the whole simulation state as bytes, so a peer joining late
//! can be handed the world and carry on in lockstep, and its books still
//! balance. Everything that affects a future tick is in here; nothing that
//! only affects drawing is.

use crate::cell::Cell;
use crate::flow::Flow;
use crate::ledger::{Books, Totals};
use crate::rad::{Light, Sky};
use crate::sim::{Command, Scheduled, Sim};
use crate::substance::GASES;
use crate::units::*;
use crate::world::World;

const MAGIC: &[u8; 4] = b"FVR4";

struct W(Vec<u8>);
impl W {
    fn u64(&mut self, v: u64) { self.0.extend_from_slice(&v.to_le_bytes()) }
    fn i64(&mut self, v: i64) { self.0.extend_from_slice(&v.to_le_bytes()) }
    fn i128(&mut self, v: i128) { self.0.extend_from_slice(&v.to_le_bytes()) }
    fn u8(&mut self, v: u8) { self.0.push(v) }
}
struct R<'a>(&'a [u8], usize);
impl R<'_> {
    fn take<const N: usize>(&mut self) -> [u8; N] { let mut a = [0; N]; a.copy_from_slice(&self.0[self.1..self.1 + N]); self.1 += N; a }
    fn u64(&mut self) -> u64 { u64::from_le_bytes(self.take()) }
    fn i64(&mut self) -> i64 { i64::from_le_bytes(self.take()) }
    fn i128(&mut self) -> i128 { i128::from_le_bytes(self.take()) }
    fn u8(&mut self) -> u8 { self.take::<1>()[0] }
}

fn totals(w: &mut W, t: &Totals) {
    for v in [t.thermal, t.chemical, t.nuclear, t.bodies, t.solid_mass, t.water_mass] { w.i128(v) }
    for v in t.gas_mass { w.i128(v) }
}
fn read_totals(r: &mut R) -> Totals {
    let mut t = Totals { thermal: r.i128(), chemical: r.i128(), nuclear: r.i128(), bodies: r.i128(), solid_mass: r.i128(), water_mass: r.i128(), ..Totals::default() };
    for k in 0..GASES { t.gas_mass[k] = r.i128() }
    t
}

pub fn export(sim: &Sim) -> Vec<u8> {
    assert!(!sim.bodies.in_flight(), "checkpoint between falls (pieces in flight aren't saved yet)");
    let mut w = W(Vec::new());
    w.0.extend_from_slice(MAGIC);
    let wd = &sim.world;
    for v in [wd.nx, wd.ny, wd.nz] { w.u64(v as u64) }
    w.u64(wd.seed);
    w.u64(wd.tick);
    w.i64(wd.ambient.0);
    w.i64(sim.sky.sun_w_m2);
    for v in sim.sky.sun_dir { w.i64(v) }
    w.i64(sim.sky.sky_w_m2);
    w.i64(sim.sky.moon_uw_m2);
    for v in sim.sky.moon_dir { w.i64(v) }
    w.u8(sim.sky.clear as u8);
    w.i64(sim.advect_steps);
    for v in sim.flow.wind_mean { w.i64(v) }
    for v in sim.flow.wind { w.i64(v) }
    for v in &sim.flow.vel { for a in v { w.i64(*a as i64) } }
    let b = &sim.books;
    totals(&mut w, &b.start);
    w.i128(b.edge_energy);
    for v in b.edge_gas { w.i128(v) }
    for v in [b.radiation_in, b.radiation_out, b.commands, b.commands_mass, b.gravity_work, b.edge_matter_mass, b.edge_matter_energy] { w.i128(v) }
    w.u64(sim.queue.len() as u64);
    for s in &sim.queue {
        w.u64(s.tick);
        w.u64(s.peer as u64);
        w.u64(s.seq as u64);
        match s.command {
            Command::Heat { x, y, z, energy } => { w.u8(0); for v in [x, y, z] { w.u64(v as u64) } w.i64(energy.0) }
            Command::Gas { x, y, z, mass } => { w.u8(1); for v in [x, y, z] { w.u64(v as u64) } w.i64(mass.0) }
        }
    }
    for c in &wd.cells {
        w.u8(c.substance);
        w.u8(c.bonds);
        w.u8(c.lit as u8);
        for b in c.front.to_le_bytes() { w.u8(b) }
        for m in c.solid { w.i64(m.0) }
        w.i64(c.water.0);
        for m in c.gas { w.i64(m.0) }
        w.i64(c.energy.0);
        w.i64(c.solid_energy.0);
        w.i64(c.skin.0);
        w.i64(c.nuclear.0);
    }
    w.0
}

pub fn import(bytes: &[u8]) -> Sim {
    assert_eq!(&bytes[..4], MAGIC, "not a checkpoint");
    let mut r = R(bytes, 4);
    let (nx, ny, nz) = (r.u64() as usize, r.u64() as usize, r.u64() as usize);
    let seed = r.u64();
    let tick = r.u64();
    let ambient = Temperature(r.i64());
    let mut world = World::new(nx, ny, nz, seed, ambient);
    world.tick = tick;
    let sky = Sky { sun_w_m2: r.i64(), sun_dir: [r.i64(), r.i64(), r.i64()], sky_w_m2: r.i64(), moon_uw_m2: r.i64(), moon_dir: [r.i64(), r.i64(), r.i64()], clear: r.u8() != 0 };
    let advect_steps = r.i64();
    let mut flow = Flow::new(&world);
    flow.wind_mean = [r.i64(), r.i64()];
    flow.wind = [r.i64(), r.i64()];
    for v in flow.vel.iter_mut() { for a in v.iter_mut() { *a = r.i64() as i32 } }
    let mut books = Books { start: read_totals(&mut r), edge_energy: r.i128(), ..Books::default() };
    for k in 0..GASES { books.edge_gas[k] = r.i128() }
    books.radiation_in = r.i128();
    books.radiation_out = r.i128();
    books.commands = r.i128();
    books.commands_mass = r.i128();
    books.gravity_work = r.i128();
    books.edge_matter_mass = r.i128();
    books.edge_matter_energy = r.i128();
    let mut queue = Vec::new();
    for _ in 0..r.u64() {
        let (t, peer, seq) = (r.u64(), r.u64() as u16, r.u64() as u32);
        let kind = r.u8();
        assert!(kind <= 1, "unknown command");
        let (x, y, z) = (r.u64() as u16, r.u64() as u16, r.u64() as u16);
        let command = if kind == 0 { Command::Heat { x, y, z, energy: Energy(r.i64()) } } else { Command::Gas { x, y, z, mass: Mass(r.i64()) } };
        queue.push(Scheduled { tick: t, peer, seq, command });
    }
    for c in world.cells.iter_mut() {
        let mut n = Cell { substance: r.u8(), bonds: r.u8(), lit: r.u8() != 0, front: u16::from_le_bytes([r.u8(), r.u8()]), ..Cell::default() };
        for m in n.solid.iter_mut() { *m = Mass(r.i64()) }
        n.water = Mass(r.i64());
        for m in n.gas.iter_mut() { *m = Mass(r.i64()) }
        n.energy = Energy(r.i64());
        n.solid_energy = Energy(r.i64());
        n.skin = Energy(r.i64());
        n.nuclear = Energy(r.i64());
        *c = n;
    }
    let light = Light::new(world.cells.len());
    let mut sim = Sim::new(world, sky);
    sim.flow = flow;
    sim.books = books;
    sim.light = light;
    sim.queue = queue;
    sim.advect_steps = advect_steps;
    sim
}
