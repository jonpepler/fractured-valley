// A software iso rasteriser that turns 10 cm voxels into pixel art.
//
// Every voxel is a fixed 4×4 pixel sprite (2:1 dimetric):
//
//    . T T .      T = top face
//    T T T T      L = the +y face (lower left)
//    L L R R      R = the +x face (lower right)
//    L L R R
//
// Stepping +x moves the sprite (+2,+1), +y moves it (-2,+1), +z moves it
// (0,-2). All integer, so the image is exact pixel art at native scale and
// identical on every machine for the same world (acceptance check 0.4).
// Nearer = larger x+y+z; a depth buffer resolves overlaps.
//
// Cost scales with change, not world size: finished pixels are cached per
// 4×4×4 unit of cells, and the grid's change log says which units to
// rebuild. A frame is a depth-tested blit plus the burning cells, which
// flicker and are drawn fresh.

import type { Grid } from './grid'
import { M, MATERIALS, mat } from './materials'
import { cellCentre } from './physics'
import { hash32, valueNoise2 } from './rng'
import { FR, type Fields } from './fields'
import type { Ember } from './sim'

/**
 * The colour grade: one place to tune the overall look.
 * desaturate: 0 = palette as authored, 1 = greyscale.
 * haze: blend toward a warm grey, which flattens and "washes out".
 * The shade factors are the face lighting; closer to 1 = flatter.
 */
export const GRADE = {
  desaturate: 0.32,
  haze: 0.14,
  hazeColour: [0x9a, 0x94, 0x80] as const,
  shades: { top: 1.0, topShadow: 0.88, left: 0.86, right: 0.76, ledge: 1.05 },
}

const BAYER = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5]
const BG = rgb(0x58, 0x7a, 0x82)
const OUTLINE = rgb(0x1b, 0x15, 0x20)
const SECTION: readonly (readonly number[])[] = [[0x3b, 0x2f, 0x3c], [0x37, 0x2b, 0x38], [0x40, 0x34, 0x41]]
const FIRE: readonly (readonly number[])[] = [[0xff, 0xd2, 0x4a], [0xff, 0x9a, 0x2a], [0xe8, 0x5a, 0x1e], [0xff, 0xf0, 0x9a]]

// Shade slots in the colour lookup table.
const TOP = 0, TOP_AO = 1, LEFT = 2, RIGHT = 3, TOP_EDGE = 4
const SHADES = [GRADE.shades.top, GRADE.shades.topShadow, GRADE.shades.left, GRADE.shades.right, GRADE.shades.ledge]
const PICKS = 8

function rgb(r: number, g: number, b: number): number {
  return (0xff << 24 | b << 16 | g << 8 | r) >>> 0
}

function grade(c: readonly number[]): number[] {
  const l = c[0]! * 0.3 + c[1]! * 0.59 + c[2]! * 0.11
  return [0, 1, 2].map(i => {
    const v = c[i]! + (l - c[i]!) * GRADE.desaturate
    return v + (GRADE.hazeColour[i]! - v) * GRADE.haze
  })
}

function shade(c: readonly number[], k: number): number {
  const f = (v: number) => Math.max(0, Math.min(255, Math.round(v * k)))
  return rgb(f(c[0]!), f(c[1]!), f(c[2]!))
}

// Palettes sorted dark → light, so neighbouring picks are neighbouring
// shades: that is what makes ordered dithering read as texture.
const lum = (c: readonly number[]) => c[0]! * 0.3 + c[1]! * 0.59 + c[2]! * 0.11
const RAMPS = MATERIALS.map(m => [...m.palette].map(grade).sort((a, b) => lum(a) - lum(b)))
const LUT = new Uint32Array(MATERIALS.length * PICKS * SHADES.length)
for (const m of MATERIALS) {
  const ramp = RAMPS[m.id]!
  for (let p = 0; p < PICKS; p++) {
    const c = ramp.length ? ramp[Math.min(p, ramp.length - 1)]! : [0, 0, 0]
    for (let s = 0; s < SHADES.length; s++) LUT[(m.id * PICKS + p) * SHADES.length + s] = shade(c, SHADES[s]!)
  }
}
const lut = (material: number, pick: number, s: number) => LUT[(material * PICKS + pick) * SHADES.length + s]!
const packed = (c: readonly number[]) => rgb(c[0]!, c[1]!, c[2]!)
const SECTION_PX = SECTION.map(c => packed(grade(c)))
const FIRE_PX = FIRE.map(packed)
// Flames, hot base → cool tip. Burning material chars toward CHAR and
// shows glowing EMBERs; smoke is blended over whatever is behind it.
const FLAME_PX = [[0xff, 0xf6, 0xc8], [0xff, 0xdc, 0x60], [0xff, 0xb0, 0x34], [0xf4, 0x7a, 0x22], [0xd2, 0x48, 0x1a], [0x8a, 0x2c, 0x16]].map(packed)
const CHAR_PX = rgb(0x26, 0x1d, 0x1a)
const EMBER_PX = [[0xff, 0x8a, 0x24], [0xe8, 0x52, 0x18], [0xff, 0xc0, 0x50]].map(packed)
// Smoke from steam (light) to soot (dark).
const SMOKE_PX = [[0xb4, 0xb0, 0xa8], [0x96, 0x92, 0x8c], [0x66, 0x62, 0x5e], [0x4c, 0x48, 0x46]].map(packed)
const MAX_FLAME_PARTICLES = 12000
const MAX_PUFFS = 4000
/** Field smoke (soot + steam) a cell needs, near the fire, to give off a visible puff. */
const PUFF_MIN_SMOKE = 600
/** Puff opacity steps, out of 255. */
const PUFF_LEVELS = [0, 50, 90, 125, 155, 180]
interface SmokePuff { x: number; y: number; z: number; r: number; age: number; life: number; amount: number; dark: number }
/** Splatted weight a pixel needs to show as flame. */
const FLAME_THRESHOLD = 7
/** Burning (simulation flame units) at which a cell feeds flame every tick. */
const FLAME_FULL_FEED = 48
const FIRELIGHT = rgb(0xff, 0x9a, 0x40)
/** Smoke opacity levels, out of 255, from barely there to thick. */
const SMOKE_LEVELS = [0, 15, 28, 40, 50, 58, 65]
interface FlameParticle { x: number; y: number; z: number; vx: number; vy: number; vz: number; heat: number; cool: number; life: number }
// The 14 pixels of a cube sprite: [dx, dy, face] with face 0 top, 1 left, 2 right.
const SPRITE: readonly (readonly [number, number, number])[] = [
  [1, 0, 0], [2, 0, 0], [0, 1, 0], [1, 1, 0], [2, 1, 0], [3, 1, 0],
  [0, 2, 1], [1, 2, 1], [2, 2, 2], [3, 2, 2], [0, 3, 1], [1, 3, 1], [2, 3, 2], [3, 3, 2],
]

