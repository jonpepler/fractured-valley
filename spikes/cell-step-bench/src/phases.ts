// Per-phase cost breakdown (diffusion / fire / gravity), TS vs WASM plain vs WASM simd128.
//   node src/phases.ts [--n 256] [--ticks 100]
import { stepBricksPhases } from "./sim.ts";
import { WASM_BASE, loadWasmModule, pagesFor } from "./engines.ts";
import { initWorld, makeLayout, makeViews, tempBuffers, tempOffsets } from "./world.ts";

const arg = (k: string, d: number) => {
  const i = process.argv.indexOf(`--${k}`);
  return i >= 0 ? Number(process.argv[i + 1]) : d;
};
const n = arg("n", 256);
const ticks = arg("ticks", 100);
const phases: [string, number][] = [["diffuse", 1], ["fire", 2], ["gravity", 4], ["all", 7]];

function timeIt(stepOnce: (t: number) => void): number {
  for (let t = 0; t < 30; t++) stepOnce(t); // warm-up
  const t0 = performance.now();
  for (let t = 30; t < 30 + ticks; t++) stepOnce(t);
  return (performance.now() - t0) / ticks;
}

console.log(`n=${n} bricks, ${ticks} ticks, ms/tick (ns/cell)`);
for (const [name, mask] of phases) {
  const out: string[] = [];
  {
    const L = makeLayout(n, 0);
    const v = makeViews(new ArrayBuffer(L.end), L);
    initWorld(L, v);
    out.push(fmt("ts", timeIt((t) => {
      const [tc, tn] = tempBuffers(v, t);
      stepBricksPhases(v.mat, tc, tn, v.state, v.nb, v.lists, n, t, mask);
    })));
  }
  for (const variant of ["plain", "simd"] as const) {
    const L = makeLayout(n, WASM_BASE);
    const memory = new WebAssembly.Memory({ initial: pagesFor(L) });
    const inst = new WebAssembly.Instance(loadWasmModule(variant), { env: { memory } });
    const f = inst.exports.step_bricks_phases as (...a: number[]) => void;
    initWorld(L, makeViews(memory.buffer, L));
    out.push(fmt(`wasm-${variant}`, timeIt((t) => {
      const [tc, tn] = tempOffsets(L, t);
      f(L.matOff, tc, tn, L.stateOff, L.nbOff, L.listOff, n, t, mask);
    })));
  }
  console.log(`${name.padEnd(8)} ${out.join("   ")}`);
}
function fmt(label: string, ms: number): string {
  return `${label} ${ms.toFixed(3)} (${((ms * 1e6) / (n * 4096)).toFixed(2)})`;
}
