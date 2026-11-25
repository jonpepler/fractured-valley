// The deterministic simulation step.
//
// Rules for lockstep:
// - State changes only inside step(), driven by the seed, the tick and the
//   commands scheduled for that tick. No wall-clock time, no Math.random.
// - Integer maths only. Every collection is walked in sorted order.
// - Commands are applied in (tick, peer, seq) order on every peer.

import { Grid, hashGrids } from './grid'
import { M, mat } from './materials'
import { chance, hash32 } from './rng'

export type Command =
  | { readonly kind: 'ignite'; readonly x: number; readonly y: number; readonly z: number }
  | { readonly kind: 'drop'; readonly x: number; readonly y: number; readonly z: number }

export interface Scheduled {
  readonly tick: number
  readonly peer: number
  readonly seq: number
  readonly command: Command
}

export interface Body {
  readonly grid: Grid
  /** Downward speed in 1/16 cell per tick. */
  vz: number
  /** Sub-cell progress in 1/16 cell. */
  acc: number
}

export const pack = (x: number, y: number, z: number) => x | y << 10 | z << 20
export const unpack = (p: number): [number, number, number] => [p & 1023, (p >> 10) & 1023, p >>> 20]

const SPREAD: readonly [number, number, number][] = [
  [1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1],
  [1, 0, 1], [-1, 0, 1], [0, 1, 1], [0, -1, 1],
]
const N6: readonly [number, number, number][] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]]

export interface StepStats {
  burning: number
  loose: number
  bodies: number
  detached: number
  failed: number
}

export class Sim {
  tick = 0
  readonly bodies: Body[] = []
  private burning = new Set<number>()
  private loose = new Set<number>()
  private queue: Scheduled[] = []
  /** Cells whose removal may have left something unsupported. */
  private touched: number[] = []
  last: StepStats = { burning: 0, loose: 0, bodies: 0, detached: 0, failed: 0 }

  constructor(readonly world: Grid, readonly seed: number) {}

  get grids(): Grid[] {
    return [this.world, ...this.bodies.map(b => b.grid)]
  }

  schedule(s: Scheduled): void {
    if (s.tick < this.tick) throw new Error(`command for tick ${s.tick} arrived at ${this.tick}: too late for lockstep`)
    this.queue.push(s)
  }

  step(): void {
    const stats: StepStats = { burning: 0, loose: 0, bodies: 0, detached: 0, failed: 0 }
    this.applyCommands()
    this.stepFire(stats)
    this.stepGrowth()
    this.stepLoose()
    if (this.tick % 4 === 0) this.checkLoads(stats)
    this.checkConnectivity(stats)
    this.stepBodies()
    if (this.tick % 200 === 0) this.world.compact()
    stats.burning = this.burning.size
    stats.loose = this.loose.size
    stats.bodies = this.bodies.length
    this.last = stats
    this.tick++
  }

  hash(): number {
    let h = hashGrids(this.grids)
    h = hash32(h, this.tick)
    for (const b of this.bodies) h = hash32(h, b.vz, b.acc)
    return h
  }

  // ── commands ────────────────────────────────────────────────────────────

  private applyCommands() {
    const due = this.queue.filter(s => s.tick === this.tick)
    if (!due.length) return
    this.queue = this.queue.filter(s => s.tick !== this.tick)
    due.sort((a, b) => a.peer - b.peer || a.seq - b.seq)
    for (const { command: c } of due) {
      if (c.kind === 'ignite') {
        for (let dz = -1; dz <= 2; dz++) for (let dy = -1; dy <= 1; dy++) for (let dx = -1; dx <= 1; dx++)
          this.ignite(c.x + dx, c.y + dy, c.z + dz)
      } else {
        const g = new Grid(6, 6, 6)
        for (let z = 0; z < 6; z++) for (let y = 0; y < 6; y++) for (let x = 0; x < 6; x++) g.set(x, y, z, M.Metal)
        g.ox = c.x - 3; g.oy = c.y - 3; g.oz = c.z + 40
        this.bodies.push({ grid: g, vz: 0, acc: 0 })
      }
    }
  }

