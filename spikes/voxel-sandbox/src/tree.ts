// Grown trees: a deterministic space-colonisation model, integer-only.
//
// 1. Scatter seeded attraction points through a lumpy crown volume.
// 2. From the trunk, every branch tip that has points nearby grows one step
//    towards their average direction; points close to a branch are used up.
// 3. Branch thickness follows the pipe model: a branch is as thick as what
//    it carries (radius² ∝ number of twig tips above it).
// 4. Leaves are clumps on the twigs, with gaps, so fire has to travel
//    along branches and jump between clumps rather than fill a sphere.
//
// Positions are fixed-point (1/16 cell). No floats, no trig, sorted loops.
import type { Grid } from './grid'
import { M } from './materials'
import { hash32, valueNoise2 } from './rng'

const FP = 16

export interface TreeShape {
  /** Trunk height before the crown starts, in cells. */
  trunk: number
  /** Crown radius and height, in cells. */
  radius: number
  crownHeight: number
  /** Number of attraction points: more gives denser branching. */
  points: number
}

interface Node { x: number; y: number; z: number; parent: number; tips: number }

export function isqrt(n: number): number {
  if (n <= 0) return 0
  let x = Math.floor(Math.sqrt(n)) // exact for n < 2^52, then corrected
  while (x * x > n) x--
  while ((x + 1) * (x + 1) <= n) x++
  return x
}

