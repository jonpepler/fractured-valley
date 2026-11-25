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
import { prevailing, windAt, type Wind } from './wind'

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
const SMOKE = rgb(0x4e, 0x4a, 0x48)
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

  /** World seed, so flames can lean with the (pure-function) wind. */
  seed = 0
  private windBase: Wind = { x: 0, y: 0 }

  render(grids: readonly Grid[], tick: number): { surfaceCells: number } {
    const world = grids[0]!
    this.windBase = prevailing(this.seed, tick)
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
      for (let z = 0; z < g.sz; z++) for (let y = 0; y < g.sy; y++) for (let x = 0; x < g.sx; x++) {
        const m = g.get(x, y, z)
        if (m !== M.Air) this.drawBodyCell(g, x, y, z, m)
      }
    }
    for (const u of this.units.values()) {
      for (let i = 0; i < u.water.length; i++) {
        const p = u.water[i]!
        this.drawWater(world, p & 1023, (p >> 10) & 1023, p >>> 20, tick)
      }
    }
    this.outline()
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
      const c = burning ? fire(h ^ tick) : lut(m, n - 1 - (h % 2), TOP)
      const c2 = burning ? fire(h ^ (tick + 1)) : lut(m, (h >> 3) % n, LEFT)
      const ox = 1 + (h & 1)
      this.put(ax + ox, ay + 2, d, c2, id)
      this.put(ax + ox, ay + 1, d, c, id)
      if (m !== M.Moss && (h & 6) !== 0) this.put(ax + ox + ((h >> 4) & 1 ? 1 : -1), ay + ((h >> 5) & 1), d, c, id)
      if (m === M.Flower) this.put(ax + ox, ay, d, lut(m, n - 1, TOP_EDGE), id)
      if (burning) this.flames(x, y, z, ax, ay + 1, d, g.getState(x, y, z), material.flammable!.burnTicks, tick, id, true)
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
    const s = g.getState(x, y, z), burnTicks = material.flammable!.burnTicks
    const progress = Math.min(255, Math.floor((s * 256) / burnTicks))
    const charA = Math.min(235, 70 + ((progress * 180) >> 8))
    const faces = [blend(top, CHAR_PX, charA), blend(left, CHAR_PX, charA), blend(right, CHAR_PX, charA)]
    // Quick-burning stuff (leaves, grass) is mostly glowing; wood glows more as it burns through.
    const emberPct = burnTicks < 40 ? 45 : 6 + (progress >> 4)
    for (let i = 0; i < SPRITE.length; i++) {
      const [dx, dy, face] = SPRITE[i]!
      const e = hash32(x * 31 + i, y, z, tick >> 2)
      this.put(ax + dx, ay + dy, d, e % 100 < emberPct ? EMBER_PX[(e >>> 8) % 3]! : faces[face]!, id)
    }
    const sideClear = isClear(g, x + 1, y, z) || isClear(g, x, y + 1, z)
    if (topClear || sideClear) this.flames(x, y, z, ax, ay, d, s, burnTicks, tick, id, topClear)
  }

  /**
   * Flame tongues rising from a burning cell: taller for quick, bright fuel,
   * growing as it catches and dying as it burns out, leaning with the wind,
   * flickering every tick. Smoke drifts off some of them. View-only.
   */
  private flames(x: number, y: number, z: number, ax: number, ay: number, d: number, s: number, burnTicks: number, tick: number, id: number, fromTop: boolean) {
    const h = hash32(x, y, z, tick)
    const progress = Math.min(255, Math.floor((s * 256) / burnTicks))
    // Envelope: catch (first 15%), full, dying (last 25%).
    const env = progress < 38 ? 96 + progress * 4 : progress > 192 ? Math.max(32, (255 - progress) * 4) : 256
    const tall = burnTicks < 40 ? 4 + (h % 6) : 3 + (h % 4)
    const H = Math.max(1, ((fromTop ? tall : 2 + (h & 1)) * env) >> 8)
    const wind = windAt(this.seed, tick, x, y, this.windBase)
    const leanX = wind.x - wind.y // screen-horizontal component: +x drifts right, +y left
    const x0 = ax + 1 + (h & 1), y0 = fromTop ? ay : ay + 2
    for (let k = 0; k < H; k++) {
      const wobble = k > 1 ? (hash32(x, y, z, tick * 8 + k) % 3) - 1 : 0
      const sx = x0 + ((leanX * k) >> 8) + wobble
      const c = FLAME_PX[Math.min(5, Math.floor((k * 6) / (H + 1)) + ((h >>> (k + 4)) & 1))]!
      const dk = d + 1 + (k >> 1)
      this.put(sx, y0 - k, dk, c, id)
      if (k < H >> 1) this.put(sx + 1, y0 - k, dk, c, id)
    }
    // Smoke: a puff every so often, rising and drifting downwind, fading out.
    if (fromTop && hash32(x, y, z, 0x5e0c) % 3 === 0) {
      const phase = (tick + (hash32(x, y, z, 1) % 48)) % 48
      const rise = H + phase
      const alpha = 150 - phase * 3
      const sx = x0 + ((leanX * rise) >> 8) + ((hash32(x, y, z, phase >> 3) % 3) - 1)
      this.putBlend(sx, y0 - rise, d + 2 + (rise >> 1), SMOKE, alpha)
      if (phase > 8) this.putBlend(sx + 1, y0 - rise, d + 2 + (rise >> 1), SMOKE, alpha >> 1)
    }
  }

  private putBlend(sx: number, sy: number, d: number, color: number, alpha: number) {
    if (sx < 0 || sy < 0 || sx >= this.width || sy >= this.height || alpha <= 0) return
    const i = sy * this.width + sx
    if (d < this.depth[i]!) return
    this.px[i] = blend(this.px[i]!, color, alpha)
  }

  private drawBodyCell(g: Grid, x: number, y: number, z: number, m: number) {
    // A moving body is drawn by placing each of its cells at the world cell
    // its centre currently falls in: voxels stay voxels while it tumbles.
    let wx = x + g.ox, wy = y + g.oy, wz = z + g.oz
    if (g.pose) {
      const [cx, cy, cz] = cellCentre(g.pose, x, y, z)
      wx = Math.floor(cx); wy = Math.floor(cy); wz = Math.floor(cz)
    }
    const [ax, ay] = this.anchor(wx, wy, wz)
    const d = wx + wy + wz
    const pick = this.pickOf(m, wx, wy, wz)
    const burning = mat(m).flammable !== undefined && g.getState(x, y, z) > 0
    const h = hash32(wx, wy, wz)
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
