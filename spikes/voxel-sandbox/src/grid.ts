// A sparse voxel grid of 10 cm cells, stored as 16³ bricks.
//
// A brick that is all one material with no state is stored as just that
// material id (no arrays), so solid ground and open air cost ~2 bytes per
// 4096 cells. Only bricks with detail hold typed arrays (struct of arrays).
//
// The world is a *list* of grids (decision 1.6): the static world is one,
// and each detached rigid body is a small grid with an integer offset.

export const BRICK = 16
export const BRICK_SHIFT = 4
export const BRICK_CELLS = BRICK * BRICK * BRICK

export class Brick {
  readonly material = new Uint8Array(BRICK_CELLS)
  readonly state = new Uint8Array(BRICK_CELLS)
}

/** Position and orientation of a moving body (world cells; quaternion). */
export interface Pose {
  x: number; y: number; z: number
  qx: number; qy: number; qz: number; qw: number
}

export class Grid {
  readonly bx: number
  readonly by: number
  readonly bz: number
  /** Detailed bricks, or null where the brick is uniform. */
  readonly bricks: (Brick | null)[]
  /** Material of each uniform brick (ignored where a Brick exists). */
  readonly uniform: Uint8Array
  /** Bumped whenever a brick changes, so views can cache per brick. */
  readonly version: Uint32Array
  /** Cached content hash per brick, valid while hashedAt[b] === version[b] + 1. */
  readonly brickHash: Uint32Array
  readonly hashedAt: Uint32Array
  /** When set, every changed cell is appended here (packed), for views. */
  changeLog: number[] | undefined
  /** Set while this grid is a moving rigid body. */
  pose: Pose | undefined
  /** Integer position of this grid in world cells (non-zero for bodies). */
  ox = 0
  oy = 0
  oz = 0

  constructor(readonly sx: number, readonly sy: number, readonly sz: number) {
    this.bx = Math.ceil(sx / BRICK)
    this.by = Math.ceil(sy / BRICK)
    this.bz = Math.ceil(sz / BRICK)
    const n = this.bx * this.by * this.bz
    this.bricks = new Array<Brick | null>(n).fill(null)
    this.uniform = new Uint8Array(n)
    this.version = new Uint32Array(n)
    this.brickHash = new Uint32Array(n)
    this.hashedAt = new Uint32Array(n)
  }

  inBounds(x: number, y: number, z: number): boolean {
    return x >= 0 && y >= 0 && z >= 0 && x < this.sx && y < this.sy && z < this.sz
  }

  brickIndex(x: number, y: number, z: number): number {
    return ((z >> BRICK_SHIFT) * this.by + (y >> BRICK_SHIFT)) * this.bx + (x >> BRICK_SHIFT)
  }

  static local(x: number, y: number, z: number): number {
    return (((z & 15) << 4 | (y & 15)) << 4) | (x & 15)
  }

  get(x: number, y: number, z: number): number {
    if (!this.inBounds(x, y, z)) return 0
    const b = this.brickIndex(x, y, z)
    const brick = this.bricks[b]
    return brick ? brick.material[Grid.local(x, y, z)]! : this.uniform[b]!
  }

  getState(x: number, y: number, z: number): number {
    if (!this.inBounds(x, y, z)) return 0
    const brick = this.bricks[this.brickIndex(x, y, z)]
    return brick ? brick.state[Grid.local(x, y, z)]! : 0
  }

  set(x: number, y: number, z: number, material: number, state = 0): void {
    if (!this.inBounds(x, y, z)) return
    const b = this.brickIndex(x, y, z)
    let brick = this.bricks[b]
    if (!brick) {
      if (this.uniform[b] === material && state === 0) return
      brick = new Brick()
      brick.material.fill(this.uniform[b]!)
      this.bricks[b] = brick
    }
    const i = Grid.local(x, y, z)
    if (brick.material[i] === material && brick.state[i] === state) return
    brick.material[i] = material
    brick.state[i] = state
    this.version[b]!++
    this.changeLog?.push(x | y << 10 | z << 20)
  }

  setState(x: number, y: number, z: number, state: number): void {
    this.set(x, y, z, this.get(x, y, z), state)
  }

  /** Fill a whole brick with one material without allocating. */
  fillBrick(b: number, material: number): void {
    if (this.changeLog) throw new Error('fillBrick is for generation only')
    this.bricks[b] = null
    this.uniform[b] = material
    this.version[b]!++
  }

  /** Collapse bricks that became uniform back to a single value. */
  compact(): number {
    let freed = 0
    for (let b = 0; b < this.bricks.length; b++) {
      const brick = this.bricks[b]
      if (!brick) continue
      const m = brick.material[0]!
      let isUniform = true
      for (let i = 0; i < BRICK_CELLS; i++) {
        if (brick.material[i] !== m || brick.state[i] !== 0) { isUniform = false; break }
      }
      if (isUniform) {
        this.bricks[b] = null
        this.uniform[b] = m
        freed++
      }
    }
    return freed
  }

  stats(): { bricks: number; detailed: number; bytes: number; cells: number } {
    let detailed = 0
    for (const brick of this.bricks) if (brick) detailed++
    const bricks = this.bricks.length
    return {
      bricks,
      detailed,
      // two typed arrays per detailed brick + the uniform/version tables
      bytes: detailed * BRICK_CELLS * 2 + bricks * (1 + 4 + 8),
      cells: this.sx * this.sy * this.sz,
    }
  }
}

const FNV = 0x811c9dc5
const mix = (h: number, v: number) => Math.imul(h ^ v, 0x01000193) >>> 0

// Hash of a brick filled with one material, exactly as if it were detailed,
// so the hash describes the world, not how compactly a peer happens to store it.
const uniformHash = new Map<number, number>()
function hashUniform(u: number): number {
  let h = uniformHash.get(u)
  if (h === undefined) {
    h = FNV
    for (let i = 0; i < BRICK_CELLS; i++) h = mix(h, u)
    uniformHash.set(u, h)
  }
  return h
}

function hashBrick(g: Grid, b: number): number {
  // Valid until the brick's version moves. compact() keeps the version but
  // not the content's meaning, so it can't make a cached hash stale.
  if (g.hashedAt[b] === g.version[b]! + 1) return g.brickHash[b]!
  const brick = g.bricks[b]
  let h: number
  if (!brick) h = hashUniform(g.uniform[b]!)
  else {
    h = FNV
    const m = brick.material, s = brick.state
    for (let i = 0; i < BRICK_CELLS; i++) h = mix(h, m[i]! | (s[i]! << 8))
  }
  g.brickHash[b] = h
  g.hashedAt[b] = g.version[b]! + 1
  return h
}

/**
 * The desync check: a hash of every grid's cells in a fixed order.
 * Each brick's hash is cached until that brick changes, so the cost follows
 * what changed since the last check, not the size of the world.
 */
export function hashGrids(grids: readonly Grid[]): number {
  let h = FNV
  for (const g of grids) {
    h = mix(h, g.ox); h = mix(h, g.oy); h = mix(h, g.oz); h = mix(h, g.sx); h = mix(h, g.sy); h = mix(h, g.sz)
    for (let b = 0; b < g.bricks.length; b++) h = mix(h, hashBrick(g, b))
  }
  return h
}
