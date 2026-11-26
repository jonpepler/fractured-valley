import { beforeAll, test } from 'vitest'
import { F } from '../src/fields'
import { DEFAULT_SPEC, generate } from '../src/gen'
import { M } from '../src/materials'
import { initPhysics } from '../src/physics'
import { Sim } from '../src/sim'
beforeAll(() => initPhysics())
test('flame by height', { timeout: 600_000 }, () => {
  const world = generate(DEFAULT_SPEC)
  const sim = new Sim(world, DEFAULT_SPEC.seed)
  let z = DEFAULT_SPEC.sz - 1; while (z > 0 && world.get(95, 103, z) === M.Air) z--
  sim.schedule({ tick: 5, peer: 0, seq: 0, command: { kind: 'ignite', x: 95, y: 103, z } })
  const seen = new Set<number>()
  for (let t = 1; t <= 600; t++) {
    sim.step()
    for (const c of sim.fields.cells()) if (c[3 + F.Flame]! > 0) seen.add(c[0]! * 4096 + c[1]!)
    if (t % 100) continue
    // flame per field-cell height above that column's ground (field cell = 2 voxels)
    const byH = new Map<number, number>(), fuelH = new Map<number, number>()
    for (const c of sim.fields.cells()) {
      const [fx, fy, fz] = c
      let g = DEFAULT_SPEC.sz - 1; while (g > 0 && world.get(fx * 2, fy * 2, g) === M.Air) g--
      const h = fz - (g >> 1)
      byH.set(h, (byH.get(h) ?? 0) + c[3 + F.Flame]!)
      fuelH.set(h, (fuelH.get(h) ?? 0) + c[3 + F.Fuel]!)
    }
    const hs = [...byH.keys()].sort((a, b) => a - b).filter(h => h >= 0 && h < 16)
    console.log(t, 'area', seen.size, 'flame', hs.map(h => `${h}:${byH.get(h)}`).join(' '))
    console.log(t, 'fuel ', hs.map(h => `${h}:${fuelH.get(h)}`).join(' '))
  }
})
