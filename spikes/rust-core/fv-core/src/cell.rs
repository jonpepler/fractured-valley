//! A cell: a 10 cm cube of the world, and everything derived from it.
//!
//! A cell stores only amounts: masses, two energies, decay energy left. Its
//! temperatures, how much room its gas has, how well it conducts, how strong
//! it is: all of these are worked out from the amounts and its substance, so
//! nothing can drift out of step.
//!
//! A cell has **two temperatures**: its solid's and its gas's. A tuft of
//! grass in a flame doesn't share the flame's temperature at once: its blades
//! heat over a second or two through their surface. Dense matter (wood,
//! stone) has so much surface inside its pores that the two stay together.

use crate::substance::*;
use crate::units::*;

/// The solid's parts.
pub const VOLATILE: usize = 0;
pub const CHAR: usize = 1;
pub const MINERAL: usize = 2;

/// Which of this cell's + faces are bonded to the neighbour there.
pub const BOND_X: u8 = 1;
pub const BOND_Y: u8 = 2;
pub const BOND_Z: u8 = 4;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Cell {
    /// What the solid here is made of (`substance::NONE` for open air).
    pub substance: u8,
    /// Bonds to the neighbours on the +x, +y and +z sides.
    pub bonds: u8,
    /// The solid, by part: [volatile, char, mineral].
    pub solid: [Mass; 3],
    /// Liquid water held in the solid.
    pub water: Mass,
    /// The gas space: see [`Gas`].
    pub gas: [Mass; GASES],
    /// The gas's thermal energy above absolute zero, plus the latent heat
    /// held by its vapour (so condensing vapour warms the gas without
    /// changing this number).
    pub energy: Energy,
    /// The solid's (and its water's) thermal energy above absolute zero.
    pub solid_energy: Energy,
    /// Radioactive decay energy still to come.
    pub nuclear: Energy,
}

impl Cell {
    pub fn solid_mass(&self) -> Mass { self.solid[0] + self.solid[1] + self.solid[2] }
    pub fn gas_mass(&self) -> Mass { self.gas.iter().copied().sum() }
    pub fn has_solid(&self) -> bool { self.solid_mass().0 > 0 || self.water.0 > 0 }

    pub fn gas_capacity(&self) -> HeatCapacity {
        let mut c = HeatCapacity::ZERO;
        for k in 0..GASES { c += self.gas[k] * GAS_SPECIFIC_HEAT[k] }
        c
    }

    pub fn solid_capacity(&self, subs: &[Substance]) -> HeatCapacity {
        let s = &subs[self.substance as usize];
        (self.solid[VOLATILE] + self.solid[MINERAL]) * s.specific_heat + self.solid[CHAR] * CHAR_SPECIFIC_HEAT + self.water * WATER_SPECIFIC_HEAT
    }

    /// Heat capacity of everything in the cell.
    pub fn heat_capacity(&self, subs: &[Substance]) -> HeatCapacity { self.gas_capacity() + self.solid_capacity(subs) }

    /// The latent heat held by the vapour.
    pub fn latent(&self) -> Energy { self.gas[Gas::Vapour as usize] * LATENT_HEAT_VAPOUR }

    pub fn gas_temperature(&self) -> Temperature {
        let c = self.gas_capacity();
        if c.0 <= 0 { return Temperature(0) }
        (self.energy - self.latent()) / c
    }

    pub fn solid_temperature(&self, subs: &[Substance]) -> Temperature {
        let c = self.solid_capacity(subs);
        if c.0 <= 0 { return self.gas_temperature() }
        self.solid_energy / c
    }

    /// The temperature that matters to the outside: an opaque cell's
    /// surface is its solid; an airy cell is mostly its gas.
    pub fn temperature(&self, subs: &[Substance]) -> Temperature {
        if self.opaque(subs) || self.gas_capacity().0 == 0 { self.solid_temperature(subs) } else { self.gas_temperature() }
    }

    /// All thermal energy (gas and solid).
    pub fn thermal(&self) -> Energy { self.energy + self.solid_energy }

    /// Chemical energy stored here: what burning everything burnable would release.
    pub fn chemical(&self, subs: &[Substance]) -> Energy {
        let s = &subs[self.substance as usize];
        let mut e = self.solid[VOLATILE] * s.volatile_chemical + self.solid[CHAR] * CHAR_CHEMICAL;
        for k in 0..GASES { e += self.gas[k] * GAS_CHEMICAL[k] }
        e
    }

