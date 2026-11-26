// Steps a Fields instance across worker threads (Node). Each worker takes a
// contiguous slice of the planned bricks; they read the same shared `cur`
// and write disjoint bricks of `next`, so the result is bit-identical to a
// single-threaded step whatever the thread count (decision 2.7b).
// In a browser the same scheme needs cross-origin isolation for shared memory.
import { Worker } from 'node:worker_threads'
import type { Fields } from './fields'

export class FieldPool {
  private workers: Worker[] = []
  private control: Int32Array
  private jobs: Int32Array
  private out: Int32Array
  private stride: number
  private generation = 0

  constructor(private readonly fields: Fields, threads: number, bricks: number) {
    const v = fields.view()
    const both = [...fields.cur, ...fields.next] // parity 0: cur first
    if (fields.parity !== 0) throw new Error('create the pool before the first step')
    this.control = new Int32Array(new SharedArrayBuffer(4 * 5))
    this.jobs = new Int32Array(new SharedArrayBuffer(4 * (2 * threads + bricks)))
    this.stride = bricks + 1
    this.out = new Int32Array(new SharedArrayBuffer(4 * threads * this.stride))
    for (let index = 0; index < threads; index++) {
      this.workers.push(new Worker(new URL('./field-worker.ts', import.meta.url), {
        execArgv: ['--import', 'tsx'],
        workerData: {
          index,
          dims: { nx: v.nx, ny: v.ny, nz: v.nz, bx: v.bx, by: v.by },
          buffers: both.map(a => a.buffer),
          solid: v.solid.buffer,
          control: this.control.buffer,
          jobs: this.jobs.buffer,
          out: this.out.buffer,
          stride: this.stride,
        },
      }))
    }
  }

  /** One field step, computed by the workers. Blocks until all are done. */
  step(seed: number, tick: number) {
    const bricks = this.fields.plan()
    const n = this.workers.length
    const per = Math.ceil(bricks.length / n)
    let at = 2 * n
    for (let w = 0; w < n; w++) {
      const slice = bricks.slice(w * per, (w + 1) * per)
      this.jobs[2 * w] = at; this.jobs[2 * w + 1] = slice.length
      this.jobs.set(slice, at)
      at += slice.length
    }
    this.control[1] = 0
    this.control[2] = seed; this.control[3] = tick; this.control[4] = this.fields.parity
    Atomics.store(this.control, 0, ++this.generation)
    Atomics.notify(this.control, 0)
    let done: number
    while ((done = Atomics.load(this.control, 1)) < n) Atomics.wait(this.control, 1, done)
    const withGas: number[] = []
    for (let w = 0; w < n; w++) {
      const o = w * this.stride
      for (let k = 0; k < this.out[o]!; k++) withGas.push(this.out[o + 1 + k]!)
    }
    this.fields.finish(withGas.sort((a, b) => a - b))
  }

  async close() {
    Atomics.store(this.control, 0, -1)
    Atomics.notify(this.control, 0)
    await Promise.all(this.workers.map(w => w.terminate()))
  }
}
