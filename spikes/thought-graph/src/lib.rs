//! A fox's mind as a modulated thought graph in a small grounded world. Headless, deterministic,
//! integer maths only, no dependencies. A spike: a base for later tests, not the engine.
//!
//! - `world`: the grid, places, things, the dog and the fox's body.
//! - `graph`: the packed (CSR) graph, the inspectable form of a mind.
//! - `spread`: bounded, modulated spreading activation, generic over a mind's form.
//! - `fox`: the fox's founding graph, built from the world.
//! - `mind`: senses, prediction (and absence), modulators, fast weights, episodes, action choice,
//!   consolidation in idle time and sleep, learnt places, search and curiosity.
//! - `hd`: the compiled bit-vector form, its think, and decompiling it back to a graph.
//! - `packed`: a quantised, baked graph: a second compact form, for comparison.
//! - `scale`: structured random minds of 1k–100k nodes, for cost.
//! - `scenarios`: the scripted situations the tests and the viewer share.
//! - `sim`: runs a scenario and records frames; `viewer` writes them as a self-contained page.
//! - `experiments`: day-by-day measurements of the home experiment (read-only on the mind).

pub mod experiments;
pub mod fox;
pub mod graph;
pub mod hd;
pub mod mind;
pub mod packed;
pub mod rng;
pub mod scale;
pub mod scenarios;
pub mod sim;
pub mod spread;
pub mod viewer;
pub mod world;
