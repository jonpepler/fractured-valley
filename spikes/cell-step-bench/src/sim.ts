// TypeScript reference implementation of the cell step rule.
// MUST stay bit-for-bit equivalent to rust/src/lib.rs (checked by test/determinism.test.ts).
//
// Brick = 16x16x16 cells, index c = x + 16*z + 256*y (y is up).
// All bricks of a world live in shared flat arrays: cell (b, c) is at b*4096 + c.
// Integer maths only. Fixed iteration order. Seeded integer hash for randomness.

export const AIR = 0;
export const STONE = 1;
export const GRASS = 2;
export const WOOD = 3;
export const ASH = 4;
export const SAND = 5;

export const IGNITE_TEMP = 80;
export const AIR_AMBIENT = 20;
export const MOVED = 0x80;
export const BURN_MASK = 0x7f;

/** 32-bit integer hash of (a, b). Same constants/ops as the Rust `hash`. */
export function hash(a: number, b: number): number {
  let x = Math.imul(b, 0x9e3779b1) ^ Math.imul(a, 0x85ebca77);
  x ^= x >>> 16;
  x = Math.imul(x, 0x7feb352d);
  x ^= x >>> 15;
  x = Math.imul(x, 0x846ca68b);
  x ^= x >>> 16;
  return x >>> 0;
}

/**
 * Step every brick in list[listStart .. listStart+count) for one tick.
 * tc = temperature buffer read this tick (all bricks), tn = buffer written.
 * Diffusion reads neighbour bricks from tc only; fire and gravity only touch
 * cells of the brick being stepped (in tn/mat/state), so bricks are
 * independent within a tick and the order in which bricks are processed
 * does not affect the result.
 */
export function stepBricks(
  mat: Uint16Array,
  tc: Uint8Array,
  tn: Uint8Array,
  state: Uint8Array,
  nb: Int32Array,
  list: Uint32Array,
  listStart: number,
  count: number,
  tick: number,
): void {
  for (let i = 0; i < count; i++) {
    const b = list[listStart + i]!;
    diffuse(mat, tc, tn, nb, b);
    fire(mat, tn, state, b, tick);
    gravity(mat, tn, state, b, tick);
  }
}

/** Profiling helper: like stepBricks but only the phases in mask (1 diffuse, 2 fire, 4 gravity). */
export function stepBricksPhases(
  mat: Uint16Array,
  tc: Uint8Array,
  tn: Uint8Array,
  state: Uint8Array,
  nb: Int32Array,
  list: Uint32Array,
  count: number,
  tick: number,
  mask: number,
): void {
  for (let i = 0; i < count; i++) {
    const b = list[i]!;
    if (mask & 1) diffuse(mat, tc, tn, nb, b);
    if (mask & 2) fire(mat, tn, state, b, tick);
    if (mask & 4) gravity(mat, tn, state, b, tick);
  }
}

function diffuse(mat: Uint16Array, tc: Uint8Array, tn: Uint8Array, nb: Int32Array, b: number): void {
  const base = b << 12;
  const n0 = nb[b * 6]!; // -x
  const n1 = nb[b * 6 + 1]!; // +x
  const n2 = nb[b * 6 + 2]!; // -y
  const n3 = nb[b * 6 + 3]!; // +y
  const n4 = nb[b * 6 + 4]!; // -z
  const n5 = nb[b * 6 + 5]!; // +z
  for (let y = 0; y < 16; y++) {
    for (let z = 0; z < 16; z++) {
      const yz = (y << 8) | (z << 4);
      const row = base + yz;
      const down = y > 0 ? row - 256 : n2 >= 0 ? (n2 << 12) + (15 << 8) + (z << 4) : row;
      const up = y < 15 ? row + 256 : n3 >= 0 ? (n3 << 12) + (z << 4) : row;
      const zm = z > 0 ? row - 16 : n4 >= 0 ? (n4 << 12) + (y << 8) + (15 << 4) : row;
      const zp = z < 15 ? row + 16 : n5 >= 0 ? (n5 << 12) + (y << 8) : row;
      const leftEdge = n0 >= 0 ? tc[(n0 << 12) + yz + 15]! : tc[row]!;
      const rightEdge = n1 >= 0 ? tc[(n1 << 12) + yz]! : tc[row + 15]!;

      // x = 0
      {
        const c = tc[row]!;
        const s = 2 * c + leftEdge + tc[row + 1]! + tc[down]! + tc[up]! + tc[zm]! + tc[zp]!;
        let v = (s + 4) >> 3;
        if (mat[row] === AIR && v > AIR_AMBIENT) v -= 1;
        tn[row] = v;
      }
      for (let x = 1; x < 15; x++) {
        const c = tc[row + x]!;
        const s =
          2 * c + tc[row + x - 1]! + tc[row + x + 1]! + tc[down + x]! + tc[up + x]! + tc[zm + x]! + tc[zp + x]!;
        let v = (s + 4) >> 3;
        if (mat[row + x] === AIR && v > AIR_AMBIENT) v -= 1;
        tn[row + x] = v;
      }
      // x = 15
      {
        const c = tc[row + 15]!;
        const s =
          2 * c + tc[row + 14]! + rightEdge + tc[down + 15]! + tc[up + 15]! + tc[zm + 15]! + tc[zp + 15]!;
        let v = (s + 4) >> 3;
        if (mat[row + 15] === AIR && v > AIR_AMBIENT) v -= 1;
        tn[row + 15] = v;
      }
    }
  }
}

