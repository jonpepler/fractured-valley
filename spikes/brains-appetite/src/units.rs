//! Quantities with units, as exact integers, in the style of `fv-core`'s
//! `units.rs`: one newtype per kind, an integer count of a small base unit,
//! and only the products that mean something are defined. Intermediate
//! products go through `i128`; overflow panics instead of wrapping.
//!
//! | type             | base unit  | note                                         |
//! |------------------|------------|----------------------------------------------|
//! | [`Energy`]       | millijoule | a 20-day run moves ~3×10¹¹ mJ                |
//! | [`Power`]        | milliwatt  |                                              |
//! | [`Mass`]         | milligram  | a 72 kg body is 7.2×10⁷ mg                   |
//! | [`Duration`]     | second     | one body tick is 60 s                        |
//! | [`SpecificEnergy`] | kJ/kg    | = mJ/mg, so mass × specific energy is exact  |
//! | [`Q16`]          | 1/65536    | a dimensionless fraction or score            |

use core::ops::{Add, AddAssign, Mul, Sub, SubAssign};

macro_rules! quantity {
    ($(#[$m:meta])* $name:ident, $unit:literal) => {
        $(#[$m])*
        #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub i64);
        impl $name {
            pub const ZERO: Self = Self(0);
            pub const UNIT: &'static str = $unit;
            pub const fn raw(self) -> i64 { self.0 }
        }
        impl Add for $name { type Output = Self; fn add(self, o: Self) -> Self { Self(self.0.checked_add(o.0).expect(concat!(stringify!($name), " overflow"))) } }
        impl Sub for $name { type Output = Self; fn sub(self, o: Self) -> Self { Self(self.0.checked_sub(o.0).expect(concat!(stringify!($name), " overflow"))) } }
        impl AddAssign for $name { fn add_assign(&mut self, o: Self) { *self = *self + o } }
        impl SubAssign for $name { fn sub_assign(&mut self, o: Self) { *self = *self - o } }
    };
}

quantity!(
    /// Energy, in millijoules.
    Energy, "mJ");
quantity!(
    /// Power, in milliwatts.
    Power, "mW");
quantity!(
    /// Mass, in milligrams.
    Mass, "mg");
quantity!(
    /// A span of time, in seconds.
    Duration, "s");
quantity!(
    /// Chemical energy per mass, in kJ/kg (= mJ/mg).
    SpecificEnergy, "kJ/kg");

pub const fn joules(v: i64) -> Energy { Energy(v * 1_000) }
pub const fn kilojoules(v: i64) -> Energy { Energy(v * 1_000_000) }
pub const fn megajoules(v: i64) -> Energy { Energy(v * 1_000_000_000) }
pub const fn watts(v: i64) -> Power { Power(v * 1_000) }
pub const fn grams(v: i64) -> Mass { Mass(v * 1_000) }
pub const fn kilograms(v: i64) -> Mass { Mass(v * 1_000_000) }
pub const fn seconds(v: i64) -> Duration { Duration(v) }
pub const fn kj_per_kg(v: i64) -> SpecificEnergy { SpecificEnergy(v) }

/// power × duration = energy (mW × s = mJ, exactly).
impl Mul<Duration> for Power {
    type Output = Energy;
    fn mul(self, t: Duration) -> Energy { Energy(self.0.checked_mul(t.0).expect("energy overflow")) }
}

/// mass × specific energy = energy (mg × kJ/kg = mJ, exactly).
impl Mul<SpecificEnergy> for Mass {
    type Output = Energy;
    fn mul(self, e: SpecificEnergy) -> Energy { Energy(self.0.checked_mul(e.0).expect("energy overflow")) }
}

impl Energy {
    /// How many whole milligrams of a substance hold at most this energy.
    pub fn whole_mass_of(self, e: SpecificEnergy) -> Mass { Mass(self.0.div_euclid(e.0)) }
    /// The smallest whole mass of a substance holding at least this energy.
    pub fn mass_covering(self, e: SpecificEnergy) -> Mass { Mass((self.0 + e.0 - 1).div_euclid(e.0)) }
    /// A share in parts per thousand, rounding down. The rest is `self - share`.
    pub fn per_mille(self, pm: i64) -> Energy { Energy((self.0 as i128 * pm as i128 / 1_000) as i64) }
    /// A share as a Q16 fraction, rounding down.
    pub fn times(self, q: Q16) -> Energy { Energy(((self.0 as i128 * q.0 as i128) >> 16) as i64) }
    /// This energy as a fraction of another (clamped to 0..=8).
    pub fn ratio(self, of: Energy) -> Q16 { Q16::ratio(self.0, of.0) }
    pub fn kj(self) -> f64 { self.0 as f64 / 1e6 }
}
impl Mass {
    pub fn ratio(self, of: Mass) -> Q16 { Q16::ratio(self.0, of.0) }
    pub fn times(self, q: Q16) -> Mass { Mass(((self.0 as i128 * q.0 as i128) >> 16) as i64) }
    pub fn kg(self) -> f64 { self.0 as f64 / 1e6 }
}
impl Power {
    pub fn times(self, q: Q16) -> Power { Power(((self.0 as i128 * q.0 as i128) >> 16) as i64) }
    pub fn w(self) -> f64 { self.0 as f64 / 1e3 }
}

/// A dimensionless fixed-point number, 16 fractional bits. Fractions, curve
/// outputs, utility scores and habit gains are all `Q16`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Q16(pub i64);
impl Q16 {
    pub const ZERO: Q16 = Q16(0);
    pub const ONE: Q16 = Q16(1 << 16);
    /// From thousandths: `Q16::milli(250)` is 0.25.
    pub const fn milli(v: i64) -> Q16 { Q16(v * 65_536 / 1_000) }
    /// a / b, clamped to 0..=8 (a store can't be emptier than empty).
    pub fn ratio(a: i64, b: i64) -> Q16 {
        if b <= 0 { return Q16::ZERO }
        Q16(((a.max(0) as i128) << 16).checked_div(b as i128).unwrap().min(8 << 16) as i64)
    }
    pub fn clamp01(self) -> Q16 { Q16(self.0.clamp(0, 1 << 16)) }
    pub fn inv(self) -> Q16 { Q16((1 << 16) - self.clamp01().0) }
    pub fn f(self) -> f64 { self.0 as f64 / 65_536.0 }
}
impl Mul for Q16 {
    type Output = Q16;
    fn mul(self, o: Q16) -> Q16 { Q16(((self.0 as i128 * o.0 as i128) >> 16) as i64) }
}
impl Add for Q16 { type Output = Q16; fn add(self, o: Q16) -> Q16 { Q16(self.0 + o.0) } }
impl Sub for Q16 { type Output = Q16; fn sub(self, o: Q16) -> Q16 { Q16(self.0 - o.0) } }

/// Integer square root (floor), exact on every machine.
pub fn isqrt(n: u128) -> u128 {
    if n < 2 { return n }
    let mut x = 1u128 << ((128 - n.leading_zeros()).div_ceil(2));
    loop {
        let y = (x + n / x) / 2;
        if y >= x { return x }
        x = y;
    }
}

/// n^(3/4), floor-ish, as sqrt(n · sqrt(n)). Exact integers throughout.
pub fn pow_three_quarters(n: u128) -> u128 { isqrt(n * isqrt(n)) }
