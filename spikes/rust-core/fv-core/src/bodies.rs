//! Falling pieces: groups of cells that came loose move as rigid bodies
//! (Rapier, built for cross-platform determinism), then settle back into
//! the grid where they come to rest.
//!
//! The books stay exact through all of it:
//! - while a piece falls, gravity does work on it (`Books::gravity_work`);
//!   what doesn't stay as motion (friction, impact, the solver's damping)
//!   becomes heat in the piece's own cells, so heat + motion = work done;
//! - when it comes to rest, its remaining motion becomes heat too;
//! - the space a piece leaves fills with air from the surroundings (counted
//!   as crossing the edge), and the air a landing piece displaces goes into
//!   its own pores, from where pressure pushes it out.
//!
//! A piece that tips off its support pivots on its real contact edge, so a
//! post burnt through at its foot leans and falls, rather than snapping
//! sideways.

use crate::cell::{Cell, BOND_X, BOND_Y, BOND_Z};
use crate::ledger::Books;
use crate::scene::natural_bonds;
use crate::substance::NONE;
use crate::units::*;
use crate::world::{Tally, World};
use rapier3d::prelude::*;

/// Gravity, m/s².
const G: f32 = 9.806_65;
/// Physics sub-steps per tick (Rapier is steadier at 12.5 ms).
const SUBSTEPS: usize = 4;
/// Ticks a piece must be nearly still before it settles into the grid.
const SETTLE_TICKS: u32 = 10;
const CELL_M: f32 = 0.1;

pub struct Piece {
    pub handle: RigidBodyHandle,
    /// Cells with their offset (in cells) from the piece's origin.
    pub cells: Vec<([i32; 3], Cell)>,
    /// Mass, kg.
    pub mass: f32,
    /// Height of the centre of mass when it came loose, m.
    pub start_z: f32,
    /// Work gravity has done on it so far, µJ.
    pub gravity_work: i64,
    /// Kinetic energy at the end of the last tick, µJ.
    pub kinetic: i64,
    pub still: u32,
}

pub struct Bodies {
    pub physics: PhysicsWorld,
    pub pieces: Vec<Piece>,
    /// The ground the pieces collide with: solid cells near them.
    ground: Option<ColliderHandle>,
}

impl Default for Bodies { fn default() -> Self { Self::new() } }

impl Bodies {
    pub fn new() -> Bodies {
        let mut physics = PhysicsWorld::new();
        physics.gravity = Vector::new(0.0, 0.0, -G);
        physics.integration_parameters.dt = 0.05 / SUBSTEPS as f32;
        Bodies { physics, pieces: Vec::new(), ground: None }
    }

    pub fn in_flight(&self) -> bool { !self.pieces.is_empty() }

    /// Kinetic energy of everything moving, µJ.
    pub fn kinetic(&self) -> i64 { self.pieces.iter().map(|p| p.kinetic).sum() }

    /// Lift a loose group of cells out of the grid and let it move.
    pub fn detach(&mut self, w: &mut World, cells: &[[usize; 3]], tally: &mut Tally) {
        let (min_x, min_y, min_z) = (cells.iter().map(|p| p[0]).min().unwrap(), cells.iter().map(|p| p[1]).min().unwrap(), cells.iter().map(|p| p[2]).min().unwrap());
        let mut parts = Vec::with_capacity(cells.len());
        let mut by_sub: std::collections::BTreeMap<u8, (Vec<IVector>, f32)> = Default::default();
        let mut mass = 0f32;
        for p in cells {
            let c = *w.get(p[0], p[1], p[2]);
            let off = [(p[0] - min_x) as i32, (p[1] - min_y) as i32, (p[2] - min_z) as i32];
            let m = (c.solid_mass().0 + c.water.0) as f32 * 1e-9;
            mass += m;
            let e = by_sub.entry(c.substance).or_insert((Vec::new(), 0.0));
            e.0.push(IVector::new(off[0], off[1], off[2]));
            e.1 += m;
            parts.push((off, c));
            // The space it leaves fills with air from the surroundings.
            w.set(p[0], p[1], p[2], w.outside_air);
            tally.energy_in += w.outside_air.thermal().0;
            for k in 0..tally.gas_in.len() { tally.gas_in[k] += w.outside_air.gas[k].0 }
        }
        let origin = Vector::new(min_x as f32 * CELL_M, min_y as f32 * CELL_M, min_z as f32 * CELL_M);
        let body = RigidBodyBuilder::dynamic().translation(origin).ccd_enabled(true).build();
        let handle = self.physics.bodies.insert(body);
        for (_, (voxels, m)) in by_sub {
            let density = m / (voxels.len() as f32 * CELL_M * CELL_M * CELL_M);
            let col = ColliderBuilder::voxels(Vector::splat(CELL_M), &voxels).density(density.max(1.0)).friction(0.7).restitution(0.1).build();
            self.physics.colliders.insert_with_parent(col, handle, &mut self.physics.bodies);
        }
        let start_z = self.physics.bodies[handle].center_of_mass().z;
        self.pieces.push(Piece { handle, cells: parts, mass, start_z, gravity_work: 0, kinetic: 0, still: 0 });
        self.rebuild_ground(w);
    }

