//! Radiation as a field: the discrete-ordinates method (as FDS uses; Raithby
//! & Chui 1990, McGrattan et al., FDS Technical Reference, ch. 6).
//!
//! Instead of sending packets, radiant energy flows from cell to cell in 26
//! fixed directions (the faces, edges and corners of a cube), in each of the
//! five bands. Each tick, for each direction and band, one **sweep** passes
//! through the box in the order the light travels, so every cell's outflow
//! depends only on what flowed into it this tick (from upstream cells) and
//! what it emits; nothing is iterated, so the result does not depend on
//! threads. In each cell the flow is:
//!
//! - **absorbed** by the same κ as the packets meet: blades (σ·share/4),
//!   soot and water drops (ionising: mass attenuation μρ). What is caught is
//!   split between them in proportion to their κ;
//! - **emitted** by Kirchhoff's law, exactly as much as the packets carry on
//!   average (the same [`emitters`](crate::rad) and flame light), but split
//!   exactly over the directions, with no noise;
//! - stopped at **opaque faces**, which absorb (1 − albedo) of each band and
//!   reflect the rest diffusely (cosine-weighted) out of that face. A red
//!   surface lit white sends red back into the room: colours mix.
//!
//! Reflection and scattering are bounces: after the first sweep, what was
//! reflected or scattered is the source of the next set of sweeps. After
//! [`BOUNCES`] bounces (infrared: one, as surfaces reflect little of it)
//! what would bounce again is absorbed where it landed (the packets stop
//! after four), so all of a tick's light lands within the tick and none is
//! left in flight. The ionising band is a field too: it is only absorbed
//! (by mass, through solids as well as air), never reflected, which is the
//! easy case for this method, and it loses the packets' shot noise.
//!
//! Direct sunlight is collimated, so it is not spread over directions: it is
//! marched along the sun's exact direction, one ray per column, losing a
//! fraction e^(−τ) in each cell it crosses. Shadows from the sun stay sharp.
//! Skylight is diffuse and enters the field at the top.
//!
//! Every split is integer and exact (the last part takes the remainder), so
//! every microjoule emitted is absorbed somewhere or leaves the box, and is
//! booked as such.
//!
//! Known costs of the method: light from a small source spreads along the
//! 26 directions (the "ray effect"), and the upwind step from cell to cell
//! blurs beams that travel off the grid's axes ("false scattering"). So
//! shadows cast by firelight are soft and a lone hot spot throws faint
//! spokes. Sunlight avoids both by being marched as rays.

use crate::rad::{absorb_table, band_albedo, drop_albedo, emitters, exposed, net_sigma, settle, Light, Sky, IONISING, IR, ONE};
use crate::substance::*;
use crate::tables::{k_index, tables};
use crate::units::*;
use crate::world::{World, TICK};
use core::sync::atomic::{AtomicBool, AtomicI64, Ordering::Relaxed};
use std::sync::OnceLock;

pub const BANDS: usize = 5;
/// Reflections and scatterings followed within a tick.
pub const BOUNCES: usize = 2;
const PASSES: usize = BOUNCES + 1;
const MAX_BEAM_STEPS: usize = 400;

/// The directions: their components (−1, 0 or 1 on each axis) and how the
/// sphere is shared between them.
pub(crate) struct Dirs {
    pub d: Vec<[i64; 3]>,
    /// The axes each direction moves along.
    axes: Vec<Vec<usize>>,
    /// Mean path through a cell (cell lengths, Q16) by number of axes: the
    /// cell's volume over its area seen from that direction, 1/(|dx|+|dy|+|dz|).
    len_q16: [i64; 4],
    /// Cumulative share of an isotropic source per direction, out of 2³² (len D+1).
    cum_iso: Vec<u64>,
    /// Cumulative share of a diffuse (Lambertian) source leaving a face with
    /// outward normal f, per direction.
    cum_lam: [Vec<u64>; 6],
    /// The face normals whose diffuse light goes into each direction.
    normals: Vec<Vec<usize>>,
}

/// Face index for the face whose outward normal is `sign` along `axis`.
#[inline]
pub(crate) fn face(axis: usize, sign: i64) -> usize { axis * 2 + usize::from(sign > 0) }
fn normal(f: usize) -> (usize, i64) { (f / 2, if f % 2 == 1 { 1 } else { -1 }) }

pub(crate) fn dirs() -> &'static Dirs {
    static D: OnceLock<Dirs> = OnceLock::new();
    D.get_or_init(build_dirs)
}

