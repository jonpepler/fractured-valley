//! Changes inside a cell: the solid and gas trading heat, water drying and
//! condensing, and burning.
//!
//! Nothing here has a threshold set per material. Every rate is a smooth
//! function of temperature (Arrhenius tables) and of what's present, so
//! "ignition" is the point where heating, drying and gas release run away,
//! and it comes out of the physics (decision 4.2).
//!
//! Each change moves amounts between stores and keeps both books exact:
//! - **energy**: thermal energy lives in two stores, the solid's and the
//!   gas's, and chemical energy is counted from the burnable masses, so
//!   burning `m` adds exactly `m × (energy in it − energy in what it became)`
//!   to a store. When matter moves between solid and gas (water evaporating,
//!   wood giving off gas), its sensible heat moves with it;
//! - **mass**: every product's mass is what went in, to the microgram.

use crate::cell::*;
use crate::substance::*;
use crate::tables::{k_index, tables, Tables};
use crate::units::*;
use crate::world::{World, BRICK_CELLS, TICK};

/// Boiling point at sea level.
const BOIL: Temperature = Temperature(373_150);
/// Oxygen per wood gas burnt (CH₁.₆O₀.₇ + 1.05 O₂ → CO₂ + 0.8 H₂O), per mille.
const O2_PER_FUEL: i64 = 1_355;
/// Of the burnt products, the share that's CO₂ by mass (44 / 58.4), per mille.
const CO2_SHARE: i64 = 753;
/// Oxygen per carbon burnt (C + O₂ → CO₂): 32/12.
const O2_PER_CARBON_NUM: i64 = 8;
const O2_PER_CARBON_DEN: i64 = 3;
/// Surface area per volume at which water leaves a fuel as fast as it can
/// (thin blades); thicker fuel dries in proportion to its σ.
const FINE_SIGMA: i64 = 4_000;
/// The lowest flame temperature a fuel–air mix can sustain (FDS default
/// critical flame temperature, 1427 °C).
const CRITICAL_FLAME: Temperature = Temperature(1_700_000);
/// Below this, burnable gas can't catch: a pilot flame lights wood gas off
/// a surface at about 350 °C (Babrauskas, Ignition Handbook, 2003). The
/// gas or the surface it leaves must be this hot.
pub const IGNITION: Temperature = Temperature(623_000);
/// Share of a flame's heat it gives off as light, mostly from glowing soot
/// (FDS's RADIATIVE_FRACTION, 0.35 for wood; Tewarson, SFPE Handbook). A
/// 10 cm cell can't resolve the thin sheet that glows, so its light is set
/// from the heat the flame releases, not from the cell's average temperature.
pub const RADIATIVE_FRACTION: Fraction = Fraction(1_503_238_554); // 0.35
/// Temperature a flame's soot glows at, for the colour of its light: about
/// 1 400 K in wood and other sooty flames. Its light is split between bands
/// by soot's own spectrum (`Tables::soot_bands`), not a grey body's.
///
/// Check against measurement: a flame's light per watt of heat is then
/// 0.35 × 0.12 lm/W ≈ 0.04 lm/W (683·V(λ) over soot's spectrum at 1 400 K),
/// against 0.1–0.2 lm/W for a candle (Wikipedia, "Luminous efficacy"),
/// whose small flame burns hotter: the right size, a little low. With a grey
/// body it was 0.012, ten times too dim.
pub const FLAME_GLOW: Temperature = Temperature(1_400_000);
/// Share of a flammable mix that burns per tick when the flame holds. Fuel
/// and air burn only as fast as they mix (FDS's eddy dissipation concept:
/// rate = min(Y_F, Y_O₂/s)/τ_mix). With no model of a cell's sub-grid
/// turbulence, τ_mix is FDS's slowest bound, the time buoyancy takes to
/// stir a cell: τ_g = √(2Δ/g) (McGrattan et al., FDS Technical Reference
/// Guide, §5.1), 0.143 s at 10 cm, so 1 − e^(−0.05/0.143) = 0.295 a tick;
/// 0.101 s at 5 cm (0.391).
#[cfg(not(feature = "cell-5cm"))]
const MIXING: Fraction = Fraction(1_268_733_232);
#[cfg(feature = "cell-5cm")]
const MIXING: Fraction = Fraction(1_677_276_774);
/// Anderson's flame residence time: τ [s] = this / σ [1/m].
const RESIDENCE_S_PER_SIGMA: i64 = 75_600;
/// Charring rate of a dense solid's exposed face, µm per minute.
const CHAR_UM_PER_MIN: i64 = 1_500;
/// Heat released at the surface when carbon burns to carbon monoxide:
/// 110.5 kJ/mol over 12 g.
const CHAR_TO_CO_HEAT: SpecificEnergy = kj_per_kg(9_200);
/// The outer surface oxygen reaches on a dense solid: one face.
const OUTER_FACE_MM2: i64 = FACE_MM2;
/// Most of a flame's heat that can go straight into the fuel it wraps, and
/// the fuel surface (mm² per cell) at which it's half that: a 10 cm grass
/// tuft (~14 000 mm²) gets ~30 %, open air none. The half-point is a surface
/// per volume (10 000 mm² per litre), so the same fuel gets the same share
/// whatever the cell's size.
const FEEDBACK_MAX: Fraction = Fraction(1_288_490_189); // 30 %
const FEEDBACK_HALF_MM2: i128 = 10 * CELL_VOLUME_ML as i128;
/// Heat transfer between a solid's surface and the gas around it, in
/// mW/(m²·K) per 1/m of σ: h = Nu·k/d with d = 4/σ, Nu ≈ 2, k ≈ 0.05 W/(m·K).
const SURFACE_H_PER_SIGMA: i64 = 25;

