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
import { prevailing, windAt } from './wind'

export const FR = 2 // world cells per field cell
const FB = BRICK / FR // field cells per brick edge (8)
const CAP = 65535
/** Upward drift (1/256 field cells per tick): warm smoke keeps rising gently; hot gas fast. */
const rise = (heat: number) => Math.max(56, Math.min(300, heat >> 1))
const FACES: readonly [number, number, number][] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]]

export const enum F { Heat = 0, Soot = 1, Steam = 2 }
const KINDS = 3

/** How fast each field fades per tick, out of 256. */
const FADE = [22, 3, 6]
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

  /** @param shared put the buffers in shared memory, so worker threads can compute bricks. */
  constructor(private readonly world: Grid, shared = false) {
    this.nx = Math.ceil(world.sx / FR)
    this.ny = Math.ceil(world.sy / FR)
    this.nz = Math.ceil(world.sz / FR)
    const n = this.nx * this.ny * this.nz
    const u16 = () => new Uint16Array(shared ? new SharedArrayBuffer(n * 2) : new ArrayBuffer(n * 2))
    this.cur = Array.from({ length: KINDS }, u16)
    this.next = Array.from({ length: KINDS }, u16)
    this.solid = new Uint8Array(shared ? new SharedArrayBuffer(n) : new ArrayBuffer(n))
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

  /** Add gas at a world cell (saturating; order-independent). */
  emit(x: number, y: number, z: number, heat: number, soot: number, steam: number) {
    const fx = Math.floor(x / FR), fy = Math.floor(y / FR), fz = Math.floor(z / FR)
    if (fx < 0 || fy < 0 || fz < 0 || fx >= this.nx || fy >= this.ny || fz >= this.nz) return
    const i = this.index(fx, fy, fz)
    if (heat) this.cur[F.Heat]![i] = Math.min(MAX_HEAT, this.cur[F.Heat]![i]! + heat)
    if (soot) this.cur[F.Soot]![i] = Math.min(CAP, this.cur[F.Soot]![i]! + soot)
    if (steam) this.cur[F.Steam]![i] = Math.min(CAP, this.cur[F.Steam]![i]! + steam)
    this.active.add(this.world.brickIndex(fx * FR, fy * FR, fz * FR))
  }

  /**
   * Gas velocity at a field cell, in 1/256 field cells per tick: the wind,
   * plus hot air rising (buoyancy grows with heat, capped).
   */
  velocity(seed: number, tick: number, fx: number, fy: number, heat: number, base = prevailing(seed, tick)): [number, number, number] {
    const w = windAt(seed, tick, fx * FR + 1, fy * FR + 1, base)
    // wind ±256 ≈ a strong breeze ≈ 1 world cell (half a field cell) per tick
    return [w.x >> 1, w.y >> 1, rise(heat)]
  }

  /** Field buffers and dimensions, for the kernel (here, in a worker, or on a peer). */
  view(): FieldView {
    return { nx: this.nx, ny: this.ny, nz: this.nz, bx: this.world.bx, by: this.world.by, cur: this.cur, next: this.next, solid: this.solid }
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
    this.finish(computeBricks(this.view(), this.plan(), seed, tick))
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

  private clearBrick(arrs: Uint16Array[], b: number) {
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
    for (const b of [...this.active].sort((a, c) => a - c)) {
      h = mix(h, b)
      const w = this.world
      const bx = b % w.bx, by = Math.floor(b / w.bx) % w.by, bz = Math.floor(b / (w.bx * w.by))
      for (let lz = 0; lz < FB; lz++) for (let ly = 0; ly < FB; ly++) for (let lx = 0; lx < FB; lx++) {
        const fx = bx * FB + lx, fy = by * FB + ly, fz = bz * FB + lz
        if (fx >= this.nx || fy >= this.ny || fz >= this.nz) continue
        const i = this.index(fx, fy, fz)
        h = mix(h, this.cur[0]![i]! | (this.cur[1]![i]! << 16))
        h = mix(h, this.cur[2]![i]!)
      }
    }
    return h
  }

  /** Every field cell with gas, for drawing and inspection: [fx, fy, fz, heat, soot, steam]. */
  *cells(): Generator<[number, number, number, number, number, number]> {
    const w = this.world
    for (const b of this.active) {
      const bx = b % w.bx, by = Math.floor(b / w.bx) % w.by, bz = Math.floor(b / (w.bx * w.by))
      for (let lz = 0; lz < FB; lz++) for (let ly = 0; ly < FB; ly++) for (let lx = 0; lx < FB; lx++) {
        const fx = bx * FB + lx, fy = by * FB + ly, fz = bz * FB + lz
        if (fx >= this.nx || fy >= this.ny || fz >= this.nz) continue
        const i = this.index(fx, fy, fz)
        const hh = this.cur[0]![i]!, s = this.cur[1]![i]!, st = this.cur[2]![i]!
        if (hh | s | st) yield [fx, fy, fz, hh, s, st]
      }
    }
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
        if (fx >= this.nx || fy >= this.ny || fz >= this.nz) { values.push(0, 0, 0); continue }
        const i = this.index(fx, fy, fz)
        values.push(this.cur[0]![i]!, this.cur[1]![i]!, this.cur[2]![i]!)
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
        if (fx >= this.nx || fy >= this.ny || fz >= this.nz) { k += 3; continue }
        const i = this.index(fx, fy, fz)
        this.cur[0]![i] = values[k++]!; this.cur[1]![i] = values[k++]!; this.cur[2]![i] = values[k++]!
      }
      this.active.add(b)
    }
  }
}

