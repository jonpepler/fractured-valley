// A worker thread that computes its slice of each air-step phase into shared
// memory. It waits on a shared counter; each round it runs the phase the main
// thread posted, for its slice of the planned bricks, then signals done. The
// phase functions are the same ones the main thread uses single-threaded.
import { workerData } from 'node:worker_threads'
import { computeBricks, type FieldView, KINDS } from './fields'
import { flowDivergence, flowForces, flowProject, flowRound, type FlowView, finalPressure, Phase } from './flow'

interface Init {
  index: number
  dims: { nx: number; ny: number; nz: number; bx: number; by: number }
  /** Every gas kind's buffer for parity 0, then every kind's for parity 1. */
  buffers: SharedArrayBuffer[]
  solid: SharedArrayBuffer
  /** Air, if it moves: [u, v, w] for parity 0 then parity 1; div, p, pNext, mask. */
  air?: { vel: SharedArrayBuffer[]; div: SharedArrayBuffer; p: SharedArrayBuffer; pNext: SharedArrayBuffer; mask: SharedArrayBuffer }
  /** [generation, done, seed, tick, gasParity, phase, round, velParity] */
  control: SharedArrayBuffer
  /** Per worker: [start, length] into `jobs`, from offset 2*index. Then brick ids from 2*workers. */
  jobs: SharedArrayBuffer
  /** Per worker: count, then brick ids, in a block of `stride`. */
  out: SharedArrayBuffer
  stride: number
}


const d = workerData as Init
const control = new Int32Array(d.control)
const jobs = new Int32Array(d.jobs)
const out = new Int32Array(d.out)
const gas = d.buffers.map(b => new Uint16Array(b))
const solid = new Uint8Array(d.solid)
const vel = d.air?.vel.map(b => new Int32Array(b)) ?? []
const gasView = (parity: number): FieldView => ({
  ...d.dims, solid,
  cur: parity === 0 ? gas.slice(0, KINDS) : gas.slice(KINDS),
  next: parity === 0 ? gas.slice(KINDS) : gas.slice(0, KINDS),
  // The gas phase runs after the air has swapped, so the parity names the new velocity.
  ...(vel.length ? { vel: control[7] === 0 ? vel.slice(0, 3) : vel.slice(3) } : {}),
})
const airView = (gasParity: number, velParity: number): FlowView => ({
  ...d.dims, solid, heat: gas[gasParity === 0 ? 0 : KINDS]!,
  vel: velParity === 0 ? vel.slice(0, 3) : vel.slice(3), velNext: velParity === 0 ? vel.slice(3) : vel.slice(0, 3),
  div: new Int32Array(d.air!.div), p: new Int32Array(d.air!.p), pNext: new Int32Array(d.air!.pNext), mask: new Uint8Array(d.air!.mask),
})

let seen = 0
for (;;) {
  Atomics.wait(control, 0, seen)
  seen = Atomics.load(control, 0)
  if (seen < 0) break
  const start = jobs[2 * d.index]!, len = jobs[2 * d.index + 1]!
  const bricks = Array.from(jobs.subarray(start, start + len))
  const seed = control[2]!, tick = control[3]!, gasParity = control[4]!, phase = control[5]!, round = control[6]!, velParity = control[7]!
  let result: number[] = []
  if (phase === Phase.Gas) result = computeBricks(gasView(gasParity), bricks, seed, tick)
  if (phase === Phase.Forces) flowForces(airView(gasParity, velParity), bricks, seed, tick)
  if (phase === Phase.Divergence) flowDivergence(airView(gasParity, velParity), bricks)
  if (phase === Phase.Round) {
    const v = airView(gasParity, velParity)
    flowRound(v, bricks, round % 2 ? v.pNext : v.p, round % 2 ? v.p : v.pNext)
  }
  if (phase === Phase.Project) { const v = airView(gasParity, velParity); result = flowProject(v, bricks, finalPressure(v)) }
  const o = d.index * d.stride
  out[o] = result.length
  out.set(result, o + 1)
  Atomics.add(control, 1, 1)
  Atomics.notify(control, 1)
}
