// Lockstep over a peer transport.
//
// Every peer runs the same deterministic simulation. Ticks advance only when
// an input frame (possibly empty) has arrived from every peer for that tick,
// so all peers apply identical commands at identical ticks. Local input is
// sent `delay` ticks ahead to hide latency.
//
// The world is seed + frames: a late joiner receives the frame log from the
// host and replays it to catch up, with no world snapshot sent at all.
// Peers exchange state hashes every HASH_EVERY ticks to detect desyncs.

import type { Command, Sim } from './sim'

export interface Frame {
  readonly peer: string
  readonly tick: number
  readonly cmds: readonly Command[]
}

export type NetMsg =
  | { readonly t: 'hello'; readonly from: string; readonly seed: number }
  | { readonly t: 'welcome'; readonly from: string; readonly to: string; readonly seed: number; readonly roster: readonly [string, number][]; readonly frames: readonly Frame[] }
  | { readonly t: 'join'; readonly from: string; readonly peer: string; readonly fromTick: number }
  | { readonly t: 'frame'; readonly from: string; readonly frame: Frame }
  | { readonly t: 'hash'; readonly from: string; readonly tick: number; readonly hash: number }
  | { readonly t: 'bye'; readonly from: string }
  | { readonly t: 'leaving'; readonly from: string; readonly peer: string; readonly frames: readonly Frame[] }
  | { readonly t: 'drop'; readonly from: string; readonly peer: string; readonly after: number; readonly frames: readonly Frame[] }

export interface Transport {
  readonly self: string
  readonly kind: string
  send(msg: NetMsg): void
  onMessage(handler: (msg: NetMsg) => void): void
  /** A peer vanished without saying bye (closed tab, lost connection). */
  onLeave(handler: (peer: string) => void): void
  close(): void
}

const newId = () => Math.random().toString(36).slice(2, 10) // identity only, never in the sim

/** Peers in tabs of the same browser: instant, for local testing. */
export class BroadcastTransport implements Transport {
  readonly self = newId()
  readonly kind = 'broadcast'
  private readonly channel: BroadcastChannel
  private leaveHandlers: ((peer: string) => void)[] = []

  constructor(room: string) {
    this.channel = new BroadcastChannel(`fv-${room}`)
    addEventListener('pagehide', () => this.close())
  }
  send(msg: NetMsg) { this.channel.postMessage(msg) }
  onMessage(handler: (msg: NetMsg) => void) {
    this.channel.addEventListener('message', ev => {
      const msg = ev.data as NetMsg
      if (msg.t === 'bye') this.leaveHandlers.forEach(h => h(msg.from))
      handler(msg)
    })
  }
  onLeave(handler: (peer: string) => void) { this.leaveHandlers.push(handler) }
  close() { this.send({ t: 'bye', from: this.self }); this.channel.close() }
}

/** Real peer-to-peer over WebRTC, signalled through public Nostr relays. */
export class TrysteroTransport implements Transport {
  readonly self = newId()
  readonly kind = 'trystero'
  private room: Awaited<ReturnType<typeof import('trystero').joinRoom>> | undefined
  private queue: NetMsg[] = []
  private handlers: ((msg: NetMsg) => void)[] = []
  private leaveHandlers: ((peer: string) => void)[] = []
  /** trystero peer id → our peer id, learnt from their first message. */
  private readonly ids = new Map<string, string>()
  private sendRaw: ((data: string) => Promise<void>) | undefined

  constructor(room: string) {
    void import('trystero').then(({ joinRoom }) => {
      const r = joinRoom({ appId: 'fractured-valley/voxel-sandbox' }, room)
      this.room = r
      const action = r.makeAction<string>('m')
      this.sendRaw = data => action.send(data)
      action.onMessage((data, ctx) => {
        const msg = JSON.parse(data) as NetMsg
        this.ids.set(ctx.peerId, msg.from)
        this.handlers.forEach(h => h(msg))
      })
      r.onPeerLeave(ctx => {
        const id = this.ids.get(ctx.peerId)
        if (id) this.leaveHandlers.forEach(h => h(id))
      })
      // Say hello again whenever someone new connects, so they learn of us.
      r.onPeerJoin(() => { for (const m of this.queue.splice(0)) void this.sendRaw?.(JSON.stringify(m)) })
    })
  }
  send(msg: NetMsg) {
    if (msg.t === 'hello') this.queue.push(msg) // re-sent on each peer join
    void this.sendRaw?.(JSON.stringify(msg))
  }
  onMessage(handler: (msg: NetMsg) => void) { this.handlers.push(handler) }
  onLeave(handler: (peer: string) => void) { this.leaveHandlers.push(handler) }
  close() { void this.room?.leave() }
}

