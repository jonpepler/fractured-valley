//! The world: a bounded box of cells, stored brick by brick.
//!
//! Cells are grouped into bricks of 8×8×8 so a brick's cells sit together in
//! memory, which lets each thread (or peer) own whole bricks. Every phase of a
//! tick reads only `cells` (the previous state) and writes only its own
//! bricks of `next`, then the two swap (decision 2.7b), so any split of the
//! bricks between threads or peers gives the same bits.
//!
//! The box is a *bounded space*: outside its sides and top is still air at
//! the ambient temperature, and anything that crosses the edge is counted in
//! the [`Books`](crate::ledger::Books). Below its floor is the fixed bottom of
//! the world (nothing crosses it).

use crate::cell::Cell;
use crate::substance::{Substance, table};
use crate::units::*;

pub const B: usize = 8;
pub const BRICK_CELLS: usize = B * B * B;
/// One tick.
pub const TICK: Duration = milliseconds(50);

/// What crossed the box's edge during a tick, gathered per brick so each
/// thread writes only its own.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// Energy that came in (negative: went out), including latent heat carried by vapour.
    pub energy_in: i64,
    /// Mass of each gas that came in (negative: went out).
    pub gas_in: [i64; crate::substance::GASES],
    /// Energy that arrived as radiation from outside (sunlight, sky).
    pub radiation_in: i64,
    /// Energy that left as radiation (escaped to the sky).
    pub radiation_out: i64,
}

impl Tally {
    pub fn add(&mut self, o: &Tally) {
        self.energy_in += o.energy_in;
        for k in 0..self.gas_in.len() { self.gas_in[k] += o.gas_in[k] }
        self.radiation_in += o.radiation_in;
        self.radiation_out += o.radiation_out;
    }
}

#[derive(Clone)]
pub struct World {
    pub nx: usize,
    pub ny: usize,
    pub nz: usize,
    pub bx: usize,
    pub by: usize,
    pub bz: usize,
    pub subs: Vec<Substance>,
    /// The state at the start of the current phase.
    pub cells: Vec<Cell>,
    /// Scratch for the phase being computed.
    pub next: Vec<Cell>,
    /// Per-brick edge crossings for the current tick.
    pub tallies: Vec<Tally>,
    pub seed: u64,
    pub tick: u64,
    /// The temperature of the surroundings beyond the box.
    pub ambient: Temperature,
    /// The air outside the box, at `ambient`.
    pub outside_air: Cell,
}

impl World {
    /// The first cell holding an impossible amount (a negative mass or a
    /// negative energy store), if any. The books can balance with energy in
    /// the wrong place; this catches the worst of that.
    pub fn first_impossible(&self) -> Option<(usize, usize, usize)> {
        let i = self.cells.iter().position(|c| {
            c.gas.iter().any(|m| m.0 < 0) || c.solid.iter().any(|m| m.0 < 0) || c.water.0 < 0
                || c.energy.0 < 0 || c.solid_energy.0 < 0 || c.skin.0 < 0 || c.nuclear.0 < 0
        })?;
        let b = i / crate::world::BRICK_CELLS;
        let (ox, oy, oz) = self.brick_origin(b);
        let (lx, ly, lz) = World::local_xyz(i % crate::world::BRICK_CELLS);
        Some((ox + lx, oy + ly, oz + lz))
    }

    /// An empty box of fresh air. Sizes are rounded up to whole bricks.
    pub fn new(nx: usize, ny: usize, nz: usize, seed: u64, ambient: Temperature) -> World {
        let (bx, by, bz) = (nx.div_ceil(B), ny.div_ceil(B), nz.div_ceil(B));
        let subs = table();
        let outside_air = Cell::air(ambient, &subs);
        let n = bx * by * bz * BRICK_CELLS;
        World {
            nx: bx * B, ny: by * B, nz: bz * B, bx, by, bz, subs,
            cells: vec![outside_air; n], next: vec![outside_air; n], tallies: vec![Tally::default(); bx * by * bz],
            seed, tick: 0, ambient, outside_air,
        }
    }

    pub fn bricks(&self) -> usize { self.bx * self.by * self.bz }

    #[inline]
    pub fn idx(&self, x: usize, y: usize, z: usize) -> usize {
        let b = ((z / B) * self.by + y / B) * self.bx + x / B;
        b * BRICK_CELLS + ((z % B) * B + y % B) * B + x % B
    }

    /// The coordinates of a brick's first cell.
    #[inline]
    pub fn brick_origin(&self, b: usize) -> (usize, usize, usize) {
        (b % self.bx * B, b / self.bx % self.by * B, b / (self.bx * self.by) * B)
    }

    #[inline]
    pub fn local_xyz(l: usize) -> (usize, usize, usize) { (l % B, l / B % B, l / (B * B)) }

    pub fn inside(&self, x: i64, y: i64, z: i64) -> bool {
        x >= 0 && y >= 0 && z >= 0 && (x as usize) < self.nx && (y as usize) < self.ny && (z as usize) < self.nz
    }

