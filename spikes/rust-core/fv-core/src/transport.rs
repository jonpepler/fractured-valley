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
//! - **flame contact**: flames lean and flicker sideways and touch the fuel
//!   beside them (Finney et al. 2015: fine fuel ignites by this contact, not
//!   by radiation, which convective cooling defeats). A cell can't
//!   hold the flame's shape, so its hot gas heats the neighbour's fuel
//!   across the face for the share of the time a flame leans that way.
//!   A dense face (a log's) is touched by any flame burning in the gas
//!   beside it, over the share of the face that flame fills (its heat
//!   release against a flame's ~1.2 MW/m³);
//! - **carrying** (in [`advect`]): moving air takes a share of its contents
//!   across each face it flows through.

use crate::cell::Cell;
use crate::substance::*;
use crate::units::*;
use crate::world::{Neighbour, Tally, World, B, BRICK_CELLS, TICK};

/// Everything a face needs to know about one side, worked out once per cell.
#[derive(Copy, Clone, Debug, Default)]
pub struct Side {
    /// The gas's temperature: what gas leaving this cell carries.
    pub t: Temperature,
    /// The temperature heat is conducted from: the solid of an opaque cell,
    /// the gas of an airy one.
    pub t_cond: Temperature,
    /// An opaque cell's surface temperature (what the air film touches).
    pub t_skin: Temperature,
    pub gas_ml: i64,
    pub permeable: bool,
    /// Conductance of half the cell (centre to face).
    pub half: Conductance,
    /// Gas beyond what fits at this temperature (pushed out).
    pub excess: Mass,
    /// The surroundings' temperature (the same for every cell).
    pub ambient: Temperature,
    /// Temperature of this cell's flame gas if it is burning with a flame
    /// (its fuel is past ignition and its gas is hot), else zero.
    pub flame_t: Temperature,
    /// The most this cell's flame gas can give through one face this tick.
    pub flame_heat: Energy,
    /// Share of this cell's gas the flame burning in it fills, per mille:
    /// what it will release this tick against a flame's own heat release
    /// per volume (zero where no flame holds). A dense face bounding the
    /// cell is wetted by flame over that share of its area.
    pub flame_fill: i64,
    /// Conductance from gas to this cell's fuel surface (h·A), µW/K, if it
    /// has fuel that can catch; zero otherwise.
    pub fuel_g: i64,
    /// Share of this cell's fuel the flame front has reached, per mille
    /// (1 000 for a cell burning throughout): its flame leans over only
    /// that share of a face.
    pub reached: i64,
    /// Open water lies here: heat conducted across its faces is the water's.
    pub wet: bool,
    /// Conductance from the middle of open water down to what it lies on
    /// (half its depth of water), µW/K; zero without open water.
    pub water_g: i64,
}

/// Share of the time a flame leans over the fuel beside it (flame fingers
/// flicker at a few hertz and touch the fuel ahead intermittently: Finney
/// et al. 2015).
/// Experiments can set it at build time: `FV_FLAME_LEAN_PERMILLE=300`.
const FLAME_LEAN_PERMILLE: i64 = parse_permille(option_env!("FV_FLAME_LEAN_PERMILLE"), 500);

/// A build-time number, or the default when it's unset.
const fn parse_permille(v: Option<&str>, default: i64) -> i64 {
    let Some(v) = v else { return default };
    let b = v.as_bytes();
    let (mut i, mut n) = (0, 0i64);
    while i < b.len() {
        assert!(b[i] >= b'0' && b[i] <= b'9', "FV_FLAME_LEAN_PERMILLE must be a whole number");
        n = n * 10 + (b[i] - b'0') as i64;
        i += 1;
    }
    assert!(n <= 1_000, "FV_FLAME_LEAN_PERMILLE is out of 1000");
    n
}
/// Mean temperature of a flame's continuous region (McCaffrey 1979).
const FLAME_CONTACT: Temperature = Temperature(1_173_000);
/// A solid face inside a flame takes ~50–150 kW/m² (its convection plus the
/// thick flame's radiation; e.g. crib and wall-fire measurements, Babrauskas,
/// Ignition Handbook): about 80 W/(m²·K) at flame temperature, mW/(m²·K).
const FLAME_ON_FACE: i64 = 80_000;
/// That over one face, µW/K: the most a flame beside a cell can hand the
/// fuel across the face between them (its flux into a surface in flame
/// times the face's area, not into every blade of a 10 cm tuft at once).
const FLAME_FACE_G: i64 = FLAME_ON_FACE * FACE_MM2 / 1_000;
/// Heat a turbulent flame releases per volume of flame: about 1.2 MW/m³,
/// whatever the fuel or the fire's size (Orloff & De Ris, from pool fires,
/// as cited with a recalibration to 1.5 ± 0.2 MW/m³ in Fire Technology
/// 2022, doi 10.1007/s10694-022-01341-z), µJ per mL per tick:
/// 1 200 µW/mL × the tick.
const FLAME_UJ_PER_ML: i64 = 1_200 * TICK.0 / 1_000;

