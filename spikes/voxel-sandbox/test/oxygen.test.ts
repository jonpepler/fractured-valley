// The fire triangle: take away the oxygen and the fire starves. The same dry
// wood burns in a small stone box with and without a lid. The closed fire
// uses up its air and its flame dies away; the open one keeps burning.
import { beforeAll, expect, test } from 'vitest'
import { F } from '../src/fields'
import { Grid } from '../src/grid'
import { M } from '../src/materials'
import { initPhysics } from '../src/physics'
import { Sim } from '../src/sim'

beforeAll(() => initPhysics())

function boxFire(lid: boolean) {
  const g = new Grid(32, 32, 32)
  for (let y = 0; y < 32; y++) for (let x = 0; x < 32; x++) for (let z = 0; z < 4; z++) g.set(x, y, z, M.Stone)
  // Stone walls 2 cells thick around an 8×8 interior (x, y 12..19), 10 high; optional lid.
  for (let z = 4; z < 16; z++) for (let y = 10; y < 22; y++) for (let x = 10; x < 22; x++) {
    const wall = x < 12 || x >= 20 || y < 12 || y >= 20
    if (wall || (lid && z >= 14)) g.set(x, y, z, M.Stone)
  }
  // Dry wood (12% moisture) on the floor inside.
  for (let z = 4; z < 6; z++) for (let y = 12; y < 20; y++) for (let x = 12; x < 20; x++) g.set(x, y, z, M.Wood, 0, 12)
  const sim = new Sim(g, 7)
  sim.schedule({ tick: 1, peer: 0, seq: 0, command: { kind: 'ignite', x: 16, y: 16, z: 5 } })
  const flame: number[] = []
  for (let i = 0; i < 600; i++) {
    sim.step()
    let f = 0
    for (const c of sim.fields.cells()) f += c[3 + F.Flame]!
    flame.push(f)
  }
  const sum = (a: number[]) => a.reduce((s, x) => s + x, 0)
  return { early: sum(flame.slice(20, 220)), late: sum(flame.slice(400)) }
}

test('a fire in a closed box starves; the same fire in an open box burns on', { timeout: 240_000 }, () => {
  const open = boxFire(false), closed = boxFire(true)
  console.log({ open, closed })
  expect(open.early).toBeGreaterThan(0)
  expect(closed.early).toBeGreaterThan(0) // it did catch
  // In a box this small the air runs out within the first seconds: the sealed
  // fire burns at a small fraction of the open one, early and late.
  expect(closed.early).toBeLessThan(open.early / 10)
  expect(closed.late).toBeLessThan(open.late / 10)
})
