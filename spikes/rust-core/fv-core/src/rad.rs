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
//!   is white. Soot is not grey: it absorbs as κ_λ ∝ λ^−0.83, and by
//!   Kirchhoff's law emits by the same κ_λ, so its light leans further
//!   toward the visible than a grey body's, and smoke stops visible light
//!   more readily than heat. Brightness is energy the source gives up;
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

/// Smallest infrared packet, µJ (1.5 J in a 10 cm cell; scaled with the
/// cell's volume, so it stays the same share of what a cell holds). Hot
/// cells send many; warm cells send one now and then.
const QUANTUM: i64 = 1_500 * CELL_VOLUME_ML;
/// Smallest visible-light packet, µJ (0.5 mJ in a 10 cm cell).
const VISIBLE_QUANTUM: i64 = CELL_VOLUME_ML / 2;
/// Most packets one cell sends per tick (larger packets beyond this).
const MAX_PER_CELL: i64 = 48;
const MAX_STEPS: usize = 200;
const MAX_BOUNCES: u8 = 4;
pub(crate) const ONE: i64 = 1 << 16;

/// Light from the sky, per tick.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Sky {
    /// Direct sunlight on a surface facing the sun, W/m².
    pub sun_w_m2: i64,
    /// Direction the sunlight travels (Q16 unit vector).
    pub sun_dir: [i64; 3],
    /// Diffuse skylight on a flat surface, W/m².
    pub sky_w_m2: i64,
    /// Direct moonlight on a surface facing the moon, µW/m² (0: no moon).
    pub moon_uw_m2: i64,
    /// Direction the moonlight travels (Q16 unit vector).
    pub moon_dir: [i64; 3],
    /// A clear sky sends down the heat radiation of a sky much colder than
    /// the air ([`sky_temperature`]); an overcast one, of cloud at about
    /// the air's temperature.
    pub clear: bool,
}

fn unit_q16(d: [f64; 3]) -> [i64; 3] {
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    d.map(|v| (v / n * ONE as f64) as i64)
}

impl Sky {
    /// A clear night with no moon: only the sky's heat radiation.
    pub const NIGHT: Sky = Sky { sun_w_m2: 0, sun_dir: [0, 0, -ONE], sky_w_m2: 0, moon_uw_m2: 0, moon_dir: [0, 0, -ONE], clear: true };
    /// A clear late afternoon: sun ~35° up, from the south-west.
    pub fn afternoon() -> Sky {
        Sky { sun_w_m2: 700, sun_dir: unit_q16([0.47, 0.58, -0.66]), sky_w_m2: 90, ..Sky::NIGHT }
    }
    /// A clear night under a full moon ~50° up. The full moon is about
    /// 14 magnitudes (×4×10⁵) fainter than the sun (apparent magnitudes
    /// −12.7 and −26.7), so ~3.4 mW/m² above the air, ~2.4 mW/m² through
    /// it: about 0.25 lux, the measured full-moon illuminance (e.g. Kyba et
    /// al. 2017, Sci. Rep. 7, 6765). Moonlight is sunlight reflected by the
    /// moon, slightly redder (see [`MOON_REFLECTANCE`]).
    pub fn moon() -> Sky {
        Sky { moon_uw_m2: 2_400, moon_dir: unit_q16([-0.45, -0.35, -0.77]), ..Sky::NIGHT }
    }
}

/// The moon's reflectance per band [infrared, red, green, blue], in
/// thousandths: it rises with wavelength, ~0.08 at 450 nm, ~0.10 at 550 nm,
/// ~0.12 at 650 nm and more in the near infrared (ROLO lunar model, Kieffer
/// & Stone 2005, Astron. J. 129, 2887). Only the ratios matter here: they
/// turn sunlight's colour into moonlight's.
pub const MOON_REFLECTANCE: [i64; 4] = [160, 120, 100, 80];

/// The sky's effective temperature for heat radiation, for air at
/// `air`: a clear sky radiates like a black body at T_sky = 0.0552·T^1.5
/// (Swinbank 1963, Q. J. R. Meteorol. Soc. 89, 339; T in kelvin), about
/// 18 K below the air at 15 °C; cloud radiates at about the air's
/// temperature.
pub fn sky_temperature(sky: &Sky, air: Temperature) -> Temperature {
    if !sky.clear { return air }
    let t = air.kelvin_f64();
    Temperature((0.0552 * t * t.sqrt() * 1_000.0).round() as i64)
}

