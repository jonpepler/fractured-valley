import { beforeAll, describe, expect, test } from 'vitest'
import { DEFAULT_SPEC, generate } from '../src/gen'
import { M } from '../src/materials'
import { Sim } from '../src/sim'
import { initPhysics } from '../src/physics'

beforeAll(() => initPhysics())
import { Grid, hashGrids } from '../src/grid'

// A smaller world keeps the suite fast; the rules are the same.
const SPEC = { ...DEFAULT_SPEC, sx: 96, sy: 96 }
const POST = { x: Math.floor(SPEC.sx * 0.52), y: Math.floor(SPEC.sy * 0.56) }

function groundAt(sim: Sim, x: number, y: number) {
  let z = 0
  while (sim.world.get(x, y, z + 1) !== M.Air) z++
  return z
}

// The post tests are about structure, so they run without moving air (fast);
// moving air has its own determinism and checkpoint tests.
function burnThePost() {
  const sim = new Sim(generate(SPEC), SPEC.seed, { airFlow: false })
  const gx = POST.x - 4, gy = POST.y - 4
  sim.schedule({ tick: 1, peer: 0, seq: 0, command: { kind: 'ignite', x: gx, y: gy, z: groundAt(sim, gx, gy) } })
  sim.schedule({ tick: 5, peer: 1, seq: 0, command: { kind: 'drop', x: 30, y: 70, z: 60 } })
  // The dry post (same wood as the trees, 12% moisture) gives way ~30 s in.
  // Then run on until every falling piece has come to rest: the fire also
  // reaches trees further off, whose branches fall later (up to 70 s).
  for (let i = 0; i < 900; i++) sim.step()
  while (sim.bodies.length > 0 && sim.tick < 1400) sim.step()
  return sim
}
let burnt: Sim | undefined
const burnTheTestPost = () => (burnt ??= burnThePost())

describe('determinism', () => {
  test('the same seed generates the same world', () => {
    expect(hashGrids([generate(SPEC)])).toBe(hashGrids([generate(SPEC)]))
  })

  test('the same commands give the same world, tick for tick', () => {
    const a = burnTheTestPost()
    const b = burnThePost()
    expect(a.hash()).toBe(b.hash())
    expect(a.events).toEqual(b.events)
  }, 240_000)

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

  test('cached brick hashes never go stale', { timeout: 60_000 }, () => {
    const sim = new Sim(generate(SPEC), SPEC.seed)
    const gx = POST.x - 4, gy = POST.y - 4
    sim.schedule({ tick: 1, peer: 0, seq: 0, command: { kind: 'ignite', x: gx, y: gy, z: groundAt(sim, gx, gy) } })
    for (let i = 0; i < 300; i++) {
      sim.step()
      if (i % 20 === 0) sim.hash() // warm the cache mid-fire, as the network check does
    }
    const cached = sim.hash()
    for (const g of sim.grids) g.hashedAt.fill(0)
    expect(sim.hash()).toBe(cached)
  })

  test('commands for a tick already simulated are refused', () => {
    const sim = new Sim(generate(SPEC), SPEC.seed)
    sim.step()
    expect(() => sim.schedule({ tick: 0, peer: 0, seq: 0, command: { kind: 'drop', x: 1, y: 1, z: 1 } })).toThrow()
  })
})

describe('the burning post', () => {
  test('the post gives way under the metal block, which topples and lands', () => {
    const sim = burnTheTestPost()
    // Totals, not the event log: the log keeps only the latest 500, and a
    // tree the fire reaches can fill it with crumbs.
    const n = sim.eventCounts
    expect(n['gave-way']).toBeGreaterThan(0)
    expect(n.detached).toBeGreaterThan(0)
    expect(n.landed).toBeGreaterThan(0)
    expect(sim.bodies).toHaveLength(0) // everything has come to rest
    // The block is still in one piece, and lower than it started.
    let metal = 0, top = 0
    for (let z = 0; z < SPEC.sz; z++) for (let y = 0; y < SPEC.sy; y++) for (let x = 0; x < SPEC.sx; x++)
      if (sim.world.get(x, y, z) === M.Metal) { metal++; top = Math.max(top, z) }
    expect(metal).toBe(216 + 216) // this block and the one peer 1 dropped
    const ground = groundAt(sim, POST.x - 5, POST.y - 5)
    expect(top).toBeLessThan(ground + 16 + 6)
  }, 240_000)
})

describe('storage', () => {
  test('uniform bricks cost no cell arrays', () => {
    const g = generate(SPEC)
    const s = g.stats()
    // Only the surface needs cell arrays; in this small test world that's ~half the bricks.
    expect(s.detailed).toBeLessThan(s.bricks * 0.6)
  })
})

describe('impacts', () => {
  function throwAtTree(speed: number) {
    const spec = DEFAULT_SPEC
    const sim = new Sim(generate(spec), spec.seed)
    const tx = Math.floor(spec.sx * 0.86), ty = Math.floor(spec.sy * 0.42)
    sim.schedule({ tick: 2, peer: 0, seq: 0, command: { kind: 'launch', x: tx + 22, y: ty, z: groundAt(sim, tx + 22, ty) + 6, vx: -speed, vy: 0, vz: 10 } })
    for (let i = 0; i < 300; i++) sim.step()
    return sim.events
  }

  test('a fast metal cube cuts through a trunk and the tree falls', () => {
    const ev = throwAtTree(120)
    expect(ev.some(e => e.kind === 'smashed')).toBe(true)
    const fell = ev.find(e => e.kind === 'detached' && e.cells > 1000)
    expect(fell).toBeDefined()
    expect(ev.some(e => e.kind === 'landed' && e.cells === fell!.cells)).toBe(true)
  })

  test('a slow one only dents it', () => {
    const ev = throwAtTree(60)
    expect(ev.some(e => e.kind === 'smashed')).toBe(true)
    expect(ev.some(e => e.kind === 'detached' && e.cells > 1000)).toBe(false)
  })
})

describe('moving air', () => {
  test('the same fire with moving air gives the same world, tick for tick', { timeout: 120_000 }, () => {
    const small = { ...DEFAULT_SPEC, sx: 96, sy: 96 }
    const run = () => {
      const sim = new Sim(generate(small), small.seed)
      sim.schedule({ tick: 1, peer: 0, seq: 0, command: { kind: 'ignite', x: 40, y: 50, z: groundAt(sim, 40, 50) } })
      const out: number[] = []
      for (let i = 0; i < 240; i++) { sim.step(); if (sim.tick % 20 === 0) out.push(sim.hash()) }
      return { out, moving: sim.fields.moving.size }
    }
    const a = run(), b = run()
    expect(a.out).toEqual(b.out)
    expect(a.moving).toBeGreaterThan(0) // the air really moved
  })
})
