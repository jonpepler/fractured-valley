//! Physical curves as integer tables, one entry per kelvin.
//!
//! Rates and radiation depend on temperature through exponentials and powers.
//! Computing those in floating point at run time could differ between
//! machines in the last bit, so they're computed once, by the pure-Rust
//! `libm` (the same code on every platform), and rounded into integers. Every
//! lookup after that is integer.

use crate::units::Fraction;
use std::sync::OnceLock;

/// Highest tabulated temperature, in kelvin (the sun's surface is ~5800 K).
pub const MAX_K: usize = 6_000;
/// Seconds per tick.
const DT: f64 = 0.05;
const R_GAS: f64 = 8.314_462_618;
const SIGMA: f64 = 5.670_374_419e-8;

pub struct Tables {
    /// Share of the volatile part of organic matter turned to gas per tick
    /// (wood pyrolysis, one-step: A = 1×10¹⁰ /s, E = 140 kJ/mol; Di Blasi 2008).
    pub pyrolysis: Vec<Fraction>,
    /// Share of char burnt per tick in fresh air (A = 2×10⁵ /s, E = 100 kJ/mol),
    /// scaled by the oxygen present.
    pub char_burn: Vec<Fraction>,
    /// Share of burnable gas burnt per tick where there's oxygen
    /// (A = 1×10⁹ /s, E = 110 kJ/mol: fast above ~650 K).
    pub gas_burn: Vec<Fraction>,
    /// Share of soot burnt per tick in fresh air (A = 1×10⁸ /s, E = 150 kJ/mol).
    pub soot_burn: Vec<Fraction>,
    /// Water vapour a litre of gas holds when saturated, in µg
    /// (Magnus formula; above boiling, effectively unlimited).
    pub saturated_ug_per_l: Vec<i64>,
    /// σ·T⁴ in µW per m², per kelvin.
    pub sigma_t4: Vec<i64>,
    /// Planck shares of emission in [infrared, red, green, blue] (blue
    /// includes everything shorter), out of 2³², per kelvin.
    pub bands: Vec<[u64; 4]>,
}

pub fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(build)
}

fn per_tick(rate_per_s: f64) -> Fraction {
    // 1 − e^(−k·dt): the share gone in one tick, never more than all of it.
    Fraction::from_f64(-libm::expm1(-rate_per_s * DT))
}

fn arrhenius(a: f64, e_j_per_mol: f64) -> Vec<Fraction> {
    (0..=MAX_K).map(|k| if k < 200 { Fraction::ZERO } else { per_tick(a * libm::exp(-e_j_per_mol / (R_GAS * k as f64))) }).collect()
}

/// Share of black-body emission at wavelengths below λ, for λT in µm·K.
fn planck_below(lambda_t_um_k: f64) -> f64 {
    let c2 = 14_387.77; // µm·K
    let x = c2 / lambda_t_um_k;
    if x > 200.0 { return 0.0 }
    let mut sum = 0.0;
    for n in 1..=60 {
        let nf = n as f64;
        sum += libm::exp(-nf * x) / nf * (x * x * x + 3.0 * x * x / nf + 6.0 * x / (nf * nf) + 6.0 / (nf * nf * nf));
    }
    (15.0 / libm::pow(core::f64::consts::PI, 4.0) * sum).clamp(0.0, 1.0)
}

fn build() -> Tables {
    let saturated = (0..=MAX_K).map(|k| {
        let c = k as f64 - 273.15;
        if c >= 100.0 { return i64::MAX / 4 }
        let c = c.max(-60.0);
        let p = 610.94 * libm::exp(17.625 * c / (c + 243.04)); // Pa
        // ideal gas: ρ = p·M/(R·T), M = 18.015 g/mol → g/m³ = mg/L; ×1000 for µg/L
        (p * 18.015 / (R_GAS * (c + 273.15)) * 1000.0).round() as i64
    }).collect();
    let sigma_t4 = (0..=MAX_K).map(|k| (SIGMA * libm::pow(k as f64, 4.0) * 1e6).round() as i64).collect();
    let bands = (0..=MAX_K).map(|k| {
        if k < 300 { return [Fraction::ONE.0, 0, 0, 0] }
        let t = k as f64;
        let below = |nm: f64| planck_below(nm / 1000.0 * t);
        let (b700, b600, b500) = (below(700.0), below(600.0), below(500.0));
        let one = Fraction::ONE.0 as f64;
        let red = ((b700 - b600) * one) as u64;
        let green = ((b600 - b500) * one) as u64;
        let blue = (b500 * one) as u64;
        [Fraction::ONE.0 - red - green - blue, red, green, blue]
    }).collect();
    Tables {
        pyrolysis: arrhenius(1.0e10, 140_000.0),
        char_burn: arrhenius(2.0e5, 100_000.0),
        gas_burn: arrhenius(1.0e9, 110_000.0),
        soot_burn: arrhenius(1.0e8, 150_000.0),
        saturated_ug_per_l: saturated,
        sigma_t4,
        bands,
    }
}

/// Table index for a temperature in millikelvin.
#[inline]
pub fn k_index(t_mk: i64) -> usize { (t_mk / 1_000).clamp(0, MAX_K as i64) as usize }
