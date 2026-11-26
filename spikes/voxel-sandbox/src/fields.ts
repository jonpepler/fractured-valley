// Gas fields: heat, soot (dark smoke) and steam (white smoke), at 20 cm
// resolution (one field cell = 2×2×2 world cells).
//
// They are simulation state, not decoration: heat dries and lights fuel,
// and fire spreads through them. Each tick they are carried by the wind and
// by hot air rising (semi-Lagrangian advection), blur a little, and fade.
// Flames and plumes on screen are drawn from them (render.ts).
//
// Shape (decision 2.7b): each step reads only the previous buffers and
// writes only the cell it is computing, so regions could run on any thread
// or peer. Integer maths only. Only bricks with gas in them (and their
// neighbours) are processed.
import { BRICK, type Grid } from './grid'
import { mat } from './materials'
import { stepFlow, type FlowView } from './flow'
import { prevailing, windAt } from './wind'

export const FR = 2 // world cells per field cell
const FB = BRICK / FR // field cells per brick edge (8)
const CAP = 65535
/** Upward drift (1/256 field cells per tick): warm smoke keeps rising gently; hot gas fast. */
const rise = (heat: number) => Math.max(56, Math.min(300, heat >> 1))
const FACES: readonly [number, number, number][] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]]

/**
 * What the air carries. Fuel is burnable vapour given off by hot fuel; where
 * the air is hot enough it burns, adding heat and a little soot, and the
 * amount burning this tick is Flame (not carried: it is made fresh each tick).
 */
export const enum F { Heat = 0, Soot = 1, Steam = 2, Fuel = 3, Flame = 4, O2Used = 5 }
export const KINDS = 6

/**
 * How fast each field fades per tick, out of 256 (vapour that never burns
 * slowly disperses). Oxygen used doesn't fade: fresh air only comes back by
 * mixing in from around, so a closed space stays starved.
 */
const FADE = [14, 9, 12, 24, 0, 0]
/** Oxygen in fresh air, in the same units as O2Used (21% of air ≈ 2100). */
const O2_FRESH = 2100
/** Oxygen used per unit of vapour burnt. */
const O2_PER_FUEL = 2
/** Vapour burns where the air is at least this hot (°C), given oxygen. */
const VAPOUR_IGNITES = 420
/** Share of the vapour in a cell that burns per tick, out of 256, and heat released per unit burnt. */
const BURN_SHARE = 220
const HEAT_PER_FUEL = 6
/** Soot made per unit of vapour burnt, out of 256 (a well-fed flame makes little). */
const SOOT_PER_FUEL = 40
/** Heat a cell gains from flame in the cells around it, per unit of flame, in 1/16 °C. */
const RADIANT = 8
/** Gas can't get hotter than a flame (°C). */
const MAX_HEAT = 1100

export class Fields {
  readonly nx: number
  readonly ny: number
  readonly nz: number
  /** Current values, [kind][cell]. */
  cur: Uint16Array[]
  next: Uint16Array[]
  /** Bricks (world brick index) that hold any gas. */
  active = new Set<number>()
  /** Field cells that are mostly solid: gas can't be there. */
  readonly solid: Uint8Array

  /** Air velocity (departure from the prevailing wind), when the air moves. */
  vel: Int32Array[] | undefined
  private velNext: Int32Array[] = []
  private scratch: { mask: Uint8Array; div: Int32Array; p: Int32Array; pNext: Int32Array } | undefined
  /** Bricks where the air departs from the wind. */
  moving = new Set<number>()
  private lastTodo = new Set<number>()

  /**
   * @param shared put the buffers in shared memory, so worker threads can compute bricks.
   * @param flow give the air its own velocity (buoyancy, pressure); otherwise gas is just carried by the wind and a fixed rise.
   */
  constructor(private readonly world: Grid, shared = false, readonly flow = false) {
    this.nx = Math.ceil(world.sx / FR)
    this.ny = Math.ceil(world.sy / FR)
    this.nz = Math.ceil(world.sz / FR)
    const n = this.nx * this.ny * this.nz
    const u16 = () => new Uint16Array(shared ? new SharedArrayBuffer(n * 2) : new ArrayBuffer(n * 2))
    this.cur = Array.from({ length: KINDS }, u16)
    this.next = Array.from({ length: KINDS }, u16)
    this.solid = new Uint8Array(shared ? new SharedArrayBuffer(n) : new ArrayBuffer(n))
    if (flow) {
      const i32 = () => new Int32Array(shared ? new SharedArrayBuffer(n * 4) : new ArrayBuffer(n * 4))
      this.vel = [0, 1, 2].map(i32)
      this.velNext = [0, 1, 2].map(i32)
      this.scratch = { mask: new Uint8Array(shared ? new SharedArrayBuffer(n) : new ArrayBuffer(n)), div: i32(), p: i32(), pNext: i32() }
    }
  }

