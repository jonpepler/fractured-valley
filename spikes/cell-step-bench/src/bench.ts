// Headless benchmark: TS vs Rust->WASM (plain / simd128) single-threaded, and both
// multi-threaded with worker_threads over shared memory. Verifies identical state hashes.
//
//   node src/bench.ts [--ticks 200] [--sizes 64,256,1024] [--workers 2,4]

import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import os from "node:os";
import { type Engine, makeTsEngine, makeWasmEngine } from "./engines.ts";
import { makeParallelEngine } from "./parallel.ts";
import { census, stateHash } from "./world.ts";

function arg(name: string, def: string): string {
  const i = process.argv.indexOf(`--${name}`);
  return i >= 0 ? process.argv[i + 1]! : def;
}
const TICKS = Number(arg("ticks", "200"));
const SIZES = arg("sizes", "64,256,1024").split(",").map(Number);
const WORKERS = arg("workers", "2,4").split(",").map(Number);
/** Ticks excluded from the steady-state stats (per-engine JIT / tier-up, worker spin-up). */
const SKIP = Number(arg("skip", "20"));

interface Row {
  n: number;
  engine: string;
  meanMs: number;
  p50Ms: number;
  p95Ms: number;
  warmMs: number;
  nsPerCell: number;
  memMiB: number;
  hash: string;
}

function run(e: Engine, ticks: number): { times: number[]; hash: string } {
  const times = new Array<number>(ticks);
  for (let t = 0; t < ticks; t++) {
    const t0 = performance.now();
    e.step(t);
    times[t] = performance.now() - t0;
  }
  return { times, hash: stateHash(e.views, ticks) };
}

function summarize(n: number, e: Engine, times: number[], hash: string): Row {
  const steady = times.slice(SKIP);
  const sorted = [...steady].sort((a, b) => a - b);
  const mean = steady.reduce((a, b) => a + b, 0) / steady.length;
  const warm = times.slice(0, SKIP);
  const warmMs = warm.length ? warm.reduce((a, b) => a + b, 0) / warm.length : 0;
  const q = (p: number) => sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))]!;
  return {
    n,
    engine: e.name,
    meanMs: mean,
    p50Ms: q(0.5),
    p95Ms: q(0.95),
    warmMs,
    nsPerCell: (mean * 1e6) / (n * 4096),
    memMiB: e.bytes / 2 ** 20,
    hash,
  };
}

const cpu = (() => {
  try {
    return /model name\s*:\s*(.*)/.exec(readFileSync("/proc/cpuinfo", "utf8"))?.[1] ?? os.cpus()[0]?.model;
  } catch {
    return os.cpus()[0]?.model;
  }
})();
console.log(`# cell-step-bench  ticks=${TICKS} (stats over ticks ${SKIP}..${TICKS - 1}; hash over all ${TICKS})`);
console.log(`node ${process.version}, v8 ${process.versions.v8}, ${os.platform()} ${os.arch()}`);
console.log(`cpu: ${cpu}, nproc(availableParallelism)=${os.availableParallelism()}`);

// Warm-up so TurboFan (TS) and the optimizing wasm tier are in place before timing.
for (const mk of [() => makeTsEngine(64), () => makeWasmEngine(64, "plain"), () => makeWasmEngine(64, "simd")]) {
  run(mk(), 60);
}

const rows: Row[] = [];
let mismatch = false;
for (const n of SIZES) {
  const configs: { label: string; make: () => Promise<Engine & { close?: () => Promise<void> }> }[] = [
    { label: "ts", make: async () => makeTsEngine(n) },
    { label: "wasm", make: async () => makeWasmEngine(n, "plain") },
    { label: "wasm-simd", make: async () => makeWasmEngine(n, "simd") },
  ];
  for (const w of WORKERS) {
    configs.push({ label: "", make: () => makeParallelEngine(n, "ts", w, "checker") });
    configs.push({ label: "", make: () => makeParallelEngine(n, "wasm-shared-simd", w, "checker") });
  }
  const wMax = Math.max(...WORKERS);
  configs.push({ label: "", make: () => makeParallelEngine(n, "ts", wMax, "free") });
  configs.push({ label: "", make: () => makeParallelEngine(n, "wasm-shared", wMax, "free") });
  configs.push({ label: "", make: () => makeParallelEngine(n, "wasm-shared-simd", wMax, "free") });

  let ref: string | null = null;
  for (const c of configs) {
    const e = await c.make();
    const { times, hash } = run(e, TICKS);
    if (e.close) await e.close();
    const row = summarize(n, e, times, hash);
    ref ??= hash;
    if (hash !== ref) mismatch = true;
    rows.push(row);
    console.log(
      `n=${String(n).padStart(4)} ${row.engine.padEnd(28)} mean ${row.meanMs.toFixed(3).padStart(8)} ms  p50 ${row.p50Ms
        .toFixed(3)
        .padStart(8)}  p95 ${row.p95Ms.toFixed(3).padStart(8)}  first${SKIP} ${row.warmMs.toFixed(2).padStart(7)}  ${row.nsPerCell.toFixed(2).padStart(6)} ns/cell  ${row.memMiB.toFixed(
        1,
      )} MiB  hash ${hash}${hash === ref ? "" : "  MISMATCH"}`,
    );
    if (c === configs[0]) console.log(`        census after ${TICKS} ticks: ${JSON.stringify(census(e.views))}`);
  }
}

// Markdown table for RESULTS.md
const singleTs = new Map(rows.filter((r) => r.engine === "ts").map((r) => [r.n, r.meanMs]));
const lines = [
  `| bricks | cells | engine | mean ms/tick | p50 | p95 | first ${SKIP} ticks mean | ns/cell | speedup vs TS | sim MiB | hash |`,
  "|---:|---:|---|---:|---:|---:|---:|---:|---:|---:|---|",
  ...rows.map(
    (r) =>
      `| ${r.n} | ${(r.n * 4096).toLocaleString("en")} | ${r.engine} | ${r.meanMs.toFixed(2)} | ${r.p50Ms.toFixed(2)} | ${r.p95Ms.toFixed(
        2,
      )} | ${r.warmMs.toFixed(2)} | ${r.nsPerCell.toFixed(2)} | ${(singleTs.get(r.n)! / r.meanMs).toFixed(2)}x | ${r.memMiB.toFixed(1)} | \`${r.hash}\` |`,
  ),
];
console.log("\n" + lines.join("\n"));
console.log(`\nrss ${(process.memoryUsage().rss / 2 ** 20).toFixed(0)} MiB`);
mkdirSync(new URL("../results/", import.meta.url), { recursive: true });
writeFileSync(
  new URL("../results/latest.json", import.meta.url),
  JSON.stringify({ ticks: TICKS, skip: SKIP, node: process.version, cpu, nproc: os.availableParallelism(), rows }, null, 2),
);
writeFileSync(new URL("../results/latest.md", import.meta.url), lines.join("\n") + "\n");
if (mismatch) {
  console.error("\nDETERMINISM FAILURE: state hashes differ between engines");
  process.exit(1);
}
console.log("\nall engines produced identical state hashes per size");
