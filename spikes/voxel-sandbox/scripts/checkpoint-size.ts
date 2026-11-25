// How big would a join checkpoint be? World = seed + edits, so a checkpoint
// only needs the bricks that differ from freshly generated ones (palette-
// packed), plus the physics state and the small simulation sets.
import { DEFAULT_SPEC, generate } from '../src/gen'
import { BRICK_CELLS } from '../src/grid'
import { M } from '../src/materials'
import { initPhysics } from '../src/physics'
import { Sim } from '../src/sim'
await initPhysics()

const fresh = generate(DEFAULT_SPEC)
const sim = new Sim(generate(DEFAULT_SPEC), DEFAULT_SPEC.seed)
for (let i = 0; i < 6; i++) {
  const x = 30 + i * 25, y = 60 + (i % 3) * 30
  let z = 0; while (sim.world.get(x, y, z + 1) !== M.Air) z++
  sim.schedule({ tick: 1 + i * 100, peer: 0, seq: 0, command: { kind: 'ignite', x, y, z } })
  sim.schedule({ tick: 2 + i * 100, peer: 0, seq: 1, command: { kind: 'drop', x: x + 5, y: y + 5, z } })
}
for (const at of [600, 3000]) {
  while (sim.tick < at) sim.step()
  const w = sim.world
  let changed = 0, bytes = 0
  for (let b = 0; b < w.bricks.length; b++) {
    const a = w.bricks[b], f = fresh.bricks[b]
    const mat = (br: typeof a, u: number, i: number) => (br ? br.material[i]! : u)
    const st = (br: typeof a, i: number) => (br ? br.state[i]! : 0)
    let differs = false
    for (let i = 0; i < BRICK_CELLS && !differs; i++)
      if (mat(a, w.uniform[b]!, i) !== mat(f, fresh.uniform[b]!, i) || st(a, i) !== st(f, i)) differs = true
    if (!differs) continue
    changed++
    const used = new Set<number>(); let stated = 0
    for (let i = 0; i < BRICK_CELLS; i++) { used.add(mat(a, w.uniform[b]!, i)); if (st(a, i)) stated++ }
    const bits = used.size <= 2 ? 1 : used.size <= 4 ? 2 : used.size <= 16 ? 4 : 8
    bytes += (BRICK_CELLS * bits) / 8 + used.size * 2 + stated * 4 + 8
  }
  const physicsBytes = sim.physics.world.takeSnapshot().length
  console.log({ tick: at, changedBricks: changed, of: w.bricks.length, worldKiB: +(bytes / 1024).toFixed(1), physicsKiB: +(physicsBytes / 1024).toFixed(1), bodies: sim.bodies.length })
}
