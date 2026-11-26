// Loading the two WebAssembly builds, and the shared test scenario.
//
// The packages are produced by wasm-bindgen into dist/pkg-st (single-threaded)
// and dist/pkg-mt (threads). They are loaded by URL at run time rather than
// bundled: the threaded build's worker script finds its own files relative to
// where they are served, so they must stay as wasm-bindgen wrote them.

import type * as Fv from '../dist/pkg-st/fv_wasm.js'

export type FvModule = typeof Fv & { initThreadPool?: (n: number) => Promise<void> }
export type Engine = Fv.Engine

/** The determinism scenario. fv-wasm/examples/native_hashes.rs uses the same. */
export const SCENARIO = { seed: 5, n: 32, nz: 32, lit: 40, ticks: 60 } as const

/** Load and start one build: 'st' (single-threaded) or 'mt' (threads). */
export async function load(build: 'st' | 'mt'): Promise<FvModule> {
  const url = new URL(`../pkg-${build}/fv_wasm.js`, import.meta.url).href
  const m = (await import(/* @vite-ignore */ url)) as FvModule & { default: () => Promise<unknown> }
  await m.default()
  return m
}

/** A fresh copy of the scenario world. */
export function scenario(fv: FvModule): Engine {
  const s = SCENARIO
  return new fv.Engine(s.seed, s.n, s.nz, s.lit)
}

/** Run `ticks` whole ticks; return each tick's hash and the time per tick. */
export function run(e: Engine, ticks: number): { hashes: string[]; msPerTick: number } {
  const t0 = performance.now()
  const hashes = e.run(ticks).split(',')
  return { hashes, msPerTick: (performance.now() - t0) / ticks }
}
