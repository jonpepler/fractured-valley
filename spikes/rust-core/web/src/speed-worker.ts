// Times the dead-tree scene (as `probe_tree dead` natively: a 48×48×48 box,
// crib lit by a torch) in both builds: single-threaded, and threaded on
// `threads` Web Workers. Reports each tick's hash too, so the test can check
// both against the native build (dist/native-tree.json).

import { load } from './fv'

self.onmessage = async ({ data }: MessageEvent<{ threads: number; ticks: number }>) => {
  try {
    const time = (e: { run(n: number): string }, ticks: number) => {
      const t0 = performance.now()
      const hashes = e.run(ticks).split(',')
      return { hashes, msPerTick: (performance.now() - t0) / ticks }
    }
    const st = await load('st')
    const a = time(st.Engine.tree(4, 48, 48, true), data.ticks)
    const mt = await load('mt')
    if (!mt.threaded()) throw new Error('pkg-mt is not the threaded build')
    await mt.initThreadPool!(data.threads)
    mt.set_threads(0)
    const b = time(mt.Engine.tree(4, 48, 48, true), data.ticks)
    postMessage({ threads: data.threads, hardwareConcurrency: navigator.hardwareConcurrency, ticks: data.ticks, st: a, mt: b })
  } catch (err) {
    postMessage({ error: String(err) + (err instanceof Error && err.stack ? `\n${err.stack}` : '') })
  }
}
