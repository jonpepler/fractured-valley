// Moving air: a velocity field for the gas grid (Stam's stable fluids, in
// integers). Velocity is stored as the *departure from the prevailing
// wind*, so still air costs nothing and the edge of the computed region
// stays clean. Each tick:
//
//   1. forces: hot air is pushed up (buoyancy); the departure decays, so air
//      far from a fire relaxes back to the wind;
//   2. the air carries its own motion along (semi-Lagrangian self-advection);
//   3. pressure: 20 Jacobi rounds find the pressure that stops air piling
//      up or vanishing, and its gradient is subtracted. This is what makes
//      rising hot air draw fresh air in at its base (a fire makes its own
//      wind) and makes air flow around trunks and walls instead of through.
//
// Every phase reads the previous buffer and writes only its own cell, with a
// fixed number of rounds: decision 2.7b, so regions can still be split.
// Units: velocity in 1/256 field cells per tick; heat in °C.
import { BRICK } from './grid'
import { prevailing, windAt } from './wind'

const FB = 8 // field cells per brick edge
/** Vertical push per tick from 1 °C of heat, in 1/65536 field cells per tick (×256 then >>8). */
const BUOYANCY = 40
/** How much of the departure from the wind survives each tick, out of 256. */
const KEEP = 248
/** Pressure rounds per tick (fixed, so every peer does the same work). */
export const PRESSURE_ROUNDS = 12
/** Speed limit, so gas never skips more than one brick boundary a tick. */
const MAX_SPEED = 400

export interface FlowView {
  nx: number; ny: number; nz: number
  bx: number; by: number
  solid: Uint8Array
  heat: Uint16Array
  /** Departure from the prevailing wind: [u, v, w] current and next. */
  vel: Int32Array[]
  velNext: Int32Array[]
  /** Scratch: 1 for field cells in the computed bricks (set and cleared here). */
  mask: Uint8Array
  /** Scratch: divergence and two pressure buffers. */
  div: Int32Array
  p: Int32Array
  pNext: Int32Array
}

/** Ambient wind for a brick, in the same units as the velocity field. */
export function ambient(seed: number, tick: number, bx: number, by: number, base = prevailing(seed, tick)): [number, number] {
  const w = windAt(seed, tick, bx * BRICK + 8, by * BRICK + 8, base)
  return [w.x >> 1, w.y >> 1]
}

function forEachCell(v: FlowView, bricks: readonly number[], f: (i: number, fx: number, fy: number, fz: number, b: number) => void) {
  for (const b of bricks) {
    const bx = b % v.bx, by = Math.floor(b / v.bx) % v.by, bz = Math.floor(b / (v.bx * v.by))
    for (let lz = 0; lz < FB; lz++) for (let ly = 0; ly < FB; ly++) for (let lx = 0; lx < FB; lx++) {
      const fx = bx * FB + lx, fy = by * FB + ly, fz = bz * FB + lz
      if (fx < v.nx && fy < v.ny && fz < v.nz) f((fz * v.ny + fy) * v.nx + fx, fx, fy, fz, b)
    }
  }
}

/** Trilinear sample at a fixed-point position (1/256 cell); outside reads 0. */
export function sample(a: Int32Array | Uint16Array, nx: number, ny: number, nz: number, sx: number, sy: number, sz: number): number {
  const x0 = sx >> 8, y0 = sy >> 8, z0 = sz >> 8
  const tx = sx & 255, ty = sy & 255, tz = sz & 255
  const nxy = nx * ny
  const okx0 = x0 >= 0 && x0 < nx, okx1 = x0 + 1 >= 0 && x0 + 1 < nx
  const oky0 = y0 >= 0 && y0 < ny, oky1 = y0 + 1 >= 0 && y0 + 1 < ny
  const okz0 = z0 >= 0 && z0 < nz, okz1 = z0 + 1 >= 0 && z0 + 1 < nz
  const i = z0 * nxy + y0 * nx + x0
  const c000 = okx0 && oky0 && okz0 ? a[i]! : 0, c100 = okx1 && oky0 && okz0 ? a[i + 1]! : 0
  const c010 = okx0 && oky1 && okz0 ? a[i + nx]! : 0, c110 = okx1 && oky1 && okz0 ? a[i + nx + 1]! : 0
  const c001 = okx0 && oky0 && okz1 ? a[i + nxy]! : 0, c101 = okx1 && oky0 && okz1 ? a[i + nxy + 1]! : 0
  const c011 = okx0 && oky1 && okz1 ? a[i + nxy + nx]! : 0, c111 = okx1 && oky1 && okz1 ? a[i + nxy + nx + 1]! : 0
  const x00 = c000 + (((c100 - c000) * tx) >> 8), x10 = c010 + (((c110 - c010) * tx) >> 8)
  const x01 = c001 + (((c101 - c001) * tx) >> 8), x11 = c011 + (((c111 - c011) * tx) >> 8)
  const y0v = x00 + (((x10 - x00) * ty) >> 8), y1v = x01 + (((x11 - x01) * ty) >> 8)
  return y0v + (((y1v - y0v) * tz) >> 8)
}

/**
 * Advance the velocity one tick over the given bricks (sorted). Returns the
 * bricks where the air still departs noticeably from the wind.
 */
