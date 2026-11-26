//! Moving heat and gas between neighbouring cells.
//!
//! Everything here is a *flux across a face*: an amount that leaves one cell
//! and enters the other. Each flux is a pure function of the two cells'
//! previous state, evaluated by both cells with the lower cell first, so
//! both get exactly the same number with opposite signs. However the work is
//! split (threads, peers), what leaves one cell is exactly what the other
//! receives. Across the box's edge, the cell outside is the surroundings, and
//! what crosses is written into the edge brick's tally.
//!
//! Three kinds of transfer:
//! - **conduction**: heat flows down a temperature difference, through each
//!   cell's own conductivity; where solid meets air, through a surface film;
//! - **mixing**: gas spreads between cells (gently in still air, faster in
//!   moving, turbulent air), carrying its heat with it;
//! - **pressure**: a cell holding more gas than fits at its temperature
//!   (hot air, or gas just given off by burning wood) pushes the excess out;
//! - **carrying** (in [`advect`]): moving air takes a share of its contents
//!   across each face it flows through.

use crate::cell::Cell;
use crate::substance::*;
use crate::units::*;
use crate::world::{Neighbour, Tally, World, BRICK_CELLS, TICK};

/// Everything a face needs to know about one side, worked out once per cell.
#[derive(Copy, Clone, Debug, Default)]
pub struct Side {
    /// The gas's temperature: what gas leaving this cell carries.
    pub t: Temperature,
    /// The temperature heat is conducted from: the solid of an opaque cell,
    /// the gas of an airy one.
    pub t_cond: Temperature,
    pub gas_ml: i64,
    pub permeable: bool,
    /// Conductance of half the cell (centre to face).
    pub half: Conductance,
    /// Gas beyond what fits at this temperature (pushed out).
    pub excess: Mass,
    /// The surroundings' temperature (the same for every cell).
    pub ambient: Temperature,
}

/// Thermal conductivity of still air, mW/(m·K).
const AIR_CONDUCTIVITY: i64 = 26;
/// Film coefficient where a solid surface meets air: ~15 W/(m²·K) on a 10 cm face.
const FILM: Conductance = Conductance(150_000);
/// Exchange of gas across a face per tick in still air, in mL (eddies of a
/// few mm/s: effective diffusivity ~1×10⁻³ m²/s).
const STILL_MIX_ML: i64 = 5;
/// Extra exchange per mm/s of air speed at the face, in µL (turbulent eddies ≈ 10 % of the flow).
const TURBULENT_MIX_UL_PER_MM_S: i64 = 50;
/// Gas seeps through non-permeable porous solids this many times slower.
const SEEP_DIVISOR: i64 = 20;

pub fn side(c: &Cell, subs: &[Substance], ambient: Temperature) -> Side {
    let s = &subs[c.substance as usize];
    let share = c.solid_share(subs);
    let k = share.of(s.conductivity) + AIR_CONDUCTIVITY - share.of(AIR_CONDUCTIVITY);
    let t = c.gas_temperature();
    let t_cond = c.temperature(subs);
    let gas_ml = c.gas_volume_ml(subs);
    let fresh: i64 = FRESH_AIR.iter().map(|m| m.0).sum();
    // What fits at this temperature (ideal gas at constant pressure).
    let fits = if t.0 > 0 { (fresh as i128 * gas_ml as i128 * ambient.0 as i128 / (CELL_VOLUME_ML as i128 * t.0 as i128)) as i64 } else { i64::MAX };
    Side {
        t, t_cond, gas_ml, permeable: c.permeable(subs),
        // k [mW/(m·K)] × 0.01 m² / 0.05 m = 0.2·k mW/K = 200·k µW/K
        half: Conductance(k * 200),
        excess: Mass((c.gas_mass().0 - fits).max(0)),
        ambient,
    }
}

/// The surroundings' temperature, which every side carries.
#[inline]
fn ambient_of(a: &Side, _b: &Side) -> Temperature { a.ambient }

