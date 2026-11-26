//! Substances: what a solid is made of, as data with units.
//!
//! Every substance carries the same set of properties (decision 4.1). Nothing
//! here says "flammable" or "burns at": whether and how something burns comes
//! from its composition (how much of it is volatile or char), the reaction
//! rates in `chem`, and the energy it receives. Values are textbook figures
//! for the real material unless marked otherwise.

use crate::units::*;

/// Gases and suspended matter in a cell's pore space.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Gas {
    Oxygen = 0,
    Nitrogen = 1,
    CarbonDioxide = 2,
    /// Water vapour.
    Vapour = 3,
    /// Burnable gas given off by heated organic matter (wood gas).
    Fuel = 4,
    /// Fine carbon particles: dark smoke.
    Soot = 5,
    /// Tiny liquid water drops: white smoke and mist.
    Droplets = 6,
}
pub const GASES: usize = 7;
pub const ALL_GASES: [Gas; GASES] =
    [Gas::Oxygen, Gas::Nitrogen, Gas::CarbonDioxide, Gas::Vapour, Gas::Fuel, Gas::Soot, Gas::Droplets];

/// Specific heat of each gas-space component.
pub const GAS_SPECIFIC_HEAT: [SpecificHeat; GASES] = [
    j_per_kg_k(918),  // O₂
    j_per_kg_k(1040), // N₂
    j_per_kg_k(844),  // CO₂
    j_per_kg_k(2010), // water vapour
    j_per_kg_k(1500), // wood gas (mixed CO, H₂, light organics; approximate)
    j_per_kg_k(710),  // soot (graphite-like carbon)
    j_per_kg_k(4186), // liquid water drops
];

/// Chemical energy each gas-space component releases when fully burnt.
pub const GAS_CHEMICAL: [SpecificEnergy; GASES] = [
    kj_per_kg(0), kj_per_kg(0), kj_per_kg(0), kj_per_kg(0),
    kj_per_kg(16_000), // wood gas, per kg of gas
    kj_per_kg(32_800), // soot: carbon
    kj_per_kg(0),
];

/// Liquid water, bound in a solid or as drops.
pub const WATER_SPECIFIC_HEAT: SpecificHeat = j_per_kg_k(4186);
/// Energy to turn liquid water into vapour.
pub const LATENT_HEAT_VAPOUR: SpecificEnergy = kj_per_kg(2257);
/// Char (the carbon left after organic matter gives off its gas).
pub const CHAR_SPECIFIC_HEAT: SpecificHeat = j_per_kg_k(1000);
pub const CHAR_CHEMICAL: SpecificEnergy = kj_per_kg(32_800);

/// A cell is a 10 cm cube.
pub const CELL_VOLUME_ML: i64 = 1_000;
/// Fresh air at sea level, by mass in one litre at 20 °C (1.204 g/L).
pub const FRESH_AIR: [Mass; GASES] = [
    Mass(279_300), // O₂, 23.2 %
    Mass(909_000), // N₂, 75.5 % (with argon folded in)
    Mass(600),     // CO₂
    Mass(8_600),   // vapour: ~50 % humidity at 20 °C
    Mass(0), Mass(0), Mass(0),
];

/// How a substance treats each band of radiation.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Optics {
    /// Share of arriving light reflected, per band [infrared, red, green, blue].
    /// What isn't reflected is absorbed (solids are opaque).
    pub albedo: [Fraction; 4],
    /// Emissivity in the infrared (how well its surface radiates heat).
    pub emissivity: Fraction,
    /// Mass attenuation for ionising radiation, in cm²/g (≈0.06 for ~1 MeV
    /// gamma in most matter; higher for heavy elements).
    pub ionising_cm2_per_g: i64,
}

