//! Spike: one person whose appetite habit lags their activity.
//!
//! A body model (stores, metabolism by substance, Kleiber basal rate, muscle
//! power, injury), a layered brain (reflexes, utility choice, nightly
//! retrospection into a habit), integer maths and seeded hashing throughout.
//! See README.md.

#![forbid(unsafe_code)]

pub mod body;
pub mod brain;
pub mod curve;
pub mod rng;
pub mod sim;
pub mod units;
