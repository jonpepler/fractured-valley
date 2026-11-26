//! Spike: continuous, interacting matter properties, owned by the substance.
//!
//! The sandbox's `Material` has `solid`, `liquid` and `loose` as separate
//! booleans, and the relationships between them live elsewhere. Here a cell
//! holds only *state* (mass, stored energy, water, integrity), a substance
//! holds only *constants with units*, and every behaviour-relevant property
//! (temperature, how much of it is melted or boiled, how readily it flows,
//! how strongly it holds together, whether it slumps) is **derived** by the
//! substance from that state, continuously. Nothing is a flag.
//!
//! Written without `else` (early returns), as the project's lint asks.

use fv_units::*;
use typenum::{N1, N2, P1, Z0};

/// Pressure (Pa = kg·m⁻¹·s⁻²): a cell's weight per area, or a bond's strength.
pub type Pressure = Quantity<Dim<N1, P1, N2, Z0>>;
/// Gravitational acceleration as a unit-checked quantity (m·s⁻²).
pub type Acceleration = Quantity<Dim<P1, Z0, N2, Z0>>;
pub const GRAVITY: Acceleration = Acceleration::from_si(9.81);
pub const CELL: Length = metres(0.1);

/// Constants only. Every number has a unit; none says what a cell *is*.
pub struct Substance {
    pub name: &'static str,
    pub density: Density,
    pub heat_capacity: [HeatCapacity; 3], // solid, liquid, gas
    pub melts_at: Temperature,
    pub boils_at: Temperature,
    pub heat_of_melting: SpecificEnergy,
    pub heat_of_boiling: SpecificEnergy,
    /// Bond strength of the intact solid (tensile, Pa). Zero for a powder.
    pub bond_strength: Pressure,
    /// Extra bond from water between grains, at its best (Pa): sand's "damp" strength.
    pub water_bridge_strength: Pressure,
    /// Water (fraction of dry mass) where water bridges are strongest; beyond twice this the grains float apart.
    pub best_water: Moisture,
}

/// State only: what a cell holds. Its meaning comes from the substance.
#[derive(Clone, Copy)]
pub struct Cell {
    pub mass: Mass,
    /// Energy stored, measured from absolute zero as solid.
    pub energy: Energy,
    /// Water carried in or on it, as a fraction of its own mass.
    pub water: Moisture,
    /// How intact its structure is: 1 = sound, 0 = shattered or burnt through.
    pub integrity: Dimensionless,
}

/// Everything derived. A rule reads these; it never asks "is it a liquid?".
#[derive(Debug, Clone, Copy)]
pub struct Derived {
    pub temperature: Temperature,
    /// Fractions of the mass melted and boiled (each 0..1, continuous).
    pub melted: Dimensionless,
    pub boiled: Dimensionless,
    /// Propensity to flow and mix: 0 for a rigid solid, rising through liquid to gas.
    pub mobility: Dimensionless,
    /// How strongly it holds to its neighbours (Pa).
    pub cohesion: Pressure,
    /// Weight it must carry ÷ what its bonds can hold, for a column of `height`:
    /// above 1 it slumps or flows ("loose" as a number, not a flag).
    pub slump: Dimensionless,
}

const ZERO_K: Temperature = Temperature::kelvin(0.0);
const ONE: Dimensionless = Dimensionless::from_si(1.0);
const ZERO: Dimensionless = Dimensionless::ZERO;

impl Substance {
    /// Energy per kilogram at each landmark of the heating curve.
    fn landmarks(&self) -> [SpecificEnergy; 4] {
        let [cs, cl, _] = self.heat_capacity;
        let start_melt = cs * (self.melts_at - ZERO_K);
        let end_melt = start_melt + self.heat_of_melting;
        let start_boil = end_melt + cl * (self.boils_at - self.melts_at);
        [start_melt, end_melt, start_boil, start_boil + self.heat_of_boiling]
    }

    /// Temperature and phase from stored energy. While melting or boiling,
    /// added energy goes into the change, not the temperature: a plateau.
    fn temperature_and_phase(&self, c: &Cell) -> (Temperature, Dimensionless, Dimensionless) {
        let e: SpecificEnergy = c.energy / c.mass;
        let [cs, cl, cg] = self.heat_capacity;
        let [m0, m1, b0, b1] = self.landmarks();
        if e < m0 {
            return (ZERO_K + e / cs, ZERO, ZERO);
        }
        if e < m1 {
            return (self.melts_at, (e - m0) / self.heat_of_melting, ZERO);
        }
        if e < b0 {
            return (self.melts_at + (e - m1) / cl, ONE, ZERO);
        }
        if e < b1 {
            return (self.boils_at, ONE, (e - b0) / self.heat_of_boiling);
        }
        (self.boils_at + (e - b1) / cg, ONE, ONE)
    }

