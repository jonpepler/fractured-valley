//! The body: a model beside the world, not cells in it (decision 7.5).
//!
//! It holds a gut (food by substance), a short-term store (glycogen, as
//! energy) and a long-term store (adipose tissue, as whole milligrams), plus
//! muscles with a power limit that an injury caps. Everything that crosses the
//! boundary is booked: food in; heat, mechanical work and unabsorbed waste out.
//! [`Body::energy_gap`] is exactly zero after every tick.

use crate::units::*;

/// One body tick. Metabolism changes over minutes, so a minute is honest here;
/// the engine's 20 Hz tick would be 1,200 of these per body tick.
pub const TICK: Duration = seconds(60);
pub const TICKS_PER_HOUR: u64 = 60;
pub const TICKS_PER_DAY: u64 = 24 * TICKS_PER_HOUR;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Substance {
    pub name: &'static str,
    /// Chemical energy as eaten.
    pub energy: SpecificEnergy,
    /// How much leaves the stomach per tick.
    pub digest_per_tick: Mass,
    /// Share of the chemical energy absorbed (the rest leaves as waste), ‰.
    pub absorbed_pm: i64,
    /// Share of the absorbed energy spent digesting it, as heat, ‰.
    pub digest_heat_pm: i64,
}

pub const BREAD: Substance = Substance {
    name: "bread", energy: kj_per_kg(11_000), digest_per_tick: grams(2), absorbed_pm: 960, digest_heat_pm: 70,
};
pub const MEAT: Substance = Substance {
    name: "meat", energy: kj_per_kg(8_500), digest_per_tick: grams(1), absorbed_pm: 940, digest_heat_pm: 250,
};
pub const FOODS: [Substance; 2] = [BREAD, MEAT];

/// Adipose tissue (about 85% lipid): what a kilogram of body fat holds.
pub const FAT_TISSUE: SpecificEnergy = kj_per_kg(32_000);
/// Storing fat costs extra energy, lost as heat: ‰ on top of what is stored.
pub const FAT_STORE_COST_PM: i64 = 150;
/// Glycogen is stored with about 3 g of water per gram: energy per wet kg.
pub const GLYCOGEN_WET: SpecificEnergy = kj_per_kg(4_000);
/// Glycogen capacity per kg of lean mass (liver and muscle together).
pub const GLYCOGEN_PER_LEAN: SpecificEnergy = kj_per_kg(130);
/// Mechanical efficiency of muscle, ‰: the rest of the effort is heat.
pub const MUSCLE_EFFICIENCY_PM: i64 = 250;
/// Kleiber: resting power = 3.39 W × (mass in kg)^0.75 (70 kcal/day).
/// In milliwatts per mg^0.75 that is 3.39 W / 31,623 = 0.1072 mW; kept as
/// 1,072 / 10,000.
pub const KLEIBER_MW_E4: u128 = 1_072;
/// Fat release into the short-term store at most, when glycogen is empty.
pub const FAT_RELEASE_MAX: Power = watts(200);
/// Fat storage from the short-term store at most, when glycogen is full.
pub const FAT_STORE_MAX: Power = watts(150);
/// Below this glycogen fill fat is released; above `STORE_ABOVE` it is stored.
pub const RELEASE_BELOW: Q16 = Q16::milli(500);
pub const STORE_ABOVE: Q16 = Q16::milli(800);

/// What the body is asked to do this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Effort {
    /// Mechanical power asked of the muscles (capped by the body).
    pub mech: Power,
    /// Extra metabolic power that is not muscle work (posture, chewing), all heat.
    pub overhead: Power,
}

/// The boundary books, cumulative since birth.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Books {
    pub initial_stored: Energy,
    pub food_in: Energy,
    pub heat_out: Energy,
    pub work_out: Energy,
    pub waste_out: Energy,
}

