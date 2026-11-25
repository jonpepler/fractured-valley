// Wind: a pure function of (seed, tick, position). Nothing is stored, so it
// costs no state, never desyncs, and any system (fire, rendering, later
// plants and smoke) can ask for it. Integer maths only.
//
// - The prevailing direction drifts slowly (one cycle is minutes).
// - Its strength swells and drops over tens of seconds.
// - Gusts are patches of stronger wind that travel downwind.
import { valueNoise2 } from './rng'

/** 16 compass directions as integer unit vectors ×256 (no trig at runtime). */
const DIRS: readonly [number, number][] = [
  [256, 0], [237, 98], [181, 181], [98, 237], [0, 256], [-98, 237], [-181, 181], [-237, 98],
  [-256, 0], [-237, -98], [-181, -181], [-98, -237], [0, -256], [98, -237], [181, -181], [237, -98],
]

export interface Wind {
  /** Direction and strength: a vector ×256, length up to about 256 (a strong breeze). */
  x: number
  y: number
}

/** The prevailing wind at a tick, before gusts. */
export function prevailing(seed: number, tick: number): Wind {
  // Direction: noise over time, mapped to 16 sectors with interpolation.
  const turn = valueNoise2(seed ^ 0x5717, tick, 0, 2400) // 0..65535, drifting over 2 min
  const a = (turn * 16) >> 16, f = ((turn * 16) & 0xffff) >> 8 // sector and 0..255 within it
  const [x0, y0] = DIRS[a & 15]!, [x1, y1] = DIRS[(a + 1) & 15]!
  const dx = x0 + (((x1 - x0) * f) >> 8), dy = y0 + (((y1 - y0) * f) >> 8)
  // Strength 0..256: calm spells and breezy spells over ~40 s.
  const s = valueNoise2(seed ^ 0x9a11, tick, 7, 800) >> 8
  return { x: (dx * s) >> 8, y: (dy * s) >> 8 }
}

/** Wind at a place: the prevailing wind plus a gust patch drifting with it. */
export function windAt(seed: number, tick: number, x: number, y: number, base = prevailing(seed, tick)): Wind {
  // Gust patches ~5 m across, carried downwind at about 1 m/s (20 ticks per m at 10 cm cells: 1 cell per 2 ticks).
  const ox = (base.x * tick) >> 9, oy = (base.y * tick) >> 9
  const g = valueNoise2(seed ^ 0x6057, x - ox + 4096, y - oy + 4096, 48) // 0..65535
  const gust = 128 + (g >> 8) // ×(0.5..1.5)
  return { x: (base.x * gust) >> 8, y: (base.y * gust) >> 8 }
}