    /// The solid cells pieces can hit: every opaque cell near a piece.
    fn rebuild_ground(&mut self, w: &World) {
        if let Some(h) = self.ground.take() { self.physics.remove_collider(h); }
        let mut voxels = Vec::new();
        for z in 0..w.nz { for y in 0..w.ny { for x in 0..w.nx {
            let c = w.get(x, y, z);
            if c.substance != NONE && c.opaque(&w.subs) { voxels.push(IVector::new(x as i32, y as i32, z as i32)) }
        }}}
        if voxels.is_empty() { return }
        let col = ColliderBuilder::voxels(Vector::splat(CELL_M), &voxels).friction(0.8).build();
        self.ground = Some(self.physics.colliders.insert(col));
    }

    /// One tick of motion. Heat from friction and impacts goes into the
    /// pieces' cells; pieces that have come to rest go back into the grid.
    pub fn step(&mut self, w: &mut World, books: &mut Books) {
        if self.pieces.is_empty() { return }
        for _ in 0..SUBSTEPS { self.physics.step() }
        let mut settled = Vec::new();
        for (i, p) in self.pieces.iter_mut().enumerate() {
            let b = &self.physics.bodies[p.handle];
            let z = b.center_of_mass().z;
            let gw = (p.mass * G * (p.start_z - z) * 1e6).round() as i64;
            let ke = (b.kinetic_energy() * 1e6).round() as i64;
            let (dgw, dke) = (gw - p.gravity_work, ke - p.kinetic);
            books.gravity_work += dgw as i128;
            heat_cells(&mut p.cells, dgw - dke);
            p.gravity_work = gw;
            p.kinetic = ke;
            let moving = b.linvel().length() > 0.05 || b.angvel().length() > 0.1;
            p.still = if moving { 0 } else { p.still + 1 };
            if p.still >= SETTLE_TICKS || b.is_sleeping() { settled.push(i) }
        }
        for i in settled.into_iter().rev() {
            let p = self.pieces.remove(i);
            self.settle(w, books, p);
        }
        // A piece that has gone past the box's edge has left the bounded
        // space: it and everything it holds are booked as leaving.
        let (ex, ey) = (w.nx as f32 * CELL_M, w.ny as f32 * CELL_M);
        let mut i = 0;
        while i < self.pieces.len() {
            let t = self.physics.bodies[self.pieces[i].handle].center_of_mass();
            let outside = t.z < -1.0 || t.x < -1.0 || t.y < -1.0 || t.x > ex + 1.0 || t.y > ey + 1.0;
            if !outside { i += 1; continue }
            let p = self.pieces.remove(i);
            let mut energy = p.kinetic as i128;
            let mut mass = 0i128;
            for (_, c) in &p.cells {
                energy += (c.thermal().0 + c.chemical(&w.subs).0 + c.nuclear.0) as i128;
                mass += (c.solid_mass().0 + c.water.0 + c.gas_mass().0) as i128;
            }
            books.edge_matter_energy += energy;
            books.edge_matter_mass += mass;
            self.physics.bodies.remove(p.handle, &mut self.physics.islands, &mut self.physics.colliders, &mut self.physics.impulse_joints, &mut self.physics.multibody_joints, &mut self.physics.soft_bodies, true);
        }
        if !self.pieces.is_empty() { /* the ground only changes when a piece settles */ }
    }

