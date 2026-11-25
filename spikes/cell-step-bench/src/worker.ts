import { parentPort, workerData } from "node:worker_threads";
import { type StepFn, instantiate } from "./engines.ts";
import type { WorkerInit } from "./parallel.ts";
import { stepBricks } from "./sim.ts";
import { makeViews, tempBuffers, tempOffsets } from "./world.ts";

const init = workerData as WorkerInit;
const L = init.layout;
const ctrl = new Int32Array(init.ctrl);
const W = init.workers;
const slot = init.listIndex * L.n;

let stepSeg: (start: number, count: number, tick: number) => void;
if (init.kind === "ts") {
  const v = makeViews(init.buffer!, L);
  stepSeg = (start, count, tick) => {
    const [tc, tn] = tempBuffers(v, tick);
    stepBricks(v.mat, tc, tn, v.state, v.nb, v.lists, slot + start, count, tick);
  };
} else {
  const step: StepFn = instantiate(init.module!, init.memory!);
  stepSeg = (start, count, tick) => {
    const [tc, tn] = tempOffsets(L, tick);
    step(L.matOff, tc, tn, L.stateOff, L.nbOff, L.listOff + (slot + start) * 4, count, tick);
  };
}

function barrier(): void {
  const gen = Atomics.load(ctrl, 5);
  if (Atomics.add(ctrl, 4, 1) === W - 1) {
    Atomics.store(ctrl, 4, 0);
    Atomics.add(ctrl, 5, 1);
    Atomics.notify(ctrl, 5);
  } else {
    while (Atomics.load(ctrl, 5) === gen) Atomics.wait(ctrl, 5, gen);
  }
}

let seq = Atomics.load(ctrl, 0);
parentPort!.postMessage("ready");
for (;;) {
  Atomics.wait(ctrl, 0, seq);
  const now = Atomics.load(ctrl, 0);
  if (now === seq) continue;
  seq = now;
  const tick = Atomics.load(ctrl, 2);
  if (tick < 0) break;
  const segs = init.segments;
  for (let c = 0; c < segs.length; c++) {
    const [start, count] = segs[c]!;
    if (count > 0) stepSeg(start, count, tick);
    if (c < segs.length - 1) barrier();
  }
  if (Atomics.add(ctrl, 1, 1) === W - 1) Atomics.notify(ctrl, 1);
}
process.exit(0);