#[derive(Clone, Copy, Debug)]
struct Packet {
    pos: [i64; 3],
    dir: [i64; 3],
    energy: i64,
    band: u8,
    bounces: u8,
    /// Temperature of the spectrum it was emitted with, in kelvin: soot's
    /// infrared κ depends on where in the infrared the light lies.
    kelvin: u16,
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
    /// Working space for the field method, kept between ticks.
    pub(crate) field: Option<Box<crate::field::Scratch>>,
}

impl Light {
    pub fn new(n: usize) -> Light { Light { incident: vec![[0; 3]; n], glow: vec![[0; 3]; n], dose: vec![0; n], field: None } }
}

/// How radiation travels. Both keep the same books and give the same kind
/// of [`Light`]; they differ in how light gets from where it is emitted to
/// where it lands.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Radiation {
    /// Monte Carlo packets (this file).
    #[cfg_attr(not(feature = "field-light"), default)]
    Packets,
    /// A field of radiant energy in fixed directions (discrete ordinates,
    /// [`crate::field`]).
    #[cfg_attr(feature = "field-light", default)]
    Field,
}

/// One tick of radiation, by the chosen method.
pub fn radiate_by(mode: Radiation, w: &mut World, sky: &Sky, light: &mut Light) {
    match mode {
        Radiation::Packets => radiate(w, sky, light),
        Radiation::Field => crate::field::radiate(w, sky, light),
    }
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
pub(crate) fn absorb_table() -> &'static Vec<u64> {
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
pub(crate) fn exposed(w: &World, x: usize, y: usize, z: usize) -> i64 {
    let mut n = 0;
    for axis in 0..3 { for dir in [-1i64, 1] {
        let mut p = [x as i64, y as i64, z as i64];
        p[axis] += dir;
        if !w.inside(p[0], p[1], p[2]) { n += i64::from(!(axis == 2 && dir < 0)); continue }
        if !w.get(p[0] as usize, p[1] as usize, p[2] as usize).opaque(&w.subs) { n += 1 }
    }}
    n
}

/// How much of a band a material reflects or scatters (the rest it
/// absorbs). Infrared follows the material's emissivity, so by Kirchhoff's
/// law it emits exactly as well as it absorbs: matter that soaked up more
/// heat radiation than it could give off would heat without limit.
pub fn band_albedo(s: &Substance, band: usize) -> u64 {
    if band == IR { return Fraction::ONE.0 - s.optics.emissivity.0 }
    s.optics.albedo[band].0
}

/// Albedo of water drops: they scatter light almost entirely and absorb
/// about 60 % of infrared.
pub(crate) fn drop_albedo(band: usize) -> u64 { if band == IR { Fraction::ONE.0 * 2 / 5 } else { Fraction::ONE.0 * 99 / 100 } }

/// One part of a cell that radiates: its area (in cell faces, Q16), its
/// temperature, its albedo per band, and which store it draws from.
/// `most`: the most it may send this tick (µJ): half the heat its store holds
/// above the surroundings. A thin hot layer radiating for a whole tick at its
/// starting temperature could otherwise send more than it has.
/// `water`: open water, sending up from its surface (1) or down from its
/// bottom into its bed (−1); 0 for everything else.
pub(crate) struct Emitter { pub area_q16: i128, pub t: Temperature, pub albedo: [u64; 4], pub from_gas: bool, pub most: i64, pub soot: bool, pub water: i8 }

/// How open water treats a band: what it reflects of what it meets (the
/// rest it absorbs). Infrared by its emissivity; visible light it barely
/// scatters, and so neither does it glow (Kirchhoff).
pub fn water_albedo(band: usize) -> u64 { if band == IR { Fraction::ONE.0 - WATER_EMISSIVITY.0 } else { 0 } }

/// Absorption of open water, Q16 per metre of water, per band.
pub(crate) fn water_kappa(band: usize) -> i64 { WATER_KAPPA_MILLI_PER_M[band] * ONE / 1_000 }

/// Optical depth (Q16) of the open water lying in a cell's bottom, along a
/// ray that starts at height `z0` in the cell (Q16 of its height), rises
/// `dz` and runs `len` (both Q16 cell lengths): only the part of the path
/// below the water's surface passes through water.
pub(crate) fn water_tau(c: &Cell, band: usize, z0: i64, dz: i64, len: i64) -> i64 {
    let h = c.water_depth_q16();
    if h == 0 || band >= 4 { return 0 }
    let (lo, hi) = (z0.min(z0 + dz), z0.max(z0 + dz));
    if lo >= h { return 0 }
    let in_water = if hi <= h { len as i128 } else { div_i128(len as i128 * (h - lo) as i128, (hi - lo).max(1) as i128) };
    div_i128(div_i128(water_kappa(band) as i128 * in_water, ONE as i128) * CELL_MM as i128, 1_000).min(i64::MAX as i128 / 2) as i64
}

impl Emitter {
    /// How its emission splits between bands: soot's own spectrum, or grey.
    pub fn shares(&self) -> [u64; 4] { if self.soot { soot_shares(self.t) } else { grey_shares(self.t) } }
    /// What it sends this tick (µJ) before its albedo: σT⁴ over its area
    /// (all of it: what comes back from its surroundings, the sky included,
    /// arrives as radiation too), and for soot times κ_P(T)/κ_ref, its
    /// Planck-mean κ against the 1 per metre per mg/L its area is counted
    /// in, so the sum over bands is 4V∫κ_λB_λ dλ.
    pub fn energy(&self, _ambient: Temperature) -> i64 {
        let full = tables().sigma_t4[k_index(self.t.0)] as i128;
        if full <= 0 { return 0 }
        // Blackbody power over this area (µW): µW/m² × faces × face area.
        let mut power = div_i128(div_i128(full * self.area_q16, ONE as i128) * FACE_MM2 as i128, 1_000_000);
        if self.soot { power = power * soot_planck_mean(self.t) as i128 >> 32 }
        (Power(power as i64) * TICK).0.min(self.most)
    }
}

/// Soot's Planck-mean κ at `t` against κ_ref = 1 per metre per mg/L, out of 2³².
pub(crate) fn soot_planck_mean(t: Temperature) -> u64 { tables().soot_emission[k_index(t.0)].iter().sum() }

/// Soot's κ in band `band` for light from a source at `kelvin`, Q16 per
/// metre per µg of soot per mL (= mg/L). See [`crate::tables::SOOT_C`].
#[inline]
pub(crate) fn soot_kappa(band: usize, kelvin: usize) -> i64 {
    let t = tables();
    if band == IR { t.soot_kappa_ir[kelvin.min(crate::tables::MAX_K)] } else { t.soot_kappa_vis[band - 1] }
}

/// Kelvin of a temperature, as packets carry it.
#[inline]
fn kelvin(t: Temperature) -> u16 { k_index(t.0) as u16 }
/// The sun's surface, for sunlight and skylight.
const SUN_K: u16 = 5_778;

/// Planck's split between bands for a grey body at `t`.
pub(crate) fn grey_shares(t: Temperature) -> [u64; 4] { tables().bands[k_index(t.0)] }
/// The split for glowing soot at `t` (see [`crate::tables::Tables::soot_bands`]).
pub(crate) fn soot_shares(t: Temperature) -> [u64; 4] { tables().soot_bands[k_index(t.0)] }

/// The radiating parts of a cell. A surface radiates from its exposed
/// faces; airy matter radiates from 4κV (the same κ light is stopped by):
/// blades (σ·share/4 per metre), soot (counted at κ_ref = 1 per metre per
/// mg/L, its Planck mean at 1 400 K; [`Emitter::energy`] scales it to its
/// own temperature) and water drops
/// (0.15 per metre per mg/L), none more than a black cube (six faces).
pub(crate) fn emitters(c: &Cell, subs: &[Substance], faces: i64, ambient: Temperature) -> Vec<Emitter> {
    let s = &subs[c.substance as usize];
    // What the surface store and the gas hold above the surroundings, halved
    // (the gas's shared between soot and drops).
    let surface_cap = { let k = c.skin_capacity(subs); if k.0 > 0 { k } else { c.solid_capacity(subs) } };
    let surface_store = if c.skin_capacity(subs).0 > 0 { c.skin } else { c.solid_energy };
    let _ = (surface_cap, ambient);
    let solid_most = (surface_store.0 / 2).max(0);
    let gas_most = ((c.energy - c.latent()).0 / 4).max(0);
    let one = Fraction::ONE.0 as i128;
    let solid_albedo = [0, 1, 2, 3].map(|b| band_albedo(s, b));
    if c.opaque(subs) {
        return vec![Emitter { area_q16: faces as i128 * ONE as i128, t: c.surface_temperature(subs), albedo: solid_albedo, from_gas: false, most: solid_most, soot: false, water: 0 }];
    }
    let cap = 6 * ONE as i128;
    let share = c.solid_share(subs).0 as i128;
    let mut out = Vec::new();
    if c.open_water() {
        // Open water radiates from its surface (the cell's floor area, one
        // face) and, as any layer does, from its bottom into the bed it lies
        // on, which radiates back into it: the two nearly cancel at the same
        // temperature, and contact carries the rest.
        let albedo = [0, 1, 2, 3].map(water_albedo);
        let t = c.surface_temperature(subs);
        out.push(Emitter { area_q16: ONE as i128, t, albedo, from_gas: false, most: solid_most / 2, soot: false, water: 1 });
        out.push(Emitter { area_q16: ONE as i128, t, albedo, from_gas: false, most: solid_most / 2, soot: false, water: -1 });
    } else if c.has_solid() {
        // 4κV = σ·share·L³ = σ·share·L faces (L in m: 0.1 at 10 cm).
        out.push(Emitter { area_q16: div_i128(div_i128(s.surface_per_volume as i128 * share, one) * ONE as i128 * CELL_MM as i128, 1_000).min(cap), t: c.surface_temperature(subs), albedo: solid_albedo, from_gas: false, most: solid_most, soot: false, water: 0 });
    }
    let tg = c.gas_temperature();
    // Soot: 4κV = 4 × (1 per m per mg/L) × mg/V × V = 0.004 m² per mg,
    // whatever the cell: 0.4 faces of 0.01 m² per mg = µg/2500 at 10 cm.
    let soot = div_i128(c.gas[Gas::Soot as usize].0 as i128 * ONE as i128, 2_500 * FACE_MM2 as i128 / 10_000);
    if soot > 0 { out.push(Emitter { area_q16: soot.min(cap), t: tg, albedo: [0; 4], from_gas: true, most: gas_most, soot: true, water: 0 }) }
    // Drops: 4 × 0.15 per m per mg/L: 0.0006 m² per mg = µg/16 667 faces at 10 cm.
    let drops = div_i128(c.gas[Gas::Droplets as usize].0 as i128 * ONE as i128, 16_667 * FACE_MM2 as i128 / 10_000);
    if drops > 0 { out.push(Emitter { area_q16: drops.min(cap), t: tg, albedo: [0, 1, 2, 3].map(drop_albedo), from_gas: true, most: gas_most, soot: false, water: 0 }) }
    out
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
    let (mut from_solid, mut from_gas) = (0, 0);
    for e in emitters(c, subs, faces, w.ambient) {
        let energy = e.energy(w.ambient);
        if energy <= 0 { continue }
        let start = if e.water > 0 { Start::WaterTop(c.water_depth_q16()) } else if e.water < 0 { Start::WaterBottom } else if opaque && !e.from_gas { Start::Face } else { Start::Volume };
        let sent = send(w, x, y, z, start, energy, e.most, e.shares(), e.albedo, kelvin(e.t), &mut stream, &mut glow, out);
        if e.from_gas { from_gas += sent } else { from_solid += sent }
    }
    // A flame's own light, set from the heat it released (see chem).
    if c.flame.0 > 0 {
        // At most the gas's heat, less what its soot and drops already sent.
        let flame = c.flame.0.min(((c.energy - c.latent()).0 - from_gas).max(0));
        from_gas += send(w, x, y, z, Start::Volume, flame, (c.energy - c.latent()).0.max(0) / 2, soot_shares(crate::chem::FLAME_GLOW), [0; 4], kelvin(crate::chem::FLAME_GLOW), &mut stream, &mut glow, out);
    }
    (from_solid, from_gas, glow, local_decay)
}

/// Turns `energy` of emission, split between bands by `shares`, into packets, each band
/// weighted by how well the emitter emits there (`1 − albedo`); returns how
/// much was actually sent. Each band gets its own packet size: visible light is a tiny
/// share of a fire's output but it is what lights a scene, so it travels as
/// many small packets (millijoules) while infrared goes as fewer big ones.
/// Small amounts go as one packet now and then (on average exactly their
/// emission).
/// Where a source's packets set out from.
#[derive(Copy, Clone)]
enum Start {
    /// Anywhere in the cell, any direction (airy matter, flames).
    Volume,
    /// Just outside one of an opaque cell's exposed faces.
    Face,
    /// From the surface of open water this deep (Q16 of the cell), upward.
    WaterTop(i64),
    /// From the bottom of open water, downward into its bed.
    WaterBottom,
}

#[allow(clippy::too_many_arguments)]
/// `budget`: the most the store can give. Below a whole packet, a band
/// sends one packet by chance (fair on average); it only takes that chance
/// when the store can afford the whole packet, so a nearly empty store can't
/// go negative.
fn send(w: &World, x: usize, y: usize, z: usize, start: Start, energy: i64, budget: i64, shares: [u64; 4], albedo: [u64; 4], kelvin: u16, stream: &mut Stream, glow: &mut [i64; 3], out: &mut Vec<Packet>) -> i64 {
    if energy <= 0 { return 0 }
    let mut sent_total = 0;
    for band in 0..4 {
        // Each band: the blackbody share, times how well it emits there
        // (1 − albedo, Kirchhoff).
        let e_band = Fraction(Fraction(Fraction::ONE.0 - albedo[band]).of(Fraction(shares[band]).of(energy)) as u64).0 as i64;
        if e_band <= 0 { continue }
        let quantum = if band == IR { QUANTUM } else { VISIBLE_QUANTUM };
        let (count, size, sent) = if e_band >= quantum * MAX_PER_CELL {
            (MAX_PER_CELL, e_band / MAX_PER_CELL, e_band)
        } else {
            let whole = e_band / quantum;
            let roll = stream.below(quantum as u64) < (e_band % quantum) as u64;
            let affordable = sent_total + (whole + 1) * quantum <= budget;
            let extra = i64::from(roll && affordable);
            (whole + extra, quantum, (whole + extra) * quantum)
        };
        let mut left = sent;
        for n in 0..count {
            let e = if n == count - 1 { left } else { size };
            left -= e;
            if (1..=3).contains(&band) { glow[band - 1] += e }
            let (pos, dir) = match start {
                Start::Face => face_start(w, x, y, z, stream),
                Start::Volume => (centre(x, y, z, stream), isotropic(stream)),
                Start::WaterTop(h) => {
                    let mut pos = centre(x, y, z, stream);
                    pos[2] = z as i64 * ONE + h.clamp(0, ONE - 1);
                    (pos, lambert(stream, 2, 1))
                }
                Start::WaterBottom => {
                    let mut pos = centre(x, y, z, stream);
                    pos[2] = z as i64 * ONE;
                    (pos, lambert(stream, 2, -1))
                }
            };
            out.push(Packet { pos, dir, energy: e, band: band as u8, bounces: 0, kelvin, stream: Stream(stream.next()) });
        }
        sent_total += sent;
    }
    sent_total
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
        out.push(Packet { pos, dir, energy: e, band, bounces: 0, kelvin: 0, stream: Stream(s.next()) });
    }
}