/// Solid-angle weights by integrating over the sphere (sampled through the
/// faces of a cube) and giving each sample to its nearest direction. Only
/// arithmetic and square roots (exactly rounded in IEEE), once at start, then
/// rounded to integers: the same on every machine.
fn build_dirs() -> Dirs {
    let mut d = Vec::new();
    for z in -1i64..=1 { for y in -1i64..=1 { for x in -1i64..=1 {
        if (x, y, z) != (0, 0, 0) { d.push([x, y, z]) }
    }}}
    let unit: Vec<[f64; 3]> = d.iter().map(|v| {
        let n = ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) as f64).sqrt();
        [v[0] as f64 / n, v[1] as f64 / n, v[2] as f64 / n]
    }).collect();
    let m = 96;
    let mut iso = vec![0f64; d.len()];
    let mut lam: [Vec<f64>; 6] = core::array::from_fn(|_| vec![0f64; d.len()]);
    for f in 0..6 {
        let (axis, sign) = normal(f);
        for i in 0..m { for j in 0..m {
            let u = -1.0 + (2 * i + 1) as f64 / m as f64;
            let v = -1.0 + (2 * j + 1) as f64 / m as f64;
            let mut p = [0f64; 3];
            p[axis] = sign as f64;
            p[(axis + 1) % 3] = u;
            p[(axis + 2) % 3] = v;
            let r2 = u * u + v * v + 1.0;
            let r = r2.sqrt();
            let dw = (2.0 / m as f64) * (2.0 / m as f64) / (r2 * r);
            let ph = [p[0] / r, p[1] / r, p[2] / r];
            let mut best = 0;
            let mut best_dot = f64::MIN;
            for (k, e) in unit.iter().enumerate() {
                let dot = e[0] * ph[0] + e[1] * ph[1] + e[2] * ph[2];
                if dot > best_dot + 1e-12 { best = k; best_dot = dot }
            }
            iso[best] += dw;
            for (g, l) in lam.iter_mut().enumerate() {
                let (ga, gs) = normal(g);
                let cos = ph[ga] * gs as f64;
                if cos > 0.0 { l[best] += dw * cos }
            }
        }}
    }
    let cum = |w: &[f64]| -> Vec<u64> {
        let total: f64 = w.iter().sum();
        let mut out = vec![0u64; w.len() + 1];
        let mut acc = 0f64;
        for k in 0..w.len() {
            acc += w[k];
            out[k + 1] = ((acc / total) * Fraction::ONE.0 as f64).round() as u64;
        }
        out[w.len()] = Fraction::ONE.0;
        out
    };
    let cum_iso = cum(&iso);
    let cum_lam: [Vec<u64>; 6] = core::array::from_fn(|f| cum(&lam[f]));
    let normals = (0..d.len()).map(|k| (0..6).filter(|&f| cum_lam[f][k + 1] > cum_lam[f][k]).collect()).collect();
    let axes = d.iter().map(|v| (0..3).filter(|&a| v[a] != 0).collect()).collect();
    let len_q16 = [0, ONE, (ONE as f64 / 2f64.sqrt()).round() as i64, (ONE as f64 / 3f64.sqrt()).round() as i64];
    Dirs { d, axes, len_q16, cum_iso, cum_lam, normals }
}

/// The share of `e` that goes in direction `k`, by cumulative weights:
/// the shares over all directions add up to `e` exactly.
#[inline]
fn share(e: i64, cum: &[u64], k: usize) -> i64 {
    if e == 0 { return 0 }
    let hi = (e as i128 * cum[k + 1] as i128) >> 32;
    let lo = (e as i128 * cum[k] as i128) >> 32;
    (hi - lo) as i64
}

/// Sources of one pass: isotropic, inside a cell; and diffuse, leaving a
/// face into the cell (indexed by that face's normal, which points into it).
struct Sources {
    vol: [Vec<i64>; BANDS],
    face: [Vec<[i64; 6]>; BANDS],
    /// Bit b set: the cell has a source in band b.
    mask: Vec<u8>,
    /// The cells with any source, each once.
    touched: Vec<u32>,
}

impl Sources {
    fn new(n: usize) -> Sources {
        Sources { vol: core::array::from_fn(|_| vec![0; n]), face: core::array::from_fn(|_| vec![[0; 6]; n]), mask: vec![0; n], touched: Vec::new() }
    }
    #[inline]
    fn touch(&mut self, l: usize, b: usize) {
        if self.mask[l] == 0 { self.touched.push(l as u32) }
        self.mask[l] |= 1 << b;
    }
    #[inline]
    fn add_vol(&mut self, b: usize, l: usize, e: i64) { if e != 0 { self.touch(l, b); self.vol[b][l] += e } }
    #[inline]
    fn add_face(&mut self, b: usize, l: usize, f: usize, e: i64) { if e != 0 { self.touch(l, b); self.face[b][l][f] += e } }
    fn clear(&mut self) {
        for &l in &self.touched {
            let l = l as usize;
            for b in 0..BANDS { self.vol[b][l] = 0; self.face[b][l] = [0; 6] }
            self.mask[l] = 0;
        }
        self.touched.clear();
    }
}

