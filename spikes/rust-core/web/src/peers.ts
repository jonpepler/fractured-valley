// Two peers splitting each tick's work, in two real pages.
//
//   peers.html?role=host&room=r&joinAt=10&ticks=40[&corrupt=30][&net=trystero]
//   peers.html?role=join&room=r
//
// The host starts the world alone and runs it to tick `joinAt` (or on until
// nothing is falling, since rigid bodies are not in checkpoints yet), then waits.
// The joiner says hello; the host sends a checkpoint (the whole state as
// bytes) and the joiner carries on from it. From then on, every tick:
//
//   1. both run the tick up to the diffusion phase (`begin_tick`);
//   2. each computes its own half of the bricks (host: even, joiner: odd) and
//      sends the result as bytes;
//   3. each re-computes a seeded quarter of the other's bricks to check them
//      (a wrong brick is replaced by its own result and reported);
//   4. both finish the tick from all the bricks and swap state hashes.
//
// With `corrupt=T` the host sends one wrong value at tick T (in a brick the
// joiner will check). The page publishes what happened on `window.result`.

import { BroadcastTransport, type Msg, type Transport, TrysteroTransport } from './net'
import { type Engine, load, scenario } from './fv'

declare global { interface Window { result?: unknown; error?: string; fvStatus?: string } }

type M =
  | { t: 'hello'; from: string }
  | { t: 'welcome'; from: string; to: string; tick: number; hash: string; bytes: Uint8Array }
  | { t: 'part'; from: string; tick: number; bytes: Uint8Array }
  | { t: 'hash'; from: string; tick: number; hash: string }
  | { t: 'bye'; from: string }

const q = new URLSearchParams(location.search)
const role = q.get('role') === 'join' ? 'join' : 'host'
const room = q.get('room') ?? 'fv-wasm'
const joinAt = Number(q.get('joinAt') ?? 10)
const ticks = Number(q.get('ticks') ?? 40)
const corrupt = q.has('corrupt') ? Number(q.get('corrupt')) : undefined
const out = document.getElementById('out')!
const say = (s: string) => { window.fvStatus = s; out.textContent = s }

const net: Transport<M & Msg> = q.get('net') === 'trystero' ? new TrysteroTransport(room) : new BroadcastTransport(room)

// A mailbox: messages arrive whenever, the protocol waits for specific ones.
const inbox: M[] = []
let wake: (() => void) | undefined
net.onMessage(m => { inbox.push(m); wake?.() })
async function receive<T extends M>(want: (m: M) => m is T, timeoutMs = 30_000): Promise<T> {
  const deadline = performance.now() + timeoutMs
  for (;;) {
    const i = inbox.findIndex(want)
    if (i >= 0) return inbox.splice(i, 1)[0] as T
    if (performance.now() > deadline) throw new Error(`timed out waiting (${role})`)
    await new Promise<void>(r => { wake = r; setTimeout(r, 100) })
  }
}

async function main() {
  const fv = await load('st')
  let e: Engine
  let other: string
  const hashes: { tick: number; hash: string }[] = []
  let joinHashMatched: boolean | undefined
  const t0 = performance.now()

  if (role === 'host') {
    e = scenario(fv)
    // Run alone to `joinAt`, and on until nothing is falling (rigid bodies
    // are not in checkpoints yet).
    while (e.tick() < joinAt || !e.can_checkpoint()) { e.step(); hashes.push({ tick: e.tick() - 1, hash: e.hash() }) }
    say(`waiting for a peer at tick ${e.tick()}`)
    const hello = await receive((m): m is Extract<M, { t: 'hello' }> => m.t === 'hello', 60_000)
    other = hello.from
    const bytes = e.export()
    net.send({ t: 'welcome', from: net.self, to: other, tick: e.tick(), hash: e.hash(), bytes })
  } else {
    // Keep saying hello until the host answers (it may not be listening yet).
    say('looking for a host')
    const hi = setInterval(() => net.send({ t: 'hello', from: net.self }), 200)
    net.send({ t: 'hello', from: net.self })
    const w = await receive((m): m is Extract<M, { t: 'welcome' }> => m.t === 'welcome' && m.to === net.self, 60_000)
    clearInterval(hi)
    other = w.from
    e = fv.Engine.import(w.bytes)
    joinHashMatched = e.hash() === w.hash && e.tick() === w.tick
  }

  const me = role === 'host' ? 0 : 1
  const firstSplitTick = e.tick()
  const n = e.brick_count()
  const mine = Uint32Array.from({ length: n }, (_, b) => b).filter(b => b % 2 === me)
  const caught: { tick: number; bricks: number[] }[] = []
  const peerHashMismatches: number[] = []
  let checked = 0, bytesSent = 0, bricksSent = 0
  const tSplit = performance.now()

  while (e.tick() < ticks) {
    const tick = e.tick()
    say(`split tick ${tick}`)
    e.begin_tick()
    const ours = e.diffuse_part(mine)
    const send = role === 'host' && tick === corrupt ? e.corrupt_for_test(ours) : ours
    net.send({ t: 'part', from: net.self, tick, bytes: send })
    bytesSent += send.length
    bricksSent += mine.length
    e.add_part(ours)
    const theirs = await receive((m): m is Extract<M, { t: 'part' }> => m.t === 'part' && m.from === other && m.tick === tick)
    checked += e.checked_in(theirs.bytes)
    const bad = e.check_and_add_part(theirs.bytes)
    if (bad.length) caught.push({ tick, bricks: [...bad] })
    e.finish_tick()
    const hash = e.hash()
    hashes.push({ tick, hash })
    net.send({ t: 'hash', from: net.self, tick, hash })
    const h = await receive((m): m is Extract<M, { t: 'hash' }> => m.t === 'hash' && m.from === other && m.tick === tick)
    if (h.hash !== hash) peerHashMismatches.push(tick)
  }

  const splitTicks = ticks - firstSplitTick
  window.result = {
    role, firstSplitTick, hashes, caught, checked, bricksSent, bytesSent, peerHashMismatches, joinHashMatched,
    energyGap: e.energy_gap(), massGap: e.mass_gap(),
    msPerSplitTick: (performance.now() - tSplit) / Math.max(1, splitTicks), totalMs: performance.now() - t0,
    transport: net.kind,
  }
  say(`done: ${splitTicks} split ticks, final hash ${hashes.at(-1)?.hash}, caught ${JSON.stringify(caught)}`)
}

main().catch(err => { window.error = String(err); say(`error: ${err}`) })
