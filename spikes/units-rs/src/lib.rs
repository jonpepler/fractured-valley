//! Spike: dimension-checked, fixed-point quantities for the simulation core.
//!
//! Modelled on Gonogo's units discipline (see the store repo's
//! `research/reference/gonogo-units-system.md`), adapted to a deterministic
//! Rust core:
//!
//! - Every quantity's **dimension is part of its type** (powers of length,
//!   mass, time and temperature, as type-level integers). Adding a length to
//!   a time does not compile; energy ÷ time *is* power, worked out by the
//!   compiler.
//! - Magnitudes are **fixed-point integers** (Q32.32 in an `i64`, products
//!   through `i128`), so every machine gets identical bits. Overflow panics
//!   rather than wrapping silently.
//! - **Same dimension, different meaning** is a separate type, as in Gonogo:
//!   an absolute temperature is a *point*, a temperature difference is a
//!   *vector* (`ut` versus `s` there), and a moisture content is a fraction
//!   of dry mass that does not add to other dimensionless numbers.
//!
//! The things that must not compile are pinned as `compile_fail` doc tests:
//!
//! These are illustrations. The real guard is `tests/refusals.rs`, which
//! checks each snippet fails *with the expected error*: stable rustdoc
//! ignores the error codes below, so on their own they could pass for any
//! reason (Gonogo's lesson: a check that can fail for any reason proves
//! nothing).
//!
//! ```compile_fail,E0308
//! use fv_units::*;
//! let _ = metres(1.0) + seconds(1.0); // length and time
//! ```
//! ```compile_fail,E0369
//! use fv_units::*;
//! let _ = celsius(20.0) + celsius(30.0); // two instants of temperature
//! ```
//! ```compile_fail,E0308
//! use fv_units::*;
//! let p: Power = joules(10.0); // energy is not power
//! ```
//! ```compile_fail,E0308
//! use fv_units::*;
//! let _ = moisture(0.5) + ratio(0.5); // moisture is its own kind
//! ```
//! ```compile_fail,E0369
//! use fv_units::*;
//! let _ = celsius(20.0) * 2; // twice a temperature is meaningless
//! ```
//!
//! And these do:
//!
//! ```
//! use fv_units::*;
//! let e: Energy = watts(100.0) * seconds(2.0);
//! let rise: TempDelta = celsius(300.0) - celsius(20.0);
//! let heat: Energy = kilograms(0.35) * WOOD_HEAT_CAPACITY * rise;
//! assert!(heat > e);
//! ```

#![forbid(unsafe_code)]

use core::cmp::Ordering;
use core::fmt;
use core::marker::PhantomData;
use core::ops::{Add, Div, Mul, Neg, Sub};
use typenum::{Diff, Integer, Sum, N1, N2, N3, P1, P2, P3, Z0};

// ── dimensions ──────────────────────────────────────────────────────────────

/// A dimension: powers of length (m), mass (kg), time (s) and temperature (K).
pub struct Dim<L, M, T, K>(PhantomData<(L, M, T, K)>);

/// Reads a dimension's powers at runtime (for printing).
pub trait Dimension {
    const POWERS: [i32; 4];
}
impl<L: Integer, M: Integer, T: Integer, K: Integer> Dimension for Dim<L, M, T, K> {
    const POWERS: [i32; 4] = [L::I32, M::I32, T::I32, K::I32];
}

/// The dimension of a product: powers add.
pub trait DimMul<Rhs> {
    type Output;
}
impl<L1, M1, T1, K1, L2, M2, T2, K2> DimMul<Dim<L2, M2, T2, K2>> for Dim<L1, M1, T1, K1>
where
    L1: Add<L2>,
    M1: Add<M2>,
    T1: Add<T2>,
    K1: Add<K2>,
{
    type Output = Dim<Sum<L1, L2>, Sum<M1, M2>, Sum<T1, T2>, Sum<K1, K2>>;
}

/// The dimension of a quotient: powers subtract.
pub trait DimDiv<Rhs> {
    type Output;
}
impl<L1, M1, T1, K1, L2, M2, T2, K2> DimDiv<Dim<L2, M2, T2, K2>> for Dim<L1, M1, T1, K1>
where
    L1: Sub<L2>,
    M1: Sub<M2>,
    T1: Sub<T2>,
    K1: Sub<K2>,
{
    type Output = Dim<Diff<L1, L2>, Diff<M1, M2>, Diff<T1, T2>, Diff<K1, K2>>;
}

