import { beforeAll, test } from 'vitest'
import { F } from '../src/fields'
import { DEFAULT_SPEC, generate } from '../src/gen'
import { M } from '../src/materials'
import { initPhysics } from '../src/physics'
import { Sim } from '../src/sim'
beforeAll(() => initPhysics())
// The big north tree with a dry wood pile against its trunk, as in the bigtree recording.
test('pile', { timeout: 900_000 }, () => {
  const world = generate(DEFAULT_SPEC)
  // A crib of 2x2 logs with 2-voxel gaps, layers alternating direction (as bigtree.cjs).
  for (let layer = 0; layer < 6; layer++) {
    const z = 59 + layer * 2
    for (let k = 0; k < 3; k++) for (let a = 0; a <= 10; a++) for (let b = 0; b < 2; b++) for (let c = 0; c < 2; c++) {
      const [x, y] = layer % 2 === 0 ? [99 + a, 36 + k * 4 + b] : [99 + k * 4 + b, 36 + Math.min(a, 8)]
      if (world.get(x, y, z + c) === M.Air) world.set(x, y, z + c, M.Wood, 0, 12)
    }
  }
  const sim = new Sim(world, DEFAULT_SPEC.seed)
  for (const x of [101, 105, 109]) sim.schedule({ tick: 5, peer: 0, seq: x, command: { kind: 'ignite', x, y: 40, z: 59 } })
  for (let t = 1; t <= 900; t++) {
    sim.step()
    if (t % 150) continue
    const fl: number[] = [], fu: number[] = [], o2: number[] = [], ht: number[] = []
    for (const c of sim.fields.cells()) {
      const [fx, fy, fz] = c
      if (fx < 46 || fx > 58 || fy < 12 || fy > 24) continue
      const h = fz - 29
      if (h < 0 || h > 20) continue
      fl[h] = (fl[h] ?? 0) + c[3 + F.Flame]!; fu[h] = (fu[h] ?? 0) + c[3 + F.Fuel]!
      ht[h] = Math.max(ht[h] ?? 0, c[3 + F.Heat]!)
    }
    let burning = 0
    for (let z = 59; z <= 80; z++) for (let y = 28; y <= 44; y++) for (let x = 96; x <= 112; x++) if (world.get(x, y, z) !== M.Air && world.getState(x, y, z) > 0) burning++
    console.log(t, 'burning', burning)
    console.log(t, 'flame', fl.map((v, i) => `${i}:${v ?? 0}`).join(' '))
    console.log(t, 'fuel ', fu.map((v, i) => `${i}:${v ?? 0}`).join(' '))
    console.log(t, 'maxheat', ht.map((v, i) => `${i}:${v ?? 0}`).join(' '))
  }
})