/// A buoyant plume's entrainment coefficient: air is drawn in sideways at
/// α times its rising speed (Morton, Taylor & Turner 1956: α ≈ 0.09–0.12
/// for round plumes), per mille.
const ENTRAINMENT_PERMILLE: i64 = 100;

/// Thermal conductivity of liquid water, mW/(m·K) (0.59 W/(m·K) at 20 °C:
/// e.g. Incropera, Fundamentals of Heat and Mass Transfer, table A.6).
const WATER_CONDUCTIVITY: i64 = 590;
/// Thermal conductivity of still air, mW/(m·K).
const AIR_CONDUCTIVITY: i64 = 26;
/// Film coefficient where a solid surface meets air: ~15 W/(m²·K) over one
/// face (W/(m²·K) × mm² = µW/K).
const FILM: Conductance = Conductance(15 * FACE_MM2);
/// Exchange of gas across a face per tick in still air, in µL: eddies of a
/// few mm/s, effective diffusivity D ~1×10⁻³ m²/s, so D·A/L·dt, which grows
/// with the cell's edge (5 mL at 10 cm).
const STILL_MIX_UL: i64 = 50 * CELL_MM;
/// Extra exchange per mm/s of air speed at the face, in nL: turbulent eddies
/// carry ≈ 10 % of the flow through the face's area (50 µL at 10 cm).
const TURBULENT_MIX_NL_PER_MM_S: i64 = 50_000 * FACE_MM2 / 10_000;
/// Mixed volumes are counted in thousandths of a cell (1 mL at 10 cm).
const MIX_QUANTUM_UL: i64 = CELL_VOLUME_ML;
/// Gas seeps through non-permeable porous solids this many times slower.
const SEEP_DIVISOR: i64 = 20;

pub fn side(c: &Cell, subs: &[Substance], ambient: Temperature) -> Side { side_lit(c, subs, ambient, false) }

