import { beforeAll, expect, test } from 'vitest'
import { restoreCheckpoint, takeCheckpoint } from '../src/checkpoint'
import { DEFAULT_SPEC, generate } from '../src/gen'
import { M } from '../src/materials'
import { initPhysics } from '../src/physics'
import { Sim } from '../src/sim'

beforeAll(() => initPhysics())
const SPEC = { ...DEFAULT_SPEC, sx: 144, sy: 144 }

test('a joiner restored from a checkpoint stays identical to the peer that took it', { timeout: 120_000 }, () => {
  const live = new Sim(generate(SPEC), SPEC.seed)
  const ground = (x: number, y: number) => { let z = SPEC.sz - 1; while (z > 0 && live.world.get(x, y, z) === M.Air) z--; return z }
  const px = Math.floor(SPEC.sx * 0.52), py = Math.floor(SPEC.sy * 0.56)
  live.schedule({ tick: 1, peer: 0, seq: 0, command: { kind: 'ignite', x: px - 4, y: py - 4, z: ground(px - 4, py - 4) } })
  live.schedule({ tick: 5, peer: 0, seq: 1, command: { kind: 'drop', x: 30, y: 100, z: 60 } })
  // Already queued when the checkpoint is taken: must travel with it.
  live.schedule({ tick: 360, peer: 1, seq: 0, command: { kind: 'drop', x: px + 6, y: py + 6, z: ground(px + 6, py + 6) } })
  // Run past the topple, then on to a moment when nothing is moving.
  while (live.tick < 300 || live.bodies.length) live.step()
  const { data } = takeCheckpoint(live, SPEC)
  const { sim: joiner } = restoreCheckpoint(data)
  expect(joiner.tick).toBe(live.tick)
  expect(joiner.hash()).toBe(live.hash())
  for (let i = 0; i < 300; i++) {
    live.step(); joiner.step()
    if (i % 10 === 0) expect(joiner.hash(), `tick ${live.tick}`).toBe(live.hash())
  }
  expect(joiner.hash()).toBe(live.hash())
})