const U = 4 // unit edge in cells
const unitKey = (ux: number, uy: number, uz: number) => ux | uy << 9 | uz << 18

interface UnitPixels {
  pix: Int32Array
  col: Uint32Array
  dep: Int32Array
  ids: Int32Array
  /** Burning cells, redrawn every frame. */
  dynamic: Int32Array
  /** Water cells open to the air, blended after the opaque pass. */
  water: Int32Array
  cells: number
}

export class IsoRenderer {
  readonly width: number
  readonly height: number
  readonly image: ImageData
  private readonly px: Uint32Array
  private readonly depth: Int32Array
  /** Picking: which cell is under each pixel (packed), -1 for none. */
  readonly pick: Int32Array
  private readonly units = new Map<number, UnitPixels>()
  /** When set, put() records pixels into this instead of drawing them. */
  private recording: { pix: number[]; col: number[]; dep: number[]; ids: number[] } | undefined
  /** Units rebuilt in the last frame, for the HUD. */
  rebuilt = 0

  constructor(readonly world: Grid, readonly margin = 8) {
    this.width = 2 * (world.sx + world.sy) + margin * 2
    this.height = world.sx + world.sy + 2 * world.sz + margin * 2
    this.image = new ImageData(this.width, this.height)
    this.px = new Uint32Array(this.image.data.buffer)
    this.depth = new Int32Array(this.width * this.height)
    this.pick = new Int32Array(this.width * this.height)
    const ux = Math.ceil(world.sx / U), uy = Math.ceil(world.sy / U), uz = Math.ceil(world.sz / U)
    for (let z = 0; z < uz; z++) for (let y = 0; y < uy; y++) for (let x = 0; x < ux; x++) {
      const b = world.brickIndex(x * U, y * U, z * U)
      if (world.bricks[b] === null && world.uniform[b] === M.Air) continue // nothing to draw
      this.buildUnit(x, y, z)
    }
    world.changeLog = []
  }

  /** Screen anchor (top-left of the sprite) of a world cell. */
  anchor(x: number, y: number, z: number): [number, number] {
    return [2 * (x - y) + 2 * this.world.sy + this.margin - 2, x + y - 2 * z + 2 * this.world.sz + this.margin]
  }

  /**
   * How a rotating body is drawn (view only; the simulation is unaffected):
   * - 'snap': each cell at the world cell its centre falls in (tears and doubles up).
   * - 'splat': each cell's sprite at its exact projected position, rounded to a pixel.
   * - 'quarter': orientation snapped to the nearest quarter turn, position exact.
   */
  bodyStyle: 'snap' | 'splat' | 'quarter' = 'quarter'
  /** Debug view: temperature as colour and air motion as short lines, instead of flames and smoke. */
  airView = false
  /** World seed, so flames can lean with the (pure-function) wind. */
  seed = 0

  render(grids: readonly Grid[], tick: number, air?: { fields: Fields; embers: readonly Ember[] }): { surfaceCells: number } {
    const world = grids[0]!
    this.applyChanges(world)
    const { px, depth, pick } = this
    px.fill(BG)
    depth.fill(-1 << 30)
    pick.fill(-1)
    let surfaceCells = 0
    for (const u of this.units.values()) {
      surfaceCells += u.cells
      const { pix, col, dep, ids } = u
      for (let i = 0; i < pix.length; i++) {
        const j = pix[i]!
        if (dep[i]! < depth[j]!) continue
        depth[j] = dep[i]!
        px[j] = col[i]!
        pick[j] = ids[i]!
      }
      for (let i = 0; i < u.dynamic.length; i++) {
        const p = u.dynamic[i]!
        this.drawCell(world, p & 1023, (p >> 10) & 1023, p >>> 20, tick)
      }
    }
    // Bodies are small: draw every cell of each.
    for (let gi = 1; gi < grids.length; gi++) {
      const g = grids[gi]!
      const place = this.bodyPlacer(g)
      for (let z = 0; z < g.sz; z++) for (let y = 0; y < g.sy; y++) for (let x = 0; x < g.sx; x++) {
        const m = g.get(x, y, z)
        if (m !== M.Air) this.drawBodyCell(g, x, y, z, m, place)
      }
    }
    for (const u of this.units.values()) {
      for (let i = 0; i < u.water.length; i++) {
        const p = u.water[i]!
        this.drawWater(world, p & 1023, (p >> 10) & 1023, p >>> 20, tick)
      }
    }
    this.outline()
    // Gas and embers after the outline pass, which would ring them in black.
    if (air) {
      if (this.airView) this.drawAirView(air.fields, tick)
      else { this.drawGas(air.fields, tick); this.drawEmbers(air.embers, tick) }
    }
    return { surfaceCells }
  }