/// A cell's side, given whether a flame reaches its gas this tick
/// ([`crate::chem::flame_reach`]).
pub fn side_lit(c: &Cell, subs: &[Substance], ambient: Temperature, pilot: bool) -> Side {
    let s = &subs[c.substance as usize];
    let share = c.solid_share(subs);
    let k = share.of(s.conductivity) + AIR_CONDUCTIVITY - share.of(AIR_CONDUCTIVITY);
    let t = c.gas_temperature();
    let wet = c.open_water();
    let t_cond = if wet { c.solid_temperature(subs) } else if c.permeable(subs) { t } else { c.solid_temperature(subs) };
    // k × face / (depth / 2): mW/(m·K) × mm² / µm is 10³ µW/K. Depth in µm
    // is the water's µg over the face's mm² (1 mm over 1 mm² is 1 mg).
    let water_g = if wet {
        let half_um = (c.water.0 / (2 * FACE_MM2)).max(500);
        div_i128(WATER_CONDUCTIVITY as i128 * FACE_MM2 as i128 * 1_000, half_um as i128) as i64
    } else { 0 };
    let t_skin = c.surface_temperature(subs);
    let gas_ml = c.gas_volume_ml(subs);
    let fresh: i64 = FRESH_AIR.iter().map(|m| m.0).sum();
    // What fits at this temperature (ideal gas at constant pressure).
    let fits = if t.0 > 0 { div_i128(fresh as i128 * gas_ml as i128 * ambient.0 as i128, CELL_VOLUME_ML as i128 * t.0 as i128) as i64 } else { i64::MAX };
    let volatile = c.solid[crate::cell::VOLATILE].0 > 0;
    // Burning with a flame: hot fuel in hot gas, or a front working through
    // fine fuel with a flame over it.
    let flaming = volatile && ((t_skin >= crate::chem::IGNITION && t >= crate::chem::IGNITION) || (c.front > 0 && c.lit && crate::chem::fine_fuel(c, subs)));
    // What touches the fuel is the flame itself, not the cell's average gas:
    // the continuous flame region averages ~900 °C (McCaffrey 1979).
    let flame_t = if flaming { FLAME_CONTACT } else { Temperature(0) };
    // A flame in the open gas between fuel (the shafts of a crib, the air
    // over a bed) fills part of its cell: the part its heat release needs
    // at a flame's heat per volume.
    let release = if pilot && gas_ml > 0 { crate::chem::flame_release(c, subs, true).0 } else { 0 };
    let flame_fill = (release as i128 * 1_000 / (FLAME_UJ_PER_ML as i128 * gas_ml.max(1) as i128)).min(1_000) as i64;
    // What a flame can give the faces around it: its gas's heat above the
    // surroundings, and the heat it releases this tick (a flame against a
    // wall gives the wall part of its own heat, not just the gas's), a
    // sixth through each face.
    let excess = ((c.energy - c.latent()).0 - (c.gas_capacity() * ambient).0).max(0);
    let flame_heat = if flaming || flame_fill > 0 { Energy((excess + release) / 6) } else { Energy::ZERO };
    let sigma = s.surface_per_volume;
    let fuel_g = if volatile && sigma > 0 {
        let area = if c.permeable(subs) { c.inner_surface_mm2(subs) } else { FACE_MM2 };
        // h [mW/(m²·K)] × A [mm²] = 10⁻⁹ W/K = 10⁻³ µW/K
        let h = if c.permeable(subs) { crate::chem::surface_h(sigma, t, ambient) * sigma } else { FLAME_ON_FACE };
        mul_div(h, area, 1_000) as i64
    } else { 0 };
    Side {
        flame_t, flame_heat, flame_fill, fuel_g, wet, water_g,
        reached: if crate::chem::fine_fuel(c, subs) && t_skin < crate::chem::IGNITION { c.front as i64 * 1_000 / crate::cell::FRONT_FULL as i64 } else { 1_000 },
        t, t_cond, t_skin, gas_ml, permeable: c.permeable(subs),
        // k × face / half an edge = k·L²/(L/2) = 2·k·L: k [mW/(m·K)] × L [mm]
        // is µW/K (200·k at 10 cm).
        half: Conductance(k * 2 * CELL_MM),
        excess: Mass((c.gas_mass().0 - fits).max(0)),
        ambient,
    }
}

/// The surroundings' temperature, which every side carries.
#[inline]
fn ambient_of(a: &Side, _b: &Side) -> Temperature { a.ambient }

/// Speed of the eddies hot gas makes, mm/s: √(g·l·ΔT/T) with l one cell's edge
/// (the largest eddy a cell holds).
#[inline]
pub fn buoyant_churn(t: Temperature, ambient: Temperature) -> i64 {
    let dt = (t.0 - ambient.0).max(0) as i128;
    if dt == 0 { return 0 }
    // g·l = 9 807 mm/s² × l [mm] (980 700 mm²/s² at 10 cm) ... ×ΔT/T
    let v2 = div_i128(9_807i128 * CELL_MM as i128 * dt, ambient.0.max(1) as i128);
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
    /// Whether this face is an opaque surface against air (its conducted
    /// heat goes to and from the surface layer).
    pub film: bool,
    pub gas: [Mass; GASES],
    /// Flame contact: heat from one side's flame gas into the other's fuel
    /// surface (positive: lower cell's gas to upper cell's fuel).
    pub flame: Energy,
}

/// Energy carried by an amount of gas leaving a cell at temperature `t`.
#[inline]
fn carried(k: usize, m: Mass, t: Temperature) -> Energy {
    let latent = if k == Gas::Vapour as usize { m * LATENT_HEAT_VAPOUR } else { Energy::ZERO };
    (m * GAS_SPECIFIC_HEAT[k]) * t + latent
}

