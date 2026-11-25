// Deterministic integer hashing. Everything random in the simulation is a
// pure function of (seed, tick, position, salt), so peers agree without
// sharing a stateful RNG. Only 32-bit integer ops: identical on every engine.

export function hash32(a: number, b = 0, c = 0, d = 0): number {
  let h = (a ^ 0x9e3779b9) >>> 0
  h = Math.imul(h ^ (b >>> 0), 0x85ebca6b) >>> 0
  h = (h ^ (h >>> 13)) >>> 0
  h = Math.imul(h ^ (c >>> 0), 0xc2b2ae35) >>> 0
  h = (h ^ (h >>> 16)) >>> 0
  h = Math.imul(h ^ (d >>> 0), 0x27d4eb2f) >>> 0
  h = (h ^ (h >>> 15)) >>> 0
  return h
}

/** True with probability num/den, decided by the hash. */
export function chance(h: number, num: number, den: number): boolean {
  return h % den < num
}

/**
 * Smooth 2D value noise in fixed point, for world generation. Returns 0..65535.
 * Uses only integer maths so every peer generates the identical world.
 */
export function valueNoise2(seed: number, x: number, y: number, cell: number): number {
  const gx = Math.floor(x / cell)
  const gy = Math.floor(y / cell)
  const fx = Math.floor(((x - gx * cell) * 256) / cell) // 0..255
  const fy = Math.floor(((y - gy * cell) * 256) / cell)
  const sx = smooth(fx)
  const sy = smooth(fy)
  const v00 = hash32(seed, gx, gy) & 0xffff
  const v10 = hash32(seed, gx + 1, gy) & 0xffff
  const v01 = hash32(seed, gx, gy + 1) & 0xffff
  const v11 = hash32(seed, gx + 1, gy + 1) & 0xffff
  const a = v00 + (((v10 - v00) * sx) >> 8)
  const b = v01 + (((v11 - v01) * sx) >> 8)
  return a + (((b - a) * sy) >> 8)
}

function smooth(t: number): number {
  // 3t^2 - 2t^3 in 8-bit fixed point.
  return (t * t * (768 - 2 * t)) >> 16
}

/** Fractal value noise, 0..65535. */
export function fbm2(seed: number, x: number, y: number, cell: number, octaves: number): number {
  let sum = 0
  let weight = 0
  let w = 256
  let c = cell
  for (let o = 0; o < octaves; o++) {
    sum += valueNoise2(seed + o * 1013, x, y, Math.max(1, c)) * w
    weight += w
    w >>= 1
    c >>= 1
  }
  return Math.floor(sum / weight)
}
