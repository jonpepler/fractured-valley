# spread-kernels

Kernels for the problem in [`PROBLEM.md`](PROBLEM.md): bounded integer propagation for many
instances that share one graph. Numbers and method: [`results/results.md`](results/results.md).

## Findings

1. **The oracle's step was rebuilt from the outside.** `think` and `gain` were treated as black
   boxes; what was read of `mind-compaction` is signatures and types, `Params::default`, the
   `Form` impl for `Graph`, and the builder/materialiser code (to reproduce link dedup). (A
   callgrind source annotation also printed a few lines of `gain`'s body; nothing here relies
   on them — the gain properties used are measured and tested.) The step's semantics come from
   probing public functions on hand-built graphs and from differential tests (`src/model.rs`). The rebuilt
   model matches `spread::think` bit for bit on the generator's instances, on 20,000 adversarial
   ones, on instances built to overflow i32, and on crafted states (negative x, nodes with x but
   not active, zero entries still active). Details that matter: frontier amounts are the
   round's summed flow, not x; seeds keep every positive carry; flows are computed in i64 then
   truncated to i32 *before* the `min_act` test; sums wrap in i32; gain can be negative at
   extreme modifiers; the budget is applied before the support cap, over the whole list.
2. **Only ~5% of edge visits carry a flow.** Per step ~65 nodes are expanded and ~780 edges
   looked at, but only ~40 flows pass `min_act`. The oracle's `gain` depends only on (type
   class, three tag bits, value clamped to ±1000) — 32 x 2001 keys — and for fixed modifiers its
   maximum over the value is at -1000, 0 or 1000 (property P, checked over every value).
   Storing each node's edges heaviest first and bounding gain per class lets a node stop at the
   first edge that cannot reach `min_act`: **edges looked at drop ~4x, and the step runs
   ~3x faster than the oracle (3.3 µs vs 10 µs, one core)**, still bit-exact.
3. **State per instance is tiny; the private delta is not.** The state is ~37 entries (~0.3 KB,
   2 KB cap) instead of two dense N-sized arrays. The default overlay (2% of edges reweighted,
   uniformly) patches ~15% of all source nodes, so the compiled delta is about 2.2x the raw overlay:
   43 KB / 434 KB / 4.3 MB per instance at N = 10k / 100k / 1M. That, not compute,
   limits M at N = 1M.
4. **Throughput with M is cache-bound, not compute- or bandwidth-bound.** One core: ~300k
   exact steps/s when an instance is hot, ~150k at M >= 1000. Four cores: 500-770k exact and
   580-910k redefined steps/s for M >= 100. Estimated traffic is ~4 KB per step.
5. **Batching by source node is not worth it for this workload**: instances' frontiers overlap
   only 1.5-1.9x at N >= 100k (measured), and SIMD across instances has nothing to vectorise
   after pruning (~3 edges per expanded node, divergent frontiers, per-instance gain).
6. **Track B** (`src/redef.rs`): gain interpolated between three oracle calls per class,
   gain and hop decay folded into one rounding, saturating 32-bit arithmetic, pruning that is
   exact by construction. 1.2-1.5x cheaper than exact-pruned, top-8 overlap ~98.5%, relative L1
   ~0.6%, downstream argmax differs on ~0.2-0.3% of steps.
7. **GPU**: track B as a WGSL integer compute shader (one invocation per instance). Bit-identical
   to the CPU kernel on llvmpipe (wgpu GL backend). No real GPU here; lavapipe (Vulkan) crashes
   in its JIT on this shader. The exact track does not fit a GPU well: `gain` is a black box
   (the host would have to ship a 64k-entry table per instance per step) and the exact
   arithmetic needs 64-bit integers, which WGSL does not have.
8. **WASM**: all kernels hash identically natively and as wasm32-wasip1 under node (~1.7x slower).

## Tracks and exactness

| kernel | file | exact? |
|---|---|---|
| `model::Dense` | `src/model.rs` | reference rebuild; equals the oracle, including raw dense arrays |
| `exact::step` (v1) | `src/exact.rs` | bit-exact (assumes only the gain key reduction, tested) |
| `fast::step` | `src/fast.rs` | bit-exact (same) |
| `fast::step_pruned` | `src/fast.rs` | bit-exact given property P (tested, not proved) |
| `redef::step` | `src/redef.rs` | track B; deterministic, tested across threads/order/WASM/GPU |

Known corner not verified against the oracle: a node listed twice in the active list. It needs a
wrapped (overflowed) increment that cancels a node's x to exactly 0, followed by a positive one
in the same step; the kernels follow the model there.

## Layout

- `src/shared.rs` — the shared graph: 8-byte edges (target, weight, gain key), heaviest first.
- `src/delta.rs` — an instance's overlay compiled into per-source overrides, extra edges and a
  rank bitmap; `View` gives it back as a `Form` for the equality test with `gen::materialise`.
- `src/harness.rs` — generator adapter, oracle driver, adversarial and overflowing instances.
- `src/bench.rs`, `src/bin/bench.rs` — lockstep multi-instance drivers and the CSV benchmark.
- `src/metrics.rs`, `src/bin/distance.rs` — track B distance.
- `gpu/` — the wgpu crate (separate, so the main crate stays dependency-free).
- `wasm/` — node runner and native-vs-wasm hash comparison.

## Commands

```sh
cargo test --release                       # all equality / determinism tests
cargo test --release -- --ignored          # exhaustive gain-key check (slow)
cargo run --release --bin bench -- 10000,100000,1000000 1,10,100,1000,10000   # -> results/bench.csv
python3 results/tables.py > results/tables.md
cargo run --release --bin distance -- 100000 100 200
wasm/check.sh 100000 8 100                 # needs the wasm32-wasip1 target and node
cd gpu && cargo run --release -- 100000 1000,10000 50     # GPU check + benchmark (one command)
cd gpu && WGPU_BACKEND=gl cargo test --release            # software adapter here
```

Benchmarks append to `results/bench.csv`; every row is the minimum of several passes that all
produced the same hash.
