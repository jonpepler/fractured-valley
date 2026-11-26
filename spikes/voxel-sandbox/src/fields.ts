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
const FADE = [22, 4, 8]
/** Gas can't get hotter than a flame (°C). */
const MAX_HEAT = 1100

export class Fields {
  readonly nx: number
  readonly ny: number
  readonly nz: number
  /** Current values, [kind][cell]. */
  cur: Uint16Array[]
  private next: Uint16Array[]
  /** Bricks (world brick index) that hold any gas. */
  active = new Set<number>()
  /** Field cells that are mostly solid: gas can't be there. */
  private solid: Uint8Array

  constructor(private readonly world: Grid) {
    this.nx = Math.ceil(world.sx / FR)
    this.ny = Math.ceil(world.sy / FR)
    this.nz = Math.ceil(world.sz / FR)
    const n = this.nx * this.ny * this.nz
    this.cur = Array.from({ length: KINDS }, () => new Uint16Array(n))
    this.next = Array.from({ length: KINDS }, () => new Uint16Array(n))
    this.solid = new Uint8Array(n)
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

  step(seed: number, tick: number) {
    const w = this.world
    // Bricks to compute: any with gas, and their neighbours (gas moves ≤ 1.2 field cells a tick).
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
    const base = prevailing(seed, tick)
    const nextActive = new Set<number>()
    for (const b of bricks) {
      const bx = b % w.bx, by = Math.floor(b / w.bx) % w.by, bz = Math.floor(b / (w.bx * w.by))
      if (w.version[b] !== this.solidAt[b]) this.refreshSolid(b, bx, by, bz)
      let any = false
      // Wind once per brick (gusts are ~5 m across; a brick is 1.6 m).
      const wind = windAt(seed, tick, bx * BRICK + 8, by * BRICK + 8, base)
      const wx = wind.x >> 1, wy = wind.y >> 1
      const { nx, ny, nz } = this
      const nxy = nx * ny
      const cur0 = this.cur[0]!, cur1 = this.cur[1]!, cur2 = this.cur[2]!
      const nx0 = this.next[0]!, nx1 = this.next[1]!, nx2 = this.next[2]!
      for (let lz = 0; lz < FB; lz++) for (let ly = 0; ly < FB; ly++) for (let lx = 0; lx < FB; lx++) {
        const fx = bx * FB + lx, fy = by * FB + ly, fz = bz * FB + lz
        if (fx >= nx || fy >= ny || fz >= nz) continue
        const i = (fz * ny + fy) * nx + fx
        if (this.solid[i]) { nx0[i] = 0; nx1[i] = 0; nx2[i] = 0; continue }
        // Where did the gas now here come from? (fixed point, 1/256 field cell)
        // Wind, plus hot air rising.
        const sx = fx * 256 - wx, sy = fy * 256 - wy, sz = fz * 256 - rise(cur0[i]!)
        const x0 = sx >> 8, y0 = sy >> 8, z0 = sz >> 8
        const tx = sx & 255, ty = sy & 255, tz = sz & 255
        // The 8 corners of the source cell, clamped to the field (outside reads as 0).
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
        // Neighbour indices for the blur (outside reads as 0).
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
          let v = (adv * 3 + nb) >> 2
          v -= (v * FADE[k]! + 255) >> 8
          if (v < 24) v = 0
          ;(k === 0 ? nx0 : k === 1 ? nx1 : nx2)[i] = v
          if (v) any = true
        }
      }
      if (any) nextActive.add(b)
    }
    // Swap. The old buffer becomes the spare: zero it where it held gas, so
    // the spare is all zeros at the start of every step.
    const t = this.cur; this.cur = this.next; this.next = t
    for (const b of this.active) this.clearBrick(this.next, b)
    this.active = nextActive
  }

  private solidAt = new Int32Array(0)

  private refreshSolid(b: number, bx: number, by: number, bz: number) {
    if (this.solidAt.length !== this.world.version.length) this.solidAt = new Int32Array(this.world.version.length).fill(-1)
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

  private val(a: Uint16Array, x: number, y: number, z: number): number {
    if (x < 0 || y < 0 || z < 0 || x >= this.nx || y >= this.ny || z >= this.nz) return 0
    return a[this.index(x, y, z)]!
  }

  /** Trilinear sample at a fixed-point position (1/256 field cell). */
  private sample(a: Uint16Array, sx: number, sy: number, sz: number): number {
    const x0 = sx >> 8, y0 = sy >> 8, z0 = sz >> 8
    const tx = sx & 255, ty = sy & 255, tz = sz & 255
    const lerp = (p: number, q: number, t: number) => p + (((q - p) * t) >> 8)
    const c00 = lerp(this.val(a, x0, y0, z0), this.val(a, x0 + 1, y0, z0), tx)
    const c10 = lerp(this.val(a, x0, y0 + 1, z0), this.val(a, x0 + 1, y0 + 1, z0), tx)
    const c01 = lerp(this.val(a, x0, y0, z0 + 1), this.val(a, x0 + 1, y0, z0 + 1), tx)
    const c11 = lerp(this.val(a, x0, y0 + 1, z0 + 1), this.val(a, x0 + 1, y0 + 1, z0 + 1), tx)
    return lerp(lerp(c00, c10, ty), lerp(c01, c11, ty), tz)
  }

  private neighbourMean(a: Uint16Array, x: number, y: number, z: number): number {
    return (this.val(a, x + 1, y, z) + this.val(a, x - 1, y, z) + this.val(a, x, y + 1, z) +
      this.val(a, x, y - 1, z) + this.val(a, x, y, z + 1) + this.val(a, x, y, z - 1)) / 6 | 0
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