/// Heat transfer between open water's surface and the air over it, in
/// mW/(m²·K): about 10 W/(m²·K) in light air (h = 5.7 + 3.8·v W/(m²·K) at
/// v ≈ 1 m/s, McAdams 1954, as used for open water by Watmuff et al. 1977).
const WATER_SURFACE_H: i64 = 10_000;
/// Vapour carried off a water surface: by the Chilton–Colburn analogy
/// h_m = h / (ρ·c_p·Le^⅔) with ρ·c_p ≈ 1 200 J/(m³·K) and Le ≈ 0.85 for
/// water vapour in air, ≈ 9.2 mm/s at the h above (in µm/s). Per tick it
/// sweeps h_m × the water's surface × the tick of air over it: 4.6 mL on a
/// 10 cm cell.
const WATER_TRANSFER_UM_PER_S: i64 = 9_200;
const WATER_SWEPT_UL: i64 = WATER_TRANSFER_UM_PER_S * FACE_MM2 * TICK.0 / 1_000_000;
/// A water surface fills a cell's floor, mm².
const WATER_SURFACE_MM2: i64 = FACE_MM2;

/// Heat transfer from a twig or blade to the gas flowing past it, in the
/// same units as `SURFACE_H_PER_SIGMA`. Hot gas rises through a fuel bed at
/// about √(g·l·ΔT/T) (see `transport::buoyant_churn`), and flow across a
/// cylinder of diameter d = 4/σ carries heat as Nu = 0.3 + 0.62·Re^½·Pr^⅓
/// (Churchill & Bernstein 1977, low-Re form, Pr ≈ 0.7), never below still
/// air's Nu = 2. Kinematic viscosity of air ν ≈ 1.5×10⁻⁵·(T/293 K)^1.75.
pub fn surface_h(sigma: i64, t_gas: Temperature, ambient: Temperature) -> i64 {
    if sigma <= 0 { return SURFACE_H_PER_SIGMA }
    let v = crate::transport::buoyant_churn(t_gas, ambient) as f64 / 1000.0; // m/s
    let d = 4.0 / sigma as f64;
    let t = (t_gas.0.max(250_000) as f64) / 1000.0;
    let nu = 1.5e-5 * libm::pow(t / 293.0, 1.75);
    let re = v * d / nu;
    let nusselt = (0.3 + 0.62 * libm::sqrt(re) * 0.888).max(2.0);
    // h·d/k = Nu and SURFACE_H_PER_SIGMA is h at Nu = 2 per unit σ: scale it.
    (SURFACE_H_PER_SIGMA as f64 * nusselt / 2.0).round() as i64
}

