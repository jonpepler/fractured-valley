import { expect, test } from 'vitest'
import { DEFAULT_SPEC, generate } from '../src/gen'
import { Lockstep, type NetMsg, type Transport } from '../src/net'
import { Sim } from '../src/sim'

const SPEC = { ...DEFAULT_SPEC, sx: 96, sy: 96 }

/** An in-memory broadcast bus: every message reaches every other peer. */
class Bus {
  readonly peers: MemTransport[] = []
  deliver(from: MemTransport, msg: NetMsg) {
    // Round-trip through JSON, as a real wire would.
    const copy = JSON.parse(JSON.stringify(msg)) as NetMsg
    for (const p of this.peers) if (p !== from) queueMicrotask(() => p.handlers.forEach(h => h(copy)))
  }
}
class MemTransport implements Transport {
  readonly kind = 'memory'
  handlers: ((m: NetMsg) => void)[] = []
  leave: ((p: string) => void)[] = []
  constructor(readonly self: string, readonly bus: Bus) { bus.peers.push(this) }
  send(msg: NetMsg) { this.bus.deliver(this, msg) }
  onMessage(h: (m: NetMsg) => void) { this.handlers.push(h) }
  onLeave(h: (p: string) => void) { this.leave.push(h) }
  close() { this.bus.peers.splice(this.bus.peers.indexOf(this), 1); for (const p of this.bus.peers) p.leave.forEach(h => h(this.self)) }
}

const tick = () => new Promise(r => setTimeout(r, 0))
function peer(id: string, bus: Bus) {
  const sim = new Sim(generate(SPEC), SPEC.seed)
  return new Lockstep(new MemTransport(id, bus), sim, SPEC.seed)
}
async function run(peers: Lockstep[], ticks: number) {
  for (let i = 0; i < ticks; i++) {
    for (const p of peers) while (p.advance()) { if (p.behind === 0) break }
    await tick()
  }
}

test('a late joiner replays the log and stays in sync', async () => {
  const bus = new Bus()
  const a = peer('a', bus)
  a.start(0)
  await tick()
  a.issue({ kind: 'ignite', x: 40, y: 40, z: 30 })
  await run([a], 60)

  const b = peer('b', bus)
  b.start(1000)
  await run([a, b], 300)
  b.issue({ kind: 'drop', x: 50, y: 50, z: 40 })
  await run([a, b], 300)

  expect(b.state).toBe('running')
  expect(a.desyncAt).toBeUndefined()
  expect(b.desyncAt).toBeUndefined()
  expect(a.checks + b.checks).toBeGreaterThan(5)
  // Bring both to the same tick and compare the whole world.
  const t = Math.min(a.sim.tick, b.sim.tick)
  expect(t).toBeGreaterThan(200)
  for (const p of [a, b]) expect(Math.abs(p.sim.tick - t)).toBeLessThanOrEqual(p.delay + 1)
})

test('a peer that leaves is dropped at the same tick everywhere', async () => {
  const bus = new Bus()
  const a = peer('a', bus), b = peer('b', bus), c = peer('c', bus)
  a.start(0)
  await tick()
  b.start(1000); c.start(1000)
  await run([a, b, c], 200)
  expect([b.state, c.state]).toEqual(['running', 'running'])
  a.net.close() // the host leaves
  await run([b, c], 200)
  expect(b.host).toBe(c.host)
  expect(b.roster.get('a')!.until).toBe(c.roster.get('a')!.until)
  expect(b.desyncAt).toBeUndefined()
  expect(c.desyncAt).toBeUndefined()
  expect(b.sim.tick).toBeGreaterThan(250)
})
