// Runs the scenario three ways and reports the hashes:
//   st:  the single-threaded build,
//   mt1: the threaded build with the pool switched off (set_threads(1)),
//   mt:  the threaded build on `threads` Web Workers.
// All three must be identical (and equal to native). Also times a bigger
// world both ways, to see whether the workers actually help.

import { SCENARIO, load, run, scenario } from './fv'

self.onmessage = async ({ data }: MessageEvent<{ threads: number }>) => {
  try {
    const st = await load('st')
    const a = run(scenario(st), SCENARIO.ticks)

    const mt = await load('mt')
    if (!mt.threaded()) throw new Error('pkg-mt is not the threaded build')
    const t0 = performance.now()
    await mt.initThreadPool!(data.threads)
    const poolMs = performance.now() - t0

    mt.set_threads(1)
    const b = run(scenario(mt), SCENARIO.ticks)
    mt.set_threads(0)
    const c = run(scenario(mt), SCENARIO.ticks)

    // A 64×64×32 world (4× the cells) for timing only.
    const big = (threads: number) => { mt.set_threads(threads); return run(new mt.Engine(5, 64, 32, 40), 10).msPerTick }
    const bigSingle = big(1)
    const bigThreaded = big(0)

    postMessage({
      threads: data.threads, hardwareConcurrency: navigator.hardwareConcurrency, poolMs,
      st: a.hashes, mt1: b.hashes, mt: c.hashes,
      msPerTick: { st: a.msPerTick, mt1: b.msPerTick, mt: c.msPerTick, big64Single: bigSingle, big64Threaded: bigThreaded },
    })
  } catch (err) {
    postMessage({ error: String(err) + (err instanceof Error && err.stack ? `\n${err.stack}` : '') })
  }
}
