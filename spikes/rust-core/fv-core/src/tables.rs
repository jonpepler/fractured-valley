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
    /// Soot's absorption coefficient in the infrared band (λ > 700 nm), Q16
    /// per metre per mg/L of soot, per kelvin of the light's source: the
    /// band mean of κ_λ = C·c·λ^−α weighted by Planck's spectrum at that
    /// temperature (see [`SOOT_ALPHA`], [`SOOT_C`]). Infrared spans a
    /// factor of hundreds in wavelength, over which κ_λ changes several
    /// times, so its mean depends on whose light it is: a cool wall's
    /// infrared lies further out, where soot absorbs less.
    pub soot_kappa_ir: Vec<i64>,
    /// Soot's absorption coefficient in red (600–700 nm), green (500–600)
    /// and blue (400–500), Q16 per metre per mg/L: plain means of κ_λ over
    /// each band. They are narrow, so the light's spectrum barely shifts
    /// them (a 1 400 K source's Planck weighting lowers them 2–5 %).
    pub soot_kappa_vis: [i64; 3],
    /// What a sooty volume emits in each band [IR, R, G, B], out of 2³² of
    /// 4·κ_ref·V·σT⁴ (κ_ref = 1 per metre per mg/L), per kelvin: the band's
    /// κ times its Planck share, so emission is 4V∫κ_λB_λ dλ by the very κ
    /// that absorbs (Kirchhoff). The row sums to the Planck-mean κ_P(T)/κ_ref:
    /// one at 1 400 K, rising as T^0.83.
    pub soot_emission: Vec<[u64; 4]>,
    /// The same, normalised: how soot's emission splits between bands, out
    /// of 2³² (each row sums to exactly 2³²). Soot is not grey: small carbon
    /// particles absorb, and so emit, more strongly at shorter wavelengths,
    /// which tips a flame's light toward the visible (at 1 400 K ~0.09 %
    /// visible against a grey body's 0.025 %).
    pub soot_bands: Vec<[u64; 4]>,
    /// Rothermel's effective heating number ε = exp(−138/σ) (σ in 1/ft),
    /// per mille, by σ in 1/m: the share of a fine fuel that must reach
    /// ignition before it lights (its surface layer; thin blades nearly all).
    pub heating_permille: Vec<i64>,
    /// For the surface heat transfer of fine fuel (`chem::surface_h`): per
    /// σ of the substances, per whole kelvin of the gas, 0.62 × 0.888 ×
    /// √(d / (1000·ν(T))) in units of 2⁻²⁴, with d = 4/σ the twig's
    /// diameter and ν the air's kinematic viscosity. Times √(v in mm/s) it
    /// is the Churchill–Bernstein term 0.62·Re^½·Pr^⅓.
    pub surface_g: Vec<(i64, Vec<u32>)>,
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

/// Soot's spectral exponent α: κ_λ ∝ λ^−α, measured 0.83 ± 0.08 from 0.51
/// to 5.2 µm for overfire soot of many fuels (Köylü & Faeth 1996, J. Heat
/// Transfer 118:415; Siddall & McGrath 1963 found 0.93 for candle soot in
/// the infrared). Rayleigh absorption by particles far smaller than λ with a
/// constant refractive index would give α = 1 (Modest, *Radiative Heat
/// Transfer*, on soot); carbon's index drifts with λ, which lowers it.
pub const SOOT_ALPHA: f64 = 0.83;

/// Soot's mass-specific spectral absorption, κ_λ = C·c·λ^−α with c in mg/L
/// (= g/m³) and λ in µm, giving per metre. C is fixed so the Planck mean at
/// 1 400 K (`chem::FLAME_GLOW`, the temperature flame soot glows at) is the
/// grey 1 per metre per mg/L the engine used before:
///
///   κ_P/c = C·⟨λ^−α⟩ = C·(T/c₂)^α·Γ(4+α)ζ(4+α)/Γ(4)ζ(4) = 0.4325·C = 1,
///   so C = 2.312.
///
/// The grey value was itself a Planck mean: for κ_λ = C₀f_v/λ Modest
/// (*Radiative Heat Transfer*) gives κ_P = 3.72·C₀·f_v·T/c₂, and with
/// Dalzell & Sarofim's (1969) refractive index m = 1.57 − 0.56i
/// (C₀ = 36πnk/((n² − k² + 2)² + 4n²k²) = 4.9) and soot's density 1.8 g/cm³
/// (f_v = 5.6×10⁻⁷ per mg/L) that is 0.98 per metre per mg/L at 1 400 K.
/// The same C gives 3.8 m²/g at 550 nm, against 4.9 from Dalzell–Sarofim
/// (C₀/(ρλ)) and 7.5 ± 1.2 in Bond & Bergstrom's (2006) review of measured
/// black carbon: within soot's spread, on the low side.
pub const SOOT_C: f64 = 2.312;

/// Second radiation constant, µm·K.
const C2: f64 = 14_387.77;

/// ∫ₓ^∞ tᵖ/(eᵗ − 1) dt for x on a grid of step 1/1000 up to 80 (e^−80 is far
/// below anything that matters), by Simpson's rule: the Planck integrals in
/// x = c₂/(λT), the same for every temperature, so they're done once.
fn above_x(p: f64) -> Vec<f64> {
    let h = 0.001;
    let n = 80_000;
    let f = |x: f64| if x <= 0.0 { 0.0 } else { libm::pow(x, p) / libm::expm1(x) };
    let mut above = vec![0.0; n + 1];
    for i in (0..n).rev() {
        let (a, b) = (i as f64 * h, (i + 1) as f64 * h);
        above[i] = above[i + 1] + (b - a) / 6.0 * (f(a) + 4.0 * f((a + b) / 2.0) + f(b));
    }
    above
}

