//! Quantities with units, as exact integers.
//!
//! Every stored number in the simulation has one of these types. Each type is
//! an integer count of a small base unit, so every machine computes the same
//! bits and the energy books can balance exactly:
//!
//! | type             | base unit      | why this size                              |
//! |------------------|----------------|--------------------------------------------|
//! | [`Energy`]       | microjoule     | a 10 cm cube of stone holds ~6×10¹¹ µJ     |
//! | [`Mass`]         | microgram      | the air in a cell is ~1.2×10⁶ µg           |
//! | [`Temperature`]  | millikelvin    | absolute: a point, not a difference        |
//! | [`TempDelta`]    | millikelvin    | a difference between two temperatures      |
//! | [`HeatCapacity`] | nanojoule / K  | µg × J/(kg·K) lands here exactly           |
//! | [`SpecificHeat`] | J / (kg·K)     | the textbook number                        |
//! | [`SpecificEnergy`] | kJ / kg      | = µJ/µg: burning or boiling a mass multiplies exactly |
//! | [`Conductance`]  | microwatt / K  | heat flow per kelvin across a face         |
//! | [`Power`]        | microwatt      |                                            |
//! | [`Duration`]     | millisecond    | one tick is 50 ms                          |
//!
//! Following the Gonogo discipline: same dimension with a different meaning is
//! a different type (a temperature is not a temperature difference), only the
//! products that mean something are defined, and intermediate products go
//! through `i128` so they can't overflow silently.

use core::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// `n / d` for 128-bit integers, rounding toward zero: exactly what `n / d`
/// gives, but done in 64 bits when both fit, as they almost always do.
/// (A 128-bit division is a slow library call, above all in WebAssembly.)
#[inline]
pub fn div_i128(n: i128, d: i128) -> i128 {
    if let (Ok(a), Ok(b)) = (i64::try_from(n), i64::try_from(d)) {
        if b != 0 && !(a == i64::MIN && b == -1) { return (a / b) as i128 }
    }
    n / d
}

/// The unsigned twin of [`div_i128`].
#[inline]
pub fn div_u128(n: u128, d: u128) -> u128 {
    if let (Ok(a), Ok(b)) = (u64::try_from(n), u64::try_from(d)) {
        if b != 0 { return (a / b) as u128 }
    }
    n / d
}

/// `a × b / d` through 128 bits (no overflow), rounding toward zero.
#[inline]
pub fn mul_div(a: i64, b: i64, d: i64) -> i128 { div_i128(a as i128 * b as i128, d as i128) }