/// What the sweeps caught: inside cells (by any κ) and on opaque faces.
/// Integer sums, so the order sweeps add to them does not matter.
struct Caught {
    vol: [Vec<AtomicI64>; BANDS],
    hit: [Vec<[AtomicI64; 6]>; BANDS],
    /// Set on any catch in the cell, so only those are looked at after.
    any: Vec<AtomicBool>,
}

impl Caught {
    fn new(n: usize) -> Caught {
        Caught {
            vol: core::array::from_fn(|_| (0..n).map(|_| AtomicI64::new(0)).collect()),
            hit: core::array::from_fn(|_| (0..n).map(|_| core::array::from_fn(|_| AtomicI64::new(0))).collect()),
            any: (0..n).map(|_| AtomicBool::new(false)).collect(),
        }
    }
    #[inline]
    fn vol(&self, b: usize, l: usize, e: i64) { self.vol[b][l].fetch_add(e, Relaxed); self.any[l].store(true, Relaxed) }
    #[inline]
    fn hit(&self, b: usize, l: usize, f: usize, e: i64) { self.hit[b][l][f].fetch_add(e, Relaxed); self.any[l].store(true, Relaxed) }
}

/// Working space kept between ticks.
pub struct Scratch {
    n: usize,
    src: Sources,
    next: Sources,
    caught: Caught,
}

/// The box in plain x-fastest order, with what the sweeps need of each cell.
struct Grid {
    dims: [usize; 3],
    strides: [usize; 3],
    /// Cell index of each grid position.
    idx: Vec<u32>,
    opaque: Vec<bool>,
    /// Attenuation per metre (Q16) per band.
    kappa: [Vec<i64>; BANDS],
}

impl Grid {
    fn build(w: &World) -> Grid {
        let (nx, ny, nz) = (w.nx, w.ny, w.nz);
        let n = nx * ny * nz;
        let mut idx = vec![0u32; n];
        let mut opaque = vec![false; n];
        let mut kappa: [Vec<i64>; BANDS] = core::array::from_fn(|_| vec![0; n]);
        for z in 0..nz { for y in 0..ny { for x in 0..nx {
            let l = (z * ny + y) * nx + x;
            let i = w.idx(x, y, z);
            idx[l] = i as u32;
            let c = &w.cells[i];
            opaque[l] = c.opaque(&w.subs);
            let k = kappas(c, &w.subs);
            for b in 0..4 { kappa[b][l] = k.0 + k.1 + k.2 }
            kappa[IONISING][l] = ionising_kappa(c, &w.subs);
        }}}
        Grid { dims: [nx, ny, nz], strides: [1, nx, nx * ny], idx, opaque, kappa }
    }
    #[inline]
    fn n(&self) -> usize { self.idx.len() }
    #[inline]
    fn xyz(&self, l: usize) -> [usize; 3] { [l % self.dims[0], l / self.dims[0] % self.dims[1], l / self.strides[2]] }
    /// The grid position one step across face f, if inside.
    #[inline]
    fn across(&self, l: usize, f: usize) -> Option<usize> {
        let (a, s) = normal(f);
        let p = self.xyz(l)[a] as i64 + s;
        if p < 0 || p >= self.dims[a] as i64 { return None }
        Some((l as i64 + s * self.strides[a] as i64) as usize)
    }
}

/// κ of blades, soot and drops (Q16 per metre), as the packets meet them.
fn kappas(c: &crate::cell::Cell, subs: &[Substance]) -> (i64, i64, i64) {
    let sub = &subs[c.substance as usize];
    let share = c.solid_share(subs).0 as i64;
    let k_solid = sub.surface_per_volume * share / Fraction::ONE.0 as i64 * ONE / 4;
    let k_soot = c.gas[Gas::Soot as usize].0 * ONE / CELL_VOLUME_ML;
    let k_drop = c.gas[Gas::Droplets as usize].0 * ONE * 15 / (100 * CELL_VOLUME_ML);
    (k_solid, k_soot, k_drop)
}

fn ionising_kappa(c: &crate::cell::Cell, subs: &[Substance]) -> i64 {
    let sub = &subs[c.substance as usize];
    let mass = (c.solid_mass().0 + c.water.0 + c.gas_mass().0) / CELL_VOLUME_ML;
    let mu = if c.substance == NONE { 60 } else { sub.optics.ionising_cm2_per_g };
    mu * mass / 1_000 * ONE / 10_000
}

