// A checkpoint is "world = seed + edits": the generator spec, the bricks that
// differ from freshly generated ones (palette-packed), and the simulation's
// small sets. Taken only while nothing is moving, so physics is empty.
// A joiner restores it and replays the input frames since.
import { generate, type WorldSpec } from './gen'
import { BRICK_CELLS, type Grid } from './grid'
import type { Command, Scheduled, SimState } from './sim'
import { Sim } from './sim'

class Writer {
  private buf = new Uint8Array(1 << 16)
  length = 0
  private room(n: number) {
    if (this.length + n <= this.buf.length) return
    const next = new Uint8Array(Math.max(this.buf.length * 2, this.length + n))
    next.set(this.buf.subarray(0, this.length)); this.buf = next
  }
  byte(v: number) { this.room(1); this.buf[this.length++] = v }
  uint(v: number) { // LEB128
    this.room(5)
    do { let b = v & 0x7f; v >>>= 7; if (v) b |= 0x80; this.buf[this.length++] = b } while (v)
  }
  int(v: number) { this.uint(((v << 1) ^ (v >> 31)) >>> 0) } // zigzag
  bytes(a: Uint8Array) { this.room(a.length); this.buf.set(a, this.length); this.length += a.length }
  ascending(list: number[]) { this.uint(list.length); let prev = 0; for (const v of list) { this.uint(v - prev); prev = v } }
  done() { return this.buf.slice(0, this.length) }
}

class Reader {
  at = 0
  constructor(private buf: Uint8Array) {}
  byte() { return this.buf[this.at++]! }
  uint() { let v = 0, shift = 0, b: number; do { b = this.buf[this.at++]!; v |= (b & 0x7f) << shift; shift += 7 } while (b & 0x80); return v >>> 0 }
  int() { const u = this.uint(); return (u >>> 1) ^ -(u & 1) }
  bytes(n: number) { const a = this.buf.subarray(this.at, this.at + n); this.at += n; return a }
  ascending() { const n = this.uint(), out: number[] = []; let prev = 0; for (let i = 0; i < n; i++) { prev += this.uint(); out.push(prev) } return out }
}

const MAGIC = 0x46564332 // "FVC2": with moisture, gas and embers
/** Values per gas brick: 8³ field cells × (heat, soot, steam). */
const GAS_VALUES = 8 * 8 * 8 * 3

function cellsOf(g: Grid, b: number, into: Uint8Array, state: Uint8Array, wet: Uint8Array) {
  const brick = g.bricks[b]
  if (brick) { into.set(brick.material); state.set(brick.state); wet.set(brick.moisture) } else { into.fill(g.uniform[b]!); state.fill(0); wet.fill(0) }
}

function writeCommand(w: Writer, c: Command) {
  w.byte(c.kind === 'ignite' ? 0 : 1); w.int(c.x); w.int(c.y); w.int(c.z)
}

export interface CheckpointStats { bytes: number; changedBricks: number; bricks: number }

