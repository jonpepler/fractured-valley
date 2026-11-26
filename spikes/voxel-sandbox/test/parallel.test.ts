// Keeps two capabilities alive (decision 2.7b): the same tick computed on
// several threads, or split between peers, is bit-identical to one thread.
import { afterAll, beforeAll, describe, expect, test } from 'vitest'
import { FieldPool } from '../src/field-pool'
import { computeBricks, type FieldView } from '../src/fields'
import { DEFAULT_SPEC, generate } from '../src/gen'
import { M } from '../src/materials'
import { initPhysics } from '../src/physics'
import { hash32 } from '../src/rng'
import { Sim } from '../src/sim'

beforeAll(() => initPhysics())
const SPEC = { ...DEFAULT_SPEC, sx: 96, sy: 96 }
const TICKS = 240
const CELLS = 8 * 8 * 8

function lit(sim: Sim) {
  for (const [x, y] of [[40, 50], [60, 30]] as const) {
    let z = SPEC.sz - 1; while (z > 0 && sim.world.get(x, y, z) === M.Air) z--
    sim.schedule({ tick: 1, peer: 0, seq: x, command: { kind: 'ignite', x, y, z } })
  }
  return sim
}

function soloHashes(): number[] {
  const sim = lit(new Sim(generate(SPEC), SPEC.seed, { airFlow: false }))
  const out: number[] = []
  for (let i = 0; i < TICKS; i++) { sim.step(); if (sim.tick % 20 === 0) out.push(sim.hash()) }
  return out
}
let solo: number[]
beforeAll(() => { solo = soloHashes() })

describe('threads', () => {
  const pools: FieldPool[] = []
  afterAll(() => Promise.all(pools.map(p => p.close())))

  for (const threads of [3, 4]) {
    test(`gas stepped on ${threads} worker threads matches one thread, tick for tick`, { timeout: 120_000 }, () => {
      const world = generate(SPEC)
      const sim = lit(new Sim(world, SPEC.seed, { sharedFields: true, airFlow: false }))
      const pool = new FieldPool(sim.fields, threads, world.bricks.length)
      pools.push(pool)
      sim.fieldStep = (s, t) => pool.step(s, t)
      const out: number[] = []
      for (let i = 0; i < TICKS; i++) { sim.step(); if (sim.tick % 20 === 0) out.push(sim.hash()) }
      expect(out).toEqual(solo)
      expect(sim.fields.active.size).toBeGreaterThan(0) // the fire really made gas
    })
  }
})

// ── peers: each computes the bricks it owns, sends them, checks a sample ──

/** One brick's gas values from `next`, as they would travel on the network. */
function pack(v: FieldView, b: number): Uint16Array {
  const out = new Uint16Array(3 * CELLS)
  each(v, b, (i, k) => { out[k * 3] = v.next[0]![i]!; out[k * 3 + 1] = v.next[1]![i]!; out[k * 3 + 2] = v.next[2]![i]! })
  return out
}
function unpack(v: FieldView, b: number, data: Uint16Array) {
  each(v, b, (i, k) => { v.next[0]![i] = data[k * 3]!; v.next[1]![i] = data[k * 3 + 1]!; v.next[2]![i] = data[k * 3 + 2]! })
}
function each(v: FieldView, b: number, f: (i: number, k: number) => void) {
  const bx = b % v.bx, by = Math.floor(b / v.bx) % v.by, bz = Math.floor(b / (v.bx * v.by))
  let k = 0
  for (let lz = 0; lz < 8; lz++) for (let ly = 0; ly < 8; ly++) for (let lx = 0; lx < 8; lx++, k++) {
    const fx = bx * 8 + lx, fy = by * 8 + ly, fz = bz * 8 + lz
    if (fx < v.nx && fy < v.ny && fz < v.nz) f((fz * v.ny + fy) * v.nx + fx, k)
  }
}