#[inline]
fn caught_fraction(kappa: i64, len_q16: i64) -> u64 {
    // τ (Q16) = κ (Q16 per metre) × length (cells, Q16) × edge (m).
    let tau = (kappa as i128 * len_q16 as i128 / ONE as i128 * CELL_MM as i128 / 1_000) as i64;
    absorb_table()[((tau >> 8) as usize).min(4095)]
}

/// Where a set of bands' sources are this pass: the box around them.
struct Extent { lo: [usize; 3], hi: [usize; 3] }

fn extent(g: &Grid, src: &Sources, bits: u8) -> Option<Extent> {
    let mut lo = g.dims;
    let mut hi = [0usize; 3];
    let mut any = false;
    for &l in &src.touched {
        if src.mask[l as usize] & bits == 0 { continue }
        any = true;
        let p = g.xyz(l as usize);
        for a in 0..3 { lo[a] = lo[a].min(p[a]); hi[a] = hi[a].max(p[a]) }
    }
    any.then_some(Extent { lo, hi })
}

/// One sweep in direction k of N bands that meet the same κ (infrared and
/// the three colours together; ionising alone). Returns what left the box.
///
/// Light only travels downstream, so the sweep covers just the part of the
/// box downstream of the sources' extent: exactly the same result as
/// sweeping the whole box (the cells left out would only ever see zero).
fn sweep<const N: usize>(g: &Grid, k: usize, bands: [usize; N], src: &Sources, ext: &Extent, caught: &Caught, buf: &mut [Vec<[i64; N]>; 3]) -> i64 {
    let ds = dirs();
    let d = ds.d[k];
    let axes = &ds.axes[k];
    let len = ds.len_q16[axes.len()];
    let normals = &ds.normals[k];
    let ion = bands[0] == IONISING;
    let st = g.strides;
    let na = axes.len() as i64;
    let mut escaped = 0i64;
    let kap = &g.kappa[bands[0]];
    let bits = bands.iter().fold(0u8, |m, &b| m | 1 << b);
    // The swept range on each axis, inclusive, and the cell it starts from
    // (which has no upstream neighbour inside the range).
    let range: [(usize, usize); 3] = core::array::from_fn(|a| match d[a] {
        1 => (ext.lo[a], g.dims[a] - 1),
        -1 => (0, ext.hi[a]),
        _ => (ext.lo[a], ext.hi[a]),
    });
    let first: [usize; 3] = core::array::from_fn(|a| if d[a] < 0 { range[a].1 } else { range[a].0 });
    let order = |a: usize, i: usize| if d[a] < 0 { range[a].1 - i } else { range[a].0 + i };
    let count = |a: usize| range[a].1 - range[a].0 + 1;
    const Z: i64 = 0;
    for zi in 0..count(2) {
        let z = order(2, zi);
        for yi in 0..count(1) {
            let y = order(1, yi);
            for xi in 0..count(0) {
                let x = order(0, xi);
                let p = [x, y, z];
                let l = (z * g.dims[1] + y) * g.dims[0] + x;
                if !ion && g.opaque[l] {
                    for &a in axes { buf[a][l] = [Z; N] }
                    continue;
                }
                let mut inflow = [0i64; N];
                for &a in axes {
                    if p[a] != first[a] {
                        let up = &buf[a][(l as i64 - d[a] * st[a] as i64) as usize];
                        for j in 0..N { inflow[j] += up[j] }
                    }
                }
                let mut s = [0i64; N];
                if src.mask[l] & bits != 0 {
                    for (j, &b) in bands.iter().enumerate() {
                        s[j] = share(src.vol[b][l], &ds.cum_iso, k);
                        let fs = &src.face[b][l];
                        for &f in normals { s[j] += share(fs[f], &ds.cum_lam[f], k) }
                    }
                }
                if inflow == [0; N] && s == [0; N] {
                    for &a in axes { buf[a][l] = [Z; N] }
                    continue;
                }
                let mut rest = [0i64; N];
                if kap[l] > 0 {
                    // Light arriving crosses the whole cell; light made in it, half.
                    let (full, half) = (Fraction(caught_fraction(kap[l], len)), Fraction(caught_fraction(kap[l], len / 2)));
                    let mut caught_any = false;
                    for j in 0..N {
                        let absorbed = full.of(inflow[j]) + half.of(s[j]);
                        if absorbed > 0 { caught.vol[bands[j]][l].fetch_add(absorbed, Relaxed); caught_any = true }
                        rest[j] = inflow[j] + s[j] - absorbed;
                    }
                    if caught_any { caught.any[l].store(true, Relaxed) }
                } else {
                    for j in 0..N { rest[j] = inflow[j] + s[j] }
                }
                let each: [i64; N] = core::array::from_fn(|j| rest[j] / na);
                let mut left = rest;
                for (m, &a) in axes.iter().enumerate() {
                    let part: [i64; N] = if m + 1 == axes.len() { left } else { each };
                    for j in 0..N { left[j] -= part[j] }
                    let q = p[a] as i64 + d[a];
                    if q < 0 || q >= g.dims[a] as i64 { escaped += part.iter().sum::<i64>(); buf[a][l] = [Z; N]; continue }
                    let nl = (l as i64 + d[a] * st[a] as i64) as usize;
                    if !ion && g.opaque[nl] {
                        for j in 0..N { if part[j] > 0 { caught.hit(bands[j], nl, face(a, -d[a]), part[j]) } }
                        buf[a][l] = [Z; N];
                        continue;
                    }
                    buf[a][l] = part;
                }
            }
        }
    }
    escaped
}

