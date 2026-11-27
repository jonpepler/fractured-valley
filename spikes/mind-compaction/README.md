# Spike: a benchmark lab for cheaper, smaller minds

A standing baseline for the thought-graph mind (`spikes/thought-graph`). It generates structured minds of 1k–1M nodes and replays a fixed, seeded workload of thinks through 42 variants of the mind. Each variant is measured against the reference: bytes, nanoseconds per think, and agreement. Every future idea for a cheaper or smaller mind should be added here and measured the same way.

Standalone crate, no dependencies, integer maths only. The reference graph, spreading think and hash mixer are **copied** from `spikes/thought-graph` (commit `fe89380`), not depended on.

```bash
cargo run --release --bin baseline                     # all sizes, ~70 s; writes results/baseline.md and .csv
cargo run --release --bin baseline -- --sizes 1k,10k   # a quick look (also writes results/)
cargo run --release --bin baseline -- --only compact   # some variants (results/ not written)
cargo test --release                                   # determinism, exactness and agreement floors, ~50 s
```

The current numbers are in [`results/baseline.md`](results/baseline.md) and [`results/baseline.csv`](results/baseline.csv).

## The harness (`gen.rs`)

- **The species base graph:**
  - eight core nodes: hunger, fatigue, pain, food, danger, rest, anger, comfort;
  - communities of 16–256 nodes on a 2D map, each about 8% places, 12% things at a place, and 80% concepts;
  - power-law out-degree (2–400, mean ≈ 10) and preferential in-links;
  - skewed weights: many weak links, few strong;
  - places link `near` their 6 nearest places by real distance; things link `at` their place, and the place `holds` them;
  - 6% of links carry a link valence.
- **An individual** is the base plus an **overlay**: 2% of links reweighted, 1% added, 1% of nodes revalued, 0.5% new personal nodes. 70% of the changes fall in a home range of 3% of communities.
  - The reference runs the individual materialised as its own graph.
  - The COW variants read it through the shared base.
- **The workload:** 1500 ticks.
  - A creature walks between places and rests at them.
  - Senses inject its place and the things there, plus glimpses while walking.
  - Needs inject hunger and fatigue, felt in steps of 50.
  - The modulators drift.
  - About 1 tick in 200 brings a surprise: a danger and an interrupt that damps the carried thought.
- **The decision rule** (`decide.rs`) is the fox's walk choice: *on my mind × how it feels*, with a one-hop look-ahead.

## Metrics (`bench.rs`)

- **Bytes per mind and per node.** Shared parts (a species base, a codebook, per-thread scratch) are counted apart.
- **State:** bytes kept per mind between thinks.
- **Build time.**
- **ns per think, mean and p99.** Each tick's time is the minimum over 5 measured passes, so other work on the machine doesn't skew it. All passes must hash the same.
- **Cache misses.** They come from `perf_event_open` where the machine exposes a PMU. This VM doesn't, so they show `n/a`.
- **Links read per think.**
- **Agreement with the reference, tick by tick:**
  - the same top thought;
  - top-8 workspace overlap;
  - the same action;
  - **decision regret:** the variant's choice scored by the reference, as a share of the reference's best;
  - activation error: Σ|Δ| / Σ reference.
  - **Tie-aware** figures sit beside the strict ones. Integer activations tie often, and the reference breaks ties by node id.
- **Determinism:** a hash over each tick's workspace, decision and activation. *Exact* means the hash equals the reference's.

## Variants

| Family | Variants |
|---|---|
| baseline | the reference graph; the packed graph (baseline 2); 2048-bit bit vectors (baseline 3, ≤ 10k only) |
| state | swap-in state: dense arrays only while thinking, an active list between thinks |
| sharing | shared base + link-level overlay (COW), with sparse or dense per-mind node valence; page-level COW (bytes only) |
| precision | weights at u16, u8, 4-bit linear, 4-bit log, 3-bit log and 2-bit log, all keeping link valence |
| pruning | links below 50, 100 or 200 dropped; top 16, 8 or 4 links per node |
| hierarchy | cluster-level spread first, then node-level spreading only inside the top 4, 16 or 64 clusters |
| event | skip unchanged thinks (tolerance 0, 20 or 60); delta input (steady input not re-spread) |
| think | frontier by `select_nth` or a fixed-size heap; a gain memo; a two-stage gain table; fixed-point flow and gain; a fused think (no edge list, packed node records) |
| layout | array-of-structs links; shuffled ids (learning order); shuffled then BFS or RCM order |
| compact | gap-coded varint targets with 4-bit-log or u8 weights; the "stack" (compact + swap + fixed-point think) |
| bit vectors elsewhere (`recog.rs`) | recognition (identify, categorise, food?) and episode recall, against the graph doing the same job |

## Tests (`tests/lab.rs`)

- **The harness is pinned:** graph, workload, overlay and reference run hash, at 1k.
- **Every variant** is deterministic at 1k and 10k: two separately built labs give the same hash, and `run` checks that its 5 passes agree.
- **Exact variants** must match the reference's hash.
- **Renumbered layouts** must have identical activations and agree 100% tie-aware.
- **Faithful variants** must reach 95% top-1 and workspace (tie-aware), at most 60‰ regret and at most 30‰ activation error.
- **COW** reads exactly the materialised mind.
- **Compact** keeps every target, type and valence sign.

## Adding an idea

1. Add a `Kind` and a `Spec` in `bench.rs`, built from the lab's mind.
2. Implement `Form`, which is enough for a new layout, or `Variant` for a new think.
3. Pick its intent: `Exact`, `UpToTies`, `Floor` or `Explore`.
4. Run the baseline and the tests, then commit `results/` with the change.
