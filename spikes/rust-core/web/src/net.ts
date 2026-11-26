// Peer transports, copied from spikes/voxel-sandbox/src/net.ts.
//
// Changes from the original: the message type is generic (this harness sends
// its own messages, including raw bytes), and the Trystero transport packs a
// message as one binary frame so a Uint8Array field crosses WebRTC without
// being turned into JSON. The Lockstep class is not copied: the Rust core's
// split-tick protocol lives in peers.ts instead.

/** Every message says who sent it. A message may carry one `bytes` field. */
export interface Msg { readonly t: string; readonly from: string; readonly bytes?: Uint8Array }

export interface Transport<M extends Msg> {
  readonly self: string
  readonly kind: string
  send(msg: M): void
  onMessage(handler: (msg: M) => void): void
  /** A peer vanished without saying bye (closed tab, lost connection). */
  onLeave(handler: (peer: string) => void): void
  close(): void
}

export const newId = () => Math.random().toString(36).slice(2, 10) // identity only, never in the sim

/** Peers in tabs of the same browser: instant, for local testing. Bytes are
 *  copied by the structured clone, so no encoding is needed. */
export class BroadcastTransport<M extends Msg> implements Transport<M> {
  readonly self = newId()
  readonly kind = 'broadcast'
  private readonly channel: BroadcastChannel
  private leaveHandlers: ((peer: string) => void)[] = []

  constructor(room: string) {
    this.channel = new BroadcastChannel(`fv-${room}`)
    addEventListener('pagehide', () => this.close())
  }
  send(msg: M) { this.channel.postMessage(msg) }
  onMessage(handler: (msg: M) => void) {
    this.channel.addEventListener('message', ev => {
      const msg = ev.data as M
      if (msg.t === 'bye') this.leaveHandlers.forEach(h => h(msg.from))
      handler(msg)
    })
  }
  onLeave(handler: (peer: string) => void) { this.leaveHandlers.push(handler) }
  close() { this.send({ t: 'bye', from: this.self } as unknown as M); this.channel.close() }
}

/** A message as one binary frame: 4-byte header length, the JSON header
 *  (everything but `bytes`), then the bytes. */
function pack(msg: Msg): Uint8Array {
  const { bytes, ...rest } = msg
  const head = new TextEncoder().encode(JSON.stringify(rest))
  const out = new Uint8Array(4 + head.length + (bytes?.length ?? 0))
  new DataView(out.buffer).setUint32(0, head.length, true)
  out.set(head, 4)
  if (bytes) out.set(bytes, 4 + head.length)
  return out
}
function unpack<M extends Msg>(data: Uint8Array): M {
  const n = new DataView(data.buffer, data.byteOffset).getUint32(0, true)
  const head = JSON.parse(new TextDecoder().decode(data.subarray(4, 4 + n)))
  const rest = data.subarray(4 + n)
  return (rest.length ? { ...head, bytes: rest.slice() } : head) as M
}

/** Real peer-to-peer over WebRTC, signalled through public Nostr relays. */
export class TrysteroTransport<M extends Msg> implements Transport<M> {
  readonly self = newId()
  readonly kind = 'trystero'
  private room: Awaited<ReturnType<typeof import('trystero').joinRoom>> | undefined
  private queue: M[] = []
  private handlers: ((msg: M) => void)[] = []
  private leaveHandlers: ((peer: string) => void)[] = []
  /** trystero peer id → our peer id, learnt from their first message. */
  private readonly ids = new Map<string, string>()
  private sendRaw: ((data: Uint8Array) => Promise<void>) | undefined

  constructor(room: string) {
    void import('trystero').then(({ joinRoom }) => {
      const r = joinRoom({ appId: 'fractured-valley/fv-wasm' }, room)
      this.room = r
      const action = r.makeAction<Uint8Array>('m')
      this.sendRaw = data => action.send(data)
      action.onMessage((data, ctx) => {
        const msg = unpack<M>(data)
        this.ids.set(ctx.peerId, msg.from)
        this.handlers.forEach(h => h(msg))
      })
      r.onPeerLeave(ctx => {
        const id = this.ids.get(ctx.peerId)
        if (id) this.leaveHandlers.forEach(h => h(id))
      })
      // Say hello again whenever someone new connects, so they learn of us.
      r.onPeerJoin(() => { for (const m of this.queue.splice(0)) void this.sendRaw?.(pack(m)) })
    })
  }
  send(msg: M) {
    if (msg.t === 'hello') this.queue.push(msg) // re-sent on each peer join
    void this.sendRaw?.(pack(msg))
  }
  onMessage(handler: (msg: M) => void) { this.handlers.push(handler) }
  onLeave(handler: (peer: string) => void) { this.leaveHandlers.push(handler) }
  close() { void this.room?.leave() }
}
