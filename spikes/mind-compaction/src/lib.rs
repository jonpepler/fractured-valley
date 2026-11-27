//! A benchmark lab for cheaper, smaller minds. It generates structured minds (1k–1M nodes),
//! replays a fixed seeded workload of thinks through many variants of the thought graph's mind,
//! and measures each against the reference: bytes, nanoseconds per think, and agreement.
//! Integer only, deterministic, no dependencies.
//!
//! - `graph`, `spread`, `rng`: the reference, copied from `spikes/thought-graph`.
//! - `gen`: the harness's minds (species base, individual overlays) and workload.
//! - `decide`: the fixed decision rule every variant is scored on.
//! - `forms`: compact forms (packed, precision, gap-coded, AoS, pruned, reordered, COW, bit vectors).
//! - `think2`: rewritten thinks (frontier selection, gain memo, fixed point, delta input).
//! - `fused`: the think specialised to one layout (no edge list, packed node records).
//! - `variants`: every variant behind one interface (incl. swap-in state, skip, hierarchical).
//! - `recog`: bit vectors against the graph on recognition and episode recall.
//! - `bench`: the runner; `report`: the table, `results/baseline.md` and `.csv`.

pub mod bench;
pub mod decide;
pub mod forms;
pub mod fused;
pub mod gen;
pub mod graph;
pub mod perf;
pub mod recog;
pub mod report;
pub mod rng;
pub mod spread;
pub mod think2;
pub mod variants;
