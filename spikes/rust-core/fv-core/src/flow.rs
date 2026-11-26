//! Moving air: buoyancy, wind and a pressure solve (Stam's stable fluids),
//! on a grid of blocks of 2×2×2 cells (20 cm at 10 cm cells).
//!
//! The flow carries no energy of its own in the books (brief 06, decision 2):
//! air at 1 m/s in a cell has ~0.6 mJ of motion against ~300 J of heat, and a
//! pressure projection doesn't conserve motion anyway. The flow only says
//! *where* gas goes; `transport::advect` moves it, conserving every
//! milligram and joule. A test checks the motion stays negligible next to
//! the heat it moves.
//!
//! Hot air is lighter, so it rises (Boussinesq buoyancy). Wind enters from the
//! box's sides. A pressure solve then makes the flow go around solid matter
//! and pull fresh air in under a plume, rather than piling up.

use crate::rng::hash;
use crate::transport::FaceVelocity;
use crate::world::{World, TICK};

pub const FR: usize = 2;
/// A block's face, mm².
const BLOCK_FACE_MM2: i64 = (FR * FR) as i64 * crate::substance::FACE_MM2;
/// Gravity, mm/s².
const G: i64 = 9_807;
/// Share of motion kept each tick (eddy viscosity), out of 256.
const KEEP: i64 = 245;
const PRESSURE_ROUNDS: usize = 30;
/// The fastest the transport can carry: [`ADVECT_STEPS`] sub-steps of one
/// cell each are 4 m/s at any cell size; a little under it.
pub const MAX_SPEED: i64 = 3_800;
/// Sub-steps of air motion per tick: two of 10 cm, so a smaller cell takes
/// more (each carries at most one cell's worth: speed·dt/dx ≤ 1).
pub const ADVECT_STEPS: i64 = 2 * 100 / crate::substance::CELL_MM;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Flow {
    pub cx: usize,
    pub cy: usize,
    pub cz: usize,
    /// Velocity across each block's + faces, mm/s: [x, y, z] per block.
    pub vel: Vec<[i32; 3]>,
    /// Prevailing wind at the box's sides, mm/s [x, y] (this tick, with gusts).
    pub wind: [i64; 2],
    /// The wind's average, mm/s [x, y]: the scene sets it.
    pub wind_mean: [i64; 2],
}

impl Flow {
    pub fn new(w: &World) -> Flow {
        let (cx, cy, cz) = (w.nx / FR, w.ny / FR, w.nz / FR);
        Flow { cx, cy, cz, vel: vec![[0; 3]; cx * cy * cz], wind: [0, 0], wind_mean: [500, 120] }
    }

    #[inline]
    fn ci(&self, x: usize, y: usize, z: usize) -> usize { (z * self.cy + y) * self.cx + x }

    /// Velocity across the + face of block (x, y, z) along `axis`, where the
    /// block may be −1 (the box's lower edge) or the last block (upper edge).
    #[inline]
    pub fn block_face(&self, x: i64, y: i64, z: i64, axis: usize) -> i64 {
        let n = [self.cx as i64, self.cy as i64, self.cz as i64];
        let p = [x, y, z];
        // Faces on the box's edge: the wind at the sides, closed at the floor, free at the top.
        if p[axis] < 0 || p[axis] >= n[axis] - 1 {
            if axis == 2 {
                if p[axis] < 0 { return 0 }
                return self.vel[self.ci(x as usize, y as usize, z as usize)][2] as i64;
            }
            return self.wind[axis];
        }
        self.vel[self.ci(x as usize, y as usize, z as usize)][axis] as i64
    }