/// Energy spent in one tick, by cause.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Spent {
    pub basal: Energy,
    /// Metabolic cost above basal: muscle effort (work + its heat) and overhead.
    pub activity: Energy,
    /// Mechanical work done on the world (part of `activity`).
    pub work: Energy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Body {
    pub lean: Mass,
    pub fat: Mass,
    pub glycogen: Energy,
    /// Food in the gut, by index into [`FOODS`].
    pub gut: [Mass; 2],
    pub gut_capacity: Mass,
    /// Sustainable mechanical power when healthy.
    pub muscle_power: Power,
    /// Share of muscle power an injury leaves, ‰ (1,000 = healthy).
    pub injury_cap_pm: i64,
    pub books: Books,
}

impl Body {
    pub fn adult() -> Body {
        let mut b = Body {
            lean: kilograms(60),
            fat: kilograms(12),
            glycogen: Energy::ZERO,
            gut: [Mass::ZERO; 2],
            gut_capacity: grams(800),
            muscle_power: watts(120),
            injury_cap_pm: 1_000,
            books: Books::default(),
        };
        b.glycogen = b.glycogen_capacity().per_mille(700);
        b.books.initial_stored = b.stored();
        b
    }

    pub fn glycogen_capacity(&self) -> Energy { self.lean * GLYCOGEN_PER_LEAN }
    pub fn gut_mass(&self) -> Mass { self.gut[0] + self.gut[1] }

    /// Body mass: lean + fat + wet glycogen + gut contents.
    pub fn mass(&self) -> Mass {
        self.lean + self.fat + self.glycogen.whole_mass_of(GLYCOGEN_WET) + self.gut_mass()
    }

    /// Chemical energy held inside the boundary.
    pub fn stored(&self) -> Energy {
        let mut e = self.glycogen + self.fat * FAT_TISSUE;
        for (i, f) in FOODS.iter().enumerate() { e += self.gut[i] * f.energy }
        e
    }

    /// Kleiber's law from the current mass, so a heavier body burns more.
    pub fn basal_power(&self) -> Power {
        let m = self.mass().0.max(0) as u128;
        Power((crate::units::pow_three_quarters(m) * KLEIBER_MW_E4 / 10_000) as i64)
    }

    /// The muscle power available now.
    pub fn muscle_available(&self) -> Power { Power(self.muscle_power.0 * self.injury_cap_pm / 1_000) }

    /// Readings the brain senses (decision 7.5: needs are readings of stores).
    pub fn glycogen_fill(&self) -> Q16 { self.glycogen.ratio(self.glycogen_capacity()) }
    pub fn gut_fill(&self) -> Q16 { self.gut_mass().ratio(self.gut_capacity) }
    pub fn capability(&self) -> Q16 { Q16::ratio(self.injury_cap_pm, 1_000) }

    /// Swallow up to `want` of a food; returns the energy taken in.
    pub fn eat(&mut self, food: usize, want: Mass) -> Energy {
        let room = Mass((self.gut_capacity - self.gut_mass()).0.max(0));
        let m = if want < room { want } else { room };
        self.gut[food] += m;
        let e = m * FOODS[food].energy;
        self.books.food_in += e;
        e
    }