  index(fx: number, fy: number, fz: number): number {
    return (fz * this.ny + fy) * this.nx + fx
  }

  /** Field value at a world cell. */
  at(kind: F, x: number, y: number, z: number): number {
    const fx = Math.floor(x / FR), fy = Math.floor(y / FR), fz = Math.floor(z / FR)
    if (fx < 0 || fy < 0 || fz < 0 || fx >= this.nx || fy >= this.ny || fz >= this.nz) return 0
    return this.cur[kind]![this.index(fx, fy, fz)]!
  }

  /**
   * Raise the air at a world cell to at least this temperature: a burning
   * surface glows (700 °C or so) and the air touching it is that hot. Uses
   * max, so the order cells do it in doesn't matter.
   */
  warm(x: number, y: number, z: number, heat: number) {
    const fx = Math.floor(x / FR), fy = Math.floor(y / FR), fz = Math.floor(z / FR)
    if (fx < 0 || fy < 0 || fz < 0 || fx >= this.nx || fy >= this.ny || fz >= this.nz) return
    const i = this.index(fx, fy, fz)
    if (this.cur[F.Heat]![i]! < heat) this.cur[F.Heat]![i] = Math.min(MAX_HEAT, heat)
    this.active.add(this.world.brickIndex(fx * FR, fy * FR, fz * FR))
  }

  /** Add gas at a world cell (saturating; order-independent). */
  emit(x: number, y: number, z: number, heat: number, soot: number, steam: number, fuel = 0) {
    const fx = Math.floor(x / FR), fy = Math.floor(y / FR), fz = Math.floor(z / FR)
    if (fx < 0 || fy < 0 || fz < 0 || fx >= this.nx || fy >= this.ny || fz >= this.nz) return
    const i = this.index(fx, fy, fz)
    if (heat) this.cur[F.Heat]![i] = Math.min(MAX_HEAT, this.cur[F.Heat]![i]! + heat)
    if (soot) this.cur[F.Soot]![i] = Math.min(CAP, this.cur[F.Soot]![i]! + soot)
    if (steam) this.cur[F.Steam]![i] = Math.min(CAP, this.cur[F.Steam]![i]! + steam)
    if (fuel) this.cur[F.Fuel]![i] = Math.min(CAP, this.cur[F.Fuel]![i]! + fuel)
    this.active.add(this.world.brickIndex(fx * FR, fy * FR, fz * FR))
  }

  /**
   * Gas velocity at a field cell, in 1/256 field cells per tick: the wind,
   * plus hot air rising (buoyancy grows with heat, capped).
   */
  velocity(seed: number, tick: number, fx: number, fy: number, fz: number, heat: number, base = prevailing(seed, tick)): [number, number, number] {
    const w = windAt(seed, tick, fx * FR + 1, fy * FR + 1, base)
    // wind ±256 ≈ a strong breeze ≈ 1 world cell (half a field cell) per tick
    if (this.vel && fx >= 0 && fy >= 0 && fz >= 0 && fx < this.nx && fy < this.ny && fz < this.nz) {
      const i = this.index(fx, fy, fz)
      return [(w.x >> 1) + this.vel[0]![i]!, (w.y >> 1) + this.vel[1]![i]!, this.vel[2]![i]!]
    }
    return [w.x >> 1, w.y >> 1, rise(heat)]
  }

  /** Field buffers and dimensions, for the kernel (here, in a worker, or on a peer). */
  view(): FieldView {
    return { nx: this.nx, ny: this.ny, nz: this.nz, bx: this.world.bx, by: this.world.by, cur: this.cur, next: this.next, solid: this.solid, ...(this.vel ? { vel: this.vel } : {}) }
  }

