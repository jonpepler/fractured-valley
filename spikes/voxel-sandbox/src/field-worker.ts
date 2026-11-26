// A worker thread that computes gas-field bricks into shared memory.
// It waits on a shared counter; each round it computes its slice of the
// planned bricks with the same kernel as the main thread, then signals done.
import { workerData } from 'node:worker_threads'
import { computeBricks, type FieldView, KINDS } from './fields'

interface Init {
  index: number
  dims: { nx: number; ny: number; nz: number; bx: number; by: number }
  /** Every kind's buffer for parity 0, then every kind's for parity 1. */
  buffers: SharedArrayBuffer[]
  solid: SharedArrayBuffer
  /** [generation, done, seed, tick, parity] */
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
const arr = d.buffers.map(b => new Uint16Array(b))
const view = (parity: number): FieldView => ({
  ...d.dims,
  cur: parity === 0 ? arr.slice(0, KINDS) : arr.slice(KINDS),
  next: parity === 0 ? arr.slice(KINDS) : arr.slice(0, KINDS),
  solid: new Uint8Array(d.solid),
})

let seen = 0
for (;;) {
  Atomics.wait(control, 0, seen)
  seen = Atomics.load(control, 0)
  if (seen < 0) break
  const start = jobs[2 * d.index]!, len = jobs[2 * d.index + 1]!
  const bricks = Array.from(jobs.subarray(start, start + len))
  const withGas = computeBricks(view(control[4]!), bricks, control[2]!, control[3]!)
  const o = d.index * d.stride
  out[o] = withGas.length
  out.set(withGas, o + 1)
  Atomics.add(control, 1, 1)
  Atomics.notify(control, 1)
}