  /** Rebuild every unit whose pixels a changed cell can affect. */
  private applyChanges(g: Grid) {
    const log = g.changeLog
    this.rebuilt = 0
    if (!log || !log.length) return
    const dirty = new Set<number>()
    const mark = (x: number, y: number, z: number) => {
      if (x < 0 || y < 0 || z < 0) return
      dirty.add(unitKey(Math.floor(x / U), Math.floor(y / U), Math.floor(z / U)))
    }
    for (const p of log) {
      const x = p & 1023, y = (p >> 10) & 1023, z = p >>> 20
      mark(x, y, z)
      // A cell's sprite depends on its +x, +y, +z neighbours (exposure),
      // its front neighbours (ledge light) and two cells above-behind (shade).
      mark(x - 1, y, z); mark(x, y - 1, z); mark(x, y, z - 1)
      mark(x + 1, y, z - 1); mark(x, y + 1, z - 1)
    }
    log.length = 0
    for (const k of dirty) {
      this.buildUnit(k & 511, (k >> 9) & 511, k >>> 18)
      this.rebuilt++
    }
  }

  private buildUnit(ux: number, uy: number, uz: number) {
    const g = this.world
    const key = unitKey(ux, uy, uz)
    const rec = { pix: [] as number[], col: [] as number[], dep: [] as number[], ids: [] as number[] }
    const dynamic: number[] = []
    const water: number[] = []
    let cells = 0
    this.recording = rec
    for (let z = uz * U; z < Math.min(g.sz, uz * U + U); z++)
      for (let y = uy * U; y < Math.min(g.sy, uy * U + U); y++)
        for (let x = ux * U; x < Math.min(g.sx, ux * U + U); x++) {
          const m = g.get(x, y, z)
          if (m === M.Air) continue
          if (m === M.Water) {
            if (g.get(x, y, z + 1) === M.Air) water.push(x | y << 10 | z << 20)
            continue
          }
          if (!isClear(g, x + 1, y, z) && !isClear(g, x, y + 1, z) && !isClear(g, x, y, z + 1)) continue
          cells++
          if (mat(m).flammable && g.getState(x, y, z) > 0) dynamic.push(x | y << 10 | z << 20)
          else this.drawCell(g, x, y, z, 0)
        }
    this.recording = undefined
    if (!cells && !water.length) { this.units.delete(key); return }
    this.units.set(key, {
      pix: Int32Array.from(rec.pix),
      col: Uint32Array.from(rec.col),
      dep: Int32Array.from(rec.dep),
      ids: Int32Array.from(rec.ids),
      dynamic: Int32Array.from(dynamic),
      water: Int32Array.from(water),
      cells,
    })
  }

  private put(sx: number, sy: number, d: number, color: number, id: number) {
    if (sx < 0 || sy < 0 || sx >= this.width || sy >= this.height) return
    const i = sy * this.width + sx
    const rec = this.recording
    if (rec) { rec.pix.push(i); rec.col.push(color); rec.dep.push(d); rec.ids.push(id); return }
    if (d < this.depth[i]!) return
    this.depth[i] = d
    this.px[i] = color
    this.pick[i] = id
  }

  private drawCell(g: Grid, x: number, y: number, z: number, tick: number) {
    const m = g.get(x, y, z)
    const [ax, ay] = this.anchor(x, y, z)
    const d = x + y + z
    const id = x | y << 10 | z << 20
    const material = mat(m)
    const burning = material.flammable !== undefined && g.getState(x, y, z) > 0

    if (m === M.TallGrass || m === M.Flower || m === M.Moss) {
      // Thin plants: a couple of blade pixels, not a cube.
      const h = hash32(x, y, z, 77)
      const n = RAMPS[m]!.length
      // Burning blades char dark, with an occasional glowing point; the flame
      // itself is drawn from the burning vapour above them.
      const glowing = burning && (hash32(h, tick >> 2) % 100) < 12
      const charBlade = burning ? blend(lut(m, 0, LEFT), CHAR_PX, 190) : 0
      const c = glowing ? EMBER_PX[h % 3]! : burning ? charBlade : lut(m, n - 1 - (h % 2), TOP)
      const c2 = burning ? charBlade : lut(m, (h >> 3) % n, LEFT)
      const ox = 1 + (h & 1)
      this.put(ax + ox, ay + 2, d, c2, id)
      this.put(ax + ox, ay + 1, d, c, id)
      if (m !== M.Moss && (h & 6) !== 0) this.put(ax + ox + ((h >> 4) & 1 ? 1 : -1), ay + ((h >> 5) & 1), d, c, id)
      if (m === M.Flower) this.put(ax + ox, ay, d, lut(m, n - 1, TOP_EDGE), id)
      return
    }

    const pick = this.pickOf(m, x, y, z)
    const topClear = isClear(g, x, y, z + 1)
    const ao = topClear && (isSolidish(g, x - 1, y, z + 1) || isSolidish(g, x, y - 1, z + 1))
    // A ledge catches light: brighten the top where the cell in front is lower.
    const edge = topClear && (isClear(g, x + 1, y, z) || isClear(g, x, y + 1, z))
    const top = lut(m, pick, ao ? TOP_AO : edge ? TOP_EDGE : TOP)
    const secX = x + 1 >= g.sx, secY = y + 1 >= g.sy
    const left = secY ? section(x, z) : lut(m, pick, LEFT)
    const right = secX ? section(y, z) : lut(m, pick, RIGHT)

    if (!burning) {
      this.put(ax + 1, ay, d, top, id); this.put(ax + 2, ay, d, top, id)
      this.put(ax, ay + 1, d, top, id); this.put(ax + 1, ay + 1, d, top, id)
      this.put(ax + 2, ay + 1, d, top, id); this.put(ax + 3, ay + 1, d, top, id)
      this.put(ax, ay + 2, d, left, id); this.put(ax + 1, ay + 2, d, left, id)
      this.put(ax + 2, ay + 2, d, right, id); this.put(ax + 3, ay + 2, d, right, id)
      this.put(ax, ay + 3, d, left, id); this.put(ax + 1, ay + 3, d, left, id)
      this.put(ax + 2, ay + 3, d, right, id); this.put(ax + 3, ay + 3, d, right, id)
      return
    }
    // Burning: the material chars as it burns, with embers glowing in it.
    // The flames themselves rise *above* it (flames()).
    const progress = g.getState(x, y, z) // 1..255 through burning
    const fine = material.flammable!.fineness
    const charA = Math.min(235, 70 + ((progress * 180) >> 8))
    const faces = [blend(top, CHAR_PX, charA), blend(left, CHAR_PX, charA), blend(right, CHAR_PX, charA)]
    // Quick-burning stuff (leaves, grass) is mostly glowing; wood glows more as it burns through.
    // Fine fuel (grass, leaves) glows only while it first catches and goes
    // dark as it chars, so glow sits on the burning edge, not across the
    // burnt ground behind it. Wood glows more as it burns through.
    const emberPct = fine >= 5 ? Math.max(0, 8 - (progress >> 4)) : 6 + (progress >> 4)
    for (let i = 0; i < SPRITE.length; i++) {
      const [dx, dy, face] = SPRITE[i]!
      const e = hash32(x * 31 + i, y, z, tick >> 2)
      this.put(ax + dx, ay + dy, d, e % 100 < emberPct ? EMBER_PX[(e >>> 8) % 3]! : faces[face]!, id)
    }
  }