/// Speed of the eddies hot gas makes, mm/s: √(g·l·ΔT/T) with l = 0.1 m.
#[inline]
pub fn buoyant_churn(t: Temperature, ambient: Temperature) -> i64 {
    let dt = (t.0 - ambient.0).max(0) as i128;
    if dt == 0 { return 0 }
    // g·l = 9.807 m/s² × 0.1 m = 0.9807 m²/s² = 980_700 mm²/s² ... ×ΔT/T
    let v2 = 980_700i128 * dt / ambient.0.max(1) as i128;
    (v2 as f64).sqrt() as i64
}

/// What flows from the lower cell to the upper across one face.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Flux {
    /// Conducted heat: leaves the lower cell's conducting store (solid if
    /// opaque, else gas) and enters the upper cell's.
    pub energy: Energy,
    /// Heat carried by the moving gas itself (gas store on both sides).
    pub carried: Energy,
    pub gas: [Mass; GASES],
}

/// Energy carried by an amount of gas leaving a cell at temperature `t`.
#[inline]
fn carried(k: usize, m: Mass, t: Temperature) -> Energy {
    let latent = if k == Gas::Vapour as usize { m * LATENT_HEAT_VAPOUR } else { Energy::ZERO };
    (m * GAS_SPECIFIC_HEAT[k]) * t + latent
}

/// Conduction, mixing and pressure across one face, lower cell first.
/// `speed` is the air speed at the face (mm/s, unsigned).
pub fn diffuse_face(lo: &Cell, a: &Side, hi: &Cell, b: &Side, speed: i64) -> Flux {
    let mut f = Flux::default();
    // Conduction: two half-cells in series; a solid meeting air goes through the film.
    let (ga, gb) = (a.half.0.max(1) as i128, b.half.0.max(1) as i128);
    let mut g = ga * gb / (ga + gb);
    if a.permeable != b.permeable {
        let solid = if a.permeable { gb } else { ga };
        g = solid * FILM.0 as i128 / (solid + FILM.0 as i128);
    }
    f.energy = (Conductance(g as i64) * (a.t_cond - b.t_cond)) * TICK;

    // Mixing: each side sends a share of its gas, by concentration. Eddies
    // come from the wind and from heat itself: hot gas churns at about
    // √(g·l·ΔT/T) (buoyant turbulence, l = 10 cm), metres a second in a flame.
    let both_gas = a.gas_ml > 0 && b.gas_ml > 0;
    if both_gas {
        let churn = buoyant_churn(a.t.max(b.t), ambient_of(a, b));
        let mut ml = STILL_MIX_ML + (speed + churn) * TURBULENT_MIX_UL_PER_MM_S / 1_000;
        if !(a.permeable && b.permeable) { ml /= SEEP_DIVISOR }
        let ml = ml.min(a.gas_ml.min(b.gas_ml) / 8);
        for k in 0..GASES {
            let out_a = Mass(lo.gas[k].0 * ml / a.gas_ml);
            let out_b = Mass(hi.gas[k].0 * ml / b.gas_ml);
            f.gas[k] += out_a - out_b;
            f.carried += carried(k, out_a, a.t) - carried(k, out_b, b.t);
        }
    }

    // Pressure: gas beyond what fits, per millilitre of room, is the
    // overpressure. The higher side pushes toward the lower, an eighth of the
    // difference over the smaller volume per tick. Into or between solid
    // pores it's a slow seep.
    if both_gas && (a.excess.0 > 0 || b.excess.0 > 0) {
        let both_solid = !a.permeable && !b.permeable;
        let vol = a.gas_ml.min(b.gas_ml) as i128;
        let over = a.excess.0 as i128 * vol / a.gas_ml as i128 - b.excess.0 as i128 * vol / b.gas_ml as i128;
        let push = (over / if both_solid { 8 * SEEP_DIVISOR as i128 } else { 8 }) as i64;
        let (from, from_side, sign) = if push >= 0 { (lo, a, 1) } else { (hi, b, -1) };
        let total = from.gas_mass().0;
        if total > 0 && push != 0 {
            let push = push.abs().min(total / 8);
            for k in 0..GASES {
                let m = Mass((push as i128 * from.gas[k].0 as i128 / total as i128) as i64);
                f.gas[k] += Mass(m.0 * sign);
                f.carried += Energy(carried(k, m, from_side.t).0 * sign);
            }
        }
    }
    f
}