/// How strongly a substance holds together.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Strength {
    /// Pulling apart, in kilopascals.
    pub tensile_kpa: i64,
    /// Crushing, in kilopascals.
    pub compressive_kpa: i64,
    /// Stiffness (Young's modulus), in megapascals.
    pub stiffness_mpa: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Substance {
    pub name: &'static str,
    /// Mass of solid in a full cell (bulk density × 1 L): a log is dense, a
    /// tuft of grass is mostly air.
    pub full_mass: Mass,
    /// Share of the cell's volume that's open pore space (gas lives here).
    pub porosity: Fraction,
    /// Whether air can blow through it (grass, leaves) or only seep (wood, stone).
    pub permeable: bool,
    /// Surface area per volume of the solid's pieces, in 1/m (Rothermel's σ):
    /// grass blades ~3500, leaves ~4000, a log seen at 10 cm ~60, stone ~60.
    pub surface_per_volume: i64,
    /// Composition by mass, in parts per thousand: [volatile, char-forming, mineral].
    pub composition: [i64; 3],
    /// Water it typically holds, as a share of dry mass in per-mille.
    pub typical_moisture: i64,
    pub specific_heat: SpecificHeat,
    /// Thermal conductivity, in mW/(m·K).
    pub conductivity: i64,
    /// Chemical energy of its volatile part, released when that part (once
    /// turned to gas) burns. Lower than the gas's own by the heat it takes
    /// to give the gas off, so pyrolysis costs heat.
    pub volatile_chemical: SpecificEnergy,
    pub optics: Optics,
    pub strength: Strength,
    /// Decay energy still to come, per mass, and the decay rate per tick in
    /// parts of 2⁶⁴ (zero for stable matter). A demo substance carries it.
    pub nuclear: SpecificEnergy,
    pub decay_per_tick: u64,
    /// Share of decay energy carried off as penetrating (ionising) radiation;
    /// the rest is absorbed where it's made.
    pub penetrating: Fraction,
    /// Display colour of the clean, dry, unburnt surface (sRGB): view only,
    /// and kept consistent with `albedo`.
    pub colour: [u8; 3],
}

const fn f(v: u64) -> Fraction { Fraction(v * (1 << 32) / 1000) }

pub const NONE: u8 = 0;
pub const STONE: u8 = 1;
pub const SOIL: u8 = 2;
pub const GRASS: u8 = 3;
pub const LEAVES: u8 = 4;
pub const WOOD: u8 = 5;
pub const BRICK: u8 = 6;
pub const MORTAR: u8 = 7;
pub const IRON: u8 = 8;
pub const ORE: u8 = 9;