/// Direct sunlight along one ray: from `pos` (Q16 cells) entering through
/// face `entry` of its first cell, losing e^(−τ) in each cell it crosses.
/// Returns what left the box.
fn beam(g: &Grid, mut pos: [i64; 3], dir: [i64; 3], mut entry: usize, mut e: [i64; 4], caught: &Caught) -> i64 {
    let mut cell = pos.map(|v| v.div_euclid(ONE));
    let inside = |c: [i64; 3]| (0..3).all(|a| c[a] >= 0 && c[a] < g.dims[a] as i64);
    let lin = |c: [i64; 3]| (c[2] as usize * g.dims[1] + c[1] as usize) * g.dims[0] + c[0] as usize;
    for _ in 0..MAX_BEAM_STEPS {
        if !inside(cell) { return e.iter().sum() }
        let l = lin(cell);
        if g.opaque[l] {
            for b in 0..4 { if e[b] > 0 { caught.hit(b, l, entry, e[b]) } }
            return 0;
        }
        let mut t_exit = i64::MAX;
        let mut exit_axis = 0;
        for a in 0..3 {
            if dir[a] == 0 { continue }
            let edge = if dir[a] > 0 { (cell[a] + 1) * ONE - pos[a] } else { pos[a] - cell[a] * ONE + 1 };
            let t = edge * ONE / dir[a].abs();
            if t < t_exit { t_exit = t; exit_axis = a }
        }
        let t_exit = t_exit.max(1);
        for b in 0..4 {
            let k = g.kappa[b][l];
            if k > 0 && e[b] > 0 {
                let a = Fraction(caught_fraction(k, t_exit)).of(e[b]);
                if a > 0 { caught.vol(b, l, a); e[b] -= a }
            }
        }
        if e.iter().all(|&v| v == 0) { return 0 }
        let step = if dir[exit_axis] > 0 { 1 } else { -1 };
        for a in 0..3 { pos[a] += dir[a] * t_exit / ONE }
        cell[exit_axis] += step;
        pos[exit_axis] = if step > 0 { cell[exit_axis] * ONE } else { cell[exit_axis] * ONE + ONE - 1 };
        entry = face(exit_axis, -step);
    }
    // Still going after its longest path: it stops where it is.
    if inside(cell) {
        let l = lin(cell);
        for b in 0..4 { if e[b] > 0 { caught.vol(b, l, e[b]) } }
        return 0;
    }
    e.iter().sum()
}

/// What a cell emits this tick, exactly (no sampling).
struct Emission {
    /// Isotropic, inside the cell, per band.
    vol: [i64; BANDS],
    /// Diffuse, out of each exposed face (opaque cells), per band.
    faces: [[i64; BANDS]; 6],
    from_solid: i64,
    from_gas: i64,
    glow: [i64; 3],
    decay_heat: i64,
}

/// The blackbody emission `energy` at `t`, per band, weighted by how well the
/// emitter emits there (1 − albedo, Kirchhoff): what the packets carry on average.
fn bands_of(energy: i64, t: Temperature, albedo: [u64; 4]) -> [i64; 4] {
    if energy <= 0 { return [0; 4] }
    let shares = tables().bands[k_index(t.0)];
    core::array::from_fn(|b| Fraction(Fraction(Fraction::ONE.0 - albedo[b]).of(Fraction(shares[b]).of(energy)) as u64).0 as i64)
}

