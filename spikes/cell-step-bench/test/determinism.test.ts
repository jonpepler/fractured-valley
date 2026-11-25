import { describe, expect, it } from "vitest";
import { makeTsEngine, makeWasmEngine } from "../src/engines.ts";
import { makeParallelEngine } from "../src/parallel.ts";
import { hash } from "../src/sim.ts";
import { stateHash } from "../src/world.ts";

const TICKS = 64;

function runHash(e: { step(t: number): void; views: Parameters<typeof stateHash>[0] }): string {
  for (let t = 0; t < TICKS; t++) e.step(t);
  return stateHash(e.views, TICKS);
}

describe("TS vs Rust/WASM determinism", () => {
  it("hash function is a fixed function of its inputs", () => {
    expect([hash(0, 0), hash(1, 2), hash(123456, 4095)]).toMatchInlineSnapshot(`
      [
        0,
        706246470,
        2184388575,
      ]
    `);
  });

  it("initial state is identical", () => {
    expect(stateHash(makeWasmEngine(8, "plain").views, 0)).toBe(stateHash(makeTsEngine(8).views, 0));
  });

  it(`TS, WASM and WASM+simd128 agree after ${TICKS} ticks on 16 bricks`, () => {
    const ts = runHash(makeTsEngine(16));
    expect(runHash(makeWasmEngine(16, "plain"))).toBe(ts);
    expect(runHash(makeWasmEngine(16, "simd"))).toBe(ts);
  });

  it("worker_threads engines (TS + shared-memory WASM, checkerboard) agree with single-threaded TS", async () => {
    const ref = runHash(makeTsEngine(16));
    for (const kind of ["ts", "wasm-shared", "wasm-shared-simd"] as const) {
      const e = await makeParallelEngine(16, kind, 2, "checker");
      const h = runHash(e);
      await e.close();
      expect(h, kind).toBe(ref);
    }
  });

  it("the sim actually changes state (not a vacuous match)", () => {
    const e = makeTsEngine(16);
    const before = stateHash(e.views, 0);
    expect(runHash(e)).not.toBe(before);
  });
});