    /// One tick of air motion, from the cells' temperatures and solidity.
    pub fn step(&mut self, w: &World) {
        let (cx, cy, cz) = (self.cx, self.cy, self.cz);
        let n = cx * cy * cz;
        // Wind: the scene's breeze, with slow gusts of ±40 %.
        let g = (hash(w.seed, w.tick / 40, 0, 0x91) % 801) as i64 - 400;
        self.wind = [self.wind_mean[0] + self.wind_mean[0] * g / 1000, self.wind_mean[1] + self.wind_mean[1] * g / 1000];
        // Gather each block's mean temperature over its open cells, openness,
        // and how much gas it holds beyond what fits (as a volume, mL).
        let mut temp = vec![w.ambient.0; n];
        let mut open = vec![false; n];
        let mut over_ml = vec![0i64; n];
        let fresh: i64 = crate::substance::FRESH_AIR.iter().map(|m| m.0).sum();
        for z in 0..cz { for y in 0..cy { for x in 0..cx {
            let (mut sum, mut cnt, mut over) = (0i64, 0i64, 0i64);
            for d in 0..8 {
                let c = w.get(x * FR + (d & 1), y * FR + (d >> 1 & 1), z * FR + (d >> 2));
                if !c.permeable(&w.subs) { continue }
                let t = c.gas_temperature().0.max(1);
                sum += t;
                cnt += 1;
                // Gas beyond what fits at this temperature, as millilitres of fresh air.
                let ml = c.gas_volume_ml(&w.subs);
                let fits = (fresh as i128 * ml as i128 * w.ambient.0 as i128 / (crate::substance::CELL_VOLUME_ML as i128 * t as i128)) as i64;
                over += (c.gas_mass().0 - fits) * crate::substance::CELL_VOLUME_ML / fresh.max(1);
            }
            let i = self.ci(x, y, z);
            open[i] = cnt >= 4;
            if cnt > 0 { temp[i] = sum / cnt }
            over_ml[i] = over;
        }}}
        let amb = w.ambient.0;
        // Forces: buoyancy on vertical faces, damping, and a little turbulence
        // in hot air so plumes flicker and break rather than stand still.
        for z in 0..cz { for y in 0..cy { for x in 0..cx {
            let i = self.ci(x, y, z);
            for axis in 0..3 {
                let mut v = self.vel[i][axis] as i64 * KEEP / 256;
                let mut q = [x, y, z];
                q[axis] += 1;
                let up_ok = q[axis] < [cx, cy, cz][axis];
                let j = if up_ok { self.ci(q[0], q[1], q[2]) } else { i };
                if !(open[i] && up_ok && open[j]) { self.vel[i][axis] = 0; continue }
                let t = (temp[i] + temp[j]) / 2;
                if axis == 2 {
                    // g·ΔT/T·dt, in mm/s
                    v += G * (t - amb) / amb * TICK.0 / 1_000;
                }
                let hot = (t - amb).max(0) / 1_000; // kelvin above ambient
                if hot > 20 {
                    let h = hash(w.seed, w.tick / 3, i as u64, axis as u64);
                    v += ((h % 201) as i64 - 100) * hot.min(600) / 100;
                }
                self.vel[i][axis] = v.clamp(-MAX_SPEED, MAX_SPEED) as i32;
            }
        }}}
        // Pressure: make the flow divergence-free through open blocks.
        let mut p = vec![0i64; n];
        let mut div = vec![0i64; n];
        for z in 0..cz { for y in 0..cy { for x in 0..cx {
            let i = self.ci(x, y, z);
            if !open[i] { continue }
            let (xi, yi, zi) = (x as i64, y as i64, z as i64);
            div[i] = self.block_face(xi, yi, zi, 0) - self.block_face(xi - 1, yi, zi, 0)
                + self.block_face(xi, yi, zi, 1) - self.block_face(xi, yi - 1, zi, 1)
                + self.block_face(xi, yi, zi, 2) - self.block_face(xi, yi, zi - 1, 2);
            // A block holding more gas than fits should send the extra out
            // over the next couple of ticks: ask for that much net outflow.
            // Half of it each tick, through a block face of (2 cells)²:
            // v [mm/s] = mL × 10³ mm³ / (2 × dt [s] × A [mm²]) (over/4 at 10 cm).
            div[i] -= over_ml[i] * 1_000_000 / (2 * TICK.0 * BLOCK_FACE_MM2);
        }}}
        let mut q = vec![0i64; n];
        for _ in 0..PRESSURE_ROUNDS {
            for z in 0..cz { for y in 0..cy { for x in 0..cx {
                let i = self.ci(x, y, z);
                if !open[i] { q[i] = 0; continue }
                let (mut sum, mut cnt) = (0i64, 0i64);
                for (dx, dy, dz) in [(-1i64, 0i64, 0i64), (1, 0, 0), (0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1)] {
                    let (a, b, c) = (x as i64 + dx, y as i64 + dy, z as i64 + dz);
                    let inside = a >= 0 && b >= 0 && c >= 0 && (a as usize) < cx && (b as usize) < cy && (c as usize) < cz;
                    if !inside {
                        // Sides and top: open air at zero pressure; floor: closed.
                        if c < 0 { continue }
                        cnt += 1;
                        continue;
                    }
                    let j = self.ci(a as usize, b as usize, c as usize);
                    if !open[j] { continue }
                    sum += p[j];
                    cnt += 1;
                }
                q[i] = if cnt > 0 { (sum - div[i]) / cnt } else { 0 };
            }}}
            core::mem::swap(&mut p, &mut q);
        }
        // Subtract the pressure gradient across interior open faces.
        for z in 0..cz { for y in 0..cy { for x in 0..cx {
            let i = self.ci(x, y, z);
            for axis in 0..3 {
                let mut r = [x, y, z];
                r[axis] += 1;
                if r[axis] >= [cx, cy, cz][axis] {
                    // The top face of the top block: free outflow.
                    if axis == 2 && open[i] { self.vel[i][2] = (self.vel[i][2] as i64 + p[i]).clamp(-MAX_SPEED, MAX_SPEED) as i32 }
                    continue;
                }
                let j = self.ci(r[0], r[1], r[2]);
                if !(open[i] && open[j]) { self.vel[i][axis] = 0; continue }
                let v = self.vel[i][axis] as i64 - (p[j] - p[i]);
                self.vel[i][axis] = v.clamp(-MAX_SPEED, MAX_SPEED) as i32;
            }
        }}}
    }

