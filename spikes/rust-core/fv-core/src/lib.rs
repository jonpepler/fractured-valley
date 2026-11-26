//! Fractured Valley simulation core (spike).
//!
//! Energy-first: every transfer moves energy or mass from one place to
//! another, and [`ledger::Books`] checks that nothing is made or lost.

pub mod cell;
pub mod checkpoint;
pub mod chem;
pub mod flow;
pub mod ledger;
pub mod rad;
pub mod rng;
pub mod scene;
pub mod sim;
pub mod substance;
pub mod tables;
pub mod transport;
pub mod units;
pub mod view;
pub mod world;
