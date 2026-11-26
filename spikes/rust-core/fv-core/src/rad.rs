//! Radiation: one system for heat, light and ionising radiation.
//!
//! Energy leaves matter as **packets**, each a fixed amount of energy in one
//! band: infrared, red, green, blue, or ionising (Monte Carlo radiative
//! transfer; Lucy 1999). A packet travels in a straight line, cell by cell,
//! and is absorbed, reflected or scattered by what it meets, by each
//! material's own coefficients for that band; or it leaves the box and is
//! counted as escaped. Every packet ends up somewhere within the tick, so
//! what was emitted equals what was absorbed plus what escaped, exactly.
//!
//! Sources:
//! - **hot matter** radiates its excess over the surroundings
//!   (εσ(T⁴ − T_ambient⁴)), split between bands by Planck's law: a warm wall
//!   gives almost only infrared, a fire's soot glows red to yellow, the sun
//!   is white. Brightness is energy the source gives up;
//! - **radioactive matter** turns decay energy into heat where it is, and its
//!   penetrating share into ionising packets that pass through air and are
//!   stopped by dense matter;
//! - **the sky**: sunlight and skylight enter at the top of the box (counted
//!   as coming in).
//!
//! The visible light arriving at each cell is kept, per band, as the light
//! the renderer draws. A scene at night is lit only by what the simulation
//! itself emitted.

use crate::cell::Cell;
use crate::rng::{hash, Stream};
use crate::substance::*;
use crate::tables::{k_index, tables};
use crate::units::*;
use crate::world::{Tally, World, BRICK_CELLS, TICK};
use core::sync::atomic::{AtomicI64, Ordering::Relaxed};
use std::sync::OnceLock;

pub const IR: usize = 0;
pub const RED: usize = 1;
pub const GREEN: usize = 2;
pub const BLUE: usize = 3;
pub const IONISING: usize = 4;

/// Smallest infrared packet, µJ (1.5 J). Hot cells send many; warm cells send one now and then.
const QUANTUM: i64 = 1_500_000;
/// Smallest visible-light packet, µJ (0.5 mJ).
const VISIBLE_QUANTUM: i64 = 500;
/// Most packets one cell sends per tick (larger packets beyond this).
const MAX_PER_CELL: i64 = 48;
const MAX_STEPS: usize = 200;
const MAX_BOUNCES: u8 = 4;
const ONE: i64 = 1 << 16;

/// Light from the sky, per tick.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Sky {
    /// Direct sunlight on a surface facing the sun, W/m².
    pub sun_w_m2: i64,
    /// Direction the sunlight travels (Q16 unit vector).
    pub sun_dir: [i64; 3],
    /// Diffuse skylight on a flat surface, W/m².
    pub sky_w_m2: i64,
}

impl Sky {
    pub const NIGHT: Sky = Sky { sun_w_m2: 0, sun_dir: [0, 0, -ONE], sky_w_m2: 0 };
    /// A clear late afternoon: sun ~35° up, from the south-west.
    pub fn afternoon() -> Sky {
        let d = [0.47f64, 0.58, -0.66];
        let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        Sky { sun_w_m2: 700, sun_dir: d.map(|v| (v / n * ONE as f64) as i64), sky_w_m2: 90 }
    }
}

#[derive(Clone, Copy, Debug)]
struct Packet {
    pos: [i64; 3],
    dir: [i64; 3],
    energy: i64,
    band: u8,
    bounces: u8,
    stream: Stream,
}

/// Per-cell results of a tick's radiation, kept between ticks for the renderer.
pub struct Light {
    /// Visible light arriving at each cell this tick, µJ, per band [red, green, blue].
    pub incident: Vec<[i64; 3]>,
    /// Energy each cell emitted this tick (for the renderer's glow), µJ, per band [red, green, blue].
    pub glow: Vec<[i64; 3]>,
    /// Ionising energy absorbed by each cell this tick (dose), µJ.
    pub dose: Vec<i64>,
}