/// Linear interpolation in an [`above_x`] table.
fn at_x(above: &[f64], x: f64) -> f64 {
    let at = x / 0.001;
    let i = at.floor() as usize;
    if i + 1 >= above.len() { return 0.0 }
    let f = at - i as f64;
    above[i] + (above[i + 1] - above[i]) * f
}

/// Mean of λ^−α over [lo, hi] µm.
fn mean_power(lo: f64, hi: f64) -> f64 {
    (libm::pow(hi, 1.0 - SOOT_ALPHA) - libm::pow(lo, 1.0 - SOOT_ALPHA)) / ((1.0 - SOOT_ALPHA) * (hi - lo))
}

/// The visible bands' edges, µm: [red, green, blue].
const VISIBLE_UM: [(f64, f64); 3] = [(0.6, 0.7), (0.5, 0.6), (0.4, 0.5)];

/// Soot's κ in the infrared at source temperature t (per metre per mg/L):
/// C·⟨λ^−α⟩ over λ > 700 nm, weighted by B_λ(t). With λ = c₂/(xt),
/// λ^−α B_λ dλ ∝ (t/c₂)^α x^(3+α)/(eˣ − 1) dx.
fn kappa_ir(t: f64, grey: &[f64], soot: &[f64]) -> f64 {
    let t = t.max(1.0);
    let x700 = C2 / (0.7 * t);
    let num = soot[0] - at_x(soot, x700);
    let den = grey[0] - at_x(grey, x700);
    SOOT_C * libm::pow(t / C2, SOOT_ALPHA) * if den > 0.0 { num / den } else { 0.0 }
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
    let bands: Vec<[u64; 4]> = (0..=MAX_K).map(|k| {
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
    let (grey_above, soot_above) = (above_x(3.0), above_x(3.0 + SOOT_ALPHA));
    let q16 = |v: f64| (v * 65_536.0).round() as i64;
    let soot_kappa_vis = VISIBLE_UM.map(|(lo, hi)| q16(SOOT_C * mean_power(lo, hi)));
    let soot_kappa_ir: Vec<i64> = (0..=MAX_K).map(|k| q16(kappa_ir(k as f64, &grey_above, &soot_above))).collect();
    // Emission per band from the same (rounded) κ that absorbs it.
    let soot_emission: Vec<[u64; 4]> = (0..=MAX_K).map(|k| {
        let s: &[u64; 4] = &bands[k];
        let kappa = [soot_kappa_ir[k], soot_kappa_vis[0], soot_kappa_vis[1], soot_kappa_vis[2]];
        core::array::from_fn(|b| (s[b] as u128 * kappa[b] as u128 >> 16) as u64)
    }).collect();
    let soot_bands = soot_emission.iter().map(|e| {
        let total: u128 = e.iter().map(|&v| v as u128).sum();
        if total == 0 { return [Fraction::ONE.0, 0, 0, 0] }
        let part = |v: u64| (v as u128 * Fraction::ONE.0 as u128 / total) as u64;
        let (r, g, b) = (part(e[1]), part(e[2]), part(e[3]));
        [Fraction::ONE.0 - r - g - b, r, g, b]
    }).collect();
    Tables {
        soot_kappa_ir,
        soot_kappa_vis,
        soot_emission,
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
        surface_g: {
            let mut sigmas: Vec<i64> = crate::substance::table().iter().map(|s| s.surface_per_volume).filter(|&s| s > 0).collect();
            sigmas.sort();
            sigmas.dedup();
            sigmas.into_iter().map(|sigma| {
                let d = 4.0 / sigma as f64;
                (sigma, (0..=MAX_K).map(|k| {
                    let t = k.max(250) as f64;
                    let nu = 1.5e-5 * libm::pow(t / 293.0, 1.75);
                    (0.62 * 0.888 * libm::sqrt(d / (1000.0 * nu)) * (1u64 << 24) as f64).round() as u32
                }).collect())
            }).collect()
        },
    }
}

impl Tables {
    /// Water vapour that saturates air at `t` (µg/L), read between the
    /// whole kelvin of the table: saturation climbs ~6 % a kelvin, more
    /// than a humid night's dew-point depression, so it is interpolated.
    pub fn saturated_at(&self, t: crate::units::Temperature) -> i64 {
        let k = k_index(t.0);
        let lo = self.saturated_ug_per_l[k];
        let hi = self.saturated_ug_per_l[(k + 1).min(MAX_K)];
        // Not toward "unlimited" (from 100 °C up): between 373 K and the
        // boiling point that would hand a surface at 99.9 °C room for ~10¹⁷
        // µg of vapour a litre, a heat sink that pinned wet log faces there.
        if hi >= i64::MAX / 8 { return lo }
        let frac = (t.0 - k as i64 * 1_000).clamp(0, 1_000);
        lo + ((hi - lo) as i128 * frac as i128 / 1_000) as i64
    }
}

impl Tables {
    /// The `surface_g` row for a substance's σ.
    pub fn surface_g_for(&self, sigma: i64) -> &[u32] {
        &self.surface_g.iter().find(|(s, _)| *s == sigma).expect("σ of a substance in the table").1
    }
}

/// ⌊√v⌋, exactly.
pub fn isqrt(v: u64) -> u64 {
    if v == 0 { return 0 }
    let mut x = (v as f64).sqrt() as u64; // a starting guess only; corrected exactly below
    while x as u128 * x as u128 > v as u128 { x -= 1 }
    while (x as u128 + 1) * (x as u128 + 1) <= v as u128 { x += 1 }
    x
}

/// Table index for a temperature in millikelvin.
#[inline]
pub fn k_index(t_mk: i64) -> usize { (t_mk / 1_000).clamp(0, MAX_K as i64) as usize }
