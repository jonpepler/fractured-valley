//! Round 2's toolkit (decision 9.10): tools a mind makes and uses instead of thinking something
//! through. A tool is **data** (a description in a small, typed, integer language) run by one
//! deterministic, fuel-bounded interpreter. The foundational tools, the schedule alternatives
//! and the tool-maker are all written in it; the tool-maker builds new descriptions from the
//! questions a mind keeps asking and what it has seen; the belt is the brain-tick interface.
//!
//! - `lang`: the description language (types, text and binary forms, hashing, plain words).
//! - `run`: the interpreter (state, fuel).
//! - `kit`: the foundational set, the schedule alternatives and the makers, as text.
//! - `cost`: static worst-case cost from a description, and which brain may reach for what.
//! - `maker`: the tool-maker's search (the `make` primitive).
//! - `belt`: a mind's tools and the calls a brain tick makes (`tell`, `ask`, `bind`, `tick`, `sleep`).
//! - `data`: observation streams (a generated routine valley, and a real village run).
//! - `study`: scoring tools against the truth.

pub mod belt;
pub mod cost;
pub mod data;
pub mod kit;
pub mod lang;
pub mod maker;
pub mod run;
pub mod study;