macro_rules! quantity {
    ($(#[$m:meta])* $name:ident, $unit:literal) => {
        $(#[$m])*
        #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub i64);
        impl $name {
            pub const ZERO: Self = Self(0);
            /// The raw count of base units.
            pub const fn raw(self) -> i64 { self.0 }
            pub const UNIT: &'static str = $unit;
        }
    };
}

macro_rules! additive {
    ($name:ident) => {
        impl Add for $name { type Output = Self; fn add(self, o: Self) -> Self { Self(self.0.checked_add(o.0).expect(concat!(stringify!($name), " overflow"))) } }
        impl Sub for $name { type Output = Self; fn sub(self, o: Self) -> Self { Self(self.0.checked_sub(o.0).expect(concat!(stringify!($name), " overflow"))) } }
        impl AddAssign for $name { fn add_assign(&mut self, o: Self) { *self = *self + o } }
        impl SubAssign for $name { fn sub_assign(&mut self, o: Self) { *self = *self - o } }
        impl Neg for $name { type Output = Self; fn neg(self) -> Self { Self(-self.0) } }
        impl core::iter::Sum for $name { fn sum<I: Iterator<Item = Self>>(i: I) -> Self { i.fold(Self::ZERO, |a, b| a + b) } }
    };
}

quantity!(
    /// Energy, in microjoules.
    Energy, "µJ");
additive!(Energy);
quantity!(
    /// Mass, in micrograms.
    Mass, "µg");
additive!(Mass);
quantity!(
    /// An absolute temperature, in millikelvin. A point: two temperatures
    /// don't add, and their difference is a [`TempDelta`].
    Temperature, "mK");
quantity!(
    /// A difference between two temperatures, in millikelvin.
    TempDelta, "mK");
additive!(TempDelta);
quantity!(
    /// Heat capacity of a body, in nanojoules per kelvin.
    HeatCapacity, "nJ/K");
additive!(HeatCapacity);
quantity!(
    /// Specific heat of a substance, in J/(kg·K).
    SpecificHeat, "J/(kg·K)");
quantity!(
    /// Energy per mass (heat of combustion, latent heat), in kJ/kg (= µJ/µg).
    SpecificEnergy, "kJ/kg");
quantity!(
    /// Heat flow per kelvin of difference, in microwatts per kelvin.
    Conductance, "µW/K");
additive!(Conductance);
quantity!(
    /// Power, in microwatts.
    Power, "µW");
additive!(Power);
quantity!(
    /// A span of time, in milliseconds.
    Duration, "ms");
quantity!(
    /// A length, in millimetres.
    Length, "mm");
additive!(Length);
quantity!(
    /// A speed, in millimetres per second.
    Speed, "mm/s");
additive!(Speed);

// ── constructors in everyday units ─────────────────────────────────────────

pub const fn microjoules(v: i64) -> Energy { Energy(v) }
pub const fn millijoules(v: i64) -> Energy { Energy(v * 1_000) }
pub const fn joules(v: i64) -> Energy { Energy(v * 1_000_000) }
pub const fn kilojoules(v: i64) -> Energy { Energy(v * 1_000_000_000) }
pub const fn micrograms(v: i64) -> Mass { Mass(v) }
pub const fn milligrams(v: i64) -> Mass { Mass(v * 1_000) }
pub const fn grams(v: i64) -> Mass { Mass(v * 1_000_000) }
pub const fn kelvin(v: i64) -> Temperature { Temperature(v * 1_000) }
/// Celsius to the absolute scale (0 °C = 273.15 K).
pub const fn celsius(v: i64) -> Temperature { Temperature(v * 1_000 + 273_150) }
pub const fn kelvin_delta(v: i64) -> TempDelta { TempDelta(v * 1_000) }
pub const fn j_per_kg_k(v: i64) -> SpecificHeat { SpecificHeat(v) }
pub const fn kj_per_kg(v: i64) -> SpecificEnergy { SpecificEnergy(v) }
pub const fn mj_per_kg(v: i64) -> SpecificEnergy { SpecificEnergy(v * 1_000) }
pub const fn watts(v: i64) -> Power { Power(v * 1_000_000) }
pub const fn milliseconds(v: i64) -> Duration { Duration(v) }
pub const fn millimetres(v: i64) -> Length { Length(v) }
pub const fn mm_per_s(v: i64) -> Speed { Speed(v) }

// ── temperature: a point ───────────────────────────────────────────────────

impl Sub for Temperature {
    type Output = TempDelta;
    fn sub(self, o: Self) -> TempDelta { TempDelta(self.0 - o.0) }
}
impl Add<TempDelta> for Temperature {
    type Output = Temperature;
    fn add(self, d: TempDelta) -> Temperature { Temperature(self.0 + d.0) }
}
impl Temperature {
    pub fn kelvin_f64(self) -> f64 { self.0 as f64 / 1_000.0 }
    pub fn celsius_f64(self) -> f64 { (self.0 - 273_150) as f64 / 1_000.0 }
    /// Whole kelvin, for table lookups.
    pub fn whole_kelvin(self) -> i64 { self.0.div_euclid(1_000) }
}

// ── the products that mean something ────────────────────────────────────────

/// mass × specific heat = heat capacity (µg × J/(kg·K) = nJ/K, exactly).
impl Mul<SpecificHeat> for Mass {
    type Output = HeatCapacity;
    fn mul(self, c: SpecificHeat) -> HeatCapacity { HeatCapacity(self.0.checked_mul(c.0).expect("heat capacity overflow")) }
}

/// mass × specific energy = energy (µg × kJ/kg = µJ, exactly).
impl Mul<SpecificEnergy> for Mass {
    type Output = Energy;
    fn mul(self, e: SpecificEnergy) -> Energy { Energy(self.0.checked_mul(e.0).expect("energy overflow")) }
}

/// heat capacity × absolute temperature = the thermal energy above absolute zero.
/// (nJ/K × mK = 10⁻¹² J = 10⁻⁶ µJ.) Rounds toward zero.
impl Mul<Temperature> for HeatCapacity {
    type Output = Energy;
    fn mul(self, t: Temperature) -> Energy { Energy(i64::try_from(mul_div(self.0, t.0, 1_000_000)).expect("energy overflow")) }
}

/// heat capacity × temperature difference = energy.
impl Mul<TempDelta> for HeatCapacity {
    type Output = Energy;
    fn mul(self, t: TempDelta) -> Energy { Energy(i64::try_from(mul_div(self.0, t.0, 1_000_000)).expect("energy overflow")) }
}

/// energy ÷ heat capacity = temperature above absolute zero.
impl Div<HeatCapacity> for Energy {
    type Output = Temperature;
    fn div(self, c: HeatCapacity) -> Temperature {
        if c.0 <= 0 { return Temperature(0) }
        Temperature(i64::try_from(mul_div(self.0, 1_000_000, c.0)).expect("temperature overflow"))
    }
}

/// conductance × temperature difference = power (µW/K × mK = nW... kept exact
/// by dividing by 1000).
impl Mul<TempDelta> for Conductance {
    type Output = Power;
    fn mul(self, d: TempDelta) -> Power { Power(i64::try_from(mul_div(self.0, d.0, 1_000)).expect("power overflow")) }
}

/// power × duration = energy (µW × ms = 10⁻⁹ J = 10⁻³ µJ). Rounds toward zero.
impl Mul<Duration> for Power {
    type Output = Energy;
    fn mul(self, t: Duration) -> Energy { Energy(i64::try_from(mul_div(self.0, t.0, 1_000)).expect("energy overflow")) }
}

/// speed × duration = length (mm/s × ms = µm; kept in mm, rounding toward zero).
impl Mul<Duration> for Speed {
    type Output = Length;
    fn mul(self, t: Duration) -> Length { Length(self.0 * t.0 / 1_000) }
}

/// A fraction in parts of 2³², for rates per tick and shares.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fraction(pub u64);
impl Fraction {
    pub const ONE: Fraction = Fraction(1 << 32);
    pub const ZERO: Fraction = Fraction(0);
    pub fn from_f64(v: f64) -> Fraction { Fraction((v.clamp(0.0, 1.0) * (1u64 << 32) as f64) as u64) }
    /// This share of a mass, rounding toward zero.
    pub fn of_mass(self, m: Mass) -> Mass { Mass((m.0 as i128 * self.0 as i128 >> 32) as i64) }
    pub fn of_energy(self, e: Energy) -> Energy { Energy((e.0 as i128 * self.0 as i128 >> 32) as i64) }
    pub fn of(self, v: i64) -> i64 { (v as i128 * self.0 as i128 >> 32) as i64 }
    pub fn min(self, o: Fraction) -> Fraction { if self.0 < o.0 { self } else { o } }
}