  private ignite(x: number, y: number, z: number): boolean {
    const w = this.world
    const m = mat(w.get(x, y, z))
    if (!m.flammable || w.getState(x, y, z) > 0) return false
    w.setState(x, y, z, 1)
    this.burning.add(pack(x, y, z))
    return true
  }

  // ── fire ────────────────────────────────────────────────────────────────

  private stepFire(stats: StepStats) {
    const w = this.world
    const cells = [...this.burning].sort((a, b) => a - b)
    for (const p of cells) {
      const [x, y, z] = unpack(p)
      const m = mat(w.get(x, y, z))
      const f = m.flammable
      const s = w.getState(x, y, z)
      if (!f || s === 0) { this.burning.delete(p); continue }
      for (let k = 0; k < SPREAD.length; k++) {
        const [dx, dy, dz] = SPREAD[k]!
        const nx = x + dx, ny = y + dy, nz = z + dz
        const nf = mat(w.get(nx, ny, nz)).flammable
        if (nf && w.getState(nx, ny, nz) === 0 && chance(hash32(this.seed, this.tick, pack(nx, ny, nz), k), nf.catchPerMille, 1000))
          this.ignite(nx, ny, nz)
      }
      if (s + 1 >= f.burnTicks) {
        this.burning.delete(p)
        w.set(x, y, z, f.becomes, 0)
        if (mat(f.becomes).loose) this.loose.add(p)
        this.touched.push(p)
        stats.failed += 0
      } else {
        w.setState(x, y, z, s + 1)
      }
    }
  }

  // ── growth ──────────────────────────────────────────────────────────────

  /** Samples random columns each tick: regrowth, spreading grass, decay. */
  private stepGrowth() {
    const w = this.world
    for (let k = 0; k < 220; k++) {
      const h = hash32(this.seed, this.tick, k, 0x6077)
      const x = h % w.sx
      const y = (h >>> 12) % w.sy
      let z = w.sz - 1
      while (z > 0 && w.get(x, y, z) === M.Air) z--
      const top = w.get(x, y, z)
      const r = hash32(h, 1) % 1000
      if (w.getState(x, y, z) > 0) continue
      if (top === M.Ash && r < 40) {
        w.set(x, y, z, M.Air)
        this.loose.delete(pack(x, y, z))
      } else if (top === M.Scorched && r < 60) {
        w.set(x, y, z, M.Dirt)
      } else if (top === M.Dirt && z + 1 < w.sz) {
        if (this.grassNear(x, y, z) && r < 300) w.set(x, y, z, M.Grass)
      } else if (top === M.Grass) {
        if (r < 25) w.set(x, y, z + 1, M.TallGrass)
        else if (r < 28) w.set(x, y, z + 1, M.Flower)
      } else if (top === M.TallGrass && w.get(x, y, z - 1) === M.TallGrass) {
        // already two blades high
      } else if (top === M.TallGrass && r < 15 && z + 1 < w.sz) {
        w.set(x, y, z + 1, M.TallGrass)
      }
    }
  }