impl Light {
    pub fn new(n: usize) -> Light { Light { incident: vec![[0; 3]; n], glow: vec![[0; 3]; n], dose: vec![0; n] } }
}

struct Trig { cos: Vec<i64>, sin: Vec<i64> }
fn trig() -> &'static Trig {
    static T: OnceLock<Trig> = OnceLock::new();
    T.get_or_init(|| {
        let n = 1024;
        let a = |i: usize| 2.0 * core::f64::consts::PI * i as f64 / n as f64;
        Trig {
            cos: (0..n).map(|i| (libm::cos(a(i)) * ONE as f64).round() as i64).collect(),
            sin: (0..n).map(|i| (libm::sin(a(i)) * ONE as f64).round() as i64).collect(),
        }
    })
}

/// 1 − e^(−τ) for τ = i/256, i < 4096, out of 2³².
fn absorb_table() -> &'static Vec<u64> {
    static T: OnceLock<Vec<u64>> = OnceLock::new();
    T.get_or_init(|| (0..4096).map(|i| Fraction::from_f64(-libm::expm1(-(i as f64) / 256.0)).0).collect())
}

#[inline]
fn isqrt(v: u64) -> u64 {
    if v == 0 { return 0 }
    let mut x = (v as f64).sqrt() as u64; // a starting guess only; corrected exactly below
    while x * x > v { x -= 1 }
    while (x + 1) * (x + 1) <= v { x += 1 }
    x
}

fn isotropic(s: &mut Stream) -> [i64; 3] {
    let z = (s.below(2 * ONE as u64 + 1) as i64) - ONE;
    let r = isqrt(((ONE * ONE - z * z).max(0)) as u64) as i64;
    let a = s.below(1024) as usize;
    let t = trig();
    [r * t.cos[a] / ONE, r * t.sin[a] / ONE, z]
}

/// Cosine-weighted direction leaving a face whose outward normal is `sign` along `axis`.
fn lambert(s: &mut Stream, axis: usize, sign: i64) -> [i64; 3] {
    let u = s.below(ONE as u64) as i64;
    let r = isqrt((u * ONE) as u64) as i64;
    let n = isqrt(((ONE - u) * ONE) as u64) as i64;
    let a = s.below(1024) as usize;
    let t = trig();
    let (p, q) = (r * t.cos[a] / ONE, r * t.sin[a] / ONE);
    let mut d = [0i64; 3];
    d[axis] = n.max(1) * sign;
    d[(axis + 1) % 3] = p;
    d[(axis + 2) % 3] = q;
    d
}

/// How many of a cell's six faces look onto something light passes through.
fn exposed(w: &World, x: usize, y: usize, z: usize) -> i64 {
    let mut n = 0;
    for axis in 0..3 { for dir in [-1i64, 1] {
        let mut p = [x as i64, y as i64, z as i64];
        p[axis] += dir;
        if !w.inside(p[0], p[1], p[2]) { n += i64::from(!(axis == 2 && dir < 0)); continue }
        if !w.get(p[0] as usize, p[1] as usize, p[2] as usize).opaque(&w.subs) { n += 1 }
    }}
    n
}

/// σ(T⁴ − T_ambient⁴), µW/m².
fn net_sigma(t: Temperature, ambient: Temperature) -> i64 {
    let tab = tables();
    (tab.sigma_t4[k_index(t.0)] - tab.sigma_t4[k_index(ambient.0)]).max(0)
}

