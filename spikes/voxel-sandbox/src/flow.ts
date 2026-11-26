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
import { valueNoise2 } from './rng'
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
/** Strongest swirl pushed into warm air per tick (1/256 field cells per tick). */
const TURBULENCE = 90

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

/** The phases a worker can be asked to run (see field-pool.ts). Lives here, not in the worker module, because importing that module runs the worker. */
export enum Phase { Gas = 1, Forces = 2, Divergence = 3, Round = 4, Project = 5 }

// ── phases ────────────────────────────────────────────────────────────────
// Each phase is a pass over a set of bricks that reads shared state and
// writes only its own cells, so the bricks can be split across threads or
// peers with a barrier between phases (and between pressure rounds).

/** Mark (1) or unmark (0) the cells of these bricks as part of this step's region. */
export function flowMark(v: FlowView, bricks: readonly number[], on: 0 | 1) {
  forEachCell(v, bricks, i => { v.mask[i] = on })
}

/** Forces and self-advection: departure from the wind, into velNext. */
export function flowForces(v: FlowView, bricks: readonly number[], seed: number, tick: number) {
  const { nx, ny, nz } = v
  const [u0, v0, w0] = v.vel as [Int32Array, Int32Array, Int32Array]
  const [u1, v1, w1] = v.velNext as [Int32Array, Int32Array, Int32Array]
  const base = prevailing(seed, tick)
  const clamp = (x: number) => Math.max(-MAX_SPEED, Math.min(MAX_SPEED, x))
  for (const b of bricks) {
    const [ax, ay] = ambient(seed, tick, b % v.bx, Math.floor(b / v.bx) % v.by, base)
    forEachCell(v, [b], (i, fx, fy, fz) => {
      if (v.solid[i]) { u1[i] = -ax; v1[i] = -ay; w1[i] = 0; return } // no air moves inside a solid
      const sx = fx * 256 - (ax + u0[i]!), sy = fy * 256 - (ay + v0[i]!), sz = fz * 256 - w0[i]!
      u1[i] = clamp((sample(u0, nx, ny, nz, sx, sy, sz) * KEEP) >> 8)
      v1[i] = clamp((sample(v0, nx, ny, nz, sx, sy, sz) * KEEP) >> 8)
      w1[i] = clamp(((sample(w0, nx, ny, nz, sx, sy, sz) * KEEP) >> 8) + ((v.heat[i]! * BUOYANCY) >> 8))
      // Turbulence: warm air gets a small seeded swirl, slowly changing, which
      // the pressure step turns into rolling eddies. Plumes billow and widen.
      const heat = v.heat[i]!
      if (heat > 40) {
        const amp = Math.min(TURBULENCE, heat >> 2)
        const t = tick >> 2
        const n1 = valueNoise2(seed ^ 0x7a1, fx * 3 + fz * 7, fy * 3 + t * 5, 5) - 32768
        const n2 = valueNoise2(seed ^ 0x7a2, fy * 3 + fz * 5, fx * 3 + t * 3, 5) - 32768
        u1[i] = clamp(u1[i]! + ((n1 * amp) >> 15))
        v1[i] = clamp(v1[i]! + ((n2 * amp) >> 15))
      }
    })
  }
}

/** Neighbour index, or -1 outside the grid (open air). */
function nbr(v: FlowView, i: number, fx: number, fy: number, fz: number, d: number): number {
  const nxy = v.nx * v.ny
  return d === 0 ? (fx + 1 < v.nx ? i + 1 : -1) : d === 1 ? (fx > 0 ? i - 1 : -1)
    : d === 2 ? (fy + 1 < v.ny ? i + v.nx : -1) : d === 3 ? (fy > 0 ? i - v.nx : -1)
    : d === 4 ? (fz + 1 < v.nz ? i + nxy : -1) : (fz > 0 ? i - nxy : -1)
}

