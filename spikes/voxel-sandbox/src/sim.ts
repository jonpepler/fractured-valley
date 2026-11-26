// The deterministic simulation step.
//
// Rules for lockstep:
// - State changes only inside step(), driven by the seed, the tick and the
//   commands scheduled for that tick. No wall-clock time, no Math.random.
// - Integer maths only. Every collection is walked in sorted order.
// - Commands are applied in (tick, peer, seq) order on every peer.

import { type AirState, F, FR, Fields } from './fields'
import { prevailing } from './wind'
import { Grid, hashGrids } from './grid'
import { M, mat } from './materials'
import { cellCentre, Physics, poseBits, supports } from './physics'
import { chance, hash32 } from './rng'

export type Command =
  | { readonly kind: 'ignite'; readonly x: number; readonly y: number; readonly z: number }
  | { readonly kind: 'drop'; readonly x: number; readonly y: number; readonly z: number }
  /** A metal cube thrown from (x, y, z) at (vx, vy, vz) cells per second. */
  | { readonly kind: 'launch'; readonly x: number; readonly y: number; readonly z: number; readonly vx: number; readonly vy: number; readonly vz: number }

/** Broken-off pieces smaller than this fall as debris rather than as rigid bodies. */
export const SMALL_PIECE = 8
/** The most cells a loose pile (ash) can hold up; bigger structures fall through it. */
const LOOSE_CAN_HOLD = 32
/** Air hotter than this (°C) dries fuel next to it. */
const DRYING_HEAT = 100
const MAX_EMBERS = 150
/** Temperature (°C) of the air touching a burning, dry surface. */
const SURFACE_GLOW = 700
const GLOW_LOST_PER_FINENESS = 140
/** How much faster fuel gives off vapour in the hottest air than in air at its own glow (×1 + this). */
const FEEDBACK_MAX = 8
/** Field cells either side counted for feedback (2 → a 1 m cube). */
const FEEDBACK_RADIUS = 2
/** Air this hot (°C) counts as flame for feedback. */
const FLAME_HEAT = 800
/** Downward pull on an ember per tick (1/256 cells per tick²). */
const EMBER_GRAVITY = 6
/** Which side of a burning cell its heat goes to: up first, then around. */
const OPEN_ORDER: readonly [number, number, number][] = [[0, 0, 1], [1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, -1]]

/** An ember in flight: position and velocity in 1/256 cells (per tick). */
export interface Ember { x: number; y: number; z: number; vx: number; vy: number; vz: number; life: number }
/** Velocity change in one tick (1/16 cells/s) that counts as an impact: 24 cells/s. */
const IMPACT_DV = 24 * 16
/** How much harder a cell is to smash than to crush slowly (× load capacity). */
const IMPACT_TOUGHNESS = 1

export interface Scheduled {
  readonly tick: number
  readonly peer: number
  readonly seq: number
  readonly command: Command
}

export interface Body {
  readonly grid: Grid
  /** Consecutive ticks it has been at rest; it settles into the world after a few. */
  resting: number
  /** Ticks since it started moving: anything still rocking after 30 s settles anyway. */
  age: number
}

export const pack = (x: number, y: number, z: number) => x | y << 10 | z << 20
export const unpack = (p: number): [number, number, number] => [p & 1023, (p >> 10) & 1023, p >>> 20]

const SPREAD: readonly [number, number, number][] = [
  [1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1],
  [1, 0, 1], [-1, 0, 1], [0, 1, 1], [0, -1, 1],
]
const N6: readonly [number, number, number][] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]]

/** Notable moments, kept for inspection and tests (pillar 6). */
export interface SimEvent {
  readonly tick: number
  readonly kind: 'gave-way' | 'detached' | 'crumbled' | 'landed' | 'smashed' | 'ember-lit'
  readonly cells: number
  readonly at: readonly [number, number, number]
}

export interface StepStats {
  burning: number
  loose: number
  bodies: number
  detached: number
  failed: number
}

export interface SimState {
  tick: number
  embers: Ember[]
  gas: { brick: number; values: number[] }[]
  air: AirState
  burning: number[]
  loose: number[]
  debris: number[]
  touched: number[]
  queue: Scheduled[]
}

