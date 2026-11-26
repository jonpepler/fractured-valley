//! Spike: brains that talk. Utterances are in-world acts carried by sound; each brain keeps
//! short-term, long-term and emotional memory and a relationship per person it has met, and a
//! retrospection pass consolidates one into the others. Integer maths and hash randomness only.

pub mod brain;
pub mod memory;
pub mod model;
pub mod rng;
pub mod sound;
pub mod world;

pub use brain::{Brain, Personality};
pub use memory::{Belief, Cause, Episode, Rel, Source, Trace, TraceKind};
pub use model::{Ent, Fact, Pred, Utt};
pub use world::{Body, Event, Place, Spoken, World};

/// Multiply two per-mille numbers (1000 = 1.0).
#[inline]
pub fn pm(a: i32, b: i32) -> i32 { ((a as i64 * b as i64) / 1000) as i32 }

/// Clamp a signed per-mille value to -1000..=1000.
#[inline]
pub fn clampv(x: i32) -> i32 { x.clamp(-1000, 1000) }
