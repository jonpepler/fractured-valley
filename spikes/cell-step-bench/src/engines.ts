// Single-threaded engines: TS over an ArrayBuffer, and Rust/WASM over imported linear memory.
// Both share the same Layout, so init + hashing is identical code.

import { readFileSync } from "node:fs";
import { stepBricks } from "./sim.ts";
import { type Layout, type Views, initWorld, makeLayout, makeViews, tempBuffers, tempOffsets } from "./world.ts";

export type WasmVariant = "plain" | "simd" | "shared" | "shared-simd";

/** Region of linear memory below this is left to Rust (shadow stack, __heap_base). */
export const WASM_BASE = 2 * 1024 * 1024;
const PAGE = 65536;

export interface Engine {
  name: string;
  layout: Layout;
  views: Views;
  /** bytes of sim memory (world arrays) */
  bytes: number;
  step(tick: number): void;
}

export function makeTsEngine(n: number): Engine {
  const layout = makeLayout(n, 0);
  const views = makeViews(new ArrayBuffer(layout.end), layout);
  initWorld(layout, views);
  return {
    name: "ts",
    layout,
    views,
    bytes: layout.end,
    step(tick) {
      const [tc, tn] = tempBuffers(views, tick);
      stepBricks(views.mat, tc, tn, views.state, views.nb, views.lists, 0, n, tick);
    },
  };
}

export type StepFn = (
  mat: number,
  tc: number,
  tn: number,
  state: number,
  nb: number,
  list: number,
  count: number,
  tick: number,
) => void;

const wasmUrl = (variant: WasmVariant) => new URL(`../wasm/cell_step.${variant}.wasm`, import.meta.url);

export function loadWasmModule(variant: WasmVariant): WebAssembly.Module {
  return new WebAssembly.Module(readFileSync(wasmUrl(variant)));
}

export function instantiate(mod: WebAssembly.Module, memory: WebAssembly.Memory): StepFn {
  const inst = new WebAssembly.Instance(mod, { env: { memory } });
  const heapBase = (inst.exports.__heap_base as WebAssembly.Global).value as number;
  if (heapBase > WASM_BASE) throw new Error(`__heap_base ${heapBase} overlaps world at ${WASM_BASE}`);
  return inst.exports.step_bricks as StepFn;
}

export function pagesFor(layout: Layout): number {
  return Math.ceil(layout.end / PAGE);
}

export function makeWasmEngine(n: number, variant: "plain" | "simd"): Engine {
  const layout = makeLayout(n, WASM_BASE);
  const memory = new WebAssembly.Memory({ initial: pagesFor(layout) });
  const step = instantiate(loadWasmModule(variant), memory);
  const views = makeViews(memory.buffer, layout);
  initWorld(layout, views);
  return {
    name: `wasm-${variant}`,
    layout,
    views,
    bytes: memory.buffer.byteLength,
    step(tick) {
      const [tc, tn] = tempOffsets(layout, tick);
      step(layout.matOff, tc, tn, layout.stateOff, layout.nbOff, layout.listOff, n, tick);
    },
  };
}