fn emit(w: &World, i: usize, x: usize, y: usize, z: usize) -> Option<Emission> {
    let c = &w.cells[i];
    let subs = &w.subs;
    let mut out = Emission { vol: [0; BANDS], faces: [[0; BANDS]; 6], from_solid: 0, from_gas: 0, glow: [0; 3], decay_heat: 0 };
    if c.nuclear.0 > 0 {
        let s = &subs[c.substance as usize];
        let decayed = ((c.nuclear.0 as u128 * s.decay_per_tick as u128) >> 64) as i64;
        let pen = s.penetrating.of(decayed);
        out.decay_heat = decayed - pen;
        out.vol[IONISING] = pen;
    }
    let opaque = c.opaque(subs);
    let faces = if opaque { exposed(w, x, y, z) } else { 6 };
    if faces == 0 { return (out.vol[IONISING] != 0 || out.decay_heat != 0).then_some(out) }
    let mut bands = [0i64; 4]; // what goes as volume or face emission, per band
    for e in emitters(c, subs, faces, w.ambient) {
        let net = net_sigma(e.t, w.ambient) as i128;
        if net <= 0 { continue }
        let power = (net * e.area_q16 / ONE as i128 * FACE_MM2 as i128 / 1_000_000) as i64;
        let energy = (Power(power) * TICK).0.min(e.most);
        let sent = bands_of(energy, e.t, e.albedo);
        let total: i64 = sent.iter().sum();
        if e.from_gas { out.from_gas += total } else { out.from_solid += total }
        for b in 0..4 { bands[b] += sent[b] }
    }
    if c.flame.0 > 0 {
        let flame = c.flame.0.min((c.energy - c.latent()).0.max(0));
        let sent = bands_of(flame, crate::chem::FLAME_GLOW, [0; 4]);
        out.from_gas += sent.iter().sum::<i64>();
        for b in 0..4 { bands[b] += sent[b] }
    }
    for b in 1..4 { out.glow[b - 1] = bands[b] }
    if opaque {
        // Out of the exposed faces, shared equally (the first takes the remainder).
        let mut open = [false; 6];
        for f in 0..6 {
            let (a, s) = normal(f);
            let mut p = [x as i64, y as i64, z as i64];
            p[a] += s;
            open[f] = if w.inside(p[0], p[1], p[2]) { !w.get(p[0] as usize, p[1] as usize, p[2] as usize).opaque(subs) } else { !(a == 2 && s < 0) };
        }
        let n = faces;
        for b in 0..4 {
            let each = bands[b] / n;
            let mut left = bands[b];
            let mut first = true;
            for f in 0..6 {
                if !open[f] { continue }
                let part = if first { bands[b] - each * (n - 1) } else { each };
                first = false;
                left -= part;
                out.faces[f][b] = part;
            }
            debug_assert_eq!(left, 0);
        }
    } else {
        for b in 0..4 { out.vol[b] += bands[b] }
    }
    let nothing = out.from_solid == 0 && out.from_gas == 0 && out.decay_heat == 0 && out.vol[IONISING] == 0;
    (!nothing).then_some(out)
}

/// FV_FIELD_PROFILE set: print where a tick's time goes (stderr; changes nothing).
fn profiling() -> bool {
    static P: OnceLock<bool> = OnceLock::new();
    *P.get_or_init(|| std::env::var_os("FV_FIELD_PROFILE").is_some())
}

