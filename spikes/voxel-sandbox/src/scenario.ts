// A fixed scenario for checking that every browser and CPU computes the same
// world: fires, a burning post that topples, metal blocks, a tree that falls.
// Used by determinism.html (browsers) and scripts/determinism.ts (Node).
import { DEFAULT_SPEC, generate } from './gen'
import { M } from './materials'
import { Sim } from './sim'

export const SCENARIO_SPEC = { ...DEFAULT_SPEC, sx: 96, sy: 96 }
export const SCENARIO_TICKS = 600
export const SCENARIO_EVERY = 50

export interface ScenarioResult {
  /** sim.hash() every SCENARIO_EVERY ticks, as hex. */
  hashes: string[]
  /** Summary of what happened, so a mismatch can be read without a debugger. */
  events: string[]
  msPerTick: number
}

export function runScenario(onTick?: (sim: Sim) => void): { sim: Sim; result: ScenarioResult } {
  const spec = SCENARIO_SPEC
  const sim = new Sim(generate(spec), spec.seed)
  const ground = (x: number, y: number) => { let z = spec.sz - 1; while (z > 0 && sim.world.get(x, y, z) === M.Air) z--; return z }
  let seq = 0
  const at = (tick: number, kind: 'ignite' | 'drop', x: number, y: number) =>
    sim.schedule({ tick, peer: 0, seq: seq++, command: { kind, x, y, z: ground(x, y) } })
  const px = Math.floor(spec.sx * 0.52), py = Math.floor(spec.sy * 0.56)
  at(1, 'ignite', px - 4, py - 4)
  at(5, 'drop', 30, 100)
  at(60, 'ignite', 40, 40)
  at(120, 'drop', px + 8, py + 10)
  at(140, 'drop', px + 10, py + 12)
  at(300, 'ignite', 110, 30)
  // Throw a cube at the north tree's trunk: impacts, substeps, a falling tree.
  const tx = Math.floor(spec.sx * 0.55), ty = Math.floor(spec.sy * 0.18)
  sim.schedule({ tick: 200, peer: 0, seq: seq++, command: { kind: 'launch', x: tx, y: ty - 20, z: ground(tx, ty - 20) + 6, vx: 0, vy: 120, vz: 10 } })
  const hashes: string[] = []
  const start = performance.now()
  for (let t = 0; t < SCENARIO_TICKS; t++) {
    sim.step()
    onTick?.(sim)
    if (sim.tick % SCENARIO_EVERY === 0) hashes.push(sim.hash().toString(16).padStart(8, '0'))
  }
  const msPerTick = (performance.now() - start) / SCENARIO_TICKS
  const events = sim.events.filter(e => e.kind !== 'gave-way' && e.kind !== 'crumbled').map(e => `${e.tick} ${e.kind} ${e.cells}`)
  return { sim, result: { hashes, events, msPerTick } }
}
