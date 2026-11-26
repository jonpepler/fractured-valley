//! Fractured Valley simulation core (spike).
//!
//! Energy-first: every transfer moves energy or mass from one place to
//! another, and [`ledger::Books`] checks that nothing is made or lost.

pub mod cell;
pub mod ledger;
pub mod rng;
pub mod substance;
pub mod tables;
pub mod transport;
pub mod units;
pub mod world;
