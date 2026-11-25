// How long does a late joiner take to replay history? Measures ticks/second
// of headless replay for a quiet world and for one with fire and falling
// blocks, and what that means for joining after an hour of play.
import { DEFAULT_SPEC, generate } from '../src/gen'
import { M } from '../src/materials'
import { initPhysics } from '../src/physics'
import { Sim } from '../src/sim'
await initPhysics()

function measure(label: string, busy: boolean, ticks: number) {
  const sim = new Sim(generate(DEFAULT_SPEC), DEFAULT_SPEC.seed)
  if (busy) {
    for (let i = 0; i < 6; i++) {
      const x = 30 + i * 25, y = 60 + (i % 3) * 30
      let z = 0; while (sim.world.get(x, y, z + 1) !== M.Air) z++
      sim.schedule({ tick: 1 + i * 100, peer: 0, seq: 0, command: { kind: 'ignite', x, y, z } })
      sim.schedule({ tick: 2 + i * 100, peer: 0, seq: 1, command: { kind: 'drop', x: x + 5, y: y + 5, z } })
    }
  }
  const t0 = performance.now()
  for (let i = 0; i < ticks; i++) sim.step()
  const ms = performance.now() - t0
  const perTick = ms / ticks
  return { label, ticks, msPerTick: perTick.toFixed(3), replaySpeed: `${(50 / perTick).toFixed(0)}× real time`, joinAfter1h: `${((72000 * perTick) / 1000).toFixed(1)} s` }
}
console.table([measure('quiet valley', false, 2000), measure('fires + falling blocks', true, 2000)])