/** No network: a solo world through the same code path. */
export class SoloTransport implements Transport {
  readonly self = newId()
  readonly kind = 'solo'
  send() {}
  onMessage() {}
  onLeave() {}
  close() {}
}

const HASH_EVERY = 20
const key = (peer: string, tick: number) => `${peer}:${tick}`

export class Lockstep {
  state: 'connecting' | 'catching-up' | 'running' = 'connecting'
  /** Who takes part, from which tick, until which tick (inclusive). */
  readonly roster = new Map<string, { from: number; until: number }>()
  private readonly frames = new Map<string, Frame>()
  private pending: Command[] = []
  private readonly ownHashes = new Map<number, number>()
  private readonly theirHashes = new Map<number, Map<string, number>>()
  waitingFor: string | undefined
  checks = 0
  desyncAt: number | undefined
  /** Highest tick any frame has arrived for: how far ahead peers are. */
  newestFrame = 0
  /** Leave barrier: for each departing peer, the frames of theirs each remaining peer holds. */
  private readonly leaving = new Map<string, Map<string, readonly Frame[]>>()

  constructor(readonly net: Transport, readonly sim: Sim, readonly seed: number, readonly delay = 3) {
    net.onMessage(m => this.receive(m))
    net.onLeave(p => this.peerLeft(p))
  }

  get self() { return this.net.self }

  /** The host is the longest-standing peer: it admits joiners and settles leaves. */
  get host(): string | undefined {
    let best: [string, number] | undefined
    for (const [p, r] of this.roster) {
      if (r.until !== Infinity || this.leaving.has(p)) continue
      if (!best || r.from < best[1] || (r.from === best[1] && p < best[0])) best = [p, r.from]
    }
    return best?.[0]
  }

  start(waitMs: number) {
    this.net.send({ t: 'hello', from: this.self, seed: this.seed })
    setTimeout(() => {
      if (this.state !== 'connecting') return
      // Nobody answered: found a new world.
      this.roster.set(this.self, { from: 0, until: Infinity })
      this.becomeActive(0)
    }, waitMs)
  }

  issue(command: Command) {
    this.pending.push(command)
  }

  /** Step the simulation once if every peer's input for this tick is here. */
  advance(): boolean {
    if (this.state === 'connecting') return false
    const tick = this.sim.tick
    const active = [...this.roster].filter(([, r]) => r.from <= tick && tick <= r.until).map(([p]) => p).sort()
    for (const p of active) {
      if (!this.frames.has(key(p, tick))) { this.waitingFor = p; return false }
    }
    this.waitingFor = undefined
    active.forEach((p, peerIndex) => {
      this.frames.get(key(p, tick))!.cmds.forEach((command, seq) => this.sim.schedule({ tick, peer: peerIndex, seq, command }))
    })
    this.sim.step()
    const mine = this.roster.get(this.self)
    if (mine && tick >= mine.from) {
      this.sendFrame(tick + this.delay)
      if (this.state === 'catching-up' && tick + 1 >= mine.from) this.state = 'running'
    }
    if (this.sim.tick % HASH_EVERY === 0) {
      const h = this.sim.hash()
      this.ownHashes.set(this.sim.tick, h)
      this.net.send({ t: 'hash', from: this.self, tick: this.sim.tick, hash: h })
      this.compare(this.sim.tick)
    }
    return true
  }

  /** Ticks we could step right now without waiting (to catch up quickly). */
  get behind(): number {
    return Math.max(0, this.newestFrame - this.delay - this.sim.tick)
  }

  private becomeActive(from: number) {
    for (let t = from; t < from + this.delay; t++) this.sendFrame(t)
    this.state = this.sim.tick >= from ? 'running' : 'catching-up'
  }

  private sendFrame(tick: number) {
    const frame: Frame = { peer: this.self, tick, cmds: this.pending.splice(0) }
    this.frames.set(key(this.self, tick), frame)
    this.net.send({ t: 'frame', from: this.self, frame })
  }

