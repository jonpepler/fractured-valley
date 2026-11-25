# cell-step-bench: TypeScript vs Rust→WASM for the voxel cell step

**Recommendation: write the cellular sim core in Rust→WASM (raw `extern "C"`, no wasm-bindgen,
JS-owned linear memory), and use hand-written `simd128` for the stencil passes.** Rendering,
networking and orchestration stay in TypeScript, and they read the brick arrays directly through
typed-array views over WASM memory, with no per-tick copy. Plan for worker threads from the start,
but expect only about 2–2.5× from 4 threads.

Why:

- On the same rule and data, **WASM is 2.1–2.6× faster than TS scalar-for-scalar, and 3.6–5× faster with SIMD**.
  The step is ~12–13 ns/cell in TS and ~3.4 ns/cell in WASM+SIMD (single thread, 256–1024 bricks).
- **JS cannot use SIMD.** Heat diffusion is the classic stencil, and hand-written `simd128` makes it
  **4× faster again (2.2 → 0.55 ns/cell)**. TS has no way to get that.
- **Determinism held for every engine.** TS, WASM, WASM+SIMD and 2–4 worker threads produced
  identical FNV-1a state hashes after 200 ticks at every size. Integer-only WASM (including integer SIMD) is deterministic by spec.
- On a 4 ms per-tick sim budget, single-threaded TS handles about **80 bricks** (0.33 M cells).
  WASM+SIMD handles about **290 bricks** on one thread, or **~700 bricks** across 4 workers.

## Machine & method

- CPU: `Intel(R) Xeon(R) Processor @ 2.30GHz` (from /proc/cpuinfo; AVX2/AVX-512 host), `nproc` = 4 (a shared cloud VM, 15 GiB RAM)
- Node v22.23.5 (V8 12.4), rustc 1.97.0 stable, target `wasm32-unknown-unknown`, `opt-level=3, lto, codegen-units=1, panic=abort`
- Each engine runs a **fresh world** for 200 ticks. Stats cover ticks 20–199, with the first 20 reported separately
  in `results/latest.md` (tier-up / worker spin-up). A global 60-tick warm-up runs before anything is timed.
  The state hash covers all 200 ticks.