    /// Water's contribution to bonds: rises to a best amount, then falls away
    /// (damp sand holds a shape; dry and soaked sand don't).
    fn water_bridges(&self, water: Moisture) -> Pressure {
        let best = self.best_water.to_si();
        if best <= 0.0 {
            return Pressure::ZERO;
        }
        let w = water.to_si() / best; // 1 at best
        let shape = if w < 1.0 { w } else { (2.0 - w).max(0.0) }; // a tent: 0 → 1 → 0
        Pressure::from_si(self.water_bridge_strength.to_si() * shape)
    }

    /// Everything a rule needs, derived from this cell's state.
    pub fn derive(&self, c: &Cell, column: Length) -> Derived {
        let (temperature, melted, boiled) = self.temperature_and_phase(c);
        let solid = ONE - melted;
        // Mobility: solid 0, liquid ½, gas 1, blended by how much is in each phase.
        let mobility = (melted - boiled) * Dimensionless::from_si(0.5) + boiled;
        // Softening: solids lose strength as they near melting (from half the melting point up).
        let homologous = (temperature - ZERO_K).to_si() / (self.melts_at - ZERO_K).to_si();
        let softening = ratio((1.0 - (homologous - 0.5).max(0.0) * 2.0).clamp(0.0, 1.0));
        let bonds = Pressure::from_si(self.bond_strength.to_si() * c.integrity.to_si() * softening.to_si());
        let cohesion = Pressure::from_si((bonds + self.water_bridges(c.water)).to_si() * solid.to_si());
        // A column of this stuff presses on its base with density × g × height.
        let weight: Pressure = Pressure::from_si((self.density * GRAVITY * column).to_si());
        let slump = ratio(weight.to_si() / cohesion.to_si().max(1e-6));
        Derived { temperature, melted, boiled, mobility, cohesion, slump }
    }

    /// The cell holding `mass` of this substance at a temperature (as a solid below melting).
    pub fn cell_at(&self, mass: Mass, t: Temperature, water: Moisture) -> Cell {
        let [cs, _, _] = self.heat_capacity;
        Cell { mass, energy: mass * cs * (t - ZERO_K), water, integrity: ONE }
    }
}

/// How well two cells' matter can meet (0..1), from their mobility: the review's
/// point that *contact*, not temperature, is why frozen acid sits on metal.
/// A liquid wets a surface; solids touch at a few points.
pub fn contact(a: &Derived, b: &Derived) -> Dimensionless {
    let m = a.mobility.max(b.mobility).to_si();
    ratio((m * 2.0).min(1.0)) // fully mobile partner (liquid or gas) → full contact
}

pub const WATER: Substance = Substance {
    name: "water",
    density: Density::from_si(1000.0),
    heat_capacity: [HeatCapacity::from_si(2100.0), HeatCapacity::from_si(4186.0), HeatCapacity::from_si(2000.0)],
    melts_at: celsius(0.0),
    boils_at: celsius(100.0),
    heat_of_melting: SpecificEnergy::from_si(334_000.0),
    heat_of_boiling: SpecificEnergy::from_si(2_257_000.0),
    bond_strength: Pressure::from_si(1_000_000.0), // ice, roughly 1 MPa in tension
    water_bridge_strength: Pressure::from_si(0.0),
    best_water: moisture(0.0),
};

pub const SAND: Substance = Substance {
    name: "sand",
    density: Density::from_si(1600.0),
    heat_capacity: [HeatCapacity::from_si(830.0), HeatCapacity::from_si(1000.0), HeatCapacity::from_si(1000.0)],
    melts_at: celsius(1700.0),
    boils_at: celsius(2230.0),
    heat_of_melting: SpecificEnergy::from_si(150_000.0),
    heat_of_boiling: SpecificEnergy::from_si(10_000_000.0),
    bond_strength: Pressure::from_si(0.0), // grains: no bonds of their own
    water_bridge_strength: Pressure::from_si(4_000.0), // damp sand: a few kPa
    best_water: moisture(0.08),
};

