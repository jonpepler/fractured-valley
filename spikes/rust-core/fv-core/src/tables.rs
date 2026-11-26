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
    /// The same split for soot, which is not grey: small carbon particles
    /// absorb, and so emit, more strongly at shorter wavelengths, ε ∝ λ^−α
    /// with α = 0.83 ± 0.08 measured from 0.51 to 5.2 µm (Köylü & Faeth
    /// 1996; Siddall & McGrath 1963 found 0.93 for candle soot in the
    /// infrared). That tips a flame's light toward the visible: at 1 400 K
    /// about 0.08 % is visible instead of a grey body's 0.025 %. Only the
    /// split changes; how much a flame or a sooty gas emits in total is set
    /// elsewhere.
    pub soot_bands: Vec<[u64; 4]>,
    /// Rothermel's effective heating number ε = exp(−138/σ) (σ in 1/ft),
    /// per mille, by σ in 1/m: the share of a fine fuel that must reach
    /// ignition before it lights (its surface layer; thin blades nearly all).
    pub heating_permille: Vec<i64>,
}

/// Largest surface-to-volume ratio the heating table covers, 1/m.
pub const MAX_SIGMA: usize = 20_000;

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

/// Soot's spectral emissivity exponent α (ε ∝ λ^−α), Köylü & Faeth (1996).
const SOOT_ALPHA: f64 = 0.83;

/// Share of soot's emission at wavelengths below λ, by x = c₂/(λT), for
/// ε ∝ λ^−α: emission per dx goes as x^(3+α)/(eˣ − 1), the same for every
/// temperature, so it is integrated once (Simpson's rule, step 1/1000) and
/// looked up. Returns the cumulative integral from each x up, normalised.
fn soot_above_x() -> (Vec<f64>, f64) {
    let h = 0.001;
    let n = 80_000; // x up to 80: e^−80 is far below anything that matters
    let f = |x: f64| if x <= 0.0 { 0.0 } else { libm::pow(x, 3.0 + SOOT_ALPHA) / libm::expm1(x) };
    // Integral over each step [i·h, (i+1)·h], then summed from the top.
    let mut above = vec![0.0; n + 1];
    for i in (0..n).rev() {
        let (a, b) = (i as f64 * h, (i + 1) as f64 * h);
        above[i] = above[i + 1] + (b - a) / 6.0 * (f(a) + 4.0 * f((a + b) / 2.0) + f(b));
    }
    let total = above[0];
    (above.into_iter().map(|v| v / total).collect(), h)
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
    let (above, h) = soot_above_x();
    let soot_below = |nm: f64, t: f64| {
        let x = 14_387.77 / (nm / 1000.0 * t);
        let at = x / h;
        let i = at.floor() as usize;
        if i + 1 >= above.len() { return 0.0 }
        let f = at - i as f64;
        above[i] + (above[i + 1] - above[i]) * f
    };
    let soot_bands = (0..=MAX_K).map(|k| {
        if k < 300 { return [Fraction::ONE.0, 0, 0, 0] }
        let t = k as f64;
        let (b700, b600, b500) = (soot_below(700.0, t), soot_below(600.0, t), soot_below(500.0, t));
        let one = Fraction::ONE.0 as f64;
        let red = ((b700 - b600) * one) as u64;
        let green = ((b600 - b500) * one) as u64;
        let blue = (b500 * one) as u64;
        [Fraction::ONE.0 - red - green - blue, red, green, blue]
    }).collect();
    Tables {
        soot_bands,
        pyrolysis: arrhenius(1.0e10, 140_000.0),
        char_burn: arrhenius(2.0e5, 100_000.0),
        gas_burn: arrhenius(1.0e9, 110_000.0),
        soot_burn: arrhenius(1.0e8, 150_000.0),
        saturated_ug_per_l: saturated,
        sigma_t4,
        bands,
        heating_permille: (0..=MAX_SIGMA).map(|sigma| {
            if sigma == 0 { return 0 }
            (libm::exp(-138.0 / (sigma as f64 * 0.3048)) * 1000.0).round() as i64
        }).collect(),
    }
}

/// Table index for a temperature in millikelvin.
#[inline]
pub fn k_index(t_mk: i64) -> usize { (t_mk / 1_000).clamp(0, MAX_K as i64) as usize }
