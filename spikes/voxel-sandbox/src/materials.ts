// Materials are bundles of composed properties, not a class hierarchy.
// Behaviour comes from which properties a material has: anything with
// `flammable` burns, anything with `loose` falls, whatever its name.

export type Rgb = readonly [number, number, number]

/**
 * How a substance burns. Whether a particular cell *can* burn right now
 * depends on its own moisture and on the heat around it (src/fields.ts):
 * the same wood is a slow, wet living trunk or a quick, dry post.
 */
export interface Flammable {
  /** Gas temperature (°C above ambient) that lights it once it is dry enough. */
  readonly ignitesAt: number
  /** Moisture (% of dry mass) above which it cannot burn: heat must dry it first. */
  readonly extinction: number
  /** Surface-to-volume, 1 (solid wood) to 8 (grass blades): how fast heat dries and lights it, and how readily it throws embers. */
  readonly fineness: number
  /** Ticks to burn through once alight. */
  readonly burnTicks: number
  /**
   * Heat given to the air per tick while burning (°C, into one 20 cm field
   * cell). Roughly the fuel's energy ÷ its burn time: a wisp of grass gives
   * little for a short while; a block of wood a steady glow for minutes.
   */
  readonly heat: number
  /** Soot (dark smoke) given off per tick while burning. */
  readonly soot: number
  /** What it becomes when burnt out. */
  readonly becomes: MaterialId
}

export interface Structural {
  /** Load (mass units) one cell can carry before failing. */
  readonly capacity: number
}

export interface Material {
  readonly id: MaterialId
  readonly name: string
  /** Blocks movement and holds things up. */
  readonly solid: boolean
  /** Hides what is behind it. */
  readonly opaque: boolean
  /** Falls when nothing solid is below. */
  readonly loose?: true
  readonly liquid?: true
  /** Part of the ground: anchors structures. */
  readonly terrain?: true
  /** Mass per cell, in integer units (water = 10). */
  readonly density: number
  /** Typical moisture when generated or grown (% of dry mass); cells vary from it. */
  readonly moisture?: number
  readonly flammable?: Flammable
  readonly structural?: Structural
  /** Colour ramp: per-cell variation picks from these. */
  readonly palette: readonly Rgb[]
}

export const M = {
  Air: 0,
  Stone: 1,
  MossStone: 2,
  Dirt: 3,
  Grass: 4,
  TallGrass: 5,
  Flower: 6,
  Wood: 7,
  Leaves: 8,
  Ash: 9,
  Water: 10,
  Metal: 11,
  Scorched: 12,
  Moss: 13,
} as const
export type MaterialId = (typeof M)[keyof typeof M]

const hex = (s: string): Rgb => [parseInt(s.slice(1, 3), 16), parseInt(s.slice(3, 5), 16), parseInt(s.slice(5, 7), 16)]

const list: Material[] = [
  { id: M.Air, name: 'air', solid: false, opaque: false, density: 0, palette: [] },
  {
    id: M.Stone, name: 'stone', solid: true, opaque: true, terrain: true, density: 26,
    structural: { capacity: 100000 },
    palette: ['#a89b7c', '#9a8d6f', '#8c7f63', '#b3a687', '#7d7158'].map(hex),
  },
  {
    id: M.MossStone, name: 'mossy stone', solid: true, opaque: true, terrain: true, density: 26,
    structural: { capacity: 100000 },
    palette: ['#7f9a3e', '#6e8c35', '#8fa848', '#9a8d6f', '#5e7d2f'].map(hex),
  },
  {
    id: M.Dirt, name: 'dirt', solid: true, opaque: true, terrain: true, density: 16,
    palette: ['#5a4636', '#66513f', '#745e4a', '#806b54'].map(hex),
  },
  {
    id: M.Grass, name: 'grass', solid: true, opaque: true, terrain: true, density: 16,
    moisture: 35,
    flammable: { ignitesAt: 260, extinction: 40, fineness: 6, burnTicks: 40, heat: 95, soot: 50, becomes: M.Scorched },
    palette: ['#3d6428', '#4b742e', '#5b8236', '#6c8a3c', '#7d8c46', '#8e9152'].map(hex),
  },
  {
    id: M.TallGrass, name: 'tall grass', solid: false, opaque: true, density: 1,
    moisture: 18,
    flammable: { ignitesAt: 230, extinction: 45, fineness: 8, burnTicks: 30, heat: 80, soot: 40, becomes: M.Air },
    palette: ['#56883a', '#6a9844', '#8c9a52', '#a3a462'].map(hex),
  },
  {
    id: M.Flower, name: 'flower', solid: false, opaque: true, density: 1,
    moisture: 70,
    flammable: { ignitesAt: 250, extinction: 45, fineness: 8, burnTicks: 20, heat: 60, soot: 15, becomes: M.Air },
    palette: ['#d9788a', '#e39aa5', '#c95f75'].map(hex),
  },
  {
    // Wood, living or dead: the difference is the cell's moisture (a living
    // trunk ~100%, a seasoned post ~12%), not a different material.
    id: M.Wood, name: 'wood', solid: true, opaque: true, density: 7,
    moisture: 100,
    flammable: { ignitesAt: 320, extinction: 30, fineness: 1, burnTicks: 900, heat: 180, soot: 120, becomes: M.Ash },
    structural: { capacity: 12000 },
    palette: ['#7a5232', '#6a4529', '#86603a'].map(hex),
  },
  {
    id: M.Leaves, name: 'leaves', solid: true, opaque: true, density: 1,
    moisture: 140,
    flammable: { ignitesAt: 260, extinction: 120, fineness: 7, burnTicks: 160, heat: 90, soot: 50, becomes: M.Air },
    palette: ['#3d7a2c', '#2f6a27', '#4b8a33', '#58993b'].map(hex),
  },
  {
    id: M.Ash, name: 'ash', solid: true, opaque: true, loose: true, density: 3,
    palette: ['#5d5a57', '#6e6a66', '#4d4a48'].map(hex),
  },
  {
    id: M.Water, name: 'water', solid: false, opaque: false, liquid: true, density: 10,
    palette: ['#4d8fa3', '#5a9db0', '#47859a'].map(hex),
  },
  {
    id: M.Metal, name: 'metal', solid: true, opaque: true, density: 78,
    structural: { capacity: 200000 },
    palette: ['#8a96a3', '#7b8794', '#9aa6b2', '#6c7884'].map(hex),
  },
  {
    id: M.Scorched, name: 'scorched earth', solid: true, opaque: true, terrain: true, density: 16,
    palette: ['#3e3129', '#4a3a2f', '#352a24'].map(hex),
  },
  {
    id: M.Moss, name: 'moss', solid: false, opaque: true, density: 1,
    moisture: 200,
    flammable: { ignitesAt: 260, extinction: 60, fineness: 6, burnTicks: 40, heat: 40, soot: 20, becomes: M.Air },
    palette: ['#6e9a35', '#82ab3f', '#5c8a2e'].map(hex),
  },
]

export const MATERIALS: readonly Material[] = list
export const mat = (id: number): Material => list[id]!
export const MATERIAL_COUNT = list.length