/// Thermal power radiated in excess of the surroundings, µW, as
/// (from the solid, from the gas's soot).
fn thermal_power(c: &Cell, subs: &[Substance], ambient: Temperature, faces: i64) -> (i64, i64) {
    let s = &subs[c.substance as usize];
    let eps = s.optics.emissivity.0 as i128;
    let one = Fraction::ONE.0 as i128;
    if c.opaque(subs) {
        // A surface: its exposed faces (each 0.01 m²) at the solid's temperature.
        let net = net_sigma(c.solid_temperature(subs), ambient) as i128;
        return ((net * faces as i128 * eps / one / 100) as i64, 0);
    }
    // Thin fuel radiates from its pieces' whole surface (σ·V, the same as
    // 4κV with κ = σ·share/4); soot: 4κV, optically thin. Neither more than
    // a black cube (six faces of 0.01 m²).
    let share = c.solid_share(subs).0 as i128;
    let solid_faces_q16 = (s.surface_per_volume as i128 * share / one * ONE as i128 / 10).min(6 * ONE as i128);
    let soot_faces_q16 = (c.gas[Gas::Soot as usize].0 as i128 * ONE as i128 / 25_000).min(6 * ONE as i128);
    let solid = if c.has_solid() { net_sigma(c.solid_temperature(subs), ambient) as i128 * solid_faces_q16 * eps / one / ONE as i128 / 100 } else { 0 };
    let soot = net_sigma(c.gas_temperature(), ambient) as i128 * soot_faces_q16 / ONE as i128 / 100;
    (solid as i64, soot as i64)
}

/// A cell's emission this tick. Returns (energy the solid sends, energy the
/// gas sends, the visible part per band, decay heat the solid keeps).
/// Packets go into `out`. Deterministic: the same cell, tick and seed always
/// give the same packets.
fn emit(w: &World, i: usize, x: usize, y: usize, z: usize, out: &mut Vec<Packet>) -> (i64, i64, [i64; 3], i64) {
    let c = &w.cells[i];
    let subs = &w.subs;
    let opaque = c.opaque(subs);
    let faces = if opaque { exposed(w, x, y, z) } else { 6 };
    let mut stream = Stream(hash(w.seed, w.tick, i as u64, 0x5ad));
    let mut glow = [0i64; 3];
    // Decay: heat where it happens, the penetrating share as ionising packets.
    let mut local_decay = 0;
    if c.nuclear.0 > 0 {
        let s = &subs[c.substance as usize];
        let decayed = ((c.nuclear.0 as u128 * s.decay_per_tick as u128) >> 64) as i64;
        let pen = s.penetrating.of(decayed);
        local_decay = decayed - pen;
        push_split(out, pen, IONISING as u8, &mut stream, |s| (centre(x, y, z, s), isotropic(s)));
    }
    if faces == 0 { return (0, 0, glow, local_decay) }
    let (p_solid, p_gas) = thermal_power(c, subs, w.ambient, faces);
    let from_solid = send(w, x, y, z, opaque, (Power(p_solid) * TICK).0, c.solid_temperature(subs), &mut stream, &mut glow, out);
    let from_gas = send(w, x, y, z, false, (Power(p_gas) * TICK).0, c.gas_temperature(), &mut stream, &mut glow, out);
    (from_solid, from_gas, glow, local_decay)
}

/// Turns `energy` of thermal emission at `t` into packets; returns how much
/// was actually sent. The energy is split exactly between bands by Planck's
/// law, and each band gets its own packet size: visible light is a tiny
/// share of a fire's output but it is what lights a scene, so it travels as
/// many small packets (millijoules) while infrared goes as fewer big ones.
/// Small amounts go as one packet now and then (on average exactly their
/// emission).
#[allow(clippy::too_many_arguments)]
fn send(w: &World, x: usize, y: usize, z: usize, from_face: bool, energy: i64, t: Temperature, stream: &mut Stream, glow: &mut [i64; 3], out: &mut Vec<Packet>) -> i64 {
    if energy <= 0 { return 0 }
    let shares = tables().bands[k_index(t.0)];
    let mut sent_total = 0;
    let mut left_to_split = energy;
    for band in 0..4 {
        // The last band takes what rounding left, so the split is exact.
        let e_band = if band == 3 { left_to_split } else { Fraction(shares[band]).of(energy) };
        left_to_split -= e_band;
        if e_band <= 0 { continue }
        let quantum = if band == IR { QUANTUM } else { VISIBLE_QUANTUM };
        let (count, size, sent) = if e_band >= quantum * MAX_PER_CELL {
            (MAX_PER_CELL, e_band / MAX_PER_CELL, e_band)
        } else {
            let whole = e_band / quantum;
            let extra = i64::from(stream.below(quantum as u64) < (e_band % quantum) as u64);
            (whole + extra, quantum, (whole + extra) * quantum)
        };
        let mut left = sent;
        for n in 0..count {
            let e = if n == count - 1 { left } else { size };
            left -= e;
            if (1..=3).contains(&band) { glow[band - 1] += e }
            let (pos, dir) = if from_face { face_start(w, x, y, z, stream) } else { (centre(x, y, z, stream), isotropic(stream)) };
            out.push(Packet { pos, dir, energy: e, band: band as u8, bounces: 0, stream: Stream(stream.next()) });
        }
        sent_total += sent;
    }
    sent_total
}

