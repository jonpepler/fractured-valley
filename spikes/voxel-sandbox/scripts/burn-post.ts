// Burn the benchmark post headlessly and print what happens: tsx scripts/burn-post.ts
import { DEFAULT_SPEC, generate } from '../src/gen'
import { M } from '../src/materials'
import { initPhysics } from '../src/physics'
import { Sim } from '../src/sim'
await initPhysics()
const SPEC = { ...DEFAULT_SPEC, sx: 144, sy: 144 }
const PX = Math.floor(SPEC.sx * 0.52), PY = Math.floor(SPEC.sy * 0.56)
const sim = new Sim(generate(SPEC), SPEC.seed)
let z = 0; while (sim.world.get(PX - 4, PY - 4, z + 1) !== M.Air) z++
sim.schedule({ tick: 1, peer: 0, seq: 0, command: { kind: 'ignite', x: PX - 4, y: PY - 4, z } })
for (let i = 0; i < 400; i++) {
  sim.step()
  if (sim.bodies.length && i % 5 === 0) { const p = sim.bodies[0]!.grid.pose!; console.log(sim.tick, sim.bodies.length, sim.physics.speeds(sim.bodies[0]!.grid).map(v=>v.toFixed(3)), sim.bodies[0]!.grid.sx, sim.bodies[0]!.grid.sy, sim.bodies[0]!.grid.sz, 'body', p.x.toFixed(1), p.y.toFixed(1), p.z.toFixed(1), 'q', p.qx.toFixed(2), p.qy.toFixed(2), p.qz.toFixed(2), p.qw.toFixed(2)) }
}
console.log(sim.events.filter(e => e.kind !== 'gave-way'))
let n = 0; const where: number[][] = []
for (let zz = 0; zz < SPEC.sz; zz++) for (let y = 0; y < SPEC.sy; y++) for (let x = 0; x < SPEC.sx; x++) if (sim.world.get(x, y, zz) === M.Metal) { n++; if (n < 3) where.push([x, y, zz]) }
console.log('metal cells', n, where, 'post at', PX, PY, 'bodies', sim.bodies.length)