    pub fn get(&self, x: usize, y: usize, z: usize) -> &Cell { &self.cells[self.idx(x, y, z)] }
    pub fn get_mut(&mut self, x: usize, y: usize, z: usize) -> &mut Cell { let i = self.idx(x, y, z); &mut self.cells[i] }
    pub fn set(&mut self, x: usize, y: usize, z: usize, c: Cell) { let i = self.idx(x, y, z); self.cells[i] = c }

    /// The neighbour of (x, y, z) one step along `axis` (0, 1, 2) in direction
    /// `dir` (−1 or +1): a cell inside the box, or what stands just outside it.
    ///
    /// Outside the top, and outside a side next to open air (or grass or
    /// leaves), is fresh air at the ambient temperature. Outside the floor,
    /// and outside a side next to solid matter, is a mirror of the edge cell,
    /// so nothing flows there: the ground continues unchanged beyond the box.
    #[inline]
    pub fn neighbour(&self, x: usize, y: usize, z: usize, axis: usize, dir: i64) -> Neighbour<'_> {
        let mut p = [x as i64, y as i64, z as i64];
        p[axis] += dir;
        if self.inside(p[0], p[1], p[2]) {
            return Neighbour::Inside(&self.cells[self.idx(p[0] as usize, p[1] as usize, p[2] as usize)]);
        }
        let edge = &self.cells[self.idx(x, y, z)];
        let floor = axis == 2 && dir < 0;
        let top = axis == 2 && dir > 0;
        if top || (!floor && edge.permeable(&self.subs)) { return Neighbour::Outside(&self.outside_air) }
        Neighbour::Mirror
    }

    /// Every brick's index, in order.
    pub fn brick_ids(&self) -> core::ops::Range<usize> { 0..self.bricks() }

    /// `f(x, y, z, i)` for every cell `i` at (x, y, z), collected in cell
    /// index order. Built on worker threads with the `parallel` feature (the
    /// vector is filled in place, with no serial pass to clear it first).
    pub fn map_cells<T: Send>(&self, f: impl Fn(usize, usize, usize, usize) -> T + Sync) -> Vec<T> {
        let at = |i: usize| {
            let (ox, oy, oz) = self.brick_origin(i / BRICK_CELLS);
            let (lx, ly, lz) = World::local_xyz(i % BRICK_CELLS);
            f(ox + lx, oy + ly, oz + lz, i)
        };
        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            if THREADS.load(core::sync::atomic::Ordering::Relaxed) != 1 {
                return (0..self.cells.len()).into_par_iter().with_min_len(BRICK_CELLS).map(at).collect();
            }
        }
        (0..self.cells.len()).map(at).collect()
    }

    /// Run a phase: `f(world, brick, out)` fills one brick of `next` (and its
    /// tally) from `cells`. Then `next` becomes the state. Bricks may run on
    /// any thread in any order; the result is the same.
    pub fn run_phase<F>(&mut self, f: F)
    where
        F: Fn(&World, usize, &mut [Cell], &mut Tally) + Sync,
    {
        let mut next = core::mem::take(&mut self.next);
        let mut tallies = core::mem::take(&mut self.tallies);
        {
            let world: &World = self;
            run_bricks(&mut next, &mut tallies, |b, out, t| f(world, b, out, t));
        }
        core::mem::swap(&mut self.cells, &mut next);
        self.next = next;
        self.tallies = tallies;
    }

    /// A hash of the whole state, for comparing peers and runs: each brick's
    /// hash (see [`World::brick_hash`]), chained in brick order. A brick's
    /// hash depends only on its own cells, so any thread, peer or GPU can
    /// work them out in parallel, and a mismatch says which brick differs.
    pub fn hash(&self) -> u64 {
        let mut h = crate::rng::Hasher::new(self.seed ^ self.tick);
        for b in self.brick_ids() { h.u64(self.brick_hash(b)) }
        h.finish()
    }

    /// One brick's hash: its cells in order, from a seed of its index.
    pub fn brick_hash(&self, b: usize) -> u64 {
        let mut h = crate::rng::Hasher::new(b as u64);
        for c in &self.cells[b * BRICK_CELLS..(b + 1) * BRICK_CELLS] { h.cell(c) }
        h.finish()
    }
}

pub enum Neighbour<'a> {
    Inside(&'a Cell),
    Outside(&'a Cell),
    Mirror,
}

/// Runs `f` once per brick, on worker threads when the `parallel` feature is
/// on. Each call gets only its own brick's slice of `next` and its own tally.
pub fn run_bricks<F>(next: &mut [Cell], tallies: &mut [Tally], f: F)
where
    F: Fn(usize, &mut [Cell], &mut Tally) + Sync,
{
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        if THREADS.load(core::sync::atomic::Ordering::Relaxed) != 1 {
            next.par_chunks_mut(BRICK_CELLS).zip(tallies.par_iter_mut()).enumerate().for_each(|(b, (out, t))| f(b, out, t));
            return;
        }
    }
    for (b, (out, t)) in next.chunks_mut(BRICK_CELLS).zip(tallies.iter_mut()).enumerate() { f(b, out, t) }
}

/// 1 forces single-threaded running (for comparing against threads); anything
/// else uses the thread pool.
pub static THREADS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