function addHeat(t: Uint8Array, i: number, amount: number): void {
  const v = t[i]! + amount;
  t[i] = v > 255 ? 255 : v;
}

function fire(mat: Uint16Array, t: Uint8Array, state: Uint8Array, b: number, tick: number): void {
  const base = b << 12;
  const salt = (tick << 1) >>> 0;
  for (let c = 0; c < 4096; c++) {
    const i = base + c;
    const m = mat[i]!;
    if (m !== GRASS && m !== WOOD) continue;
    const st = state[i]!;
    let s = st & BURN_MASK;
    if (s === 0) {
      if (t[i]! >= IGNITE_TEMP && (hash(salt, i) & 3) !== 0) {
        state[i] = (st & MOVED) | 1;
      }
      continue;
    }
    // burning
    s += 1;
    addHeat(t, i, m === GRASS ? 24 : 32);
    if (c < 15 * 256) addHeat(t, i + 256, 16); // fire rises
    const h = hash(salt, i);
    // stochastic sideways spread to one horizontal neighbour inside the brick
    const x = c & 15;
    const z = (c >> 4) & 15;
    switch ((h >>> 8) & 3) {
      case 0:
        if (x > 0) addHeat(t, i - 1, 48);
        break;
      case 1:
        if (x < 15) addHeat(t, i + 1, 48);
        break;
      case 2:
        if (z > 0) addHeat(t, i - 16, 48);
        break;
      default:
        if (z < 15) addHeat(t, i + 16, 48);
        break;
    }
    const burnout = m === GRASS ? 12 : 40;
    if (s >= burnout) {
      mat[i] = ASH;
      state[i] = st & MOVED;
    } else {
      state[i] = (st & MOVED) | s;
    }
  }
}

function gravity(mat: Uint16Array, t: Uint8Array, state: Uint8Array, b: number, tick: number): void {
  const base = b << 12;
  const salt = ((tick << 1) | 1) >>> 0;
  for (let y = 15; y >= 0; y--) {
    for (let zx = 0; zx < 256; zx++) {
      const c = (y << 8) | zx;
      const i = base + c;
      const st = state[i]!;
      if (st & MOVED) {
        state[i] = st & BURN_MASK;
        continue;
      }
      if (y === 0) continue;
      const m = mat[i]!;
      if (m !== ASH && m !== SAND) continue;
      let dest = -1;
      const below = i - 256;
      if (mat[below] === AIR) {
        dest = below;
      } else {
        const h = hash(salt, i);
        const x = zx & 15;
        const z = zx >> 4;
        for (let k = 0; k < 2; k++) {
          const d = (h + 2 * k) & 3;
          let j = -1;
          if (d === 0) {
            if (x > 0) j = below - 1;
          } else if (d === 1) {
            if (x < 15) j = below + 1;
          } else if (d === 2) {
            if (z > 0) j = below - 16;
          } else {
            if (z < 15) j = below + 16;
          }
          if (j >= 0 && mat[j] === AIR) {
            dest = j;
            break;
          }
        }
      }
      if (dest >= 0) {
        mat[dest] = m;
        mat[i] = AIR;
        const tt = t[dest]!;
        t[dest] = t[i]!;
        t[i] = tt;
        state[dest] = (st & BURN_MASK) | MOVED;
        state[i] = 0;
      }
    }
  }
}