/// One tick of radiation as a field.
pub fn radiate(w: &mut World, sky: &Sky, light: &mut Light) {
    let prof = profiling();
    let t0 = std::time::Instant::now();
    let mark = |what: &str| if prof { eprintln!("  {what}: {:.2} ms", t0.elapsed().as_secs_f64() * 1e3) };
    let g = Grid::build(w);
    mark("grid");
    let n = g.n();
    let cells = w.cells.len();
    let scratch = light.field.get_or_insert_with(|| Box::new(Scratch { n, src: Sources::new(n), next: Sources::new(n), caught: Caught::new(n) }));
    if scratch.n != n { **scratch = Scratch { n, src: Sources::new(n), next: Sources::new(n), caught: Caught::new(n) } }
    let Scratch { src, next, caught, .. } = &mut **scratch;
    src.clear();

    // 1. Emission, exact.
    let mut emitted = vec![[0i64; 2]; cells];
    let mut decay_heat = vec![0i64; cells];
    let mut out_total = 0i64;
    let mut in_total = 0i64;
    for v in light.glow.iter_mut() { *v = [0; 3] }
    let layer = |z: usize| -> Vec<(usize, Emission)> {
        let mut out = Vec::new();
        for l in z * g.strides[2]..(z + 1) * g.strides[2] {
            let [x, y, z] = g.xyz(l);
            if let Some(e) = emit(w, g.idx[l] as usize, x, y, z) { out.push((l, e)) }
        }
        out
    };
    #[cfg(feature = "parallel")]
    let layers: Vec<Vec<(usize, Emission)>> = { use rayon::prelude::*; (0..g.dims[2]).into_par_iter().map(layer).collect() };
    #[cfg(not(feature = "parallel"))]
    let layers: Vec<Vec<(usize, Emission)>> = (0..g.dims[2]).map(layer).collect();
    for (l, e) in layers.into_iter().flatten() {
        let i = g.idx[l] as usize;
        emitted[i] = [e.from_solid, e.from_gas];
        decay_heat[i] = e.decay_heat;
        light.glow[i] = e.glow;
        for b in 0..BANDS { src.add_vol(b, l, e.vol[b]) }
        for f in 0..6 {
            if e.faces[f].iter().all(|&v| v == 0) { continue }
            match g.across(l, f) {
                Some(nl) => for b in 0..BANDS { src.add_face(b, nl, f, e.faces[f][b]) },
                None => out_total += e.faces[f].iter().sum::<i64>(),
            }
        }
    }

    mark("emission");
    // 2. The sky: direct sun as rays, skylight into the field at the top.
    let top = g.dims[2] - 1;
    if sky.sun_w_m2 > 0 || sky.sky_w_m2 > 0 {
        let sun_bands = tables().bands[5_778];
        let cos_z = (-sky.sun_dir[2]).max(0);
        let sun_col = (Power(sky.sun_w_m2 * FACE_MM2 * cos_z / ONE) * TICK).0;
        let sky_col = (Power(sky.sky_w_m2 * FACE_MM2) * TICK).0;
        // Skylight: bluer than the sun (the packets' average: 45 % infrared,
        // 15 % red, 17 % green, 23 % blue).
        let sky_split = { let r = sky_col * 15 / 100; let gr = sky_col * 17 / 100; let bl = sky_col * 23 / 100; [sky_col - r - gr - bl, r, gr, bl] };
        let mut beams: Vec<([i64; 3], usize, [i64; 4])> = Vec::new();
        for y in 0..g.dims[1] { for x in 0..g.dims[0] {
            let l = (top * g.dims[1] + y) * g.dims[0] + x;
            if sun_col > 0 {
                let e: [i64; 4] = core::array::from_fn(|b| Fraction(sun_bands[b]).of(sun_col));
                in_total += e.iter().sum::<i64>();
                beams.push(([x as i64 * ONE + ONE / 2, y as i64 * ONE + ONE / 2, g.dims[2] as i64 * ONE - 1], face(2, 1), e));
            }
            if sky_col > 0 {
                in_total += sky_col;
                for b in 0..4 {
                    if g.opaque[l] { caught.hit(b, l, face(2, 1), sky_split[b]) } else { src.add_face(b, l, face(2, -1), sky_split[b]) }
                }
            }
        }}
        // Low sun also enters through the box's sunward sides, through open air.
        if sky.sun_w_m2 > 0 {
            for axis in 0..2 {
                let toward = sky.sun_dir[axis];
                if toward == 0 { continue }
                let e_face = (Power(sky.sun_w_m2 * FACE_MM2 * toward.abs() / ONE) * TICK).0;
                let e: [i64; 4] = core::array::from_fn(|b| Fraction(sun_bands[b]).of(e_face));
                let edge = if toward > 0 { 0 } else { g.dims[axis] - 1 };
                let other = 1 - axis;
                for z in 0..g.dims[2] { for o in 0..g.dims[other] {
                    let mut p = [0usize; 3];
                    p[axis] = edge; p[other] = o; p[2] = z;
                    let l = (p[2] * g.dims[1] + p[1]) * g.dims[0] + p[0];
                    if g.opaque[l] { continue }
                    in_total += e.iter().sum::<i64>();
                    let mut pos = [p[0] as i64 * ONE + ONE / 2, p[1] as i64 * ONE + ONE / 2, p[2] as i64 * ONE + ONE / 2];
                    pos[axis] = if toward > 0 { 1 } else { g.dims[axis] as i64 * ONE - 2 };
                    beams.push((pos, face(axis, if toward > 0 { -1 } else { 1 }), e));
                }}
            }
        }
        let c: &Caught = caught;
        #[cfg(feature = "parallel")]
        { use rayon::prelude::*; out_total += beams.par_iter().map(|&(p, f, e)| beam(&g, p, sky.sun_dir, f, e, c)).sum::<i64>(); }
        #[cfg(not(feature = "parallel"))]
        { out_total += beams.iter().map(|&(p, f, e)| beam(&g, p, sky.sun_dir, f, e, c)).sum::<i64>(); }
    }

    mark("sky");
    // 3. Sweeps, then bounces.
    let mut to_solid = vec![0i64; cells];
    let mut to_gas = vec![0i64; cells];
    for v in light.incident.iter_mut() { *v = [0; 3] }
    for v in light.dose.iter_mut() { *v = 0 }
    let nd = dirs().d.len();
    for pass in 0..PASSES {
        // Infrared bounces once (surfaces reflect little of it); light, BOUNCES times.
        let last_for = |b: usize| pass + 1 >= if b == IR { 2 } else { PASSES };
        // Infrared and the colours meet the same κ, so they sweep together.
        const LIGHT: [usize; 4] = [IR, 1, 2, 3];
        let light_ext = extent(&g, src, 0b1111);
        let ion_ext = extent(&g, src, 1 << IONISING);
        if prof { eprintln!("pass {pass}: light {:?}, ionising {:?}", light_ext.as_ref().map(|e| (e.lo, e.hi)), ion_ext.as_ref().map(|e| (e.lo, e.hi))) }
        let jobs: Vec<(usize, bool)> = light_ext.iter().flat_map(|_| (0..nd).map(|k| (k, false)))
            .chain(ion_ext.iter().flat_map(|_| (0..nd).map(|k| (k, true)))).collect();
        {
            let (s, c, gr): (&Sources, &Caught, &Grid) = (src, caught, &g);
            let new_buf = || -> ([Vec<[i64; 4]>; 3], [Vec<[i64; 1]>; 3]) { (core::array::from_fn(|_| vec![[0; 4]; n]), core::array::from_fn(|_| vec![[0; 1]; n])) };
            let run = |bufs: &mut ([Vec<[i64; 4]>; 3], [Vec<[i64; 1]>; 3]), &(k, ion): &(usize, bool)| -> i64 {
                if ion { sweep(gr, k, [IONISING], s, ion_ext.as_ref().unwrap(), c, &mut bufs.1) } else { sweep(gr, k, LIGHT, s, light_ext.as_ref().unwrap(), c, &mut bufs.0) }
            };
            #[cfg(feature = "parallel")]
            {
                use rayon::prelude::*;
                if crate::world::THREADS.load(Relaxed) != 1 {
                    out_total += jobs.par_iter().map_init(new_buf, run).sum::<i64>();
                } else {
                    let mut bufs = new_buf();
                    for j in &jobs { out_total += run(&mut bufs, j) }
                }
            }
            #[cfg(not(feature = "parallel"))]
            {
                let mut bufs = new_buf();
                for j in &jobs { out_total += run(&mut bufs, j) }
            }
        }
        mark("sweeps");
        // What was caught: absorbed, or (before the last pass) sent on as the next pass's sources.
        next.clear();
        for l in 0..n {
            if !core::mem::take(caught.any[l].get_mut()) { continue }
            let i = g.idx[l] as usize;
            let c = &w.cells[i];
            let sub = &w.subs[c.substance as usize];
            for b in 0..BANDS {
                let last = last_for(b);
                let v = core::mem::take(caught.vol[b][l].get_mut());
                if v > 0 {
                    if b == IONISING {
                        light.dose[i] += v;
                        if c.has_solid() { to_solid[i] += v } else { to_gas[i] += v }
                    } else {
                        if b != IR { light.incident[i][b - 1] += v }
                        let (ks, kso, kd) = kappas(c, &w.subs);
                        let total = (ks + kso + kd).max(1) as i128;
                        let vs = (v as i128 * ks as i128 / total) as i64;
                        let vso = (v as i128 * kso as i128 / total) as i64;
                        let vd = v - vs - vso;
                        let rs = if last { 0 } else { Fraction(band_albedo(sub, b)).of(vs) };
                        let rd = if last { 0 } else { Fraction(drop_albedo(b)).of(vd) };
                        to_solid[i] += vs - rs;
                        to_gas[i] += vso + vd - rd;
                        next.add_vol(b, l, rs + rd);
                    }
                }
                for f in 0..6 {
                    let h = core::mem::take(caught.hit[b][l][f].get_mut());
                    if h == 0 { continue }
                    if b != IR && b != IONISING { light.incident[i][b - 1] += h }
                    let r = if last { 0 } else { Fraction(band_albedo(sub, b)).of(h) };
                    to_solid[i] += h - r;
                    match g.across(l, f) {
                        Some(nl) => next.add_face(b, nl, f, r),
                        None => out_total += r, // reflected off the box's edge, back to the sky
                    }
                }
            }
        }
        mark("caught");
        core::mem::swap(src, next);
    }
    let bricks = w.bricks();
    let mut edge = vec![(0i64, 0i64); bricks];
    edge[0] = (in_total, out_total);
    settle(w, &to_solid, &to_gas, &emitted, &decay_heat, &edge);
    mark("settle");
}