export class Sim {
  tick = 0
  readonly bodies: Body[] = []
  private burning = new Set<number>()
  private loose = new Set<number>()
  /** Cells of small broken-off pieces, falling straight down whatever their material. */
  private debris = new Set<number>()
  private queue: Scheduled[] = []
  /** Cells whose removal may have left something unsupported. */
  private touched: number[] = []
  /** Velocity given to pieces that break off during an impact this tick. */
  private push: { x: number; y: number; z: number } | undefined
  private pushAt = -1
  last: StepStats = { burning: 0, loose: 0, bodies: 0, detached: 0, failed: 0 }
  readonly events: SimEvent[] = []
  readonly physics = new Physics()
  /** Heat, soot and steam in the air. */
  readonly fields: Fields
  private hotScratch = new Uint16Array(0)
  /** Glowing embers in flight (simulation state: they can start fires). */
  embers: Ember[] = []

  /** How many of each event there has ever been (the log above keeps only the latest). */
  readonly eventCounts: Partial<Record<SimEvent['kind'], number>> = {}

  private note(kind: SimEvent['kind'], cells: number, at: number) {
    this.eventCounts[kind] = (this.eventCounts[kind] ?? 0) + 1
    this.events.push({ tick: this.tick, kind, cells, at: unpack(at) })
    if (this.events.length > 500) this.events.shift()
  }

  /**
   * How the gas fields advance each tick. Defaults to the fields' own step;
   * a worker pool (or peers) can take it over with identical results.
   */
  fieldStep: (seed: number, tick: number) => void

  constructor(readonly world: Grid, readonly seed: number, opts: { sharedFields?: boolean; airFlow?: boolean } = {}) {
    this.fields = new Fields(world, opts.sharedFields ?? false, opts.airFlow ?? true)
    this.fieldStep = (s, t) => this.fields.step(s, t)
  }

  /** Everything besides the world grid that carries from one tick to the next. */
  exportState(): SimState {
    if (this.bodies.length || this.tick - this.pushAt <= 1) throw new Error('checkpoint only while nothing is moving')
    const sorted = (set: Set<number>) => [...set].sort((a, b) => a - b)
    return {
      tick: this.tick,
      embers: this.embers.map(e => ({ ...e })),
      gas: this.fields.exportActive(),
      air: this.fields.exportAir(),
      burning: sorted(this.burning),
      loose: sorted(this.loose),
      debris: sorted(this.debris),
      touched: [...this.touched],
      queue: [...this.queue].sort((a, b) => a.tick - b.tick || a.peer - b.peer || a.seq - b.seq),
    }
  }