pub const IRON: Substance = Substance {
    name: "iron",
    density: Density::from_si(7870.0),
    heat_capacity: [HeatCapacity::from_si(450.0), HeatCapacity::from_si(820.0), HeatCapacity::from_si(450.0)],
    melts_at: celsius(1538.0),
    boils_at: celsius(2862.0),
    heat_of_melting: SpecificEnergy::from_si(247_000.0),
    heat_of_boiling: SpecificEnergy::from_si(6_090_000.0),
    bond_strength: Pressure::from_si(200_000_000.0),
    water_bridge_strength: Pressure::from_si(0.0),
    best_water: moisture(0.0),
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Ice heated steadily: temperature climbs, then *holds at 0 °C* while the
    /// melt fraction goes 0 → 1, then climbs again. One energy number per cell.
    #[test]
    fn melting_is_a_plateau_not_a_switch() {
        let mut c = WATER.cell_at(kilograms(1.0), celsius(-20.0), moisture(0.0));
        let step = joules(20_000.0);
        let mut seen = Vec::new();
        for _ in 0..30 {
            let d = WATER.derive(&c, CELL);
            seen.push((d.temperature.to_celsius(), d.melted.to_si(), d.mobility.to_si()));
            c.energy = c.energy + step;
        }
        for (t, m, mob) in &seen {
            println!("{t:7.2} °C  melted {m:4.2}  mobility {mob:4.2}");
        }
        // Somewhere it sits at 0 °C partly melted, with mobility between solid and liquid.
        assert!(seen.iter().any(|(t, m, mob)| t.abs() < 0.01 && *m > 0.2 && *m < 0.8 && *mob > 0.05 && *mob < 0.45));
        // It ends liquid and above 0 °C.
        let (t, m, _) = *seen.last().unwrap();
        assert!(t > 0.5 && (m - 1.0).abs() < 1e-6);
    }

    /// Sand's hold on its neighbours comes from water between the grains:
    /// a 10 cm column of damp sand stands; dry or soaked sand slumps.
    #[test]
    fn damp_sand_stands_dry_and_soaked_sand_slump() {
        let column = metres(0.1);
        let at = |w: f64| SAND.derive(&SAND.cell_at(kilograms(1.6), celsius(15.0), moisture(w)), column).slump.to_si();
        let (dry, damp, soaked) = (at(0.0), at(0.08), at(0.2));
        println!("slump: dry {dry:.2}, damp {damp:.3}, soaked {soaked:.2}");
        assert!(damp < 1.0, "damp sand should stand");
        assert!(dry > 1.0 && soaked > 1.0, "dry and soaked sand should slump");
    }

    /// The same derivation makes iron soften as it nears melting, and
    /// ice give way as it melts: no special case for either.
    #[test]
    fn solids_soften_near_melting_and_lose_bonds_as_they_melt() {
        let column = metres(1.0);
        let iron = |c: f64| IRON.derive(&IRON.cell_at(kilograms(7.87), celsius(c), moisture(0.0)), column).cohesion.to_si();
        assert!(iron(20.0) > iron(1200.0) * 2.0);
        let mut ice = WATER.cell_at(kilograms(1.0), celsius(-1.0), moisture(0.0));
        let solid = WATER.derive(&ice, column).cohesion;
        ice.energy = ice.energy + joules(200_000.0); // about 60% melted
        let half = WATER.derive(&ice, column);
        assert!(half.cohesion < solid && half.melted.to_si() > 0.4);
    }

    /// Frozen acid on metal versus liquid acid: contact comes from mobility,
    /// so the frozen block barely touches and the liquid wets the surface.
    /// (Same −1 °C both times would need supercooling; here the point is
    /// that contact, not temperature, is the gate, as the review argued.)
    #[test]
    fn frozen_acid_barely_touches_liquid_acid_wets() {
        let metal = IRON.derive(&IRON.cell_at(kilograms(7.87), celsius(15.0), moisture(0.0)), CELL);
        let frozen = WATER.derive(&WATER.cell_at(kilograms(1.0), celsius(-5.0), moisture(0.0)), CELL);
        let mut melt = WATER.cell_at(kilograms(1.0), celsius(-0.1), moisture(0.0));
        melt.energy = melt.energy + joules(400_000.0);
        let liquid = WATER.derive(&melt, CELL);
        assert_eq!(contact(&frozen, &metal), ratio(0.0));
        assert_eq!(contact(&liquid, &metal), ratio(1.0));
    }
}