// ── kinds: same dimension, different meaning ────────────────────────────────

/// What a quantity *means*, beyond its dimension. Only quantities of the
/// same kind add or compare; products and quotients are `Plain`.
pub trait Kind {}
/// An ordinary physical quantity.
pub struct Plain;
/// Water as a fraction of dry mass (0.12 = 12%). Dimensionless, but not a
/// ratio you can add to other ratios.
pub struct MoistureKind;
impl Kind for Plain {}
impl Kind for MoistureKind {}

// ── the quantity ────────────────────────────────────────────────────────────

/// Fixed-point fraction bits: Q32.32.
pub const FRAC: u32 = 32;
const ONE: i64 = 1 << FRAC;

/// A magnitude in SI base units, fixed-point, with its dimension and kind in the type.
pub struct Quantity<D, K: Kind = Plain> {
    raw: i64,
    _d: PhantomData<(D, K)>,
}

impl<D, K: Kind> Quantity<D, K> {
    /// From an f64 in SI base units. Meant for constants (evaluated by the
    /// compiler, so identical everywhere) and tests, never for simulation input.
    pub const fn from_si(v: f64) -> Self {
        Self { raw: (v * ONE as f64) as i64, _d: PhantomData }
    }
    pub const fn from_raw(raw: i64) -> Self {
        Self { raw, _d: PhantomData }
    }
    pub const fn raw(self) -> i64 {
        self.raw
    }
    /// For display and tests only.
    pub fn to_si(self) -> f64 {
        self.raw as f64 / ONE as f64
    }
    pub const ZERO: Self = Self::from_raw(0);
}

impl<D, K: Kind> Clone for Quantity<D, K> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<D, K: Kind> Copy for Quantity<D, K> {}
impl<D, K: Kind> PartialEq for Quantity<D, K> {
    fn eq(&self, o: &Self) -> bool {
        self.raw == o.raw
    }
}
impl<D, K: Kind> Eq for Quantity<D, K> {}
impl<D, K: Kind> PartialOrd for Quantity<D, K> {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl<D, K: Kind> Ord for Quantity<D, K> {
    fn cmp(&self, o: &Self) -> Ordering {
        self.raw.cmp(&o.raw)
    }
}

/// Same dimension and kind only.
impl<D, K: Kind> Add for Quantity<D, K> {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::from_raw(self.raw.checked_add(o.raw).expect("quantity overflow"))
    }
}
impl<D, K: Kind> Sub for Quantity<D, K> {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::from_raw(self.raw.checked_sub(o.raw).expect("quantity overflow"))
    }
}
impl<D, K: Kind> Neg for Quantity<D, K> {
    type Output = Self;
    fn neg(self) -> Self {
        Self::from_raw(-self.raw)
    }
}

fn mul_raw(a: i64, b: i64) -> i64 {
    let p = (a as i128 * b as i128) >> FRAC;
    i64::try_from(p).expect("quantity overflow")
}
fn div_raw(a: i64, b: i64) -> i64 {
    assert!(b != 0, "division by a zero quantity");
    i64::try_from(((a as i128) << FRAC) / b as i128).expect("quantity overflow")
}

/// Any quantity × any quantity: dimensions multiply, the result is Plain.
impl<D1, K1: Kind, D2, K2: Kind> Mul<Quantity<D2, K2>> for Quantity<D1, K1>
where
    D1: DimMul<D2>,
{
    type Output = Quantity<<D1 as DimMul<D2>>::Output, Plain>;
    fn mul(self, o: Quantity<D2, K2>) -> Self::Output {
        Quantity::from_raw(mul_raw(self.raw, o.raw))
    }
}
impl<D1, K1: Kind, D2, K2: Kind> Div<Quantity<D2, K2>> for Quantity<D1, K1>
where
    D1: DimDiv<D2>,
{
    type Output = Quantity<<D1 as DimDiv<D2>>::Output, Plain>;
    fn div(self, o: Quantity<D2, K2>) -> Self::Output {
        Quantity::from_raw(div_raw(self.raw, o.raw))
    }
}
/// Scaling by a plain integer count keeps the unit.
impl<D, K: Kind> Mul<i64> for Quantity<D, K> {
    type Output = Self;
    fn mul(self, n: i64) -> Self {
        Self::from_raw(self.raw.checked_mul(n).expect("quantity overflow"))
    }
}