  /** The bricks the next step must compute (sorted), with their solid masks up to date. */
  plan(): number[] {
    const w = this.world
    const todo = new Set<number>()
    for (const b of this.active) {
      const bx = b % w.bx, by = Math.floor(b / w.bx) % w.by, bz = Math.floor(b / (w.bx * w.by))
      todo.add(b)
      for (const [dx, dy, dz] of FACES) {
        const x = bx + dx, y = by + dy, z = bz + dz
        if (x >= 0 && y >= 0 && z >= 0 && x < w.bx && y < w.by && z < w.bz) todo.add((z * w.by + y) * w.bx + x)
      }
    }
    const bricks = [...todo].sort((a, b) => a - b)
    for (const b of bricks) this.ensureSolid(b)
    return bricks
  }

  /** Swap buffers once every planned brick has been computed into `next`. */
  finish(withGas: Iterable<number>) {
    const t = this.cur; this.cur = this.next; this.next = t
    this.parity ^= 1
    for (const b of this.active) this.clearBrick(this.next, b)
    this.active = new Set(withGas)
  }

  /** Which of the two buffers is current (0 or 1); shared-memory workers need it. */
  parity = 0

  private ensureSolid(b: number) {
    const w = this.world
    if (this.solidAt.length !== w.version.length) this.solidAt = new Int32Array(w.version.length).fill(-1)
    if (w.version[b] !== this.solidAt[b]) this.refreshSolid(b, b % w.bx, Math.floor(b / w.bx) % w.by, Math.floor(b / (w.bx * w.by)))
  }

  step(seed: number, tick: number) {
    const bricks = this.plan()
    if (this.vel) this.stepAir(bricks, seed, tick)
    this.finish(computeBricks(this.view(), bricks, seed, tick))
  }

  /** Advance the air's velocity over these bricks, before gas is carried by it. */
  /** The air's buffers, for the flow phases (here or in workers). */
  airView(): FlowView {
    const s = this.scratch!
    return { nx: this.nx, ny: this.ny, nz: this.nz, bx: this.world.bx, by: this.world.by, solid: this.solid,
      heat: this.cur[0]!, vel: this.vel!, velNext: this.velNext, mask: s.mask, div: s.div, p: s.p, pNext: s.pNext }
  }

  /** Which velocity buffer is current (0 or 1), for shared-memory workers. */
  velParity = 0

  private stepAir(bricks: number[], seed: number, tick: number) {
    this.finishAir(bricks, stepFlow(this.airView(), bricks, seed, tick))
  }

  /** After the flow phases have filled velNext for these bricks: swap and tidy. */
  finishAir(bricks: number[], moving: number[]) {
    const t = this.vel!; this.vel = this.velNext; this.velNext = t
    this.velParity ^= 1
    // Bricks no longer computed go back to plain wind; the spare buffer is
    // zeroed where it held velocity, so stale motion never resurfaces.
    const todo = new Set(bricks)
    for (const b of this.lastTodo) if (!todo.has(b)) this.clearBrick(this.vel!, b)
    for (const b of this.lastTodo) this.clearBrick(this.velNext, b)
    this.lastTodo = todo
    this.moving = new Set(moving)
  }

  private solidAt = new Int32Array(0)

  private refreshSolid(b: number, bx: number, by: number, bz: number) {
    const w = this.world
    for (let lz = 0; lz < FB; lz++) for (let ly = 0; ly < FB; ly++) for (let lx = 0; lx < FB; lx++) {
      const fx = bx * FB + lx, fy = by * FB + ly, fz = bz * FB + lz
      if (fx >= this.nx || fy >= this.ny || fz >= this.nz) continue
      let solid = 0
      for (let dz = 0; dz < FR; dz++) for (let dy = 0; dy < FR; dy++) for (let dx = 0; dx < FR; dx++)
        if (mat(w.get(fx * FR + dx, fy * FR + dy, fz * FR + dz)).solid) solid++
      this.solid[this.index(fx, fy, fz)] = solid >= 6 ? 1 : 0
    }
    this.solidAt[b] = w.version[b]!
  }