  /**
   * Gas, drawn from the simulation's fields: each 20 cm field cell with gas is
   * a dithered blob (pixel art has no alpha). Smoke shades from light (steam)
   * to dark (soot) and is lit orange where it is hot; hot enough, it is flame,
   * coloured by temperature, with streaks rising through it.
   */
  private drawGas(fields: Fields, tick: number) {
    const W = this.width, H = this.height
    if (this.gasSmoke.length !== W * H) {
      this.gasSmoke = new Uint32Array(W * H); this.gasSoot = new Uint32Array(W * H)
      this.gasHeat = new Uint32Array(W * H); this.gasMark = new Uint8Array(W * H); this.gasWeight = new Uint32Array(W * H)
    }
    const { gasSmoke, gasSoot, gasHeat, gasMark, gasWeight } = this
    const touched: number[] = []
    if (tick !== this.particleTick) { this.stepFlames(fields, tick); this.stepPuffs(fields, tick); this.particleTick = tick }
    this.drawFirelight(fields, tick)
    // Pass 1: add up the gas over every pixel, each field cell spreading as a
    // soft blob (stronger at its centre). Gas behind the world is hidden.
    for (const [fx, fy, fz, heat, soot, steam] of fields.cells()) {
      const [cx, cy] = this.anchor(fx * FR + 1, fy * FR + 1, fz * FR + 1)
      const d = (fx + fy + fz) * FR + 3
      const smoke = soot + steam
      for (let dy = -5; dy <= 5; dy++) for (let dx = -5; dx <= 6; dx++) {
        const r = Math.abs(dx - 0.5) + Math.abs(dy)
        if (r > 7) continue
        const px = cx + 2 + dx, py = cy + 2 + dy
        if (px < 0 || py < 0 || px >= W || py >= H) continue
        const i = py * W + px
        if (d < this.depth[i]!) continue
        const wt = 8 - r // 1..8
        if (!gasMark[i]) { gasMark[i] = 1; touched.push(i) }
        gasSmoke[i]! += (smoke * wt) >> 3
        gasSoot[i]! += (soot * wt) >> 3
        // Heat is averaged, weighted towards blob centres, so the glow it
        // casts on smoke has no edges to trace.
        gasHeat[i]! += heat * wt
        gasWeight[i]! += wt
      }
    }
    // Pass 2: decide each pixel once. Smoke is laid over what's behind it in a
    // few steps of opacity (pixel-art shading, not a screen-door dither), with
    // a clumpy texture that scrolls upward with the rising smoke. Its shade
    // runs light (steam) to dark (soot), with a soft orange wash from hot gas.
    for (const i of touched) {
      const px = i % W, py = (i / W) | 0
      const heat = gasWeight[i]! ? gasHeat[i]! / gasWeight[i]! : 0
      const smoke = gasSmoke[i]!, soot = gasSoot[i]!
      gasMark[i] = 0; gasSmoke[i] = 0; gasSoot[i] = 0; gasHeat[i] = 0; gasWeight[i] = 0
      // Clumps ~4 px across, drifting up 1 px a tick, re-rolled slowly. They
      // break the outline most where the smoke is thin, so edges are ragged and
      // see-through rather than a crisp contour.
      // Smooth noise (not per-pixel random, which reads as stipple) gives soft
      // puffs ~6 px across that rise 1 px a tick and slowly change shape.
      const puff = (valueNoise2(this.seed ^ 0x5340, px * 4 + (tick >> 2), (py + tick) * 4, 24) >> 9) - 64
      const base = smoke >> 1
      const density = base + (puff * Math.min(Math.max(base, 40), 140)) / 80
      if (density < 30) continue
      const opacity = Math.min(220, (density - 30) * 2)
      const level = SMOKE_LEVELS[Math.min(SMOKE_LEVELS.length - 1, opacity >> 5)]!
      if (!level) continue
      const dark = smoke ? Math.min(3, Math.floor((soot * 4) / smoke)) : 0
      let col = SMOKE_PX[dark]!
      // Warm only on the underside, just above hot flames, and faintly.
      const lit = heat > 760 ? Math.min(60, (heat - 760) >> 2) : 0
      if (lit) col = blend(col, FLAME_PX[3]!, lit)
      this.px[i] = blend(this.px[i]!, col, level)
    }
    this.drawPuffs(tick)
    this.drawFlames()
  }