fn pick_band(bands: &[u64; 4], r: u64) -> usize {
    let mut acc = 0u64;
    for (b, share) in bands.iter().enumerate() { acc += share; if r < acc { return b } }
    IR
}

fn centre(x: usize, y: usize, z: usize, s: &mut Stream) -> [i64; 3] {
    [x as i64 * ONE + s.below(ONE as u64) as i64, y as i64 * ONE + s.below(ONE as u64) as i64, z as i64 * ONE + s.below(ONE as u64) as i64]
}

/// Start just outside one of an opaque cell's exposed faces, heading out.
fn face_start(w: &World, x: usize, y: usize, z: usize, s: &mut Stream) -> ([i64; 3], [i64; 3]) {
    let mut options = [(0usize, 0i64); 6];
    let mut n = 0;
    for axis in 0..3 { for dir in [-1i64, 1] {
        let mut p = [x as i64, y as i64, z as i64];
        p[axis] += dir;
        let open = if w.inside(p[0], p[1], p[2]) { !w.get(p[0] as usize, p[1] as usize, p[2] as usize).opaque(&w.subs) } else { !(axis == 2 && dir < 0) };
        if open { options[n] = (axis, dir); n += 1 }
    }}
    let (axis, dir) = options[s.below(n.max(1) as u64) as usize];
    let mut pos = centre(x, y, z, s);
    pos[axis] = if dir > 0 { (p_of(x, y, z, axis) + 1) * ONE + 1 } else { p_of(x, y, z, axis) * ONE - 1 };
    (pos, lambert(s, axis, dir))
}
fn p_of(x: usize, y: usize, z: usize, axis: usize) -> i64 { [x, y, z][axis] as i64 }

/// Split `energy` into packets of one band, from positions/directions `f` gives.
fn push_split(out: &mut Vec<Packet>, energy: i64, band: u8, s: &mut Stream, mut f: impl FnMut(&mut Stream) -> ([i64; 3], [i64; 3])) {
    if energy <= 0 { return }
    let count = (energy / QUANTUM).clamp(1, MAX_PER_CELL);
    let mut left = energy;
    for n in 0..count {
        let e = if n == count - 1 { left } else { energy / count };
        left -= e;
        let (pos, dir) = f(s);
        out.push(Packet { pos, dir, energy: e, band, bounces: 0, stream: Stream(s.next()) });
    }
}

struct Deposits { gas: Vec<AtomicI64>, solid: Vec<AtomicI64>, light: Vec<[AtomicI64; 3]>, dose: Vec<AtomicI64> }

/// What a packet meets in a permeable cell.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Meets { Solid, Gas }