  private clearBrick(arrs: (Uint16Array | Int32Array)[], b: number) {
    const w = this.world
    const bx = b % w.bx, by = Math.floor(b / w.bx) % w.by, bz = Math.floor(b / (w.bx * w.by))
    for (let lz = 0; lz < FB; lz++) for (let ly = 0; ly < FB; ly++) for (let lx = 0; lx < FB; lx++) {
      const fx = bx * FB + lx, fy = by * FB + ly, fz = bz * FB + lz
      if (fx >= this.nx || fy >= this.ny || fz >= this.nz) continue
      const i = this.index(fx, fy, fz)
      for (const a of arrs) a[i] = 0
    }
  }







  /** Hash of all gas, in a fixed order (part of the desync check). */
  hash(h: number, mix: (h: number, v: number) => number): number {
    if (this.vel) for (const b of [...this.moving].sort((a, c) => a - c)) {
      h = mix(h, b)
      this.eachIndex(b, i => { h = mix(h, this.vel![0]![i]! ^ (this.vel![1]![i]! << 11) ^ (this.vel![2]![i]! << 22)) })
    }
    for (const b of [...this.active].sort((a, c) => a - c)) {
      h = mix(h, b)
      const w = this.world
      const bx = b % w.bx, by = Math.floor(b / w.bx) % w.by, bz = Math.floor(b / (w.bx * w.by))
      for (let lz = 0; lz < FB; lz++) for (let ly = 0; ly < FB; ly++) for (let lx = 0; lx < FB; lx++) {
        const fx = bx * FB + lx, fy = by * FB + ly, fz = bz * FB + lz
        if (fx >= this.nx || fy >= this.ny || fz >= this.nz) continue
        const i = this.index(fx, fy, fz)
        for (let k = 0; k < KINDS; k += 2) h = mix(h, this.cur[k]![i]! | ((this.cur[k + 1]?.[i] ?? 0) << 16))
      }
    }
    return h
  }

  /** Every field cell with gas, for drawing and inspection: [fx, fy, fz, heat, soot, steam, fuel, flame]. */
  *cells(): Generator<[number, number, number, number, number, number, number, number]> {
    const w = this.world
    for (const b of this.active) {
      const bx = b % w.bx, by = Math.floor(b / w.bx) % w.by, bz = Math.floor(b / (w.bx * w.by))
      for (let lz = 0; lz < FB; lz++) for (let ly = 0; ly < FB; ly++) for (let lx = 0; lx < FB; lx++) {
        const fx = bx * FB + lx, fy = by * FB + ly, fz = bz * FB + lz
        if (fx >= this.nx || fy >= this.ny || fz >= this.nz) continue
        const i = this.index(fx, fy, fz)
        const hh = this.cur[0]![i]!, s = this.cur[1]![i]!, st = this.cur[2]![i]!, fu = this.cur[3]![i]!, fl = this.cur[4]![i]!
        if (hh | s | st | fu | fl | this.cur[5]![i]!) yield [fx, fy, fz, hh, s, st, fu, fl]
      }
    }
  }

  /** Field coordinates of every cell in a brick. */
  cellsOfBrick(b: number, f: (fx: number, fy: number, fz: number) => void) {
    const w = this.world
    const bx = b % w.bx, by = Math.floor(b / w.bx) % w.by, bz = Math.floor(b / (w.bx * w.by))
    for (let lz = 0; lz < FB; lz++) for (let ly = 0; ly < FB; ly++) for (let lx = 0; lx < FB; lx++) {
      const fx = bx * FB + lx, fy = by * FB + ly, fz = bz * FB + lz
      if (fx < this.nx && fy < this.ny && fz < this.nz) f(fx, fy, fz)
    }
  }

  private eachIndex(b: number, f: (i: number) => void) {
    const w = this.world
    const bx = b % w.bx, by = Math.floor(b / w.bx) % w.by, bz = Math.floor(b / (w.bx * w.by))
    for (let lz = 0; lz < FB; lz++) for (let ly = 0; ly < FB; ly++) for (let lx = 0; lx < FB; lx++) {
      const fx = bx * FB + lx, fy = by * FB + ly, fz = bz * FB + lz
      if (fx < this.nx && fy < this.ny && fz < this.nz) f(this.index(fx, fy, fz))
    }
  }

  /** The moving air, exactly, for checkpoints: both bookkeeping sets and the velocity of every brick in either. */
  exportAir(): AirState {
    if (!this.vel) return { moving: [], todo: [], bricks: [] }
    const moving = [...this.moving].sort((a, c) => a - c), todo = [...this.lastTodo].sort((a, c) => a - c)
    const bricks = [...new Set([...moving, ...todo])].sort((a, c) => a - c).map(b => {
      const values: number[] = []
      this.eachIndex(b, i => values.push(this.vel![0]![i]!, this.vel![1]![i]!, this.vel![2]![i]!))
      return { brick: b, values }
    })
    return { moving, todo, bricks }
  }