  importState(s: SimState): void {
    this.tick = s.tick
    this.embers = s.embers.map(e => ({ ...e }))
    this.fields.importActive(s.gas)
    this.fields.importAir(s.air)
    this.burning = new Set(s.burning)
    this.loose = new Set(s.loose)
    this.debris = new Set(s.debris)
    this.touched = [...s.touched]
    this.queue = [...s.queue]
  }

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
    this.fieldStep(this.seed, this.tick)
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
    h = this.fields.hash(h, (a, v) => hash32(a, v))
    for (const e of this.embers) h = hash32(hash32(h, e.x, e.y, e.z), e.vx, e.vy, e.vz ^ (e.life << 16))
    for (const b of this.bodies) for (const v of poseBits(b.grid.pose!)) h = hash32(h, v, b.resting)
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
        // A flame held there: a burst of heat, and anything dry enough catches.
        // Wet things (a living trunk) only start drying.
        for (let dz = -1; dz <= 2; dz++) for (let dy = -1; dy <= 1; dy++) for (let dx = -1; dx <= 1; dx++) {
          const x = c.x + dx, y = c.y + dy, z = c.z + dz
          const fl = mat(this.world.get(x, y, z)).flammable
          if (fl && this.world.getMoisture(x, y, z) <= fl.extinction) this.ignite(x, y, z)
        }
        this.fields.emit(c.x, c.y, c.z + 1, 900, 0, 0)
      } else {
        const g = new Grid(6, 6, 6)
        for (let z = 0; z < 6; z++) for (let y = 0; y < 6; y++) for (let x = 0; x < 6; x++) g.set(x, y, z, M.Metal)
        g.ox = c.x - 3; g.oy = c.y - 3; g.oz = c.kind === 'drop' ? c.z + 40 : c.z
        this.addBody(g, c.kind === 'launch' ? { x: c.vx, y: c.vy, z: c.vz } : undefined)
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
  //
  // Fire is heat moving through the gas fields (src/fields.ts):
  // - a burning cell heats the air next to it and gives off soot;
  // - hot air dries the fuel it touches (steam is the water leaving), and
  //   fuel dry enough and hot enough catches;
  // - wind and rising hot air carry heat on, so fire runs downwind and up;
  // - fine fuel throws embers that ride the plume and can light what they hit.
  // Burn progress lives in the cell's state byte, 1..255.

  private stepFire(stats: StepStats) {
    const w = this.world, f = this.fields
    // 1. Burning cells: advance, heat the air, throw embers, burn out.
    for (const p of [...this.burning].sort((a, b) => a - b)) {
      const [x, y, z] = unpack(p)
      const m = mat(w.get(x, y, z))
      const fl = m.flammable
      const s = w.getState(x, y, z)
      if (!fl || s === 0) { this.burning.delete(p); continue }
      // Heat goes into the air: above if open, else beside.
      const [ax, ay, az] = this.openSide(x, y, z)
      // Water still in the fuel boils off as it burns: green leaves give white
      // smoke (steam), dry wood dark smoke (soot).
      // While there is water left, burning spends its heat boiling it off
      // (about one point every other tick) and makes no progress.
      const wet = w.getMoisture(x, y, z)
      const h0 = hash32(this.seed, this.tick, p, 0xb1)
      const boil = wet && (h0 & 1) ? 1 : 0
      if (boil) w.setMoisture(x, y, z, wet - 1)
      // A quarter of the energy is direct heat from glowing char; the rest
      // leaves as burnable vapour, which burns in the air as flame.
      // Feedback: fuel inside a big fire is heated by radiation from all the
      // flame around it, so it gives off vapour faster, up to ×(1 + FEEDBACK_MAX),
      // and burns through faster to match. What counts is how much of the
      // space around (about 1 m across) is at flame heat: a thin grass fire or
      // a lone log fills little of it and stays near ×1; a bonfire or a
      // burning crown fills most of it. Thick fuel feels this most: it burns
      // only as fast as heat soaks in, while thin blades and leaves burn in
      // their own flame either way, so the gain is divided by fineness.
      const engulfed = f.shareAtLeast(F.Heat, ax, ay, az, FEEDBACK_RADIUS, FLAME_HEAT) // 0..256
      const feed = 256 + (((engulfed * engulfed * FEEDBACK_MAX) >> 8) / fl.fineness | 0) // ×1 … ×(1 + FEEDBACK_MAX / fineness), out of 256
      f.emit(ax, ay, az, fl.heat >> 2, fl.soot >> 1, boil * 240, ((fl.heat >> 3) * feed) >> 8)
      // Once dry, the burning surface glows and the air touching it is that
      // hot, so its own vapour lights (a lone log keeps burning).
      // Thick fuel chars and glows; fine fuel (blades, leaves) has almost no
      // char to glow, so its surface adds nothing beyond its own flame.
      if (!boil) f.warm(ax, ay, az, Math.max(0, SURFACE_GLOW - (fl.fineness - 1) * GLOW_LOST_PER_FINENESS))
      const h = hash32(this.seed, this.tick, p, 0xb0)
      if (this.embers.length < MAX_EMBERS && fl.fineness >= 5 && chance(h >>> 8, fl.fineness, 40000)) {
        // An ember leaves upward with a little random push.
        const r = (k: number) => ((h >>> k) & 31) - 16
        this.embers.push({ x: ax * 256 + 128, y: ay * 256 + 128, z: az * 256 + 128, vx: r(0) * 3, vy: r(5) * 3, vz: 120 + ((h >>> 10) & 63), life: 60 + ((h >>> 16) & 63) })
      }
      // Progress 1→255 over burnTicks, advanced by chance so any duration fits a byte.
      if (!wet && chance(h, (255 * feed) >> 8, fl.burnTicks)) {
        if (s + 1 >= 255) {
          this.burning.delete(p)
          w.set(x, y, z, fl.becomes, 0, 0)
          if (mat(fl.becomes).loose) this.loose.add(p)
          this.touched.push(p)
        } else w.setState(x, y, z, s + 1)
      }
    }
    // 2. Fuel next to hot gas dries, and catches if dry and hot enough.
    // Fuel cell → hottest air next to it, in a flat array indexed by cell
    // (a Map here cost ~40 ms a tick in a big fire).
    const n = w.sx * w.sy * w.sz
    if (this.hotScratch.length !== n) this.hotScratch = new Uint16Array(n)
    const hotAt = this.hotScratch
    const hotList: number[] = []
    for (const [fx, fy, fz, heat] of f.cells()) {
      if (heat < DRYING_HEAT) continue
      for (let dz = -1; dz <= FR; dz++) for (let dy = -1; dy <= FR; dy++) for (let dx = -1; dx <= FR; dx++) {
        const x = fx * FR + dx, y = fy * FR + dy, z = fz * FR + dz
        if (x < 0 || y < 0 || z < 0 || x >= w.sx || y >= w.sy || z >= w.sz) continue
        const idx = (z * w.sy + y) * w.sx + x
        if (hotAt[idx]! >= heat) continue
        if (!mat(w.get(x, y, z)).flammable) continue
        if (!hotAt[idx]) hotList.push(pack(x, y, z))
        hotAt[idx] = heat
      }
    }
    const hot: [number, number][] = hotList.sort((a, b) => a - b).map(p => {
      const [x, y, z] = unpack(p)
      const idx = (z * w.sy + y) * w.sx + x
      const heat = hotAt[idx]!
      hotAt[idx] = 0
      return [p, heat]
    })
    for (const [p, heat] of hot) {
      const [x, y, z] = unpack(p)
      const fl = mat(w.get(x, y, z)).flammable!
      if (w.getState(x, y, z) > 0) continue
      let wet = w.getMoisture(x, y, z)
      if (wet > 0) {
        // Drying: faster for hotter air and finer fuel. The water leaves as steam.
        const rate = ((heat - DRYING_HEAT) * fl.fineness) >> 6
        const h = hash32(this.seed, this.tick, p, 0xd1)
        const loss = Math.min(wet, (rate >> 8) + (chance(h, rate & 255, 256) ? 1 : 0))
        if (loss) {
          wet -= loss
          w.setMoisture(x, y, z, wet)
          const [ax, ay, az] = this.openSide(x, y, z)
          f.emit(ax, ay, az, 0, 0, loss * 90)
        }
      }
      if (wet <= fl.extinction && heat >= fl.ignitesAt) {
        // The hotter and finer, the likelier it lights this tick.
        const odds = Math.min(1000, 1 + (((heat - fl.ignitesAt) * fl.fineness) >> 6))
        if (chance(hash32(this.seed, this.tick, p, 0x1a), odds, 1000)) this.ignite(x, y, z)
      }
    }
    // 3. Embers ride the gas: wind plus updraft, with drag and gravity.
    const base = prevailing(this.seed, this.tick)
    const kept: Ember[] = []
    for (const e of this.embers) {
      const wx = e.x >> 8, wy = e.y >> 8, wz = e.z >> 8
      const heat = f.at(F.Heat, wx, wy, wz)
      const [gx, gy, gz] = f.velocity(this.seed, this.tick, Math.floor(wx / FR), Math.floor(wy / FR), Math.floor(wz / FR), heat, base)
      // Field velocity is per field cell; ×FR for world cells.
      e.vx += ((gx * FR - e.vx) >> 3); e.vy += ((gy * FR - e.vy) >> 3); e.vz += ((gz * FR - e.vz) >> 3) - EMBER_GRAVITY
      const nx = e.x + e.vx, ny = e.y + e.vy, nz = e.z + e.vz
      e.life--
      const tx = nx >> 8, ty = ny >> 8, tz = nz >> 8
      if (!w.inBounds(tx, ty, tz) || e.life <= 0) continue
      const m = mat(w.get(tx, ty, tz))
      if (m.solid || m.flammable) {
        // Landed: fine, dry fuel may catch.
        const fl = m.flammable
        if (fl && w.getState(tx, ty, tz) === 0 && w.getMoisture(tx, ty, tz) <= fl.extinction &&
          chance(hash32(this.seed, this.tick, pack(tx, ty, tz), 0xe3), fl.fineness * 30, 1000)) {
          this.ignite(tx, ty, tz)
          this.note('ember-lit', 1, pack(tx, ty, tz))
        }
        continue
      }
      e.x = nx; e.y = ny; e.z = nz
      kept.push(e)
    }
    this.embers = kept
    stats.burning = this.burning.size
  }

  /** The air cell a burning cell heats: above if open, else the first open side. */
  private openSide(x: number, y: number, z: number): [number, number, number] {
    const w = this.world
    for (const [dx, dy, dz] of OPEN_ORDER) if (!mat(w.get(x + dx, y + dy, z + dz)).solid) return [x + dx, y + dy, z + dz]
    return [x, y, z + 1]
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
        w.set(x, y, z, M.Dirt, 0, 0)
      } else if (top === M.Dirt && z + 1 < w.sz) {
        if (this.grassNear(x, y, z) && r < 300) w.set(x, y, z, M.Grass, 0, mat(M.Grass).moisture)
      } else if (top === M.Grass) {
        if (r < 25) w.set(x, y, z + 1, M.TallGrass, 0, mat(M.TallGrass).moisture)
        else if (r < 28) w.set(x, y, z + 1, M.Flower, 0, mat(M.Flower).moisture)
      } else if (top === M.TallGrass && w.get(x, y, z - 1) === M.TallGrass) {
        // already two blades high
      } else if (top === M.TallGrass && r < 15 && z + 1 < w.sz) {
        w.set(x, y, z + 1, M.TallGrass, 0, mat(M.TallGrass).moisture)
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
      const isDebris = this.debris.has(p)
      if (!mat(m).loose && !isDebris) { this.loose.delete(p); continue }
      const canEnter = (nx: number, ny: number, nz: number) => w.inBounds(nx, ny, nz) && !mat(w.get(nx, ny, nz)).solid && w.get(nx, ny, nz) !== M.Water
      let to: number | undefined
      if (z > 0 && canEnter(x, y, z - 1)) to = pack(x, y, z - 1)
      else if (z > 0 && !isDebris) {
        // Loose material (ash) also slides off slopes; debris just stops.
        const start = hash32(this.seed, this.tick, p) % 4
        for (let k = 0; k < 4 && to === undefined; k++) {
          const [dx, dy] = ([[1, 0], [-1, 0], [0, 1], [0, -1]] as const)[(start + k) % 4]!
          if (canEnter(x + dx, y + dy, z - 1) && canEnter(x + dx, y + dy, z)) to = pack(x + dx, y + dy, z - 1)
        }
      }
      this.loose.delete(p)
      this.debris.delete(p)
      if (to === undefined) continue
      const [tx, ty, tz] = unpack(to)
      const st = w.getState(x, y, z)
      w.set(x, y, z, M.Air)
      w.set(tx, ty, tz, m, st)
      this.loose.add(to)
      if (isDebris) this.debris.add(to)
      // A burning ember keeps burning as it falls.
      if (this.burning.delete(p)) this.burning.add(to)
      // Whatever rested on this cell may now fall too.
      const above = w.get(x, y, z + 1)
      if (mat(above).loose) this.loose.add(pack(x, y, z + 1))
      else if (this.isStructure(above)) this.touched.push(p)
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
      let gaveWay = false
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
          // Burn progress is 1..255: what's left of the cell carries proportionally less.
          const capacity = Math.floor((m.structural.capacity * (255 - s)) / 255)
          if (load > capacity) {
            w.set(x, y, z, M.Ash, 0)
            this.burning.delete(c)
            this.loose.add(c)
            this.touched.push(c)
            stats.failed++
            this.note('gave-way', 1, c)
            gaveWay = true
          }
        }
        massAbove += layerMass
      }
      // Once it gives way, charred parts of the same structure shatter
      // rather than riding down on what's left (no toppling yet: 1.5).
      if (gaveWay) {
        for (const c of comp.cells) {
          const [x, y, z] = unpack(c)
          const m = mat(w.get(x, y, z))
          const s = w.getState(x, y, z)
          if (!m.structural || !m.flammable || s < 128) continue
          w.set(x, y, z, M.Ash, 0)
          this.burning.delete(c)
          this.loose.add(c)
          this.touched.push(c)
        }
      }
    }
  }

  /** Flood a structure; report whether it touches the ground. */
  private flood(start: number, limit: number): { cells: number[]; anchored: boolean; capped: boolean } {
    const w = this.world
    const seen = new Set<number>([start])
    const stack = [start]
    const cells: number[] = []
    let anchored = false, onLoose = false
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
        const md = mat(m)
        if (md.terrain) { anchored = true; continue }
        // Lying on a pile of loose material (ash, say) is supported too;
        // if the pile moves, stepLoose re-checks whatever was on it.
        if (dz === -1 && md.loose && md.solid) { onLoose = true; continue }
        if (!this.isStructure(m)) continue
        const q = pack(nx, ny, nz)
        if (seen.has(q)) continue
        seen.add(q)
        stack.push(q)
      }
    }
    cells.sort((a, b) => a - b)
    // A small crumb can rest on a loose pile (ash), but a pile can't hold up a
    // big structure: a post burnt to an ash column must not keep its block
    // aloft. (Stopgap until bonds, brief 05 decision 5, decide this properly.)
    if (!anchored && onLoose && cells.length <= LOOSE_CAN_HOLD) anchored = true
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
      if (comp.cells.length < SMALL_PIECE) {
        // A crumb (a burnt-off twig or clump of leaves) isn't worth a rigid
        // body: it drops as debris, cell by cell, still burning if it was.
        for (const c of comp.cells) { this.loose.add(c); this.debris.add(c) }
        this.note('crumbled', comp.cells.length, comp.cells[0]!)
        continue
      }
      this.detach(comp.cells)
      this.note('detached', comp.cells.length, comp.cells[0]!)
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
      g.set(x - x0, y - y0, z - z0, w.get(x, y, z), w.getState(x, y, z))
      w.set(x, y, z, M.Air)
      this.burning.delete(c)
    }
    this.addBody(g)
  }

  /**
   * Nothing breaks perfectly evenly: give each new body a small, seeded
   * twist so tall things can tip. Deterministic, so every peer agrees.
   */
  private addBody(g: Grid, velocity?: { x: number; y: number; z: number }) {
    const h = hash32(this.seed, this.tick, g.ox, g.oy ^ (g.oz << 10))
    const s = (k: number) => (((h >>> k) & 255) - 128) / 128
    const scale = g.sx * g.sy * g.sz * 0.15
    this.physics.add(g, { x: s(0) * scale, y: s(8) * scale, z: s(16) * scale * 0.2 }, velocity ?? (this.tick - this.pushAt <= 1 ? this.push : undefined))
    this.bodies.push({ grid: g, resting: 0, age: 0 })
  }

  // ── rigid bodies ────────────────────────────────────────────────────────

  private stepBodies() {
    if (!this.bodies.length) return
    const before = new Map(this.bodies.map(b => [b, this.physics.velocity(b.grid)] as const))
    this.physics.step(this.world, this.bodies.map(b => b.grid))
    for (const b of [...this.bodies]) {
      // A sudden change of velocity means it hit something hard.
      const [bx, by, bz] = before.get(b)!, [ax, ay, az] = this.physics.velocity(b.grid)
      const dv2 = (ax - bx) ** 2 + (ay - by) ** 2 + (az - bz) ** 2
      if (dv2 > IMPACT_DV * IMPACT_DV) this.impact(b, [bx, by, bz], Math.floor(Math.sqrt(dv2)))
    }
    for (const b of [...this.bodies]) {
      b.resting = this.physics.isResting(b.grid) ? b.resting + 1 : 0
      b.age++
      if (b.resting >= 8 || b.age > 600 || b.grid.pose!.z < -40) this.settle(b)
    }
    if (!this.bodies.length) this.physics.reset()
  }

  /**
   * Impact: the body's momentum change (mass × Δv) is shared among the
   * structure cells just ahead of it along its old velocity. A cell whose
   * share exceeds its strength breaks into falling splinters; the structure
   * check then decides what falls (a cut trunk takes its tree with it).
   * Terrain and stone are far too strong to break this way.
   */
  private impact(b: Body, v: [number, number, number], dv: number) {
    const w = this.world, g = b.grid, pose = g.pose!
    const len = Math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) || 1
    const ux = v[0] / len, uy = v[1] / len, uz = v[2] / len
    let mass = 0
    const hit = new Set<number>()
    for (let z = 0; z < g.sz; z++) for (let y = 0; y < g.sy; y++) for (let x = 0; x < g.sx; x++) {
      const m = g.get(x, y, z)
      if (m === M.Air) continue
      mass += mat(m).density
      const [cx, cy, cz] = cellCentre(pose, x, y, z)
      // The first structure cell within 3 cells ahead (round things curve away).
      for (const reach of [0.6, 1.2, 1.8, 2.4, 3.0]) {
        const hx = Math.floor(cx + ux * reach), hy = Math.floor(cy + uy * reach), hz = Math.floor(cz + uz * reach)
        const m = w.get(hx, hy, hz)
        if (this.isStructure(m)) { hit.add(pack(hx, hy, hz)); break }
        if (m !== M.Air && mat(m).solid) break
      }
    }
    if (!hit.size) return
    const momentum = Math.floor((mass * dv) / 16) // mass units × cells/s
    const share = Math.floor(momentum / hit.size)
    let broke = 0
    // Each contact's share of the momentum punches on along the direction of
    // travel, breaking cells until it is spent: hard enough, and it cuts through.
    const strengthAt = (x: number, y: number, z: number) => {
      const m = mat(w.get(x, y, z))
      if (!m.structural || !this.isStructure(w.get(x, y, z))) return Infinity
      const s = w.getState(x, y, z) // burnt wood is weaker, as in the load check
      const f = m.flammable
      return Math.floor((m.structural.capacity * IMPACT_TOUGHNESS * (f && s > 0 ? 255 - s : 255)) / 255)
    }
    const first = [...hit].sort((a, b) => a - b)
    for (const c of first) {
      let [x, y, z] = unpack(c)
      let budget = share
      for (let depth = 0; depth < 12; depth++) {
        const st = strengthAt(x, y, z)
        if (st === Infinity) break
        if (budget <= st) break
        budget -= st
        // Smashed: the cell is gone (there's no splinter material yet).
        w.set(x, y, z, M.Air)
        this.burning.delete(pack(x, y, z))
        this.touched.push(pack(x, y, z))
        broke++
        x = Math.floor(x + 0.5 + ux); y = Math.floor(y + 0.5 + uy); z = Math.floor(z + 0.5 + uz)
      }
    }
    if (broke) {
      this.note('smashed', broke, [...hit][0]!)
      // Whatever breaks off this tick is shoved the way the body was going.
      const k = Math.min(1, dv / len) * 0.35
      this.push = { x: (v[0] / 16) * k, y: (v[1] / 16) * k, z: 0 }
      this.pushAt = this.tick
    }
  }

  /** Write a body back into the grid where it came to rest. */
  private settle(b: Body) {
    const w = this.world
    const g = b.grid
    const pose = g.pose!
    const cells: { m: number; s: number; x: number; y: number; z: number }[] = []
    for (let z = 0; z < g.sz; z++) for (let y = 0; y < g.sy; y++) for (let x = 0; x < g.sx; x++) {
      const m = g.get(x, y, z)
      if (m === M.Air) continue
      const [cx, cy, cz] = cellCentre(pose, x, y, z)
      cells.push({ m, s: g.getState(x, y, z), x: Math.floor(cx), y: Math.floor(cy), z: Math.floor(cz) })
    }
    // Snapping a rotated body to the grid can leave it hovering a cell up:
    // lower the whole shape until something it rests on is directly below.
    const blocked = (dz: number) => cells.some(c => c.z - dz < 0 || supports(w.get(c.x, c.y, c.z - dz)))
    let drop = 0
    while (drop < 8 && !blocked(drop + 1)) drop++
    let first = -1
    for (const c of cells) {
      let tz = c.z - drop
      // Rotation can map two cells to one; stack the extra on top, however
      // high that takes. Never overwrite: a body must not destroy material.
      while (w.inBounds(c.x, c.y, tz) && supports(w.get(c.x, c.y, tz))) tz++
      if (!w.inBounds(c.x, c.y, tz)) continue
      w.set(c.x, c.y, tz, c.m, c.s)
      if (c.s > 0 && mat(c.m).flammable) this.burning.add(pack(c.x, c.y, tz))
      if (first < 0) first = pack(c.x, c.y, tz)
    }
    this.physics.remove(g)
    this.bodies.splice(this.bodies.indexOf(b), 1)
    this.note('landed', cells.length, first < 0 ? 0 : first)
  }

}