/// What a cell does to a band as light passes through it: attenuation per
/// metre (Q16), and if it interacts, what it met and the albedo of that.
fn volume_optics(c: &Cell, subs: &[Substance], band: usize, s: &mut Stream) -> (i64, Meets, u64) {
    let sub = &subs[c.substance as usize];
    if band == IONISING {
        // Mass attenuation: κ = μ·ρ. (per-mille cm²/g) × (mg in 1 L) → per metre.
        let mass = (c.solid_mass().0 + c.water.0 + c.gas_mass().0) / 1_000;
        let mu = if c.substance == NONE { 60 } else { sub.optics.ionising_cm2_per_g };
        return (mu * mass / 1_000 * ONE / 10_000, if c.has_solid() { Meets::Solid } else { Meets::Gas }, 0);
    }
    let share = c.solid_share(subs).0 as i64;
    let k_solid = sub.surface_per_volume * share / Fraction::ONE.0 as i64 * ONE / 4;
    let k_soot = c.gas[Gas::Soot as usize].0 * ONE / 1_000; // κ = 1 per metre per mg/L
    let k_drop = c.gas[Gas::Droplets as usize].0 * ONE * 15 / 100_000; // 0.15 per metre per mg/L
    let total = k_solid + k_soot + k_drop;
    if total == 0 { return (0, Meets::Gas, 0) }
    // Which of them does it meet? In proportion to each one's attenuation.
    let r = s.below(total as u64) as i64;
    if r < k_solid { return (total, Meets::Solid, sub.optics.albedo[band].0) }
    if r < k_solid + k_soot { return (total, Meets::Gas, 0) }
    (total, Meets::Gas, if band == IR { Fraction::ONE.0 * 2 / 5 } else { Fraction::ONE.0 * 99 / 100 })
}

fn trace(w: &World, mut p: Packet, dep: &Deposits, tally: &mut Tally) {
    let subs = &w.subs;
    let tab = absorb_table();
    let mut cell = p.pos.map(|v| v.div_euclid(ONE));
    for _ in 0..MAX_STEPS {
        if !w.inside(cell[0], cell[1], cell[2]) { tally.radiation_out += p.energy; return }
        let i = w.idx(cell[0] as usize, cell[1] as usize, cell[2] as usize);
        let c = &w.cells[i];
        // Distance to this cell's exit along the ray, in Q16 cell lengths.
        let mut t_exit = i64::MAX;
        let mut exit_axis = 0;
        for a in 0..3 {
            if p.dir[a] == 0 { continue }
            let edge = if p.dir[a] > 0 { (cell[a] + 1) * ONE - p.pos[a] } else { p.pos[a] - cell[a] * ONE + 1 };
            let t = edge * ONE / p.dir[a].abs();
            if t < t_exit { t_exit = t; exit_axis = a }
        }
        let t_exit = t_exit.max(1);
        // Does it interact inside this cell?
        let (k, meets, albedo) = volume_optics(c, subs, p.band as usize, &mut p.stream);
        if k > 0 {
            let tau = (k as i128 * t_exit as i128 / ONE as i128 / 10) as i64; // κ (Q16 /m) × length (cells × 0.1 m)
            let prob = tab[((tau >> 8) as usize).min(4095)];
            if p.stream.chance(prob) {
                if (1..=3).contains(&(p.band as usize)) { dep.light[i][p.band as usize - 1].fetch_add(p.energy, Relaxed); }
                if p.bounces < MAX_BOUNCES && p.stream.chance(albedo) {
                    p.bounces += 1;
                    p.dir = isotropic(&mut p.stream);
                    continue;
                }
                absorb(dep, i, &p, meets);
                return;
            }
        }
        // Move to the next cell.
        let mut next = cell;
        let step = if p.dir[exit_axis] > 0 { 1 } else { -1 };
        next[exit_axis] += step;
        for a in 0..3 { p.pos[a] += p.dir[a] * t_exit / ONE }
        p.pos[exit_axis] = if step > 0 { next[exit_axis] * ONE } else { next[exit_axis] * ONE + ONE - 1 };
        // An opaque cell ahead stops light at its surface (ionising passes into it).
        if p.band as usize != IONISING && w.inside(next[0], next[1], next[2]) {
            let j = w.idx(next[0] as usize, next[1] as usize, next[2] as usize);
            let n = &w.cells[j];
            if n.opaque(subs) {
                let sub = &subs[n.substance as usize];
                if (1..=3).contains(&(p.band as usize)) { dep.light[j][p.band as usize - 1].fetch_add(p.energy, Relaxed); }
                if p.bounces < MAX_BOUNCES && p.stream.chance(sub.optics.albedo[p.band as usize].0) {
                    p.bounces += 1;
                    p.dir = lambert(&mut p.stream, exit_axis, -step);
                    p.pos[exit_axis] = if step > 0 { next[exit_axis] * ONE - 1 } else { next[exit_axis] * ONE + ONE };
                    continue;
                }
                absorb(dep, j, &p, Meets::Solid);
                return;
            }
        }
        cell = next;
    }
    // Still travelling after its longest path: it stops where it is.
    if w.inside(cell[0], cell[1], cell[2]) {
        let i = w.idx(cell[0] as usize, cell[1] as usize, cell[2] as usize);
        let meets = if w.cells[i].opaque(subs) { Meets::Solid } else { Meets::Gas };
        absorb(dep, i, &p, meets);
        return;
    }
    tally.radiation_out += p.energy;
}

