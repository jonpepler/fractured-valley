import { describe, expect, test } from 'vitest'
import { DEFAULT_SPEC, generate } from '../src/gen'
import { M } from '../src/materials'
import { Sim } from '../src/sim'
import { Grid, hashGrids } from '../src/grid'

// A smaller world keeps the suite fast; the rules are the same.
const SPEC = { ...DEFAULT_SPEC, sx: 144, sy: 144 }
const POST = { x: Math.floor(SPEC.sx * 0.52), y: Math.floor(SPEC.sy * 0.56) }

function groundAt(sim: Sim, x: number, y: number) {
  let z = 0
  while (sim.world.get(x, y, z + 1) !== M.Air) z++
  return z
}

function burnThePost() {
  const sim = new Sim(generate(SPEC), SPEC.seed)
  const gx = POST.x - 4, gy = POST.y - 4
  sim.schedule({ tick: 1, peer: 0, seq: 0, command: { kind: 'ignite', x: gx, y: gy, z: groundAt(sim, gx, gy) } })
  sim.schedule({ tick: 5, peer: 1, seq: 0, command: { kind: 'drop', x: 30, y: 100, z: 60 } })
  for (let i = 0; i < 400; i++) sim.step()
  return sim
}

describe('determinism', () => {
  test('the same seed generates the same world', () => {
    expect(hashGrids([generate(SPEC)])).toBe(hashGrids([generate(SPEC)]))
  })

  test('the same commands give the same world, tick for tick', () => {
    const a = burnThePost()
    const b = burnThePost()
    expect(a.hash()).toBe(b.hash())
    expect(a.events).toEqual(b.events)
  })

  test('the hash describes content, not how compactly it is stored', () => {
    const g = new Grid(32, 32, 32)
    g.set(3, 3, 3, M.Stone)
    const before = hashGrids([g])
    g.set(3, 3, 3, M.Air)
    g.compact()
    const empty = new Grid(32, 32, 32)
    expect(hashGrids([g])).toBe(hashGrids([empty]))
    expect(before).not.toBe(hashGrids([empty]))
  })

  test('commands for a tick already simulated are refused', () => {
    const sim = new Sim(generate(SPEC), SPEC.seed)
    sim.step()
    expect(() => sim.schedule({ tick: 0, peer: 0, seq: 0, command: { kind: 'drop', x: 1, y: 1, z: 1 } })).toThrow()
  })
})

describe('the burning post', () => {
  test('the post gives way under the metal block, which falls and lands', () => {
    const sim = burnThePost()
    const kinds = sim.events.map(e => e.kind)
    expect(kinds).toContain('gave-way')
    expect(kinds).toContain('detached')
    expect(kinds).toContain('landed')
    // The block ends lower than it started, still in one piece.
    let metal = 0, top = 0
    for (let z = 0; z < SPEC.sz; z++) for (let y = POST.y - 6; y < POST.y + 8; y++) for (let x = POST.x - 6; x < POST.x + 8; x++)
      if (sim.world.get(x, y, z) === M.Metal) { metal++; top = Math.max(top, z) }
    expect(metal).toBe(216)
    const ground = groundAt(sim, POST.x - 5, POST.y - 5)
    expect(top).toBeLessThan(ground + 16 + 6)
  })
})

describe('storage', () => {
  test('uniform bricks cost no cell arrays', () => {
    const g = generate(SPEC)
    const s = g.stats()
    expect(s.detailed).toBeLessThan(s.bricks / 2)
  })
})
