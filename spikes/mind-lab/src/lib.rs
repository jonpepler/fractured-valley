//! The mind lab's shared mind: one library that every later scenario (teaching, danger, the hunt,
//! the village) builds on. Headless, deterministic, integer maths only, no dependencies.
//!
//! - `graph`: link types, node tags, anchors, link keys, the founding-graph builder.
//! - `think`: bounded, modulated spreading activation (fixed point is the reference; ties by
//!   stable node key), and swap-in activation state.
//! - `store`: two forms of one mind behind one interface: the runtime form (a shared species
//!   base, gap-coded with u8 weights, plus a per-mind overlay and sparse fast weights) and the
//!   expanded, inspectable form. Lossless conversion both ways.
//! - `mind`: senses, prediction (and absence), modulators, fast weights, episodes, action choice,
//!   idle consolidation and sleep, learnt places, volatility, search and curiosity. Generic over
//!   the form and over a species.
//! - `fox`: the fox and its valley (ported from `spikes/thought-graph`) as the first species:
//!   world, founding, scenarios, sims of one fox or many, the home experiment, the viewer.
//! - `scale`: big minds (the fox's founding graph plus a generated body of knowledge) for
//!   measuring storage and cost at 10k and 100k nodes.

pub mod danger;
pub mod fox;
pub mod graph;
pub mod hashes;
pub mod mind;
pub mod people;
pub mod rng;
pub mod scale;
pub mod store;
pub mod think;