- The bench ran **3 full times**. The table shows the **median of the 3 runs** for mean and p95 ms/tick.
  Run-to-run spread was about ±30% (shared vCPUs, and another agent's dev server was running).
- World: N bricks laid out as (√(N/4))² columns × 4 bricks high (64 cells = 6.4 m). Terrain: stone, 4 layers of sand, grass top,
  wood "trees", floating sand clumps (exercise gravity) and hot seeds on grass. Every brick is active.
  The fire really spreads: at 1024 bricks, ~23k grass/wood cells become ash within 200 ticks.
- `ns/cell` = mean ms/tick ÷ (N × 4096). Each cell update includes all three phases.

### The step rule (same in `src/sim.ts` and `rust/src/lib.rs`)

Per brick, in list order, per tick:

1. **Diffusion** (double-buffered `temp`): `v = (2·t + Σ6 neighbours + 4) >> 3`. Air above 20 cools by 1.
   This step reads **across brick boundaries** through a 6-neighbour brick table. A missing neighbour counts as an insulated face.
2. **Fire** (grass=2, wood=3), in place on the new temp buffer. At `temp ≥ 80`, a cell ignites with p=3/4 from `hash(2·tick, cell)`.
   While burning, the cell gains +24/+32 heat, the cell above gets +16, and one random horizontal neighbour gets +48.
   It becomes ash (4) after 12 ticks (grass) or 40 ticks (wood).
3. **Gravity** (ash=4, sand=5): scan top-down within the brick. A cell falls into air below, or else tries 2 diagonals
   in an order picked by `hash(2·tick+1, cell)`. A `MOVED` bit in `state` stops a cell moving twice in one tick, and the bit is cleared when the scan reaches it.

The hash is the murmur3/splitmix-style finaliser on `(b·0x9E3779B1) ^ (a·0x85EBCA77)`. It uses only `Math.imul`/`wrapping_mul`, with no floats and no `Math.random`.

Brick independence within a tick: diffusion only *reads* neighbour bricks from the previous buffer. Fire and gravity only
*write* inside their own brick. So the result does not depend on brick order, which is what makes threading trivially
deterministic.

## Results (median of 3 runs, 200 ticks)

| bricks (cells) | engine | mean ms/tick | p95 ms/tick | ns/cell-update | vs TS 1-thread | sim memory |
|---|---|---:|---:|---:|---:|---:|
| 64 (0.26 M) | ts | 4.54 | 5.36 | 17.31 | 1.0x | 1.3 MiB |
| 64 (0.26 M) | wasm-plain | 1.86 | 2.14 | 7.08 | 2.4x | 3.3 MiB |
| 64 (0.26 M) | wasm-simd | 0.90 | 1.17 | 3.43 | 5.0x | 3.3 MiB |
| 64 (0.26 M) | ts-x2-checker | 2.77 | 3.41 | 10.56 | 1.6x | 1.3 MiB |
| 64 (0.26 M) | wasm-shared-simd-x2-checker | 0.88 | 1.05 | 3.34 | 5.2x | 3.3 MiB |
| 64 (0.26 M) | ts-x4-checker | 2.06 | 3.36 | 7.86 | 2.2x | 1.3 MiB |
| 64 (0.26 M) | wasm-shared-simd-x4-checker | 0.83 | 1.18 | 3.15 | 5.5x | 3.3 MiB |
| 64 (0.26 M) | ts-x4-free | 1.92 | 3.04 | 7.32 | 2.4x | 1.3 MiB |
| 64 (0.26 M) | wasm-shared-x4-free | 0.75 | 1.19 | 2.85 | 6.1x | 3.3 MiB |
| 64 (0.26 M) | wasm-shared-simd-x4-free | 0.55 | 0.80 | 2.08 | 8.3x | 3.3 MiB |
| 256 (1.05 M) | ts | 12.70 | 16.54 | 12.11 | 1.0x | 5.0 MiB |
| 256 (1.05 M) | wasm-plain | 6.07 | 7.80 | 5.79 | 2.1x | 7.1 MiB |
| 256 (1.05 M) | wasm-simd | 3.56 | 4.58 | 3.40 | 3.6x | 7.1 MiB |
| 256 (1.05 M) | ts-x2-checker | 10.57 | 11.48 | 10.08 | 1.2x | 5.0 MiB |
| 256 (1.05 M) | wasm-shared-simd-x2-checker | 2.76 | 3.31 | 2.64 | 4.6x | 7.1 MiB |
| 256 (1.05 M) | ts-x4-checker | 6.88 | 9.09 | 6.56 | 1.8x | 5.0 MiB |
| 256 (1.05 M) | wasm-shared-simd-x4-checker | 1.73 | 2.23 | 1.65 | 7.3x | 7.1 MiB |
| 256 (1.05 M) | ts-x4-free | 6.40 | 10.36 | 6.10 | 2.0x | 5.0 MiB |
| 256 (1.05 M) | wasm-shared-x4-free | 3.00 | 4.33 | 2.86 | 4.2x | 7.1 MiB |
| 256 (1.05 M) | wasm-shared-simd-x4-free | 1.80 | 2.68 | 1.72 | 7.1x | 7.1 MiB |
| 1024 (4.19 M) | ts | 53.31 | 79.69 | 12.71 | 1.0x | 20.0 MiB |
| 1024 (4.19 M) | wasm-plain | 20.33 | 30.10 | 4.85 | 2.6x | 22.1 MiB |
| 1024 (4.19 M) | wasm-simd | 14.75 | 21.36 | 3.52 | 3.6x | 22.1 MiB |
| 1024 (4.19 M) | ts-x2-checker | 28.52 | 41.49 | 6.80 | 1.9x | 20.0 MiB |
| 1024 (4.19 M) | wasm-shared-simd-x2-checker | 8.85 | 10.67 | 2.11 | 6.0x | 22.1 MiB |
| 1024 (4.19 M) | ts-x4-checker | 24.03 | 29.50 | 5.73 | 2.2x | 20.0 MiB |
| 1024 (4.19 M) | wasm-shared-simd-x4-checker | 5.89 | 7.25 | 1.40 | 9.0x | 22.1 MiB |
| 1024 (4.19 M) | ts-x4-free | 21.91 | 27.00 | 5.22 | 2.4x | 20.0 MiB |
| 1024 (4.19 M) | wasm-shared-x4-free | 8.84 | 10.20 | 2.11 | 6.0x | 22.1 MiB |
| 1024 (4.19 M) | wasm-shared-simd-x4-free | 6.85 | 10.39 | 1.63 | 7.8x | 22.1 MiB |

State hashes after 200 ticks were identical for every engine in all 3 runs: `64: 4cf9dce6`, `256: 23dca13d`, `1024: fca5ea99`.

Engine names: `ts` / `wasm-plain` / `wasm-simd` are single-threaded on the main thread. `-xK` means K `worker_threads`
over a `SharedArrayBuffer` (TS) or a shared `WebAssembly.Memory` (Rust built with `+atomics`, `--shared-memory`).
`checker` uses an 8-colour 2×2×2 brick checkerboard with a barrier after each colour (7 barriers plus a join per tick).
`free` uses one pass with one join per tick.

### Where the time goes (`pnpm --filter cell-step-bench phases`, single thread, ns/cell)

| phase | TS (256 / 1024 bricks) | WASM plain | WASM +simd128 (hand-written diffusion) |
|---|---:|---:|---:|
| diffusion | 5.0 / 6.8 | 2.1 / 4.1 | **0.54 / 0.94** |
| fire | 2.8 / 4.2 | 0.59 / 1.13 | 0.55 / 1.10 |
| gravity | 4.4 / 4.6 | 1.8 / 1.8 | 1.8 / 1.9 |

### Memory

- Each brick takes **20 KiB**: `material` 8 KiB + `temp` 4 KiB × 2 (double buffer) + `state` 4 KiB. The 6-entry neighbour table is negligible. That makes 1024 bricks = 20 MiB.
- WASM adds a fixed 2 MiB below the world for Rust's shadow stack. The kernel never uses the stack, and `__heap_base` is checked at load.
  The `.wasm` files are 2.6–3.4 KB. WASM linear memory never shrinks, so the game needs a brick pool or free-list inside it.
- Process RSS for the whole bench (all engines at 1024 bricks + 4 workers) was ~210 MiB, mostly V8 and isolate overhead.

## Findings, including the surprising ones

1. **`-C target-feature=+simd128` alone gave nothing (±0%).** LLVM did not auto-vectorise the scalar diffusion loop:
   the raw pointers may alias, and the edge handling plus the u16 material test get in the way. The asm showed only
   `extract_lane`. A ~40-line explicit `core::arch::wasm32` version of the row update (`diffuse_row_simd`) is what gave 4×.
   It is bit-identical to the scalar path, and the hashes prove it.
2. **Gravity is now the bottleneck** (~50–60% of WASM+SIMD time). It is a scalar scan that touches `state` and `material`
   for every cell, even though almost nothing moves. The next win in either language is
   skipping work: a per-brick or per-row "has loose material / has MOVED bits" mask, or a SIMD
   scan of 16 cells at a time. This is algorithmic and applies to TS too. Only the SIMD scan is WASM-only.
3. **Threads scale worse than you would hope**: WASM+SIMD gave 1.0× at 64 bricks, 2.1× at 256 and 2.5× at 1024 with 4 workers
   (TS gave 2.2× at 1024). At small N, per-tick sync (`Atomics.wait`/`notify` wake-up ≈ tens of µs × barriers) cancels out
   the gain. At large N, the single-threaded 1024-brick case is already partly memory-bound: ns/cell rises from 3.4 to 3.5
   (WASM) and from 0.54 to 0.94 for diffusion, because the working set (~22 MiB) exceeds the per-core cache.
   Only 4 vCPUs on a shared VM were available.
4. **Checkerboard cost**: 7 extra barriers per tick cost ~0.3 ms at 64 bricks (0.83 vs 0.55 ms). At 1024 bricks the cost is in the noise.
5. **TS workers pay JIT warm-up again in every worker**: their first-20-tick mean is 1.5–3× their steady state.
   WASM workers instantiate a pre-compiled `WebAssembly.Module` that is posted to them, so they start at full speed.
6. TS ns/cell is *worse* at 64 bricks (17 ns) than at 256 bricks (12 ns) in the median, but it ranged 11.6–18.5 ns across
   runs. Treat that size as noise-dominated.

## Cross-brick moves (not implemented, cost estimate)

Gravity here stays within each brick, so a grain stops at a brick floor. Allowing moves across bricks means a brick
*writes* into its neighbours. That breaks brick independence, which leaves two options:

- **Checkerboard** (implemented here for scheduling and measured above): 8 colours, with a barrier between colours.
  Bricks of the same colour never touch, so writes are race-free, and the fixed colour order keeps it deterministic. The cost is
  ~7 extra barriers per tick (measured ~0.04 ms each on this box) plus a neighbour-table lookup on the slow path for boundary cells (the y=0 layer, 1/16 of cells, for straight falls; x/z edge columns for diagonals).
  It also introduces a direction bias from the colour order, which needs alternating colour order by tick parity.
- **Outbox / margin exchange**: each brick appends boundary moves to a per-brick queue, and a second deterministic pass
  resolves conflicts in a fixed order. This needs only one barrier, but it adds code and a second pass over usually-empty queues.

Both are the same amount of work in TS and Rust, so they do not change the recommendation.

## Caveats

- **This is Node, not a browser.** Chrome uses the same V8 (newer), so expect similar ratios. Firefox and Safari JITs differ for both
  JS and WASM. Re-run in each browser before committing to frame budgets.
- **Browser threading needs cross-origin isolation** (COOP/COEP headers) for `SharedArrayBuffer` and shared WASM memory.
  Without it you are limited to the single-threaded numbers.
- **Rust threads on stable**: `+atomics` warns that it is unstable, but it builds and works because the crate is `#![no_std]`, has
  no allocator and no statics, and JS owns the memory layout. Using `std` or an allocator with shared memory
  needs nightly `-Z build-std`. The kernel also must not use the shadow stack, because all instances share one region.
  The asm was checked and shows no `__stack_pointer` use.
- The Rust kernel uses raw pointers (no bounds checks). V8 keeps most typed-array bounds checks. That is a fair
  "what each language gives you" comparison, but not a like-for-like instruction count.
- The TS version is idiomatic and reasonably tuned (row-based stencil, no allocation, monomorphic typed arrays).
  It was not hand-optimised further, for example by reading 4 cells at a time through `Uint32Array`.
- These are single-machine, small-sample results with ±30% run-to-run variance and other load present. Ratios are more trustworthy than absolute ms.
- Every brick is active here. Real worlds have most bricks asleep, which favours whichever side is cheaper to wake and skip.
  That is mostly scheduling code, which can live in either language.

## How to run

```sh
pnpm --filter cell-step-bench bench      # builds 4 wasm variants (cargo) then benches 64/256/1024 bricks
pnpm --filter cell-step-bench phases     # per-phase breakdown (node src/phases.ts --n 1024 for other sizes)
pnpm --filter cell-step-bench test       # vitest: TS == WASM == WASM+simd == worker engines (state hash)
node src/bench.ts --sizes 256 --ticks 400 --workers 2,3,4   # custom
```

Files: `src/sim.ts` (TS rule), `rust/src/lib.rs` (Rust rule + SIMD diffusion), `src/world.ts` (layout, terrain, FNV-1a),
`src/engines.ts` (single-thread engines), `src/parallel.ts` + `src/worker.ts` (threads + checkerboard),
`scripts/build-wasm.mjs` (plain / simd / shared / shared-simd builds), `test/determinism.test.ts`.
Raw results: `results/run{1,2,3}.json` (the runs behind the table) and `results/latest.{md,json}` (git-ignored).