  /**
   * Flame, drawn as particles of burning gas. Each tick every cell where
   * vapour is burning (the simulation's Flame field) gives off a few, at
   * seeded positions inside it. They take the air's velocity and the
   * cell's temperature, then rise, drift and cool for a few ticks: white
   * when hottest, through yellow and orange, to dark red. View only, and
   * seeded from the tick, so every peer draws the same flames.
   */
  private stepFlames(fields: Fields, tick: number) {
    const kept: FlameParticle[] = []
    for (const q of this.flames) {
      q.x += q.vx; q.y += q.vy; q.z += q.vz
      q.vz += 0.03 // still buoyant, so tongues accelerate upward
      q.heat -= q.cool
      if (--q.life > 0 && q.heat > 500) kept.push(q)
    }
    for (const [fx, fy, fz, heat, , , , flame] of fields.cells()) {
      if (!flame) continue
      // Tongues come and go: each burning cell is briefly quiet about a third
      // of the time (re-rolled every 8 ticks), and has its own height for that
      // spell, so the front breaks into separate tongues with gaps between.
      const spell = hash32(fx * 7919 + fy, fz, tick >> 3)
      if ((spell & 255) < 85) continue
      // A cell burning only faintly feeds a tongue only now and then, in
      // proportion: thin burning shows as nothing, not as scattered sparks.
      if (flame < FLAME_FULL_FEED && (hash32(fx * 31 + fy, fz, tick, 0xfe) & 255) >= (flame * 256) / FLAME_FULL_FEED) continue
      // Height follows how much is burning here: a small burn gives a short
      // tongue, only heavy burning a tall one, so a dying fire shrinks.
      const fed = Math.min(1, flame / 160)
      const tall = (0.45 + ((spell >>> 8) & 255) / 400) * (0.5 + 0.6 * fed) // ×0.22 … ×1.2
      const n = Math.min(3, 1 + (flame >> 7))
      const [gx, gy, gz] = fields.velocity(this.seed, tick, fx, fy, fz, heat)
      for (let k = 0; k < n && kept.length < MAX_FLAME_PARTICLES; k++) {
        // Each cell feeds up to 4 streams; a stream's foot moves slowly (every
        // 6 ticks), so successive sparks line up into a tongue.
        const foot = hash32(fx * 131 + fy, fz * 977 + k, Math.floor(tick / 6))
        const h = hash32(fx * 131 + fy, fz * 977 + k, tick)
        const r = (x: number, bits: number) => ((x >>> bits) & 255) / 256
        kept.push({
          x: fx * FR + r(foot, 0) * FR, y: fy * FR + r(foot, 8) * FR, z: fz * FR + r(foot, 16) * 0.6,
          // field velocity is 1/256 field cells per tick: ×FR/256 for world cells
          vx: (gx * FR) / 256 + (r(h, 4) - 0.5) * 0.05, vy: (gy * FR) / 256 + (r(h, 12) - 0.5) * 0.05,
          vz: Math.max(0.12, (gz * FR) / 256) + r(h, 20) * 0.12,
          heat: Math.min(1000, heat - 40), cool: (45 + r(h, 24) * 45) / tall, life: Math.round((4 + ((h >>> 28) & 7)) * tall),
        })
      }
    }
    this.flames = kept
  }

  /**
   * Firelight: ground and walls near burning vapour get a warm wash, scaled
   * by how much is burning there and pulsing gently. Driven by the
   * simulation's Flame field; it only changes pixels (view).
   */
  private drawFirelight(fields: Fields, tick: number) {
    const W = this.width, H = this.height
    if (this.glow.length !== W * H) this.glow = new Uint32Array(W * H)
    const glow = this.glow, lit: number[] = []
    const R = 14
    for (const [fx, fy, fz, , , , , flame] of fields.cells()) {
      if (flame < 8) continue
      const [cx, cy] = this.anchor(fx * FR + 1, fy * FR + 1, fz * FR)
      for (let dy = -2; dy <= R; dy += 1) for (let dx = -R * 2; dx <= R * 2; dx += 1) { // ground at and below the flame
        const r = Math.abs(dx) / 2 + Math.abs(dy)
        if (r >= R) continue
        const px = cx + 2 + dx, py = cy + 3 + dy
        if (px < 0 || py < 0 || px >= W || py >= H) continue
        const i = py * W + px
        if (!glow[i]) lit.push(i)
        glow[i]! += ((R - r) * Math.min(flame, 64)) | 0
      }
    }
    const pulse = 220 + (hash32(tick >> 1, 0x9100) & 31)
    for (const i of lit) {
      const a = Math.min(70, (glow[i]! * pulse) >> 14)
      glow[i] = 0
      // Light, not paint: a modest warm wash, and never over moving bodies (whose faces we can't orient yet).
      if (a > 4 && this.px[i] !== BG && this.pick[i] !== -2) this.px[i] = blend(this.px[i]!, FIRELIGHT, a)
    }
  }

  private glow = new Uint32Array(0)

  /**
   * Flames as connected shapes: every spark splats a small upright blob of
   * weight and heat into a buffer; a pixel becomes flame only where enough
   * weight gathers, coloured by the average heat there. Sparks close together
   * merge into tongues with a smooth yellow → orange → red gradient; a lone
   * spark is too faint to show, except as a hot tip.
   */
  private drawFlames() {
    const W = this.width, H = this.height
    if (this.fAcc.length !== W * H) { this.fAcc = new Uint32Array(W * H); this.fHeat = new Uint32Array(W * H); this.fDepth = new Int32Array(W * H) }
    const { fAcc, fHeat, fDepth } = this
    const [bx, by] = this.anchor(0, 0, 0)
    const hit: number[] = []
    for (const q of this.flames) {
      const px = Math.round(bx + 2 * (q.x - q.y)) + 1, py = Math.round(by + q.x + q.y - 2 * q.z) + 1
      const d = Math.round(q.x + q.y + q.z)
      // An upright blob 3 px wide, 5 px tall, heaviest at its centre.
      for (let dy = -3; dy <= 1; dy++) for (let dx = -1; dx <= 1; dx++) {
        const x = px + dx, y = py + dy
        if (x < 0 || y < 0 || x >= W || y >= H) continue
        const i = y * W + x
        if (d < this.depth[i]!) continue
        const wt = 4 - Math.abs(dx) - (dy < -1 ? -dy - 1 : 0) - (dy > 0 ? 1 : 0)
        if (wt <= 0) continue
        if (!fAcc[i]) { hit.push(i); fDepth[i] = d }
        fAcc[i]! += wt
        fHeat[i]! += q.heat * wt
      }
    }
    for (const i of hit) {
      const acc = fAcc[i]!, heat = fHeat[i]! / acc
      fAcc[i] = 0; fHeat[i] = 0
      if (acc < FLAME_THRESHOLD) continue
      // Denser flame is hotter-looking at its core; thin edges take a redder shade.
      let shown = acc < FLAME_THRESHOLD + 3 ? heat - 140 : heat + Math.min(80, (acc - FLAME_THRESHOLD) * 6)
      // Narrow darker streaks inside the body, drifting slowly, so neighbouring
      // tongues read as separate rather than one flat sheet.
      const px = i % W
      if ((hash32(px >> 1, this.particleTick >> 3, 0x57) & 7) === 0) shown -= 160
      this.px[i] = FLAME_PX[shown > 1000 ? 0 : shown > 880 ? 1 : shown > 760 ? 2 : shown > 640 ? 3 : shown > 540 ? 4 : 5]!
      this.depth[i] = fDepth[i]!
    }
  }

