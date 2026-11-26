// Physical sanity of the moving air: a hot spot on the ground in still air
// should make a plume, with air rising above it and drawn in at its base.
import { expect, test } from 'vitest'
import { Fields } from '../src/fields'
import { Grid } from '../src/grid'
import { M } from '../src/materials'

test('a hot spot makes a rising plume that draws air in at its base', () => {
  const g = new Grid(64, 64, 64)
  for (let y = 0; y < 64; y++) for (let x = 0; x < 64; x++) for (let z = 0; z < 4; z++) g.set(x, y, z, M.Stone)
  const f = new Fields(g, false, true)
  for (let t = 0; t < 60; t++) {
    for (let dx = -2; dx <= 2; dx++) for (let dy = -2; dy <= 2; dy++) f.emit(32 + dx, 32 + dy, 5, 400, 200, 0)
    f.step(1, t)
  }
  const vel = (k: 0 | 1 | 2, x: number, y: number, z: number) => f.vel![k]![f.index(x >> 1, y >> 1, z >> 1)]!
  // Rising above the source, at 1–3 m/s or more (100 = 0.39 field cells/tick ≈ 1.6 m/s).
  for (const z of [6, 10, 16]) expect(vel(2, 32, 32, z)).toBeGreaterThan(100)
  // Drawn inward at the base, on average around it (turbulence swirls any one
  // point, so measure the mean inward flow over a ring of 16 points 0.6 m out).
  let inward = 0
  for (const [dx, dy] of [[6, 0], [-6, 0], [0, 6], [0, -6], [4, 4], [-4, 4], [4, -4], [-4, -4],
    [6, 2], [-6, 2], [6, -2], [-6, -2], [2, 6], [-2, 6], [2, -6], [-2, -6]] as const) {
    const len = Math.hypot(dx, dy) // test-only maths, not simulation
    inward -= (vel(0, 32 + dx, 32 + dy, 6) * dx + vel(1, 32 + dx, 32 + dy, 6) * dy) / len
  }
  expect(inward / 16).toBeGreaterThan(0)
  // Heat is carried up: warmer 2 m up than 2 m to the side.
  expect(f.at(0, 32, 32, 16)).toBeGreaterThan(f.at(0, 52, 32, 6))
})