  importAir(air: AirState) {
    if (!this.vel) return
    for (const { brick: b, values } of air.bricks) {
      let k = 0
      this.eachIndex(b, i => { this.vel![0]![i] = values[k++]!; this.vel![1]![i] = values[k++]!; this.vel![2]![i] = values[k++]! })
    }
    this.moving = new Set(air.moving)
    this.lastTodo = new Set(air.todo)
  }

  /** Raw values of the active bricks, for checkpoints. */
  exportActive(): { brick: number; values: number[] }[] {
    const out: { brick: number; values: number[] }[] = []
    const w = this.world
    for (const b of [...this.active].sort((a, c) => a - c)) {
      const bx = b % w.bx, by = Math.floor(b / w.bx) % w.by, bz = Math.floor(b / (w.bx * w.by))
      const values: number[] = []
      for (let lz = 0; lz < FB; lz++) for (let ly = 0; ly < FB; ly++) for (let lx = 0; lx < FB; lx++) {
        const fx = bx * FB + lx, fy = by * FB + ly, fz = bz * FB + lz
        if (fx >= this.nx || fy >= this.ny || fz >= this.nz) { for (let k = 0; k < KINDS; k++) values.push(0); continue }
        const i = this.index(fx, fy, fz)
        for (let k = 0; k < KINDS; k++) values.push(this.cur[k]![i]!)
      }
      out.push({ brick: b, values })
    }
    return out
  }

  importActive(list: { brick: number; values: number[] }[]) {
    const w = this.world
    for (const { brick: b, values } of list) {
      const bx = b % w.bx, by = Math.floor(b / w.bx) % w.by, bz = Math.floor(b / (w.bx * w.by))
      let k = 0
      for (let lz = 0; lz < FB; lz++) for (let ly = 0; ly < FB; ly++) for (let lx = 0; lx < FB; lx++) {
        const fx = bx * FB + lx, fy = by * FB + ly, fz = bz * FB + lz
        if (fx >= this.nx || fy >= this.ny || fz >= this.nz) { k += KINDS; continue }
        const i = this.index(fx, fy, fz)
        for (let kind = 0; kind < KINDS; kind++) this.cur[kind]![i] = values[k++]!
      }
      this.active.add(b)
    }
  }
}

export interface AirState {
  moving: number[]
  todo: number[]
  bricks: { brick: number; values: number[] }[]
}

/** The gases the air carries (Flame is made fresh each tick, not carried). */
const CARRIED = [F.Heat, F.Soot, F.Steam, F.Fuel, F.O2Used] as const

/** Plain arrays and sizes: everything the kernel needs, and nothing else. */
export interface FieldView {
  nx: number; ny: number; nz: number
  /** World bricks per axis (a brick is FB³ field cells). */
  bx: number; by: number
  cur: Uint16Array[]
  next: Uint16Array[]
  solid: Uint8Array
  /** Air velocity (departure from the wind), if the air moves; else gas just rises. */
  vel?: Int32Array[]
}

/**
 * One tick of gas movement for the given bricks: reads only `cur`, writes
 * only the cells of those bricks in `next`. That is the 2.7b shape, so any
 * split of the bricks (threads, peers) gives bit-identical results.
 * Returns the bricks that still hold gas.
 */