impl<D: Dimension, K: Kind> fmt::Debug for Quantity<D, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.to_si(), unit_name(D::POWERS))
    }
}

/// The base-unit spelling of a dimension, e.g. `kg·m²·s⁻²`.
pub fn unit_name(p: [i32; 4]) -> String {
    let names = ["m", "kg", "s", "K"];
    let parts: Vec<String> = names
        .iter()
        .zip(p)
        .filter(|(_, e)| *e != 0)
        .map(|(n, e)| if e == 1 { n.to_string() } else { format!("{n}^{e}") })
        .collect();
    if parts.is_empty() { "1".into() } else { parts.join("·") }
}

// ── named dimensions ────────────────────────────────────────────────────────

pub type Dimensionless = Quantity<Dim<Z0, Z0, Z0, Z0>>;
pub type Moisture = Quantity<Dim<Z0, Z0, Z0, Z0>, MoistureKind>;
pub type Length = Quantity<Dim<P1, Z0, Z0, Z0>>;
pub type Volume = Quantity<Dim<P3, Z0, Z0, Z0>>;
pub type Mass = Quantity<Dim<Z0, P1, Z0, Z0>>;
pub type Time = Quantity<Dim<Z0, Z0, P1, Z0>>;
pub type TempDelta = Quantity<Dim<Z0, Z0, Z0, P1>>;
pub type Density = Quantity<Dim<N3, P1, Z0, Z0>>;
pub type Energy = Quantity<Dim<P2, P1, N2, Z0>>;
pub type Power = Quantity<Dim<P2, P1, N3, Z0>>;
/// J/(kg·K)
pub type HeatCapacity = Quantity<Dim<P2, Z0, N2, N1>>;
/// J/kg (latent heat, heat of combustion)
pub type SpecificEnergy = Quantity<Dim<P2, Z0, N2, Z0>>;

// ── absolute temperature: a point, not a vector ─────────────────────────────

/// An absolute temperature. Points subtract to a `TempDelta`, take a delta
/// added, and compare; they never add to each other or scale.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Temperature(TempDelta); // stored as kelvin above absolute zero

impl Temperature {
    pub const fn kelvin(k: f64) -> Self {
        Self(TempDelta::from_si(k))
    }
    pub fn to_celsius(self) -> f64 {
        self.0.to_si() - 273.15
    }
}
impl Sub for Temperature {
    type Output = TempDelta;
    fn sub(self, o: Self) -> TempDelta {
        self.0 - o.0
    }
}
impl Add<TempDelta> for Temperature {
    type Output = Self;
    fn add(self, d: TempDelta) -> Self {
        Self(self.0 + d)
    }
}
impl fmt::Debug for Temperature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1} °C", self.to_celsius())
    }
}

// ── constructors (SI in, as the substance table would write them) ──────────

pub const fn metres(v: f64) -> Length { Length::from_si(v) }
pub const fn seconds(v: f64) -> Time { Time::from_si(v) }
pub const fn kilograms(v: f64) -> Mass { Mass::from_si(v) }
pub const fn joules(v: f64) -> Energy { Energy::from_si(v) }
pub const fn watts(v: f64) -> Power { Power::from_si(v) }
pub const fn kelvin_delta(v: f64) -> TempDelta { TempDelta::from_si(v) }
pub const fn celsius(v: f64) -> Temperature { Temperature::kelvin(v + 273.15) }
pub const fn moisture(fraction_of_dry_mass: f64) -> Moisture { Moisture::from_si(fraction_of_dry_mass) }
pub const fn ratio(v: f64) -> Dimensionless { Dimensionless::from_si(v) }

impl Moisture {
    /// The water in fuel of this dry mass. (Moisture × mass → mass of water.)
    pub fn water_in(self, dry: Mass) -> Mass {
        Mass::from_raw(mul_raw(self.raw(), dry.raw()))
    }
}

// ── a slice of the substance table, with units ──────────────────────────────