    /// One tick of metabolism.
    pub fn step(&mut self, effort: Effort) -> Spent {
        // Digestion: gut → short-term store, with waste and the heat of digesting.
        for (i, f) in FOODS.iter().enumerate() {
            let m = if f.digest_per_tick < self.gut[i] { f.digest_per_tick } else { self.gut[i] };
            self.gut[i] -= m;
            let e = m * f.energy;
            let absorbed = e.per_mille(f.absorbed_pm);
            let heat = absorbed.per_mille(f.digest_heat_pm);
            self.books.waste_out += e - absorbed;
            self.books.heat_out += heat;
            self.glycogen += absorbed - heat;
        }

        // Spending: basal by Kleiber, muscle effort capped by injury.
        let mech = if effort.mech < self.muscle_available() { effort.mech } else { self.muscle_available() };
        let muscle = Power(mech.0 * 1_000 / MUSCLE_EFFICIENCY_PM);
        let basal = self.basal_power() * TICK;
        let activity = (muscle + effort.overhead) * TICK;
        let work = mech * TICK;
        let total = basal + activity;
        self.books.work_out += work;
        self.books.heat_out += total - work;
        self.glycogen -= total;
        if self.glycogen < Energy::ZERO {
            // Short of fuel: burn whole milligrams of fat, credit the change.
            let m = (Energy::ZERO - self.glycogen).mass_covering(FAT_TISSUE);
            let m = if m < self.fat { m } else { self.fat };
            self.fat -= m;
            self.glycogen += m * FAT_TISSUE;
        }

        // Exchange between the stores, steered by how full the short-term one is.
        let fill = self.glycogen_fill();
        if fill < RELEASE_BELOW {
            let share = Q16::ratio((RELEASE_BELOW - fill).0, RELEASE_BELOW.0);
            let m = (FAT_RELEASE_MAX.times(share) * TICK).whole_mass_of(FAT_TISSUE);
            let m = if m < self.fat { m } else { self.fat };
            self.fat -= m;
            self.glycogen += m * FAT_TISSUE;
        }
        let cap = self.glycogen_capacity();
        let over = Energy((self.glycogen - cap).0.max(0));
        let fill = self.glycogen_fill();
        let wanted = if fill > STORE_ABOVE {
            let share = Q16::ratio((fill - STORE_ABOVE).0, (Q16::ONE - STORE_ABOVE).0);
            FAT_STORE_MAX.times(share.clamp01()) * TICK
        } else { Energy::ZERO };
        let to_store = if over > wanted { over } else { wanted };
        let per_mg = Mass(1) * FAT_TISSUE;
        let cost_per_mg = per_mg + per_mg.per_mille(FAT_STORE_COST_PM);
        let m = Mass(to_store.0 / cost_per_mg.0);
        self.fat += m;
        self.glycogen -= Energy(m.0 * cost_per_mg.0);
        self.books.heat_out += Energy(m.0 * (cost_per_mg - per_mg).0);

        Spent { basal, activity, work }
    }

    /// Initial stores + food in − stores now − everything out. Always zero.
    pub fn energy_gap(&self) -> Energy {
        let b = &self.books;
        b.initial_stored + b.food_in - self.stored() - b.heat_out - b.work_out - b.waste_out
    }

    pub fn hash_into(&self, h: &mut crate::rng::Hasher) {
        for v in [self.lean.0, self.fat.0, self.glycogen.0, self.gut[0].0, self.gut[1].0, self.injury_cap_pm,
                  self.books.food_in.0, self.books.heat_out.0, self.books.work_out.0, self.books.waste_out.0] {
            h.i64(v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resting_adult_is_kleiber() {
        let b = Body::adult();
        let w = b.basal_power().w();
        assert!((80.0..=100.0).contains(&w), "basal {w} W");
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;
    use crate::curve::Curve;

    #[test]
    fn the_books_can_see_a_leak() {
        // A planted fault: a millijoule of heat booked that never left.
        let mut b = Body::adult();
        b.step(Effort::default());
        assert_eq!(b.energy_gap(), Energy::ZERO);
        b.books.heat_out += Energy(1);
        assert_eq!(b.energy_gap(), Energy(-1));
    }

    #[test]
    fn integer_roots_are_exact() {
        for n in [0u128, 1, 2, 3, 4, 15, 16, 17, 72_000_000, u64::MAX as u128] {
            let r = crate::units::isqrt(n);
            assert!(r * r <= n && (r + 1) * (r + 1) > n, "{n}");
        }
    }

    #[test]
    fn curves_interpolate_between_points() {
        let c = Curve::milli("t", [0, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000]);
        assert_eq!(c.eval(Q16::ZERO), Q16::ZERO);
        assert_eq!(c.eval(Q16(2048)).0, 65_536 / 2);
        assert_eq!(c.eval(Q16::ONE).0, 65_536);
        assert_eq!(c.eval(Q16(-5)), Q16::ZERO);
    }

    #[test]
    fn a_heavier_body_burns_more_at_rest() {
        let mut b = Body::adult();
        let light = b.basal_power();
        b.fat += kilograms(10);
        assert!(b.basal_power() > light);
    }
}