pub fn table() -> Vec<Substance> {
    let organic = Strength { tensile_kpa: 0, compressive_kpa: 0, stiffness_mpa: 0 };
    vec![
        Substance {
            name: "nothing", full_mass: Mass(0), porosity: Fraction::ONE, permeable: true, surface_per_volume: 0,
            composition: [0, 0, 0], typical_moisture: 0, specific_heat: j_per_kg_k(0), conductivity: 0,
            volatile_chemical: kj_per_kg(0),
            optics: Optics { albedo: [Fraction::ZERO; 4], emissivity: Fraction::ZERO, ionising_cm2_per_g: 0 },
            strength: organic, nuclear: kj_per_kg(0), decay_per_tick: 0, penetrating: Fraction::ZERO, colour: [0, 0, 0],
        },
        Substance {
            name: "stone", full_mass: grams(2_650), porosity: f(10), permeable: false, surface_per_volume: 60,
            composition: [0, 0, 1000], typical_moisture: 2, specific_heat: j_per_kg_k(790), conductivity: 2_800,
            volatile_chemical: kj_per_kg(0),
            optics: Optics { albedo: [f(300), f(420), f(390), f(330)], emissivity: f(900), ionising_cm2_per_g: 60 },
            strength: Strength { tensile_kpa: 7_000, compressive_kpa: 130_000, stiffness_mpa: 50_000 },
            nuclear: kj_per_kg(0), decay_per_tick: 0, penetrating: Fraction::ZERO, colour: [168, 155, 124],
        },
        Substance {
            name: "soil", full_mass: grams(1_400), porosity: f(450), permeable: false, surface_per_volume: 60,
            composition: [40, 20, 940], typical_moisture: 150, specific_heat: j_per_kg_k(800), conductivity: 600,
            volatile_chemical: kj_per_kg(15_600),
            optics: Optics { albedo: [f(250), f(200), f(160), f(120)], emissivity: f(930), ionising_cm2_per_g: 60 },
            strength: Strength { tensile_kpa: 15, compressive_kpa: 200, stiffness_mpa: 20 },
            nuclear: kj_per_kg(0), decay_per_tick: 0, penetrating: Fraction::ZERO, colour: [102, 81, 63],
        },
        Substance {
            // Grass blades: ~0.5 kg/m² of fuel over 10 cm of height.
            name: "grass", full_mass: grams(5), porosity: f(996), permeable: true, surface_per_volume: 3_500,
            composition: [760, 200, 40], typical_moisture: 1_200, specific_heat: j_per_kg_k(1_700), conductivity: 150,
            volatile_chemical: kj_per_kg(15_600),
            optics: Optics { albedo: [f(500), f(180), f(330), f(90)], emissivity: f(950), ionising_cm2_per_g: 60 },
            strength: organic, nuclear: kj_per_kg(0), decay_per_tick: 0, penetrating: Fraction::ZERO, colour: [110, 150, 60],
        },
        Substance {
            // Leaves in a crown: ~3 kg/m³ of foliage.
            name: "leaves", full_mass: grams(3), porosity: f(997), permeable: true, surface_per_volume: 4_000,
            composition: [760, 200, 40], typical_moisture: 1_400, specific_heat: j_per_kg_k(1_700), conductivity: 150,
            volatile_chemical: kj_per_kg(15_600),
            optics: Optics { albedo: [f(450), f(120), f(300), f(70)], emissivity: f(950), ionising_cm2_per_g: 60 },
            strength: Strength { tensile_kpa: 2, compressive_kpa: 2, stiffness_mpa: 1 },
            nuclear: kj_per_kg(0), decay_per_tick: 0, penetrating: Fraction::ZERO, colour: [72, 110, 52],
        },
        Substance {
            name: "wood", full_mass: grams(500), porosity: f(600), permeable: false, surface_per_volume: 60,
            composition: [800, 180, 20], typical_moisture: 900, specific_heat: j_per_kg_k(1_700), conductivity: 150,
            volatile_chemical: kj_per_kg(15_600),
            optics: Optics { albedo: [f(400), f(450), f(300), f(190)], emissivity: f(900), ionising_cm2_per_g: 60 },
            strength: Strength { tensile_kpa: 40_000, compressive_kpa: 30_000, stiffness_mpa: 10_000 },
            nuclear: kj_per_kg(0), decay_per_tick: 0, penetrating: Fraction::ZERO, colour: [110, 76, 46],
        },
        Substance {
            name: "brick", full_mass: grams(1_900), porosity: f(250), permeable: false, surface_per_volume: 60,
            composition: [0, 0, 1000], typical_moisture: 10, specific_heat: j_per_kg_k(840), conductivity: 700,
            volatile_chemical: kj_per_kg(0),
            optics: Optics { albedo: [f(350), f(520), f(300), f(230)], emissivity: f(930), ionising_cm2_per_g: 60 },
            strength: Strength { tensile_kpa: 3_000, compressive_kpa: 20_000, stiffness_mpa: 15_000 },
            nuclear: kj_per_kg(0), decay_per_tick: 0, penetrating: Fraction::ZERO, colour: [160, 96, 72],
        },
        Substance {
            // Lime mortar: weak in tension, so masonry breaks along its joints.
            name: "mortar", full_mass: grams(1_700), porosity: f(300), permeable: false, surface_per_volume: 60,
            composition: [0, 0, 1000], typical_moisture: 20, specific_heat: j_per_kg_k(900), conductivity: 800,
            volatile_chemical: kj_per_kg(0),
            optics: Optics { albedo: [f(400), f(560), f(540), f(500)], emissivity: f(920), ionising_cm2_per_g: 60 },
            strength: Strength { tensile_kpa: 400, compressive_kpa: 5_000, stiffness_mpa: 5_000 },
            nuclear: kj_per_kg(0), decay_per_tick: 0, penetrating: Fraction::ZERO, colour: [196, 190, 176],
        },
        Substance {
            name: "iron", full_mass: grams(7_870), porosity: Fraction::ZERO, permeable: false, surface_per_volume: 60,
            composition: [0, 0, 1000], typical_moisture: 0, specific_heat: j_per_kg_k(450), conductivity: 50_000,
            volatile_chemical: kj_per_kg(0),
            optics: Optics { albedo: [f(700), f(560), f(560), f(560)], emissivity: f(300), ionising_cm2_per_g: 60 },
            strength: Strength { tensile_kpa: 350_000, compressive_kpa: 350_000, stiffness_mpa: 200_000 },
            nuclear: kj_per_kg(0), decay_per_tick: 0, penetrating: Fraction::ZERO, colour: [140, 146, 154],
        },
        Substance {
            // Demo substance: a rock rich in a heat-giving radioisotope, at
            // ~40 W per kilogram (like a weak radioisotope heater; real ore
            // gives ~10⁻⁴ W/kg). Half-life ~88 years, as plutonium-238.
            // Decay energy left: 40 W/kg × 88 y / ln 2 ≈ 1.6×10¹¹ J/kg.
            name: "radioactive ore", full_mass: grams(2_900), porosity: f(10), permeable: false, surface_per_volume: 60,
            composition: [0, 0, 1000], typical_moisture: 0, specific_heat: j_per_kg_k(700), conductivity: 2_500,
            volatile_chemical: kj_per_kg(0),
            optics: Optics { albedo: [f(250), f(300), f(330), f(250)], emissivity: f(900), ionising_cm2_per_g: 70 },
            strength: Strength { tensile_kpa: 7_000, compressive_kpa: 130_000, stiffness_mpa: 50_000 },
            nuclear: kj_per_kg(160_000_000),
            // λ·dt = ln2 / (88 y) × 0.05 s = 1.25×10⁻¹¹ per tick → × 2⁶⁴.
            decay_per_tick: 230_584_300,
            penetrating: f(200), colour: [96, 110, 90],
        },
    ]
}
