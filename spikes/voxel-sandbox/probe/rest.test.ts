import { beforeAll, test } from 'vitest'
import { DEFAULT_SPEC, generate } from '../src/gen'
import { M } from '../src/materials'
import { initPhysics } from '../src/physics'
import { Sim } from '../src/sim'
beforeAll(() => initPhysics())
const SPEC = { ...DEFAULT_SPEC, sx: 96, sy: 96 }
const POST = { x: Math.floor(SPEC.sx * 0.52), y: Math.floor(SPEC.sy * 0.56) }
// The post test's scenario: which bodies are still moving at the end, and when do they stop?
test('rest', { timeout: 600_000 }, () => {
  const sim = new Sim(generate(SPEC), SPEC.seed, { airFlow: false })
  const gAt = (x: number, y: number) => { let z = 0; while (sim.world.get(x, y, z + 1) !== M.Air) z++; return z }
  sim.schedule({ tick: 1, peer: 0, seq: 0, command: { kind: 'ignite', x: POST.x - 4, y: POST.y - 4, z: gAt(POST.x - 4, POST.y - 4) } })
  sim.schedule({ tick: 5, peer: 1, seq: 0, command: { kind: 'drop', x: 30, y: 70, z: 60 } })
  for (let i = 0; i < 1400; i++) {
    sim.step()
    if (i >= 880 && i % 40 === 0) console.log(sim.tick, 'bodies', sim.bodies.length, JSON.stringify(sim.events.filter(e => e.kind === 'detached' || e.kind === 'landed').slice(-4).map(e => `${e.tick}:${e.kind}:${e.cells}@${e.at}`)))
  }
  console.log(JSON.stringify(sim.eventCounts))
})
