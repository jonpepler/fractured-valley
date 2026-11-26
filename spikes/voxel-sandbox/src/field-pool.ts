// Steps a Fields instance across worker threads (Node). Each worker takes a
// contiguous slice of the planned bricks for every phase: moving the air
// (forces, divergence, each pressure round, projection) and then the gases.
// They read the same shared state and write disjoint bricks, with a barrier
// between phases, so the result is bit-identical to a single-threaded step
// whatever the thread count (decision 2.7b).
// In a browser the same scheme needs cross-origin isolation for shared memory.
import { Worker } from 'node:worker_threads'
import type { Fields } from './fields'
import { flowMark, Phase, PRESSURE_ROUNDS } from './flow'

export class FieldPool {
  private workers: Worker[] = []
  private control: Int32Array
  private jobs: Int32Array
  private out: Int32Array
  private stride: number
  private generation = 0

  constructor(private readonly fields: Fields, threads: number, bricks: number) {
    const v = fields.view()
    if (fields.parity !== 0 || fields.velParity !== 0) throw new Error('create the pool before the first step')
    const both = [...fields.cur, ...fields.next] // parity 0: cur first
    const air = fields.vel ? (() => {
      const a = fields.airView()
      return { vel: [...a.vel, ...a.velNext].map(x => x.buffer as SharedArrayBuffer), div: a.div.buffer, p: a.p.buffer, pNext: a.pNext.buffer, mask: a.mask.buffer }
    })() : undefined
    this.control = new Int32Array(new SharedArrayBuffer(4 * 8))
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
          air,
          control: this.control.buffer,
          jobs: this.jobs.buffer,
          out: this.out.buffer,
          stride: this.stride,
        },
      }))
    }
  }

  /** Run one phase on every worker and wait for all of them (the barrier). Returns their brick lists, merged and sorted. */
  private run(phase: Phase, seed: number, tick: number, round = 0): number[] {
    const n = this.workers.length
    this.control[1] = 0
    this.control[2] = seed; this.control[3] = tick; this.control[4] = this.fields.parity
    this.control[5] = phase; this.control[6] = round; this.control[7] = this.fields.velParity
    Atomics.store(this.control, 0, ++this.generation)
    Atomics.notify(this.control, 0)
    let done: number
    while ((done = Atomics.load(this.control, 1)) < n) Atomics.wait(this.control, 1, done)
    const all: number[] = []
    for (let w = 0; w < n; w++) {
      const o = w * this.stride
      for (let k = 0; k < this.out[o]!; k++) all.push(this.out[o + 1 + k]!)
    }
    return all.sort((a, b) => a - b)
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
    if (this.fields.vel) {
      const view = this.fields.airView()
      flowMark(view, bricks, 1)
      this.run(Phase.Forces, seed, tick)
      this.run(Phase.Divergence, seed, tick)
      for (let r = 0; r < PRESSURE_ROUNDS; r++) this.run(Phase.Round, seed, tick, r)
      const moving = this.run(Phase.Project, seed, tick)
      flowMark(view, bricks, 0)
      this.fields.finishAir(bricks, moving)
    }
    this.fields.finish(this.run(Phase.Gas, seed, tick))
  }

  async close() {
    Atomics.store(this.control, 0, -1)
    Atomics.notify(this.control, 0)
    await Promise.all(this.workers.map(w => w.terminate()))
  }
}