/**
 * Two peers share the gas step: peer p owns brick b when b % 2 === p. Each
 * computes its own, sends them, and spot-checks a seeded sample of the
 * other's by recomputing it. `corruptAt` makes peer 0 send one wrong value.
 */
function runPeers(corruptAt = -1) {
  const peers = [lit(new Sim(generate(SPEC), SPEC.seed, { airFlow: false })), lit(new Sim(generate(SPEC), SPEC.seed, { airFlow: false }))]
  const mismatches: number[] = []
  let sent = 0
  // Both peers step together; the field step of each is the shared exchange.
  const exchange = (seed: number, tick: number) => {
    const plans = peers.map(p => p.fields.plan())
    expect(plans[1]).toEqual(plans[0]) // same state, so the same plan
    const msgs = peers.map((p, me) => {
      const mine = plans[me]!.filter(b => b % 2 === me)
      const withGas = computeBricks(p.fields.view(), mine, seed, tick)
      const data = mine.map(b => pack(p.fields.view(), b))
      if (me === 0 && tick === corruptAt) {
        // Corrupt one value in a brick the other peer will spot-check. (A brick
        // outside the 1-in-4 sample slips past this check; the regular
        // whole-world hash comparison then catches the desync instead.)
        const j = mine.findIndex(b => hash32(seed, tick, b, 0xc4ec) % 4 === 0)
        if (j >= 0) { const d = data[j]!; d[5] = d[5]! ^ 1 }
      }
      sent += data.length
      return { mine, withGas, data }
    })
    for (const [me, p] of peers.entries()) {
      const theirs = msgs[1 - me]!
      theirs.mine.forEach((b, j) => unpack(p.fields.view(), b, theirs.data[j]!))
      // Spot-check: recompute a seeded sample (1 in 4) of the other's bricks.
      // Recomputing reads only `cur`, which both peers hold identically.
      const check = theirs.mine.filter(b => hash32(seed, tick, b, 0xc4ec) % 4 === 0)
      const scratch = { ...p.fields.view(), next: [0, 1, 2].map(() => new Uint16Array(p.fields.cur[0]!.length)) }
      computeBricks(scratch, check, seed, tick)
      for (const b of check) {
        const got = theirs.data[theirs.mine.indexOf(b)]!, want = pack(scratch, b)
        if (got.some((x, i) => x !== want[i])) mismatches.push(tick)
      }
    }
    for (const [me, p] of peers.entries()) {
      const all = [...msgs[me]!.withGas, ...msgs[1 - me]!.withGas]
      p.fields.finish(all.sort((a, b) => a - b))
    }
  }
  // The exchange runs once both peers have reached the gas step of the same
  // tick: peer 0 marks itself ready, and peer 1's gas step runs it for both.
  // (Nothing after the gas step reads the gas until the next tick.)
  let ready = -1
  peers[0]!.fieldStep = (_s, t) => { ready = t }
  peers[1]!.fieldStep = (s, t) => { if (ready !== t) throw new Error('peers out of step'); exchange(s, t) }
  const hashes: number[][] = [[], []]
  for (let i = 0; i < TICKS; i++) {
    peers[0]!.step(); peers[1]!.step()
    if (peers[0]!.tick % 20 === 0) peers.forEach((p, k) => hashes[k]!.push(p.hash()))
  }
  return { hashes, mismatches, sent }
}

describe('peers', () => {
  test('two peers splitting the gas step both match a solo world', { timeout: 120_000 }, () => {
    const { hashes, mismatches, sent } = runPeers()
    expect(hashes[0]).toEqual(solo)
    expect(hashes[1]).toEqual(solo)
    expect(mismatches).toEqual([])
    expect(sent).toBeGreaterThan(100) // they really did share work
  })

  test('a peer that sends a wrong brick is caught by the spot-check', { timeout: 120_000 }, () => {
    const { mismatches } = runPeers(41)
    expect(mismatches.length).toBeGreaterThan(0)
    expect(mismatches[0]).toBe(41)
  })
})