pub fn react(world: &mut World) {
    let t = tables();
    let lit = flame_reach(world);
    world.run_phase(|w, b, out, _tally| {
        let base = b * BRICK_CELLS;
        for l in 0..BRICK_CELLS { out[l] = react_cell(w.cells[base + l], &w.subs, t, w.ambient, lit[base + l]) }
    });
}

/// Which cells' gas a flame can reach this tick: gas holding both fuel and
/// oxygen, joined through such gas to something that lights it (hot gas or
/// a hot fuel surface, see [`lights`]). A flame is anchored to burning fuel
/// and runs through the flammable gas around it: a 10 cm cell's average
/// says nothing of the flame sheet inside it, and FDS has no ignition
/// temperature for gas by default (its only test is the critical flame
/// temperature, which `react_cell` applies to each cell).
pub fn flame_reach(world: &World) -> Vec<bool> {
    let (o2, fuel) = (Gas::Oxygen as usize, Gas::Fuel as usize);
    let n = world.cells.len();
    let burnable = |c: &Cell| c.gas[fuel].0 > 0 && c.gas[o2].0 > 0;
    let mut lit = vec![false; n];
    let mut queue = Vec::new();
    for i in 0..n {
        let c = &world.cells[i];
        if burnable(c) && lights(c, &world.subs) { lit[i] = true; queue.push(i) }
    }
    // Spread to face neighbours that hold burnable gas (deterministic: a
    // cell's final state doesn't depend on the order it's reached in).
    let mut head = 0;
    while head < queue.len() {
        let i = queue[head];
        head += 1;
        let ((ox, oy, oz), (lx, ly, lz)) = (world.brick_origin(i / BRICK_CELLS), World::local_xyz(i % BRICK_CELLS));
        let (x, y, z) = (ox + lx, oy + ly, oz + lz);
        for axis in 0..3 { for dir in [-1i64, 1] {
            let mut p = [x as i64, y as i64, z as i64];
            p[axis] += dir;
            if !world.inside(p[0], p[1], p[2]) { continue }
            let j = world.idx(p[0] as usize, p[1] as usize, p[2] as usize);
            if !lit[j] && burnable(&world.cells[j]) && world.cells[j].permeable(&world.subs) { lit[j] = true; queue.push(j) }
        }}
    }
    lit
}

/// Whether a cell can light burnable gas: its gas is past ignition, or it
/// holds fuel whose surface is (a burning or glowing surface is a pilot).
pub fn lights(c: &Cell, subs: &[Substance]) -> bool {
    (c.gas_capacity().0 > 0 && c.gas_temperature() >= IGNITION)
        || (c.solid[VOLATILE].0 + c.solid[CHAR].0 > 0 && c.surface_temperature(subs) >= IGNITION)
}

/// Fresh-air oxygen for a gas volume, µg.
#[inline]
fn fresh_o2(ml: i64) -> i64 { FRESH_AIR[Gas::Oxygen as usize].0 * ml / CELL_VOLUME_ML }

/// Moves the sensible heat of `m` (specific heat `sh`, at `t`) from the
/// solid store to the gas store, as that matter leaves the solid.
#[inline]
fn solid_to_gas(c: &mut Cell, subs: &[Substance], m: Mass, sh: SpecificHeat, t: Temperature) {
    let e = (m * sh) * t;
    *c.surface_store(subs) -= e;
    c.energy += e;
}

/// Water inside a solid boils off with the core's heat.
fn boil_from_core(c: &mut Cell, dm: Mass, t: Temperature) {
    if dm.0 <= 0 { return }
    let e = (dm * GAS_SPECIFIC_HEAT[Gas::Vapour as usize]) * t + dm * LATENT_HEAT_VAPOUR;
    c.water -= dm;
    c.gas[Gas::Vapour as usize] += dm;
    c.solid_energy -= e;
    c.energy += e;
}