/** Grow a tree whose trunk base is at (bx, by, bz). Returns cells written. */
export function growTree(g: Grid, seed: number, bx: number, by: number, bz: number, shape: TreeShape): number {
  const treeSeed = hash32(seed, bx, by)
  const h = (a: number, b = 0) => hash32(treeSeed, a, b)
  // ── attraction points in a lumpy ellipsoid crown ──
  const cx = bx * FP, cy = by * FP
  const cz0 = (bz + shape.trunk) * FP
  const R = shape.radius * FP, H = shape.crownHeight * FP
  const pts: [number, number, number][] = []
  for (let i = 0; pts.length < shape.points && i < shape.points * 20; i++) {
    const px = (h(i, 1) % (2 * R + 1)) - R
    const py = (h(i, 2) % (2 * R + 1)) - R
    const pz = h(i, 3) % (H + 1)
    // Inside an ellipsoid whose middle sits 40% up the crown, roughened by noise.
    const dz = pz - Math.floor((H * 2) / 5)
    const e = Math.floor((px * px + py * py) * 1000 / (R * R)) + Math.floor(dz * dz * 1000 / (Math.floor(H * 3 / 5) ** 2))
    const lump = valueNoise2(seed + bx, (px >> 4) + 64, (py >> 4) + (pz >> 4) * 7, 6) % 400
    if (e > 800 + lump) continue
    pts.push([cx + px, cy + py, cz0 + pz])
  }
  // ── trunk: straight up to the crown, with a seeded lean ──
  const step = 2 * FP
  const nodes: Node[] = [{ x: cx, y: cy, z: bz * FP, parent: -1, tips: 0 }]
  const leanX = (h(90) % 9) - 4, leanY = (h(91) % 9) - 4
  while (nodes[nodes.length - 1]!.z < cz0) {
    const p = nodes[nodes.length - 1]!
    nodes.push({ x: p.x + leanX, y: p.y + leanY, z: p.z + step, parent: nodes.length - 1, tips: 0 })
  }
  // ── colonise ──
  const influence = shape.radius * FP * 2, kill = 3 * FP
  const alive = pts.map(() => true)
  for (let iter = 0; iter < 120; iter++) {
    const pull = new Map<number, [number, number, number]>()
    let any = false
    for (let i = 0; i < pts.length; i++) {
      if (!alive[i]) continue
      const [px, py, pz] = pts[i]!
      let best = -1, bestD = influence * influence
      for (let n = 0; n < nodes.length; n++) {
        const q = nodes[n]!
        const d = (px - q.x) ** 2 + (py - q.y) ** 2 + (pz - q.z) ** 2
        if (d < bestD) { bestD = d; best = n }
      }
      if (best < 0) continue
      if (bestD < kill * kill) { alive[i] = false; continue }
      any = true
      const q = nodes[best]!
      const len = isqrt(bestD)
      const v = pull.get(best) ?? [0, 0, 0]
      v[0] += Math.trunc(((px - q.x) * 64) / len); v[1] += Math.trunc(((py - q.y) * 64) / len); v[2] += Math.trunc(((pz - q.z) * 64) / len)
      pull.set(best, v)
    }
    if (!any) break
    for (const [n, v] of [...pull].sort((a, b) => a[0] - b[0])) {
      const q = nodes[n]!
      v[2] += 6 // branches reach up a little (phototropism)
      const len = isqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) || 1
      const nx = q.x + Math.trunc((v[0] * step) / len), ny = q.y + Math.trunc((v[1] * step) / len), nz = q.z + Math.trunc((v[2] * step) / len)
      // Don't grow a duplicate on top of an existing node.
      if (nodes.some(o => Math.abs(o.x - nx) + Math.abs(o.y - ny) + Math.abs(o.z - nz) < FP)) continue
      nodes.push({ x: nx, y: ny, z: nz, parent: n, tips: 0 })
    }
  }
  // ── pipe model: count twig tips carried by each node ──
  const children = new Array<number>(nodes.length).fill(0)
  for (const n of nodes) if (n.parent >= 0) children[n.parent]!++
  for (let i = nodes.length - 1; i >= 0; i--) {
    const n = nodes[i]!
    if (children[i] === 0) n.tips = 1
    if (n.parent >= 0) nodes[n.parent]!.tips += n.tips
  }
  const maxTips = nodes[0]!.tips
  const trunkR = Math.max(FP, Math.floor((shape.radius * FP) / 6)) // radius at the base, fixed-point
  let written = 0
  const put = (x: number, y: number, z: number, m: number) => {
    const cur = g.get(x, y, z)
    if (cur === M.Air || cur === M.TallGrass || cur === M.Flower || (m === M.Wood && cur === M.Leaves)) { g.set(x, y, z, m); written++ }
  }
  // ── wood: each segment as a run of discs, radius from the pipe model ──
  for (let i = 1; i < nodes.length; i++) {
    const n = nodes[i]!, p = nodes[n.parent]!
    // r ∝ sqrt(tips): radius² scales with what it carries.
    const r = Math.max(FP / 2, Math.floor((trunkR * isqrt(n.tips * 256)) / isqrt(maxTips * 256)))
    const reach = Math.ceil(r / FP)
    for (let s = 0; s <= 4; s++) {
      const x = p.x + Math.trunc(((n.x - p.x) * s) / 4), y = p.y + Math.trunc(((n.y - p.y) * s) / 4), z = p.z + Math.trunc(((n.z - p.z) * s) / 4)
      for (let dz = -reach; dz <= reach; dz++) for (let dy = -reach; dy <= reach; dy++) for (let dx = -reach; dx <= reach; dx++) {
        const ox = (x >> 4) + dx, oy = (y >> 4) + dy, oz = (z >> 4) + dz
        const ddx = ox * FP + 8 - x, ddy = oy * FP + 8 - y, ddz = oz * FP + 8 - z
        if (ddx * ddx + ddy * ddy + ddz * ddz <= r * r) put(ox, oy, oz, M.Wood)
      }
    }
  }
  // A few roots flaring at the base, lying on the ground.
  const rr = Math.ceil(trunkR / FP)
  for (let k = 0; k < 4; k++) {
    const [dx, dy] = ([[1, 0], [-1, 0], [0, 1], [0, -1]] as const)[k]!
    const len = 2 + (h(70, k) % 3)
    for (let s = 0; s < len; s++) put(bx + dx * (rr + s), by + dy * (rr + s), bz, M.Wood)
  }
  // ── leaves: clumps on thin branches, with holes ──
  for (let i = 1; i < nodes.length; i++) {
    const n = nodes[i]!
    if (n.tips > 2) continue
    const r = n.tips === 1 ? 2 : 1
    const lx = n.x >> 4, ly = n.y >> 4, lz = n.z >> 4
    for (let dz = -r + 1; dz <= r; dz++) for (let dy = -r; dy <= r; dy++) for (let dx = -r; dx <= r; dx++) {
      if (dx * dx + dy * dy + (dz - 1) * (dz - 1) * 2 > r * r + 1) continue
      if (hash32(seed ^ 77, lx + dx, ly + dy, lz + dz) % 100 < 30) continue
      put(lx + dx, ly + dy, lz + dz, M.Leaves)
    }
  }
  return written
}