/// Applies a face's flux to the cell on one side: `sign` is −1 for the lower
/// cell (it gives) and +1 for the upper (it receives). Conducted heat goes
/// to the solid of an opaque cell, the gas of an airy one.
#[inline]
pub fn apply(out: &mut Cell, me: &Side, f: &Flux, sign: i64) {
    if me.permeable { out.energy += Energy(f.energy.0 * sign) } else { out.solid_energy += Energy(f.energy.0 * sign) }
    out.energy += Energy(f.carried.0 * sign);
    for k in 0..GASES { out.gas[k] += Mass(f.gas[k].0 * sign) }
}

/// Records a flux that crossed the box's edge, from the box's point of view.
#[inline]
pub fn tally_edge(t: &mut Tally, f: &Flux, sign: i64) {
    t.energy_in += (f.energy.0 + f.carried.0) * sign;
    for k in 0..GASES { t.gas_in[k] += f.gas[k].0 * sign }
}

/// Every cell's face-side summary, computed in parallel by brick.
pub fn sides(world: &World) -> Vec<Side> {
    let mut out = vec![Side::default(); world.cells.len()];
    let subs = &world.subs;
    let amb = world.ambient;
    let cells = &world.cells;
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        if crate::world::THREADS.load(core::sync::atomic::Ordering::Relaxed) != 1 {
            out.par_chunks_mut(BRICK_CELLS).zip(cells.par_chunks(BRICK_CELLS)).for_each(|(o, c)| {
                for i in 0..BRICK_CELLS { o[i] = side(&c[i], subs, amb) }
            });
            return out;
        }
    }
    for (o, c) in out.iter_mut().zip(cells) { *o = side(c, subs, amb) }
    out
}

/// Air speed across faces, supplied by the flow (see `flow`): the velocity
/// across the + face of cell (x, y, z) along `axis`, in mm/s.
pub trait FaceVelocity: Sync {
    /// The + face of cell (x, y, z); x, y or z may be −1 for the face on the
    /// box's lower edge.
    fn face(&self, x: i64, y: i64, z: i64, axis: usize) -> i64;
}

/// Still air everywhere.
pub struct Still;
impl FaceVelocity for Still { fn face(&self, _: i64, _: i64, _: i64, _: usize) -> i64 { 0 } }

/// The same velocity on every face (tests).
pub struct Uniform(pub [i64; 3]);
impl FaceVelocity for Uniform { fn face(&self, _: i64, _: i64, _: i64, axis: usize) -> i64 { self.0[axis] } }

/// The diffusion phase: conduction, mixing and pressure on every face.
pub fn diffuse(world: &mut World, vel: &dyn FaceVelocity) {
    let sides = sides(world);
    let outside = side(&world.outside_air, &world.subs, world.ambient);
    world.run_phase(|w, b, out, tally| diffuse_brick(w, &sides, &outside, vel, b, out, tally));
}

/// One brick of the diffusion phase, from the previous state only. Any peer
/// or thread computing brick `b` gets exactly these cells and this tally.
pub fn diffuse_brick(w: &World, sides: &[Side], outside: &Side, vel: &dyn FaceVelocity, b: usize, out: &mut [Cell], tally: &mut Tally) {
    let (ox, oy, oz) = w.brick_origin(b);
    let base = b * BRICK_CELLS;
    for l in 0..BRICK_CELLS {
        let (lx, ly, lz) = World::local_xyz(l);
        let (x, y, z) = (ox + lx, oy + ly, oz + lz);
        let me = &w.cells[base + l];
        let my = &sides[base + l];
        let mut next = *me;
        for axis in 0..3 {
            for dir in [-1i64, 1] {
                let speed = face_speed(w, vel, x, y, z, axis, dir);
                match w.neighbour(x, y, z, axis, dir) {
                    Neighbour::Inside(n) => {
                        let ni = neighbour_index(w, x, y, z, axis, dir);
                        let ns = &sides[ni];
                        let flux = if dir > 0 { diffuse_face(me, my, n, ns, speed) } else { diffuse_face(n, ns, me, my, speed) };
                        apply(&mut next, my, &flux, -dir);
                    }
                    Neighbour::Outside(o) => {
                        let flux = if dir > 0 { diffuse_face(me, my, o, outside, speed) } else { diffuse_face(o, outside, me, my, speed) };
                        apply(&mut next, my, &flux, -dir);
                        tally_edge(tally, &flux, -dir);
                    }
                    Neighbour::Mirror => {}
                }
            }
        }
        out[l] = next;
    }
}