/// Water leaves the solid as vapour at the surface's temperature: the
/// vapour's sensible heat (as steam) and its latent heat come out of the
/// solid's store and go into the gas's. The water's own extra heat capacity
/// stays with the solid.
fn evaporate(c: &mut Cell, subs: &[Substance], dm: Mass, ts: Temperature) {
    if dm.0 <= 0 { return }
    let e = (dm * GAS_SPECIFIC_HEAT[Gas::Vapour as usize]) * ts + dm * LATENT_HEAT_VAPOUR;
    c.water -= dm;
    c.gas[Gas::Vapour as usize] += dm;
    *c.surface_store(subs) -= e;
    c.energy += e;
}

/// Burnable gas a cell's solid gives off this tick, at its surface's
/// temperature, within the two measured caps (see `react_cell`).
pub fn pyrolysis_rate(c: &Cell, subs: &[Substance], t: &Tables) -> Mass {
    let s = &subs[c.substance as usize];
    if c.solid[VOLATILE].0 <= 0 { return Mass(0) }
    let full = s.full_mass.0 * s.composition[VOLATILE] / 1_000;
    let cap = if c.permeable(subs) {
        (full as i128 * s.surface_per_volume.max(1) as i128 * TICK.0 as i128 / (RESIDENCE_S_PER_SIGMA as i128 * 1_000)) as i64
    } else {
        // ρ × face area × depth charred per tick: full mass × (β·dt / edge)
        (full as i128 * CHAR_UM_PER_MIN as i128 * TICK.0 as i128 / (60_000i128 * CELL_MM as i128 * 1_000)) as i64
    };
    t.pyrolysis[k_index(c.surface_temperature(subs).0)].of_mass(c.solid[VOLATILE]).min(Mass(cap.max(1)))
}

pub fn react_cell(c: Cell, subs: &[Substance], t: &Tables, ambient: Temperature, pilot: bool) -> Cell {
    let core_before = c.core_capacity(subs);
    let mut c = react_inner(c, subs, t, ambient, pilot);
    // Matter leaves from the surface, but a burning solid shrinks as a
    // whole (the surface layer is a share of it): the core gives up the heat
    // of what it no longer holds to the surface, keeping its temperature.
    let core_after = c.core_capacity(subs);
    if core_after < core_before && core_before.0 > 0 {
        let keep = Energy((c.solid_energy.0 as i128 * core_after.0 as i128 / core_before.0 as i128) as i64);
        let moved = c.solid_energy - keep;
        c.solid_energy = keep;
        *c.surface_store(subs) += moved;
    }
    c
}

