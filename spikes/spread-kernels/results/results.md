# Results

Machine: 4-core Xeon @ 2.3 GHz (AVX-512), 15 GB RAM, shared with other sessions. No hardware
performance counters (no `perf`); profiles come from callgrind. Every number is the **minimum
of 5 timing passes, and every pass produced the same hash** (the benchmark panics otherwise).
Raw rows: [`bench.csv`](bench.csv); generated tables: [`tables.md`](tables.md).

Workload: `gen::species(N, 7)`, one private `gen::overlay` per instance (`OverlaySpec::DEFAULT`,
except N = 1M with M = 10,000, which uses 1/10 of it to fit in RAM), `gen::workload` per
instance. Ticks per instance: `200000 / M`, clamped to 10..2000. All instances step in lockstep:
one barrier per tick across threads.

Variants (all produce the per-step top 8 and a 64-bit hash of the full state):

| variant | track | what it is |
|---|---|---|
| `oracle` | – | `mind_compaction::spread::think` over the materialised instance graph |
| `exact-v1` | A | sparse state, shared graph, per-thread hash table, gain memo |
| `exact-fast` | A | v1 minus bookkeeping: frontiers selected, never sorted; merges through the table |
| `exact-pruned` | A* | fast plus early exit per node under a per-class gain bound (property P) |
| `redef` | B | interpolated gain, folded rounding, saturating u32/i32, exact pruning |
| `redef-lazy` | B | as redef, knots computed on first use instead of up front |

A* = bit-exact against the oracle in every test, *given* property P of the oracle's gain (its
maximum over values -1000..1000 is at -1000, 0 or 1000 for a fixed modifier set and class),
which is checked over every value for 1,500 modifier sets, including corner ones, but not proved.

## Throughput, 1 thread (steps per second)

| N | M | oracle | exact-v1 | exact-fast | exact-pruned | redef |
|---|---|---|---|---|---|---|
| 10k | 1 | 96k | 107k | 152k | 305k | 406k |
| 10k | 100 | – | 88k | 119k | 230k | 300k |
| 10k | 10,000 | – | 88k | 101k | 151k | 244k |
| 100k | 1 | 72k | 80k | 108k | 219k | 292k |
| 100k | 100 | – | 73k | 99k | 206k | 253k |
| 100k | 10,000 | – | 65k | 94k | 190k | 192k |
| 1M | 1 | 89k | 110k | 151k | 292k | 390k |
| 1M | 100 | – | 78k | 99k | 168k | 220k |
| 1M | 1,000 | – | 66k | 91k | 147k | 181k |
| 1M | 10,000 (1/10 delta) | – | 57k | 72k | 145k | 191k |

(All M values are in `tables.md`.) The oracle runs only for M <= 10: it keeps two dense
N-sized arrays and a materialised graph per instance (about 100 MB per instance at N = 1M).

**Per-instance step cost is ~3.3 µs (A*) / ~2.5 µs (B) when one instance's data is hot, rising
to ~6.8 / ~5.3 µs at M >= 1000**, where each instance's delta and state are cold again by its
next turn. The oracle is ~10-14 µs here, in line with the 16 µs the problem quotes.

## Throughput, 4 threads (steps per second, whole machine)

| N | M | exact-v1 | exact-fast | exact-pruned | redef |
|---|---|---|---|---|---|
| 10k | 10 | 132k | 149k | 176k | 184k |
| 10k | 100 | 282k | 351k | 640k | 783k |
| 10k | 10,000 | 332k | 392k | 774k | 911k |
| 100k | 1,000 | 255k | 320k | 573k | 786k |
| 100k | 10,000 | 256k | 328k | 628k | 584k |
| 1M | 100 | 278k | 346k | 599k | 718k |
| 1M | 1,000 | 219k | 283k | 499k | 580k |
| 1M | 10,000 (1/10 delta) | 220k | 266k | 615k | 789k |

Scaling with M: below ~100 instances, threads do not help (one barrier per tick costs more than
a tick's work: at M = 10, four threads are *slower* than one). From M = 100 on, four threads give
2.7-4.2x. Per core that is ~150-200k exact steps/s and ~145-260k redefined steps/s. A few cells
are out of line (for example `redef-lazy`, 4 threads, N = 10k, M = 1000: 358k) — the machine is
shared; the minimum of five passes does not remove every disturbance.

## Where the time goes (callgrind, N = 10k, M = 1)

- **exact-v1**: a quarter of the step in `finish`, a fifth in sorting candidate lists (top-k
  by sort), the rest in the edge loop and its hash-table and gain-memo lookups.
- **exact-fast**: ~85k instructions per step, ~37% of them inside the oracle's `gain` (about
  220 distinct (class, value) keys per step, ~100 instructions each), most of the rest in the
  edge loop (one 64-bit division and two by-constant divisions per edge).
- **exact-pruned**: ~47k instructions per step; `gain` still ~25% (bounds plus surviving edges).
- **redef**: ~28k instructions per step; ~39 `gain` calls (13 classes x 3 knots); what is left
  is bookkeeping spread thinly (table, selection, finish, hash).