#[inline]
fn absorb(dep: &Deposits, i: usize, p: &Packet, meets: Meets) {
    let store = if meets == Meets::Solid { &dep.solid[i] } else { &dep.gas[i] };
    store.fetch_add(p.energy, Relaxed);
    if p.band as usize == IONISING { dep.dose[i].fetch_add(p.energy, Relaxed); }
}

/// Sunlight and skylight entering at the top: one packet per band per column
/// for the sun (fixed direction: no noise), one skylight packet per column.
fn sky_packets(w: &World, sky: &Sky, x: usize, y: usize, out: &mut Vec<Packet>) -> i64 {
    if sky.sun_w_m2 == 0 && sky.sky_w_m2 == 0 { return 0 }
    let mut stream = Stream(hash(w.seed, w.tick, (y * w.nx + x) as u64, 0x5ca));
    let top = w.nz as i64 * ONE - 1;
    let pos = [x as i64 * ONE + stream.below(ONE as u64) as i64, y as i64 * ONE + stream.below(ONE as u64) as i64, top];
    let mut total = 0;
    // Sunlight per column: irradiance on a horizontal 10 cm square = W/m² × 0.01 m² × cos(zenith).
    let cos_z = (-sky.sun_dir[2]).max(0);
    let sun_mj = (Power(sky.sun_w_m2 * 10_000 * cos_z / ONE) * TICK).0;
    let bands = tables().bands[5_778];
    for (b, share) in bands.iter().enumerate() {
        let e = Fraction(*share).of(sun_mj);
        if e <= 0 { continue }
        out.push(Packet { pos, dir: sky.sun_dir, energy: e, band: b as u8, bounces: 0, stream: Stream(stream.next()) });
        total += e;
    }
    // Skylight: bluer, from a random direction across the sky.
    let sky_mj = (Power(sky.sky_w_m2 * 10_000) * TICK).0;
    if sky_mj > 0 {
        let r = stream.below(100);
        let band = if r < 45 { IR } else if r < 60 { RED } else if r < 77 { GREEN } else { BLUE };
        let d = lambert(&mut stream, 2, -1);
        out.push(Packet { pos, dir: d, energy: sky_mj, band: band as u8, bounces: 0, stream: Stream(stream.next()) });
        total += sky_mj;
    }
    total
}