    /// Put a piece back into the grid where it lies, rounding each cell to
    /// the nearest place (a quarter-turn on its side lands square).
    fn settle(&mut self, w: &mut World, books: &mut Books, mut p: Piece) {
        let b = self.physics.bodies[p.handle].clone();
        let pose = *b.position();
        // Where each cell ends up, and the work gravity did getting it there.
        let mut placed: Vec<([usize; 3], Cell)> = Vec::new();
        let mut taken = std::collections::BTreeSet::new();
        let mut gw = 0f64;
        for (off, c) in &p.cells {
            let local = Vector::new((off[0] as f32 + 0.5) * CELL_M, (off[1] as f32 + 0.5) * CELL_M, (off[2] as f32 + 0.5) * CELL_M);
            let at = pose * local;
            let mut q = [(at.x / CELL_M).floor().max(0.0) as usize, (at.y / CELL_M).floor().max(0.0) as usize, (at.z / CELL_M).floor().max(0.0) as usize];
            q = [q[0].min(w.nx - 1), q[1].min(w.ny - 1), q[2].min(w.nz - 1)];
            // Somewhere free: this place, or the first free one above it.
            while q[2] + 1 < w.nz && (taken.contains(&q) || (w.get(q[0], q[1], q[2]).substance != NONE && w.get(q[0], q[1], q[2]).solid_mass().0 > 0)) { q[2] += 1 }
            taken.insert(q);
            placed.push((q, *c));
        }
        // Work done by gravity from where it started to where its cells lie.
        let mut m_total = 0f64;
        let mut mz = 0f64;
        for (q, c) in &placed {
            let m = (c.solid_mass().0 + c.water.0) as f64 * 1e-9;
            m_total += m;
            mz += m * (q[2] as f64 + 0.5) * CELL_M as f64;
        }
        if m_total > 0.0 { gw = m_total * G as f64 * (p.start_z as f64 - mz / m_total) * 1e6 }
        let gw = gw.round() as i64;
        books.gravity_work += (gw - p.gravity_work) as i128;
        // Its motion and the last of the fall become heat in its cells.
        heat_cells(&mut p.cells, gw - p.gravity_work + p.kinetic);
        p.kinetic = 0;
        // The cells' heat was updated above; place the updated cells.
        for (k, (q, _)) in placed.iter().enumerate() {
            let mut c = p.cells[k].1;
            // The air that was there goes into the piece's pores.
            let there = *w.get(q[0], q[1], q[2]);
            for g in 0..c.gas.len() { c.gas[g] += there.gas[g] }
            c.energy += there.energy;
            c.solid_energy += there.solid_energy;
            c.bonds = 0;
            w.set(q[0], q[1], q[2], c);
        }
        // Cells of the piece that lie next to each other stay joined.
        let spots: std::collections::BTreeSet<[usize; 3]> = placed.iter().map(|(q, _)| *q).collect();
        for q in &spots {
            let a = w.get(q[0], q[1], q[2]).substance;
            let mut bits = 0;
            for (axis, bit) in [(0usize, BOND_X), (1, BOND_Y), (2, BOND_Z)] {
                let mut r = *q;
                r[axis] += 1;
                if spots.contains(&r) && natural_bonds(a, w.get(r[0], r[1], r[2]).substance) { bits |= bit }
            }
            w.get_mut(q[0], q[1], q[2]).bonds = bits;
        }
        self.physics.bodies.remove(p.handle, &mut self.physics.islands, &mut self.physics.colliders, &mut self.physics.impulse_joints, &mut self.physics.multibody_joints, &mut self.physics.soft_bodies, true);
        self.rebuild_ground(w);
    }

    /// Every cell in flight, for the books and for drawing: (world position
    /// of the cell's centre in metres, rotation as a quaternion [x, y, z, w], cell).
    pub fn cells(&self) -> impl Iterator<Item = ([f32; 3], [f32; 4], &Cell)> + '_ {
        self.pieces.iter().flat_map(move |p| {
            let pose = *self.physics.bodies[p.handle].position();
            p.cells.iter().map(move |(off, c)| {
                let local = Vector::new((off[0] as f32 + 0.5) * CELL_M, (off[1] as f32 + 0.5) * CELL_M, (off[2] as f32 + 0.5) * CELL_M);
                let at = pose * local;
                let r = pose.rotation;
                ([at.x, at.y, at.z], [r.x, r.y, r.z, r.w], c)
            })
        })
    }
}

/// Add (or, rarely, take) heat across a piece's cells in proportion to how
/// many there are, exactly: the last cell takes the remainder.
fn heat_cells(cells: &mut [([i32; 3], Cell)], energy: i64) {
    if cells.is_empty() || energy == 0 { return }
    let n = cells.len() as i64;
    let each = energy / n;
    for (_, c) in cells.iter_mut() { c.solid_energy += Energy(each) }
    cells.last_mut().unwrap().1.solid_energy += Energy(energy - each * n);
}