Per step (from counters): ~37 state entries, ~65 nodes expanded over ~3 rounds, ~780 edges
looked at by the unpruned kernel, of which only ~40 (5%) carry a flow above `min_act`. Pruning
cuts the edges looked at to ~150-200 (A*) and ~50 (B).

## Memory traffic per step (estimate)

No counters on this machine, so this is an estimate from the kernel's own counts: 8 B per
edge looked at, 28 B per expanded node (offset, denominator, class mask, rank word), 16 B per
state entry (read and write). **About 3.7-4.2 KB per step for A\*, ~3 KB for B**, of which the
instance's own data is ~0.6 KB. At 600k steps/s that is ~2.5 GB/s — far from bandwidth limits;
the cost at large M is latency (cold lines), not bandwidth.

## Memory per instance

| N | raw overlay (`Overlay::bytes`) | compiled delta (what the kernels read) | state |
|---|---|---|---|
| 10k | 19 KB | 43 KB | ~0.3 KB (cap 2 KB) |
| 100k | 201 KB | 434 KB | ~0.3 KB |
| 1M | 2,007 KB | 4,347 KB | ~0.3 KB |
| 1M, 1/10 overlay | 220 KB | 831 KB | ~0.3 KB |

Shared graph: 83 MB read by the kernels at N = 1M (8 B per edge plus 12 B per node), 189 MB
with the tables only the delta compiler needs. The compiled delta is ~2.2x the raw overlay
because a revalued node turns into a weight-and-key override on each edge into it (so the edge
loop never looks values up), plus a 1.5 bit/node rank directory (the fixed cost that dominates
the 1/10 overlay). The default overlay changes 2% of edges spread uniformly, so ~15% of all
nodes are "patched" sources; that, not the state, is what limits M at N = 1M here.

## Track B distance from the oracle

Each instance runs its own trajectory in both kernels; 100 instances x 200 ticks
([`distance.txt`](distance.txt)):

| N | top-8 overlap | identical top-8 lists | relative L1 of x | argmax of linear score differs |
|---|---|---|---|---|
| 10k | 98.5% | 80.3% | 0.54% | 0.19% |
| 100k | 98.1% | 78.2% | 0.58% | 0.27% |
| 1M | 98.7% | 80.8% | 0.58% | 0.32% |

The downstream score is 4 fixed hash-derived weight vectors (-100..100 per node); "argmax
differs" is how often the winning vector differs. Cost: 1.2-1.5x cheaper than A* per step,
and it needs no assumption about `gain` for its pruning.

## GPU (wgpu, WGSL, integer only; track B)

Software adapter only: llvmpipe through wgpu's GL backend ([`gpu_llvmpipe.txt`](gpu_llvmpipe.txt)).
**Every tick's top-8 and every final state equal the CPU redefined kernel**, at N = 10k
(M = 1000, 10,000), 100k (M = 1000) and 1M (M = 200), with no table overflows. llvmpipe
speed (171-187k steps/s at N = 10k, 83k at 100k, 12k at 1M) says nothing about real GPUs.
lavapipe (Mesa's Vulkan software driver) segfaults inside its shader JIT on this shader (a
null descriptor read); the GL path runs the same WGSL. The host computes the gain knots
(0.43-0.64 µs per instance-step on the CPU) because `gain` is a black box.

## WASM

`wasm/check.sh` builds the hash check natively and for `wasm32-wasip1`, runs the second under
node 22, and compares: **all four kernels hash identically** at N = 10k and N = 100k (8
instances x 100 ticks). Under node: exact-pruned 6.5 µs/step, redef 5.5 µs/step (native 3.8 / 3.0).

## What did not help

- **Exact reciprocals for `a * w / den`** (Lemire-Kaser-Kurz, one 128-bit multiply): no gain,
  slightly worse; this CPU's hardware divider is fast, and the division was never the bottleneck.
- **Binary search over an instance's patched sources**: at M = 100 it cost ~20% of the step in
  cache misses; replaced by a rank bitmap (popcount), 17.5 -> 14.1 µs at the time.
- **Lazy knots (`redef-lazy`)**: slightly slower than computing ~13 classes' knots up front.
- **Batching instances by source node** (not built): measured frontier sharing across 300-1000
  instances at one tick is 2.3-4.5x at N = 10k, where everything is in cache anyway, 1.6-1.9x
  at 100k and 1.5x at 1M ([`overlap.txt`](overlap.txt)). At most ~1.6x fewer edge-list reads
  where reads cost something, against having to keep many instances' tables live at once.
- **SIMD across instances**: not attempted. Frontiers diverge after the first round (above),
  each instance's gain differs (its own modifiers), and the per-edge work after pruning is ~3-4
  edges per expanded node: there is little to put in lanes.
- **Threads at small M**: a barrier per tick costs more than the work at M <= 10.

## What could not be measured here

- Real GPU throughput (no GPU). Run `cd gpu && cargo run --release -- 100000 1000,10000 50`.
- Hardware cache misses and memory bandwidth (no counters); traffic above is an estimate.
- ARM native (only x86 here); WASM in a browser (node only).
- N = 1M with M = 10,000 at the default overlay (needs ~45 GB for the deltas).
- The oracle at M > 10 (dense per-instance state).