    /// Largest speed anywhere, mm/s.
    pub fn max_speed(&self) -> i64 { self.vel.iter().flat_map(|v| v.iter()).map(|v| (*v as i64).abs()).max().unwrap_or(0) }

    /// Kinetic energy of all the moving air, µJ (for the negligibility test).
    pub fn kinetic(&self) -> i64 {
        // ½·m·v² per block face, with ~1.2 mg of air per mL of block
        // (9.6 g in a 10 cm-cell block of 8 L).
        let mut e: i128 = 0;
        for v in &self.vel { for a in v { e += (*a as i128) * (*a as i128) } }
        let block_mg = 1_200 * 8 * crate::substance::CELL_VOLUME_ML as i128 / 1_000;
        (e * block_mg / 2 / 1_000_000) as i64 // mg·(mm/s)² = 10⁻¹² J → µJ
    }
}

impl FaceVelocity for Flow {
    /// A cell's + face along `axis`: on a block face, that face's velocity;
    /// in the middle of a block, the average of the block's two faces.
    fn face(&self, x: i64, y: i64, z: i64, axis: usize) -> i64 {
        let p = [x, y, z];
        let b = [x.div_euclid(FR as i64), y.div_euclid(FR as i64), z.div_euclid(FR as i64)];
        let on_block_face = (p[axis] + 1).rem_euclid(FR as i64) == 0;
        if on_block_face { return self.block_face(b[0], b[1], b[2], axis) }
        let mut lo = b;
        lo[axis] -= 1;
        (self.block_face(b[0], b[1], b[2], axis) + self.block_face(lo[0], lo[1], lo[2], axis)) / 2
    }
}