  private fAcc = new Uint32Array(0)
  private fHeat = new Uint32Array(0)
  private fDepth = new Int32Array(0)

  /**
   * Smoke puffs: the smoke field (simulation) given visible shape. Puffs are
   * born where the field's smoke is thick near the fire, take its mix of
   * soot and steam for their shade, then ride the simulation's air velocity,
   * growing and thinning as they rise, so the eye can follow smoke travelling
   * and leaning with the wind. View only; seeded from the tick.
   */
  private stepPuffs(fields: Fields, tick: number) {
    const kept: SmokePuff[] = []
    for (const q of this.puffs) {
      const fx = Math.floor(q.x / FR), fy = Math.floor(q.y / FR), fz = Math.floor(q.z / FR)
      const [gx, gy, gz] = fields.velocity(this.seed, tick, fx, fy, fz, 0)
      q.x += (gx * FR) / 256; q.y += (gy * FR) / 256; q.z += Math.max(0.04, (gz * FR) / 256)
      q.r += 0.035
      q.amount *= 0.982
      if (++q.age < q.life && q.amount > 0.06) kept.push(q)
    }
    for (const [fx, fy, fz, heat, soot, steam] of fields.cells()) {
      const smoke = soot + steam
      if (smoke < PUFF_MIN_SMOKE || heat < 120 || kept.length >= MAX_PUFFS) continue
      const h = hash32(fx * 7333 + fy, fz, tick)
      if (h % 8 !== 0) continue // about one puff per thick cell every 8 ticks
      kept.push({
        x: fx * FR + ((h >>> 8) & 255) / 128, y: fy * FR + ((h >>> 16) & 255) / 128, z: fz * FR + 1,
        r: 0.8 + ((h >>> 24) & 7) / 10, age: 0, life: 60 + (h & 63),
        amount: Math.min(1, smoke / 2500), dark: Math.min(3, (soot * 4) / smoke),
      })
    }
    this.puffs = kept
  }

  private drawPuffs(_tick: number) {
    const W = this.width, H = this.height
    if (this.pAcc.length !== W * H) { this.pAcc = new Uint32Array(W * H); this.pDark = new Uint32Array(W * H) }
    const { pAcc, pDark } = this
    const [bx, by] = this.anchor(0, 0, 0)
    const hit: number[] = []
    for (const q of this.puffs) {
      const cx = bx + 2 * (q.x - q.y) + 1, cy = by + q.x + q.y - 2 * q.z + 1
      const d = Math.round(q.x + q.y + q.z)
      const rp = q.r * 2.2 // screen radius in px
      const r2 = rp * rp
      const a = Math.round(q.amount * 256)
      for (let dy = Math.floor(-rp); dy <= rp; dy++) for (let dx = Math.floor(-rp); dx <= rp; dx++) {
        const dd = dx * dx + dy * dy
        if (dd > r2) continue
        const x = Math.round(cx + dx), y = Math.round(cy + dy)
        if (x < 0 || y < 0 || x >= W || y >= H) continue
        const i = y * W + x
        if (d < this.depth[i]!) continue
        const wgt = (a * (r2 - dd)) / r2 // soft edge: full at the centre, nothing at the rim
        if (!pAcc[i]) hit.push(i)
        pAcc[i]! += wgt
        pDark[i]! += wgt * q.dark
      }
    }
    for (const i of hit) {
      const acc = pAcc[i]!, dark = pDark[i]! / acc
      pAcc[i] = 0; pDark[i] = 0
      const level = acc > 420 ? 5 : acc > 300 ? 4 : acc > 200 ? 3 : acc > 120 ? 2 : acc > 60 ? 1 : 0
      if (!level) continue
      const lo = Math.floor(dark), hi = Math.min(3, lo + 1), t = Math.round((dark - lo) * 255)
      const col = blend(SMOKE_PX[lo]!, SMOKE_PX[hi]!, t)
      this.px[i] = blend(this.px[i]!, col, PUFF_LEVELS[level]!)
    }
  }

  private puffs: SmokePuff[] = []
  private pAcc = new Uint32Array(0)
  private pDark = new Uint32Array(0)

  private flames: FlameParticle[] = []
  private particleTick = -1

  private gasWeight = new Uint32Array(0)
  private gasSmoke = new Uint32Array(0)
  private gasSoot = new Uint32Array(0)
  private gasHeat = new Uint32Array(0)
  private gasMark = new Uint8Array(0)

