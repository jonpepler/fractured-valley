// World layout + deterministic terrain init + FNV-1a state hash.
// The same layout is used for a plain/Shared ArrayBuffer (TS) and for WASM linear
// memory (at a base offset), so init and hashing are shared code.

import { GRASS, SAND, STONE, WOOD, hash } from "./sim.ts";

export const BRICK_CELLS = 4096;
export const BRICK_H = 4; // bricks stacked vertically (64 cells = 6.4 m)

export interface Layout {
  n: number;
  bx: number;
  bz: number;
  matOff: number;
  tempAOff: number;
  tempBOff: number;
  stateOff: number;
  nbOff: number;
  /** (1 + maxWorkers) lists of n u32 each: list 0 = all bricks in order. */
  listOff: number;
  maxWorkers: number;
  end: number;
}

const align = (v: number, a: number) => Math.ceil(v / a) * a;

export function makeLayout(n: number, base: number, maxWorkers = 4): Layout {
  const cols = n / BRICK_H;
  const bx = Math.round(Math.sqrt(cols));
  const bz = cols / bx;
  if (!Number.isInteger(bz) || bx * bz * BRICK_H !== n) throw new Error(`bad brick count ${n}`);
  const cells = n * BRICK_CELLS;
  let o = align(base, 64);
  const matOff = o;
  o = align(o + cells * 2, 64);
  const tempAOff = o;
  o = align(o + cells, 64);
  const tempBOff = o;
  o = align(o + cells, 64);
  const stateOff = o;
  o = align(o + cells, 64);
  const nbOff = o;
  o = align(o + n * 6 * 4, 64);
  const listOff = o;
  o = align(o + (1 + maxWorkers) * n * 4, 64);
  return { n, bx, bz, matOff, tempAOff, tempBOff, stateOff, nbOff, listOff, maxWorkers, end: o };
}

export interface Views {
  mat: Uint16Array;
  tempA: Uint8Array;
  tempB: Uint8Array;
  state: Uint8Array;
  nb: Int32Array;
  lists: Uint32Array;
}

export function makeViews(buf: ArrayBufferLike, L: Layout): Views {
  const cells = L.n * BRICK_CELLS;
  return {
    mat: new Uint16Array(buf, L.matOff, cells),
    tempA: new Uint8Array(buf, L.tempAOff, cells),
    tempB: new Uint8Array(buf, L.tempBOff, cells),
    state: new Uint8Array(buf, L.stateOff, cells),
    nb: new Int32Array(buf, L.nbOff, L.n * 6),
    lists: new Uint32Array(buf, L.listOff, (1 + L.maxWorkers) * L.n),
  };
}

/** Brick index for brick coords. */
export const brickIndex = (L: Layout, x: number, y: number, z: number) => x + L.bx * (z + L.bz * y);

export function brickCoords(L: Layout, b: number): [number, number, number] {
  const x = b % L.bx;
  const z = Math.floor(b / L.bx) % L.bz;
  const y = Math.floor(b / (L.bx * L.bz));
  return [x, y, z];
}

/** Temperature buffers to read / write for a given tick (double buffer). */
export function tempBuffers(v: Views, tick: number): [Uint8Array, Uint8Array] {
  return tick % 2 === 0 ? [v.tempA, v.tempB] : [v.tempB, v.tempA];
}
export function tempOffsets(L: Layout, tick: number): [number, number] {
  return tick % 2 === 0 ? [L.tempAOff, L.tempBOff] : [L.tempBOff, L.tempAOff];
}
/** Current (most recently written) temperature buffer after `ticksDone` ticks. */
export const currentTemp = (v: Views, ticksDone: number) => (ticksDone % 2 === 0 ? v.tempA : v.tempB);

/** Deterministic terrain: stone below, sand + grass on top, trees, floating sand clumps, hot seeds. */
export function initWorld(L: Layout, v: Views): void {
  v.mat.fill(0);
  v.tempA.fill(20);
  v.tempB.fill(0);
  v.state.fill(0);
  for (let b = 0; b < L.n; b++) {
    const [bx, by, bz] = brickCoords(L, b);
    const dirs: [number, number, number][] = [
      [-1, 0, 0],
      [1, 0, 0],
      [0, -1, 0],
      [0, 1, 0],
      [0, 0, -1],
      [0, 0, 1],
    ];
    for (let d = 0; d < 6; d++) {
      const [dx, dy, dz] = dirs[d]!;
      const x = bx + dx;
      const y = by + dy;
      const z = bz + dz;
      const inside = x >= 0 && x < L.bx && y >= 0 && y < BRICK_H && z >= 0 && z < L.bz;
      v.nb[b * 6 + d] = inside ? brickIndex(L, x, y, z) : -1;
    }
    v.lists[b] = b;
    const base = b * BRICK_CELLS;
    for (let y = 0; y < 16; y++) {
      for (let z = 0; z < 16; z++) {
        for (let x = 0; x < 16; x++) {
          const X = bx * 16 + x;
          const Y = by * 16 + y;
          const Z = bz * 16 + z;
          const H = 24 + (hash(X >> 3, (Z >> 3) + 1000) % 12);
          const i = base + x + (z << 4) + (y << 8);
          let m = 0;
          if (Y < H - 5) m = STONE;
          else if (Y < H - 1) m = SAND;
          else if (Y === H - 1) m = GRASS;
          else if (Y < H + 6 && hash(X, Z + 7777) % 97 === 0) m = WOOD;
          else if (Y >= H + 8 && Y < H + 12 && hash(X >> 2, (Z >> 2) + 31) % 7 === 0) m = SAND;
          v.mat[i] = m;
          if (m === GRASS && hash(X, Z + 555) % 401 === 0) {
            v.tempA[i] = 255;
            v.state[i] = 1;
          }
        }
      }
    }
  }
}

/** FNV-1a 32 over material bytes (LE), current temperature, state. */
export function stateHash(v: Views, ticksDone: number): string {
  let h = 0x811c9dc5;
  const feed = (bytes: Uint8Array) => {
    for (let i = 0; i < bytes.length; i++) {
      h = Math.imul(h ^ bytes[i]!, 0x01000193);
    }
  };
  feed(new Uint8Array(v.mat.buffer, v.mat.byteOffset, v.mat.byteLength));
  feed(currentTemp(v, ticksDone));
  feed(v.state);
  return (h >>> 0).toString(16).padStart(8, "0");
}

/** Simple census for sanity (how much is happening). */
export function census(v: Views): Record<string, number> {
  const out: Record<string, number> = { air: 0, stone: 0, grass: 0, wood: 0, ash: 0, sand: 0, burning: 0 };
  const names = ["air", "stone", "grass", "wood", "ash", "sand"];
  for (let i = 0; i < v.mat.length; i++) {
    const k = names[v.mat[i]!]!;
    out[k]!++;
    if ((v.mat[i] === GRASS || v.mat[i] === WOOD) && (v.state[i]! & 0x7f) !== 0) out.burning!++;
  }
  return out;
}