/** Plain arrays and sizes: everything the kernel needs, and nothing else. */
export interface FieldView {
  nx: number; ny: number; nz: number
  /** World bricks per axis (a brick is FB³ field cells). */
  bx: number; by: number
  cur: Uint16Array[]
  next: Uint16Array[]
  solid: Uint8Array
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
  const cur0 = v.cur[0]!, cur1 = v.cur[1]!, cur2 = v.cur[2]!
  const nx0 = v.next[0]!, nx1 = v.next[1]!, nx2 = v.next[2]!
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
      if (v.solid[i]) { nx0[i] = 0; nx1[i] = 0; nx2[i] = 0; continue }
      // Where did the gas now here come from? Wind, plus hot air rising (1/256 field cell).
      const sx = fx * 256 - wx, sy = fy * 256 - wy, sz = fz * 256 - rise(cur0[i]!)
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
      const w011 = ((256 - tx) * ty >> 8) * tz >> 8, w111 = (tx * ty >> 8) * tz >> 8
      const c000 = okx0 && oky0 && okz0, c100 = okx1 && oky0 && okz0, c010 = okx0 && oky1 && okz0, c110 = okx1 && oky1 && okz0
      const c001 = okx0 && oky0 && okz1, c101 = okx1 && oky0 && okz1, c011 = okx0 && oky1 && okz1, c111 = okx1 && oky1 && okz1
      // Neighbours for the blur (outside reads as 0).
      const ixp = fx + 1 < nx ? i + 1 : -1, ixm = fx > 0 ? i - 1 : -1
      const iyp = fy + 1 < ny ? i + nx : -1, iym = fy > 0 ? i - nx : -1
      const izp = fz + 1 < nz ? i + nxy : -1, izm = fz > 0 ? i - nxy : -1
      for (let k = 0; k < KINDS; k++) {
        const a = k === 0 ? cur0 : k === 1 ? cur1 : cur2
        const adv = ((c000 ? a[base0]! * w000 : 0) + (c100 ? a[base0 + 1]! * w100 : 0) +
          (c010 ? a[base0 + nx]! * w010 : 0) + (c110 ? a[base0 + nx + 1]! * w110 : 0) +
          (c001 ? a[base0 + nxy]! * w001 : 0) + (c101 ? a[base0 + nxy + 1]! * w101 : 0) +
          (c011 ? a[base0 + nxy + nx]! * w011 : 0) + (c111 ? a[base0 + nxy + nx + 1]! * w111 : 0)) >> 8
        const nb = ((ixp >= 0 ? a[ixp]! : 0) + (ixm >= 0 ? a[ixm]! : 0) + (iyp >= 0 ? a[iyp]! : 0) +
          (iym >= 0 ? a[iym]! : 0) + (izp >= 0 ? a[izp]! : 0) + (izm >= 0 ? a[izm]! : 0)) / 6 | 0
        // A little blur with the neighbours, then fade.
        let val = (adv * 3 + nb) >> 2
        val -= (val * FADE[k]! + 255) >> 8
        if (val < 24) val = 0
        ;(k === 0 ? nx0 : k === 1 ? nx1 : nx2)[i] = val
        if (val) any = true
      }
    }
    if (any) withGas.push(b)
  }
  return withGas
}

