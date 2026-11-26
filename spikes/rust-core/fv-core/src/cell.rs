//! A cell: a 10 cm cube of the world, and everything derived from it.
//!
//! A cell stores only amounts: masses, energy, decay energy left. Its
//! temperature, how much room its gas has, how well it conducts, how strong it
//! is: all of these are worked out from the amounts and its substance, so
//! nothing can drift out of step.

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
    /// Thermal energy above absolute zero, plus the latent heat held by the
    /// water vapour (so evaporating water cools the cell without changing
    /// this number).
    pub energy: Energy,
    /// Radioactive decay energy still to come.
    pub nuclear: Energy,
}

impl Cell {
    pub fn solid_mass(&self) -> Mass { self.solid[0] + self.solid[1] + self.solid[2] }
    pub fn gas_mass(&self) -> Mass { self.gas.iter().copied().sum() }

    /// Heat capacity of everything in the cell.
    pub fn heat_capacity(&self, subs: &[Substance]) -> HeatCapacity {
        let s = &subs[self.substance as usize];
        let mut c = (self.solid[VOLATILE] + self.solid[MINERAL]) * s.specific_heat;
        c += self.solid[CHAR] * CHAR_SPECIFIC_HEAT;
        c += self.water * WATER_SPECIFIC_HEAT;
        for k in 0..GASES { c += self.gas[k] * GAS_SPECIFIC_HEAT[k] }
        c
    }

    /// The latent heat held by the vapour.
    pub fn latent(&self) -> Energy { self.gas[Gas::Vapour as usize] * LATENT_HEAT_VAPOUR }

    pub fn temperature(&self, subs: &[Substance]) -> Temperature {
        (self.energy - self.latent()) / self.heat_capacity(subs)
    }

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

    /// Whether light passes through it the way it passes through air. Opaque
    /// cells stop radiation at their surface.
    pub fn opaque(&self, subs: &[Substance]) -> bool { !self.permeable(subs) }

    /// How intact the solid is, out of 1000: the bonds of burnt or charred
    /// matter carry less. Char holds about a fifth as well as sound wood.
    pub fn integrity_permille(&self, subs: &[Substance]) -> i64 {
        let s = &subs[self.substance as usize];
        if s.full_mass.0 == 0 { return 0 }
        let sound = self.solid[VOLATILE].0 + self.solid[MINERAL].0 + self.solid[CHAR].0 / 5;
        (sound * 1000 / s.full_mass.0).clamp(0, 1000)
    }

    /// A cell of open air at the given temperature, in the given volume.
    pub fn air(t: Temperature, subs: &[Substance]) -> Cell {
        let mut c = Cell::default();
        c.gas = FRESH_AIR;
        c.energy = c.heat_capacity(subs) * t + c.latent();
        c
    }

    /// A cell of a substance with its typical moisture, filled `share`
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
        c.energy = c.heat_capacity(subs) * t + c.latent();
        c
    }
}