/// A brick's result for one phase, as a peer would send it.
#[derive(Clone, Debug, PartialEq)]
pub struct BrickResult { pub brick: usize, pub cells: Vec<Cell>, pub tally: Tally }

/// Computes only the given bricks of the diffusion phase (one peer's share).
pub fn diffuse_part(w: &World, vel: &dyn FaceVelocity, bricks: &[usize]) -> Vec<BrickResult> {
    let sides = sides(w);
    let outside = side(&w.outside_air, &w.subs, w.ambient);
    bricks.iter().map(|&b| {
        let mut cells = vec![Cell::default(); BRICK_CELLS];
        let mut tally = Tally::default();
        diffuse_brick(w, &sides, &outside, vel, b, &mut cells, &mut tally);
        BrickResult { brick: b, cells, tally }
    }).collect()
}

/// Finishes a phase from every brick's result (ours and the other peers'):
/// the results become the state, and their tallies are added.
pub fn apply_parts(w: &mut World, parts: &[BrickResult]) {
    let mut next = core::mem::take(&mut w.next);
    for p in parts {
        next[p.brick * BRICK_CELLS..(p.brick + 1) * BRICK_CELLS].copy_from_slice(&p.cells);
        w.tallies[p.brick].add(&p.tally);
    }
    core::mem::swap(&mut w.cells, &mut next);
    w.next = next;
}

/// Air carried by the flow, as `steps` sub-steps (each moves at most one
/// cell's worth). A cell sends `speed·dt/dx` of its gas out through each face
/// the air leaves by, scaled down if that would add up to more than all of it.
pub fn advect(world: &mut World, vel: &dyn FaceVelocity, steps: i64) {
    for _ in 0..steps {
        let sides = sides(world);
        let outside = side(&world.outside_air, &world.subs, world.ambient);
        world.run_phase(|w, b, out, tally| {
            let (ox, oy, oz) = w.brick_origin(b);
            let base = b * BRICK_CELLS;
            for l in 0..BRICK_CELLS {
                let (lx, ly, lz) = World::local_xyz(l);
                let (x, y, z) = (ox + lx, oy + ly, oz + lz);
                let me = &w.cells[base + l];
                let my = &sides[base + l];
                let mut next = *me;
                for axis in 0..3 {
                    for dir in [-1i64, 1] {
                        let u = signed_face_velocity(w, vel, x, y, z, axis, dir);
                        if u == 0 { continue }
                        match w.neighbour(x, y, z, axis, dir) {
                            Neighbour::Inside(n) => {
                                let ni = neighbour_index(w, x, y, z, axis, dir);
                                let (nx, ny, nz) = step_xyz(x, y, z, axis, dir);
                                let flux = carry_face(w, vel, steps, (me, my, x, y, z), (n, &sides[ni], nx, ny, nz), axis, dir, u);
                                apply(&mut next, my, &flux, -dir);
                            }
                            Neighbour::Outside(o) => {
                                let flux = carry_face_outside(w, vel, steps, (me, my, x, y, z), o, &outside, axis, dir, u);
                                apply(&mut next, my, &flux, -dir);
                                tally_edge(tally, &flux, -dir);
                            }
                            Neighbour::Mirror => {}
                        }
                    }
                }
                out[l] = next;
            }
        });
    }
}

#[inline]
fn step_xyz(x: usize, y: usize, z: usize, axis: usize, dir: i64) -> (usize, usize, usize) {
    let mut p = [x as i64, y as i64, z as i64];
    p[axis] += dir;
    (p[0] as usize, p[1] as usize, p[2] as usize)
}

#[inline]
fn neighbour_index(w: &World, x: usize, y: usize, z: usize, axis: usize, dir: i64) -> usize {
    let (a, b, c) = step_xyz(x, y, z, axis, dir);
    w.idx(a, b, c)
}

