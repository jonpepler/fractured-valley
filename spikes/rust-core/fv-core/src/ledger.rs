//! The books: every joule and every milligram, counted exactly.
//!
//! The identity the tests hold the simulation to:
//!
//! ```text
//! energy now = energy at the start
//!            + what crossed the box's edge (air carrying heat, light from outside, light escaping)
//!            + what commands put in (a match, a torch)
//!            + the work gravity did on falling bodies
//! ```
//!
//! and the same for mass. "Energy" here is every store the simulation has:
//! thermal (with the latent heat in vapour), chemical (what's still
//! burnable), nuclear (decay still to come), radiation in flight, and the
//! motion of bodies. There are no others: a rule that makes or loses energy
//! anywhere shows up as a gap.

use crate::substance::{GASES, GAS_CHEMICAL};
use crate::world::{Tally, World};

/// Totals of every store, at one moment.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub thermal: i128,
    pub chemical: i128,
    pub nuclear: i128,
    /// Mechanical energy of moving bodies plus anything held on them.
    pub bodies: i128,
    pub solid_mass: i128,
    pub water_mass: i128,
    pub gas_mass: [i128; GASES],
}

impl Totals {
    pub fn energy(&self) -> i128 { self.thermal + self.chemical + self.nuclear + self.bodies }
    pub fn mass(&self) -> i128 { self.solid_mass + self.water_mass + self.gas_mass.iter().sum::<i128>() }

    pub fn of(world: &World) -> Totals {
        let mut t = Totals::default();
        for c in &world.cells {
            t.thermal += c.thermal().0 as i128;
            t.chemical += c.chemical(&world.subs).0 as i128;
            t.nuclear += c.nuclear.0 as i128;
            t.solid_mass += c.solid_mass().0 as i128;
            t.water_mass += c.water.0 as i128;
            for k in 0..GASES { t.gas_mass[k] += c.gas[k].0 as i128 }
        }
        t
    }
}

/// The running account of everything that came in or went out.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Books {
    pub start: Totals,
    /// Energy carried in across the edge by air (negative: out).
    pub edge_energy: i128,
    /// Gas carried in across the edge, per kind.
    pub edge_gas: [i128; GASES],
    /// Radiation arriving from outside (sun, sky).
    pub radiation_in: i128,
    /// Radiation leaving through the edge.
    pub radiation_out: i128,
    /// Energy put in by commands.
    pub commands: i128,
    /// Mass put in by commands (placing things).
    pub commands_mass: i128,
    /// Work done by gravity on bodies (their fall turned into motion, then heat).
    pub gravity_work: i128,
}

impl Books {
    pub fn open(world: &World) -> Books { Books { start: Totals::of(world), ..Books::default() } }

    pub fn record(&mut self, t: &Tally) {
        self.edge_energy += t.energy_in as i128;
        for k in 0..GASES { self.edge_gas[k] += t.gas_in[k] as i128 }
        self.radiation_in += t.radiation_in as i128;
        self.radiation_out += t.radiation_out as i128;
    }

    /// Chemical energy carried across the edge by burnable gas and soot.
    pub fn edge_chemical(&self) -> i128 {
        (0..GASES).map(|k| self.edge_gas[k] * GAS_CHEMICAL[k].0 as i128).sum()
    }

    /// What the energy should be now.
    pub fn expected_energy(&self) -> i128 {
        self.start.energy() + self.edge_energy + self.edge_chemical() + self.radiation_in - self.radiation_out + self.commands + self.gravity_work
    }
    pub fn expected_mass(&self) -> i128 { self.start.mass() + self.edge_gas.iter().sum::<i128>() + self.commands_mass }

    /// The gap between what the stores hold and what the books say: zero, or
    /// something made or lost energy.
    pub fn energy_gap(&self, now: &Totals) -> i128 { now.energy() - self.expected_energy() }
    pub fn mass_gap(&self, now: &Totals) -> i128 { now.mass() - self.expected_mass() }
}