fn react_inner(mut c: Cell, subs: &[Substance], t: &Tables, ambient: Temperature, pilot: bool) -> Cell {
    let ml = c.gas_volume_ml(subs);
    let s = &subs[c.substance as usize];
    let vap = Gas::Vapour as usize;
    let drops = Gas::Droplets as usize;

    // ── solid and gas trade heat across the solid's surface ──────────────
    // Dense matter has its pores' walls, so its gas follows the solid at
    // once; a tuft of grass in a flame warms over a second or two.
    if c.has_solid() && c.gas_capacity().0 > 0 {
        // Pore gas trades with the core (an opaque cell's skin is its outer face).
        // A dense solid's pore gas touches its core; the air in a fuel bed
        // touches the twigs' surfaces.
        let porous_skin = !c.opaque(subs) && c.skin_capacity(subs).0 > 0;
        let (cs, cg) = (if porous_skin { c.skin_capacity(subs) } else { c.core_capacity(subs) }, c.gas_capacity());
        let (ts, tg) = (if porous_skin { c.surface_temperature(subs) } else { c.solid_temperature(subs) }, c.gas_temperature());
        // G = h·A with h in mW/(m²·K) and A in mm²: product is 10⁻⁹ W/K = 10⁻³ µW/K.
        let h = (surface_h(s.surface_per_volume, tg, ambient) * s.surface_per_volume) as i128;
        // Open water trades heat with the air over it across its surface.
        let g = if c.open_water() { WATER_SURFACE_H as i128 * WATER_SURFACE_MM2 as i128 / 1_000 } else { h * c.inner_surface_mm2(subs) as i128 / 1_000 };
        let mut q = (Conductance(g as i64) * (tg - ts)) * TICK;
        // Never past the shared temperature: at most what equalises them.
        // Worked out from the gas side: the shared temperature is rounded to
        // a millikelvin, and that rounding must land in the big store.
        let held = if porous_skin { c.skin } else { c.solid_energy };
        let shared = (c.energy - c.latent() + held) / (cs + cg);
        let limit = (c.energy - c.latent()) - cg * shared;
        if q.0.abs() > limit.0.abs() || !c.permeable(subs) { q = limit }
        if porous_skin { c.skin += q } else { c.solid_energy += q }
        c.energy -= q;
    }
    // ── an opaque cell's surface layer and core trade heat ───────────────
    // Through the solid's own conductivity, across half a cell: in wood
    // (0.15 W/(m·K)) a hot surface warms the core over many minutes.
    if c.skin_capacity(subs).0 > 0 && c.core_capacity(subs).0 > 0 {
        let g = if c.opaque(subs) {
            Conductance(s.conductivity * 2 * CELL_MM) // k × face / half an edge
        } else {
            // Into a twig from its surface, set by how long heat takes to
            // soak through it (r²/α, r = 2/σ): g = C_core·α/r², which is
            // V_core·k·σ²/4 = (1 − ε)·k·σ·A/4. A 6 mm twig: ~100 s.
            // k[mW/(m·K)] × σ × A[mm²] → µW/K ÷ 1000.
            let e = crate::tables::tables().heating_permille[(s.surface_per_volume as usize).min(crate::tables::MAX_SIGMA)];
            Conductance((s.conductivity as i128 * s.surface_per_volume as i128 * c.inner_surface_mm2(subs) as i128 * (1_000 - e) as i128 / (4_000 * 1_000)) as i64)
        };
        let (tk, tc) = (c.surface_temperature(subs), c.solid_temperature(subs));
        let mut q = (g * (tk - tc)) * TICK;
        let (ck, cc) = (c.skin_capacity(subs), c.core_capacity(subs));
        let shared = (c.skin + c.solid_energy) / (ck + cc);
        let limit = c.skin - ck * shared; // from the smaller (surface) side
        if q.0.abs() > limit.0.abs() { q = limit }
        c.solid_energy += q;
        c.skin -= q;
    }
    if ml <= 0 { return c }

    // ── water ──────────────────────────────────────────────────────────────
    // Water leaving the solid takes its latent heat from the solid: above
    // boiling, the excess heat boils water off until the solid is back at
    // 100 °C (the plateau that keeps wet things from burning).
    // Water leaves from the surface: an opaque cell's surface layer.
    let surface_cap = |c: &Cell| if c.skin_capacity(subs).0 > 0 { c.skin_capacity(subs) } else { c.solid_capacity(subs) };
    // Water sits through the whole solid; the surface layer holds its share
    // of it. A hot surface boils off what it holds and then heats on (a log
    // chars while its heart is still wet); water inside boils once the core
    // itself passes 100 °C, and its steam escapes through the surface.
    let ts = c.surface_temperature(subs);
    // Each µg boiled takes its latent heat, plus its own sensible heat at 100 °C.
    let per_ug = LATENT_HEAT_VAPOUR.0 + (Mass(1) * WATER_SPECIFIC_HEAT * BOIL).0;
    let skin_cap = c.skin_capacity(subs);
    if c.water.0 > 0 && ts > BOIL {
        let excess = surface_cap(&c) * (ts - BOIL);
        let held = if skin_cap.0 > 0 { (c.water.0 as i128 * skin_cap.0 as i128 / c.solid_capacity(subs).0.max(1) as i128) as i64 } else { c.water.0 };
        let dm = Mass((excess.0 / per_ug.max(1)).min(held.max(1)).min(c.water.0));
        evaporate(&mut c, subs, dm, ts);
    }
    let tc = c.solid_temperature(subs);
    if c.water.0 > 0 && skin_cap.0 > 0 && tc > BOIL {
        let excess = c.core_capacity(subs) * (tc - BOIL);
        let dm = Mass((excess.0 / per_ug.max(1)).min(c.water.0));
        boil_from_core(&mut c, dm, tc);
    }
    let ts = c.surface_temperature(subs);
    // Below boiling, water evaporates while the vapour the wet surface holds
    // (at the surface's own temperature) is more than the air around it
    // already has; faster for fine fuel (more surface).
    let sat = t.saturated_ug_per_l[k_index(ts.0)].saturating_mul(ml) / 1_000;
    // (At or above boiling, the boiling step above has already done this.)
    if c.water.0 > 0 && ts < BOIL && c.gas[vap].0 < sat {
        let room = sat - c.gas[vap].0;
        let speed = s.surface_per_volume.min(FINE_SIGMA).max(1);
        // Only the water the surface layer holds can leave from it.
        let skin_cap = c.skin_capacity(subs);
        let held = if skin_cap.0 > 0 { Mass((c.water.0 as i128 * skin_cap.0 as i128 / c.solid_capacity(subs).0.max(1) as i128) as i64).max(Mass(1)) } else { c.water };
        let dm = if c.open_water() {
            // Open water: the air swept over its surface carries off what it
            // holds short of saturation at the water's temperature (Dalton's
            // law, E = h_m·(ρ_sat(T_water) − ρ_air)). Warm water holds more
            // vapour at its surface, so sunlight and heat speed it up.
            let air = c.gas[vap].0 as i128 * CELL_VOLUME_ML as i128 / ml as i128; // µg/L
            let sat_l = t.saturated_ug_per_l[k_index(ts.0)] as i128;
            Mass((((sat_l - air).max(0) * WATER_SWEPT_UL as i128 + 500_000) / 1_000_000) as i64).min(Mass(room))
        } else {
            Mass((room as i128 * speed as i128 / (FINE_SIGMA as i128 * 4)) as i64)
        }.min(held).min(c.water);
        evaporate(&mut c, subs, dm, ts);
    }
    // Open water that has all gone leaves its last heat in the air.
    if c.substance == NONE && c.water.0 == 0 && c.solid_mass().0 == 0 && c.solid_energy.0 != 0 {
        c.energy += c.solid_energy;
        c.solid_energy = Energy::ZERO;
    }
    // Vapour beyond what the air holds condenses into drops (mist, white
    // smoke); drops in unsaturated air evaporate. Both stay in the gas.
    let sat = t.saturated_ug_per_l[k_index(c.gas_temperature().0)].saturating_mul(ml) / 1_000;
    if c.gas[vap].0 > sat {
        let dm = Mass((c.gas[vap].0 - sat) / 2);
        c.gas[vap] -= dm;
        c.gas[drops] += dm;
    }
    if c.gas[drops].0 > 0 && c.gas[vap].0 < sat {
        let dm = Mass(((sat - c.gas[vap].0) / 2).min(c.gas[drops].0));
        c.gas[drops] -= dm;
        c.gas[vap] += dm;
    }
    // Burning happens at the surface: an opaque cell's outer layer.
    let ts = c.surface_temperature(subs);
    let ks = k_index(ts.0);

    // ── pyrolysis: hot organic matter gives off burnable gas ─────────────
    // A 10 cm cell can't resolve a flame working through a tuft or charring
    // into a log, so two measured rates cap how fast the gas can come:
    // a fuel bed burns over Anderson's flame residence time τ = 75 600/σ s
    // (Anderson 1969: 384/σ min with σ in ft⁻¹), ~6.6 s for fine grass;
    // dense wood chars inward ~1.5 mm a minute from an exposed face
    // (fire-exposed charring rates, e.g. Eurocode 5: 0.65–1 mm/min standard,
    // faster in intense fires), so a 10 cm log burns for most of an hour.
    if c.solid[VOLATILE].0 > 0 {
        let dm = pyrolysis_rate(&c, subs, t);
        if dm.0 > 0 {
            solid_to_gas(&mut c, subs, dm, s.specific_heat, ts);
            c.solid[VOLATILE] -= dm;
            c.gas[Gas::Fuel as usize] += dm;
            // The gas holds more chemical energy than the solid it came
            // from: the difference is heat taken from the surface.
            *c.surface_store(subs) -= dm * GAS_CHEMICAL[Gas::Fuel as usize] - dm * s.volatile_chemical;
        }
    }

    let o2 = Gas::Oxygen as usize;
    let fresh = fresh_o2(ml).max(1);

    // ── char glows and burns where oxygen reaches it ─────────────────────
    // Hot char burns as fast as oxygen can diffuse to its surface. Above
    // ~700 °C it burns mostly to carbon monoxide (2C + O₂ → 2CO), which
    // releases only 9.2 of carbon's 32.8 kJ/g at the surface; the CO burns
    // in the gas beside it and carries the rest (e.g. Turns, An Introduction
    // to Combustion, ch. 14). So a glowing ember heats the air around it
    // more than itself, and sits at 600–900 °C rather than far hotter.
    if c.solid[CHAR].0 > 0 && c.gas[o2].0 > 0 {
        let kinetic = t.char_burn[ks].of_mass(c.solid[CHAR]).0 * c.gas[o2].0.min(fresh) / fresh;
        let area = if c.permeable(subs) { c.inner_surface_mm2(subs) } else { OUTER_FACE_MM2 };
        // h_m·A·ρ_O₂·dt with h_m = 0.05 m/s: A[mm²] × O₂[µg/mL] × 2.5×10⁻³ µg of O₂.
        let o2_reach = (area as i128 * c.gas[o2].0 as i128 / ml.max(1) as i128 / 400) as i64;
        // Burning to CO takes half the oxygen per carbon that CO₂ does.
        let diffusion = o2_reach * 2 * O2_PER_CARBON_DEN / O2_PER_CARBON_NUM;
        let want = kinetic.min(diffusion);
        let dm = want.min(c.gas[o2].0 * 2 * O2_PER_CARBON_DEN / O2_PER_CARBON_NUM);
        if dm > 0 {
            let ox = dm * O2_PER_CARBON_NUM / (2 * O2_PER_CARBON_DEN);
            solid_to_gas(&mut c, subs, Mass(dm), CHAR_SPECIFIC_HEAT, ts);
            c.solid[CHAR] -= Mass(dm);
            c.gas[o2] -= Mass(ox);
            // The CO is carried as burnable gas holding the same energy (wood
            // gas burns the same way); the rest of its mass is inert.
            let co_energy = Mass(dm) * CHAR_CHEMICAL - Mass(dm) * CHAR_TO_CO_HEAT;
            let as_fuel = Mass(co_energy.0 / GAS_CHEMICAL[Gas::Fuel as usize].0.max(1)).min(Mass(dm + ox));
            c.gas[Gas::Fuel as usize] += as_fuel;
            c.gas[Gas::CarbonDioxide as usize] += Mass(dm + ox) - as_fuel;
            *c.surface_store(subs) += Mass(dm) * CHAR_CHEMICAL - as_fuel * GAS_CHEMICAL[Gas::Fuel as usize];
        }
    }

    // ── burnable gas burns with oxygen: this is flame ────────────────────
    // A flame is a thin hot sheet where fuel meets air, far hotter than the
    // cell's average. Whether a mix can carry one is a crossing of lines:
    // the heat it would release against the heat the gas soaks up. If
    // burning what's here could lift the gas to the critical flame
    // temperature, it burns as fast as fuel and air mix; if not, the flame
    // can't hold and only slow oxidation goes on (the extinction model of
    // FDS; McGrattan et al., FDS Technical Reference Guide).
    let fuel = Gas::Fuel as usize;
    // Not inside a dense solid: gas given off in wood's pores escapes through
    // its char and burns outside it, as flame, in the open air around it
    // (the pore gas is pushed out by its own pressure).
    if c.gas[fuel].0 > 0 && c.gas[o2].0 > 0 && c.permeable(subs) {
        let tg = c.gas_temperature();
        let could = c.gas[fuel].0.min(c.gas[o2].0 * 1_000 / O2_PER_FUEL);
        // The flame sheet holds fuel and just the air it needs to burn, with
        // whatever that air carries along (nitrogen, and in a smoky cell,
        // smoke): FDS checks that mix, not the cell's average, which at 10 cm
        // is always too thin to burn.
        let o2_needed = could as i128 * O2_PER_FUEL as i128 / 1_000;
        let air_cap = (c.gas_capacity() - c.gas[fuel] * GAS_SPECIFIC_HEAT[fuel]).0 as i128;
        let sheet_cap = (Mass(could) * GAS_SPECIFIC_HEAT[fuel]).0 as i128 + air_cap * o2_needed / c.gas[o2].0.max(1) as i128;
        let rise = Mass(could) * GAS_CHEMICAL[fuel] / HeatCapacity(sheet_cap as i64);
        // Gas coming off a hot surface ignites at that surface (a flame
        // anchored to the fuel), so the reaction runs at the hotter of the gas
        // and the solid it's leaving.
        let anchored = if c.has_solid() { tg.max(c.surface_temperature(subs)) } else { tg };
        let kinetic = t.gas_burn[k_index(anchored.0)].of_mass(c.gas[fuel]).0;
        let holds = (anchored >= IGNITION || pilot) && tg.0 + rise.0 >= CRITICAL_FLAME.0;
        // Fuel and air must meet before they burn, however fast the
        // chemistry: no more than mixing brings together burns in a tick.
        let mixable = MIXING.of_mass(Mass(could)).0;
        let mixed = if holds { mixable } else { 0 };
        let want = if holds { mixed } else { kinetic.min(mixable) };
        // Short of oxygen, it burns dirty: up to a tenth of it becomes soot.
        let starved = 1_000 - (c.gas[o2].0 * 1_000 / fresh).min(1_000);
        let soot_pm = 10 + 90 * starved / 1_000;
        // Oxygen limits how much can burn this tick.
        let by_o2 = c.gas[o2].0 * 1_000 / O2_PER_FUEL * 1_000 / (1_000 - soot_pm).max(1);
        let dm = want.min(by_o2);
        if dm > 0 {
            let soot = dm * soot_pm / 1_000;
            let burnt = dm - soot;
            let ox = (burnt * O2_PER_FUEL / 1_000).min(c.gas[o2].0);
            let products = burnt + ox;
            let co2 = products * CO2_SHARE / 1_000;
            c.gas[fuel] -= Mass(dm);
            c.gas[o2] -= Mass(ox);
            c.gas[Gas::Soot as usize] += Mass(soot);
            c.gas[Gas::CarbonDioxide as usize] += Mass(co2);
            c.gas[vap] += Mass(products - co2);
            // Heat released: the chemical energy that went away. (Chemical
            // energies are higher heating values, water counted as liquid,
            // so the new vapour's latent heat is drawn from this automatically.)
            let heat = Mass(dm) * GAS_CHEMICAL[fuel] - Mass(soot) * GAS_CHEMICAL[Gas::Soot as usize];
            // Inside a fuel bed the flame wraps the fuel's surface, so part of
            // its heat goes straight into the solid (flame-to-fuel feedback,
            // 20–40 % in flame-spread models): more surface, more of it.
            let area = c.inner_surface_mm2(subs);
            let share = Fraction((FEEDBACK_MAX.0 as i128 * area as i128 / (area as i128 + FEEDBACK_HALF_MM2)) as u64);
            let to_solid = if c.has_solid() { share.of_energy(heat) } else { Energy::ZERO };
            *c.surface_store(subs) += to_solid;
            c.energy += heat - to_solid;
            // A flame (not slow oxidation) glows: part of its heat leaves as light.
            if mixed > 0 { c.flame += RADIATIVE_FRACTION.of_energy(heat).min(heat - to_solid) }
        }
    }

    // ── soot burns away in very hot air with oxygen ──────────────────────
    let soot = Gas::Soot as usize;
    if c.gas[soot].0 > 0 && c.gas[o2].0 > 0 {
        let k = k_index(c.gas_temperature().0);
        let want = t.soot_burn[k].of_mass(c.gas[soot]).0 * c.gas[o2].0.min(fresh) / fresh;
        let dm = want.min(c.gas[o2].0 * O2_PER_CARBON_DEN / O2_PER_CARBON_NUM);
        if dm > 0 {
            let ox = dm * O2_PER_CARBON_NUM / O2_PER_CARBON_DEN;
            c.gas[soot] -= Mass(dm);
            c.gas[o2] -= Mass(ox);
            c.gas[Gas::CarbonDioxide as usize] += Mass(dm + ox);
            c.energy += Mass(dm) * GAS_CHEMICAL[soot];
        }
    }
    c
}