/** Divergence of the new velocity (outside the region reads as plain wind); zeroes both pressure buffers. */
export function flowDivergence(v: FlowView, bricks: readonly number[]) {
  const [u1, v1, w1] = v.velNext as [Int32Array, Int32Array, Int32Array]
  const m = v.mask
  const vin = (a: Int32Array, j: number) => (j >= 0 && m[j] ? a[j]! : 0)
  forEachCell(v, bricks, (i, fx, fy, fz) => {
    v.p[i] = 0; v.pNext[i] = 0
    if (v.solid[i]) { v.div[i] = 0; return }
    v.div[i] = ((vin(u1, nbr(v, i, fx, fy, fz, 0)) - vin(u1, nbr(v, i, fx, fy, fz, 1))) +
      (vin(v1, nbr(v, i, fx, fy, fz, 2)) - vin(v1, nbr(v, i, fx, fy, fz, 3))) +
      (vin(w1, nbr(v, i, fx, fy, fz, 4)) - vin(w1, nbr(v, i, fx, fy, fz, 5)))) >> 1
  })
}

/**
 * One Jacobi pressure round: reads `from`, writes `to`. Outside the region the
 * air is open (pressure 0); a solid neighbour reflects (reads as this cell).
 */
export function flowRound(v: FlowView, bricks: readonly number[], from: Int32Array, to: Int32Array) {
  const m = v.mask, solid = v.solid
  forEachCell(v, bricks, (i, fx, fy, fz) => {
    if (solid[i]) { to[i] = 0; return }
    const own = from[i]!
    let sum = 0
    for (let d = 0; d < 6; d++) {
      const j = nbr(v, i, fx, fy, fz, d)
      if (j >= 0 && m[j]) sum += solid[j] ? own : from[j]!
    }
    to[i] = Math.trunc((sum - v.div[i]!) / 6)
  })
}

/** Subtract the pressure gradient. Returns the bricks whose air still departs from the wind. */
export function flowProject(v: FlowView, bricks: readonly number[], p: Int32Array): number[] {
  const [u1, v1, w1] = v.velNext as [Int32Array, Int32Array, Int32Array]
  const m = v.mask, solid = v.solid
  const pAt = (j: number, own: number) => (j >= 0 && m[j] ? (solid[j] ? own : p[j]!) : 0)
  const moving: number[] = []
  for (const b of bricks) {
    let any = false
    forEachCell(v, [b], (i, fx, fy, fz) => {
      if (solid[i]) return
      const own = p[i]!
      u1[i]! -= (pAt(nbr(v, i, fx, fy, fz, 0), own) - pAt(nbr(v, i, fx, fy, fz, 1), own)) >> 1
      v1[i]! -= (pAt(nbr(v, i, fx, fy, fz, 2), own) - pAt(nbr(v, i, fx, fy, fz, 3), own)) >> 1
      w1[i]! -= (pAt(nbr(v, i, fx, fy, fz, 4), own) - pAt(nbr(v, i, fx, fy, fz, 5), own)) >> 1
      if (Math.abs(u1[i]!) + Math.abs(v1[i]!) + Math.abs(w1[i]!) > 48) any = true
    })
    if (any) moving.push(b)
  }
  return moving
}

/** The pressure buffer holding the result after all rounds. */
export const finalPressure = (v: FlowView) => (PRESSURE_ROUNDS % 2 === 0 ? v.p : v.pNext)

/**
 * Advance the velocity one tick over the given bricks (sorted), all phases in
 * order on this thread. Returns the bricks where the air still departs
 * noticeably from the wind.
 */
export function stepFlow(v: FlowView, bricks: readonly number[], seed: number, tick: number): number[] {
  flowMark(v, bricks, 1)
  flowForces(v, bricks, seed, tick)
  flowDivergence(v, bricks)
  for (let r = 0; r < PRESSURE_ROUNDS; r++) flowRound(v, bricks, r % 2 ? v.pNext : v.p, r % 2 ? v.p : v.pNext)
  const moving = flowProject(v, bricks, finalPressure(v))
  flowMark(v, bricks, 0)
  return moving
}