/// Velocity across the face on `dir` side of the cell, positive in +axis.
#[inline]
fn signed_face_velocity(_w: &World, vel: &dyn FaceVelocity, x: usize, y: usize, z: usize, axis: usize, dir: i64) -> i64 {
    let mut p = [x as i64, y as i64, z as i64];
    if dir < 0 { p[axis] -= 1 }
    vel.face(p[0], p[1], p[2], axis)
}

#[inline]
fn face_speed(w: &World, vel: &dyn FaceVelocity, x: usize, y: usize, z: usize, axis: usize, dir: i64) -> i64 {
    signed_face_velocity(w, vel, x, y, z, axis, dir).abs()
}

/// Share (of 2³²) of a cell's gas that leaves through all its outflow faces
/// in one sub-step, before scaling: Σ |u|·dt/dx over faces the air leaves by.
fn outflow_share(w: &World, vel: &dyn FaceVelocity, steps: i64, x: usize, y: usize, z: usize) -> u64 {
    let mut total: u64 = 0;
    for axis in 0..3 {
        let plus = signed_face_velocity(w, vel, x, y, z, axis, 1);
        let minus = signed_face_velocity(w, vel, x, y, z, axis, -1);
        if plus > 0 { total += share_of_speed(plus, steps) }
        if minus < 0 { total += share_of_speed(-minus, steps) }
    }
    total
}

/// |u|·dt/dx as a share of 2³²: dx = 100 mm, dt = 50 ms / steps.
#[inline]
fn share_of_speed(u: i64, steps: i64) -> u64 {
    ((u as u128 * TICK.0 as u128 * (1u128 << 32)) / (100u128 * 1_000 * steps as u128)) as u64
}

/// The share actually sent through one face, after scaling so the cell never
/// sends more than it has.
fn face_share(w: &World, vel: &dyn FaceVelocity, steps: i64, donor: (usize, usize, usize), u: i64) -> Fraction {
    let s = share_of_speed(u.abs(), steps);
    let total = outflow_share(w, vel, steps, donor.0, donor.1, donor.2);
    if total <= Fraction::ONE.0 { return Fraction(s) }
    Fraction((s as u128 * Fraction::ONE.0 as u128 / total as u128) as u64)
}

type SideRef<'a> = (&'a Cell, &'a Side, usize, usize, usize);

fn carry_from(donor: &Cell, ds: &Side, share: Fraction, sign: i64) -> Flux {
    let mut f = Flux::default();
    for k in 0..GASES {
        let m = share.of_mass(donor.gas[k]);
        f.gas[k] = Mass(m.0 * sign);
        f.carried += Energy(carried(k, m, ds.t).0 * sign);
    }
    f
}

fn carry_face(w: &World, vel: &dyn FaceVelocity, steps: i64, me: SideRef, n: SideRef, axis: usize, dir: i64, u: i64) -> Flux {
    if !me.1.permeable || !n.1.permeable { return Flux::default() }
    // Lower and upper cell along the axis; u > 0 flows lower → upper.
    let (lo, hi) = if dir > 0 { (me, n) } else { (n, me) };
    let _ = axis;
    if u > 0 {
        let share = face_share(w, vel, steps, (lo.2, lo.3, lo.4), u);
        carry_from(lo.0, lo.1, share, 1)
    } else {
        let share = face_share(w, vel, steps, (hi.2, hi.3, hi.4), u);
        carry_from(hi.0, hi.1, share, -1)
    }
}

fn carry_face_outside(w: &World, vel: &dyn FaceVelocity, steps: i64, me: SideRef, o: &Cell, os: &Side, _axis: usize, dir: i64, u: i64) -> Flux {
    if !me.1.permeable { return Flux::default() }
    // Outside air coming in: the outside is a large reservoir, so its share is
    // simply speed·dt/dx of a fresh cell.
    let leaving = (u > 0) == (dir > 0);
    if leaving {
        let share = face_share(w, vel, steps, (me.2, me.3, me.4), u);
        return carry_from(me.0, me.1, share, if dir > 0 { 1 } else { -1 });
    }
    let share = Fraction(share_of_speed(u.abs(), steps).min(Fraction::ONE.0));
    carry_from(o, os, share, if dir > 0 { -1 } else { 1 })
}