  /**
   * The air, for inspection: each field cell with gas or moving air is
   * tinted by temperature (blue cool → red → yellow flame-hot), and every
   * other cell gets a short line showing where its air is going.
   */
  private drawAirView(fields: Fields, tick: number) {
    const cells = new Set<number>()
    const vel = fields.vel
    const visit = (fx: number, fy: number, fz: number) => {
      const i = fields.index(fx, fy, fz)
      if (cells.has(i)) return
      cells.add(i)
      const heat = fields.cur[0]![i]!
      const [cx, cy] = this.anchor(fx * FR + 1, fy * FR + 1, fz * FR + 1)
      const d = (fx + fy + fz) * FR + 3
      if (heat > 20) {
        const t = Math.min(255, heat >> 2)
        const col = t < 85 ? rgb(40, 60 + t, 160 - t) : t < 170 ? rgb(60 + (t - 85) * 2, 100, 60) : rgb(230, 120 + (t - 170), 40)
        for (let dy = 0; dy < 3; dy++) for (let dx = 0; dx < 3; dx++) if (((cx + dx + cy + dy) & 1) === 0) this.putPixel(cx + 1 + dx, cy + 1 + dy, d, col)
      }
      if (vel && ((fx + fy + fz) & 1) === 0) {
        // Screen direction of the air's motion (+x → right-down, +y → left-down, +z → up).
        const u = vel[0]![i]!, v = vel[1]![i]!, w = vel[2]![i]!
        const sx = 2 * (u - v), sy = u + v - 2 * w
        const len = Math.max(Math.abs(sx), Math.abs(sy))
        if (len > 24) {
          const steps = Math.min(5, 1 + (len >> 7))
          for (let k = 0; k <= steps; k++) this.putPixel(cx + 2 + Math.round((sx * k) / len), cy + 2 + Math.round((sy * k) / len), d + 1, k === steps ? rgb(255, 255, 255) : rgb(200, 220, 240))
        }
      }
    }
    for (const [fx, fy, fz] of fields.cells()) visit(fx, fy, fz)
    if (vel) for (const b of fields.moving) fields.cellsOfBrick(b, visit)
  }

  /** Embers in flight: a bright pixel with a short dim trail. */
  private drawEmbers(embers: readonly Ember[], tick: number) {
    for (const e of embers) {
      const x = e.x / 256, y = e.y / 256, z = e.z / 256
      const [bx, by] = this.anchor(0, 0, 0)
      const px = Math.round(bx + 2 * (x - y)) + 2, py = Math.round(by + x + y - 2 * z) + 1
      const d = Math.round(x + y + z) + 1
      const c = EMBER_PX[(e.life + tick) % 3]!
      this.putPixel(px, py, d, c)
    }
  }

  private putPixel(px: number, py: number, d: number, c: number) {
    if (px < 0 || py < 0 || px >= this.width || py >= this.height) return
    const i = py * this.width + px
    if (d < this.depth[i]!) return
    this.px[i] = c
    this.depth[i] = d
  }

  private putBlend(sx: number, sy: number, d: number, color: number, alpha: number) {
    if (sx < 0 || sy < 0 || sx >= this.width || sy >= this.height || alpha <= 0) return
    const i = sy * this.width + sx
    if (d < this.depth[i]!) return
    this.px[i] = blend(this.px[i]!, color, alpha)
  }

  /** Where each cell of a body is drawn: world-space corner (x, y, z), fractional for splat/quarter. */
  private bodyPlacer(g: Grid): (x: number, y: number, z: number) => [number, number, number] {
    const pose = g.pose
    if (!pose) return (x, y, z) => [x + g.ox, y + g.oy, z + g.oz]
    if (this.bodyStyle === 'snap') return (x, y, z) => { const c = cellCentre(pose, x, y, z); return [Math.floor(c[0]), Math.floor(c[1]), Math.floor(c[2])] }
    if (this.bodyStyle === 'splat') return (x, y, z) => { const c = cellCentre(pose, x, y, z); return [c[0] - 0.5, c[1] - 0.5, c[2] - 0.5] }
    // Quarter turns: snap each rotated axis to the nearest world axis, rotate
    // about the body's true centre so the snap doesn't make it jump.
    const hx = g.sx / 2, hy = g.sy / 2, hz = g.sz / 2
    const centre = cellCentre(pose, hx - 0.5, hy - 0.5, hz - 0.5)
    const o = cellCentre(pose, -0.5, -0.5, -0.5)
    const axis = (v: [number, number, number]): [number, number, number] => {
      const d: [number, number, number] = [v[0] - o[0], v[1] - o[1], v[2] - o[2]]
      const i = Math.abs(d[0]) >= Math.abs(d[1]) && Math.abs(d[0]) >= Math.abs(d[2]) ? 0 : Math.abs(d[1]) >= Math.abs(d[2]) ? 1 : 2
      const r: [number, number, number] = [0, 0, 0]; r[i] = Math.sign(d[i]!); return r
    }
    const ax = axis(cellCentre(pose, 0.5, -0.5, -0.5)), ay = axis(cellCentre(pose, -0.5, 0.5, -0.5))
    // z axis = x × y keeps it a proper rotation even if the snap was ambiguous.
    const az: [number, number, number] = [ax[1] * ay[2] - ax[2] * ay[1], ax[2] * ay[0] - ax[0] * ay[2], ax[0] * ay[1] - ax[1] * ay[0]]
    const cx = Math.round(centre[0] * 2) / 2, cy = Math.round(centre[1] * 2) / 2, cz = Math.round(centre[2] * 2) / 2
    return (x, y, z) => {
      const lx = x + 0.5 - hx, ly = y + 0.5 - hy, lz = z + 0.5 - hz
      return [
        cx + ax[0] * lx + ay[0] * ly + az[0] * lz - 0.5,
        cy + ax[1] * lx + ay[1] * ly + az[1] * lz - 0.5,
        cz + ax[2] * lx + ay[2] * ly + az[2] * lz - 0.5,
      ]
    }
  }

