import { beforeAll, test } from 'vitest'
import { DEFAULT_SPEC, generate } from '../src/gen'
import { M } from '../src/materials'
import { initPhysics } from '../src/physics'
import { Sim } from '../src/sim'
beforeAll(() => initPhysics())
const SPEC = { ...DEFAULT_SPEC, sx: 96, sy: 96 }
const POST = { x: Math.floor(SPEC.sx * 0.52), y: Math.floor(SPEC.sy * 0.56) }
test('post', { timeout: 600_000 }, () => {
  const sim = new Sim(generate(SPEC), SPEC.seed, { airFlow: false })
  const gAt = (x: number, y: number) => { let z = 0; while (sim.world.get(x, y, z + 1) !== M.Air) z++; return z }
  sim.schedule({ tick: 1, peer: 0, seq: 0, command: { kind: 'ignite', x: POST.x - 4, y: POST.y - 4, z: gAt(POST.x - 4, POST.y - 4) } })
  for (let i = 0; i < 900; i++) {
    sim.step()
    if (i % 100 === 0) {
      const counts: Record<string, number> = {}
      for (let z = 0; z < SPEC.sz; z++) for (let y = POST.y - 6; y < POST.y + 6; y++) for (let x = POST.x - 6; x < POST.x + 6; x++) {
        const m = sim.world.get(x, y, z); if (m === M.Wood) { const k = 'wood s' + (sim.world.getState(x, y, z) > 0 ? 'burn' : '0') + ' m' + sim.world.getMoisture(x, y, z); counts[k] = (counts[k] ?? 0) + 1 }
      }
      console.log(i, JSON.stringify(counts), sim.events.filter(e => e.kind !== 'crumbled' && e.kind !== 'ember-lit').map(e => e.tick + e.kind).join(' '))
    }
  }
})