export function takeCheckpoint(sim: Sim, spec: WorldSpec, fresh: Grid = generate(spec)): { data: Uint8Array; stats: CheckpointStats } {
  const st: SimState = sim.exportState()
  const w = new Writer()
  w.uint(MAGIC)
  w.uint(spec.seed); w.uint(spec.sx); w.uint(spec.sy); w.uint(spec.sz); w.uint(spec.waterLevel)
  w.uint(st.tick)
  w.ascending(st.burning); w.ascending(st.loose); w.ascending(st.debris)
  w.uint(st.touched.length); for (const t of st.touched) w.uint(t)
  w.uint(st.queue.length)
  for (const s of st.queue) { w.uint(s.tick); w.uint(s.peer); w.uint(s.seq); writeCommand(w, s.command) }
  // Embers in flight, and the gas in the air (heat, soot, steam per active brick).
  w.uint(st.embers.length)
  for (const e of st.embers) { w.int(e.x); w.int(e.y); w.int(e.z); w.int(e.vx); w.int(e.vy); w.int(e.vz); w.int(e.life) }
  w.uint(st.gas.length)
  for (const { brick, values } of st.gas) { w.uint(brick); for (const v of values) w.uint(v) }

  const g = sim.world
  const a = new Uint8Array(BRICK_CELLS), as = new Uint8Array(BRICK_CELLS), aw = new Uint8Array(BRICK_CELLS)
  const f = new Uint8Array(BRICK_CELLS), fs = new Uint8Array(BRICK_CELLS), fw = new Uint8Array(BRICK_CELLS)
  const changed: number[] = []
  for (let b = 0; b < g.bricks.length; b++) {
    cellsOf(g, b, a, as, aw); cellsOf(fresh, b, f, fs, fw)
    for (let i = 0; i < BRICK_CELLS; i++) if (a[i] !== f[i] || as[i] !== fs[i] || aw[i] !== fw[i]) { changed.push(b); break }
  }
  w.ascending(changed)
  for (const b of changed) {
    cellsOf(g, b, a, as, aw)
    // Palette: the brick's distinct materials, then 1/2/4/8 bits per cell.
    const palette = [...new Set(a)].sort((x, y) => x - y)
    const index = new Map(palette.map((m, i) => [m, i]))
    const bits = palette.length <= 2 ? 1 : palette.length <= 4 ? 2 : palette.length <= 16 ? 4 : 8
    w.uint(palette.length); for (const m of palette) w.byte(m)
    const packed = new Uint8Array((BRICK_CELLS * bits) >> 3)
    for (let i = 0; i < BRICK_CELLS; i++) packed[(i * bits) >> 3]! |= index.get(a[i]!)! << ((i * bits) & 7)
    if (palette.length > 1) w.bytes(packed)
    // States are sparse (burning cells): count, then (cell, state) pairs.
    const stated: number[] = []
    for (let i = 0; i < BRICK_CELLS; i++) if (as[i]) stated.push(i)
    w.ascending(stated); for (const i of stated) w.byte(as[i]!)
    // Moisture: absent, or one byte per cell (it varies smoothly; compresses well).
    const anyWet = aw.some(v => v !== 0)
    w.byte(anyWet ? 1 : 0)
    if (anyWet) w.bytes(aw)
  }
  return { data: w.done(), stats: { bytes: w.length, changedBricks: changed.length, bricks: g.bricks.length } }
}

export function restoreCheckpoint(data: Uint8Array): { sim: Sim; spec: WorldSpec } {
  const r = new Reader(data)
  if (r.uint() !== MAGIC) throw new Error('not a checkpoint')
  const spec: WorldSpec = { seed: r.uint(), sx: r.uint(), sy: r.uint(), sz: r.uint(), waterLevel: r.uint() }
  const tick = r.uint()
  const burning = r.ascending(), loose = r.ascending(), debris = r.ascending()
  const touched: number[] = []; for (let n = r.uint(); n > 0; n--) touched.push(r.uint())
  const queue: Scheduled[] = []
  for (let n = r.uint(); n > 0; n--) {
    const t = r.uint(), peer = r.uint(), seq = r.uint(), kind = r.byte() === 0 ? 'ignite' as const : 'drop' as const
    queue.push({ tick: t, peer, seq, command: { kind, x: r.int(), y: r.int(), z: r.int() } })
  }
  const embers: SimState['embers'] = []
  for (let n = r.uint(); n > 0; n--) embers.push({ x: r.int(), y: r.int(), z: r.int(), vx: r.int(), vy: r.int(), vz: r.int(), life: r.int() })
  const gas: SimState['gas'] = []
  for (let n = r.uint(); n > 0; n--) {
    const brick = r.uint(), values: number[] = []
    for (let k = 0; k < GAS_VALUES; k++) values.push(r.uint())
    gas.push({ brick, values })
  }
  const world = generate(spec)
  const changed = r.ascending()
  for (const b of changed) {
    const palette = Array.from(r.bytes(r.uint()))
    const bits = palette.length <= 2 ? 1 : palette.length <= 4 ? 2 : palette.length <= 16 ? 4 : 8
    const packed = palette.length > 1 ? r.bytes((BRICK_CELLS * bits) >> 3) : undefined
    const stated = r.ascending()
    const states = new Uint8Array(BRICK_CELLS)
    for (const i of stated) states[i] = r.byte()
    const wet = r.byte() ? r.bytes(BRICK_CELLS) : new Uint8Array(BRICK_CELLS)
    const bxz = b % world.bx, rest = Math.floor(b / world.bx), byy = rest % world.by, bzz = Math.floor(rest / world.by)
    const mask = (1 << bits) - 1
    for (let i = 0; i < BRICK_CELLS; i++) {
      const m = packed ? palette[(packed[(i * bits) >> 3]! >> ((i * bits) & 7)) & mask]! : palette[0]!
      const x = (bxz << 4) | (i & 15), y = (byy << 4) | ((i >> 4) & 15), z = (bzz << 4) | (i >> 8)
      world.set(x, y, z, m, states[i]!, wet[i]!)
    }
  }
  world.compact()
  const sim = new Sim(world, spec.seed)
  sim.importState({ tick, burning, loose, debris, touched, queue, embers, gas })
  return { sim, spec }
}
