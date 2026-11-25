// Multi-threaded engine: bricks split across worker_threads over a SharedArrayBuffer
// (TS) or a shared WebAssembly.Memory (Rust built with +atomics / --shared-memory).
//
// Modes:
//  - "checker": 8-colour (2x2x2) checkerboard. All workers step colour 0, barrier,
//    colour 1, barrier, ... This is what you need once rules write into neighbour
//    bricks (cross-brick gravity / fire spread): two bricks of the same colour are
//    never face/edge/corner neighbours, so they can be stepped concurrently.
//  - "free": one pass, one barrier per tick. Valid for the current rule set, because
//    diffusion only reads the previous temp buffer of neighbours and fire/gravity only
//    write inside the brick being stepped.
// Either way the result is bit-identical to the single-threaded order (checked by hash).

import { Worker } from "node:worker_threads";
import { WASM_BASE, type Engine, loadWasmModule, pagesFor } from "./engines.ts";
import { type Layout, brickCoords, initWorld, makeLayout, makeViews } from "./world.ts";

export type ParallelKind = "ts" | "wasm-shared" | "wasm-shared-simd";
export type ParallelMode = "checker" | "free";

export interface ParallelEngine extends Engine {
  close(): Promise<void>;
}

/** ctrl layout (Int32): 0 seq, 1 done count, 2 tick (-1 = quit), 4 barrier count, 5 barrier gen */
export const CTRL_LEN = 8;

export interface WorkerInit {
  kind: ParallelKind;
  buffer: SharedArrayBuffer | null;
  memory: WebAssembly.Memory | null;
  module: WebAssembly.Module | null;
  layout: Layout;
  ctrl: SharedArrayBuffer;
  workers: number;
  listIndex: number; // which lists slot (1 + worker id)
  segments: [number, number][]; // [start, count] per colour, relative to the list slot
}

export async function makeParallelEngine(
  n: number,
  kind: ParallelKind,
  workers: number,
  mode: ParallelMode,
): Promise<ParallelEngine> {
  let layout: Layout;
  let buffer: SharedArrayBuffer;
  let memory: WebAssembly.Memory | null = null;
  let module: WebAssembly.Module | null = null;
  if (kind === "ts") {
    layout = makeLayout(n, 0, workers);
    buffer = new SharedArrayBuffer(layout.end);
  } else {
    layout = makeLayout(n, WASM_BASE, workers);
    memory = new WebAssembly.Memory({ initial: pagesFor(layout), maximum: 16384, shared: true });
    buffer = memory.buffer as unknown as SharedArrayBuffer;
    module = loadWasmModule(kind === "wasm-shared" ? "shared" : "shared-simd");
  }
  const views = makeViews(buffer, layout);
  initWorld(layout, views);

  // Assign bricks: per colour, round-robin across workers.
  const colours = mode === "checker" ? 8 : 1;
  const perWorker: number[][][] = Array.from({ length: workers }, () => Array.from({ length: colours }, () => []));
  for (let col = 0; col < colours; col++) {
    let k = 0;
    for (let b = 0; b < n; b++) {
      const [x, y, z] = brickCoords(layout, b);
      const c = mode === "checker" ? (x & 1) | ((y & 1) << 1) | ((z & 1) << 2) : 0;
      if (c !== col) continue;
      perWorker[k % workers]![col]!.push(b);
      k++;
    }
  }
  const ctrlBuf = new SharedArrayBuffer(CTRL_LEN * 4);
  const ctrl = new Int32Array(ctrlBuf);
  const threads: Worker[] = [];
  const ready: Promise<void>[] = [];
  for (let w = 0; w < workers; w++) {
    const slot = 1 + w;
    const segments: [number, number][] = [];
    let off = 0;
    for (const list of perWorker[w]!) {
      views.lists.set(list, slot * n + off);
      segments.push([off, list.length]);
      off += list.length;
    }
    const init: WorkerInit = {
      kind,
      buffer: kind === "ts" ? buffer : null,
      memory,
      module,
      layout,
      ctrl: ctrlBuf,
      workers,
      listIndex: slot,
      segments,
    };
    const t = new Worker(new URL("./worker.ts", import.meta.url), { workerData: init });
    threads.push(t);
    ready.push(
      new Promise((resolve, reject) => {
        t.once("message", () => resolve());
        t.once("error", reject);
      }),
    );
  }
  await Promise.all(ready);

  return {
    name: `${kind}-x${workers}-${mode}`,
    layout,
    views,
    bytes: buffer.byteLength,
    step(tick) {
      Atomics.store(ctrl, 1, 0);
      Atomics.store(ctrl, 2, tick);
      Atomics.add(ctrl, 0, 1);
      Atomics.notify(ctrl, 0);
      let d: number;
      while ((d = Atomics.load(ctrl, 1)) < workers) Atomics.wait(ctrl, 1, d);
    },
    async close() {
      Atomics.store(ctrl, 2, -1);
      Atomics.add(ctrl, 0, 1);
      Atomics.notify(ctrl, 0);
      await Promise.all(threads.map((t) => new Promise((r) => t.once("exit", r))));
    },
  };
}