  private drawBodyCell(g: Grid, x: number, y: number, z: number, m: number, place: (x: number, y: number, z: number) => [number, number, number]) {
    const [fx, fy, fz] = place(x, y, z)
    const wx = Math.floor(fx + 0.5), wy = Math.floor(fy + 0.5), wz = Math.floor(fz + 0.5)
    // Screen position from the (possibly fractional) corner, rounded to a pixel.
    const [bx, by] = this.anchor(0, 0, 0)
    const ax = Math.round(bx + 2 * (fx - fy)), ay = Math.round(by + fx + fy - 2 * fz)
    const d = Math.round(fx + fy + fz)
    const pick = this.pickOf(m, wx, wy, wz)
    const burning = mat(m).flammable !== undefined && g.getState(x, y, z) > 0
    const h = hash32(x, y, z, g.sx * 7 + g.sy)
    const charred = (c: number, k: number) => (burning ? ((h >>> k) % 100 < 25 ? EMBER_PX[(h >>> k) % 3]! : blend(c, CHAR_PX, 170)) : c)
    const top = charred(lut(m, pick, g.get(x, y, z + 1) === M.Air ? TOP_EDGE : TOP), 0)
    const left = charred(lut(m, pick, LEFT), 3), right = charred(lut(m, pick, RIGHT), 5)
    const id = -2
    this.put(ax + 1, ay, d, top, id); this.put(ax + 2, ay, d, top, id)
    for (let k = 0; k < 4; k++) this.put(ax + k, ay + 1, d, top, id)
    for (let r = 2; r < 4; r++) {
      this.put(ax, ay + r, d, left, id); this.put(ax + 1, ay + r, d, left, id)
      this.put(ax + 2, ay + r, d, right, id); this.put(ax + 3, ay + r, d, right, id)
    }
  }


  /**
   * Colour choice per cell. Stone is coloured per carved block (8×8×6 cells)
   * with darker joints, which gives the chiselled look of the reference;
   * organic materials pick a shade from low-frequency noise, dithered.
   */
  private pickOf(m: number, x: number, y: number, z: number): number {
    const n = RAMPS[m]!.length
    if (m === M.Metal) return 1 + (hash32(x, y, z, 3) % 5 === 0 ? 1 : 0)
    if (m === M.Stone || m === M.MossStone) {
      const bz = Math.floor(z / 6)
      const off = (bz & 1) * 4 // stagger courses like masonry
      const bx = (x + off) >> 3, by = (y + off) >> 3
      const joint = ((x + off) & 7) === 0 || ((y + off) & 7) === 0 || z % 6 === 0
      if (joint) return 0
      const blockPick = 1 + (hash32(bx, by, bz, 11) % (n - 1))
      return hash32(x, y, z) % 6 === 0 ? Math.max(1, blockPick - 1) : blockPick
    }
    // Clumps: low-frequency noise picks a shade; a 4×4 Bayer threshold
    // dithers between neighbouring shades, like hand-dithered pixel art.
    const v = valueNoise2(m * 31, x + z, y - z, 7) * (n - 1)
    const bayer = BAYER[(y & 3) * 4 + (x & 3)]!
    return Math.max(0, Math.min(n - 1, Math.floor((v + bayer * 4096 - 30720) / 65535)))
  }

  private drawWater(g: Grid, x: number, y: number, z: number, tick: number) {
    const w = this.width
    const [ax, ay] = this.anchor(x, y, z)
    const n = valueNoise2(5, x * 2 + (tick >> 3), y * 2 - (tick >> 4), 9)
    const line = Math.abs(n - 32768) < 1600
    const cells: [number, number][] = [[1, 0], [2, 0], [0, 1], [1, 1], [2, 1], [3, 1]]
    if (x + 1 >= g.sx) cells.push([2, 2], [3, 2], [2, 3], [3, 3])
    if (y + 1 >= g.sy) cells.push([0, 2], [1, 2], [0, 3], [1, 3])
    const tint = line ? rgb(0x9f, 0xd4, 0xcc) : rgb(0x52, 0x86, 0x96)
    for (const [dx, dy] of cells) {
      const sx = ax + dx, sy = ay + dy
      if (sx < 0 || sy < 0 || sx >= w || sy >= this.height) continue
      const i = sy * w + sx
      if (this.depth[i]! > x + y + z + 1) continue // something stands in front
      this.px[i] = blend(this.px[i]!, tint, line ? 190 : 150)
      this.depth[i] = x + y + z
    }
  }

  private outline() {
    const { width: w, height: h, px } = this
    const edge: number[] = []
    for (let y = 1; y < h - 1; y++) for (let x = 1; x < w - 1; x++) {
      const i = y * w + x
      if (px[i] !== BG) continue
      if (px[i - 1] !== BG || px[i + 1] !== BG || px[i - w] !== BG || px[i + w] !== BG) edge.push(i)
    }
    for (const i of edge) px[i] = OUTLINE
  }
}

function isClear(g: Grid, x: number, y: number, z: number): boolean {
  if (!g.inBounds(x, y, z)) return true
  const m = g.get(x, y, z)
  return !mat(m).opaque || m === M.TallGrass || m === M.Flower || m === M.Moss
}

function isSolidish(g: Grid, x: number, y: number, z: number): boolean {
  const m = g.get(x, y, z)
  return m !== M.Air && m !== M.Water
}

function fire(h: number): number {
  return FIRE_PX[(h >>> 0) % FIRE_PX.length]!
}

function section(a: number, z: number): number {
  // Mostly flat, with a sparse dither so the cut face isn't dead.
  const h = hash32(a, z, 41) % 11
  return SECTION_PX[h === 0 ? 1 : h === 1 ? 2 : 0]!
}

function blend(under: number, over: number, alpha: number): number {
  const ia = 255 - alpha
  const r = ((under & 0xff) * ia + (over & 0xff) * alpha) >> 8
  const g = (((under >> 8) & 0xff) * ia + ((over >> 8) & 0xff) * alpha) >> 8
  const b = (((under >> 16) & 0xff) * ia + ((over >> 16) & 0xff) * alpha) >> 8
  return rgb(r, g, b)
}
