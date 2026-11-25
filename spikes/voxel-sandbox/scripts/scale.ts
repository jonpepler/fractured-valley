// How much memory would a 10 cm world need at valley scale?
//
// Uses the real generator's column profiles, without building the grid:
// a brick needs cell arrays only where the ground surface passes through
// it (between the stone line and the top of the turf and plants). Bricks
// wholly in the air or wholly in rock are uniform and cost ~nothing.
// Samples brick columns and extrapolates.

import { column, type WorldSpec } from '../src/gen'

const BYTES_PER_CELL = 6 // material u16 + state, temp, damage, support u8 (decision 1.3)

function estimate(sizeM: number, heightM: number) {
  const cells = sizeM * 10
  const spec: WorldSpec = { seed: 1337, sx: cells, sy: cells, sz: heightM * 10, waterLevel: 14 }
  const brickCols = Math.ceil(cells / 16)
  const stride = Math.max(1, Math.floor(brickCols / 64))
  let sampled = 0, detailed = 0
  for (let by = 0; by < brickCols; by += stride) for (let bx = 0; bx < brickCols; bx += stride) {
    let lo = Infinity, hi = -Infinity
    for (let y = by * 16; y < by * 16 + 16; y += 3) for (let x = bx * 16; x < bx * 16 + 16; x += 3) {
      const c = column(spec, x, y)
      lo = Math.min(lo, c.stoneTop - 1)
      hi = Math.max(hi, c.top + 2)
    }
    detailed += Math.floor(hi / 16) - Math.floor(lo / 16) + 1
    sampled++
  }
  const perCol = detailed / sampled
  const totalDetailed = perCol * brickCols * brickCols
  const totalBricks = brickCols * brickCols * Math.ceil(spec.sz / 16)
  return {
    world: `${sizeM} m × ${sizeM} m × ${heightM} m`,
    cells: `${((cells * cells * spec.sz) / 1e9).toFixed(1)} B`,
    detailedBricks: Math.round(totalDetailed),
    detailedShare: `${((100 * totalDetailed) / totalBricks).toFixed(1)}%`,
    memory: `${((totalDetailed * 4096 * BYTES_PER_CELL) / 2 ** 30).toFixed(2)} GiB`,
    denseWouldBe: `${((cells * cells * spec.sz * BYTES_PER_CELL) / 2 ** 30).toFixed(0)} GiB`,
  }
}

console.table([
  estimate(64, 32),
  estimate(128, 48),
  estimate(256, 64),
  estimate(512, 96),
  estimate(1024, 128),
])
