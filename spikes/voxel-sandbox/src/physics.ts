// Rigid bodies with rotation, through Rapier's deterministic build.
//
// Rapier "deterministic" compiles without SIMD and with a software maths
// library, so the same inputs give bit-identical results on every platform.
// We keep it deterministic by feeding it in a fixed order: static colliders
// are (re)built brick by brick in index order, bodies in creation order.
//
// The voxel grid stays the truth for everything at rest. Only things in
// motion live in Rapier; when a body comes to rest it is written back into
// the grid at its final pose (decision 1.5: rigid, heavy, can tip over).

import RAPIER from '@dimforge/rapier3d-deterministic-compat'
import { BRICK, type Grid, type Pose } from './grid'
import { M, mat } from './materials'

export const initPhysics = () => RAPIER.init()

/** 1 unit = one 10 cm cell; gravity is 9.81 m/s² = 98.1 cells/s². */
const GRAVITY = -98.1
const TICK_SECONDS = 1 / 20

export type { Pose }

/** Does this world cell hold bodies up? Loose ash and plants do not. */
export function supports(m: number): boolean {
  const d = mat(m)
  return d.solid && !d.loose
}

export class Physics {
  world: InstanceType<typeof RAPIER.World>
  private readonly statics = new Map<number, { collider: InstanceType<typeof RAPIER.Collider>; version: number }>()
  private readonly bodies = new Map<Grid, InstanceType<typeof RAPIER.RigidBody>>()

  constructor() {
    this.world = Physics.freshWorld()
  }

  private static freshWorld() {
    const w = new RAPIER.World({ x: 0, y: 0, z: GRAVITY })
    w.timestep = TICK_SECONDS
    return w
  }

  /**
   * Once nothing is moving, throw the physics world away. Every peer does
   * this at the same tick, so Rapier never carries history the voxel grid
   * doesn't show, its memory stays small, and a checkpoint taken while all
   * is still needs no physics state at all.
   */
  reset() {
    this.world.free()
    this.world = Physics.freshWorld()
    this.statics.clear()
    this.bodies.clear()
  }

  /** Make a grid a dynamic body, optionally with a small push. */
  add(g: Grid, impulse?: { x: number; y: number; z: number }) {
    const coords: number[] = []
    let mass = 0
    for (let z = 0; z < g.sz; z++) for (let y = 0; y < g.sy; y++) for (let x = 0; x < g.sx; x++) {
      const m = g.get(x, y, z)
      if (m === M.Air) continue
      coords.push(x, y, z)
      mass += mat(m).density
    }
    const rb = this.world.createRigidBody(RAPIER.RigidBodyDesc.dynamic().setTranslation(g.ox, g.oy, g.oz).setCcdEnabled(true))
    const density = mass / Math.max(1, coords.length / 3)
    this.world.createCollider(
      RAPIER.ColliderDesc.voxels(new Int32Array(coords), { x: 1, y: 1, z: 1 }).setDensity(density).setFriction(0.9).setRestitution(0.05),
      rb,
    )
    if (impulse) rb.applyTorqueImpulse(impulse, true)
    this.bodies.set(g, rb)
    g.pose = { x: g.ox, y: g.oy, z: g.oz, qx: 0, qy: 0, qz: 0, qw: 1 }
  }

  remove(g: Grid) {
    const rb = this.bodies.get(g)
    if (rb) this.world.removeRigidBody(rb)
    this.bodies.delete(g)
  }

  /**
   * Step the world. Static colliders exist only for bricks near moving
   * bodies, rebuilt when the brick has changed since.
   */
  step(world: Grid, moving: readonly Grid[]): void {
    const wanted = new Set<number>()
    for (const g of moving) {
      const p = g.pose!
      const r = Math.ceil(Math.hypot(g.sx, g.sy, g.sz)) + 4
      const lo = (v: number) => Math.max(0, Math.floor((v - r) / BRICK))
      const hi = (v: number, n: number) => Math.min(n - 1, Math.floor((v + r) / BRICK))
      for (let bz = lo(p.z); bz <= hi(p.z, world.bz); bz++)
        for (let by = lo(p.y); by <= hi(p.y, world.by); by++)
          for (let bx = lo(p.x); bx <= hi(p.x, world.bx); bx++) wanted.add((bz * world.by + by) * world.bx + bx)
    }
    for (const b of [...wanted].sort((a, c) => a - c)) this.ensureStatic(world, b)
    this.world.step()
    for (const g of moving) {
      const rb = this.bodies.get(g)!
      const t = rb.translation(), q = rb.rotation()
      g.pose = { x: t.x, y: t.y, z: t.z, qx: q.x, qy: q.y, qz: q.z, qw: q.w }
    }
  }

  /** Linear and angular speed, for inspection. */
  speeds(g: Grid): [number, number] {
    const rb = this.bodies.get(g)
    if (!rb) return [0, 0]
    const v = rb.linvel(), w = rb.angvel()
    return [Math.hypot(v.x, v.y, v.z), Math.hypot(w.x, w.y, w.z)]
  }

  isResting(g: Grid): boolean {
    const rb = this.bodies.get(g)
    if (!rb) return true
    if (rb.isSleeping()) return true
    const v = rb.linvel(), w = rb.angvel()
    return Math.hypot(v.x, v.y, v.z) < 1 && Math.hypot(w.x, w.y, w.z) < 0.25
  }

  private ensureStatic(world: Grid, b: number) {
    const have = this.statics.get(b)
    const version = world.version[b]!
    if (have && have.version === version) return
    if (have) this.world.removeCollider(have.collider, true)
    this.statics.delete(b)
    const bx = b % world.bx, by = Math.floor(b / world.bx) % world.by, bz = Math.floor(b / (world.bx * world.by))
    const coords: number[] = []
    for (let z = bz * BRICK; z < Math.min(world.sz, bz * BRICK + BRICK); z++)
      for (let y = by * BRICK; y < Math.min(world.sy, by * BRICK + BRICK); y++)
        for (let x = bx * BRICK; x < Math.min(world.sx, bx * BRICK + BRICK); x++)
          if (supports(world.get(x, y, z))) coords.push(x, y, z)
    // A floor under the world, so nothing falls forever.
    if (bz === 0) for (let y = by * BRICK; y < by * BRICK + BRICK; y++) for (let x = bx * BRICK; x < bx * BRICK + BRICK; x++) coords.push(x, y, -1)
    if (!coords.length) return
    const collider = this.world.createCollider(RAPIER.ColliderDesc.voxels(new Int32Array(coords), { x: 1, y: 1, z: 1 }).setFriction(0.9))
    this.statics.set(b, { collider, version })
  }
}

/** World position of the centre of a body's cell at its current pose. */
export function cellCentre(p: Pose, x: number, y: number, z: number): [number, number, number] {
  const vx = x + 0.5, vy = y + 0.5, vz = z + 0.5
  // v' = q v q*, expanded.
  const { qx, qy, qz, qw } = p
  const tx = 2 * (qy * vz - qz * vy), ty = 2 * (qz * vx - qx * vz), tz = 2 * (qx * vy - qy * vx)
  return [
    p.x + vx + qw * tx + (qy * tz - qz * ty),
    p.y + vy + qw * ty + (qz * tx - qx * tz),
    p.z + vz + qw * tz + (qx * ty - qy * tx),
  ]
}

/** Float bits of a pose, for the state hash. */
export function poseBits(p: Pose): number[] {
  return [...new Uint32Array(new Float32Array([p.x, p.y, p.z, p.qx, p.qy, p.qz, p.qw]).buffer)]
}
