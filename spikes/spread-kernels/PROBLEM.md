# Problem: bounded integer propagation on many weighted sparse graphs

A performance problem, stated on its own terms.

## Instance

- A directed graph `G` with `N` nodes (1k to 1M) and about `10·N` edges. Out-degree is power-law (2 to 400, mean about 10). Edges are mostly local within clusters of 16 to 256 consecutive ids, with about 12% long-range.
- Each node `v` has a small integer `val[v]` (−1000..1000) and an 8-bit tag set `tags[v]`.
- Each edge has a weight `w` (1..1000, skewed low, median about 260), a type `ty` (0..7), and a sparse edge value `ev` (−1000..1000; nonzero on about 6% of edges).
- **Many instances share one `G`.** `M` instances (1 to 10,000) each apply a small private delta to `G`: a few thousand changed edge weights, added edges, changed `val`s and a handful of extra nodes. Deltas are clustered in about 3% of the id space for each instance.

## Per step, per instance

A state is a sparse map `x: node → i32 ≥ 0`. Each step receives:

- a 5-tuple of global parameters `m` (each 0..1000, one of them signed);
- a short list of `(node, amount)` injections (about 2 to 20);
- occasionally a scale factor applied to `x` before the step.

The step runs a bounded propagation:

1. Decay `x` by a constant factor. Drop entries below a threshold `ε`.
2. Seed a frontier from a fraction of the surviving `x`, plus the injections. Keep the top `F` (48) entries by amount, ties broken by a stable key.
3. For up to `H` (4) rounds:
   - each frontier node `u` sends along each out-edge `e`, with
     `flow = src · w / max(Σw_out(u), 1000) · gain(m, ty, tags[to], val[to] + ev) / 1000 · decay / 1000`;
   - drop flows below `ε`;
   - sum incoming flow per target, add it into `x` (capped), and make the top `F` targets the next frontier.
4. If the total of `x` exceeds a budget, scale it down. Cap the support at 256 entries.
5. **Output:** the top 8 nodes by `x`, plus a 64-bit hash of the full state.

`gain` is a cheap integer function of `m`, `ty`, the target's tags and the summed value. It's a chain of per-mille multiplies and divides.

## Requirements

- **Bit-exact determinism.** The output must be identical on every machine, thread count and order of instances: native x86 and ARM, WASM in a browser, and a GPU if used. Integer arithmetic only. A different floating-point result would split a lockstep network.
- **The oracle:** `mind_compaction::spread::think` over `mind_compaction::gen::species` plus overlays, driven by `gen::workload` (crate `spikes/mind-compaction`). Treat the crate as an opaque instance generator and reference function.
- **Two tracks:**
  - **(A) Exact.** Same outputs as the oracle, bit for bit.
  - **(B) Redefined.** A different kernel with the same intent. You may propose different rounding or a different order of operations. Report its distance from the oracle: overlap of the top 8, relative L1 of `x`, and how often the argmax of a small downstream linear score over `x` differs. A redefinition is acceptable only if it's equally deterministic and clearly cheaper.

## What's known

At `N` = 1M on one core, the baseline is about 16 µs per step per instance, with about 40% spent in `gain`'s division chain.

- **Helped:** fixed-point reciprocals for the divisions (about 1.6×, track B). Gap-coded edges with u8 weights (2.2× smaller, same speed).
- **Didn't help:** reordering ids (BFS, RCM), structure-of-arrays versus array-of-structs, heaps for top-`F`.
- **Lost:** hierarchical clustering, top-k edge caps, delta inputs.

## What's open, and wanted

1. **Throughput for `M` instances per step,** not one instance's latency. Ideas include:
   - batching instances that share `G`, by edge or by node;
   - SIMD across instances;
   - sorting work by source node;
   - sparse-matrix × sparse-multi-vector formulations;
   - GPU compute (integer, deterministic; `wgpu`, and what WebGPU would need).

   Report steps per second per core, and memory traffic per step.
2. **Anything else** that changes the asymptotics or constants: different frontier selection with the same results, precomputing `gain` per (type, tag class) for a fixed `m`, caching, bit tricks.
3. **Memory per instance** for the private deltas.

There's no GPU on the development machine. Use a software adapter for correctness only, and leave a one-command benchmark for real hardware. Other processes share the CPU: take the minimum of several passes, and check that every pass hashes the same.
