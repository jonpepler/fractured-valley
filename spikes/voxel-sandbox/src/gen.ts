// Deterministic world generation: world = generate(seed) + edits.
// Every function here is pure integer maths, so every peer builds the
// identical world from the seed alone, and an unedited region never needs
// to be stored or sent: it can be regenerated.

import { Grid } from './grid'
import { M } from './materials'
import { fbm2, hash32, valueNoise2 } from './rng'

export interface Column {
  /** Top solid cell (inclusive). */
  readonly top: number
  /** Cells at or below this are stone. */
  readonly stoneTop: number
  /** Material of the top cell. */
  readonly surface: number
  /** What sits on the surface, if anything (tall grass, flowers). */
  readonly decoration: number
}

export interface WorldSpec {
  readonly seed: number
  /** Size in 10 cm cells. */
  readonly sx: number
  readonly sy: number
  readonly sz: number
  readonly waterLevel: number
}

export const DEFAULT_SPEC: WorldSpec = { seed: 1337, sx: 192, sy: 192, sz: 112, waterLevel: 14 }

/** The ground profile of one column, from the seed alone. */
export function column(spec: WorldSpec, x: number, y: number): Column {
  const { seed, sx, sy } = spec
  // A slab that rises towards the back corner, like a valley side.
  const slope = Math.floor(((sx - x) * 14 + (sy - y) * 9) / 100)
  const rolling = (fbm2(seed, x, y, 64, 3) * 18) >> 16
  const fine = (valueNoise2(seed + 7, x, y, 6) * 3) >> 16
  let top = 12 + slope + rolling + fine

  // Rocky outcrops: blocky steps of stone pushing through the turf.
  const rock = fbm2(seed + 101, x, y, 36, 2)
  let isRock = false
  if (rock > 40000) {
    const lift = Math.floor((rock - 40000) / 900)
    top += Math.floor(lift / 6) * 6 // quantised into 60 cm courses
    isRock = lift >= 6
  }

  // A pool at the low front corner.
  const px = x - Math.floor(sx * 0.74)
  const py = y - Math.floor(sy * 0.86)
  const pr = px * px + py * py * 2
  if (pr < 900) top = Math.min(top, spec.waterLevel - 3 - Math.floor((900 - pr) / 300))

  const stoneTop = isRock ? top : top - 5 - ((valueNoise2(seed + 3, x, y, 10) * 4) >> 16)

  let surface: number = isRock ? M.Stone : M.Grass
  if (isRock && valueNoise2(seed + 55, x, y, 9) > 30000) surface = M.MossStone
  if (top < spec.waterLevel) surface = M.Dirt
  // Worn, bare patches of earth between the turf.
  if (surface === M.Grass && valueNoise2(seed + 61, x, y, 22) < 15000 && hash32(seed, x, y, 62) % 5 !== 0) surface = M.Dirt

  let decoration: number = M.Air
  if (surface === M.Grass) {
    const patch = valueNoise2(seed + 77, x, y, 14)
    const h = hash32(seed, x, y, 5)
    if (patch > 36000 && h % 100 < 45) decoration = M.TallGrass
    else if (h % 1000 < 6) decoration = M.Flower
  }
  return { top, stoneTop, surface, decoration }
}

export function generate(spec: WorldSpec = DEFAULT_SPEC): Grid {
  const g = new Grid(spec.sx, spec.sy, spec.sz)

  // Bricks entirely below every column's stone line start as uniform stone,
  // without allocating. Everything else is filled cell by cell.
  const cols: Column[] = []
  for (let y = 0; y < spec.sy; y++) for (let x = 0; x < spec.sx; x++) cols.push(column(spec, x, y))
  const col = (x: number, y: number) => cols[y * spec.sx + x]!

  for (let bz = 0; bz < g.bz; bz++) for (let by = 0; by < g.by; by++) for (let bx = 0; bx < g.bx; bx++) {
    let minStone = Infinity
    for (let y = by * 16; y < Math.min(spec.sy, by * 16 + 16); y++)
      for (let x = bx * 16; x < Math.min(spec.sx, bx * 16 + 16); x++) minStone = Math.min(minStone, col(x, y).stoneTop)
    if (bz * 16 + 15 <= minStone) g.fillBrick((bz * g.by + by) * g.bx + bx, M.Stone)
  }

  for (let y = 0; y < spec.sy; y++) for (let x = 0; x < spec.sx; x++) {
    const c = col(x, y)
    for (let z = 0; z <= c.top; z++) {
      const want = z === c.top ? c.surface : z <= c.stoneTop ? M.Stone : M.Dirt
      if (g.get(x, y, z) !== want) g.set(x, y, z, want)
    }
    for (let z = c.top + 1; z < spec.waterLevel; z++) g.set(x, y, z, M.Water)
    if (c.decoration !== M.Air && c.top + 1 >= spec.waterLevel) {
      g.set(x, y, c.top + 1, c.decoration)
      // Taller tufts in the densest patches.
      if (c.decoration === M.TallGrass && hash32(spec.seed, x, y, 9) % 3 === 0) g.set(x, y, c.top + 2, M.TallGrass)
    }
  }

  placeRuin(g, spec, col, Math.floor(spec.sx * 0.3), Math.floor(spec.sy * 0.22))
  placeTree(g, spec, col, Math.floor(spec.sx * 0.12), Math.floor(spec.sy * 0.62), 26)
  placeTree(g, spec, col, Math.floor(spec.sx * 0.55), Math.floor(spec.sy * 0.18), 30)
  placeTree(g, spec, col, Math.floor(spec.sx * 0.86), Math.floor(spec.sy * 0.42), 22)
  placePostAndBlock(g, col, Math.floor(spec.sx * 0.52), Math.floor(spec.sy * 0.56))
  g.compact()
  return g
}