/// One tick of radiation: emit, travel, absorb, and record the light.
pub fn radiate(w: &mut World, sky: &Sky, light: &mut Light) {
    let n = w.cells.len();
    let bricks = w.bricks();
    // 1. Every brick works out its cells' emission and packets.
    let mut packets: Vec<Vec<Packet>> = (0..bricks).map(|_| Vec::new()).collect();
    let mut emitted = vec![[0i64; 2]; n]; // [from solid, from gas]
    let mut decay_heat = vec![0i64; n];
    let mut sky_in = vec![0i64; bricks];
    {
        let wr: &World = w;
        let work = |b: usize, list: &mut Vec<Packet>, em: &mut [[i64; 2]], dh: &mut [i64], glow: &mut [[i64; 3]], skyin: &mut i64| {
            let (ox, oy, oz) = wr.brick_origin(b);
            for l in 0..BRICK_CELLS {
                let (lx, ly, lz) = World::local_xyz(l);
                let (x, y, z) = (ox + lx, oy + ly, oz + lz);
                let i = b * BRICK_CELLS + l;
                let (from_solid, from_gas, g, local) = emit(wr, i, x, y, z, list);
                em[l] = [from_solid, from_gas];
                dh[l] = local;
                glow[l] = g;
                if z == wr.nz - 1 { *skyin += sky_packets(wr, sky, x, y, list) }
            }
        };
        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            packets.par_iter_mut().zip(emitted.par_chunks_mut(BRICK_CELLS)).zip(decay_heat.par_chunks_mut(BRICK_CELLS))
                .zip(light.glow.par_chunks_mut(BRICK_CELLS)).zip(sky_in.par_iter_mut()).enumerate()
                .for_each(|(b, ((((list, em), dh), g), s))| work(b, list, em, dh, g, s));
        }
        #[cfg(not(feature = "parallel"))]
        for (b, ((((list, em), dh), g), s)) in packets.iter_mut().zip(emitted.chunks_mut(BRICK_CELLS)).zip(decay_heat.chunks_mut(BRICK_CELLS))
            .zip(light.glow.chunks_mut(BRICK_CELLS)).zip(sky_in.iter_mut()).enumerate() { work(b, list, em, dh, g, s) }
    }
    // 2. Trace every packet; deposits are integer sums, so any order gives the same totals.
    let dep = Deposits {
        gas: (0..n).map(|_| AtomicI64::new(0)).collect(),
        solid: (0..n).map(|_| AtomicI64::new(0)).collect(),
        light: (0..n).map(|_| [AtomicI64::new(0), AtomicI64::new(0), AtomicI64::new(0)]).collect(),
        dose: (0..n).map(|_| AtomicI64::new(0)).collect(),
    };
    let mut traced = vec![Tally::default(); bricks];
    {
        let wr: &World = w;
        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            packets.par_iter().zip(traced.par_iter_mut()).for_each(|(list, t)| for p in list { trace(wr, *p, &dep, t) });
        }
        #[cfg(not(feature = "parallel"))]
        for (list, t) in packets.iter().zip(traced.iter_mut()) { for p in list { trace(wr, *p, &dep, t) } }
    }
    // 3. Each cell gives up what it emitted and keeps what it absorbed.
    for i in 0..n {
        light.incident[i] = [dep.light[i][0].load(Relaxed), dep.light[i][1].load(Relaxed), dep.light[i][2].load(Relaxed)];
        light.dose[i] = dep.dose[i].load(Relaxed);
    }
    let to_gas: Vec<i64> = dep.gas.iter().map(|a| a.load(Relaxed)).collect();
    let to_solid: Vec<i64> = dep.solid.iter().map(|a| a.load(Relaxed)).collect();
    w.run_phase(|wr, b, out, tally| {
        let base = b * BRICK_CELLS;
        for l in 0..BRICK_CELLS {
            let i = base + l;
            let mut c = wr.cells[i];
            let s = &wr.subs[c.substance as usize];
            if c.nuclear.0 > 0 {
                let decayed = ((c.nuclear.0 as u128 * s.decay_per_tick as u128) >> 64) as i64;
                c.nuclear -= Energy(decayed);
            }
            c.solid_energy += Energy(to_solid[i] - emitted[i][0] + decay_heat[i]);
            c.energy += Energy(to_gas[i] - emitted[i][1]);
            out[l] = c;
        }
        tally.radiation_out += traced[b].radiation_out;
        tally.radiation_in += sky_in[b];
    });
}
