// What does joining cost with checkpoints? A busy valley (6 fires, 6 metal
// blocks) is checkpointed at a quiet moment after 1, 5 and 15 minutes. For
// each: checkpoint size (raw, and deflated as it would travel), time to take
// and restore it, and the worst-case replay of up to 60 s of inputs after it.
import { deflateRawSync } from 'node:zlib'
import { restoreCheckpoint, takeCheckpoint } from '../src/checkpoint'
import { DEFAULT_SPEC, generate } from '../src/gen'
import { M } from '../src/materials'
import { initPhysics } from '../src/physics'
import { Sim } from '../src/sim'
await initPhysics()

const spec = DEFAULT_SPEC
const sim = new Sim(generate(spec), spec.seed)
const fresh = generate(spec)
// Fires and blocks keep arriving all session, as players would cause them.
let seq = 0
for (let i = 0; i < 90; i++) {
  const x = 20 + ((i * 37) % 150), y = 20 + ((i * 53) % 150)
  let z = spec.sz - 1; while (z > 0 && sim.world.get(x, y, z) === M.Air) z--
  sim.schedule({ tick: 1 + i * 200, peer: 0, seq: seq++, command: { kind: i % 2 ? 'drop' : 'ignite', x, y, z } })
}
const rows = []
for (const minutes of [1, 5, 15]) {
  while (sim.tick < minutes * 1200 || sim.bodies.length) sim.step()
  let t = performance.now()
  const { data, stats } = takeCheckpoint(sim, spec, fresh)
  const takeMs = performance.now() - t
  const deflated = deflateRawSync(data, { level: 6 }).length
  t = performance.now()
  const { sim: joiner } = restoreCheckpoint(data)
  const restoreMs = performance.now() - t
  if (joiner.hash() !== sim.hash()) throw new Error('restored world differs')
  // Worst case: the checkpoint is a full minute old.
  t = performance.now()
  for (let i = 0; i < 1200; i++) joiner.step()
  const replayMs = performance.now() - t
  for (let i = 0; i < 1200; i++) sim.step()
  if (joiner.hash() !== sim.hash()) throw new Error('joiner fell out of step')
  rows.push({
    at: `${minutes} min`, changedBricks: `${stats.changedBricks}/${stats.bricks}`,
    rawKiB: +(stats.bytes / 1024).toFixed(0), sentKiB: +(deflated / 1024).toFixed(0),
    takeMs: +takeMs.toFixed(0), restoreMs: +restoreMs.toFixed(0), replay60sMs: +replayMs.toFixed(0),
    joinS: +((restoreMs + replayMs) / 1000).toFixed(1),
  })
}
console.table(rows)