type ColFn = (x: number, y: number) => Column

/** A broken tower of stone courses, mossy on top, moss creeping down. */
function placeRuin(g: Grid, spec: WorldSpec, col: ColFn, cx: number, cy: number) {
  const w = 22
  let base = Infinity
  for (let y = cy; y < cy + w; y++) for (let x = cx; x < cx + w; x++) base = Math.min(base, col(x, y).top)
  for (let y = cy; y < cy + w; y++) for (let x = cx; x < cx + w; x++) {
    // Broken top: each 6×6 block column stops at its own height.
    const blockH = 40 + (hash32(spec.seed, x >> 3, y >> 3, 21) % 5) * 7
    const height = base + blockH
    for (let z = base - 2; z <= height; z++) {
      const nearTop = height - z
      const mossy = nearTop < 2 || (nearTop < 14 && valueNoise2(spec.seed + 9, x * 3 + z, y * 3 + z, 7) > 42000)
      g.set(x, y, z, mossy ? M.MossStone : M.Stone)
    }
    if (hash32(spec.seed, x, y, 23) % 4 === 0) g.set(x, y, height + 1, M.Moss)
  }
  // A lower wall stub running out from the tower.
  for (let y = cy + 4; y < cy + 14; y++) for (let x = cx + w; x < cx + w + 34; x++) {
    const h = base + 14 - Math.floor((x - cx - w) / 5) + (hash32(spec.seed, x >> 2, y, 31) % 3)
    for (let z = col(x, y).top; z <= h; z++) g.set(x, y, z, h - z < 2 ? M.MossStone : M.Stone)
  }
}

function placeTree(g: Grid, spec: WorldSpec, col: ColFn, cx: number, cy: number, height: number) {
  const base = col(cx, cy).top
  for (let z = base + 1; z <= base + height; z++)
    for (let dy = 0; dy < 3; dy++) for (let dx = 0; dx < 3; dx++) g.set(cx + dx, cy + dy, z, M.Wood)
  const r = 9
  const lz = base + height
  for (let dz = -r; dz <= r; dz++) for (let dy = -r; dy <= r; dy++) for (let dx = -r; dx <= r; dx++) {
    const d = dx * dx + dy * dy + dz * dz * 2
    if (d > r * r) continue
    if (hash32(spec.seed, cx + dx, cy + dy, lz + dz) % 100 < 12) continue
    if (g.get(cx + 1 + dx, cy + 1 + dy, lz + dz) === M.Air) g.set(cx + 1 + dx, cy + 1 + dy, lz + dz, M.Leaves)
  }
}

/** The benchmark rig: a wooden post carrying a metal block. */
function placePostAndBlock(g: Grid, col: ColFn, cx: number, cy: number) {
  const base = col(cx, cy).top
  const postH = 16
  for (let z = base + 1; z <= base + postH; z++)
    for (let dy = 0; dy < 2; dy++) for (let dx = 0; dx < 2; dx++) g.set(cx + dx, cy + dy, z, M.Wood)
  for (let dz = 1; dz <= 6; dz++) for (let dy = -2; dy < 4; dy++) for (let dx = -2; dx < 4; dx++)
    g.set(cx + dx, cy + dy, base + postH + dz, M.Metal)
  // Dry grass around its foot, so a fire there takes hold.
  for (let dy = -5; dy < 7; dy++) for (let dx = -5; dx < 7; dx++) {
    const c = col(cx + dx, cy + dy)
    if (g.get(cx + dx, cy + dy, c.top + 1) === M.Air) g.set(cx + dx, cy + dy, c.top + 1, M.TallGrass)
  }
}