    /// How full of solid the cell is, out of 2³²: 0 for air, near 1 for stone.
    pub fn solid_share(&self, subs: &[Substance]) -> Fraction {
        let s = &subs[self.substance as usize];
        if s.full_mass.0 == 0 { return Fraction::ZERO }
        // The solid fills (1 − porosity) of the cell when complete; burning away shrinks it.
        let filled = (Fraction::ONE.0 - s.porosity.0) as i128;
        let share = filled * self.solid_mass().0.max(0) as i128 / s.full_mass.0 as i128;
        Fraction(share.clamp(0, Fraction::ONE.0 as i128) as u64)
    }

    /// Room for gas, in millilitres.
    pub fn gas_volume_ml(&self, subs: &[Substance]) -> i64 {
        CELL_VOLUME_ML - self.solid_share(subs).of(CELL_VOLUME_ML)
    }

    /// Whether air can blow through this cell (open air, grass, leaves).
    pub fn permeable(&self, subs: &[Substance]) -> bool {
        let s = &subs[self.substance as usize];
        s.permeable || self.solid_share(subs).0 < Fraction::ONE.0 / 20
    }

    /// Whether light stops at this cell's surface (it doesn't pass through
    /// the way it passes through air).
    pub fn opaque(&self, subs: &[Substance]) -> bool { !self.permeable(subs) }

    /// Surface between solid and gas inside the cell, in mm² (σ × solid
    /// volume): grass ~14 000 mm² per tuft.
    pub fn inner_surface_mm2(&self, subs: &[Substance]) -> i64 {
        let s = &subs[self.substance as usize];
        // σ [1/m] × share × 0.001 m³ = σ·share·0.001 m² = σ·share·1000 mm²
        s.surface_per_volume * self.solid_share(subs).of(1_000)
    }

    /// How intact the solid is, out of 1000: what's left of the part that
    /// carries its strength. In wood that's the fibre (the volatile part);
    /// charcoal holds ~2 % as well in tension and ash nothing. In rock or
    /// brick it's the mineral itself.
    pub fn integrity_permille(&self, subs: &[Substance]) -> i64 {
        let s = &subs[self.substance as usize];
        if s.full_mass.0 == 0 { return 0 }
        let mineral_matter = s.composition[MINERAL] >= 500;
        let weigh = |m: [i64; 3]| -> i64 {
            if mineral_matter { return m[MINERAL] }
            m[VOLATILE] + m[CHAR] / 50
        };
        let full = weigh([s.full_mass.0 * s.composition[0] / 1000, s.full_mass.0 * s.composition[1] / 1000, s.full_mass.0 * s.composition[2] / 1000]);
        if full <= 0 { return 0 }
        (weigh([self.solid[0].0, self.solid[1].0, self.solid[2].0]) * 1000 / full).clamp(0, 1000)
    }

    /// Set both energies from one temperature.
    pub fn set_temperature(&mut self, t: Temperature, subs: &[Substance]) {
        self.energy = self.gas_capacity() * t + self.latent();
        self.solid_energy = self.solid_capacity(subs) * t;
    }

    /// A cell of open air at the given temperature.
    pub fn air(t: Temperature, subs: &[Substance]) -> Cell {
        let mut c = Cell { gas: FRESH_AIR, ..Cell::default() };
        c.set_temperature(t, subs);
        c
    }

    /// A cell of a substance with the given moisture, filled `share`
    /// (out of 1000) and the rest of its pores holding fresh air, at `t`.
    pub fn solid_of(substance: u8, share_permille: i64, moisture_permille: i64, t: Temperature, subs: &[Substance]) -> Cell {
        let s = &subs[substance as usize];
        let mut c = Cell { substance, ..Cell::default() };
        let mass = s.full_mass.0 * share_permille / 1000;
        c.solid = [Mass(mass * s.composition[0] / 1000), Mass(mass * s.composition[1] / 1000), Mass(mass * s.composition[2] / 1000)];
        c.water = Mass(mass * moisture_permille / 1000);
        let vol = c.gas_volume_ml(subs);
        for k in 0..GASES { c.gas[k] = Mass(FRESH_AIR[k].0 * vol / CELL_VOLUME_ML) }
        // Dense matter's decay energy is part of the substance.
        c.nuclear = Mass(mass) * s.nuclear;
        c.set_temperature(t, subs);
        c
    }
}