  private receive(m: NetMsg) {
    switch (m.t) {
      case 'hello': {
        if (this.host !== this.self || this.state !== 'running') return
        if (m.seed !== this.seed) { console.warn(`peer ${m.from} has seed ${m.seed}, not ${this.seed}`); return }
        // Far enough ahead that no peer has already passed it.
        const fromTick = this.sim.tick + this.delay * 2 + 6
        this.roster.set(m.from, { from: fromTick, until: Infinity })
        this.net.send({ t: 'join', from: this.self, peer: m.from, fromTick })
        this.net.send({
          t: 'welcome', from: this.self, to: m.from, seed: this.seed,
          roster: [...this.roster].map(([p, r]) => [p, r.from]),
          frames: [...this.frames.values()],
        })
        return
      }
      case 'welcome': {
        if (m.to !== this.self || this.state !== 'connecting') return
        for (const [p, from] of m.roster) this.roster.set(p, { from, until: Infinity })
        for (const f of m.frames) this.addFrame(f)
        this.becomeActive(this.roster.get(this.self)!.from)
        return
      }
      case 'join':
        if (!this.roster.has(m.peer)) this.roster.set(m.peer, { from: m.fromTick, until: Infinity })
        return
      case 'frame':
        this.addFrame(m.frame)
        return
      case 'hash': {
        let at = this.theirHashes.get(m.tick)
        if (!at) this.theirHashes.set(m.tick, (at = new Map()))
        at.set(m.from, m.hash)
        this.compare(m.tick)
        return
      }
      case 'leaving': {
        this.peerLeft(m.peer) // we may not have noticed yet
        this.leaving.get(m.peer)?.set(m.from, m.frames)
        this.decideLeave(m.peer)
        return
      }
      case 'drop': {
        for (const f of m.frames) this.addFrame(f)
        const r = this.roster.get(m.peer)
        if (r) r.until = m.after
        this.leaving.delete(m.peer)
        return
      }
      case 'bye':
        return // handled through onLeave
    }
  }

  private addFrame(f: Frame) {
    this.frames.set(key(f.peer, f.tick), f)
    this.newestFrame = Math.max(this.newestFrame, f.tick)
  }

  /**
   * A peer left. Leave barrier: every remaining peer reports the leaver's
   * frames it holds. The host waits for all reports (or a timeout), then
   * drops the leaver after the newest frame *anyone* holds and relays those
   * frames. A peer that already stepped a tick with the leaver's input has
   * reported that input, so nobody has to undo anything. Meanwhile every
   * peer simply stalls at the first tick it lacks the leaver's frame for.
   */
  private peerLeft(peer: string) {
    const r = this.roster.get(peer)
    if (!r || r.until !== Infinity || this.leaving.has(peer) || peer === this.self) return
    this.leaving.set(peer, new Map())
    const frames = [...this.frames.values()].filter(f => f.peer === peer && f.tick >= this.sim.tick - 200) // peers never drift further apart than the input delay
    this.leaving.get(peer)!.set(this.self, frames)
    this.net.send({ t: 'leaving', from: this.self, peer, frames })
    this.decideLeave(peer)
    setTimeout(() => this.decideLeave(peer, true), 2000)
  }

  private decideLeave(peer: string, timedOut = false) {
    const reports = this.leaving.get(peer)
    if (!reports || this.host !== this.self) return
    const needed = [...this.roster].filter(([p, r]) => r.until === Infinity && !this.leaving.has(p)).map(([p]) => p)
    if (!timedOut && !needed.every(p => reports.has(p))) return
    const frames = new Map<number, Frame>()
    for (const fs of reports.values()) for (const f of fs) frames.set(f.tick, f)
    for (const f of this.frames.values()) if (f.peer === peer) frames.set(f.tick, f)
    const r = this.roster.get(peer)!
    let after = r.from - 1
    for (const t of frames.keys()) after = Math.max(after, t)
    const msg = { t: 'drop', from: this.self, peer, after, frames: [...frames.values()] } as const
    this.net.send(msg)
    this.receive(msg)
  }

  private compare(tick: number) {
    const own = this.ownHashes.get(tick)
    const theirs = this.theirHashes.get(tick)
    if (own === undefined || !theirs) return
    for (const h of theirs.values()) {
      if (h !== own) this.desyncAt ??= tick
      else this.checks++
    }
    this.theirHashes.delete(tick)
  }
}