/// Conduction, mixing and pressure across one face, lower cell first.
/// `speed` is the air speed at the face (mm/s, unsigned).
pub fn diffuse_face(lo: &Cell, a: &Side, hi: &Cell, b: &Side, speed: i64, axis: usize) -> Flux {
    let mut f = Flux::default();
    // Flame contact. Into fine fuel: sideways, a flame in burning fuel
    // leans over its neighbour part of the time. Into a dense face: any
    // flame beside it, wherever it is, over the share of the face it fills.
    {
        let touch = |from: &Side, to: &Side| -> i64 {
            if to.fuel_g == 0 { return 0 }
            // A dense face (a log's) has no pores for flame gas to enter: a
            // flame reaches it only across the face, over the share of the
            // face the flame next to it fills. Fine fuel takes the flame
            // through its own gas, and the leaning flame of fuel beside it.
            let share = if !to.permeable { from.flame_fill }
                else if from.flame_t.0 == 0 { 0 }
                else if axis != 2 { FLAME_LEAN_PERMILLE * from.reached / 1_000 } else { 0 };
            if share == 0 || to.t_skin >= FLAME_CONTACT { return 0 }
            let g = Conductance(to.fuel_g.min(FLAME_FACE_G) * share / 1_000);
            ((g * (FLAME_CONTACT - to.t_skin)) * TICK).0.min(from.flame_heat.0)
        };
        f.flame = Energy(touch(a, b) - touch(b, a));
    }
    // Conduction: two half-cells in series; a solid meeting air goes through the film.
    let (ga, gb) = (a.half.0.max(1) as i128, b.half.0.max(1) as i128);
    let mut g = ga * gb / (ga + gb);
    let (mut ta, mut tb) = (a.t_cond, b.t_cond);
    if a.permeable != b.permeable {
        // A surface meeting air: through the film, from the surface layer.
        let solid = if a.permeable { gb } else { ga };
        // Open water lying on a solid touches it through the water itself,
        // not an air film.
        let film = if axis == 2 && b.wet && !a.permeable { b.water_g as i128 } else { FILM.0 as i128 };
        g = solid * film / (solid + film);
        f.film = true;
        if !a.permeable { ta = a.t_skin }
        if !b.permeable { tb = b.t_skin }
    }
    f.energy = (Conductance(g as i64) * (ta - tb)) * TICK;

    // Mixing: each side sends a share of its gas, by concentration. Eddies
    // come from the wind and from heat itself: hot gas churns at about
    // √(g·l·ΔT/T) (buoyant turbulence, l = a cell), metres a second in a flame.
    let both_gas = a.gas_ml > 0 && b.gas_ml > 0;
    if both_gas {
        // Hot gas churns at about √(g·l·ΔT/T), but buoyant eddies are
        // stretched upward: a plume trades gas sideways with the air around
        // it only at its entrainment velocity, α·w with α ≈ 0.1 (Morton,
        // Taylor & Turner 1956). So the churn mixes fully across horizontal
        // faces (up and down) and at a tenth of it across vertical ones.
        let churn = buoyant_churn(a.t.max(b.t), ambient_of(a, b));
        let churn = if axis == 2 { churn } else { churn * ENTRAINMENT_PERMILLE / 1_000 };
        let q = MIX_QUANTUM_UL;
        let mut ul = (STILL_MIX_UL + (speed + churn) * TURBULENT_MIX_NL_PER_MM_S / 1_000) / q * q;
        if !(a.permeable && b.permeable) { ul = ul / SEEP_DIVISOR / q * q }
        let ul = ul.min(a.gas_ml.min(b.gas_ml) * 1_000 / 8 / q * q) as i128;
        for k in 0..GASES {
            let out_a = Mass(div_i128(lo.gas[k].0 as i128 * ul, a.gas_ml as i128 * 1_000) as i64);
            let out_b = Mass(div_i128(hi.gas[k].0 as i128 * ul, b.gas_ml as i128 * 1_000) as i64);
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
        let over = div_i128(a.excess.0 as i128 * vol, a.gas_ml as i128) - div_i128(b.excess.0 as i128 * vol, b.gas_ml as i128);
        let push = div_i128(over, if both_solid { 8 * SEEP_DIVISOR as i128 } else { 8 }) as i64;
        let (from, from_side, sign) = if push >= 0 { (lo, a, 1) } else { (hi, b, -1) };
        let total = from.gas_mass().0;
        if total > 0 && push != 0 {
            let push = push.abs().min(total / 8);
            for k in 0..GASES {
                let m = Mass(mul_div(push, from.gas[k].0, total) as i64);
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
    let e = Energy(f.energy.0 * sign);
    if me.wet { out.solid_energy += e } else if me.permeable { out.energy += e } else if f.film { out.skin += e } else { out.solid_energy += e }
    // Flame contact: the giver's flame gas loses it, the taker's fuel surface gains it.
    let flame = f.flame.0 * sign;
    // (Fuel that can catch always has a surface layer.)
    if flame < 0 { out.energy += Energy(flame) } else {
        out.skin += Energy(flame);
    }
    out.energy += Energy(f.carried.0 * sign);
    for k in 0..GASES { out.gas[k] += Mass(f.gas[k].0 * sign) }
}

/// Records a flux that crossed the box's edge, from the box's point of view.
#[inline]
pub fn tally_edge(t: &mut Tally, f: &Flux, sign: i64) {
    t.energy_in += (f.energy.0 + f.carried.0) * sign;
    for k in 0..GASES { t.gas_in[k] += f.gas[k].0 * sign }
}

/// Every cell's face-side summary, computed in parallel.
pub fn sides(world: &World) -> Vec<Side> {
    let (subs, amb) = (&world.subs, world.ambient);
    let lit = crate::chem::flame_reach(world);
    world.map_cells(|_, _, _, i| side_lit(&world.cells[i], subs, amb, lit[i]))
}

/// One per million: a face's gas flows at full strength.
const FULL: i64 = 1_000_000;

/// Scales a face's gas flow (and the heat it carries) by `ppm` per million.
fn scale_gas(f: &mut Flux, ppm: i64) {
    if ppm >= FULL { return }
    for k in 0..GASES { f.gas[k] = Mass(mul_div(f.gas[k].0, ppm, FULL) as i64) }
    f.carried = Energy(mul_div(f.carried.0, ppm, FULL) as i64);
}

/// Every face's raw flux (before the limiter), each worked out once and read
/// by the cells on both sides. `diffuse_face` is a pure function of the two
/// cells' previous state, so this is exactly what each side would compute
/// for itself.
pub struct Faces {
    /// The + face of each cell along x, y and z (`None`: a mirrored edge,
    /// nothing crosses).
    plus: Vec<[Option<Flux>; 3]>,
    /// The − faces on the box's lower edges, per axis: x = 0 indexed by
    /// (y, z), y = 0 by (x, z), z = 0 by (x, y).
    lower: [Vec<Option<Flux>>; 3],
}

impl Faces {
    pub fn new(w: &World, sides: &[Side], outside: &Side, vel: &dyn FaceVelocity) -> Faces {
        let flux = |i: usize, x: usize, y: usize, z: usize, axis: usize, dir: i64| -> Option<Flux> {
            let (me, my) = (&w.cells[i], &sides[i]);
            let speed = face_speed(w, vel, x, y, z, axis, dir);
            match w.neighbour(x, y, z, axis, dir) {
                Neighbour::Inside(n) => {
                    let ns = &sides[neighbour_index(w, x, y, z, axis, dir)];
                    Some(if dir > 0 { diffuse_face(me, my, n, ns, speed, axis) } else { diffuse_face(n, ns, me, my, speed, axis) })
                }
                Neighbour::Outside(o) => Some(if dir > 0 { diffuse_face(me, my, o, outside, speed, axis) } else { diffuse_face(o, outside, me, my, speed, axis) }),
                Neighbour::Mirror => None,
            }
        };
        let plus = w.map_cells(|x, y, z, i| [flux(i, x, y, z, 0, 1), flux(i, x, y, z, 1, 1), flux(i, x, y, z, 2, 1)]);
        let (nx, ny, nz) = (w.nx, w.ny, w.nz);
        let lower = [
            (0..ny * nz).map(|k| { let (y, z) = (k % ny, k / ny); flux(w.idx(0, y, z), 0, y, z, 0, -1) }).collect(),
            (0..nx * nz).map(|k| { let (x, z) = (k % nx, k / nx); flux(w.idx(x, 0, z), x, 0, z, 1, -1) }).collect(),
            (0..nx * ny).map(|k| { let (x, y) = (k % nx, k / nx); flux(w.idx(x, y, 0), x, y, 0, 2, -1) }).collect(),
        ];
        Faces { plus, lower }
    }

    /// The raw flux across one face of cell `i` at (x, y, z), and the cell
    /// on the other side (`usize::MAX` outside the box); `None` where
    /// nothing crosses.
    #[inline]
    fn get(&self, w: &World, i: usize, x: usize, y: usize, z: usize, axis: usize, dir: i64) -> Option<(Flux, usize)> {
        let p = [x, y, z];
        if dir > 0 {
            let f = self.plus[i][axis]?;
            let n = [w.nx, w.ny, w.nz];
            return Some((f, if p[axis] + 1 < n[axis] { neighbour_index(w, x, y, z, axis, dir) } else { usize::MAX }));
        }
        if p[axis] > 0 {
            let ni = neighbour_index(w, x, y, z, axis, dir);
            return self.plus[ni][axis].map(|f| (f, ni));
        }
        let k = match axis { 0 => z * w.ny + y, 1 => z * w.nx + x, _ => y * w.nx + x };
        self.lower[axis][k].map(|f| (f, usize::MAX))
    }
}

/// How far each cell's outgoing gas must be scaled so that, summed over all
/// its faces, it never sends more of any gas than it holds (a flux limiter,
/// as in Zalesak's flux-corrected transport). Each face's own limits (mixing,
/// pressure) can't see the other faces, so together they could overdraw a
/// cell, e.g. in a windy plume. Worked out from the previous state only, so
/// every thread and peer gets the same numbers; a face then flows at the
/// smaller of its two cells' factors, so both sides still agree exactly.
/// Almost every cell gets the full factor and is untouched.
pub fn gas_limits(w: &World, faces: &Faces) -> Vec<i64> {
    w.map_cells(|x, y, z, i| {
        let mut leaving = [0i64; GASES];
        for axis in 0..3 { for dir in [-1i64, 1] {
            if let Some((f, _)) = faces.get(w, i, x, y, z, axis, dir) {
                for k in 0..GASES { let change = f.gas[k].0 * -dir; if change < 0 { leaving[k] -= change } }
            }
        }}
        let c = &w.cells[i];
        let mut ppm = FULL;
        for k in 0..GASES {
            // Keep an eighth back: never empty a cell in one step.
            let can = c.gas[k].0.max(0) * 7 / 8;
            if leaving[k] > can { ppm = ppm.min(mul_div(can, FULL, leaving[k]) as i64) }
        }
        ppm
    })
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

/// Everything the diffusion phase works out from the previous state before
/// any brick is computed: each cell's side, each face's flux and each
/// cell's limiter.
pub struct Prepared { pub sides: Vec<Side>, pub faces: Faces, pub limits: Vec<i64> }

pub fn prepare(world: &World, vel: &dyn FaceVelocity) -> Prepared {
    let sides = sides(world);
    let outside = side(&world.outside_air, &world.subs, world.ambient);
    let vel = CellVelocity::new(world, vel);
    let faces = Faces::new(world, &sides, &outside, &vel);
    let limits = gas_limits(world, &faces);
    Prepared { sides, faces, limits }
}

/// The diffusion phase: conduction, mixing and pressure on every face.
pub fn diffuse(world: &mut World, vel: &dyn FaceVelocity) {
    let p = prepare(world, vel);
    world.run_phase(|w, b, out, tally| diffuse_brick(w, &p, b, out, tally));
}

/// One brick of the diffusion phase, from the previous state only. Any peer
/// or thread computing brick `b` gets exactly these cells and this tally.
pub fn diffuse_brick(w: &World, p: &Prepared, b: usize, out: &mut [Cell], tally: &mut Tally) {
    let (sides, limits) = (&p.sides, &p.limits);
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
                let Some((mut flux, ni)) = p.faces.get(w, base + l, x, y, z, axis, dir) else { continue };
                if ni != usize::MAX {
                    scale_gas(&mut flux, limits[base + l].min(limits[ni]));
                    apply(&mut next, my, &flux, -dir);
                } else {
                    scale_gas(&mut flux, limits[base + l]);
                    apply(&mut next, my, &flux, -dir);
                    tally_edge(tally, &flux, -dir);
                }
            }
        }
        // While a neighbour's flame leans over this cell's fuel, the fuel
        // sits in flame gas, not its own cooler air: for that share of the
        // time, take back the cooling its own air would have given it (the
        // reaction step cools it fully against its own air).
        if my.fuel_g > 0 && my.t_skin > my.t {
            let mut lean = 0;
            for axis in 0..2 { for dir in [-1i64, 1] {
                if let Neighbour::Inside(_) = w.neighbour(x, y, z, axis, dir) {
                    if sides[neighbour_index(w, x, y, z, axis, dir)].flame_t.0 > 0 { lean += FLAME_LEAN_PERMILLE }
                }
            }}
            let lean = lean.min(1_000);
            if lean > 0 {
                let g = Conductance(my.fuel_g * lean / 1_000);
                let q = ((g * (my.t_skin - my.t)) * TICK).0.min(((me.energy - me.latent()).0 / 4).max(0));
                next.energy -= Energy(q);
                next.skin += Energy(q);
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
    let p = prepare(w, vel);
    bricks.iter().map(|&b| {
        let mut cells = vec![Cell::default(); BRICK_CELLS];
        let mut tally = Tally::default();
        diffuse_brick(w, &p, b, &mut cells, &mut tally);
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

/// The flow's velocity on every cell face, looked up once (a [`Flow`]
/// interpolates between its blocks on every call). Gives exactly what the
/// flow it was made from gives.
///
/// [`Flow`]: crate::flow::Flow
pub struct CellVelocity<'a> {
    src: &'a dyn FaceVelocity,
    n: [usize; 3],
    bricks: [usize; 2],
    /// The + face of each cell (in cell index order).
    plus: Vec<[i64; 3]>,
    /// The faces on the box's lower edges (cell −1 along the axis), indexed
    /// as in [`Faces`].
    lower: [Vec<i64>; 3],
}

impl<'a> CellVelocity<'a> {
    pub fn new(w: &World, src: &'a dyn FaceVelocity) -> CellVelocity<'a> {
        let plus = w.map_cells(|x, y, z, _| { let (x, y, z) = (x as i64, y as i64, z as i64); [src.face(x, y, z, 0), src.face(x, y, z, 1), src.face(x, y, z, 2)] });
        let (nx, ny, nz) = (w.nx, w.ny, w.nz);
        let lower = [
            (0..ny * nz).map(|k| src.face(-1, (k % ny) as i64, (k / ny) as i64, 0)).collect(),
            (0..nx * nz).map(|k| src.face((k % nx) as i64, -1, (k / nx) as i64, 1)).collect(),
            (0..nx * ny).map(|k| src.face((k % nx) as i64, (k / nx) as i64, -1, 2)).collect(),
        ];
        CellVelocity { src, n: [nx, ny, nz], bricks: [w.bx, w.by], plus, lower }
    }
}

impl FaceVelocity for CellVelocity<'_> {
    #[inline]
    fn face(&self, x: i64, y: i64, z: i64, axis: usize) -> i64 {
        let p = [x, y, z];
        let inside = |k: usize| p[k] >= 0 && (p[k] as usize) < self.n[k];
        if inside(0) && inside(1) && inside(2) {
            let (x, y, z) = (x as usize, y as usize, z as usize);
            let b = ((z / B) * self.bricks[1] + y / B) * self.bricks[0] + x / B;
            return self.plus[b * BRICK_CELLS + ((z % B) * B + y % B) * B + x % B][axis];
        }
        if p[axis] == -1 && (0..3).all(|k| k == axis || inside(k)) {
            let (x, y, z) = (x as usize, y as usize, z as usize);
            let k = match axis { 0 => z * self.n[1] + y, 1 => z * self.n[0] + x, _ => y * self.n[0] + x };
            return self.lower[axis][k];
        }
        self.src.face(x, y, z, axis)
    }
}

/// What carrying needs of a cell: its gas's temperature (what leaving gas
/// takes with it) and whether air passes through it.
#[derive(Copy, Clone, Default)]
struct Air { t: Temperature, permeable: bool }

/// Air carried by the flow, as `steps` sub-steps (each moves at most one
/// cell's worth). A cell sends `speed·dt/dx` of its gas out through each face
/// the air leaves by, scaled down if that would add up to more than all of it.
pub fn advect(world: &mut World, vel: &dyn FaceVelocity, steps: i64) {
    let vel = CellVelocity::new(world, vel);
    // Each cell's total outflow share: the flow is the same for every sub-step.
    let total = world.map_cells(|x, y, z, _| outflow_share(&vel, steps, x, y, z));
    for _ in 0..steps {
        let air = { let w: &World = world; w.map_cells(|_, _, _, i| { let c = &w.cells[i]; Air { t: c.gas_temperature(), permeable: c.permeable(&w.subs) } }) };
        let outside_t = world.outside_air.gas_temperature();
        world.run_phase(|w, b, out, tally| {
            let (ox, oy, oz) = w.brick_origin(b);
            let base = b * BRICK_CELLS;
            for l in 0..BRICK_CELLS {
                let (lx, ly, lz) = World::local_xyz(l);
                let (x, y, z) = (ox + lx, oy + ly, oz + lz);
                let i = base + l;
                let me = &w.cells[i];
                let my = air[i];
                let mut next = *me;
                // Nothing is carried into or out of a cell air can't pass through.
                if my.permeable {
                    for axis in 0..3 {
                        for dir in [-1i64, 1] {
                            let u = signed_face_velocity(w, &vel, x, y, z, axis, dir);
                            if u == 0 { continue }
                            match w.neighbour(x, y, z, axis, dir) {
                                Neighbour::Inside(n) => {
                                    let ni = neighbour_index(w, x, y, z, axis, dir);
                                    if !air[ni].permeable { continue }
                                    // Lower and upper cell along the axis; u > 0 flows lower → upper.
                                    let ((lo, lo_i), (hi, hi_i)) = if dir > 0 { ((me, i), (n, ni)) } else { ((n, ni), (me, i)) };
                                    let flux = if u > 0 { carry_from(lo, air[lo_i].t, face_share(total[lo_i], steps, u), 1) }
                                        else { carry_from(hi, air[hi_i].t, face_share(total[hi_i], steps, u), -1) };
                                    carry_apply(&mut next, &flux, -dir);
                                }
                                Neighbour::Outside(o) => {
                                    // Outside air coming in: the outside is a large reservoir, so its share is
                                    // simply speed·dt/dx of a fresh cell.
                                    let leaving = (u > 0) == (dir > 0);
                                    let flux = if leaving { carry_from(me, my.t, face_share(total[i], steps, u), if dir > 0 { 1 } else { -1 }) }
                                        else { carry_from(o, outside_t, Fraction(share_of_speed(u.abs(), steps).min(Fraction::ONE.0)), if dir > 0 { -1 } else { 1 }) };
                                    carry_apply(&mut next, &flux, -dir);
                                    tally_edge(tally, &flux, -dir);
                                }
                                Neighbour::Mirror => {}
                            }
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
fn outflow_share(vel: &dyn FaceVelocity, steps: i64, x: usize, y: usize, z: usize) -> u64 {
    let mut total: u64 = 0;
    for axis in 0..3 {
        let plus = vel.face(x as i64, y as i64, z as i64, axis);
        let mut p = [x as i64, y as i64, z as i64];
        p[axis] -= 1;
        let minus = vel.face(p[0], p[1], p[2], axis);
        if plus > 0 { total += share_of_speed(plus, steps) }
        if minus < 0 { total += share_of_speed(-minus, steps) }
    }
    total
}

/// |u|·dt/dx as a share of 2³²: dx = 100 mm, dt = 50 ms / steps.
#[inline]
fn share_of_speed(u: i64, steps: i64) -> u64 {
    div_u128(u as u128 * TICK.0 as u128 * (1u128 << 32), 100u128 * 1_000 * steps as u128) as u64
}

/// The share actually sent through one face, after scaling so the donor
/// (whose outflow shares add up to `total`) never sends more than it has.
#[inline]
fn face_share(total: u64, steps: i64, u: i64) -> Fraction {
    let s = share_of_speed(u.abs(), steps);
    if total <= Fraction::ONE.0 { return Fraction(s) }
    Fraction(div_u128(s as u128 * Fraction::ONE.0 as u128, total as u128) as u64)
}

/// What air carries across a face: `share` of the donor's gas at its
/// temperature `t`, with `sign` +1 from the lower cell to the upper.
fn carry_from(donor: &Cell, t: Temperature, share: Fraction, sign: i64) -> Flux {
    let mut f = Flux::default();
    for k in 0..GASES {
        let m = share.of_mass(donor.gas[k]);
        f.gas[k] = Mass(m.0 * sign);
        f.carried += Energy(carried(k, m, t).0 * sign);
    }
    f
}

/// [`apply`] for a carried flux (gas and the heat it holds, nothing conducted).
#[inline]
fn carry_apply(out: &mut Cell, f: &Flux, sign: i64) {
    out.energy += Energy(f.carried.0 * sign);
    for k in 0..GASES { out.gas[k] += Mass(f.gas[k].0 * sign) }
}