export function stepFlow(v: FlowView, bricks: readonly number[], seed: number, tick: number): number[] {
  const { nx, ny, nz } = v
  const nxy = nx * ny
  const [u0, v0, w0] = v.vel as [Int32Array, Int32Array, Int32Array]
  const [u1, v1, w1] = v.velNext as [Int32Array, Int32Array, Int32Array]
  const base = prevailing(seed, tick)
  const winds = new Map<number, [number, number]>()
  for (const b of bricks) winds.set(b, ambient(seed, tick, b % v.bx, Math.floor(b / v.bx) % v.by, base))

  // 1–2. Forces and self-advection.
  forEachCell(v, bricks, (i, fx, fy, fz, b) => {
    const [ax, ay] = winds.get(b)!
    if (v.solid[i]) { u1[i] = -ax; v1[i] = -ay; w1[i] = 0; return } // no air moves inside a solid
    const sx = fx * 256 - (ax + u0[i]!), sy = fy * 256 - (ay + v0[i]!), sz = fz * 256 - w0[i]!
    const clamp = (x: number) => Math.max(-MAX_SPEED, Math.min(MAX_SPEED, x))
    u1[i] = clamp((sample(u0, nx, ny, nz, sx, sy, sz) * KEEP) >> 8)
    v1[i] = clamp((sample(v0, nx, ny, nz, sx, sy, sz) * KEEP) >> 8)
    w1[i] = clamp(((sample(w0, nx, ny, nz, sx, sy, sz) * KEEP) >> 8) + ((v.heat[i]! * BUOYANCY) >> 8))
  })

  // 3. Pressure. Outside the computed bricks the air is open (pressure 0);
  // a solid neighbour reflects (its pressure reads as this cell's).
  // A mask of computed cells and a flat list of them keep the rounds cheap.
  const cells: number[] = []
  forEachCell(v, bricks, i => { v.mask[i] = 1; cells.push(i) })
  const m = v.mask, solid = v.solid, div = v.div
  // Neighbour index or -1 when outside the grid (open air).
  const xs = new Int32Array(cells.length * 6)
  cells.forEach((i, k) => {
    const fx = i % nx, fy = Math.floor(i / nx) % ny, fz = Math.floor(i / nxy)
    xs[k * 6] = fx + 1 < nx ? i + 1 : -1; xs[k * 6 + 1] = fx > 0 ? i - 1 : -1
    xs[k * 6 + 2] = fy + 1 < ny ? i + nx : -1; xs[k * 6 + 3] = fy > 0 ? i - nx : -1
    xs[k * 6 + 4] = fz + 1 < nz ? i + nxy : -1; xs[k * 6 + 5] = fz > 0 ? i - nxy : -1
  })
  const vin = (a: Int32Array, j: number) => (j >= 0 && m[j] ? a[j]! : 0)
  for (let k = 0; k < cells.length; k++) {
    const i = cells[k]!
    if (solid[i]) { div[i] = 0; continue }
    div[i] = ((vin(u1, xs[k * 6]!) - vin(u1, xs[k * 6 + 1]!)) + (vin(v1, xs[k * 6 + 2]!) - vin(v1, xs[k * 6 + 3]!)) +
      (vin(w1, xs[k * 6 + 4]!) - vin(w1, xs[k * 6 + 5]!))) >> 1
  }
  let p = v.p, q = v.pNext
  for (const i of cells) { p[i] = 0; q[i] = 0 }
  for (let round = 0; round < PRESSURE_ROUNDS; round++) {
    for (let k = 0; k < cells.length; k++) {
      const i = cells[k]!
      if (solid[i]) { q[i] = 0; continue }
      const own = p[i]!
      let sum = 0
      for (let d = 0; d < 6; d++) {
        const j = xs[k * 6 + d]!
        if (j >= 0 && m[j]) sum += solid[j] ? own : p[j]!
      }
      q[i] = Math.trunc((sum - div[i]!) / 6)
    }
    const t = p; p = q; q = t
  }
  // Subtract the pressure gradient.
  const stillMoving: number[] = []
  const pAt = (j: number, own: number) => (j >= 0 && m[j] ? (solid[j] ? own : p[j]!) : 0)
  let k = 0, moving = false
  for (const b of bricks) {
    moving = false
    const bx = b % v.bx, by = Math.floor(b / v.bx) % v.by, bz = Math.floor(b / (v.bx * v.by))
    for (let lz = 0; lz < FB; lz++) for (let ly = 0; ly < FB; ly++) for (let lx = 0; lx < FB; lx++) {
      if (bx * FB + lx >= nx || by * FB + ly >= ny || bz * FB + lz >= nz) continue
      const i = cells[k]!
      const o = k * 6
      k++
      if (solid[i]) continue
      const own = p[i]!
      u1[i]! -= (pAt(xs[o]!, own) - pAt(xs[o + 1]!, own)) >> 1
      v1[i]! -= (pAt(xs[o + 2]!, own) - pAt(xs[o + 3]!, own)) >> 1
      w1[i]! -= (pAt(xs[o + 4]!, own) - pAt(xs[o + 5]!, own)) >> 1
      if (Math.abs(u1[i]!) + Math.abs(v1[i]!) + Math.abs(w1[i]!) > 48) moving = true
    }
    if (moving) stillMoving.push(b)
  }
  for (const i of cells) m[i] = 0
  return stillMoving
}