  private grassNear(x: number, y: number, z: number): boolean {
    const w = this.world
    for (let dz = -1; dz <= 1; dz++)
      for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]] as const)
        if (w.get(x + dx, y + dy, z + dz) === M.Grass) return true
    return false
  }

  // ── loose material ──────────────────────────────────────────────────────

  private stepLoose() {
    const w = this.world
    const cells = [...this.loose].sort((a, b) => a - b)
    for (const p of cells) {
      const [x, y, z] = unpack(p)
      const m = w.get(x, y, z)
      if (!mat(m).loose) { this.loose.delete(p); continue }
      const canEnter = (nx: number, ny: number, nz: number) => w.inBounds(nx, ny, nz) && !mat(w.get(nx, ny, nz)).solid && w.get(nx, ny, nz) !== M.Water
      let to: number | undefined
      if (z > 0 && canEnter(x, y, z - 1)) to = pack(x, y, z - 1)
      else if (z > 0) {
        const start = hash32(this.seed, this.tick, p) % 4
        for (let k = 0; k < 4 && to === undefined; k++) {
          const [dx, dy] = ([[1, 0], [-1, 0], [0, 1], [0, -1]] as const)[(start + k) % 4]!
          if (canEnter(x + dx, y + dy, z - 1) && canEnter(x + dx, y + dy, z)) to = pack(x + dx, y + dy, z - 1)
        }
      }
      this.loose.delete(p)
      if (to === undefined) continue
      const [tx, ty, tz] = unpack(to)
      w.set(x, y, z, M.Air)
      w.set(tx, ty, tz, m)
      this.loose.add(to)
      // Whatever rested on this cell may now fall too.
      if (mat(w.get(x, y, z + 1)).loose) this.loose.add(pack(x, y, z + 1))
    }
  }

  // ── structure ───────────────────────────────────────────────────────────

  /** Is this cell part of a structure that could fall (solid, not ground)? */
  private isStructure(m: number): boolean {
    const d = mat(m)
    return d.solid && !d.terrain && !d.loose
  }

  /**
   * Burning structural cells lose capacity as they burn. Where the load
   * carried through a layer of a structure exceeds what its burning cells
   * can still bear, those cells give way.
   */
  private checkLoads(stats: StepStats) {
    const w = this.world
    const burningStructural = [...this.burning].sort((a, b) => a - b).filter(p => {
      const [x, y, z] = unpack(p)
      return mat(w.get(x, y, z)).structural !== undefined
    })
    const seen = new Set<number>()
    for (const start of burningStructural) {
      if (seen.has(start)) continue
      const comp = this.flood(start, 60000)
      for (const c of comp.cells) seen.add(c)
      if (comp.capped) continue
      // Mass carried through each layer, and how many cells share it.
      const byLayer = new Map<number, number[]>()
      for (const c of comp.cells) {
        const z = c >>> 20
        let l = byLayer.get(z)
        if (!l) byLayer.set(z, (l = []))
        l.push(c)
      }
      const layers = [...byLayer.keys()].sort((a, b) => b - a)
      let massAbove = 0
      for (const z of layers) {
        const cells = byLayer.get(z)!
        let layerMass = 0
        for (const c of cells) { const [x, y] = unpack(c); layerMass += mat(w.get(x, y, z)).density }
        const load = Math.floor(massAbove / cells.length)
        for (const c of cells) {
          const [x, y] = unpack(c)
          const m = mat(w.get(x, y, z))
          const s = w.getState(x, y, z)
          if (!m.structural || !m.flammable || s === 0) continue
          const remaining = m.flammable.burnTicks - s
          const capacity = Math.floor((m.structural.capacity * remaining) / m.flammable.burnTicks)
          if (load > capacity) {
            w.set(x, y, z, M.Ash, 0)
            this.burning.delete(c)
            this.loose.add(c)
            this.touched.push(c)
            stats.failed++
          }
        }
        massAbove += layerMass
      }
    }
  }

  /** Flood a structure; report whether it touches the ground. */
  private flood(start: number, limit: number): { cells: number[]; anchored: boolean; capped: boolean } {
    const w = this.world
    const seen = new Set<number>([start])
    const stack = [start]
    const cells: number[] = []
    let anchored = false
    while (stack.length) {
      const p = stack.pop()!
      cells.push(p)
      if (cells.length > limit) return { cells, anchored: true, capped: true }
      const [x, y, z] = unpack(p)
      if (z === 0) anchored = true
      for (const [dx, dy, dz] of N6) {
        const nx = x + dx, ny = y + dy, nz = z + dz
        if (!w.inBounds(nx, ny, nz)) continue
        const m = w.get(nx, ny, nz)
        if (mat(m).terrain) { anchored = true; continue }
        if (!this.isStructure(m)) continue
        const q = pack(nx, ny, nz)
        if (seen.has(q)) continue
        seen.add(q)
        stack.push(q)
      }
    }
    cells.sort((a, b) => a - b)
    return { cells, anchored, capped: false }
  }

  /** After removals, anything no longer connected to the ground detaches. */
  private checkConnectivity(stats: StepStats) {
    if (!this.touched.length) return
    const w = this.world
    const starts = new Set<number>()
    for (const p of this.touched) {
      const [x, y, z] = unpack(p)
      for (const [dx, dy, dz] of N6) {
        const nx = x + dx, ny = y + dy, nz = z + dz
        if (w.inBounds(nx, ny, nz) && this.isStructure(w.get(nx, ny, nz))) starts.add(pack(nx, ny, nz))
      }
    }
    this.touched = []
    const done = new Set<number>()
    for (const s of [...starts].sort((a, b) => a - b)) {
      if (done.has(s)) continue
      const comp = this.flood(s, 60000)
      for (const c of comp.cells) done.add(c)
      if (comp.anchored) continue
      this.detach(comp.cells)
      stats.detached++
    }
  }

  /** Lift a floating structure out of the world into its own grid. */
  private detach(cells: number[]) {
    const w = this.world
    let x0 = 1e9, y0 = 1e9, z0 = 1e9, x1 = -1, y1 = -1, z1 = -1
    for (const c of cells) {
      const [x, y, z] = unpack(c)
      x0 = Math.min(x0, x); y0 = Math.min(y0, y); z0 = Math.min(z0, z)
      x1 = Math.max(x1, x); y1 = Math.max(y1, y); z1 = Math.max(z1, z)
    }
    const g = new Grid(x1 - x0 + 1, y1 - y0 + 1, z1 - z0 + 1)
    g.ox = x0; g.oy = y0; g.oz = z0
    for (const c of cells) {
      const [x, y, z] = unpack(c)
      g.set(x - x0, y - y0, z - z0, w.get(x, y, z))
      w.set(x, y, z, M.Air)
      this.burning.delete(c)
    }
    this.bodies.push({ grid: g, vz: 0, acc: 0 })
  }

  // ── rigid bodies ────────────────────────────────────────────────────────

  private stepBodies() {
    const w = this.world
    const landed: Body[] = []
    for (const b of this.bodies) {
      b.vz = Math.min(64, b.vz + 3)
      b.acc += b.vz
      while (b.acc >= 16) {
        b.acc -= 16
        if (this.blocked(b.grid)) { landed.push(b); break }
        b.grid.oz--
      }
    }
    for (const b of landed) {
      const g = b.grid
      for (let z = 0; z < g.sz; z++) for (let y = 0; y < g.sy; y++) for (let x = 0; x < g.sx; x++) {
        const m = g.get(x, y, z)
        if (m !== M.Air) w.set(x + g.ox, y + g.oy, z + g.oz, m)
      }
      this.bodies.splice(this.bodies.indexOf(b), 1)
    }
  }

  /** Would moving this body down one cell hit something solid? */
  private blocked(g: Grid): boolean {
    const w = this.world
    for (let z = 0; z < g.sz; z++) for (let y = 0; y < g.sy; y++) for (let x = 0; x < g.sx; x++) {
      if (g.get(x, y, z) === M.Air) continue
      if (z > 0 && g.get(x, y, z - 1) !== M.Air) continue
      const wz = z + g.oz - 1
      if (wz < 0) return true
      const below = mat(w.get(x + g.ox, y + g.oy, wz))
      if (below.solid && !below.loose) return true
    }
    return false
  }
}