pub const WOOD_DENSITY: Density = Density::from_si(500.0); // dry, kg/m³
pub const WOOD_HEAT_CAPACITY: HeatCapacity = HeatCapacity::from_si(1700.0); // J/(kg·K)
pub const WOOD_DECOMPOSES_AT: Temperature = celsius(300.0); // releases volatiles
pub const WATER_HEAT_CAPACITY: HeatCapacity = HeatCapacity::from_si(4186.0);
pub const WATER_BOILS_AT: Temperature = celsius(100.0);
pub const WATER_LATENT_HEAT: SpecificEnergy = SpecificEnergy::from_si(2.257e6); // J/kg
/// One 10 cm cell.
pub const CELL_VOLUME: Volume = Volume::from_si(0.001);

/// The energy a cell of fuel must absorb before it starts giving off
/// burnable gas: warm the dry fuel to its decomposition temperature, warm
/// its water to boiling, and boil the water off. This is the "lines
/// crossing" threshold, derived, with no ignition point anywhere.
pub fn heat_to_ignite(
    dry: Mass,
    wet: Moisture,
    c_dry: HeatCapacity,
    from: Temperature,
    decomposes_at: Temperature,
) -> Energy {
    let water = wet.water_in(dry);
    let warm_fuel = dry * c_dry * (decomposes_at - from);
    let warm_water = water * WATER_HEAT_CAPACITY * (WATER_BOILS_AT - from);
    let boil_off = water * WATER_LATENT_HEAT;
    warm_fuel + warm_water + boil_off
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell_of_wood(wet: Moisture) -> Energy {
        let dry: Mass = WOOD_DENSITY * CELL_VOLUME;
        heat_to_ignite(dry, wet, WOOD_HEAT_CAPACITY, celsius(15.0), WOOD_DECOMPOSES_AT)
    }

    #[test]
    fn a_living_trunk_needs_several_times_the_heat_of_a_dry_post() {
        let post = cell_of_wood(moisture(0.12));
        let trunk = cell_of_wood(moisture(1.0));
        println!("dry post cell: {post:?}, living trunk cell: {trunk:?}");
        assert!(trunk > post * 3);
    }

    /// The derived threshold reproduces fire science's measured line:
    /// Rothermel's heat of pre-ignition, Q_ig = 250 + 1116·Mf Btu/lb
    /// (= 581.5 + 2595.8·Mf kJ/kg). Ours comes only from substance constants.
    #[test]
    fn the_derived_threshold_matches_rothermel() {
        let per_kg = |m: f64| {
            heat_to_ignite(kilograms(1.0), moisture(m), WOOD_HEAT_CAPACITY, celsius(15.0), WOOD_DECOMPOSES_AT).to_si()
        };
        for m in [0.0, 0.1, 0.3, 1.0, 2.0] {
            let ours = per_kg(m) / 1000.0;
            let rothermel = 581.5 + 2595.8 * m;
            let off = (ours - rothermel).abs() / rothermel;
            println!("moisture {m:>4}: ours {ours:7.1} kJ/kg, Rothermel {rothermel:7.1}, {:.0}% apart", off * 100.0);
            assert!(off < 0.2, "{m}: {ours} vs {rothermel}");
        }
    }

    #[test]
    fn energy_over_time_is_power_and_back() {
        let p: Power = joules(600.0) / seconds(2.0);
        assert_eq!(p, watts(300.0));
        let e: Energy = p * seconds(2.0);
        assert_eq!(e, joules(600.0));
    }

    #[test]
    fn temperatures_are_points() {
        let rise: TempDelta = celsius(300.0) - celsius(20.0);
        assert_eq!(rise, kelvin_delta(280.0));
        assert_eq!(celsius(20.0) + rise, celsius(300.0));
    }

    #[test]
    fn fixed_point_is_exact_and_identical_everywhere() {
        // The same inputs give the same raw bits; no float in the arithmetic.
        let a = cell_of_wood(moisture(0.5));
        let b = cell_of_wood(moisture(0.5));
        assert_eq!(a.raw(), b.raw());
    }

    #[test]
    #[should_panic(expected = "quantity overflow")]
    fn overflow_is_loud_not_silent() {
        // T⁴ for radiation overflows Q32.32 (1000⁴ = 10¹²): the type system
        // can't see that, so it fails loudly. Radiation needs its own scale.
        let t = kelvin_delta(1000.0);
        let _ = t * t * t * t;
    }

    #[test]
    fn debug_prints_the_unit() {
        assert_eq!(format!("{:?}", watts(2.0)), "2 m^2·kg·s^-3");
    }
}