struct Deposits { gas: Vec<AtomicI64>, solid: Vec<AtomicI64>, light: Vec<[AtomicI64; 3]>, dose: Vec<AtomicI64> }

/// What a packet meets in a permeable cell.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Meets { Solid, Gas }

/// What a cell does to a band as light passes through it: attenuation per
/// metre (Q16), and if it interacts, what it met and the albedo of that.
/// `kelvin`: the temperature of the light's source (soot's infrared κ).
fn volume_optics(c: &Cell, subs: &[Substance], band: usize, kelvin: usize, s: &mut Stream) -> (i64, Meets, u64) {
    let sub = &subs[c.substance as usize];
    if band == IONISING {
        // Mass attenuation: κ = μ·ρ. (per-mille cm²/g) × (mg per L) → per metre.
        let mass = (c.solid_mass().0 + c.water.0 + c.gas_mass().0) / CELL_VOLUME_ML;
        let mu = if c.substance == NONE { 60 } else { sub.optics.ionising_cm2_per_g };
        return (mu * mass / 1_000 * ONE / 10_000, if c.has_solid() { Meets::Solid } else { Meets::Gas }, 0);
    }
    let share = c.solid_share(subs).0 as i64;
    let k_solid = sub.surface_per_volume * share / Fraction::ONE.0 as i64 * ONE / 4;
    let soot = c.gas[Gas::Soot as usize].0;
    let k_soot = if soot == 0 { 0 } else { soot * soot_kappa(band, kelvin) / CELL_VOLUME_ML };
    let k_drop = c.gas[Gas::Droplets as usize].0 * ONE * 15 / (100 * CELL_VOLUME_ML); // 0.15 per metre per mg/L
    let total = k_solid + k_soot + k_drop;
    if total == 0 { return (0, Meets::Gas, 0) }
    // Which of them does it meet? In proportion to each one's attenuation.
    let r = s.below(total as u64) as i64;
    if r < k_solid { return (total, Meets::Solid, band_albedo(sub, band)) }
    if r < k_solid + k_soot { return (total, Meets::Gas, 0) }
    (total, Meets::Gas, drop_albedo(band))
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
        let (k, mut meets, mut albedo) = volume_optics(c, subs, p.band as usize, p.kelvin as usize, &mut p.stream);
        // Open water lying in the cell's bottom: only the path below its surface.
        let tau_water = if c.open_water() && (p.band as usize) < 4 {
            water_tau(c, p.band as usize, p.pos[2] - cell[2] * ONE, p.dir[2] * t_exit / ONE, t_exit)
        } else { 0 };
        if k > 0 || tau_water > 0 {
            let tau_volume = div_i128(div_i128(k as i128 * t_exit as i128, ONE as i128) * CELL_MM as i128, 1_000) as i64; // κ (Q16 /m) × length (cells × edge in m)
            let tau = tau_volume.saturating_add(tau_water);
            let prob = tab[((tau >> 8) as usize).min(4095)];
            if tau_water > 0 && p.stream.below(tau as u64) < tau_water as u64 {
                // What it met was the water.
                meets = Meets::Solid;
                albedo = water_albedo(p.band as usize);
            }
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
                if p.bounces < MAX_BOUNCES && p.stream.chance(band_albedo(sub, p.band as usize)) {
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
    let mut stream = Stream(hash(w.seed, w.tick, (y * w.nx + x) as u64, 0x5ca));
    let top = w.nz as i64 * ONE - 1;
    let pos = [x as i64 * ONE + stream.below(ONE as u64) as i64, y as i64 * ONE + stream.below(ONE as u64) as i64, top];
    let mut total = 0;
    // Sunlight per column: irradiance on a horizontal cell face = W/m² × face × cos(zenith).
    let cos_z = (-sky.sun_dir[2]).max(0);
    let sun_mj = (Power(sky.sun_w_m2 * FACE_MM2 * cos_z / ONE) * TICK).0;
    let bands = tables().bands[5_778];
    for (b, share) in bands.iter().enumerate() {
        let e = Fraction(*share).of(sun_mj);
        if e <= 0 { continue }
        out.push(Packet { pos, dir: sky.sun_dir, energy: e, band: b as u8, bounces: 0, kelvin: SUN_K, stream: Stream(stream.next()) });
        total += e;
    }
    // Skylight: bluer, from a random direction across the sky.
    let sky_mj = (Power(sky.sky_w_m2 * FACE_MM2) * TICK).0;
    if sky_mj > 0 {
        let r = stream.below(100);
        let band = if r < 45 { IR } else if r < 60 { RED } else if r < 77 { GREEN } else { BLUE };
        let d = lambert(&mut stream, 2, -1);
        out.push(Packet { pos, dir: d, energy: sky_mj, band: band as u8, bounces: 0, kelvin: SUN_K, stream: Stream(stream.next()) });
        total += sky_mj;
    }
    // Moonlight: sunlight's bands × the moon's reflectance, a few µJ per
    // column per tick, rounded by chance to whole µJ (fair on average).
    if sky.moon_uw_m2 > 0 {
        let cos_z = (-sky.moon_dir[2]).max(0) as i128;
        let nj = sky.moon_uw_m2 as i128 * FACE_MM2 as i128 * cos_z / ONE as i128 * TICK.0 as i128 / 1_000_000; // nJ per tick
        let weight: [i128; 4] = core::array::from_fn(|b| Fraction(tables().bands[5_778][b]).of(1_000_000) as i128 * MOON_REFLECTANCE[b] as i128);
        let sum: i128 = weight.iter().sum();
        for b in 0..4 {
            let part = nj * weight[b] / sum.max(1);
            let e = (part / 1_000) as i64 + i64::from(stream.below(1_000) < (part % 1_000) as u64);
            if e <= 0 { continue }
            out.push(Packet { pos, dir: sky.moon_dir, energy: e, band: b as u8, bounces: 0, kelvin: SUN_K, stream: Stream(stream.next()) });
            total += e;
        }
    }
    // The sky's own heat radiation, diffuse, at its effective temperature.
    let t_sky = sky_temperature(sky, w.ambient);
    let ir = (Power(tables().sigma_t4[k_index(t_sky.0)] * FACE_MM2 / 1_000_000) * TICK).0;
    if ir > 0 {
        let d = lambert(&mut stream, 2, -1);
        out.push(Packet { pos, dir: d, energy: ir, band: IR as u8, bounces: 0, kelvin: kelvin(t_sky), stream: Stream(stream.next()) });
        total += ir;
    }
    total
}

/// Heat radiation from the surroundings beyond the box's sides, at the air's
/// temperature (the land around continues at it), onto edge cell (x, y, z)
/// through its outward side faces. An open cell gets it as packets that
/// travel in; an opaque cell absorbs its share (its emissivity) at its
/// face, returned as the second value. Returns (energy sent in as packets,
/// energy absorbed at faces).
fn surroundings(w: &World, sky: &Sky, i: usize, x: usize, y: usize, z: usize, out: &mut Vec<Packet>) -> (i64, i64) {
    let edge_faces: [(usize, i64); 4] = [(0, -1), (0, 1), (1, -1), (1, 1)];
    let c = &w.cells[i];
    let opaque = c.opaque(&w.subs);
    let e_face = (Power(tables().sigma_t4[k_index(w.ambient.0)] * FACE_MM2 / 1_000_000) * TICK).0;
    let t_sky = sky_temperature(sky, w.ambient);
    let e_sky = (Power(tables().sigma_t4[k_index(t_sky.0)] * FACE_MM2 / 1_000_000) * TICK).0;
    let (mut sent, mut absorbed) = (0, 0);
    let mut stream = Stream(hash(w.seed, w.tick, i as u64, 0x5d6));
    for (axis, dir) in edge_faces {
        let p = [x as i64, y as i64][axis] + dir;
        if p >= 0 && p < [w.nx, w.ny][axis] as i64 { continue }
        if opaque {
            let s = &w.subs[c.substance as usize];
            absorbed += Fraction(Fraction::ONE.0 - band_albedo(s, IR)).of(e_face);
        } else {
            let mut pos = centre(x, y, z, &mut stream);
            pos[axis] = if dir < 0 { [x, y][axis] as i64 * ONE } else { ([x, y][axis] as i64 + 1) * ONE - 1 };
            // Through open air, what heads down came from the sky beyond
            // and what heads up from the land beyond (at the air's
            // temperature): the two halves of a side face's view.
            let d = lambert(&mut stream, axis, -dir);
            let (e, k) = if d[2] < 0 { (e_sky, kelvin(t_sky)) } else { (e_face, kelvin(w.ambient)) };
            out.push(Packet { pos, dir: d, energy: e, band: IR as u8, bounces: 0, kelvin: k, stream: Stream(stream.next()) });
            sent += e;
        }
    }
    (sent, absorbed)
}

/// Direct sunlight entering through a side face of the box (an open-air
/// cell on the sunward edge): W/m² × face × the sun's angle to that face.
fn side_sun(w: &World, sky: &Sky, x: usize, y: usize, z: usize, axis: usize, out: &mut Vec<Packet>) -> i64 {
    if sky.sun_w_m2 == 0 { return 0 }
    let mut stream = Stream(hash(w.seed, w.tick, w.idx(x, y, z) as u64, 0x5ce + axis as u64));
    let cos = sky.sun_dir[axis].abs();
    let e_total = (Power(sky.sun_w_m2 * FACE_MM2 * cos / ONE) * TICK).0;
    let mut pos = [x as i64 * ONE + stream.below(ONE as u64) as i64, y as i64 * ONE + stream.below(ONE as u64) as i64, z as i64 * ONE + stream.below(ONE as u64) as i64];
    pos[axis] = if sky.sun_dir[axis] > 0 { 1 } else { [w.nx, w.ny][axis] as i64 * ONE - 2 };
    let bands = tables().bands[5_778];
    let mut total = 0;
    for (b, share) in bands.iter().enumerate() {
        let e = Fraction(*share).of(e_total);
        if e <= 0 { continue }
        out.push(Packet { pos, dir: sky.sun_dir, energy: e, band: b as u8, bounces: 0, kelvin: SUN_K, stream: Stream(stream.next()) });
        total += e;
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
                // The surroundings' heat radiation through the box's sides.
                let (sent_in, absorbed) = if x == 0 || y == 0 || x == wr.nx - 1 || y == wr.ny - 1 { surroundings(wr, sky, i, x, y, z, list) } else { (0, 0) };
                *skyin += sent_in + absorbed;
                // Absorbed at the face: the surface gains it (emission less it).
                em[l] = [from_solid - absorbed, from_gas];
                dh[l] = local;
                glow[l] = g;
                if z == wr.nz - 1 { *skyin += sky_packets(wr, sky, x, y, list) }
                // Low sun also crosses the box's sunward sides through open air.
                if !wr.cells[i].opaque(&wr.subs) {
                    for axis in 0..2 {
                        let toward = sky.sun_dir[axis];
                        let at_edge = [x, y][axis] == if toward > 0 { 0 } else { [wr.nx, wr.ny][axis] - 1 };
                        if toward != 0 && at_edge { *skyin += side_sun(wr, sky, x, y, z, axis, list) }
                    }
                }
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
    let edge: Vec<(i64, i64)> = (0..bricks).map(|b| (sky_in[b], traced[b].radiation_out)).collect();
    settle(w, &to_solid, &to_gas, &emitted, &decay_heat, &edge);
}

/// The end of a tick's radiation, whichever way it travelled: each cell
/// gives up what it emitted and keeps what it absorbed (indices are cell
/// indices), decay is taken from the nuclear store, the flame is spent, and
/// `edge[brick]` = (came in, went out) is booked on that brick's tally.
pub(crate) fn settle(w: &mut World, to_solid: &[i64], to_gas: &[i64], emitted: &[[i64; 2]], decay_heat: &[i64], edge: &[(i64, i64)]) {
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
            // Surfaces (opaque cells' outer layer, or airy cells' solid) take
            // what arrived and give what they sent; decay heat stays inside.
            *c.surface_store(&wr.subs) += Energy(to_solid[i] - emitted[i][0]);
            if c.has_solid() { c.solid_energy += Energy(decay_heat[i]) } else { c.energy += Energy(decay_heat[i]) }
            c.energy += Energy(to_gas[i] - emitted[i][1]);
            c.flame = Energy::ZERO;
            out[l] = c;
        }
        if let Some(&(i, o)) = edge.get(b) {
            tally.radiation_out += o;
            tally.radiation_in += i;
        }
    });
}