export function computeBricks(v: FieldView, bricks: readonly number[], seed: number, tick: number): number[] {
  const { nx, ny, nz } = v
  const nxy = nx * ny
  const cur0 = v.cur[0]!
  const base = prevailing(seed, tick)
  const withGas: number[] = []
  for (const b of bricks) {
    const bx = b % v.bx, by = Math.floor(b / v.bx) % v.by, bz = Math.floor(b / (v.bx * v.by))
    let any = false
    // Wind once per brick (gusts are ~5 m across; a brick is 1.6 m).
    const wind = windAt(seed, tick, bx * BRICK + 8, by * BRICK + 8, base)
    const wx = wind.x >> 1, wy = wind.y >> 1
    for (let lz = 0; lz < FB; lz++) for (let ly = 0; ly < FB; ly++) for (let lx = 0; lx < FB; lx++) {
      const fx = bx * FB + lx, fy = by * FB + ly, fz = bz * FB + lz
      if (fx >= nx || fy >= ny || fz >= nz) continue
      const i = (fz * ny + fy) * nx + fx
      if (v.solid[i]) { for (let k = 0; k < KINDS; k++) v.next[k]![i] = 0; continue }
      // Where did the gas now here come from? The wind plus the air's own
      // motion, or (without moving air) the wind plus a fixed rise for hot gas.
      const vel = v.vel
      const sx = fx * 256 - wx - (vel ? vel[0]![i]! : 0), sy = fy * 256 - wy - (vel ? vel[1]![i]! : 0)
      const sz = fz * 256 - (vel ? vel[2]![i]! : rise(cur0[i]!))
      const x0 = sx >> 8, y0 = sy >> 8, z0 = sz >> 8
      const tx = sx & 255, ty = sy & 255, tz = sz & 255
      // The 8 corners of the source cell (outside the field reads as 0).
      const okx0 = x0 >= 0 && x0 < nx, okx1 = x0 + 1 >= 0 && x0 + 1 < nx
      const oky0 = y0 >= 0 && y0 < ny, oky1 = y0 + 1 >= 0 && y0 + 1 < ny
      const okz0 = z0 >= 0 && z0 < nz, okz1 = z0 + 1 >= 0 && z0 + 1 < nz
      const base0 = z0 * nxy + y0 * nx + x0
      const w000 = ((256 - tx) * (256 - ty) >> 8) * (256 - tz) >> 8, w100 = (tx * (256 - ty) >> 8) * (256 - tz) >> 8
      const w010 = ((256 - tx) * ty >> 8) * (256 - tz) >> 8, w110 = (tx * ty >> 8) * (256 - tz) >> 8
      const w001 = ((256 - tx) * (256 - ty) >> 8) * tz >> 8, w101 = (tx * (256 - ty) >> 8) * tz >> 8
      const w011 = ((256 - tx) * ty >> 8) * tz >> 8
      // The last weight takes the rounding remainder, so the eight always sum
      // to exactly 256: carrying gas neither loses nor makes any of it.
      const w111 = 256 - w000 - w100 - w010 - w110 - w001 - w101 - w011
      const c000 = okx0 && oky0 && okz0, c100 = okx1 && oky0 && okz0, c010 = okx0 && oky1 && okz0, c110 = okx1 && oky1 && okz0
      const c001 = okx0 && oky0 && okz1, c101 = okx1 && oky0 && okz1, c011 = okx0 && oky1 && okz1, c111 = okx1 && oky1 && okz1
      // Neighbours for the blur (outside reads as 0).
      const ixp = fx + 1 < nx ? i + 1 : -1, ixm = fx > 0 ? i - 1 : -1
      const iyp = fy + 1 < ny ? i + nx : -1, iym = fy > 0 ? i - nx : -1
      const izp = fz + 1 < nz ? i + nxy : -1, izm = fz > 0 ? i - nxy : -1
      // Carry, blur and fade each gas (flame is not carried: it is made below).
      const val = [0, 0, 0, 0, 0, 0]
      let lostFuel = 0
      for (const k of CARRIED) {
        const a = v.cur[k]!
        // Gas traced back into a wall reads as this cell's own (walls neither
        // hold nor supply gas), so a sealed room keeps its air.
        const own = a[i]!, sol = v.solid
        const j0 = base0, j1 = base0 + 1, j2 = base0 + nx, j3 = base0 + nx + 1
        const j4 = base0 + nxy, j5 = base0 + nxy + 1, j6 = base0 + nxy + nx, j7 = base0 + nxy + nx + 1
        const adv = ((c000 ? (sol[j0] ? own : a[j0]!) * w000 : 0) + (c100 ? (sol[j1] ? own : a[j1]!) * w100 : 0) +
          (c010 ? (sol[j2] ? own : a[j2]!) * w010 : 0) + (c110 ? (sol[j3] ? own : a[j3]!) * w110 : 0) +
          (c001 ? (sol[j4] ? own : a[j4]!) * w001 : 0) + (c101 ? (sol[j5] ? own : a[j5]!) * w101 : 0) +
          (c011 ? (sol[j6] ? own : a[j6]!) * w011 : 0) + (c111 ? (sol[j7] ? own : a[j7]!) * w111 : 0) + 128) >> 8
        // A solid neighbour counts as this cell (gas doesn't soak into walls,
        // and a wall doesn't supply fresh air); outside the grid is open air.
        const nb = ((ixp < 0 ? 0 : sol[ixp] ? own : a[ixp]!) + (ixm < 0 ? 0 : sol[ixm] ? own : a[ixm]!) +
          (iyp < 0 ? 0 : sol[iyp] ? own : a[iyp]!) + (iym < 0 ? 0 : sol[iym] ? own : a[iym]!) +
          (izp < 0 ? 0 : sol[izp] ? own : a[izp]!) + (izm < 0 ? 0 : sol[izm] ? own : a[izm]!) + 3) / 6 | 0
        // Smoke (soot, steam) mixes into the air around it faster than heat or vapour, so plumes widen and thin.
        // Moving air is turbulent and mixes with its neighbours faster: fast
        // air (a fire's plume) pulls fresh air in, so flames get oxygen. It's
        // mixing, not a source: a sealed room still runs out.
        const mixMore = vel ? Math.min(3, (Math.abs(vel[0]![i]!) + Math.abs(vel[1]![i]!) + Math.abs(vel[2]![i]!)) >> 7) : 0
        const nbShare = (k === F.Soot || k === F.Steam ? 3 : 2) + mixMore // out of 8
        let x = (adv * (8 - nbShare) + nb * nbShare + 4) >> 3
        const faded = (x * FADE[k]! + 255) >> 8
        x -= faded
        val[k] = x < 24 ? 0 : x
        lostFuel = k === F.Fuel ? faded : lostFuel
      }
      // Vapour that cools and thins before it finds air doesn't vanish: it
      // condenses into tarry droplets, which is smoke.
      val[F.Soot] = Math.min(CAP, val[F.Soot]! + (lostFuel >> 1))
      // Radiation: flame next door (last tick) heats this cell, most from the
      // flame beside and below it. This is how flame reaches fuel it isn't in.
      const fl0 = v.cur[F.Flame]!
      const glow = (ixp >= 0 ? fl0[ixp]! : 0) + (ixm >= 0 ? fl0[ixm]! : 0) + (iyp >= 0 ? fl0[iyp]! : 0) +
        (iym >= 0 ? fl0[iym]! : 0) + (izm >= 0 ? fl0[izm]! : 0) * 2 + (izp >= 0 ? fl0[izp]! : 0)
      if (glow) val[F.Heat] = Math.min(MAX_HEAT, val[F.Heat]! + ((glow * RADIANT) >> 4))
      // Combustion: vapour in hot air burns, adding heat and a little soot.
      // (Oxygen is assumed plentiful for now; brief 05 has it in the air.)
      // It needs oxygen: it can only burn what the oxygen here allows, uses
      // it up, and where oxygen runs short it burns dirty (more soot).
      let flame = 0
      const o2 = Math.max(0, O2_FRESH - val[F.O2Used]!)
      if (val[F.Fuel]! > 0 && val[F.Heat]! >= VAPOUR_IGNITES && o2 > 0) {
        flame = Math.max(Math.min(val[F.Fuel]!, 24), (val[F.Fuel]! * BURN_SHARE) >> 8)
        flame = Math.min(flame, Math.floor(o2 / O2_PER_FUEL))
        val[F.Fuel]! -= flame
        val[F.O2Used] = Math.min(O2_FRESH, val[F.O2Used]! + flame * O2_PER_FUEL)
        val[F.Heat] = Math.min(MAX_HEAT, val[F.Heat]! + flame * HEAT_PER_FUEL)
        const starved = 256 + ((768 * (O2_FRESH - o2)) / O2_FRESH | 0) // ×1 fresh … ×4 starved
        val[F.Soot] = Math.min(CAP, val[F.Soot]! + ((((flame * SOOT_PER_FUEL) >> 8) * starved) >> 8))
      }
      for (const k of CARRIED) v.next[k]![i] = val[k]!
      v.next[F.Flame]![i] = Math.min(CAP, flame)
      if (val[0]! | val[1]! | val[2]! | val[3]! | val[5]! | flame) any = true
    }
    if (any) withGas.push(b)
  }
  return withGas
}

